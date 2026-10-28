//! SEC-100..SEC-123 of reference-grant-review-and-consent.md and the threat model, host half.
//! Everything under `ilium_plugin_host::review`, `consent`, `stores` and `approval` is a gap API
//! (API_GAPS.md) written as the manual specifies it.

use std::time::Duration;

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::manifest::CheckMode;
use ilium_plugin_host::approval::{ApprovalChannel, ApprovalError};
use ilium_plugin_host::cli::run_cli;
use ilium_plugin_host::consent::{ConsentGate, ConsentOutcome, DialogGuard, DialogKind};
use ilium_plugin_host::install::Installer;
use ilium_plugin_host::registry::PackageStore;
use ilium_plugin_host::review::{
    display_name, review_rows, scroll_indicator, update_review, DeclineCounter, Widening,
};
use ilium_plugin_host::stores::{AuditLog, ConsentStore, GrantStore, StoreError, TrustStore};

use crate::support::{args, cli_env, file_names, package_bytes, scratch, PackageSpec};

fn grant(text: &str) -> Grant {
    Grant::parse(text).expect("grant parses")
}

// ---------------------------------------------------------------- SEC-100 pipeline order

/// REQ: SEC-100
#[test]
fn sec_100_defective_inventory_is_reported_before_a_later_grant_fault() {
    let bytes = package_bytes(&PackageSpec {
        corrupt_hash: true,
        ..PackageSpec::with_grants(&["fs.nope"])
    });
    let dir = scratch();
    let installer = Installer::new(PackageStore {
        root: dir.path().to_path_buf(),
    });
    let report = installer.check(&bytes, CheckMode::Check);
    let first = report
        .diagnostics
        .first()
        .expect("a defective package reports a diagnostic");
    // SEC-100 step 3: a corrupted file hash is `manifest.file-hash`
    assert_eq!(
        first.code.as_str(),
        "manifest.file-hash",
        "inventory fails first"
    );
}

/// REQ: SEC-100
#[test]
fn sec_100_grant_faults_are_reported_once_the_inventory_is_sound() {
    let bytes = package_bytes(&PackageSpec::with_grants(&["fs.nope"]));
    let dir = scratch();
    let installer = Installer::new(PackageStore {
        root: dir.path().to_path_buf(),
    });
    let report = installer.check(&bytes, CheckMode::Check);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "manifest.grant-unknown"),
        "{:?}",
        report.diagnostics
    );
}

// ---------------------------------------------------------------- SEC-102 kernel-drawn dialogs

/// REQ: SEC-102
#[test]
fn sec_102_dialog_kinds_are_drawn_by_the_kernel_only() {
    for kind in [
        DialogKind::Review,
        DialogKind::Consent,
        DialogKind::Update,
        DialogKind::Trust,
    ] {
        assert!(kind.is_kernel_drawn(), "{kind:?}");
    }
}

/// REQ: SEC-102
#[test]
fn sec_102_a_plugin_panel_cannot_present_a_consent_dialog() {
    let gate = ConsentGate::new();
    assert!(!gate.accepts_plugin_drawn_dialogs());
}

// ---------------------------------------------------------------- SEC-103 click-jack guard

/// REQ: SEC-103
#[test]
fn sec_103_input_before_500_ms_is_ignored() {
    let mut guard = DialogGuard::opened_at(Duration::ZERO);
    assert!(!guard.accepts_input_at(Duration::from_millis(499)));
}

/// REQ: SEC-103
#[test]
fn sec_103_input_from_500_ms_is_accepted() {
    let mut guard = DialogGuard::opened_at(Duration::ZERO);
    assert!(guard.accepts_input_at(Duration::from_millis(500)));
}

/// REQ: SEC-103
#[test]
fn sec_103_key_repeat_within_80_ms_is_dropped() {
    let mut guard = DialogGuard::opened_at(Duration::ZERO);
    assert!(guard.accepts_input_at(Duration::from_millis(600)));
    assert!(!guard.accepts_input_at(Duration::from_millis(679)));
    assert!(guard.accepts_input_at(Duration::from_millis(680)));
}

// ---------------------------------------------------------------- SEC-104 display name

/// REQ: SEC-104
#[test]
fn sec_104_control_characters_are_removed_from_the_display_name() {
    let shown = display_name("Evil\u{1b}[31m\u{7}Plugin\n");
    assert!(shown.chars().all(|c| !c.is_control()), "{shown:?}");
    // ESC, BEL and the newline go; the printable remainder of the escape sequence stays
    assert_eq!(shown, "Evil[31mPlugin");
}

/// REQ: SEC-104
#[test]
fn sec_104_display_name_is_truncated_with_an_ellipsis() {
    let shown = display_name(&"a".repeat(500));
    assert!(shown.chars().count() <= 64, "{}", shown.chars().count());
    assert!(shown.ends_with('\u{2026}'));
}

