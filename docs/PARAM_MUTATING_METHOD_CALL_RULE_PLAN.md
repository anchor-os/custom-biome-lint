# Plan: a rule for mutating method calls on function parameters

**Status: PROPOSED, not started.** Follows from the `no-param-reassign` audit
that produced `DESTRUCTURED_PARAM_MUTATION_RULES_PLAN.md`. That audit's
final verification pass found 3 remaining uncovered entries; 2 are confirmed
non-issues (out of scope by design, or a dead comment), and the 3rd —
`src/api/hb/graphql/transforms/tourAttendees.js:42`,
`productsData.push(product);` — is a genuinely new gap none of the four
existing rules (or Biome itself) can close, because it's a **mutating method
call**, not an assignment expression. This document scopes that gap for real
against the dashboard corpus (not just the one instance that happened to
surface) before proposing a rule.

## Rationale

All four shipped rules — `destructure-default-param-assign`,
`destructure-param-prop-assign`, `bare-arrow-param-prop-assign`,
`deep-param-prop-assign` — and Biome's own `lint/style/noParameterAssign`
key off **assignment-shaped** AST nodes: `x = y`, `x.y = z`, `x[y] = z`,
update expressions, `for...of`/`for...in` targets. None of them inspect a
`CallExpression` at all. That means:

```js
export const buildProductsData = ({ productsData }, product) => {
  ...
  } else {
    productsData.push(product); // mutates the caller's array; no rule sees this
  }
};
```

is invisible to every rule in this tool and to Biome, for the same
underlying reason `no-param-reassign` used to catch it and
`noParameterAssign` doesn't: ESLint's `no-param-reassign` (removed from this
codebase) tracked property mutation broadly enough to include method calls
that are known to mutate (`push`, `splice`, etc., via the
`assignmentsToWatch`/known-mutating-method-list mechanism most codebases'
`no-param-reassign` configs enable, called `props: true` with warnings on
these specific method names in `eslint-plugin-...` implementations codebases
commonly layer on). Biome's `noParameterAssign` was never designed to reach
method calls — it works on assignment expressions, not call expressions —
so this was never going to be "one more depth level" the way Rules 3 & 4
were; it's a structurally different AST shape needing its own detection
logic entirely.

### Biome alternative check (step 1) — confirmed, none exists

Checked `biome explain` against every plausibly-relevant rule name and swept
the full rule list in `node_modules/@biomejs/biome/configuration_schema.json`
for anything mentioning mutation, parameters, assignment, or method calls:

```
noAssignInExpressions, noCatchAssign, noClassAssign, noConstAssign,
noDoneCallback, noDuplicateParameters, noEmptyTypeParameters,
noExcessiveNestedCallbacks, noFunctionAssign, noGlobalAssign,
noGlobalObjectCalls, noImportAssign, noMisrefactoredShorthandAssign,
noMultiAssign, noParameterAssign, noParameterProperties,
noParametersOnlyUsedInRecursion, noReactPropAssignments, noReturnAssign,
noSelfAssign, noUnassignedVariables, noUnusedFunctionParameters,
useConsistentMethodSignatures, useDefaultParameterLast,
useIterableCallbackReturn, useMaxParams, ..., useReduceTypeParameter, ...
```

The closest-sounding candidate, `noAccumulatingSpread`
(`lint/performance/noAccumulatingSpread`), is a performance rule about
`...spread` inside `reduce` accumulators, unrelated to method-call mutation.
`noReactPropAssignments` is React-prop-specific, not general parameter
mutation. Nothing in this list, nor `noParameterAssign`'s own
description (`biome explain noParameterAssign` — "Disallow reassigning
`function` parameters," examples are all assignment/update-expression
shaped), reaches a `CallExpression`. Confirmed: no Biome alternative exists,
same conclusion as the earlier `noParameterAssign` gap investigation, same
diligence applied.

## Scoping (step 2) — real corpus data, not the one known instance

