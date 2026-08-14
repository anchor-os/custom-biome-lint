use crate::analyzer::runner::FileContext;
use crate::config::RuleSeverity;
use crate::diagnostics::Violation;

/// A single lint rule.
///
/// `check` receives an already-parsed [`FileContext`] rather than raw source so
/// that a file is parsed once and every rule shares the tree. The path and
/// source are still reachable via [`FileContext::path`] and
/// [`FileContext::source`].
///
/// To add a rule: create `src/rules/my_rule.rs`, implement this trait, register
/// it in [`crate::rules::registry::RuleRegistry::with_all_rules`], and add
/// `fixtures/my_rule/{valid,invalid,suppressed}.js`.
pub trait Rule: Send + Sync {
    /// Kebab-case identifier used in output and in suppression comments.
    fn name(&self) -> &'static str;

    fn description(&self) -> &'static str;

    /// Extensions this rule can analyze, with leading dots (e.g. `[".js", ".jsx"]`).
    fn supported_extensions(&self) -> &'static [&'static str];

    fn check(&self, file: &FileContext) -> Vec<Violation>;

    /// The severity this rule runs at when `ignoreBiomeExtensionRules` has no
    /// entry for it.
    ///
    /// Defaults to [`RuleSeverity::Error`] — every rule is on unless explicitly
    /// turned off. Override with [`RuleSeverity::Off`] for a rule whose
    /// findings are opinionated enough that a consuming repo should have to opt
    /// in, by setting it to `"warn"`/`"error"` in `ignoreBiomeExtensionRules`.
    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Error
    }
}
