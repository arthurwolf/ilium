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
    let name = wit_name.trim_start_matches('%').replace('-', "_");
    if RUST_KEYWORDS.contains(&name.as_str()) { format!("r#{name}") } else { name }
}

/// `kebab-case` type to Rust `PascalCase` (reference-wit.md section 10).
pub fn rust_type_name(wit_name: &str) -> String {
    pascal_name(wit_name)
}

/// `kebab-case` function to TypeScript `camelCase`; `-json` becomes `Json` (reference-wit.md section 10).
pub fn typescript_function_name(wit_name: &str) -> String {
    let mut words = wit_name.trim_start_matches('%').split('-');
    let mut name = words.next().unwrap_or_default().to_owned();
    for word in words { name.push_str(&capitalize(word)); }
    name
}

/// `kebab-case` type to TypeScript `PascalCase` (reference-wit.md section 10).
pub fn typescript_type_name(wit_name: &str) -> String {
    pascal_name(wit_name)
}

/// ABI-051: the SDK wrapper names of a function: the plain name, plus the `_request` /
/// `Request` starting form when the function returns a `request-id`.
pub fn wrapper_names(function: &WitFunction, language: Language) -> Vec<String> {
    let (plain, suffix) = match language {
        Language::Rust => (rust_function_name(function.name), "_request"),
        Language::TypeScript => (typescript_function_name(function.name), "Request"),
    };
    let result: String = function.result.chars().filter(|character| !character.is_whitespace()).collect();
    let is_request = result == "request-id" || result.starts_with("result<request-id,");
    let mut names = vec![plain.clone()];
    if is_request {
        // Raw escaping belongs to the complete identifier, not to a prefix.
        let base = plain.strip_prefix("r#").unwrap_or(&plain);
        names.push(format!("{base}{suffix}"));
    }
    names
}

fn capitalize(word: &str) -> String {
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
        None => String::new(),
    }
}

fn pascal_name(wit_name: &str) -> String {
    wit_name.trim_start_matches('%').split('-').map(capitalize).collect()
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
    "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
    "trait", "true", "type", "unsafe", "use", "where", "while", "abstract", "become",
    "box", "do", "final", "gen", "macro", "override", "priv", "try", "typeof", "unsized",
    "virtual", "yield",
];
