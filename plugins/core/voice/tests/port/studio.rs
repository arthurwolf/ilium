//! Twins of `ilium-client/src/onboarding/studio.rs` and `studio_ui.rs` (the sound studio of the onboarding
//! flow). The studio is assumed to be the onboarding step `sound` of this plugin; its numbers are the kernel
//! settings `design.<control>` of the tab `sound` (pulse rate in tenths of a hertz) and its preview is a job of
//! kind `sound-preview`. See API_GAPS_svc01.md.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};
use serde_json::{json, Value};

use crate::common::client_host;

const PREVIEW_JOB: &str = "sound-preview";
const NUMERIC_CONTROLS: [(&str, &str); 3] = [
    ("pitch_hz", "Pitch"),
    ("sweep", "Sweep"),
    ("pulse_rate", "Pulse rate"),
];

fn step() -> ViewTarget {
    ViewTarget::OnboardingStep("sound".to_string())
}

fn studio_text(host: &mut FakeHost, columns: u16, rows: u16) -> String {
    host.render(step(), columns, rows).text()
}

fn design(host: &mut FakeHost, key: &str) -> Option<Value> {
    host.kernel_settings()
        .get("sound", &format!("design.{key}"))
}

fn enter(host: &mut FakeHost, key: &str, text: &str) {
    host.send_action(
        step(),
        key,
        ActionKind::Submitted,
        ActionValue::Text(text.to_string()),
    );
    host.settle();
}

fn adjust(host: &mut FakeHost, key: &str, value: f64) {
    host.send_action(step(), key, ActionKind::Changed, ActionValue::Number(value));
    host.settle();
}

fn preview_jobs(host: &mut FakeHost) -> Vec<ilium_plugin_sdk::testing::fakes::jobs::JobRecord> {
    host.jobs()
        .list()
        .into_iter()
        .filter(|job| job.kind == PREVIEW_JOB)
        .collect()
}

fn request_preview(host: &mut FakeHost) -> Value {
    host.send_action(step(), "preview", ActionKind::Activated, ActionValue::None);
    host.settle();
    let job = preview_jobs(host).pop().expect("a preview job was started");
    serde_json::from_slice(&job.input).expect("the preview input is JSON")
}

/// Test hook of the studio model: sets its preview revision counter (assumed, API_GAPS_svc01.md item 3).
trait StudioHostExt {
    fn set_preview_revision(&mut self, revision: u64);
}

impl StudioHostExt for FakeHost {
    fn set_preview_revision(&mut self, _revision: u64) {
        unimplemented!("voice.studio-set-preview-revision")
    }
}

/// REQ: PORT:ilium-client/src/onboarding/studio.rs::exact_studio_entries_preserve_non_step_values_and_reject_invalid_input
#[test]
fn port_001_exact_studio_entries_keep_non_step_values_and_reject_invalid_input() {
    let mut host = client_host();
    enter(&mut host, "pitch_hz", "0731");
    assert_eq!(design(&mut host, "pitch_hz"), Some(json!(731)));
    enter(&mut host, "sweep", "-173");
    assert_eq!(design(&mut host, "sweep"), Some(json!(-173)));
    enter(&mut host, "pulse_rate", "1.7");
    assert_eq!(design(&mut host, "pulse_rate"), Some(json!(17)));
    enter(&mut host, "pulse_rate", "16.0");
    assert_eq!(design(&mut host, "pulse_rate"), Some(json!(160)));
    for text in ["1.75", "NaN", "inf", "16.1", "-0.1"] {
        enter(&mut host, "pulse_rate", text);
        assert_eq!(
            design(&mut host, "pulse_rate"),
            Some(json!(160)),
            "{text} must leave 16.0 unchanged"
        );
    }
    let waveform = design(&mut host, "waveform");
    enter(&mut host, "waveform", "1");
    assert_eq!(
        design(&mut host, "waveform"),
        waveform,
        "a waveform is chosen, never typed"
    );
    enter(&mut host, "pulse_rate", "0.0");
    assert_eq!(design(&mut host, "pulse_rate"), Some(json!(0)));
    for (key, _label) in NUMERIC_CONTROLS {
        let before = design(&mut host, key);
        enter(&mut host, key, "99999999999999999999999999999999999999999");
        assert_eq!(
            design(&mut host, key),
            before,
            "{key} rejects a huge number"
        );
    }
}