Grepped `src/` and `cypress/` in the dashboard repo for the mutating method
names, then **parsed every matching file with `@babel/parser` +
`@babel/traverse`** (both already present in the dashboard's own
`node_modules`) and resolved each call's receiver identifier to its actual
scope binding — not name-matching, real binding resolution — keeping only
calls whose receiver binding is `kind === 'param'` (a function parameter,
plain or from a destructuring pattern, any depth of member-chain from it).
This is the same rigor as the original classification report
(`no-param-reassign-classification.md`), just automated with a real parser
instead of manual `biome check --only=...` repro, since this shape needs
call-expression + scope resolution that a lint-rule-flag repro can't test
directly.

Methods checked: `push, pop, shift, unshift, splice, sort, reverse, fill,
copyWithin` (array-mutating) and `set, delete, clear, add` (Map/Set-shaped
names).

**Raw result: 333 confirmed parameter-receiver mutating-method calls.**

| Category | Count | Methods |
| --- | --- | --- |
| Array-shaped | 251 | `push` 228, `sort` 21, `splice` 2 |
| Map/Set-shaped | 82 | `set` 69, `delete` 8, `add` 5 |
| **Total** | **333** | |

Split by parameter shape: 275 plain-identifier receivers, 58 destructured
(e.g. `({ productsData }) => { productsData.push(...) }`). Split by chain
depth: 216 direct (`param.push(...)`), 117 via a property chain from the
parameter (`param.items.push(...)`, `event.products.splice(...)`).

This easily clears "more than a handful" — but the raw count overstates the
genuinely actionable scope, for a reason specific to this codebase that the
existing four rules never had to deal with (see next section).

### The real precision problem: this codebase's two dominant non-mutating, same-named APIs

Unlike `x.y = z` (property assignment is unambiguously a mutation no matter
what `x` is), a bare method name like `.push`/`.set`/`.add`/`.delete` is
**not proof of mutation** — it depends entirely on the receiver's runtime
type, which this tool has no type system to determine. Sampling the raw 333
found this is not a theoretical concern; it's the dominant pattern in this
specific codebase, for two reasons documented in this repo's own
`CLAUDE.md` (`Redux with Immutable.js for state management`,
`Redux-Form is used throughout`):

