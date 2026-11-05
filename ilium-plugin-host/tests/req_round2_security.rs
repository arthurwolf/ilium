//! Requirement tests for the second round of SEC rows of the plugin security policy
//! (SEC-203 .. SEC-208, SEC-986 .. SEC-999, reference-requirements-*.md).
//!
//! Each test is red on purpose: no entry point for the required behaviour exists in the host, the
//! CLI, the registry client or the kernel audit log yet, so each test fails with a message that
//! starts with the requirement ID and says what the requirement needs. The test bodies describe
//! the observable the requirement names (an error code, a dialog text, a file, an exit code) so
//! that the failure becomes a real assertion once the feature is built. No assertion is ignored
//! or gated, and no test is weakened to pass.
//!
//! Rows that describe documentation or owner-register content (SEC-205, SEC-206 list part,
//! SEC-989, SEC-990, SEC-991) are checked by a scan of the manual pages once the scan exists; until
//! then they fail with the ID named.

/// REQ: SEC-203
#[test]
fn sec_203_terminal_history_dialog_warns_about_secrets_and_names_grant_for_non_project_pane() {
    panic!(
        "SEC-203 not implemented: the review dialog for terminal.history must state that the text \
         can contain secrets the user typed or pasted; for a non-project pane target the \
         high-risk text must also name the grant terminal.history; for a project pane only the \
         secret warning is shown"
    );
}

/// REQ: SEC-204
#[test]
fn sec_204_scrollback_replaces_plugin_secret_values_with_redacted_and_leaves_other_tokens() {
    panic!(
        "SEC-204 not implemented: scrollback text returned to a plugin must replace each value \
         equal to a current plugin secret with [redacted], and must return other secret-like text \
         (an unrelated token printed by a command) unchanged, without claiming redaction for it"
    );
}

/// REQ: SEC-205
#[test]
fn sec_205_design_decision_d5_lists_sandbox_criteria_owner_and_revisit_trigger() {
    panic!(
        "SEC-205 not implemented: design decision D5 must list the three acceptance criteria for a \
         macOS or Windows sandbox backend (the escape test suite, the resource limits and the \
         SEC-057 table row), an owner line deciding on them, and the revisit trigger that the \
         completion of all three criteria; each criterion must name its test or table row"
    );
}

/// REQ: SEC-206
#[test]
fn sec_206_sandbox_page_lists_t2_t3_core_features_and_install_refusal_names_package() {
    panic!(
        "SEC-206 not implemented: the reference-sandbox page must list every core feature that \
         needs a T2 or T3 plugin, or state `none`; on Windows and macOS installing a T2 package \
         must be refused by the install dialog naming the package id and version with the code \
         sandbox.unavailable"
    );
}

/// REQ: SEC-207
#[test]
fn sec_207_min_review_level_refuses_lower_review_level_with_policy_code() {
    panic!(
        "SEC-207 not implemented: packages.min_review_level (A or B) must refuse install and update \
         of a version whose review_level is lower than the setting with policy.review-level-too-low, \
         must accept a level B version when the setting is B, and must not disable an installed copy"
    );
}

/// REQ: SEC-208
#[test]
fn sec_208_require_verified_refuses_unverified_publisher_only_when_enabled() {
    panic!(
        "SEC-208 not implemented: with publishers.require_verified = true, install and update must \
         refuse a publisher whose publisher_verified is false with policy.publisher-unverified; with \
         the key false (the default) the flag must not affect install"
    );
}

/// REQ: SEC-986
#[test]
fn sec_986_audit_log_writes_signed_checkpoints_at_start_and_every_thousand_records() {
    panic!(
        "SEC-986 not implemented: the kernel must write a signed checkpoint record to audit.jsonl \
         at each server start and at least every 1,000 records, carrying the last seq, the last \
         hash and a signature made with a key held in the secrets store; after 2,500 records there \
         must be checkpoints at start, at or after seq 1000 and at or after seq 2000"
    );
}

/// REQ: SEC-987
#[test]
fn sec_987_audit_verify_reports_missing_or_invalid_checkpoint_and_exits_three() {
    panic!(
        "SEC-987 not implemented: `ilium plugin audit --verify` must check every checkpoint; a \
         deleted checkpoint covering seq 1000 must print `checkpoint-missing 1000`, a changed byte \
         in a checkpoint signature must print `checkpoint-invalid` with its seq, and both must exit \
         with code 3"
    );
}

/// REQ: SEC-988
#[test]
fn sec_988_encrypt_at_rest_refuses_writes_without_keyring_and_keeps_reading() {
    panic!(
        "SEC-988 not implemented: the opt-in [data] encrypt_at_rest = true must encrypt plugin data, \
         snapshots, plugin logs and backup archives with a key in the OS keyring; with the keyring \
         locked at start the host must not create those files, must keep reading existing files and \
         must report data.keyring-unavailable; after unlock and restart the datum must be written \
         encrypted and read back"
    );
}

