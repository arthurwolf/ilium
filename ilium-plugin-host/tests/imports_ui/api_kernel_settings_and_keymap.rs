//! `ilium:host/kernel-settings` and `ilium:host/keymap` (api-kernel-settings-and-keymap.md).

use ilium_plugin_api::generated::host::kernel_settings::HostKernelSettings;
use ilium_plugin_api::generated::host::keymap::{Binding, HostKeymap};
use ilium_plugin_api::ErrorCode;
use proptest::prelude::*;

use crate::common::*;

const GRANTS: &[&str] = &["settings.read-kernel:*", "keymap.read"];

const TAB_IDS: [&str; 25] = [
    "user-interface",
    "icons",
    "agent-monitoring",
    "agent-cost",
    "optimization",
    "remote-compaction",
    "keyboard",
    "terminal",
    "editor",
    "session",
    "git",
    "kanban-board",
    "animations",
    "sound",
    "voice-control",
    "reset-planning",
    "inference",
    "llm-instructions",
    "titles",
    "triggers",
    "text-triggers",
    "debug",
    "api",
    "about",
    "setup",
];

const WRITE_GRANTS: &[&str] = &["settings.read-kernel:*", "settings.write-kernel:terminal"];

/// REQ: WIT:ilium:host/kernel-settings.write
#[test]
fn wit_kernel_settings_write_sets_one_key_and_reads_it_back() {
    let rig = rig(WRITE_GRANTS);
    rig.kernel
        .set_kernel_tab("terminal", "{\"scrollback\":5000}");
    rig.host
        .kernel_settings()
        .write(
            "terminal".to_owned(),
            "scrollback".to_owned(),
            "6000".to_owned(),
        )
        .expect("writes");
    let json = rig
        .host
        .kernel_settings()
        .read("terminal".to_owned())
        .expect("reads");
    assert!(
        json.contains("\"scrollback\":6000") || json.contains("\"scrollback\": 6000"),
        "{json}"
    );
}

/// REQ: WIT:ilium:host/kernel-settings.read
#[test]
fn wit_kernel_settings_read_returns_a_json_object() {
    let rig = rig(GRANTS);
    rig.kernel
        .set_kernel_tab("terminal", "{\"scrollback\":5000}");
    let json = rig
        .host
        .kernel_settings()
        .read("terminal".to_owned())
        .expect("reads");
    assert!(
        json.contains("\"scrollback\":5000") || json.contains("\"scrollback\": 5000"),
        "{json}"
    );
}

/// REQ: WIT:ilium:host/kernel-settings.read
#[test]
fn wit_kernel_settings_read_every_documented_tab_id_resolves() {
    let rig = rig(GRANTS);
    for tab in TAB_IDS {
        rig.kernel.set_kernel_tab(tab, "{}");
        let json = rig
            .host
            .kernel_settings()
            .read(tab.to_owned())
            .unwrap_or_else(|e| panic!("{tab}: {e:?}"));
        assert!(json.trim_start().starts_with('{'), "{tab}: {json}");
    }
}

/// REQ: WIT:ilium:host/kernel-settings.read
#[test]
fn wit_kernel_settings_read_unknown_tab_not_found() {
    let rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.kernel_settings().read("no-such-tab".to_owned())),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/kernel-settings.read
#[test]
fn wit_kernel_settings_read_secrets_are_redacted() {
    let rig = rig(GRANTS);
    rig.kernel
        .set_kernel_tab_secret("inference", "api_key", "sk-live-123");
    let json = rig
        .host
        .kernel_settings()
        .read("inference".to_owned())
        .expect("reads");
    assert!(json.contains("<redacted>"), "{json}");
    assert!(!json.contains("sk-live-123"), "{json}");
}

/// REQ: WIT:ilium:host/kernel-settings.read
#[test]
fn wit_kernel_settings_read_declined_grant_not_permitted() {
    let rig = declined_rig(&["settings.read-kernel:*"]);
    assert_eq!(
        code_of(rig.host.kernel_settings().read("terminal".to_owned())),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/keymap.bindings
#[test]
fn wit_keymap_bindings_lists_commands_with_their_chords() {
    let rig = rig(GRANTS);
    rig.kernel.set_keymap(vec![Binding {
        command: "example.notes/new-note".to_owned(),
        chords: vec!["ctrl+n".to_owned(), "ctrl+shift+n".to_owned()],
        context: "global".to_owned(),
        is_default: true,
    }]);
    let bindings = rig.host.keymap().bindings();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].chords, vec!["ctrl+n", "ctrl+shift+n"]);
    assert!(bindings[0].is_default);
}

/// REQ: WIT:ilium:host/keymap.bindings
#[test]
fn wit_keymap_bindings_empty_keymap_is_empty() {
    let rig = rig(GRANTS);
    rig.kernel.set_keymap(Vec::new());
    assert!(rig.host.keymap().bindings().is_empty());
}

/// REQ: WIT:ilium:host/keymap.bindings
#[test]
fn wit_keymap_bindings_declined_grant_is_empty() {
    let rig = declined_rig(&["keymap.read"]);
    rig.kernel.set_keymap(vec![Binding::default()]);
    assert!(rig.host.keymap().bindings().is_empty());
}

/// REQ: WIT:ilium:host/keymap.chord-label
#[test]
fn wit_keymap_chord_label_capitalises_modifiers_and_key() {
    let rig = rig(GRANTS);
    assert_eq!(
        rig.host.keymap().chord_label("ctrl+shift+k".to_owned()),
        "Ctrl+Shift+K"
    );
}

/// REQ: WIT:ilium:host/keymap.chord-label
#[test]
fn wit_keymap_chord_label_single_key() {
    let rig = rig(GRANTS);
    assert_eq!(rig.host.keymap().chord_label("f5".to_owned()), "F5");
}

/// REQ: WIT:ilium:host/keymap.chord-label
#[test]
fn wit_keymap_chord_label_is_idempotent_on_its_own_output() {
    let rig = rig(GRANTS);
    let once = rig.host.keymap().chord_label("alt+enter".to_owned());
    assert_eq!(rig.host.keymap().chord_label(once.clone()), once);
}

proptest! {
    // REQ: WIT:ilium:host/keymap.chord-label
    #[test]
    fn wit_keymap_chord_label_keeps_one_segment_per_chord_part(
        parts in proptest::collection::vec("[a-z][a-z0-9]{0,5}", 1..4)
    ) {
        let rig = rig(GRANTS);
        let label = rig.host.keymap().chord_label(parts.join("+"));
        prop_assert_eq!(label.split('+').count(), parts.len());
    }
}

/// REQ: WIT:ilium:host/kernel-settings.write
#[test]
fn wit_kernel_settings_write_without_the_tab_write_grant_is_refused() {
    let rig = rig(&["settings.read-kernel:*"]);
    rig.kernel
        .set_kernel_tab("terminal", "{\"scrollback\":5000}");
    let result = rig.host.kernel_settings().write(
        "terminal".to_owned(),
        "scrollback".to_owned(),
        "6000".to_owned(),
    );
    assert!(
        result.is_err(),
        "a plugin without settings.write-kernel for the tab must be refused"
    );
}
