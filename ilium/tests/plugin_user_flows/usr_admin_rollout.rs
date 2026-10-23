//! Administrator rules for policy rollout and plugin removal (USR-107 to USR-111, USR-954). The
//! procedure is written in guide-administration.md; these tests read it and assert the concrete
//! steps, ring sizes, revert path and purge commands.

use crate::support::*;

fn states(page: &str, fact: &str) {
    assert!(page.contains(fact), "the page must state: {fact}");
}

/// REQ: USR-107
#[test]
fn usr_107_the_procedure_copies_the_current_policy_to_a_previous_copy_first() {
    let page = manual_page("guide-administration");
    states(&page, "Copy the current policy file to a previous-copy file beside it");
    states(&page, "`/etc/ilium/policy.toml.previous`");
}

/// REQ: USR-107
#[test]
fn usr_107_the_previous_copy_is_kept_until_every_ring_has_passed_and_restored_by_rename() {
    let page = manual_page("guide-administration");
    states(&page, "Keep the previous copy until every ring has passed [USR-107].");
    states(
        &page,
        "copy the previous-copy file back over the policy file with the same rename, then restart the sessions on the affected machines",
    );
}

/// REQ: USR-108
#[test]
fn usr_108_the_first_ring_runs_policy_show_and_compares_the_effective_values() {
    let page = manual_page("guide-administration");
    states(&page, "On that machine, run `ilium plugin policy show`.");
    states(
        &page,
        "Compare the merged `policy` line with the values you intended",
    );
}

/// REQ: USR-108
#[test]
fn usr_108_a_syntactically_valid_policy_is_still_checked_for_its_effect() {
    let page = manual_page("guide-administration");
    states(
        &page,
        "A file that passes this check can still forbid a grant or plugin you need, so step 6 checks the effect.",
    );
}

/// REQ: USR-109
#[test]
fn usr_109_the_rings_are_sized_one_machine_or_five_then_at_most_double() {
    let page = manual_page("guide-administration");
    states(
        &page,
        "The first ring is one machine, or at most five machines (proposed, owner to confirm).",
    );
    states(
        &page,
        "Each later ring may be at most twice the size of the one before (proposed, owner to confirm).",
    );
}

/// REQ: USR-109
#[test]
fn usr_109_the_next_ring_waits_one_working_day_with_no_violation_or_blocked_plugin() {
    let page = manual_page("guide-administration");
    states(
        &page,
        "Move to the next ring only after the first ring has run the new policy for one working day (proposed, owner to confirm)",
    );
    states(
        &page,
        r#"no plugin reported as "Blocked by policy" that should run"#,
    );
    states(&page, "Stop the rollout at the first sign of a problem.");
}

/// REQ: USR-111
#[test]
fn usr_111_removal_for_a_security_reason_asks_each_user_to_run_uninstall_purge() {
    let page = manual_page("guide-administration");
    states(
        &page,
        "Ask each user to run `ilium plugin uninstall <id> --purge`.",
    );
}

/// REQ: USR-111
#[test]
fn usr_111_the_policy_file_alone_deletes_nothing_on_the_machines() {
    let page = manual_page("guide-administration");
    states(&page, "The policy does not delete anything on its own");
}

/// REQ: USR-954
#[test]
fn usr_954_removal_for_a_security_reason_asks_each_user_to_run_erase_data() {
    let page = manual_page("guide-administration");
    states(
        &page,
        "the administrator MUST ask each user to run `ilium plugin erase-data` for that plugin",
    );
    states(&page, "because the purge may leave crash records in place");
}

/// REQ: USR-954
#[test]
fn usr_954_erase_data_is_the_command_that_deletes_crash_records_the_purge_leaves() {
    let page = manual_page("user-privacy");
    states(&page, "`erase-data` deletes them ([USR-326])");
}
