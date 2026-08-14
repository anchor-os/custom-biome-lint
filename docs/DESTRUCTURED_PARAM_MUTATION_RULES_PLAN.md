# Plan: four rules closing the `no-param-reassign` → `noParameterAssign` gap

**Status: IMPLEMENTED** (v0.3.0). All four rules, the `default_severity()`
plumbing, fixtures, tests and docs are in the tree. This document is kept as the
design record; see [RULES.md](RULES.md) for the shipped behaviour, which is what
to trust where the two disagree.

Deviations from the plan as written, all deliberate:

| Plan said | Shipped as | Why |
| --- | --- | --- |
| Rule names `destructureDefaultParamAssign`, `destructureParamPropAssign` | `destructure-default-param-assign`, `destructure-param-prop-assign` | The `Rule` trait requires kebab-case names (`ADDING_A_RULE.md`); the plan's camelCase spellings were inconsistent with its own rules 3 & 4. |
| `RuleRegistry::enabled` filters on `config.severity_override(name).unwrap_or_else(\|\| rule.default_severity())` | `config.severity(name, rule.default_severity())`, a new method | `severity_override` returns `None` for *both* "no entry" and `"off"`, so the plan's expression would have resurrected an explicitly-disabled rule at its default severity. |
| Rule 3's coverage of bare *reassignment* left "to decide at implementation time" | Not covered | Decided by repro, as the plan asked: Biome 2.5.8 **does** flag `d => { d = 5 }`. Only property mutation is missed, so only property mutation is in scope. |
| Rule 3 detection by visiting `JsArrowFunctionExpression` nodes and walking their bodies | Uniform assignment-target walk, filtered on the parameter binding's declared shape | Same result with no body-walking special case, and nested-arrow attribution falls out of semantic resolution rather than needing its own handling. |
| Expected ~13 findings from rules 1/2 and ~101 from rules 3/4 on the dashboard | 12 and 218 | Verified in full; see [RULES.md](RULES.md#the-four-parameter-mutation-rules). The estimates counted existing `eslint-disable` comments bucketed by cause; the rules report every occurrence, and 96% of findings still land on a previously-suppressed line. |

The original proposal follows unchanged.

Covers all 186 of the dashboard's uncovered `no-param-reassign` entries, in two
pairs:

| # | Rule | Closes | Default |
| --- | --- | --- | --- |
| 1 | `destructureDefaultParamAssign` | destructured-param reassignment (13 entries) | always on |
| 2 | `destructureParamPropAssign` | destructured-param property mutation (13 entries, shared with #1) | always on |
| 3 | `bare-arrow-param-prop-assign` | plain-param property mutation on an unparenthesized single-arrow parameter (~69 of 173) | **off**, opt-in |
| 4 | `deep-param-prop-assign` | plain-param property mutation 2+ levels deep (~32 of 173, overlaps with #3 on some lines) | **off**, opt-in |

Rules 1 & 2 were the original scope of this plan (see "Rule 1"/"Rule 2" below,
unchanged). Rules 3 & 4 were added in a follow-up round after the maintainers
decided to also close the two buckets originally listed under "Non-goals."

**Origin:** the dashboard's `no-param-reassign` → Biome migration coverage audit
found 186 `eslint-disable(-next-line/-line) no-param-reassign` comments with no
matching `biome-ignore`. Biome's own `lint/style/noParameterAssign` (already
enabled with `propertyAssignment: "deny"`) turns out to only track **plain
identifier parameters** — confirmed with an isolated repro:

```js
function plainParam(a) { a = 5; }              // Biome flags this
function plainPropAssign(d) { d.token = 'x'; } // Biome flags this

function destructuredDefault({ b = '' }) { b = 'x'; }    // Biome does NOT flag this
function destructuredPropAssign({ c }) { c.token = 'x'; } // Biome does NOT flag this
```

Of the 186, 13 are destructured-param cases — a categorical gap, not a config
issue, since `noParameterAssign` has no option that extends its reach to
destructuring. These two rules close exactly that gap. (The other 173 entries
are plain params invisible to Biome for unrelated reasons — bare single-arrow-param
formatting interaction and multi-level chained-mutation blind spots — out of
scope here; see the dashboard's `no-param-reassign-classification.md` for that
half of the investigation.)

Two rules rather than one because they're independently useful and independently
suppressible: a team may want to allow default-value normalization
(`{ limit = 10 }` then `limit = clamp(limit)`) while still banning payload
mutation, or vice versa. Splitting them keeps `// custom-biome-ignore-line`
comments precise about which behavior is being accepted.

## Rationale

The dashboard is Redux + Immutable.js, with sagas and reducers built almost
entirely around destructured `action`/`payload`/`state` parameters:

```js
export function* getSMSSessionsOfCustomersSaga({ payload }) {
  payload.token = yield call(getIdToken); // mutates the caller's action object
  ...
}
```

Mutating a destructured parameter is the same hazard `noParameterAssign`
already exists to catch for plain parameters: callers don't expect their
argument to change out from under them, and in a codebase where action objects
and Immutable-wrapped state get passed through saga chains, an in-place
mutation on one destructured field can silently corrupt state a sibling saga
or reducer reads later. Biome's rule stops at the parameter's own top-level
binding shape; it never asks "did this identifier come from a destructuring
pattern in the parameter list," so every one of these mutations is currently
invisible under both ESLint (removed) and Biome (doesn't reach here). Real
examples from the dashboard's 13 confirmed cases (see
`no-param-reassign-classification.md`):

```js
// src/sagas/getSMSSessionsOfCustomers.js — destructureParamPropAssign territory
export function* getSMSSessionsOfCustomersSaga({ payload }) {
  payload.token = yield call(getIdToken);
  ...
}

// src/lib/objects.js — destructureParamPropAssign territory
export const renameObjKey = ({ obj, oldName, newName }) => {
  obj[newName] = obj[oldName];
  delete obj[oldName];
  ...
};

// src/util/barcodeSuggestionsGenerator.js — destructureDefaultParamAssign territory
const generateBarcodeSuggestions = ({ barcodeFormat = 'EAN_8', prefix = '', showHelnyCodeOption = false }) => {
  if (prefix.length < 8) { ... } else {
    prefix = ''; // reassigning the destructured (default-valued) binding itself
  }
  ...
};
```

## Rule 1: `destructureDefaultParamAssign`

**Message:** `Reassigning destructured parameter "<name>" mutates a local
binding a caller can't see change. Use a new local variable instead.`

**What it catches:** reassignment of a binding introduced by object or array
destructuring in a function or arrow function's parameter list — the
destructuring equivalent of `noParameterAssign`'s plain-identifier check, but
for the binding itself (`x = ...`, `x += ...`, `x++`, `for (x of ...)`), not a
property of it (that's Rule 2).

### Should flag

```js
function destructuredDefault({ b = '' }) {
  b = 'x';
}

const f = ({ x }) => {
  x = 1;
};

function arrayDestructure([first]) {
  first = first.trim();
}

function nested({ outer: { inner } }) {
  inner = 'x';
}
```

### Should NOT flag

```js
// Plain parameter — Biome's noParameterAssign already covers this
function plainParam(a) {
  a = 5;
}

// Not a reassignment — just reading and using the value
function ok({ b = '' }) {
  return b.trim();
}

// A new local variable, not the destructured binding
function ok2({ b = '' }) {
  const local = b;
  local = 'x'; // eslint/biome would separately flag pointless reassignment elsewhere, not this rule
}
```

### Detection sketch

- Visit `JsAssignmentExpression` (covers `=`, `+=`, `-=`, etc. — same set
  `noParameterAssign` itself walks), `JsPreUpdateExpression` /
  `JsPostUpdateExpression` (`++`/`--`), and `JsForOfStatement` /
  `JsForInStatement` where the left-hand side is a bare identifier assignment
  target (`AnyJsExpression::JsIdentifierExpression` on the assignment side, not
  a member expression — member-expression targets are Rule 2's job).
- For each such identifier, resolve it via `file.semantic()` (the same
  resolve-by-binding approach `no_native_map.rs` uses for `Map`, rather than
  name-matching) to its declaring `Binding`.
- Reject early unless `binding.kind == BindingKind::Parameter` — reassigning an
  ordinary local `let` is not this rule's concern.
- The semantic model's `Binding` only records `declared_at` (a byte offset),
  not "was this destructured." Determine that by re-finding the syntax node at
  `declared_at` and walking its ancestors: if an ancestor between the binding's
  own node and the nearest enclosing `JsParameters`/single-arrow-parameter is
  an `AnyJsBindingPattern::JsObjectBindingPattern` or
  `JsArrayBindingPattern`, the parameter is destructured — flag. If the
  binding's immediate parameter node is a plain `AnyJsBinding` with no such
  ancestor, it's a plain parameter — leave it to `noParameterAssign` and don't
  flag (this is the exact boundary that keeps the two rules from double-reporting).
- Report at the assignment/update/for-loop-head node's `text_trimmed_range()`
  start, same convention as the existing three rules.

### Suppression

Standard, no per-rule wiring needed — the runner's generic
`custom-biome-ignore-line` / `custom-biome-ignore-next-line` scanner (and its
`{/* ... */}` JSX-children form) covers any rule automatically:

```js
function f({ b = '' }) {
  // custom-biome-ignore-next-line destructureDefaultParamAssign
  b = 'x';
}
```

### Edge cases to pin down with fixtures

- **Default value present or not** — `{ b }` vs `{ b = '' }` reassigned the
  same way; both must flag identically (the default only affects the initial
  value, not whether the binding is a destructured parameter).
- **Nested destructuring** — `{ outer: { inner } }`, `{ a: [first] }`. The
  ancestor walk must not stop at the first `JsObjectBindingPattern` it's
  *inside*; any destructuring ancestor between the binding and the parameter
  list qualifies.
- **Rest siblings** — `({ a, ...rest })`, reassigning `rest`. `rest` is itself
  a destructured (rest) binding and should flag; reassigning `a` should flag
  too — the presence of a rest sibling doesn't change either one's status.
- **Array vs. object destructuring** — both `JsObjectBindingPattern` and
  `JsArrayBindingPattern` need the same ancestor-walk treatment; don't special-case
  one and miss the other, mirroring how `no_native_map.rs` already handles
  `JsObjectBindingPattern` for import bindings (a precedent for the pattern-walking code, not the same use case).
- **Arrow function, no parens, single param** — N/A for this rule structurally:
  a bare `x => ...` single param can never itself be a destructuring pattern
  (`{ x } => ...` without parens is a syntax error — JS requires parens around
  a destructured parameter). No special case needed, but worth a fixture
  comment noting *why* there's no bare-arrow variant to test, so a future
  reader doesn't wonder if one was missed.
- **TypeScript parameter properties** (`constructor(public x)`) — not
  applicable; this is a `.js`/`.jsx`-only codebase and rule (`JS_EXTENSIONS`,
  same as all three existing rules).
- **Shadowing** — a destructured parameter named the same as an outer-scope
  variable must resolve to the *parameter* binding via the semantic model, not
  the outer one, the same correctness property `no_native_map.rs` established
  for `Map` shadowing.
- **Compound assignment and update operators** — `b += 1`, `b++`, `for (b of list)`
  where `b` is destructured all count as reassignment, same as
  `noParameterAssign` treats them for plain parameters.

## Rule 2: `destructureParamPropAssign`

**Message:** `Mutating a property of destructured parameter "<name>" changes
data the caller still holds a reference to. Copy it first.`

**What it catches:** mutating a property (dot or bracket, any depth) of a
binding introduced by destructuring in the parameter list — the destructuring
equivalent of `noParameterAssign`'s `propertyAssignment: "deny"` check.

### Should flag

```js
function destructuredPropAssign({ c }) {
  c.token = 'x';
}

const f = ({ payload }) => {
  payload.token = val;
};

function bracketForm({ acc }) {
  acc[key] = value;
}

function chained({ state }) {
  state.tours[id].priceBands = {};
}
```

### Should NOT flag

```js
// Plain parameter — Biome's noParameterAssign (propertyAssignment: "deny") already covers this
function plainPropAssign(d) {
  d.token = 'x';
}

// Reading a property, not assigning to it
function ok({ c }) {
  return c.token;
}

// Mutating a property of something reached THROUGH the destructured binding
// but not derived from parameter data at all (e.g. a module-level singleton) —
// still flagged, deliberately: the rule can't (and shouldn't try to) distinguish
// "the object c happens to still be a shared reference to caller data" from
// "c was reassigned to something local first." See "Known non-goal" below.
```

### Detection sketch

- Visit `JsAssignmentExpression` (and update/for-of/for-in forms, same set as
  Rule 1) where the assignment target is a member expression
  (`AnyJsExpression::JsStaticMemberExpression` for `.prop`, or
  `JsComputedMemberExpression` for `[prop]`), of any nesting depth.
- Unlike Rule 1, don't require the *immediate* object to be the identifier —
  walk down the leftmost chain of the member expression
  (`member.object()`, repeating while the object is itself a member
  expression) until reaching a bare identifier, the same way one would find
  the root of `state.tours[id].priceBands`. This is what makes the rule depth-
  independent, unlike Biome's own one-level-only implementation.
- Resolve that root identifier via `file.semantic()`, same as Rule 1: must be
  `BindingKind::Parameter`, and the same destructuring-ancestor walk must find
  an `JsObjectBindingPattern`/`JsArrayBindingPattern` between its declaration
  and the parameter list.
- Report at the outermost member expression's `text_trimmed_range()` start
  (the leftmost identifier, e.g. `state` in `state.tours[id].priceBands = {}`)
  so the diagnostic points at the parameter name, matching how
  `noParameterAssign` itself anchors its "Assigning to a property of a
  function parameter" message on the base identifier rather than the deepest
  property.

### Distinguishing Rule 1 from Rule 2 cleanly

Both rules visit the same assignment-expression node types; the split is
purely on the assignment target's shape:

| Target shape | Rule |
| --- | --- |
| `AnyJsExpression::JsIdentifierExpression` (bare identifier) | Rule 1 (`destructureDefaultParamAssign`) |
| `AnyJsExpression::JsStaticMemberExpression` / `JsComputedMemberExpression` whose root resolves to a destructured parameter | Rule 2 (`destructureParamPropAssign`) |

A single shared helper — say `fn destructured_param_binding(root_ident, file) ->
Option<String>` (returns the parameter name if it resolves to one) — belongs in
a small shared module (or `rules/destructure_param.rs` with both rules in one
file, mirroring how `reselect.rs` holds shared logic for
`no_arrow_function_create_selector` and `reselect_arity_match`) so the
ancestor-walk and semantic resolution logic isn't duplicated between the two
rule files.

### Suppression

Same generic mechanism as Rule 1 and all existing rules:

```js
const f = ({ payload }) => {
  // custom-biome-ignore-next-line destructureParamPropAssign
  payload.token = val;
};
```

### Edge cases to pin down with fixtures

- **Depth independence is the headline feature over Biome's own rule** —
  fixtures must include a 1-level (`c.token = 'x'`), 2-level
  (`acc[k].total = 'x'`), and 3+-level (`state.tours[id].priceBands = {}`)
  case, all flagged identically. This is explicitly the dimension Biome's
  built-in rule fails at (confirmed: Biome catches exactly one level, misses
  two or more) — the fixture set should make that contrast obvious to a
  reviewer comparing the two.
- **Mixed dot/bracket chains** — `state.tours[id].priceBands`,
  `state['tours'][id].priceBands` — both notations, same root-walk logic.
- **Method calls are NOT assignment** — `payload.items.push(x)` is a mutation
  in effect but has no `JsAssignmentExpression` node at all; explicitly out of
  scope for both rules (matches `noParameterAssign`'s own scope — it doesn't
  catch `d.items.push(x)` either. Confirmed by re-reading Biome's rule
  description: "Disallow reassigning function parameters" / property
  *assignment*, not arbitrary mutating method calls). Document this as a
  known non-goal, not a bug, exactly the way `RULES.md` documents
  `no-native-map`'s `mapboxgl.Map` false positive as an intentional,
  documented boundary rather than something to silently patch later.
- **Known non-goal: aliasing.** If a destructured binding is copied to another
  variable first (`const local = payload; local.token = 'x'`), this rule does
  NOT flag it — `local`'s own binding is a plain `const`, not a parameter, so
  it resolves to `BindingKind::Var`/`Const` and is correctly out of scope. This
  is the same "gap not a false positive" posture the existing rules take for
  cases outside their exact resolution model (e.g.
  `no-arrow-function-create-selector`'s member-expression callee gap) — worth
  stating explicitly so it isn't reported as a missed case later. A rule that
  tried to track aliasing would need real dataflow analysis, which is out of
  scope for a tree-walking `check()`.
- **Rest siblings** — `({ a, ...rest })`, mutating `rest.x = 1` — `rest` is a
  destructured (rest) binding, should flag like any other.
- **Optional chaining on the write side** — `payload?.token = 'x'` is not
  valid JS (optional chaining isn't assignable), so no case needed; note it in
  the fixture file only if the parser's error-tolerance makes it worth an
  explicit "not applicable" comment.

## Rule 3: `bare-arrow-param-prop-assign`

**Message:** `Mutating a property of parameter "<name>" is invisible to Biome's
noParameterAssign because this arrow's single parameter has no parens.
Add parens or copy the value first.`

**What it catches:** property/index mutation (dot or bracket, any depth — see
Rule 4 for the depth angle specifically) of a **plain** (non-destructured)
parameter, specifically when that parameter is the sole, unparenthesized
parameter of an arrow function. This is a plain-param case in principle — the
same shape `noParameterAssign` is supposed to cover — but confirmed by direct
repro to be invisible to Biome's rule for this exact AST shape:

```js
export const arrowPlainPropDot = d => {
  d.token = 'x';   // NOT flagged by Biome's noParameterAssign
};

export const parenSingleParamDot = (d) => {
  d.token = 'x';   // IS flagged — identical mutation, parens present
};
```

Confirmed independent of this repo's `arrowParentheses: "asNeeded"` formatter
setting — that setting is just what makes the bare form common here (Biome's
formatter actively strips the parens this rule's detection needs Biome's own
checker to see), not the cause of Biome's miss. The miss is in how
`noParameterAssign` resolves the parameter node when there is no enclosing
`JsParameters` node — an arrow with a single unparenthesized parameter binds
directly as an `AnyJsBinding` under the arrow, not inside a `JsParameters`
list the way `(d) => ...`, `(a, b) => ...`, and `function f(d) {}` all are.

### Should flag

```js
item => { item.x = 1; };
booking => { booking.laneIndex = i; };
event => { event.maxLanes = laneCount; };
```

### Should NOT flag

```js
// Parenthesized single param — Biome's noParameterAssign already covers this
(d) => { d.token = 'x'; };

// Multi-param arrow — always parenthesized by JS grammar, already covered
(accum, item) => { accum.token = item; };

// Named function declaration/expression — already covered
function f(d) { d.token = 'x'; }

// Destructured single param — Rule 2's territory, not this rule's
({ c }) => { c.token = 'x'; };

// No mutation, just a read
item => item.x;
```

### Detection sketch

- Visit `JsArrowFunctionExpression` nodes whose `parameters()` is the
  bare-binding form (Biome's grammar exposes an arrow's parameter list as
  `AnyJsArrowFunctionParameters`, with two variants: `JsParameters` — the
  parenthesized list, used for zero, two-or-more, or explicitly-parenthesized
  single params — and a bare `AnyJsBinding` for the unparenthesized
  single-param form). This rule only cares about the second variant.
- For that arrow, resolve its parameter's binding, and walk the arrow's body
  for `JsAssignmentExpression`/update-expression targets whose member-chain
  root (same leftmost-identifier walk as Rule 2) resolves via
  `file.semantic()` to that exact parameter binding.
- No destructuring check needed here — by construction, a bare
  single-arrow-param binding cannot be a destructuring pattern (same JS-grammar
  fact noted in Rule 1's edge cases), so this rule and Rules 1/2 never overlap
  in scope by construction, not just by convention.
- Report at the mutation's outermost member-expression start, same convention
  as Rule 2.

### Suppression

Same generic mechanism, no per-rule wiring:

```js
item => {
  // custom-biome-ignore-next-line bare-arrow-param-prop-assign
  item.x = 1;
};
```

### Edge cases to pin down with fixtures

- **Nested arrows** — `arr.map(item => other.forEach(x => { item.y = 1; }))`:
  the mutation lives inside a *different* arrow's body than the one whose
  parameter is being mutated. The semantic-model resolution (not lexical
  nesting alone) is what makes this correct — `item` must resolve to the
  outer arrow's parameter regardless of how many arrows separate the mutation
  from the declaration.
- **Reassignment, not just property mutation** — `item => { item = x; }` (the
  bare identifier itself, no property access) is arguably *also* invisible to
  Biome for the same structural reason property mutation is. Decide at
  implementation time whether Rule 3 should cover bare reassignment too, or
  whether that's already caught by Biome (worth a direct repro check before
  writing the fixture, the same way the property-mutation gap was confirmed —
  don't assume symmetry between reassignment and property-mutation detection
  without checking).
- **This rule + Rule 4 on the same line** — see "Rule 3 / Rule 4 overlap"
  below.

## Rule 4: `deep-param-prop-assign`

**Message:** `Mutating parameter "<name>" 2+ levels deep ("<chain>") is
invisible to Biome's noParameterAssign, which only tracks one level. Copy the
value first.`

**What it catches:** property/index mutation of a **plain** (non-destructured)
parameter, at member/subscript-chain depth 2 or more, regardless of
arrow-parens style. Confirmed by direct repro:

```js
function f(acc) { acc[x] = 1; }         // IS flagged by Biome — depth 1
function f(acc) { acc[x][y] = 1; }      // NOT flagged — depth 2
function f(accum) { accum.tours[id].priceBands = {}; } // NOT flagged — depth 3
```

This is the plain-parameter counterpart to Rule 2's depth-independence — same
detection technique, different eligibility check (plain parameter, not
destructured).

### Should flag

```js
function f(acc) {
  acc[instance.id][documentName] = moment().unix();
}

function f(accum) {
  accum.tours[bookingTypeId].priceBands = priceBands;
}

const f = (acc, item) => {
  acc[item.id].total += item.total;
};
```

### Should NOT flag

```js
// Depth 1 — Biome's noParameterAssign already covers this
function f(acc) { acc[x] = 1; }
function f(d) { d.token = 'x'; }

// Destructured root — Rule 2's territory
function f({ acc }) { acc[x][y] = 1; }
```

### Detection sketch

- Visit `JsAssignmentExpression`/update-expression targets that are member
  expressions, same node types as Rule 2.
- Walk the leftmost chain to the root identifier (shared helper with Rule 2 —
  see "Shared implementation notes" below), resolve via `file.semantic()`.
- Eligibility for *this* rule: `BindingKind::Parameter`, and **not**
  destructured (the inverse of Rule 1/2's ancestor check — a plain
  `AnyJsBinding` parameter, whether from a `JsParameters` list or a bare
  single-arrow-param).
- Count chain depth (number of `.prop`/`[expr]` hops from the root to the
  assignment target); flag only when depth ≥ 2. Depth-1 plain-param mutation
  is deliberately left alone — that's exactly what `noParameterAssign` already
  covers, and re-flagging it here would be duplicate noise Biome already
  reports under a different rule name.
- Report at the outermost member expression's start, same as Rule 2.

### Suppression

```js
function f(accum) {
  // custom-biome-ignore-next-line deep-param-prop-assign
  accum.tours[bookingTypeId].priceBands = priceBands;
}
```

### Edge cases to pin down with fixtures

- **Exact depth boundary** — a depth-1 fixture case that must NOT flag
  (proving no overlap with `noParameterAssign`'s own territory) alongside
  depth-2 and depth-3+ cases that must.
- **Mixed dot/bracket chains**, same as Rule 2's equivalent case.
- **Arrow-parens style is irrelevant here** — include both a parenthesized
  and bare-single-param depth-2+ case, both flagged identically, to make clear
  this rule doesn't care about arrow-parens (that's Rule 3's axis, not this
  one).

### Rule 3 / Rule 4 overlap — decision

A bare-single-arrow-param with a depth-2+ mutation trips **both** rules:

```js
item => {
  item.a.b = 1; // bare param (Rule 3) AND depth-2 chain (Rule 4)
};
```

**Decision: let both fire independently. Rule 4 does not special-case away
from what Rule 3 already reports.** Rationale:

- They test genuinely different structural conditions (arrow-parens shape vs.
  chain depth), each independently true or false regardless of the other. A
  line can trip either one alone or both together, and each combination is a
  real, distinct fact about the code.
- Suppressing them independently is useful, not redundant: a reviewer might
  parenthesize the arrow (satisfying Rule 3 — Biome's own rule now sees the
  mutation) while still wanting Rule 4 to keep watching the chain depth, or
  vice versa in a hypothetical where depth-1 chains are the norm but bare
  arrows are pervasive. Collapsing the two into one report would force an
  all-or-nothing suppression the two rules' own justifications
  (`-- reason text`) shouldn't need to share.
- Coupling Rule 4 to Rule 3's detection logic (e.g. "skip if Rule 3 would also
  fire here") makes Rule 4 fragile to Rule 3's implementation changing later,
  for a benefit (avoiding two comments on one line) that the suppression
  syntax already handles cleanly — both rule names can go on one marker:

  ```js
  item => {
    // custom-biome-ignore-next-line bare-arrow-param-prop-assign, deep-param-prop-assign
    item.a.b = 1;
  };
  ```
- `ADDING_A_RULE.md`'s own guidance reinforces this: "Do not check for
  suppression comments... just report everything you find" — the existing
  rules are written to be simple, independent detectors with no
  cross-rule awareness, and there's no precedent in the current three rules
  for one rule suppressing itself based on what another rule would also
  report.

## Config option for Rules 3 & 4: default `"allow"`, opt-in `"deny"`

Rules 1 & 2 are always-on (no option), matching the existing three rules,
which take no per-rule options today. Rules 3 & 4 are different: the
maintainers decided these two should ship **off by default**, enabled only
when a consuming repo explicitly opts in — mirroring the semantics of Biome's
own `noParameterAssign.propertyAssignment` option (`"allow"` default,
`"deny"` to enable property-assignment checking), which is exactly the option
this dashboard's own `biome.json` already flips to `"deny"`.

### What exists today

Checked `src/config/package_config.rs` and how it's consumed
(`src/rules/registry.rs`, `src/cli/mod.rs`): **no rule currently takes a
per-rule option.** The only configuration surface is
`ignoreBiomeExtensionRules`, which sets a whole rule's *severity* —
`"off"` / `"warn"` / `"error"` — via `PackageConfig::severities`. Critically,
a rule with **no entry** in that config defaults to **`"error"`** (see
`PackageConfig::severity_override`'s doc comment: "A rule with no entry keeps
its default severity"). There is no existing concept of a rule defaulting to
off absent configuration — every rule today is "on unless explicitly turned
off." This is new plumbing, and a prerequisite for Rules 3 & 4 as scoped.

### Recommended approach: reuse the severity mechanism, don't invent a parallel one

Rather than adding a second, Biome-shaped config namespace
(`{ "customBiomeLintOptions": { "bare-arrow-param-prop-assign": { "level": "deny" } } }`)
purely to mirror Biome's naming, extend the existing `Rule` trait with one new
method and thread it through the one place that decides which rules run:

```rust
pub trait Rule: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn supported_extensions(&self) -> &'static [&'static str];
    fn check(&self, file: &FileContext) -> Vec<Violation>;

    /// A rule's severity when the config has no entry for it. Defaults to
    /// `Error` (today's implicit behavior for every existing rule) so no
    /// existing rule needs to change. Override to `Off` for a rule that
    /// should require explicit opt-in.
    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Error
    }
}
```

`bare-arrow-param-prop-assign` and `deep-param-prop-assign` are the only two
overrides, each returning `RuleSeverity::Off`.

Then `RuleRegistry::enabled()` (`src/rules/registry.rs:32`), which today
filters with `!config.is_ignored(rule.name())`, needs to instead ask "what
severity would this rule run at" and skip only when that resolves to `Off`:

```rust
.filter(|rule| {
    config
        .severity_override(rule.name())
        .unwrap_or_else(|| rule.default_severity())
        != RuleSeverity::Off
})
```

`registry.ignored(&config)` (used for the `-v` "N rules ignored" listing) needs
the mirror-image change so a default-off rule shows up there when
unconfigured, not just when explicitly set to `"off"`.

**Why this over a new config namespace:** it reuses a mechanism that already
has parsing, warnings-on-bad-input, and tests
(`src/config/package_config.rs`'s existing test module) — one trait method
and two `filter` predicates, versus a second parallel config schema, a second
parser, and a second set of config tests, for a need ("this one rule defaults
differently") that a single severity default already expresses. The
consuming-repo-facing action ends up looking exactly like Biome's own
`"deny"` opt-in in spirit, just phrased in this tool's existing vocabulary:

```json
{
  "ignoreBiomeExtensionRules": {
    "bare-arrow-param-prop-assign": "error",
    "deep-param-prop-assign": "error"
  }
}
```

No entry (the default) → both rules never run, matching Biome's own
`propertyAssignment: "allow"` default. An entry of `"error"` or `"warn"` →
the rule runs at that severity, matching Biome's `"deny"`.

### Alternative considered, not recommended

A literal per-rule options object mirroring Biome's `{ rule: { options } }`
shape more closely by name (e.g. a `level: "allow" | "deny"` key instead of
reusing `"off" | "warn" | "error"`). This would read slightly more familiarly
to someone coming from `biome.json`, but doubles the config surface for a
single boolean distinction rules 3/4 need, and every future rule wanting a
non-default severity would face a choice of which namespace to use. Flagging
this as the fallback only if the maintainers want naming parity with Biome
badly enough to justify the second schema — the plan's default recommendation
is the trait-method approach above.

## Suppression support — confirmed for all four rules

Re-confirmed against `src/suppress/mod.rs`: the suppression scanner is
rule-name-agnostic — it parses `// custom-biome-ignore-line <rules>` and
`// custom-biome-ignore-next-line <rules>` (comma-separated rule list, or bare
for "suppress everything on this line"), plus the `{/* ... */}` JSX-children
form, entirely independently of which rules exist or are registered. **All
four new rules get this for free**, identically to the existing three — no
rule-specific suppression code is possible or needed in this tool's
architecture (`ADDING_A_RULE.md`: "Do not check for suppression comments. The
runner drops suppressed violations after `check` returns.").

## `--write-fix` / `--auto-fix` / `--dry-run` — per-rule decisions

Two independent mechanisms exist (`src/fixer.rs` vs. `src/autofix.rs`):

- **`--write-fix`** inserts a suppression comment near the violation. It
  operates on *any* `Violation` regardless of rule and regardless of whether
  the rule ever produces a `Fix` — it's the same mechanism whether or not the
  rule can autofix its own code. **Works identically for all four new rules,
  no rule-specific code needed** — confirmed by `fixer.rs`'s module doc
  ("Writes suppression comments back into source files," no mention of
  requiring a `Fix`).
- **`--auto-fix`** rewrites the flagged code itself, but only for violations
  whose rule attached a `Violation::fix: Some(Fix)` at detection time. A
  violation with no `Fix` is reported as **skipped**, not guessed at
  (`src/autofix.rs`: "A violation whose rule did not produce one is left
  unfixed and reported as such, rather than guessed at here"). Per-rule
  decision, following `ADDING_A_RULE.md`'s "only attach a Fix when the
  correction is unambiguous" standard:

| Rule | `Fix`? | Why |
| --- | --- | --- |
| 1. `destructureDefaultParamAssign` | **No** | The only real fix is renaming the destructured binding and every reference to it within the enclosing scope — a multi-site rewrite, not a single byte-range replacement `Fix` supports. Same reasoning as Biome's own `noParameterAssign`, which itself has no fix ("use a local variable instead" is a suggestion in the diagnostic text, not something Biome rewrites for you — confirmed via `biome explain noParameterAssign`). |
| 2. `destructureParamPropAssign` | **No** | The real fix is "copy the object before mutating it," but the correct copy shape (shallow object spread, deep clone, `Immutable.js`-aware update) can't be inferred from the mutation site alone — same "would have to guess" disqualifier `reselect-arity-match` and `no-native-map` already use. |
| 3. `bare-arrow-param-prop-assign` | **No** | A mechanically safe fix *exists* — wrap the bare parameter in parens (`item =>` → `(item) =>`) — but it doesn't resolve the violation, it just hands the same mutation to Biome's `noParameterAssign` to re-report under a different rule name on the next run. An "autofix" whose result is "still flagged, now by a different tool" is worse UX than reporting it as unfixable, so no `Fix` is attached. (If the maintainers want the parens-only mechanical fix anyway as a distinct convenience separate from resolving the mutation concern, that would need its own explicit design discussion — flagged here, not decided.) |
| 4. `deep-param-prop-assign` | **No** | Same reasoning as #2 — the real fix is copying before mutating, and the correct copy shape can't be inferred generically. |

- **`--dry-run`** composes with either flag exactly as already implemented in
  `src/cli/args.rs` (it's a generic "report what would change, write nothing"
  flag layered on top of whichever of the two mechanisms is active) — no
  per-rule handling exists today and none is needed for the new rules.

Net effect: all four rules support `--write-fix` and `--dry-run` fully; none
supports `--auto-fix` (all four are reported as skipped fixes, honestly,
exactly like `no-native-map` and `reselect-arity-match` today).

## Implementation checklist (step-by-step, in dependency order)

Written so a fresh session with no other context can implement all four rules
end-to-end by following this list in order.

1. **Prerequisite plumbing — config default-severity.** Add
   `default_severity(&self) -> RuleSeverity { RuleSeverity::Error }` to the
   `Rule` trait in `src/rules/rule.rs`. Update `RuleRegistry::enabled()` and
   `RuleRegistry::ignored()` in `src/rules/registry.rs` to consult
   `config.severity_override(rule.name()).unwrap_or_else(|| rule.default_severity())`
   instead of `config.is_ignored(rule.name())`. Add a test to
   `src/config/package_config.rs` (or a new registry test) proving: (a) a
   rule with `default_severity() == Error` and no config entry is enabled
   (existing behavior, must not regress — run the full existing test suite
   after this step before moving on), (b) a hypothetical rule with
   `default_severity() == Off` and no config entry is NOT enabled, (c) that
   same rule becomes enabled once `ignoreBiomeExtensionRules` sets it to
   `"warn"` or `"error"`.
2. **Shared helper module.** Create `src/rules/param_mutation.rs` (or fold
   into an existing shared module if one better fits) holding: (a) a
   leftmost-member-chain-root walker (identifier from any depth of
   `.prop`/`[expr]` chain — needed by Rules 2, 3, and 4), (b) a
   destructured-vs-plain classifier for a `Parameter` binding (ancestor walk
   for `JsObjectBindingPattern`/`JsArrayBindingPattern` between a binding's
   declaration and its enclosing parameter list — needed by Rules 1, 2, and
   4's "not destructured" check), (c) chain-depth counter (needed by Rule 4).
   Building this before the four rule files avoids four slightly-different
   copies of the same walk.
3. **Rule 1 — `destructureDefaultParamAssign`.** Create
   `src/rules/destructure_default_param_assign.rs`. Detection per the sketch
   above: assignment/update/for-of-in targets that are bare identifiers,
   resolved via `file.semantic()` to a `Parameter` binding, filtered to
   destructured-only via the Step 2 helper.
4. **Rule 1 fixtures.** Create
   `fixtures/destructure_default_param_assign/{valid,invalid,suppressed,edge-cases}.js`
   covering: the "Should flag" and "Should NOT flag" examples above, plus the
   "Edge cases to pin down with fixtures" list (defaults present/absent,
   nested destructuring, rest siblings, array destructuring, shadowing,
   compound/update operators).
5. **Rule 1 tests.** Add a `destructure_default_param_assign` module to
   `tests/integration.rs` following the `no_console_log` shape in
   `ADDING_A_RULE.md`: a flagged case with asserted line/col, a plain-param
   near-miss that must not fire, a suppression case.
6. **Rule 2 — `destructureParamPropAssign`.** Create
   `src/rules/destructure_param_prop_assign.rs`, reusing the Step 2 chain-root
   walker and destructured-classifier. Detection per the sketch above:
   member-expression assignment targets whose root resolves to a destructured
   `Parameter` binding, any depth.
7. **Rule 2 fixtures and tests.** Same structure as steps 4–5, covering the
   1-level/2-level/3+-level depth-independence cases explicitly (the headline
   feature over Biome's own rule), mixed dot/bracket chains, method-call
   non-goal, aliasing non-goal, rest siblings.
8. **Register Rules 1 & 2.** Add both modules to `src/rules/mod.rs` (declare +
   re-export) and both to `RuleRegistry::with_all_rules()` in
   `src/rules/registry.rs`. Run `cargo test && cargo clippy --all-targets`
   before proceeding — these two rules are always-on and should be fully
   green before layering the opt-in rules on top.
9. **Rule 3 — `bare-arrow-param-prop-assign`.** Create
   `src/rules/bare_arrow_param_prop_assign.rs`. Detection per the sketch
   above: `JsArrowFunctionExpression` nodes with a bare (non-`JsParameters`)
   single-binding parameter, member-expression mutation targets inside the
   body resolving to that parameter via `file.semantic()`. Override
   `default_severity()` to return `RuleSeverity::Off`.
10. **Rule 3 fixtures and tests.** Cover bare vs. parenthesized vs. multi-param
    arrow (only bare should flag), named function declarations (should not
    flag — different AST shape entirely, already covered by Biome), nested
    arrows with semantic resolution across nesting, and — since this rule
    defaults off — a test proving it produces zero violations with no config
    and produces its normal violations once configured to `"error"` via
    `ignoreBiomeExtensionRules` (exercises the Step 1 plumbing end-to-end for
    this specific rule, not just the generic registry test).
11. **Rule 4 — `deep-param-prop-assign`.** Create
    `src/rules/deep_param_prop_assign.rs`, reusing the Step 2 chain-root
    walker, depth counter, and destructured-classifier (eligibility here is
    "plain," i.e. classifier says NOT destructured). Detection per the sketch
    above: chain depth ≥ 2, any arrow-parens style. Override
    `default_severity()` to return `RuleSeverity::Off`.
12. **Rule 4 fixtures and tests.** Cover the exact depth-1/depth-2/depth-3+
    boundary, both parenthesized and bare-arrow forms (proving arrow-parens
    style is irrelevant to this rule), destructured-root near-miss (Rule 2's
    territory, must not double-fire here), and the same
    default-off-then-configured-on test shape as Rule 3.
13. **Rule 3 / Rule 4 overlap fixture.** Add a dedicated case (in either rule's
    `edge-cases.js`, or a small combined fixture if the existing convention
    doesn't fit a two-rule scenario well) with a bare-single-arrow-param and a
    depth-2+ mutation on the same line, asserting **both** rules report a
    violation at that line — proving the "let both fire independently"
    decision is actually implemented, not just documented.
14. **Register Rules 3 & 4.** Add both modules to `src/rules/mod.rs` and
    `RuleRegistry::with_all_rules()`. Run `cargo test && cargo clippy --all-targets`
    again — full suite, all four new rules plus the three existing ones.
15. **`RULES.md`.** Add four new `##` sections following the exact existing
    template (Message / Source / "What it catches" / "Before / after" / any
    rule-specific subsection), in the order Rules 1–4 are numbered here. Add
    four new rows to the summary table at the top (note the `Extensions`
    column is the same `.js`, `.jsx` for all four; add a note in Rules 3 & 4's
    rows that they're off by default, unlike the other five). Update the "Real
    -codebase findings" section with actual counts from a real run against
    the dashboard, not the ~13/~69/~32 estimates from the classification
    report — re-verify against the dashboard's current state at
    implementation time, since it's under active rebase and file:line
    specifics will have drifted.
16. **`ADDING_A_RULE.md`.** Add a short note under "Autofix (optional)" and/or
    a new subsection documenting the `default_severity()` trait addition from
    Step 1, so the next rule author knows the option exists and when to use
    it (a rule whose finding is noisy/opinionated enough to want opt-in rather
    than on-by-default) — this is new tool capability, not just new rules, and
    deserves the same documentation treatment `ADDING_A_RULE.md` gives every
    other trait method.
17. **Integration sanity check against the real dashboard.** Run
    `custom-biome-lint 'src/**/*.{js,jsx}' 'cypress/**/*.{js,jsx}'` against
    the dashboard repo with rules 3 & 4 enabled via
    `ignoreBiomeExtensionRules` set to `"error"` for both, and compare the
    finding count/positions against the 186-line source list in
    `no-param-reassign-classification.md` (re-run
    `scripts/eslint-disable-coverage-report.js` first, since the dashboard
    will have moved on from the exact snapshot that report was generated
    against). Expect roughly 13 findings from Rules 1/2 combined and up to
    ~101 from Rules 3/4 combined (69 + 32, with the overlap lines counted by
    both) — treat significant deviation from that shape as a signal to
    re-examine the detection logic against real code before treating the
    rules as ready, the same verification discipline
    `no-native-map`/`reselect-arity-match`'s "0 violations across N call
    sites" real-codebase sections in `RULES.md` already model.
18. **Final gate.** `cargo test && cargo clippy --all-targets && cargo build
    --release`, then `./target/release/custom-biome-lint fixtures` and
    confirm the new rules' fixture counts are included in the total (per
    `ADDING_A_RULE.md`'s Step 4 verification), before considering this plan
    implemented.

## Appendix: Rule 1 & 2 implementation notes

(Detail supporting the checklist above, specific to Rules 1 & 2 — kept from
the original single-pair version of this plan.)

- Both are `.js`/`.jsx`-only (`JS_EXTENSIONS`, no widening needed — same
  extension set as all three existing rules).
- Both need `file.semantic()`, following the precedent set by all three
  existing rules resolving identifiers by binding rather than by name (see
  `SEMANTIC_MODEL.md`) — this is a correctness requirement, not an
  optimization: without it, a plain local variable that happens to be named
  the same as an outer destructured parameter would be misclassified by name
  alone.
- `RULES.md` gets two new sections following the exact existing per-rule
  template (Message / Source / "What it catches" / "Before / after" / any
  rule-specific subsection / suppression note), plus a new row each in the
  summary table at the top and the "Real-codebase findings" section updated
  with the dashboard's actual count once implemented (expected: up to 13,
  pending re-verification at implementation time since the dashboard is under
  active rebase). Folded into Step 15 of the checklist, alongside Rules 3 & 4.

## Non-goals for this plan

- **Not** attempting to close Biome's own `noParameterAssign` depth-1
  limitation for plain parameters by *extending* `noParameterAssign` itself,
  or by modifying Biome — Rules 3 & 4 are new, separate tool rules, not a
  patch to Biome's rule. (Originally this whole bucket — the 173
  plain-parameter entries — was scoped OUT of this plan entirely; the
  maintainers have since decided to close it too, via Rules 3 & 4 above,
  rather than leaving it as a permanent gap.)
- **Not** implementing dataflow/alias tracking (see "Known non-goal:
  aliasing" under Rule 2 above) — scope is limited to what a single-pass tree
  walk with semantic-model identifier resolution can determine, consistent
  with how all three existing rules are built. This applies to all four new
  rules, not just Rules 1 & 2.
- **Not** deciding the "parens-only mechanical fix" question flagged in Rule
  3's `--auto-fix` row — noted as a possible future convenience, explicitly
  not decided here.
- **Not** implementing the "Alternative considered" Biome-shaped config
  namespace for Rules 3 & 4 — the recommended path reuses the existing
  severity mechanism; the alternative is documented only in case the
  maintainers override that recommendation.
