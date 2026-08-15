//! Bans `while (...) { ... }` statements.
//!
//! Part of the loop-statement ban family (see
//! `docs/LOOP_STATEMENT_BAN_RULES_PLAN.md`). The old ESLint `no-restricted-syntax`
//! config banned `WhileStatement` — alongside `ForStatement` and
//! `DoWhileStatement` — as a "no loops, use functional iteration" house style.
//! Biome has no equivalent, so this rule fills the gap. It ships off by default:
//! banning loops is a house style, not a universal correctness fix, so a
//! consuming repo must opt in.
//!
//! Detection is a pure cast-and-report walk — no semantic model, no scope walk,
//! and no autofix: there is no mechanical rewrite for a loop, only a human can
//! decide what functional form replaces it.

use biome_js_syntax::JsWhileStatement;
use biome_rowan::AstNode;

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;

const MESSAGE: &str =
    "Avoid `while` loops — use functional iteration (e.g. `Array.prototype` methods) instead.";

/// Reports `while` loop statements (existence ban, ships off by default).
pub struct NoWhileStatement;

impl Rule for NoWhileStatement {
    fn name(&self) -> &'static str {
        "no-while-statement"
    }

    fn description(&self) -> &'static str {
        "Disallow `while` loop statements"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS
    }

    fn default_severity(&self) -> crate::config::RuleSeverity {
        crate::config::RuleSeverity::Off
    }

    /// Walks the parsed tree and reports every `JsWhileStatement`.
    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let mut violations = Vec::new();
        for node in file.tree().descendants() {
            let Some(stmt) = JsWhileStatement::cast_ref(&node) else {
                continue;
            };
            let offset = usize::from(stmt.syntax().text_trimmed_range().start());
            let (line, col) = file.line_col(offset);
            violations.push(Violation::error(self.name(), line, col, MESSAGE));
        }
        violations
    }
}
