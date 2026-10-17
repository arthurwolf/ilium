//! Shared helpers: paths, page and manifest readers, the client host, seeded keymap data, the
//! 35-action table of the page.

use std::path::PathBuf;

use ilium_plugin_core_keys_and_help::KeysAndHelpClient;
use ilium_plugin_sdk::host::keymap::Binding;
use ilium_plugin_sdk::testing::FakeHost;
use serde_json::Value;

/// The plugin id (manifest `id`).
pub const PLUGIN_ID: &str = "ilium.core.keys-and-help";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/keys-and-help).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    manual_page("core-plugin-keys-and-help.md")
}

/// Any page of the manual by file name.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
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

/// The client grants of the manifest.
pub fn client_grants() -> Vec<String> {
    manifest()["capabilities"]["client"]
        .as_array()
        .expect("client grants")
        .iter()
        .map(|grant| grant.as_str().expect("grant string").to_string())
        .collect()
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<KeysAndHelpClient>();
    host
}

/// A client-side host with the plugin activated and the tmux default keymap seeded: the Keyboard
/// tab holds `shortcut_base` `ctrl+b`, `navigation_shortcut_base` `ctrl+b` and preset `tmux`.
pub fn seeded_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.kernel_settings().set_tab(
        "keyboard",
        serde_json::json!({"shortcut_base": "ctrl+b", "navigation_shortcut_base": "ctrl+b", "preset": "tmux"}),
    );
    host.keymap().set_bindings(sample_bindings());
    host.activate::<KeysAndHelpClient>();
    host
}

/// One binding. The context is the group heading of the key reference, the command is the label
/// ("Labels come from the keymap").
pub fn binding(context: &str, label: &str, chord: &str, is_default: bool) -> Binding {
    Binding {
        command: label.to_string(),
        chords: vec![chord.to_string()],
        context: context.to_string(),
        is_default,
    }
}

/// The bindings of the key reference figure: two panes actions and two tree actions.
pub fn sample_bindings() -> Vec<Binding> {
    vec![
        binding("Panes", "Split vertically", "prefix+%", true),
        binding("Panes", "Split horizontally", "prefix+\"", true),
        binding("Tree", "Move up", "prefix+Up", true),
        binding("Tree", "Rename", "prefix+r", true),
    ]
}

/// Display labels of the sample chords, as `chord-label` returns them.
pub fn seed_sample_labels(host: &mut FakeHost) {
    host.keymap().set_label("prefix+%", "Prefix  %");
    host.keymap().set_label("prefix+\"", "Prefix  \"");
    host.keymap().set_label("prefix+Up", "Prefix  Up");
    host.keymap().set_label("prefix+r", "Prefix  r");
}

/// Rows of the first markdown table that starts with `header_prefix`: the cells of each body row.
pub fn table_rows(text: &str, header_prefix: &str) -> Vec<Vec<String>> {
    let start = text
        .find(header_prefix)
        .unwrap_or_else(|| panic!("table {header_prefix} missing"));
    text[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

/// One row of the "Action names, command ids and presets" table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRow {
    pub name: String,
    pub command: String,
    pub default: String,
    pub screen: String,
    pub tmux: String,
}

fn unquote(cell: &str) -> String {
    cell.trim().trim_matches('`').to_string()
}

/// The 35 rows of the action table of the page.
pub fn action_rows() -> Vec<ActionRow> {
    table_rows(&page_text(), "| Action name |")
        .into_iter()
        .map(|cells| ActionRow {
            name: unquote(&cells[0]),
            command: unquote(&cells[1]),
            default: unquote(&cells[2]),
            screen: unquote(&cells[3]),
            tmux: unquote(&cells[4]),
        })
        .collect()
}

/// A binding of the kernel keymap for one action: command `kernel.<name>`, one chord.
pub fn action_binding(row: &ActionRow, key: &str) -> Binding {
    Binding {
        command: row.command.clone(),
        chords: vec![format!("prefix+{key}")],
        context: "Keyboard".to_string(),
        is_default: true,
    }
}

/// The golden file `name` under tests/features/golden.
pub fn golden_path(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/features/golden")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// The `<pre class="term">` figures of the page, tags stripped, trailing spaces trimmed.
pub fn page_figures() -> Vec<String> {
    let page = page_text();
    let mut figures = Vec::new();
    let mut rest = page.as_str();
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let body = &rest[start + "<pre class=\"term\">".len()..];
        let end = body.find("</pre>").expect("figure end");
        let mut text = String::new();
        let mut in_tag = false;
        for character in body[..end].chars() {
            match character {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => text.push(character),
                _ => {}
            }
        }
        let lines: Vec<&str> = text.trim_matches('\n').lines().map(str::trim_end).collect();
        figures.push(lines.join("\n"));
        rest = &body[end..];
    }
    figures
}

/// Screen text with trailing spaces trimmed per line and no trailing blank lines.
pub fn normalized(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    lines.join("\n").trim_end_matches('\n').to_string()
}
