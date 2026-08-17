use std::io::Write;
use std::path::Path;

use custom_biome_lint::{
    fixer::{plan_file, Placement},
    lint_source, lint_source_enriched, RuleRegistry, RuleSeverity, Severity, Violation,
};

fn check_source(rule_name: &str, source: &str) -> Vec<Violation> {
    let registry = RuleRegistry::with_all_rules();
    let rules: Vec<&dyn custom_biome_lint::Rule> = registry
        .all()
        .into_iter()
        .filter(|rule| rule.name() == rule_name)
        .collect();
    assert_eq!(rules.len(), 1, "rule {rule_name} registered exactly once");
    lint_source_enriched(source, Path::new("demo.js"), &rules)
}

/// §3.1 — stable locations (startLine/startColumn) and span (end) for a rule
/// that tracks a contiguous range.
#[test]
fn enriched_diagnostics_have_stable_locations() {
    let violations = check_source("no-native-map", "const m = new Map();\n");
    assert_eq!(violations.len(), 1);
    let v = &violations[0];
    assert_eq!(v.line, 1);
    assert_eq!(v.col, 15);
    assert!(v.end.is_some(), "no-native-map tracks a range");
    let (end_line, end_col) = v.end.unwrap();
    assert_eq!(end_line, 1);
    assert_eq!(end_col, 18);
}

/// §3.1 — rules that point at a line only (no tracked span) must not emit
/// endLine/endColumn.
#[test]
fn line_only_rules_have_no_end() {
    let violations = check_source("no-for-statement", "for (;;) {}\n");
    assert_eq!(violations.len(), 1);
    let v = &violations[0];
    assert!(v.end.is_none(), "no-for-statement has no tracked span");
}

/// §3.3 — safe fixes are surfaced as structured suggestions reusing the same
/// byte ranges as `--auto-fix`.
#[test]
fn safe_fix_emitted_for_arrow_selector() {
    let source =
        "import { createSelector } from 'reselect';\nconst sel = () => createSelector(a, b);\n";
    let violations = check_source("no-arrow-function-create-selector", source);
    assert_eq!(violations.len(), 1);
    let v = &violations[0];
    assert_eq!(v.fixes.len(), 1, "exactly one safe fix");
    let fix = &v.fixes[0];
    assert_eq!(fix.kind, "safe");
    assert_eq!(fix.edits.len(), 1);
    let edit = &fix.edits[0];
    assert!(!edit.replacement.is_empty());
    assert_eq!((edit.start_line, edit.start_column), (2, 13));
    assert_eq!((edit.end_line, edit.end_column), (2, 39));
}

/// §3.3 — suppressions are surfaced as structured suggestions reusing the
/// same mechanism as `--write-fix`.
#[test]
fn suppression_suggestion_emitted() {
    let violations = check_source("no-native-map", "const m = new Map();\n");
    assert_eq!(violations.len(), 1);
    let v = &violations[0];
    assert_eq!(v.suppressions.len(), 1);
    let suppress = &v.suppressions[0];
    assert_eq!(suppress.kind, "suppress");
    assert_eq!(suppress.edits.len(), 1);
    let edit = &suppress.edits[0];
    assert!(edit.replacement.contains("custom-biome-ignore-line"));
    assert_eq!((edit.start_line, edit.start_column), (1, 21));
    assert_eq!((edit.end_line, edit.end_column), (1, 21));
}

/// §2 — `--rules` exposes every rule with stable, machine-readable metadata.
#[test]
fn rule_metadata_is_complete() {
    let registry = RuleRegistry::with_all_rules();
    let mut names: Vec<&str> = registry.all().iter().map(|r| r.name()).collect();
    names.sort();
    assert_eq!(names.len(), 11);
    for rule in registry.all() {
        assert!(!rule.name().is_empty());
        assert!(
            !rule.description().is_empty(),
            "{} needs a description",
            rule.name()
        );
        assert!(
            !rule.supported_extensions().is_empty(),
            "{} needs extensions",
            rule.name()
        );
        let _ = rule.default_severity();
    }
}

