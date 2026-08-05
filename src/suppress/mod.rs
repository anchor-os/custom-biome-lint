use std::collections::HashMap;

pub const IGNORE_LINE: &str = "biome-ignore-line";
pub const IGNORE_NEXT_LINE: &str = "biome-ignore-next-line";

/// A suppression comment found in the source.
///
/// The fixer uses this to extend an existing comment instead of adding a second
/// one to the same line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressionComment {
    /// 1-based line holding the comment text.
    pub comment_line: usize,
    /// 1-based line the marker applies to.
    pub target_line: usize,
    pub marker: &'static str,
    /// Rule names listed on the marker. Empty means "every rule".
    pub rules: Vec<String>,
    /// Byte offset within `comment_line` just past the final rule name, where a
    /// further `, rule-name` can be spliced in ahead of any `--` justification.
    pub append_at: usize,
}

impl SuppressionComment {
    /// A marker with no rule names, which suppresses every rule on its target.
    pub fn is_wildcard(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Which rules are suppressed on one line.
#[derive(Debug, Default)]
struct LineSuppression {
    /// Set by a bare marker: suppresses every rule on the line.
    all: bool,
    rules: Vec<String>,
}

/// Maps a 1-based line number to the rules suppressed on it.
#[derive(Debug, Default)]
pub struct Suppressions {
    by_line: HashMap<usize, LineSuppression>,
}

impl Suppressions {
    /// Scans comments for `// biome-ignore-line <rules>` (applies to the comment's
    /// own line) and `// biome-ignore-next-line <rules>` (applies to the line after).
    ///
    /// A marker with no rule names suppresses every rule on its target line.
    pub fn parse(source: &str) -> Self {
        let mut by_line: HashMap<usize, LineSuppression> = HashMap::new();

        for comment in find_suppression_comments(source) {
            let entry = by_line.entry(comment.target_line).or_default();
            if comment.is_wildcard() {
                entry.all = true;
            } else {
                entry.rules.extend(comment.rules);
            }
        }

        Self { by_line }
    }

    pub fn is_suppressed(&self, line: usize, rule: &str) -> bool {
        self.by_line
            .get(&line)
            .is_some_and(|entry| entry.all || entry.rules.iter().any(|r| r == rule))
    }

    pub fn is_empty(&self) -> bool {
        self.by_line.is_empty()
    }
}

/// Every suppression comment in `source`, in source order.
pub fn find_suppression_comments(source: &str) -> Vec<SuppressionComment> {
    let mut found = Vec::new();

    for (index, text) in source.lines().enumerate() {
        let line_no = index + 1;
        let Some((body_offset, comment)) = comment_body(text) else {
            continue;
        };

        // Checked before IGNORE_LINE only for clarity; the two markers are not
        // substrings of one another, so order does not affect matching.
        let (marker, target_line, marker_pos) = if let Some(pos) = comment.find(IGNORE_NEXT_LINE) {
            (IGNORE_NEXT_LINE, line_no + 1, pos)
        } else if let Some(pos) = comment.find(IGNORE_LINE) {
            (IGNORE_LINE, line_no, pos)
        } else {
            continue;
        };

        let rest_offset = body_offset + marker_pos + marker.len();
        let (rules, rules_end) = parse_rule_names(&text[rest_offset..]);

        found.push(SuppressionComment {
            comment_line: line_no,
            target_line,
            marker,
            rules,
            append_at: rest_offset + rules_end,
        });
    }

    found
}

/// The text following the first `//` or `/*` on a line, with its byte offset, so
/// that a marker appearing inside a string literal is not treated as a
/// suppression.
fn comment_body(line: &str) -> Option<(usize, &str)> {
    let slash = line.find("//");
    let block = line.find("/*");
    let start = match (slash, block) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };
    let body = start + 2;
    Some((body, &line[body..]))
}

/// Comma- and/or space-separated rule names, plus the byte offset in `rest` just
/// past the final name. A `--` token ends the list so a trailing justification
/// can be written after the rules.
fn parse_rule_names(rest: &str) -> (Vec<String>, usize) {
    let cleaned = rest.trim_end().trim_end_matches("*/");
    let mut names = Vec::new();
    let mut end = 0usize;

    for (offset, token) in tokens(cleaned) {
        if token == "--" || !is_rule_name(token) {
            break;
        }
        names.push(token.to_string());
        end = offset + token.len();
    }

    (names, end)
}

/// Non-empty comma/space/tab-separated tokens with their byte offsets.
fn tokens(text: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let mut start: Option<usize> = None;

    for (i, ch) in text.char_indices() {
        if matches!(ch, ',' | ' ' | '\t') {
            if let Some(s) = start.take() {
                found.push((s, &text[s..i]));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        found.push((s, &text[s..]));
    }

    found
}

fn is_rule_name(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_line_single_rule() {
        let s = Suppressions::parse("const x = 1; // biome-ignore-line no-native-map\n");
        assert!(s.is_suppressed(1, "no-native-map"));
        assert!(!s.is_suppressed(1, "reselect-arity-match"));
    }

    #[test]
    fn next_line_multiple_rules() {
        let s = Suppressions::parse(
            "// biome-ignore-next-line no-native-map, reselect-arity-match\nconst x = 1;\n",
        );
        assert!(s.is_suppressed(2, "no-native-map"));
        assert!(s.is_suppressed(2, "reselect-arity-match"));
        assert!(!s.is_suppressed(1, "no-native-map"));
    }

    #[test]
    fn justification_after_double_dash_is_ignored() {
        let s = Suppressions::parse("x; // biome-ignore-line no-native-map -- legacy call site\n");
        assert!(s.is_suppressed(1, "no-native-map"));
    }

    #[test]
    fn marker_in_string_literal_is_not_a_suppression() {
        let s = Suppressions::parse("const x = 'biome-ignore-line no-native-map';\n");
        assert!(!s.is_suppressed(1, "no-native-map"));
    }

    #[test]
    fn block_comment_form() {
        let s = Suppressions::parse("x; /* biome-ignore-line no-native-map */\n");
        assert!(s.is_suppressed(1, "no-native-map"));
    }

    #[test]
    fn bare_marker_suppresses_every_rule() {
        let s = Suppressions::parse("x; // biome-ignore-line\n");
        assert!(s.is_suppressed(1, "no-native-map"));
        assert!(s.is_suppressed(1, "reselect-arity-match"));
        assert!(s.is_suppressed(1, "a-rule-that-does-not-exist"));
        assert!(!s.is_suppressed(2, "no-native-map"));
    }

    #[test]
    fn bare_next_line_marker_suppresses_every_rule_on_the_following_line() {
        let s = Suppressions::parse("// biome-ignore-next-line\nx;\n");
        assert!(s.is_suppressed(2, "no-native-map"));
        assert!(!s.is_suppressed(1, "no-native-map"));
    }

    #[test]
    fn jsx_comment_form_is_recognised() {
        let s = Suppressions::parse("{/* biome-ignore-next-line no-native-map */}\nx;\n");
        assert!(s.is_suppressed(2, "no-native-map"));
    }

    #[test]
    fn append_at_points_just_past_the_last_rule_name() {
        let line = "x; // biome-ignore-line no-native-map -- why";
        let found = find_suppression_comments(line);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rules, vec!["no-native-map"]);
        assert_eq!(
            &line[..found[0].append_at],
            "x; // biome-ignore-line no-native-map"
        );
    }

    #[test]
    fn append_at_of_a_wildcard_marker_is_just_past_the_marker() {
        let line = "x; // biome-ignore-line";
        let found = find_suppression_comments(line);
        assert_eq!(found.len(), 1);
        assert!(found[0].is_wildcard());
        assert_eq!(found[0].append_at, line.len());
    }
}
