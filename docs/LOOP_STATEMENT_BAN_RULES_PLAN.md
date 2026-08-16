# Loop statement ban rules — plan

**Status: IMPLEMENTED** (rules `no-while-statement`, `no-do-while-statement`,
`no-for-statement`, all shipped off by default). The "8 rules exist today" framing
below describes the state the plan was written against; the three loop rules are
now the 9th–11th registered rules.

## How this plan fits the repo

This plan is a *design* document. The *mechanics* of adding a rule — the exact
trait signatures, the `mod.rs`/`registry.rs` edits, the fixture layout, the test
helpers, the gate commands — live in the single canonical guide
[`docs/ADDING_A_RULE.md`](ADDING_A_RULE.md). Read that alongside this plan; where
they overlap, the guide is authoritative.

Reality checks against the current codebase (2026-08-15), so this plan needs
fewer round-trips:

- **8 rules exist today**, all in `src/rules/` and all registered in one place,
  `RuleRegistry::with_all_rules`. Three ship **off by default**:
  `bare-arrow-param-prop-assign`, `deep-param-prop-assign`, and
  `param-mutating-array-method-call`. These three loop-ban rules are a fourth,
  fifth, and sixth off-by-default rule, using the exact same `default_severity`
  override documented in `ADDING_A_RULE.md`.
- **A rule detects only.** Parsing, file-extension filtering, suppression
  scanning, output, and exit codes all belong to the runner (see
  [`ARCHITECTURE.md`](ARCHITECTURE.md)). A rule implements `check` and returns
  `Violation`s; it never scans for its own suppression comments. The generic
  suppression scanner in `src/suppress/mod.rs` keys off each rule's `name()`, so
  no suppression-specific code is needed here.
- **No semantic model needed.** Like most existing rules, these three are pure
  cast-and-report walks over `file.tree().descendants()`. `file.semantic()` is
  only for identifier-resolution rules (see [`SEMANTIC_MODEL.md`](SEMANTIC_MODEL.md));
  leave it untouched here.
- **Four fixture files are mandatory** per rule — `valid.js`, `invalid.js`,
  `suppressed.js`, and `edge-cases.js`. The `every_rule_has_fixtures_for_all_four_cases`
  test fails the build if any registered rule is missing one (see
  [`TESTING.md`](TESTING.md)). This plan's first draft listed only three; that
  was wrong and is corrected below.
- **Tests use `check_source` / `check_one`** in `tests/integration.rs`, which
  run *only* the named rule so other rules can't pollute expectations.

## Rationale

The <PRIVATE_REPO> repo's old ESLint config banned 9 AST constructs under one rule,
`no-restricted-syntax` (the classic Airbnb "no loops, use functional iteration
instead" rule): `DoWhileStatement`, `ForStatement`, `ForInStatement`,
`ForOfStatement`, `SwitchCase`, `SwitchStatement`, `WhileStatement`,
`WithStatement`, `UnaryExpression[operator='delete']`.

Every other selector in that list has since been resolved against Biome
itself, each via a different real Biome rule — not a straight port of "ban
this construct," a mix of literal coverage and deliberate policy reversal:

| Old selector | Resolution | Kind |
|---|---|---|
| `ForInStatement` | `noForIn` | literal — Biome bans it too |
| `WithStatement` | `noWith` | literal — Biome bans it too |
| `UnaryExpression[operator='delete']` | `noDelete` | literal, with Biome's own narrower scope (computed non-literal keys are exempt by design) |
| `SwitchCase` / `SwitchStatement` | `noSwitchDeclarations` + `noUselessSwitchCase` + `noUselessElse` + `useDefaultSwitchClauseLast` | **policy reversal** — switch is no longer banned, it's required to be well-formed |
| `ForOfStatement` | `useForOf` | **policy reversal** — for-of is now *preferred* over indexed loops, not banned |

The remaining three — `DoWhileStatement`, `ForStatement`, `WhileStatement` —
have no Biome equivalent in any form, because Biome has no rule that inspects
or restricts loop constructs as a category the way it does switch statements.
This is confirmed via `biome explain` on every plausible name
(`noInfiniteLoop`, `noUselessWhile`, `noConstantCondition` — the closest real
rule, already enabled repo-wide, checked directly against every file with a
remaining loop suppression: 0 findings, because it catches literal constant
conditions like `if (false)`, not the mere existence of a loop).

As of the 2026-08-15 audit (`eslint-disable-coverage-audit.md`,
`no-restricted-syntax` section, broken down by construct type):

- `while` — **42** remaining
- `for` (plain indexed/counted loops) — **15** remaining
- `do-while` — **0** remaining (none exist in the codebase currently)
- `other/unclassified` — 7 (file-level banners and one ambiguous case, not
  necessarily loop-related — out of scope for this plan)

Given `do-while` currently has zero live instances, a rule for it is still
worth building (matching the original ban's completeness and guarding against
regressions), but its fixture set will necessarily be thinner.

## Design decision 1 — three separate rules, not one generic rule

`custom-biome-lint`'s config surface (`src/config/package_config.rs`) is a
flat map from rule name to `RuleSeverity` (`ignoreBiomeExtensionRules: {
"rule-name": "off" | "warn" | "error" }`) — there is no per-rule options
payload the way Biome's own `noParameterAssign` takes a
`{ propertyAssignment: "deny" }` object. Building a single generic rule that
mimics ESLint's own `no-restricted-syntax` (an arbitrary list of banned AST
selectors passed as an option) would require inventing new config plumbing
this tool doesn't have — the same conclusion the destructured-param-mutation
plan already reached and rejected for a much smaller ask (a boolean-ish
default-severity override, which *did* get built, see `Rule::default_severity`
in `src/rules/rule.rs`).

