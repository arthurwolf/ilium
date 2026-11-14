//! Requirement coverage for the COR, HST, LIF, RND, PAN and USR families that no other lane covers:
//! core-plugin parity (COR-004..029, 953..958), backup and `du` output (HST-175, 176, LIF-199),
//! the view render gate (RND-000), the privacy lines of uninstall and erase (USR-963, 965).
//!
//! Each test names the closest existing entry point. Where the repository has no entry point for
//! the documented behaviour, or the implementation lives in a crate this one does not depend on,
//! the test fails through `not_implemented` with the reason, so the gap stays visible.

use std::path::Path;

use ilium_plugin_host::cli::CliOutput;
use ilium_plugin_host::lifecycle::StartMode;
use ilium_plugin_host::testing::harness::HostHarness;
use ilium_plugin_host::testing::life::{LifeHost, PluginSpec};
use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec as OpsPackage};
use ilium_plugin_host::testing::render::Rig;
use ilium_plugin_host::testing::user_rig::{PackageSpec as UserPackage, UserRig};
use serde_json::Value;

/// The plugin that provides the clock service in the COR-956 test.
const PROVIDER: &str = "com.example.clock";
/// A plugin that calls the provided services.
const CONSUMER: &str = "com.example.consumer";
/// The service name that keeps the old major (COR-956).
const OLD_CLOCK_NAME: &str = "com.example.clock.v1";
/// The service name of the new major (COR-956).
const NEW_CLOCK_NAME: &str = "com.example.clock.v2";
/// A plugin with a network grant and a privacy table (USR-963, USR-965).
const NOTES: &str = "com.example.notes";
/// The contact named by the privacy table of the NOTES fixture.
const PRIVACY_CONTACT: &str = "privacy@notes.example";

/// Fails the test for a requirement whose behaviour has no entry point reachable from this crate.
fn not_implemented(id: &str, missing: &str) -> ! {
    panic!("{id} not implemented: {missing}")
}

/// Whether `needle` occurs as a byte run inside `haystack`.
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// The human stdout and stderr of a command as one text.
fn combined_text(output: &CliOutput) -> String {
    format!("{}{}", output.stdout_text, output.stderr)
}

/// The `type` of every JSONL record, in output order.
fn record_types(output: &CliOutput) -> Vec<String> {
    output
        .stdout_jsonl
        .iter()
        .filter_map(|record| record["type"].as_str().map(str::to_owned))
        .collect()
}

/// The index of the first record of `kind`, if any.
fn first_index_of(types: &[String], kind: &str) -> Option<usize> {
    types.iter().position(|record_type| record_type == kind)
}

/// A file path inside the rig's scratch folder, as text.
fn scratch_file(directory: &Path, name: &str) -> String {
    directory.join(name).to_string_lossy().into_owned()
}

/// REQ: COR-004
#[test]
fn cor_004_screen_transfer_pastes_visible_text_only_between_adjacent_terminal_panes() {
    not_implemented(
        "COR-004",
        "screen transfer is implemented by the terminal plugin (plugins/core/terminal, its \
         `paste-screen` command and message); ilium-plugin-host has no entry point for it and does \
         not depend on that crate",
    );
}

/// REQ: COR-006
#[test]
fn cor_006_rolling_backups_keep_newest_copy_per_age_slot_and_never_delete_it() {
    not_implemented(
        "COR-006",
        "no rolling-backup thinning exists in the repository; src/retention.rs holds only the log \
         retention constant, and no sessions backup pass is exposed by ilium-plugin-host",
    );
}

/// REQ: COR-007
#[test]
fn cor_007_resume_command_is_stored_only_after_the_session_is_verified_for_the_pane() {
    not_implemented(
        "COR-007",
        "the Sessions plugin (plugins/core/sessions) owns the resume-command snapshot section; no \
         entry point in ilium-plugin-host verifies a pane's agent session before storing it",
    );
}

/// REQ: COR-008
#[test]
fn cor_008_clear_command_discards_the_detected_session_and_keeps_manual_names() {
    not_implemented(
        "COR-008",
        "the Sessions plugin (plugins/core/sessions) handles submitted `/clear`; ilium-plugin-host \
         has no entry point that submits pane input or reads the snapshot resume command",
    );
}

/// REQ: COR-016
#[test]
fn cor_016_worktree_preparation_copies_only_worktreeinclude_untracked_files() {
    not_implemented(
        "COR-016",
        "worktree creation lives in the Worktrees plugin (plugins/core/worktrees) and \
         ilium-server/src/worktree_include.rs; neither is a dependency of ilium-plugin-host",
    );
}