/// REQ: PORT:ilium-client/src/onboarding/studio.rs::sound_preview_install_rejects_stale_revision_and_replaced_studio
#[test]
fn port_002_a_preview_result_is_installed_only_for_the_current_revision_and_studio() {
    let mut host = client_host();
    let old = request_preview(&mut host);
    let old_request = preview_jobs(&mut host).pop().expect("old job").request;
    adjust(&mut host, "pitch_hz", 801.0);
    let current = request_preview(&mut host);
    let current_request = preview_jobs(&mut host).pop().expect("current job").request;
    assert_ne!(old["revision"], current["revision"]);

    host.jobs().finish(old_request, Ok(vec![0u8; 64]));
    host.settle();
    assert!(
        !studio_text(&mut host, 118, 33).contains("Preview ready"),
        "a stale revision is rejected"
    );

    host.jobs().finish(current_request, Ok(vec![0u8; 64]));
    host.settle();
    assert!(
        studio_text(&mut host, 118, 33).contains("Preview ready"),
        "the current revision is accepted"
    );

    // A replaced studio (the step closed and opened again) refuses the earlier studio's preview.
    let before_close = request_preview(&mut host);
    let before_close_request = preview_jobs(&mut host).pop().expect("job").request;
    host.send_action(step(), "sound", ActionKind::Closed, ActionValue::None);
    host.settle();
    let replacement = request_preview(&mut host);
    assert_ne!(before_close["studio"], replacement["studio"]);
    host.jobs().finish(before_close_request, Ok(vec![0u8; 64]));
    host.settle();
    assert!(!studio_text(&mut host, 118, 33).contains("Preview ready"));
}

/// REQ: PORT:ilium-client/src/onboarding/studio.rs::sound_preview_is_not_synthesized_during_interactive_model_changes
#[test]
fn port_003_no_preview_is_synthesized_while_the_model_changes() {
    let mut host = client_host();
    assert!(preview_jobs(&mut host).is_empty());
    let initial = request_preview(&mut host);
    adjust(&mut host, "pitch_hz", 801.0);
    adjust(&mut host, "pitch_hz", 802.0);
    assert_eq!(
        preview_jobs(&mut host).len(),
        1,
        "changes alone start no synthesis"
    );
    let current = request_preview(&mut host);
    assert_ne!(current["revision"], initial["revision"]);
    assert_eq!(preview_jobs(&mut host).len(), 2);
}

/// REQ: PORT:ilium-client/src/onboarding/studio.rs::sound_preview_revision_wrap_rotates_studio_identity
#[test]
fn port_004_a_revision_counter_wrap_rotates_the_studio_identity() {
    let mut host = client_host();
    let before = request_preview(&mut host);
    host.set_preview_revision(u64::MAX);
    adjust(&mut host, "pitch_hz", 801.0);
    let after = request_preview(&mut host);
    assert_eq!(after["revision"], 0);
    assert_ne!(before["studio"], after["studio"]);
}

fn busy_start() -> ilium_plugin_sdk::types::PluginError {
    ilium_plugin_sdk::types::PluginError {
        code: ilium_plugin_sdk::types::ErrorCode::Busy,
        message: "jobs full".to_string(),
        retry_after_ms: Some(50),
    }
}

/// REQ: PORT:ilium-client/src/app.rs::sound_preview_refusal_retry_reports_only_admitted_request
#[test]
fn port_005_a_refused_preview_start_reports_no_request_and_the_retry_starts_exactly_one() {
    let mut host = client_host();
    host.inject_failure("jobs.start", busy_start());
    host.send_action(step(), "preview", ActionKind::Activated, ActionValue::None);
    host.settle();
    assert_eq!(
        preview_jobs(&mut host).len(),
        0,
        "the refused start admitted nothing"
    );
    assert!(!studio_text(&mut host, 118, 33).contains("Preview ready"));
    host.send_action(step(), "preview", ActionKind::Activated, ActionValue::None);
    host.settle();
    assert_eq!(
        preview_jobs(&mut host).len(),
        1,
        "only the retry was admitted"
    );
}

