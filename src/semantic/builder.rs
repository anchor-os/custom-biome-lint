//! Builds a [`SemanticModel`] with a single recursive walk over the syntax
//! tree, dispatching by [`JsSyntaxKind`] rather than casting every node type
//! the tree could contain. Any node kind not explicitly handled below just
//! recurses into its children unchanged (`walk`'s `_` arm) -- this is what
//! lets the walk reach arrow functions or blocks nested arbitrarily deep
//! inside expressions, JSX, or anything else without a case for every
//! possible container.
//!
//! Two passes, not one: while walking, every reference identifier -- and
//! every assignment target, which Biome models as the distinct
//! `JsIdentifierAssignment` node type -- is recorded as
//! `(offset, scope, name)` in `pending_refs` rather than resolved
//! immediately. Resolution happens once, after the whole tree (and
//! so every scope's full set of bindings) is known. Resolving eagerly
//! during the walk would get forward references wrong -- a function that
//! calls another function declared later in the same scope, or a variable
//! referenced before a `var` declared later in the same block -- since the
//! later binding wouldn't exist yet at the point the reference is visited.
//! This intentionally does not model the finer-grained *temporal dead
//! zone* distinction between `var`/function hoisting and `let`/`const`;
//! see docs/SEMANTIC_MODEL.md.

use std::collections::HashMap;

use biome_js_syntax::{
    AnyJsArrayBindingPatternElement, AnyJsArrowFunctionParameters, AnyJsBinding,
    AnyJsBindingPattern, AnyJsCombinedSpecifier, AnyJsConstructorParameter,
    AnyJsForInOrOfInitializer, AnyJsForInitializer, AnyJsFormalParameter, AnyJsFunctionBody,
    AnyJsImportClause, AnyJsNamedImportSpecifier, AnyJsObjectBindingPatternMember, AnyJsParameter,
    JsArrowFunctionExpression, JsCatchClause, JsClassDeclaration, JsConstructorClassMember,
    JsConstructorParameters, JsDefaultImportSpecifier, JsForInStatement, JsForOfStatement,
    JsForStatement, JsForVariableDeclaration, JsFunctionBody, JsFunctionDeclaration,
    JsFunctionExpression, JsGetterClassMember, JsGetterObjectMember, JsIdentifierAssignment,
    JsImport, JsInitializerClause, JsMethodClassMember, JsMethodObjectMember,
    JsNamedImportSpecifiers, JsNamespaceImportSpecifier, JsParameters, JsReferenceIdentifier,
    JsRestParameter, JsSetterClassMember, JsSetterObjectMember, JsSwitchStatement, JsSyntaxKind,
    JsSyntaxNode, JsVariableDeclaration, JsVariableDeclarator,
};
use biome_rowan::{AstNode, AstSeparatedList};

use super::binding::{Binding, BindingId, BindingKind, ImportBinding, ImportedName};
use super::scope::{Scope, ScopeId, ScopeKind};
use super::SemanticModel;

pub(super) fn build(root: &JsSyntaxNode) -> SemanticModel {
    let mut builder = Builder::default();
    let global = builder.push_scope(ScopeKind::Global, None);
    builder.walk(root, global);
    builder.finish(global)
}

