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
        "4b47934f4285ae426f680929b59af7151f4ac2e73ad41292872cfccd516cda30",
    ),
    (
        "carpet",
        "carpet-1.0.0.iliumanim",
        "c4cfdbc6d088361e488e8a7544162cc19a55dd0fea1c8bb237ad467b029db870",
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
