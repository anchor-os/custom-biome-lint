use std::path::{Path, PathBuf};

use custom_biome_lint::{lint_source, PackageConfig, Rule, RuleRegistry, RuleSeverity, Violation};

fn fixture(rule_dir: &str, name: &str) -> (PathBuf, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(rule_dir)
        .join(name);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()));
    (path, source)
}

/// Runs only the named rule against `source`, so one rule's expectations cannot
/// be polluted by another rule firing on the same file.
fn check_source(rule_name: &str, source: &str, path: &Path) -> Vec<Violation> {
    let registry = RuleRegistry::with_all_rules();
    let rules: Vec<&dyn Rule> = registry
        .all()
        .into_iter()
        .filter(|rule| rule.name() == rule_name)
        .collect();
    assert_eq!(
        rules.len(),
        1,
        "rule {rule_name} is not registered exactly once"
    );
    lint_source(source, path, &rules)
}

fn check_one(rule_name: &str, rule_dir: &str, fixture_name: &str) -> Vec<Violation> {
    let (path, source) = fixture(rule_dir, fixture_name);
    check_source(rule_name, &source, &path)
}

#[test]
fn registry_exposes_all_three_rules() {
    let registry = RuleRegistry::with_all_rules();
    let mut names: Vec<&str> = registry.all().iter().map(|rule| rule.name()).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "no-arrow-function-create-selector",
            "no-native-map",
            "reselect-arity-match"
        ]
    );
}

#[test]
fn default_pattern_covers_js_and_jsx() {
    let registry = RuleRegistry::with_all_rules();
    assert_eq!(registry.default_pattern(), "src/**/*.{js,jsx}");
}

#[test]
fn every_rule_has_fixtures_for_all_three_cases() {
    let registry = RuleRegistry::with_all_rules();
    for rule in registry.all() {
        let dir = rule.name().replace('-', "_");
        for case in ["valid.js", "invalid.js", "suppressed.js"] {
            let (_, source) = fixture(&dir, case);
            assert!(!source.trim().is_empty(), "{dir}/{case} is empty");
        }
    }
}

mod no_native_map {
    use super::*;

    #[test]
    fn immutable_map_import_is_allowed() {
        assert!(check_one("no-native-map", "no_native_map", "valid.js").is_empty());
    }

    #[test]
    fn native_map_is_reported() {
        let violations = check_one("no-native-map", "no_native_map", "invalid.js");
        assert_eq!(violations.len(), 2, "got {violations:?}");
        assert!(violations
            .iter()
            .all(|v| v.rule == "no-native-map" && v.message.contains("Immutable.js Map")));
    }

    #[test]
    fn suppression_comments_silence_the_rule() {
        assert!(check_one("no-native-map", "no_native_map", "suppressed.js").is_empty());
    }

    #[test]
    fn namespace_import_with_destructured_map_is_allowed() {
        let source =
            "import Immutable from 'immutable';\nconst { Map } = Immutable;\nconst m = Map();\n";
        assert!(check_source("no-native-map", source, Path::new("a.js")).is_empty());
    }

    #[test]
    fn require_alias_with_member_access_is_allowed() {
        let source =
            "const Immutable = require('immutable');\nconst ImmutableMap = Immutable.Map;\n";
        assert!(check_source("no-native-map", source, Path::new("a.js")).is_empty());
    }

    #[test]
    fn map_imported_from_another_module_is_still_reported() {
        let source = "import { Map } from 'not-immutable';\nconst m = new Map();\n";
        let violations = check_source("no-native-map", source, Path::new("a.js"));
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert_eq!(
            violations[0].line, 2,
            "the import specifier itself is exempt"
        );
    }
}

mod no_arrow_function_create_selector {
    use super::*;

    const RULE: &str = "no-arrow-function-create-selector";
    const DIR: &str = "no_arrow_function_create_selector";

    #[test]
    fn direct_and_factory_forms_are_allowed() {
        assert!(check_one(RULE, DIR, "valid.js").is_empty());
    }

    #[test]
    fn wrapped_create_selector_is_reported() {
        let violations = check_one(RULE, DIR, "invalid.js");
        assert_eq!(violations.len(), 2, "got {violations:?}");
        assert!(violations
            .iter()
            .all(|v| v.message.contains("breaks memoization")));
    }

    #[test]
    fn suppression_comments_silence_the_rule() {
        assert!(check_one(RULE, DIR, "suppressed.js").is_empty());
    }
}

