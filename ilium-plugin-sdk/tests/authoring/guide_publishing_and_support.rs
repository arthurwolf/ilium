//! Rule statements of the guide-publishing-and-support page as the manual states them (doc-lint tests).
use crate::support::{assert_page_line, assert_rule, page, table_row};

/// REQ: AUT-301
#[test]
fn doc_aut_301_package_file_is_the_certain_publishing_route() {
    assert_rule(
        "AUT-301",
        "MUST",
        &["base route that needs no registry", "MAY add a registry"],
    );
}

/// REQ: AUT-302
#[test]
fn doc_aut_302_release_passes_check_and_lint() {
    assert_rule(
        "AUT-302",
        "MUST",
        &["pass `check` without errors", "`lint --deny warnings`"],
    );
}

/// REQ: AUT-303
#[test]
fn doc_aut_303_release_built_with_release_flag() {
    assert_rule(
        "AUT-303",
        "MUST",
        &["`--release`", "`lint.debug-info-in-release`"],
    );
}

/// REQ: AUT-304
#[test]
fn doc_aut_304_package_contains_no_secret() {
    assert_rule(
        "AUT-304",
        "MUST NOT",
        &["MUST NOT contain a secret, a key or a token"],
    );
}

/// REQ: AUT-305
#[test]
fn doc_aut_305_signed_id_starts_with_publisher_id() {
    assert_rule(
        "AUT-305",
        "MUST",
        &[
            "publisher id of the signing key and a dot",
            "`manifest.id-publisher`",
        ],
    );
}

/// REQ: AUT-306
#[test]
fn doc_aut_306_secret_key_never_committed_packaged_or_printed() {
    assert_rule(
        "AUT-306",
        "MUST NOT",
        &["MUST NOT be committed, packaged or printed"],
    );
}

/// REQ: AUT-306
#[test]
fn doc_aut_306_key_kept_out_of_repository_and_out_of_command_lines() {
    let guide = page("guide-publishing-and-support");
    assert!(
        guide.contains("Keep `publisher.key` outside the project and outside version control"),
        "AUT-306: the guide lacks the key-location rule"
    );
    assert!(
        guide.contains("The key never appears in a command line or a log"),
        "AUT-306: the guide lacks the CI key-passing rule"
    );
    assert!(
        guide.contains("`ILIUM_PLUGIN_SIGNING_KEY`") && guide.contains("`ILIUM_PUBLISHER_KEY`"),
        "AUT-306: the guide does not name the environment routes for the key"
    );
}

/// REQ: AUT-307
#[test]
fn doc_aut_307_sign_after_last_pack_and_signing_is_deterministic() {
    assert_rule(
        "AUT-307",
        "MUST",
        &[
            "signed after the last `pack`",
            "Signing twice with the same key gives the same bytes",
        ],
    );
}

/// REQ: AUT-308 WARN
#[test]
fn doc_aut_308_full_fingerprint_published_on_separate_channel() {
    assert_rule(
        "AUT-308",
        "SHOULD",
        &["full fingerprint on a channel separate from the download"],
    );
}

/// REQ: AUT-309
#[test]
fn doc_aut_309_leaked_key_announced_with_replacement_fingerprint() {
    assert_rule(
        "AUT-309",
        "MUST",
        &[
            "same channel as the releases",
            "fingerprint of the key that replaces it",
        ],
    );
}

/// REQ: AUT-310
#[test]
fn doc_aut_310_release_notes_name_new_grants_and_data_versions() {
    assert_rule(
        "AUT-310",
        "MUST",
        &["every new grant and every raised `data_version`"],
    );
}

/// REQ: AUT-311 WARN
#[test]
fn doc_aut_311_readme_and_changelog_shipped() {
    assert_rule(
        "AUT-311",
        "SHOULD",
        &[
            "`README.md` and `CHANGELOG.md`",
            "`lint.readme-missing` and `lint.changelog-missing`",
        ],
    );
}

/// REQ: AUT-312 WARN
#[test]
fn doc_aut_312_package_named_id_version_iliumpkg() {
    assert_rule(
        "AUT-312",
        "SHOULD NOT",
        &["`<id>-<version>.iliumpkg`", "SHOULD NOT rename it"],
    );
}

/// REQ: AUT-313
#[test]
fn doc_aut_313_release_page_states_linux_requirement() {
    assert_rule("AUT-313", "MUST", &["need Linux"]);
}

/// REQ: AUT-314
#[test]
fn doc_aut_314_unredacted_not_asked_from_normal_install() {
    assert_rule(
        "AUT-314",
        "MUST NOT",
        &["`--unredacted` works only for a plugin installed with `ilium plugin dev`"],
    );
}

/// REQ: AUT-315
#[test]
fn doc_aut_315_log_lines_carry_no_secrets_or_payloads() {
    assert_rule(
        "AUT-315",
        "MUST NOT",
        &[
            "MUST NOT write secrets, tokens, file contents or whole payloads",
            "16 KiB at most",
        ],
    );
}

