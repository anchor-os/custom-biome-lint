use biome_js_syntax::{
    AnyJsArrowFunctionParameters, AnyJsCallArgument, AnyJsExpression, JsCallExpression,
};
use biome_rowan::{AstNode, AstSeparatedList};

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::rules::rule::Rule;
use crate::rules::{JS_EXTENSIONS, JS_PATTERN};

pub struct ReselectArityMatch;

impl Rule for ReselectArityMatch {
    fn name(&self) -> &'static str {
        "reselect-arity-match"
    }

    fn description(&self) -> &'static str {
        "Ensure input selectors and result function arity in createSelector match"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS
    }

    fn default_pattern(&self) -> &'static str {
        JS_PATTERN
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let mut violations = Vec::new();

        for node in file.tree().descendants() {
            let Some(call) = JsCallExpression::cast_ref(&node) else {
                continue;
            };
            if !is_create_selector_callee(&call) {
                continue;
            }

            let Ok(arguments) = call.arguments() else {
                continue;
            };
            let args: Vec<AnyJsCallArgument> = arguments.args().iter().flatten().collect();
            if args.len() < 2 {
                continue;
            }

            let Some(AnyJsCallArgument::AnyJsExpression(result_func)) = args.last() else {
                continue;
            };
            let Some((param_count, offset)) = result_function_arity(result_func) else {
                continue;
            };

            let expected = args.len() - 1;
            if expected == param_count {
                continue;
            }

            let (line, col) = file.line_col(offset);
            violations.push(Violation::error(
                self.name(),
                line,
                col,
                format!(
                    "createSelector expects {expected} parameter(s) in the result function, \
                     but found {param_count}."
                ),
            ));
        }

        violations
    }
}

/// Matches `createSelector(...)` and `something.createSelector(...)`, mirroring
/// the ESLint rule's Identifier/MemberExpression callee check.
fn is_create_selector_callee(call: &JsCallExpression) -> bool {
    match call.callee() {
        Ok(AnyJsExpression::JsIdentifierExpression(ident)) => ident
            .name()
            .and_then(|n| n.value_token())
            .is_ok_and(|t| t.text_trimmed() == "createSelector"),
        Ok(AnyJsExpression::JsStaticMemberExpression(member)) => member
            .member()
            .ok()
            .and_then(|m| m.as_js_name().cloned())
            .and_then(|name| name.value_token().ok())
            .is_some_and(|t| t.text_trimmed() == "createSelector"),
        _ => false,
    }
}

/// Parameter count and report offset for an arrow or function expression.
/// Returns `None` for anything else — the ESLint rule only checks these two
/// forms, since a selector passed by reference has no visible arity.
fn result_function_arity(expr: &AnyJsExpression) -> Option<(usize, usize)> {
    let (count, node) = match expr {
        AnyJsExpression::JsArrowFunctionExpression(arrow) => {
            let count = match arrow.parameters().ok()? {
                // `x => ...` binds exactly one parameter without a parameter list.
                AnyJsArrowFunctionParameters::AnyJsBinding(_) => 1,
                AnyJsArrowFunctionParameters::JsParameters(params) => params.items().len(),
            };
            (count, arrow.syntax())
        }
        AnyJsExpression::JsFunctionExpression(func) => {
            let count = func.parameters().ok()?.items().len();
            (count, func.syntax())
        }
        _ => return None,
    };
    Some((count, usize::from(node.text_trimmed_range().start())))
}