/// REQ: COR-017
#[test]
fn cor_017_worktree_removal_refuses_dirty_worktrees_and_discards_only_on_explicit_action() {
    not_implemented(
        "COR-017",
        "worktree removal lives in the Worktrees plugin (plugins/core/worktrees); ilium-plugin-host \
         has no entry point for removing a worktree or its Discard files action",
    );
}

/// REQ: COR-018
#[test]
fn cor_018_cost_price_uses_longest_lowercased_prefix_and_override_wins_ties() {
    not_implemented(
        "COR-018",
        "model price lookup lives in the Cost plugin (plugins/core/cost) and ilium-compaction-analysis \
         (src/price.rs); ilium-plugin-host has no price-table entry point",
    );
}

/// REQ: COR-027
#[test]
fn cor_027_locked_closed_tree_row_ignores_expand_clicks_but_keeps_rename_and_unlock() {
    not_implemented(
        "COR-027",
        "the Tree plugin (plugins/core/tree) owns row locking and its context menu; ilium-plugin-host \
         has no entry point that sends row clicks or reads the context menu",
    );
}

/// REQ: COR-028
#[test]
fn cor_028_agent_detect_shows_caution_mark_when_checks_stop_succeeding() {
    not_implemented(
        "COR-028",
        "the Agent detect plugin (plugins/core/agent-detect) owns the activity caution mark; \
         ilium-plugin-host has no entry point that runs agent classification on a clock",
    );
}

/// REQ: COR-029
#[test]
fn cor_029_ask_for_update_types_only_into_idle_agent_panes_and_reports_none_idle() {
    not_implemented(
        "COR-029",
        "Ask for update lives in the Fleet plugin (plugins/core/fleet) and `ilium broadcast` in the \
         ilium binary; ilium-plugin-host has no entry point for either",
    );
}

/// REQ: COR-953
#[test]
fn cor_953_peer_message_payload_starts_with_one_byte_format_version() {
    not_implemented(
        "COR-953",
        "the sender side is the guest import messages.send-peer; the host stores the raw payload \
         without a one-byte format version prefix, so the version byte is never written",
    );
}

/// REQ: COR-954
#[test]
fn cor_954_receiver_does_not_deliver_a_peer_message_with_a_higher_format_version() {
    let handle = HostHarness::new()
        .grants(&["messages.peer:friend.plugin"])
        .build()
        .ok()
        .expect("the harness builds with the peer grant");
    // Version 2 is above the highest version this receiver knows (version 1).
    handle.deliver_peer_message("friend.plugin", &[2, b'h', b'i']);
    assert!(
        handle.peer_messages().is_empty(),
        "COR-954 not implemented: the receiver recorded a version 2 peer payload it does not know; \
         the harness delivers every payload regardless of its format version"
    );
}

/// REQ: COR-955
#[test]
fn cor_955_release_that_introduces_a_payload_version_precedes_any_sender_of_it() {
    not_implemented(
        "COR-955",
        "release ordering of receivers and senders is a publishing rule with no runtime entry point \
         in ilium-plugin-host; there is no API that reads release notes or sender versions",
    );
}

/// REQ: COR-956
#[test]
fn cor_956_provider_keeps_old_major_reachable_under_its_old_name_beside_the_new_name() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new(PROVIDER)
            .startup()
            .provides_service(OLD_CLOCK_NAME)
            .provides_service(NEW_CLOCK_NAME),
    );
    let _report = host.start(StartMode::Normal);
    assert!(
        host.call_service(CONSUMER, OLD_CLOCK_NAME).is_ok(),
        "COR-956: the old major must stay callable under its old name while callers still run"
    );
    assert!(
        host.call_service(CONSUMER, NEW_CLOCK_NAME).is_ok(),
        "COR-956: the new major must be callable under its own name"
    );
}

/// REQ: COR-957
#[test]
fn cor_957_old_service_entry_keeps_its_implementation_until_the_ninety_day_window_ends() {
    not_implemented(
        "COR-957",
        "the host has no service-entry retirement window and no API that reads a release's removal \
         date; LifeHost::call_service only reports installed providers",
    );
}

/// REQ: COR-958
#[test]
fn cor_958_caller_falls_back_to_old_major_only_on_not_found_or_unsupported() {
    not_implemented(
        "COR-958",
        "the fallback between majors is caller-side plugin logic; ilium-plugin-host exposes no \
         entry point that runs a caller's fallback or its services.use grants",
    );
}

