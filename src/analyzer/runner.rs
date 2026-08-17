use std::cell::OnceCell;
use std::path::Path;

use biome_js_parser::{parse, JsParserOptions};
use biome_js_syntax::JsSyntaxNode;
use biome_languages::JsFileSource;

use crate::diagnostics::{Edit, Suggestion, Violation};
use crate::fixer::plan_file;
use crate::rules::Rule;
use crate::semantic::SemanticModel;
use crate::suppress::Suppressions;

/// One file, parsed exactly once. Every rule that runs against the file shares
/// this tree and this line index rather than re-parsing the source.
pub struct FileContext<'a> {
    path: &'a Path,
    source: &'a str,
    tree: JsSyntaxNode,
    line_starts: Vec<usize>,
    parsed_cleanly: bool,
    /// Built lazily on first access via [`Self::semantic`] and shared by
    /// every rule that asks for it afterward, so a rule that never calls
    /// `semantic()` never pays to build it.
    semantic: OnceCell<SemanticModel>,
}

impl<'a> FileContext<'a> {
    pub fn parse(source: &'a str, path: &'a Path) -> Self {
        // jsx() is a superset of plain JS here: this codebase has JSX inside .js files.
        let parsed = parse(source, JsFileSource::jsx(), JsParserOptions::default());
        Self {
            path,
            source,
            parsed_cleanly: !parsed.has_errors(),
            tree: parsed.syntax(),
            line_starts: line_starts(source),
            semantic: OnceCell::new(),
        }
    }

    pub fn path(&self) -> &Path {
        self.path
    }

    pub fn source(&self) -> &str {
        self.source
    }

    pub fn tree(&self) -> &JsSyntaxNode {
        &self.tree
    }

    pub fn parsed_cleanly(&self) -> bool {
        self.parsed_cleanly
    }

    /// Byte offset -> 1-based line and column.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        (line + 1, offset - self.line_starts[line] + 1)
    }

    /// The file's lexical scope/binding model, built on first use and
    /// shared by every rule that asks for it afterward. See
    /// `docs/SEMANTIC_MODEL.md`.
    pub fn semantic(&self) -> &SemanticModel {
        self.semantic
            .get_or_init(|| SemanticModel::build(&self.tree))
    }
}

pub struct AnalyzedFile {
    pub violations: Vec<Violation>,
    pub parsed_cleanly: bool,
    pub rules_run: Vec<&'static str>,
}

/// Parses `source` once, runs every rule that supports the file's extension,
/// drops suppressed violations, and returns the rest in source order.
///
/// The public entry points are [`analyze_file`] (no IDE enrichment) and
/// [`analyze_file_enriched`] (fills in the machine-readable fix/suppression
/// edits). This private helper carries the `enrich` flag so the two public
/// signatures stay backward compatible — existing consumers keep calling
/// `analyze_file` with three arguments and never see the IDE-only fields.
fn analyze_file_impl(path: &Path, source: &str, rules: &[&dyn Rule], enrich: bool) -> AnalyzedFile {
    let context = FileContext::parse(source, path);
    let suppressions = Suppressions::parse(source);

    let mut violations = Vec::new();
    let mut rules_run = Vec::new();

    for rule in rules {
        if !rule_supports(*rule, path) {
            continue;
        }
        rules_run.push(rule.name());
        for violation in rule.check(&context) {
            if suppressions.is_suppressed(violation.line, violation.rule) {
                continue;
            }
            violations.push(violation);
        }
    }

    violations.sort_by_key(|v| (v.line, v.col, v.rule));

    // Enrichment is additive: only when requested do we surface the IDE-only
    // fields (`fixes`/`suppressions`). The base `version:1` contract stays
    // unchanged, and unenriched callers (existing JSON consumers) get no
    // suggestions.
    if enrich {
        // Safe-fix edits reuse each rule's own `Fix` byte range — the exact
        // same data `--auto-fix` applies to disk — so the IDE contract and the
        // CLI autofix can never silently disagree about what a fix does.
        for violation in &mut violations {
            if let Some(fix) = &violation.fix {
                let (s_line, s_col) = context.line_col(fix.start);
                let (e_line, e_col) = context.line_col(fix.end);
                violation.fixes.push(Suggestion {
                    kind: "safe",
                    title: format!("Apply safe fix for {}", violation.rule),
                    edits: vec![Edit {
                        start_line: s_line,
                        start_column: s_col,
                        end_line: e_line,
                        end_column: e_col,
                        replacement: fix.replacement.clone(),
                    }],
                });
            }
        }

        attach_suppression_suggestions(&mut violations, &context, path, source);
    }

    AnalyzedFile {
        violations,
        parsed_cleanly: context.parsed_cleanly(),
        rules_run,
    }
}

