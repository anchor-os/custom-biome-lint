# Prompt: add a new rule to custom-biome-lint

This document is an **executable prompt**. Hand it to an AI agent (or a developer
new to the codebase) and it should be able to add a rule end to end without
further context.

[ADDING_A_RULE.md](ADDING_A_RULE.md) is the human-oriented reference for the same
task — the `Rule` trait table, the `FileContext` API, and tips for finding Biome
AST types. Read it alongside this document; this one adds the agent framing, a
fully worked and verified example, and the failure modes that actually bite.

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
| Tests pass | `cargo test` | 84 unit + 52 integration + 1 doc-test pass **before** your change; your new tests are additive |
| No lint warnings | `cargo clippy --all-targets` | zero warnings |
| Release builds | `cargo build --release` | succeeds |
| Fixtures behave | `./target/release/custom-biome-lint fixtures` | your rule reports `invalid.js`, stays silent on `valid.js` and `suppressed.js` |
| Documented | — | a section in [RULES.md](RULES.md) |

Do not report success on the basis of code that compiles. Run the gates.

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
}
```

There is no `default_pattern()` on the trait. The CLI's default glob is derived
by `RuleRegistry::default_pattern()` from the union of every registered rule's
`supported_extensions()`.

`name()` is load-bearing in three places at once: the rightmost column of CLI
output, the identifier users write in `// biome-ignore-line <name>`, and the
string they list in `ignoreBiomeExtensionRules` in `package.json`. Pick it once
and use it verbatim everywhere.

### What `FileContext` gives you

```rust
file.tree()            // &JsSyntaxNode — shared parsed AST. Never re-parse.
file.line_col(offset)  // byte offset -> (1-based line, 1-based col)
file.source()          // &str — original text
file.path()            // &Path — file being linted
file.parsed_cleanly()  // bool — did parsing produce errors
file.semantic()        // &SemanticModel — lexical scopes/bindings, built lazily on
                        // first call. Only reach for this if your rule genuinely
                        // needs identifier resolution (e.g. "does this call refer
                        // to a particular import, or a same-named local?"); see
                        // SEMANTIC_MODEL.md. None of the three existing rules use
                        // it — they're exact ESLint-parity ports, where matching
                        // by name/shape alone is the deliberate, documented
                        // behavior, not a gap to close with real resolution.
```

### The three existing rules, and what each teaches

| Rule | File | AST techniques worth copying |
| --- | --- | --- |
| `no-native-map` | `src/rules/no_native_map.rs` | Import/`require` tracking, destructuring patterns, **stateful** traversal — accumulates `ImmutableBindings` while walking, relying on `descendants()` being preorder so a declaration is seen before its uses |
| `no-arrow-function-create-selector` | `src/rules/no_arrow_function_create_selector.rs` | Arrow functions, walking *up* to a parent declarator |
| `reselect-arity-match` | `src/rules/reselect_arity_match.rs` | Call expressions, identifier vs member callees, parameter-list arity |

The shape they all share:

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

Walk `descendants()`, `cast_ref` to the type you care about, `continue` on
anything uninteresting, convert a byte offset to line/col, push a `Violation`.

---

## Worked example: `no-await-in-loop`

Implement this rule as your exercise. The code below is **verified**: it compiles
against `biome_js_syntax` 0.5.7, passes `cargo clippy --all-targets` with zero
warnings, and satisfies every behavioural assertion in the test section.

### What it detects

`await` in the body of a `for` / `for...of` / `for...in` / `while` / `do...while`
loop. Each iteration waits for the previous one, serialising work that is usually
parallelisable.

```js
// INVALID — serial: each call waits for the last
for (const item of items) {
  await process(item);
}

// VALID — parallel
const results = await Promise.all(items.map(process));
```

Suppress with `// biome-ignore-line no-await-in-loop` (sometimes serialisation is
deliberate — rate limiting, ordered writes).

### The two cases that make this rule non-trivial

Naively reporting every `JsAwaitExpression` that has a loop ancestor is wrong in
two ways, and both appear in real code:

**1. `await` in the loop *header* runs once, not per iteration.**

```js
for (const x of await getItems()) { }  // fine — one await, before the loop
while (await hasNext()) { }            // condition, not body
```

**2. `await` inside a function declared in the loop body belongs to that
function.**

