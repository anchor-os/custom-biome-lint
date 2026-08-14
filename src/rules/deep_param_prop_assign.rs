//! Property mutation of a plain parameter, two or more levels deep.
//!
//! **Off by default** — see [`Rule::default_severity`]. Biome's
//! `noParameterAssign` tracks exactly one level of property write on a plain
//! parameter; deeper chains are invisible to it. Verified against Biome 2.5.8:
//!
//! ```js
//! function f(acc)   { acc[x] = 1; }                     // flagged — depth 1
//! function f(acc)   { acc[x][y] = 1; }                  // NOT flagged — depth 2
//! function f(accum) { accum.tours[id].priceBands = {}; } // NOT flagged — depth 3
//! ```
//!
//! Depth 1 is deliberately left alone: re-reporting what Biome already
//! reports, under a second rule name, would be pure noise. This is the
//! plain-parameter counterpart to
//! [`super::destructure_param_prop_assign`]'s depth independence — same walk,
//! inverted eligibility (plain here, destructured there), so a destructured
//! root never trips both.
//!
//! Overlap with [`super::bare_arrow_param_prop_assign`] is intentional and
//! unmanaged: a bare-arrow parameter mutated 2+ levels deep trips both rules,
//! because arrow-parens shape and chain depth are independent facts and each
//! is separately worth suppressing. Neither rule knows about the other.

use crate::analyzer::runner::FileContext;
use crate::config::RuleSeverity;
use crate::diagnostics::Violation;
use crate::rules::param_mutation::{
    assignment_targets, eligible_member_target, report_offset, ParamShape, ParamShapes,
};
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;
use biome_rowan::AstNode;

/// Chains shorter than this are `noParameterAssign`'s own territory.
const MIN_DEPTH: usize = 2;

pub struct DeepParamPropAssign;

impl Rule for DeepParamPropAssign {
    fn name(&self) -> &'static str {
        "deep-param-prop-assign"
    }

    fn description(&self) -> &'static str {
        "Disallow mutating a plain parameter's properties two or more levels deep (off by default)"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS
    }

    /// Opt-in: deep chained writes are idiomatic accumulator code in some
    /// codebases, so this ships off and is enabled per repo.
    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Off
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let semantic = file.semantic();
        let shapes = ParamShapes::collect(file.tree());
        let mut violations = Vec::new();

        for target in assignment_targets(file.tree()) {
            let Some((member, name)) =
                eligible_member_target(&target, semantic, &shapes, ParamShape::is_plain)
            else {
                continue;
            };
            if member.depth < MIN_DEPTH {
                continue;
            }

            let chain = target.syntax().text_trimmed().to_string();
            let (line, col) = file.line_col(report_offset(&target));
            violations.push(Violation::error(
                self.name(),
                line,
                col,
                format!(
                    "Mutating parameter \"{name}\" {MIN_DEPTH}+ levels deep (\"{chain}\") is \
                     invisible to Biome's noParameterAssign, which only tracks one level. \
                     Copy the value first."
                ),
            ));
        }

        violations
    }
}