/// Parses and lints a single file, returning the same shape as before this
/// IDE work. Violations do **not** carry the IDE-only `fixes`/`suppressions`
/// fields. Kept at its original three-argument signature so existing library
/// and test consumers keep compiling unchanged.
pub fn analyze_file(path: &Path, source: &str, rules: &[&dyn Rule]) -> AnalyzedFile {
    analyze_file_impl(path, source, rules, false)
}

/// Like [`analyze_file`], but each surviving violation is also filled in with
/// the machine-readable fix and suppression edits the IDE contract exposes (see
/// [`Violation::fixes`] / [`Violation::suppressions`]). The CLI enables this
/// only for `--format json`; text and `--auto-fix`/`--write-fix` output use the
/// unenriched [`analyze_file`].
pub fn analyze_file_enriched(path: &Path, source: &str, rules: &[&dyn Rule]) -> AnalyzedFile {
    analyze_file_impl(path, source, rules, true)
}

/// Offers a suppression-comment insertion for every violation the Rust tool can
/// actually place one for, reusing the exact placement logic `--write-fix`
/// uses. The IDE must never compute suppression placement itself — this keeps
/// every placement rule (JS/JSX, comments, existing markers, line length)
/// owned by Rust.
fn attach_suppression_suggestions(
    violations: &mut [Violation],
    context: &FileContext<'_>,
    path: &Path,
    source: &str,
) {
    let plan = plan_file(path, source, violations);
    for change in &plan.changes {
        let (line, col) = context.line_col(change.insert_offset);
        let edit = Edit {
            start_line: line,
            start_column: col,
            end_line: line,
            end_column: col,
            replacement: change.insert_text.clone(),
        };
        for violation in violations.iter_mut() {
            if violation.line == change.line_number
                && change.rules.iter().any(|rule| rule == violation.rule)
            {
                violation.suppressions.push(Suggestion {
                    kind: "suppress",
                    title: format!("Suppress {}", violation.rule),
                    edits: vec![edit.clone()],
                });
            }
        }
    }
}

pub fn rule_supports(rule: &dyn Rule, path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    rule.supported_extensions()
        .iter()
        .any(|supported| supported.trim_start_matches('.') == ext)
}

fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::semantic::BindingKind;
    use biome_js_syntax::JsReferenceIdentifier;
    use biome_rowan::AstNode;

    /// Cypress step definitions use `$`-prefixed identifiers (the `$el`
    /// convention for aliased subjects, e.g. `cy.then(($el) => { ... })`).
    /// Biome < 1.0 rejected these as identifiers, which made every such file
    /// emit a spurious parse-error warning. Modern Biome (2.x) must accept them.
    #[test]
    fn dollar_prefixed_identifiers_parse_cleanly() {
        let src = "cy.then(($el) => {\n  expect($el).to.exist;\n});\n";
        let ctx = FileContext::parse(src, Path::new("stepDefinitions/foo.cy.js"));
        assert!(
            ctx.parsed_cleanly(),
            "expected $-prefixed identifiers (Cypress $el) to parse cleanly"
        );

        // The `$el` *use* must resolve to its arrow parameter binding, not leak
        // to a global — otherwise rules that read the semantic model would
        // mis-handle Cypress step definitions.
        let binding = ctx
            .semantic()
            .resolve(&find_reference(&ctx, "$el", 0))
            .unwrap_or_else(|| {
                panic!("expected `$el` reference to resolve to its arrow parameter")
            });
        assert_eq!(binding.name, "$el");
        assert_eq!(binding.kind, BindingKind::Parameter);

        let src2 = "const f = ($el) => $el + 1;\n";
        let ctx2 = FileContext::parse(src2, Path::new("a.js"));
        assert!(
            ctx2.parsed_cleanly(),
            "expected $-prefixed identifiers to parse cleanly"
        );

        let binding2 = ctx2
            .semantic()
            .resolve(&find_reference(&ctx2, "$el", 0))
            .unwrap_or_else(|| {
                panic!("expected `$el` reference to resolve to its arrow parameter")
            });
        assert_eq!(binding2.name, "$el");
        assert_eq!(binding2.kind, BindingKind::Parameter);
    }

    /// The `n`th (0-based, source order) *use* of `name` — a
    /// `JsReferenceIdentifier`, not a declaration — mirroring the resolution
    /// helpers in `tests/integration.rs`.
    fn find_reference(ctx: &FileContext<'_>, name: &str, n: usize) -> JsReferenceIdentifier {
        ctx.tree()
            .descendants()
            .filter_map(JsReferenceIdentifier::cast)
            .filter(|ident| {
                ident
                    .value_token()
                    .is_ok_and(|token| token.text_trimmed() == name)
            })
            .nth(n)
            .unwrap_or_else(|| panic!("no occurrence #{n} of reference `{name}`"))
    }
}
