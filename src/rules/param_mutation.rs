//! Shared machinery for the four parameter-mutation rules
//! (`destructure-default-param-assign`, `destructure-param-prop-assign`,
//! `bare-arrow-param-prop-assign`, `deep-param-prop-assign`).
//!
//! All four answer variations of one question — "is this assignment target
//! rooted in a function parameter, and what shape was that parameter declared
//! in?" — so the AST walking lives here once rather than in four
//! slightly-different copies. Each rule file is then just an eligibility
//! predicate plus a message.
//!
//! Two pieces:
//!
//! - [`assignment_targets`] enumerates every assignment target in a file,
//!   whatever syntax put it there (`=`, `+=`, `++`, `for (x of ...)`,
//!   destructuring assignment).
//! - [`ParamShapes`] classifies each parameter binding in the file as
//!   destructured, plain-parenthesized, or a bare single-arrow parameter —
//!   the distinction the four rules split on. Built once per file, because
//!   classifying on demand would re-walk the tree per violation.

use std::collections::HashMap;

use biome_js_syntax::{
    AnyJsAssignment, AnyJsExpression, JsIdentifierAssignment, JsReferenceIdentifier, JsSyntaxKind,
    JsSyntaxNode,
};
use biome_rowan::AstNode;

use crate::semantic::{Binding, BindingKind, SemanticModel};

/// How a parameter binding was declared. The three cases the rules split on;
/// see each rule's own docs for which shapes it considers eligible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamShape {
    /// Reached through an object or array binding pattern in the parameter
    /// list: `function f({ a })`, `({ outer: { inner } }) => ...`, `f([first])`.
    Destructured,
    /// A plain identifier parameter inside a parenthesized parameter list:
    /// `function f(a)`, `(a) => ...`, `(a, b) => ...`.
    Plain,
    /// A plain identifier parameter that is an arrow function's sole,
    /// *unparenthesized* parameter: `a => ...`. Structurally distinct because
    /// it binds directly under the arrow with no `JsParameters` node in
    /// between — which is exactly why Biome's own `noParameterAssign` misses
    /// property mutations on it.
    ///
    /// Never destructured: `{ a } => ...` is a syntax error in JS, so a bare
    /// arrow parameter is always a plain identifier.
    BareArrow,
}

impl ParamShape {
    /// Whether this shape is one Biome's `noParameterAssign` can see as a
    /// parameter at all, i.e. anything but [`Self::Destructured`].
    pub fn is_plain(self) -> bool {
        !matches!(self, Self::Destructured)
    }
}

/// Every parameter binding in one file, keyed by the byte offset its
/// identifier was declared at — the same offset [`Binding::declared_at`]
/// carries, which is what lets a resolved binding be looked up here.
///
/// A [`Binding`] records only *where* it was declared, not what syntax
/// declared it, so the shape has to be recovered from the tree. Doing that
/// once per file keeps every rule's own check an O(1) lookup.
#[derive(Debug, Default)]
pub struct ParamShapes {
    shapes: HashMap<usize, ParamShape>,
}

impl ParamShapes {
    pub fn collect(tree: &JsSyntaxNode) -> Self {
        let mut shapes = HashMap::new();
        for node in tree.descendants() {
            if node.kind() != JsSyntaxKind::JS_IDENTIFIER_BINDING {
                continue;
            }
            if let Some(shape) = classify_binding_declaration(&node) {
                shapes.insert(usize::from(node.text_trimmed_range().start()), shape);
            }
        }
        Self { shapes }
    }

    /// The shape of `binding`'s declaration, or `None` if it is not a
    /// parameter at all (a `const`, an import, a function's own name, ...).
    ///
    /// Checks [`BindingKind::Parameter`] as well as the offset lookup: the two
    /// agree today, and requiring both means a future semantic-model change to
    /// either one can't silently widen what these rules flag.
    pub fn shape_of(&self, binding: &Binding) -> Option<ParamShape> {
        if binding.kind != BindingKind::Parameter {
            return None;
        }
        self.shapes.get(&binding.declared_at).copied()
    }
}

