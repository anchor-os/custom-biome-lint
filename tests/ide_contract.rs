use std::io::Write;
use std::path::Path;

use custom_biome_lint::{
    fixer::{plan_file, Placement},
    lint_source, RuleRegistry, RuleSeverity, Severity, Violation,
};

fn check_source(rule_name: &str, source: &str) -> Vec<Violation> {
    let registry = RuleRegistry::with_all_rules();
    let rules: Vec<&dyn custom_biome_lint::Rule> = registry
        .all()
        .into_iter()
        .filter(|rule| rule.name() == rule_name)
        .collect();
    assert_eq!(rules.len(), 1, "rule {rule_name} registered exactly once");
    lint_source(source, Path::new("demo.js"), &rules, true)
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
    let violations = lint_source(
        "const m = new Map();\n",
        Path::new("demo.js"),
        &rules,
        false,
    );
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
