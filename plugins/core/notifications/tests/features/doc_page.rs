//! Doc tests (`doc_` rows): rules that the manual page, the traceability table and the user guide
//! state and that only a reading of those pages can check.

use crate::feature_only::{manual_page, page_text, table_rows, user_doc};

fn traceability_row(id: &str) -> String {
    manual_page("reference-feature-traceability.md")
        .lines()
        .find(|line| line.starts_with(&format!("| {id} |")))
        .unwrap_or_else(|| panic!("traceability row {id}"))
        .to_string()
}

/// REQ: F-047
#[test]
fn doc_047_the_traceability_row_names_the_sound_alerts_feature() {
    let row = traceability_row("F-047");
    assert!(row.contains("| Sound alerts | `notifications` | plugin-wasm | notifications plugin | server | sound.play |"), "{row}");
}

/// REQ: F-048
#[test]
fn doc_048_the_traceability_row_names_the_desktop_notifications_feature() {
    let row = traceability_row("F-048");
    assert!(row.contains("| Desktop notifications | `notifications` | plugin-wasm | notifications plugin | server | notify.desktop |"), "{row}");
}

/// REQ: F-049
#[test]
fn doc_049_the_traceability_row_names_the_coalescing_feature() {
    let row = traceability_row("F-049");
    assert!(row.contains("| Coalescing of redundant task alerts | `notifications` | plugin-wasm | notifications plugin | server | timers |"), "{row}");
}

/// REQ: F-047, F-048, F-049
#[test]
fn doc_047_all_three_features_ship_in_phase_4a_with_requirement_cor_011() {
    for id in ["F-047", "F-048", "F-049"] {
        let row = traceability_row(id);
        assert!(row.contains("| 4a | COR-011 |"), "{row}");
    }
}

/// REQ: F-047
#[test]
fn doc_047_the_page_lists_the_four_things_the_plugin_covers() {
    let page = page_text();
    for bullet in [
        "- **Sound alerts.** One sound per event kind",
        "- **Desktop notifications.** A notice from the operating system",
        "- **Coalescing of redundant alerts.** Task alerts that repeat inside a window are merged or skipped",
        "- **Attention marks on nodes.** Nodes that need you carry a mark until you acknowledge it",
    ] {
        assert!(page.contains(bullet), "{bullet}");
    }
}

/// REQ: F-047
#[test]
fn doc_047_the_kernel_keeps_the_look_of_the_mark_the_done_prefix_and_the_sound_rate() {
    assert!(page_text().contains(
        "The look of an attention mark, the done prefix and the sound rate belong to the kernel, so the plugin only decides when to ask."
    ));
}

/// REQ: F-047
#[test]
fn doc_047_the_page_has_five_samples_in_order() {
    let page = page_text();
    let mut last = 0;
    for heading in [
        "#### 1. Sound on approval prompts",
        "#### 2. Desktop notification, coalesced",
        "#### 3. In-app toast with actions",
        "#### 4. Attention marks",
        "#### 5. Mute sound with a toggle",
    ] {
        let at = page.find(heading).unwrap_or_else(|| panic!("{heading}"));
        assert!(at > last, "{heading} out of order");
        last = at;
    }
}

/// REQ: F-047
#[test]
fn doc_047_sample_one_keys_the_sound_by_node_and_revision() {
    assert!(page_text().contains("format!(\"ask:{}:{}\", change.node, change.revision)"));
}

/// REQ: F-048
#[test]
fn doc_048_sample_two_merges_a_burst_with_a_30000_ms_window_and_a_60000_ms_limit() {
    let page = page_text();
    assert!(page.contains("coalesce_window_ms: 30_000"));
    assert!(page.contains("(at most 60,000 ms)"));
}

/// REQ: F-048
#[test]
fn doc_048_sample_four_limits_the_mark_reason_to_80_characters() {
    assert!(page_text().contains("// at most 80 characters"));
}

/// REQ: F-047
#[test]
fn doc_047_the_guide_has_six_sound_events_and_four_desktop_events() {
    let guide = user_doc("notifications.md");
    let rows = table_rows(
        &guide,
        "| Event | Sound switch | Desktop switch | Sound default | Desktop default |",
    );
    assert_eq!(rows.len(), 6);
    let desktop = rows.iter().filter(|row| row[2] != "none").count();
    assert_eq!(desktop, 4);
    assert!(guide.contains("So there are six sound events and four desktop-notification events."));
}