Three separate rules — `no-while-statement`, `no-do-while-statement`,
`no-for-statement` — need **no new config plumbing at all**. Each is toggled
independently via the existing `ignoreBiomeExtensionRules` mechanism, exactly
like `bare-arrow-param-prop-assign`, `deep-param-prop-assign`, and
`param-mutating-array-method-call` already are (those three ship off by default
today).
This also matches the tool's established one-rule-per-AST-shape convention
(one rule per `Js*Statement` kind, same as every rule shipped so far maps to
one specific node shape or mutation pattern). Recommendation: **three rules**.

## Design decision 2 — default severity

Recommend **`RuleSeverity::Off`** for all three, by default.

This is *not* for the same reason the other off-by-default rules ship off
(`bare-arrow-param-prop-assign`, `deep-param-prop-assign`,
`param-mutating-array-method-call` — false-positive risk from a name-based
heuristic with no type information). These three rules have **zero
ambiguity** — a `while` statement either exists syntactically in the tree or
it doesn't, there is no ambiguity or heuristic guessing involved, unlike
matching `.push()`/`.set()` calls by method name.

The reason to still default off is the criterion `Rule::default_severity`'s
own doc comment names independently of false-positive risk: *"a rule whose
findings are opinionated enough that a consuming repo should have to opt
in."* Banning `while`/`for`/`do-while` outright is a house style choice (the
Airbnb "no loops, use functional iteration" philosophy), not a universal
correctness or performance improvement the way `no-native-map` or
`reselect-arity-match` are. A consuming repo that has no opinion on loop
style has no use for these rules at all — same posture as
`bare-arrow-param-prop-assign`'s own doc: *"a repo [with a different
convention] has no use for it at all."* Ship off, let the <PRIVATE_REPO> repo (and
any other adopting repo with the same house style) opt in explicitly.

## Rule 1 — `no-while-statement`

### What it catches

Any `while (...) { ... }` statement, unconditionally — the AST node existing
is the violation, nothing about its condition, body, or context matters.

```js
while (queue.length > 0) {           // flagged
  process(queue.shift());
}
```

```js
queue.forEach(item => process(item)); // not flagged — no while statement
```

### Detection sketch

The simplest rule shape in the tool — no semantic model, no binding
resolution, no scope walk. Walk `file.tree().descendants()`, match
`JsWhileStatement::cast_ref(&node)`, report one violation per match at the
statement's own start position (the `while` keyword, not the condition or
body — mirrors how `no_native_map.rs` reports at the occurrence's own
position rather than an enclosing node's).

```rust
for node in file.tree().descendants() {
    let Some(stmt) = JsWhileStatement::cast_ref(&node) else { continue };
    let offset = stmt.syntax().text_trimmed_range().start();
    let (line, col) = file.line_col(offset);
    violations.push(Violation::error(self.name(), line, col, MESSAGE));
}
```

No `FileContext::semantic()` call needed at all, unlike every rule shipped so
far except possibly the simplest cast-and-report shape inside
`no_native_map.rs`'s own occurrence loop.

