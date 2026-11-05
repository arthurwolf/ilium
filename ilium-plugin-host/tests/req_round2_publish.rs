//! Requirement tests for the PUB rows (legal process, takedown, erasure, availability, review
//! and notice rules) and the CMP rows (compatibility refusals and the deprecation register) that
//! the round 2 coverage audit found without a test.
//!
//! Each test asserts the observable the requirement names. Where no entry point exists in the
//! host, the CLI or the registry client, the test fails with a message that starts with the
//! requirement ID and says what is missing. Nothing here is ignored or gated, and no assertion
//! is weakened.
//!
//! Entry points used:
//! - `ilium_plugin_host::testing::ops::OpsRig` with `PackageSpec` (the CLI rig: `write_package`,
//!   and `cli` for `check --against` and `test --host-contract`), for CMP-022.
//!
//! Not yet built (explicit failures): the registry operator's legal-process, transparency,
//! takedown, erasure, backup, status-page, review-training and quarterly re-review processes
//! (PUB-073 .. PUB-083), the publish-time notice and licence checks (PUB-084, PUB-085), and the
//! deprecation register and its release-note comparison (CMP-023 .. CMP-025).

use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

/// REQ: PUB-073
#[test]
fn pub_073_legal_orders_are_accepted_only_at_legal_contact_and_verified_before_action() {
    panic!(
        "PUB-073 not implemented: the registry operator must accept legal process only at the \
         address in policy.json `legal_contact`, verify the issuing body, official channel and \
         case number of each order before acting, and record an order sent to another address or \
         with an unconfirmed case number as rejected with no action taken"
    );
}

/// REQ: PUB-074
#[test]
fn pub_074_valid_order_discloses_only_the_requested_field_and_is_recorded() {
    panic!(
        "PUB-074 not implemented: a valid order under the policy.json jurisdiction that asks for \
         the contact address of one publisher account must disclose only that field, and the \
         operator's private record must hold the order, its legal basis and its outcome"
    );
}

/// REQ: PUB-075
#[test]
fn pub_075_transparency_report_counts_three_measures_and_names_no_undisclosed_party() {
    panic!(
        "PUB-075 not implemented: the latest transparency report must be dated within 12 months, \
         give a count of legal requests, of takedowns by category and of appeals, and must not \
         name a requester or a publisher id whose data was not disclosed"
    );
}

/// REQ: PUB-076
#[test]
fn pub_076_reported_secret_blob_is_taken_down_within_24_hours_and_tombstoned() {
    panic!(
        "PUB-076 not implemented: after a report that a published blob holds a labelled synthetic \
         token, the blob path must stop serving the blob within 24 hours, plugins/<id>.json must \
         keep the digest and a tombstone (PKG-821), and the transparency log must hold an entry \
         with the reason and the time of the takedown"
    );
}

/// REQ: PUB-077
#[test]
fn pub_077_reinstating_a_leaked_digest_requires_rotation_and_a_clean_replacement_note() {
    panic!(
        "PUB-077 not implemented: reinstating a version with a digest withdrawn for a secret leak \
         must be refused until a rotation record exists, and the replacement version's release \
         notes must state that the earlier version was withdrawn for a secret leak without \
         containing the secret string"
    );
}

/// REQ: PUB-078
#[test]
fn pub_078_erasure_request_removes_account_personal_data_and_keeps_log_fingerprints() {
    panic!(
        "PUB-078 not implemented: a written erasure request from a publisher account holder must \
         be completed within 30 days, leaving no contact address, display name or recovery data \
         for that account, while its transparency log entries keep only key fingerprints and \
         publisher ids"
    );
}

/// REQ: PUB-079
#[test]
fn pub_079_erased_account_is_absent_from_the_first_restore_after_the_request() {
    panic!(
        "PUB-079 not implemented: the operator must state the retention periods of account data, \
         access logs and each backup, and a restore cycle from a backup taken before an erasure \
         request must hold no data of that account except its transparency log entries"
    );
}

/// REQ: PUB-080
#[test]
fn pub_080_status_page_states_monthly_objective_and_posts_outage_notice_within_one_hour() {
    panic!(
        "PUB-080 not implemented: the status page must state a monthly availability objective of \
         99.5 percent for metadata and blob serving, and an outage lasting more than 15 minutes \
         must have an outage notice posted within 1 hour of its detection"
    );
}

/// REQ: PUB-081
#[test]
fn pub_081_outage_keeps_cached_metadata_and_shows_last_successful_refresh() {
    panic!(
        "PUB-081 not implemented: with the registry host blocked, `ilium plugin registry status` \
         must print the time of the last successful refresh, and `ilium plugin install` of a \
         version present in the cache must succeed from the cache"
    );
}

