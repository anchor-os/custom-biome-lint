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

/// A single text edit in 1-based line/column coordinates.
///
/// Both safe fixes ([`Violation::fixes`]) and suppression fixes
/// ([`Violation::suppressions`]) are expressed as a list of these so an IDE
/// adapter can apply them through its own editor API without re-implementing
/// any placement logic. A zero-width range (`start == end`) is an insertion.
///
/// Coordinate convention — matching the AST column this tool already reports
/// in [`Violation::col`]: lines and columns are 1-based, and a column counts
/// bytes from the start of the line (so for ASCII it equals the visible
/// character column; for non-ASCII source an adapter must convert). This is
/// intentional: the byte column keeps the contract consistent with `col`
/// rather than introducing a second, divergent column meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub replacement: String,
}

/// A machine-applicable correction or suppression offered for a violation.
///
/// Rendered by an IDE adapter as a Quick Fix. `kind` is `"safe"` for an
/// unambiguous code rewrite and `"suppress"` for a suppression-comment
/// insertion. The IDE contract never invents a fix: a rule either produced a
/// [`Fix`](Violation::fix) (which becomes a `"safe"` suggestion) or it did
/// not, and a suppression is only offered where the Rust tool itself can place
/// one.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub kind: &'static str,
    pub title: String,
    pub edits: Vec<Edit>,
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
    /// `end` of the violation's precise span, `(line, column)`, when the rule
    /// that detected it tracks one. Always the position reported in `line`/
    /// `col`; omitted from JSON when `None` so existing consumers keep working.
    pub end: Option<(usize, usize)>,
    /// Safe, deterministic code rewrites (see the module docs on [`Fix`]).
    /// Empty when the rule has no unambiguous fix for this violation.
    pub fixes: Vec<Suggestion>,
    /// Suppression-comment insertions the IDE can offer for this violation.
    /// Empty when the Rust tool cannot place a suppression here.
    pub suppressions: Vec<Suggestion>,
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
            end: None,
            fixes: Vec::new(),
            suppressions: Vec::new(),
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
            end: None,
            fixes: Vec::new(),
            suppressions: Vec::new(),
        }
    }

    /// Attaches the rule-owned fix for this violation.
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }

    /// Records the `(line, column)` end of the violation's precise span.
    pub fn with_end(mut self, end: (usize, usize)) -> Self {
        self.end = Some(end);
        self
    }

    pub fn position(&self) -> String {
        format!("{}:{}", self.line, self.col)
    }
}
