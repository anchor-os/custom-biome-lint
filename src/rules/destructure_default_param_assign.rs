//! Reassignment of a destructured parameter binding.
//!
//! The destructuring half of Biome's `noParameterAssign`, which only tracks
//! plain identifier parameters: `function f(a) { a = 5 }` is flagged by Biome,
//! `function f({ a }) { a = 5 }` is not (verified against Biome 2.5.8). That
//! is a categorical gap, not a configuration one — `noParameterAssign` has no
//! option extending it to destructuring.
//!
//! Property mutation of a destructured parameter is the sibling rule,
//! [`super::destructure_param_prop_assign`]; the two split purely on the
//! assignment target's shape (bare identifier here, member chain there) so
//! neither can double-report the same line.

use biome_rowan::AstNode;

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::rules::param_mutation::{
    assignment_targets, identifier_target, ParamShape, ParamShapes,
};
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;

pub struct DestructureDefaultParamAssign;

impl Rule for DestructureDefaultParamAssign {
    fn name(&self) -> &'static str {
        "destructure-default-param-assign"
    }

    fn description(&self) -> &'static str {
        "Disallow reassigning a parameter binding introduced by destructuring"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let semantic = file.semantic();
        let shapes = ParamShapes::collect(file.tree());
        let mut violations = Vec::new();

        for target in assignment_targets(file.tree()) {
            let Some(identifier) = identifier_target(&target) else {
                continue;
            };
            let Some(binding) = semantic.resolve_assignment(&identifier) else {
                continue;
            };
            if shapes.shape_of(binding) != Some(ParamShape::Destructured) {
                continue;
            }

            let offset = usize::from(identifier.syntax().text_trimmed_range().start());
            let (line, col) = file.line_col(offset);
            violations.push(Violation::error(
                self.name(),
                line,
                col,
                format!(
                    "Reassigning destructured parameter \"{}\" mutates a local binding a caller \
                     can't see change. Use a new local variable instead.",
                    binding.name
                ),
            ));
        }

        violations
    }
}