```js
for (const i of items) {
  items.map(async x => await g(x));  // fine — await is the arrow's, not the loop's
}
```

So the ancestor walk must (a) confirm it entered the loop through its **body**,
and (b) **stop at the first function boundary**. That is the entire subtlety of
this rule, and it is why `is_in_loop_body` looks the way it does.

### `src/rules/no_await_in_loop.rs`

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

// Message as a const, matching the other rules: it is asserted on in tests.
const MESSAGE: &str =
    "Avoid await inside a loop; collect the promises and await Promise.all instead.";

// Unit struct — a rule holds no state between files. Per-file state lives in
// check() (see ImmutableBindings in no_native_map.rs).
pub struct NoAwaitInLoop;

impl Rule for NoAwaitInLoop {
    fn name(&self) -> &'static str {
        "no-await-in-loop" // kebab-case; users type this in ignore comments
    }

    fn description(&self) -> &'static str {
        "Disallow await expressions inside loop bodies"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS // [".js", ".jsx"] — shared constant, do not inline
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let mut violations = Vec::new();

        // Preorder walk of the shared tree. No re-parsing, no extension check,
        // no suppression check — the runner owns all three.
        for node in file.tree().descendants() {
            // Not an await: skip. `for await (...)` is part of the for-of
            // statement itself, not a JsAwaitExpression, so it never lands here.
            let Some(await_expression) = JsAwaitExpression::cast_ref(&node) else {
                continue;
            };
            if !is_in_loop_body(&node) {
                continue;
            }

            // text_trimmed_range() excludes leading trivia, so the column points
            // at `await` rather than at the whitespace before it.
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
///
/// - a loop, which counts only if the path entered through its body. This keeps
///   `for (const x of await getItems())` clean: that await is in the loop header
///   and runs once, not once per iteration.
/// - a function boundary, which does not count. An await inside a callback
///   declared in the loop body belongs to that callback, not to the loop.
fn is_in_loop_body(node: &JsSyntaxNode) -> bool {
    let mut child = node.clone();

    // skip(1) because ancestors() yields `node` itself first.
    for ancestor in node.ancestors().skip(1) {
        if is_loop(ancestor.kind()) {
            // Identity comparison: was `child` the loop's body, or its header?
            return loop_body(&ancestor).is_some_and(|body| body == child);
        }
        if is_function_boundary(ancestor.kind()) {
            return false;
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
```

Note the accessor style: `body()` returns a `Result` because Biome's AST is
error-tolerant. Every accessor is `.ok()?` / `let Ok(..) = .. else` — a malformed
file must be skipped, never panic.

---

## Step-by-step checklist

### 1. Create `src/rules/no_await_in_loop.rs`

Implement the `Rule` trait exactly as above.

### 2. Declare and re-export the module in `src/rules/mod.rs`

Both lists are alphabetical:

```rust
pub mod no_arrow_function_create_selector;
pub mod no_await_in_loop;                     // <- add
pub mod no_native_map;

pub use no_arrow_function_create_selector::NoArrowFunctionCreateSelector;
pub use no_await_in_loop::NoAwaitInLoop;      // <- add
pub use no_native_map::NoNativeMap;
```

### 3. Register in `src/rules/registry.rs`

Two edits — the `use`, and the vector in `with_all_rules`:

```rust
use super::no_await_in_loop::NoAwaitInLoop;   // <- add

pub fn with_all_rules() -> Self {
    Self {
        rules: vec![
            Box::new(NoNativeMap),
            Box::new(NoArrowFunctionCreateSelector),
            Box::new(ReselectArityMatch),
            Box::new(NoAwaitInLoop),           // <- add
        ],
    }
}
```

This is the **only** registration point. Order does not affect output — the
runner sorts by `(line, col, rule)`. `default_pattern()` widens automatically
from the union of all rules' extensions, so declaring a new extension needs no
further wiring.

### 4. Create `fixtures/no_await_in_loop/`

The directory name is the rule name with **underscores** — the
`every_rule_has_fixtures_for_all_three_cases` test derives it via
`rule.name().replace('-', "_")` and will fail if you use dashes.

Fixtures are the readable specification of the rule; someone deciding whether to
enable it reads `invalid.js` first. Keep them small and keep line numbers stable,
because tests assert on them.

`valid.js`:

```js
// Parallel: the whole point of the rule.
export async function processAll(items) {
  const results = await Promise.all(items.map(item => process(item)));
  return results;
}

// The await is in the loop header — it runs once, before iterating.
export async function iterateFetched() {
  for (const item of await getItems()) {
    render(item);
  }
}

// The await belongs to the arrow function, not to the loop.
export async function nestedCallback(items) {
  for (const item of items) {
    items.map(async other => await process(other));
  }
}

// Not a loop at all.
export async function single(item) {
  return await process(item);
}
```

`invalid.js`:

```js
export async function serialForOf(items) {
  for (const item of items) {
    await process(item);
  }
}

export async function serialWhile() {
  while (hasNext()) {
    await advance();
  }
}

export async function serialDoWhile() {
  do {
    await advance();
  } while (hasNext());
}
```

`suppressed.js` — the same violations, silenced both ways:

```js
export async function rateLimited(items) {
  for (const item of items) {
    await process(item); // biome-ignore-line no-await-in-loop -- deliberate rate limiting
  }
}

export async function orderedWrites(items) {
  for (const item of items) {
    // biome-ignore-next-line no-await-in-loop
    await write(item);
  }
}
```

### 5. Add tests to `tests/integration.rs`

Follow the existing module pattern. `check_source` runs **only** the named rule,
so another rule firing on the same source cannot pollute your expectations:

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
    fn braceless_loop_body_is_reported() {
        assert_eq!(check("async function f(a){ for (const i of a) await g(i); }").len(), 1);
    }

    #[test]
    fn position_points_at_the_await_keyword() {
        let violations = check("async function f(a){\n  for (const i of a) {\n    await g(i);\n  }\n}");
        assert_eq!((violations[0].line, violations[0].col), (3, 5));
    }
}
```

Minimum coverage for any rule: a positive case with asserted line/col, a
near-miss that must not fire, and a suppression case.

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

Add a `## \`no-await-in-loop\`` section matching the existing structure: what it
catches, valid/invalid examples, rationale, and **known limitations**. Be honest
about the limitations — see below.

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
- Rustdoc comments explain *why* a check exists (the header-vs-body distinction),
  not what the next line does.

## Common pitfalls

**Do not re-implement the plumbing.**

- **Suppression comments.** Do not scan for `biome-ignore-*` in your rule. Report
  everything you find; the runner filters. Re-implementing it means suppressions
  applied twice, or inconsistently with the other rules.
- **File extensions.** Do not check `file.path().extension()`. The runner skipped
  the file already if `supported_extensions()` did not cover it.
- **Re-parsing.** Never call `FileContext::parse` inside a rule. Use
  `file.tree()`. The whole point of passing a `FileContext` is that a file is
  parsed once and all rules share the tree.
- **Printing.** Return `Violation`s. The CLI owns all output; a `println!` in a
  rule corrupts the report format.

**Scope your violations to the file in hand.** `check` receives one
`FileContext` and must return violations for that file only. There is no
cross-file state, and the tool makes no ordering guarantee between files.

**Get the position right.** Suppressions key off the **reported** line. If you
report at a position away from what the user sees as the offending code, their
ignore comment will not work where they put it. Verify with
`--write-fix --dry-run`.

**Fixture directory uses underscores.** `no_await_in_loop`, not
`no-await-in-loop`.

**Know your rule's limits and write them down.** The example above is purely
syntactic. It cannot see through an indirection:

```js
const runAll = async items => { for (const i of items) await g(i); }; // caught
const step = async i => { await g(i); };
for (const i of items) { step(i); }   // NOT caught — no await in the loop body
```

That is acceptable — every rule here is syntactic, matching the ESLint rules they
replace. What is not acceptable is leaving it undocumented. Put it under "Known
limitations" in [RULES.md](RULES.md).

---

## Reference

| Topic | Document |
| --- | --- |
| Human guide to the same task, plus Biome AST tips | [ADDING_A_RULE.md](ADDING_A_RULE.md) |
| Why the runner owns suppression and filtering | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Existing rules, examples, limitations | [RULES.md](RULES.md) |
| Test layout and verbosity flags | [TESTING.md](TESTING.md) |
| Build and toolchain setup | [SETUP.md](SETUP.md) |
| Lexical scope/binding model and identifier resolution | [SEMANTIC_MODEL.md](SEMANTIC_MODEL.md) |
