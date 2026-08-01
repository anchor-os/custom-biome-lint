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

/// A single rule violation at a 1-based line/column in one file.
#[derive(Debug, Clone)]
pub struct Violation {
    pub rule: &'static str,
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub severity: Severity,
}

impl Violation {
    pub fn error(rule: &'static str, line: usize, col: usize, message: impl Into<String>) -> Self {
        Self {
            rule,
            line,
            col,
            message: message.into(),
            severity: Severity::Error,
        }
    }

    pub fn warning(rule: &'static str, line: usize, col: usize, message: impl Into<String>) -> Self {
        Self {
            rule,
            line,
            col,
            message: message.into(),
            severity: Severity::Warning,
        }
    }

    pub fn position(&self) -> String {
        format!("{}:{}", self.line, self.col)
    }
}
