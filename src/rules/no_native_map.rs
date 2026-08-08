use std::collections::HashSet;

use biome_js_syntax::{
    AnyJsBindingPattern, AnyJsCallArgument, AnyJsExpression, JsName, JsObjectBindingPattern,
    JsReferenceIdentifier, JsStaticMemberExpression, JsSyntaxKind, JsSyntaxNode,
    JsVariableDeclarator,
};
use biome_rowan::{AstNode, AstSeparatedList};

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::rules::rule::Rule;
use crate::rules::JS_EXTENSIONS;
use crate::semantic::{Binding, ImportedName, SemanticModel};

const IMMUTABLE_MODULE: &str = "immutable";
const MAP: &str = "Map";
const MESSAGE: &str = "Use Immutable.js Map instead of native Map.";

pub struct NoNativeMap;

impl Rule for NoNativeMap {
    fn name(&self) -> &'static str {
        "no-native-map"
    }

    fn description(&self) -> &'static str {
        "Disallow the use of native Map in favour of Immutable.js Map"
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        JS_EXTENSIONS
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let semantic = file.semantic();
        let aliases = ImmutableAliases::collect(file, semantic);
        let mut violations = Vec::new();

        for node in file.tree().descendants() {
            let Some(offset) = suspect_map_occurrence(&node, semantic, &aliases) else {
                continue;
            };
            let (line, col) = file.line_col(offset);
            violations.push(Violation::error(self.name(), line, col, MESSAGE));
        }

        violations
    }
}

/// Byte offsets of local bindings (destructured or CommonJS-derived) that
/// stand in for pieces of the `immutable` module, resolved once per file so
/// every occurrence check below is a cheap set lookup rather than its own
/// AST walk. Two separate sets because a binding can represent the whole
/// module (`Immutable` in `Immutable.Map`) without itself being Immutable's
/// `Map` (`ImmutableMap` in `const ImmutableMap = Immutable.Map`), or vice
/// versa (`Map` in `const { Map } = Immutable`).
struct ImmutableAliases {
    /// A binding whose value is the `immutable` module itself -- from a
    /// default/namespace import, or `require('immutable')`.
    module: HashSet<usize>,
    /// A binding whose value is `immutable`'s `Map` export specifically --
    /// from a named import (including aliased), or derived from a `module`
    /// binding via destructuring (`const { Map } = Immutable`) or member
    /// access (`const M = Immutable.Map`).
    map: HashSet<usize>,
}

impl ImmutableAliases {
    /// A single forward pass mirrors `descendants()`'s preorder guarantee
    /// the same way the rule's previous ad hoc scan relied on it: a
    /// `require('immutable')` binding (or an import) is always visited
    /// before any declarator that destructures or member-accesses it, so
    /// resolving the declarator's initializer here always sees a complete
    /// `module`/`map` set for anything declared earlier in the file.
    /// Forward references to a not-yet-classified alias are the one case
    /// this cannot see -- consistent with the model's own single-pass
    /// resolution (see `semantic/builder.rs`), and not a pattern any
    /// existing fixture or the migration brief exercises.
    fn collect(file: &FileContext, semantic: &SemanticModel) -> Self {
        let mut aliases = Self {
            module: HashSet::new(),
            map: HashSet::new(),
        };

        for node in file.tree().descendants() {
            let Some(declarator) = JsVariableDeclarator::cast_ref(&node) else {
                continue;
            };
            aliases.observe_declarator(semantic, &declarator);
        }

        aliases
    }

