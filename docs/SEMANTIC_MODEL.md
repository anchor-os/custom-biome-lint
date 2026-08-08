# Semantic model

A lightweight, single-file, syntax-only model of lexical scopes, declarations,
and identifier resolution, in `src/semantic/`. It answers exactly one
question:

> What does this identifier refer to, in this file, right here?

Built once per file via [`FileContext::semantic`](../src/analyzer/runner.rs),
lazily and cached, so a rule that never calls it never pays for it, and a rule
that does call it shares the same model every other rule that asked for it
already got.

## Status: backs all three existing rules

All three rules resolve the identifiers they care about against this model
rather than matching by name alone — `no-native-map` for `Map`,
`no-arrow-function-create-selector` and `reselect-arity-match` for
`createSelector` (the latter only for a bare-identifier callee; see "Migrating
the existing rules" below for why the member-expression form,
`x.createSelector(...)`, was deliberately left alone). This was not the
original plan — see that section for why the initial call was to leave the
rules untouched, and what changed.

## What it can answer

```rust
use custom_biome_lint::FileContext;

let file = FileContext::parse(source, path);
let model = file.semantic(); // built and cached on first call

// Given a JsReferenceIdentifier node (a *use* of a name, not a declaration):
if let Some(binding) = model.resolve(&identifier) {
    match binding.kind {
        BindingKind::Import(_) => { /* came from an import */ }
        BindingKind::Parameter => { /* a function/arrow parameter */ }
        _ => {}
    }
    if let Some(import) = binding.import() {
        // import.source  -- module specifier, e.g. "reselect"
        // import.imported -- Named("createSelector") | Default | Namespace
        // import.local    -- the name in scope here (differs from `imported`
        //                     only when there's an `as` alias)
    }
}
```

`resolve` walks from the identifier's own scope out to the file's global
scope and returns the *nearest* matching binding — ordinary lexical shadowing.
A `None` result means the name is unbound in this file: a global/host
built-in (`console`, `Math`, `window`, ...) or a genuine typo, neither of
which this model tries to tell apart.

## Scopes and bindings

Five scope kinds, forming a parent/child tree rooted at one `Global` scope
per file (see [`ScopeKind`](../src/semantic/scope.rs)):

