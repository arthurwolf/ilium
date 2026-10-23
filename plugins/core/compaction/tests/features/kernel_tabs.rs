//! Kernel tabs owned by the plugin (COR-030, COR-033), the effect of disabling it (COR-022) and the Optimization tab (F-115).

use ilium_plugin_sdk::types::DeactivationReason;
use serde_json::json;

use crate::common::*;

/// REQ: COR-030, COR-033, F-115
#[test]
fn doc_cor_030_page_lists_exactly_the_optimization_and_remote_compaction_tabs() {
    let page = page_text();
    let optimization = table_row(&page, "| `optimization` |");
    assert_eq!(
        &optimization[1..5],
        [
            "`optimization`",
            "Optimization",
            "none (writes the agent's own config)",
            "none"
        ]
    );
    let remote = table_row(&page, "| `remote-compaction` |");
    assert_eq!(
        &remote[1..5],
        [
            "`remote-compaction`",
            "Remote compaction",
            "`[remote_compaction]`",
            "none"
        ]
    );
}

/// REQ: COR-030, F-115
#[test]
fn doc_cor_030_migration_page_names_compaction_as_owner_of_both_tabs() {
    let page = pages_text("reference-config-and-data-migration.md");
    let optimization = table_row(&page, "| `optimization` |");
    assert_eq!(optimization[2], "Optimization");
    assert_eq!(optimization[3], "none (writes the agent's own config)");
    assert_eq!(optimization[4], "Compaction");
    assert_eq!(optimization[6], "none");
    let remote = table_row(&page, "| `remote-compaction` |");
    assert_eq!(remote[4], "Compaction");
    assert_eq!(remote[6], "`[remote_compaction]`");
}

/// REQ: COR-030, COR-033
#[test]
fn doc_cor_033_tab_grants_in_the_manifest_match_the_tab_ids() {
    let manifest = manifest_value();
    let tab_grants: Vec<String> = manifest["capabilities"]["client"]
        .as_array()
        .expect("client grants")
        .iter()
        .map(|grant| grant.as_str().expect("string").to_string())
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .collect();
    assert_eq!(
        tab_grants,
        [
            "settings.write-kernel:optimization",
            "settings.write-kernel:remote-compaction"
        ]
    );
}

/// REQ: COR-030
#[test]
fn kernel_030_server_side_holds_no_kernel_tab_grant() {
    let manifest = manifest_value();
    let server = manifest["capabilities"]["server"]
        .as_array()
        .expect("server grants");
    assert!(server.iter().all(|grant| !grant
        .as_str()
        .expect("string")
        .starts_with("settings.write-kernel:")));
}

/// REQ: COR-033
#[test]
fn doc_cor_033_migration_page_lists_the_sixteen_remote_compaction_keys() {
    let page = pages_text("reference-config-and-data-migration.md");
    let row = table_row(&page, "| `[remote_compaction]` |");
    let keys: Vec<String> = row[2]
        .split(',')
        .map(|key| key.trim().trim_matches('`').to_string())
        .collect();
    assert_eq!(
        keys,
        [
            "enabled",
            "automatic",
            "threshold_percent",
            "claude_technique",
            "codex_technique",
            "other_technique",
            "custom_prompt",
            "tail_tokens",
            "protected_recent_tool_tokens",
            "tool_result_chars",
            "redact_secrets",
            "summarizer_context_tokens",
            "pause_timeout_seconds",
            "cooldown_minutes",
            "keep_backups",
            "privacy_banner_dismissed"
        ]
    );
}

/// REQ: COR-022
#[test]
fn doc_cor_022_disabled_notice_is_stated_verbatim_on_the_page() {
    assert!(page_text().contains("\"Compaction is disabled. Values are kept.\""));
}

/// REQ: COR-022
#[test]
fn doc_cor_022_kernel_boot_row_says_no_compact_action_and_stored_settings_stay() {
    let page = pages_text("reference-kernel-and-boot.md");
    let rows: Vec<&str> = page
        .lines()
        .filter(|line| line.starts_with("| Compaction | No compact action"))
        .collect();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains("No compact action, no Optimization tab behaviour"));
    assert!(rows[0].contains("Stored technique settings stay"));
}

/// REQ: COR-022, COR-033
#[test]
fn kernel_022_disabling_keeps_every_stored_remote_compaction_value() {
    let mut host = client_host();
    host.settings()
        .set("remote_compaction.threshold_percent", json!(70))
        .expect("setting accepted");
    host.settings()
        .set("remote_compaction.codex_technique", json!("opencode"))
        .expect("setting accepted");
    host.deactivate(DeactivationReason::Disabled);
    assert_eq!(
        host.settings().get("remote_compaction.threshold_percent"),
        Some(json!(70))
    );
    assert_eq!(
        host.settings().get("remote_compaction.codex_technique"),
        Some(json!("opencode"))
    );
}

/// REQ: COR-022
#[test]
fn kernel_022_a_disabled_instance_answers_no_compact_command() {
    let mut host = server_host();
    host.deactivate(DeactivationReason::Disabled);
    let submissions = host.deliver_one(command_on(
        COMPACT_COMMAND,
        ilium_plugin_sdk::types::CommandOrigin::Palette,
        Some(1),
    ));
    assert!(submissions.is_empty());
    assert!(host.jobs().list().is_empty());
}

/// REQ: COR-033
#[test]
fn kernel_033_changing_one_setting_leaves_unknown_keys_alone() {
    // The kernel keeps unknown keys of [remote_compaction]; the plugin must not drop or rename them.
    let mut host = client_host();
    host.settings()
        .set("remote_compaction.future_key", json!("kept"))
        .expect("unknown key stored");
    host.settings()
        .set("remote_compaction.keep_backups", json!(5))
        .expect("setting accepted");
    assert_eq!(
        host.settings().get("remote_compaction.future_key"),
        Some(json!("kept"))
    );
    assert_eq!(
        host.settings().get("remote_compaction.keep_backups"),
        Some(json!(5))
    );
}
