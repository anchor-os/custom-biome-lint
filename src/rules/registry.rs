use crate::config::{PackageConfig, RuleSeverity};

use super::bare_arrow_param_prop_assign::BareArrowParamPropAssign;
use super::deep_param_prop_assign::DeepParamPropAssign;
use super::destructure_default_param_assign::DestructureDefaultParamAssign;
use super::destructure_param_prop_assign::DestructureParamPropAssign;
use super::no_arrow_function_create_selector::NoArrowFunctionCreateSelector;
use super::no_do_while_statement::NoDoWhileStatement;
use super::no_for_statement::NoForStatement;
use super::no_native_map::NoNativeMap;
use super::no_while_statement::NoWhileStatement;
use super::param_mutating_array_method_call::ParamMutatingArrayMethodCall;
use super::reselect_arity_match::ReselectArityMatch;
use super::rule::Rule;

/// The severity a rule will actually run at under `config`.
fn resolved_severity(rule: &dyn Rule, config: &PackageConfig) -> RuleSeverity {
    config.severity(rule.name(), rule.default_severity())
}

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
                Box::new(NoDoWhileStatement),
                Box::new(NoForStatement),
                Box::new(NoWhileStatement),
                Box::new(ReselectArityMatch),
                Box::new(DestructureDefaultParamAssign),
                Box::new(DestructureParamPropAssign),
                Box::new(BareArrowParamPropAssign),
                Box::new(DeepParamPropAssign),
                Box::new(ParamMutatingArrayMethodCall),
            ],
        }
    }

    pub fn all(&self) -> Vec<&dyn Rule> {
        self.rules.iter().map(AsRef::as_ref).collect()
    }

    /// Every rule that resolves to a non-`Off` severity: those with no config
    /// entry and a non-`Off` [`Rule::default_severity`], plus those the config
    /// explicitly sets to `"warn"`/`"error"`.
    pub fn enabled(&self, config: &PackageConfig) -> Vec<&dyn Rule> {
        self.rules
            .iter()
            .map(AsRef::as_ref)
            .filter(|rule| resolved_severity(*rule, config) != RuleSeverity::Off)
            .collect()
    }

    /// The mirror image of [`Self::enabled`] — every rule that will not run,
    /// whether because the config turned it off or because it defaults to off
    /// and nothing turned it on.
    pub fn ignored(&self, config: &PackageConfig) -> Vec<&dyn Rule> {
        self.rules
            .iter()
            .map(AsRef::as_ref)
            .filter(|rule| resolved_severity(*rule, config) == RuleSeverity::Off)
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
