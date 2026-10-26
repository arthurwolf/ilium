//! SDK naming and plugin identifier contract boundaries.
use ilium_plugin_api::generated::WitFunction;
use ilium_plugin_api::ids::parse_plugin_id;
use ilium_plugin_api::naming::{rust_function_name, rust_type_name, typescript_function_name, typescript_type_name, wrapper_names, Language};

#[test]
fn identifiers_enforce_segment_and_total_boundaries() {
    for text in ["a.b", "0.1", "com.acme.git-tools", "a.b.c.d.e.f.g.h"] {
        assert_eq!(parse_plugin_id(text).unwrap().0, text);
    }
    let longest = ["a".repeat(32), "b".repeat(32), "c".repeat(32), "d".repeat(29)].join(".");
    assert_eq!(longest.len(), 128);
    assert!(parse_plugin_id(&longest).is_ok());
    for text in [String::new(), "demo".into(), "a.b.c.d.e.f.g.h.i".into(), "a..b".into(), "a.-b".into(), "a.b-".into(), "A.b".into(), "a.b_c".into(), "a.é".into(), format!("a.{}", "b".repeat(33)), longest + "d"] {
        assert_eq!(parse_plugin_id(&text).unwrap_err().code, "manifest.id-invalid", "{text}");
    }
}

#[test]
fn names_preserve_json_and_escape_only_keywords() {
    assert_eq!(rust_function_name("%type"), "r#type");
    assert_eq!(rust_function_name("%list"), "list");
    assert_eq!(rust_function_name("get-plugin-json"), "get_plugin_json");
    assert_eq!(typescript_function_name("get-plugin-json"), "getPluginJson");
    assert_eq!(typescript_function_name("%type"), "type");
    assert_eq!(rust_type_name("ui-tree"), "UiTree");
    assert_eq!(typescript_type_name("request-id"), "RequestId");
}

#[test]
fn only_request_results_add_starting_wrappers() {
    let mut function = WitFunction { package: "ilium:host", interface: "http", name: "fetch-json", result: "result<request-id, plugin-error>", unstable: None };
    assert_eq!(wrapper_names(&function, Language::Rust), ["fetch_json", "fetch_json_request"]);
    assert_eq!(wrapper_names(&function, Language::TypeScript), ["fetchJson", "fetchJsonRequest"]);
    for result in ["", "option<request-id>", "result<list<request-id>, plugin-error>", "result<string, request-id>"] {
        function.result = result;
        assert_eq!(wrapper_names(&function, Language::Rust), ["fetch_json"], "{result}");
    }
}
