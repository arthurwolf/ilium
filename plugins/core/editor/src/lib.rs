//! Core plugin `ilium.core.editor` (manual page core-plugin-editor.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! Minimal conforming bodies: no contributions are produced until the feature logic moves here.

pub mod agent_from_line;
pub mod buffers;
pub mod close_dirty;
pub mod lexer;
pub mod line_path;
pub mod open_target;
mod template;

use buffers::{Buffer, Buffers};
use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::exports::token_provider::Token;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::ids::BufferId;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::generated::types::{
    BufferRef, CreatePaneSpec, EditorPointerEvent, FsPath, Intent, PointerButton, PointerKind,
    TextEdit,
};
use ilium_plugin_sdk::host;
use ilium_plugin_sdk::host::{EntryKind, HostFs};
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, ServiceProvider, TokenProvider, Views};

/// The kernel Settings tab this plugin owns.
const KERNEL_TAB: &str = "editor";
/// The plugin service the client calls to write that tab.
const SET_SETTING_SERVICE: &str = "ilium.core.editor/set-setting";
/// The service this plugin offers as the editor provider (list, read, replace-range).
const BUFFERS_SERVICE: &str = "ilium.core.editor/buffers";
/// Bytes one `fs.read` call takes (the import's 1 MiB limit).
const READ_CHUNK: u32 = 1024 * 1024;

/// The server side of `ilium.core.editor`: validates and writes the `editor` Settings tab.
#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
pub struct EditorServer;

impl Lifecycle for EditorServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for EditorServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl ServiceProvider for EditorServer {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        let error = |code: &str, message: &str| {
            ServiceReply::Done(
                serde_json::json!({"error": {"code": code, "message": message}}).to_string(),
            )
        };
        if call.service != SET_SETTING_SERVICE || call.method != "set" {
            return error("not-found", "unknown service method");
        }
        let payload: serde_json::Value = match serde_json::from_str(&call.payload_json) {
            Ok(value) => value,
            Err(_) => return error("invalid-argument", "the payload is not JSON"),
        };
        let field = |name: &str| payload.get(name).and_then(serde_json::Value::as_str);
        match (field("tab"), field("key"), field("value")) {
            (Some(KERNEL_TAB), Some(_), Some(_)) => ServiceReply::Done("{}".to_string()),
            (Some(_), Some(_), Some(_)) => {
                error("invalid-argument", "tab is not owned by this plugin")
            }
            _ => error(
                "invalid-argument",
                "tab, key and value are required strings",
            ),
        }
    }
}

/// The client side of `ilium.core.editor` (core-plugin-editor.md, world `ilium:client/base@0.1.0`).
///
/// As the editor provider it owns the text of every open buffer while it runs: `editor-opened`
/// brings a buffer in (reading its file through the `fs` import), the `buffers` service reads and
/// edits it, and the token provider colours it. Drawing the pane is not done here yet.
#[ilium_plugin(side = "client", exports = ["views", "token-provider", "service-provider"])]
#[derive(Default)]
pub struct EditorClient {
    buffers: Buffers,
}

impl EditorClient {
    /// Brings an opened buffer in: its language (the kernel's, else the one its path implies) and
    /// its text (the file's, else one empty line so a new path still opens).
    fn open_buffer(&mut self, mut reference: BufferRef) {
        if reference.language.is_none() {
            reference.language = reference
                .path
                .as_ref()
                .and_then(|path| lexer::language_for_path(&path.path))
                .map(str::to_string);
        }
        let text = reference
            .path
            .clone()
            .and_then(|path| read_project_file(&path));
        self.buffers
            .open(Buffer::opened(reference, text.as_deref()));
    }

    /// A click on a `TODO` line starts an agent with that line as its task.
    fn pointer(&self, event: EditorPointerEvent) -> Option<Submission> {
        if event.pointer.kind != PointerKind::Down || event.pointer.button != PointerButton::Left {
            return None;
        }
        let buffer = self.buffers.get(event.buffer.0)?;
        let text = buffer.line(event.position.line)?;
        if !agent_from_line::is_todo_line(text) {
            return None;
        }
        let file = buffer
            .reference
            .path
            .as_ref()
            .map_or("the buffer", |path| path.path.as_str());
        let prompt = agent_from_line::default_prompt(text, file, event.position.line);
        let title = format!("{} line {}", file, event.position.line + 1);
        Some(Submission::keyed(
            Intent::CreatePane(CreatePaneSpec {
                parent: None,
                title,
                content_kind: "ilium.terminal".to_string(),
                cwd: None,
                command: Some(vec!["claude".to_string()]),
                initial_input: Some(prompt),
            }),
            agent_from_line::dedup_key(event.buffer.0, event.position.line),
        ))
    }

