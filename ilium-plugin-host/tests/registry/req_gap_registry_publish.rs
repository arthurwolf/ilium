//! Gap-review requirements of the registry-publish lane: review deadlines (PUB-000), takedown of a
//! queued version and its appeal (PUB-042..PUB-044), reporter privacy and abuse intake
//! (PUB-045, PUB-046), reason text rules (PUB-047), dependents notices (PUB-048) and the
//! quarantine withdrawal (PUB-049).
//!
//! These tests target behaviour that is not implemented yet. Each one drives the closest CLI verb
//! that exists today, or the verb the manual names, and asserts the required result, so it fails
//! at runtime rather than at compile time.

use crate::common::*;
use ilium_plugin_host::cli::CliOutput;
use serde_json::Value;

const DAY: i64 = 86_400;
const FRESH: &str = "com.acme.fresh";
const FRESH_VERSION: &str = "com.acme.fresh@1.0.0";

/// Publishes a signed package of `FRESH` with the given version on the standard fixture registry.
fn publish_fresh(sandbox: &Sandbox, version: &str) -> CliOutput {
    let file = format!("{FRESH}-{version}.iliumpkg");
    let path = sandbox.project_file(&file, &signed_package(FRESH, version));
    let path = path.to_str().expect("utf-8 path").to_owned();
    sandbox.run(&["publish", path.as_str(), "--registry", BASE])
}

/// The signed document `plugins/<id>.json` of the served registry, parsed.
fn plugin_entry(sandbox: &Sandbox, plugin_id: &str) -> Value {
    let key = format!("plugins/{plugin_id}.json");
    let tree = sandbox.snapshot();
    let bytes = tree
        .get(key.as_str())
        .unwrap_or_else(|| panic!("{key} must be served"));
    signed_of(bytes)
}

/// Every served document of the registry joined into one lossy UTF-8 text.
fn served_text(tree: &Tree) -> String {
    tree.values()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a served text carries `"<key>":<count>` or `"<key>": <count>`.
fn carries_count(text: &str, key: &str, count: u64) -> bool {
    text.contains(&format!("\"{key}\":{count}")) || text.contains(&format!("\"{key}\": {count}"))
}

// ---- PUB-000 queued review records its time and the review deadline --------------------------

/// REQ: PUB-000
#[test]
fn req_pub_000_queued_review_records_time_and_info_prints_it_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    let entry = plugin_entry(&sandbox, FRESH).to_string();
    assert!(
        entry.contains("review_queued_at"),
        "plugins/<id>.json must carry review_queued_at of a queued-review version: {entry}"
    );
    let info = sandbox.run(&["info", FRESH_VERSION]);
    assert!(
        output_contains(&info, &at(0)),
        "info must print review_queued_at in UTC seconds: {info:?}"
    );
}

/// REQ: PUB-000
#[test]
fn req_pub_000_review_past_deadline_shows_overdue_and_stays_level_a_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    // Well past five working days, which is at least seven calendar days.
    let info = sandbox.run_at(now_plus(12 * DAY), &["info", FRESH_VERSION]);
    assert!(
        output_contains(&info, "Review overdue"),
        "a review past its deadline must show `Review overdue`: {info:?}"
    );
    assert!(
        !output_contains(&info, "\"review_level\":\"B\"")
            && !output_contains(&info, "\"review_level\": \"B\"")
            && !output_contains(&info, "level B"),
        "an overdue review must keep the version at level A: {info:?}"
    );
}

// ---- PUB-042 refusing a queued review is a takedown of that version ------------------------

/// REQ: PUB-042
#[test]
fn req_pub_042_refused_queued_version_is_taken_down_with_tombstone_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    assert_ok(&sandbox.run(&["install", FRESH_VERSION]));
    assert_eq!(
        sandbox.installed_version(FRESH).as_deref(),
        Some("1.0.0"),
        "the client must hold the queued version at level A before the refusal"
    );
    assert_ok(&sandbox.run(&[
        "review",
        "refuse",
        FRESH_VERSION,
        "--rule",
        "malware",
        "--reason",
        "review refused: exfiltrates data",
    ]));
    let refreshed = sandbox.run(&["update", FRESH]);
    assert!(
        output_contains(&refreshed, "disabled-blocked"),
        "the client copy must move to disabled-blocked at the next refresh: {refreshed:?}"
    );
    assert!(
        plugin_entry(&sandbox, FRESH).to_string().contains("tombstone"),
        "plugins/<id>.json must keep a tombstone for the refused version"
    );
    let republish = publish_fresh(&sandbox, "1.0.0");
    assert_code(&republish, "registry.version-exists");
}