/// §2 / §4 — severity labels are stable strings an IDE can map to its own
/// diagnostic levels.
#[test]
fn rule_severity_labels_are_stable() {
    assert_eq!(RuleSeverity::Error.label(), "error");
    assert_eq!(RuleSeverity::Warn.label(), "warn");
    assert_eq!(RuleSeverity::Off.label(), "off");
    assert_eq!(Severity::Error.to_string(), "error");
    assert_eq!(Severity::Warning.to_string(), "warning");
}

/// §1 — the JSON protocol is versioned and every violation carries the stable
/// location fields an IDE needs, even when emitted through the CLI.
#[test]
fn cli_json_diagnostics_are_ide_ready() {
    let bin = env!("CARGO_BIN_EXE_custom-biome-lint");
    let tmp = std::env::temp_dir().join("cbl-ide-no-native-map.js");
    std::fs::write(&tmp, "const m = new Map();\n").unwrap();

    let output = std::process::Command::new(bin)
        .arg(tmp.to_str().unwrap())
        .arg("--format")
        .arg("json")
        .output()
        .expect("run binary");
    // A non-zero exit is expected here (the file has an error-level violation);
    // the JSON report is still emitted on stdout.
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["version"], 1);
    let file = &value["files"][0];
    let v = &file["violations"][0];
    assert_eq!(v["rule"], "no-native-map");
    assert_eq!(v["startLine"], 1);
    assert_eq!(v["startColumn"], 15);
    assert_eq!(v["endLine"], 1);
    assert_eq!(v["endColumn"], 18);
    assert_eq!(v["severity"], "error");
    assert!(v["suppressions"].as_array().unwrap().len() == 1);
}

/// §2 — `--rules` prints a versioned, structured rule catalog.
#[test]
fn cli_rules_emits_metadata() {
    let bin = env!("CARGO_BIN_EXE_custom-biome-lint");
    let output = std::process::Command::new(bin)
        .arg("--rules")
        .output()
        .expect("run binary");
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["version"], 1);
    let rules = value["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 11);
    let first = &rules[0];
    assert!(first["name"].is_string());
    assert!(first["description"].is_string());
    assert!(first["defaultSeverity"].is_string());
    assert!(first["enabledByDefault"].is_boolean());
    assert!(!first["supportedExtensions"].as_array().unwrap().is_empty());
}

/// §5 — single-file + stdin support produces diagnostics for an ad-hoc path.
#[test]
fn cli_stdin_emits_diagnostics() {
    let bin = env!("CARGO_BIN_EXE_custom-biome-lint");
    let tmp = std::env::temp_dir().join("cbl-ide-stdin.js");
    std::fs::write(&tmp, "const m = new Map();\n").unwrap();

    let mut child = std::process::Command::new(bin)
        .arg("--stdin")
        .arg(tmp.to_str().unwrap())
        .arg("--format")
        .arg("json")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn binary");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(b"const m = new Map();\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["files"][0]["violations"][0]["rule"], "no-native-map");
}

/// §8 — enrichment is additive: diagnostics still build when enrichment is off,
/// guaranteeing no regression for the existing `version:1` consumers.
#[test]
fn diagnostics_build_without_enrichment() {
    let registry = RuleRegistry::with_all_rules();
    let rules: Vec<&dyn custom_biome_lint::Rule> = registry
        .all()
        .into_iter()
        .filter(|r| r.name() == "no-native-map")
        .collect();
    let violations = lint_source("const m = new Map();\n", Path::new("demo.js"), &rules);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].fixes.is_empty());
    assert!(violations[0].suppressions.is_empty());
}

