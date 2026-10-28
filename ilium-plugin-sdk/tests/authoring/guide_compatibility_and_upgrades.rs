//! Rule statements of the guide-compatibility-and-upgrades page as the manual states them (doc-lint tests).
use crate::support::{assert_page_line, assert_rule};

/// REQ: AUT-201
#[test]
fn doc_aut_201_version_raised_for_every_package() {
    assert_rule(
        "AUT-201",
        "MUST",
        &[
            "raise `version` for every package",
            "same version and different content",
        ],
    );
}

/// REQ: AUT-202
#[test]
fn doc_aut_202_stable_names_never_renamed_or_removed() {
    assert_rule(
        "AUT-202",
        "MUST NOT",
        &[
            "MUST NOT rename or remove any name of this table",
            "keep the old one working",
        ],
    );
}

/// REQ: AUT-203
#[test]
fn doc_aut_203_old_name_works_two_minor_releases() {
    assert_rule(
        "AUT-203",
        "MUST",
        &["at least two minor releases", "announced in the changelog"],
    );
}

/// REQ: AUT-204
#[test]
fn doc_aut_204_old_command_hidden_but_still_runs() {
    assert_rule(
        "AUT-204",
        "MUST",
        &[
            "hidden from the command list and from the palette",
            "MUST still run when a key or a script calls it",
        ],
    );
}

/// REQ: AUT-205
#[test]
fn doc_aut_205_new_grant_checked_with_not_permitted_fallback() {
    assert_rule("AUT-205", "MUST", &["`not-permitted`", "fall back"]);
}

/// REQ: AUT-206
#[test]
fn doc_aut_206_restore_layout_first_byte_identifies_format() {
    assert_rule(
        "AUT-206",
        "MUST",
        &[
            "first byte identifies the format",
            "fall back to defaults when the first byte is not a format it knows",
        ],
    );
}

/// REQ: AUT-207
#[test]
fn doc_aut_207_durable_data_converted_by_migrator() {
    assert_rule(
        "AUT-207",
        "MUST",
        &[
            "`migrator`",
            "restore bytes are delivered only when the `data_version` matches",
        ],
    );
}

/// REQ: AUT-208 WARN
#[test]
fn doc_aut_208_data_readable_by_previous_release() {
    assert_rule(
        "AUT-208",
        "SHOULD",
        &["adding keys instead of changing the meaning of old ones"],
    );
}

/// REQ: AUT-209 WARN
#[test]
fn doc_aut_209_rebuild_against_each_new_minor_contract() {
    assert_rule("AUT-209", "SHOULD", &["each new minor contract version"]);
}

/// REQ: AUT-210
#[test]
fn doc_aut_210_published_plugin_lists_no_unstable_feature() {
    assert_rule(
        "AUT-210",
        "MUST NOT",
        &[
            "MUST NOT list an unstable feature",
            "installable only in development mode",
        ],
    );
}

/// REQ: AUT-211
#[test]
fn doc_aut_211_tester_package_has_prerelease_version() {
    assert_rule("AUT-211", "MUST", &["prerelease part in its version"]);
}

/// REQ: AUT-212
#[test]
fn doc_aut_212_requires_range_no_wider_than_tested() {
    assert_rule(
        "AUT-212",
        "MUST",
        &["`requires` range no wider than the versions it was tested with"],
    );
}

/// REQ: AUT-213
#[test]
fn doc_aut_213_incompatible_payload_raises_major_of_point() {
    assert_rule(
        "AUT-213",
        "MUST",
        &[
            "raise the major version of the point",
            "keep the old major working",
        ],
    );
}

/// REQ: AUT-214
#[test]
fn doc_aut_214_stable_public_names_kept_across_releases() {
    assert_rule("AUT-214", "MUST", &["every name of the stable public list"]);
}

/// REQ: AUT-215
#[test]
fn doc_aut_215_update_not_detected_from_reason_or_restore() {
    assert_rule("AUT-215", "MUST NOT", &["`upgrade`", "`ilium plugin dev`"]);
}

/// REQ: AUT-203
#[test]
fn doc_aut_203_old_name_keeps_working_until_it_is_removed() {
    assert_page_line("AUT-203", &["before it is removed"]);
}

/// REQ: AUT-204
#[test]
fn doc_aut_204_old_command_is_hidden_only_once_the_new_one_exists() {
    assert_page_line("AUT-204", &["once the new one exists"]);
}

/// REQ: AUT-208
#[test]
fn doc_aut_208_rollback_loses_nothing_when_old_keys_are_kept() {
    assert_page_line("AUT-208", &["A user who rolls back then loses nothing."]);
}

/// REQ: AUT-209
#[test]
fn doc_aut_209_publish_before_the_host_drops_the_minor() {
    assert_page_line(
        "AUT-209",
        &["so that users never hold a plugin outside the window"],
    );
}

/// REQ: AUT-211
#[test]
fn doc_aut_211_tester_prerelease_cannot_pass_for_a_release() {
    assert_page_line("AUT-211", &["so that it cannot be taken for a release"]);
}

/// REQ: AUT-212
#[test]
fn doc_aut_212_requires_range_window_matches_the_support_window() {
    assert_page_line(
        "AUT-212",
        &["the same window in which `requires` and prereleases are supported"],
    );
}

/// REQ: AUT-213
#[test]
fn doc_aut_213_incompatible_payload_change_is_named_as_such() {
    assert_page_line("AUT-213", &["changes the payload incompatibly"]);
}

/// REQ: AUT-214
#[test]
fn doc_aut_214_stable_public_names_are_kept_across_releases() {
    assert_page_line("AUT-214", &["across its own releases"]);
}
