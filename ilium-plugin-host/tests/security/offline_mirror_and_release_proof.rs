//! Requirements SEC-960 .. SEC-962 (reference-host-release-verification.md): an offline site checks the
//! attestation on a connected machine before it carries an archive in, a mirror holds one unchanged
//! `SHA256SUMS`, and every offline check leaves a dated record that includes the attestation result.

use crate::support::*;

/// REQ: SEC-960
#[test]
fn sec_960_an_offline_site_runs_the_attestation_check_on_a_connected_machine_first() {
    let page = manual_page("reference-host-release-verification.md");
    assert!(
        page.contains(
            "A site without GitHub access MUST run the attestation check of [SEC-429] on a connected machine, against the same archive bytes that the site will receive"
        ),
        "the page must require the attestation check on a connected machine for the same bytes"
    );
    assert!(
        page.contains(
            "MUST NOT carry the archive into the site until that check reports a verified attestation"
        ),
        "the page must forbid carrying the archive in before a verified attestation"
    );
}

/// REQ: SEC-960
#[test]
fn sec_960_the_offline_site_computes_the_archive_hash_again_and_matching_hash_proves_no_origin() {
    let page = manual_page("reference-host-release-verification.md");
    assert!(
        page.contains("The hash of the archive MUST be computed again on the offline site."),
        "the page must require a second hash computation on the offline site"
    );
    assert!(
        page.contains("It shows nothing about the builder, so the record of [SEC-962] is the only proof of origin that the site keeps."),
        "a matching hash must not be presented as proof of origin"
    );
}

/// REQ: SEC-961
#[test]
fn sec_961_a_mirror_holds_the_five_archives_and_one_unchanged_sha256sums_of_one_tag() {
    let page = manual_page("reference-host-release-verification.md");
    assert!(
        page.contains("A mirror of a host release MUST hold the five archives of the archive inventory and the `SHA256SUMS` of one release tag, with the file names and bytes unchanged."),
        "the page must state the mirror contents and unchanged bytes"
    );
    assert!(
        page.contains("`ilium-linux-x86_64.tar.gz`, `ilium-linux-aarch64.tar.gz`, `ilium-windows-x86_64.zip`, `ilium-macos-aarch64.tar.gz` and `ilium-macos-x86_64.tar.gz`"),
        "the archive inventory must name the five archives a mirror holds"
    );
}

/// REQ: SEC-961
#[test]
fn sec_961_a_mirror_never_resigns_sha256sums_or_mixes_two_release_tags() {
    let page = manual_page("reference-host-release-verification.md");
    assert!(
        page.contains("A mirror MUST NOT edit, rebuild or re-sign `SHA256SUMS`, and MUST NOT combine files from two release tags."),
        "the page must forbid editing, rebuilding, re-signing or mixing tags"
    );
}

/// REQ: SEC-962
#[test]
fn sec_962_each_offline_verification_leaves_a_record_with_tag_name_hash_time_machine_and_result() {
    let page = manual_page("reference-host-release-verification.md");
    assert!(
        page.contains(
            "Each offline verification MUST leave a written record with the release tag, the archive name, the SHA-256 hash, the time of the check in `YYYY-MM-DD HH:mm:ss`, the machine that ran the attestation check and the result."
        ),
        "the page must list the seven fields of the offline record and the timestamp format"
    );
}

/// REQ: SEC-962
#[test]
fn sec_962_a_record_without_an_attestation_result_is_not_proof_of_genuineness() {
    let page = manual_page("reference-host-release-verification.md");
    assert!(
        page.contains(
            "A record without the attestation result MUST NOT be used as proof that the archive is genuine."
        ),
        "the page must refuse a record that lacks the attestation result as proof"
    );
}
