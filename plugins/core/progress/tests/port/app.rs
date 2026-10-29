//! Footer behaviour in the application shell (original ilium-client/src/app.rs).

use ilium_plugin_sdk::types::Intent;
use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/app.rs::completed_progress_expiry_hides_text_without_resizing_or_losing_result
#[test]
fn port_061_an_expired_footer_hides_its_text_sends_nothing_and_shows_again_for_a_new_monitor() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![monitor_with(
            1,
            JOB,
            "done",
            100.0,
            "Build finished",
            "",
            Value::Null,
        )],
    );
    let before = host.submitted().len();
    pass(&mut host, 61_000);
    assert!(!footer(&mut host, 80, 12).contains("Build finished"));
    let sent = host.submitted();
    assert!(
        sent[before..]
            .iter()
            .all(|s| !matches!(s.intent, Intent::SendInput(_))),
        "nothing typed on expiry"
    );
    show(
        &mut host,
        vec![
            monitor_with(1, JOB, "done", 100.0, "Build finished", "", Value::Null),
            monitor_with(
                2,
                "next-job",
                "running",
                5.0,
                "Next job running",
                "",
                Value::Null,
            ),
        ],
    );
    assert!(footer(&mut host, 80, 12).contains("Next job running"));
}

/// REQ: PORT:ilium-client/src/app.rs::hovering_the_progress_footer_shows_the_long_description_tooltip
#[test]
fn port_061_hovering_the_footer_shows_the_long_details_as_a_tooltip() {
    let mut host = activated_client();
    let details = "What: compile the app for release\nWhy: you asked for a deployable build\nNext: install and verify it";
    show(
        &mut host,
        vec![monitor_with(
            1,
            JOB,
            "running",
            40.0,
            "Building the release binaries - crate 4 of 10",
            details,
            Value::Null,
        )],
    );
    let tooltip = hover(&mut host, 2, 1).expect("tooltip");
    assert!(
        tooltip.contains("What: compile the app for release")
            && tooltip.contains("Next: install and verify it"),
        "{tooltip}"
    );
}

/// REQ: PORT:ilium-client/src/app.rs::progress_fill_style_row_changes_and_persists_the_frame_family
#[test]
fn port_061_the_frame_family_setting_takes_the_blocks_preset_and_reads_back() {
    let mut host = activated_client();
    let blocks = json!([
        "\u{2581}", "\u{2582}", "\u{2583}", "\u{2584}", "\u{2585}", "\u{2586}", "\u{2587}",
        "\u{2588}"
    ]);
    set_kernel_setting(&mut host, "ui.task_progress_frames", blocks.clone()).expect("accepted");
    assert_eq!(host.settings().get("ui.task_progress_frames"), Some(blocks));
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::completed_progress_footer_expires_during_continuous_pty_output
#[test]
fn port_061_a_done_footer_expires_at_three_seconds_while_other_events_keep_arriving() {
    let mut host = activated_client();
    set_kernel_setting(
        &mut host,
        "ui.completed_progress_hide_after_seconds",
        json!(3),
    )
    .expect("accepted");
    show(
        &mut host,
        vec![monitor_with(
            1,
            JOB,
            "done",
            100.0,
            "Smoke done",
            "",
            Value::Null,
        )],
    );
    for _ in 0..2 {
        pass(&mut host, 1_000);
        show(
            &mut host,
            vec![monitor_with(
                1,
                JOB,
                "done",
                100.0,
                "Smoke done",
                "",
                Value::Null,
            )],
        );
    }
    assert!(footer(&mut host, 80, 12).contains("Smoke done"));
    pass(&mut host, 1_000);
    assert!(!footer(&mut host, 80, 12).contains("Smoke done"));
}
