pub mod args;
pub mod output;

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

pub use args::{CliArgs, OutputFormat};
pub use output::{Reporter, HELP};

use crate::analyzer::{analyze_file, discover_files, resolve_pattern, GlobSet};
use crate::cache::CacheManager;
use crate::config::PackageConfig;
use crate::diagnostics::{tally, FileReport, Violation};
use crate::fixer::Fixer;
use crate::rules::RuleRegistry;
use crate::{dlog, vlog};
use rayon::prelude::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const EXIT_VIOLATIONS: u8 = 1;
const EXIT_USAGE: u8 = 2;

pub fn run<I>(argv: I, cwd: &Path) -> ExitCode
where
    I: IntoIterator<Item = String>,
{
    let start = Instant::now();

    let args = match CliArgs::parse(argv) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("custom-biome-lint: {message}");
            eprintln!("Try 'custom-biome-lint --help' for usage.");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    if args.help {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if args.version {
        println!("custom-biome-lint {VERSION}");
        return ExitCode::SUCCESS;
    }

    let reporter = Reporter::new(args.log_level(), args.trace);
    let registry = RuleRegistry::with_all_rules();

    dlog!(reporter, "cwd = {}", cwd.display());
    dlog!(reporter, "args = {args:?}");

    let config = PackageConfig::load(cwd);
    for warning in &config.warnings {
        reporter.warn(warning);
    }
    match &config.source {
        Some(path) => vlog!(reporter, 1, "config: {}", path.display()),
        None => vlog!(
            reporter,
            1,
            "config: no package.json found; all rules enabled"
        ),
    }

    let rules = registry.enabled(&config);
    if reporter.enabled(1) {
        vlog!(
            reporter,
            1,
            "{} of {} rule(s) enabled:",
            rules.len(),
            registry.len()
        );
        reporter.print_rules(&registry);
        for ignored in registry.ignored(&config) {
            vlog!(
                reporter,
                1,
                "skipping {} (ignored by config)",
                ignored.name()
            );
        }
    }

    if rules.is_empty() {
        reporter.warn("every rule is disabled by ignoreBiomeExtensionRules; nothing to check");
        return ExitCode::SUCCESS;
    }

    let default_pattern = registry.default_pattern();
    let requested = args.pattern.clone();
    let pattern_input = requested.clone().unwrap_or_else(|| default_pattern.clone());
    let extensions = registry.supported_extensions().join(",");
    let pattern = resolve_pattern(&pattern_input, cwd, &extensions);

    vlog!(reporter, 1, "pattern: {}", pattern.raw());
    vlog!(reporter, 2, "expanded to: {:?}", pattern.alternatives());

    if requested.is_some() {
        report_pattern_extensions(&pattern, &registry, &reporter);
    }

    let root = cwd.join(pattern.root_dir());
    if !root.exists() {
        reporter.error(&format!("path does not exist: {}", root.display()));
        return ExitCode::from(EXIT_USAGE);
    }
    vlog!(reporter, 2, "walk root: {}", root.display());

    let discovery = discover_files(&pattern, cwd);
    vlog!(
        reporter,
        2,
        "discovered {} file(s) from {} considered across {} director(ies)",
        discovery.files.len(),
        discovery.files_considered,
        discovery.dirs_scanned
    );
    for skipped in &discovery.dirs_skipped {
        dlog!(reporter, "skipped directory {}", skipped.display());
    }

    // Initialize cache if not disabled
    let mut cache = if !args.no_cache {
        let mut cm = match CacheManager::new(cwd) {
            Ok(cm) => cm,
            Err(e) => {
                vlog!(reporter, 1, "cache: {e}");
                CacheManager::new(cwd).unwrap() // Fallback to empty cache
            }
        };
        let _ = cm.load();
        cm
    } else {
        CacheManager::new(cwd).unwrap()
    };

    // Compute rule hash to detect rule changes
    let rule_hash = compute_rule_hash(&rules);
    dlog!(reporter, "rule_hash = {rule_hash}");

    // Analyze files (parallel or sequential)
    let (analyzed_files, unparsed, checked) = if args.parallel {
        analyze_files_parallel(&discovery.files, &rules, &rule_hash, &cache, cwd, &reporter)
    } else {
        analyze_files_sequential(&discovery.files, &rules, &rule_hash, &cache, cwd, &reporter)
    };

    // Mark analyzed files as cached
    let mut cache_hits = 0usize;
    for (file, analyzed) in &analyzed_files {
        if analyzed.parsed_cleanly
            && analyzed.violations.is_empty()
            && cache.mark_valid(file, &rule_hash).is_ok()
        {
            cache_hits += 1;
        }
    }

    // Save cache for next run
    let cache_saved = if !args.no_cache {
        cache.save().is_ok()
    } else {
        false
    };

    // Build reports
    let mut reports = Vec::new();
    let mut to_fix: BTreeMap<PathBuf, Vec<Violation>> = BTreeMap::new();

    for (file, analyzed) in analyzed_files {
        if !analyzed.parsed_cleanly {
            reporter.warn(&format!("parse errors in {}", file.display()));
        }

        let source = fs::read_to_string(&file).unwrap_or_default();
        vlog!(
            reporter,
            3,
            "{}: ran [{}], {} violation(s), {} line(s)",
            file.display(),
            analyzed.rules_run.join(", "),
            analyzed.violations.len(),
            source.lines().count()
        );

        if !analyzed.violations.is_empty() {
            if args.write_fix {
                to_fix.insert(file.clone(), analyzed.violations.clone());
            }
            reports.push(FileReport::new(
                display_path(&file, cwd),
                analyzed.violations,
            ));
        }
    }

    // Report cache statistics in verbose mode
    if !args.no_cache && reporter.enabled(1) {
        let (cached_count, _) = cache.stats();
        vlog!(
            reporter,
            1,
            "cache: {} file(s) cached, {} hit(s)",
            cached_count,
            cache_hits
        );
    }

    if unparsed > 0 {
        reporter.warn(&format!("{unparsed} file(s) had parse errors"));
    }

    if args.write_fix {
        let fixed = Fixer::apply_suppressions(&to_fix, !args.dry_run);
        reporter.print_fix_report(&fixed, cwd);

        if cache_saved {
            report_cache_location(&reporter, &cache, cwd);
        }

        // A dry run leaves the violations in place, so it reports failure
        // whenever there is anything left to write.
        let clean = fixed.is_complete() && (fixed.wrote || fixed.changes.is_empty());
        return if clean {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(EXIT_VIOLATIONS)
        };
    }

    let mut totals = tally(&reports, checked);
    totals.elapsed = start.elapsed();
    reporter.print_report(&reports, &totals, args.format);

    // The cache-location notice goes to stdout, so it must not appear in
    // --format json mode: that stream is a single machine-readable document,
    // and appending a plain-text line after it would make the output invalid
    // JSON for any consumer that doesn't stop reading after the first value.
    if cache_saved && args.format == OutputFormat::Text {
        report_cache_location(&reporter, &cache, cwd);
    }

    if totals.errors > 0 {
        ExitCode::from(EXIT_VIOLATIONS)
    } else {
        ExitCode::SUCCESS
    }
}

/// Prints where the incremental cache was written, relative to the run
/// directory, so it's obvious that a `.custom-biome-lint-cache/` directory
/// showing up in the project is this tool and not stray npm state.
fn report_cache_location(reporter: &Reporter, cache: &CacheManager, cwd: &Path) {
    reporter.info(&format!(
        "cache saved at {}",
        display_path(cache.cache_dir(), cwd).display()
    ));
}

/// Warns when an explicit pattern targets extensions no rule can analyze.
/// Verbose-only: a narrower pattern is a legitimate thing to do, so this must
/// not add noise to normal runs.
fn report_pattern_extensions(pattern: &GlobSet, registry: &RuleRegistry, reporter: &Reporter) {
    if !reporter.enabled(1) {
        return;
    }

    let supported = registry.supported_extensions();
    let requested = pattern.extensions();

    if requested.is_empty() {
        vlog!(
            reporter,
            1,
            "pattern has no literal extension; files are filtered by each rule's supported extensions ({})",
            supported.join(", ")
        );
        return;
    }

    let unsupported: Vec<&String> = requested
        .iter()
        .filter(|ext| !supported.contains(&ext.as_str()))
        .collect();

    if unsupported.is_empty() {
        vlog!(
            reporter,
            1,
            "pattern extensions ok: {}",
            requested.join(", ")
        );
        return;
    }

    for ext in &unsupported {
        reporter.warn(&format!(
            "pattern matches .{ext} files, which no enabled rule supports (supported: {})",
            supported.join(", ")
        ));
    }
}

fn display_path(path: &Path, cwd: &Path) -> PathBuf {
    path.strip_prefix(cwd).unwrap_or(path).to_path_buf()
}

/// Compute a hash of all enabled rules to detect rule changes.
fn compute_rule_hash(rules: &[&dyn crate::Rule]) -> String {
    // Sorted so the hash is independent of registration order, and joined with
    // a separator so `["ab", "c"]` and `["a", "bc"]` cannot collide.
    let mut rule_names: Vec<&str> = rules.iter().map(|r| r.name()).collect();
    rule_names.sort_unstable();

    let mut hasher = DefaultHasher::new();
    rule_names.join(",").hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Analyze files sequentially.
fn analyze_files_sequential(
    files: &[PathBuf],
    rules: &[&dyn crate::Rule],
    rule_hash: &str,
    cache: &CacheManager,
    _cwd: &Path,
    reporter: &Reporter,
) -> (Vec<(PathBuf, crate::analyzer::AnalyzedFile)>, usize, usize) {
    let mut analyzed = Vec::new();
    let mut unparsed = 0usize;
    let mut checked = 0usize;

    for file in files {
        // Check cache first
        if cache.is_valid(file, rule_hash) {
            dlog!(reporter, "cache hit: {}", file.display());
            continue;
        }

        let source = match fs::read_to_string(file) {
            Ok(source) => source,
            Err(error) => {
                reporter.warn(&format!("could not read {}: {error}", file.display()));
                continue;
            }
        };
        checked += 1;

        let result = analyze_file(file, &source, rules);
        if !result.parsed_cleanly {
            unparsed += 1;
        }
        analyzed.push((file.clone(), result));
    }

    (analyzed, unparsed, checked)
}

/// Analyze files in parallel using rayon.
fn analyze_files_parallel(
    files: &[PathBuf],
    rules: &[&dyn crate::Rule],
    rule_hash: &str,
    cache: &CacheManager,
    _cwd: &Path,
    _reporter: &Reporter,
) -> (Vec<(PathBuf, crate::analyzer::AnalyzedFile)>, usize, usize) {
    // First pass: filter out cached files
    let files_to_analyze: Vec<_> = files
        .iter()
        .filter(|f| !cache.is_valid(f, rule_hash))
        .cloned()
        .collect();

    // Parallel analysis
    let analyzed: Vec<_> = files_to_analyze
        .into_par_iter()
        .map(|file| {
            let source = fs::read_to_string(&file).unwrap_or_default();
            let result = analyze_file(&file, &source, rules);
            (file, result)
        })
        .collect();

    // Count metrics
    let unparsed = analyzed.iter().filter(|(_, a)| !a.parsed_cleanly).count();
    let checked = analyzed.len();

    (analyzed, unparsed, checked)
}
