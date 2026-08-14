//! Property mutation of a destructured parameter, at any depth.
//!
//! The destructuring counterpart to `noParameterAssign`'s
//! `propertyAssignment: "deny"` check, which — like the rule's reassignment
//! half — only sees plain identifier parameters: Biome flags
//! `function f(d) { d.token = 'x' }` but not
//! `function f({ c }) { c.token = 'x' }` (verified against Biome 2.5.8).
//!
//! Unlike Biome's version this is depth-independent, which matters for the
//! saga/reducer code it was written for: `state.tours[id].priceBands = {}` is
//! the same hazard as `state.x = 1` and is reported the same way. See
//! [`super::deep_param_prop_assign`] for the plain-parameter equivalent of
//! that depth argument.

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::rules::param_mutation::{
    assignment_targets, eligible_member_target, report_offset, ParamShape, ParamShapes,
};
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;

pub struct DestructureParamPropAssign;

impl Rule for DestructureParamPropAssign {
    fn name(&self) -> &'static str {
        "destructure-param-prop-assign"
    }

    fn description(&self) -> &'static str {
        "Disallow mutating a property of a destructured parameter"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let semantic = file.semantic();
        let shapes = ParamShapes::collect(file.tree());
        let mut violations = Vec::new();

        for target in assignment_targets(file.tree()) {
            let Some((_, name)) = eligible_member_target(&target, semantic, &shapes, |shape| {
                shape == ParamShape::Destructured
            }) else {
                continue;
            };

            let (line, col) = file.line_col(report_offset(&target));
            violations.push(Violation::error(
                self.name(),
                line,
                col,
                format!(
                    "Mutating a property of destructured parameter \"{name}\" changes data the \
                     caller still holds a reference to. Copy it first."
                ),
            ));
        }

        violations
    }
}