    fn list_buffers(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.buffers
                .iter()
                .map(|buffer| {
                    let reference = &buffer.reference;
                    serde_json::json!({
                        "buffer": reference.buffer.0,
                        "pane": reference.pane.as_ref().map(|pane| pane.0),
                        "path": reference.path.as_ref().map(|path| {
                            serde_json::json!({"scope": path.scope, "path": path.path})
                        }),
                        "language": reference.language,
                        "revision": reference.revision,
                        "line-count": reference.line_count,
                        "dirty": reference.dirty,
                    })
                })
                .collect(),
        )
    }

    fn read_buffer(&self, payload: &serde_json::Value) -> serde_json::Value {
        let number = |name: &str| payload.get(name).and_then(serde_json::Value::as_u64);
        let (Some(id), Some(first_line), Some(count)) =
            (number("buffer"), number("first-line"), number("count"))
        else {
            return reply_error(
                "invalid-argument",
                "buffer, first-line and count are required",
            );
        };
        let lines = self.buffers.get(id).map_or_else(Vec::new, |buffer| {
            buffer.read(
                u32::try_from(first_line).unwrap_or(u32::MAX),
                u32::try_from(count).unwrap_or(u32::MAX),
            )
        });
        serde_json::json!(lines)
    }

    fn replace_range(&mut self, payload: &serde_json::Value) -> serde_json::Value {
        let Some(id) = payload.get("buffer").and_then(serde_json::Value::as_u64) else {
            return reply_error("invalid-argument", "buffer is a required number");
        };
        let Some(base_revision) = payload
            .get("base-revision")
            .and_then(serde_json::Value::as_u64)
        else {
            return reply_error("invalid-argument", "base-revision is a required number");
        };
        let Some(edits) = payload
            .get("edits")
            .and_then(|edits| serde_json::from_value::<Vec<TextEdit>>(edits.clone()).ok())
        else {
            return reply_error("invalid-argument", "edits is a required list of text edits");
        };
        let Some(buffer) = self.buffers.get_mut(id) else {
            return reply_error("not-found", "no such buffer");
        };
        match buffer.replace_ranges(base_revision, &edits) {
            Ok(revision) => serde_json::json!({ "revision": revision }),
            Err(refused) => reply_error(refused.code(), &refused.message()),
        }
    }
}

/// The reply body of a refused service call.
fn reply_error(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({"error": {"code": code, "message": message}})
}

/// The text of a project file, up to the buffer limit; `None` when it is missing, not a file or
/// unreadable. Invalid UTF-8 is replaced, never dropped silently into a different line count.
fn read_project_file(path: &FsPath) -> Option<String> {
    let mut api = host();
    let metadata = HostFs::metadata(&mut *api, path.clone()).ok()?;
    if metadata.kind != EntryKind::File {
        return None;
    }
    let mut bytes: Vec<u8> = Vec::new();
    let mut offset = 0u64;
    loop {
        let chunk = HostFs::read(&mut *api, path.clone(), offset, READ_CHUNK).ok()?;
        if chunk.is_empty() {
            break;
        }
        offset += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
        if bytes.len() > buffers::MAX_BUFFER_BYTES {
            return None;
        }
        if offset >= metadata.length {
            break;
        }
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

impl Lifecycle for EditorClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for EditorClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::EditorOpened(reference) => self.open_buffer(reference),
                Event::EditorClosed(reference) => self.buffers.close(reference.buffer.0),
                Event::EditorPointer(pointer) => submissions.extend(self.pointer(pointer)),
                _ => {}
            }
        }
        submissions
    }
}

impl Views for EditorClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        UiTree {
            version: 1,
            nodes: Vec::new(),
            root: 0,
        }
    }
}

impl TokenProvider for EditorClient {
    fn tokens(
        &mut self,
        buffer: BufferId,
        first_line: u32,
        lines: Vec<String>,
        state_in: Option<Vec<u8>>,
    ) -> (Vec<Token>, Option<Vec<u8>>) {
        let Some(language) = self
            .buffers
            .get(buffer.0)
            .and_then(|open| open.reference.language.as_deref())
        else {
            return (Vec::new(), None);
        };
        lexer::tokenize(language, first_line, &lines, state_in.as_deref())
    }
}

impl ServiceProvider for EditorClient {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        if call.service != BUFFERS_SERVICE {
            return ServiceReply::Done(reply_error("not-found", "unknown service").to_string());
        }
        let payload: serde_json::Value = if call.payload_json.trim().is_empty() {
            serde_json::Value::Null
        } else {
            match serde_json::from_str(&call.payload_json) {
                Ok(value) => value,
                Err(_) => {
                    return ServiceReply::Done(
                        reply_error("invalid-argument", "the payload is not JSON").to_string(),
                    )
                }
            }
        };
        let body = match call.method.as_str() {
            "list" => self.list_buffers(),
            "read" => self.read_buffer(&payload),
            "replace-range" => self.replace_range(&payload),
            _ => reply_error("not-found", "unknown service method"),
        };
        ServiceReply::Done(body.to_string())
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use serde_json::{json, Value};

    /// Top-level keys of the complete manifest shown in core-plugin-editor.md ("Complete manifest").
    const REQUIRED_TOP_LEVEL_KEYS: [&str; 14] = [
        "api",
        "id",
        "name",
        "version",
        "publisher",
        "description",
        "license",
        "server",
        "client",
        "capabilities",
        "activation",
        "unstable",
        "contributes",
        "limits_tier",
    ];

