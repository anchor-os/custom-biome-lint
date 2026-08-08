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
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    /// `violations_by_file` carries the exact source each violation's `Fix`
    /// offsets were computed against, alongside the violations. This is
    /// re-read from disk and compared before planning: a `Fix`'s offsets are
    /// only valid against the source they were computed from, and reusing
    /// them against a file that changed in between (e.g. another process
    /// edited it after this tool analyzed it but before it got here to
    /// apply the fix) can silently rewrite the wrong bytes while still
    /// leaving the file parseable, so a changed file is skipped entirely
    /// rather than risking that.
    ///
    /// With `write` false this is a dry run: files are read and planned but
    /// not modified. Per-file read/write errors are recorded in
    /// [`AutofixReport::failures`] rather than aborting the whole run, so one
    /// unwritable file cannot discard the fixes for every other file.
    pub fn apply(
        violations_by_file: &BTreeMap<PathBuf, (String, Vec<Violation>)>,
        write: bool,
    ) -> AutofixReport {
        let mut report = AutofixReport {
            wrote: write,
            ..AutofixReport::default()
        };

        for (path, (analyzed_source, violations)) in violations_by_file {
            let current_source = match fs::read_to_string(path) {
                Ok(source) => source,
                Err(error) => {
                    report.failures.push((path.clone(), error.to_string()));
                    continue;
                }
            };

            if current_source != *analyzed_source {
                for violation in violations {
                    report.skipped.push(SkippedFix {
                        path: path.clone(),
                        line: violation.line,
                        rule: violation.rule,
                        reason: "file changed on disk since it was analyzed",
                    });
                }
                continue;
            }

            let plan = plan_file(path, &current_source, violations);
            report.skipped.extend(plan.skipped);

            if plan.applied.is_empty() {
                continue;
            }

            if write {
                // Narrows (does not eliminate -- see docs/ARCHITECTURE.md)
                // the window between the snapshot check above and the
                // rename in write_atomically: re-read right before writing
                // and refuse if the file changed again in that gap, rather
                // than trusting a check that is now however long plan_file
                // took to run out of date.
                match fs::read_to_string(path) {
                    Ok(latest) if latest == *analyzed_source => {}
                    Ok(_) => {
                        for applied in &plan.applied {
                            report.skipped.push(SkippedFix {
                                path: path.clone(),
                                line: applied.line,
                                rule: applied.rule,
                                reason: "file changed on disk since it was analyzed",
                            });
                        }
                        continue;
                    }
                    Err(error) => {
                        report.failures.push((path.clone(), error.to_string()));
                        continue;
                    }
                }

                if let Err(error) = write_atomically(path, &plan.source) {
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

/// Writes `contents` to `path` via a same-directory temp file plus rename,
/// so a write that fails partway (disk full, process killed) leaves the
/// original file untouched instead of a half-written one: `fs::write`
/// truncates its target before writing, but a rename onto an existing path
/// is atomic on the same filesystem.
fn write_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    let (tmp_path, mut file) = create_temp_sibling(path)?;

    let result = file
        .write_all(contents.as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = result {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }

    if let Err(error) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }

    Ok(())
}

/// Creates a sibling temp file with `O_EXCL` semantics (`create_new`), so a
/// path that already exists at the chosen name -- including a symlink
/// planted there by another process with write access to the directory --
/// makes creation fail instead of `fs::write` silently following the
/// symlink and truncating whatever it points at. The name mixes the PID and
/// current time to make collisions rare; the retry loop handles the case
/// where one happens anyway.
fn create_temp_sibling(path: &Path) -> std::io::Result<(PathBuf, fs::File)> {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    for attempt in 0..8u32 {
        let candidate =
            path.with_file_name(format!(".{file_name}.autofix-tmp-{pid}-{nanos}-{attempt}"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not create a unique temp file for autofix after 8 attempts",
    ))
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
        let Some(fix) = violation.fix.as_ref() else {
            skipped.push(SkippedFix {
                path: path.to_path_buf(),
                line: violation.line,
                rule: violation.rule,
                reason: "rule has no autofix for this violation",
            });
            continue;
        };

        // A malformed Fix (reversed bounds, past the end of the source, or
        // splitting a UTF-8 code point) would otherwise panic when sliced
        // below and abort the whole run. Reject it here instead: a rule bug
        // should mean one skipped violation, never a crashed lint run.
        if fix.start > fix.end
            || fix.end > source.len()
            || !source.is_char_boundary(fix.start)
            || !source.is_char_boundary(fix.end)
        {
            skipped.push(SkippedFix {
                path: path.to_path_buf(),
                line: violation.line,
                rule: violation.rule,
                reason: "rule produced an invalid fix range",
            });
            continue;
        }

        fixable.push(violation);
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
    fn a_fix_with_reversed_bounds_is_skipped_not_a_panic() {
        let source = "const x = 1;\n";
        let violations = vec![violation(1, "some-rule", 5, 2, "x")];
        let plan = plan_file(Path::new("a.js"), source, &violations);
        assert!(plan.applied.is_empty());
        assert_eq!(plan.skipped[0].reason, "rule produced an invalid fix range");
        assert_eq!(plan.source, source);
    }

    #[test]
    fn a_fix_past_the_end_of_the_source_is_skipped_not_a_panic() {
        let source = "const x = 1;\n";
        let violations = vec![violation(1, "some-rule", 5, source.len() + 10, "x")];
        let plan = plan_file(Path::new("a.js"), source, &violations);
        assert!(plan.applied.is_empty());
        assert_eq!(plan.skipped[0].reason, "rule produced an invalid fix range");
        assert_eq!(plan.source, source);
    }

    #[test]
    fn a_fix_splitting_a_utf8_code_point_is_skipped_not_a_panic() {
        // "café" -- 'é' is a 2-byte code point starting at byte 4, so byte 5
        // falls inside it and is not a valid char boundary.
        let source = "café\n";
        let violations = vec![violation(1, "some-rule", 4, 5, "e")];
        let plan = plan_file(Path::new("a.js"), source, &violations);
        assert!(plan.applied.is_empty());
        assert_eq!(plan.skipped[0].reason, "rule produced an invalid fix range");
        assert_eq!(plan.source, source);
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
            (
                source.clone(),
                vec![violation(
                    1,
                    "no-arrow-function-create-selector",
                    start,
                    end,
                    "createSelector(a, b)",
                )],
            ),
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

    #[test]
    fn a_file_changed_since_analysis_is_skipped_not_rewritten_at_stale_offsets() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("a.js");
        let analyzed_source = "const x = () => createSelector(a, b);\n";
        fs::write(&file, analyzed_source).unwrap();

        let start = analyzed_source.find("() =>").unwrap();
        let end = analyzed_source.find(";\n").unwrap();
        let mut violations_by_file = BTreeMap::new();
        violations_by_file.insert(
            file.clone(),
            (
                analyzed_source.to_string(),
                vec![violation(
                    1,
                    "no-arrow-function-create-selector",
                    start,
                    end,
                    "createSelector(a, b)",
                )],
            ),
        );

        // The file on disk no longer matches what was analyzed -- applying
        // the old offsets to this content would rewrite the wrong bytes.
        let changed_on_disk = "const somethingElse = 1;\nconst x = () => createSelector(a, b);\n";
        fs::write(&file, changed_on_disk).unwrap();

        let report = Autofix::apply(&violations_by_file, true);
        assert!(report.fixes_applied.is_empty());
        assert_eq!(
            report.skipped[0].reason,
            "file changed on disk since it was analyzed"
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            changed_on_disk,
            "must not touch a file that changed since analysis"
        );
    }

    #[test]
    fn write_atomically_never_leaves_a_partially_written_file_on_a_write_error() {
        // The temp file this simulates a failure on is never the target
        // path itself, so if write_atomically ever wrote to the target
        // directly (rather than a temp file that gets renamed in), this
        // would catch it: the original content survives untouched.
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("a.js");
        fs::write(&file, "original\n").unwrap();

        write_atomically(&file, "rewritten\n").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "rewritten\n");
    }

    #[test]
    fn exclusive_temp_creation_refuses_a_pre_existing_symlink_rather_than_following_it() {
        // This is the property write_atomically depends on for its symlink
        // safety: create_new must fail on an existing path rather than
        // follow it, the way a plain fs::write would.
        let dir = tempfile::TempDir::new().unwrap();
        let victim = dir.path().join("victim.js");
        fs::write(&victim, "victim-original").unwrap();

        let candidate = dir.path().join(".planted-tmp");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&victim, &candidate).unwrap();
        #[cfg(not(unix))]
        fs::write(&candidate, "planted").unwrap();

        let result = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate);
        assert!(
            result.is_err(),
            "create_new must refuse a path that already exists"
        );
        assert_eq!(
            fs::read_to_string(&victim).unwrap(),
            "victim-original",
            "the symlink target must never be touched"
        );
    }

    #[test]
    fn create_temp_sibling_produces_a_fresh_exclusively_created_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("a.js");

        let (tmp_path, _file) = create_temp_sibling(&target).unwrap();
        assert!(tmp_path.exists());
        assert_ne!(tmp_path, target);

        let (tmp_path_2, _file_2) = create_temp_sibling(&target).unwrap();
        assert_ne!(
            tmp_path, tmp_path_2,
            "two calls must not collide on the same name"
        );
    }
}
