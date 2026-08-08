//! A lightweight, single-file, syntax-only model of lexical scopes,
//! declarations, and identifier resolution.
//!
//! See `docs/SEMANTIC_MODEL.md` for what this does and, importantly, does
//! not attempt to do -- in short: no types, no control/data-flow analysis,
//! no cross-file resolution. It answers exactly one question: "what does
//! this identifier refer to, in this file, right here?"
//!
//! Built once per file (see [`crate::FileContext::semantic`]) and shared by
//! every rule that asks for it; a rule that never calls
//! [`FileContext::semantic`](crate::FileContext::semantic) never pays for
//! building it.

mod binding;
mod builder;
mod scope;

pub use binding::{Binding, BindingId, BindingKind, ImportBinding, ImportedName};
pub use scope::{Scope, ScopeId, ScopeKind};

use std::collections::HashMap;

use biome_js_syntax::{JsReferenceIdentifier, JsSyntaxNode};
use biome_rowan::AstNode;

/// The scope tree, declarations, and resolved references for one file.
#[derive(Debug)]
pub struct SemanticModel {
    scopes: Vec<Scope>,
    bindings: Vec<Binding>,
    global: ScopeId,
    /// Byte offset of a reference identifier -> the binding it resolved to.
    /// Populated once, after the whole tree is walked, so that a binding
    /// declared later in the same scope than a reference (hoisting) still
    /// resolves correctly -- see `builder.rs`.
    resolutions: HashMap<usize, BindingId>,
}

impl SemanticModel {
    pub(crate) fn build(root: &JsSyntaxNode) -> Self {
        builder::build(root)
    }

    /// The file's top-level scope. Every other scope is a descendant of it.
    pub fn global_scope(&self) -> ScopeId {
        self.global
    }

    pub fn scope(&self, id: ScopeId) -> &Scope {
        &self.scopes[id.0]
    }

    pub fn binding(&self, id: BindingId) -> &Binding {
        &self.bindings[id.0]
    }

    /// The nearest binding `identifier` refers to, walking from its own
    /// scope out to the global scope. `None` means it's unbound in this
    /// file -- a global/host builtin (`console`, `Math`, ...) or a name
    /// that genuinely isn't declared anywhere this model can see.
    pub fn resolve(&self, identifier: &JsReferenceIdentifier) -> Option<&Binding> {
        let offset = usize::from(identifier.syntax().text_trimmed_range().start());
        self.resolutions.get(&offset).map(|id| self.binding(*id))
    }

    /// The same walk [`Self::resolve`] does -- from `scope` up to the
    /// global scope, returning the nearest binding named `name` -- exposed
    /// directly for callers that already have a scope and a name rather
    /// than a reference identifier node.
    pub fn resolve_in(&self, scope: ScopeId, name: &str) -> Option<&Binding> {
        let mut current = Some(scope);
        while let Some(id) = current {
            let scope = self.scope(id);
            if let Some(binding_id) = scope.binding(name) {
                return Some(self.binding(binding_id));
            }
            current = scope.parent();
        }
        None
    }
}