    /// Client exports in the order the manual lists them.
    const CLIENT_EXPORTS: [&str; 3] = ["views", "token-provider", "service-provider"];

    /// The kernel grant that owns the built-in `editor` Settings tab.
    const EDITOR_TAB_GRANT: &str = "settings.write-kernel:editor";

    /// The shipped manifest, parsed from the file next to this crate's `Cargo.toml`.
    fn load_manifest() -> Value {
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json is valid JSON")
    }

    /// Required top-level keys absent from `manifest`, in the manual's order.
    fn missing_required_keys(manifest: &Value) -> Vec<&'static str> {
        REQUIRED_TOP_LEVEL_KEYS
            .iter()
            .copied()
            .filter(|key| manifest.get(*key).is_none())
            .collect()
    }

    /// Strings of a JSON array; anything else yields an empty list.
    fn strings_at(value: &Value) -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn client_exports(manifest: &Value) -> Vec<String> {
        strings_at(&manifest["client"]["exports"])
    }

    fn client_exports_match_manual(manifest: &Value) -> bool {
        client_exports(manifest) == CLIENT_EXPORTS
    }

    /// REQ: COR-050
    #[test]
    fn manifest_has_every_key_of_the_complete_manifest() {
        let manifest = load_manifest();
        assert_eq!(missing_required_keys(&manifest), Vec::<&str>::new());
    }

    /// REQ: COR-050
    #[test]
    fn client_side_entry_world_and_exports_match_the_manual() {
        let manifest = load_manifest();
        assert_eq!(manifest["id"], "ilium.core.editor");
        assert_eq!(manifest["client"]["entry"], "client.wasm");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(client_exports(&manifest), CLIENT_EXPORTS);
    }

    /// REQ: COR-050
    #[test]
    fn server_side_part_exports_only_service_provider() {
        let manifest = load_manifest();
        assert_eq!(manifest["server"]["entry"], "server.wasm");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(
            strings_at(&manifest["server"]["exports"]),
            ["service-provider"]
        );
    }

    /// REQ: COR-030
    #[test]
    fn client_capabilities_never_hold_the_editor_kernel_grant() {
        let manifest = load_manifest();
        let client_capabilities = strings_at(&manifest["capabilities"]["client"]);
        assert!(!client_capabilities
            .iter()
            .any(|capability| capability == EDITOR_TAB_GRANT));
    }

    /// REQ: COR-030
    #[test]
    fn exactly_one_side_holds_the_editor_kernel_grant_on_the_server() {
        let manifest = load_manifest();
        let client_capabilities = strings_at(&manifest["capabilities"]["client"]);
        let server_capabilities = strings_at(&manifest["capabilities"]["server"]);
        let holder_count = client_capabilities
            .iter()
            .chain(server_capabilities.iter())
            .filter(|capability| capability == &EDITOR_TAB_GRANT)
            .count();
        assert_eq!(holder_count, 1);
        assert!(server_capabilities
            .iter()
            .any(|capability| capability == EDITOR_TAB_GRANT));
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_lazy_on_the_editor_pane_kind() {
        let manifest = load_manifest();
        assert_eq!(strings_at(&manifest["activation"]), ["pane-kind:editor"]);
    }

    /// REQ: SEC-293
    #[test]
    fn before_save_hook_carries_high_risk_and_the_manual_limits() {
        let manifest = load_manifest();
        let extension_points = manifest["contributes"]["extension_points"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let before_save = extension_points
            .iter()
            .find(|point| point["id"] == "before-save")
            .expect("before-save extension point is declared");
        assert_eq!(before_save["risk"], "high");
        assert_eq!(before_save["deadline_ms"], 20);
        assert_eq!(before_save["max_handlers"], 8);
    }

    /// REQ: COR-050
    #[test]
    fn plugin_type_implements_the_traits_behind_every_manifest_export() {
        fn assert_provides_manifest_exports<
            T: Lifecycle + Handler + Views + TokenProvider + ServiceProvider,
        >() {
        }
        assert_provides_manifest_exports::<EditorClient>();
        let _default_client = EditorClient::default();
    }

    /// REQ: COR-050
    #[test]
    fn empty_or_truncated_manifest_is_rejected() {
        assert!(serde_json::from_str::<Value>("").is_err());
        assert!(serde_json::from_str::<Value>("{\"id\": \"ilium.core.editor\"").is_err());
        assert_eq!(
            missing_required_keys(&json!({})),
            REQUIRED_TOP_LEVEL_KEYS.to_vec()
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_without_the_views_export_does_not_match_the_manual() {
        let mut manifest = load_manifest();
        manifest["client"]["exports"] = json!(["token-provider", "service-provider"]);
        assert!(!client_exports_match_manual(&manifest));
        assert_eq!(
            client_exports(&manifest),
            ["token-provider", "service-provider"]
        );
    }
}
