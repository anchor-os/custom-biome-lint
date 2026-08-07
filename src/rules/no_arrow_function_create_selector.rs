use biome_js_syntax::{
    AnyJsBindingPattern, AnyJsExpression, AnyJsFunctionBody, JsArrowFunctionExpression,
    JsInitializerClause, JsVariableDeclarator,
};
use biome_rowan::AstNode;

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;

pub struct NoArrowFunctionCreateSelector;

impl Rule for NoArrowFunctionCreateSelector {
    fn name(&self) -> &'static str {
        "no-arrow-function-create-selector"
    }

    fn description(&self) -> &'static str {
        "Disallow wrapping createSelector in an arrow function, which breaks memoization"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let mut violations = Vec::new();

        for node in file.tree().descendants() {
            let Some(arrow) = JsArrowFunctionExpression::cast_ref(&node) else {
                continue;
            };
            if !is_bare_create_selector_body(&arrow) {
                continue;
            }
            let Some(declarator) = enclosing_declarator(&arrow) else {
                continue;
            };
            let Some(name) = declared_name(&declarator) else {
                continue;
            };
            if is_factory_name(&name) {
                continue;
            }

            let offset = usize::from(arrow.syntax().text_trimmed_range().start());
            let (line, col) = file.line_col(offset);
            violations.push(Violation::error(
                self.name(),
                line,
                col,
                format!(
                    "Avoid wrapping createSelector in an arrow function for \"{name}\". \
                     It breaks memoization (a new selector is created on every call). \
                     Use createSelector directly, or rename to \"{}\".",
                    factory_name(&name)
                ),
            ));
        }

        violations
    }
}

/// True when the arrow's concise body is exactly `createSelector(...)`.
fn is_bare_create_selector_body(arrow: &JsArrowFunctionExpression) -> bool {
    let Ok(AnyJsFunctionBody::AnyJsExpression(expr)) = arrow.body() else {
        return false;
    };
    let AnyJsExpression::JsCallExpression(call) = expr else {
        return false;
    };
    let Ok(AnyJsExpression::JsIdentifierExpression(ident)) = call.callee() else {
        return false;
    };
    ident
        .name()
        .and_then(|n| n.value_token())
        .is_ok_and(|t| t.text_trimmed() == "createSelector")
}

/// The arrow must be the *initializer* of a declarator, matching the original
/// ESLint rule's direct-parent check. In Biome's tree that is
/// `JsVariableDeclarator > JsInitializerClause > JsArrowFunctionExpression`.
fn enclosing_declarator(arrow: &JsArrowFunctionExpression) -> Option<JsVariableDeclarator> {
    let initializer = arrow
        .syntax()
        .parent()
        .and_then(JsInitializerClause::cast)?;
    initializer
        .syntax()
        .parent()
        .and_then(JsVariableDeclarator::cast)
}

fn declared_name(declarator: &JsVariableDeclarator) -> Option<String> {
    let AnyJsBindingPattern::AnyJsBinding(binding) = declarator.id().ok()? else {
        return None;
    };
    let ident = binding.as_js_identifier_binding()?;
    Some(ident.name_token().ok()?.text_trimmed().to_string())
}

/// Mirrors the original ESLint rule's `/^make[A-Z]/` factory-name test: a
/// deliberate selector *factory* is allowed to wrap createSelector.
fn is_factory_name(name: &str) -> bool {
    name.strip_prefix("make")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_uppercase())
}

fn factory_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => format!("make{}{}", first.to_uppercase(), chars.as_str()),
        None => "makeSelector".to_string(),
    }
}
