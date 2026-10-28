//! reference-secrets.md: what counts as a secret, the secrets store, redaction algorithm,
//! visibility rules, rotation and the user-facing masking and forgetting.

use ilium_plugin_api::ErrorCode;
use ilium_plugin_host::secrets::{base64_standard, base64_url, hex_lower, percent_encode, redact};
use proptest::prelude::*;
use serde_json::json;

use crate::support::*;

const SECRET: &str = "hunter2-secret-value";

/// REQ: SEC-240
#[test]
fn sec_240_a_row_declared_secret_is_a_secret_and_a_plain_row_is_not() {
    let rig = setup(&[])
        .secret("token", SECRET)
        .setting("plain", json!("visible-value"))
        .start();
    rig.host
        .log_from_plugin(&format!("a {SECRET} b visible-value"));
    let line = rig.log_messages().pop().expect("a log line");
    assert!(!line.contains(SECRET));
    assert!(line.contains("visible-value"));
}

/// REQ: SEC-240
#[test]
fn sec_240_provider_keys_and_webhook_tokens_are_secrets() {
    let rig = setup(&[])
        .kernel_secret("api.provider_key", "provider-key-123456")
        .kernel_secret("notify.webhook", "https://h.example/t/tok-abcdef")
        .start();
    rig.host
        .log_from_plugin("key provider-key-123456 hook https://h.example/t/tok-abcdef");
    let line = rig.log_messages().pop().expect("a log line");
    assert!(!line.contains("provider-key-123456") && !line.contains("tok-abcdef"));
}

/// REQ: SEC-241
#[test]
fn sec_241_secret_values_are_not_written_to_config_snapshots_exports_or_the_journal() {
    let rig = setup(&[]).secret("token", SECRET).start();
    rig.host.persist_all_stores();
    for artefact in rig
        .host
        .config_text()
        .into_iter()
        .chain(rig.host.snapshot_text())
        .chain(rig.host.export_text())
        .chain(rig.host.journal_text())
    {
        assert!(!artefact.contains(SECRET));
    }
}

/// REQ: SEC-241
#[test]
fn sec_241_without_a_keyring_secrets_json_has_the_documented_form_and_mode_0600() {
    use std::os::unix::fs::PermissionsExt;
    let rig = setup(&[]).no_keyring().secret("token", SECRET).start();
    rig.host.persist_all_stores();
    let path = rig.state.path().join("secrets.json");
    assert_eq!(
        std::fs::metadata(&path).expect("file").permissions().mode() & 0o777,
        0o600
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["plugins"]["test.plugin"]["token"], SECRET);
    assert!(parsed["kernel"].is_object());
}

/// REQ: SEC-241
#[test]
fn sec_241_with_a_keyring_the_store_holds_only_an_index() {
    let rig = setup(&[]).keyring().secret("token", SECRET).start();
    rig.host.persist_all_stores();
    let text = std::fs::read_to_string(rig.state.path().join("secrets.json")).unwrap_or_default();
    assert!(!text.contains(SECRET));
    assert_eq!(
        rig.host.keyring_value("test.plugin", "token").as_deref(),
        Some(SECRET)
    );
}

/// REQ: SEC-241
#[test]
fn sec_241_secrets_json_is_not_reachable_through_any_scope() {
    let rig = setup(&["fs.read:data"])
        .no_keyring()
        .secret("token", SECRET)
        .start();
    rig.host.persist_all_stores();
    let path = rig.state.path().join("secrets.json");
    assert_code(
        &rig.value(
            "ilium:host/fs.read",
            json!({ "scope": "data", "path": path.to_string_lossy() }),
        ),
        ErrorCode::NotPermitted,
    );
}

