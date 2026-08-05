use std::path::Path;

use crate::diagnostics::{format_reports, FileReport, Totals};
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
        --dry-run      With --write-fix, report the comments without writing
    -v, --verbose      Standard verbosity (shows rules, skips)
    -vv                Deep verbosity (file discovery, rule execution)
    -vvv               Super verbose (per-file AST details)
    -d, --debug        Debug: internal state, every step
        --trace        Prefix log lines with their source location
    -h, --help         Show this help
    -V, --version      Show version

CONFIGURATION:
    package.json may disable rules by name:

        { \"ignoreBiomeExtensionRules\": [\"no-native-map\"] }

SUPPRESSIONS:
    // biome-ignore-line rule-name[, rule-name2]
    // biome-ignore-next-line rule-name[, rule-name2]

    A marker with no rule names suppresses every rule on its target line.
    Inside JSX children the {/* ... */} form is required, and is what
    --write-fix emits there.

EXIT CODES:
    0    no violations (with --write-fix: everything was suppressed)
    1    violations found, or a violation could not be suppressed
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

    pub fn print_report(&self, reports: &[FileReport], totals: &Totals) {
        print!("{}", format_reports(reports, totals));
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