/// §3.3 — the suppression edit preserves the source line ending (CRLF), so an
/// IDE applying the edit matches what `--write-fix` writes to disk. A wide line
/// forces the own-line placement (trailing is refused by width).
#[test]
fn suppression_edit_preserves_crlf() {
    let source = format!("const x = {}; const m = new Map();\r\n", "a".repeat(100));
    let violation = Violation::error(
        "no-native-map",
        1,
        1,
        "Use Immutable.js Map instead of native Map.",
    );
    let plan = plan_file(Path::new("demo.js"), &source, &[violation]);
    let own_line = plan
        .changes
        .iter()
        .find(|c| matches!(c.placement, Placement::OwnLine))
        .expect("own-line suppression is planned for a too-wide line");
    assert!(
        own_line.insert_text.ends_with("\r\n"),
        "own-line suppression must preserve the source CRLF ending, got {:?}",
        own_line.insert_text
    );
}

// ---------------------------------------------------------------------------
// Helpers for applying the production edit objects back to source.
// ---------------------------------------------------------------------------

/// Byte offsets of each line's first byte (1-based: line N starts at index N-1).
fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// 1-based (line, startColumn, endColumn) of the first occurrence of `needle`,
/// assuming it sits on a single line. Columns are byte offsets, the span is
/// half-open (`endColumn` is one past the last byte).
fn locate(source: &str, needle: &str) -> (usize, usize, usize) {
    let idx = source.find(needle).expect("needle present in source");
    assert!(
        !needle.contains('\n'),
        "locate helper expects single-line needles"
    );
    let starts = line_starts(source);
    let line = starts.partition_point(|&s| s <= idx);
    let line_start = starts[line - 1];
    let start_col = idx - line_start + 1;
    let end_col = idx + needle.len() - line_start + 1;
    (line, start_col, end_col)
}

/// Applies production `Edit` objects (1-based line/byte-column, half-open span)
/// to `source` and returns the rewritten string. Edits are applied from the end
/// of the file backward so earlier offsets stay valid. Out-of-range edits are
/// rejected rather than silently corrupting the source.
fn apply_edits(source: &str, edits: &[custom_biome_lint::Edit]) -> Result<String, String> {
    let starts = line_starts(source);
    let mut spans: Vec<(usize, usize, String)> = Vec::with_capacity(edits.len());
    for e in edits {
        if e.start_line == 0 || e.start_line > starts.len() {
            return Err(format!("start line {} out of range", e.start_line));
        }
        if e.end_line == 0 || e.end_line > starts.len() {
            return Err(format!("end line {} out of range", e.end_line));
        }
        let s = starts[e.start_line - 1] + e.start_column.saturating_sub(1);
        let en = starts[e.end_line - 1] + e.end_column.saturating_sub(1);
        if s > source.len() || en > source.len() || s > en {
            return Err(format!("edit offset out of range: {s}..{en}"));
        }
        spans.push((s, en, e.replacement.clone()));
    }
    // Apply highest offset first so earlier edits keep their positions.
    spans.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut out = source.to_string();
    for (s, en, rep) in spans {
        out.replace_range(s..en, &rep);
    }
    Ok(out)
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_custom-biome-lint")
}

// ---------------------------------------------------------------------------
// §1 — existing API must keep working; enriched API is additive.
// ---------------------------------------------------------------------------

#[test]
fn existing_lint_source_api_unchanged() {
    let registry = RuleRegistry::with_all_rules();
    let rules: Vec<&dyn custom_biome_lint::Rule> = registry
        .all()
        .into_iter()
        .filter(|r| r.name() == "no-native-map")
        .collect();
    // Original three-argument signature, no enrich flag.
    let violations = lint_source("const m = new Map();\n", Path::new("demo.js"), &rules);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].fixes.is_empty());
    assert!(violations[0].suppressions.is_empty());
}

#[test]
fn enriched_api_returns_suggestions() {
    let violations = check_source("no-native-map", "const m = new Map();\n");
    assert_eq!(violations.len(), 1);
    assert!(!violations[0].suppressions.is_empty());
}

// ---------------------------------------------------------------------------
// §2 — byte-column coordinates with Unicode.
// ---------------------------------------------------------------------------

