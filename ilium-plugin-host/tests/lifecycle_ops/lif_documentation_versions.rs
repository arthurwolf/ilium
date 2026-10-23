//! Documentation snapshot and correction rules (LIF-960..975).
//!
//! Every row is a documentation process rule written in `reference-documentation-versions`. Each
//! test reads that page and asserts the concrete wording of the rule: the paths, the retention
//! span, the correction routes, the advisory and support statements. Publishing a snapshot on the
//! website is a release check, not something the host can drive, so these tests check the text
//! the release process relies on.

use crate::common::*;

/// The documentation-versions page text.
fn versions_page() -> String {
    manual_page("reference-documentation-versions")
}

/// Asserts that the page contains `phrase` and names the requirement in the message.
fn assert_states(page: &str, phrase: &str, id: &str) {
    assert!(
        page.contains(phrase),
        "{id}: the documentation-versions page must state: {phrase}"
    );
}

/// REQ: LIF-960
#[test]
fn lif_960_each_host_release_has_one_snapshot_of_its_own() {
    let page = versions_page();
    assert_states(
        &page,
        "Each host release MUST be published with one manual snapshot of its own.",
        "LIF-960",
    );
}

/// REQ: LIF-960
#[test]
fn lif_960_a_snapshot_is_never_edited_to_describe_a_later_release() {
    let page = versions_page();
    assert_states(
        &page,
        "it MUST NOT be edited to describe a later release.",
        "LIF-960",
    );
}

/// REQ: LIF-961
#[test]
fn lif_961_snapshot_is_served_under_versioned_path_and_current_under_latest() {
    let page = versions_page();
    assert_states(
        &page,
        "The snapshot of host release `X.Y.Z` MUST be served under the path `/docs/X.Y.Z/`",
        "LIF-961",
    );
    assert_states(
        &page,
        "the current manual MUST be served under `/docs/latest/`",
        "LIF-961",
    );
}

/// REQ: LIF-961
#[test]
fn lif_961_every_snapshot_page_keeps_its_permalink_relative_to_the_version_path() {
    let page = versions_page();
    assert_states(
        &page,
        "Every page of a snapshot keeps its `permalink` relative to that path",
        "LIF-961",
    );
}

/// REQ: LIF-962
#[test]
fn lif_962_every_snapshot_page_shows_its_host_release_and_contract_line() {
    let page = versions_page();
    assert_states(
        &page,
        "Every page of a snapshot MUST show the host release it describes and the contract line it was written for",
        "LIF-962",
    );
}

/// REQ: LIF-962
#[test]
fn lif_962_the_version_must_be_visible_without_the_site_menu() {
    let page = versions_page();
    assert_states(
        &page,
        "so that a reader who arrives by a search engine link can see the version without the site menu",
        "LIF-962",
    );
}

/// REQ: LIF-963
#[test]
fn lif_963_a_feature_from_a_later_minor_release_is_labelled_with_its_minor_version() {
    let page = versions_page();
    assert_states(
        &page,
        "MUST say so with the minor version of that feature",
        "LIF-963",
    );
}

/// REQ: LIF-963
#[test]
fn lif_963_a_later_feature_is_never_presented_as_available_in_every_version() {
    let page = versions_page();
    assert_states(
        &page,
        "The page MUST NOT present such a feature as available in every version.",
        "LIF-963",
    );
}

/// REQ: LIF-964
#[test]
fn lif_964_a_snapshot_stays_online_for_at_least_twenty_four_months_after_the_next_minor() {
    let page = versions_page();
    assert_states(
        &page,
        "MUST stay online for at least 24 months after the release of the next minor version",
        "LIF-964",
    );
}

/// REQ: LIF-964
#[test]
fn lif_964_a_snapshot_is_never_removed_while_its_minor_still_gets_security_fixes() {
    let page = versions_page();
    assert_states(
        &page,
        "A snapshot is never removed while its minor version still receives security fixes under [SEC-323].",
        "LIF-964",
    );
}

/// REQ: LIF-965
#[test]
fn lif_965_a_snapshot_is_not_replaced_by_the_current_manual() {
    let page = versions_page();
    assert_states(
        &page,
        "A snapshot MUST NOT be replaced by the current manual, even when the current manual has a correction that the snapshot lacks.",
        "LIF-965",
    );
}

/// REQ: LIF-965
#[test]
fn lif_965_the_correction_goes_to_the_snapshot_under_corrections_after_a_release() {
    let page = versions_page();
    assert_states(
        &page,
        "The correction goes to the snapshot as described under \"Corrections after a release\".",
        "LIF-965",
    );
}

/// REQ: LIF-966
#[test]
fn lif_966_a_removed_snapshot_is_replaced_by_a_page_naming_version_replacement_and_date() {
    let page = versions_page();
    assert_states(
        &page,
        "A snapshot that is removed MUST be replaced by a page that names the removed minor version, the host release that replaces it and the date of removal",
        "LIF-966",
    );
}

/// REQ: LIF-966
#[test]
fn lif_966_the_removed_snapshot_page_must_not_be_left_blank_for_old_links() {
    let page = versions_page();
    assert_states(
        &page,
        "so that a link from an older advisory or a forum post still lands on a clear answer",
        "LIF-966",
    );
}

/// REQ: LIF-967
#[test]
fn lif_967_a_correction_is_made_in_the_current_manual_and_in_every_online_snapshot() {
    let page = versions_page();
    assert_states(
        &page,
        "A correction to a page after a release MUST be made in the current manual.",
        "LIF-967",
    );
    assert_states(
        &page,
        "It MUST also be made in every snapshot that is still online and that makes the same wrong statement",
        "LIF-967",
    );
}