/// REQ: AUT-316 WARN
#[test]
fn doc_aut_316_unreproducible_report_becomes_fixture() {
    assert_rule("AUT-316", "SHOULD", &["turned into a fixture"]);
}

/// REQ: AUT-317 WARN
#[test]
fn doc_aut_317_unfixable_plugin_deprecated_with_successor() {
    assert_rule("AUT-317", "SHOULD", &["deprecated with a successor named"]);
}

/// REQ: AUT-318
#[test]
fn doc_aut_318_contact_published_before_first_release() {
    assert_rule(
        "AUT-318",
        "MUST",
        &[
            "way to reach them",
            "in the README and in the package description",
        ],
    );
}

/// REQ: AUT-319
#[test]
fn doc_aut_319_published_version_never_reused() {
    assert_rule("AUT-319", "MUST NOT", &["`package.version-reused`"]);
}

/// REQ: AUT-320
#[test]
fn doc_aut_320_check_lint_and_test_run_before_release() {
    assert_rule(
        "AUT-320",
        "MUST",
        &["`ilium plugin check`, `lint` and `test`"],
    );
}

/// REQ: AUT-321
#[test]
fn doc_aut_321_no_metadata_keys_beyond_manifest() {
    assert_rule(
        "AUT-321",
        "MUST NOT",
        &[
            "`manifest.unknown-key`",
            "the support contact, the home page and the changelog",
        ],
    );
}

/// REQ: AUT-322
#[test]
fn doc_aut_322_secret_key_out_of_package_and_vcs_leak_announced() {
    assert_rule(
        "AUT-322",
        "MUST",
        &[
            "out of the package and out of version control",
            "fingerprint of its replacement",
        ],
    );
}

/// REQ: AUT-301
#[test]
fn doc_aut_301_package_route_is_paired_with_a_registry_publish_command() {
    assert_page_line("AUT-301", &["uploads a signed package to a registry"]);
}

/// REQ: AUT-302
#[test]
fn doc_aut_302_release_check_and_lint_sentence_on_the_guide_line() {
    assert_page_line(
        "AUT-302",
        &["A release MUST pass `check` without errors and SHOULD pass `lint --deny warnings`."],
    );
}

/// REQ: AUT-304
#[test]
fn doc_aut_304_package_is_a_file_anyone_can_open() {
    assert_page_line("AUT-304", &["The package is a file that anyone can open."]);
}

/// REQ: AUT-307
#[test]
fn doc_aut_307_users_receive_the_signed_file_itself() {
    assert_page_line(
        "AUT-307",
        &["the signed file MUST be the file that users receive"],
    );
}

/// REQ: AUT-308
#[test]
fn doc_aut_308_short_fingerprint_is_shown_in_the_trust_dialog() {
    assert_page_line(
        "AUT-308",
        &["Users see its short form (the first 8 digits, for example `3f2a 91c0`) in the trust dialog"],
    );
}

/// REQ: AUT-309
#[test]
fn doc_aut_309_leaked_key_is_announced_to_users() {
    assert_page_line("AUT-309", &["A leaked key MUST be announced to users"]);
}

/// REQ: AUT-310
#[test]
fn doc_aut_310_release_notes_sentence_names_grants_and_data_versions() {
    assert_page_line("AUT-310", &["Release notes MUST name every new grant"]);
}

/// REQ: AUT-314
#[test]
fn doc_aut_314_normal_trace_prints_payloads_as_redacted_byte_counts() {
    assert_page_line(
        "AUT-314",
        &["Payloads in a normal trace are redacted and print as `<redacted N bytes>`"],
    );
}

/// REQ: AUT-315
#[test]
fn doc_aut_315_log_writes_a_count_or_an_id_instead_of_content() {
    assert_page_line("AUT-315", &["so write a count or an id"]);
}

/// REQ: AUT-316
#[test]
fn doc_aut_316_replay_on_fake_clock_reproduces_timing_problems() {
    assert_page_line(
        "AUT-316",
        &["The fake host runs on a fake clock, so timing problems are reproduced with `advance_clock`."],
    );
}

/// REQ: AUT-317
#[test]
fn doc_aut_317_vulnerability_is_private_until_a_fix_is_out() {
    assert_page_line("AUT-317", &["treat it as private until a fix is out"]);
}

/// REQ: AUT-318
#[test]
fn doc_aut_318_contact_way_is_in_place_before_the_first_release() {
    assert_page_line("AUT-318", &["before the first release"]);
}

/// REQ: AUT-320
#[test]
fn doc_aut_320_release_step_is_tested_through_the_tutorial_script() {
    assert!(table_row("AUT-320").contains("release script of the tutorial"));
}

/// REQ: AUT-322
#[test]
fn doc_aut_322_signed_package_carries_the_publisher_key_trust() {
    assert_page_line(
        "AUT-322",
        &["A signed package carries the trust of its publisher key."],
    );
}