/// REQ: PUB-042
#[test]
fn req_pub_042_refusal_reason_and_rule_are_readable_not_rejected_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    assert_ok(&sandbox.run(&[
        "review",
        "refuse",
        FRESH_VERSION,
        "--rule",
        "malware",
        "--reason",
        "review refused: exfiltrates data",
    ]));
    let info = sandbox.run(&["info", FRESH_VERSION, "--json"]);
    assert!(
        output_contains(&info, "review refused: exfiltrates data"),
        "info must show the refusal reason: {info:?}"
    );
    assert!(
        output_contains(&info, "malware"),
        "info must show the policy rule that was applied: {info:?}"
    );
    assert!(
        !output_contains(&info, "rejected"),
        "a refused queued version must not move to rejected with a free number: {info:?}"
    );
}

// ---- PUB-043 an appeal does not suspend a takedown -----------------------------------------

/// REQ: PUB-043
#[test]
fn req_pub_043_pending_appeal_keeps_client_copy_disabled_blocked_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    assert_ok(&sandbox.run(&["install", FRESH_VERSION]));
    assert_ok(&sandbox.run(&["takedown", FRESH_VERSION, "--reason", "malware"]));
    assert_ok(&sandbox.run(&[
        "appeal",
        FRESH,
        "--reason",
        "false positive from the scanner",
        "--account",
        "maintainer",
    ]));
    let refreshed = sandbox.run(&["update", FRESH]);
    assert!(
        output_contains(&refreshed, "disabled-blocked"),
        "a pending appeal must not lift the takedown on the client: {refreshed:?}"
    );
    let revocations = signed_of(&sandbox.snapshot()["revocations.json"]).to_string();
    assert!(
        revocations.contains(FRESH),
        "revocations.json must keep the blocked_ids entry while the appeal is pending"
    );
}

/// REQ: PUB-043
#[test]
fn req_pub_043_successful_appeal_lifts_id_block_and_keeps_tombstones_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    assert_ok(&sandbox.run(&["takedown", FRESH_VERSION, "--reason", "malware"]));
    assert_ok(&sandbox.run(&[
        "appeal",
        FRESH,
        "--reason",
        "false positive from the scanner",
        "--account",
        "maintainer",
    ]));
    assert_ok(&sandbox.run(&["appeal", "decide", FRESH, "--outcome", "successful"]));
    let revocations = signed_of(&sandbox.snapshot()["revocations.json"]).to_string();
    assert!(
        !revocations.contains(FRESH),
        "a successful appeal must remove the id-level blocked_ids entry"
    );
    assert!(
        plugin_entry(&sandbox, FRESH).to_string().contains("1.0.0"),
        "every version tombstone must survive a successful appeal"
    );
    assert_ok(&publish_fresh(&sandbox, "1.1.0"));
}

// ---- PUB-044 only a maintainer of the publisher id may appeal -----------------------------

/// REQ: PUB-044
#[test]
fn req_pub_044_appeal_from_non_maintainer_is_refused_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    assert_ok(&sandbox.run(&["takedown", FRESH_VERSION, "--reason", "malware"]));
    let stranger = sandbox.run(&[
        "appeal",
        FRESH,
        "--reason",
        "not my plugin",
        "--account",
        "stranger",
    ]);
    assert_ne!(
        stranger.exit_code, 0,
        "an appeal from a non-maintainer must be refused: {stranger:?}"
    );
    let maintainer = sandbox.run(&[
        "appeal",
        FRESH,
        "--reason",
        "the maintainer's appeal",
        "--account",
        "maintainer",
    ]);
    assert_ok(&maintainer);
}