/// REQ: SEC-989
#[test]
fn sec_989_doctor_prints_encrypt_at_rest_default_false_matching_settings_table() {
    panic!(
        "SEC-989 not implemented: starting with an empty config.toml, `ilium plugin doctor` must \
         print `encrypt_at_rest false`, and that default must match the reference-settings-schema \
         table row and the owner decision recorded on the Secrets page"
    );
}

/// REQ: SEC-990
#[test]
fn sec_990_every_proposed_owner_to_confirm_value_has_one_open_decisions_row() {
    panic!(
        "SEC-990 not implemented: every security value marked `proposed, owner to confirm` in the \
         security pages must appear exactly once in reference-security-open-decisions with its ID, \
         the proposed value, the owner and a due release"
    );
}

/// REQ: SEC-991
#[test]
fn sec_991_release_checklist_fails_and_names_any_open_decisions_row() {
    panic!(
        "SEC-991 not implemented: with one row of the open-decisions register having no confirmed \
         value, the release checklist must fail and must name that row; it must list every open row"
    );
}

/// REQ: SEC-992
#[test]
fn sec_992_registry_sensitive_actions_require_second_factor_and_refuse_password_only_session() {
    panic!(
        "SEC-992 not implemented: creating a publish token, adding a maintainer, rotating a token or \
         transferring a namespace must require a second factor (TOTP or WebAuthn); a password-only \
         session must be refused with registry.second-factor-required, and after the second factor \
         the token must be created"
    );
}

/// REQ: SEC-993
#[test]
fn sec_993_publisher_session_expires_twelve_hours_after_second_factor_check() {
    panic!(
        "SEC-993 not implemented: a publisher web or API session must expire 12 hours after its last \
         second-factor check; after advancing the registry clock by 12 hours and one second a token \
         rotation must be refused with registry.session-expired"
    );
}

/// REQ: SEC-994
#[test]
fn sec_994_sign_refuses_unprotected_key_unless_allow_plain_key_writes_audit_line() {
    panic!(
        "SEC-994 not implemented: `ilium plugin sign` must refuse a publisher.key readable by group \
         or others (sign.key-permissions) or without a passphrase (sign.key-unprotected) for a \
         mode 0644 plain key; with --allow-plain-key it must write the signature and one audit line \
         with the code sign-plain-key on standard error"
    );
}

/// REQ: SEC-995
#[test]
fn sec_995_doctor_reports_key_custody_plain_or_passphrase_for_each_publisher_key() {
    panic!(
        "SEC-995 not implemented: the publishing guide must say a registry publisher key SHOULD be \
         hardware-backed or passphrase-protected, and `ilium plugin doctor` must report each \
         publisher.key in the key folder as `custody hardware`, `custody passphrase` or \
         `custody plain`; a plain key and a passphrase key must show `custody plain` and \
         `custody passphrase`"
    );
}

/// REQ: SEC-996
#[test]
fn sec_996_high_risk_grant_held_365_days_shows_review_due_once_per_start() {
    panic!(
        "SEC-996 not implemented: a high-risk grant with a decision time 366 days in the past must \
         show the badge `review due` on the Plugins screen and write one grant-review-due audit \
         record per plugin per server start; two starts must give two records"
    );
}

/// REQ: SEC-997
#[test]
fn sec_997_review_due_badge_does_not_change_grant_and_set_expiry_stores_expiry() {
    panic!(
        "SEC-997 not implemented: showing review due must not revoke or change the grant; Keep must \
         clear the badge with the grant unchanged; Set expiry for one year must store that expiry \
         for the grant as SEC-717 defines it; the kernel-drawn dialog offers Keep, Revoke and Set \
         expiry"
    );
}

/// REQ: SEC-998
#[test]
fn sec_998_agent_transcript_dialog_warns_about_secrets_and_names_grant_for_non_project_pane() {
    panic!(
        "SEC-998 not implemented: the review dialog for agent.transcript must state that the text can \
         contain secrets the user typed or pasted; for a non-project pane target the high-risk text \
         must also name the grant agent.transcript; for a project pane only the secret warning is \
         shown"
    );
}

/// REQ: SEC-999
#[test]
fn sec_999_transcript_replaces_plugin_secret_values_with_redacted_and_leaves_other_tokens() {
    panic!(
        "SEC-999 not implemented: transcript text returned to a plugin must replace each value equal \
         to a current plugin secret with [redacted], and must return an unrelated pasted token \
         unchanged without claiming redaction for it"
    );
}
