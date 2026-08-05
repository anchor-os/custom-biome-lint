//! Writes suppression comments back into source files.
//!
//! Placement is deliberately cautious. A comment is only added where it is
//! provably a comment: never inside a multi-line string, never inside an
//! existing block comment, and — because `//` inside JSX children is rendered
//! text rather than a comment — using the `{/* ... */}` form when the insertion
//! point falls in a JSX child list. Anything that cannot be placed safely is
//! reported as unfixable instead of being written.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use biome_js_syntax::{JsSyntaxKind, JsSyntaxNode};

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::suppress::{find_suppression_comments, Suppressions, IGNORE_LINE, IGNORE_NEXT_LINE};

/// A trailing comment is only used when the resulting line stays within this
/// width; longer lines get the comment on its own line above.
const MAX_TRAILING_WIDTH: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Appended to the end of the offending line as `biome-ignore-line`.
    Trailing,
    /// Inserted on its own line above as `biome-ignore-next-line`.
    OwnLine,
    /// Rule names added to a suppression comment that was already there.
    Merged,
}

impl Placement {
    pub fn label(self) -> &'static str {
        match self {
            Placement::Trailing => "trailing",
            Placement::OwnLine => "own line",
            Placement::Merged => "merged",
        }
    }
}

/// One suppression comment written (or, in a dry run, that would be written).
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    /// The full comment text, or the added fragment for a merge.
    pub comment_added: String,
    /// 1-based line of the offending code, as numbered in the original file.
    pub line_number: usize,
    pub placement: Placement,
    pub rules: Vec<String>,
}

/// A violation that could not be suppressed without risking a change in meaning.
#[derive(Debug, Clone)]
pub struct Unfixable {
    pub path: PathBuf,
    pub line_number: usize,
    pub rules: Vec<String>,
    pub reason: &'static str,
}

#[derive(Debug, Default)]
pub struct FixReport {
    pub files_modified: usize,
    pub suppressions_added: usize,
    pub changes: Vec<FileChange>,
    pub unfixable: Vec<Unfixable>,
    /// Files that could not be read or written, with the reason.
    pub failures: Vec<(PathBuf, String)>,
    /// False for a dry run, in which case nothing was written to disk.
    pub wrote: bool,
}

impl FixReport {
    /// True when every violation was suppressed and no file failed.
    pub fn is_complete(&self) -> bool {
        self.unfixable.is_empty() && self.failures.is_empty()
    }
}

/// The rewritten source for one file, plus what it changed.
pub struct FilePlan {
    pub source: String,
    pub changes: Vec<FileChange>,
    pub unfixable: Vec<Unfixable>,
}

impl FilePlan {
    pub fn is_noop(&self) -> bool {
        self.changes.is_empty()
    }
}

pub struct Fixer;

impl Fixer {
    /// Adds suppression comments for every violation in `violations_by_file`.
    ///
    /// With `write` false this is a dry run: files are read and planned but not
    /// modified. Per-file read/write errors are recorded in
    /// [`FixReport::failures`] rather than aborting the whole run, so one
    /// unwritable file cannot discard the fixes for every other file.
    pub fn apply_suppressions(
        violations_by_file: &BTreeMap<PathBuf, Vec<Violation>>,
        write: bool,
    ) -> FixReport {
        let mut report = FixReport {
            wrote: write,
            ..FixReport::default()
        };

        for (path, violations) in violations_by_file {
            let source = match fs::read_to_string(path) {
                Ok(source) => source,
                Err(error) => {
                    report.failures.push((path.clone(), error.to_string()));
                    continue;
                }
            };

            let mut plan = plan_file(path, &source, violations);
            report.unfixable.append(&mut plan.unfixable);

            if plan.is_noop() {
                continue;
            }

            if write {
                if let Err(error) = fs::write(path, &plan.source) {
                    report.failures.push((path.clone(), error.to_string()));
                    continue;
                }
            }

            report.files_modified += 1;
            report.suppressions_added += plan.changes.len();
            report.changes.extend(plan.changes);
        }

        report
    }
}

