use std::collections::HashMap;

use super::binding::BindingId;

/// Index into [`super::SemanticModel`]'s scope arena. Stable for the
/// lifetime of the model; scopes are never removed once created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub(super) usize);

/// What kind of lexical scope a [`Scope`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// The whole file: top-level declarations and imports.
    Global,
    /// A function, function expression, or arrow function -- also holds
    /// that function's own parameters and its body's declarations.
    Function,
    /// A bare `{ ... }` block, or an if/loop/try body block that is not
    /// itself a function's body.
    Block,
    /// A `for`/`for-in`/`for-of` loop head, holding the loop's own declared
    /// variable (e.g. the `i` in `for (let i = 0; ...)`).
    Loop,
    /// A `catch (error) { ... }` clause, holding the caught binding.
    Catch,
}

/// One lexical scope: the bindings declared directly in it, plus a link to
/// the scope it is nested in (`None` only for the root
/// [`ScopeKind::Global`] scope).
#[derive(Debug, Clone)]
pub struct Scope {
    pub(super) kind: ScopeKind,
    pub(super) parent: Option<ScopeId>,
    pub(super) bindings: HashMap<String, BindingId>,
}

impl Scope {
    pub fn kind(&self) -> ScopeKind {
        self.kind
    }

    pub fn parent(&self) -> Option<ScopeId> {
        self.parent
    }

    /// The binding this scope itself declares for `name`, if any -- does
    /// not walk to parent scopes; see [`super::SemanticModel::resolve`] for
    /// that.
    pub fn binding(&self, name: &str) -> Option<BindingId> {
        self.bindings.get(name).copied()
    }
}
