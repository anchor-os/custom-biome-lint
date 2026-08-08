# Adding a rule

Four steps: write the rule file, register it, add fixtures, add tests. The runner
handles suppression comments and extension filtering for you, so a rule contains
detection logic and nothing else.

This guide works through a complete, real example: a rule that flags
`console.log` calls.

## Step 1 — Create the rule file

Create `src/rules/no_console_log.rs`. A rule is a unit struct implementing the
`Rule` trait:

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

### The `Rule` trait

```rust
pub trait Rule: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn supported_extensions(&self) -> &'static [&'static str];
    fn check(&self, file: &FileContext) -> Vec<Violation>;
}
```

| Method | Notes |
| --- | --- |
| `name` | **Kebab-case.** This is the identifier users write in suppression comments and in `ignoreBiomeExtensionRules`, and what appears in the output's rightmost column. |
| `description` | One line, shown under `-v`. |
| `supported_extensions` | Leading dots, e.g. `[".js", ".jsx"]`. Use the `JS_EXTENSIONS` constant unless your rule needs a different set. The runner skips files whose extension is not listed. |
| `check` | Detection logic. Receives an already-parsed file. |

There is no `default_pattern` on the trait: the CLI's default glob is derived by
`RuleRegistry::default_pattern()` from the union of every registered rule's
`supported_extensions()`, so registering a rule with a new extension widens the
default automatically — no per-rule glob to keep in sync.

### What `FileContext` gives you

```rust
file.tree()            // &JsSyntaxNode — the parsed AST, shared across all rules
file.line_col(offset)  // byte offset -> (1-based line, 1-based col)
file.source()          // &str — original text, if you need it
file.path()            // &Path — the file being linted
file.parsed_cleanly()  // bool — whether parsing produced errors
```

The file is parsed **once** and every rule shares this tree — that is why `check`
takes a `FileContext` rather than a raw `&str`. Never call
`FileContext::parse` inside a rule.

### Things a rule must NOT do

- **Do not check for suppression comments.** The runner drops suppressed
  violations after `check` returns. Just report everything you find.
- **Do not check the file extension.** The runner already skipped the file if
  your `supported_extensions` did not cover it.
- **Do not print anything.** Return `Violation`s; the CLI owns all output.
- **Do not re-parse the source.** Use `file.tree()`.

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
comment will not be where they expect it.

### Severity

`Violation::error(name, line, col, message)` is the common case.
`Violation::warning(...)` exists for non-blocking findings — warnings appear in
output but the tool's exit code is driven by errors.

### Autofix (optional)

Only attach a [`Fix`](../src/diagnostics/violation.rs) when the correction is
unambiguous. If producing one would mean guessing (which argument to add,
which side of a mismatch is wrong), leave it out — a violation with no `Fix`
is reported as skipped by `--auto-fix`, which is the honest answer, rather
than silently doing the wrong rewrite. This is why `reselect-arity-match` and
`no-native-map` don't have one, while `no-arrow-function-create-selector`
does: unwrapping `() => createSelector(...)` to `createSelector(...)` is
always correct, with nothing to guess.

You compute the `Fix` in the same `check()` call that detected the violation
— never in a second pass — because only the code that found the violation
still has the exact syntax node in hand:

```rust
let range = arrow.syntax().text_trimmed_range();
let fix = Fix {
    start: usize::from(range.start()),
    end: usize::from(range.end()),
    replacement: call.syntax().text_trimmed().to_string(),
};
violations.push(Violation::error(self.name(), line, col, message).with_fix(fix));
```

`--auto-fix` (see `src/autofix.rs`) applies the byte-range replacement,
verifies the rewritten file still parses, and only then writes it.

## Step 2 — Register the rule

Two edits. First, add the module and re-export in `src/rules/mod.rs`:

```rust
pub mod no_arrow_function_create_selector;
pub mod no_console_log;                          // <- add
pub mod no_native_map;
pub mod registry;
pub mod reselect_arity_match;
pub mod rule;

pub use no_arrow_function_create_selector::NoArrowFunctionCreateSelector;
pub use no_console_log::NoConsoleLog;            // <- add
pub use no_native_map::NoNativeMap;
```