/// How a callable class/object member declares its parameters. Four shapes
/// around the same binding logic: a getter has none, a method has a
/// `JsParameters` list, a setter has one unlisted `AnyJsFormalParameter`, and a
/// constructor has its own `JsConstructorParameters`.
enum MemberParams<'a> {
    None,
    List(&'a JsParameters),
    Single(&'a AnyJsFormalParameter),
    Constructor(&'a JsConstructorParameters),
}

#[derive(Default)]
struct Builder {
    scopes: Vec<Scope>,
    bindings: Vec<Binding>,
    pending_refs: Vec<(usize, ScopeId, String)>,
}

impl Builder {
    fn push_scope(&mut self, kind: ScopeKind, parent: Option<ScopeId>) -> ScopeId {
        let id = ScopeId(self.scopes.len());
        self.scopes.push(Scope {
            kind,
            parent,
            bindings: HashMap::new(),
        });
        id
    }

    fn add_binding(&mut self, scope: ScopeId, name: String, kind: BindingKind, declared_at: usize) {
        let id = BindingId(self.bindings.len());
        self.bindings.push(Binding {
            name: name.clone(),
            kind,
            scope,
            declared_at,
        });
        self.scopes[scope.0].bindings.insert(name, id);
    }

    /// `var` hoists to the nearest enclosing function or global scope,
    /// skipping any block/loop/catch scopes in between.
    fn hoist_target(&self, mut scope: ScopeId) -> ScopeId {
        loop {
            match self.scopes[scope.0].kind {
                ScopeKind::Function | ScopeKind::Global => return scope,
                ScopeKind::Block | ScopeKind::Loop | ScopeKind::Catch => {
                    match self.scopes[scope.0].parent {
                        Some(parent) => scope = parent,
                        None => return scope,
                    }
                }
            }
        }
    }

    fn finish(self, global: ScopeId) -> SemanticModel {
        let mut resolutions = HashMap::with_capacity(self.pending_refs.len());
        for (offset, scope, name) in &self.pending_refs {
            let mut current = Some(*scope);
            while let Some(id) = current {
                if let Some(binding_id) = self.scopes[id.0].bindings.get(name) {
                    resolutions.insert(*offset, *binding_id);
                    break;
                }
                current = self.scopes[id.0].parent;
            }
        }
        SemanticModel {
            scopes: self.scopes,
            bindings: self.bindings,
            global,
            resolutions,
        }
    }

    // ---- generic traversal ----

    fn walk(&mut self, node: &JsSyntaxNode, scope: ScopeId) {
        match node.kind() {
            JsSyntaxKind::JS_REFERENCE_IDENTIFIER => {
                if let Some(ident) = JsReferenceIdentifier::cast_ref(node) {
                    self.record_reference(&ident, scope);
                }
            }
            JsSyntaxKind::JS_IDENTIFIER_ASSIGNMENT => {
                // An assignment target (`x = 1`, `x++`, `for (x of ...)`) is a
                // use of an existing binding, not a declaration, but Biome
                // models it as `JsIdentifierAssignment` rather than
                // `JsReferenceIdentifier` -- so it needs recording here to be
                // resolvable at all. See `SemanticModel::resolve_assignment`.
                if let Some(ident) = JsIdentifierAssignment::cast_ref(node) {
                    self.record_assignment_target(&ident, scope);
                }
            }
            JsSyntaxKind::JS_IMPORT => {
                if let Some(import) = JsImport::cast_ref(node) {
                    self.handle_import(&import, scope);
                }
            }
            JsSyntaxKind::JS_VARIABLE_DECLARATION => {
                if let Some(decl) = JsVariableDeclaration::cast_ref(node) {
                    self.handle_variable_declaration(&decl, scope);
                }
            }
            JsSyntaxKind::JS_FUNCTION_DECLARATION => {
                if let Some(decl) = JsFunctionDeclaration::cast_ref(node) {
                    self.handle_function_declaration(&decl, scope);
                }
            }
            JsSyntaxKind::JS_FUNCTION_EXPRESSION => {
                if let Some(expr) = JsFunctionExpression::cast_ref(node) {
                    self.handle_function_expression(&expr, scope);
                }
            }
            JsSyntaxKind::JS_ARROW_FUNCTION_EXPRESSION => {
                if let Some(arrow) = JsArrowFunctionExpression::cast_ref(node) {
                    self.handle_arrow_function(&arrow, scope);
                }
            }
            JsSyntaxKind::JS_CLASS_DECLARATION => {
                if let Some(decl) = JsClassDeclaration::cast_ref(node) {
                    self.handle_class_declaration(&decl, scope);
                }
            }
            // Callable class/object members. Each owns a function scope holding
            // its parameters, exactly like a function expression -- without
            // these arms a method's parameters are never bound at all, and an
            // identifier in its body resolves straight past them to whatever
            // the enclosing scope happens to have (or to nothing).
            JsSyntaxKind::JS_METHOD_CLASS_MEMBER => {
                if let Some(member) = JsMethodClassMember::cast_ref(node) {
                    self.handle_callable_member(
                        member_name(member.name().ok()),
                        member
                            .parameters()
                            .ok()
                            .as_ref()
                            .map_or(MemberParams::None, MemberParams::List),
                        member.body().ok(),
                        scope,
                    );
                }
            }
            JsSyntaxKind::JS_METHOD_OBJECT_MEMBER => {
                if let Some(member) = JsMethodObjectMember::cast_ref(node) {
                    self.handle_callable_member(
                        member_name(member.name().ok()),
                        member
                            .parameters()
                            .ok()
                            .as_ref()
                            .map_or(MemberParams::None, MemberParams::List),
                        member.body().ok(),
                        scope,
                    );
                }
            }
            // A setter takes exactly one parameter and is not wrapped in a
            // `JsParameters` list; a getter takes none. Both still need their
            // own scope so declarations in the body don't leak outward.
            JsSyntaxKind::JS_SETTER_CLASS_MEMBER => {
                if let Some(member) = JsSetterClassMember::cast_ref(node) {
                    self.handle_callable_member(
                        member_name(member.name().ok()),
                        member
                            .parameter()
                            .ok()
                            .as_ref()
                            .map_or(MemberParams::None, MemberParams::Single),
                        member.body().ok(),
                        scope,
                    );
                }
            }
            JsSyntaxKind::JS_SETTER_OBJECT_MEMBER => {
                if let Some(member) = JsSetterObjectMember::cast_ref(node) {
                    self.handle_callable_member(
                        member_name(member.name().ok()),
                        member
                            .parameter()
                            .ok()
                            .as_ref()
                            .map_or(MemberParams::None, MemberParams::Single),
                        member.body().ok(),
                        scope,
                    );
                }
            }
            JsSyntaxKind::JS_GETTER_CLASS_MEMBER => {
                if let Some(member) = JsGetterClassMember::cast_ref(node) {
                    self.handle_callable_member(
                        member_name(member.name().ok()),
                        MemberParams::None,
                        member.body().ok(),
                        scope,
                    );
                }
            }
            JsSyntaxKind::JS_GETTER_OBJECT_MEMBER => {
                if let Some(member) = JsGetterObjectMember::cast_ref(node) {
                    self.handle_callable_member(
                        member_name(member.name().ok()),
                        MemberParams::None,
                        member.body().ok(),
                        scope,
                    );
                }
            }
            // A constructor's parameters live in `JsConstructorParameters`, a
            // different node type from a method's `JsParameters` (it is the one
            // that can hold TS parameter properties), so it needs its own arm
            // rather than sharing the method one.
            JsSyntaxKind::JS_CONSTRUCTOR_CLASS_MEMBER => {
                if let Some(member) = JsConstructorClassMember::cast_ref(node) {
                    self.handle_callable_member(
                        None,
                        member
                            .parameters()
                            .ok()
                            .as_ref()
                            .map_or(MemberParams::None, MemberParams::Constructor),
                        member.body().ok(),
                        scope,
                    );
                }
            }
            JsSyntaxKind::JS_BLOCK_STATEMENT => {
                let block_scope = self.push_scope(ScopeKind::Block, Some(scope));
                self.walk_children(node, block_scope);
            }
            JsSyntaxKind::JS_SWITCH_STATEMENT => {
                if let Some(stmt) = JsSwitchStatement::cast_ref(node) {
                    self.handle_switch_statement(&stmt, scope);
                }
            }
            JsSyntaxKind::JS_CATCH_CLAUSE => {
                if let Some(clause) = JsCatchClause::cast_ref(node) {
                    self.handle_catch_clause(&clause, scope);
                }
            }
            JsSyntaxKind::JS_FOR_STATEMENT => {
                if let Some(stmt) = JsForStatement::cast_ref(node) {
                    self.handle_for_statement(&stmt, scope);
                }
            }
            JsSyntaxKind::JS_FOR_IN_STATEMENT => {
                if let Some(stmt) = JsForInStatement::cast_ref(node) {
                    self.handle_for_in_statement(&stmt, scope);
                }
            }
            JsSyntaxKind::JS_FOR_OF_STATEMENT => {
                if let Some(stmt) = JsForOfStatement::cast_ref(node) {
                    self.handle_for_of_statement(&stmt, scope);
                }
            }
            _ => self.walk_children(node, scope),
        }
    }

    fn walk_children(&mut self, node: &JsSyntaxNode, scope: ScopeId) {
        for child in node.children() {
            self.walk(&child, scope);
        }
    }

    fn record_reference(&mut self, identifier: &JsReferenceIdentifier, scope: ScopeId) {
        let Ok(token) = identifier.value_token() else {
            return;
        };
        let offset = usize::from(identifier.syntax().text_trimmed_range().start());
        self.pending_refs
            .push((offset, scope, token.text_trimmed().to_string()));
    }

    fn record_assignment_target(&mut self, target: &JsIdentifierAssignment, scope: ScopeId) {
        let Ok(token) = target.name_token() else {
            return;
        };
        let offset = usize::from(target.syntax().text_trimmed_range().start());
        self.pending_refs
            .push((offset, scope, token.text_trimmed().to_string()));
    }

    fn bind_identifier(&mut self, binding: &AnyJsBinding, kind: BindingKind, scope: ScopeId) {
        let Some(ident) = binding.as_js_identifier_binding() else {
            return;
        };
        let Ok(token) = ident.name_token() else {
            return;
        };
        let offset = usize::from(ident.syntax().text_trimmed_range().start());
        self.add_binding(scope, token.text_trimmed().to_string(), kind, offset);
    }

    fn walk_default(&mut self, init: Option<JsInitializerClause>, scope: ScopeId) {
        let Some(init) = init else {
            return;
        };
        if let Ok(expr) = init.expression() {
            self.walk(expr.syntax(), scope);
        }
    }

    // ---- destructuring / parameter patterns ----

    /// Recursively collects every identifier bound by `pattern` (handling
    /// arbitrarily nested object/array destructuring, renames, defaults,
    /// and rest elements), adding each as a binding of `kind` in
    /// `binding_scope`. Default-value expressions found along the way are
    /// walked for references in `ref_scope` -- the scope the pattern's
    /// containing statement is textually in, which for a hoisted `var` is
    /// not the same as `binding_scope`.
    fn collect_pattern_bindings(
        &mut self,
        pattern: &AnyJsBindingPattern,
        kind: BindingKind,
        binding_scope: ScopeId,
        ref_scope: ScopeId,
    ) {
        match pattern {
            AnyJsBindingPattern::AnyJsBinding(binding) => {
                self.bind_identifier(binding, kind, binding_scope);
            }
            AnyJsBindingPattern::JsObjectBindingPattern(object) => {
                for member in object.properties().iter().flatten() {
                    match member {
                        AnyJsObjectBindingPatternMember::JsObjectBindingPatternProperty(
                            property,
                        ) => {
                            // A computed key (`{ [key]: value }`) is itself
                            // an expression that can reference an outer
                            // binding; a literal key (`{ foo: value }`) has
                            // no reference to record, so walking it here is
                            // a harmless no-op.
                            if let Ok(member) = property.member() {
                                self.walk(member.syntax(), ref_scope);
                            }
                            if let Ok(nested) = property.pattern() {
                                self.collect_pattern_bindings(
                                    &nested,
                                    kind.clone(),
                                    binding_scope,
                                    ref_scope,
                                );
                            }
                            self.walk_default(property.init(), ref_scope);
                        }
                        AnyJsObjectBindingPatternMember::JsObjectBindingPatternShorthandProperty(
                            property,
                        ) => {
                            if let Ok(binding) = property.identifier() {
                                self.bind_identifier(&binding, kind.clone(), binding_scope);
                            }
                            self.walk_default(property.init(), ref_scope);
                        }
                        AnyJsObjectBindingPatternMember::JsObjectBindingPatternRest(rest) => {
                            if let Ok(binding) = rest.binding() {
                                self.bind_identifier(&binding, kind.clone(), binding_scope);
                            }
                        }
                        AnyJsObjectBindingPatternMember::JsBogusBinding(_) => {}
                    }
                }
            }
            AnyJsBindingPattern::JsArrayBindingPattern(array) => {
                for element in array.elements().iter().flatten() {
                    match element {
                        AnyJsArrayBindingPatternElement::JsArrayBindingPatternElement(element) => {
                            if let Ok(nested) = element.pattern() {
                                self.collect_pattern_bindings(
                                    &nested,
                                    kind.clone(),
                                    binding_scope,
                                    ref_scope,
                                );
                            }
                            self.walk_default(element.init(), ref_scope);
                        }
                        AnyJsArrayBindingPatternElement::JsArrayBindingPatternRestElement(rest) => {
                            if let Ok(nested) = rest.pattern() {
                                self.collect_pattern_bindings(
                                    &nested,
                                    kind.clone(),
                                    binding_scope,
                                    ref_scope,
                                );
                            }
                        }
                        AnyJsArrayBindingPatternElement::JsArrayHole(_) => {}
                    }
                }
            }
        }
    }

    fn bind_parameters(&mut self, params: &JsParameters, scope: ScopeId) {
        for item in params.items().iter().flatten() {
            match item {
                AnyJsParameter::AnyJsFormalParameter(param) => {
                    self.bind_formal_parameter(&param, scope)
                }
                AnyJsParameter::JsRestParameter(param) => self.bind_rest_parameter(&param, scope),
                AnyJsParameter::TsThisParameter(_) => {}
            }
        }
    }

    /// One `a`, `{ a }`, or `a = default` parameter, wherever it appears -- a
    /// parameter list, a setter's single unlisted parameter, or a constructor's
    /// own parameter list, which are three different node types around the same
    /// `AnyJsFormalParameter`.
    fn bind_formal_parameter(&mut self, param: &AnyJsFormalParameter, scope: ScopeId) {
        let AnyJsFormalParameter::JsFormalParameter(param) = param else {
            return;
        };
        if let Ok(pattern) = param.binding() {
            self.collect_pattern_bindings(&pattern, BindingKind::Parameter, scope, scope);
        }
        self.walk_default(param.initializer(), scope);
    }

    fn bind_rest_parameter(&mut self, param: &JsRestParameter, scope: ScopeId) {
        if let Ok(pattern) = param.binding() {
            self.collect_pattern_bindings(&pattern, BindingKind::Parameter, scope, scope);
        }
    }

    // ---- declarations ----

    fn handle_variable_declaration(&mut self, decl: &JsVariableDeclaration, scope: ScopeId) {
        let is_var = decl.kind().is_ok_and(|t| t.text_trimmed() == "var");
        let is_const = decl.kind().is_ok_and(|t| t.text_trimmed() == "const");
        let kind = if is_var {
            BindingKind::Var
        } else if is_const {
            BindingKind::Const
        } else {
            BindingKind::Let
        };
        let binding_scope = if is_var {
            self.hoist_target(scope)
        } else {
            scope
        };
        for declarator in decl.declarators().iter().flatten() {
            self.handle_declarator(&declarator, kind.clone(), binding_scope, scope);
        }
    }

    fn handle_declarator(
        &mut self,
        declarator: &JsVariableDeclarator,
        kind: BindingKind,
        binding_scope: ScopeId,
        ref_scope: ScopeId,
    ) {
        if let Ok(pattern) = declarator.id() {
            self.collect_pattern_bindings(&pattern, kind, binding_scope, ref_scope);
        }
        self.walk_default(declarator.initializer(), ref_scope);
    }

    fn handle_function_declaration(&mut self, decl: &JsFunctionDeclaration, scope: ScopeId) {
        if let Ok(id) = decl.id() {
            self.bind_identifier(&id, BindingKind::Function, scope);
        }
        let function_scope = self.push_scope(ScopeKind::Function, Some(scope));
        if let Ok(params) = decl.parameters() {
            self.bind_parameters(&params, function_scope);
        }
        if let Ok(body) = decl.body() {
            for statement in body.statements() {
                self.walk(statement.syntax(), function_scope);
            }
        }
    }

    fn handle_function_expression(&mut self, expr: &JsFunctionExpression, scope: ScopeId) {
        let function_scope = self.push_scope(ScopeKind::Function, Some(scope));
        // A named function expression's own name is visible only inside its
        // own body, not the enclosing scope -- unlike a declaration.
        if let Some(id) = expr.id() {
            self.bind_identifier(&id, BindingKind::Function, function_scope);
        }
        if let Ok(params) = expr.parameters() {
            self.bind_parameters(&params, function_scope);
        }
        if let Ok(body) = expr.body() {
            for statement in body.statements() {
                self.walk(statement.syntax(), function_scope);
            }
        }
    }

    fn handle_arrow_function(&mut self, arrow: &JsArrowFunctionExpression, scope: ScopeId) {
        let function_scope = self.push_scope(ScopeKind::Function, Some(scope));
        if let Ok(params) = arrow.parameters() {
            match params {
                AnyJsArrowFunctionParameters::AnyJsBinding(binding) => {
                    self.bind_identifier(&binding, BindingKind::Parameter, function_scope);
                }
                AnyJsArrowFunctionParameters::JsParameters(params) => {
                    self.bind_parameters(&params, function_scope);
                }
            }
        }
        if let Ok(body) = arrow.body() {
            match body {
                AnyJsFunctionBody::JsFunctionBody(body) => {
                    for statement in body.statements() {
                        self.walk(statement.syntax(), function_scope);
                    }
                }
                AnyJsFunctionBody::AnyJsExpression(expr) => {
                    self.walk(expr.syntax(), function_scope);
                }
            }
        }
    }

    /// One function scope for a callable class/object member -- method,
    /// constructor, getter or setter -- holding its parameters and body.
    ///
    /// Deliberately does not bind the member's *name* anywhere: a method name
    /// is a property of the class or object, not a binding in any lexical
    /// scope, so there is nothing for an identifier to resolve to. A *computed*
    /// name is different -- see the walk of `name` below.
    fn handle_callable_member(
        &mut self,
        name: Option<JsSyntaxNode>,
        parameters: MemberParams<'_>,
        body: Option<JsFunctionBody>,
        scope: ScopeId,
    ) {
        // A computed name (`{ [key]() {} }`, `class C { get [key]() {} }`) is an
        // expression evaluated in the *enclosing* scope, before the member's own
        // scope exists -- so it is walked here, against `scope`, not
        // `function_scope`. A literal name has nothing to record and walking it
        // is a harmless no-op, the same way `collect_pattern_bindings` treats a
        // destructuring pattern's literal key.
        if let Some(name) = name {
            self.walk(&name, scope);
        }

        let function_scope = self.push_scope(ScopeKind::Function, Some(scope));
        match parameters {
            MemberParams::None => {}
            MemberParams::List(parameters) => self.bind_parameters(parameters, function_scope),
            MemberParams::Single(parameter) => {
                self.bind_formal_parameter(parameter, function_scope)
            }
            MemberParams::Constructor(parameters) => {
                for parameter in parameters.parameters().iter().flatten() {
                    match parameter {
                        AnyJsConstructorParameter::AnyJsFormalParameter(parameter) => {
                            self.bind_formal_parameter(&parameter, function_scope)
                        }
                        AnyJsConstructorParameter::JsRestParameter(parameter) => {
                            self.bind_rest_parameter(&parameter, function_scope)
                        }
                        // `constructor(public x)` is TypeScript-only, and this
                        // tool analyzes .js/.jsx.
                        AnyJsConstructorParameter::TsPropertyParameter(_) => {}
                    }
                }
            }
        }
        if let Some(body) = body {
            for statement in body.statements() {
                self.walk(statement.syntax(), function_scope);
            }
        }
    }

    fn handle_class_declaration(&mut self, decl: &JsClassDeclaration, scope: ScopeId) {
        if let Ok(id) = decl.id() {
            self.bind_identifier(&id, BindingKind::Class, scope);
        }
        // Lightweight: no dedicated class scope or member analysis -- just
        // recurse generically so nested functions/arrows inside methods are
        // still found and scoped correctly. Re-walking the `id` node this
        // way is harmless: JS_IDENTIFIER_BINDING isn't one of `walk`'s
        // special cases, so it's a no-op.
        self.walk_children(decl.syntax(), scope);
    }

    fn handle_catch_clause(&mut self, clause: &JsCatchClause, scope: ScopeId) {
        let catch_scope = self.push_scope(ScopeKind::Catch, Some(scope));
        if let Some(declaration) = clause.declaration() {
            if let Ok(pattern) = declaration.binding() {
                self.collect_pattern_bindings(
                    &pattern,
                    BindingKind::CatchParameter,
                    catch_scope,
                    catch_scope,
                );
            }
        }
        if let Ok(body) = clause.body() {
            for statement in body.statements() {
                self.walk(statement.syntax(), catch_scope);
            }
        }
    }

    /// All of a `switch`'s case clauses share one block scope -- a `let`
    /// declared in one case is visible in the others, per real JS
    /// semantics, since there's no block around each case body. The
    /// discriminant is evaluated before that scope exists, so it's walked
    /// in the outer scope instead.
    fn handle_switch_statement(&mut self, stmt: &JsSwitchStatement, scope: ScopeId) {
        if let Ok(discriminant) = stmt.discriminant() {
            self.walk(discriminant.syntax(), scope);
        }
        let switch_scope = self.push_scope(ScopeKind::Block, Some(scope));
        for case in stmt.cases() {
            self.walk(case.syntax(), switch_scope);
        }
    }

    // ---- loops ----

    fn handle_for_statement(&mut self, stmt: &JsForStatement, scope: ScopeId) {
        let loop_scope = self.push_scope(ScopeKind::Loop, Some(scope));
        match stmt.initializer() {
            Some(AnyJsForInitializer::JsVariableDeclaration(decl)) => {
                self.handle_variable_declaration(&decl, loop_scope);
            }
            Some(AnyJsForInitializer::AnyJsExpression(expr)) => {
                self.walk(expr.syntax(), loop_scope);
            }
            None => {}
        }
        if let Some(test) = stmt.test() {
            self.walk(test.syntax(), loop_scope);
        }
        if let Some(update) = stmt.update() {
            self.walk(update.syntax(), loop_scope);
        }
        if let Ok(body) = stmt.body() {
            self.walk(body.syntax(), loop_scope);
        }
    }

    fn handle_for_in_statement(&mut self, stmt: &JsForInStatement, scope: ScopeId) {
        let loop_scope = self.push_scope(ScopeKind::Loop, Some(scope));
        if let Ok(initializer) = stmt.initializer() {
            self.handle_for_in_or_of_initializer(initializer, loop_scope);
        }
        if let Ok(expr) = stmt.expression() {
            // The iterated expression is evaluated in the outer scope: the
            // loop variable it's about to be assigned into isn't in scope
            // for it.
            self.walk(expr.syntax(), scope);
        }
        if let Ok(body) = stmt.body() {
            self.walk(body.syntax(), loop_scope);
        }
    }

    fn handle_for_of_statement(&mut self, stmt: &JsForOfStatement, scope: ScopeId) {
        let loop_scope = self.push_scope(ScopeKind::Loop, Some(scope));
        if let Ok(initializer) = stmt.initializer() {
            self.handle_for_in_or_of_initializer(initializer, loop_scope);
        }
        if let Ok(expr) = stmt.expression() {
            self.walk(expr.syntax(), scope);
        }
        if let Ok(body) = stmt.body() {
            self.walk(body.syntax(), loop_scope);
        }
    }

    fn handle_for_in_or_of_initializer(
        &mut self,
        initializer: AnyJsForInOrOfInitializer,
        scope: ScopeId,
    ) {
        match initializer {
            AnyJsForInOrOfInitializer::JsForVariableDeclaration(decl) => {
                self.handle_for_variable_declaration(&decl, scope);
            }
            AnyJsForInOrOfInitializer::AnyJsAssignmentPattern(pattern) => {
                // Not a declaration -- an existing binding being assigned
                // into (`for (x in obj)`), so walk it for references rather
                // than treating it as introducing a new one.
                self.walk(pattern.syntax(), scope);
            }
        }
    }

    fn handle_for_variable_declaration(&mut self, decl: &JsForVariableDeclaration, scope: ScopeId) {
        let is_var = decl.kind_token().is_ok_and(|t| t.text_trimmed() == "var");
        let is_const = decl.kind_token().is_ok_and(|t| t.text_trimmed() == "const");
        let kind = if is_var {
            BindingKind::Var
        } else if is_const {
            BindingKind::Const
        } else {
            BindingKind::Let
        };
        let binding_scope = if is_var {
            self.hoist_target(scope)
        } else {
            scope
        };
        if let Ok(declarator) = decl.declarator() {
            self.handle_declarator(&declarator, kind, binding_scope, scope);
        }
    }

    // ---- imports ----

    fn handle_import(&mut self, import: &JsImport, scope: ScopeId) {
        let Ok(source) = import.source_text() else {
            return;
        };
        let source = source.text().to_string();
        let Ok(clause) = import.import_clause() else {
            return;
        };
        match clause {
            AnyJsImportClause::JsImportBareClause(_) => {}
            AnyJsImportClause::JsImportDefaultClause(clause) => {
                if let Ok(specifier) = clause.default_specifier() {
                    self.bind_import_default(&specifier, &source, scope);
                }
            }
            AnyJsImportClause::JsImportNamespaceClause(clause) => {
                if let Ok(specifier) = clause.namespace_specifier() {
                    self.bind_import_namespace(&specifier, &source, scope);
                }
            }
            AnyJsImportClause::JsImportNamedClause(clause) => {
                if let Ok(named) = clause.named_specifiers() {
                    self.bind_named_specifiers(&named, &source, scope);
                }
            }
            AnyJsImportClause::JsImportCombinedClause(clause) => {
                if let Ok(default) = clause.default_specifier() {
                    self.bind_import_default(&default, &source, scope);
                }
                if let Ok(specifier) = clause.specifier() {
                    match specifier {
                        AnyJsCombinedSpecifier::JsNamedImportSpecifiers(named) => {
                            self.bind_named_specifiers(&named, &source, scope);
                        }
                        AnyJsCombinedSpecifier::JsNamespaceImportSpecifier(namespace) => {
                            self.bind_import_namespace(&namespace, &source, scope);
                        }
                    }
                }
            }
        }
    }

    fn bind_import_default(
        &mut self,
        specifier: &JsDefaultImportSpecifier,
        source: &str,
        scope: ScopeId,
    ) {
        let Ok(binding) = specifier.local_name() else {
            return;
        };
        let Some(ident) = binding.as_js_identifier_binding() else {
            return;
        };
        let Ok(token) = ident.name_token() else {
            return;
        };
        let local = token.text_trimmed().to_string();
        let offset = usize::from(ident.syntax().text_trimmed_range().start());
        self.add_binding(
            scope,
            local.clone(),
            BindingKind::Import(ImportBinding {
                source: source.to_string(),
                imported: ImportedName::Default,
                local,
            }),
            offset,
        );
    }

    fn bind_import_namespace(
        &mut self,
        specifier: &JsNamespaceImportSpecifier,
        source: &str,
        scope: ScopeId,
    ) {
        let Ok(binding) = specifier.local_name() else {
            return;
        };
        let Some(ident) = binding.as_js_identifier_binding() else {
            return;
        };
        let Ok(token) = ident.name_token() else {
            return;
        };
        let local = token.text_trimmed().to_string();
        let offset = usize::from(ident.syntax().text_trimmed_range().start());
        self.add_binding(
            scope,
            local.clone(),
            BindingKind::Import(ImportBinding {
                source: source.to_string(),
                imported: ImportedName::Namespace,
                local,
            }),
            offset,
        );
    }

    fn bind_named_specifiers(
        &mut self,
        specifiers: &JsNamedImportSpecifiers,
        source: &str,
        scope: ScopeId,
    ) {
        for specifier in specifiers.specifiers().iter().flatten() {
            let (local_binding, imported_name) = match &specifier {
                AnyJsNamedImportSpecifier::JsNamedImportSpecifier(named) => {
                    let Ok(local) = named.local_name() else {
                        continue;
                    };
                    let imported = named
                        .name()
                        .ok()
                        .and_then(|export_name| export_name.value().ok())
                        .map(|token| token.text_trimmed().to_string());
                    (local, imported)
                }
                AnyJsNamedImportSpecifier::JsShorthandNamedImportSpecifier(shorthand) => {
                    let Ok(local) = shorthand.local_name() else {
                        continue;
                    };
                    (local, None)
                }
                AnyJsNamedImportSpecifier::JsBogusNamedImportSpecifier(_) => continue,
            };
            let Some(ident) = local_binding.as_js_identifier_binding() else {
                continue;
            };
            let Ok(local_token) = ident.name_token() else {
                continue;
            };
            let local = local_token.text_trimmed().to_string();
            let offset = usize::from(ident.syntax().text_trimmed_range().start());
            let imported_name = imported_name.unwrap_or_else(|| local.clone());
            self.add_binding(
                scope,
                local.clone(),
                BindingKind::Import(ImportBinding {
                    source: source.to_string(),
                    imported: ImportedName::Named(imported_name),
                    local,
                }),
                offset,
            );
        }
    }
}

/// The syntax node of a class/object member name, for walking a computed
/// name's expression. Generic over the two name enums (`AnyJsClassMemberName`
/// and `AnyJsObjectMemberName`) since only the node is needed.
fn member_name<N: AstNode<Language = biome_js_syntax::JsLanguage>>(
    name: Option<N>,
) -> Option<JsSyntaxNode> {
    name.map(|name| name.syntax().clone())
}
