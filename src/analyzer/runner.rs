use std::cell::OnceCell;
use std::path::Path;

use biome_js_parser::{parse, JsParserOptions};
use biome_js_syntax::{JsFileSource, JsSyntaxNode};

use crate::diagnostics::Violation;
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
pub fn analyze_file(path: &Path, source: &str, rules: &[&dyn Rule]) -> AnalyzedFile {
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

    AnalyzedFile {
        violations,
        parsed_cleanly: context.parsed_cleanly(),
        rules_run,
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