Second, add it to the registry in `src/rules/registry.rs` — this is the single
registration point:

```rust
use super::no_console_log::NoConsoleLog;         // <- add

impl RuleRegistry {
    pub fn with_all_rules() -> Self {
        Self {
            rules: vec![
                Box::new(NoNativeMap),
                Box::new(NoArrowFunctionCreateSelector),
                Box::new(ReselectArityMatch),
                Box::new(NoConsoleLog),           // <- add
            ],
        }
    }
}
```

That is all the wiring. Registration order does not affect output — the runner
sorts violations by `(line, col, rule)`.

If your rule declares extensions the others do not (say `.ts`), the registry's
`default_pattern()` automatically widens to include them, because it is derived
from the union of all rules' `supported_extensions()` rather than hardcoded.

## Step 3 — Add fixtures

Create `fixtures/no_console_log/` with three files.

`valid.js` — patterns that must NOT be reported:

```js
// Not console.log — different member.
console.warn('this is fine');
console.error('so is this');

// Not the console object.
const logger = { log: () => {} };
logger.log('fine');
```

`invalid.js` — patterns that must be reported. Keep it small and keep the line
numbers stable, because tests assert on them:

```js
export function debugThing(thing) {
  console.log('thing is', thing);
  return thing;
}
```

`suppressed.js` — the same violations, silenced both ways:

```js
export function debugThing(thing) {
  console.log('same-line form', thing); // biome-ignore-line no-console-log

  // biome-ignore-next-line no-console-log
  console.log('next-line form', thing);

  return thing;
}
```

Fixtures are not just test inputs — they are the readable specification of the
rule. Someone deciding whether to enable your rule will read `invalid.js` first.

## Step 4 — Add tests

Add a module to `tests/integration.rs` following the existing pattern:

```rust
mod no_console_log {
    use super::*;

    fn rule() -> NoConsoleLog {
        NoConsoleLog
    }

    #[test]
    fn flags_console_log() {
        let violations = lint_source(
            "console.log('x');\n",
            Path::new("a.js"),
            &[&rule()],
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, 1);
        assert_eq!(violations[0].rule, "no-console-log");
    }

    #[test]
    fn allows_other_console_methods() {
        let violations = lint_source(
            "console.warn('x');\nconsole.error('y');\n",
            Path::new("a.js"),
            &[&rule()],
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn respects_suppression() {
        let violations = lint_source(
            "console.log('x'); // biome-ignore-line no-console-log\n",
            Path::new("a.js"),
            &[&rule()],
        );
        assert!(violations.is_empty());
    }
}
```

`lint_source` runs the full pipeline — parse, check, suppression filtering — so
these tests exercise the same path the CLI does.

Aim to cover, at minimum: a positive case with asserted line/col, a near-miss
that must not fire, and a suppression case.

Then verify:

```sh
cargo test
cargo clippy --all-targets
cargo build --release
./target/release/custom-biome-lint fixtures
```

The last command should now include your rule's findings from
`fixtures/no_console_log/invalid.js`. See [TESTING.md](TESTING.md).

## Checklist

- [ ] `src/rules/<rule_name>.rs` implements `Rule`
- [ ] Rule name is kebab-case and matches everywhere it appears
- [ ] Module declared and re-exported in `src/rules/mod.rs`
- [ ] Added to `RuleRegistry::with_all_rules` in `src/rules/registry.rs`
- [ ] `fixtures/<rule_name>/{valid,invalid,suppressed}.js` exist
- [ ] Test module added to `tests/integration.rs`
- [ ] Rule documented in [RULES.md](RULES.md), including any known limitations
- [ ] Decided whether the rule can attach a `Fix` (only if unambiguous — see
      Autofix above); either way, no silent guessing
- [ ] `cargo test` and `cargo clippy --all-targets` are clean

## Finding the right Biome AST types

The hardest part of writing a rule is knowing which `biome_js_syntax` type to
cast to. Useful approaches:

- **Read an existing rule.** `reselect_arity_match.rs` covers call expressions,
  callees and parameter lists. `no_arrow_function_create_selector.rs` covers
  arrow functions and walking up to a parent declarator.
  `no_native_map.rs` covers imports, `require`, and destructuring patterns.
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
