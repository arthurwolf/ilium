use ilium_animation_js::{
    package::{Package, PackageLimits},
    trust::{OfficialPackage, TrustVerifier},
};

fn fixture() -> Package {
    Package::from_bytes(
        include_bytes!("fixtures/sdk-fixture.iliumanim"),
        PackageLimits::default(),
    )
    .unwrap()
}
#[test]
fn identity_and_exact_content_inventory_are_both_required() {
    let package = fixture();
    let official = OfficialPackage {
        id: package.manifest().id.clone(),
        digest: package.digest().to_owned(),
    };
    let verified = TrustVerifier::from_release_inventory(vec![official])
        .unwrap()
        .verify(&package);
    assert!(verified.is_ilium());
    assert_eq!(verified.digest(), package.digest());
    let renamed = OfficialPackage {
        id: "spoof".into(),
        digest: package.digest().to_owned(),
    };
    assert!(!TrustVerifier::from_release_inventory(vec![renamed])
        .unwrap()
        .verify(&package)
        .is_ilium());
}
#[test]
fn unknown_and_tampered_replacements_never_inherit_automatic_rights() {
    let package = fixture();
    assert!(!TrustVerifier::from_release_inventory(vec![])
        .unwrap()
        .verify(&package)
        .is_ilium());
    let wrong = OfficialPackage {
        id: package.manifest().id.clone(),
        digest: "0".repeat(64),
    };
    assert!(!TrustVerifier::from_release_inventory(vec![wrong])
        .unwrap()
        .verify(&package)
        .is_ilium());
}
