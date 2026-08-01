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
            Some(segments) if !segments.is_empty() => {
                // Splitting "/a/b" yields a leading empty segment. Collecting it
                // away would turn the absolute root into a relative one, which
                // then gets joined onto the cwd.
                let mut root = PathBuf::new();
                if self.is_absolute() {
                    root.push("/");
                }
                root.extend(segments.iter().filter(|segment| !segment.is_empty()));
                root
            }
            _ => PathBuf::from("."),
        }
    }

    pub fn is_absolute(&self) -> bool {
        self.raw.starts_with('/')
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
        assert!(set.root_dir().is_absolute());
        assert!(set.is_match("/tmp/proj/src/a/b.js"));
        assert!(!set.is_match("src/a/b.js"));
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
