pub mod formatter;
pub mod violation;

/// The IDE protocol version emitted as the top-level `version` field of every
/// JSON report (`--format json`) and the `--rules` catalog. Bump only when a
/// breaking change is required, and keep the prior version documented in
/// `docs/IDE_PROTOCOL.md`. This is the single source of truth for the version
/// number — both the diagnostic report and the rule catalog read it.
pub const PROTOCOL_VERSION: u8 = 1;

pub use formatter::{
    format_reports, format_reports_json, format_summary, tally, FileReport, Totals,
};
pub use violation::{Edit, Fix, Severity, Suggestion, Violation};