/// REQ: SEC-104
#[test]
fn sec_104_a_plain_name_is_shown_unchanged() {
    assert_eq!(display_name("Git Helper"), "Git Helper");
}

// ---------------------------------------------------------------- SEC-105 consent gate

/// REQ: SEC-105
#[test]
fn sec_105_second_prompt_while_one_is_open_is_busy() {
    let mut gate = ConsentGate::new();
    gate.open("test.plugin", "http:example.com")
        .expect("first prompt opens");
    let outcome = gate
        .open("other.plugin", "net.http:example.org")
        .expect_err("busy");
    assert_eq!(outcome, ConsentOutcome::Busy);
}

/// REQ: SEC-105
#[test]
fn sec_105_a_plugin_is_rate_limited_after_repeated_prompts() {
    let mut gate = ConsentGate::new();
    for index in 0..6 {
        // Distinct targets isolate the hourly quota from the three-decline
        // block on a single target required by SEC-110.
        gate.open("test.plugin", &format!("http:host{index}.example.com")).expect("within six-prompt quota");
        gate.close(ConsentOutcome::Declined);
    }
    assert_eq!(gate.open("test.plugin", "http:seventh.example.com"), Err(ConsentOutcome::RateLimited));
}

/// REQ: SEC-105
#[test]
fn sec_105_an_unanswered_prompt_times_out() {
    let mut gate = ConsentGate::new();
    gate.open("test.plugin", "http:example.com")
        .expect("opens");
    assert_eq!(
        gate.tick(Duration::from_secs(24 * 3600)),
        Some(ConsentOutcome::TimedOut)
    );
}

// ---------------------------------------------------------------- SEC-106 / 107 review rows and details

/// REQ: SEC-106
#[test]
fn sec_106_each_requested_grant_is_one_row() {
    let rows = review_rows(&[grant("tree.read"), grant("fs.read:project")]);
    assert_eq!(rows.len(), 2);
}

/// REQ: SEC-106
#[test]
fn sec_106_a_high_risk_row_is_marked_and_listed_first() {
    let rows = review_rows(&[grant("tree.read"), grant("fs.read:path:{home}/**")]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].grant, "fs.read:path:{home}/**");
    assert!(rows[0].is_toxic, "the high-risk row is marked");
    assert!(!rows[1].is_toxic, "the normal row is not marked");
    assert!(rows[0].risk_rank > rows[1].risk_rank);
}

/// REQ: SEC-107
#[test]
fn sec_107_every_row_carries_plain_language_details() {
    for row in review_rows(&[grant("tree.read"), grant("sound.play")]) {
        assert!(!row.details.trim().is_empty());
    }
}

// ---------------------------------------------------------------- SEC-108 update diff

/// REQ: SEC-108
#[test]
fn sec_108_widened_argument_is_listed_as_wider() {
    let review = update_review(&[grant("http:api.example.com")], &[grant("http:*.example.com")]);
    assert_eq!(review.wider.len(), 1);
    assert_eq!(review.wider[0].widening, Widening::Argument);
}

/// REQ: SEC-108
#[test]
fn sec_108_removed_grants_are_listed() {
    let review = update_review(
        &[grant("tree.read"), grant("sound.play")],
        &[grant("tree.read")],
    );
    assert_eq!(review.removed.len(), 1);
}

/// REQ: SEC-108
#[test]
fn sec_108_narrowing_is_not_listed_as_wider() {
    let review = update_review(&[grant("http:*.example.com")], &[grant("http:api.example.com")]);
    assert!(review.wider.is_empty());
}

// ---------------------------------------------------------------- SEC-109 reset on key or tier change