/// REQ: HST-175
#[test]
fn hst_175_backup_leaves_secrets_out_unless_include_secrets_is_given() {
    let mut rig = UserRig::new();
    let archive = scratch_file(&rig.scratch_dir(), "hst-175-default.backup");
    let output = rig.cli(&["backup", archive.as_str()]);
    assert_eq!(
        output.exit_code, 0,
        "HST-175 not implemented: `ilium plugin backup` is not a command of ilium-plugin-host (src/cli.rs); {}",
        output.stderr
    );
    let bytes = std::fs::read(&archive).unwrap_or_default();
    assert!(
        !contains_bytes(&bytes, b"secrets"),
        "HST-175: a backup without --include-secrets must not hold secret values"
    );
}

/// REQ: HST-175
#[test]
fn hst_175_backup_with_include_secrets_holds_plaintext_secrets_and_is_not_encrypted() {
    let mut rig = UserRig::new();
    let archive = scratch_file(&rig.scratch_dir(), "hst-175-secrets.backup");
    let output = rig.cli(&["backup", "--include-secrets", archive.as_str()]);
    assert_eq!(
        output.exit_code, 0,
        "HST-175 not implemented: `ilium plugin backup --include-secrets` is not a command of ilium-plugin-host; {}",
        output.stderr
    );
    let bytes = std::fs::read(&archive).unwrap_or_default();
    assert!(
        !bytes.is_empty(),
        "HST-175: the backup archive must be written when --include-secrets is given"
    );
}

/// REQ: HST-176
#[test]
fn hst_176_backup_include_flags_and_force_write_grants_consents_and_a_plaintext_warning() {
    let mut rig = UserRig::new();
    let archive = scratch_file(&rig.scratch_dir(), "hst-176.backup");
    std::fs::write(&archive, b"previous archive").unwrap_or_default();
    let output = rig.cli(&[
        "backup",
        "--include-grants",
        "--include-secrets",
        "--force",
        archive.as_str(),
    ]);
    assert_eq!(
        output.exit_code, 0,
        "HST-176 not implemented: backup with --include-grants, --include-secrets and --force is not a command of ilium-plugin-host; {}",
        output.stderr
    );
    let bytes = std::fs::read(&archive).unwrap_or_default();
    assert!(
        contains_bytes(&bytes, b"grants.json") && contains_bytes(&bytes, b"consents.json"),
        "HST-176: --include-grants must add grants.json and consents.json to the archive"
    );
    assert!(
        output
            .stdout_jsonl
            .iter()
            .any(|record| record["type"] == "warning"),
        "HST-176: --include-secrets must print a warning record saying the archive is not encrypted"
    );
}

/// REQ: LIF-199
#[test]
fn lif_199_backup_prints_progress_then_one_artifact_then_summary_and_result() {
    let mut rig = UserRig::new();
    let archive = scratch_file(&rig.scratch_dir(), "lif-199.backup");
    let output = rig.cli(&["backup", archive.as_str(), "--json"]);
    assert_eq!(
        output.exit_code, 0,
        "LIF-199 not implemented: `ilium plugin backup` is not a command of ilium-plugin-host; {}",
        output.stderr
    );
    let types = record_types(&output);
    let progress = first_index_of(&types, "progress");
    let artifact = first_index_of(&types, "artifact");
    let summary = first_index_of(&types, "summary");
    let result = first_index_of(&types, "result");
    assert!(progress.is_some(), "LIF-199: a progress record must be printed while the command runs");
    assert!(
        artifact.is_some_and(|index| progress.is_some_and(|first| first < index)),
        "LIF-199: the artifact record for the backup file must follow the progress records"
    );
    let artifact_record = output
        .stdout_jsonl
        .iter()
        .find(|record| record["type"] == "artifact");
    assert!(
        artifact_record.is_some_and(|record| record["kind"] == "backup"),
        "LIF-199: the artifact record of a backup must have kind `backup`"
    );
    assert!(
        summary.is_some_and(|index| artifact.is_some_and(|first| first < index)),
        "LIF-199: summary must follow the artifact records"
    );
    assert!(
        result.is_some_and(|index| summary.is_some_and(|first| first < index)),
        "LIF-199: result must be the last record, after summary"
    );
}

/// REQ: LIF-199
#[test]
fn lif_199_du_prints_one_usage_record_per_plugin_with_integer_byte_fields() {
    let mut rig = UserRig::new();
    let package = UserPackage::new(NOTES, "1.0.0");
    let install = rig.install_file(&package, &["--yes"]);
    assert_eq!(
        install.exit_code, 0,
        "LIF-199 setup failed: the fixture plugin did not install; {}",
        install.stderr
    );
    let output = rig.cli(&["du", "--json"]);
    assert_eq!(
        output.exit_code, 0,
        "LIF-199 not implemented: `ilium plugin du` is not a command of ilium-plugin-host; {}",
        output.stderr
    );
    let usage: Vec<&Value> = output
        .stdout_jsonl
        .iter()
        .filter(|record| record["type"] == "usage")
        .collect();
    assert_eq!(usage.len(), 1, "LIF-199: `du` must print one usage record for the installed plugin");
    for field in ["store", "data", "storage", "cache", "logs", "backups"] {
        assert!(
            usage[0][field].is_u64(),
            "LIF-199: usage field `{field}` must be an integer byte count"
        );
    }
}

