//! Twins of agent_toolbar.rs: button set, layout, keystroke stages and effort chrome of the agent toolbar.
//! Request/reply shape: see API_GAPS_ports.md.
use crate::common::{ask, Place};
use serde_json::Value;

fn check(place: Place, op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(place, op, request);
    let fields = expected.as_object().expect("expected is an object");
    for (name, value) in fields {
        assert_eq!(
            &reply[name.as_str()],
            value,
            "field {name} of reply {reply}"
        );
    }
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::adjacent_buttons_keep_at_least_a_two_column_gap
#[test]
fn port_agent_toolbar_adjacent_buttons_keep_at_least_a_two_column_gap() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "claude", "width": 120}"####,
        r####"{"min_gap": 2}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::antigravity_hides_compact_and_fast
#[test]
fn port_agent_toolbar_antigravity_hides_compact_and_fast() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "antigravity"}"####,
        r####"{"has_compact": false, "has_fast": false, "command_for": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::center_group_is_actually_centered
#[test]
fn port_agent_toolbar_center_group_is_actually_centered() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "claude", "width": 120}"####,
        r####"{"center_offset_at_most": 1}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::claude_exposes_four_models_and_effort_and_fast
#[test]
fn port_agent_toolbar_claude_exposes_four_models_and_effort_and_fast() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "claude"}"####,
        r####"{"models": ["🐁Haiku", "🐈Sonnet", "🦁Opus", "🦖Fable"], "has_effort": true, "has_fast": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::claude_models_use_their_individually_configurable_icons
#[test]
fn port_agent_toolbar_claude_models_use_their_individually_configurable_icons() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "claude", "icons": ["•", "◉", "◆", "✦"]}"####,
        r####"{"models": ["•Haiku", "◉Sonnet", "◆Opus", "✦Fable"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::click_outside_toolbar_row_misses
#[test]
fn port_agent_toolbar_click_outside_toolbar_row_misses() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "claude", "click_row": "outside"}"####,
        r####"{"hit": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::codex_keystroke_stages_open_picker_then_pick_model_then_level
#[test]
fn port_agent_toolbar_codex_keystroke_stages_open_picker_then_pick_model_then_level() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "codex", "tier": "Astra", "level": "High"}"####,
        r####"{"stages": ["/model", "\r", "2", "3"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::codex_max_and_ultra_add_an_advanced_reasoning_stage
#[test]
fn port_agent_toolbar_codex_max_and_ultra_add_an_advanced_reasoning_stage() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "codex", "tier": "Sol", "levels": ["Max", "Ultra"]}"####,
        r####"{"Max": ["/model", "\r", "1", "5", "1"], "Ultra": ["/model", "\r", "1", "5", "2"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::codex_toolbar_exposes_sol_astra_luna_tiers_not_flat_models
#[test]
fn port_agent_toolbar_codex_toolbar_exposes_sol_astra_luna_tiers_not_flat_models() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "codex"}"####,
        r####"{"tiers": ["☀️Sol", "⭐Astra", "🌙Luna"], "flat_models": []}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::effort_chrome_paints_and_hits_full_native_values_with_provider_gating
#[test]
fn port_agent_toolbar_effort_chrome_paints_and_hits_full_native_values_with_provider_gating() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "claude", "widths": [180, 260]}"####,
        r####"{"chrome": "← + →", "hits": {"left_on_value": "next-choice", "right_on_value": "previous-choice", "open": "open-choices", "label": null}, "gated_none_for": ["none", "codex", "antigravity"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::effort_cycles_through_every_level_and_wraps
#[test]
fn port_agent_toolbar_effort_cycles_through_every_level_and_wraps() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "claude"}"####,
        r####"{"cycles_all_levels": true, "wraps": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::keystroke_stages_for_rejects_codex_reasoning_level_on_other_providers
#[test]
fn port_agent_toolbar_keystroke_stages_for_rejects_codex_reasoning_level_on_other_providers() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "claude", "level": "High"}"####,
        r####"{"stages": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::keystroke_stages_for_single_command_actions_matches_command_for_plus_enter
#[test]
fn port_agent_toolbar_keystroke_stages_for_single_command_actions_matches_command_for_plus_enter() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "claude", "action": "clear"}"####,
        r####"{"stages": ["/clear\r"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::luna_reasoning_levels_omit_ultra
#[test]
fn port_agent_toolbar_luna_reasoning_levels_omit_ultra() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "codex", "tier": "Luna"}"####,
        r####"{"levels": ["Low", "Medium", "High", "Extra high", "Max"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::narrow_toolbar_drops_center_buttons_before_hiding_close
#[test]
fn port_agent_toolbar_narrow_toolbar_drops_center_buttons_before_hiding_close() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": "claude", "width": 6}"####,
        r####"{"has_close": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::selection_toggle_button_reflects_current_on_off_state
#[test]
fn port_agent_toolbar_selection_toggle_button_reflects_current_on_off_state() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"selection": ["on", "off"]}"####,
        r####"{"text": "<glyph>on|<glyph>off"}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::selection_toggle_tooltip_names_current_state
#[test]
fn port_agent_toolbar_selection_toggle_tooltip_names_current_state() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"selection": ["on", "off"]}"####,
        r####"{"tooltip_contains": ["on", "off"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::show_labels_appends_readable_text_after_icon_only_buttons
#[test]
fn port_agent_toolbar_show_labels_appends_readable_text_after_icon_only_buttons() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"show_labels": true}"####,
        r####"{"labels": ["Stop", "Close"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::tooltip_fits_beside_the_buttons_when_area_has_room
#[test]
fn port_agent_toolbar_tooltip_fits_beside_the_buttons_when_area_has_room() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"action": "stop", "area_width": 120}"####,
        r####"{"fits": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::tree_model_icons_follow_the_same_model_button_glyphs
#[test]
fn port_agent_toolbar_tree_model_icons_follow_the_same_model_button_glyphs() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"models": {"claude-sonnet-5": "sonnet", "claude-haiku-4-5": "haiku", "gpt-6-sol": "☀️", "GPT-6-Astra high": "⭐", "gpt-6-luna": "🌙", "gpt-5.6-terra": null, "gemini-3-pro": "model-icon", "Gemini 3.5 Flash (High)": "model-icon", "Gemini 3.5 Ultra": null}}"####,
        r####"{"as_requested": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/agent_toolbar.rs::universal_buttons_present_without_a_known_provider
#[test]
fn port_agent_toolbar_universal_buttons_present_without_a_known_provider() {
    check(
        Place::Client,
        "toolbar.case",
        r####"{"provider": null}"####,
        r####"{"buttons": ["stop", "copy-screen", "smart-copy", "toggle-text-selection", "close"], "right_edge_click": "close"}"####,
    );
}