/// REQ: PORT:ilium-client/src/render_cache.rs::sound_preview_status_changes_only_after_server_completion
#[test]
fn port_006_the_preview_status_changes_only_when_the_job_completes() {
    let mut host = client_host();
    request_preview(&mut host);
    assert!(
        !studio_text(&mut host, 118, 33).contains("Preview ready"),
        "requested is not ready"
    );
    let failed = preview_jobs(&mut host).pop().expect("job").request;
    host.jobs().finish(failed, Err(busy_start()));
    host.settle();
    assert!(
        !studio_text(&mut host, 118, 33).contains("Preview ready"),
        "a failed job is not ready"
    );
    request_preview(&mut host);
    let played = preview_jobs(&mut host).pop().expect("second job").request;
    host.jobs().finish(played, Ok(vec![0u8; 64]));
    host.settle();
    assert!(studio_text(&mut host, 118, 33).contains("Preview ready"));
}

/// REQ: PORT:ilium-client/src/onboarding/sound_preview.rs::newer_design_cancels_running_preview_and_coalesces_to_latest_revision
#[test]
fn port_007_a_newer_design_cancels_the_running_preview_and_only_the_latest_revision_is_installed() {
    let mut host = client_host();
    let first = request_preview(&mut host);
    let first_request = preview_jobs(&mut host).pop().expect("first job").request;
    assert_eq!(first["revision"], 0);
    adjust(&mut host, "pitch_hz", 801.0);
    host.send_action(step(), "preview", ActionKind::Activated, ActionValue::None);
    adjust(&mut host, "pitch_hz", 802.0);
    host.send_action(step(), "preview", ActionKind::Activated, ActionValue::None);
    host.settle();
    assert!(
        host.jobs().cancelled().contains(&first_request),
        "the running preview is cancelled"
    );
    let revisions: Vec<Value> = preview_jobs(&mut host)
        .iter()
        .map(|job| {
            serde_json::from_slice::<Value>(&job.input).expect("JSON input")["revision"].clone()
        })
        .collect();
    assert!(
        !revisions.contains(&json!(1)),
        "the intermediate revision coalesces away: {revisions:?}"
    );
    assert_eq!(revisions.last(), Some(&json!(2)));
    let latest = preview_jobs(&mut host).pop().expect("latest job").request;
    host.jobs().finish(latest, Ok(vec![0u8; 64]));
    host.settle();
    assert!(studio_text(&mut host, 118, 33).contains("Preview ready"));
}

/// REQ: PORT:ilium-client/src/onboarding/sound_preview.rs::waveform_is_prepared_on_cpu_worker_and_remains_charged_until_replaced
#[test]
fn port_008_the_waveform_is_prepared_by_a_job_and_dropped_when_the_design_changes() {
    let mut host = client_host();
    request_preview(&mut host);
    assert_eq!(
        preview_jobs(&mut host).len(),
        1,
        "the waveform is prepared by a job, not inline"
    );
    assert!(
        !studio_text(&mut host, 118, 33).contains("Preview ready"),
        "nothing is ready before the job finishes"
    );
    let request = preview_jobs(&mut host).pop().expect("job").request;
    host.jobs().finish(request, Ok(vec![0u8; 64]));
    host.settle();
    assert!(studio_text(&mut host, 118, 33).contains("Preview ready"));
    adjust(&mut host, "pitch_hz", 801.0);
    assert!(
        !studio_text(&mut host, 118, 33).contains("Preview ready"),
        "a changed design releases the retained waveform"
    );
}

const SIZES: [(u16, u16); 4] = [(38, 9), (80, 16), (118, 33), (198, 53)];
const LABELS: [&str; 5] = ["Waveform", "Pitch", "Sweep", "Pulse rate", "Save"];

/// Index of the first row of `lines` holding `needle` at or after `from`.
fn row_of(lines: &[String], needle: &str, from: usize) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, line)| line.contains(needle))
        .map(|(index, _)| index)
}

/// REQ: PORT:ilium-client/src/onboarding/studio_ui.rs::compact_sound_studio_previews_keep_a_row_between_their_frames
#[test]
fn port_005_compact_previews_keep_a_blank_row_between_their_frames() {
    let mut host = client_host();
    let lines = host.render(step(), 40, 16).lines();
    let waveform_top = row_of(&lines, "Waveform", 0).expect("the waveform frame");
    let waveform_bottom =
        row_of(&lines, "\u{2570}", waveform_top).expect("the waveform frame closes");
    let envelope_top = row_of(&lines, "Envelope", waveform_bottom).expect("the envelope frame");
    assert!(
        envelope_top > waveform_bottom + 1,
        "rows {waveform_bottom} and {envelope_top}"
    );
    assert_eq!(lines[waveform_bottom + 1].trim(), "", "{lines:?}");
}

