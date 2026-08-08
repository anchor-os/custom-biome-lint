# Semantic model

A lightweight, single-file, syntax-only model of lexical scopes, declarations,
and identifier resolution, in `src/semantic/`. It answers exactly one
question:

> What does this identifier refer to, in this file, right here?

Built once per file via [`FileContext::semantic`](../src/analyzer/runner.rs),
lazily and cached, so a rule that never calls it never pays for it, and a rule
that does call it shares the same model every other rule that asked for it
already got.

## Status: infrastructure, not yet used by a rule

This is deliberately new, currently-unused infrastructure — see "Why no
existing rule uses this yet" below. It exists so a *future* rule that
genuinely needs identifier resolution (e.g. "does this call actually refer to
the `createSelector` reselect exports, or a same-named local function?") has
something to build on, without reinventing scope tracking from scratch the way
`no-native-map` currently does with its own bespoke, non-lexical
`ImmutableBindings` state machine.

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

## Why no existing rule uses this yet

All three existing rules are deliberately, exactly ESLint-parity ports (see
[RULES.md](RULES.md) and the README's "ESLint parity" section) — matching the
*original* rule's textual/structural reach, including its known gaps, is the
entire point, because it's what makes "turning this tool on instead of the
old ESLint rule changes nothing" a verifiable claim rather than an assumption:

- `no-native-map` flags a parameter or local variable named `Map` shadowing
  an Immutable import "both at its declaration and at its use... exactly like
  the original ESLint rule" (see `fixtures/no_native_map/edge-cases.js`).
  That is precisely the shadowing case this semantic model resolves
  correctly — which means using it here would *fix* a behavior the project
  documents as intentional parity, not a bug. Doing so would be a real
  regression against this codebase's actual goal, dressed up as an
  improvement.
- `no-arrow-function-create-selector` and `reselect-arity-match` both match
  `createSelector` (bare or as `x.createSelector`) by name alone, never by
  where it came from. RULES.md documents this explicitly: "matching the
  original rule's reach exactly is what the parity guarantee requires."
  Making either import-aware would change what they flag on any file that
  happens to have an unrelated same-named local `createSelector` — again, a
  deviation from parity, not a fix.

None of the three rules were rewritten as a result. This is the intended
outcome per the design brief's own guidance: integrate a rule only where
semantic resolution improves *this codebase's* definition of correctness, and
leave the rest alone otherwise. A future rule that isn't an ESLint port —
one where there's no legacy behavior to stay faithful to — is a much better
candidate to build directly on this model.

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
