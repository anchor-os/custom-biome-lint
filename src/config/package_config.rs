use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_KEY: &str = "ignoreBiomeExtensionRules";

/// A rule's configured severity. `Off` disables the rule entirely (it never
/// runs); `Warn`/`Error` override the severity of whatever violations it
/// reports, without changing whether it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSeverity {
    Off,
    Warn,
    Error,
}

impl RuleSeverity {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// Rule severities configured via `package.json`.
///
/// Two accepted shapes for backward compatibility:
///
/// ```json
/// { "ignoreBiomeExtensionRules": ["no-native-map"] }
/// ```
///
/// — the legacy array form, equivalent to setting every listed rule to
/// `"off"` — and the richer object form:
///
/// ```json
/// {
///   "ignoreBiomeExtensionRules": {
///     "no-native-map": "off",
///     "reselect-arity-match": "warn"
///   }
/// }
/// ```
///
/// A rule with no entry keeps its default severity (`error`).
#[derive(Debug, Default, Clone)]
pub struct PackageConfig {
    pub severities: HashMap<String, RuleSeverity>,
    pub source: Option<PathBuf>,
    pub warnings: Vec<String>,
}

impl PackageConfig {
    /// Finds the nearest `package.json` at or above `start` and reads the
    /// severity config from it. A missing file is not an error — the tool
    /// runs with all rules enabled at their default severity.
    pub fn load(start: &Path) -> Self {
        let Some(path) = find_package_json(start) else {
            return Self::default();
        };

        let mut config = Self {
            source: Some(path.clone()),
            ..Self::default()
        };

        let Ok(text) = fs::read_to_string(&path) else {
            config
                .warnings
                .push(format!("could not read {}", path.display()));
            return config;
        };

        let json: serde_json::Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(error) => {
                config
                    .warnings
                    .push(format!("{} is not valid JSON: {error}", path.display()));
                return config;
            }
        };

        match json.get(CONFIG_KEY) {
            None => {}
            Some(serde_json::Value::Array(items)) => {
                // Legacy shape: every entry means "off".
                for item in items {
                    match item.as_str() {
                        Some(name) => {
                            config.severities.insert(name.to_string(), RuleSeverity::Off);
                        }
                        None => config
                            .warnings
                            .push(format!("{CONFIG_KEY} entries must be strings, got {item}")),
                    }
                }
            }
            Some(serde_json::Value::Object(entries)) => {
                for (name, value) in entries {
                    match value.as_str().and_then(RuleSeverity::parse) {
                        Some(severity) => {
                            config.severities.insert(name.clone(), severity);
                        }
                        None => config.warnings.push(format!(
                            "{CONFIG_KEY}.{name} must be \"off\", \"warn\", or \"error\", got {value}"
                        )),
                    }
                }
            }
            Some(other) => config.warnings.push(format!(
                "{CONFIG_KEY} must be an array of rule names or an object of rule name -> \"off\"/\"warn\"/\"error\", got {other}"
            )),
        }

        config
    }

    pub fn is_ignored(&self, rule: &str) -> bool {
        self.severities.get(rule) == Some(&RuleSeverity::Off)
    }

    /// The severity `rule` actually runs at: its configured entry if it has
    /// one, otherwise `default` (the rule's own
    /// [`Rule::default_severity`](crate::rules::Rule::default_severity)).
    ///
    /// This is the only correct way to ask "does this rule run at all", because
    /// [`Self::severity_override`] deliberately collapses "no entry" and
    /// `"off"` to `None` — a distinction that does not matter when overriding a
    /// violation's severity, but is the whole question for a rule whose default
    /// is `Off`.
    pub fn severity(&self, rule: &str, default: RuleSeverity) -> RuleSeverity {
        self.severities.get(rule).copied().unwrap_or(default)
    }

    /// The configured `warn`/`error` override for `rule`, or `None` if it has
    /// no entry (keep the rule's default) or is set to `off` (irrelevant,
    /// since an off rule never produces violations to override).
    pub fn severity_override(&self, rule: &str) -> Option<RuleSeverity> {
        match self.severities.get(rule) {
            Some(RuleSeverity::Off) | None => None,
            Some(severity) => Some(*severity),
        }
    }
}

