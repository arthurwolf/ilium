//! Port twins of kernel-owned IPC handler originals (the `PORT:` family): the kernel keeps the
//! behaviour, so the twin drives it through the public `ServerKernel` and intent API.

use ilium_plugin_api::generated::types::{Intent, Outcome, SetBookmarkedSpec};

use crate::common::*;

/// True when some object key containing "bookmark" holds `true` anywhere in the JSON value.
fn holds_bookmark_flag(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(items) => items.iter().any(holds_bookmark_flag),
        serde_json::Value::Object(map) => map.iter().any(|(key, item)| {
            (key.contains("bookmark") && item == &serde_json::Value::Bool(true))
                || holds_bookmark_flag(item)
        }),
        _ => false,
    }
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::bookmark_request_broadcasts_the_authoritative_tree_and_marks_it_for_persistence
#[test]
fn port_001_bookmark_intent_is_accepted_and_marks_the_tree_for_persistence() {
    in_both_modes(|kernel| {
        let root = project(kernel, "api");
        let work = group(kernel, Some(root), "work");
        kernel.advance_clock_ms(750);
        kernel.settle();
        let writes_before = kernel.snapshot_write_count();
        let fingerprint_before = kernel.tree_fingerprint();

        let outcome = apply(
            kernel,
            Intent::SetBookmarked(SetBookmarkedSpec {
                node: work,
                bookmarked: true,
            }),
        );
        assert_eq!(outcome, Outcome::Ok);

        // The bookmark is a tree change: the snapshot is marked dirty and written after the debounce.
        kernel.advance_clock_ms(750);
        kernel.settle();
        assert_eq!(
            kernel.snapshot_write_count(),
            writes_before + 1,
            "the bookmark must dirty the snapshot"
        );
        // A bookmark changes neither structure, titles nor sizes.
        assert_eq!(kernel.tree_fingerprint(), fingerprint_before);
        assert_eq!(node(kernel, work).title, "work");

        let text = kernel
            .read_project_file(".ilium/sessions/default.json")
            .expect("snapshot file");
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON snapshot");
        assert!(
            holds_bookmark_flag(&json),
            "snapshot does not record the bookmark: {json}"
        );
    });
}
