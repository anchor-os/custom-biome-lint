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
fn every_rule_has_fixtures_for_all_four_cases() {
    let registry = RuleRegistry::with_all_rules();
    for rule in registry.all() {
        let dir = rule.name().replace('-', "_");
        for case in ["valid.js", "invalid.js", "suppressed.js", "edge-cases.js"] {
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

    /// Locks in two documented, deliberate quirks (see RULES.md): the
    /// mapboxgl.Map false positive, and that a shadowed `Map` parameter's
    /// own declaration is never flagged even though its use still resolves
    /// to native Map (there is nothing for it to shadow in this file).
    #[test]
    fn edge_cases_produce_exactly_the_documented_violations() {
        let violations = check_one("no-native-map", "no_native_map", "edge-cases.js");
        assert_eq!(violations.len(), 2, "got {violations:?}");
        assert!(violations
            .iter()
            .all(|v| v.rule == "no-native-map" && v.message.contains("Immutable.js Map")));
    }

    #[test]
    fn a_parameter_shadowing_a_real_immutable_import_is_reported_as_native() {
        // The exact case the semantic migration fixes: without scope
        // awareness, an import of Immutable's Map anywhere in the file used
        // to suppress every bare `Map` reference in the whole file,
        // including this one -- a false negative, since `new Map()` here
        // plainly means the parameter, not the import.
        let source =
            "import { Map } from 'immutable';\n\nfunction test(Map) {\n  return new Map();\n}\n";
        let violations = check_source("no-native-map", source, Path::new("a.js"));
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert_eq!(violations[0].line, 4, "flags the use, not the parameter");
    }

    #[test]
    fn a_default_import_alias_is_recognized() {
        let source = "import Immutable from 'immutable';\nconst m = new Immutable.Map();\n";
        assert!(check_source("no-native-map", source, Path::new("a.js")).is_empty());
    }

    #[test]
    fn an_aliased_named_import_is_recognized() {
        let source =
            "import { Map as ImmutableMap } from 'immutable';\nconst m = new ImmutableMap();\n";
        assert!(check_source("no-native-map", source, Path::new("a.js")).is_empty());
    }

    #[test]
    fn nested_block_shadowing_is_resolved_independently_per_reference() {
        let source = "import { Map } from 'immutable';\n\nfunction test() {\n  {\n    const Map = CustomMap;\n    new Map();\n  }\n\n  new Map();\n}\n";
        let violations = check_source("no-native-map", source, Path::new("a.js"));
        // The block-scoped `new Map()` resolves to the local `CustomMap`
        // alias, not the import, so it's native; the one after the block
        // resolves back to the import, so it's clean.
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert_eq!(violations[0].line, 6);
    }

    #[test]
    fn a_local_map_unrelated_to_immutable_is_still_reported() {
        let source = "const Map = require('some-other-thing');\nnew Map();\n";
        let violations = check_source("no-native-map", source, Path::new("a.js"));
        assert_eq!(violations.len(), 1, "got {violations:?}");
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
        assert!(
            violations.iter().all(|v| v.fix.is_some()),
            "a synchronous wrapper always has an unambiguous fix: {violations:?}"
        );
    }

    /// An `async` arrow returns a Promise that resolves to the selector, not
    /// the selector itself. Unwrapping it would silently hand callers a
    /// selector instead of a Promise, so the violation is still reported but
    /// left unfixed for a human to look at.
    #[test]
    fn async_wrapper_is_reported_but_left_unfixed() {
        let source = "import { createSelector } from 'reselect';\nconst selectAll = async () => createSelector(a, b);\n";
        let violations = check_source(RULE, source, Path::new("a.js"));
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert!(violations[0].message.contains("breaks memoization"));
        assert!(
            violations[0].fix.is_none(),
            "async wrapper must not get an autofix: {:?}",
            violations[0]
        );
    }

    #[test]
    fn suppression_comments_silence_the_rule() {
        assert!(check_one(RULE, DIR, "suppressed.js").is_empty());
    }

    /// Locks in three documented coverage gaps (block body, argument
    /// position, member-expression callee — none flagged) plus the literal
    /// `/^make[A-Z]/` factory check: "makeup" does not match it, so it IS
    /// flagged despite starting with "make".
    #[test]
    fn edge_cases_flag_only_the_non_factory_make_prefixed_name() {
        let violations = check_one(RULE, DIR, "edge-cases.js");
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert!(violations[0].message.contains("\"makeup\""));
    }

    #[test]
    fn an_aliased_reselect_import_is_recognized() {
        let source = "import { createSelector as selector } from 'reselect';\nconst foo = () => selector(a, b);\n";
        let violations = check_source(RULE, source, Path::new("a.js"));
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert!(violations[0].fix.is_some(), "aliased form still gets a fix");
    }

    #[test]
    fn a_same_named_local_function_is_not_reselect() {
        let source = "function createSelector() {}\nconst selector = () => createSelector(a, b);\n";
        assert!(check_source(RULE, source, Path::new("a.js")).is_empty());
    }

    #[test]
    fn a_createselector_from_a_different_module_is_not_reselect() {
        let source =
            "import { createSelector } from 'some-other-library';\nconst selector = () => createSelector(a, b);\n";
        assert!(check_source(RULE, source, Path::new("a.js")).is_empty());
    }

    #[test]
    fn a_function_parameter_shadowing_the_import_is_not_reselect() {
        let source = "import { createSelector } from 'reselect';\n\nfunction test(createSelector) {\n    const selector = () => createSelector(a, b);\n    return selector;\n}\n";
        assert!(check_source(RULE, source, Path::new("a.js")).is_empty());
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

    /// Locks in three documented gaps (by-reference selector, fewer than 2
    /// arguments, concise single-param arrow counting as 1 -- none flagged)
    /// alongside a real mismatch behind a namespaced callee, which still is.
    #[test]
    fn edge_cases_flag_only_the_namespaced_mismatch() {
        let violations = check_one(RULE, DIR, "edge-cases.js");
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert!(violations[0].message.contains("expects 2 parameter(s)"));
        assert!(violations[0].message.contains("but found 1"));
    }

    #[test]
    fn an_aliased_reselect_import_is_checked() {
        let source =
            "import { createSelector as selector } from 'reselect';\nselector(a, b, x => x);\n";
        assert_eq!(check_source(RULE, source, Path::new("a.js")).len(), 1);
    }

    #[test]
    fn a_same_named_local_function_is_not_reselect() {
        let source = "function createSelector() {}\ncreateSelector(a, b, x => x);\n";
        assert!(check_source(RULE, source, Path::new("a.js")).is_empty());
    }

    #[test]
    fn a_createselector_from_a_different_module_is_not_reselect() {
        let source =
            "import { createSelector } from 'other-library';\ncreateSelector(a, b, x => x);\n";
        assert!(check_source(RULE, source, Path::new("a.js")).is_empty());
    }

    #[test]
    fn a_function_parameter_shadowing_the_import_is_not_reselect() {
        let source = "import { createSelector } from 'reselect';\n\nfunction test(createSelector) {\n    createSelector(a, b, x => x);\n}\n";
        assert!(check_source(RULE, source, Path::new("a.js")).is_empty());
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
            12,
            "expected 4 fixtures for each of 3 rules, got {:?}",
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

mod cli_behavior {
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn json_format_still_emits_a_document_when_every_rule_is_disabled() {
        let tmpdir = TempDir::new().unwrap();
        fs::write(
            tmpdir.path().join("package.json"),
            r#"{"ignoreBiomeExtensionRules":["no-native-map","no-arrow-function-create-selector","reselect-arity-match"]}"#,
        )
        .unwrap();
        fs::write(tmpdir.path().join("clean.js"), "export const x = 1;\n").unwrap();

        let output = Command::new(env!("CARGO_BIN_EXE_custom-biome-lint"))
            .arg(".")
            .arg("--format")
            .arg("json")
            .arg("--no-cache")
            .current_dir(tmpdir.path())
            .output()
            .expect("failed to run custom-biome-lint");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(stdout.trim()).expect("stdout must be valid JSON");
        assert_eq!(parsed["summary"]["clean"], true);
        assert_eq!(parsed["files"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn auto_fix_unwraps_the_arrow_and_relinting_is_then_clean() {
        let tmpdir = TempDir::new().unwrap();
        fs::write(
            tmpdir.path().join("sample.js"),
            "import { createSelector } from 'reselect';\nconst selectAll = () => createSelector(a, b);\n",
        )
        .unwrap();

        let fix = Command::new(env!("CARGO_BIN_EXE_custom-biome-lint"))
            .arg(".")
            .arg("--auto-fix")
            .arg("--no-cache")
            .current_dir(tmpdir.path())
            .output()
            .expect("failed to run custom-biome-lint");
        assert!(fix.status.success(), "{:?}", fix);

        let rewritten = fs::read_to_string(tmpdir.path().join("sample.js")).unwrap();
        assert_eq!(
            rewritten,
            "import { createSelector } from 'reselect';\nconst selectAll = createSelector(a, b);\n"
        );

        let relint = Command::new(env!("CARGO_BIN_EXE_custom-biome-lint"))
            .arg(".")
            .arg("--no-cache")
            .current_dir(tmpdir.path())
            .output()
            .expect("failed to run custom-biome-lint");
        assert!(
            relint.status.success(),
            "fixed file must relint clean: {:?}",
            relint
        );
    }

    #[test]
    fn auto_fix_dry_run_leaves_the_file_untouched() {
        let tmpdir = TempDir::new().unwrap();
        let source = "import { createSelector } from 'reselect';\nconst selectAll = () => createSelector(a, b);\n";
        fs::write(tmpdir.path().join("sample.js"), source).unwrap();

        let output = Command::new(env!("CARGO_BIN_EXE_custom-biome-lint"))
            .arg(".")
            .arg("--auto-fix")
            .arg("--dry-run")
            .arg("--no-cache")
            .current_dir(tmpdir.path())
            .output()
            .expect("failed to run custom-biome-lint");

        assert!(!output.status.success(), "unfixed violations remain");
        assert_eq!(
            fs::read_to_string(tmpdir.path().join("sample.js")).unwrap(),
            source
        );
    }

    #[test]
    fn write_fix_and_auto_fix_together_is_rejected() {
        let output = Command::new(env!("CARGO_BIN_EXE_custom-biome-lint"))
            .arg("--write-fix")
            .arg("--auto-fix")
            .output()
            .expect("failed to run custom-biome-lint");
        assert!(!output.status.success());
    }
}

mod semantic_model {
    use super::*;
    use biome_js_syntax::JsReferenceIdentifier;
    use biome_rowan::AstNode;
    use custom_biome_lint::{BindingKind, FileContext, ImportedName, ScopeKind};

    fn parse(source: &'static str) -> FileContext<'static> {
        FileContext::parse(source, Path::new("a.js"))
    }

    /// The `n`th (0-based, in source order) reference to `name` -- i.e. a
    /// *use* of the identifier, not a declaration of it.
    fn nth_reference(file: &FileContext<'_>, name: &str, n: usize) -> JsReferenceIdentifier {
        file.tree()
            .descendants()
            .filter_map(JsReferenceIdentifier::cast)
            .filter(|ident| {
                ident
                    .value_token()
                    .is_ok_and(|token| token.text_trimmed() == name)
            })
            .nth(n)
            .unwrap_or_else(|| panic!("no occurrence #{n} of reference `{name}` in source"))
    }

    #[test]
    fn basic_declarations_are_all_resolvable() {
        let file = parse(
            "const foo = 1;\nlet bar = 2;\nvar baz = 3;\nfunction quux() {}\nclass Corge {}\nfoo; bar; baz; quux; Corge;\n",
        );
        let model = file.semantic();

        let cases = [
            ("foo", BindingKind::Const),
            ("bar", BindingKind::Let),
            ("baz", BindingKind::Var),
            ("quux", BindingKind::Function),
            ("Corge", BindingKind::Class),
        ];
        for (name, expected_kind) in cases {
            let reference = nth_reference(&file, name, 0);
            let binding = model
                .resolve(&reference)
                .unwrap_or_else(|| panic!("{name} did not resolve"));
            assert_eq!(binding.name, name);
            assert_eq!(binding.kind, expected_kind, "wrong kind for {name}");
        }
    }

    #[test]
    fn a_function_parameter_resolves_to_itself() {
        let file = parse("function test(foo) {\n    foo();\n}\n");
        let model = file.semantic();

        let reference = nth_reference(&file, "foo", 0);
        let binding = model.resolve(&reference).expect("foo should resolve");
        assert_eq!(binding.kind, BindingKind::Parameter);
        let (line, _) = file.line_col(binding.declared_at);
        assert_eq!(line, 1, "parameter foo is declared on line 1");
    }

    #[test]
    fn nested_scopes_each_shadow_the_next_one_out() {
        let file = parse(
            "const foo = 1;\n\nfunction test() {\n    const foo = 2;\n\n    {\n        const foo = 3;\n        use(foo);\n    }\n\n    use(foo);\n}\n",
        );
        let model = file.semantic();

        // The reference inside the block resolves to the innermost `foo`
        // (declared on line 7), the one after the block to the function's
        // own `foo` (line 4) -- neither ever sees the module-level one.
        let inner = nth_reference(&file, "foo", 0);
        let inner_binding = model.resolve(&inner).unwrap();
        assert_eq!(file.line_col(inner_binding.declared_at).0, 7);

        let outer = nth_reference(&file, "foo", 1);
        let outer_binding = model.resolve(&outer).unwrap();
        assert_eq!(file.line_col(outer_binding.declared_at).0, 4);
    }

    #[test]
    fn nearest_lexical_binding_wins_over_an_outer_one() {
        let file = parse(
            "const foo = 1;\n\nfunction test() {\n    const foo = 2;\n    console.log(foo);\n}\n",
        );
        let model = file.semantic();

        let reference = nth_reference(&file, "foo", 0);
        let binding = model.resolve(&reference).unwrap();
        assert_eq!(file.line_col(binding.declared_at).0, 4);
    }

    #[test]
    fn default_import_is_tracked() {
        let file = parse("import foo from \"module\";\nfoo;\n");
        let model = file.semantic();

        let reference = nth_reference(&file, "foo", 0);
        let binding = model.resolve(&reference).unwrap();
        let import = binding.import().expect("foo is an import");
        assert_eq!(import.source, "module");
        assert_eq!(import.imported, ImportedName::Default);
        assert_eq!(import.local, "foo");
    }

    #[test]
    fn named_import_is_tracked() {
        let file = parse("import { foo } from \"module\";\nfoo;\n");
        let model = file.semantic();

        let reference = nth_reference(&file, "foo", 0);
        let binding = model.resolve(&reference).unwrap();
        let import = binding.import().unwrap();
        assert_eq!(import.source, "module");
        assert_eq!(import.imported, ImportedName::Named("foo".to_string()));
        assert_eq!(import.local, "foo");
    }

    #[test]
    fn aliased_named_import_keeps_both_names_distinct() {
        let file = parse("import { createSelector as selector } from \"reselect\";\nselector();\n");
        let model = file.semantic();

        let reference = nth_reference(&file, "selector", 0);
        let binding = model.resolve(&reference).unwrap();
        let import = binding.import().unwrap();
        assert_eq!(import.source, "reselect");
        assert_eq!(
            import.imported,
            ImportedName::Named("createSelector".to_string())
        );
        assert_eq!(import.local, "selector");
    }

    #[test]
    fn namespace_import_is_tracked() {
        let file = parse("import * as foo from \"module\";\nfoo.bar();\n");
        let model = file.semantic();

        let reference = nth_reference(&file, "foo", 0);
        let binding = model.resolve(&reference).unwrap();
        let import = binding.import().unwrap();
        assert_eq!(import.source, "module");
        assert_eq!(import.imported, ImportedName::Namespace);
        assert_eq!(import.local, "foo");
    }

    #[test]
    fn a_parameter_shadows_an_imported_name_of_the_same_name() {
        // The parameter must win -- the call inside test() must NOT resolve
        // to the module-level import.
        let file = parse(
            "import { createSelector } from \"reselect\";\n\nfunction test(createSelector) {\n    return createSelector(a, b);\n}\n",
        );
        let model = file.semantic();

        let reference = nth_reference(&file, "createSelector", 0);
        let binding = model.resolve(&reference).unwrap();
        assert_eq!(binding.kind, BindingKind::Parameter);
        assert!(
            binding.import().is_none(),
            "must resolve to the parameter, not the import"
        );
    }

    #[test]
    fn a_local_redeclaration_shadows_an_import_of_the_same_name() {
        let file = parse(
            "import { Map } from \"immutable\";\n\nfunction test() {\n    const Map = CustomMap;\n    return new Map();\n}\n",
        );
        let model = file.semantic();

        // `Map` in `import { Map }` and in `const Map = ...` are both
        // bindings, not references -- the only actual reference to `Map`
        // is in `new Map()`.
        let reference = nth_reference(&file, "Map", 0);
        let binding = model.resolve(&reference).unwrap();
        assert_eq!(binding.kind, BindingKind::Const);
        assert!(
            binding.import().is_none(),
            "must resolve to the local const, not the import"
        );
        assert_eq!(file.line_col(binding.declared_at).0, 4);
    }

    #[test]
    fn object_destructuring_binds_every_name() {
        let file = parse("const { foo } = obj;\nconst { foo: bar } = obj;\nfoo; bar;\n");
        let model = file.semantic();

        let foo = model.resolve(&nth_reference(&file, "foo", 0)).unwrap();
        assert_eq!(foo.kind, BindingKind::Const);
        let bar = model.resolve(&nth_reference(&file, "bar", 0)).unwrap();
        assert_eq!(bar.kind, BindingKind::Const);
    }

    #[test]
    fn array_destructuring_binds_every_name() {
        let file = parse("const [foo] = values;\nfoo;\n");
        let model = file.semantic();

        let binding = model.resolve(&nth_reference(&file, "foo", 0)).unwrap();
        assert_eq!(binding.kind, BindingKind::Const);
    }

    #[test]
    fn a_computed_destructuring_key_is_a_reference() {
        let file = parse("const key = \"foo\";\nconst { [key]: value } = obj;\nvalue;\n");
        let model = file.semantic();

        // `key` inside `[key]` must resolve to the outer `const key`, not be
        // silently dropped because the property's key expression was never
        // walked for references.
        let key_reference = nth_reference(&file, "key", 0);
        let key_binding = model
            .resolve(&key_reference)
            .expect("computed key must be resolvable");
        assert_eq!(key_binding.kind, BindingKind::Const);

        let value_binding = model.resolve(&nth_reference(&file, "value", 0)).unwrap();
        assert_eq!(value_binding.kind, BindingKind::Const);
    }

    #[test]
    fn arrow_function_parameter_is_bound() {
        let file = parse("const test = foo => foo;\n");
        let model = file.semantic();

        let binding = model.resolve(&nth_reference(&file, "foo", 0)).unwrap();
        assert_eq!(binding.kind, BindingKind::Parameter);
    }

    #[test]
    fn block_scope_shadows_then_restores_the_outer_binding() {
        let file =
            parse("const foo = 1;\n\n{\n    const foo = 2;\n    use(foo);\n}\n\nuse(foo);\n");
        let model = file.semantic();

        let inner = model.resolve(&nth_reference(&file, "foo", 0)).unwrap();
        assert_eq!(file.line_col(inner.declared_at).0, 4);

        let outer = model.resolve(&nth_reference(&file, "foo", 1)).unwrap();
        assert_eq!(file.line_col(outer.declared_at).0, 1);
    }

    #[test]
    fn catch_scope_binds_the_caught_error() {
        let file = parse("try {\n} catch (error) {\n    console.log(error);\n}\n");
        let model = file.semantic();

        let binding = model.resolve(&nth_reference(&file, "error", 0)).unwrap();
        assert_eq!(binding.kind, BindingKind::CatchParameter);
    }

    #[test]
    fn a_var_hoists_out_of_a_nested_block_to_the_function_scope() {
        let file =
            parse("function test() {\n    {\n        var foo = 1;\n    }\n    use(foo);\n}\n");
        let model = file.semantic();

        let binding = model.resolve(&nth_reference(&file, "foo", 0)).unwrap();
        assert_eq!(binding.kind, BindingKind::Var);
        // The var's binding lives in the function scope, not the block it
        // was textually declared in.
        let function_scope = binding.scope;
        assert_eq!(model.scope(function_scope).kind(), ScopeKind::Function);
    }

    #[test]
    fn a_let_in_a_for_loop_head_is_scoped_to_the_loop() {
        let file = parse("for (let i = 0; i < 10; i++) {\n    use(i);\n}\n");
        let model = file.semantic();

        let binding = model.resolve(&nth_reference(&file, "i", 0)).unwrap();
        assert_eq!(binding.kind, BindingKind::Let);
        assert_eq!(model.scope(binding.scope).kind(), ScopeKind::Loop);
    }

    #[test]
    fn a_switch_statement_shares_one_block_scope_across_every_case() {
        let file = parse(
            "function test(x) {\n    switch (x) {\n        case 1: {\n            const foo = 1;\n            use(foo);\n            break;\n        }\n        case 2:\n            let bar = 2;\n            use(bar);\n            break;\n    }\n}\n",
        );
        let model = file.semantic();

        // `bar`, declared directly in a case body (no braces around it), is
        // scoped to the whole switch -- not the enclosing function -- per
        // real JS semantics.
        let bar = model.resolve(&nth_reference(&file, "bar", 0)).unwrap();
        assert_eq!(bar.kind, BindingKind::Let);
        assert_eq!(model.scope(bar.scope).kind(), ScopeKind::Block);

        // `foo`, inside its own `{ }` case block, gets a nested block scope
        // as usual, one level further in than the switch's own scope.
        let foo = model.resolve(&nth_reference(&file, "foo", 0)).unwrap();
        assert_eq!(foo.kind, BindingKind::Const);
        assert_eq!(model.scope(foo.scope).kind(), ScopeKind::Block);
        assert_eq!(model.scope(foo.scope).parent(), Some(bar.scope));
    }

    #[test]
    fn scopes_form_the_expected_parent_chain() {
        let file = parse("function test() {\n    const foo = 1;\n    foo;\n}\n");
        let model = file.semantic();

        // `test` itself is declared in the global scope.
        let test_binding = model
            .scope(model.global_scope())
            .binding("test")
            .map(|id| model.binding(id))
            .expect("function declaration `test` is bound in the global scope");
        assert_eq!(test_binding.kind, BindingKind::Function);

        // `foo`, declared inside `test`, lives in a function scope whose
        // parent is the global scope.
        let foo_binding = model.resolve(&nth_reference(&file, "foo", 0)).unwrap();
        let function_scope = model.scope(foo_binding.scope);
        assert_eq!(function_scope.kind(), ScopeKind::Function);
        assert_eq!(function_scope.parent(), Some(model.global_scope()));
    }
}
