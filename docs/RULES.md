# Rules

Eight rules in two groups.

**The original three** are behaviour-for-behaviour ports of custom ESLint rules
from `eslint-rules/`. The ports are deliberately faithful rather than
"improved" — including reproducing one known false-positive class,
`no-native-map`'s `mapboxgl.Map` — so that turning this tool on produces
exactly the findings the old ESLint setup produced, no more and no less. That
property is what made the initial migration verifiable.

One deliberate exception since then: all three resolve
`createSelector`/`Map` identifiers against
[the semantic model](SEMANTIC_MODEL.md) instead of matching by name alone,
which changes output in the one place semantic resolution and ESLint parity
actually conflicted — see each rule's own section, and
[SEMANTIC_MODEL.md](SEMANTIC_MODEL.md#why-no-existing-rule-uses-this-yet) for
the reasoning. A file that never shadows these names or imports them from an
unrelated module — the overwhelming common case — sees no change at all.

**The five parameter-mutation rules** are not ports. They close specific,
measured gaps between ESLint's `no-param-reassign` (removed) and Biome's
`lint/style/noParameterAssign` (enabled here with
`propertyAssignment: "deny"`), which turns out to reach only *plain identifier
parameters*, at *one* level of property depth, in a *parenthesized* parameter
list. Each of the four gaps below was confirmed by direct repro against Biome
2.5.8 rather than inferred from its documentation:

```js
function plainParam(a)        { a = 5; }          // Biome flags
function plainPropAssign(d)   { d.token = 'x'; }  // Biome flags
function depthOne(acc, x)     { acc[x] = 1; }     // Biome flags
const bareReassign = d =>     { d = 5; };         // Biome flags

function destrReassign({ b })  { b = 'x'; }        // MISSED -> rule 1
function destrProp({ c })      { c.token = 'x'; }  // MISSED -> rule 2
const bareProp = d =>          { d.token = 'x'; }; // MISSED -> rule 3
function depthTwo(acc, x, y)   { acc[x][y] = 1; }  // MISSED -> rule 4
```

Rules 1 and 2 are always on, like the original three. Rules 3 and 4 ship
**off by default** and must be opted into per repo, as does
`param-mutating-array-method-call` (rule 5, a `CallExpression` companion to
rules 1–4 that closes the gap ESLint's `no-param-reassign` used to cover) — see
[Opting into the three default-off rules](#opting-into-the-three-default-off-rules).

| Rule | Guards against | Extensions | `--auto-fix` |
| --- | --- | --- | --- |
| [`no-native-map`](#no-native-map) | Native `Map` leaking into Immutable.js state | `.js`, `.jsx` | No — flags known false positives (below), so no rewrite is always safe |
| [`no-arrow-function-create-selector`](#no-arrow-function-create-selector) | Broken Reselect memoization | `.js`, `.jsx` | Yes — unwraps the arrow |
| [`reselect-arity-match`](#reselect-arity-match) | Silently dropped selector inputs | `.js`, `.jsx` | No — the fix would have to guess which side of the mismatch is wrong |
| [`destructure-default-param-assign`](#destructure-default-param-assign) | Reassigning a destructured parameter | `.js`, `.jsx` | No — the fix renames a binding and every reference to it, which is not one byte range |
| [`destructure-param-prop-assign`](#destructure-param-prop-assign) | Mutating a destructured parameter's properties, at any depth | `.js`, `.jsx` | No — the right copy shape (spread, deep clone, Immutable update) can't be inferred from the mutation site |
| [`bare-arrow-param-prop-assign`](#bare-arrow-param-prop-assign) **(off by default)** | Property mutation Biome misses on an arrow's unparenthesized single parameter | `.js`, `.jsx` | No — adding parens would just hand the same mutation to Biome to re-report |
| [`deep-param-prop-assign`](#deep-param-prop-assign) **(off by default)** | Plain-parameter mutation 2+ levels deep, past where Biome stops looking | `.js`, `.jsx` | No — same "would have to guess the copy shape" as above |
| [`param-mutating-array-method-call`](#param-mutating-array-method-call) **(off by default)** | Mutating array-method calls (`push`, `sort`, `splice`, …) on a parameter — the `CallExpression` gap `no-param-reassign` covered, Biome doesn't | `.js`, `.jsx` | No — the right copy shape can't be inferred from the call site |

---

## `no-native-map`

**Message:** `Use Immutable.js Map instead of native Map.`

**Source:** `src/rules/no_native_map.rs` (324 lines — the largest rule, because it
resolves the most import/alias forms against the semantic model)

### What it catches

The dashboard's entire Redux state tree is Immutable.js. Mixing a native `Map`
into that tree breaks equality checks (Immutable's structural equality does not
apply), breaks `toJS()` conversion, and violates the purity assumptions reducers
are written against. The failure is silent — no error, just wrong behaviour in
state comparison and re-render decisions.

The rule reports any bare `Map` identifier reference in a file where Immutable's
`Map` is not in scope.

### Before / after

```js
// ✗ Flagged — native Map in a file with no Immutable Map binding
const cache = new Map();
const lookup = new Map([['a', 1]]);
```

```js
// ✓ Clean — Immutable's Map is bound, so `Map` is not the native one
import { Map } from 'immutable';

const cache = Map();
```

```js
// ✓ Clean — namespace form
import Immutable from 'immutable';

const cache = Immutable.Map();
```

### Immutable-binding forms it understands

The rule tracks how `immutable` entered the file before deciding whether a bare
`Map` is native. All of these register `Map` as Immutable's:

```js
import { Map } from 'immutable';              // named import
import { Map as ImmutableMap } from 'immutable'; // aliased named import
import Immutable from 'immutable';            // default import (namespace alias)
const Immutable = require('immutable');       // require
const { Map } = Immutable;                    // destructure off the namespace
const { Map } = require('immutable');         // destructure off require
```

Each of these registers a *binding* as Immutable's `Map`, and each `Map`
reference is then resolved against [the semantic model](SEMANTIC_MODEL.md)
independently — not a single file-wide switch. A `Map` reference that
resolves to one of these bindings is clean; one that resolves to anything
else (a shadowing parameter, an unrelated local variable, or nothing at all)
is native and gets reported, no matter what else is imported elsewhere in
the same file. See "Scope-aware since the semantic migration" below.

A `Map`-named binding's own declaration is never reported on its own —
`import { Map } from 'immutable'` never flags its own `Map`, and neither
does a parameter or local variable simply named `Map`; only *uses* of a
`Map` value are ever in question.

### Known quirk: `new mapboxgl.Map()` is a false positive

**This is faithful to the original ESLint rule, not a port bug.**

```js
// ✗ Flagged, but not actually a native Map
const map = new mapboxgl.Map({ container, style });
```

The original ESLint rule visited every identifier node named `Map` — including
the property identifier in a member expression — and had no way to distinguish
`mapboxgl.Map` (the Mapbox GL JS map constructor) from a bare global `Map`. This
port reproduces that behaviour exactly.

**Why keep the false positive.** Fixing it would be an easy change, but it would
break the parity guarantee that makes this migration checkable: the tool's output
would no longer match ESLint's, and there would be no way to tell a genuine
regression from an intentional improvement. It is also why the eight existing
Mapbox call sites in the codebase already carry disable comments — the team has
already absorbed this cost. If you want to narrow the rule to skip member-expression
property names, do it as a deliberate, separately-reviewed change after the
migration lands, not as part of it.

### Scope-aware since the semantic migration

This rule now resolves every `Map` reference against
[the semantic model](SEMANTIC_MODEL.md) rather than a single file-wide
"Immutable's Map is bound somewhere" flag. Two consequences, both a
deliberate departure from the original ESLint rule's behavior (see
[SEMANTIC_MODEL.md](SEMANTIC_MODEL.md#why-no-existing-rule-uses-this-yet) for
the earlier decision not to make this change, and why the current migration
brief explicitly asked for it anyway):

- **Shadowing resolves correctly.** `import { Map } from "immutable";
  function test(Map) { return new Map(); }` now reports the inner `new
  Map()` — it resolves to the parameter, not the import, so it really is
  native Map. Previously this was a false negative: the file-wide flag
  suppressed every `Map` in the file the moment any Immutable-derived
  binding existed anywhere in it. A `Map`-named binding's own *declaration*
  (the parameter itself, `const Map = ...`) is never flagged either way —
  declaring a local by that name isn't itself a use of a value.
- **No more file-wide suppression.** A function that imports Immutable's
  `Map` for one purpose and also uses a differently-scoped native `Map` for
  another is now evaluated per reference, not silenced wholesale.

The `Immutable.Map`-shaped forms this rule recognizes (named import, default
import, namespace-style member access, aliases, and the CommonJS
`require('immutable')` form) are unchanged — see the recognized-forms list
above.

---

## `no-arrow-function-create-selector`

**Message:** ``Avoid wrapping createSelector in an arrow function for "<name>". It
breaks memoization (a new selector is created on every call). Use createSelector
directly, or rename to "make<Name>".``

**Source:** `src/rules/no_arrow_function_create_selector.rs`

### What it catches

Reselect memoizes on the **selector instance**. Wrapping `createSelector` in a
throwaway arrow function builds a brand-new selector — with a fresh, empty cache
— on every single call. The memoization is completely defeated. There is no
error and no warning; the only symptom is recomputation and re-rendering on
every access, which is very hard to spot in review.

The rule reports an arrow function whose entire concise body is a
`createSelector(...)` call, when that arrow is the initializer of a variable
declarator, unless the variable's name matches `/^make[A-Z]/`.

### Before / after

```js
// ✗ Flagged — new selector instance on every call, no memoization
export const selectVisibleUsers = () =>
  createSelector([getUsers, getFilter], (users, filter) =>
    users.filter(u => u.type === filter)
  );
```

```js
// ✓ Fixed by calling createSelector directly — one memoized instance
export const selectVisibleUsers = createSelector(
  [getUsers, getFilter],
  (users, filter) => users.filter(u => u.type === filter)
);
```

```js
// ✓ Also accepted — the `make` prefix declares this is a deliberate factory
export const makeSelectVisibleUsers = () =>
  createSelector([getUsers, getFilter], (users, filter) =>
    users.filter(u => u.type === filter)
  );
```

The factory escape hatch exists because per-component selector instances are a
legitimate Reselect pattern (each connected component instance needs its own
cache). The `make` prefix is how the codebase declares that intent, and the rule
honours it via the same `/^make[A-Z]/` test the ESLint rule used.

### Detection precision

The rule requires the arrow's body to be *exactly* `createSelector(...)` as a
concise (expression-bodied) arrow, and requires the tree shape
`JsVariableDeclarator > JsInitializerClause > JsArrowFunctionExpression`. That
mirrors the ESLint rule's direct-parent check.

Consequences — all shared with the original rule:

```js
// Not flagged — block body, not a concise expression body
const selectThing = () => { return createSelector(a, b); };

// Not flagged — the arrow is an argument, not a declarator initializer
useMemo(() => createSelector(a, b), []);

// Not flagged — callee is a member expression, not a bare identifier
const selectThing = () => reselect.createSelector(a, b);
```

### The callee must resolve to reselect's `createSelector`

The bare-identifier callee (`createSelector(...)`, not `x.createSelector(...)`)
is resolved against [the semantic model](SEMANTIC_MODEL.md): it must be bound
by `import { createSelector } from "reselect"`, aliased or not — a same-named
local function, or a `createSelector` imported from a different module, is not
flagged, and an aliased import is still flagged even though the identifier at
the call site is spelled differently:

```js
// ✗ Flagged — resolves to reselect's createSelector despite the alias
import { createSelector as selector } from 'reselect';
export const selectAll = () => selector(a, b);

// ✓ Not flagged — a local function, not reselect's export
function createSelector() {}
export const selectAll = () => createSelector(a, b);

// ✓ Not flagged — createSelector from an unrelated module
import { createSelector } from 'some-other-library';
export const selectAll = () => createSelector(a, b);
```

These are gaps in coverage rather than false positives, and they are intentional:
matching the original rule's reach exactly is what the parity guarantee requires.

### Real-codebase findings

**0 violations** across the dashboard's 141 `createSelector` call sites. The
ESLint rule has kept this clean, and this port confirms it stays clean.

---

## `reselect-arity-match`

**Message:** `createSelector expects <N> parameter(s) in the result function, but
found <M>.`

**Source:** `src/rules/reselect_arity_match.rs`

### What it catches

`createSelector` passes each input selector's output as a positional argument to
the result function. If the result function declares fewer parameters than there
are input selectors, the extra values are silently dropped and the selector
returns wrong data. If it declares more, the extras are `undefined`. Either way:
no error, no warning, just incorrect output. This is the "silent wrong answer"
bug class, which is the worst kind to debug in production.

The rule compares `args.len() - 1` (the input selectors) against the result
function's declared parameter count.

### Before / after

```js
// ✗ Flagged — 2 input selectors, 1 parameter. `filter` is silently dropped.
const selectVisible = createSelector([getUsers, getFilter], users =>
  users.filter(u => u.active)
);
```

```js
// ✓ Fixed — parameter count matches input count
const selectVisible = createSelector([getUsers, getFilter], (users, filter) =>
  users.filter(u => u.type === filter)
);
```

```js
// ✗ Flagged — 1 input selector, 2 parameters. `extra` is always undefined.
const selectCount = createSelector([getUsers], (users, extra) => users.size + extra);
```

### Callee and result-function forms

Both callee shapes are matched, mirroring the ESLint rule's
Identifier/MemberExpression check:

```js
createSelector(...)            // bare identifier
reselect.createSelector(...)   // static member expression
```

The bare-identifier form is resolved against
[the semantic model](SEMANTIC_MODEL.md), the same way as
`no-arrow-function-create-selector`: it must be bound by
`import { createSelector } from "reselect"` (aliased or not), so a
same-named local function or a `createSelector` from a different module is
not checked, while an aliased import still is. The member-expression form
(`x.createSelector(...)`) is unchanged from the original ESLint rule —
matched by member name alone, regardless of what `x` is — since resolving it
semantically would only cover the narrow namespace/default-import case, and
`edge-cases.js`'s own `Reselect.createSelector(...)` example deliberately
never imports `Reselect` at all, to pin down that this form stays
name-based. See
[SEMANTIC_MODEL.md](SEMANTIC_MODEL.md#migrating-the-existing-rules) for the
full reasoning.

Only two result-function forms have a checkable arity:

```js
createSelector(a, b, (x, y) => ...)        // arrow with parameter list
createSelector(a, b, x => ...)             // concise single-param arrow (counts as 1)
createSelector(a, b, function (x, y) {})   // function expression
```

Anything else returns no arity and the call is skipped — most importantly a
selector passed **by reference**, which has no visible parameter list at the call
site:

```js
// Not checked — combineResults' arity is not visible here
createSelector([getA, getB], combineResults);
```

Calls with fewer than 2 arguments are skipped entirely, since there is no
result function to check.

### Report position

The violation is reported at the **result function's** position, not the
`createSelector` call's. This matters for suppressions: the ignore comment goes
on the result function's line.

```js
const selectVisible = createSelector(
  [getUsers, getFilter],
  // custom-biome-ignore-next-line reselect-arity-match -- filter applied upstream
  users => users.filter(u => u.active)
);
```

### Real-codebase findings

**0 violations** across the 141 `createSelector` call sites.

---

## `destructure-default-param-assign`

**Message:** `Reassigning destructured parameter "<name>" mutates a local
binding a caller can't see change. Use a new local variable instead.`

**Source:** `src/rules/destructure_default_param_assign.rs`

### What it catches

Reassignment of a binding introduced by object or array destructuring in a
parameter list — the destructuring equivalent of `noParameterAssign`'s
plain-identifier check. Biome stops at the parameter's own top-level binding
shape; it never asks "did this identifier come from a destructuring pattern in
the parameter list", so none of these are visible to it.

The hazard is the one `noParameterAssign` already exists to catch: the caller
cannot see the change, and the reassigned name no longer means what the
signature says it means for the rest of the function.

### Before / after

```js
// ✗ Flagged — the binding itself is reassigned
const generateBarcodeSuggestions = ({ prefix = '' }) => {
  if (prefix.length >= 8) {
    prefix = '';
  }
  return prefix;
};
```

```js
// ✓ Clean — a new local, leaving the parameter binding alone
const generateBarcodeSuggestions = ({ prefix = '' }) => {
  const normalised = prefix.length >= 8 ? '' : prefix;
  return normalised;
};
```

### What counts as reassignment

The same set `noParameterAssign` itself walks, plus the destructuring-assignment
forms: `b = x`, `b += 1`, `b++`, `--b`, `for (b of list)`, `for (b in obj)`,
`[b] = pair`, `({ b } = source)`.

Nesting depth and defaults are irrelevant — `{ b }`, `{ b = '' }`,
`{ outer: { inner } }`, `{ items: [{ id }] }` and `{ a, ...rest }` all declare
destructured parameters, and reassigning any of their bindings is reported
identically.

### Why there is no bare-arrow variant

JS requires parentheses around a destructured parameter: `{ x } => ...` is a
syntax error. A bare single arrow parameter can therefore never be
destructured, so this rule has no unparenthesized case to handle — unlike
[`bare-arrow-param-prop-assign`](#bare-arrow-param-prop-assign), which exists
precisely because plain parameters *do* have one.

### Boundary with `destructure-param-prop-assign`

Both rules visit the same assignment nodes; the split is purely the assignment
target's shape. A bare identifier target is this rule's; a member/index chain is
the other's. Neither can report the same *assignment target* as the other —
though one line can carry both, since it can carry two writes:
`function f({ b, c }) { b = 1; c.token = 2; }` is one finding from each.

---

## `destructure-param-prop-assign`

**Message:** `Mutating a property of destructured parameter "<name>" changes
data the caller still holds a reference to. Copy it first.`

**Source:** `src/rules/destructure_param_prop_assign.rs`

### What it catches

Property or index writes (dot or bracket, **any depth**) through a binding
introduced by destructuring in a parameter list — the destructuring equivalent
of `noParameterAssign`'s `propertyAssignment: "deny"` check.

This is the rule the dashboard's saga and reducer layer needs: those functions
are built almost entirely around destructured `action`/`payload`/`state`
parameters, and an in-place write on one destructured field silently corrupts
state a sibling saga or reducer reads later.

### Before / after

```js
// ✗ Flagged — mutates the caller's action object
export function* getSMSSessionsOfCustomersSaga({ payload }) {
  payload.token = yield call(getIdToken);
}
```

```js
// ✓ Clean — copy, then mutate the copy
export function* getSMSSessionsOfCustomersSaga({ payload }) {
  const authed = { ...payload, token: yield call(getIdToken) };
}
```

### Depth independence is the point

Biome's own rule catches exactly one level and misses two or more. This one
walks the leftmost chain to its root identifier, so all three of these report
identically:

```js
c.token = 'x';                       // depth 1 — Biome also catches this shape
acc[k].total = 1;                    // depth 2 — Biome misses
state.tours[id].priceBands = {};     // depth 3 — Biome misses
state['tours'][id].priceBands = {};  // mixed notation, same walk
```

Parentheses inside a chain are transparent and do not count as a hop.

### Known non-goals

Both are gaps, deliberately, not bugs — and both are stated here so they are not
re-reported later as missed cases:

- **Aliasing.** `const local = payload; local.token = 'x'` is not flagged.
  `local`'s own binding is a `const`, not a parameter, so it resolves out of
  scope. A rule that tracked this would need real dataflow analysis, which is
  out of scope for a tree-walking `check()` — the same posture
  `no-arrow-function-create-selector` takes for its member-expression callee
  gap.
- **Mutating method calls.** `payload.items.push(x)` mutates in effect but
  contains no assignment node at all. This matches `noParameterAssign`'s own
  scope — it does not catch `d.items.push(x)` either.

---

## `bare-arrow-param-prop-assign`

**Off by default.** See
[Opting into the three default-off rules](#opting-into-the-three-default-off-rules).

**Message:** `Mutating a property of parameter "<name>" is invisible to Biome's
noParameterAssign because this arrow's single parameter has no parens. Add
parens or copy the value first.`

**Source:** `src/rules/bare_arrow_param_prop_assign.rs`

### What it catches

Property or index writes through a **plain** parameter, when that parameter is
the sole, unparenthesized parameter of an arrow function. This is a plain-param
case in principle — the shape `noParameterAssign` is supposed to cover — but
Biome misses it for this exact AST shape:

```js
export const bare  =  d  => { d.token = 'x'; };  // NOT flagged by Biome
export const paren = (d) => { d.token = 'x'; };  // flagged by Biome
```

The cause is structural, not a formatter setting: an arrow with a single
unparenthesized parameter binds it directly under the arrow as an
`AnyJsBinding`, with no `JsParameters` node — unlike `(d) => ...`,
`(a, b) => ...` and `function f(d) {}`, all of which Biome handles. A repo
formatting with `arrowParentheses: "asNeeded"` (as the dashboard does) just
makes the missed shape the common one; Biome's formatter actively strips the
parens its own checker needs.

### Before / after

```js
// ✗ Flagged
initialValues.cabins.forEach(cabin => {
  cabin.id = undefined;
});
```

```js
// ✓ Clean — a copy per element instead of mutating the caller's objects
const cabins = initialValues.cabins.map(({ id, ...rest }) => rest);
```

### Property mutation only — reassignment is Biome's

`d => { d = 5; }` is **not** flagged. The obvious assumption is that
reassignment is missed for the same structural reason property mutation is; the
repro says otherwise — Biome flags bare reassignment. The asymmetry was checked
rather than assumed, and this rule stays out of what Biome already reports.

### Resolution is by binding, not nesting

The mutation need not sit in the body of the arrow that declares the parameter:

```js
arr.map(item => other.forEach(x => { item.y = 1; })); // attributed to `item`
```

`item` resolves through [the semantic model](SEMANTIC_MODEL.md) regardless of
how many arrows separate the write from the declaration, and a local shadowing
the parameter correctly resolves to the local.

---

## `deep-param-prop-assign`

**Off by default.** See
[Opting into the three default-off rules](#opting-into-the-three-default-off-rules).

**Message:** `Mutating parameter "<name>" 2+ levels deep ("<chain>") is
invisible to Biome's noParameterAssign, which only tracks one level. Copy the
value first.`

**Source:** `src/rules/deep_param_prop_assign.rs`

### What it catches

Property or index writes through a **plain** parameter at chain depth 2 or more,
whatever the arrow-parens style. Biome tracks exactly one level:

```js
function f(acc)   { acc[x] = 1; }                       // flagged by Biome — depth 1
function f(acc)   { acc[x][y] = 1; }                    // NOT flagged — depth 2
function f(accum) { accum.tours[id].priceBands = {}; }  // NOT flagged — depth 3
```

This is the plain-parameter counterpart to
[`destructure-param-prop-assign`](#destructure-param-prop-assign)'s depth
independence: same chain walk, inverted eligibility (plain here, destructured
there), so a destructured root never trips both.

### Before / after

```js
// ✗ Flagged — depth 3 through a plain parameter
export function collect(accum, bookingTypeId, priceBands) {
  accum.tours[bookingTypeId].priceBands = priceBands;
}
```

```js
// ✓ Clean — build the new shape rather than writing into the caller's
export function collect(accum, bookingTypeId, priceBands) {
  return {
    ...accum,
    tours: {
      ...accum.tours,
      [bookingTypeId]: { ...accum.tours[bookingTypeId], priceBands }
    }
  };
}
```

### The depth floor is deliberate

Depth 1 is exactly what `noParameterAssign` already reports. Re-flagging it
here, under a second rule name, would be duplicate noise on a line the user is
already being told about — so this rule starts at 2.

### Overlap with `bare-arrow-param-prop-assign` — both fire

A bare-arrow parameter mutated 2+ levels deep trips both opt-in rules:

```js
item => {
  item.a.b = 1; // bare parameter (rule 3) AND depth 2 (rule 4)
};
```

**Both report, independently.** They test genuinely different structural
conditions — arrow-parens shape versus chain depth — each true or false
regardless of the other, and each separately worth suppressing: a reviewer might
parenthesize the arrow (satisfying rule 3, handing the mutation to Biome) while
still wanting the depth watched. Coupling rule 4 to rule 3's detection would
make it fragile to rule 3 changing later, for a benefit the suppression syntax
already covers — one marker carries both names:

```js
item => {
  // custom-biome-ignore-next-line bare-arrow-param-prop-assign, deep-param-prop-assign
  item.a.b = 1;
};
```

On the dashboard, 21 lines trip both rules; the rest trip exactly one.

---

## `param-mutating-array-method-call`

**Off by default.** See
[Opting into the three default-off rules](#opting-into-the-three-default-off-rules).

**Message:** `Mutating array method called on a parameter — this modifies data
the caller still holds a reference to. Copy the parameter first.` Findings in a
file that imports `immutable` (or a `/immutable`-suffixed subpath such as
`redux-form/immutable`) carry a `(low confidence: file imports 'immutable')`
suffix; a finding on a receiver named `fields`/`field` carries a `(low
confidence: receiver named 'fields' may be a redux-form helper)` suffix.

**Severity:** This rule is heuristic because it does not perform type inference.
High-confidence findings indicate a method call that matches a known mutating
Array API on a parameter and report at **error** severity. Low-confidence
findings indicate a known API collision where the same method name may be
non-mutating (an `immutable` import or a `fields`/`field` receiver); they keep
the explanatory suffix and report at **warning** severity instead of
**error** — still surfaced, but de-prioritized for triage. Both severities are
capped by the rule's own off-by-default posture.

**Source:** `src/rules/param_mutating_array_method_call.rs`

### What it catches

Mutating array-method calls (`push`, `pop`, `shift`, `unshift`, `splice`,
`sort`, `reverse`, `fill`, `copyWithin`) whose receiver is rooted in a function
parameter — at any chain depth, in either a plain or destructured parameter
declaration. It is the `CallExpression` counterpart to the four assignment-shaped
parameter-mutation rules: all of those key off assignment/update expressions and
none inspect a call, so `param.push(item)` — a genuine parameter mutation — was
invisible to them and to Biome's `noParameterAssign` (which only sees
assignments), exactly the gap ESLint's `no-param-reassign` used to close.

```js
// Plain or destructured, direct or chain-rooted — all fire on `param.x(...)`
export const buildProductsData = ({ productsData }, product) => {
  productsData.push(product); // the instance that motivated the rule
};

function collect(accum, item) { accum.push(item); }            // plain, direct
function addToGroup(accum, key, item) { accum[key].items.push(item); } // chained
function sortInPlace(list) { list.sort(); }                    // in-place sort
```

The detection is **name-based**: a fixed mutating-method list, on a receiver
whose chain root resolves via the semantic model to a `Parameter` binding. There
is deliberately no type inference — this tool resolves identifiers to *bindings*,
not *types* — which is the central design tradeoff of the rule (see below).

### Before / after

```js
// ✗ Flagged — mutates the caller's array in place
export const buildProductsData = ({ productsData }, product) => {
  productsData.push(product);
};
```

```js
// ✓ Clean — copy first, then mutate the copy
export const buildProductsData = ({ productsData }, product) => {
  const result = [...productsData, product];
  return result;
};
```

### The precision tradeoff (read this before enabling)

Unlike the four assignment rules, whose property-assignment targets are
*unambiguously* mutations, `x.push(...)` only mutates `x` when `x` is a native
`Array` at runtime — which this tool cannot determine from syntax alone. Two
pervasive same-named, **non-mutating** idioms in codebases like the dashboard
reuse these exact method names:

- **Immutable.js** (`Immutable.List#push`, `Immutable.Map#set`, …) returns a new
  collection and does not mutate the receiver.
- **Redux-Form's `FieldArray` `fields` helper** (`fields.push()`, `fields.remove()`)
  dispatches actions rather than mutating a plain array.

In scoping against the dashboard corpus, roughly half of the raw mutating-method
calls on parameters turned out to be one of these two idioms. The rule therefore
ships **off by default** and, when enabled, treats the two findings differently:

- **High-confidence findings** — a known mutating Array API on a parameter with
  no collision signal — report at **error** severity.
- **Low-confidence findings** — the same call but in a file that imports
  `immutable` or on a `fields`/`field` receiver — report at **warning** severity
  and carry the `(low confidence: …)` suffix.

The suffix plus the downgraded severity is a *marker for triage priority*, **not**
a suppression — the finding still reports, because a name/import
heuristic is not safe enough to auto-hide a real `productsData.push(product)`-
shaped bug behind a coincidental variable name. Low-confidence findings surface
as warnings so they are visible but do not block a build the way an error would.

### Known non-goals

- **Map/Set-shaped method names (`set`, `delete`, `clear`, `add`) are entirely
  out of scope for v1.** Every sampled `set`/`delete`/`add` call on the
  dashboard turned out to be Immutable.js; a name-based rule over them would
  round to near-zero true positives, so no companion rule is proposed.
- **Bracket/computed calls** (`list['push'](item)`) — rare enough in practice to
  not model in v1.
- **Aliasing** (`const local = list; local.push(item)`) — same non-goal the
  assignment rules carry; needs real dataflow analysis.
- **Distinguishing Immutable.js / redux-form receivers from plain arrays by
  type** — the core limitation above, addressed by scope-narrowing (array methods
  only) and workflow (mandatory manual triage before suppression), not by
  pretending the tool can tell the difference syntactically.

### Rollout recommendation

Do **not** adopt this rule the way the assignment rules were (enable, `--write-fix`,
blanket-suppress the old `eslint-disable`s). Generate a findings-review doc first
and triage every finding — especially the low-confidence ones — by hand before
adding any `custom-biome-ignore` comment. Auto-suppressing an Immutable.js
`.push()` false positive reads as "yes, this is a real parameter mutation,
intentionally allowed," which is actively misleading. `--write-fix` still works
mechanically; the recommendation is a workflow caution, not a capability gap.

---

## Opting into the three default-off rules

`bare-arrow-param-prop-assign`, `deep-param-prop-assign`, and
`param-mutating-array-method-call` are the only rules whose `default_severity()`
is `off`. With no configuration they never run — the same posture as Biome's own
`noParameterAssign.propertyAssignment: "allow"` default. Turn them on by giving
them a severity in `package.json`:

```json
{
  "ignoreBiomeExtensionRules": {
    "bare-arrow-param-prop-assign": "error",
    "deep-param-prop-assign": "warn",
    "param-mutating-array-method-call": "warn"
  }
}
```

No entry → the rule never runs. `"warn"`/`"error"` → it runs at that severity.
`"off"` → the same as no entry, stated explicitly.

This reuses the existing severity mechanism rather than adding a second,
Biome-shaped config namespace; see
[ADDING_A_RULE.md](ADDING_A_RULE.md#default-severity-shipping-a-rule-off-by-default)
for how `default_severity()` works and when a new rule should use it. Unlike the
other five rules, these three appear in the `-v` "rules ignored" listing when
unconfigured, because that is what unconfigured means for them.

---

## Real-codebase findings summary

### The original three rules

Run against the dashboard's `src/` tree
(`custom-biome-lint 'src/**/*.{js,jsx}'`):

```
✖ 8 errors in 8 files
```

All 8 are `no-native-map`. All 8 are **pre-existing and already suppressed under
ESLint** — every one sits on a line that already carries
`// eslint-disable-next-line customPlugin/no-native-map`. All 8 are the
`new mapboxgl.Map()` false-positive class described above.

| File | Reported line | Existing disable comment |
| --- | --- | --- |
| `src/components/AddExternalEvents/ExternalEventForm.jsx` | 173:30 | line 172 |
| `src/components/CompassDashboardV2/Map.js` | 17:30 | line 16 |
| `src/components/FacilitiesManager/GeofenceEditor.jsx` | 82:30 | line 81 |
| `src/components/FacilitiesManager/LocationPicker.jsx` | 36:30 | line 35 |
| `src/components/MapBuilder/MapCanvas.jsx` | 120:30 | line 119 |
| `src/components/POIManager/tabs/LocationTab.jsx` | 109:30 | line 108 |
| `src/sagas/fetchResourceCapacityForVessel.js` | 99:24 | line 98 |
| `src/sagas/manageSagaCancellation.js` | 11:25 | line 10 |

**Positional parity is verified**: in every case the reported line is exactly one
past the existing `eslint-disable-next-line` comment, which is precisely where
ESLint was suppressing a finding. That one-to-one correspondence — 8 findings, 8
pre-existing suppressions, each adjacent — is the evidence that the port
reproduces the original rule's behaviour rather than approximating it.

**These 8 comments need translating** from ESLint syntax to this tool's syntax
when ESLint is removed. The tool deliberately does not read `eslint-disable*`
comments. See [MIGRATION_NOTES.md](MIGRATION_NOTES.md) for the exact diff.

Note the migration plan (`biome-migration.md`) refers to "9 active suppressions".
That count is off by one: a repo-wide search finds exactly 8
`eslint-disable-next-line customPlugin/no-native-map` comments in `src/`, matching
the 8 findings one-for-one. The only other file mentioning the rule name is
`scripts/rebuildEslintDisables.js`, which references it as a string in tooling
rather than suppressing anything. Treat 8 as the real number.

### The four assignment-shaped parameter-mutation rules

Run against the dashboard's `src/` and `cypress/` trees (4,416 files) with both
opt-in rules enabled, at dashboard commit `be48c6d81`:

`param-mutating-array-method-call` (the call-shaped fifth rule) is deliberately
absent from this tally: its dashboard corpus scoping lives in
`docs/PARAM_MUTATING_METHOD_CALL_RULE_PLAN.md`, and — unlike the four rules
above — it was scoped *before* being built rather than verified against a prior
`eslint-disable` baseline, because the dashboard's removed `no-param-reassign`
never reached `CallExpression`s at all.

| Rule | Findings | Files |
| --- | --- | --- |
| `destructure-default-param-assign` | 4 | 4 |
| `destructure-param-prop-assign` | 8 | 7 |
| `bare-arrow-param-prop-assign` | 81 | 31 |
| `deep-param-prop-assign` | 137 | 34 |

**Positional parity with the removed ESLint rule is the verification here, the
same way adjacency to existing `eslint-disable` comments verified
`no-native-map`'s port.** 222 of the 230 findings (96%) land on a line that
already carries an `eslint-disable`/`eslint-disable-next-line` for
`no-param-reassign` — i.e. on precisely the lines the dashboard's own ESLint
setup was suppressing before the rule was removed. 21 lines trip both opt-in
rules, consistent with the documented decision to let them fire independently.

The 8 findings *not* on a previously-suppressed line were each read by hand and
are all genuine:

- 5 in `src/components/VesselAddEdit/index.js` sit under an inline
  `/*eslint no-param-reassign: ["error", { "props": false }]*/` comment, which
  turned ESLint's property check off for the rest of that file. This tool
  deliberately does not read `eslint-*` comments, so it reports them; they want
  a `custom-biome-ignore` marker if that local decision still stands.
- 1 in `src/lib/datadog/dogapi/api/metric.js` (`metrics[i].points = ...`) and
  2 others are depth-2 writes in code ESLint was not covering.

Two notes for anyone re-running this:

- The scoping estimates in
  `DESTRUCTURED_PARAM_MUTATION_RULES_PLAN.md` (~13 / ~69 / ~32) were derived by
  counting existing `eslint-disable` comments and bucketing them by cause. The
  rules report every occurrence, not only previously-suppressed ones, and a
  single line can belong to more than one bucket — which is why
  `deep-param-prop-assign` lands at 137 rather than ~32. The 98%
  already-suppressed rate for that rule is what rules out over-firing as the
  explanation.
- `bare-arrow-param-prop-assign`'s count is a direct function of the dashboard's
  `arrowParentheses: "asNeeded"` formatting. A repo that parenthesizes arrow
  parameters has no use for that rule at all, which is why it ships off.

## Fixtures

Each rule has four fixture files under `fixtures/<rule_name>/`:

| File | Purpose |
| --- | --- |
| `valid.js` | Patterns that must **not** be reported |
| `invalid.js` | Patterns that must be reported, at known line:col |
| `suppressed.js` | The same violations, silenced by ignore comments |
| `edge-cases.js` | Documented boundary behavior — gaps in coverage and known quirks, each pinned to an exact violation count |

Running the tool over all fixtures yields **52 errors in 10 files** out of 28
fixture files, each a deliberate, documented behavior rather than a bug:

| Rule | `invalid.js` | `edge-cases.js` |
| --- | --- | --- |
| `no-native-map` | 2 | 2 |
| `no-arrow-function-create-selector` | 2 | 1 |
| `reselect-arity-match` | 3 | 1 |
| `destructure-default-param-assign` | 5 | 17 |
| `destructure-param-prop-assign` | 5 | 14 |

`valid.js` and `suppressed.js` contribute nothing. The two opt-in rules
contribute nothing either — they are off in this repo's own `package.json`, so
their fixtures are exercised by `cargo test` (which runs a named rule directly)
rather than by a CLI run over `fixtures/`.

Four `valid.js` files carry a `custom-biome-ignore` marker naming the *other*
rule of a pair: a near-miss for one rule is often a genuine finding for its
sibling (a destructured parameter is the wrong shape for
`bare-arrow-param-prop-assign` and exactly the right shape for
`destructure-param-prop-assign`). Keeping the marker there is what preserves the
property that no `valid.js` reports anything in a whole-directory run.

See [TESTING.md](TESTING.md).
