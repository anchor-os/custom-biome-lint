# Semantic model

A lightweight, single-file, syntax-only model of lexical scopes, declarations,
and identifier resolution, in `src/semantic/`. It answers exactly one
question:

> What does this identifier refer to, in this file, right here?

Built once per file via [`FileContext::semantic`](../src/analyzer/runner.rs),
lazily and cached, so a rule that never calls it never pays for it, and a rule
that does call it shares the same model every other rule that asked for it
already got.

## Status: backs every rule

Every rule resolves the identifiers it cares about against this model rather
than matching by name alone — `no-native-map` for `Map`,
`no-arrow-function-create-selector` and `reselect-arity-match` for
`createSelector` (the latter only for a bare-identifier callee; see "Migrating
the existing rules" below for why the member-expression form,
`x.createSelector(...)`, was deliberately left alone), and the four
parameter-mutation rules for the identifier an assignment target is rooted in.
For the original three this was not the original plan — see that section for why
the initial call was to leave them untouched, and what changed.

For the parameter-mutation rules, resolution is not an optimization but a
correctness requirement: without it, a local variable that happens to share a
name with a destructured parameter in an enclosing scope would be misclassified
by name alone, and a mutation several arrow functions away from its parameter's
declaration would not be attributable at all.

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

### Assignment targets resolve through a second method

Biome models the identifier being *written to* as `JsIdentifierAssignment`, a
different node type from the `JsReferenceIdentifier` used in read positions —
`x` in `x = 1` and `x` in `f(x)` are not the same kind of node. Both are uses of
an existing binding and both resolve identically, so there is a parallel entry
point rather than a second mechanism:

```rust
// The `x` in `x = 1`, `x++`, `for (x of list)`, or `[x] = pair`.
if let Some(binding) = model.resolve_assignment(&identifier_assignment) {
    // same Binding, same shadowing rules as `resolve`
}
```

Both delegate to one offset-keyed lookup, and the builder records assignment
targets into the same `pending_refs` list as read references, so the two-pass
hoisting behaviour described below applies to them too. Without this, an
assignment target could not be resolved at all — which is what
`destructure-default-param-assign` needs in order to tell a destructured
parameter from a same-named local.

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
binding (functions, arrows, class/object methods, getters, setters, blocks,
loops, catch clauses, variable declarations, imports, class declarations) and
otherwise just recursing into every child node unchanged. That fallback is what lets the walk find an arrow
function nested three levels deep inside a call argument or a JSX expression
container without a special case for every possible container node — nothing
about *how* a scope-creating construct is reached matters, only that it's
reached.

A method, getter or setter gets a `Function` scope holding its parameters, just
like a function expression — a getter has none, and a setter's single parameter
is not wrapped in a `JsParameters` list, but both still own a scope so
declarations in their bodies don't leak outward. Their *names* are deliberately
not bound anywhere: a method name is a property of the class or object, not a
binding any identifier can resolve to.

Resolution happens in a second, much cheaper pass, not during the walk:
every reference identifier and assignment target is recorded as `(byte offset, current scope, name)`
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

### Known gap: a class expression's own name

`const C = class Inner { m() { return Inner; } }` — the `Inner` reference does
not resolve. In real JS a named class expression binds its own name inside the
class body, the same way a named *function* expression does (which this model
handles). Classes are treated lightly on purpose: there is no class scope at all
(see `handle_class_declaration`), and adding one just for this would be the
first step toward class-member analysis this model deliberately avoids.

It is a gap rather than a defect in any rule: an unresolved identifier is not a
parameter binding, so the parameter-mutation rules stay quiet rather than
misfiring on it. Pinned by
`semantic_model::a_class_expressions_own_name_is_a_known_gap` so that closing it
later is a deliberate decision, not an accident.

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
