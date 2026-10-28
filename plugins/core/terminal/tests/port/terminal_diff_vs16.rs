//! Twin of ilium-client/tests/terminal_diff_vs16.rs: the render diff itself stays in the kernel;
//! the plugin-visible part is that VS16 icon text in a terminal title survives unchanged.

use ilium_plugin_core_terminal::TerminalServer;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    Event, Intent, NodeId, PaneTitleChangedEvent, RenameNodeSpec, TitleOrigin, TitleSource,
};

/// REQ: PORT:ilium-client/tests/terminal_diff_vs16.rs::width_transition_preserves_text_after_vs16_icons
#[test]
fn port_vs16_title_with_icons_is_preserved_verbatim() {
    let mut host = FakeHost::for_manifest(concat!(env!("CARGO_MANIFEST_DIR"), "/manifest.json"))
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<TerminalServer>();
    let title = "🗂️ ledger-desk 🖥️ Ledger";
    let event = Event::PaneTitleChanged(PaneTitleChangedEvent {
        node: NodeId(7),
        title: title.to_string(),
        source: TitleOrigin::Osc,
    });
    let intents: Vec<Intent> = host
        .deliver(vec![event])
        .into_iter()
        .map(|s| s.intent)
        .collect();
    assert_eq!(
        intents,
        vec![Intent::RenameNode(RenameNodeSpec {
            node: NodeId(7),
            title: title.to_string(),
            source: TitleSource::Inferred,
            expected_revision: None
        })]
    );
}

/// REQ: PORT:ilium-client/tests/terminal_diff_vs16.rs::width_transition_preserves_text_after_vs16_icons
#[test]
fn port_vs16_title_keeps_spaces_and_a_trailing_icon_verbatim() {
    let mut host = FakeHost::for_manifest(concat!(env!("CARGO_MANIFEST_DIR"), "/manifest.json"))
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<TerminalServer>();
    let title = "build  ✔️ done ⚠️";
    let event = Event::PaneTitleChanged(PaneTitleChangedEvent {
        node: NodeId(8),
        title: title.to_string(),
        source: TitleOrigin::Osc,
    });
    let intents: Vec<Intent> = host
        .deliver(vec![event])
        .into_iter()
        .map(|s| s.intent)
        .collect();
    assert_eq!(
        intents,
        vec![Intent::RenameNode(RenameNodeSpec {
            node: NodeId(8),
            title: title.to_string(),
            source: TitleSource::Inferred,
            expected_revision: None
        })]
    );
}