mod reselect_arity_match {
    use super::*;

    const RULE: &str = "reselect-arity-match";
    const DIR: &str = "reselect_arity_match";

    #[test]
    fn matching_arity_is_allowed() {
        assert!(check_one(RULE, DIR, "valid.js").is_empty());
    }

    #[test]
    fn mismatched_arity_is_reported() {
        let violations = check_one(RULE, DIR, "invalid.js");
        assert_eq!(violations.len(), 3, "got {violations:?}");
        assert!(violations[0].message.contains("expects 2 parameter(s)"));
        assert!(violations[0].message.contains("but found 1"));
    }

    #[test]
    fn suppression_comments_silence_the_rule() {
        assert!(check_one(RULE, DIR, "suppressed.js").is_empty());
    }

    #[test]
    fn member_expression_callee_is_checked() {
        let source = "Reselect.createSelector(a, b, x => x);\n";
        assert_eq!(check_source(RULE, source, Path::new("a.js")).len(), 1);
    }
}

mod config {
    use super::*;

    #[test]
    fn missing_package_json_enables_everything() {
        let config = PackageConfig::default();
        let registry = RuleRegistry::with_all_rules();
        assert_eq!(registry.enabled(&config).len(), registry.len());
    }

    #[test]
    fn ignored_rules_are_filtered_out() {
        let mut config = PackageConfig::default();
        config
            .severities
            .insert("no-native-map".to_string(), RuleSeverity::Off);
        let registry = RuleRegistry::with_all_rules();
        let enabled: Vec<&str> = registry
            .enabled(&config)
            .iter()
            .map(|rule| rule.name())
            .collect();
        assert!(!enabled.contains(&"no-native-map"));
        assert_eq!(enabled.len(), registry.len() - 1);
    }

    #[test]
    fn warn_severity_does_not_disable_the_rule() {
        let mut config = PackageConfig::default();
        config
            .severities
            .insert("no-native-map".to_string(), RuleSeverity::Warn);
        let registry = RuleRegistry::with_all_rules();
        let enabled: Vec<&str> = registry
            .enabled(&config)
            .iter()
            .map(|rule| rule.name())
            .collect();
        assert!(enabled.contains(&"no-native-map"));
        assert_eq!(
            config.severity_override("no-native-map"),
            Some(RuleSeverity::Warn)
        );
    }
}

mod patterns {
    use custom_biome_lint::{discover_files, resolve_pattern, GlobSet};
    use std::path::Path;

    fn manifest_dir() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn bare_directory_expands_to_a_brace_glob() {
        let pattern = resolve_pattern("fixtures", manifest_dir(), "js,jsx");
        assert_eq!(pattern.raw(), "fixtures/**/*.{js,jsx}");
        let mut alternatives = pattern.alternatives().to_vec();
        alternatives.sort();
        assert_eq!(
            alternatives,
            vec!["fixtures/**/*.js", "fixtures/**/*.jsx"],
            "the brace group must survive expansion"
        );
    }

    #[test]
    fn bare_directory_discovers_every_fixture() {
        let pattern = resolve_pattern("fixtures", manifest_dir(), "js,jsx");
        let discovered = discover_files(&pattern, manifest_dir());
        assert_eq!(
            discovered.files.len(),
            9,
            "expected 3 fixtures for each of 3 rules, got {:?}",
            discovered.files
        );
    }

    #[test]
    fn explicit_glob_is_passed_through_unchanged() {
        let pattern = resolve_pattern("fixtures/**/invalid.js", manifest_dir(), "js,jsx");
        assert_eq!(pattern.raw(), "fixtures/**/invalid.js");
        assert_eq!(discover_files(&pattern, manifest_dir()).files.len(), 3);
    }

    #[test]
    fn node_modules_is_never_walked() {
        let pattern = GlobSet::new("**/*.js");
        let discovered = discover_files(&pattern, manifest_dir());
        assert!(discovered
            .files
            .iter()
            .all(|path| !path.to_string_lossy().contains("node_modules")));
    }
}

mod extensions {
    use super::*;

    #[test]
    fn unsupported_extension_yields_no_violations() {
        let registry = RuleRegistry::with_all_rules();
        let rules = registry.all();
        let source = "const cache = new Map();\n";
        assert!(lint_source(source, Path::new("a.ts"), &rules).is_empty());
        assert!(!lint_source(source, Path::new("a.js"), &rules).is_empty());
    }
}