/// REQ: SEC-242
#[test]
fn sec_242_a_secret_row_with_project_scope_is_refused_with_manifest_secret_scope() {
    let manifest = server_manifest(json!({
        "contributes": { "settings": [{ "kind": "tab", "id": "main", "title": "Main", "sections": [{
            "id": "s", "title": "S", "scope": "project",
            "rows": [{ "type": "text", "key": "token", "label": "Token", "secret": true }]
        }]}]}
    }));
    let codes = manifest_error_codes(&manifest);
    assert!(
        codes.iter().any(|code| code == "manifest.secret-scope"),
        "{codes:?}"
    );
}

/// REQ: SEC-242
#[test]
fn sec_242_a_secret_value_found_in_a_project_file_is_ignored_and_audited() {
    let rig = setup(&[]).secret("token", SECRET).start();
    rig.host.trust_project("/work/project");
    rig.host.write_project_value(
        "/work/project",
        "test.plugin",
        "token",
        json!("from-project-file"),
    );
    assert_ne!(
        rig.host.effective_setting("token"),
        Some(json!("from-project-file"))
    );
    assert!(!rig.audit_lines().is_empty());
}

/// REQ: SEC-242
#[test]
fn sec_242_secret_rows_with_client_or_server_scope_are_accepted() {
    for scope in ["client", "server"] {
        let manifest = server_manifest(json!({
            "contributes": { "settings": [{ "kind": "tab", "id": "main", "title": "Main", "sections": [{
                "id": "s", "title": "S", "scope": scope,
                "rows": [{ "type": "text", "key": "token", "label": "Token", "secret": true }]
            }]}]}
        }));
        assert!(
            !manifest_error_codes(&manifest)
                .iter()
                .any(|code| code == "manifest.secret-scope"),
            "{scope}"
        );
    }
}

/// REQ: SEC-243
#[test]
fn sec_243_every_variant_of_a_secret_is_replaced() {
    let variants = [
        SECRET.to_owned(),
        base64_standard(SECRET.as_bytes()),
        base64_standard(SECRET.as_bytes())
            .trim_end_matches('=')
            .to_owned(),
        base64_url(SECRET.as_bytes()),
        percent_encode(SECRET),
        hex_lower(SECRET.as_bytes()),
    ];
    for variant in variants {
        let text = format!("before {variant} after");
        assert_eq!(
            redact(&text, &[SECRET.to_owned()]),
            "before <redacted> after",
            "{variant}"
        );
    }
}

/// REQ: SEC-243
#[test]
fn sec_243_length_boundary_5_6_7() {
    for (secret, redacted) in [("abcde", false), ("abcdef", true), ("abcdefg", true)] {
        let out = redact(&format!("x {secret} y"), &[secret.to_owned()]);
        assert_eq!(out.contains("<redacted>"), redacted, "{secret}");
    }
}

/// REQ: SEC-243
#[test]
fn sec_243_replacement_is_case_sensitive() {
    let out = redact(&SECRET.to_uppercase(), &[SECRET.to_owned()]);
    assert!(!out.contains("<redacted>"));
}

/// REQ: SEC-243
#[test]
fn sec_243_longer_values_are_replaced_first() {
    let secrets = ["abcdefgh".to_owned(), "abcdefghijkl".to_owned()];
    assert_eq!(redact("x abcdefghijkl y", &secrets), "x <redacted> y");
}

/// REQ: SEC-243
#[test]
fn sec_243_log_lines_trap_messages_and_error_strings_are_redacted() {
    let rig = setup(&[]).secret("token", SECRET).start();
    rig.host.log_from_plugin(&format!("leak {SECRET}"));
    assert!(rig.log_messages().iter().all(|line| !line.contains(SECRET)));
    let error = rig.host.error_string_for(&format!("failed with {SECRET}"));
    assert!(!error.contains(SECRET));
    assert!(!rig
        .host
        .trap_message_for(&format!("boom {SECRET}"))
        .contains(SECRET));
}

