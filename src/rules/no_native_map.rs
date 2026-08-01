use biome_js_syntax::{
    AnyJsBinding, AnyJsBindingPattern, AnyJsCallArgument, AnyJsExpression, AnyJsImportClause,
    AnyJsNamedImportSpecifier, AnyJsObjectBindingPatternMember, JsCallExpression,
    JsIdentifierBinding, JsImport, JsInitializerClause, JsName, JsObjectBindingPattern,
    JsReferenceIdentifier, JsSyntaxKind, JsSyntaxNode, JsVariableDeclarator,
};
use biome_rowan::{AstNode, AstSeparatedList};

use crate::analyzer::runner::FileContext;
use crate::diagnostics::Violation;
use crate::rules::rule::Rule;
use crate::rules::{JS_EXTENSIONS, JS_PATTERN};

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

    fn default_pattern(&self) -> &'static str {
        JS_PATTERN
    }

    fn check(&self, file: &FileContext) -> Vec<Violation> {
        let mut state = ImmutableBindings::default();
        let mut violations = Vec::new();

        // `descendants()` is preorder, so a declaration is visited before the
        // identifiers inside it. That is the same order the ESLint rule's
        // visitors fire in, which is what makes `const { Map } = Immutable`
        // register as an Immutable binding before its own `Map` is examined.
        for node in file.tree().descendants() {
            if let Some(import) = JsImport::cast_ref(&node) {
                state.observe_import(&import);
                continue;
            }
            if let Some(call) = JsCallExpression::cast_ref(&node) {
                state.observe_require(&call);
                continue;
            }
            if let Some(declarator) = JsVariableDeclarator::cast_ref(&node) {
                state.observe_declarator(&declarator);
                continue;
            }
            if state.has_map_binding {
                continue;
            }
            if let Some(offset) = native_map_reference(&node) {
                let (line, col) = file.line_col(offset);
                violations.push(Violation::error(self.name(), line, col, MESSAGE));
            }
        }

        violations
    }
}

/// Tracks how `immutable` was brought into the current file.
#[derive(Default)]
struct ImmutableBindings {
    /// Immutable's `Map` is in scope, so bare `Map` is not the native one.
    has_map_binding: bool,
    /// Local name bound to the whole `immutable` namespace.
    alias: Option<String>,
}

impl ImmutableBindings {
    /// `import { Map } from 'immutable'` / `import Immutable from 'immutable'`.
    fn observe_import(&mut self, import: &JsImport) {
        let Ok(clause) = import.import_clause() else {
            return;
        };
        let Ok(source) = clause.source() else {
            return;
        };
        let Ok(module) = source.inner_string_text() else {
            return;
        };
        if module.text() != IMMUTABLE_MODULE {
            return;
        }

        for descendant in import.syntax().descendants() {
            let Some(specifier) = AnyJsNamedImportSpecifier::cast_ref(&descendant) else {
                continue;
            };
            if specifier
                .imported_name()
                .is_some_and(|token| token.text_trimmed() == MAP)
            {
                self.has_map_binding = true;
            }
        }

        if let Some(name) = default_import_name(&clause) {
            self.alias = Some(name);
        }
    }

    /// `const Immutable = require('immutable')`.
    fn observe_require(&mut self, call: &JsCallExpression) {
        if !is_callee_named(call, "require") {
            return;
        }
        let Ok(arguments) = call.arguments() else {
            return;
        };
        let Some(AnyJsCallArgument::AnyJsExpression(first)) = arguments.args().iter().flatten().next()
        else {
            return;
        };
        if string_literal_text(&first).as_deref() != Some(IMMUTABLE_MODULE) {
            return;
        }

        let declarator = call
            .syntax()
            .parent()
            .and_then(JsInitializerClause::cast)
            .and_then(|clause| clause.syntax().parent())
            .and_then(JsVariableDeclarator::cast);
        let Some(declarator) = declarator else {
            return;
        };
        if let Some(name) = declarator_name(&declarator) {
            self.alias = Some(name);
        }
    }

