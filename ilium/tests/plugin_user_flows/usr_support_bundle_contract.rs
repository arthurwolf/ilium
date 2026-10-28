//! Support bundle contract rules (USR-104 to USR-106, USR-953). The governing-value table of
//! core-plugin-diagnostics.md is the only source of the bundle's command, entries and retention.
//! Documentation rules read the manual page; behaviour rules drive the plugin command line and the
//! user rig, and fail until the bundle writer and the held-bundle cleanup exist (see API_GAPS.md).

use crate::support::*;

fn states(page: &str, fact: &str) {
    assert!(page.contains(fact), "the page must state: {fact}");
}

/// The entry names of a zip archive, read from its central directory.
fn zip_entry_names(bytes: &[u8]) -> Vec<String> {
    const CENTRAL_HEADER: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
    let mut names = Vec::new();
    let mut at = 0;
    while at + 46 <= bytes.len() {
        if bytes[at..at + 4] != CENTRAL_HEADER {
            at += 1;
            continue;
        }
        let name_length = usize::from(u16::from_le_bytes([bytes[at + 28], bytes[at + 29]]));
        let extra_length = usize::from(u16::from_le_bytes([bytes[at + 30], bytes[at + 31]]));
        let comment_length = usize::from(u16::from_le_bytes([bytes[at + 32], bytes[at + 33]]));
        let name_end = at + 46 + name_length;
        if name_end > bytes.len() {
            break;
        }
        names.push(String::from_utf8_lossy(&bytes[at + 46..name_end]).into_owned());
        at = name_end + extra_length + comment_length;
    }
    names
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
    let mut rig = UserRig::new();
    for superseded in [
        vec!["bundle", "com.example.writer", "--out", "x.zip"],
        vec!["diagnostics", "--out", "x.zip"],
    ] {
        let output = rig.cli(&superseded);
        assert_eq!(
            error_codes(&output),
            vec!["usage.command-unknown".to_string()],
            "{superseded:?} must be an unknown command"
        );
    }
    let output = rig.cli(&["support-bundle", "com.example.writer", "--out", "x.zip"]);
    assert!(
        !error_codes(&output).contains(&"usage.command-unknown".to_string()),
        "support-bundle must be a known command: {:?}",
        error_codes(&output)
    );
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
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.writer", "1.0.0", &["tree.read"]),
    );
    let file = rig.scratch_dir().join("bundle.zip");
    let file_text = file.to_string_lossy().into_owned();
    let output = rig.cli(&["support-bundle", "com.example.writer", "--out", &file_text]);
    assert_eq!(result_exit_code(&output), 0, "{:?}", error_codes(&output));
    let mut entries = zip_entry_names(&std::fs::read(&file).expect("bundle written"));
    entries.sort();
    assert_eq!(
        entries,
        vec!["crashes.txt", "log.txt", "stats.txt", "summary.txt"],
        "a bundle holds exactly the four entries of USR-221"
    );
    assert!(!entries.iter().any(|name| name.contains("manifest")));
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
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.writer", "1.0.0", &[]));
    let held_folder = rig.state_dir().join("support-bundles");
    std::fs::create_dir_all(&held_folder).expect("held folder");
    let old_held = held_folder.join("old.zip");
    let new_held = held_folder.join("new.zip");
    let user_file = rig.scratch_dir().join("mine.zip");
    for path in [&old_held, &new_held, &user_file] {
        std::fs::write(path, b"PK").expect("write bundle");
    }
    rig.age_file(&old_held, 15);
    rig.age_file(&user_file, 400);
    rig.restart_server();
    assert!(!old_held.exists(), "a held bundle 14 days or older is deleted at start");
    assert!(new_held.exists(), "a younger held bundle is kept");
    assert!(user_file.exists(), "a file written by --out is never touched");
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
