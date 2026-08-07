//! Applies rule-owned fixes (see [`crate::diagnostics::Fix`]) directly to
//! source files.
//!
//! This is a different mechanism from [`crate::fixer`]: that module silences
//! a violation by adding a suppression comment around it, without touching
//! the flagged code. This module rewrites the flagged code itself, using the
//! exact byte-range replacement the rule that detected the violation
//! produced. A violation whose rule did not produce one is left unfixed and
//! reported as such, rather than guessed at here.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;

/// One fix applied (or, in a dry run, that would be applied).
#[derive(Debug, Clone)]
pub struct AppliedFix {
    pub path: PathBuf,
    pub line: usize,
    pub rule: &'static str,
    pub replacement: String,
}

/// A violation left unfixed, and why.
#[derive(Debug, Clone)]
pub struct SkippedFix {
    pub path: PathBuf,
    pub line: usize,
    pub rule: &'static str,
    pub reason: &'static str,
}

#[derive(Debug, Default)]
pub struct AutofixReport {
    pub files_modified: usize,
    pub fixes_applied: Vec<AppliedFix>,
    pub skipped: Vec<SkippedFix>,
    /// Files that could not be read or written, with the reason.
    pub failures: Vec<(PathBuf, String)>,
    /// False for a dry run, in which case nothing was written to disk.
    pub wrote: bool,
}

impl AutofixReport {
    /// True when every violation was fixed and no file failed.
    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty() && self.failures.is_empty()
    }
}

/// The rewritten source for one file, plus what it changed.
struct FilePlan {
    source: String,
    applied: Vec<AppliedFix>,
    skipped: Vec<SkippedFix>,
}

pub struct Autofix;

impl Autofix {
    /// Applies every fix rules produced for `violations_by_file`.
    ///
    /// With `write` false this is a dry run: files are read and planned but
    /// not modified. Per-file read/write errors are recorded in
    /// [`AutofixReport::failures`] rather than aborting the whole run, so one
    /// unwritable file cannot discard the fixes for every other file.
    pub fn apply(
        violations_by_file: &BTreeMap<PathBuf, Vec<Violation>>,
        write: bool,
    ) -> AutofixReport {
        let mut report = AutofixReport {
            wrote: write,
            ..AutofixReport::default()
        };

        for (path, violations) in violations_by_file {
            let source = match fs::read_to_string(path) {
                Ok(source) => source,
                Err(error) => {
                    report.failures.push((path.clone(), error.to_string()));
                    continue;
                }
            };

            let plan = plan_file(path, &source, violations);
            report.skipped.extend(plan.skipped);

            if plan.applied.is_empty() {
                continue;
            }

            if write {
                if let Err(error) = fs::write(path, &plan.source) {
                    report.failures.push((path.clone(), error.to_string()));
                    continue;
                }
            }

            report.files_modified += 1;
            report.fixes_applied.extend(plan.applied);
        }

        report
    }
}

