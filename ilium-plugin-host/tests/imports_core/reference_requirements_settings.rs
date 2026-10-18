//! reference-requirements-settings.md: the runtime rows (SET-005, SET-011, SET-020..SET-024,
//! SET-031, SET-050, SET-051). The install-time rows (SET-001..004, 010, 060..064) are swept in
//! `reference_settings_schema`.
//!
//! The fixture is `ilium_plugin_host::testing::imports_fixture` (see API_GAPS.md).

use std::time::Duration;

use ilium_plugin_api::generated::host::settings::FieldError;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::{ErrorCode, PluginError, SettingsOrigin};
use ilium_plugin_api::Tier;
use ilium_plugin_host::testing::imports_fixture::{ImportsFixture, UserInput, UserSave};
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::support::*;

fn changed_events(fixture: &ImportsFixture) -> Vec<(Vec<String>, SettingsOrigin)> {
    fixture
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::SettingsChanged(change) => Some((change.keys, change.origin)),
            _ => None,
        })
        .collect()
}

fn stored(fixture: &mut ImportsFixture) -> Value {
    serde_json::from_str(&fixture.settings().get_json()).expect("get-json returns one JSON object")
}

// ---- SET-005: dropped plugin sections and tabs -----------------------------------------------

fn section_plugin(index: usize) -> String {
    let mut manifest = minimal_manifest();
    manifest["id"] = json!(format!("test.section{index}"));
    manifest["capabilities"] = json!({ "server": ["settings.contribute:user-interface"] });
    manifest["contributes"] = json!({ "settings": [{
        "kind": "section", "id": "extra", "tab": "user-interface",
        "title": "Extra", "scope": "server",
        "rows": [toggle(&format!("on{index}"), true)]
    }] });
    manifest.to_string()
}

fn tab_plugin(index: usize) -> String {
    let mut manifest = minimal_manifest();
    manifest["id"] = json!(format!("test.tab{index}"));
    manifest["contributes"] = json!({ "settings": [{
        "kind": "tab", "id": "own", "title": format!("Own tab {index}"),
        "sections": [{ "id": "s", "title": "S", "rows": [toggle("on", true)] }]
    }] });
    manifest.to_string()
}

fn fixture_with_plugins(manifests: &[String]) -> ImportsFixture {
    let mut builder = ImportsFixture::builder().tier(Tier::T1);
    for manifest in manifests {
        builder = builder.also_install(manifest);
    }
    builder.build()
}

/// REQ: SET-005
#[test]
fn set_005_six_plugin_sections_in_one_built_in_tab_all_drawn() {
    let manifests: Vec<String> = (0..6).map(section_plugin).collect();
    let fixture = fixture_with_plugins(&manifests);
    assert_eq!(fixture.drawn_plugin_sections("user-interface"), 6);
    assert!(!fixture
        .load_warning_codes()
        .contains(&"settings.section-dropped".to_owned()));
}

/// REQ: SET-005
#[test]
fn set_005_seventh_plugin_section_dropped_with_settings_section_dropped() {
    let manifests: Vec<String> = (0..7).map(section_plugin).collect();
    let fixture = fixture_with_plugins(&manifests);
    assert_eq!(fixture.drawn_plugin_sections("user-interface"), 6);
    let dropped = fixture
        .load_warning_codes()
        .iter()
        .filter(|code| code.as_str() == "settings.section-dropped")
        .count();
    assert_eq!(dropped, 1);
}

/// REQ: SET-005
#[test]
fn set_005_eight_plugin_sections_drop_two_with_settings_section_dropped() {
    let manifests: Vec<String> = (0..8).map(section_plugin).collect();
    let fixture = fixture_with_plugins(&manifests);
    let dropped = fixture
        .load_warning_codes()
        .iter()
        .filter(|code| code.as_str() == "settings.section-dropped")
        .count();
    assert_eq!(dropped, 2);
}

/// REQ: SET-005
#[test]
fn set_005_sixteen_plugin_tabs_all_in_the_rail() {
    let manifests: Vec<String> = (0..16).map(tab_plugin).collect();
    let fixture = fixture_with_plugins(&manifests);
    assert_eq!(fixture.drawn_plugin_tabs(), 16);
    assert!(!fixture
        .load_warning_codes()
        .contains(&"settings.tab-dropped".to_owned()));
}

