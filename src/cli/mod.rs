pub mod args;
pub mod output;

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use serde_json::json;

pub use args::{CliArgs, OutputFormat};
pub use output::{Reporter, HELP};

use crate::analyzer::{analyze_file, discover_files, resolve_pattern, GlobSet};
use crate::autofix::Autofix;
use crate::cache::{hash_content, CacheManager};
use crate::config::{PackageConfig, RuleSeverity, CONFIG_KEY};
use crate::diagnostics::{tally, FileReport, Severity, Violation};
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

    // Rule metadata is a standalone, side-effect-free query — it does not need
    // a package.json, a pattern, or any file on disk.
    if args.rules {
        print_rule_metadata(&RuleRegistry::with_all_rules());
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
            "config: no package.json found; every default-on rule enabled"
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
        // Two different reasons a rule is not running, worth telling apart:
        // the config turned it off, or it ships off and nothing turned it on.
        // Only the second is actionable, and only it needs the hint.
        for ignored in registry.ignored(&config) {
            let reason = if config.severities.contains_key(ignored.name()) {
                "ignored by config".to_string()
            } else {
                format!(
                    "off by default; enable with {CONFIG_KEY}: {{ \"{}\": \"error\" }}",
                    ignored.name()
                )
            };
            vlog!(reporter, 1, "skipping {} ({reason})", ignored.name());
        }
    }

    // `custom-biome-lint --stdin <path> --format json` lints one in-memory
    // document (the IDE streams the open editor buffer). It skips discovery,
    // caching, and fix modes (those are rejected at parse time) and reuses the
    // exact same analysis and enrichment path as a normal run.
    if args.stdin {
        return run_stdin(&args, cwd, &reporter, &config, &registry);
    }

    if rules.is_empty() {
        reporter.warn(&format!(
            "no rules are enabled: every rule is either disabled by {CONFIG_KEY} or ships off by default; nothing to check"
        ));
        // --format json must always produce a document on stdout, even here:
        // returning immediately (as this used to) left a CI consumer parsing
        // stdout as JSON with nothing to parse. --write-fix/--auto-fix have no
        // rules to fix either way, so they still return early without a report.
        if !args.write_fix && !args.auto_fix {
            let mut totals = tally(&[], 0, 0);
            totals.elapsed = start.elapsed();
            reporter.print_report(&[], &totals, args.format);
        }
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

    // Compute the cache key: enabled rules + tool version. Either changing
    // invalidates every cached file at once (see compute_cache_key).
    let cache_key = compute_cache_key(&rules);
    dlog!(reporter, "cache_key = {cache_key}");

    // Analyze files (parallel or sequential)
    let (analyzed_files, unparsed, checked, cache_skipped) = if args.parallel {
        analyze_files_parallel(
            &discovery.files,
            &rules,
            &cache_key,
            &cache,
            args.format == OutputFormat::Json,
        )
    } else {
        analyze_files_sequential(
            &discovery.files,
            &rules,
            &cache_key,
            &cache,
            &reporter,
            args.format == OutputFormat::Json,
        )
    };

    // Mark freshly-analyzed, clean files as cached for next run. Distinct
    // from `cache_skipped` above: that counts files the cache already had
    // and this run never even re-read the AST for; this counts files this
    // run analyzed from scratch that turned out clean and so get cached now.
    let mut newly_cached = 0usize;
    for (file, source, analyzed) in &analyzed_files {
        if analyzed.parsed_cleanly && analyzed.violations.is_empty() {
            cache.mark_valid(file, &hash_content(source), &cache_key);
            newly_cached += 1;
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
    // Carries the exact source each violation's Fix offsets were computed
    // against, so Autofix::apply can detect a file that changed on disk
    // between analysis and here and refuse to apply now-stale offsets to it.
    let mut to_autofix: BTreeMap<PathBuf, (String, Vec<Violation>)> = BTreeMap::new();

    for (file, source, mut analyzed) in analyzed_files {
        if !analyzed.parsed_cleanly {
            reporter.warn(&format!("parse errors in {}", file.display()));
        }

        apply_severity_overrides(&mut analyzed.violations, &config);

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
            if args.auto_fix {
                to_autofix.insert(file.clone(), (source.clone(), analyzed.violations.clone()));
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
            "cache: {} file(s) cached total, {} skipped this run (already valid), \
             {} newly marked clean this run",
            cached_count,
            cache_skipped,
            newly_cached
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

    if args.auto_fix {
        let fixed = Autofix::apply(&to_autofix, !args.dry_run);
        reporter.print_autofix_report(&fixed, cwd);

        if cache_saved {
            report_cache_location(&reporter, &cache, cwd);
        }

        // A dry run leaves the violations in place, so it reports failure
        // whenever there is anything left to write.
        let clean = fixed.is_complete() && (fixed.wrote || fixed.fixes_applied.is_empty());
        return if clean {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(EXIT_VIOLATIONS)
        };
    }

    let mut totals = tally(&reports, checked, cache_skipped);
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

/// Applies package.json's per-rule "warn"/"error" severity overrides.
/// "off" never reaches here: `RuleRegistry::enabled` already keeps an
/// off rule from running at all, so it has no violations to override.
fn apply_severity_overrides(violations: &mut [Violation], config: &PackageConfig) {
    for violation in violations {
        match config.severity_override(violation.rule) {
            Some(RuleSeverity::Warn) => violation.severity = Severity::Warning,
            Some(RuleSeverity::Error) => violation.severity = Severity::Error,
            Some(RuleSeverity::Off) | None => {}
        }
    }
}

/// Computes the cache key that gates every cached entry at once: the enabled
/// rule set plus the tool's own version. Either changing means past cache
/// entries can no longer be trusted -- a rule's detection logic or a rule
/// being turned on/off can change what a given file's content produces, and
/// so can a tool upgrade, even though the file itself never changed.
fn compute_cache_key(rules: &[&dyn crate::Rule]) -> String {
    // Sorted so the hash is independent of registration order, and joined with
    // a separator so `["ab", "c"]` and `["a", "bc"]` cannot collide.
    let mut rule_names: Vec<&str> = rules.iter().map(|r| r.name()).collect();
    rule_names.sort_unstable();

    let mut hasher = DefaultHasher::new();
    rule_names.join(",").hash(&mut hasher);
    VERSION.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Analyze files sequentially. Each file is read once; its content hash
/// decides cache validity, and the same in-memory source is reused for
/// parsing (and later for verbose line-count logging) if it's not.
fn analyze_files_sequential(
    files: &[PathBuf],
    rules: &[&dyn crate::Rule],
    cache_key: &str,
    cache: &CacheManager,
    reporter: &Reporter,
    enrich: bool,
) -> (
    Vec<(PathBuf, String, crate::analyzer::AnalyzedFile)>,
    usize,
    usize,
    usize,
) {
    let mut analyzed = Vec::new();
    let mut unparsed = 0usize;
    let mut checked = 0usize;
    let mut cache_skipped = 0usize;

    for file in files {
        let source = match fs::read_to_string(file) {
            Ok(source) => source,
            Err(error) => {
                reporter.warn(&format!("could not read {}: {error}", file.display()));
                continue;
            }
        };

        if cache.is_valid(file, &hash_content(&source), cache_key) {
            dlog!(reporter, "cache hit: {}", file.display());
            cache_skipped += 1;
            continue;
        }
        checked += 1;

        let result = analyze_file(file, &source, rules, enrich);
        if !result.parsed_cleanly {
            unparsed += 1;
        }
        analyzed.push((file.clone(), source, result));
    }

    (analyzed, unparsed, checked, cache_skipped)
}

/// Analyze files in parallel using rayon. Same read-once, hash-to-decide
/// approach as the sequential path, just fanned out across files.
fn analyze_files_parallel(
    files: &[PathBuf],
    rules: &[&dyn crate::Rule],
    cache_key: &str,
    cache: &CacheManager,
    enrich: bool,
) -> (
    Vec<(PathBuf, String, crate::analyzer::AnalyzedFile)>,
    usize,
    usize,
    usize,
) {
    let cache_skipped = std::sync::atomic::AtomicUsize::new(0);

    let analyzed: Vec<_> = files
        .par_iter()
        .filter_map(|file| {
            let source = fs::read_to_string(file).ok()?;
            if cache.is_valid(file, &hash_content(&source), cache_key) {
                cache_skipped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return None;
            }
            let result = analyze_file(file, &source, rules, enrich);
            Some((file.clone(), source, result))
        })
        .collect();

    // Count metrics
    let unparsed = analyzed
        .iter()
        .filter(|(_, _, a)| !a.parsed_cleanly)
        .count();
    let checked = analyzed.len();

    (
        analyzed,
        unparsed,
        checked,
        cache_skipped.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// Lints a single document supplied on stdin. The positional `pattern` is the
/// file path (used for extension-based rule selection and as the reported
/// path). Reuses `analyze_file` — including the IDE enrichment that attaches
/// fix/suppression edits when `--format json` is set — so stdin output is
/// byte-for-byte the same contract as a file run.
fn run_stdin(
    args: &CliArgs,
    cwd: &Path,
    reporter: &Reporter,
    config: &PackageConfig,
    registry: &RuleRegistry,
) -> ExitCode {
    let path = PathBuf::from(
        args.pattern
            .clone()
            .expect("stdin requires a path (validated)"),
    );

    let mut source = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut source) {
        reporter.error(&format!("failed to read stdin: {error}"));
        return ExitCode::from(EXIT_USAGE);
    }

    let rules = registry.enabled(config);
    let mut analyzed = analyze_file(&path, &source, &rules, args.format == OutputFormat::Json);

    if !analyzed.parsed_cleanly {
        reporter.warn(&format!("parse errors in {}", path.display()));
    }
    apply_severity_overrides(&mut analyzed.violations, config);

    let mut reports = Vec::new();
    if !analyzed.violations.is_empty() {
        reports.push(FileReport::new(
            display_path(&path, cwd),
            analyzed.violations,
        ));
    }

    let mut totals = tally(&reports, 1, 0);
    totals.elapsed = std::time::Instant::now().elapsed();
    reporter.print_report(&reports, &totals, args.format);

    if totals.errors > 0 {
        ExitCode::from(EXIT_VIOLATIONS)
    } else {
        ExitCode::SUCCESS
    }
}

/// Prints stable, machine-readable metadata for every rule. The data comes from
/// the rule registry (name/description/default severity/extensions) — never a
/// duplicated hard-coded table — so it cannot drift from the real rules.
fn print_rule_metadata(registry: &RuleRegistry) {
    let rules: Vec<serde_json::Value> = registry
        .all()
        .iter()
        .map(|rule| {
            json!({
                "name": rule.name(),
                "description": rule.description(),
                "defaultSeverity": rule.default_severity().label(),
                "enabledByDefault": rule.default_severity() != RuleSeverity::Off,
                "supportedExtensions": rule
                    .supported_extensions()
                    .iter()
                    .map(|ext| ext.trim_start_matches('.'))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    let doc = json!({ "version": 1, "rules": rules });
    // A plain `json!` tree of owned values never fails to serialize.
    println!(
        "{}",
        serde_json::to_string_pretty(&doc).expect("rule metadata is always serializable")
    );
}