/// REQ: SEC-109
#[test]
fn sec_109_a_changed_signing_key_resets_stored_decisions() {
    let dir = scratch();
    let mut store = GrantStore::open(dir.path()).expect("opens");
    store
        .set("test.plugin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "T2", &[grant("tree.read")])
        .expect("set");
    let kept = store.load("test.plugin", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "T2").expect("load");
    assert!(kept.is_empty());
}

/// REQ: SEC-109
#[test]
fn sec_109_a_changed_tier_resets_stored_decisions() {
    let dir = scratch();
    let mut store = GrantStore::open(dir.path()).expect("opens");
    store
        .set("test.plugin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "T2", &[grant("tree.read")])
        .expect("set");
    assert!(store
        .load("test.plugin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "T3")
        .expect("load")
        .is_empty());
}

// ---------------------------------------------------------------- SEC-110 three declines

/// REQ: SEC-110
#[test]
fn sec_110_third_decline_stops_the_prompt() {
    let mut counter = DeclineCounter::default();
    for _ in 0..3 {
        counter.record_decline("test.plugin", "http:example.com");
    }
    assert!(counter.is_silenced("test.plugin", "http:example.com"));
}

/// REQ: SEC-110
#[test]
fn sec_110_two_declines_do_not_silence() {
    let mut counter = DeclineCounter::default();
    for _ in 0..2 {
        counter.record_decline("test.plugin", "http:example.com");
    }
    assert!(!counter.is_silenced("test.plugin", "http:example.com"));
}

// ---------------------------------------------------------------- SEC-111 first start

/// REQ: SEC-111
#[test]
fn sec_111_first_start_without_decisions_grants_nothing() {
    let dir = scratch();
    let store = GrantStore::open(dir.path()).expect("opens");
    assert!(store
        .load("test.plugin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "T2")
        .expect("load")
        .is_empty());
}

// ---------------------------------------------------------------- SEC-112..114 stores

/// REQ: SEC-112
#[cfg(unix)]
#[test]
fn sec_112_grant_store_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch();
    let mut store = GrantStore::open(dir.path()).expect("opens");
    store
        .set("test.plugin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "T2", &[grant("tree.read")])
        .expect("set");
    let path = store.path().to_owned();
    let mode = std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

/// REQ: SEC-113
#[test]
fn sec_113_unknown_store_version_is_refused() {
    let dir = scratch();
    let path = dir.path().join("grants.json");
    std::fs::write(&path, br#"{"version":999,"plugins":{}}"#).expect("write");
    let error = GrantStore::open(dir.path()).expect_err("future version");
    assert!(matches!(error, StoreError::UnsupportedVersion(999)));
}

/// REQ: SEC-113
#[test]
fn sec_113_corrupt_store_is_renamed_and_a_fresh_one_starts() {
    let dir = scratch();
    std::fs::write(dir.path().join("grants.json"), b"{ not json").expect("write");
    let store = GrantStore::open(dir.path()).expect("recovers");
    assert!(store
        .load("test.plugin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "T2")
        .expect("load")
        .is_empty());
    assert!(file_names(dir.path())
        .iter()
        .any(|name| name.starts_with("grants.json.corrupt")));
}

/// REQ: SEC-114
#[test]
fn sec_114_external_edit_of_the_store_is_recorded_in_the_audit_log() {
    let dir = scratch();
    let path = {
        let mut store = GrantStore::open(dir.path()).expect("opens");
        store
            .set("test.plugin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "T2", &[grant("tree.read")])
            .expect("set");
        store.path().to_owned()
    };
    let mut document: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).expect("read actual store")).expect("store JSON");
    document["plugins"]["test.plugin"]["grants"]["server"] = serde_json::json!(["tree.read", "sound.play"]);
    std::fs::write(&path, serde_json::to_vec(&document).expect("encode actual store")).expect("external edit");
    let _ = GrantStore::open(dir.path()).expect("reopens");
    let log = AuditLog::open(dir.path()).expect("audit").entries();
    assert!(
        log.iter()
            .any(|entry| entry.kind == "store-external-edit"),
        "{log:?}"
    );
}

// ---------------------------------------------------------------- SEC-116 / 117 approval channel

/// REQ: SEC-116
#[test]
fn sec_116_high_risk_approval_goes_through_an_attached_client() {
    let mut channel = ApprovalChannel::with_client();
    channel.script_answer(true);
    assert_eq!(
        channel.ask("test.plugin", &[grant("fs.read:path:{home}/**")]),
        Ok(true)
    );
}

/// REQ: SEC-117
#[test]
fn sec_117_without_a_client_a_high_risk_grant_needs_review() {
    let mut channel = ApprovalChannel::without_client();
    assert_eq!(
        channel.ask("test.plugin", &[grant("fs.read:path:{home}/**")]),
        Err(ApprovalError::ReviewRequired)
    );
}

/// REQ: SEC-117
#[test]
fn sec_117_the_cli_reports_install_review_required_without_a_client() {
    let dir = scratch();
    let env = cli_env(dir.path());
    let outcome = run_cli(
        &env,
        &args(&[
            "plugin",
            "grants",
            "test.plugin",
            "--grant",
            "fs.read:path:{home}/**",
            "--json",
        ]),
    );
    assert_eq!(outcome.exit_code, 1);
    assert!(
        outcome.stdout.contains("install.review-required"),
        "{}",
        outcome.stdout
    );
}

// ---------------------------------------------------------------- SEC-118..120 revoke, uninstall, CLI

/// REQ: SEC-118
#[test]
fn sec_118_revoking_a_grant_removes_it_from_the_store() {
    let dir = scratch();
    let mut store = GrantStore::open(dir.path()).expect("opens");
    store
        .set(
            "test.plugin",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "T2",
            &[grant("tree.read"), grant("sound.play")],
        )
        .expect("set");
    store.revoke("test.plugin", "sound.play").expect("revoke");
    assert_eq!(
        store.load("test.plugin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "T2").expect("load"),
        vec![grant("tree.read")]
    );
}

/// REQ: SEC-119
#[test]
fn sec_119_uninstall_deletes_grants_and_consents() {
    let dir = scratch();
    let mut grants = GrantStore::open(dir.path()).expect("opens");
    grants
        .set("test.plugin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "T2", &[grant("tree.read")])
        .expect("set");
    let mut consents = ConsentStore::open(dir.path()).expect("opens");
    consents
        .record("test.plugin", "http:example.com", true)
        .expect("record");
    grants.forget("test.plugin").expect("forget");
    consents.forget("test.plugin").expect("forget");
    assert!(grants
        .load("test.plugin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "T2")
        .expect("load")
        .is_empty());
    assert!(consents.decisions("test.plugin").is_empty());
}

/// REQ: SEC-120
#[test]
fn sec_120_cli_consents_lists_stored_decisions_as_json() {
    let dir = scratch();
    let env = cli_env(dir.path());
    // manual does not pin which directory holds the consent store; record into both
    for root in [&env.data_dir, &env.state_dir] {
        std::fs::create_dir_all(root).expect("creates the store directory");
        ConsentStore::open(root)
            .expect("opens")
            .record("test.plugin", "http:example.com", true)
            .expect("record");
    }
    let outcome = run_cli(
        &env,
        &args(&["plugin", "consents", "test.plugin", "--json"]),
    );
    assert_eq!(outcome.exit_code, 0);
    // SEC-120: consents print as JSONL, one JSON value per line
    let records: Vec<serde_json::Value> = outcome
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect();
    assert!(!records.is_empty(), "{}", outcome.stdout);
    assert!(
        records
            .iter()
            .any(|record| record.to_string().contains("http:example.com")),
        "{}",
        outcome.stdout
    );
}

/// REQ: SEC-120
#[test]
fn sec_120_cli_grants_with_unknown_grant_fails_with_the_grant_code() {
    let dir = scratch();
    let env = cli_env(dir.path());
    let outcome = run_cli(
        &env,
        &args(&[
            "plugin",
            "grants",
            "test.plugin",
            "--grant",
            "fs.nope",
            "--json",
        ]),
    );
    // manual does not pin the exit code of a failed grants command; the error record is pinned
    assert_ne!(outcome.exit_code, 0);
    assert!(
        outcome.stdout.contains("manifest.grant-unknown"),
        "{}",
        outcome.stdout
    );
}

// ---------------------------------------------------------------- SEC-121..123

/// REQ: SEC-121
#[test]
fn sec_121_dialog_label_names_the_kernel_as_its_source() {
    assert_eq!(DialogKind::Consent.source_label(), "Ilium");
}

/// REQ: SEC-122
#[test]
fn sec_122_scroll_indicator_reads_n_of_m() {
    assert_eq!(scroll_indicator(3, 12), "3 of 12");
}

/// REQ: SEC-122
#[test]
fn sec_122_a_list_that_fits_shows_no_indicator() {
    assert_eq!(scroll_indicator(5, 5), "");
}

/// REQ: SEC-123
#[test]
fn sec_123_declined_trust_is_remembered_as_trust_declined() {
    let dir = scratch();
    let mut trust = TrustStore::open(dir.path()).expect("opens");
    trust.decline("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").expect("decline");
    assert_eq!(trust.state("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").as_str(), "trust.declined");
}

/// REQ: SEC-107
#[test]
fn sec_107_each_grant_gets_its_own_details_text() {
    let rows = review_rows(&[grant("tree.read"), grant("sound.play")]);
    assert_eq!(rows.len(), 2, "one row per grant");
    assert_ne!(
        rows[0].details, rows[1].details,
        "each grant needs its own plain-language details"
    );
}

/// REQ: SEC-111
#[test]
fn sec_111_three_not_now_answers_for_one_plugin_silence_its_prompt() {
    let mut counter = DeclineCounter::default();
    counter.record_decline("com.example.plugin", "fs.read:project");
    counter.record_decline("com.example.plugin", "net.http");
    counter.record_decline("com.example.plugin", "clipboard");
    assert!(
        counter.is_silenced("com.example.plugin", "fs.read:project"),
        "three Not now answers for one plugin must stop its prompt"
    );
}

/// REQ: SEC-121
#[test]
fn sec_121_review_dialog_also_names_ilium_as_its_source() {
    assert_eq!(DialogKind::Review.source_label(), "Ilium");
}
