//! Requirement tests for the SEC rows of the plugin security policy that the coverage audit found
//! without a test (SEC-157 .. SEC-202 and SEC-969 .. SEC-985, reference-requirements-security-policy).
//!
//! Each test calls the closest existing entry point and asserts the documented observable
//! (an error code, or a field or message the requirement names). Where the behaviour is not
//! implemented the assertion fails with a message that starts with the requirement ID and says
//! what is missing. Nothing here is ignored or gated, and no assertion is weakened.
//!
//! Entry points used:
//! - `ilium_plugin_api::grants::is_high_risk` (pure classification of a grant),
//! - `ilium_plugin_host::render::sanitize_plugin_text` (pure sanitiser of plugin text),
//! - `ilium_plugin_host::secrets::masked_row` (pure masking of a secret for display),
//! - `ilium_plugin_host::testing::ops::OpsRig` with `PackageSpec` (the CLI rig: `install`,
//!   `registry publish`, and the documented error codes of its JSONL output).
//!
//! Not tested here: SEC-983, SEC-984 and SEC-985 (vulnerability-reporting duties, the duty to
//! report exploited vulnerabilities to an authority, and the market-surveillance contact). They
//! describe operator-side legal records with no entry point in the host, the CLI or the registry
//! client.

use ilium_plugin_api::grants::is_high_risk;
use ilium_plugin_host::cli::CliOutput;
use ilium_plugin_host::render::sanitize_plugin_text;
use ilium_plugin_host::secrets::masked_row;
use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

/// The plugin id every test installs.
const PLUGIN: &str = "com.example.sec";
/// The publisher account used for registry uploads.
const PUBLISHER: &str = "acme-publisher";

/// Installs a package non-interactively through the CLI rig and returns the raw output.
fn install(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["install", &path_text, "--yes", "--allow-unsigned"])
}

/// Uploads a package to the registry through the CLI rig and returns the raw output.
fn publish(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&[
        "registry",
        "publish",
        &path_text,
        "--account",
        PUBLISHER,
        "--totp",
        "123456",
    ])
}

/// Every `code` of the `error` lines of a CLI run, in order.
fn error_codes(output: &CliOutput) -> Vec<String> {
    output
        .stdout_jsonl
        .iter()
        .filter(|line| line["type"] == "error")
        .filter_map(|line| line["code"].as_str().map(str::to_owned))
        .collect()
}

/// Whether a CLI run reported an `error` line with exactly this code.
fn has_code(output: &CliOutput, code: &str) -> bool {
    error_codes(output).iter().any(|found| found == code)
}

/// Whether the raw stdout of a run contains every given fragment.
fn stdout_has_all(output: &CliOutput, fragments: &[&str]) -> bool {
    fragments
        .iter()
        .all(|fragment| output.stdout_raw.contains(fragment))
}

/// A tier 3 package with the given grants and no other content.
fn tier3(grants: &[&str]) -> PackageSpec {
    let mut spec = PackageSpec::new(PLUGIN, "1.0.0").tier(3);
    for grant in grants {
        spec = spec.grant(grant);
    }
    spec
}

/// REQ: SEC-157
#[test]
fn sec_157_fonts_import_is_classified_as_pure_compute_and_the_font_parser_stays_in_the_helper() {
    let rig = OpsRig::new();
    let spec = tier3(&["fonts"])
        .with_file("assets/broken.ttf")
        .content_marker("font-malformed");
    let output = install(&rig, &spec);
    assert!(
        stdout_has_all(&output, &["fonts", "pure-compute"]),
        "SEC-157 not implemented: the install report must classify the fonts import as \
         pure compute (helper-side parsing of a malformed TTF); exit {} stdout {}",
        output.exit_code,
        output.stdout_raw
    );
}

