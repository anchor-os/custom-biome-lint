use std::path::{Path, PathBuf};

/// A glob pattern expanded into brace-free alternatives.
///
/// Supports `*`, `?`, `**` and `{a,b}` brace sets — enough for the
/// `src/**/*.{js,jsx}` shapes this tool is pointed at, without pulling in a
/// glob crate.
#[derive(Debug, Clone)]
pub struct GlobSet {
    raw: String,
    alternatives: Vec<String>,
}

impl GlobSet {
    pub fn new(pattern: &str) -> Self {
        let normalized = normalize(pattern);
        Self {
            alternatives: expand_braces(&normalized),
            raw: normalized,
        }
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn alternatives(&self) -> &[String] {
        &self.alternatives
    }

    pub fn is_match(&self, path: &str) -> bool {
        let path = normalize(path);
        self.alternatives
            .iter()
            .any(|pattern| matches_pattern(pattern, &path))
    }

    /// The deepest directory that every alternative shares before its first
    /// wildcard — the walk root, so `src/**/*.js` never scans outside `src`.
    pub fn root_dir(&self) -> PathBuf {
        let mut common: Option<Vec<&str>> = None;
        for alternative in &self.alternatives {
            let literal = literal_prefix(alternative);
            common = Some(match common {
                None => literal,
                Some(existing) => existing
                    .into_iter()
                    .zip(literal)
                    .take_while(|(a, b)| a == b)
                    .map(|(a, _)| a)
                    .collect(),
            });
        }
        match common {
            // No literal prefix. This is either an all-wildcard prefix
            // (`*`, `**`, `*.js`) or a bare Unix root `/` — `normalize` strips
            // the trailing slash, so `/` collapses to an empty `raw`. The former
            // starts the walk at cwd; the latter is the filesystem root.
            Some(segments) if segments.is_empty() => {
                if self.raw.is_empty() {
                    PathBuf::from("/")
                } else {
                    PathBuf::from(".")
                }
            }
            Some(segments) => {
                let joined = segments.join("/");
                if joined.is_empty() {
                    // Bare Unix root `/`: `normalize` strips the trailing slash,
                    // collapsing the literal prefix to `[""]` and the joined
                    // path to `""`. Map it back to the filesystem root so
                    // discovery starts at `/` rather than the cwd.
                    PathBuf::from("/")
                } else if joined.len() == 2
                    && (is_drive_path(&joined) || is_bare_drive_prefix(&joined))
                {
                    // Bare Windows drive root: a pattern like `C:/**/*.js` has a
                    // literal prefix of just `["C:"]` (the `**` ends the prefix),
                    // so `joined` is `C:` without a trailing slash. `is_drive_path`
                    // requires a following `/`, so it misses this case. Re-add the
                    // slash so the path resolves to the drive root `C:/` instead
                    // of a drive-relative location (which then fails `root.exists()`
                    // and makes the run emit no stdout).
                    PathBuf::from(format!("{joined}/"))
                } else {
                    // Rebuild the literal prefix as a single path so Windows
                    // drive prefixes (`C:`) and Unix roots (`/`) survive intact.
                    // Building it manually with a leading `"/"` plus `extend`
                    // would discard a `C:` drive prefix, turning an absolute
                    // `C:/Users/...` root into the drive-relative (and
                    // non-existent) `C:Users/...`, which then fails
                    // `root.exists()` and makes the run emit no stdout at all.
                    PathBuf::from(joined)
                }
            }
            None => PathBuf::from("."),
        }
    }

    pub fn is_absolute(&self) -> bool {
        // `normalize` rewrites backslashes to forward slashes, so a Windows
        // drive path arrives as `C:/Users/...` — still absolute, even though it
        // has no leading slash. `Path::is_absolute` would only catch the Unix
        // `/`-root form, so check the drive prefix explicitly too. A bare Unix
        // root `/` normalizes to `""` and a bare drive root `C:/` to `C:`, both
        // of which must still count as absolute.
        self.raw.starts_with('/') || self.raw.is_empty() || is_drive_path(&self.raw)
    }

    /// File extensions the pattern can match, without the leading dot.
    /// Empty when the pattern's final segment has no literal extension.
    pub fn extensions(&self) -> Vec<String> {
        let mut found = Vec::new();
        for alternative in &self.alternatives {
            let last = alternative.rsplit('/').next().unwrap_or(alternative);
            let Some((_, ext)) = last.rsplit_once('.') else {
                continue;
            };
            if ext.is_empty() || ext.contains(['*', '?']) {
                continue;
            }
            if !found.iter().any(|e: &String| e == ext) {
                found.push(ext.to_string());
            }
        }
        found
    }
}

pub fn has_magic(pattern: &str) -> bool {
    pattern.contains(['*', '?', '{'])
}

fn normalize(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    let trimmed = replaced.trim_start_matches("./");
    trimmed.trim_end_matches('/').to_string()
}

/// True for a Windows drive path like `C:/Users` (after `normalize` has turned
/// backslashes into forward slashes). Such a path is absolute by `Path`'s
/// definition on Windows, but has no leading `/`, so the simple `starts_with('/')`
/// check misses it.
fn is_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.first().is_some_and(|c| c.is_ascii_alphabetic())
        && bytes.get(1) == Some(&b':')
        && bytes.get(2) == Some(&b'/')
}