/// Plans the suppression comments for a single file without touching disk.
pub fn plan_file(path: &Path, source: &str, violations: &[Violation]) -> FilePlan {
    let mut wanted: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
    for violation in violations {
        wanted
            .entry(violation.line)
            .or_default()
            .insert(violation.rule);
    }

    let unchanged = |unfixable| FilePlan {
        source: source.to_string(),
        changes: Vec::new(),
        unfixable,
    };

    if wanted.is_empty() {
        return unchanged(Vec::new());
    }

    let context = FileContext::parse(source, path);
    if !context.parsed_cleanly() {
        // Rewriting a file the parser could not make sense of risks corrupting
        // it, and the JSX detection below would be unreliable anyway.
        return unchanged(all_unfixable(path, &wanted, "file has parse errors"));
    }

    let lines = split_lines(source);
    let states = line_states(&lines);
    let starts = line_offsets(&lines);
    let jsx = JsxText::collect(context.tree());

    let existing = find_suppression_comments(source);
    let marked_lines: HashSet<usize> = existing.iter().map(|c| c.comment_line).collect();
    let mut by_target: HashMap<usize, &crate::suppress::SuppressionComment> = HashMap::new();
    for comment in &existing {
        by_target.entry(comment.target_line).or_insert(comment);
    }

    let mut prefix_inserts: HashMap<usize, String> = HashMap::new();
    let mut appends: HashMap<usize, String> = HashMap::new();
    let mut merges: HashMap<usize, (usize, String)> = HashMap::new();
    let mut changes = Vec::new();
    let mut unfixable = Vec::new();

    for (&line, rules) in &wanted {
        let rules: Vec<&str> = rules.iter().copied().collect();
        let owned: Vec<String> = rules.iter().map(|r| r.to_string()).collect();

        // A comment already targets this line but lists other rules: extend it
        // rather than adding a second marker, which would be ignored anyway
        // since only the first marker on a line is parsed.
        if let Some(comment) = by_target.get(&line) {
            let missing: Vec<&str> = rules
                .iter()
                .copied()
                .filter(|rule| !comment.rules.iter().any(|have| have == rule))
                .collect();
            if missing.is_empty() {
                continue;
            }
            let fragment = format!(", {}", missing.join(", "));
            merges.insert(comment.comment_line, (comment.append_at, fragment.clone()));
            changes.push(FileChange {
                path: path.to_path_buf(),
                comment_added: fragment,
                line_number: line,
                placement: Placement::Merged,
                rules: missing.iter().map(|r| r.to_string()).collect(),
            });
            continue;
        }

        let Some((content, _)) = lines.get(line - 1) else {
            unfixable.push(Unfixable {
                path: path.to_path_buf(),
                line_number: line,
                rules: owned,
                reason: "line is outside the file",
            });
            continue;
        };
        let (start_state, end_state) = states[line - 1];
        let line_start = starts[line - 1];

        // Trailing placement needs the end of the line to be code (or an
        // existing line comment we can extend), and no marker already on it.
        //
        // It is also refused in JSX children: `{expr} {/* ... */}` leaves a
        // whitespace-only text node between the two containers, which React
        // renders as a space. Own-line placement is inert there instead, because
        // JSX discards a whitespace run that contains a newline.
        let trailing_ok = matches!(end_state, Lex::Code | Lex::LineComment)
            && !marked_lines.contains(&line)
            && !jsx.contains(line_start + content.len());
        if trailing_ok {
            let comment = comment_text(IGNORE_LINE, &rules, false);
            if content.chars().count() + 1 + comment.chars().count() <= MAX_TRAILING_WIDTH {
                appends.insert(line, format!(" {comment}"));
                changes.push(FileChange {
                    path: path.to_path_buf(),
                    comment_added: comment,
                    line_number: line,
                    placement: Placement::Trailing,
                    rules: owned,
                });
                continue;
            }
        }

        // Own-line placement needs the start of the line to be code, otherwise
        // the "comment" would become string or comment content.
        if start_state == Lex::Code {
            let in_jsx = jsx.contains(line_start);
            let indent: String = content.chars().take_while(|c| c.is_whitespace()).collect();
            let comment = comment_text(IGNORE_NEXT_LINE, &rules, in_jsx);
            prefix_inserts.insert(line, format!("{indent}{comment}"));
            changes.push(FileChange {
                path: path.to_path_buf(),
                comment_added: comment,
                line_number: line,
                placement: Placement::OwnLine,
                rules: owned,
            });
            continue;
        }

        unfixable.push(Unfixable {
            path: path.to_path_buf(),
            line_number: line,
            rules: owned,
            reason: "inside a multi-line string or comment",
        });
    }

    if changes.is_empty() {
        return unchanged(unfixable);
    }

    let (rewritten, line_map) = emit(&lines, &prefix_inserts, &appends, &merges);

    // Cheap insurance against a placement bug silently producing a file whose
    // suppressions do not actually apply.
    if !verify(path, &rewritten, &changes, &line_map) {
        unfixable.extend(all_unfixable(
            path,
            &wanted,
            "suppression could not be verified",
        ));
        return unchanged(unfixable);
    }

    FilePlan {
        source: rewritten,
        changes,
        unfixable,
    }
}

