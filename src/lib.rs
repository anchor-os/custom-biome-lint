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
pub mod cache;
pub mod cli;
pub mod config;
pub mod diagnostics;
pub mod fixer;
pub mod rules;
pub mod suppress;

pub use analyzer::runner::{analyze_file, AnalyzedFile, FileContext};
pub use analyzer::{discover_files, resolve_pattern, Discovery, GlobSet};
pub use cli::{run, CliArgs, OutputFormat, Reporter, VERSION};
pub use config::PackageConfig;
pub use diagnostics::{format_reports_json, FileReport, Severity, Totals, Violation};
pub use fixer::{plan_file, FileChange, FilePlan, FixReport, Fixer, Placement, Unfixable};
pub use rules::{Rule, RuleRegistry};
pub use suppress::{find_suppression_comments, SuppressionComment, Suppressions};

use std::path::Path;

/// Lints a single in-memory source string, honouring suppression comments.
pub fn lint_source(source: &str, path: &Path, rules: &[&dyn Rule]) -> Vec<Violation> {
    analyze_file(path, source, rules).violations
}
