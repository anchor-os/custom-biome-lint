pub mod no_arrow_function_create_selector;
pub mod no_native_map;
pub mod registry;
pub mod reselect_arity_match;
pub mod rule;

pub use no_arrow_function_create_selector::NoArrowFunctionCreateSelector;
pub use no_native_map::NoNativeMap;
pub use registry::RuleRegistry;
pub use reselect_arity_match::ReselectArityMatch;
pub use rule::Rule;

/// Extensions shared by all current rules.
pub const JS_EXTENSIONS: &[&str] = &[".js", ".jsx"];

/// Default glob shared by all current rules.
pub const JS_PATTERN: &str = "src/**/*.{js,jsx}";
