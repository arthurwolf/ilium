//! Host release verification doc checks: one test per requirement ID for the installer's
//! checksum file, download, hash comparison, failure handling and gzip validation. Each test
//! reads the manual page that states the rule and checks the rule's key phrases.

use crate::support::manual_page;

/// REQ: SEC-433
#[test]
fn sec_433_checksum_file_needs_exactly_one_record_per_archive() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "accept `SHA256SUMS` only when it holds",
        "exactly one valid record",
        "`Malformed, duplicate or missing checksum record`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-434
#[test]
fn sec_434_download_over_https_from_same_tag_in_staging() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "over HTTPS from the same release tag",
        "MUST NOT fall back to another release",
        "private staging directory",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-435
#[test]
fn sec_435_hash_comparison_uses_whole_value_and_exact_name() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "MUST use the whole 64-character value",
        "the exact archive name",
        "MUST treat the download as failed",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-436
#[test]
fn sec_436_host_never_claims_its_own_archive_passed_verification() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "MUST NOT claim, in `ilium --version --verbose`",
        "Verification belongs to the installer",
        "before the first run",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-437
#[test]
fn sec_437_failed_check_exits_one_and_leaves_no_half_install() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "the installer MUST exit with status 1",
        "MUST print the fixed message",
        "MUST NOT leave an unpacked or half-installed host",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-438
#[test]
fn sec_438_invalid_gzip_stops_install_after_hash_check() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "`Malformed gzip archive or exhausted staging space`",
        "runs only after the hash check has passed",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-433
#[test]
fn sec_433_error_table_names_the_exactly_one_record_rule() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "The selected archive name has exactly one record",
        "`Malformed, duplicate or missing checksum record`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-434
#[test]
fn sec_434_missing_asset_ends_install_with_no_latest_fallback_message() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "HTTPS download failed (network, rate limit, or missing release asset); no latest fallback",
        "The installer script downloads the archive and `SHA256SUMS` from the same release",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-435
#[test]
fn sec_435_user_finds_the_record_by_archive_name_and_hashes_must_be_identical() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "whose second field is the same archive name",
        "The two MUST be identical.",
        "A user who finds no line for the archive, or more than one",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-436
#[test]
fn sec_436_matching_hash_does_not_prove_who_built_the_archive() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "It does not, by itself, show who built the file. The attestation check adds that proof.",
        "The host MUST NOT claim",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-437
#[test]
fn sec_437_output_on_failure_unpacks_nothing() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "On failure the installer exits 1 and unpacks nothing",
        "Each failure has a fixed message.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-438
#[test]
fn sec_438_gzip_failure_installs_nothing() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "The archive is a valid gzip file",
        "Nothing is installed.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
