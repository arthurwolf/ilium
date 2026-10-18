//! The page itself: sections, samples in both languages, links and the traceability rows.

use ilium_plugin_sdk::exports::ServiceReply;
use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{
    Delivery, EditorOpenSpec, EditorPlacement, Event, FsPath, Intent, LinkActivatedEvent, NodeId,
    PaneTitleChangedEvent, SendInputSpec, TitleOrigin,
};

use crate::common::{client_host, intents, page, page_text, server_host, workspace_root};

/// REQ: F-013
#[test]
fn feature_013_traceability_row_lists_the_two_terminal_budget_settings() {
    let traceability = page("reference-feature-traceability.md");
    let row = traceability
        .lines()
        .find(|line| line.starts_with("| F-013 "))
        .expect("F-013 row");
    assert!(row.contains("[terminal] scrollback_budget_mib, engine_memory_budget_mib"));
    assert!(row.contains("kernel; terminal plugin"));
}

/// REQ: F-014
#[test]
fn feature_014_traceability_row_routes_input_through_terminal_write() {
    let traceability = page("reference-feature-traceability.md");
    let row = traceability
        .lines()
        .find(|line| line.starts_with("| F-014 "))
        .expect("F-014 row");
    assert!(row.contains("terminal.write"));
}

/// REQ: F-015
#[test]
fn feature_015_traceability_row_assigns_links_to_the_terminal_plugin_on_the_client() {
    let traceability = page("reference-feature-traceability.md");
    let row = traceability
        .lines()
        .find(|line| line.starts_with("| F-015 "))
        .expect("F-015 row");
    assert!(
        row.contains("terminal plugin")
            && row.contains("| client |")
            && row.contains("terminal decorations")
    );
}

/// REQ: F-017
#[test]
fn feature_017_traceability_row_uses_read_screen_and_write() {
    let traceability = page("reference-feature-traceability.md");
    let row = traceability
        .lines()
        .find(|line| line.starts_with("| F-017 "))
        .expect("F-017 row");
    assert!(row.contains("terminal.read-screen, terminal.write"));
}

/// REQ: COR-050
#[test]
fn cor_050_each_page_sample_exists_in_rust_and_typescript() {
    let text = page_text();
    for heading in [
        "#### Clickable links",
        "#### Pane title from the terminal",
        "#### Screen transfer between panes",
        "#### Live terminal in a panel",
        "#### Interrupt and watch exits",
        "#### Visible rows that changed",
    ] {
        let start = text
            .find(heading)
            .unwrap_or_else(|| panic!("{heading} missing"));
        let section = &text[start..];
        let end = section[heading.len()..]
            .find("\n#### ")
            .map_or(section.len(), |at| at + heading.len());
        let section = &section[..end];
        assert!(
            section.contains("**Rust**") && section.contains("**TypeScript**"),
            "{heading}"
        );
    }
}

/// REQ: COR-050
#[test]
fn cor_050_the_page_says_emulation_stays_in_the_kernel() {
    assert!(page_text().contains("Emulation stays in the kernel."));
}

/// REQ: COR-022
#[test]
fn cor_022_the_kernel_boot_page_says_what_is_lost_when_terminal_is_disabled() {
    let boot = page("reference-kernel-and-boot.md");
    let row = boot
        .lines()
        .find(|line| line.starts_with("| Terminal | No links"))
        .expect("terminal row");
    assert!(row.contains("No links, no context-menu extras, no history transfer"));
    assert!(row.contains("The kernel terminal still works"));
}

/// REQ: COR-050
#[test]
fn cor_050_the_kernel_boot_page_activates_terminal_on_the_first_frame() {
    let boot = page("reference-kernel-and-boot.md");
    let row = boot
        .lines()
        .find(|line| line.starts_with("| Terminal | `event:screen-changed`"))
        .expect("activation row");
    assert!(
        row.contains("`startup`")
            && row.contains("Links and the context menu need the first frame")
    );
}

/// REQ: F-015
#[test]
fn feature_015_link_sample_opens_the_editor_in_a_split() {
    let mut host = client_host();
    let event = Event::LinkActivated(LinkActivatedEvent {
        node: NodeId(7),
        detector_id: "path-line".to_string(),
        text: "src/server/routes.rs:42".to_string(),
    });
    let submissions = host.deliver(vec![event]);
    assert_eq!(
        intents(&submissions),
        vec![Intent::EditorOpen(EditorOpenSpec {
            path: FsPath {
                scope: "project".to_string(),
                path: "src/server/routes.rs".to_string()
            },
            line: Some(41),
            placement: EditorPlacement::Split,
        })]
    );
}

/// REQ: F-018
#[test]
fn feature_018_title_sample_ignores_everything_except_osc() {
    let mut host = server_host();
    for source in [TitleOrigin::Manual, TitleOrigin::Inferred] {
        let event = Event::PaneTitleChanged(PaneTitleChangedEvent {
            node: NodeId(7),
            title: "t".to_string(),
            source,
        });
        assert!(host.deliver(vec![event]).is_empty());
    }
}

/// REQ: F-017
#[test]
fn feature_017_transfer_sample_reads_500_rows_and_pastes() {
    let mut session = FakeSession::new();
    session.install_core("kanban");
    session
        .install(workspace_root().join("plugins/core/terminal"))
        .start();
    let rows: Vec<String> = (0..500).map(|row| format!("row {row}")).collect();
    let row_refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    session
        .server("ilium.core.terminal")
        .terminal()
        .set_screen(3, &row_refs, 1);
    let request = "{\"method\":\"send-screen\",\"source\":3,\"target\":4}";
    let reply = session.call_service(
        "ilium.core.kanban",
        "ilium.core.terminal/transfer",
        "^0.1",
        request,
    );
    assert!(matches!(reply, Ok(ServiceReply::Done(_))));
    session.pump().expect("pump");
    let sent = session.server("ilium.core.terminal").submitted();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].intent,
        Intent::SendInput(SendInputSpec {
            pane: NodeId(4),
            text: rows.join("\n"),
            delivery: Delivery::Pasted,
            prompt_epoch: None,
            stages: Vec::new(),
            raw: None,
            mouse: None,
            if_current: None,
            lock_user_input: false,
            journal: false,
        })
    );
}
