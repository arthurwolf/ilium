//! Client-side input of Smart Copy: the pointer claims of terminal panes (mouse selection and the
//! Smart Copy light gesture), the terminal right-click menu and the `copy-path` and `transfer`
//! commands (manual page core-plugin-smart-copy.md, "Terminal right-click menu").

use ilium_plugin_sdk::generated::host::{HostClipboard, HostKernelSettings, HostTree};
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::primitives::{HostTerminal, ScreenPart};
use ilium_plugin_sdk::types::{
    ErrorCode, NodeId, NodeQuery, PointerModifiers, PrimitiveKind,
};
use ilium_plugin_sdk::generated::exports::menu_provider::MenuEntry;
use ilium_plugin_sdk::HostApi;
use serde_json::Value;

/// The content kind of an ordinary terminal pane.
const TERMINAL_CONTENT_KIND: &str = "ilium.terminal";
/// Most rows `transfer` reads (the pane reader's own bound).
const TRANSFER_ROWS: u32 = 10_000;

/// What the settings say about the pointer claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClaimPolicy {
    /// `[ui] terminal_text_selection_enabled`: a whole-pane claim for mouse selection.
    pub selection: bool,
    /// `[terminal] smart_copy_light`: a gesture claim with the light modifier.
    pub light: bool,
    /// The modifier of `[terminal] smart_copy_light_key`.
    pub modifier: PointerModifiers,
}

impl Default for ClaimPolicy {
    fn default() -> Self {
        Self { selection: true, light: true, modifier: PointerModifiers::CONTROL }
    }
}

/// The modifier of a `smart_copy_light_key` value; aliases and case follow the config loader.
pub(crate) fn light_modifier(value: &str) -> Option<PointerModifiers> {
    match value.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(PointerModifiers::CONTROL),
        "alt" | "option" => Some(PointerModifiers::ALT),
        "shift" => Some(PointerModifiers::SHIFT),
        _ => None,
    }
}

fn tab_values(api: &mut dyn HostApi, tab: &str) -> Option<Value> {
    let text = HostKernelSettings::read(api, tab.to_owned()).ok()?;
    serde_json::from_str(&text).ok()
}

/// Reads the three settings; an unreadable tab or a missing key keeps its documented default.
pub(crate) fn read_policy(api: &mut dyn HostApi) -> ClaimPolicy {
    let mut policy = ClaimPolicy::default();
    if let Some(flag) = tab_values(api, "user-interface")
        .and_then(|values| values.get("terminal_text_selection_enabled")?.as_bool())
    {
        policy.selection = flag;
    }
    if let Some(values) = tab_values(api, "terminal") {
        if let Some(flag) = values.get("smart_copy_light").and_then(Value::as_bool) {
            policy.light = flag;
        }
        if let Some(modifier) = values
            .get("smart_copy_light_key")
            .and_then(Value::as_str)
            .and_then(light_modifier)
        {
            policy.modifier = modifier;
        }
    }
    policy
}

/// The terminal panes the plugin has claimed (or would claim), so a settings change reaches them.
#[derive(Default)]
pub(crate) struct Claims {
    panes: Vec<u64>,
}

impl Claims {
    /// Claims every terminal pane that exists now (panes restored before the plugin activated).
    pub(crate) fn adopt_existing(&mut self, api: &mut dyn HostApi) {
        let query = NodeQuery {
            content_kind: Some(TERMINAL_CONTENT_KIND.to_owned()),
            ..NodeQuery::default()
        };
        for node in HostTree::find(api, query) {
            self.track(api, node.0);
        }
    }

    fn track(&mut self, api: &mut dyn HostApi, pane: u64) {
        if !self.panes.contains(&pane) {
            self.panes.push(pane);
        }
        let policy = read_policy(api);
        apply(api, pane, policy);
    }

    /// Reacts to one event: a new terminal pane, a closed pane or a changed setting.
    pub(crate) fn on_event(&mut self, api: &mut dyn HostApi, event: &Event) {
        match event {
            Event::PaneCreated(info) if info.primitive == PrimitiveKind::Terminal => {
                self.track(api, info.node.0);
            }
            Event::PaneExited(exited) => {
                self.panes.retain(|pane| *pane != exited.node.0);
            }
            Event::KernelSettingsChanged(changed)
                if matches!(changed.tab.as_str(), "ui" | "user-interface" | "terminal") =>
            {
                let policy = read_policy(api);
                for pane in self.panes.clone() {
                    apply(api, pane, policy);
                }
            }
            _ => {}
        }
    }
}

/// Replaces the plugin's claims on a pane with the ones the policy asks for.
fn apply(api: &mut dyn HostApi, pane: u64, policy: ClaimPolicy) {
    let node = NodeId(pane);
    HostTerminal::release_pointer(api, node);
    if policy.selection {
        let _ = HostTerminal::claim_pointer(api, node);
    }
    if policy.light {
        let _ = HostTerminal::claim_pointer_gesture(api, node, policy.modifier);
    }
}

/// The entries of the terminal right-click menu of a pane (none without a pane).
pub(crate) fn menu_items(api: &mut dyn HostApi, pane: Option<NodeId>) -> Vec<MenuEntry> {
    let Some(pane) = pane else {
        return Vec::new();
    };
    let selected = HostTerminal::get_selection(api, pane).is_some_and(|selection| !selection.text.is_empty());
    let mut entries = vec![
        entry("copy", "Copy", None, false),
        entry("copy-path", "Copy path", None, false),
        entry("smart-copy", "Smart Copy", Some("ctrl+shift+c"), false),
        entry("transfer", "Transfer screen", None, true),
    ];
    if !selected {
        entries[0].disabled_reason = Some("nothing selected".to_owned());
    }
    entries
}

fn entry(id: &str, label: &str, shortcut: Option<&str>, separator_before: bool) -> MenuEntry {
    MenuEntry {
        id: id.to_owned(),
        label: label.to_owned(),
        icon: None,
        shortcut: shortcut.map(str::to_owned),
        parent: None,
        is_submenu: false,
        separator_before,
        checked: None,
        disabled_reason: None,
        tooltip: None,
        danger: false,
    }
}

/// Copies the pane's working directory; the status line says what happened.
pub(crate) fn copy_path(api: &mut dyn HostApi, pane: NodeId) -> String {
    let Some(path) = HostTree::get(api, pane).and_then(|node| node.cwd) else {
        return "No path known for this pane".to_owned();
    };
    write_clipboard(api, path, "path")
}

/// Copies the visible screen of the pane as text.
pub(crate) fn transfer_screen(api: &mut dyn HostApi, pane: NodeId) -> String {
    let Some(screen) = HostTerminal::read_screen(api, pane, ScreenPart::Visible, TRANSFER_ROWS) else {
        return "Screen unavailable".to_owned();
    };
    let text = screen.rows.iter().map(|row| row.trim_end()).collect::<Vec<_>>().join("\n");
    write_clipboard(api, text, "screen")
}

fn write_clipboard(api: &mut dyn HostApi, text: String, what: &str) -> String {
    let characters = text.chars().count();
    match HostClipboard::write_text(api, text) {
        Ok(()) => format!("Copied {what} ({characters} characters)"),
        Err(error) if error.code == ErrorCode::NotPermitted => "Clipboard not permitted".to_owned(),
        Err(error) => format!("Copy failed: {}", error.message),
    }
}
