use biome_js_syntax::{
    AnyJsBindingPattern, AnyJsExpression, AnyJsFunctionBody, JsArrowFunctionExpression,
    JsCallExpression, JsInitializerClause, JsVariableDeclarator,
};
use biome_rowan::AstNode;

use crate::analyzer::runner::FileContext;
use crate::diagnostics::{Fix, Violation};
use crate::rules::reselect::resolves_to_reselect_create_selector;
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;
use crate::semantic::SemanticModel;

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
        let semantic = file.semantic();
        let mut violations = Vec::new();

        for node in file.tree().descendants() {
            let Some(arrow) = JsArrowFunctionExpression::cast_ref(&node) else {
                continue;
            };
            let Some(call) = bare_create_selector_body(&arrow, semantic) else {
                continue;
            };
            let Some(declarator) = enclosing_declarator(&arrow) else {
                continue;
            };
            let Some(name) = declared_name(&declarator) else {
                continue;
            };
            if is_factory_name(&name) {
                continue;
            }

            let range = arrow.syntax().text_trimmed_range();
            let offset = usize::from(range.start());
            let (line, col) = file.line_col(offset);
            let mut violation = Violation::error(
                self.name(),
                line,
                col,
                format!(
                    "Avoid wrapping createSelector in an arrow function for \"{name}\". \
                     It breaks memoization (a new selector is created on every call). \
                     Use createSelector directly, or rename to \"{}\".",
                    factory_name(&name)
                ),
            );
            // Unwrapping the arrow is otherwise always the same mechanical
            // edit: replace the whole `(...) => createSelector(...)` with
            // just the call, keeping the call's own original formatting
            // verbatim. But an `async` arrow returns a Promise that resolves
            // to the selector, not the selector itself -- unwrapping it would
            // silently change what callers get back, so it is still reported
            // but left for a human to fix.
            if arrow.async_token().is_none() {
                violation = violation.with_fix(Fix {
                    start: offset,
                    end: usize::from(range.end()),
                    replacement: call.syntax().text_trimmed().to_string(),
                });
            }
            violations.push(violation);
        }

        violations
    }
}

/// The arrow's concise body, if it is exactly a call to reselect's
/// `createSelector` -- resolved semantically, so a same-named local
/// function or a `createSelector` imported from a different module
/// correctly does not match, and an aliased import
/// (`import { createSelector as selector } from "reselect"`) does, even
/// though the identifier here is spelled `selector`.
fn bare_create_selector_body(
    arrow: &JsArrowFunctionExpression,
    semantic: &SemanticModel,
) -> Option<JsCallExpression> {
    let AnyJsFunctionBody::AnyJsExpression(expr) = arrow.body().ok()? else {
        return None;
    };
    let AnyJsExpression::JsCallExpression(call) = expr else {
        return None;
    };
    let AnyJsExpression::JsIdentifierExpression(ident) = call.callee().ok()? else {
        return None;
    };
    let reference = ident.name().ok()?;
    resolves_to_reselect_create_selector(semantic, &reference).then_some(call)
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