| Kind | Created by |
| --- | --- |
| `Global` | The whole file (exactly one, the tree root) |
| `Function` | A function declaration, function expression, or arrow function — holds its parameters *and* its body's own declarations, not split into two layers |
| `Block` | A bare `{ ... }`, an if/loop/try body block that isn't itself a function body, or a `switch` statement's cases, which all share one such scope (there's no block around each case body) |
| `Loop` | A `for`/`for-in`/`for-of` head, holding the loop's own declared variable |
| `Catch` | A `catch (name) { ... }` clause, holding the caught binding |

Eight binding kinds (see [`BindingKind`](../src/semantic/binding.rs)): `Var`,
`Let`, `Const`, `Function`, `Class`, `Parameter`, `CatchParameter`, and
`Import(ImportBinding)`. Each [`Binding`](../src/semantic/binding.rs) records
its name, kind, owning scope, and the byte offset it was declared at (feed
that to `FileContext::line_col` for a line/column).

`var` hoists to the nearest enclosing `Function` or `Global` scope even when
it's textually written inside a nested block, exactly like real JavaScript;
`let`/`const`/`function`/`class`/parameters/catch bindings do not hoist past
their own scope.

## How it's built: one recursive walk, dispatched by syntax kind

`src/semantic/builder.rs` walks the tree exactly once, matching on
`JsSyntaxKind` for the handful of constructs that create a scope or a
binding (functions, arrows, blocks, loops, catch clauses, variable
declarations, imports, class declarations) and otherwise just recursing into
every child node unchanged. That fallback is what lets the walk find an arrow
function nested three levels deep inside a call argument or a JSX expression
container without a special case for every possible container node — nothing
about *how* a scope-creating construct is reached matters, only that it's
reached.

Resolution happens in a second, much cheaper pass, not during the walk:
every reference identifier is recorded as `(byte offset, current scope, name)`
while walking, and only resolved against the *complete* scope tree afterward.
Resolving eagerly during the walk would get forward references wrong — a
function that calls another function declared later in the same scope, or a
`var` referenced before its (hoisted) declaration later in the same block —
since the later binding wouldn't exist yet at the point the reference is
visited. This intentionally does not model the finer-grained distinction
between that (hoisting) and real temporal-dead-zone semantics for
`let`/`const`, which would require modeling execution order — out of scope
for a lexical resolver; see "What this is not," below.

## What this is not

Per the original design brief, none of the following are implemented, and
none should be added without first checking whether they'd turn this into a
different, larger tool than the one described here:

- TypeScript type inference or checking
- Control-flow or data-flow analysis (no notion of "is this reachable",
  "is this always assigned before use", temporal-dead-zone timing, etc.)
- Cross-file symbol resolution, module graph or npm/filesystem resolution
- Autocomplete, compiler diagnostics, or anything resembling a language server

If a change to this model starts requiring any of the above, that's a signal
to stop and simplify back to lexical scope/binding tracking, not to keep
building.

## Migrating the existing rules

When this semantic model first landed, none of the three existing rules were
wired up to it. The reasoning at the time: all three are deliberately,
exactly ESLint-parity ports (see [RULES.md](RULES.md) and the README's
"ESLint parity" section), matching the *original* rule's textual/structural
reach — including its known gaps — is what made "turning this tool on
instead of the old ESLint rule changes nothing" a verifiable claim. Fixing
`no-native-map`'s shadowing false-negative, or making `createSelector`
import-aware, would each have been a real behavior change dressed up as
"just using the infrastructure that's already there."

A follow-up migration explicitly asked for exactly that change anyway,
having weighed the tradeoff deliberately rather than as an incidental side
effect of "the model exists, so use it everywhere." What changed as a
result:

- **`no-native-map`** replaced its bespoke, non-lexical `ImmutableBindings`
  state machine (a single file-wide "is Immutable's Map bound anywhere in
  this file" boolean, built from ad hoc AST scanning) with per-reference
  semantic resolution. `import { Map } from "immutable"; function test(Map)
  { return new Map(); }` now correctly reports the inner `new Map()` — it
  resolves to the parameter, not the import, so it's genuinely native. This
  used to be a false negative: the file-wide flag suppressed every `Map` in
  the file the instant *any* Immutable-derived binding existed anywhere in
  it, parameter shadowing or not. A `Map`-named binding's own *declaration*
  (the parameter itself) is still never flagged — declaring a local by that
  name isn't a use of a value, so there's nothing for semantic resolution to
  adjudicate there.
- **`no-arrow-function-create-selector`** and **`reselect-arity-match`**
  resolve a bare `createSelector(...)` callee against
  `import { createSelector } from "reselect"` (aliased or not) instead of
  matching the identifier's spelling. A same-named local function, or a
  `createSelector` imported from an unrelated module, correctly no longer
  matches; an aliased import (`import { createSelector as selector } from
  "reselect"`) correctly still does, even though the identifier at the call
  site is spelled `selector`.
- **`reselect-arity-match`**'s member-expression callee form
  (`x.createSelector(...)`) was deliberately left untouched, matched by
  member name alone exactly as before. Resolving `x` semantically would only
  cover the narrow case of a namespace/default import used via member
  access, and the fixtures exercise this form with `x` never actually
  imported at all (`Reselect.createSelector(...)` with no `Reselect` import
  anywhere in the file) — precisely the "don't invent module-resolution
  logic" boundary this model exists to respect. See
  `src/rules/reselect_arity_match.rs`'s `is_create_selector_callee` for the
  exact split.

A small shared helper, `src/rules/reselect.rs`, holds the
"does this reference resolve to reselect's `createSelector`" check once,
used by both rules that need it, rather than each re-implementing the same
`ImportedName::Named("createSelector") && source == "reselect"` match.

This remains the model's only integration surface: it does not gain a
capability, a new binding kind, or an extra pass to support this — the
existing `resolve` + `Binding::import()` API was already sufficient. A
future rule with no legacy ESLint behavior to stay faithful to is still the
easiest kind of rule to build directly on this model, with no parity
tradeoff to weigh at all.

## Tests

`tests/integration.rs`'s `semantic_model` module covers: basic declarations
(`const`/`let`/`var`/`function`/`class`), function parameters, nested-scope
shadowing (module → function → block, each level restoring correctly on the
way back out), all four import forms (default, named, aliased named,
namespace) and their `source`/`imported`/`local` fields, a parameter and a
local redeclaration each correctly shadowing an import of the same name,
object and array destructuring (including a computed key, `{ [key]: value }`,
as a reference in its own right), arrow function parameters, block scope,
catch scope, a `switch` statement's cases sharing one block scope, `var`
hoisting out of a nested block, a `let` scoped to a `for` loop head, and the
scope parent-chain hierarchy.
