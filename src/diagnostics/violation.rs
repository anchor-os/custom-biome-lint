use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A machine-generated correction for one violation, expressed as a
/// byte-range replacement in the original source.
///
/// Only the rule that detected the violation is in a position to know
/// whether a correction is unambiguous, so a `Fix` is always produced by the
/// same `check()` call that produced the [`Violation`] it belongs to — never
/// reconstructed later from the violation's line/column alone. A rule with
/// no safe, single correction (e.g. one where the fix would have to guess at
/// missing arguments) simply leaves `Violation::fix` as `None`.
#[derive(Debug, Clone)]
pub struct Fix {
    /// Byte offset of the first byte replaced, in the original source.
    pub start: usize,
    /// Byte offset one past the last byte replaced.
    pub end: usize,
    pub replacement: String,
}

/// A single rule violation at a 1-based line/column in one file.
#[derive(Debug, Clone)]
pub struct Violation {
    pub rule: &'static str,
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub severity: Severity,
    /// Set only by rules that can produce one; see [`Fix`].
    pub fix: Option<Fix>,
}

impl Violation {
    pub fn error(rule: &'static str, line: usize, col: usize, message: impl Into<String>) -> Self {
        Self {
            rule,
            line,
            col,
            message: message.into(),
            severity: Severity::Error,
            fix: None,
        }
    }

    pub fn warning(
        rule: &'static str,
        line: usize,
        col: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule,
            line,
            col,
            message: message.into(),
            severity: Severity::Warning,
            fix: None,
        }
    }

    /// Attaches the rule-owned fix for this violation.
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }

    pub fn position(&self) -> String {
        format!("{}:{}", self.line, self.col)
    }
}
