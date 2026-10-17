//! Shared helpers: the host, a board pane bound to the sample file, and readers.

use std::path::PathBuf;

use ilium_plugin_core_kanban::KanbanClient;
use ilium_plugin_sdk::testing::{FakeHost, Screen, TreeNode};
use ilium_plugin_sdk::types::{Event, NodeClass, NodeId, PaneInfo, PrimitiveKind, ViewTarget};
use serde_json::Value;

/// The global pane kind id: `<plugin-id>.<local id>` (capability-panes-provide-kind.md).
pub const BOARD_KIND: &str = "ilium.core.kanban.board";

/// The board file of the page samples, in the harness shorthand `scope:path`.
pub const BOARD_PATH: &str = "project:docs/plan.md";

/// A board in the format of src/docs/editors-and-boards.md "How boards are stored".
pub const SAMPLE_BOARD: &str =
    "# Board\n\n## To do\n- Write docs\n- Add tests\n\n## Doing\n- Port parser\n\n## Done\n- Fix typos\n";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/kanban).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-kanban.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// A user documentation page under src/docs.
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

/// The manifest as JSON.
pub fn manifest() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

/// The first json code block after the heading text `heading`.
pub fn json_block_after(page: &str, heading: &str) -> Value {
    let start = page
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
}

/// A client-side host with the manifest grants; the file system holds `board_text` at `BOARD_PATH`
/// (when given) before the plugin activates.
pub fn client_host_with(board_text: Option<&str>) -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    if let Some(text) = board_text {
        host.fs().write(BOARD_PATH, text.as_bytes());
    }
    host.activate::<KanbanClient>();
    host
}

/// A client host with the sample board open in a pane; returns the host and the pane node.
pub fn board_host(board_text: &str) -> (FakeHost, NodeId) {
    let mut host = client_host_with(Some(board_text));
    let node = host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: "plan.md".to_string(),
        class: NodeClass::Pane,
        content_kind: Some(BOARD_KIND.to_string()),
        agent: None,
    });
    let node = NodeId(node);
    host.deliver_one(Event::PaneCreated(PaneInfo {
        node,
        content_kind: BOARD_KIND.to_string(),
        primitive: PrimitiveKind::Basic,
        columns: 80,
        rows: 24,
        focused: true,
        title: "plan.md".to_string(),
        is_restored: false,
    }));
    host.settle();
    (host, node)
}

/// Renders the board pane at the given size.
pub fn draw(host: &mut FakeHost, node: NodeId, columns: u16, rows: u16) -> Screen {
    host.render(ViewTarget::Pane(node), columns, rows)
}

/// Sends one key chord to the board pane and lets the host settle.
pub fn press(host: &mut FakeHost, node: NodeId, chord: &str) {
    host.key(ViewTarget::Pane(node), chord);
    host.settle();
}

/// Moves the selection to the header of the first column (up and left are idempotent at the edges).
pub fn select_first_header(host: &mut FakeHost, node: NodeId) {
    for _ in 0..6 {
        press(host, node, "k");
    }
    for _ in 0..6 {
        press(host, node, "h");
    }
}

/// The board file as text.
pub fn board_file(host: &mut FakeHost) -> String {
    let bytes = host.fs().read(BOARD_PATH).expect("board file exists");
    String::from_utf8(bytes).expect("board file is UTF-8")
}

/// The board file parsed into `(column title, card lines)`, ignoring blank lines and the `# Board` title.
pub fn sections(text: &str) -> Vec<(String, Vec<String>)> {
    let mut columns: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            columns.push((title.to_string(), Vec::new()));
        } else if let Some(card) = line.strip_prefix("- ") {
            if let Some(column) = columns.last_mut() {
                column.1.push(card.to_string());
            }
        }
    }
    columns
}

/// The rows of a screen as separate strings.
pub fn rows_of(screen: &Screen) -> Vec<String> {
    screen.text().lines().map(str::to_string).collect()
}
