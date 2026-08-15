# Adding a rule

This is the **single canonical guide** for adding a lint rule to
`custom-biome-lint`. It works both as a human reference and as an executable
prompt you can hand to an AI agent: a developer new to the codebase, or an agent
with no other context, should be able to add a rule end to end from this file
alone.

This document deliberately serves two readers at once: a **human contributor**
(read it top to bottom as a reference) and an **AI agent** (skip to the
[Step-by-step checklist](#step-by-step-checklist) and execute it). The
conceptual sections — Rule anatomy, the existing-rules table, pitfalls, and AST
lookup — explain *why*; the checklist is the *procedure*. You do not need the
concepts to follow the steps, but you will need them to debug a rule that
misbehaves.

It replaces the old `for-new-rule-addition.md`, which is now deleted — do not
link to that path. If you find a stray reference, point it here.

---

## Preamble — what you are doing

You are adding one lint rule to a standalone Rust linter that covers JS/JSX
patterns Biome does not. The tool parses each file once with `biome_js_parser`,
then hands the shared syntax tree to every registered rule.

A rule contains **detection logic and nothing else**. The runner already owns:

- reading files and expanding globs,
- filtering by file extension,
- parsing the source,
- discarding violations that carry a suppression comment,
- sorting output and setting the exit code.

If you find yourself writing code for any of the above, you have misunderstood
the architecture — stop and re-read [ARCHITECTURE.md](ARCHITECTURE.md).

### What success looks like

All of the following must be true before you claim the task is done:

| Gate | Command | Expected |
| --- | --- | --- |
| Tests pass | `cargo test` | the suite currently has **251** tests (94 unit + 156 integration + 1 doc-test) and must stay green; your new tests are additive |
| No lint warnings | `cargo clippy --all-targets` | zero warnings (use `-- -D warnings` in CI to escalate) |
| Release builds | `cargo build --release` | succeeds |
| Fixtures behave | `./target/release/custom-biome-lint fixtures` | your rule reports `invalid.js` and the pinned violations in `edge-cases.js`; stays silent on `valid.js` / `suppressed.js`; see [TESTING.md](TESTING.md) for the aggregate counts (they grow as rules are added) |
| Documented | — | a section in [RULES.md](RULES.md) |

Do not report success on the basis of code that compiles. Run the gates. The
fixture run exits `1` by design (the fixtures contain deliberate violations),
which is why CI wraps it in a `test "$code" -eq 1` guard — see
[TESTING.md](TESTING.md).

---

## Rule anatomy

### The trait

Every rule is a **unit struct** implementing `Rule` (`src/rules/rule.rs`):

```rust
pub trait Rule: Send + Sync {
    fn name(&self) -> &'static str;                       // kebab-case
    fn description(&self) -> &'static str;                // one line
    fn supported_extensions(&self) -> &'static [&'static str]; // leading dots
    fn check(&self, file: &FileContext) -> Vec<Violation>;

    fn default_severity(&self) -> RuleSeverity { RuleSeverity::Error }
}
```

| Method | Notes |
| --- | --- |
| `name` | **Kebab-case.** This is the identifier users write in suppression comments (`// custom-biome-ignore-line <rule>`), the key they list in `ignoreBiomeExtensionRules` in `package.json`, and the rightmost column of CLI output. Pick it once, use it verbatim everywhere. |
| `description` | One line, shown under `-v`. |
| `supported_extensions` | Leading dots, e.g. `[".js", ".jsx"]`. Use the `JS_EXTENSIONS` constant from `src/rules/mod.rs` unless your rule needs a different set. The runner skips files whose extension is not listed. |
| `check` | Detection logic. Receives an already-parsed file. |
| `default_severity` | Has a default (`Error`); override to `RuleSeverity::Off` only to ship a rule off by default. |

There is no `default_pattern()` on the trait. The CLI's default glob is derived
by `RuleRegistry::default_pattern()` from the union of every registered rule's
`supported_extensions()`, so registering a rule with a new extension widens the
default automatically — no per-rule glob to keep in sync.

### What `FileContext` gives you

```rust
file.tree()            // &JsSyntaxNode — shared parsed AST. Never re-parse.
file.line_col(offset)  // byte offset -> (1-based line, 1-based col)
file.source()          // &str — original text
file.path()            // &Path — file being linted
file.parsed_cleanly() // bool — did parsing produce errors
file.semantic()        // &SemanticModel — lexical scopes/bindings, built lazily on
                       // first call. Reach for this when your rule needs identifier
                       // resolution (e.g. "does this call refer to a particular
                       // import, or a same-named local?"); see SEMANTIC_MODEL.md.
                       // Use semantic().resolve(&reference_identifier) and check
                       // binding.import() -- do not match by identifier text alone
                       // when what you actually care about is where a name came
                       // from.
```

The file is parsed **once** and every rule shares this tree — that is why
`check` takes a `FileContext` rather than a raw `&str`. Never call
`FileContext::parse` inside a rule.

### Severity: shipping a rule off by default

Every rule is on unless `ignoreBiomeExtensionRules` turns it off. A rule can
invert that by overriding one trait method:

```rust
fn default_severity(&self) -> RuleSeverity {
    RuleSeverity::Off
}
```

With no config entry the rule then never runs; a consuming repo opts in by
giving it a severity:

```json
{ "ignoreBiomeExtensionRules": { "my-rule": "error" } }
```

`RuleRegistry::enabled`/`ignored` resolve this through
`PackageConfig::severity(name, rule.default_severity())`, which — unlike
`severity_override` — keeps `"off"` and "no entry" apart. **Use `severity`, not
`severity_override`, for any "does this rule run" question**; `severity_override`
collapses both to `None`, which is right for overriding a violation's severity
and wrong for deciding whether the rule runs at all. See
[RULES.md#opting-into-the-default-off-rules](RULES.md#opting-into-the-default-off-rules).

Reach for this when a rule's findings are opinionated or style-contingent enough
that a repo should have to ask for them. Six rules already ship off by default:
the three param-mutation rules (`bare-arrow-param-prop-assign`,
`deep-param-prop-assign`, `param-mutating-array-method-call`) and the three
loop-ban rules (`no-while-statement`, `no-do-while-statement`,
`no-for-statement`) — each is a house style, not a universal correctness fix. A
rule that is simply *noisy* is usually a rule with a detection bug, not a
candidate for this.

Two things this does not change: an off-by-default rule still shows up in
`registry.all()` (so `--help`/docs list it), and it still needs the full fixture
set — its fixtures are exercised by `cargo test`, which runs a named rule
directly, rather than by a CLI run over `fixtures/`.

### Autofix (optional)

Only attach a `Fix` when the correction is unambiguous. If producing one would
mean guessing (which argument to add, which side of a mismatch is wrong), leave
it out — a violation with no `Fix` is reported as skipped by `--auto-fix`, which
is the honest answer, rather than silently doing the wrong rewrite. An
existence-ban rule (e.g. "no `while` statement") is the extreme case: there is
no mechanical rewrite, so it never carries a `Fix`.

You compute the `Fix` in the same `check()` call that detected the violation —
never in a second pass — because only the code that found the violation still
has the exact syntax node in hand:

```rust
let range = node.syntax().text_trimmed_range();
let fix = Fix {
    start: usize::from(range.start()),
    end: usize::from(range.end()),
    replacement: call.syntax().text_trimmed().to_string(),
};
violations.push(Violation::error(self.name(), line, col, message).with_fix(fix));
```

`--auto-fix` (see `src/autofix.rs`) applies the byte-range replacement, verifies
the rewritten file still parses, and only then writes it.

### Reporting position

Report at the byte offset of the node you want the user's cursor to land on, and
convert with `file.line_col`:

```rust
let offset = usize::from(some_node.syntax().text_trimmed_range().start());
let (line, col) = file.line_col(offset);
```

Use `text_trimmed_range()` rather than `text_range()` — the untrimmed range
includes leading trivia (whitespace and comments), which would point the
diagnostic at the wrong column.

Remember that suppressions key off the **reported** line. If you report at a
position far from what the user thinks of as the offending code, their ignore
comment will not be where they expect it. Verify with
`--write-fix --dry-run` (see below).

### Things a rule must NOT do

- **Do not check for suppression comments.** The runner drops suppressed
  violations after `check` returns. Just report everything you find.
- **Do not check the file extension.** The runner already skipped the file if
  your `supported_extensions` did not cover it.
- **Do not print anything.** Return `Violation`s; the CLI owns all output.
- **Do not re-parse the source.** Use `file.tree()`.

### The existing rules, and what each teaches

There are eleven rules today. The shape they all share is the walk below; the
differences are in *which* node they cast to and how much of the semantic model
they use.

```rust
fn check(&self, file: &FileContext) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in file.tree().descendants() {
        let Some(typed) = SomeJsType::cast_ref(&node) else {
            continue;
        };
        if !is_interesting(&typed) {
            continue;
        }

        let offset = usize::from(typed.syntax().text_trimmed_range().start());
        let (line, col) = file.line_col(offset);
        violations.push(Violation::error(self.name(), line, col, MESSAGE));
    }

    violations
}
```

| Rule | File | AST techniques worth copying |
| --- | --- | --- |
| `no-native-map` | `src/rules/no_native_map.rs` | Import/`require` tracking, destructuring patterns, and **semantic resolution** — a forward pass over declarators builds two `HashSet<usize>` offset sets (which bindings represent the `immutable` module vs. its `Map` export) via `file.semantic()`, then every `Map` reference is checked against those sets independently, so shadowing resolves correctly |
| `no-arrow-function-create-selector` | `src/rules/no_arrow_function_create_selector.rs` | Arrow functions, walking *up* to a parent declarator, and resolving the callee against `import { createSelector } from "reselect"` via the shared helper in `src/rules/reselect.rs` |
| `reselect-arity-match` | `src/rules/reselect_arity_match.rs` | Call expressions, identifier vs member callees, parameter-list arity; the identifier callee is resolved semantically (same shared helper), the member-expression callee deliberately stays name-based |
| `destructure-default-param-assign` / `destructure-param-prop-assign` / `bare-arrow-param-prop-assign` / `deep-param-prop-assign` | `src/rules/param_mutation.rs` (one file, four rules) | Assignment-target resolution via `model.resolve_assignment`, walking up arrow/function scopes to the parameter a mutation is rooted in, depth-bounded reporting |
| `param-mutating-array-method-call` | `src/rules/param_mutating_array_method_call.rs` | Method-call detection with semantic resolution of the receiver |
| `no-while-statement` | `src/rules/no_while_statement.rs` | Pure cast-and-report: walk `descendants()`, cast to `JsWhileStatement`, report at the `while` keyword. No semantic model. |
| `no-do-while-statement` | `src/rules/no_do_while_statement.rs` | Same shape, cast to `JsDoWhileStatement`. |
| `no-for-statement` | `src/rules/no_for_statement.rs` | Same shape, cast to `JsForStatement` only — `for...of`/`for...in` are distinct node kinds and out of scope. |

---

## Worked example 1 — `no-console-log` (minimal)

The simplest rule shape: walk `descendants()`, match a call expression, report.

```rust
use biome_js_syntax::{AnyJsExpression, JsCallExpression};
use biome_rowan::AstNode;

use crate::analyzer::runner::FileContext;
use crate::diagnostics::{Fix, Violation};
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;

const MESSAGE: &str = "Avoid console.log in committed code.";

pub struct NoConsoleLog;

impl Rule for NoConsoleLog {
    fn name(&self) -> &'static str {
        "no-console-log"
    }

    fn description(&self) -> &'static str {
        "Disallow console.log calls"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let mut violations = Vec::new();

        for node in file.tree().descendants() {
            let Some(call) = JsCallExpression::cast_ref(&node) else {
                continue;
            };
            if !is_console_log(&call) {
                continue;
            }

            let offset = usize::from(call.syntax().text_trimmed_range().start());
            let (line, col) = file.line_col(offset);
            violations.push(Violation::error(self.name(), line, col, MESSAGE));
        }

        violations
    }
}

/// Matches `console.log(...)`.
fn is_console_log(call: &JsCallExpression) -> bool {
    let Ok(AnyJsExpression::JsStaticMemberExpression(member)) = call.callee() else {
        return false;
    };
    let object_is_console = matches!(
        member.object(),
        Ok(AnyJsExpression::JsIdentifierExpression(ident))
            if ident.name().and_then(|n| n.value_token())
            .is_ok_and(|t| t.text_trimmed() == "console")
    );
    let member_is_log = member
        .member()
        .ok()
        .and_then(|m| m.as_js_name().cloned())
        .and_then(|name| name.value_token().ok())
        .is_some_and(|t| t.text_trimmed() == "log");

    object_is_console && member_is_log
}
```

Note the member-expression matching — `call.callee()` returns a `Result` (Biome
is error-tolerant), and `object()` / `member()` do too. Handle every accessor
with `.ok()?` or `let Ok(..) = .. else { continue }`. A malformed file must be
skipped, never panic.

---

## Worked example 2 — `no-await-in-loop` (the harder shape)

This rule shows the ancestor-walk subtlety that a real rule usually hits: an
`await` is only a violation when it sits in a **loop body**, not in the loop
header, and not inside a callback declared in that body.

```js
// INVALID — serial: each call waits for the last
for (const item of items) {
  await process(item);
}
// VALID — parallel
const results = await Promise.all(items.map(process));
```

```rust
use biome_js_syntax::{
    JsAwaitExpression, JsDoWhileStatement, JsForInStatement, JsForOfStatement, JsForStatement,
    JsSyntaxKind, JsSyntaxNode, JsWhileStatement,
};
use biome_rowan::AstNode;

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;

const MESSAGE: &str =
    "Avoid await inside a loop; collect the promises and await Promise.all instead.";

pub struct NoAwaitInLoop;

impl Rule for NoAwaitInLoop {
    fn name(&self) -> &'static str { "no-await-in-loop" }
    fn description(&self) -> &'static str { "Disallow await expressions inside loop bodies" }
    fn supported_extensions(&self) -> &'static [&'static str] { JS_EXTENSIONS }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let mut violations = Vec::new();
        for node in file.tree().descendants() {
            let Some(await_expression) = JsAwaitExpression::cast_ref(&node) else {
                continue;
            };
            if !is_in_loop_body(&node) {
                continue;
            }
            let offset = usize::from(await_expression.syntax().text_trimmed_range().start());
            let (line, col) = file.line_col(offset);
            violations.push(Violation::error(self.name(), line, col, MESSAGE));
        }
        violations
    }
}

/// Whether `node` sits in the *body* of a loop in the same function.
///
/// Walks outwards tracking the child it came through, and stops at the first of:
/// - a loop, which counts only if the path entered through its body. This keeps
///   `for (const x of await getItems())` clean: that await is in the loop header
///   and runs once, not once per iteration.
/// - a function boundary, which does not count. An await inside a callback
///   declared in the loop body belongs to that callback, not to the loop.
fn is_in_loop_body(node: &JsSyntaxNode) -> bool {
    let mut child = node.clone();
    for ancestor in node.ancestors().skip(1) {
        if is_function_boundary(ancestor.kind()) {
            return false;
        }
        if is_loop(ancestor.kind())
            && loop_body(&ancestor).is_some_and(|body| body == child)
        {
            return true;
        }
        child = ancestor;
    }
    false
}

fn is_loop(kind: JsSyntaxKind) -> bool {
    matches!(
        kind,
        JsSyntaxKind::JS_FOR_STATEMENT
            | JsSyntaxKind::JS_FOR_OF_STATEMENT
            | JsSyntaxKind::JS_FOR_IN_STATEMENT
            | JsSyntaxKind::JS_WHILE_STATEMENT
            | JsSyntaxKind::JS_DO_WHILE_STATEMENT
    )
}

fn is_function_boundary(kind: JsSyntaxKind) -> bool {
    matches!(
        kind,
        JsSyntaxKind::JS_FUNCTION_DECLARATION
            | JsSyntaxKind::JS_FUNCTION_EXPRESSION
            | JsSyntaxKind::JS_ARROW_FUNCTION_EXPRESSION
            | JsSyntaxKind::JS_METHOD_CLASS_MEMBER
            | JsSyntaxKind::JS_METHOD_OBJECT_MEMBER
    )
}

/// The statement forming a loop's body, as a raw node for identity comparison.
fn loop_body(loop_node: &JsSyntaxNode) -> Option<JsSyntaxNode> {
    let body = match loop_node.kind() {
        JsSyntaxKind::JS_FOR_STATEMENT => JsForStatement::cast_ref(loop_node)?.body().ok()?,
        JsSyntaxKind::JS_FOR_OF_STATEMENT => JsForOfStatement::cast_ref(loop_node)?.body().ok()?,
        JsSyntaxKind::JS_FOR_IN_STATEMENT => JsForInStatement::cast_ref(loop_node)?.body().ok()?,
        JsSyntaxKind::JS_WHILE_STATEMENT => JsWhileStatement::cast_ref(loop_node)?.body().ok()?,
        JsSyntaxKind::JS_DO_WHILE_STATEMENT => {
            JsDoWhileStatement::cast_ref(loop_node)?.body().ok()?
        }
        _ => return None,
    };
    Some(body.syntax().clone())
}

---

## Step-by-step checklist

The snippets below use a placeholder rule named `no-example-rule`
(`no_example_rule` / `NoExampleRule`). Substitute your real rule name (kebab-case
for the `name`, underscores for the module/file). For a concrete, already-merged
example of these exact edits, see the three loop-ban rules
(`no-while-statement`, `no-do-while-statement`, `no-for-statement`) in
`src/rules/`.

### 1. Create `src/rules/<rule_name>.rs`

Implement the `Rule` trait exactly as above. Decide up front:

- **Semantic model?** Only if you need identifier resolution. The three
  loop-ban rules (already in the repo) need none — they are pure cast-and-report;
  `no-native-map` shows the opposite end, leaning heavily on the semantic model.
- **`Fix`?** Only if the rewrite is unambiguous (see Autofix above). Existence
  bans attach none.
- **`default_severity`?** Override to `Off` only for opinionated house-style
  rules.

### 2. Declare and re-export the module in `src/rules/mod.rs`

**Both lists are alphabetical** — the module declarations and the `pub use`
re-exports. Insert your three entries where they sort:

```rust
pub mod no_arrow_function_create_selector;
pub mod no_example_rule;                        // <- add (your rule, alphabetical)
pub mod no_native_map;
// …other rules, alphabetical…

pub use no_arrow_function_create_selector::NoArrowFunctionCreateSelector;
pub use no_example_rule::NoExampleRule;         // <- add
pub use no_native_map::NoNativeMap;
// …other re-exports, alphabetical…
```

### 3. Register in `src/rules/registry.rs`

This is the **only** registration point. Two edits — the `use`, and the vector
in `with_all_rules`:

```rust
use super::no_example_rule::NoExampleRule;              // <- add

pub fn with_all_rules() -> Self {
    Self {
        rules: vec![
            Box::new(NoNativeMap),
            // …existing rules, alphabetical…
            Box::new(NoExampleRule),                     // <- add
        ],
    }
}
```

Order does not affect output — the runner sorts violations by
`(line, col, rule)`. `default_pattern()` widens automatically from the union of
all rules' extensions, so declaring a new extension needs no further wiring.

### 4. Create `fixtures/<rule_name>/` — **four** files

The directory name is the rule name with **underscores** — the
`every_rule_has_fixtures_for_all_four_cases` test derives it via
`rule.name().replace('-', "_")` and will fail if you use dashes. **Four** files
are required (not three):

| File | Contents |
| --- | --- |
| `valid.js` | Patterns that must **not** be reported. |
| `invalid.js` | Patterns that must be reported. Keep it small and keep line numbers stable — tests assert on them. |
| `suppressed.js` | The same violations, silenced both ways (`// custom-biome-ignore-line <rule>` and `// custom-biome-ignore-next-line <rule>`). |
| `edge-cases.js` | Deliberate boundary behaviours — nested/compound/near-miss forms. Each violation here is a **pinned count** asserted by name (see [TESTING.md](TESTING.md)); do not let it drift. |

Fixtures are the readable specification of the rule; someone deciding whether to
enable it reads `invalid.js` first. For an existence-ban rule, `valid.js` is the
"this construct, but it's fine because …" set and `edge-cases.js` is where the
"almost but not quite" forms live.

### 5. Add tests to `tests/integration.rs`

Follow the existing module pattern. `check_source` runs **only** the named rule,
so another rule firing on the same source cannot pollute your expectations;
`check_one` loads a fixture file by name. (Both are thin wrappers over the public
`lint_source`, which runs the full parse → check → suppression-filter pipeline.)

```rust
mod no_await_in_loop {
    use super::*;

    const RULE: &str = "no-await-in-loop";
    const DIR: &str = "no_await_in_loop";

    fn check(source: &str) -> Vec<Violation> {
        check_source(RULE, source, Path::new("a.js"))
    }

    #[test]
    fn parallel_and_header_awaits_are_allowed() {
        assert!(check_one(RULE, DIR, "valid.js").is_empty());
    }

    #[test]
    fn serial_awaits_are_reported() {
        let violations = check_one(RULE, DIR, "invalid.js");
        assert_eq!(violations.len(), 3, "got {violations:?}");
        assert!(violations.iter().all(|v| v.message.contains("Promise.all")));
    }

    #[test]
    fn suppression_comments_silence_the_rule() {
        assert!(check_one(RULE, DIR, "suppressed.js").is_empty());
    }

    #[test]
    fn every_loop_form_is_covered() {
        assert_eq!(check("async function f(){ while (x) { await g(); } }").len(), 1);
        assert_eq!(check("async function f(){ do { await g(); } while (x); }").len(), 1);
        assert_eq!(check("async function f(){ for (let i=0;i<n;i++) { await g(); } }").len(), 1);
        assert_eq!(check("async function f(o){ for (const k in o) { await g(k); } }").len(), 1);
    }

    #[test]
    fn await_in_the_loop_header_is_allowed() {
        assert!(check("async function f(){ for (const x of await items()) { h(x); } }").is_empty());
        assert!(check("async function f(){ while (await next()) { h(); } }").is_empty());
    }

    #[test]
    fn await_in_a_nested_function_belongs_to_that_function() {
        let source = "async function f(a){ for (const i of a) { a.map(async x => await g(x)); } }";
        assert!(check(source).is_empty());
    }

    #[test]
    fn for_await_of_is_not_an_await_expression() {
        assert!(check("async function f(s){ for await (const c of s) { h(c); } }").is_empty());
    }

    #[test]
    fn position_points_at_the_await_keyword() {
        let violations = check("async function f(a){\n  for (const i of a) {\n    await g(i);\n  }\n}");
        assert_eq!((violations[0].line, violations[0].col), (3, 5));
    }
}
```

Minimum coverage for any rule: a positive case with asserted line/col, a
near-miss that must not fire, and a suppression case. For an existence-ban rule,
add the "different-but-related construct must not fire" case (e.g. a `for-of`
must not trip `no-for-statement`).

### 6. Run every gate

```sh
cargo test
cargo clippy --all-targets
cargo build --release
./target/release/custom-biome-lint fixtures
```

Verify the reported positions land on real violations:

```sh
./target/release/custom-biome-lint fixtures --write-fix --dry-run
```

`--dry-run` prints the suppression comments it *would* insert without touching
any file. If a proposed comment lands on a line that is not the offending code,
your reported offset is wrong.

### 7. Document in `docs/RULES.md`

Add a `## \`no-<name>\`` section matching the existing structure: what it
catches, before/after (where a rewrite exists), rationale, and **known
limitations / non-goals**. Be honest about the limits — every rule here is
syntactic, and an undocumented limitation is worse than a documented one.

---

## Code-quality guardrails

- Implement the `Rule` trait signature exactly; do not add methods or change
  arities.
- Use the `JS_EXTENSIONS` constant from `src/rules/mod.rs` rather than inlining
  `[".js", ".jsx"]`.
- Reuse the centralised suppression logic — which means **not touching it**. The
  runner calls `Suppressions::is_suppressed` after your `check` returns.
- `text_trimmed_range()`, never `text_range()`, for reported offsets.
- Every AST accessor returns `Result`/`Option`: handle with `.ok()?` or
  `let Ok(..) = .. else { continue }`. No `unwrap()` on parsed input.
- `cargo clippy --all-targets` must be silent, not merely non-fatal.

## Common pitfalls

- **Re-implementing the plumbing.** No suppression scanning, no extension
  checks, no re-parsing, no `println!` in a rule — the runner and CLI own all of
  those.
- **Scope your violations to the file in hand.** `check` receives one
  `FileContext` and must return violations for that file only. There is no
  cross-file state, and the tool makes no ordering guarantee between files.
- **Getting the position wrong.** Suppressions key off the reported line; if you
  report away from the code the user sees, their ignore comment won't work.
- **Forgetting `edge-cases.js`.** `every_rule_has_fixtures_for_all_four_cases`
  fails the build if any registered rule lacks `valid.js`, `invalid.js`,
  `suppressed.js` **or** `edge-cases.js`.
- **Non-alphabetical `mod.rs`.** Both the `pub mod` list and the `pub use` list
  are alphabetical; a misplaced entry is a needless review nit.

## Finding the right Biome AST types

The hardest part of writing a rule is knowing which `biome_js_syntax` type to
cast to. Useful approaches:

- **Read an existing rule.** `reselect_arity_match.rs` covers call expressions,
  callees and parameter lists. `no_arrow_function_create_selector.rs` covers
  arrow functions and walking up to a parent declarator. `no_native_map.rs`
  covers imports, `require`, and destructuring patterns. `param_mutation.rs`
  covers assignment-target resolution.
- **Type names are mechanical.** A JS construct maps to `Js<Construct>` (e.g.
  `JsCallExpression`, `JsArrowFunctionExpression`), and syntactic alternatives
  map to `AnyJs<Category>` enums (e.g. `AnyJsExpression`, `AnyJsCallArgument`).
- **Accessors return `Result` or `Option`.** Biome's AST is error-tolerant, so
  `call.callee()` gives a `Result`. Use `let Ok(..) = ... else { continue }` —
  a malformed file should be skipped, not panic.
- **Traits must be imported.** `AstNode` is needed for `cast_ref`/`syntax`, and
  `AstSeparatedList` for `.iter()`/`.len()` on argument and parameter lists. A
  "method not found" error on a list type usually means a missing
  `use biome_rowan::AstSeparatedList;`.
- **Inspect a tree.** `println!("{:#?}", file.tree())` in a scratch test prints
  the full node structure for a snippet, which is the fastest way to learn the
  shape you need to match.

---

## Reference

| Topic | Document |
| --- | --- |
| Why the runner owns suppression and filtering | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Existing rules, examples, limitations | [RULES.md](RULES.md) |
| Test layout, fixture guard, verbosity flags | [TESTING.md](TESTING.md) |
| Build and toolchain setup | [SETUP.md](SETUP.md) |
| Lexical scope/binding model and identifier resolution | [SEMANTIC_MODEL.md](SEMANTIC_MODEL.md) |
```
