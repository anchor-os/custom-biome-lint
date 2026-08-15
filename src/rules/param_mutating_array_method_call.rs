//! Mutating array-method calls on a function parameter, at any receiver depth.
//!
//! The companion to the assignment-shaped parameter-mutation rules
//! (`destructure-default-param-assign`, `destructure-param-prop-assign`,
//! `bare-arrow-param-prop-assign`, `deep-param-prop-assign`). Those four key
//! off assignment/update expressions; none inspect a `CallExpression`. That
//! leaves `param.push(item)` — a genuine parameter mutation — invisible to
//! every one of them and to Biome's `noParameterAssign` (which also only
//! sees assignments). See `docs/PARAM_MUTATING_METHOD_CALL_RULE_PLAN.md`.
//!
//! Detection is name-based: a fixed list of array-mutating method names
//! (`push`, `pop`, `shift`, `unshift`, `splice`, `sort`, `reverse`, `fill`,
//! `copyWithin`) called on a receiver whose chain root resolves to a
//! parameter binding. There is deliberately no type inference — this tool
//! resolves identifiers to *bindings*, not *types* — which is why the rule
//! ships off by default and carries a low-confidence signal for the
//! Immutable.js / redux-form `fields` idioms that reuse these same method
//! names without mutating anything.

use biome_js_syntax::{
    AnyJsExpression, JsCallExpression, JsImport, JsReferenceIdentifier, JsSyntaxNode,
};
use biome_rowan::AstNode;

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;
use crate::semantic::{BindingKind, SemanticModel};

/// Array methods that mutate their receiver in place. Bracket/form calls
/// (`list['push'](x)`) and the Map/Set-shaped `set`/`delete`/`clear`/`add`
/// are deliberately excluded — see the plan's Non-goals.
const MUTATING_METHODS: &[&str] = &[
    "push",
    "pop",
    "shift",
    "unshift",
    "splice",
    "sort",
    "reverse",
    "fill",
    "copyWithin",
];

const MESSAGE: &str =
    "Mutating array method called on a parameter — this modifies data the caller still holds a \
     reference to. Copy the parameter first.";

pub struct ParamMutatingArrayMethodCall;

impl Rule for ParamMutatingArrayMethodCall {
    fn name(&self) -> &'static str {
        "param-mutating-array-method-call"
    }

    fn description(&self) -> &'static str {
        "Disallow mutating array-method calls on function parameters"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS
    }

    fn default_severity(&self) -> crate::config::RuleSeverity {
        crate::config::RuleSeverity::Off
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let semantic = file.semantic();
        let imports_immutable = file_imports_immutable(file.tree());
        let mut violations = Vec::new();

        for node in file.tree().descendants() {
            let Some(call) = JsCallExpression::cast_ref(&node) else {
                continue;
            };
            let Some(root) = mutating_param_receiver(&call, semantic) else {
                continue;
            };

            let (line, col) =
                file.line_col(usize::from(root.syntax().text_trimmed_range().start()));

            let low_confidence = low_confidence_marker(&root, imports_immutable);
            let message = match low_confidence {
                Some(note) => format!("{MESSAGE} {note}"),
                None => MESSAGE.to_string(),
            };

            violations.push(Violation::error(self.name(), line, col, message));
        }

        violations
    }
}

/// If `call` is a mutating-array-method call on a parameter-bound receiver,
/// returns the chain-root reference identifier (the parameter use site, where
/// the diagnostic is anchored). Otherwise `None`.
fn mutating_param_receiver(
    call: &JsCallExpression,
    semantic: &SemanticModel,
) -> Option<JsReferenceIdentifier> {
    let Ok(callee) = call.callee() else {
        return None;
    };

    // Only the dotted form (`x.push`), not `x['push']` — computed calls are a
    // v1 non-goal. `JsStaticMemberExpression` also covers `x?.push`.
    let member = match &callee {
        AnyJsExpression::JsStaticMemberExpression(member) => member,
        _ => return None,
    };

    let method_name = member
        .member()
        .ok()
        .and_then(|m| m.as_js_name().cloned())
        .and_then(|name| name.value_token().ok())
        .map(|token| token.text_trimmed().to_string())?;
    if !MUTATING_METHODS.contains(&method_name.as_str()) {
        return None;
    }

    let root = chain_root(member.object().ok()?)?;
    let binding = semantic.resolve(&root)?;
    match binding.kind {
        BindingKind::Parameter => Some(root),
        _ => None,
    }
}

/// Walks a member/index/parenthesized chain down to its leftmost identifier,
/// the same leftmost-root walk the assignment rules use — `accum[key].items`
/// and `accum.items` both bottom out in `accum`.
fn chain_root(mut expr: AnyJsExpression) -> Option<JsReferenceIdentifier> {
    loop {
        expr = match expr {
            AnyJsExpression::JsIdentifierExpression(ident) => return ident.name().ok(),
            AnyJsExpression::JsStaticMemberExpression(member) => member.object().ok()?,
            AnyJsExpression::JsComputedMemberExpression(member) => member.object().ok()?,
            AnyJsExpression::JsParenthesizedExpression(paren) => paren.expression().ok()?,
            _ => return None,
        };
    }
}

/// Whether the file imports from `immutable` (including `/immutable`-suffixed
/// subpaths like `redux-form/immutable`), the dominant non-mutating
/// same-name API in the corpus this rule was scoped against.
fn file_imports_immutable(tree: &JsSyntaxNode) -> bool {
    tree.descendants().any(|node| {
        let Some(import) = JsImport::cast_ref(&node) else {
            return false;
        };
        let Ok(source) = import.source_text() else {
            return false;
        };
        let source = source.text();
        source == "immutable" || source.ends_with("/immutable")
    })
}

/// A short annotation for findings where the receiver might not actually be a
/// mutating array — surfaced in the message, not used to gate the finding.
/// `None` means the finding is a plain-array candidate and has no such doubt.
fn low_confidence_marker(root: &JsReferenceIdentifier, imports_immutable: bool) -> Option<String> {
    let value_token = root.value_token().ok()?;
    let name = value_token.text_trimmed();
    if imports_immutable {
        return Some("(low confidence: file imports 'immutable')".to_string());
    }
    if name == "fields" || name == "field" {
        return Some(
            "(low confidence: receiver named 'fields' may be a redux-form helper)".to_string(),
        );
    }
    None
}