/// Walks up from a `JsIdentifierBinding` to work out whether it is a
/// parameter and, if so, in what shape.
///
/// The walk stops at the first ancestor that settles the question:
/// a formal/rest parameter means "parameter, in a parenthesized list", an
/// arrow function reached directly means "bare arrow parameter", and anything
/// else that can declare a name (`const`, `catch`, a function's own id) means
/// "not a parameter". Binding-pattern nodes in between are passed through,
/// flipping the answer to [`ParamShape::Destructured`] on the way — which is
/// why nesting depth (`{ outer: { inner } }`, `{ a: [first] }`) doesn't matter.
fn classify_binding_declaration(binding: &JsSyntaxNode) -> Option<ParamShape> {
    let mut destructured = false;

    for ancestor in binding.ancestors().skip(1) {
        match ancestor.kind() {
            JsSyntaxKind::JS_OBJECT_BINDING_PATTERN | JsSyntaxKind::JS_ARRAY_BINDING_PATTERN => {
                destructured = true;
            }

            // Structure inside a binding pattern: keep walking, since the
            // pattern node itself (handled above) is what marks destructuring.
            JsSyntaxKind::JS_OBJECT_BINDING_PATTERN_PROPERTY_LIST
            | JsSyntaxKind::JS_OBJECT_BINDING_PATTERN_PROPERTY
            | JsSyntaxKind::JS_OBJECT_BINDING_PATTERN_SHORTHAND_PROPERTY
            | JsSyntaxKind::JS_OBJECT_BINDING_PATTERN_REST
            | JsSyntaxKind::JS_ARRAY_BINDING_PATTERN_ELEMENT_LIST
            | JsSyntaxKind::JS_ARRAY_BINDING_PATTERN_ELEMENT
            | JsSyntaxKind::JS_ARRAY_BINDING_PATTERN_REST_ELEMENT => {}

            // A parameter in a parenthesized list — `function f(a)`, `(a) =>`,
            // `(...rest) =>`. Also the boundary that keeps a nested arrow's own
            // parameter from being misattributed to the outer parameter it is
            // defaulted inside (`function f(a = (b) => ...)`): `b`'s walk stops
            // at `b`'s own formal parameter, never reaching `a`'s.
            JsSyntaxKind::JS_FORMAL_PARAMETER | JsSyntaxKind::JS_REST_PARAMETER => {
                return Some(if destructured {
                    ParamShape::Destructured
                } else {
                    ParamShape::Plain
                });
            }

            // Reaching the arrow itself without passing a formal parameter
            // means this is the unparenthesized single-parameter form.
            JsSyntaxKind::JS_ARROW_FUNCTION_EXPRESSION => return Some(ParamShape::BareArrow),

            // Any other declaring construct: not a parameter.
            _ => return None,
        }
    }

    None
}

/// Every assignment target in the file, in source order.
///
/// Collected by node kind rather than by visiting each *containing* syntax
/// (assignment expression, update expression, `for-of` head, destructuring
/// assignment pattern) because Biome's `JsIdentifierAssignment` /
/// `Js*MemberAssignment` nodes appear in assignment-target position and
/// nowhere else. Matching on them directly therefore covers every form that
/// writes to something — including `x += 1`, `x++`, `for (x of list)`, and
/// `[x] = pair` — with no per-form case to keep in sync.
pub fn assignment_targets(tree: &JsSyntaxNode) -> impl Iterator<Item = AnyJsAssignment> + '_ {
    tree.descendants().filter_map(|node| match node.kind() {
        JsSyntaxKind::JS_IDENTIFIER_ASSIGNMENT
        | JsSyntaxKind::JS_STATIC_MEMBER_ASSIGNMENT
        | JsSyntaxKind::JS_COMPUTED_MEMBER_ASSIGNMENT => AnyJsAssignment::cast(node),
        _ => None,
    })
}