/// True for a bare Windows drive prefix like `C:` (just a letter and colon, no
/// path yet). Unlike [`is_drive_path`], this has no trailing slash, so it fires
/// for the literal prefix of a pattern like `C:/**/*.js` where the `**` ends the
/// prefix before any slash is consumed.
fn is_bare_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn literal_prefix(pattern: &str) -> Vec<&str> {
    let segments: Vec<&str> = pattern.split('/').collect();
    let literal = segments
        .iter()
        .take_while(|segment| !segment.contains(['*', '?']))
        .count();
    if literal == segments.len() {
        // Fully literal pattern: the final segment names the file, not a directory.
        segments[..literal.saturating_sub(1)].to_vec()
    } else {
        segments[..literal].to_vec()
    }
}

/// Expands `{a,b}` sets into one pattern per combination, innermost-first.
pub fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };

    let mut depth = 0usize;
    let mut close = None;
    for (index, byte) in pattern.bytes().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(index);
                    break;
                }
            }
            _ => {}
        }
    }

    let Some(close) = close else {
        return vec![pattern.to_string()];
    };

    let prefix = &pattern[..open];
    let body = &pattern[open + 1..close];
    let suffix = &pattern[close + 1..];

    let mut out = Vec::new();
    for option in split_top_level(body) {
        for expanded in expand_braces(&format!("{prefix}{option}{suffix}")) {
            out.push(expanded);
        }
    }
    out
}

fn split_top_level(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in body.chars() {
        match ch {
            '{' => {
                depth += 1;
                current.push(ch);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    parts.push(current);
    parts
}

fn matches_pattern(pattern: &str, path: &str) -> bool {
    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let path_segments: Vec<&str> = path.split('/').collect();
    match_segments(&pattern_segments, &path_segments)
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => {
            if rest.is_empty() {
                return true;
            }
            (0..=path.len()).any(|skip| match_segments(rest, &path[skip..]))
        }
        Some((segment, rest)) => match path.split_first() {
            Some((candidate, path_rest)) if match_segment(segment, candidate) => {
                match_segments(rest, path_rest)
            }
            _ => false,
        },
    }
}

/// Wildcard match within a single path segment: `*` spans any run of characters,
/// `?` exactly one. Greedy with backtracking on the last `*`.
fn match_segment(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();

    let mut p = 0usize;
    let mut t = 0usize;
    let mut star: Option<usize> = None;
    let mut resume = 0usize;

    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            resume = t;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            resume += 1;
            t = resume;
        } else {
            return false;
        }
    }

    pattern[p..].iter().all(|c| *c == '*')
}

/// Relative, forward-slashed path used for matching.
pub fn relative_key(path: &Path, root: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    normalize(&relative.to_string_lossy())
}

