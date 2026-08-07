/// Output format for the diagnostics report.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CliArgs {
    /// Glob pattern to lint. `None` means "use the registry's default".
    pub pattern: Option<String>,
    /// 0 = quiet, 1 = -v, 2 = -vv, 3 = -vvv.
    pub verbosity: u8,
    pub debug: bool,
    pub trace: bool,
    pub help: bool,
    pub version: bool,
    /// Add suppression comments for every violation instead of reporting them.
    pub write_fix: bool,
    /// Rewrite violations in place using each rule's own fix, instead of
    /// reporting them. Only violations whose rule produced a [`crate::diagnostics::Fix`]
    /// are touched; the rest are reported as skipped.
    pub auto_fix: bool,
    /// With `write_fix` or `auto_fix`, report the changes that would be made
    /// without touching any file.
    pub dry_run: bool,
    /// Enable parallel file analysis using rayon (default: true).
    pub parallel: bool,
    /// Disable incremental caching (default: false, caching is enabled).
    pub no_cache: bool,
    /// How to print the diagnostics report (default: human-readable text).
    pub format: OutputFormat,
}

impl CliArgs {
    pub fn parse<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut parsed = Self {
            parallel: true,
            ..Default::default()
        };

        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-h" | "--help" => parsed.help = true,
                "-V" | "--version" => parsed.version = true,
                "--verbose" => parsed.verbosity = parsed.verbosity.max(1),
                "--debug" => parsed.debug = true,
                "--trace" => parsed.trace = true,
                "--write-fix" => parsed.write_fix = true,
                "--auto-fix" => parsed.auto_fix = true,
                "--dry-run" => parsed.dry_run = true,
                "--parallel" => parsed.parallel = true,
                "--no-parallel" => parsed.parallel = false,
                "--no-cache" => parsed.no_cache = true,
                "--format" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--format requires a value: text or json".to_string())?;
                    parsed.format = match value.as_str() {
                        "text" => OutputFormat::Text,
                        "json" => OutputFormat::Json,
                        other => {
                            return Err(format!(
                                "unknown --format value: {other} (expected text or json)"
                            ))
                        }
                    };
                }
                other if other.starts_with("--") => {
                    return Err(format!("unknown flag: {other}"));
                }
                // Short flags, including repeated and clustered forms (-vv, -vvv, -vd).
                other if other.starts_with('-') && other.len() > 1 => {
                    for ch in other.chars().skip(1) {
                        match ch {
                            'v' => parsed.verbosity = parsed.verbosity.saturating_add(1).min(3),
                            'd' => parsed.debug = true,
                            'h' => parsed.help = true,
                            _ => return Err(format!("unknown flag: -{ch}")),
                        }
                    }
                }
                positional => {
                    if parsed.pattern.is_some() {
                        return Err(format!(
                            "unexpected extra argument: {positional} (only one pattern is accepted)"
                        ));
                    }
                    parsed.pattern = Some(positional.to_string());
                }
            }
        }

        if parsed.dry_run && !parsed.write_fix && !parsed.auto_fix {
            return Err("--dry-run only applies to --write-fix or --auto-fix".to_string());
        }
        if parsed.write_fix && parsed.format == OutputFormat::Json {
            return Err("--format json is not supported together with --write-fix".to_string());
        }
        if parsed.auto_fix && parsed.format == OutputFormat::Json {
            return Err("--format json is not supported together with --auto-fix".to_string());
        }
        if parsed.write_fix && parsed.auto_fix {
            return Err(
                "--write-fix and --auto-fix are two different fix mechanisms; run one, look at the result, then the other"
                    .to_string(),
            );
        }

        Ok(parsed)
    }

    /// Effective log level. `--debug` outranks `-vvv`.
    pub fn log_level(&self) -> u8 {
        if self.debug {
            4
        } else {
            self.verbosity
        }
    }

    pub fn is_verbose(&self) -> bool {
        self.log_level() >= 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<CliArgs, String> {
        CliArgs::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_are_quiet_with_no_pattern() {
        let args = parse(&[]).unwrap();
        assert_eq!(args.verbosity, 0);
        assert_eq!(args.pattern, None);
        assert!(!args.is_verbose());
    }

    #[test]
    fn repeated_v_raises_verbosity() {
        assert_eq!(parse(&["-v"]).unwrap().verbosity, 1);
        assert_eq!(parse(&["-vv"]).unwrap().verbosity, 2);
        assert_eq!(parse(&["-vvv"]).unwrap().verbosity, 3);
        assert_eq!(parse(&["-v", "-v"]).unwrap().verbosity, 2);
    }

    #[test]
    fn verbosity_saturates_at_three() {
        assert_eq!(parse(&["-vvvvv"]).unwrap().verbosity, 3);
    }

    #[test]
    fn clustered_short_flags() {
        let args = parse(&["-vd"]).unwrap();
        assert_eq!(args.verbosity, 1);
        assert!(args.debug);
        assert_eq!(args.log_level(), 4);
    }

    #[test]
    fn pattern_is_positional() {
        assert_eq!(
            parse(&["src/**/*.js"]).unwrap().pattern.as_deref(),
            Some("src/**/*.js")
        );
    }

    #[test]
    fn rejects_unknown_and_duplicate_arguments() {
        assert!(parse(&["--nope"]).is_err());
        assert!(parse(&["-z"]).is_err());
        assert!(parse(&["a", "b"]).is_err());
    }

    #[test]
    fn write_fix_defaults_to_writing() {
        let args = parse(&["--write-fix"]).unwrap();
        assert!(args.write_fix);
        assert!(!args.dry_run);
    }

    #[test]
    fn dry_run_requires_write_fix() {
        assert!(parse(&["--write-fix", "--dry-run"]).unwrap().dry_run);
        assert!(parse(&["--dry-run"]).is_err());
    }

    #[test]
    fn auto_fix_defaults_to_writing() {
        let args = parse(&["--auto-fix"]).unwrap();
        assert!(args.auto_fix);
        assert!(!args.dry_run);
    }

    #[test]
    fn dry_run_also_applies_to_auto_fix() {
        assert!(parse(&["--auto-fix", "--dry-run"]).unwrap().dry_run);
    }

    #[test]
    fn write_fix_and_auto_fix_cannot_be_combined() {
        assert!(parse(&["--write-fix", "--auto-fix"]).is_err());
        assert!(parse(&["--auto-fix", "--write-fix"]).is_err());
    }

    #[test]
    fn json_format_is_rejected_with_auto_fix() {
        assert!(parse(&["--auto-fix", "--format", "json"]).is_err());
        assert!(parse(&["--format", "json", "--auto-fix"]).is_err());
        assert!(parse(&["--format", "text", "--auto-fix"]).is_ok());
    }

    #[test]
    fn parallel_defaults_to_true() {
        assert!(parse(&[]).unwrap().parallel);
    }

    #[test]
    fn parallel_flag_overrides() {
        assert!(parse(&["--parallel"]).unwrap().parallel);
        assert!(!parse(&["--no-parallel"]).unwrap().parallel);
    }

    #[test]
    fn no_cache_flag() {
        assert!(!parse(&[]).unwrap().no_cache);
        assert!(parse(&["--no-cache"]).unwrap().no_cache);
    }

    #[test]
    fn combined_parallel_and_cache_flags() {
        let args = parse(&["--no-parallel", "--no-cache", "src/**/*.js"]).unwrap();
        assert!(!args.parallel);
        assert!(args.no_cache);
        assert_eq!(args.pattern.as_deref(), Some("src/**/*.js"));
    }

    #[test]
    fn format_defaults_to_text() {
        assert_eq!(parse(&[]).unwrap().format, OutputFormat::Text);
    }

    #[test]
    fn format_flag_accepts_json_and_text() {
        assert_eq!(
            parse(&["--format", "json"]).unwrap().format,
            OutputFormat::Json
        );
        assert_eq!(
            parse(&["--format", "text"]).unwrap().format,
            OutputFormat::Text
        );
    }

    #[test]
    fn format_flag_rejects_unknown_value_and_missing_value() {
        assert!(parse(&["--format", "yaml"]).is_err());
        // Nothing follows "--format" here, so it must error rather than
        // silently leave the format at its default.
        assert!(parse(&["src/**/*.js", "--format"]).is_err());
    }

    #[test]
    fn json_format_is_rejected_with_write_fix() {
        // --write-fix's report has its own shape; conflating the two flags
        // would either need to invent a --write-fix JSON schema no one asked
        // for yet, or silently ignore --format, so reject the combination.
        assert!(parse(&["--write-fix", "--format", "json"]).is_err());
        assert!(parse(&["--format", "json", "--write-fix"]).is_err());
        assert!(parse(&["--format", "text", "--write-fix"]).is_ok());
    }
}
