//! Writes suppression comments back into source files.
//!
//! Placement is deliberately cautious. A comment is only added where it is
//! provably a comment: never inside a multi-line string, never inside an
//! existing block comment, and — because `//` inside JSX children is rendered
//! text rather than a comment — using the `{/* ... */}` form when the insertion
//! point falls in a JSX child list. Anything that cannot be placed safely is
//! reported as unfixable instead of being written.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use biome_js_syntax::{AnyJsStatement, JsSyntaxKind, JsSyntaxNode};
use biome_rowan::AstNode;

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::suppress::{find_suppression_comments, Suppressions, IGNORE_LINE, IGNORE_NEXT_LINE};

/// A trailing comment is only used when the resulting line stays within this
/// width; longer lines get the comment on its own line above.
const MAX_TRAILING_WIDTH: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Appended to the end of the offending line as `custom-biome-ignore-line`.
    Trailing,
    /// Inserted on its own line above as `custom-biome-ignore-next-line`.
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
    let lines = split_lines(source);
    let starts = line_offsets(&lines);

    // Map each violation's line to its rules plus the *byte offset of the
    // violation itself*. The statement-boundedness check must anchor at the
    // violation's column, not the physical start of the line: in
    // `noop(); accum.x = {` the line start sits inside a single-line statement
    // (`noop();`), so anchoring there would wrongly treat the real target
    // (`accum.x = {...}`) as single-line and emit a trailing comment the
    // formatter later detaches. See docs/WRITE_FIX_SUPPRESSION_PLACEMENT_BUG.md,
    // Cases 1-2 and the CodeRabbit review on PR #27.
    let mut wanted: BTreeMap<usize, BTreeMap<&'static str, usize>> = BTreeMap::new();
    for violation in violations {
        let offset = violation
            .line
            .checked_sub(1)
            .and_then(|i| starts.get(i))
            .map(|&line_start| line_start + violation.col.saturating_sub(1))
            .unwrap_or(0);
        wanted
            .entry(violation.line)
            .or_default()
            .insert(violation.rule, offset);
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

    let states = line_states(&lines);
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

    for (&line, rules_offsets) in &wanted {
        let rules: Vec<&str> = rules_offsets.keys().copied().collect();
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

        // A trailing comment only survives a downstream formatter pass when the
        // statement it attaches to begins and ends on the same physical line:
        // otherwise Biome may reprint that token onto a different line and the
        // suppression silently detaches. See docs/WRITE_FIX_SUPPRESSION_PLACEMENT_BUG.md.
        let single_line = rules_offsets
            .values()
            .all(|&offset| statement_is_single_line(&context, line, offset));

        // A foreign (or another tool's own) suppression comment on the line
        // directly above already claims adjacency to the code. Inserting a
        // leading own-line marker there would sit between that comment and the
        // code, breaking its suppression (and our own would break too).
        let foreign_above = line > 1 && is_next_line_suppression_comment(lines[line - 2].0);

        // Trailing placement needs the end of the line to be code (or an
        // existing line comment we can extend), no marker already on it, and the
        // violation line to be the single, complete physical line of the
        // smallest statement containing it.
        //
        // It is also refused in JSX children: `{expr} {/* ... */}` leaves a
        // whitespace-only text node between the two containers, which React
        // renders as a space. Own-line placement is inert there instead, because
        // JSX discards a whitespace run that contains a newline.
        let trailing_ok = matches!(end_state, Lex::Code | Lex::LineComment)
            && !marked_lines.contains(&line)
            && !jsx.contains(line_start + content.len())
            && single_line;
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

        // A foreign suppression comment directly above means a leading own-line
        // marker cannot be placed without breaking it. Trailing is the only safe
        // option, and it was already attempted above (it requires the statement
        // to be single-line). When that was not possible -- the statement spans
        // multiple lines, or trailing was refused for another reason -- no
        // placement satisfies both tools, so report it as unfixable rather than
        // silently writing a broken result.
        if foreign_above {
            let reason = if single_line {
                "leading line already claimed by another tool's suppression comment"
            } else {
                "leading line already claimed by another tool's suppression comment and target spans multiple lines"
            };
            unfixable.push(Unfixable {
                path: path.to_path_buf(),
                line_number: line,
                rules: owned,
                reason,
            });
            continue;
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
    wanted: &BTreeMap<usize, BTreeMap<&'static str, usize>>,
    reason: &'static str,
) -> Vec<Unfixable> {
    wanted
        .iter()
        .map(|(&line, rules)| Unfixable {
            path: path.to_path_buf(),
            line_number: line,
            rules: rules.keys().map(|r| r.to_string()).collect(),
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

/// Locations in which a `//` comment would be JSX text rather than a comment.
struct JsxText<'a> {
    tree: &'a JsSyntaxNode,
}

impl<'a> JsxText<'a> {
    fn collect(tree: &'a JsSyntaxNode) -> Self {
        Self { tree }
    }

    /// True only when `offset` falls in a genuinely *rendered* JSX context --
    /// a JSX text node or a `{...}` expression child -- and not merely inside
    /// the byte span of some enclosing JSX element reached through an attribute
    /// value.
    ///
    /// The previous byte-range check reported an offset as JSX children purely
    /// because it was textually nested inside a `JSX_CHILD_LIST`'s span. A JSX
    /// element passed directly as a child (no wrapping `{}`) makes every byte
    /// of its attribute values fall inside that span, so a plain-JS arrow body
    /// in `beforeToolbarCreated={toolbar => {...}}` was wrongly treated as JSX
    /// children and given an inert `{/* ... */}` marker. Deciding by tree
    /// ancestry instead: if the first JSX-related ancestor reached walking up
    /// from the offset is a `JSX_CHILD_LIST` the point really is JSX children;
    /// if an attribute node is hit first the point is ordinary JS and a normal
    /// `//` comment applies. See docs/WRITE_FIX_SUPPRESSION_PLACEMENT_BUG.md,
    /// Case 3.
    fn contains(&self, offset: usize) -> bool {
        let Some(node) = node_at_offset(self.tree, offset) else {
            return false;
        };
        for ancestor in node.ancestors() {
            match ancestor.kind() {
                JsSyntaxKind::JSX_CHILD_LIST => return true,
                JsSyntaxKind::JSX_ATTRIBUTE
                | JsSyntaxKind::JSX_ATTRIBUTE_INITIALIZER_CLAUSE
                | JsSyntaxKind::JSX_EXPRESSION_ATTRIBUTE_VALUE => return false,
                _ => {}
            }
        }
        false
    }
}

/// The deepest descendant of `tree` whose range covers `offset`. Used to anchor
/// both the JSX-context and statement-boundedness checks at the actual syntax
/// node a violation line refers to.
fn node_at_offset(tree: &JsSyntaxNode, offset: usize) -> Option<JsSyntaxNode> {
    let mut best: Option<(usize, JsSyntaxNode)> = None;
    for node in tree.descendants() {
        let range = node.text_range_with_trivia();
        let start = usize::from(range.start());
        let end = usize::from(range.end());
        // Skip empty ranges: a zero-width node (e.g. an empty directive list at
        // the very start of the file) would otherwise beat the real token that
        // also covers the offset.
        if start < end && start <= offset && offset <= end {
            let len = end - start;
            match best {
                Some((best_len, _)) if best_len <= len => {}
                _ => best = Some((len, node.clone())),
            }
        }
    }
    best.map(|(_, node)| node)
}

/// Whether the violation at byte `offset` on `line` is the single, complete
/// physical line of the smallest statement containing it.
///
/// `offset` is the violation's own byte position (not the start of its line):
/// anchoring at the line start would wrongly pick an earlier single-line
/// statement when several share the line. See
/// docs/WRITE_FIX_SUPPRESSION_PLACEMENT_BUG.md, Cases 1, 2 and the CodeRabbit
/// review on PR #27.
///
/// A trailing suppression comment is trivia attached to the statement's last
/// token. When that token is reprinted onto a different physical line by a
/// downstream formatter (which happens whenever the statement spans more than
/// the violation line), the comment no longer covers the violation. Requiring
/// the smallest enclosing statement to start and end on `line` guarantees the
/// comment and the statement's last token share a line. See Cases 1, 2 and the
/// primary fix.
fn statement_is_single_line(context: &FileContext, line: usize, offset: usize) -> bool {
    let Some(node) = node_at_offset(context.tree(), offset) else {
        // No node anchors this line (e.g. blank line): be conservative and
        // refuse trailing placement.
        return false;
    };
    for ancestor in node.ancestors() {
        if AnyJsStatement::can_cast(ancestor.kind()) {
            // `text_trimmed_range` excludes leading/trailing trivia so a
            // statement's trailing newline is not counted as its own line.
            let trimmed = ancestor.text_trimmed_range();
            let start = usize::from(trimmed.start());
            let end = usize::from(trimmed.end());
            let (start_line, _) = context.line_col(start);
            let (end_line, _) = context.line_col(end);
            return start_line == end_line && start_line == line;
        }
    }
    false
}

/// Whether `line` is a *next-line* suppression comment written by this tool or
/// a foreign one — i.e. one whose semantics claim the immediately following
/// code line (`biome-ignore`, `eslint-disable-next-line`,
/// `custom-biome-ignore-next-line`). Used to avoid placing a leading own-line
/// marker between such a comment and the code it suppresses. Directives that act
/// on their own line (`eslint-disable`, `eslint-disable-line`,
/// `custom-biome-ignore-line`) do NOT claim the next line, so a marker placed
/// on the following line does not disturb them and must not be treated as
/// circularly adjacent. See docs/WRITE_FIX_SUPPRESSION_PLACEMENT_BUG.md, Case 4
/// and the CodeRabbit review on PR #27.
fn is_next_line_suppression_comment(line: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix("//") else {
        return false;
    };
    let rest = rest.trim_start();
    for prefix in [
        "biome-ignore",
        "eslint-disable-next-line",
        "custom-biome-ignore-next-line",
    ] {
        if let Some(after) = rest.strip_prefix(prefix) {
            // Require a word boundary after the prefix so near-misses such as
            // `eslint-disablement` are not mistaken for a suppression comment.
            // A delimiter (`-` for the `-line`/`-next-line` variants, `:` or
            // whitespace for the others) or end-of-comment is expected.
            match after.chars().next() {
                None | Some(' ') | Some('\t') | Some(':') | Some('-') => return true,
                _ => {}
            }
        }
    }
    false
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
        let with_col: Vec<(usize, usize, &'static str)> = violations
            .iter()
            .map(|&(line, rule)| (line, 1, rule))
            .collect();
        plan_at(source, &with_col)
    }

    fn plan_at(source: &str, violations: &[(usize, usize, &'static str)]) -> FilePlan {
        let violations: Vec<Violation> = violations
            .iter()
            .map(|&(line, col, rule)| Violation::error(rule, line, col, "test"))
            .collect();
        plan_file(Path::new("a.jsx"), source, &violations)
    }

    #[test]
    fn short_line_gets_a_trailing_comment() {
        let plan = plan("const m = new Map();\n", &[(1, "no-native-map")]);
        assert_eq!(
            plan.source,
            "const m = new Map(); // custom-biome-ignore-line no-native-map\n"
        );
        assert_eq!(plan.changes[0].placement, Placement::Trailing);
    }

    #[test]
    fn long_line_gets_the_comment_above_with_matching_indent() {
        let source = format!("    const m = new Map(); // {}\n", "x".repeat(80));
        let plan = plan(&source, &[(1, "no-native-map")]);
        assert!(plan
            .source
            .starts_with("    // custom-biome-ignore-next-line no-native-map\n"));
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
            .contains("// custom-biome-ignore-line no-native-map, reselect-arity-match"));
    }

    #[test]
    fn an_existing_comment_is_extended_rather_than_duplicated() {
        let source = "const m = new Map(); // custom-biome-ignore-line no-native-map\n";
        let plan = plan(source, &[(1, "reselect-arity-match")]);
        assert_eq!(plan.changes[0].placement, Placement::Merged);
        assert_eq!(
            plan.source,
            "const m = new Map(); // custom-biome-ignore-line no-native-map, reselect-arity-match\n"
        );
    }

    #[test]
    fn a_justification_survives_a_merge() {
        let source = "x(); // custom-biome-ignore-line no-native-map -- legacy\n";
        let plan = plan(source, &[(1, "reselect-arity-match")]);
        assert_eq!(
            plan.source,
            "x(); // custom-biome-ignore-line no-native-map, reselect-arity-match -- legacy\n"
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
            "const a = (\n  <div>\n    {/* custom-biome-ignore-next-line no-native-map */}\n    {new Map()}\n  </div>\n);\n"
        );
        assert!(
            !plan.source.contains("// custom-biome-ignore"),
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
            "const a = <Row data={new Map()} />; // custom-biome-ignore-line no-native-map\n"
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
            "const a = 1;\r\nconst m = new Map(); // custom-biome-ignore-line no-native-map"
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
        let source =
            "const m = new Map(); // custom-biome-ignore-next-line no-native-map\nother();\n";
        let plan = plan(source, &[(1, "reselect-arity-match")]);
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
        assert!(plan
            .source
            .starts_with("// custom-biome-ignore-next-line reselect-arity-match\n"));
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

    // --- docs/WRITE_FIX_SUPPRESSION_PLACEMENT_BUG.md regression cases ---

    #[test]
    fn multi_line_arrow_body_gets_own_line_not_trailing() {
        // Case 1: the violation is on the arrow's opening line; the enclosing
        // statement spans the whole arrow body, so a trailing comment would be
        // relocated by the formatter. Own-line placement keeps it attached.
        let source = "beforeToolbarCreated = toolbar => {\n  const tabs = toolbar.getTabs();\n  toolbar.getTabs = () => {\n    const exportTab = tabs.find(tab => tab.id === 'fm-tab-export');\n  };\n};\n";
        let plan = plan(source, &[(3, "bare-arrow-param-prop-assign")]);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
        let lines: Vec<&str> = plan.source.lines().collect();
        // Marker sits above the opening line, not trailing on it.
        assert!(lines[2].contains("// custom-biome-ignore-next-line bare-arrow-param-prop-assign"));
        assert!(!lines[3].contains("custom-biome-ignore-line"));
    }

    #[test]
    fn multi_line_object_literal_gets_own_line() {
        // Case 2: the violation line opens a multi-line object literal, so the
        // enclosing statement spans several lines.
        let source = "if (!(accum[routeId] || {}).hasOwnProperty(eventId)) {\n  accum[routeId][eventId] = {\n    eventId,\n    published,\n  };\n}\n";
        let plan = plan(source, &[(2, "deep-param-prop-assign")]);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
        let lines: Vec<&str> = plan.source.lines().collect();
        assert!(lines[1].contains("// custom-biome-ignore-next-line deep-param-prop-assign"));
        assert!(!lines[2].contains("custom-biome-ignore-line"));
    }

    #[test]
    fn jsx_attribute_arrow_body_uses_plain_comment_not_brace() {
        // Case 3: an arrow body passed as a JSX attribute value is ordinary JS,
        // not JSX children, so the marker must be a plain `//` comment -- not
        // the inert `{/* ... */}` block form.
        let source = "const el = (\n  <input\n    onChange={event => {\n      filterList[index][0] = event.target.value;\n      onChange(filterList[index], index, column);\n    }}\n  />\n);\n";
        let plan = plan(source, &[(4, "deep-param-prop-assign")]);
        assert_eq!(plan.changes.len(), 1);
        // The brace form inside a JS function body is inert and must never be
        // emitted here.
        assert!(!plan.source.contains("{/*"));
        assert!(plan.source.contains("// custom-biome-ignore"));
    }

    #[test]
    fn foreign_suppression_above_single_line_target_uses_trailing() {
        // Case 4 / Case 7: a pre-existing `biome-ignore` directly above a
        // single-line target. Trailing keeps the foreign comment leading and
        // adjacent instead of inserting a broken own-line marker in between.
        let source = "// biome-ignore lint/style/noParameterAssign: ported\naccum.stackedData[valKey] = {};\n";
        let plan = plan(source, &[(2, "deep-param-prop-assign")]);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].placement, Placement::Trailing);
        assert!(plan.source.contains(
            "accum.stackedData[valKey] = {}; // custom-biome-ignore-line deep-param-prop-assign"
        ));
        assert!(plan
            .source
            .contains("// biome-ignore lint/style/noParameterAssign"));
    }

    #[test]
    fn foreign_suppression_above_multi_line_target_is_unfixable() {
        // Case 4 / Case 8: no placement satisfies both tools when the target
        // spans multiple lines, so report it rather than writing a broken file.
        let source = "// biome-ignore lint/style/noParameterAssign: ported\naccum.stackedData[valKey] = {\n  y: 1,\n};\n";
        let plan = plan(source, &[(2, "deep-param-prop-assign")]);
        assert!(plan.changes.is_empty());
        assert_eq!(plan.unfixable.len(), 1);
        assert_eq!(
            plan.unfixable[0].reason,
            "leading line already claimed by another tool's suppression comment and target spans multiple lines"
        );
    }

    #[test]
    fn near_miss_suppression_comment_is_not_treated_as_foreign() {
        // `eslint-disablement` is a near-miss of `eslint-disable`; it must not
        // be mistaken for a foreign suppression comment, so a multi-line target
        // below it is still fixed with an own-line marker rather than reported
        // unfixable.
        let source =
            "// eslint-disablement some-other-thing\naccum.stackedData[valKey] = {\n  y: 1,\n};\n";
        let plan = plan(source, &[(2, "deep-param-prop-assign")]);
        assert!(plan.unfixable.is_empty());
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
    }

    #[test]
    fn single_line_statement_inside_multi_line_arrow_still_gets_trailing() {
        // The statement-boundedness check keys off the *smallest* enclosing
        // statement: a self-contained single-line statement inside a multi-line
        // arrow body is safe to annotate with a trailing comment.
        let source = "fn = () => {\n  item.x = 1;\n  return item;\n};\n";
        let plan = plan(source, &[(2, "bare-arrow-param-prop-assign")]);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].placement, Placement::Trailing);
    }

    #[test]
    fn genuine_jsx_child_expression_still_uses_brace_form() {
        // Contrast to Case 3: a `{...}` expression that is really a JSX child
        // keeps the `{/* ... */}` form.
        let source = "const a = (\n  <div>\n    {new Map()}\n  </div>\n);\n";
        let plan = plan(source, &[(3, "no-native-map")]);
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
        assert!(plan
            .source
            .contains("{/* custom-biome-ignore-next-line no-native-map */}"));
    }

    #[test]
    fn two_statements_on_line_violation_in_multiline_gets_own_line() {
        // Regression for the CodeRabbit finding: anchoring the statement-
        // boundedness check at the line start would see `noop();` (a single-line
        // statement) and wrongly emit a trailing comment, which the formatter
        // later detaches from the multi-line assignment. Anchoring at the
        // violation column selects `accum.x = {...}` and yields an own-line
        // marker instead.
        let source = "noop(); accum.x = {\n  y: 1,\n};\n";
        let plan = plan_at(source, &[(1, 9, "deep-param-prop-assign")]);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
        let lines: Vec<&str> = plan.source.lines().collect();
        assert!(lines[0].contains("// custom-biome-ignore-next-line deep-param-prop-assign"));
    }

    #[test]
    fn eslint_disable_above_multiline_target_gets_own_line() {
        // `eslint-disable` (block form) acts on its own line; it does not claim
        // the next physical line. So a multi-line target below it is fixable with
        // an own-line marker rather than reported unfixable. Regression for the
        // CodeRabbit finding that the foreign-adjacency check over-matched
        // non-next-line directives.
        let source = "// eslint-disable no-console\naccum.x = {\n  y: 1,\n};\n";
        let plan = plan(source, &[(2, "deep-param-prop-assign")]);
        assert!(plan.unfixable.is_empty());
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
    }

    #[test]
    fn eslint_disable_next_line_above_multiline_target_is_unfixable() {
        // A next-line directive DOES claim the following line, so circular
        // adjacency still applies: a multi-line target below it is unfixable.
        // Guards the narrowing of the foreign-adjacency check against dropping
        // legitimate next-line directives.
        let source = "// eslint-disable-next-line no-console\naccum.x = {\n  y: 1,\n};\n";
        let plan = plan(source, &[(2, "deep-param-prop-assign")]);
        assert!(plan.changes.is_empty());
        assert_eq!(plan.unfixable.len(), 1);
        assert_eq!(
            plan.unfixable[0].reason,
            "leading line already claimed by another tool's suppression comment and target spans multiple lines"
        );
    }

    #[test]
    fn multi_line_function_body_gets_own_line() {
        // Table row 3: a non-arrow `function () {}` assigned across lines has the
        // same reflow risk as the arrow form; the own-line placement must apply
        // here too.
        let source = "toolbar.getTabs = function () {\n  const exportTab = new Map();\n};\n";
        let plan = plan(source, &[(1, "bare-arrow-param-prop-assign")]);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
    }

    #[test]
    fn chained_assignment_spanning_lines_gets_own_line() {
        // Table row 9: a chained/nested assignment whose LHS token sits on the
        // first line but whose statement spans multiple lines. The statement-
        // boundedness check must treat it as multi-line and place the marker on
        // its own line, not trailing on the LHS.
        let source =
            "accum[diffInDays].orderTotal =\n  accum[diffInDays].orderTotal + summary.total;\n";
        let plan = plan(source, &[(1, "deep-param-prop-assign")]);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].placement, Placement::OwnLine);
    }
}