#[test]
fn unicode_cafe_byte_columns() {
    let source = "const café = new Map();\n";
    let (line, start_col, end_col) = locate(source, "Map");
    let v = &check_source("no-native-map", source)[0];
    assert_eq!(v.line, line);
    assert_eq!(
        v.col, start_col,
        "col must be a byte column, not char column"
    );
    assert_eq!((v.end.unwrap().0, v.end.unwrap().1), (line, end_col));
    // Sanity: the char column of `Map` would be 19, the byte column is 20.
    let char_col: usize = source[..source.find("Map").unwrap()].chars().count() + 1;
    assert_ne!(v.col, char_col, "col must NOT be a character column");
    assert_eq!(v.col, start_col);
}

#[test]
fn unicode_cjk_before_diagnostic() {
    let source = "const label = \"你好\";\nconst map = new Map();\n";
    let (line, start_col, end_col) = locate(source, "Map");
    assert_eq!(line, 2, "diagnostic is on the second line");
    let v = &check_source("no-native-map", source)[0];
    assert_eq!(v.line, 2);
    assert_eq!(v.col, start_col);
    assert_eq!((v.end.unwrap().0, v.end.unwrap().1), (2, end_col));
}

#[test]
fn unicode_inside_string_before_fix_range() {
    // The arrow violation sits after a CJK string; the safe-fix range must be
    // byte-accurate despite the multi-byte literal earlier in the file.
    let source = "const s = \"😀\";\nimport { createSelector } from 'reselect';\nconst sel = () => createSelector(a, b);\n";
    let v = &check_source("no-arrow-function-create-selector", source)[0];
    assert_eq!(v.fixes.len(), 1);
    let edit = &v.fixes[0].edits[0];
    // Apply exactly the production edit and confirm the source is valid.
    let rewritten = apply_edits(source, std::slice::from_ref(edit)).expect("edit applies");
    assert!(rewritten.contains("const sel = createSelector(a, b);"));
}

// ---------------------------------------------------------------------------
// §3 — end-to-end fix/suppression application.
// ---------------------------------------------------------------------------

#[test]
fn apply_safe_fix_removes_violation() {
    let source =
        "import { createSelector } from 'reselect';\nconst sel = () => createSelector(a, b);\n";
    let v = &check_source("no-arrow-function-create-selector", source)[0];
    let edit = &v.fixes[0].edits[0];
    let rewritten = apply_edits(source, std::slice::from_ref(edit)).expect("apply safe fix");
    assert_eq!(
        rewritten,
        "import { createSelector } from 'reselect';\nconst sel = createSelector(a, b);\n"
    );

    let re_violations = check_source("no-arrow-function-create-selector", &rewritten);
    assert!(
        re_violations.is_empty(),
        "no-arrow violation must be gone after applying the safe fix"
    );
}

#[test]
fn apply_suppression_removes_violation() {
    let source = "const m = new Map();\n";
    let v = &check_source("no-native-map", source)[0];
    let edit = &v.suppressions[0].edits[0];
    let rewritten = apply_edits(source, std::slice::from_ref(edit)).expect("apply suppression");

    let expected = "const m = new Map(); // custom-biome-ignore-line no-native-map\n";
    assert_eq!(
        rewritten, expected,
        "suppression edit must match --write-fix output"
    );

    let re_violations = check_source("no-native-map", &rewritten);
    assert!(
        re_violations.is_empty(),
        "no-native-map violation must be suppressed after applying the edit"
    );
}

#[test]
fn apply_multiple_suppression_edits() {
    let source = "const a = new Map();\nconst b = new Map();\n";
    let violations = check_source("no-native-map", source);
    assert_eq!(violations.len(), 2, "two violations, one per line");
    let mut edits = Vec::new();
    for v in &violations {
        edits.extend(v.suppressions.iter().flat_map(|s| s.edits.iter().cloned()));
    }
    assert_eq!(edits.len(), 2, "one suppression edit per violation line");
    let rewritten = apply_edits(source, &edits).expect("apply both edits");
    let re_violations = check_source("no-native-map", &rewritten);
    assert!(re_violations.is_empty(), "both violations suppressed");
}

