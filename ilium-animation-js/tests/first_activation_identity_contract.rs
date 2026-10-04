//! Pure task-local packages: no V8, helper, user state or broker activation.
use ilium_animation_js::{
    error::AnimationError,
    package::{inspect_manifest, Package, PackageLimits},
    permissions::PackageIdentity as PermissionIdentity,
    trust::{OfficialPackage, TrustVerifier},
};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Write};

fn archive(source: &[u8], reversed: bool) -> Vec<u8> {
    let metadata = serde_json::to_vec(&serde_json::json!({
        "api_version":1,"id":"principal-fixture","name":"Fixture","version":"1.0.0",
        "entry":"entry.mjs","modes":["live"],
        "files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source))}]
    })).unwrap();
    let normal = [
        ("manifest.json", metadata.as_slice()),
        ("entry.mjs", source),
    ];
    let mut entries = normal.to_vec();
    if reversed {
        entries.reverse();
    }
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn package(bytes: &[u8]) -> Package {
    Package::from_bytes(bytes, PackageLimits::default()).unwrap()
}
fn inventory(packages: &[&Package]) -> TrustVerifier {
    TrustVerifier::from_release_inventory(
        packages
            .iter()
            .map(|package| OfficialPackage {
                id: package.manifest().id.clone(),
                digest: package.digest().to_owned(),
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn empty_inventory_derives_exact_native_principal_before_any_broker_or_helper() {
    let bytes = archive(b"export const value=1;", false);
    let package = package(&bytes);
    let verifier = inventory(&[]);
    let identity = verifier.permission_identity(&package).unwrap();
    let previous = PermissionIdentity::unverified(package.manifest().id.clone(), &bytes).unwrap();
    assert_eq!(identity, previous);
    assert_eq!(
        identity.content_hash(),
        <[u8; 32]>::from(Sha256::digest(&bytes))
    );
    assert_eq!(package.digest(), format!("{:x}", Sha256::digest(&bytes)));
    assert!(identity
        .principal_key()
        .starts_with("unsigned:principal-fixture:"));
    assert!(!verifier.verify(&package).is_ilium());
}

#[test]
fn exact_compiled_inventory_derives_ilium_lineage_and_same_raw_archive_hash() {
    let bytes = archive(b"export const value=1;", false);
    let package = package(&bytes);
    let verifier = inventory(&[&package]);
    let identity = verifier.permission_identity(&package).unwrap();
    assert_eq!(identity.principal_key(), "ilium:principal-fixture");
    assert_eq!(
        identity.content_hash(),
        <[u8; 32]>::from(Sha256::digest(&bytes))
    );
    assert!(verifier.verify(&package).is_ilium());
    assert_eq!(
        identity,
        verifier
            .permission_identity_for_archive(&package, &bytes)
            .unwrap()
    );
}

#[test]
fn repacking_identical_modules_and_manifest_changes_unsigned_and_release_identity() {
    let bytes1 = archive(b"export const value=1;", false);
    let bytes2 = archive(b"export const value=1;", true);
    let package1 = package(&bytes1);
    let package2 = package(&bytes2);
    assert_eq!(package1.files(), package2.files());
    assert_eq!(
        serde_json::to_value(package1.manifest()).unwrap(),
        serde_json::to_value(package2.manifest()).unwrap()
    );
    assert_ne!(package1.digest(), package2.digest());
    let unknown = inventory(&[]);
    assert_ne!(
        unknown
            .permission_identity(&package1)
            .unwrap()
            .principal_key(),
        unknown
            .permission_identity(&package2)
            .unwrap()
            .principal_key()
    );
    let official = inventory(&[&package1]);
    assert_eq!(
        official
            .permission_identity(&package1)
            .unwrap()
            .principal_key(),
        "ilium:principal-fixture"
    );
    assert!(official
        .permission_identity(&package2)
        .unwrap()
        .principal_key()
        .starts_with("unsigned:"));
    assert!(!official.verify(&package2).is_ilium());
}

#[test]
fn supplied_archive_must_match_the_same_parsed_package_not_just_title_or_files() {
    let bytes1 = archive(b"export const value=1;", false);
    let repacked = archive(b"export const value=1;", true);
    let package1 = package(&bytes1);
    let verifier = inventory(&[&package1]);
    assert_eq!(bytes1.len(), repacked.len());
    assert!(matches!(
        verifier.permission_identity_for_archive(&package1, &repacked),
        Err(AnimationError::Integrity(_))
    ));
    assert!(matches!(
        verifier.permission_identity_for_archive(&package1, &bytes1[..bytes1.len() - 1]),
        Err(AnimationError::Integrity(_))
    ));
    let mut corrupt = bytes1.clone();
    corrupt[0] ^= 1;
    assert!(matches!(
        verifier.permission_identity_for_archive(&package1, &corrupt),
        Err(AnimationError::Integrity(_))
    ));
    assert_eq!(
        verifier
            .permission_identity_for_archive(&package1, &bytes1)
            .unwrap()
            .principal_key(),
        "ilium:principal-fixture"
    );
}

#[test]
fn unrecognized_content_and_wrong_inventory_ids_never_inherit_known_lineage() {
    let first = package(&archive(b"export const value=1;", false));
    let replacement = package(&archive(b"export const value=2;", false));
    let known = inventory(&[&first]);
    let unknown = known.permission_identity(&replacement).unwrap();
    assert!(unknown
        .principal_key()
        .starts_with("unsigned:principal-fixture:"));
    assert_ne!(
        unknown.content_hash(),
        known.permission_identity(&first).unwrap().content_hash()
    );
    let wrong_id = TrustVerifier::from_release_inventory(vec![OfficialPackage {
        id: "publisher_claim".into(),
        digest: first.digest().into(),
    }])
    .unwrap();
    let wrong_hash = TrustVerifier::from_release_inventory(vec![OfficialPackage {
        id: first.manifest().id.clone(),
        digest: "0".repeat(64),
    }])
    .unwrap();
    assert!(wrong_id
        .permission_identity(&first)
        .unwrap()
        .principal_key()
        .starts_with("unsigned:"));
    assert!(wrong_hash
        .permission_identity(&first)
        .unwrap()
        .principal_key()
        .starts_with("unsigned:"));
}

#[test]
fn authenticated_updates_share_lineage_only_when_both_exact_archives_are_in_inventory() {
    let first = package(&archive(b"export const value=1;", false));
    let update = package(&archive(b"export const value=2;", false));
    let known = inventory(&[&first, &update]);
    let first_identity = known.permission_identity(&first).unwrap();
    let updated_identity = known.permission_identity(&update).unwrap();
    assert_eq!(
        first_identity.principal_key(),
        updated_identity.principal_key()
    );
    assert_ne!(
        first_identity.content_hash(),
        updated_identity.content_hash()
    );
    let unknown = inventory(&[]);
    assert_ne!(
        unknown.permission_identity(&first).unwrap().principal_key(),
        unknown
            .permission_identity(&update)
            .unwrap()
            .principal_key()
    );
}

#[test]
fn descriptor_is_not_a_package_proof_and_tampered_module_cannot_produce_identity() {
    let source = b"export const marker=42;";
    let mut bytes = archive(source, false);
    let offset = bytes
        .windows(source.len())
        .position(|value| value == source)
        .unwrap();
    bytes[offset] ^= 1;
    assert_eq!(
        inspect_manifest(&bytes, PackageLimits::default())
            .unwrap()
            .id,
        "principal-fixture"
    );
    assert!(Package::from_bytes(&bytes, PackageLimits::default()).is_err());
    // No constructor accepts a Manifest, trust flag or caller-provided digest.
}

#[test]
fn identity_lookup_retains_no_archive_and_does_not_replace_or_clone_file_storage() {
    let bytes = archive(b"export const value=1;", false);
    let package = package(&bytes);
    let raw_hash: [u8; 32] = Sha256::digest(&bytes).into();
    drop(bytes);
    let verifier = inventory(&[&package]);
    let source = package.files().get("entry.mjs").unwrap().as_ptr();
    for _ in 0..32 {
        let identity = verifier.permission_identity(&package).unwrap();
        assert_eq!(identity.content_hash(), raw_hash);
        assert_eq!(package.files().get("entry.mjs").unwrap().as_ptr(), source);
    }
}
