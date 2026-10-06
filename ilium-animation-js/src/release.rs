//! Immutable release identities. A package manifest cannot add itself here.
//! Archives ship beside the binaries; their bytes are not embedded in the TUI.
use crate::{
    error::Result,
    trust::{OfficialPackage, TrustVerifier},
};

pub const PACKAGES: &[(&str, &str, &str)] = &[
    (
        "beach",
        "beach-1.0.0.iliumanim",
        "21a195c4641ab73914ba0d7ad535ffbdff1ab116323f933f0df9cce973e33777",
    ),
    (
        "carpet",
        "carpet-1.0.0.iliumanim",
        "05abd4bd37231f1cb7502ae037a9bb7af39043440c72b173948cd12ceb8caf8b",
    ),
];

pub fn verifier() -> Result<TrustVerifier> {
    TrustVerifier::from_release_inventory(
        PACKAGES
            .iter()
            .map(|(id, _, digest)| OfficialPackage {
                id: (*id).into(),
                digest: (*digest).into(),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{Package, PackageLimits};
    #[test]
    fn shipped_archives_match_the_compiled_release_inventory() {
        let inventory = verifier().unwrap();
        for (id, file, digest) in PACKAGES {
            // Tests read release artifacts; production does not include their
            // code or assets in the program or expand them during listing.
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/packages")
                .join(file);
            let bytes = std::fs::read(path).unwrap();
            let package = Package::from_bytes(&bytes, PackageLimits::default()).unwrap();
            let identity = inventory.verify(&package);
            assert_eq!(identity.id(), *id);
            assert_eq!(identity.digest(), *digest);
            assert!(identity.is_ilium());
        }
    }
}
