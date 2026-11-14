//! Gap G13: service schemas at run time. Every schema file the core plugins ship installs, and
//! the checks the service pump runs refuse what the files describe as wrong.

use std::path::Path;

use ilium_plugin_host::schemas::{Direction, ServiceSchemas};

/// The schema files under `plugins/core/*/schemas/`, as (path, text).
fn core_schema_files() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/core");
    let mut files = Vec::new();
    let Ok(plugins) = std::fs::read_dir(&root) else {
        return files;
    };
    for plugin in plugins.flatten() {
        let Ok(schemas) = std::fs::read_dir(plugin.path().join("schemas")) else {
            continue;
        };
        for schema in schemas.flatten() {
            let path = schema.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let text = std::fs::read_to_string(&path).expect("schema file is readable");
                files.push((path.display().to_string(), text));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn every_core_schema_file_installs() {
    let files = core_schema_files();
    assert!(!files.is_empty(), "the core plugins ship schema files");
    let mut schemas = ServiceSchemas::default();
    for (path, text) in &files {
        schemas
            .install(path, text)
            .unwrap_or_else(|error| panic!("{path}: {error}"));
        assert_eq!(schemas.text(path), Some(text.as_str()));
    }
}

#[test]
fn the_cost_usage_schema_refuses_a_bad_request_and_a_bad_reply() {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/core/cost/schemas/usage.json"),
    )
    .expect("cost ships schemas/usage.json");
    let mut schemas = ServiceSchemas::default();
    schemas
        .install("ilium.core.cost/usage", &text)
        .expect("installs");
    let check = |direction, payload: &str| {
        schemas.check("ilium.core.cost/usage", "usage", direction, payload)
    };
    assert!(check(Direction::Request, r#"{"window":"day"}"#).is_ok());
    let refused = check(Direction::Request, r#"{"window":7}"#).expect_err("window is a string");
    assert_eq!(refused.pointer, "/window");
    assert!(check(Direction::Response, r#"{"panes":{}}"#).is_ok());
    let refused = check(Direction::Response, "{}").expect_err("panes is required");
    assert_eq!(refused.pointer, "/panes");
}
