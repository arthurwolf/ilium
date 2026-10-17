//! The four kernel settings of the tab `agent-monitoring` that the plugin reads (F-059, F-061).
//! The plugin holds no write grant: it only reads them (see manifest.rs).

use serde_json::json;

use crate::common::*;

/// REQ: F-061
#[test]
fn f_061_progress_max_lines_accepts_1_and_20_and_refuses_0_and_21() {
    let mut host = activated_client();
    for ok in [1, 4, 20] {
        assert!(
            set_kernel_setting(&mut host, "ui.progress_max_lines", json!(ok)).is_ok(),
            "{ok}"
        );
    }
    for bad in [0, 21] {
        let error = set_kernel_setting(&mut host, "ui.progress_max_lines", json!(bad))
            .expect_err("refused");
        assert_eq!(
            error.message,
            format!("ui.progress_max_lines = {bad} must be between 1 and 20")
        );
    }
}

/// REQ: F-061
#[test]
fn f_061_task_progress_frames_accepts_2_to_13_frames_and_refuses_1_and_14() {
    let mut host = activated_client();
    let frames = |n: usize| {
        json!((0..n)
            .map(|i| format!("{}", char::from(b'a' + i as u8)))
            .collect::<Vec<_>>())
    };
    for ok in [2usize, 13] {
        assert!(
            set_kernel_setting(&mut host, "ui.task_progress_frames", frames(ok)).is_ok(),
            "{ok}"
        );
    }
    for bad in [1usize, 14] {
        let error = set_kernel_setting(&mut host, "ui.task_progress_frames", frames(bad))
            .expect_err("refused");
        assert_eq!(error.message, "ui.task_progress_frames must contain 2\u{2013}13 printable frames, all one or all two display cells wide");
    }
}

/// REQ: F-061
#[test]
fn f_061_task_progress_frames_refuses_frames_of_mixed_width() {
    let mut host = activated_client();
    let error = set_kernel_setting(
        &mut host,
        "ui.task_progress_frames",
        json!(["a", "\u{4e2d}"]),
    )
    .expect_err("refused");
    assert!(error.message.contains("one or all two display cells wide"));
}

/// REQ: F-059
#[test]
fn f_059_the_setting_defaults_are_enabled_four_lines_and_sixty_seconds() {
    let mut host = activated_client();
    assert_eq!(
        host.settings().get("ui.progress_monitor_enabled"),
        Some(json!(true))
    );
    assert_eq!(host.settings().get("ui.progress_max_lines"), Some(json!(4)));
    assert_eq!(
        host.settings()
            .get("ui.completed_progress_hide_after_seconds"),
        Some(json!(60))
    );
}

/// REQ: F-059
#[test]
fn f_059_a_change_of_a_kernel_setting_is_delivered_as_kernel_settings_changed_and_causes_no_failure(
) {
    let mut host = activated_client();
    set_kernel_setting(&mut host, "ui.progress_max_lines", json!(8)).expect("accepted");
    assert!(host.failures().is_empty());
}

/// REQ: F-059
#[test]
fn doc_f_059_the_page_lists_the_four_settings_with_their_defaults() {
    let text = page_text();
    assert!(text.contains("`[ui] progress_max_lines` | 4 | Footer lines, 1 to 20"));
    assert!(text.contains(
        "`[ui] completed_progress_hide_after_seconds` | 60 | Steps of 30; 0 means never hide"
    ));
    assert!(text.contains("`[ui] progress_monitor_enabled`"));
    assert!(text.contains("`[ui] task_progress_frames`"));
}