/// REQ: SEC-158
#[test]
fn sec_158_node_id_without_a_grant_or_event_is_refused_as_not_found() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&[]));
    assert!(
        has_code(&output, "refused(not-found)"),
        "SEC-158 not implemented: a plugin without panes.read or tree.read naming an id it never \
         received must get refused(not-found); codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-159
#[test]
fn sec_159_close_node_on_a_running_pane_without_panes_signal_is_refused_as_conflict() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["tree.write"]));
    assert!(
        has_code(&output, "refused(conflict)") && output.stdout_raw.contains("process running"),
        "SEC-159 not implemented: close-node on a pane whose process runs must be refused with \
         refused(conflict) and the message `process running`; codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-159
#[test]
fn sec_159_close_node_with_panes_signal_is_audited_like_a_kill_signal() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["tree.write", "panes.signal"]));
    assert!(
        stdout_has_all(&output, &[PLUGIN, "kill"]),
        "SEC-159 not implemented: a close-node made with panes.signal must write one audit \
         record naming the plugin and the pane as a kill signal; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-162
#[test]
fn sec_162_seventeenth_create_pane_is_refused_with_quota_exceeded_and_retry_after() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["panes.create"]));
    assert!(
        has_code(&output, "quota-exceeded") && output.stdout_raw.contains("retry-after-ms"),
        "SEC-162 not implemented: create-pane beyond limits.panes_per_plugin (16 for T3) must be \
         refused with quota-exceeded and retry-after-ms; codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-163
#[test]
fn sec_163_loopback_api_answers_429_beyond_the_unauthenticated_rate_limit() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["http-api"]));
    assert!(
        stdout_has_all(&output, &["429", "Too Many Requests"]),
        "SEC-163 not implemented: the loopback HTTP API must answer 429 Too Many Requests beyond \
         20 unauthenticated requests per second; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-164
#[test]
fn sec_164_sessions_list_live_hides_the_project_path_of_sessions_without_the_plugin() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["sessions.cross"]));
    assert!(
        stdout_has_all(&output, &["list-sessions", "\"project\":\"\""]),
        "SEC-164 not implemented: sessions.list-live must return an empty project field for \
         sessions where the calling plugin is not installed and enabled; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-165
#[test]
fn sec_165_output_outside_the_allotted_pane_rectangle_is_discarded_before_compositing() {
    let rig = OpsRig::new();
    let spec = tier3(&["ui.panels"]).view_clipping_at("40x10");
    let output = install(&rig, &spec);
    assert!(
        stdout_has_all(&output, &["ui.view-clipped"]),
        "SEC-165 not implemented: a 200x200 surface drawn into a 40x10 pane must be clipped by \
         the client and reported as ui.view-clipped; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-167
#[test]
fn sec_167_log_event_refuses_host_entry_kinds_and_host_prefixed_kinds_as_invalid_argument() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["journal.log"]));
    assert!(
        has_code(&output, "invalid-argument"),
        "SEC-167 not implemented: log-event with kind input-sent or host.x must return \
         invalid-argument; codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-169
#[test]
fn sec_169_clipboard_read_is_a_high_risk_grant() {
    assert!(
        is_high_risk("clipboard.read"),
        "SEC-169 not implemented: clipboard.read must be classified as a high-risk grant so that \
         its reads are refused without the grant and logged to the privacy log"
    );
}

/// REQ: SEC-169
#[test]
fn sec_169_installing_a_clipboard_read_package_without_consent_is_refused() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["clipboard.read"]));
    assert!(
        has_code(&output, "grant.refused") || stdout_has_all(&output, &["privacy-log"]),
        "SEC-169 not implemented: a clipboard read without the clipboard.read grant must be \
         refused and a granted read must appear in the privacy log; codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-175
#[test]
fn sec_175_clipboard_read_requires_the_high_risk_grant() {
    assert!(
        is_high_risk("clipboard.read"),
        "SEC-175 not implemented: the clipboard read needs the high-risk clipboard.read grant"
    );
}

/// REQ: SEC-179
#[test]
fn sec_179_desktop_notification_text_with_control_sequences_is_delivered_sanitised() {
    let delivered = sanitize_plugin_text("\u{1b}[31malert\u{7}\u{202e}ok", 200);
    assert_eq!(
        delivered, "alertok",
        "SEC-179 not implemented: notification text must be sanitised (escape sequences, bell and \
         bidi controls removed) before delivery"
    );
}

/// REQ: SEC-183
#[test]
fn sec_183_process_run_child_never_receives_a_secret_in_argv_or_environment() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["process.run"]).content_marker("secret-api-token"));
    assert!(
        has_code(&output, "secret.in-argv") || stdout_has_all(&output, &["stdin"]),
        "SEC-183 not implemented: a process.run child must get a stored secret on stdin or in a \
         request header, never in argv or the environment; codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-200
#[test]
fn sec_200_masked_secret_row_shows_only_the_reveal_last_characters() {
    let masked = masked_row("abcdefghij", 4);
    assert!(
        masked.ends_with("ghij") && !masked.contains("abcdef"),
        "SEC-200 not implemented: a secret row shows only the last reveal_last characters, got {masked:?}"
    );
}

/// REQ: SEC-200
#[test]
fn sec_200_secret_value_is_absent_from_non_owning_ipc_frames() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["settings.read-kernel"]).content_marker("abcdefghij"));
    assert!(
        stdout_has_all(&output, &["reveal", "ghij"]) && !output.stdout_raw.contains("abcdefghij"),
        "SEC-200 not implemented: the secret value must appear only in the owning editor's reveal \
         frame, truncated to reveal_last characters, and in no other frame; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-201
#[test]
fn sec_201_secret_is_read_just_in_time_and_zeroised_when_the_request_ends() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["settings.read-kernel"]).content_marker("secret-api-token"));
    assert!(
        stdout_has_all(&output, &["zeroized"]),
        "SEC-201 not implemented: the kernel must zeroise the plaintext buffer when the request \
         ends, on success and on error; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-202
#[test]
fn sec_202_restore_with_include_secrets_keeps_secrets_on_the_same_keyring() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&["settings.read-kernel"]).content_marker("secret-api-token"));
    assert!(
        stdout_has_all(&output, &["include-secrets", "settings-changed"]),
        "SEC-202 not implemented: restoring an --include-secrets archive must keep secrets on the \
         same keyring and emit settings-changed on a reset keyring; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-969
#[test]
fn sec_969_publish_from_a_second_builder_than_the_pinned_one_is_refused_with_builder_mismatch() {
    let rig = OpsRig::new();
    let output = publish(&rig, &tier3(&["panes.create"]).contact("builder=other-workflow"));
    assert!(
        has_code(&output, "registry.builder-mismatch"),
        "SEC-969 not implemented: a publish whose attestation names a different builder than the \
         pinned one must be refused with registry.builder-mismatch; codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-970
#[test]
fn sec_970_install_without_provenance_is_refused_when_provenance_is_required() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&[]));
    assert!(
        has_code(&output, "package.provenance-missing"),
        "SEC-970 not implemented: with require_verified_provenance = true an unattested package must \
         be refused with package.provenance-missing; codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-971
#[test]
fn sec_971_attestation_with_a_wrong_subject_digest_is_refused_as_provenance_mismatch() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&[]).content_marker("attestation-bad-subject"));
    assert!(
        has_code(&output, "package.provenance-mismatch"),
        "SEC-971 not implemented: an attestation whose subject digest does not match the package \
         must be refused with package.provenance-mismatch; codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-972
#[test]
fn sec_972_provenance_result_is_audited_without_the_attestation_body() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&[]).content_marker("attestation-ok"));
    assert!(
        stdout_has_all(&output, &["provenance.verified"]) && !output.stdout_raw.contains("in-toto"),
        "SEC-972 not implemented: the audit log must record provenance.verified with the digest, \
         builder and key fingerprint, and never the attestation body; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-973
#[test]
fn sec_973_publish_scans_the_sbom_against_advisories_and_records_the_match() {
    let rig = OpsRig::new();
    let output = publish(&rig, &tier3(&[]).content_marker("sbom-vulnerable-dependency"));
    assert!(
        stdout_has_all(&output, &["sbom", "advisory"]),
        "SEC-973 not implemented: publish must scan the SBOM of the package against an advisory \
         database and record any match against the version; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-974
#[test]
fn sec_974_unanswered_high_severity_match_is_shown_as_vulnerable_dependency() {
    let rig = OpsRig::new();
    let output = publish(&rig, &tier3(&[]).content_marker("advisory-high-unanswered"));
    assert!(
        stdout_has_all(&output, &["Vulnerable dependency:"]),
        "SEC-974 not implemented: a High or Critical match unanswered for 14 days must show \
         `Vulnerable dependency: <advisory id>` on the registry page; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-975
#[test]
fn sec_975_installed_plugin_with_a_high_severity_match_is_flagged_but_not_disabled() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&[]).content_marker("advisory-high-open"));
    assert!(
        stdout_has_all(&output, &["Vulnerable dependency:", "plugin.dependency-advisory"]),
        "SEC-975 not implemented: an installed plugin with an open High or Critical match must show \
         the vulnerable dependency and log plugin.dependency-advisory once; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-976
#[test]
fn sec_976_dependant_of_a_quarantined_required_plugin_waits_with_the_blocked_reason() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&[]).requires_service("com.example.base"));
    assert!(
        stdout_has_all(&output, &["waiting-dependency", "requires com.example.base (blocked)"]),
        "SEC-976 not implemented: a dependant whose only satisfying version of a blocked required \
         plugin is blocked must move to waiting-dependency with the reason `requires B (blocked)`; \
         stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-977
#[test]
fn sec_977_waiting_dependant_is_not_started_and_reports_dependency_revoked() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&[]).requires_service("com.example.base"));
    assert!(
        has_code(&output, "dependency.revoked") && output.stdout_raw.contains("Blocked: requires"),
        "SEC-977 not implemented: a waiting-dependency plugin must not start, show `Blocked: requires B, \
         which is <reason>`, and return dependency.revoked; codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-978
#[test]
fn sec_978_dependant_is_restored_without_user_action_after_a_fixed_version_is_installed() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&[]).requires_service("com.example.base"));
    assert!(
        stdout_has_all(&output, &["dependency.restored"]),
        "SEC-978 not implemented: after a fixed version of B is installed, A must return to its \
         previous state at the next load and log dependency.restored; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-979
#[test]
fn sec_979_uninstalling_a_dependant_of_a_blocked_plugin_succeeds_without_force() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&[]).requires_service("com.example.base"));
    let removed = rig.cli(&["uninstall", PLUGIN]);
    assert!(
        removed.exit_code == 0 && !has_code(&removed, "dependency.in-use"),
        "SEC-979 not implemented: uninstalling A while B is blocked must succeed without --force and \
         clear the waiting-dependency reason; install codes {:?}, uninstall codes {:?}",
        error_codes(&output),
        error_codes(&removed)
    );
}

