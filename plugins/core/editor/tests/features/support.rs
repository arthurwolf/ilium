//! Shared helpers of the editor feature tests.

use ilium_plugin_core_editor::EditorClient;
use ilium_plugin_sdk::generated::exports::service_provider::{ServiceCall, ServiceReply};
use ilium_plugin_sdk::generated::types::BufferId;
use ilium_plugin_sdk::generated::types::{
    Activation, BufferRef, Event, FsPath, NodeId, PluginId, ViewTarget,
};
use ilium_plugin_sdk::testing::host::FakeHost;
use ilium_plugin_sdk::{Handler, Lifecycle, ServiceProvider};

/// Absolute path of the plugin manifest.
pub const MANIFEST_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/manifest.json");
/// The service the plugin offers (core-plugin-editor.md "Extension points").
pub const BUFFERS_SERVICE: &str = "ilium.core.editor/buffers";
/// Text of another manual page.
pub fn manual_page(name: &str) -> String {
    let path = ilium_plugin_sdk::testing::manual::pages_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_default()
}

/// A project-scope path.
pub fn project_path(path: &str) -> FsPath {
    FsPath {
        scope: "project".to_string(),
        path: path.to_string(),
    }
}

/// A buffer reference as the kernel sends it with `editor-opened`.
pub fn buffer_ref(
    id: u64,
    path: Option<&str>,
    language: Option<&str>,
    line_count: u32,
) -> BufferRef {
    BufferRef {
        buffer: BufferId(id),
        pane: Some(NodeId(7)),
        path: path.map(project_path),
        language: language.map(str::to_string),
        revision: 1,
        line_count,
        dirty: false,
    }
}

/// Number of lines the kernel reports for a text (an empty text has one line).
pub fn line_count_of(text: &str) -> u32 {
    let mut count = text.split('\n').count() as u32;
    if text.ends_with('\n') {
        count -= 1;
    }
    count.max(1)
}

/// An activation record for a client side instance.
pub fn activation(settings_json: &str) -> Activation {
    Activation {
        plugin: PluginId("ilium.core.editor".to_string()),
        api: "ilium:plugin@0.1.0".to_string(),
        instance: 1,
        side: "client".to_string(),
        settings_json: settings_json.to_string(),
        granted: vec![
            "editor.read".to_string(),
            "editor.edit".to_string(),
            "editor.provide".to_string(),
            "fs.read:project".to_string(),
            "panes.create".to_string(),
            "terminal.write".to_string(),
            "ui.panels".to_string(),
            "settings.write-kernel:editor".to_string(),
        ],
        restored: None,
    }
}

/// An activated plugin instance driven directly (no host context).
pub fn direct(settings_json: &str) -> EditorClient {
    let mut editor = EditorClient;
    Lifecycle::activate(&mut editor, activation(settings_json)).expect("activate");
    editor
}

/// Opens a buffer without a file on a directly driven instance.
pub fn direct_open(editor: &mut EditorClient, id: u64, language: Option<&str>, line_count: u32) {
    Handler::on_events(
        editor,
        vec![Event::EditorOpened(buffer_ref(
            id, None, language, line_count,
        ))],
    );
}

/// Calls the buffers service of a directly driven instance.
pub fn call_buffers(
    editor: &mut EditorClient,
    method: &str,
    payload: serde_json::Value,
) -> ServiceReply {
    ServiceProvider::handle(
        editor,
        ServiceCall {
            call: 1,
            service: BUFFERS_SERVICE.to_string(),
            method: method.to_string(),
            payload_json: payload.to_string(),
            caller: PluginId("example.caller".to_string()),
        },
    )
}

/// The JSON of a `done` reply; `Null` for anything else.
pub fn done_json(reply: ServiceReply) -> serde_json::Value {
    match reply {
        ServiceReply::Done(text) => serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
        ServiceReply::Deferred => serde_json::Value::Null,
    }
}

/// Inserts `text` at the start of an empty directly driven buffer (revision 1).
pub fn seed_text(editor: &mut EditorClient, id: u64, text: &str) {
    call_buffers(
        editor,
        "replace-range",
        serde_json::json!({
            "buffer": id,
            "base-revision": 1,
            "edits": [{
                "range": {"start": {"line": 0, "column": 0}, "end": {"line": 0, "column": 0}},
                "replacement": text,
            }],
        }),
    );
}

/// Reads `count` lines from `first_line` through the buffers service.
pub fn read_lines(
    editor: &mut EditorClient,
    id: u64,
    first_line: u32,
    count: u32,
) -> serde_json::Value {
    done_json(call_buffers(
        editor,
        "read",
        serde_json::json!({"buffer": id, "first-line": first_line, "count": count}),
    ))
}

/// A FakeHost built from the plugin manifest (client side, the manifest grants).
pub fn host() -> FakeHost {
    FakeHost::for_manifest(MANIFEST_PATH)
        .expect("manifest")
        .build()
}

/// A host whose plugin settings are set before activation, then activated.
pub fn active_host(settings: &[(&str, serde_json::Value)]) -> FakeHost {
    let mut host = host();
    for (key, value) in settings {
        host.settings().set(key, value.clone()).ok();
    }
    host.activate::<EditorClient>();
    host
}

/// Writes `text` to `project:<path>` in the fake file system and delivers `editor-opened`.
pub fn open_file(host: &mut FakeHost, id: u64, path: &str, language: Option<&str>, text: &str) {
    host.fs().write(format!("project:{path}"), text.as_bytes());
    host.deliver_one(Event::EditorOpened(buffer_ref(
        id,
        Some(path),
        language,
        line_count_of(text),
    )));
}

/// Renders the editor pane (node 7) at the given size.
pub fn render_pane(
    host: &mut FakeHost,
    columns: u16,
    rows: u16,
) -> ilium_plugin_sdk::testing::host::Screen {
    host.render(ViewTarget::Pane(NodeId(7)), columns, rows)
}
