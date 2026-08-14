use std::path::Path;

use crate::autofix::AutofixReport;
use crate::cli::args::OutputFormat;
use crate::diagnostics::{format_reports, format_reports_json, FileReport, Totals};
use crate::fixer::FixReport;
use crate::rules::RuleRegistry;

pub const HELP: &str = "\
custom-biome-lint - lint Reselect/Redux patterns that Biome does not cover

USAGE:
    custom-biome-lint [PATTERN] [FLAGS]

ARGS:
    <PATTERN>    Glob of files to lint (default: src/**/*.{js,jsx}).
                 A bare directory is expanded to <dir>/**/*.{js,jsx}.
                 Relative and absolute paths are both accepted.

FLAGS:
        --write-fix    Add a suppression comment for every violation, in place
        --auto-fix     Rewrite violations in place using each rule's own fix.
                       Only rules that can produce an unambiguous fix support
                       this; the rest are reported as skipped. Cannot be
                       combined with --write-fix.
        --dry-run      With --write-fix or --auto-fix, report the changes
                       without writing them
        --format <text|json>
                       Diagnostics output format (default: text). Not
                       supported together with --write-fix or --auto-fix.
    -v, --verbose      Standard verbosity (shows rules, skips)
    -vv                Deep verbosity (file discovery, rule execution)
    -vvv               Super verbose (per-file AST details)
    -d, --debug        Debug: internal state, every step
        --trace        Prefix log lines with their source location
    -h, --help         Show this help
    -V, --version      Show version

AUTOFIX:
    --auto-fix currently applies to:
        no-arrow-function-create-selector  unwraps the arrow, keeping the
                                            createSelector call as-is

    Every other rule has no unambiguous fix and is left for --write-fix or
    manual editing instead.

