//! Standalone linter for Reselect/Redux patterns that Biome does not cover.
//!
//! Each file is parsed once into a [`FileContext`]; every enabled [`Rule`] then
//! inspects that shared tree.
//!
//! ```no_run
//! use std::path::Path;
//! use custom_biome_lint::{lint_source, RuleRegistry};
//!
//! let registry = RuleRegistry::with_all_rules();
//! let violations = lint_source(
//!     "const selectAll = () => createSelector(a, b);",
//!     Path::new("example.js"),
//!     &registry.all(),
//!     false,
//! );
//! ```

pub mod analyzer;
pub mod autofix;
pub mod cache;
pub mod cli;
pub mod config;
pub mod diagnostics;
pub mod fixer;
pub mod rules;
pub mod semantic;
pub mod suppress;

pub use analyzer::runner::{analyze_file, AnalyzedFile, FileContext};
pub use analyzer::{discover_files, resolve_pattern, Discovery, GlobSet};
pub use autofix::{AppliedFix, Autofix, AutofixReport, SkippedFix};
pub use cli::{run, CliArgs, OutputFormat, Reporter, VERSION};
pub use config::{PackageConfig, RuleSeverity};
pub use diagnostics::{format_reports_json, FileReport, Fix, Severity, Totals, Violation};
pub use fixer::{plan_file, FileChange, FilePlan, FixReport, Fixer, Placement, Unfixable};
pub use rules::{Rule, RuleRegistry};
pub use semantic::{
    Binding, BindingId, BindingKind, ImportBinding, ImportedName, Scope, ScopeId, ScopeKind,
    SemanticModel,
};
pub use suppress::{find_suppression_comments, SuppressionComment, Suppressions};

use std::path::Path;

/// Lints a single in-memory source string, honouring suppression comments.
///
/// `enrich` mirrors [`analyze_file`]: when true, violations carry the extra
/// IDE fix/suppression edits. Library callers that only need the violations
/// themselves pass `false`.
pub fn lint_source(source: &str, path: &Path, rules: &[&dyn Rule], enrich: bool) -> Vec<Violation> {
    analyze_file(path, source, rules, enrich).violations
}
