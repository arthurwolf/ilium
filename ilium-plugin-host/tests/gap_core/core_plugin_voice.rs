//! Core-plugin gap tests for `core-plugin-voice`: capture buffers stay out of core dumps and swap,
//! and remote providers show their retention statement and need confirmation before capture.

use ilium_plugin_host::testing::ops::OpsRig;

const TONE_MARKER: &str = "voice-tone-fixture-440hz";

/// REQ: COR-105
#[test]
fn cor_105_a_held_capture_stays_out_of_core_dumps_and_swap() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let _capture = server.start_voice_capture(TONE_MARKER);
    server.send_sigabrt_to_self();
    assert!(
        !rig.core_dump_contains(TONE_MARKER),
        "the test tone must not appear in the core dump"
    );
    assert!(
        !rig.swap_contains(TONE_MARKER),
        "the test tone must not appear in swap"
    );
}

/// REQ: COR-105
#[test]
fn cor_105_where_locking_is_unavailable_the_diagnostics_page_says_so() {
    let rig = OpsRig::new();
    if rig.platform_supports_memory_lock() {
        return;
    }
    let _server = rig.start_server("s1");
    assert!(
        rig.diagnostics_text()
            .contains("cannot lock capture memory"),
        "the diagnostics page must state the limitation"
    );
}

/// REQ: COR-106
#[test]
fn cor_106_settings_shows_the_configured_retention_text_or_retention_unknown_before_capture() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.add_remote_provider("provider-with-text", Some("kept for 30 days"));
    rig.add_remote_provider("provider-without-text", None);
    let settings_text = server.render_settings_text();
    assert!(
        settings_text.contains("kept for 30 days"),
        "configured retention must be shown"
    );
    assert!(
        settings_text.contains("retention unknown"),
        "a missing retention text must be shown"
    );
}

/// REQ: COR-106
#[test]
fn cor_106_capture_with_a_remote_provider_is_blocked_until_the_user_confirms_it() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.add_remote_provider("provider-with-text", Some("kept for 30 days"));
    let blocked = server.start_voice_capture("tone");
    assert!(
        blocked.is_refused(),
        "capture must be blocked before confirmation"
    );
    server.confirm_retention("provider-with-text");
    assert!(
        !server.start_voice_capture("tone").is_refused(),
        "capture must start once confirmed"
    );
}