    fn observe_declarator(&mut self, semantic: &SemanticModel, declarator: &JsVariableDeclarator) {
        let Some(initializer) = declarator.initializer() else {
            return;
        };
        let Ok(expression) = initializer.expression() else {
            return;
        };

        // `const Immutable = require('immutable')` -- the semantic model
        // only tracks ES `import`s, so a CommonJS `require` alias is
        // classified here directly rather than by resolving anything.
        if is_require_immutable_call(&expression) {
            if let Some(offset) = declared_identifier_offset(declarator) {
                self.module.insert(offset);
            }
            return;
        }

        // `const { Map } = Immutable` / `const { Map } = require('immutable')`.
        if let AnyJsExpression::JsIdentifierExpression(ident) = &expression {
            if let Ok(reference) = ident.name() {
                if self.resolves_to_module(semantic, &reference) {
                    if let Ok(AnyJsBindingPattern::JsObjectBindingPattern(pattern)) =
                        declarator.id()
                    {
                        if let Some(offset) = object_pattern_map_offset(&pattern) {
                            self.map.insert(offset);
                        }
                    }
                }
            }
        }

        // `const M = Immutable.Map` / `const M = require('immutable').Map`.
        if let AnyJsExpression::JsStaticMemberExpression(member) = &expression {
            if self.member_is_immutable_map(semantic, member) {
                if let Some(offset) = declared_identifier_offset(declarator) {
                    self.map.insert(offset);
                }
            }
        }
    }

    /// Whether `reference` resolves to a binding that itself represents the
    /// whole `immutable` module (a default/namespace import, or a
    /// previously classified `require('immutable')` alias).
    fn resolves_to_module(
        &self,
        semantic: &SemanticModel,
        reference: &JsReferenceIdentifier,
    ) -> bool {
        let Some(binding) = semantic.resolve(reference) else {
            return false;
        };
        self.binding_is_module(binding)
    }

    fn binding_is_module(&self, binding: &Binding) -> bool {
        if self.module.contains(&binding.declared_at) {
            return true;
        }
        matches!(
            binding.import(),
            Some(import)
                if import.source == IMMUTABLE_MODULE
                    && matches!(import.imported, ImportedName::Default | ImportedName::Namespace)
        )
    }

    fn member_is_immutable_map(
        &self,
        semantic: &SemanticModel,
        member: &JsStaticMemberExpression,
    ) -> bool {
        let is_map_member = member
            .member()
            .ok()
            .and_then(|m| m.as_js_name().cloned())
            .and_then(|name| name.value_token().ok())
            .is_some_and(|token| token.text_trimmed() == MAP);
        if !is_map_member {
            return false;
        }
        let Ok(AnyJsExpression::JsIdentifierExpression(ident)) = member.object() else {
            return false;
        };
        let Ok(reference) = ident.name() else {
            return false;
        };
        self.resolves_to_module(semantic, &reference)
    }

    /// Whether a "Map" reference resolves to something this file has
    /// established stands in for `immutable`'s `Map`.
    fn resolves_to_map(&self, semantic: &SemanticModel, reference: &JsReferenceIdentifier) -> bool {
        let Some(binding) = semantic.resolve(reference) else {
            return false;
        };
        if self.map.contains(&binding.declared_at) {
            return true;
        }
        matches!(
            binding.import(),
            Some(import)
                if import.source == IMMUTABLE_MODULE
                    && matches!(&import.imported, ImportedName::Named(name) if name == MAP)
        )
    }
}

/// True for `require('immutable')`, matching the ESLint-parity CommonJS form
/// this rule has always supported.
fn is_require_immutable_call(expression: &AnyJsExpression) -> bool {
    let AnyJsExpression::JsCallExpression(call) = expression else {
        return false;
    };
    let is_require = matches!(
        call.callee(),
        Ok(AnyJsExpression::JsIdentifierExpression(ident))
            if ident.name().and_then(|n| n.value_token()).is_ok_and(|t| t.text_trimmed() == "require")
    );
    if !is_require {
        return false;
    }
    let Ok(arguments) = call.arguments() else {
        return false;
    };
    let Some(AnyJsCallArgument::AnyJsExpression(AnyJsExpression::AnyJsLiteralExpression(literal))) =
        arguments.args().iter().flatten().next()
    else {
        return false;
    };
    literal
        .as_js_string_literal_expression()
        .and_then(|s| s.inner_string_text().ok())
        .is_some_and(|text| text.text() == IMMUTABLE_MODULE)
}

