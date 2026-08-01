use crate::config::PackageConfig;

use super::no_arrow_function_create_selector::NoArrowFunctionCreateSelector;
use super::no_native_map::NoNativeMap;
use super::reselect_arity_match::ReselectArityMatch;
use super::rule::Rule;

pub struct RuleRegistry {
    rules: Vec<Box<dyn Rule>>,
}

impl RuleRegistry {
    /// Every rule shipped with the tool. Register new rules here.
    pub fn with_all_rules() -> Self {
        Self {
            rules: vec![
                Box::new(NoNativeMap),
                Box::new(NoArrowFunctionCreateSelector),
                Box::new(ReselectArityMatch),
            ],
        }
    }

    pub fn all(&self) -> Vec<&dyn Rule> {
        self.rules.iter().map(AsRef::as_ref).collect()
    }

    pub fn enabled(&self, config: &PackageConfig) -> Vec<&dyn Rule> {
        self.rules
            .iter()
            .map(AsRef::as_ref)
            .filter(|rule| !config.is_ignored(rule.name()))
            .collect()
    }

    pub fn ignored(&self, config: &PackageConfig) -> Vec<&dyn Rule> {
        self.rules
            .iter()
            .map(AsRef::as_ref)
            .filter(|rule| config.is_ignored(rule.name()))
            .collect()
    }

    /// Union of every rule's supported extensions, without leading dots.
    pub fn supported_extensions(&self) -> Vec<&'static str> {
        let mut extensions = Vec::new();
        for rule in &self.rules {
            for extension in rule.supported_extensions() {
                let bare = extension.trim_start_matches('.');
                if !extensions.contains(&bare) {
                    extensions.push(bare);
                }
            }
        }
        extensions
    }

    /// Default glob covering every registered rule's extensions.
    pub fn default_pattern(&self) -> String {
        format!("src/**/*.{{{}}}", self.supported_extensions().join(","))
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self::with_all_rules()
    }
}