CONFIGURATION:
    package.json may set rule severities by name:

        { \"ignoreBiomeExtensionRules\": [\"no-native-map\"] }
        { \"ignoreBiomeExtensionRules\": { \"no-native-map\": \"off\",
                                          \"reselect-arity-match\": \"warn\" } }

    The array form is shorthand for \"off\". The object form also accepts
    \"warn\" (reported but does not fail the run) and \"error\" (default).

    Two rules ship OFF and only run once given a severity here:

        bare-arrow-param-prop-assign   property mutation through an arrow's
                                       unparenthesized single parameter
        deep-param-prop-assign         plain-parameter mutation 2+ levels deep

    For those two, no entry means \"off\"; every other rule defaults to
    \"error\". Run with -v to see which rules are enabled and why.

SUPPRESSIONS:
    // custom-biome-ignore-line rule-name[, rule-name2]
    // custom-biome-ignore-next-line rule-name[, rule-name2]

    A marker with no rule names suppresses every rule on its target line.
    Inside JSX children the {/* ... */} form is required, and is what
    --write-fix emits there.

EXIT CODES:
    0    no violations (with --write-fix: everything was suppressed;
         with --auto-fix: everything was fixed)
    1    violations found, or a violation could not be suppressed/fixed
    2    bad usage or unreadable path
";

/// Verbosity-gated logging plus final report printing.
pub struct Reporter {
    level: u8,
    trace: bool,
}

impl Reporter {
    pub fn new(level: u8, trace: bool) -> Self {
        Self { level, trace }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    pub fn enabled(&self, level: u8) -> bool {
        self.level >= level
    }

    /// Writes a log line to stderr when `level` is enabled. Prefer the `vlog!`
    /// and `dlog!` macros, which fill in the source location for `--trace`.
    pub fn emit(&self, level: u8, location: &str, message: &str) {
        if !self.enabled(level) {
            return;
        }
        let tag = match level {
            1 => "v",
            2 => "vv",
            3 => "vvv",
            _ => "debug",
        };
        if self.trace {
            eprintln!("[{tag}] ({location}) {message}");
        } else {
            eprintln!("[{tag}] {message}");
        }
    }

    /// Prints a plain status line, unconditional and un-prefixed (unlike
    /// `warn`/`error`), for routine end-of-run notices such as cache location.
    pub fn info(&self, message: &str) {
        println!("{message}");
    }

    pub fn warn(&self, message: &str) {
        eprintln!("custom-biome-lint: warning: {message}");
    }

    pub fn error(&self, message: &str) {
        eprintln!("custom-biome-lint: error: {message}");
    }

    pub fn print_report(&self, reports: &[FileReport], totals: &Totals, format: OutputFormat) {
        match format {
            OutputFormat::Text => print!("{}", format_reports(reports, totals)),
            OutputFormat::Json => println!("{}", format_reports_json(reports, totals)),
        }
    }

    /// Prints what `--write-fix` changed, or would change in a dry run.
    pub fn print_fix_report(&self, report: &FixReport, cwd: &Path) {
        let mut current: Option<&Path> = None;
        for change in &report.changes {
            let path = change.path.as_path();
            if current != Some(path) {
                if current.is_some() {
                    println!();
                }
                println!("{}", relative(path, cwd).display());
                current = Some(path);
            }
            println!(
                "  {:>5}  + {}  [{}]",
                change.line_number,
                change.comment_added,
                change.placement.label()
            );
        }

        if !report.changes.is_empty() {
            println!();
        }

        let verb = if report.wrote {
            "added to"
        } else {
            "would be added to"
        };
        println!(
            "{} suppression(s) {verb} {} file(s).",
            report.suppressions_added, report.files_modified
        );

        for item in &report.unfixable {
            self.warn(&format!(
                "{}:{}: left unsuppressed ({}): {}",
                relative(&item.path, cwd).display(),
                item.line_number,
                item.reason,
                item.rules.join(", ")
            ));
        }
        for (path, error) in &report.failures {
            self.error(&format!("{}: {error}", relative(path, cwd).display()));
        }

        if !report.wrote && report.suppressions_added > 0 {
            println!("Dry run: nothing was written. Re-run without --dry-run to apply.");
        }
    }

    /// Prints what `--auto-fix` changed, or would change in a dry run.
    pub fn print_autofix_report(&self, report: &AutofixReport, cwd: &Path) {
        let mut current: Option<&Path> = None;
        for fix in &report.fixes_applied {
            let path = fix.path.as_path();
            if current != Some(path) {
                if current.is_some() {
                    println!();
                }
                println!("{}", relative(path, cwd).display());
                current = Some(path);
            }
            println!("  {:>5}  [{}] -> {}", fix.line, fix.rule, fix.replacement);
        }

        if !report.fixes_applied.is_empty() {
            println!();
        }

        let verb = if report.wrote {
            "fixed"
        } else {
            "would be fixed"
        };
        println!(
            "{} violation(s) {verb} in {} file(s).",
            report.fixes_applied.len(),
            report.files_modified
        );

        for item in &report.skipped {
            self.warn(&format!(
                "{}:{}: left unfixed ({}): {}",
                relative(&item.path, cwd).display(),
                item.line,
                item.reason,
                item.rule
            ));
        }
        for (path, error) in &report.failures {
            self.error(&format!("{}: {error}", relative(path, cwd).display()));
        }

        if !report.wrote && !report.fixes_applied.is_empty() {
            println!("Dry run: nothing was written. Re-run without --dry-run to apply.");
        }
    }

    pub fn print_rules(&self, registry: &RuleRegistry) {
        for rule in registry.all() {
            eprintln!(
                "[v]   {} [{}] - {}",
                rule.name(),
                rule.supported_extensions().join(", "),
                rule.description()
            );
        }
    }
}

fn relative<'a>(path: &'a Path, cwd: &Path) -> &'a Path {
    path.strip_prefix(cwd).unwrap_or(path)
}

/// Logs at an explicit verbosity level, capturing the call site for `--trace`.
#[macro_export]
macro_rules! vlog {
    ($reporter:expr, $level:expr, $($arg:tt)*) => {
        if $reporter.enabled($level) {
            $reporter.emit(
                $level,
                concat!(file!(), ":", line!()),
                &format!($($arg)*),
            );
        }
    };
}

/// Logs at debug level (`-d`/`--debug`).
#[macro_export]
macro_rules! dlog {
    ($reporter:expr, $($arg:tt)*) => {
        $crate::vlog!($reporter, 4, $($arg)*)
    };
}