/// REQ: PORT:ilium-client/src/onboarding/studio_ui.rs::every_control_is_reachable_at_minimum_and_wide_sizes
#[test]
fn port_006_every_control_is_reachable_at_minimum_and_wide_sizes() {
    for (columns, rows) in SIZES {
        let mut host = client_host();
        for label in LABELS {
            let mut seen = studio_text(&mut host, columns, rows).contains(label);
            for _ in 0..LABELS.len() * 3 {
                if seen {
                    break;
                }
                host.key(step(), "Down");
                seen = studio_text(&mut host, columns, rows).contains(label);
            }
            assert!(seen, "{columns}x{rows} never shows {label}");
        }
    }
}

/// The text of the row marked by the focus arrow, if any.
fn focused_row(host: &mut FakeHost, columns: u16, rows: u16) -> Option<String> {
    host.render(step(), columns, rows)
        .lines()
        .into_iter()
        .find(|line| line.trim_start().starts_with('\u{25b8}'))
}

/// REQ: PORT:ilium-client/src/onboarding/studio_ui.rs::keyboard_focus_wraps_and_edits_use_the_real_control_step
#[test]
fn port_007_keyboard_focus_wraps_and_edits_use_the_real_control_step() {
    let mut host = client_host();
    let (columns, rows) = (38, 9);
    assert!(focused_row(&mut host, columns, rows)
        .expect("a focus")
        .contains("Waveform"));
    host.key(step(), "Up");
    assert!(focused_row(&mut host, columns, rows)
        .expect("a focus")
        .contains("Save"));
    host.key(step(), "Down");
    assert!(focused_row(&mut host, columns, rows)
        .expect("a focus")
        .contains("Waveform"));
    // Move the focus to the pitch slider and take one keyboard step: 20 Hz.
    while !focused_row(&mut host, columns, rows).is_some_and(|row| row.contains("Pitch")) {
        host.key(step(), "Down");
    }
    let before = design(&mut host, "pitch_hz")
        .and_then(|value| value.as_i64())
        .expect("pitch");
    host.key(step(), "Right");
    host.settle();
    assert_eq!(design(&mut host, "pitch_hz"), Some(json!(before + 20)));
    host.send_action(
        step(),
        "scroll",
        ActionKind::Scrolled,
        ActionValue::Number(f64::from(i32::MAX)),
    );
    assert!(
        host.render(step(), columns, rows).text().contains("Save"),
        "scrolling clamps at the end"
    );
    host.send_action(
        step(),
        "scroll",
        ActionKind::Scrolled,
        ActionValue::Number(f64::from(i32::MIN)),
    );
    assert!(
        host.render(step(), columns, rows)
            .text()
            .contains("Waveform"),
        "scrolling clamps at the start"
    );
}

/// REQ: PORT:ilium-client/src/onboarding/studio_ui.rs::numeric_chrome_and_drag_track_use_separate_exact_hit_regions
#[test]
fn port_008_numeric_chrome_and_drag_track_are_separate_hit_regions() {
    for columns in [24u16, 40, 80, 140] {
        for (key, label) in NUMERIC_CONTROLS {
            let mut host = client_host();
            let screen = host.render(step(), columns, 28);
            let (row, _) = screen
                .find(label)
                .unwrap_or_else(|| panic!("{label} at {columns}"));
            let text = screen.row(row);
            let open = text
                .chars()
                .position(|glyph| glyph == '*')
                .unwrap_or_else(|| panic!("open glyph {label} at {columns}"));
            // The open glyph opens the number editor and never moves the slider.
            let before = design(&mut host, key);
            assert!(host.click(step(), open as u16, row));
            host.settle();
            assert_eq!(design(&mut host, key), before, "{label} at {columns}");
            let popups = host.popups();
            assert_eq!(popups.len(), 1, "{label} at {columns}: {popups:?}");
            assert!(popups[0].contains(label), "{popups:?}");
            host.send_action(
                ViewTarget::Popup(label.to_string()),
                "cancel",
                ActionKind::Closed,
                ActionValue::None,
            );
            // A click on the track sets a position and opens no editor.
            let track = text
                .chars()
                .position(|glyph| glyph == '\u{2501}')
                .unwrap_or_else(|| panic!("track {label} at {columns}"));
            assert!(host.click(step(), track as u16, row));
            host.settle();
            assert_eq!(
                host.popups().len(),
                1,
                "no second editor for {label} at {columns}"
            );
        }
    }
}

