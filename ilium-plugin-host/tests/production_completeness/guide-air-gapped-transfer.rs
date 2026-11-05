/// REQ: PDV-001
#[test]
fn pdv_001_bundle_create_writes_single_archive_with_hashed_members() {
    // Observable: Create a bundle for two installed plugins and list the archive: it contains two package files, their signature files, the metadata files and a `bundle.json` whose `members` has one SHA-256 per other member.
    panic!(
        "PDV-001 not implemented: `ilium plugin bundle create --out <file>` MUST write a single archive holding the package files, their signatures and the signed registry metadata, plus a `bundle.json` listing the SHA-256 of every member."
    );
}

/// REQ: PDV-002
#[test]
fn pdv_002_bundle_import_verifies_all_hashes_and_signatures_before_copy() {
    // Observable: Flip one byte in a package inside a bundle and import it: the exit code is 3, the output names the member, and the allowed package source directory is unchanged.
    panic!(
        "PDV-002 not implemented: `ilium plugin bundle import <file>` MUST verify every member hash and every signature before copying anything into the package source named in `packages.allowed_sources`, and on any mismatch MUST copy nothing and exit with code 3."
    );
}

/// REQ: PDV-003
#[test]
fn pdv_003_bundle_refuses_unpinned_root_with_root_not_pinned_reason() {
    // Observable: Import a bundle signed by an unpinned root on a client with a different pinned root: the exit code is 3 and the output contains `bundle.root-not-pinned`.
    panic!(
        "PDV-003 not implemented: A bundle MUST NOT carry registry root keys, and import MUST refuse with `bundle.root-not-pinned` a bundle whose metadata is signed by a root key not pinned under [PKG-985]."
    );
}

/// REQ: PDV-004
#[test]
fn pdv_004_expired_bundle_metadata_refuses_install_but_keeps_plugins_running() {
    // Observable: Advance the clock past the bundle metadata `expires` and run `ilium plugin install --from-bundle`: the exit code is non-zero with `metadata-expired`, and `ilium plugin list` still shows existing plugins as `enabled`.
    panic!(
        "PDV-004 not implemented: When the signed metadata `expires` time has passed, installs from the bundle MUST be refused with reason `metadata-expired`, and already installed plugins MUST keep running."
    );
}

/// REQ: PDV-005
#[test]
fn pdv_005_newer_revocation_list_applied_with_one_audit_record_per_key() {
    // Observable: Import a bundle whose revocation list adds two keys: the audit log gains exactly two `trust.key-revoked` lines, each with a key id.
    panic!(
        "PDV-005 not implemented: A bundle carrying a revocation list newer than the stored one MUST apply it at import and write one `trust.key-revoked` audit record per newly revoked key."
    );
}

/// REQ: PDV-006
#[test]
fn pdv_006_import_writes_bundle_import_audit_record() {
    // Observable: After a successful import, `ilium plugin audit --event bundle-import --json` prints one line with `sha256`, `member_count` and `result: "ok"`.
    panic!(
        "PDV-006 not implemented: Import MUST write one `bundle-import` audit record with the bundle SHA-256, the member count and the result."
    );
}

/// REQ: PDV-007
#[test]
fn pdv_007_older_metadata_version_import_refused_as_rollback() {
    // Observable: Import a bundle with snapshot version 4 after version 5 is stored: the exit code is 3 and the output contains `rollback`.
    panic!(
        "PDV-007 not implemented: Importing a bundle whose metadata version is older than the stored metadata version MUST be refused with reason `rollback` and exit code 3."
    );
}