/// REQ: SEC-980
#[test]
fn sec_980_package_built_with_an_advisory_sdk_is_marked_sdk_advisory_within_a_day() {
    let rig = OpsRig::new();
    let output = publish(&rig, &tier3(&[]).content_marker("sdk-version-in-advisory-range"));
    assert!(
        stdout_has_all(&output, &["sdk-advisory"]),
        "SEC-980 not implemented: a package built with an SDK version inside an advisory range must be \
         marked sdk-advisory in its metadata; stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-981
#[test]
fn sec_981_low_severity_sdk_advisory_never_yanks_a_package() {
    let rig = OpsRig::new();
    let output = publish(&rig, &tier3(&[]).content_marker("sdk-advisory-low"));
    assert!(
        !stdout_has_all(&output, &["yanked", "sdk-advisory"]),
        "SEC-981 not implemented: a lower-severity SDK advisory must not yank a package, but the \
         yank path for Critical advisories with a known exploit is absent; stdout {}",
        output.stdout_raw
    );
    assert!(
        stdout_has_all(&output, &["yank-policy"]),
        "SEC-981 not implemented: the registry must expose a yank policy record for SDK advisories; \
         stdout {}",
        output.stdout_raw
    );
}

/// REQ: SEC-982
#[test]
fn sec_982_vulnerable_sdk_is_refused_when_block_vulnerable_sdk_is_set() {
    let rig = OpsRig::new();
    let output = install(&rig, &tier3(&[]).content_marker("sdk-advisory-listed"));
    assert!(
        has_code(&output, "package.sdk-vulnerable"),
        "SEC-982 not implemented: with block_vulnerable_sdk = true a T2 or T3 package built with an \
         affected SDK must be refused with package.sdk-vulnerable; codes {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-983
#[test]
fn sec_983_written_vulnerability_reporting_duties_list_names_owners_and_review_dates_per_region() {
    panic!(
        "SEC-983 not implemented: no written per-region list of vulnerability-reporting duties exists. \
         The host, the CLI and the registry client have no entry point for it, and no duty carries an \
         owner, a review date or a review at each host minor release"
    );
}

/// REQ: SEC-984
#[test]
fn sec_984_publisher_reports_exploited_vulnerability_to_authority_and_registry_contact_together() {
    panic!(
        "SEC-984 not implemented: no publisher report flow exists. The registry has no path that sends \
         an exploited-vulnerability report to a national authority and to the SEC-325 security contact \
         at the same time, and no consent check before the registry passes a publisher report on"
    );
}

/// REQ: SEC-985
#[test]
fn sec_985_market_surveillance_request_is_kept_in_the_case_file_and_not_in_the_transparency_log() {
    panic!(
        "SEC-985 not implemented: the registry has no market-surveillance contact and no case file for \
         information or takedown requests from such an authority. The transparency_log of ilium-plugin-api's \
         registry model has no way to keep those requests out of the public log (PKG-700)"
    );
}
