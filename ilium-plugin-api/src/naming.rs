//! WIT-to-SDK name mapping (reference-wit.md section 10, ABI-020, ABI-051). Skel-api addition
//! to plan 1.3; the SDK generators and their tests share it.

use crate::generated::WitFunction;

/// An SDK language (reference-wit.md section 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// The Rust SDK.
    Rust,
    /// The TypeScript SDK.
    TypeScript,
}

/// `kebab-case` function to Rust `snake_case`, `%name` keywords escaped as `r#name`
/// (reference-wit.md section 10).
pub fn rust_function_name(wit_name: &str) -> String {
    unimplemented!("api::naming::rust_function_name")
}

/// `kebab-case` type to Rust `PascalCase` (reference-wit.md section 10).
pub fn rust_type_name(wit_name: &str) -> String {
    unimplemented!("api::naming::rust_type_name")
}

/// `kebab-case` function to TypeScript `camelCase`; `-json` becomes `Json` (reference-wit.md section 10).
pub fn typescript_function_name(wit_name: &str) -> String {
    unimplemented!("api::naming::typescript_function_name")
}

/// `kebab-case` type to TypeScript `PascalCase` (reference-wit.md section 10).
pub fn typescript_type_name(wit_name: &str) -> String {
    unimplemented!("api::naming::typescript_type_name")
}

/// ABI-051: the SDK wrapper names of a function: the plain name, plus the `_request` /
/// `Request` starting form when the function returns a `request-id`.
pub fn wrapper_names(function: &WitFunction, language: Language) -> Vec<String> {
    unimplemented!("api::naming::wrapper_names")
}
