use std::path::PathBuf;
use std::time::Duration;

use super::violation::{Severity, Violation};

/// All violations found in one file, sorted by position.
#[derive(Debug, Clone)]
pub struct FileReport {
    pub path: PathBuf,
    pub violations: Vec<Violation>,
}

impl FileReport {
    pub fn new(path: PathBuf, violations: Vec<Violation>) -> Self {
        Self { path, violations }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub errors: usize,
    pub warnings: usize,
    pub files_with_violations: usize,
    pub files_checked: usize,
    /// Wall time for the whole run. `tally` leaves this zero; the CLI fills it
    /// in before printing.
    pub elapsed: Duration,
}

impl Totals {
    pub fn is_clean(&self) -> bool {
        self.errors == 0 && self.warnings == 0
    }
}

pub fn tally(reports: &[FileReport], files_checked: usize) -> Totals {
    let mut totals = Totals {
        files_checked,
        ..Totals::default()
    };
    for report in reports {
        if report.violations.is_empty() {
            continue;
        }
        totals.files_with_violations += 1;
        for violation in &report.violations {
            match violation.severity {
                Severity::Error => totals.errors += 1,
                Severity::Warning => totals.warnings += 1,
            }
        }
    }
    totals
}

/// ESLint-style report: a path header, then aligned `line:col  severity  message  rule`
/// rows, then a one-line summary.
pub fn format_reports(reports: &[FileReport], totals: &Totals) -> String {
    let mut out = String::new();

    for report in reports.iter().filter(|r| !r.violations.is_empty()) {
        out.push_str(&report.path.display().to_string());
        out.push('\n');

        let pos_width = report
            .violations
            .iter()
            .map(|v| v.position().len())
            .max()
            .unwrap_or(0);
        let sev_width = report
            .violations
            .iter()
            .map(|v| v.severity.label().len())
            .max()
            .unwrap_or(0);
        let msg_width = report
            .violations
            .iter()
            .map(|v| v.message.len())
            .max()
            .unwrap_or(0);

        for violation in &report.violations {
            out.push_str(&format!(
                "  {:<pos$}  {:<sev$}  {:<msg$}  {}\n",
                violation.position(),
                violation.severity.label(),
                violation.message,
                violation.rule,
                pos = pos_width,
                sev = sev_width,
                msg = msg_width,
            ));
        }
        out.push('\n');
    }

    out.push_str(&format_summary(totals));
    out.push('\n');
    out
}

pub fn format_summary(totals: &Totals) -> String {
    if totals.is_clean() {
        return format!(
            "\u{2714} No violations found ({} file{} checked in {})",
            totals.files_checked,
            plural(totals.files_checked),
            format_elapsed(totals.elapsed)
        );
    }

    let mut counts = Vec::new();
    if totals.errors > 0 {
        counts.push(format!("{} error{}", totals.errors, plural(totals.errors)));
    }
    if totals.warnings > 0 {
        counts.push(format!(
            "{} warning{}",
            totals.warnings,
            plural(totals.warnings)
        ));
    }

    format!(
        "\u{2716} {} in {} file{} ({} file{} checked in {})",
        counts.join(", "),
        totals.files_with_violations,
        plural(totals.files_with_violations),
        totals.files_checked,
        plural(totals.files_checked),
        format_elapsed(totals.elapsed)
    )
}

/// Sub-second runs read better in whole milliseconds than as "0.23s".
fn format_elapsed(elapsed: Duration) -> String {
    if elapsed.as_secs() == 0 {
        format!("{}ms", elapsed.as_millis())
    } else {
        format!("{:.2}s", elapsed.as_secs_f64())
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