/// REQ: PUB-044
#[test]
fn req_pub_044_two_operator_appeal_by_approving_operator_is_refused_b() {
    let sandbox = Sandbox::standard();
    sandbox.set_registry_setting(
        "operators",
        toml::Value::Array(vec![
            toml::Value::String("op-one".to_owned()),
            toml::Value::String("op-two".to_owned()),
        ]),
    );
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    assert_ok(&sandbox.run(&[
        "takedown",
        FRESH_VERSION,
        "--reason",
        "malware",
        "--operator",
        "op-one",
        "--approve-by",
        "op-two",
    ]));
    assert_ok(&sandbox.run(&[
        "appeal",
        FRESH,
        "--reason",
        "false positive",
        "--account",
        "maintainer",
    ]));
    let decision = sandbox.run(&[
        "appeal",
        "decide",
        FRESH,
        "--outcome",
        "upheld",
        "--operator",
        "op-one",
    ]);
    assert_ne!(
        decision.exit_code, 0,
        "the operator who approved the takedown must not decide its appeal: {decision:?}"
    );
}

// ---- PUB-045 an abuse reporter stays out of every served document --------------------------

const REPORTER: &str = "reporter-unique-7f3a@example.net";
const REPORT_TEXT: &str = "unique report text 9d2e4c for the privacy check";

/// REQ: PUB-045
#[test]
fn req_pub_045_reporter_identity_absent_from_served_documents_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    assert_ok(&sandbox.run(&[
        "report",
        FRESH_VERSION,
        "--rule",
        "malware",
        "--reporter",
        REPORTER,
        "--text",
        REPORT_TEXT,
    ]));
    let takedown = sandbox.run(&["takedown", FRESH_VERSION, "--reason", "malware"]);
    assert_ok(&takedown);
    let served = served_text(&sandbox.snapshot());
    assert!(
        !served.contains(REPORTER) && !served.contains(REPORT_TEXT),
        "no served document, log entry or notice may hold the reporter or the report text"
    );
    assert!(
        !output_contains(&takedown, REPORTER) && !output_contains(&takedown, REPORT_TEXT),
        "the maintainer notice must not hold the reporter or the report text: {takedown:?}"
    );
}

/// REQ: PUB-045
#[test]
fn req_pub_045_operator_private_record_holds_reporter_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    assert_ok(&sandbox.run(&[
        "report",
        FRESH_VERSION,
        "--rule",
        "malware",
        "--reporter",
        REPORTER,
        "--text",
        REPORT_TEXT,
    ]));
    assert_ok(&sandbox.run(&["takedown", FRESH_VERSION, "--reason", "malware"]));
    let private = sandbox.run(&["registry", "reports", "--json"]);
    assert!(
        output_contains(&private, REPORTER),
        "the operator's private record must hold the reporter address: {private:?}"
    );
}

// ---- PUB-046 abuse intake acknowledges and reports the decision ----------------------------

/// REQ: PUB-046
#[test]
fn req_pub_046_abuse_report_is_acknowledged_with_report_id_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    let report = sandbox.run(&[
        "abuse",
        "report",
        FRESH_VERSION,
        "--rule",
        "malware",
        "--reporter",
        REPORTER,
        "--reason",
        "the package contains a credential",
    ]);
    assert_ok(&report);
    assert!(
        output_contains(&report, "report_id"),
        "the acknowledgement must carry a report id: {report:?}"
    );
}

/// REQ: PUB-046
#[test]
fn req_pub_046_decision_is_logged_as_abuse_decision_with_index_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&publish_fresh(&sandbox, "1.0.0"));
    assert_ok(&sandbox.run(&[
        "abuse",
        "report",
        FRESH_VERSION,
        "--rule",
        "malware",
        "--reporter",
        REPORTER,
        "--reason",
        "the package contains a credential",
    ]));
    let decision = sandbox.run(&[
        "takedown",
        FRESH_VERSION,
        "--reason",
        "malware",
        "--report-rule",
        "malware",
    ]);
    assert_ok(&decision);
    assert!(
        output_contains(&decision, "log"),
        "the reporter must receive the log entry index of the decision: {decision:?}"
    );
    let tree = sandbox.snapshot();
    assert!(
        tree.iter().any(|(path, bytes)| path.starts_with("log/entries/")
            && String::from_utf8_lossy(bytes).contains("abuse-decision")),
        "log/entries/<index>.json must carry type abuse-decision"
    );
}

// ---- PUB-047 reason texts are plain text and bounded ---------------------------------------

/// REQ: PUB-047
#[test]
fn req_pub_047_reason_with_control_character_is_refused_a() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&[
        "yank",
        "com.acme.notes@0.3.0",
        "--reason",
        "bug: colour \u{1b}[31m escape",
    ]);
    assert_code(&output, "registry.reason-invalid");
}

