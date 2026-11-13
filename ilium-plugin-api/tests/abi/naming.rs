//! WIT-to-SDK names: ABI-020 and ABI-051 (reference-wit.md section 10).

use ilium_plugin_api::generated::FUNCTIONS;
use ilium_plugin_api::naming::{
    rust_function_name, rust_type_name, typescript_function_name, typescript_type_name,
    wrapper_names, Language,
};
use proptest::prelude::*;

/// REQ: ABI-020
#[test]
fn abi_020_function_names() {
    assert_eq!(rust_function_name("get-plugin-json"), "get_plugin_json");
    assert_eq!(typescript_function_name("get-plugin-json"), "getPluginJson");
    assert_eq!(rust_function_name("fetch"), "fetch");
    assert_eq!(typescript_function_name("fetch"), "fetch");
}

/// REQ: ABI-020
#[test]
fn abi_020_type_names() {
    assert_eq!(rust_type_name("check-item"), "CheckItem");
    assert_eq!(typescript_type_name("check-item"), "CheckItem");
    assert_eq!(rust_type_name("ui-tree"), "UiTree");
}

/// REQ: ABI-020
#[test]
fn abi_020_rust_keyword_is_escaped() {
    assert_eq!(rust_function_name("%type"), "r#type");
    assert_eq!(typescript_function_name("%type"), "type");
}

/// REQ: ABI-020
#[test]
fn abi_020_percent_name_that_is_no_rust_keyword_is_plain() {
    assert_eq!(rust_function_name("%list"), "list");
}

fn is_request_function(result: &str) -> bool {
    result.starts_with("result<request-id,")
}

/// REQ: ABI-051
#[test]
fn abi_051_every_request_id_function_has_awaiting_and_starting_wrappers() {
    let mut checked = 0;
    for function in FUNCTIONS
        .iter()
        .filter(|function| is_request_function(function.result))
    {
        let rust = rust_function_name(function.name);
        let typescript = typescript_function_name(function.name);
        assert_eq!(
            wrapper_names(function, Language::Rust),
            vec![rust.clone(), format!("{rust}_request")],
            "{}/{}",
            function.interface,
            function.name
        );
        assert_eq!(
            wrapper_names(function, Language::TypeScript),
            vec![typescript.clone(), format!("{typescript}Request")],
            "{}/{}",
            function.interface,
            function.name
        );
        checked += 1;
    }
    assert_eq!(checked, 30);
}

/// REQ: ABI-051
#[test]
fn abi_051_other_functions_have_only_the_plain_wrapper() {
    let function = FUNCTIONS
        .iter()
        .find(|function| function.interface == "settings" && function.name == "get-json")
        .expect("settings.get-json");
    assert!(!is_request_function(function.result));
    assert_eq!(wrapper_names(function, Language::Rust), vec!["get_json"]);
    assert_eq!(
        wrapper_names(function, Language::TypeScript),
        vec!["getJson"]
    );
}

/// REQ: ABI-051
#[test]
fn doc_abi_051_named_interfaces_return_request_ids() {
    for interface in ["http", "jobs", "services", "inference"] {
        assert!(
            FUNCTIONS
                .iter()
                .any(|function| function.interface == interface
                    && is_request_function(function.result)),
            "{interface}"
        );
    }
}

/// REQ: ABI-051
#[test]
fn abi_051_json_suffix_is_kept() {
    for (wit, rust, typescript) in [
        ("get-json", "get_json", "getJson"),
        ("set-json", "set_json", "setJson"),
        ("get-plugin-json", "get_plugin_json", "getPluginJson"),
        ("set-plugin-json", "set_plugin_json", "setPluginJson"),
    ] {
        assert_eq!(rust_function_name(wit), rust);
        assert_eq!(typescript_function_name(wit), typescript);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // REQ: ABI-020
    #[test]
    fn abi_020_kebab_words_map_consistently(words in proptest::collection::vec("[a-z][a-z0-9]{0,7}", 1..5)) {
        prop_assume!(!(words.len() == 1 && RUST_KEYWORDS.contains(&words[0].as_str())));
        let wit = words.join("-");
        prop_assert_eq!(rust_function_name(&wit), words.join("_"));
        let camel: String = words
            .iter()
            .enumerate()
            .map(|(index, word)| if index == 0 { word.clone() } else { capitalize(word) })
            .collect();
        prop_assert_eq!(typescript_function_name(&wit), camel);
        let pascal: String = words.iter().map(|word| capitalize(word)).collect();
        prop_assert_eq!(rust_type_name(&wit), pascal.clone());
        prop_assert_eq!(typescript_type_name(&wit), pascal);
    }
}

/// Rust 2021 strict and reserved keywords plus `gen`; a lone keyword is escaped, so the
/// property excludes it.
const RUST_KEYWORDS: [&str; 51] = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "abstract", "become", "box", "do", "final", "gen", "macro", "override",
    "priv", "try", "typeof", "unsized", "virtual", "yield",
];

fn capitalize(word: &str) -> String {
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
        None => String::new(),
    }
}