/// The byte offset of the single identifier a declarator binds, for the
/// plain (non-destructured) case: `const X = ...`.
fn declared_identifier_offset(declarator: &JsVariableDeclarator) -> Option<usize> {
    let AnyJsBindingPattern::AnyJsBinding(binding) = declarator.id().ok()? else {
        return None;
    };
    let ident = binding.as_js_identifier_binding()?;
    Some(usize::from(ident.syntax().text_trimmed_range().start()))
}

/// The byte offset of the `Map` key's own binding in a destructuring
/// pattern, e.g. the `Map` in `const { Map } = Immutable` or
/// `const { Map: LocalMap } = Immutable` (either way, the key itself, not
/// its rename, is what identifies this as Immutable's `Map`).
fn object_pattern_map_offset(pattern: &JsObjectBindingPattern) -> Option<usize> {
    use biome_js_syntax::AnyJsObjectBindingPatternMember as Member;

    for member in pattern.properties().iter().flatten() {
        match member {
            Member::JsObjectBindingPatternShorthandProperty(property) => {
                let binding = property.identifier().ok()?;
                let ident = binding.as_js_identifier_binding()?;
                if ident.name_token().ok()?.text_trimmed() == MAP {
                    return Some(usize::from(ident.syntax().text_trimmed_range().start()));
                }
            }
            Member::JsObjectBindingPatternProperty(property) => {
                let key_is_map = property
                    .member()
                    .ok()
                    .and_then(|m| m.as_js_literal_member_name().cloned())
                    .and_then(|name| name.value().ok())
                    .is_some_and(|token| token.text_trimmed() == MAP);
                if key_is_map {
                    if let Ok(AnyJsBindingPattern::AnyJsBinding(binding)) = property.pattern() {
                        let ident = binding.as_js_identifier_binding()?;
                        return Some(usize::from(ident.syntax().text_trimmed_range().start()));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// A "Map" occurrence that is not resolvable to `immutable`'s `Map`, if
/// `node` is one. Only reference and member-name positions are checked --
/// binding declarations (`function f(Map)`, `const Map = ...`) are never
/// flagged on their own, since declaring a local named `Map` isn't itself a
/// use of the value; what matters is whether later *uses* of that binding
/// turn out to mean Immutable's `Map` or not, which lexical resolution
/// already answers correctly, including under shadowing.
fn suspect_map_occurrence(
    node: &JsSyntaxNode,
    semantic: &SemanticModel,
    aliases: &ImmutableAliases,
) -> Option<usize> {
    match node.kind() {
        JsSyntaxKind::JS_REFERENCE_IDENTIFIER => {
            let reference = JsReferenceIdentifier::cast_ref(node)?;
            if reference.value_token().ok()?.text_trimmed() != MAP {
                return None;
            }
            if aliases.resolves_to_map(semantic, &reference) {
                return None;
            }
            Some(usize::from(reference.syntax().text_trimmed_range().start()))
        }
        JsSyntaxKind::JS_NAME => {
            let name = JsName::cast_ref(node)?;
            if name.value_token().ok()?.text_trimmed() != MAP {
                return None;
            }
            // A member name's only well-defined "does this mean Immutable"
            // question is when it's the right-hand side of `object.Map`;
            // any other use of the bare word `Map` as a name (an object
            // literal key, a method name, ...) keeps the original
            // ESLint-parity behaviour of always flagging it -- see
            // docs/RULES.md's mapboxgl.Map false-positive note.
            if let Some(member) = node.parent().and_then(JsStaticMemberExpression::cast) {
                if aliases.member_is_immutable_map(semantic, &member) {
                    return None;
                }
            }
            Some(usize::from(node.text_trimmed_range().start()))
        }
        _ => None,
    }
}
