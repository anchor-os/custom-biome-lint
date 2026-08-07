use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

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

/// Machine-readable diagnostics for CI/tooling integration. The schema is a
/// stable, intentionally minimal contract — additive changes only, so
/// existing consumers keep working across tool versions.
///
/// ```json
/// {
///   "version": 1,
///   "files": [
///     {
///       "path": "src/a.js",
///       "violations": [
///         { "line": 1, "col": 1, "severity": "error", "rule": "no-native-map", "message": "..." }
///       ]
///     }
///   ],
///   "summary": {
///     "errors": 1, "warnings": 0, "filesWithViolations": 1,
///     "filesChecked": 9, "elapsedMs": 7, "clean": false
///   }
/// }
/// ```
pub fn format_reports_json(reports: &[FileReport], totals: &Totals) -> String {
    let files: Vec<_> = reports
        .iter()
        .filter(|r| !r.violations.is_empty())
        .map(|report| {
            let violations: Vec<_> = report
                .violations
                .iter()
                .map(|v| {
                    json!({
                        "line": v.line,
                        "col": v.col,
                        "severity": v.severity.label(),
                        "rule": v.rule,
                        "message": v.message,
                    })
                })
                .collect();
            json!({
                "path": report.path.display().to_string(),
                "violations": violations,
            })
        })
        .collect();

    let doc = json!({
        "version": 1,
        "files": files,
        "summary": {
            "errors": totals.errors,
            "warnings": totals.warnings,
            "filesWithViolations": totals.files_with_violations,
            "filesChecked": totals.files_checked,
            "elapsedMs": totals.elapsed.as_millis() as u64,
            "clean": totals.is_clean(),
        },
    });

    // A literal `json!` tree of owned values never fails to serialize.
    serde_json::to_string_pretty(&doc).expect("diagnostics JSON is always serializable")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_output_is_valid_and_carries_every_field() {
        let reports = vec![FileReport::new(
            PathBuf::from("src/a.js"),
            vec![Violation::error(
                "no-native-map",
                3,
                26,
                "Use Immutable.js Map instead of native Map.",
            )],
        )];
        let mut totals = tally(&reports, 1);
        totals.elapsed = Duration::from_millis(7);

        let json = format_reports_json(&reports, &totals);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["summary"]["errors"], 1);
        assert_eq!(parsed["summary"]["warnings"], 0);
        assert_eq!(parsed["summary"]["filesChecked"], 1);
        assert_eq!(parsed["summary"]["filesWithViolations"], 1);
        assert_eq!(parsed["summary"]["elapsedMs"], 7);
        assert_eq!(parsed["summary"]["clean"], false);

        let violation = &parsed["files"][0]["violations"][0];
        assert_eq!(parsed["files"][0]["path"], "src/a.js");
        assert_eq!(violation["line"], 3);
        assert_eq!(violation["col"], 26);
        assert_eq!(violation["severity"], "error");
        assert_eq!(violation["rule"], "no-native-map");
    }

    #[test]
    fn json_output_omits_files_with_no_violations() {
        let reports = vec![FileReport::new(PathBuf::from("src/clean.js"), vec![])];
        let totals = tally(&reports, 1);

        let json = format_reports_json(&reports, &totals);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["files"].as_array().unwrap().len(), 0);
        assert_eq!(parsed["summary"]["clean"], true);
    }

    #[test]
    fn json_output_is_stable_for_a_clean_run() {
        let totals = tally(&[], 0);
        let json = format_reports_json(&[], &totals);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["files"].as_array().unwrap().len(), 0);
        assert_eq!(parsed["summary"]["clean"], true);
    }
}