#[test]
fn stale_edit_is_rejected() {
    let source = "const m = new Map();\n";
    let bad = custom_biome_lint::Edit {
        start_line: 99,
        start_column: 1,
        end_line: 99,
        end_column: 1,
        replacement: "x".into(),
    };
    assert!(
        apply_edits(source, &[bad]).is_err(),
        "out-of-range edit rejected"
    );
}

// ---------------------------------------------------------------------------
// §4 — suppression → violation association (line-scoped, unambiguous).
// ---------------------------------------------------------------------------

#[test]
fn multiple_same_rule_violations_same_line_associated() {
    // Two occurrences of the same rule on one line. A single line-scoped
    // suppression comment suppresses the whole line, so both violations must
    // receive the (identical) suppression edit, and applying it once clears
    // both.
    let source = "const a = new Map(); const b = new Map();\n";
    let violations = check_source("no-native-map", source);
    assert_eq!(violations.len(), 2);
    for v in &violations {
        assert_eq!(v.suppressions.len(), 1, "each violation gets a suppression");
    }
    let edit = violations[0].suppressions[0].edits[0].clone();
    let rewritten = apply_edits(source, std::slice::from_ref(&edit)).expect("apply suppression");
    assert!(
        check_source("no-native-map", &rewritten).is_empty(),
        "both same-line violations suppressed by one comment"
    );
}

// ---------------------------------------------------------------------------
// §5 — deterministic, complete rule metadata.
// ---------------------------------------------------------------------------

#[test]
fn cli_rules_deterministic_and_complete() {
    let run = || {
        let output = std::process::Command::new(bin())
            .arg("--rules")
            .output()
            .expect("run binary");
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("valid json")
    };
    let first = run();
    let second = run();
    assert_eq!(first, second, "--rules output must be deterministic");

    let rules = first["rules"].as_array().expect("rules array");
    assert_eq!(rules.len(), 11, "all 11 rules exposed");

    let names: Vec<&str> = rules.iter().map(|r| r["name"].as_str().unwrap()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "rules must be sorted by name");
    assert_eq!(
        names.len(),
        names.iter().collect::<std::collections::HashSet<_>>().len(),
        "rule IDs unique"
    );

    for r in rules {
        assert!(
            !r["description"].as_str().unwrap().is_empty(),
            "description present"
        );
        let sev = r["defaultSeverity"].as_str().unwrap();
        assert!(
            matches!(sev, "error" | "warn" | "off"),
            "valid severity: {sev}"
        );
        assert_eq!(
            r["enabledByDefault"].as_bool().unwrap(),
            sev != "off",
            "enabledByDefault matches defaultSeverity"
        );
    }
}

// ---------------------------------------------------------------------------
// §6 — JSON protocol: required fields always present, optional fields omitted.
// ---------------------------------------------------------------------------

#[test]
fn cli_json_omits_end_for_line_only_rule() {
    // `reselect-arity-match` is enabled by default and reports a line-only
    // diagnostic (no tracked span), so it must omit `endLine`/`endColumn`.
    let tmp = std::env::temp_dir().join("cbl-e2e-lineonly.js");
    std::fs::write(
        &tmp,
        "import { createSelector } from 'reselect';\nconst s = createSelector(a, b, c, (x) => x);\n",
    )
    .unwrap();
    let output = std::process::Command::new(bin())
        .arg(tmp.to_str().unwrap())
        .arg("--format")
        .arg("json")
        .output()
        .expect("run binary");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let file = &value["files"][0];
    let v = &file["violations"][0];
    assert_eq!(v["rule"], "reselect-arity-match");
    assert!(file.get("path").is_some(), "path present at file level");
    // Required per-violation fields are always present.
    for key in [
        "line",
        "col",
        "severity",
        "rule",
        "message",
        "startLine",
        "startColumn",
    ] {
        assert!(v.get(key).is_some(), "required field {key} present");
    }
    // Line-only rules omit the optional span.
    assert!(
        v.get("endLine").is_none(),
        "endLine omitted for line-only rule"
    );
    assert!(
        v.get("endColumn").is_none(),
        "endColumn omitted for line-only rule"
    );
}