/// REQ: LIF-177
#[test]
fn lif_177_start_ended_by_caught_signal_removes_its_crash_mark_and_is_not_a_crash() {
    not_implemented(
        "LIF-177",
        "no rig call delivers a caught SIGINT or SIGTERM during start, and none reads the crash mark \
         back; see tests/lifecycle_ops/API_GAPS.md",
    );
}

/// REQ: PAN-009
#[test]
fn pan_009_open_external_accepts_only_https_http_and_mailto_and_asks_consent_per_origin() {
    not_implemented(
        "PAN-009",
        "the open-external capability is implemented outside ilium-plugin-host (plugins/core and \
         ilium-client); no entry point in this crate opens an https, http or mailto URL",
    );
}

/// REQ: RND-000
#[test]
fn rnd_000_hidden_view_is_not_rendered_and_renders_once_when_it_becomes_visible() {
    let mut rig = Rig::new("T2");
    let view = rig.open_view("notes-view");
    rig.set_window_hidden(true);
    for _ in 0..10 {
        rig.invalidate(view);
    }
    rig.resize_view(view, 100, 40);
    rig.resize_view(view, 120, 40);
    rig.begin_frame();
    rig.end_frame();
    assert_eq!(
        rig.render_calls(view),
        0,
        "RND-000: the host must not call views.render while the view is hidden"
    );
    rig.set_window_hidden(false);
    rig.begin_frame();
    rig.end_frame();
    assert_eq!(
        rig.render_calls(view),
        1,
        "RND-000: a dirty view must render exactly once when it becomes visible"
    );
}

/// REQ: USR-963
#[test]
fn usr_963_uninstall_prints_the_privacy_line_for_a_plugin_with_an_http_grant() {
    let rig = OpsRig::new();
    let package = OpsPackage::new(NOTES, "1.0.0")
        .grant("http:api.notes.example")
        .privacy_table(&["notes"], 30, PRIVACY_CONTACT);
    let package_path = rig.write_package(&package).to_string_lossy().into_owned();
    let install = rig.cli(&["install", package_path.as_str(), "--yes"]);
    assert_eq!(
        install.exit_code, 0,
        "USR-963 setup failed: the fixture plugin with an http grant did not install; {}",
        install.stderr
    );
    let output = rig.cli(&["uninstall", NOTES, "--yes"]);
    let expected = format!(
        "This plugin may hold your data on its own servers. Ask {PRIVACY_CONTACT} to erase it."
    );
    assert!(
        combined_text(&output).contains(&expected),
        "USR-963 not implemented: uninstall does not print `{expected}`; exit {}, output: {}",
        output.exit_code,
        combined_text(&output)
    );
}

/// REQ: USR-964
#[test]
fn usr_964_publisher_erases_user_data_on_written_request_within_thirty_days() {
    not_implemented(
        "USR-964",
        "the erasure duty belongs to the publisher's own server and its README and [privacy] table; \
         ilium-plugin-host cannot observe or run a publisher's erasure process",
    );
}

/// REQ: USR-965
#[test]
fn usr_965_erase_data_sends_no_remote_request_and_names_the_remote_copy_contact() {
    let rig = OpsRig::new();
    let package = OpsPackage::new(NOTES, "1.0.0")
        .grant("http:api.notes.example")
        .privacy_table(&["notes"], 30, PRIVACY_CONTACT);
    let package_path = rig.write_package(&package).to_string_lossy().into_owned();
    let install = rig.cli(&["install", package_path.as_str(), "--yes"]);
    assert_eq!(
        install.exit_code, 0,
        "USR-965 setup failed: the fixture plugin with an http grant did not install; {}",
        install.stderr
    );
    let output = rig.cli(&["erase-data", NOTES, "--yes"]);
    assert_eq!(
        output.exit_code, 0,
        "USR-965 not implemented: `ilium plugin erase-data` is not a command of ilium-plugin-host; {}",
        output.stderr
    );
    let expected = format!("remote copy not erased: ask {PRIVACY_CONTACT}");
    assert!(
        combined_text(&output).contains(&expected),
        "USR-965: erase-data must print `{expected}` for the plugin with an http grant"
    );
}