fn all_unfixable(
    path: &Path,
    wanted: &BTreeMap<usize, BTreeSet<&str>>,
    reason: &'static str,
) -> Vec<Unfixable> {
    wanted
        .iter()
        .map(|(&line, rules)| Unfixable {
            path: path.to_path_buf(),
            line_number: line,
            rules: rules.iter().map(|r| r.to_string()).collect(),
            reason,
        })
        .collect()
}

fn comment_text(marker: &str, rules: &[&str], jsx: bool) -> String {
    let list = rules.join(", ");
    if jsx {
        format!("{{/* {marker} {list} */}}")
    } else {
        format!("// {marker} {list}")
    }
}

/// Applies the planned edits, returning the new source and a map from original
/// 1-based line numbers to their new positions.
fn emit(
    lines: &[(&str, &str)],
    prefix_inserts: &HashMap<usize, String>,
    appends: &HashMap<usize, String>,
    merges: &HashMap<usize, (usize, String)>,
) -> (String, HashMap<usize, usize>) {
    let mut out =
        String::with_capacity(lines.iter().map(|(c, e)| c.len() + e.len()).sum::<usize>() + 256);
    let mut line_map = HashMap::new();
    let mut emitted = 0usize;

    for (index, (content, ending)) in lines.iter().enumerate() {
        let old_line = index + 1;

        if let Some(text) = prefix_inserts.get(&old_line) {
            out.push_str(text);
            // The final line of a file may have no terminator of its own.
            out.push_str(if ending.is_empty() { "\n" } else { ending });
            emitted += 1;
        }

        let mut line = (*content).to_string();
        if let Some((at, fragment)) = merges.get(&old_line) {
            line.insert_str(*at, fragment);
        }
        if let Some(text) = appends.get(&old_line) {
            line.push_str(text);
        }
        out.push_str(&line);
        out.push_str(ending);
        emitted += 1;
        line_map.insert(old_line, emitted);
    }

    (out, line_map)
}

/// Confirms the rewritten source still parses and that every rule we claimed to
/// suppress is genuinely suppressed at the code's new line number.
fn verify(
    path: &Path,
    rewritten: &str,
    changes: &[FileChange],
    line_map: &HashMap<usize, usize>,
) -> bool {
    if !FileContext::parse(rewritten, path).parsed_cleanly() {
        return false;
    }

    let suppressions = Suppressions::parse(rewritten);
    changes.iter().all(|change| {
        line_map.get(&change.line_number).is_some_and(|&line| {
            change
                .rules
                .iter()
                .all(|rule| suppressions.is_suppressed(line, rule))
        })
    })
}

/// Byte ranges in which a `//` comment would be JSX text rather than a comment.
struct JsxText {
    child_lists: Vec<(usize, usize)>,
    expressions: Vec<(usize, usize)>,
}

impl JsxText {
    fn collect(tree: &JsSyntaxNode) -> Self {
        let mut child_lists = Vec::new();
        let mut expressions = Vec::new();

        for node in tree.descendants() {
            match node.kind() {
                // The wider range for child lists and the narrower one for
                // expression containers both err towards "this is JSX text",
                // which is the safe direction.
                JsSyntaxKind::JSX_CHILD_LIST => child_lists.push(span(node.text_range())),
                JsSyntaxKind::JSX_EXPRESSION_CHILD => {
                    expressions.push(span(node.text_trimmed_range()))
                }
                _ => {}
            }
        }

        Self {
            child_lists,
            expressions,
        }
    }

    fn contains(&self, offset: usize) -> bool {
        let in_children = self
            .child_lists
            .iter()
            .any(|&(start, end)| start <= offset && offset <= end);
        if !in_children {
            return false;
        }
        // Inside `{ ... }` we are back in ordinary expression context.
        !self
            .expressions
            .iter()
            .any(|&(start, end)| start < offset && offset < end)
    }
}

fn span(range: biome_rowan::TextRange) -> (usize, usize) {
    (usize::from(range.start()), usize::from(range.end()))
}