/// REQ: F-047
#[test]
fn doc_047_the_guide_sound_defaults_are_finished_and_failed_only() {
    let guide = user_doc("notifications.md");
    let rows = table_rows(
        &guide,
        "| Event | Sound switch | Desktop switch | Sound default | Desktop default |",
    );
    let on: Vec<&str> = rows
        .iter()
        .filter(|row| row[3] == "On")
        .map(|row| row[1].as_str())
        .collect();
    assert_eq!(on, ["Agent finished", "Task failed or monitor lost"]);
}

/// REQ: F-048
#[test]
fn doc_048_the_guide_desktop_defaults_are_everything_but_task_succeeded() {
    let guide = user_doc("notifications.md");
    let rows = table_rows(
        &guide,
        "| Event | Sound switch | Desktop switch | Sound default | Desktop default |",
    );
    let on: Vec<&str> = rows
        .iter()
        .filter(|row| row[4] == "On")
        .map(|row| row[2].as_str())
        .collect();
    assert_eq!(
        on,
        ["Agent finished", "Approval required", "Task failed or lost"]
    );
    let off: Vec<&str> = rows
        .iter()
        .filter(|row| row[4] == "Off")
        .map(|row| row[2].as_str())
        .collect();
    assert_eq!(off, ["Task succeeded"]);
}

/// REQ: F-048
#[test]
fn doc_048_the_guide_switch_table_lists_the_seven_config_keys_with_defaults() {
    let guide = user_doc("notifications.md");
    let rows = table_rows(&guide, "| Switch | `config.toml` key | Default |");
    let pairs: Vec<(String, String)> = rows
        .iter()
        .map(|row| (row[1].clone(), row[2].clone()))
        .collect();
    let expected = [
        ("`enabled`", "On"),
        ("`agent_finished`", "On"),
        ("`approval_required`", "On"),
        ("`task_succeeded`", "Off"),
        ("`task_failed`", "On"),
        ("`suppress_redundant_task_outcomes`", "On"),
        ("`task_coalesce_seconds`", "30"),
    ];
    assert_eq!(pairs.len(), expected.len());
    for (pair, (key, default)) in pairs.iter().zip(expected) {
        assert_eq!((pair.0.as_str(), pair.1.as_str()), (key, default));
    }
}

/// REQ: F-049
#[test]
fn doc_049_the_guide_states_the_merge_window_rules() {
    let guide = user_doc("notifications.md");
    assert!(guide.contains("The window is 30 seconds by default, adjustable in 10-second steps up to 600 seconds. `0` disables merging."));
    assert!(guide.contains("Kinds are counted separately, so a success and a failure on one pane do not merge with each other."));
    assert!(guide
        .contains("task_coalesce_seconds = 30        # 0 to 600; values above 600 are clamped"));
}

/// REQ: F-049
#[test]
fn doc_049_the_guide_states_the_redundant_outcome_rule() {
    let guide = user_doc("notifications.md");
    assert!(guide.contains(
        "Task alerts are skipped while the agent in that pane is idle, finished-unread or parked."
    ));
    assert!(guide.contains("Panes with no agent are never skipped."));
    assert!(guide.contains("Turn this off with `suppress_redundant_task_outcomes = false`."));
}

/// REQ: F-047
#[test]
fn doc_047_the_guide_lists_the_five_sound_sources() {
    let guide = user_doc("notifications.md");
    for source in [
        "`system_beep`",
        "`sound_file`",
        "`bundled_chirping`",
        "`generated`",
        "`muted`",
    ] {
        assert!(guide.contains(source), "{source}");
    }
}

/// REQ: F-047
#[test]
fn doc_047_the_guide_states_the_design_ranges() {
    let guide = user_doc("notifications.md");
    assert!(guide.contains("pitch 60 to 1800 Hz, pitch slide plus or minus 1800 cents, duration 80 to 3000 ms, volume 0 to 100 percent"));
    assert!(guide.contains("waveforms are sine, triangle (default), saw and square"));
}

/// REQ: F-047
#[test]
fn doc_047_the_guide_bounds_one_playback_at_15_seconds() {
    assert!(user_doc("notifications.md").contains("One playback is cut off after 15 seconds."));
}

/// REQ: COR-011
#[test]
fn doc_011_the_config_page_lists_the_keys_the_plugin_owns() {
    let config = manual_page("reference-config-and-data-migration.md");
    assert!(config.contains(
        "`[sound]`: `source`, `file`, `design`, `events`. `[notifications]`: `enabled`, `agent_finished`, `approval_required`, `task_succeeded`, `task_failed`, `suppress_redundant_task_outcomes`, `task_coalesce_seconds`"
    ));
}

/// REQ: SEC-293
#[test]
fn doc_293_the_security_section_points_at_the_corrected_extension_risk_table() {
    let page = page_text();
    assert!(page.contains("Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293])."));
}