/// REQ: PUB-082
#[test]
fn pub_082_level_b_review_needs_trained_unconflicted_reviewer_and_second_confirmation_of_refusal() {
    panic!(
        "PUB-082 not implemented: a refusal recorded by a first reviewer must leave the version in \
         its previous state until a second reviewer confirms it; a level B review by a reviewer \
         with no training record or with a declared link to the publisher must be refused"
    );
}

/// REQ: PUB-083
#[test]
fn pub_083_quarterly_level_b_re_review_covers_five_percent_by_a_different_reviewer() {
    panic!(
        "PUB-083 not implemented: at the end of a quarter the re-reviews of level B decisions must \
         number at least 5 percent of those decisions, rounded up to a whole decision, each by a \
         reviewer id different from the original reviewer, and the transparency report must state \
         the agreement rate of those re-reviews"
    );
}

/// REQ: PUB-084
#[test]
fn pub_084_publish_with_bundled_asset_lacking_notice_entry_is_refused() {
    panic!(
        "PUB-084 not implemented: publishing a level A package that bundles a font with no NOTICE \
         entry must be refused with registry.policy and the detail `asset-notice-missing`, and \
         the same publish must succeed once the NOTICE entry is added"
    );
}

/// REQ: PUB-085
#[test]
fn pub_085_publish_requiring_a_licence_incompatible_plugin_is_refused() {
    panic!(
        "PUB-085 not implemented: publishing a plugin whose requires.plugins names a plugin with a \
         licence listed in policy.json as incompatible must be refused with registry.policy and \
         the detail `licence-incompatible`, and must succeed once that listing is removed"
    );
}

/// REQ: CMP-022
#[test]
fn cmp_022_check_and_test_refuse_out_of_window_host_with_install_codes() {
    // The rig writes a tier 3 package with no `requires` range (no entry point sets one yet), so
    // this test fails until the rig can build a package whose range excludes the selected host
    // version. The assertions cover the documented codes, exit statuses and the refusal before any
    // case runs.
    let rig = OpsRig::new();
    let spec = PackageSpec::new("com.example.cmp022", "1.0.0").tier(3);
    let path = rig.write_package(&spec);
    let path_text = path.to_string_lossy().into_owned();

    let check = rig.cli(&["check", &path_text, "--against", "0.0.1"]);
    let check_codes_ok = check
        .stdout_jsonl
        .iter()
        .any(|line| line["code"] == "package.api-unsupported")
        && check
            .stdout_jsonl
            .iter()
            .any(|line| line["code"] == "manifest.api-version");
    assert!(
        check_codes_ok && check.exit_code == 3,
        "CMP-022 not implemented: `check --against <host-version>` must report \
         package.api-unsupported and manifest.api-version as error findings and exit 3 when the \
         requires range or component lies outside the host window; exit {} stdout {}",
        check.exit_code,
        check.stdout_raw
    );

    let test = rig.cli(&["test", &path_text, "--host-contract", "0.0.1"]);
    assert!(
        test.exit_code == 1
            && test
                .stdout_jsonl
                .iter()
                .any(|line| line["code"] == "package.api-unsupported"),
        "CMP-022 not implemented: `test --host-contract <host-version>` must refuse with \
         package.api-unsupported and manifest.api-version and exit 1 before any case runs; exit {} \
         stdout {}",
        test.exit_code,
        test.stdout_raw
    );
}

/// REQ: CMP-023
#[test]
fn cmp_023_each_refusal_cell_names_a_command_that_exits_non_zero() {
    panic!(
        "CMP-023 not implemented: every refusal row of the backwards-compatibility test matrix must \
         name `ilium plugin check --against <host-version>` or `ilium plugin test --host-contract \
         <host-version>`, and that command must exit with a non-zero status for the refusal"
    );
}

/// REQ: CMP-024
#[test]
fn cmp_024_each_release_publishes_a_deprecation_register_page_and_matching_json() {
    panic!(
        "CMP-024 not implemented: the latest host and SDK release must publish a deprecation \
         register as a manual page and as a JSON file with the same items, each with kind (sdk, \
         cli, contract or unstable-feature), successor, removal minor and marking release"
    );
}

/// REQ: CMP-025
#[test]
fn cmp_025_register_drops_an_item_only_in_its_removing_release_with_matching_notes() {
    panic!(
        "CMP-025 not implemented: comparing the deprecation register across two releases must show \
         an item leaving the register only in the release that removes it, and that release's \
         notes must name the item exactly as the register does"
    );
}