/// Plans the fixes for a single file without touching disk.
fn plan_file(path: &Path, source: &str, violations: &[Violation]) -> FilePlan {
    let unchanged = |skipped| FilePlan {
        source: source.to_string(),
        applied: Vec::new(),
        skipped,
    };

    let mut fixable: Vec<&Violation> = Vec::new();
    let mut skipped = Vec::new();

    for violation in violations {
        if violation.fix.is_some() {
            fixable.push(violation);
        } else {
            skipped.push(SkippedFix {
                path: path.to_path_buf(),
                line: violation.line,
                rule: violation.rule,
                reason: "rule has no autofix for this violation",
            });
        }
    }

    if fixable.is_empty() {
        return unchanged(skipped);
    }

    // Sorted by start offset so overlap detection and left-to-right splicing
    // both just walk the list once.
    fixable.sort_by_key(|violation| violation.fix.as_ref().unwrap().start);

    let mut accepted: Vec<&Violation> = Vec::new();
    let mut cursor = 0usize;
    for violation in fixable {
        let fix = violation.fix.as_ref().unwrap();
        if fix.start < cursor {
            // Two rules (or two violations of the same rule) both want to
            // rewrite overlapping code; applying both would corrupt the file,
            // so only the first is kept and the rest wait for the next run.
            skipped.push(SkippedFix {
                path: path.to_path_buf(),
                line: violation.line,
                rule: violation.rule,
                reason: "overlaps another fix in this run",
            });
            continue;
        }
        cursor = fix.end;
        accepted.push(violation);
    }

    if accepted.is_empty() {
        return unchanged(skipped);
    }

    let mut rewritten = String::with_capacity(source.len());
    let mut at = 0usize;
    let mut applied = Vec::with_capacity(accepted.len());
    for violation in &accepted {
        let fix = violation.fix.as_ref().unwrap();
        rewritten.push_str(&source[at..fix.start]);
        rewritten.push_str(&fix.replacement);
        at = fix.end;
        applied.push(AppliedFix {
            path: path.to_path_buf(),
            line: violation.line,
            rule: violation.rule,
            replacement: fix.replacement.clone(),
        });
    }
    rewritten.push_str(&source[at..]);

    // Cheap insurance against a fix silently producing a file that no longer
    // parses -- never write out something worse than what was there.
    if !FileContext::parse(&rewritten, path).parsed_cleanly() {
        let mut all_unfixable: Vec<SkippedFix> = accepted
            .iter()
            .map(|violation| SkippedFix {
                path: path.to_path_buf(),
                line: violation.line,
                rule: violation.rule,
                reason: "applying the fix would leave the file unparseable",
            })
            .collect();
        all_unfixable.extend(skipped);
        return unchanged(all_unfixable);
    }

    FilePlan {
        source: rewritten,
        applied,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Fix;

    fn violation(
        line: usize,
        rule: &'static str,
        start: usize,
        end: usize,
        replacement: &str,
    ) -> Violation {
        Violation::error(rule, line, 1, "test").with_fix(Fix {
            start,
            end,
            replacement: replacement.to_string(),
        })
    }

    #[test]
    fn a_single_fix_is_applied() {
        let source = "const x = () => createSelector(a, b);\n";
        let start = source.find("() =>").unwrap();
        let end = source.find(";\n").unwrap();
        let violations = vec![violation(
            1,
            "no-arrow-function-create-selector",
            start,
            end,
            "createSelector(a, b)",
        )];
        let plan = plan_file(Path::new("a.js"), source, &violations);
        assert_eq!(plan.source, "const x = createSelector(a, b);\n");
        assert_eq!(plan.applied.len(), 1);
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn a_violation_with_no_fix_is_skipped_not_dropped() {
        let violations = vec![Violation::error("reselect-arity-match", 1, 1, "mismatch")];
        let plan = plan_file(
            Path::new("a.js"),
            "createSelector(a, b, x => x);\n",
            &violations,
        );
        assert!(plan.applied.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(
            plan.skipped[0].reason,
            "rule has no autofix for this violation"
        );
    }

    #[test]
    fn overlapping_fixes_only_apply_the_first() {
        let source = "abcdef\n";
        let violations = vec![
            violation(1, "rule-a", 0, 3, "XXX"),
            violation(1, "rule-b", 2, 5, "YYY"),
        ];
        let plan = plan_file(Path::new("a.js"), source, &violations);
        assert_eq!(plan.applied.len(), 1);
        assert_eq!(plan.applied[0].rule, "rule-a");
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].reason, "overlaps another fix in this run");
        assert_eq!(plan.source, "XXXdef\n");
    }

    #[test]
    fn a_fix_that_would_break_parsing_is_rejected() {
        let source = "const x = 1;\n";
        let violations = vec![violation(1, "some-rule", 10, 11, "=")];
        let plan = plan_file(Path::new("a.js"), source, &violations);
        assert!(plan.applied.is_empty());
        assert_eq!(
            plan.skipped[0].reason,
            "applying the fix would leave the file unparseable"
        );
        assert_eq!(plan.source, source, "must not write out the broken rewrite");
    }

    #[test]
    fn apply_reports_files_modified_and_respects_dry_run() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("a.js");
        fs::write(&file, "const x = () => createSelector(a, b);\n").unwrap();

        let mut violations_by_file = BTreeMap::new();
        let source = fs::read_to_string(&file).unwrap();
        let start = source.find("() =>").unwrap();
        let end = source.find(";\n").unwrap();
        violations_by_file.insert(
            file.clone(),
            vec![violation(
                1,
                "no-arrow-function-create-selector",
                start,
                end,
                "createSelector(a, b)",
            )],
        );

        let dry = Autofix::apply(&violations_by_file, false);
        assert_eq!(dry.files_modified, 1);
        assert!(!dry.wrote);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            source,
            "dry run must not touch disk"
        );

        let real = Autofix::apply(&violations_by_file, true);
        assert!(real.wrote);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "const x = createSelector(a, b);\n"
        );
    }
}
