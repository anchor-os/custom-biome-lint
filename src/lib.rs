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

pub use analyzer::runner::{analyze_file, analyze_file_enriched, AnalyzedFile, FileContext};
pub use analyzer::{discover_files, resolve_pattern, Discovery, GlobSet};
pub use autofix::{AppliedFix, Autofix, AutofixReport, SkippedFix};
pub use cli::{run, CliArgs, OutputFormat, Reporter, VERSION};
pub use config::{PackageConfig, RuleSeverity};
pub use diagnostics::{
    format_reports_json, Edit, FileReport, Fix, Severity, Suggestion, Totals, Violation,
};
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
/// Kept at its original three-argument signature so existing consumers keep
/// compiling unchanged. Violations do **not** carry the IDE-only
/// `fixes`/`suppressions` fields.
pub fn lint_source(source: &str, path: &Path, rules: &[&dyn Rule]) -> Vec<Violation> {
    analyze_file(path, source, rules).violations
}

/// Like [`lint_source`], but each surviving violation is also filled in with
/// the machine-readable fix and suppression edits the IDE contract exposes (see
/// [`Violation::fixes`] / [`Violation::suppressions`]). Use this when feeding an
/// IDE adapter; existing plain consumers should keep using [`lint_source`].
pub fn lint_source_enriched(source: &str, path: &Path, rules: &[&dyn Rule]) -> Vec<Violation> {
    analyze_file_enriched(path, source, rules).violations
}
