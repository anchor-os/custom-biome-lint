//! Property mutation of an arrow function's sole, unparenthesized parameter.
//!
//! **Off by default** — see [`Rule::default_severity`]. This is a plain
//! parameter, so Biome's `noParameterAssign` is nominally responsible for it;
//! the rule exists because Biome misses this one AST shape. Verified against
//! Biome 2.5.8:
//!
//! ```js
//! export const bare  =  d  => { d.token = 'x'; };  // NOT flagged by Biome
//! export const paren = (d) => { d.token = 'x'; };  // flagged
//! ```
//!
//! The cause is structural, not a formatter setting: an arrow with a single
//! unparenthesized parameter binds it directly under the arrow as an
//! `AnyJsBinding`, with no `JsParameters` node — unlike `(d) => ...`,
//! `(a, b) => ...` and `function f(d) {}`, which Biome all handles. A repo
//! formatting with `arrowParentheses: "asNeeded"` just makes the missed shape
//! the common one.
//!
//! Scope is property mutation only. Bare *reassignment* (`d => { d = 5 }`) is
//! **not** covered, because the same repro shows Biome does flag that — the
//! asymmetry was checked rather than assumed.

use crate::analyzer::runner::FileContext;
use crate::config::RuleSeverity;
use crate::diagnostics::Violation;
use crate::rules::param_mutation::{
    assignment_targets, eligible_member_target, report_offset, ParamShape, ParamShapes,
};
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;

pub struct BareArrowParamPropAssign;

impl Rule for BareArrowParamPropAssign {
    fn name(&self) -> &'static str {
        "bare-arrow-param-prop-assign"
    }

    fn description(&self) -> &'static str {
        "Disallow mutating a property of an arrow function's unparenthesized single parameter \
         (off by default)"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS
    }

    /// Opt-in: the finding is a formatting-adjacent nudge as much as a bug
    /// report, and a repo that parenthesizes its arrow parameters has no use
    /// for it at all.
    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Off
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let semantic = file.semantic();
        let shapes = ParamShapes::collect(file.tree());
        let mut violations = Vec::new();

        for target in assignment_targets(file.tree()) {
            // Resolution is by binding, not by lexical nesting, so a mutation
            // several arrows deep still attributes to the parameter it really
            // refers to: `arr.map(item => other.forEach(x => { item.y = 1 }))`.
            let Some((_, name)) = eligible_member_target(&target, semantic, &shapes, |shape| {
                shape == ParamShape::BareArrow
            }) else {
                continue;
            };

            let (line, col) = file.line_col(report_offset(&target));
            violations.push(Violation::error(
                self.name(),
                line,
                col,
                format!(
                    "Mutating a property of parameter \"{name}\" is invisible to Biome's \
                     noParameterAssign because this arrow's single parameter has no parens. \
                     Add parens or copy the value first."
                ),
            ));
        }

        violations
    }
}