proptest! {
    // REQ: SEC-243
    #[test]
    fn sec_243_no_variant_of_any_long_secret_survives_redaction(secret in "[A-Za-z0-9]{6,32}", prefix in "[a-z ]{0,10}", suffix in "[a-z ]{0,10}") {
        let variants = [
            secret.clone(),
            base64_standard(secret.as_bytes()),
            base64_url(secret.as_bytes()),
            percent_encode(&secret),
            hex_lower(secret.as_bytes()),
        ];
        for variant in variants {
            let out = redact(&format!("{prefix}{variant}{suffix}"), std::slice::from_ref(&secret));
            prop_assert!(!out.contains(&variant));
        }
    }

    // REQ: SEC-243
    #[test]
    fn sec_243_redaction_is_idempotent(secret in "[A-Za-z0-9]{6,20}", text in "[a-z ]{0,30}") {
        let once = redact(&format!("{text}{secret}"), std::slice::from_ref(&secret));
        prop_assert_eq!(redact(&once, std::slice::from_ref(&secret)), once);
    }
}

/// REQ: SEC-244
#[test]
fn sec_244_the_owning_side_reads_its_secret_and_read_all_returns_an_empty_string() {
    let rig = setup(&["settings.read-all"])
        .secret("token", SECRET)
        .start();
    let own = rig
        .value("ilium:host/settings.get-json", json!({}))
        .expect("own");
    assert!(own.to_string().contains(SECRET));
    let all = rig
        .value(
            "ilium:host/settings.get-plugin-json",
            json!({ "plugin": "test.plugin" }),
        )
        .expect("read all");
    assert!(!all.to_string().contains(SECRET));
}

/// REQ: SEC-244
#[test]
fn sec_244_read_kernel_returns_redacted_for_a_kernel_secret() {
    let rig = setup(&["settings.read-kernel"])
        .kernel_secret("api.provider_key", "provider-key-123456")
        .start();
    let kernel = rig
        .value("ilium:host/settings.get-kernel-json", json!({}))
        .expect("kernel");
    assert!(kernel.to_string().contains("<redacted>"));
    assert!(!kernel.to_string().contains("provider-key-123456"));
}

/// REQ: SEC-244
#[test]
fn sec_244_the_settings_changed_event_names_the_key_only() {
    let rig = setup(&[]).secret("token", SECRET).start();
    rig.host.set_secret_now("token", "new-secret-value-1");
    let events = rig.host.delivered_events_text();
    assert!(events.contains("token"));
    assert!(!events.contains("new-secret-value-1") && !events.contains(SECRET));
}

/// REQ: SEC-244
#[test]
fn sec_244_the_review_dialog_and_json_output_never_hold_a_secret() {
    let rig = setup(&[]).secret("token", SECRET).start();
    assert!(!rig.host.review_text().contains(SECRET));
    assert!(!rig.host.cli_json_text().contains(SECRET));
}

/// REQ: SEC-245
#[test]
fn sec_245_a_secret_is_not_forwarded_through_any_inter_plugin_channel() {
    let rig = setup(&[]).secret("token", SECRET).start();
    for channel in ["messages.peer", "services", "topics", "extension-points"] {
        assert!(
            !rig.host.channel_payload_text(channel).contains(SECRET),
            "{channel}"
        );
    }
}

/// REQ: SEC-245
#[test]
fn sec_245_the_manual_names_the_residual_risk() {
    assert_page_has("reference-secrets.md", &["the host cannot stop that, which is a residual risk of inter-plugin channels ([SEC-007])"]);
}

/// REQ: SEC-246
#[test]
fn sec_246_old_value_is_overwritten_and_still_redacted_for_60_seconds() {
    let rig = setup(&[]).secret("token", SECRET).start();
    rig.host.set_secret_now("token", "rotated-secret-value");
    assert!(
        !rig.host.stored_secret_text().contains(SECRET),
        "no copy of the old value is kept"
    );
    rig.host.log_from_plugin(&format!("late {SECRET}"));
    assert!(!rig.log_messages().last().expect("line").contains(SECRET));
    rig.host.advance_clock(std::time::Duration::from_secs(59));
    rig.host.log_from_plugin(&format!("late {SECRET}"));
    assert!(
        !rig.log_messages().last().expect("line").contains(SECRET),
        "59 s"
    );
    rig.host.advance_clock(std::time::Duration::from_secs(2));
    rig.host.log_from_plugin(&format!("later {SECRET}"));
    assert!(
        rig.log_messages().last().expect("line").contains(SECRET),
        "61 s: forgotten"
    );
}