## Rule 2 — `no-do-while-statement`

### What it catches

Any `do { ... } while (...)` statement.

```js
do {                    // flagged
  attempt();
} while (!success);
```

### Detection sketch

Identical shape to Rule 1, matching `JsDoWhileStatement` instead:

```rust
for node in file.tree().descendants() {
    let Some(stmt) = JsDoWhileStatement::cast_ref(&node) else { continue };
    // ...same as above
}
```

### Note on current scope

Zero live instances exist in the <PRIVATE_REPO> repo today (confirmed via the
audit breakdown). Build this rule anyway for completeness and regression
protection — a fixture set with a synthetic invalid case is enough; there's
no real-codebase example to draw from the way other rules cite one.

## Rule 3 — `no-for-statement`

### What it catches

Any classic three-clause `for (init; test; update) { ... }` loop — **not**
`for...in` or `for...of`, which are distinct AST node kinds
(`JsForInStatement` / `JsForOfStatement`) already resolved by `noForIn` /
`useForOf` respectively and out of scope here.

```js
for (let i = 0; i < items.length; i++) {   // flagged
  process(items[i]);
}
```

```js
for (let index = 0; index < limits.length; index += gap) {  // flagged —
  chosenLimits.push(limits[index]);                          // still a
}                                                             // ForStatement,
                                                                // even with a
                                                                // non-unit step
```

```js
for (const item of items) { process(item); }  // NOT flagged — different node kind (JsForOfStatement)
for (const key in obj) { ... }                 // NOT flagged — different node kind (JsForInStatement)
```

### Detection sketch

