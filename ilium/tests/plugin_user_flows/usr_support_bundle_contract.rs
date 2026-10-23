//! Support bundle contract rules (USR-104 to USR-106, USR-953). The governing-value table of
//! core-plugin-diagnostics.md is the only source of the bundle's command, entries and retention.
//! Documentation rules read the manual page; behaviour rules call unimplemented! stubs until the
//! bundle writer and the held-bundle cleanup exist (see API_GAPS.md).

use crate::support::*;

fn states(page: &str, fact: &str) {
    assert!(page.contains(fact), "the page must state: {fact}");
}

/// REQ: USR-104
#[test]
fn usr_104_the_contract_names_support_bundle_as_the_only_writing_command() {
    let page = manual_page("core-plugin-diagnostics");
    states(
        &page,
        "A support bundle MUST be written only by `ilium plugin support-bundle <id> --out <file>`",
    );
    states(
        &page,
        "| Command | `ilium plugin support-bundle <id> --out <file>` |",
    );
}

/// REQ: USR-104
#[test]
fn usr_104_bundle_commands_that_are_superseded_are_not_a_second_format() {
    let page = manual_page("core-plugin-diagnostics");
    states(
        &page,
        "an implementation MUST NOT offer `ilium plugin bundle` or `ilium plugin diagnostics --out <file>` as a second bundle format",
    );
    states(
        &page,
        "`ilium plugin bundle` (LIF-312); `ilium plugin diagnostics --out <file>` (HST-636)",
    );
}

/// REQ: USR-104
#[test]
fn usr_104_the_cli_dispatch_has_no_bundle_command_other_than_support_bundle() {
    unimplemented!("ilium_plugin_host::cli::run: `plugin bundle` must be an unknown command")
}

/// REQ: USR-105
#[test]
fn usr_105_a_bundle_carries_the_grant_list_only_inside_summary_txt() {
    let page = manual_page("core-plugin-diagnostics");
    states(
        &page,
        "The grant list appears only inside `summary.txt` (USR-221), and no other entry MUST hold grants.",
    );
    states(
        &page,
        "that holds exactly these entries:",
    );
}

/// REQ: USR-105
#[test]
fn usr_105_a_bundle_never_carries_manifest_or_other_plugins_health_or_state_names() {
    let page = manual_page("core-plugin-diagnostics");
    states(
        &page,
        "A bundle MUST NOT carry the plugin's manifest, the health and metrics of any other plugin, or the state names of other plugins.",
    );
    states(&page, "It MUST NOT hold any other entry.");
}

/// REQ: USR-105
#[test]
fn usr_105_a_written_bundle_holds_exactly_the_four_entries_and_no_manifest() {
    unimplemented!("ilium_plugin_host::testing::user_rig::UserRig::support_bundle_entries")
}

/// REQ: USR-106
#[test]
fn usr_106_a_host_offered_bundle_is_held_then_deleted_at_the_next_start_after_14_days() {
    let page = manual_page("core-plugin-diagnostics");
    states(
        &page,
        "MUST be held in the server state folder and MUST be deleted at the next start once it is 14 days old.",
    );
}

/// REQ: USR-106
#[test]
fn usr_106_a_file_written_by_support_bundle_out_is_never_touched_by_ilium_again() {
    let page = manual_page("core-plugin-diagnostics");
    states(
        &page,
        "Ilium MUST NOT delete, move, rename, send or read it after it is written.",
    );
}

/// REQ: USR-106
#[test]
fn usr_106_the_out_file_survives_the_next_start_and_held_bundles_are_cleaned() {
    unimplemented!("ilium_plugin_host::testing::user_rig::UserRig::held_bundles")
}

/// REQ: USR-953
#[test]
fn usr_953_a_bundle_change_starts_in_the_contract_section() {
    let page = manual_page("core-plugin-diagnostics");
    states(
        &page,
        "A change to the bundle contents, the line count, the size cap or the retention period MUST change this section first.",
    );
}

/// REQ: USR-953
#[test]
fn usr_953_the_matching_rows_of_the_other_registers_change_in_the_same_change() {
    let page = manual_page("core-plugin-diagnostics");
    states(
        &page,
        "The matching rows of the user-admin register (USR-221 to USR-224, USR-334, USR-335), the lifecycle register (LIF-312 to LIF-314) and the host telemetry register (HST-636) MUST then be updated in the same change",
    );
    states(&page, "so that no register keeps a superseded value.");
    let register = manual_page("reference-requirements-user-admin");
    states(
        &register,
        "| USR-953 | A change to bundle contents, line count, size cap or retention MUST first change the support bundle contract section",
    );
}