/// REQ: SET-005
#[test]
fn set_005_seventeenth_plugin_tab_dropped_with_settings_tab_dropped() {
    let manifests: Vec<String> = (0..17).map(tab_plugin).collect();
    let fixture = fixture_with_plugins(&manifests);
    assert_eq!(fixture.drawn_plugin_tabs(), 16);
    let dropped = fixture
        .load_warning_codes()
        .iter()
        .filter(|code| code.as_str() == "settings.tab-dropped")
        .count();
    assert_eq!(dropped, 1);
}

// ---- SET-011: a stale stored value falls back to the default ---------------------------------

/// REQ: SET-011
#[test]
fn set_011_stored_value_above_max_reads_as_the_default() {
    let mut fixture = settings_fixture(Tier::T2);
    fixture.restore_stored_json(r#"{"count": 99}"#);
    assert_eq!(stored(&mut fixture)["count"], json!(5));
}

/// REQ: SET-011
#[test]
fn set_011_stored_value_at_max_is_kept() {
    let mut fixture = settings_fixture(Tier::T2);
    fixture.restore_stored_json(r#"{"count": 10}"#);
    assert_eq!(stored(&mut fixture)["count"], json!(10));
}

/// REQ: SET-011
#[test]
fn set_011_stored_value_of_the_wrong_type_reads_as_the_default() {
    let mut fixture = settings_fixture(Tier::T2);
    fixture.restore_stored_json(r#"{"enabled": "yes"}"#);
    assert_eq!(stored(&mut fixture)["enabled"], json!(true));
}

/// REQ: SET-011
#[test]
fn set_011_stored_choice_no_longer_offered_reads_as_the_default() {
    let mut fixture = settings_fixture(Tier::T2);
    fixture.restore_stored_json(r#"{"mode": "gone"}"#);
    assert_eq!(stored(&mut fixture)["mode"], json!("fast"));
}

/// REQ: SET-011
#[test]
fn set_011_one_stale_value_does_not_reset_the_valid_ones() {
    let mut fixture = settings_fixture(Tier::T2);
    fixture.restore_stored_json(r#"{"count": 99, "label": "kept"}"#);
    let values = stored(&mut fixture);
    assert_eq!(values["count"], json!(5));
    assert_eq!(values["label"], json!("kept"));
}

// ---- SET-020: no clamping; typed outside the bounds is refused -------------------------------

/// REQ: SET-020
#[test]
fn set_020_typed_number_above_max_refused_with_row_note_and_value_unchanged() {
    let mut fixture = settings_fixture(Tier::T2);
    let result = fixture.user_edit("count", "99", UserInput::Typed);
    assert!(matches!(result, UserSave::Refused(_)));
    assert!(fixture.row_note("count").is_some());
    assert_eq!(stored(&mut fixture)["count"], json!(5));
}

/// REQ: SET-020
#[test]
fn set_020_typed_number_at_max_is_saved() {
    let mut fixture = settings_fixture(Tier::T2);
    assert_eq!(
        fixture.user_edit("count", "10", UserInput::Typed),
        UserSave::Pending
    );
    fixture.advance(Duration::from_millis(600));
    assert_eq!(stored(&mut fixture)["count"], json!(10));
}

/// REQ: SET-020
#[test]
fn set_020_typed_number_below_min_refused_with_row_note() {
    let mut fixture = settings_fixture(Tier::T2);
    let result = fixture.user_edit("count", "-1", UserInput::Typed);
    assert!(matches!(result, UserSave::Refused(_)));
    assert_eq!(stored(&mut fixture)["count"], json!(5));
}

/// REQ: SET-020
#[test]
fn set_020_stepper_stops_at_the_upper_bound() {
    let mut fixture = settings_fixture(Tier::T2);
    for _ in 0..20 {
        fixture.user_edit("count", "+1", UserInput::NumberStep);
        fixture.advance(Duration::from_millis(600));
    }
    assert_eq!(stored(&mut fixture)["count"], json!(10));
}

/// REQ: SET-020
#[test]
fn set_020_stepper_stops_at_the_lower_bound() {
    let mut fixture = settings_fixture(Tier::T2);
    for _ in 0..20 {
        fixture.user_edit("count", "-1", UserInput::NumberStep);
        fixture.advance(Duration::from_millis(600));
    }
    assert_eq!(stored(&mut fixture)["count"], json!(0));
}

// ---- SET-021: save order ---------------------------------------------------------------------

/// REQ: SET-021
#[test]
fn set_021_user_save_runs_schema_validate_write_changed_in_order() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&runtime_settings_manifest().to_string())
        .settings_validate(Box::new(|_patch, _merged| Ok(())))
        .build();
    assert_eq!(fixture.user_save(r#"{"enabled": false}"#), UserSave::Saved);
    assert_eq!(
        fixture.save_trace(),
        vec!["schema", "validate", "write", "changed"]
    );
}

/// REQ: SET-021
#[test]
fn set_021_schema_failure_stops_before_validate_and_write() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&runtime_settings_manifest().to_string())
        .settings_validate(Box::new(|_patch, _merged| Ok(())))
        .build();
    let result = fixture.user_save(r#"{"count": 99}"#);
    assert!(matches!(result, UserSave::Refused(_)));
    assert_eq!(fixture.save_trace(), vec!["schema"]);
}

/// REQ: SET-021
#[test]
fn set_021_validate_refusal_stops_before_write_and_changed() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&runtime_settings_manifest().to_string())
        .settings_validate(Box::new(|_patch, _merged| {
            Err(vec![FieldError {
                key: "label".to_owned(),
                message: "no".to_owned(),
            }])
        }))
        .build();
    let result = fixture.user_save(r#"{"label": "x"}"#);
    assert!(matches!(result, UserSave::Refused(_)));
    assert_eq!(fixture.save_trace(), vec!["schema", "validate"]);
    assert_eq!(stored(&mut fixture)["label"], json!("work"));
}

/// REQ: SET-021
#[test]
fn set_021_validate_receives_the_patch_and_the_merged_object() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
    let sink = seen.clone();
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&runtime_settings_manifest().to_string())
        .settings_validate(Box::new(move |patch, merged| {
            sink.lock()
                .unwrap()
                .push((patch.to_owned(), merged.to_owned()));
            Ok(())
        }))
        .build();
    assert_eq!(fixture.user_save(r#"{"count": 7}"#), UserSave::Saved);
    let calls = seen.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let patch: Value = serde_json::from_str(&calls[0].0).unwrap();
    let merged: Value = serde_json::from_str(&calls[0].1).unwrap();
    assert_eq!(patch["count"], json!(7));
    assert_eq!(merged["count"], json!(7));
    assert_eq!(merged["label"], json!("work"));
}

/// REQ: SET-021
#[test]
fn set_021_settings_validate_runs_only_for_user_and_reset_origins_not_for_set_json() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter = calls.clone();
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&runtime_settings_manifest().to_string())
        .settings_validate(Box::new(move |_patch, _merged| {
            *counter.lock().unwrap() += 1;
            Ok(())
        }))
        .build();
    expect_ok(
        fixture
            .settings()
            .set_json(r#"{"enabled": false}"#.to_owned()),
    );
    assert_eq!(*calls.lock().unwrap(), 0);
    assert_eq!(fixture.user_save(r#"{"enabled": true}"#), UserSave::Saved);
    assert_eq!(*calls.lock().unwrap(), 1);
}

// ---- SET-022: settings-validate deadline -----------------------------------------------------

fn fixture_with_validate_latency(milliseconds: u64) -> ImportsFixture {
    ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&runtime_settings_manifest().to_string())
        .settings_validate(Box::new(|_patch, _merged| Ok(())))
        .settings_validate_latency(Duration::from_millis(milliseconds))
        .build()
}

/// REQ: SET-022
#[test]
fn set_022_answer_in_150_ms_refuses_the_save_and_keeps_the_old_value() {
    let mut fixture = fixture_with_validate_latency(150);
    let result = fixture.user_save(r#"{"label": "new"}"#);
    assert!(matches!(result, UserSave::Refused(_)));
    assert_eq!(stored(&mut fixture)["label"], json!("work"));
}

/// REQ: SET-022
#[test]
fn set_022_answer_in_99_ms_saves() {
    let mut fixture = fixture_with_validate_latency(99);
    assert_eq!(fixture.user_save(r#"{"label": "new"}"#), UserSave::Saved);
    assert_eq!(stored(&mut fixture)["label"], json!("new"));
}

/// REQ: SET-022
#[test]
fn set_022_answer_in_exactly_100_ms_saves() {
    let mut fixture = fixture_with_validate_latency(100);
    assert_eq!(fixture.user_save(r#"{"label": "new"}"#), UserSave::Saved);
}

/// REQ: SET-022
#[test]
fn set_022_answer_in_101_ms_refuses_the_save() {
    let mut fixture = fixture_with_validate_latency(101);
    assert!(matches!(
        fixture.user_save(r#"{"label": "new"}"#),
        UserSave::Refused(_)
    ));
}

/// REQ: SET-022
#[test]
fn set_022_timeout_message_names_the_plugin_did_not_answer() {
    let mut fixture = fixture_with_validate_latency(150);
    match fixture.user_save(r#"{"label": "new"}"#) {
        UserSave::Refused(message) => {
            assert!(
                message.starts_with("Could not check this value:"),
                "{message}"
            );
            assert!(message.ends_with("did not answer."), "{message}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// ---- SET-032: saving state ---------------------------------------------------------------------

/// REQ: SET-032
#[test]
fn set_032_saving_marker_shows_until_the_debounced_write_completes() {
    let mut fixture = settings_fixture(Tier::T2);
    fixture.user_edit("label", "\"abc\"", UserInput::Typed);
    assert!(fixture.row_saving("label"));
    fixture.advance(Duration::from_millis(600));
    assert!(!fixture.row_saving("label"));
    assert_eq!(stored(&mut fixture)["label"], json!("abc"));
}

// ---- SET-023: write timing -------------------------------------------------------------------

fn count_changed(fixture: &ImportsFixture) -> usize {
    changed_events(fixture).len()
}

/// REQ: SET-023
#[test]
fn set_023_toggle_is_written_at_once() {
    let mut fixture = settings_fixture(Tier::T2);
    fixture.user_edit("enabled", "false", UserInput::Toggle);
    assert_eq!(stored(&mut fixture)["enabled"], json!(false));
    assert_eq!(count_changed(&fixture), 1);
}

/// REQ: SET-023
#[test]
fn set_023_choice_is_written_at_once() {
    let mut fixture = settings_fixture(Tier::T2);
    fixture.user_edit("mode", "\"slow\"", UserInput::Choice);
    assert_eq!(stored(&mut fixture)["mode"], json!("slow"));
}

/// REQ: SET-023
#[test]
fn set_023_dialog_confirm_is_written_at_once() {
    let mut fixture = settings_fixture(Tier::T2);
    fixture.user_edit("mode", "\"slow\"", UserInput::DialogConfirm);
    assert_eq!(stored(&mut fixture)["mode"], json!("slow"));
}

/// REQ: SET-023
#[test]
fn set_023_typed_text_is_written_600_ms_after_the_last_change() {
    let mut fixture = settings_fixture(Tier::T2);
    for (offset, value) in ["a", "ab", "abc"].iter().enumerate() {
        fixture.user_edit("label", &format!("\"{value}\""), UserInput::Typed);
        if offset < 2 {
            fixture.advance(Duration::from_millis(150));
        }
    }
    fixture.advance(Duration::from_millis(599));
    assert_eq!(count_changed(&fixture), 0);
    fixture.advance(Duration::from_millis(1));
    assert_eq!(count_changed(&fixture), 1);
    assert_eq!(stored(&mut fixture)["label"], json!("abc"));
}

/// REQ: SET-023
#[test]
fn set_023_number_steps_are_written_600_ms_after_the_last_step() {
    let mut fixture = settings_fixture(Tier::T2);
    fixture.user_edit("count", "+1", UserInput::NumberStep);
    fixture.advance(Duration::from_millis(300));
    fixture.user_edit("count", "+1", UserInput::NumberStep);
    fixture.advance(Duration::from_millis(599));
    assert_eq!(stored(&mut fixture)["count"], json!(5));
    fixture.advance(Duration::from_millis(1));
    assert_eq!(stored(&mut fixture)["count"], json!(7));
    assert_eq!(count_changed(&fixture), 1);
}

/// REQ: SET-023
#[test]
fn set_023_text_area_is_written_one_second_after_the_last_keystroke() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&manifest_with_rows(vec![text_area("notes", 4096)]).to_string())
        .build();
    fixture.user_edit("notes", "\"hello\"", UserInput::TextArea);
    fixture.advance(Duration::from_millis(999));
    assert_eq!(count_changed(&fixture), 0);
    fixture.advance(Duration::from_millis(1));
    assert_eq!(count_changed(&fixture), 1);
}

/// REQ: SET-023
#[test]
fn set_023_debounce_ms_replaces_the_default_delay() {
    let mut row = number("wide", 0, 100, 1);
    row["debounce_ms"] = json!(2000);
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&manifest_with_rows(vec![row]).to_string())
        .build();
    fixture.user_edit("wide", "+1", UserInput::NumberStep);
    fixture.advance(Duration::from_millis(1999));
    assert_eq!(count_changed(&fixture), 0);
    fixture.advance(Duration::from_millis(1));
    assert_eq!(count_changed(&fixture), 1);
}

/// REQ: SET-023
#[test]
fn set_023_three_characters_in_300_ms_give_one_settings_changed() {
    let mut fixture = settings_fixture(Tier::T2);
    for value in ["a", "ab", "abc"] {
        fixture.user_edit("label", &format!("\"{value}\""), UserInput::Typed);
        fixture.advance(Duration::from_millis(100));
    }
    fixture.advance(Duration::from_millis(600));
    assert_eq!(count_changed(&fixture), 1);
}

// ---- SET-024: value size limits --------------------------------------------------------------

fn size_manifest() -> String {
    manifest_with_rows(vec![
        text("t", 4096, ""),
        text_area("a", 65536),
        text_area("b", 65536),
        text_area("c", 65536),
        text_area("d", 65536),
        text_area("e", 65536),
    ])
    .to_string()
}

fn size_fixture() -> ImportsFixture {
    ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&size_manifest())
        .build()
}

/// REQ: SET-024
#[test]
fn set_024_text_value_of_4096_characters_accepted() {
    let mut fixture = size_fixture();
    let patch = json!({ "t": ascii(4096) }).to_string();
    expect_ok(fixture.settings().set_json(patch));
}

/// REQ: SET-024
#[test]
fn set_024_text_value_of_4097_characters_invalid_argument() {
    let mut fixture = size_fixture();
    let patch = json!({ "t": ascii(4097) }).to_string();
    assert_eq!(
        expect_code(fixture.settings().set_json(patch)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: SET-024
#[test]
fn set_024_text_area_value_of_65536_characters_accepted() {
    let mut fixture = size_fixture();
    let patch = json!({ "a": ascii(65536) }).to_string();
    expect_ok(fixture.settings().set_json(patch));
}

/// REQ: SET-024
#[test]
fn set_024_text_area_value_of_65537_characters_invalid_argument() {
    let mut fixture = size_fixture();
    let patch = json!({ "a": ascii(65537) }).to_string();
    assert_eq!(
        expect_code(fixture.settings().set_json(patch)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: SET-024
#[test]
fn set_024_all_values_of_one_plugin_over_256_kib_invalid_argument() {
    let mut fixture = size_fixture();
    // Five full text areas are 320 KiB, over the 256 KiB total.
    let patch = json!({
        "a": ascii(65536), "b": ascii(65536), "c": ascii(65536), "d": ascii(65536), "e": ascii(65536)
    })
    .to_string();
    assert_eq!(
        expect_code(fixture.settings().set_json(patch)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: SET-024
#[test]
fn set_024_three_full_text_areas_under_the_total_are_accepted() {
    let mut fixture = size_fixture();
    let patch = json!({ "a": ascii(65536), "b": ascii(65536), "c": ascii(65536) }).to_string();
    expect_ok(fixture.settings().set_json(patch));
}

/// REQ: SET-024
#[test]
fn set_024_refused_patch_stores_nothing() {
    let mut fixture = size_fixture();
    let patch = json!({ "t": "kept?", "a": ascii(65537) }).to_string();
    assert_eq!(
        expect_code(fixture.settings().set_json(patch)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(stored(&mut fixture)["t"], json!(""));
}

// ---- SET-031: set-json writes at once --------------------------------------------------------

/// REQ: SET-031
#[test]
fn set_031_set_json_is_readable_immediately() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(fixture.settings().set_json(r#"{"count": 8}"#.to_owned()));
    assert_eq!(stored(&mut fixture)["count"], json!(8));
}

/// REQ: SET-031
#[test]
fn set_031_set_json_delivers_settings_changed_without_a_debounce_wait() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(fixture.settings().set_json(r#"{"count": 8}"#.to_owned()));
    fixture.pump();
    let changes = changed_events(&fixture);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0, vec!["count".to_owned()]);
    assert_eq!(changes[0].1, SettingsOrigin::Plugin);
}

/// REQ: SET-031
#[test]
fn set_031_set_json_of_a_number_does_not_wait_600_ms() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(fixture.settings().set_json(r#"{"count": 9}"#.to_owned()));
    // No clock advance happened: the event is already queued.
    assert_eq!(changed_events(&fixture).len(), 1);
}

// ---- SET-050: rate limit of the row-update calls ---------------------------------------------

fn call_rate_limited_group(fixture: &mut ImportsFixture, index: usize) -> Result<(), PluginError> {
    // The limited calls share one budget; rotate through four of them.
    use ilium_plugin_api::generated::types::{MeterSegment, RowState, RowStateKind, Tone};
    match index % 4 {
        0 => fixture
            .settings()
            .set_info("status".to_owned(), "x".to_owned()),
        1 => fixture
            .settings()
            .set_row_note("count".to_owned(), Some("n".to_owned())),
        2 => fixture.settings().set_row_state(
            "count".to_owned(),
            RowState {
                kind: RowStateKind::Idle,
                message: None,
            },
        ),
        _ => fixture.settings().set_meter(
            "usage".to_owned(),
            vec![MeterSegment {
                label: "a".to_owned(),
                value: 0.1,
                tone: Tone::Info,
            }],
        ),
    }
}

/// REQ: SET-050
#[test]
fn set_050_ten_calls_in_a_second_all_succeed() {
    let mut fixture = settings_fixture(Tier::T2);
    for index in 0..10 {
        expect_ok(call_rate_limited_group(&mut fixture, index));
    }
}

/// REQ: SET-050
#[test]
fn set_050_eleventh_call_in_a_second_rate_limited() {
    let mut fixture = settings_fixture(Tier::T2);
    for index in 0..10 {
        expect_ok(call_rate_limited_group(&mut fixture, index));
    }
    let error = expect_error(call_rate_limited_group(&mut fixture, 10));
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert!(
        error.retry_after_ms.is_some(),
        "rate-limited carries retry-after-ms"
    );
}

/// REQ: SET-050
#[test]
fn set_050_the_previous_state_stays_after_a_rate_limited_call() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(
        fixture
            .settings()
            .set_info("status".to_owned(), "first".to_owned()),
    );
    for index in 0..9 {
        expect_ok(call_rate_limited_group(&mut fixture, index + 1));
    }
    let code = expect_code(
        fixture
            .settings()
            .set_info("status".to_owned(), "second".to_owned()),
    );
    assert_eq!(code, ErrorCode::RateLimited);
    assert_eq!(fixture.info_text("status").as_deref(), Some("x"));
}

/// REQ: SET-050
#[test]
fn set_050_budget_returns_after_one_second() {
    let mut fixture = settings_fixture(Tier::T2);
    for index in 0..10 {
        expect_ok(call_rate_limited_group(&mut fixture, index));
    }
    assert_eq!(
        expect_code(call_rate_limited_group(&mut fixture, 10)),
        ErrorCode::RateLimited
    );
    fixture.advance(Duration::from_millis(1000));
    expect_ok(call_rate_limited_group(&mut fixture, 11));
}

/// REQ: SET-050
#[test]
fn set_050_the_calls_share_one_budget_not_one_each() {
    let mut fixture = settings_fixture(Tier::T2);
    for _ in 0..5 {
        expect_ok(
            fixture
                .settings()
                .set_info("status".to_owned(), "x".to_owned()),
        );
    }
    for _ in 0..5 {
        expect_ok(fixture.settings().set_row_note("count".to_owned(), None));
    }
    let code = expect_code(
        fixture
            .settings()
            .set_info("status".to_owned(), "x".to_owned()),
    );
    assert_eq!(code, ErrorCode::RateLimited);
}

/// REQ: SET-050
#[test]
fn set_050_set_json_is_not_part_of_the_row_update_budget() {
    let mut fixture = settings_fixture(Tier::T2);
    for index in 0..10 {
        expect_ok(call_rate_limited_group(&mut fixture, index));
    }
    expect_ok(
        fixture
            .settings()
            .set_json(r#"{"enabled": false}"#.to_owned()),
    );
}

// ---- SET-051: text length of set-info and set-row-note ---------------------------------------

/// REQ: SET-051
#[test]
fn set_051_set_info_120_characters_accepted() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(fixture.settings().set_info("status".to_owned(), ascii(120)));
}

/// REQ: SET-051
#[test]
fn set_051_set_info_121_characters_invalid_argument() {
    let mut fixture = settings_fixture(Tier::T2);
    let code = expect_code(fixture.settings().set_info("status".to_owned(), ascii(121)));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: SET-051
#[test]
fn set_051_set_info_119_characters_accepted() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(fixture.settings().set_info("status".to_owned(), ascii(119)));
}

/// REQ: SET-051
#[test]
fn set_051_set_row_note_120_characters_accepted() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(
        fixture
            .settings()
            .set_row_note("count".to_owned(), Some(ascii(120))),
    );
}

/// REQ: SET-051
#[test]
fn set_051_set_row_note_121_characters_invalid_argument() {
    let mut fixture = settings_fixture(Tier::T2);
    let code = expect_code(
        fixture
            .settings()
            .set_row_note("count".to_owned(), Some(ascii(121))),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: SET-051
#[test]
fn set_051_text_counts_characters_not_bytes() {
    let mut fixture = settings_fixture(Tier::T2);
    // 120 two-byte characters are 240 bytes but still 120 characters.
    expect_ok(
        fixture
            .settings()
            .set_info("status".to_owned(), "\u{00e9}".repeat(120)),
    );
    let code = expect_code(
        fixture
            .settings()
            .set_info("status".to_owned(), "\u{00e9}".repeat(121)),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: SET-051
#[test]
fn set_051_a_refused_long_text_leaves_the_previous_info_text() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(
        fixture
            .settings()
            .set_info("status".to_owned(), "before".to_owned()),
    );
    let _ = fixture.settings().set_info("status".to_owned(), ascii(121));
    assert_eq!(fixture.info_text("status").as_deref(), Some("before"));
}

proptest! {
    /// REQ: SET-051
    #[test]
    fn set_051_set_info_refused_exactly_above_120_characters(length in 0usize..260) {
        let mut fixture = settings_fixture(Tier::T2);
        let result = fixture.settings().set_info("status".to_owned(), ascii(length));
        prop_assert_eq!(result.is_err(), length > 120);
    }

    /// REQ: SET-020
    #[test]
    fn set_020_typed_number_accepted_exactly_inside_the_bounds(value in -30i64..40) {
        let mut fixture = settings_fixture(Tier::T2);
        let result = fixture.user_edit("count", &value.to_string(), UserInput::Typed);
        let refused = matches!(result, UserSave::Refused(_));
        prop_assert_eq!(refused, !(0..=10).contains(&value));
    }

    /// REQ: SET-011
    #[test]
    fn set_011_any_stored_count_outside_the_bounds_reads_as_the_default(value in 11i64..10_000) {
        let mut fixture = settings_fixture(Tier::T2);
        fixture.restore_stored_json(&json!({ "count": value }).to_string());
        prop_assert_eq!(stored(&mut fixture)["count"].clone(), json!(5));
    }
}
