pub mod bare_arrow_param_prop_assign;
pub mod deep_param_prop_assign;
pub mod destructure_default_param_assign;
pub mod destructure_param_prop_assign;
pub mod no_arrow_function_create_selector;
pub mod no_native_map;
pub mod param_mutating_array_method_call;
mod param_mutation;
pub mod registry;
mod reselect;
pub mod reselect_arity_match;
pub mod rule;

pub use bare_arrow_param_prop_assign::BareArrowParamPropAssign;
pub use deep_param_prop_assign::DeepParamPropAssign;
pub use destructure_default_param_assign::DestructureDefaultParamAssign;
pub use destructure_param_prop_assign::DestructureParamPropAssign;
pub use no_arrow_function_create_selector::NoArrowFunctionCreateSelector;
pub use no_native_map::NoNativeMap;
pub use param_mutating_array_method_call::ParamMutatingArrayMethodCall;
pub use registry::RuleRegistry;
pub use reselect_arity_match::ReselectArityMatch;
pub use rule::Rule;

/// Extensions shared by all current rules.
pub const JS_EXTENSIONS: &[&str] = &[".js", ".jsx"];