/// Splits into `(content, line ending)` pairs, numbered like `str::lines`, so
/// that CRLF files and files without a trailing newline round-trip unchanged.
fn split_lines(source: &str) -> Vec<(&str, &str)> {
    let mut lines = Vec::new();
    let mut start = 0usize;

    for (index, byte) in source.bytes().enumerate() {
        if byte != b'\n' {
            continue;
        }
        let has_cr = index > start && source.as_bytes()[index - 1] == b'\r';
        let content_end = if has_cr { index - 1 } else { index };
        lines.push((&source[start..content_end], &source[content_end..=index]));
        start = index + 1;
    }
    if start < source.len() {
        lines.push((&source[start..], ""));
    }

    lines
}

fn line_offsets(lines: &[(&str, &str)]) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(lines.len());
    let mut at = 0usize;
    for (content, ending) in lines {
        offsets.push(at);
        at += content.len() + ending.len();
    }
    offsets
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lex {
    Code,
    LineComment,
    BlockComment,
    Single,
    Double,
    Template,
}

/// Lexical state at the start and end of each line.
///
/// This does not understand regex literals or `${}` nesting. Both blind spots
/// leave it believing it is inside a string, which only ever makes the fixer
/// decline a placement — never accept an unsafe one.
fn line_states(lines: &[(&str, &str)]) -> Vec<(Lex, Lex)> {
    let mut states = Vec::with_capacity(lines.len());
    let mut state = Lex::Code;

    for (content, _) in lines {
        let start = state;
        let bytes = content.as_bytes();
        let mut i = 0usize;

        while i < bytes.len() {
            let ch = bytes[i];
            let next = bytes.get(i + 1).copied();

            match state {
                Lex::LineComment => break,
                Lex::Code => match ch {
                    b'/' if next == Some(b'/') => {
                        state = Lex::LineComment;
                        i += 2;
                        continue;
                    }
                    b'/' if next == Some(b'*') => {
                        state = Lex::BlockComment;
                        i += 2;
                        continue;
                    }
                    b'\'' => state = Lex::Single,
                    b'"' => state = Lex::Double,
                    b'`' => state = Lex::Template,
                    _ => {}
                },
                Lex::BlockComment => {
                    if ch == b'*' && next == Some(b'/') {
                        state = Lex::Code;
                        i += 2;
                        continue;
                    }
                }
                Lex::Single | Lex::Double | Lex::Template => {
                    if ch == b'\\' {
                        i += 2;
                        continue;
                    }
                    let closer = match state {
                        Lex::Single => b'\'',
                        Lex::Double => b'"',
                        _ => b'`',
                    };
                    if ch == closer {
                        state = Lex::Code;
                    }
                }
            }
            i += 1;
        }

        let end = state;
        state = match state {
            // A line comment never continues onto the next line.
            Lex::LineComment => Lex::Code,
            // Quoted strings only span lines via a backslash continuation.
            // Without one the scan was thrown off — a regex literal containing a
            // quote is the usual cause — so resync instead of treating the rest
            // of the file as string content.
            Lex::Single | Lex::Double if !content.trim_end().ends_with('\\') => Lex::Code,
            other => other,
        };
        states.push((start, end));
    }

    states
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Violation;

    fn plan(source: &str, violations: &[(usize, &'static str)]) -> FilePlan {
        let violations: Vec<Violation> = violations
            .iter()
            .map(|&(line, rule)| Violation::error(rule, line, 1, "test"))
            .collect();
        plan_file(Path::new("a.jsx"), source, &violations)
    }

    #[test]
    fn short_line_gets_a_trailing_comment() {
        let plan = plan("const m = new Map();\n", &[(1, "no-native-map")]);
        assert_eq!(
            plan.source,
            "const m = new Map(); // biome-ignore-line no-native-map\n"
        );
        assert_eq!(plan.changes[0].placement, Placement::Trailing);
    }

    #[test]
    fn long_line_gets_the_comment_above_with_matching_indent() {
        let source = format!("    const m = new Map(); // {}\n", "x".repeat(80));
        let plan = plan(&source, &[(1, "no-native-map")]);
        assert!(plan
            .source
            .starts_with("    // biome-ignore-next-line no-native-map\n"));
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
    }

    #[test]
    fn several_rules_on_one_line_share_a_single_comment() {
        let plan = plan(
            "const m = new Map();\n",
            &[(1, "no-native-map"), (1, "reselect-arity-match")],
        );
        assert_eq!(plan.changes.len(), 1);
        assert!(plan
            .source
            .contains("// biome-ignore-line no-native-map, reselect-arity-match"));
    }

    #[test]
    fn an_existing_comment_is_extended_rather_than_duplicated() {
        let source = "const m = new Map(); // biome-ignore-line no-native-map\n";
        let plan = plan(source, &[(1, "reselect-arity-match")]);
        assert_eq!(plan.changes[0].placement, Placement::Merged);
        assert_eq!(
            plan.source,
            "const m = new Map(); // biome-ignore-line no-native-map, reselect-arity-match\n"
        );
    }

    #[test]
    fn a_justification_survives_a_merge() {
        let source = "x(); // biome-ignore-line no-native-map -- legacy\n";
        let plan = plan(source, &[(1, "reselect-arity-match")]);
        assert_eq!(
            plan.source,
            "x(); // biome-ignore-line no-native-map, reselect-arity-match -- legacy\n"
        );
    }

    #[test]
    fn jsx_children_get_a_brace_comment_on_its_own_line() {
        let source = "const a = (\n  <div>\n    {new Map()}\n  </div>\n);\n";
        let plan = plan(source, &[(3, "no-native-map")]);
        assert_eq!(plan.changes.len(), 1);
        // Own line, not trailing: a trailing comment would leave a
        // whitespace-only text node that React renders as a space.
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
        assert_eq!(
            plan.source,
            "const a = (\n  <div>\n    {/* biome-ignore-next-line no-native-map */}\n    {new Map()}\n  </div>\n);\n"
        );
        assert!(
            !plan.source.contains("// biome-ignore"),
            "a bare // in JSX children would render as text: {:?}",
            plan.source
        );
    }

    #[test]
    fn jsx_attribute_values_are_ordinary_code() {
        // Inside `{...}` on an attribute we are not in a child list, so the
        // plain comment form applies.
        let source = "const a = <Row data={new Map()} />;\n";
        let plan = plan(source, &[(1, "no-native-map")]);
        assert_eq!(
            plan.source,
            "const a = <Row data={new Map()} />; // biome-ignore-line no-native-map\n"
        );
    }

    #[test]
    fn a_line_inside_a_template_literal_is_left_alone() {
        let source = "const q = `\n  ${1} new Map()\n`;\n";
        let plan = plan(source, &[(2, "no-native-map")]);
        assert!(plan.is_noop());
        assert_eq!(plan.unfixable.len(), 1);
        assert_eq!(
            plan.unfixable[0].reason,
            "inside a multi-line string or comment"
        );
    }

    #[test]
    fn a_file_with_parse_errors_is_never_rewritten() {
        let plan = plan("const = = ;\n", &[(1, "no-native-map")]);
        assert!(plan.is_noop());
        assert_eq!(plan.unfixable[0].reason, "file has parse errors");
    }

    #[test]
    fn crlf_endings_and_a_missing_final_newline_round_trip() {
        let plan = plan(
            "const a = 1;\r\nconst m = new Map();",
            &[(2, "no-native-map")],
        );
        assert_eq!(
            plan.source,
            "const a = 1;\r\nconst m = new Map(); // biome-ignore-line no-native-map"
        );
    }

    #[test]
    fn planning_is_idempotent() {
        let first = plan("const m = new Map();\n", &[(1, "no-native-map")]);
        // The rule would no longer fire, so a second pass sees no violations.
        let second = plan_file(Path::new("a.jsx"), &first.source, &[]);
        assert!(second.is_noop());
        assert_eq!(second.source, first.source);
    }

    #[test]
    fn a_marker_targeting_another_line_forces_own_line_placement() {
        // Appending a second marker here would be swallowed: only the first
        // marker on a line is ever parsed.
        let source = "const m = new Map(); // biome-ignore-next-line no-native-map\nother();\n";
        let plan = plan(source, &[(1, "reselect-arity-match")]);
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
        assert!(plan
            .source
            .starts_with("// biome-ignore-next-line reselect-arity-match\n"));
    }

    #[test]
    fn lexer_tracks_multi_line_strings_and_comments() {
        let lines = split_lines("a;\n/* x\ny */ b;\n`t\nu`;\n");
        let states = line_states(&lines);
        assert_eq!(states[0], (Lex::Code, Lex::Code));
        assert_eq!(states[1], (Lex::Code, Lex::BlockComment));
        assert_eq!(states[2], (Lex::BlockComment, Lex::Code));
        assert_eq!(states[3], (Lex::Code, Lex::Template));
        assert_eq!(states[4], (Lex::Template, Lex::Code));
    }

    #[test]
    fn lexer_resyncs_after_a_regex_holding_a_quote() {
        let lines = split_lines("const r = /['\"]/;\nconst m = new Map();\n");
        let states = line_states(&lines);
        assert_eq!(states[1].0, Lex::Code, "line 2 must not look like a string");
    }
}
