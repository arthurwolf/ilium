//! ONB requirements of the author onboarding page: the development loop of `dev`
//! (ONB-009 to ONB-014). The statements are read from the manual, in its requirement table and
//! in the marked line of the onboarding page.
use crate::support::{assert_page_line, table_row};

/// REQ: ONB-009
#[test]
fn onb_009_dev_rebuilds_300_ms_after_last_change_a() {
    let row = table_row("ONB-009");
    assert!(
        row.contains("`dev` MUST start a rebuild 300 ms after the last file change"),
        "{row}"
    );
}

/// REQ: ONB-009
#[test]
fn onb_009_dev_ignores_build_and_vcs_folders_b() {
    assert_page_line(
        "ONB-009",
        &[
            "and MUST ignore changes under `dist/`, `target/`, `node_modules/`, `.ilium/`, `.git/` and the paths in `.gitignore`",
        ],
    );
}

/// REQ: ONB-010
#[test]
fn onb_010_successful_rebuild_reloads_server_then_clients_a() {
    let row = table_row("ONB-010");
    assert!(
        row.contains(
            "On a successful rebuild, `dev` MUST reload the server side first and then each attached client side."
        ),
        "{row}"
    );
    assert!(
        row.contains("Each reload MUST print one `reload` record."),
        "{row}"
    );
}

/// REQ: ONB-010
#[test]
fn onb_010_failed_rebuild_keeps_instance_and_sends_no_deactivate_b() {
    assert_page_line(
        "ONB-010",
        &[
            "A failed rebuild MUST print the compiler errors, keep the running instance, and send no `deactivate`",
            "no restart is needed.",
        ],
    );
}

/// REQ: ONB-011
#[test]
fn onb_011_dev_copy_installed_as_tier_t3_with_badge_a() {
    let row = table_row("ONB-011");
    assert!(
        row.contains(
            "The development copy MUST be installed as tier T3 with the `dev` badge and with a data folder of its own."
        ),
        "{row}"
    );
    assert!(
        row.contains("A real install of the same id MUST not share that data folder"),
        "{row}"
    );
}

/// REQ: ONB-011
#[test]
fn onb_011_installed_id_fails_with_code_unless_replace_flag_b() {
    assert_page_line(
        "ONB-011",
        &[
            "`dev` MUST fail with `dev.id-installed` and exit with 1, unless `--replace-installed` is given",
        ],
    );
}

/// REQ: ONB-012
#[test]
fn onb_012_dev_builds_with_debug_info_by_default_a() {
    let row = table_row("ONB-012");
    assert!(
        row.contains(
            "`dev` MUST build with debug information by default, so that a trap names `file:line`."
        ),
        "{row}"
    );
}

/// REQ: ONB-012
#[test]
fn onb_012_release_flag_omits_debug_info_b() {
    assert_page_line("ONB-012", &["`--release` MUST omit it ([DEV-182])."]);
}

/// REQ: ONB-013
#[test]
fn onb_013_ctrl_c_deactivates_with_reason_disabled_a() {
    let row = table_row("ONB-013");
    assert!(
        row.contains(
            "Ctrl+C in `dev` MUST deactivate the plugin with reason `disabled`, uninstall the development copy"
        ),
        "{row}"
    );
}

/// REQ: ONB-013
#[test]
fn onb_013_ctrl_c_keeps_data_and_exits_130_b() {
    assert_page_line(
        "ONB-013",
        &[
            "keep its data folder unless `--purge-on-exit` is given, and exit with 130",
        ],
    );
}

/// REQ: ONB-014
#[test]
fn onb_014_restore_bytes_over_1_mib_dropped_on_reload_a() {
    let row = table_row("ONB-014");
    assert!(
        row.contains("Restore bytes above 1 MiB MUST be dropped on reload"),
        "{row}"
    );
    assert!(
        row.contains(
            "the `reload` record MUST show `restore_dropped` true with `restored_bytes` 0"
        ),
        "{row}"
    );
}

/// REQ: ONB-014
#[test]
fn onb_014_author_must_not_rely_on_dropped_restore_state_b() {
    assert_page_line(
        "ONB-014",
        &[
            "An author who sees this MUST treat the reload as having lost its restore state and MUST NOT rely on that state surviving a reload",
        ],
    );
}
