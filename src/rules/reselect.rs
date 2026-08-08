//! Semantic identification of Reselect's `createSelector`, shared by
//! [`crate::rules::no_arrow_function_create_selector`] and
//! [`crate::rules::reselect_arity_match`] so the two rules agree on the
//! exact same definition rather than each re-implementing it.

use biome_js_syntax::JsReferenceIdentifier;

use crate::semantic::{ImportedName, SemanticModel};

const RESELECT_MODULE: &str = "reselect";
const CREATE_SELECTOR: &str = "createSelector";

/// Whether `reference` resolves to a binding introduced by
/// `import { createSelector } from "reselect"`, or its aliased form
/// `import { createSelector as x } from "reselect"` -- not merely an
/// identifier that happens to be spelled `createSelector`. A same-named
/// local function, or a `createSelector` imported from a different module,
/// both resolve to a binding with no matching import info and correctly
/// return `false` here.
pub(crate) fn resolves_to_reselect_create_selector(
    semantic: &SemanticModel,
    reference: &JsReferenceIdentifier,
) -> bool {
    let Some(binding) = semantic.resolve(reference) else {
        return false;
    };
    matches!(
        binding.import(),
        Some(import)
            if import.source == RESELECT_MODULE
                && matches!(&import.imported, ImportedName::Named(name) if name == CREATE_SELECTOR)
    )
}