**1. Immutable.js's fluent API reuses `Array.prototype`/`Map.prototype`
method names for non-mutating operations.** `Immutable.List#push`,
`Immutable.Map#set`, `Immutable.Map#delete`, `Immutable.Set#add` all
**return a new collection** — none of them mutate the receiver. 694 of this
codebase's files import from `'immutable'` (vs. 104 total `new Map(`/`new
Set(` constructions codebase-wide, i.e. genuine native Map/Set use is rare).
Real examples pulled straight from the scoping data:

```js
// src/components/EventBuilder/PriceMatrix/index.js:29 — Immutable.Map#set,
// NOT a mutation. product is an Immutable Record/Map; this returns a new one.
return product.set('name', name);

// src/components/EventBuilder/Edits/SelectPublicPricelists/Pricing.jsx:399
// Immutable.List#push, NOT a mutation — pricelists.push(...) returns a new List.
onClick={() => onUpdatePricingState({ pricelists: pricelists.push(Map({ pricelistId: '' })) })}

// src/selectors/dailyOpsReport.js:49 — same story, Immutable.Map#set.
export const setDailyOpsMappings = (state, payload) => state.set('mappings', payload);
```

**2. Redux-Form's `FieldArray` render prop provides a `fields` helper whose
`.push`/`.remove`/`.swap`/`.unshift` methods dispatch Redux actions — they do
not mutate a plain array parameter either**, even though the parameter
itself is a plain (non-Immutable) object:

```js
// src/components/ProductsAddEdit/VendorFields.jsx:136 — `fields` here is the
// object redux-form's <FieldArray render={({ fields }) => ...}/> provides,
// not an Array. fields.push() dispatches an ARRAY_PUSH action; it doesn't
// mutate anything the caller can observe as a mutation bug.
<Button disabled={fields.length >= 10} color={'secondary'} onClick={() => fields.push()}>
```

Both idioms are pervasive enough that a heuristic classification pass
(checking each call's enclosing file for an `immutable` import combined with
either an Immutable-constructor-wrapped argument like `Map(...)`/`fromJS(...)`
or a `set`/`delete` method name, and separately checking for a `redux-form`
import combined with a `fields`/`field` receiver name) accounts for a large
share of the raw 333 on its own — and even that heuristic under-counts,
since it missed `VendorFields.jsx`'s `fields.push()` above (that file
imports from `'redux-form/immutable'`, a subpath the heuristic's naive
`from 'redux-form'` check didn't match) and several `.set(...)` calls whose
enclosing file doesn't import `'immutable'` directly because the
Immutable-ness of the receiver (`values`, `state`, `product`) was
established elsewhere (a reducer, a prop passed down) rather than visible
in the same file:

```js
// src/components/ManageOnlineShop/AddCategoryForm/AddCategoryForm.jsx:45
// `values` is a redux-form Immutable value; .set() here is non-mutating,
// but nothing in THIS file's imports proves that.
const submissionValues = values.set('actionType', this.state.actionType);
```

After applying that (deliberately conservative, still imperfect) filter:

| Bucket | Count | Verdict |
| --- | --- | --- |
| Redux-form `fields` helper (detected) | 16 | false positive |
| Immutable.js (detected: import + wrapped arg or `set`/`delete`) | 87 | false positive |
| Imports `immutable`, signal unclear | 105 | **undetermined** — likely mostly false positive per the manual samples above, but not provably so from this heuristic |
| Neither signal found | 125 | **best candidates for genuine mutation** |

The "neither signal" 125 is dominated by exactly the pattern the existing
four rules were built to close and that the original classification report
called out as this codebase's single most common mutation shape — `reduce`/
`forEach` accumulator mutation, just via a method call instead of a
property assignment:

```js
// src/sagas/pollForOrderFulfillment.js:136
accum.push(item);

// src/components/MembershipDashboard/MembershipOverview.jsx:51,58,65
acc.push([name, count]);

// src/components/Sidebar/Sidebar.jsx:62,64
accum.favoriteLists.push(item);
accum.unFavoriteLists.push(item);

// src/api/hb/graphql/transforms/tourAttendees.js:42 — the original instance
productsData.push(product);
```

But even this "best candidate" bucket cannot be taken as 125 confirmed true
positives — 105 of the 333 remain genuinely undetermined by any heuristic
this scoping pass could apply without a type system, and the 3 `set`/`add`
calls that slipped into "neither" (`product.set(...)`,
`values.set(...)`, `state.set('mappings', ...)`) are, per the samples above,
almost certainly still Immutable.js despite the heuristic missing the
signal. **The Map/Set-named bucket (82 raw) is unsalvageable by
name-matching in this codebase** — every single sampled `set`/`delete`/`add`
call, including ones the heuristic couldn't classify, turned out on manual
inspection to be Immutable.js. The array-named bucket (251 raw) has real
signal but a meaningfully higher false-positive rate than any of the four
existing rules, which never had this problem because property-assignment
mutation has no legitimate non-mutating homonym.

**Conclusion: the scope is real and non-trivial (well over 100 likely-true
positives even after conservative filtering) — worth a rule — but the rule's
central design problem, unlike Rules 1–4, is precision, not detection.**

## Rule proposal: `param-mutating-array-method-call`

One rule, not a plain/destructured split like Rules 1 vs. 2. Rules 1 and 2
are split because *destructured-vs-plain* changes the AST shape being
matched (reassignment-of-a-binding vs. property-mutation-of-a-binding) —
detection logic genuinely differs. For method calls there is no such
split: `param.push(x)` and `({ param }) => { param.push(x) }` are the exact
same AST shape (a `CallExpression` whose callee's object resolves to a
`Parameter` binding, any depth); the only thing that varies is how the
binding was declared, which is already exactly the kind of check the shared
helper module from `DESTRUCTURED_PARAM_MUTATION_RULES_PLAN.md` step 2
(chain-root walker + destructured-vs-plain classifier) already provides and
this rule can reuse outright.

**Scope decision: array-mutating methods only for v1.** `set`/`delete`/
`clear`/`add` are excluded entirely — not shipped off-by-default like Rules
3 & 4, simply not built — because the scoping data shows they are
dominated by Immutable.js false positives in this specific codebase to a
degree no name-based heuristic resolves (see "Non-goals"). This can be
revisited if a future need for genuine native-Map/Set mutation coverage
arises with a real type-inference story attached.

### Should flag

```js
// Plain param, direct receiver — the original instance.
export const buildProductsData = ({ productsData }, product) => {
  productsData.push(product);
};

// Plain param, direct receiver, function declaration form.
function collect(accum, item) {
  accum.push(item);
  return accum;
}

// Destructured param, direct receiver.
export const addFavorite = ({ favorites }, item) => {
  favorites.push(item);
};

// Chained receiver — a property reached from the parameter, any depth
// (mirrors deep-param-prop-assign's depth-independence).
function addToGroup(accum, key, item) {
  accum[key].items.push(item);
}

// In-place sort of a parameter's array.
function sortInPlace(list) {
  list.sort();
}

// splice, shift/unshift/pop/reverse/fill/copyWithin — same shape as push.
function removeFirst(list) {
  list.shift();
}
```

### Should NOT flag

```js
// Non-mutating array methods — the precision-critical near-miss. map/filter/
// slice/concat/reduce/find/every/some/includes/indexOf never mutate; a rule
// that only recognizes the fixed mutating-method-name list never reaches
// these, but every fixture set needs an explicit example proving it.
function transform(list) {
  return list.map(x => x * 2).filter(Boolean);
}

// Reading, not calling a mutating method as a value (no CallExpression at all).
function getPusher(list) {
  return list.push; // a reference, not a call — nothing to flag
}

// A local copy, not the parameter itself.
function safeAdd(list, item) {
  const copy = [...list];
  copy.push(item); // mutates `copy`, a local const, not the parameter binding
  return copy;
}

// Known non-goal, same category as the existing rules' aliasing non-goal:
// a local alias of the parameter. Out of reach without real dataflow analysis.
function aliased(list, item) {
  const local = list;
  local.push(item); // resolves to `local`'s own binding, not `list`'s
}

// Immutable.js / redux-form `fields` helper — the central precision risk.
// This rule CANNOT reliably distinguish these from a real mutation by name
// alone; see "Design tradeoff" below for what v1 does about it.
function immutableSet(productRecord, name) {
  return productRecord.set('name', name); // NOT mutation — Immutable.Map#set
}
```

### Detection sketch

1. Find every `CallExpression` whose `callee` is a non-computed
   `MemberExpression` (`x.push(...)`, not `x['push'](...)` — bracket-form
   calls to these methods are rare enough in this codebase's samples to be a
   Non-goal, see below) whose `property.name` is in the fixed set `push,
   pop, shift, unshift, splice, sort, reverse, fill, copyWithin`.
2. Walk the callee's `object` down any chain of non-computed
   `MemberExpression`s to find the chain's root identifier (reuse the
   leftmost-member-chain-root walker from
   `DESTRUCTURED_PARAM_MUTATION_RULES_PLAN.md` step 2 — this rule needs the
   exact same walk Rules 2 and 4 already use).
3. Resolve the root identifier via `file.semantic()` to a binding; only
   fire if it resolves to a `Parameter` binding (reuse the
   destructured-vs-plain classifier from the same shared helper to label the
   finding, not to gate it — both shapes fire the same rule here).
4. No further gating in v1 — see "Design tradeoff" for why a name-based
   whitelist is the whole detection strategy, and what that costs in false
   positives.

### Design tradeoff: name-based method whitelist, no type inference

This is the central design question the coordinator's task asked to be
flagged explicitly, and the scoping data in this document is the concrete
evidence for it, not a hypothetical: **this rule cannot know, from syntax
alone, whether `x.push(...)` mutates `x` or returns a new value**, because
that depends on `x`'s runtime type (`Array` vs. `Immutable.List` vs.
redux-form's `fields` helper vs. anything else with a same-named method).
`custom-biome-lint` has no type system and no cross-module dataflow — it
resolves identifiers to *bindings* (this rule's own detection depends on
that), not to *types*.

Two paths considered:

1. **Ship it anyway, name-based, and accept the false-positive rate** —
   what this plan recommends, for the array-method subset only, gated by
   an off-by-default severity and a mandatory review-before-suppress
   workflow (below). This is consistent with how the existing rules made
   their own pragmatic tradeoffs (`no-native-map`'s accepted
   `mapboxgl.Map` false positive, ported faithfully from the original
   ESLint rule rather than "fixed"; Rules 3 & 4 shipping off-by-default
   specifically because their real occurrence count came in over
   estimate). The false-positive rate measured here (at least 87 clear +
   up to 105 more likely Immutable.js hits out of 333, i.e. roughly
   half) is higher than any of those precedents, which is exactly why
   this plan recommends being more conservative than Rules 3 & 4 were,
   not equally conservative — see "Suppression syntax and default
   enablement."
2. **Exclude Map/Set-named methods (`set`/`delete`/`clear`/`add`) from v1
   entirely** — recommended, and separate from tradeoff #1. This isn't a
   severity/config knob decision, it's a scope decision: every sampled
   `set`/`delete`/`add` call in this corpus, including ones a
   conservative import-based heuristic couldn't classify, turned out on
   manual inspection to be Immutable.js. There is no reasonable
   config-time mitigation for a rule whose true-positive rate in its
   only real test corpus rounds to zero.

A weaker mitigation worth building into the array-method rule even so:
**a "low confidence" flag on each finding** when the enclosing file imports
`'immutable'` or `'immutable'`'s `/immutable`-suffixed subpath re-exports
(`redux-form/immutable`, per `VendorFields.jsx` above), or when the method's
receiver identifier is named `fields`/`field` (the redux-form convention).
This doesn't suppress the finding — this tool doesn't have enough signal to
safely auto-suppress based on a name/import heuristic alone, and doing so
would risk hiding a real `productsData.push(product)`-shaped bug behind a
coincidental variable name — but it lets `--format json` consumers and a
human triage pass prioritize the "neither signal" findings first, mirroring
this document's own scoping methodology.

### Suppression

Same syntax as the four existing rules — `// custom-biome-ignore-line
param-mutating-array-method-call` / `// custom-biome-ignore-next-line
param-mutating-array-method-call`, JSX `{/* */}` form where required. No new
suppression mechanism needed.

### Edge cases to pin down with fixtures

- Method called via optional chaining: `list?.push(item)` — should still
  resolve the same way; confirm `OptionalMemberExpression`/
  `JsCallExpression` with an optional callee doesn't break the chain-root
  walker.
- Computed member call, `list['push'](item)` — Non-goal for v1 (see below),
  confirm the rule explicitly does not attempt to match this shape rather
  than silently missing it in a way that looks like a bug.
- Method call as a callback reference, not invoked at the call site itself:
  `list.forEach(callback)` where `callback` later does `list.push(...)` on
  a *different* list captured by closure — should resolve correctly via
  normal scope resolution since the receiver identifier in the actual
  `CallExpression` is what's checked, not the outer `forEach` call.
- A parameter shadowed by a nested function's own same-named parameter —
  must resolve to the nearest enclosing binding, same requirement Rules 1–4
  already have covered via `file.semantic()`.
- `Array.prototype` method called on something statically known to be a
  `Map`/`Set`/other non-Array value reached from a parameter (e.g.
  `paramThatIsAMap.forEach(...)` calling `.push` on an inner value) — not
  expected to occur for the array-only method list in v1; call out
  explicitly in a fixture if the scoping sample turns up an example.
- The `fields`/`field`-name low-confidence flag should not become a
  suppression gate — it needs a fixture proving the rule still fires (at
  low confidence) on a *plain array* parameter that happens to be named
  `fields`, not just on the redux-form idiom.

## Suppression syntax and default enablement

**Recommend: off by default (opt-in via `ignoreBiomeExtensionRules`), same
mechanism as Rules 3 & 4** — but with a stronger recommendation than those
two ever needed: **do not roll this out the way Rules 1–4 were rolled out
(enable, `--write-fix`, auto-remove old `eslint-disable`s).** Generate a
findings-review doc first (the same workflow already used for Rules 3 & 4's
218 findings before they were enabled for real) and require manual triage
of every finding before any auto-suppression, specifically because:

- The false-positive rate measured here (roughly half of raw findings, by
  the conservative heuristic in this document) is categorically higher than
  Rules 3 & 4's — those rules' 218-vs-101-159-estimate gap was "more real
  findings than expected," not "roughly half are provably not bugs at all."
- Auto-suppressing an Immutable.js `.push()` false positive with a
  `custom-biome-ignore-line param-mutating-array-method-call` comment is
  actively misleading — it reads as "yes, this is a real parameter mutation,
  intentionally allowed," when it isn't one at all. That's a worse outcome
  than leaving it unsuppressed, unlike Rules 1–4 where every suppressed line
  genuinely was the mutation the rule describes.

`--write-fix` should still work mechanically (it has no rule-specific
awareness, it fixes whatever `plan_file` is told to fix) — the
recommendation not to use it for a blanket rollout is a workflow
recommendation for whoever adopts this rule, not a capability gap in the
tool.

## Non-goals

- **Map/Set-shaped method names (`set`, `delete`, `clear`, `add`) — excluded
  from v1 entirely**, per the scoping data above. Revisit only alongside a
  real type-inference story (e.g. resolving a receiver's construction site
  to `new Map(`/`new Set(` with no intervening Immutable-wrapping), not as
  a name-based heuristic.
- **Bracket-form / computed method calls** (`list['push'](item)`) — rare
  enough in this codebase's sample to not be worth the added detection
  complexity in v1.
- **Aliasing** (`const local = list; local.push(item)`) — same non-goal the
  existing rules already carry; needs real dataflow analysis this tool
  doesn't have.
- **Distinguishing Immutable.js/redux-form receivers from plain
  arrays/objects by type** — the core precision limitation this whole
  document is about; addressed by scope-narrowing (array methods only) and
  workflow (mandatory manual triage before suppression), not by pretending
  the tool can tell the difference syntactically.
- **A `param-mutating-map-set-method-call` companion rule** — not proposed
  at all, not even as an off-by-default opt-in, given the near-total
  false-positive rate found. If a genuinely mutation-heavy native-Map/Set
  codebase ever needs this, it should be scoped fresh against that
  codebase's own corpus, not inherited from this document's conclusion for
  a codebase where Immutable.js dominates.

## Test cases

| # | Shape | Example | Should fire? |
| --- | --- | --- | --- |
| 1 | Plain param, direct `.push` | `({ productsData }, product) => { productsData.push(product); }` (the original instance) | yes |
| 2 | Plain param, function declaration | `function f(accum, item) { accum.push(item); return accum; }` | yes |
| 3 | Destructured param, direct receiver | `({ favorites }, item) => { favorites.push(item); }` | yes |
| 4 | Chained receiver, 1+ levels deep | `function f(accum, key, item) { accum[key].items.push(item); }` | yes |
| 5 | `.sort()` in place | `function f(list) { list.sort(); }` | yes |
| 6 | `.splice()` | `function f(list, i) { list.splice(i, 1); }` | yes |
| 7 | Non-mutating method, must never fire | `function f(list) { return list.map(x => x * 2); }` | no |
| 8 | Method reference, not a call | `function f(list) { return list.push; }` | no |
| 9 | Local copy before mutating | `function f(list, item) { const copy = [...list]; copy.push(item); return copy; }` | no |
| 10 | Aliased parameter (non-goal) | `function f(list, item) { const local = list; local.push(item); }` | no (documented non-goal) |
| 11 | Immutable.js `.push` (low-confidence marker, not suppressed) | `pricelists.push(Map({ pricelistId: '' }))` where `pricelists` is a parameter and the file imports `immutable` | yes, but flagged low-confidence in output |
| 12 | Immutable.js `.set` (excluded rule scope entirely) | `product.set('name', name)` | no (`set` not in v1's method list at all) |
| 13 | Redux-form `fields.push()` (low-confidence marker) | `fields.push()` inside a `FieldArray` render prop, parameter named `fields` | yes, but flagged low-confidence in output |
| 14 | Optional chaining receiver | `function f(list, item) { list?.push(item); }` | yes |
| 15 | Bracket-form call (non-goal) | `function f(list, item) { list['push'](item); }` | no (documented non-goal) |
| 16 | Suppressed | `// custom-biome-ignore-next-line param-mutating-array-method-call` above `productsData.push(product);` | no (suppressed) |

## TODO / implementation checklist

Written so a fresh session with no other context can implement this
end-to-end.

1. **Confirm the shared helper module exists.** Check whether
   `src/rules/param_mutation.rs` (proposed in
   `DESTRUCTURED_PARAM_MUTATION_RULES_PLAN.md` step 2) has landed with the
   chain-root walker and destructured-vs-plain classifier. If it has, reuse
   both directly. If it hasn't (i.e. this rule is being built before that
   plan finished), build the minimal subset needed here rather than
   duplicating a second copy — flag this dependency to whoever picks up
   this doc.
2. **Rule implementation.** Create
   `src/rules/param_mutating_array_method_call.rs`. Detection per the
   sketch above: `JsCallExpression` nodes whose callee is a non-computed
   `JsStaticMemberExpression` with a property name in the fixed
   mutating-array-method set, whose member chain's root resolves via
   `file.semantic()` to a `Parameter` binding.
3. **Low-confidence signal.** Add an `immutable`-import check (does the
   file import from `'immutable'` or a path ending in `/immutable`) and a
   `fields`/`field` receiver-name check to the violation's metadata (not its
   gating) — surfaced via `--format json` and, if the existing text
   formatter has a place for it, a short annotation like `(low confidence:
   file imports 'immutable')`. Check `src/diagnostics.rs`'s `Violation`
   struct for the right place to add this without disturbing the four
   existing rules' output shape.
4. **`default_severity()` returns `RuleSeverity::Off`** — opt-in only, per
   the enablement recommendation above.
5. **Fixtures.** Create
   `fixtures/param_mutating_array_method_call/{valid,invalid,suppressed,edge-cases}.js`
   covering all 16 rows of the Test Cases table above, plus the edge cases
   listed in "Edge cases to pin down with fixtures" (optional chaining,
   shadowing, forEach-callback closures).
6. **Tests.** Add a `param_mutating_array_method_call` module to
   `tests/integration.rs` following the `no_console_log` shape referenced
   in `ADDING_A_RULE.md`: a flagged case with asserted line/col, the
   non-mutating-method near-miss (row 7) that must not fire, a suppression
   case, and a case asserting the rule produces zero violations with no
   config (off by default) and its normal violations once configured to
   `"error"`.
7. **Register the rule** in `src/rules/mod.rs` and
   `RuleRegistry::with_all_rules()` in `src/rules/registry.rs`. Run `cargo
   test && cargo clippy --all-targets` before proceeding.
8. **Update `docs/RULES.md`** with a new section for this rule, following
   the existing "What it catches" / "Before / after" / rule-specific
   subsection structure used for the other seven rules — including an
   explicit callout of the Immutable.js/redux-form false-positive
   limitation and the low-confidence signal, so anyone enabling this rule
   reads the caveat before turning it on.
9. **Integration sanity check against the real corpus.** Re-run this rule
   (once implemented) against the dashboard repo with the config enabled,
   confirm the raw finding count is in the neighborhood of 251 (the
   array-method-only raw count measured in this scoping pass — some drift
   is expected from edge cases fixtures uncover that this pass's babel-based
   scan didn't model, e.g. optional chaining or shadowing). This is
   read-only verification against that repo — do not commit anything there
   as part of this check.
10. **Do not build an automated `--write-fix` rollout workflow for this
    rule** — per the enablement recommendation, findings need manual triage
    before any suppression is added, unlike Rules 1–4's workflow.
