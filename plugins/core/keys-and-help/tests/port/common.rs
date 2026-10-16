//! Helpers of the port twins: paths, the page tables, seeded client hosts, small render helpers.

use std::path::PathBuf;

use ilium_plugin_core_keys_and_help::KeysAndHelpClient;
use ilium_plugin_sdk::host::keymap::Binding;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::events::KernelSettingsEvent;
use serde_json::{json, Value};

pub const PLUGIN_ID: &str = "ilium.core.keys-and-help";

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

pub fn page_text() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/plugins-manual/pages/core-plugin-keys-and-help.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

pub fn keys_panel() -> ViewTarget {
    ViewTarget::Panel("keys".to_string())
}

pub fn keyboard_page() -> ViewTarget {
    ViewTarget::SettingsPage("keyboard".to_string())
}

/// A client host with the manifest grants, the keyboard tab holding `tab`, the plugin activated.
pub fn host_with_tab(tab: Value) -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build();
    host.kernel_settings().set_tab("keyboard", tab);
    host.keymap().set_bindings(Vec::new());
    host.activate::<KeysAndHelpClient>();
    host
}

/// The tmux default: both prefixes Ctrl+B, preset tmux.
pub fn tmux_tab() -> Value {
    json!({"shortcut_base": "ctrl+b", "navigation_shortcut_base": "ctrl+b", "preset": "tmux"})
}

pub fn tmux_host() -> FakeHost {
    host_with_tab(tmux_tab())
}

pub fn binding(context: &str, label: &str, chord: &str, is_default: bool) -> Binding {
    Binding { command: label.to_string(), chords: vec![chord.to_string()], context: context.to_string(), is_default }
}

/// Seeds the keymap and tells the plugin through `keymap-changed`.
pub fn set_keymap(host: &mut FakeHost, bindings: Vec<Binding>, labels: &[(&str, &str)]) {
    for (chord, label) in labels {
        host.keymap().set_label(chord, label);
    }
    host.keymap().set_bindings(bindings);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
}

/// Replaces the Keyboard tab and tells the plugin through `kernel-settings-changed`.
pub fn set_tab(host: &mut FakeHost, tab: Value, key: &str) {
    host.kernel_settings().set_tab("keyboard", tab);
    host.deliver_one(Event::KernelSettingsChanged(KernelSettingsEvent {
        tab: "keyboard".to_string(),
        keys: vec![key.to_string()],
    }));
    host.settle();
}

pub fn render_text(host: &mut FakeHost, target: ViewTarget, columns: u16, rows: u16) -> String {
    host.render(target, columns, rows).text()
}

/// Display width in terminal cells (CJK and fullwidth forms take two).
pub fn cell_width(text: &str) -> usize {
    text.chars().map(|character| if (character as u32) >= 0x2E80 { 2 } else { 1 }).sum()
}

/// Id of the capture action of a row label: lower case, spaces and underscores become hyphens.
pub fn capture_id(label: &str) -> String {
    format!("capture-{}", label.to_lowercase().replace([' ', '_'], "-"))
}

pub fn write_chord(host: &mut FakeHost, chord: &str) {
    host.send_action(keyboard_page(), "chord", ActionKind::Changed, ActionValue::Text(chord.to_string()));
}

pub fn activate(host: &mut FakeHost, label: &str) {
    host.send_action(keyboard_page(), &capture_id(label), ActionKind::Activated, ActionValue::None);
    host.settle();
}

/// One row of the "Action names, command ids and presets" table.
#[derive(Debug, Clone)]
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

pub fn action_rows() -> Vec<ActionRow> {
    let page = page_text();
    let start = page.find("| Action name |").expect("action table");
    page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<String> = line.trim_matches('|').split('|').map(unquote).collect();
            ActionRow {
                name: cells[0].clone(),
                command: cells[1].clone(),
                default: cells[2].clone(),
                screen: cells[3].clone(),
                tmux: cells[4].clone(),
            }
        })
        .collect()
}

/// Which column of the table a host is seeded from.
#[derive(Clone, Copy)]
pub enum Column {
    Default,
    Screen,
    Tmux,
}

impl Column {
    pub fn key<'a>(self, row: &'a ActionRow) -> &'a str {
        match self {
            Column::Default => &row.default,
            Column::Screen => &row.screen,
            Column::Tmux => &row.tmux,
        }
    }
}

/// A host whose keymap holds the 35 actions with the keys of `column`; the label of each binding
/// is the action name and the display label of its chord is the key itself.
pub fn column_host(column: Column) -> FakeHost {
    let (base, preset) = match column {
        Column::Screen => ("ctrl+a", "screen"),
        _ => ("ctrl+b", "tmux"),
    };
    let mut host = host_with_tab(json!({"shortcut_base": base, "navigation_shortcut_base": "ctrl+b", "preset": preset}));
    let mut bindings = Vec::new();
    let mut labels: Vec<(String, String)> = Vec::new();
    for row in action_rows() {
        let key = column.key(&row).to_string();
        if key == "unbound" {
            continue;
        }
        let chord = format!("prefix+{key}");
        bindings.push(Binding {
            command: row.command.clone(),
            chords: vec![chord.clone()],
            context: "Keyboard".to_string(),
            is_default: true,
        });
        labels.push((chord, key));
    }
    let label_refs: Vec<(&str, &str)> = labels.iter().map(|(chord, label)| (chord.as_str(), label.as_str())).collect();
    set_keymap(&mut host, bindings, &label_refs);
    host
}

pub fn normalized(text: &str) -> String {
    text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim_end_matches('\n').to_string()
}