Same shape again, matching `JsForStatement` (already confirmed to exist and
be in active use elsewhere in this codebase — `src/semantic/builder.rs`
already imports and matches on it for scope-handling purposes, so the cast is
proven to work against this tool's Biome version):

```rust
for node in file.tree().descendants() {
    let Some(stmt) = JsForStatement::cast_ref(&node) else { continue };
    // ...same as above
}
```

Because `JsForStatement`, `JsForInStatement`, and `JsForOfStatement` are
distinct generated node types (not one `JsForStatement` with a discriminant
field), there is no risk of this rule accidentally catching a for-in/for-of
loop — the cast simply fails to match on those node kinds. No manual
discrimination logic needed, unlike the audit script's own regex-based
classifier (`<PRIVATE_TOOLING>`), which has to guess
from source text since it doesn't have a real parser.

## Suppression

Free for all three, same as every other rule — the generic scanner in
`src/suppress/mod.rs` keys suppression comments off the rule's `name()`
string, not off anything rule-specific. `custom-biome-ignore-line
no-while-statement` / `custom-biome-ignore-next-line no-for-statement` /
etc. need no new suppression code.

## Autofix

**None of the three should implement `Fix`.** Deleting or rewriting a loop
requires understanding what the loop *does* and replacing it with equivalent
logic (a `.forEach`/`.map`/`.reduce` call, or a recursive restructure) — there
is no mechanical, unambiguous rewrite the way `no-arrow-function-create-selector`
unwraps a redundant arrow. This mirrors `deep-param-prop-assign` and
`bare-arrow-param-prop-assign`'s own "no unambiguous fix" reasoning, just more
absolute: those rules at least *could* imagine a fix (copy the object first);
these three cannot without a human deciding what the loop is for.

`--write-fix` (suppression-insertion) works for all three regardless, same as
every rule — that's just adding a comment, independent of whether a `Fix`
exists.

## Non-goals

- No attempt to detect *why* a loop exists or judge whether it's justified
  (e.g. a loop that legitimately can't be expressed as `.map`/`.reduce`
  because it needs early-exit via `break`, or a non-unit stride like
  `index += gap`) — that judgment belongs to whoever reviews the suppression,
  same posture as every existence-ban rule in this family.
- No attempt to also flag `for...in`/`for...of` here — those are out of scope,
  already resolved via `noForIn`/`useForOf`.
- No attempt to unify these three into one rule that reports three different
  messages — see Design decision 1.

## Test cases

| Rule | Case | Expected |
|---|---|---|
| `no-while-statement` | `while (x) { y(); }` | 1 violation |
| `no-while-statement` | `queue.forEach(y)` | 0 violations |
| `no-while-statement` | nested `while` inside a `for` body | 1 violation (only the `while`) |
| `no-do-while-statement` | `do { x(); } while (y);` | 1 violation |
| `no-do-while-statement` | `while (y) { x(); }` (plain while, not do-while) | 0 violations |
| `no-for-statement` | `for (let i = 0; i < n; i++) {}` | 1 violation |
| `no-for-statement` | `for (let i = 0; i < n; i += step) {}` (non-unit stride) | 1 violation — still a `ForStatement` |
| `no-for-statement` | `for (const x of xs) {}` | 0 violations (different node kind) |
| `no-for-statement` | `for (const k in obj) {}` | 0 violations (different node kind) |
| `no-while-statement` | `edge-cases.js`: nested `while`, `while` inside an arrow callback (belongs to the callback, not the enclosing function), label-`while` | each violation pinned by a named count; non-`while` statements must not fire |
| `no-do-while-statement` | `edge-cases.js`: `do-while` nested in a `for` body, `do-while` inside an arrow callback, a plain `while` near-miss | each violation pinned; the plain-`while` near-miss must not fire |
| `no-for-statement` | `edge-cases.js`: `for` inside an arrow callback, `for` inside an `if`/`try`, a labeled `for` | each violation pinned; for-of/for-in must not fire |
| all three | `custom-biome-ignore-next-line <rule-name>` directly above | 0 violations (suppressed) |
| all three | rule left at its default (no `ignoreBiomeExtensionRules` entry) | 0 violations regardless of code (ships off) |

## TODO / implementation checklist

1. Create `src/rules/no_while_statement.rs`, `src/rules/no_do_while_statement.rs`,
   `src/rules/no_for_statement.rs` — each implements `Rule`, matching the shape
   in "Detection sketch" above (and the exact trait in `ADDING_A_RULE.md`). All
   three: no `semantic()` call, no `Fix`, `default_severity()` overridden to
   `RuleSeverity::Off`.
2. Declare and re-export all three in `src/rules/mod.rs`.
3. Register all three in `RuleRegistry::with_all_rules` (`src/rules/registry.rs`).
4. Add **four** fixtures per rule (mandatory — see `TESTING.md`):
   `fixtures/no_while_statement/{valid,invalid,suppressed,edge-cases}.js`, same
   for the other two. Pull `valid`/`invalid` from the Test cases table above and
   add an `edge-cases.js` per rule covering the nested/near-miss/short-circuit
   rows in that table; each `edge-cases.js` violation is a **pinned count**
   asserted by the rule's test (`every_rule_has_fixtures_for_all_four_cases`
   enforces the four-file set, but the per-rule test must also assert the
   edge-cases count so it can't silently drift). For `no-do-while-statement`,
   since no real <PRIVATE_REPO> example exists, write a synthetic invalid case
   matching the `do-while` shape shown in this doc.
5. Add a test module per rule to `tests/integration.rs` (see `ADDING_A_RULE.md`
   §Step 5). Use `check_source(rule, source, path)` for inline snippets and
   `check_one(rule, rule_dir, fixture_name)` for fixture files — both run *only*
   the named rule. Cover: a positive case with an asserted `(line, col)`, a
   near-miss that must not fire, a suppression case, and a pinned `edge-cases.js`
   count.
6. Document all three in `RULES.md`, using the same section structure as
   `bare-arrow-param-prop-assign`/`deep-param-prop-assign` (What it catches /
   Before-after / off-by-default rationale / known non-goals).
7. Run the gates from `ADDING_A_RULE.md` — `cargo test` (currently **251**
   tests, additive), `cargo clippy --all-targets`, `cargo build --release`, and
   `./target/release/custom-biome-lint fixtures` (exits `1` by design — the
   fixtures contain deliberate violations; CI wraps it in `test "$code" -eq 1`).
   Verify reported positions with `fixtures --write-fix --dry-run`.
8. Integration sanity check: build the tool, then, from the <PRIVATE_REPO> repository
    root (or a worktree of it),
   temporarily set `"ignoreBiomeExtensionRules": { "no-while-statement":
   "error", "no-do-while-statement": "error", "no-for-statement": "error" }`
   and run `custom-biome-lint` against `src/**/*.{js,jsx}` — confirm the
   finding counts land in the same ballpark as the audit's per-construct
   breakdown (42 while, 0 do-while, 15 for) plus or minus drift from any code
   changes since 2026-08-15. Revert the temporary config after — whether to
   adopt these for real in that repo is a separate decision from building them.
