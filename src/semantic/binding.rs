use super::scope::ScopeId;

/// Index into [`super::SemanticModel`]'s binding arena. Stable for the
/// lifetime of the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(pub(super) usize);

/// How a [`Binding`] came to exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingKind {
    Var,
    Let,
    Const,
    /// A `function foo() {}` declaration's or expression's own name.
    Function,
    /// A `class Foo {}` declaration's own name.
    Class,
    /// A function or arrow function parameter, including destructured and
    /// rest parameters.
    Parameter,
    /// A `catch (name)` clause's binding.
    CatchParameter,
    /// A name introduced by an `import` declaration; see [`ImportBinding`].
    Import(ImportBinding),
}

/// Where an imported binding came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    /// The module specifier text, e.g. `import x from "reselect"` -> `reselect`.
    pub source: String,
    /// What was imported: a named export, the default export, or the whole
    /// module namespace.
    pub imported: ImportedName,
    /// The name this binding is known by in the importing file -- differs
    /// from a `Named` `imported` name only when the import has an `as`
    /// alias.
    pub local: String,
}

/// What an [`ImportBinding`] refers to on the exporting side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedName {
    /// `import { name } from "..."`, or its aliased form
    /// `import { name as other } from "..."` (`name` is still what's
    /// recorded here; `other` is [`ImportBinding::local`]).
    Named(String),
    /// `import name from "..."`.
    Default,
    /// `import * as name from "..."`.
    Namespace,
}

/// One declaration: a name, how it was introduced, and where it lives.
#[derive(Debug, Clone)]
pub struct Binding {
    pub name: String,
    pub kind: BindingKind,
    pub scope: ScopeId,
    /// Byte offset of the identifier this binding was declared at, for
    /// turning into a line/column via [`crate::FileContext::line_col`].
    pub declared_at: usize,
}

impl Binding {
    /// This binding's import information, if it was introduced by an
    /// `import` declaration.
    pub fn import(&self) -> Option<&ImportBinding> {
        match &self.kind {
            BindingKind::Import(info) => Some(info),
            _ => None,
        }
    }
}
