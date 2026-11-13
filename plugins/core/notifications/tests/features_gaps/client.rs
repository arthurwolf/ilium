//! The client side of `ilium.core.notifications`: toast commands and the Sound tab renders.

use ilium_plugin_core_notifications::NotificationsClient;
use ilium_plugin_sdk::testing::{assert_golden, FakeHost};
use ilium_plugin_sdk::types::{
    CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent, NodeId, ViewTarget,
};

use crate::common::*;

fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<NotificationsClient>();
    host
}

fn toast_command(
    host: &mut FakeHost,
    command: &str,
    node: NodeId,
) -> Vec<ilium_plugin_sdk::types::Submission> {
    let out = host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Toast,
        context: CommandContext {
            pane: Some(node),
            view: None,
            buffer: None,
            line: None,
            virtual_row: None,
            menu_item: None,
        },
    }));
    host.settle();
    out
}

fn golden_path(name: &str) -> String {
    workspace_root()
        .join("plugins/core/notifications/tests/features/golden")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// REQ: F-048
#[test]
fn client_048_the_dismiss_button_dismisses_the_toast_of_its_pane() {
    let mut host = client_host();
    let submissions = toast_command(
        &mut host,
        "ilium.core.notifications/dismiss-toast",
        NodeId(9),
    );
    let dismissed: Vec<String> = submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::DismissToast(id) => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(dismissed, ["finished-9"]);
}

/// REQ: F-048
#[test]
fn client_048_the_open_pane_button_dismisses_nothing() {
    let mut host = client_host();
    let submissions = toast_command(&mut host, "ilium.core.notifications/open-pane", NodeId(9));
    assert!(!submissions
        .iter()
        .any(|submission| matches!(submission.intent, Intent::DismissToast(_))));
}

/// REQ: F-048
#[test]
fn client_048_an_unrelated_command_is_ignored() {
    let mut host = client_host();
    assert!(toast_command(&mut host, "ilium.core.other/anything", NodeId(9)).is_empty());
}

/// REQ: F-047
#[test]
fn client_047_the_sounds_tab_renders_the_page_figure_at_48_columns() {
    let mut host = client_host();
    let screen = host.render(ViewTarget::SettingsPage("sound".to_string()), 48, 7);
    assert_golden(&golden_path("sounds_48.txt"), &screen.text());
}

/// REQ: F-047
#[test]
fn client_047_the_sounds_tab_shows_every_row_label_at_widths_40_80_and_120() {
    for width in [40u16, 80, 120] {
        let mut host = client_host();
        let screen = host.render(ViewTarget::SettingsPage("sound".to_string()), width, 24);
        let text = screen.text();
        for label in [
            "Approval needed",
            "Agent finished",
            "Task done",
            "Desktop notifications",
            "Merge task alerts within",
        ] {
            assert!(text.contains(label), "{label} at {width}");
        }
        assert!(
            text.lines()
                .all(|line| line.chars().count() <= usize::from(width)),
            "width {width}"
        );
    }
}

/// REQ: F-047
#[test]
fn client_047_the_sounds_tab_rows_keep_their_order_at_80_columns() {
    let mut host = client_host();
    let screen = host.render(ViewTarget::SettingsPage("sound".to_string()), 80, 24);
    let order = [
        "Approval needed",
        "Agent finished",
        "Task done",
        "Desktop notifications",
        "Merge task alerts within",
    ];
    let rows: Vec<u16> = order
        .iter()
        .map(|label| screen.find(label).expect(label).0)
        .collect();
    assert!(rows.windows(2).all(|pair| pair[0] < pair[1]), "{rows:?}");
}

/// REQ: F-049
#[test]
fn client_049_the_merge_row_shows_the_default_of_30_seconds() {
    let mut host = client_host();
    let screen = host.render(ViewTarget::SettingsPage("sound".to_string()), 80, 24);
    let row = screen
        .text()
        .lines()
        .find(|line| line.contains("Merge task alerts within"))
        .expect("row")
        .to_string();
    assert!(
        row.trim_end()
            .trim_end_matches('│')
            .trim_end()
            .ends_with("30 s"),
        "{row}"
    );
}

/// REQ: F-047
#[test]
fn client_047_the_sounds_tab_connects_label_and_value_with_leader_dots() {
    let mut host = client_host();
    let screen = host.render(ViewTarget::SettingsPage("sound".to_string()), 120, 24);
    let row = screen
        .text()
        .lines()
        .find(|line| line.contains("Approval needed"))
        .expect("row")
        .to_string();
    assert!(row.contains('…'), "{row}");
}
