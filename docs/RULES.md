# Rules

Three rules, each a behaviour-for-behaviour port of a custom ESLint rule from
`eslint-rules/`. The ports are deliberately faithful rather than "improved" —
including reproducing one known false-positive class — so that turning this tool
on produces exactly the findings the old ESLint setup produced, no more and no
less. That property is what makes the migration verifiable.

| Rule | Guards against | Extensions | `--auto-fix` |
| --- | --- | --- | --- |
| [`no-native-map`](#no-native-map) | Native `Map` leaking into Immutable.js state | `.js`, `.jsx` | No — flags known false positives (below), so no rewrite is always safe |
| [`no-arrow-function-create-selector`](#no-arrow-function-create-selector) | Broken Reselect memoization | `.js`, `.jsx` | Yes — unwraps the arrow |
| [`reselect-arity-match`](#reselect-arity-match) | Silently dropped selector inputs | `.js`, `.jsx` | No — the fix would have to guess which side of the mismatch is wrong |

---

## `no-native-map`

**Message:** `Use Immutable.js Map instead of native Map.`

**Source:** `src/rules/no_native_map.rs` (298 lines — the largest rule, because it
is the only stateful one)

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

Once any of these establishes a `Map` binding, the rule stops reporting bare
`Map` references in that file entirely — matching the original ESLint rule,
which used the same file-level "is Immutable's Map in scope" heuristic rather
than real scope analysis.

Identifiers inside import specifiers are never reported, so `import { Map } from
'immutable'` does not flag its own `Map`.

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

### Other limitations

- **No real scope analysis.** A locally-shadowed `Map` (e.g. a function parameter
  named `Map`) is treated the same as a global. The original rule had the same
  limitation.
- **File-level binding state.** If Immutable's `Map` is imported anywhere in the
  file, no bare `Map` is reported anywhere in that file, even in a function that
  genuinely wants the native one.

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
  // biome-ignore-next-line reselect-arity-match -- filter applied upstream
  users => users.filter(u => u.active)
);
```

### Real-codebase findings

**0 violations** across the 141 `createSelector` call sites.

---

## Real-codebase findings summary

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

## Fixtures

Each rule has four fixture files under `fixtures/<rule_name>/`:

| File | Purpose |
| --- | --- |
| `valid.js` | Patterns that must **not** be reported |
| `invalid.js` | Patterns that must be reported, at known line:col |
| `suppressed.js` | The same violations, silenced by ignore comments |
| `edge-cases.js` | Documented boundary behavior — gaps in coverage and known quirks, each pinned to an exact violation count |

Running the tool over all fixtures yields 12 errors in 6 files — 7 from the
three `invalid.js` files (2 from `no-arrow-function-create-selector`, 2 from
`no-native-map`, 3 from `reselect-arity-match`), plus 5 more from the three
`edge-cases.js` files (3 from `no-native-map`, 1 from
`no-arrow-function-create-selector`, 1 from `reselect-arity-match`) — each a
deliberate, documented behavior rather than a bug. `valid.js` and
`suppressed.js` contribute nothing. See [TESTING.md](TESTING.md).