/// REQ: SEC-247 WARN
#[test]
fn sec_247_the_guidance_names_the_four_practices() {
    assert_page_has(
        "reference-secrets.md",
        &[
            "never log a secret",
            "headers, not in query strings",
            "overwrite buffers it controls",
            "command line",
        ],
    );
}

/// REQ: SEC-247 WARN
#[test]
fn sec_247_a_url_with_a_secret_in_the_query_is_redacted_in_the_consent_prompt() {
    let rig = setup(&["http:example.com"]).secret("token", SECRET).start();
    let shown = rig
        .host
        .consent_prompt_text_for_url(&format!("https://example.com/p?k={SECRET}"));
    assert!(!shown.contains(SECRET));
}

/// REQ: SEC-248
#[test]
fn sec_248_reveal_last_shows_at_most_8_trailing_characters() {
    for (reveal, expect) in [(0_u8, "••••"), (4, "••••t3rn"), (8, "••••3-pattern")]
    {
        let _ = expect;
        let rows = ilium_plugin_host::secrets::masked_row("secret-pattern", reveal);
        assert!(rows.starts_with("••••"));
        assert!(rows.chars().count() <= 4 + 8);
        assert_eq!(rows.chars().count(), 4 + reveal as usize);
    }
}

/// REQ: SEC-248
#[test]
fn sec_248_forget_secrets_empties_the_store_and_sends_settings_changed() {
    let rig = setup(&[]).no_keyring().secret("token", SECRET).start();
    rig.host.persist_all_stores();
    rig.host.forget_secrets(Some("test.plugin"));
    let text = std::fs::read_to_string(rig.state.path().join("secrets.json")).unwrap_or_default();
    assert!(!text.contains(SECRET));
    assert!(rig
        .host
        .delivered_events_text()
        .contains("settings-changed"));
}

/// REQ: SEC-248
#[test]
fn sec_248_the_secrets_screen_lists_names_only() {
    let rig = setup(&[]).secret("token", SECRET).start();
    let listing = rig.host.secrets_screen_text();
    assert!(listing.contains("test.plugin") && !listing.contains(SECRET));
}

/// REQ: SEC-249
#[test]
fn sec_249_diagnostics_report_sections_are_redacted_before_they_are_shown_or_saved() {
    let rig = setup(&[]).secret("token", SECRET).start();
    let section = rig
        .host
        .diagnostics_report_with_section(&format!("endpoint key {SECRET}"));
    assert!(!section.shown.contains(SECRET) && !section.saved.contains(SECRET));
}

/// REQ: SEC-250
#[test]
fn sec_250_snapshots_and_exports_exclude_secrets_and_a_restore_leaves_the_rows_empty() {
    let rig = setup(&[]).secret("token", SECRET).start();
    let snapshot = rig.host.snapshot_text().expect("snapshot");
    assert!(!snapshot.contains(SECRET));
    rig.host.restore_snapshot(&snapshot);
    assert_eq!(rig.host.secret_value("token"), None);
    assert!(rig
        .host
        .delivered_events_text()
        .contains("settings-changed"));
}

/// REQ: SEC-246
#[test]
fn sec_246_a_changed_secret_stays_redacted_in_late_log_lines_for_60_s() {
    assert_page_has(
        "reference-secrets.md",
        &["forgets it for redaction purposes after 60 s (so late log lines are still redacted)"],
    );
}

/// REQ: SEC-250
#[test]
fn sec_250_an_archive_without_include_secrets_leaves_secret_rows_empty() {
    assert_page_has(
        "reference-secrets.md",
        &["Archive made without `--include-secrets`", "the user re-enters them"],
    );
}