/// Forward-slashed key to match `path` against `pattern`. An absolute pattern
/// has to be compared against the absolute path: stripping the walk root would
/// leave a relative key that no absolute alternative can match.
pub fn match_key(path: &Path, root: &Path, pattern: &GlobSet) -> String {
    if pattern.is_absolute() {
        normalize(&path.to_string_lossy())
    } else {
        relative_key(path, root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_brace_sets() {
        let mut expanded = expand_braces("src/**/*.{js,jsx}");
        expanded.sort();
        assert_eq!(expanded, vec!["src/**/*.js", "src/**/*.jsx"]);
    }

    #[test]
    fn double_star_spans_directories() {
        let set = GlobSet::new("src/**/*.{js,jsx}");
        assert!(set.is_match("src/a.js"));
        assert!(set.is_match("src/deep/nested/b.jsx"));
        assert!(!set.is_match("src/a.ts"));
        assert!(!set.is_match("other/a.js"));
    }

    #[test]
    fn single_star_stays_within_a_segment() {
        let set = GlobSet::new("src/*.js");
        assert!(set.is_match("src/a.js"));
        assert!(!set.is_match("src/deep/a.js"));
    }

    #[test]
    fn root_dir_stops_at_first_wildcard() {
        assert_eq!(GlobSet::new("src/**/*.js").root_dir(), PathBuf::from("src"));
        assert_eq!(
            GlobSet::new("src/store/**/*.js").root_dir(),
            PathBuf::from("src/store")
        );
        assert_eq!(GlobSet::new("*.js").root_dir(), PathBuf::from("."));
    }

    #[test]
    fn absolute_patterns_keep_an_absolute_root() {
        let set = GlobSet::new("/tmp/proj/src/**/*.js");
        assert!(set.is_absolute());
        assert_eq!(set.root_dir(), PathBuf::from("/tmp/proj/src"));

        // A Unix-style pattern's root_dir() has a root but, on Windows, no
        // drive prefix -- there's no drive letter to invent for `/tmp/...`.
        // `Path::is_absolute()` on Windows requires a prefix, so it reports
        // false for a bare rooted-but-unprefixed path even though it's
        // correctly rooted; `has_root()` is the check that actually holds on
        // every platform. The property that matters in practice -- cli/mod.rs
        // and analyzer/mod.rs always `.join()` this onto a real base directory,
        // never use the bare value -- is exercised directly below: per
        // PathBuf::push's documented Windows behavior, joining a rooted-but-
        // unprefixed path replaces everything except the base's own prefix,
        // so the result is genuinely absolute on every platform.
        assert!(set.root_dir().has_root());
        let joined = std::env::current_dir().unwrap().join(set.root_dir());
        assert!(joined.is_absolute());

        assert!(set.is_match("/tmp/proj/src/a/b.js"));
        assert!(!set.is_match("src/a/b.js"));
    }

    #[test]
    fn bare_unix_root_resolves_to_filesystem_root() {
        // `normalize` strips the trailing slash, so `/` collapses to `""` and
        // `/**`/`/*` keep only a root segment. The walk root must still be the
        // filesystem root `/`, not cwd, and the pattern must count as absolute.
        for pattern in ["/", "/**/*.js", "/*"] {
            let set = GlobSet::new(pattern);
            assert!(set.is_absolute(), "{pattern} should be absolute");
            assert_eq!(set.root_dir(), PathBuf::from("/"), "{pattern} root_dir");
        }
    }

    #[test]
    fn bare_windows_drive_root_resolves_to_drive_root() {
        // `C:/**/*.js` keeps only the `C:` literal prefix after the `**`; the
        // root must be the drive root `C:/`, not a drive-relative path.
        let set = GlobSet::new("C:/**/*.js");
        assert!(set.is_absolute());
        assert_eq!(set.root_dir(), PathBuf::from("C:/"));
    }

    #[test]
    fn windows_drive_path_is_absolute_and_keeps_prefix() {
        // On Windows a caller passes an absolute path with backslashes, e.g. the
        // temp file a CLI test lints. `normalize` rewrites them to forward
        // slashes; the path must still count as absolute and its walk root must
        // preserve the `C:` drive prefix. Losing it (the old behavior) produced
        // a drive-relative, non-existent root that made the run emit no stdout,
        // which the IDE JSON contract tests then failed to parse.
        let set = GlobSet::new(r"C:\Users\runner\AppData\Local\Temp\cbl-ide.js");
        assert!(set.is_absolute());
        assert_eq!(
            set.root_dir(),
            PathBuf::from("C:/Users/runner/AppData/Local/Temp")
        );
        // The literal file is the root_dir's only entry when walked directly.
        assert!(set.is_match("C:/Users/runner/AppData/Local/Temp/cbl-ide.js"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_root_is_absolute_on_windows() {
        let set = GlobSet::new("C:/Users/runner/AppData/Local/Temp/cbl-ide.js");
        assert!(set.root_dir().is_absolute());
        let joined = std::env::current_dir().unwrap().join(set.root_dir());
        assert!(joined.is_absolute());
    }

    #[test]
    fn absolute_patterns_match_on_the_absolute_key() {
        let set = GlobSet::new("/repo/src/**/*.js");
        let file = Path::new("/repo/src/a.js");
        // The walk root is stripped for relative patterns only, otherwise the
        // key could never match an absolute alternative.
        assert_eq!(match_key(file, Path::new("/repo"), &set), "/repo/src/a.js");

        let relative = GlobSet::new("src/**/*.js");
        assert_eq!(match_key(file, Path::new("/repo"), &relative), "src/a.js");
    }

    #[test]
    fn extensions_are_collected_from_alternatives() {
        let mut exts = GlobSet::new("src/**/*.{js,jsx}").extensions();
        exts.sort();
        assert_eq!(exts, vec!["js", "jsx"]);
    }

    #[test]
    fn question_mark_matches_one_char() {
        assert!(match_segment("a?c.js", "abc.js"));
        assert!(!match_segment("a?c.js", "ac.js"));
    }
}