/// REQ: LIF-967
#[test]
fn lif_967_the_snapshot_fix_applies_only_when_the_wrong_statement_changes_behaviour() {
    let page = versions_page();
    assert_states(
        &page,
        "when the wrong statement changes what a plugin or an operator does",
        "LIF-967",
    );
}

/// REQ: LIF-968
#[test]
fn lif_968_a_wording_only_correction_may_be_made_in_the_current_manual_alone() {
    let page = versions_page();
    assert_states(
        &page,
        "A correction that only fixes wording, spelling or a link MAY be made in the current manual alone.",
        "LIF-968",
    );
}

/// REQ: LIF-968
#[test]
fn lif_968_wording_only_corrections_are_not_listed_in_patch_release_notes() {
    let page = versions_page();
    assert_states(
        &page,
        "The release notes of the next patch release MUST NOT list such a correction.",
        "LIF-968",
    );
}

/// REQ: LIF-969
#[test]
fn lif_969_a_rule_number_name_or_procedure_correction_is_listed_in_the_next_patch_notes() {
    let page = versions_page();
    assert_states(
        &page,
        "Every correction that changes a rule, a number, a code name or a procedure MUST be listed in the release notes of the next patch release.",
        "LIF-969",
    );
}

/// REQ: LIF-969
#[test]
fn lif_969_the_entry_names_the_page_both_statements_and_the_snapshots_corrected() {
    let page = versions_page();
    assert_states(
        &page,
        "The entry MUST name the page, the old statement and the new statement, and it MUST say which snapshots received the correction.",
        "LIF-969",
    );
}

/// REQ: LIF-970
#[test]
fn lif_970_a_documentation_correction_does_not_change_the_contract_version_or_api_string() {
    let page = versions_page();
    assert_states(
        &page,
        "A correction to a documented number or rule MUST NOT change the contract version, the `api` string or the `ilium:api-version` section.",
        "LIF-970",
    );
}

/// REQ: LIF-970
#[test]
fn lif_970_a_real_behaviour_change_corrects_the_snapshot_of_the_host_release_that_contains_it() {
    let page = versions_page();
    assert_states(
        &page,
        "If a correction reveals a real change of behaviour, the host release that contains the change is the one whose snapshot is corrected.",
        "LIF-970",
    );
}

/// REQ: LIF-971
#[test]
fn lif_971_a_security_advisory_names_a_host_release_and_links_its_snapshot() {
    let page = versions_page();
    assert_states(
        &page,
        "The \"fixed in\" version of a security advisory MUST name a host release.",
        "LIF-971",
    );
    assert_states(
        &page,
        "The advisory MUST link to the snapshot of that release, and that snapshot MUST describe the fixed behaviour.",
        "LIF-971",
    );
}

/// REQ: LIF-971
#[test]
fn lif_971_a_snapshot_without_the_fixed_behaviour_is_a_documentation_error() {
    let page = versions_page();
    assert_states(
        &page,
        "A snapshot that does not describe the fixed behaviour is a documentation error under [LIF-967].",
        "LIF-971",
    );
}

/// REQ: LIF-972
#[test]
fn lif_972_an_advisory_says_which_behaviour_the_affected_snapshot_describes() {
    let page = versions_page();
    assert_states(
        &page,
        "Where the documented behaviour of an older snapshot differs from the behaviour of a fixed release, the advisory MUST say which behaviour the affected snapshot describes",
        "LIF-972",
    );
}

/// REQ: LIF-972
#[test]
fn lif_972_the_reader_can_tell_whether_their_host_is_affected() {
    let page = versions_page();
    assert_states(
        &page,
        "so a reader can tell whether their host is affected",
        "LIF-972",
    );
}

/// REQ: LIF-973
#[test]
fn lif_973_each_release_page_states_the_documentation_and_security_windows() {
    let page = versions_page();
    assert_states(
        &page,
        "Each release page MUST state two things: the minor versions that receive documentation corrections, and the minor versions that receive security fixes under [SEC-323].",
        "LIF-973",
    );
}

/// REQ: LIF-973
#[test]
fn lif_973_the_documentation_window_may_end_early_but_not_before_the_retention_minimum() {
    let page = versions_page();
    assert_states(
        &page,
        "The documentation window MAY end before the security window, but it MUST NOT end before the minimum retention of [LIF-964].",
        "LIF-973",
    );
}

/// REQ: LIF-974
#[test]
fn lif_974_a_support_answer_cites_the_snapshot_of_the_users_host_version() {
    let page = versions_page();
    assert_states(
        &page,
        "A support answer about a plugin or a host MUST cite the snapshot of the host version that the user runs, not the current manual.",
        "LIF-974",
    );
}

/// REQ: LIF-974
#[test]
fn lif_974_a_support_answer_citing_the_current_manual_for_an_older_host_warns_of_differences() {
    let page = versions_page();
    assert_states(
        &page,
        "A support answer that cites the current manual for an older host MUST say that the behaviour may differ.",
        "LIF-974",
    );
}

/// REQ: LIF-975
#[test]
fn lif_975_a_plugin_author_states_the_minimum_host_minor_and_api_string() {
    let page = versions_page();
    assert_states(
        &page,
        "A plugin author who publishes documentation for a plugin MUST state the minimum host minor version the plugin needs and the `api` string it was built for.",
        "LIF-975",
    );
}

/// REQ: LIF-975
#[test]
fn lif_975_that_statement_sits_with_the_release_notes_naming_the_plugin_version() {
    let page = versions_page();
    assert_states(
        &page,
        "The statement MUST be kept in the same place as the release notes that name the plugin version",
        "LIF-975",
    );
}
