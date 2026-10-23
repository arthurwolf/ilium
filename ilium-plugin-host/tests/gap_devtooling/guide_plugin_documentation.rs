//! Generated plugin documentation from the manifest (DEV-938). Page: guide-plugin-documentation.

use crate::common::*;
use crate::{new_rust_project, with_code};

/// Every file under `dir`, concatenated in sorted order.
fn all_text(dir: &std::path::Path) -> String {
    tree_of(dir)
        .iter()
        .map(|name| read_text(dir, name))
        .collect::<Vec<_>>()
        .join("\n")
}

/// REQ: DEV-938
#[test]
fn plugin_docs_are_byte_identical_across_runs_and_mark_missing_descriptions() {
    let (root, env, dir) = new_rust_project();
    let first_out = root.path().join("docs-a");
    let second_out = root.path().join("docs-b");
    let first = run_in_project(&env, "docs", &["--out", first_out.to_str().expect("utf8")]);
    assert_eq!(first.exit_code, 0, "{:?}", first.stdout_jsonl);
    let second = run_in_project(&env, "docs", &["--out", second_out.to_str().expect("utf8")]);
    assert_eq!(second.exit_code, 0, "{:?}", second.stdout_jsonl);
    let names = tree_of(&first_out);
    assert!(!names.is_empty(), "the docs run writes a Markdown page");
    assert_eq!(
        names,
        tree_of(&second_out),
        "both runs write the same files"
    );
    for name in &names {
        assert_eq!(
            read_text(&first_out, name),
            read_text(&second_out, name),
            "{name} is byte-identical"
        );
    }

    project_with(&dir, |manifest| {
        manifest["settings"] =
            serde_json::json!([{ "key": "example.level", "type": "integer", "default": 3 }]);
    });
    let undocumented_out = root.path().join("docs-undocumented");
    let undocumented = run_in_project(
        &env,
        "docs",
        &["--out", undocumented_out.to_str().expect("utf8")],
    );
    assert_eq!(undocumented.exit_code, 0, "{:?}", undocumented.stdout_jsonl);
    assert!(
        all_text(&undocumented_out).contains("(undocumented)"),
        "a manifest field with no description is marked (undocumented)"
    );
    let checked = run_in_project(&env, "check", &[]);
    assert!(
        !with_code(&checked, "docs.missing-description").is_empty(),
        "check warns docs.missing-description: {:?}",
        checked.stdout_jsonl
    );
}
