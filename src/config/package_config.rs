use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_KEY: &str = "ignoreBiomeExtensionRules";

/// Rules disabled via `package.json`:
///
/// ```json
/// { "ignoreBiomeExtensionRules": ["no-native-map"] }
/// ```
#[derive(Debug, Default, Clone)]
pub struct PackageConfig {
    pub ignored_rules: Vec<String>,
    pub source: Option<PathBuf>,
    pub warnings: Vec<String>,
}

impl PackageConfig {
    /// Finds the nearest `package.json` at or above `start` and reads the ignore
    /// list from it. A missing file is not an error — the tool runs with all
    /// rules enabled.
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
                for item in items {
                    match item.as_str() {
                        Some(name) => config.ignored_rules.push(name.to_string()),
                        None => config
                            .warnings
                            .push(format!("{CONFIG_KEY} entries must be strings, got {item}")),
                    }
                }
            }
            Some(other) => config.warnings.push(format!(
                "{CONFIG_KEY} must be an array of rule names, got {other}"
            )),
        }

        config
    }

    pub fn is_ignored(&self, rule: &str) -> bool {
        self.ignored_rules.iter().any(|ignored| ignored == rule)
    }
}

fn find_package_json(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|dir| {
        let candidate = dir.join("package.json");
        candidate.is_file().then_some(candidate)
    })
}
