//! Rule statements of the guide-publishing-and-support page as the manual states them (doc-lint tests).
use crate::support::assert_rule;

/// REQ: AUT-301
#[test]
fn doc_aut_301_package_file_is_the_certain_publishing_route() {
    assert_rule(
        "AUT-301",
        "MUST",
        &[
            "only publishing route that is certain to work",
            "MAY add a registry",
        ],
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