/// REQ: PUB-047
#[test]
fn req_pub_047_link_like_reason_is_plain_and_truncated_b() {
    let sandbox = Sandbox::standard();
    let long_tail = "END-MARKER";
    let reason = format!(
        "bug: see https://example.org/notes ]8;;http://evil.example/ link {}{long_tail}",
        "y".repeat(500)
    );
    assert_ok(&sandbox.run(&["yank", "com.acme.notes@0.3.0", "--reason", reason.as_str()]));
    let info = sandbox.run(&["info", "com.acme.notes@0.3.0"]);
    assert!(
        !output_contains(&info, "\u{1b}"),
        "info must not print terminal escape bytes: {info:?}"
    );
    assert!(
        output_contains(&info, "https://example.org/notes"),
        "the reason text must still be shown as plain text: {info:?}"
    );
    assert!(
        !output_contains(&info, long_tail),
        "a reason longer than 500 scalar values must be truncated on screen: {info:?}"
    );
}

// ---- PUB-048 takedowns notify maintainers of dependents and count them ---------------------

/// REQ: PUB-048
#[test]
fn req_pub_048_takedown_notifies_dependent_maintainers_a() {
    let base = PluginSpec::new("com.acme.base")
        .named("Base")
        .with_versions(vec![VersionSpec::signed("com.acme.base", "1.0.0")]);
    let dependent = PluginSpec::new("com.acme.dep").named("Dependent").with_versions(vec![
        VersionSpec::signed("com.acme.dep", "1.0.0").depends_on("com.acme.base", "=1.0.0"),
    ]);
    let sandbox = Sandbox::with_registry(&RegistryBuilder::with_plugins(vec![base, dependent]));
    let takedown = sandbox.run(&["takedown", "com.acme.base@1.0.0", "--reason", "malware"]);
    assert_ok(&takedown);
    assert!(
        output_contains(&takedown, "com.acme.dep"),
        "the maintainers of a dependent must receive a notice: {takedown:?}"
    );
    assert!(
        carries_count(&served_text(&sandbox.snapshot()), "dependents", 1),
        "the takedown's transparency log entry must record dependents 1"
    );
}

/// REQ: PUB-048
#[test]
fn req_pub_048_takedown_without_dependents_records_zero_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]));
    assert!(
        carries_count(&served_text(&sandbox.snapshot()), "dependents", 0),
        "a takedown with no dependents must record dependents 0 in its log entry"
    );
}

// ---- PUB-049 quarantine is withdrawn after a decision ---------------------------------------

/// REQ: PUB-049
#[test]
fn req_pub_049_quarantine_withdrawn_within_a_day_of_decision_none_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&[
        "quarantine",
        "com.acme.notes",
        "--range",
        ">=0.3.0 <0.4.0",
    ]));
    assert_ok(&sandbox.run(&[
        "abuse",
        "decide",
        "--decision",
        "none",
        "--reason",
        "no violation found",
    ]));
    let swept = sandbox.run_at(now_plus(DAY + 60), &["registry", "sweep"]);
    assert_ok(&swept);
    let quarantine = signed_of(&sandbox.snapshot()["quarantine.json"]).to_string();
    assert!(
        quarantine.contains("withdrawn"),
        "quarantine.json must carry a withdrawn entry 24 hours after decision none: {quarantine}"
    );
}

/// REQ: PUB-049
#[test]
fn req_pub_049_takedown_withdraws_quarantine_in_same_snapshot_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&[
        "quarantine",
        "com.acme.notes",
        "--range",
        ">=0.3.0 <0.4.0",
    ]));
    assert_ok(&sandbox.run(&[
        "takedown",
        "com.acme.notes@0.3.0",
        "--reason",
        "malware",
        "--report-rule",
        "malware",
    ]));
    let tree = sandbox.snapshot();
    let revocations = signed_of(&tree["revocations.json"]).to_string();
    let quarantine = signed_of(&tree["quarantine.json"]).to_string();
    assert!(
        revocations.contains("com.acme.notes"),
        "the takedown must add the blocked_ids entry"
    );
    assert!(
        quarantine.contains("withdrawn"),
        "the quarantine must be withdrawn in the same metadata change as the blocked_ids entry"
    );
}
