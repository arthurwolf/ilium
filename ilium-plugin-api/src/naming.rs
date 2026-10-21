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
pub fn rust_function_name(_wit_name: &str) -> String {
    unimplemented!("api::naming::rust_function_name")
}

/// `kebab-case` type to Rust `PascalCase` (reference-wit.md section 10).
pub fn rust_type_name(_wit_name: &str) -> String {
    unimplemented!("api::naming::rust_type_name")
}

/// `kebab-case` function to TypeScript `camelCase`; `-json` becomes `Json` (reference-wit.md section 10).
pub fn typescript_function_name(_wit_name: &str) -> String {
    unimplemented!("api::naming::typescript_function_name")
}

/// `kebab-case` type to TypeScript `PascalCase` (reference-wit.md section 10).
pub fn typescript_type_name(_wit_name: &str) -> String {
    unimplemented!("api::naming::typescript_type_name")
}

/// ABI-051: the SDK wrapper names of a function: the plain name, plus the `_request` /
/// `Request` starting form when the function returns a `request-id`.
pub fn wrapper_names(_function: &WitFunction, _language: Language) -> Vec<String> {
    unimplemented!("api::naming::wrapper_names")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::WitFunction;

    fn function_with_result(name: &'static str, result: &'static str) -> WitFunction {
        WitFunction {
            package: "ilium:host",
            interface: "http",
            name,
            result,
            unstable: None,
        }
    }

    /// REQ: ABI-020
    #[test]
    fn rust_function_name_converts_kebab_case_to_snake_case() {
        assert_eq!(rust_function_name("read-cells"), "read_cells");
    }

    /// REQ: ABI-020
    #[test]
    fn rust_function_name_keeps_single_word_unchanged() {
        assert_eq!(rust_function_name("fetch"), "fetch");
    }

    /// REQ: ABI-051
    #[test]
    fn rust_function_name_keeps_json_suffix_as_underscore_json() {
        assert_eq!(rust_function_name("get-json"), "get_json");
        assert_eq!(rust_function_name("set-plugin-json"), "set_plugin_json");
    }

    /// REQ: ABI-020
    #[test]
    fn rust_function_name_escapes_percent_keywords_as_raw_identifiers() {
        assert_eq!(rust_function_name("%type"), "r#type");
        assert_eq!(rust_function_name("%loop"), "r#loop");
    }

    /// REQ: ABI-020
    #[test]
    fn rust_function_name_empty_input_gives_empty_name() {
        assert_eq!(rust_function_name(""), "");
    }

    /// REQ: ABI-020
    #[test]
    fn rust_type_name_converts_kebab_case_to_pascal_case() {
        assert_eq!(rust_type_name("catalog-item"), "CatalogItem");
        assert_eq!(rust_type_name("node-flags"), "NodeFlags");
    }

    /// REQ: ABI-020
    #[test]
    fn rust_type_name_capitalises_single_word() {
        assert_eq!(rust_type_name("tone"), "Tone");
    }

    /// REQ: ABI-020
    #[test]
    fn rust_type_name_empty_input_gives_empty_name() {
        assert_eq!(rust_type_name(""), "");
    }

    /// REQ: ABI-020
    #[test]
    fn typescript_function_name_converts_kebab_case_to_camel_case() {
        assert_eq!(typescript_function_name("read-cells"), "readCells");
        assert_eq!(typescript_function_name("fetch"), "fetch");
    }

    /// REQ: ABI-020
    #[test]
    fn typescript_function_name_turns_json_suffix_into_json() {
        assert_eq!(typescript_function_name("get-json"), "getJson");
        assert_eq!(typescript_function_name("set-plugin-json"), "setPluginJson");
    }

    /// REQ: ABI-020
    #[test]
    fn typescript_function_name_keeps_percent_keyword_unescaped() {
        assert_eq!(typescript_function_name("%type"), "type");
    }

    /// REQ: ABI-020
    #[test]
    fn typescript_function_name_empty_input_gives_empty_name() {
        assert_eq!(typescript_function_name(""), "");
    }

    /// REQ: ABI-020
    #[test]
    fn typescript_type_name_converts_kebab_case_to_pascal_case() {
        assert_eq!(typescript_type_name("catalog-item"), "CatalogItem");
        assert_eq!(typescript_type_name("tone"), "Tone");
    }

    /// REQ: ABI-020
    #[test]
    fn typescript_type_name_empty_input_gives_empty_name() {
        assert_eq!(typescript_type_name(""), "");
    }

    /// REQ: ABI-051
    #[test]
    fn wrapper_names_request_id_function_has_plain_and_request_rust_wrappers() {
        let function = function_with_result("fetch", "result<request-id, plugin-error>");
        assert_eq!(
            wrapper_names(&function, Language::Rust),
            vec!["fetch".to_string(), "fetch_request".to_string()]
        );
    }

    /// REQ: ABI-051
    #[test]
    fn wrapper_names_request_id_function_has_plain_and_request_typescript_wrappers() {
        let function = function_with_result("fetch", "result<request-id, plugin-error>");
        assert_eq!(
            wrapper_names(&function, Language::TypeScript),
            vec!["fetch".to_string(), "fetchRequest".to_string()]
        );
    }

    /// REQ: ABI-051
    #[test]
    fn wrapper_names_json_request_function_keeps_json_before_request_suffix() {
        let function = function_with_result("fetch-json", "result<request-id, plugin-error>");
        assert_eq!(
            wrapper_names(&function, Language::Rust),
            vec!["fetch_json".to_string(), "fetch_json_request".to_string()]
        );
        assert_eq!(
            wrapper_names(&function, Language::TypeScript),
            vec!["fetchJson".to_string(), "fetchJsonRequest".to_string()]
        );
    }

    /// REQ: ABI-051
    #[test]
    fn wrapper_names_non_request_function_has_only_plain_wrapper() {
        let function = function_with_result("read-cells", "result<list<cell>, plugin-error>");
        assert_eq!(
            wrapper_names(&function, Language::Rust),
            vec!["read_cells".to_string()]
        );
        assert_eq!(
            wrapper_names(&function, Language::TypeScript),
            vec!["readCells".to_string()]
        );
    }

    /// REQ: ABI-051
    #[test]
    fn wrapper_names_function_without_result_has_only_plain_wrapper() {
        let function = function_with_result("get-json", "");
        assert_eq!(
            wrapper_names(&function, Language::Rust),
            vec!["get_json".to_string()]
        );
        assert_eq!(
            wrapper_names(&function, Language::TypeScript),
            vec!["getJson".to_string()]
        );
    }

    /// REQ: ABI-051
    #[test]
    fn wrapper_names_list_of_request_ids_is_not_a_request_function() {
        let function = function_with_result("collect", "result<list<request-id>, plugin-error>");
        assert_eq!(
            wrapper_names(&function, Language::Rust),
            vec!["collect".to_string()]
        );
    }
}