/// A member-expression assignment target broken down into the parts the rules
/// report on: `state.tours[id].priceBands = {}` is `root` = `state`,
/// `depth` = 3.
pub struct MemberTarget {
    /// The leftmost identifier the chain is rooted in.
    pub root: JsReferenceIdentifier,
    /// How many `.prop` / `[expr]` hops separate `root` from the value being
    /// written. `c.token = 'x'` is 1 — the depth Biome's own rule covers;
    /// anything higher is the blind spot `deep-param-prop-assign` closes.
    pub depth: usize,
}

impl MemberTarget {
    /// Decomposes `target` if it is a member/index write, or returns `None`
    /// for a bare-identifier write (`x = 1`), a TypeScript-only wrapper, or a
    /// chain rooted in something other than a plain identifier
    /// (`foo().bar = 1`, `this.x = 1`).
    pub fn parse(target: &AnyJsAssignment) -> Option<Self> {
        let mut object = match target {
            AnyJsAssignment::JsStaticMemberAssignment(member) => member.object().ok()?,
            AnyJsAssignment::JsComputedMemberAssignment(member) => member.object().ok()?,
            _ => return None,
        };
        let mut depth = 1;

        // Walk the leftmost chain down to the identifier it bottoms out in.
        // Parenthesized links (`(a.b).c = 1`) are transparent and don't count
        // as a hop.
        loop {
            object = match object {
                AnyJsExpression::JsIdentifierExpression(ident) => {
                    return Some(Self {
                        root: ident.name().ok()?,
                        depth,
                    });
                }
                AnyJsExpression::JsStaticMemberExpression(member) => {
                    depth += 1;
                    member.object().ok()?
                }
                AnyJsExpression::JsComputedMemberExpression(member) => {
                    depth += 1;
                    member.object().ok()?
                }
                AnyJsExpression::JsParenthesizedExpression(paren) => paren.expression().ok()?,
                _ => return None,
            };
        }
    }
}

/// The bare-identifier form of an assignment target.
///
/// No parenthesis handling needed: `(x) = 1` wraps the identifier in a
/// `JsParenthesizedAssignment`, but the `JsIdentifierAssignment` inside it is
/// itself a descendant, so [`assignment_targets`] yields it directly.
pub fn identifier_target(target: &AnyJsAssignment) -> Option<JsIdentifierAssignment> {
    match target {
        AnyJsAssignment::JsIdentifierAssignment(ident) => Some(ident.clone()),
        _ => None,
    }
}

/// The parameter a member-chain write is rooted in, if that root resolves to a
/// parameter binding whose declared shape `eligible` accepts.
///
/// Resolution goes through the semantic model rather than matching names, so a
/// local variable that shadows a parameter of the same name is correctly not
/// reported — see `docs/SEMANTIC_MODEL.md`.
pub fn eligible_member_target(
    target: &AnyJsAssignment,
    semantic: &SemanticModel,
    shapes: &ParamShapes,
    eligible: impl Fn(ParamShape) -> bool,
) -> Option<(MemberTarget, String)> {
    let member = MemberTarget::parse(target)?;
    let binding = semantic.resolve(&member.root)?;
    let shape = shapes.shape_of(binding)?;
    eligible(shape).then(|| (member, binding.name.clone()))
}

/// The byte offset a violation on `target` is reported at: its root identifier
/// for a member chain, so the diagnostic points at the parameter name, matching
/// where Biome's own `noParameterAssign` anchors its message.
///
/// The root is asked for explicitly rather than taken as the target's own start,
/// because those differ when the chain is parenthesized: `(state.child).value`
/// starts at `(`, and anchoring there would point the diagnostic at punctuation
/// instead of the parameter this rule is talking about.
pub fn report_offset(target: &AnyJsAssignment) -> usize {
    MemberTarget::parse(target).map_or_else(
        || usize::from(target.syntax().text_trimmed_range().start()),
        |member| usize::from(member.root.syntax().text_trimmed_range().start()),
    )
}