fn find_package_json(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|dir| {
        let candidate = dir.join("package.json");
        candidate.is_file().then_some(candidate)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_package_json(dir: &Path, contents: &str) {
        fs::write(dir.join("package.json"), contents).unwrap();
    }

    #[test]
    fn legacy_array_form_sets_off() {
        let tmp = TempDir::new().unwrap();
        write_package_json(
            tmp.path(),
            r#"{ "ignoreBiomeExtensionRules": ["no-native-map"] }"#,
        );
        let config = PackageConfig::load(tmp.path());
        assert!(config.is_ignored("no-native-map"));
        assert_eq!(config.severity_override("no-native-map"), None);
        assert!(config.warnings.is_empty());
    }

    #[test]
    fn object_form_sets_off_warn_and_error() {
        let tmp = TempDir::new().unwrap();
        write_package_json(
            tmp.path(),
            r#"{
                "ignoreBiomeExtensionRules": {
                    "no-native-map": "off",
                    "reselect-arity-match": "warn",
                    "no-arrow-function-create-selector": "error"
                }
            }"#,
        );
        let config = PackageConfig::load(tmp.path());
        assert!(config.is_ignored("no-native-map"));
        assert_eq!(
            config.severity_override("reselect-arity-match"),
            Some(RuleSeverity::Warn)
        );
        assert_eq!(
            config.severity_override("no-arrow-function-create-selector"),
            Some(RuleSeverity::Error)
        );
        assert!(config.warnings.is_empty());
    }

    #[test]
    fn unconfigured_rule_has_no_override() {
        let tmp = TempDir::new().unwrap();
        write_package_json(tmp.path(), r#"{ "ignoreBiomeExtensionRules": {} }"#);
        let config = PackageConfig::load(tmp.path());
        assert!(!config.is_ignored("no-native-map"));
        assert_eq!(config.severity_override("no-native-map"), None);
    }

    /// `severity` is the "does this rule run" question, and unlike
    /// `severity_override` it must keep `"off"` and "no entry" apart — that
    /// distinction is the whole point for a rule whose default is `Off`.
    #[test]
    fn severity_falls_back_to_the_default_only_when_unconfigured() {
        let tmp = TempDir::new().unwrap();
        write_package_json(
            tmp.path(),
            r#"{
                "ignoreBiomeExtensionRules": {
                    "explicitly-off": "off",
                    "opted-in": "error"
                }
            }"#,
        );
        let config = PackageConfig::load(tmp.path());

        // Unconfigured: the rule's own default decides, either way.
        assert_eq!(
            config.severity("unconfigured", RuleSeverity::Error),
            RuleSeverity::Error
        );
        assert_eq!(
            config.severity("unconfigured", RuleSeverity::Off),
            RuleSeverity::Off
        );

        // Configured: the entry wins over the default, in both directions.
        assert_eq!(
            config.severity("explicitly-off", RuleSeverity::Error),
            RuleSeverity::Off
        );
        assert_eq!(
            config.severity("opted-in", RuleSeverity::Off),
            RuleSeverity::Error
        );
    }

    #[test]
    fn invalid_severity_value_warns_and_is_skipped() {
        let tmp = TempDir::new().unwrap();
        write_package_json(
            tmp.path(),
            r#"{ "ignoreBiomeExtensionRules": { "no-native-map": "banana" } }"#,
        );
        let config = PackageConfig::load(tmp.path());
        assert!(!config.is_ignored("no-native-map"));
        assert_eq!(config.warnings.len(), 1);
        assert!(config.warnings[0].contains("no-native-map"));
    }

    #[test]
    fn non_string_array_entry_warns_and_is_skipped() {
        let tmp = TempDir::new().unwrap();
        write_package_json(tmp.path(), r#"{ "ignoreBiomeExtensionRules": [1] }"#);
        let config = PackageConfig::load(tmp.path());
        assert_eq!(config.warnings.len(), 1);
        assert!(config.severities.is_empty());
    }

    #[test]
    fn wrong_top_level_type_warns() {
        let tmp = TempDir::new().unwrap();
        write_package_json(
            tmp.path(),
            r#"{ "ignoreBiomeExtensionRules": "no-native-map" }"#,
        );
        let config = PackageConfig::load(tmp.path());
        assert_eq!(config.warnings.len(), 1);
    }

    #[test]
    fn missing_package_json_enables_everything_with_no_warnings() {
        let tmp = TempDir::new().unwrap();
        let config = PackageConfig::load(tmp.path());
        assert!(config.source.is_none());
        assert!(config.warnings.is_empty());
        assert!(!config.is_ignored("no-native-map"));
    }
}