    /// `const ImmutableMap = Immutable.Map` / `const { Map } = Immutable`.
    fn observe_declarator(&mut self, declarator: &JsVariableDeclarator) {
        let Some(alias) = self.alias.clone() else {
            return;
        };
        let Some(init) = declarator.initializer() else {
            return;
        };
        let Ok(init) = init.expression() else {
            return;
        };

        if let AnyJsExpression::JsStaticMemberExpression(member) = &init {
            let object_is_alias = member
                .object()
                .ok()
                .and_then(|object| identifier_name(&object))
                .is_some_and(|name| name == alias);
            let member_is_map = member
                .member()
                .ok()
                .and_then(|name| name.as_js_name().cloned())
                .and_then(|name| name.value_token().ok())
                .is_some_and(|token| token.text_trimmed() == MAP);
            if object_is_alias && member_is_map {
                self.has_map_binding = true;
            }
        }

        if identifier_name(&init).as_deref() == Some(alias.as_str()) {
            if let Ok(AnyJsBindingPattern::JsObjectBindingPattern(pattern)) = declarator.id() {
                if object_pattern_has_key(&pattern, MAP) {
                    self.has_map_binding = true;
                }
            }
        }
    }
}

/// An occurrence of the identifier `Map`, wherever it can appear as a name:
/// a reference (`new Map()`), a binding (`const Map = …`), or a member name
/// (`Immutable.Map`). Names inside named import specifiers are skipped, matching
/// the ESLint rule's `ImportSpecifier` exemption.
fn native_map_reference(node: &JsSyntaxNode) -> Option<usize> {
    let is_map = match node.kind() {
        JsSyntaxKind::JS_REFERENCE_IDENTIFIER => JsReferenceIdentifier::cast_ref(node)?
            .value_token()
            .ok()?
            .text_trimmed()
            == MAP,
        JsSyntaxKind::JS_IDENTIFIER_BINDING => JsIdentifierBinding::cast_ref(node)?
            .name_token()
            .ok()?
            .text_trimmed()
            == MAP,
        JsSyntaxKind::JS_NAME => JsName::cast_ref(node)?.value_token().ok()?.text_trimmed() == MAP,
        _ => false,
    };

    if !is_map || is_in_named_import_specifier(node) {
        return None;
    }
    Some(usize::from(node.text_trimmed_range().start()))
}

fn is_in_named_import_specifier(node: &JsSyntaxNode) -> bool {
    node.parent().is_some_and(|parent| {
        matches!(
            parent.kind(),
            JsSyntaxKind::JS_SHORTHAND_NAMED_IMPORT_SPECIFIER
                | JsSyntaxKind::JS_NAMED_IMPORT_SPECIFIER
        )
    })
}

fn default_import_name(clause: &AnyJsImportClause) -> Option<String> {
    let specifier = match clause {
        AnyJsImportClause::JsImportDefaultClause(clause) => clause.default_specifier().ok()?,
        AnyJsImportClause::JsImportCombinedClause(clause) => clause.default_specifier().ok()?,
        _ => return None,
    };
    binding_name(&specifier.local_name().ok()?)
}

fn binding_name(binding: &AnyJsBinding) -> Option<String> {
    Some(
        binding
            .as_js_identifier_binding()?
            .name_token()
            .ok()?
            .text_trimmed()
            .to_string(),
    )
}

fn declarator_name(declarator: &JsVariableDeclarator) -> Option<String> {
    match declarator.id().ok()? {
        AnyJsBindingPattern::AnyJsBinding(binding) => binding_name(&binding),
        _ => None,
    }
}

fn identifier_name(expr: &AnyJsExpression) -> Option<String> {
    let AnyJsExpression::JsIdentifierExpression(ident) = expr else {
        return None;
    };
    Some(ident.name().ok()?.value_token().ok()?.text_trimmed().to_string())
}

fn string_literal_text(expr: &AnyJsExpression) -> Option<String> {
    let AnyJsExpression::AnyJsLiteralExpression(literal) = expr else {
        return None;
    };
    Some(
        literal
            .as_js_string_literal_expression()?
            .inner_string_text()
            .ok()?
            .text()
            .to_string(),
    )
}

fn is_callee_named(call: &JsCallExpression, name: &str) -> bool {
    call.callee()
        .ok()
        .and_then(|callee| identifier_name(&callee))
        .is_some_and(|callee| callee == name)
}

fn object_pattern_has_key(pattern: &JsObjectBindingPattern, key: &str) -> bool {
    pattern
        .properties()
        .iter()
        .flatten()
        .any(|member| object_pattern_key(&member).as_deref() == Some(key))
}

fn object_pattern_key(member: &AnyJsObjectBindingPatternMember) -> Option<String> {
    match member {
        AnyJsObjectBindingPatternMember::JsObjectBindingPatternShorthandProperty(property) => {
            binding_name(&property.identifier().ok()?)
        }
        AnyJsObjectBindingPatternMember::JsObjectBindingPatternProperty(property) => Some(
            property
                .member()
                .ok()?
                .as_js_literal_member_name()?
                .value()
                .ok()?
                .text_trimmed()
                .to_string(),
        ),
        _ => None,
    }
}