/// REQ: PORT:ilium-client/src/onboarding/studio_ui.rs::painted_controls_and_mouse_targets_share_rectangles
#[test]
fn port_009_painted_controls_and_mouse_targets_share_rectangles() {
    let mut host = client_host();
    let (columns, rows) = (40, 16);
    for label in LABELS {
        let mut found = host.render(step(), columns, rows).find(label);
        for _ in 0..LABELS.len() * 3 {
            if found.is_some() {
                break;
            }
            host.key(step(), "Down");
            found = host.render(step(), columns, rows).find(label);
        }
        let (row, column) = found.unwrap_or_else(|| panic!("{label} is painted"));
        assert!(
            host.click(step(), column, row),
            "{label} is a mouse target where it is painted"
        );
        host.settle();
        let focused = focused_row(&mut host, columns, rows)
            .unwrap_or_else(|| panic!("focus after clicking {label}"));
        assert!(focused.contains(label), "{label}: {focused}");
    }
}

/// REQ: PORT:ilium-client/src/onboarding/studio_ui.rs::render_handles_empty_tiny_and_extremely_wide_viewports
#[test]
fn port_010_rendering_handles_empty_tiny_and_extremely_wide_viewports() {
    let mut host = client_host();
    for (columns, rows) in [(40u16, 16u16), (200, 60), (500, 16)] {
        let screen = host.render(step(), columns, rows);
        assert_eq!((screen.columns, screen.rows), (columns, rows));
        assert!(screen.text().contains("Waveform"), "{columns}x{rows}");
    }
    // Sizes outside 10..=500 by 1..=200 fall back to the harness render size.
    let tiny = host.render(step(), 1, 1);
    let small = host.render(step(), 2, 3);
    assert_eq!((tiny.columns, tiny.rows), (small.columns, small.rows));
    assert!(tiny.columns > 0 && tiny.rows > 0);
}

/// REQ: PORT:ilium-client/src/onboarding/studio_ui.rs::slider_gestures_reach_endpoints_and_drag_clamps_outside_track
#[test]
fn port_011_slider_gestures_reach_the_endpoints_and_clamp_outside_the_track() {
    let mut host = client_host();
    for (key, label) in NUMERIC_CONTROLS {
        adjust(&mut host, key, -1.0e12);
        let low = design(&mut host, key)
            .and_then(|value| value.as_i64())
            .unwrap_or_else(|| panic!("{label} low"));
        adjust(&mut host, key, 1.0e12);
        let high = design(&mut host, key)
            .and_then(|value| value.as_i64())
            .unwrap_or_else(|| panic!("{label} high"));
        assert!(low < high, "{label}: {low} {high}");
        adjust(&mut host, key, -2.0e12);
        assert_eq!(
            design(&mut host, key),
            Some(json!(low)),
            "{label} clamps below the track"
        );
        adjust(&mut host, key, 2.0e12);
        assert_eq!(
            design(&mut host, key),
            Some(json!(high)),
            "{label} clamps beyond the track"
        );
        if key == "pulse_rate" {
            assert_eq!((low, high), (0, 160));
        }
    }
}

/// REQ: PORT:ilium-client/src/onboarding/studio_ui.rs::sound_studio_previews_use_titled_rounded_frames_at_regular_and_compact_sizes
#[test]
fn port_012_previews_use_titled_rounded_frames_at_regular_and_compact_sizes() {
    let mut host = client_host();
    for (columns, rows) in [(118u16, 33u16), (40, 16)] {
        let lines = host.render(step(), columns, rows).lines();
        for title in ["Waveform", "Envelope"] {
            let top =
                row_of(&lines, title, 0).unwrap_or_else(|| panic!("{title} at {columns}x{rows}"));
            assert!(
                lines[top].contains('\u{256d}') && lines[top].contains('\u{256e}'),
                "{title} top at {columns}x{rows}: {}",
                lines[top]
            );
            let bottom = row_of(&lines, "\u{2570}", top)
                .unwrap_or_else(|| panic!("{title} bottom at {columns}x{rows}"));
            assert!(
                lines[bottom].contains('\u{256f}'),
                "{title} bottom at {columns}x{rows}: {}",
                lines[bottom]
            );
        }
    }
}
