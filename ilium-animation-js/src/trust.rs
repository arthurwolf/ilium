//! Exact immutable release inventory, independent of installation transport.
//! This inventory is supplied by the compiled release composition root, never
//! deserialized from a package manifest or editable plugin configuration.
use crate::{
    error::{AnimationError, Result},
    package::Package,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct OfficialPackage {
    pub id: String,
    pub digest: String,
}

#[derive(Debug, Clone)]
pub struct PackageIdentity {
    id: String,
    digest: String,
    verified_ilium: bool,
}
impl PackageIdentity {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn is_ilium(&self) -> bool {
        self.verified_ilium
    }
}

pub struct TrustVerifier {
    inventory: BTreeSet<(String, String)>,
}
impl TrustVerifier {
    /// Only the host release code creates this inventory. Packages cannot
    /// submit or update it through the script/broker protocol.
    pub fn from_release_inventory(entries: Vec<OfficialPackage>) -> Result<Self> {
        if entries.len() > 4096 {
            return Err(AnimationError::Budget("release inventory".into()));
        }
        let mut inventory = BTreeSet::new();
        for entry in entries {
            if entry.id.is_empty()
                || entry.id.len() > 80
                || entry.digest.len() != 64
                || !entry
                    .digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(AnimationError::InvalidPackage(
                    "invalid host release identity".into(),
                ));
            }
            if !inventory.insert((entry.id, entry.digest)) {
                return Err(AnimationError::InvalidPackage(
                    "duplicate host release identity".into(),
                ));
            }
        }
        Ok(Self { inventory })
    }
    /// Host-only principal lookup before helper creation or broker.prepare.
    /// Caller retains the original package/identity metadata admission. This
    /// borrows verified files; it neither reparses nor deep-clones the Package.
    pub fn permission_identity(
        &self,
        package: &Package,
    ) -> Result<crate::permissions::PackageIdentity> {
        let known = self
            .inventory
            .iter()
            .find(|(id, digest)| id == &package.manifest().id && digest == package.digest());
        let identity = match known {
            Some((_, digest)) => {
                crate::permissions::PackageIdentity::from_package_inventory(package, digest)
            }
            None => crate::permissions::PackageIdentity::from_package(package),
        };
        identity.map_err(|error| AnimationError::PermissionDenied(error.to_string()))
    }
    /// For native consumers that still accept compressed bytes separately,
    /// bind them to the exact parsed Package before deriving any principal.
    /// The caller must already hold input admission for the borrowed archive.
    pub fn permission_identity_for_archive(
        &self,
        package: &Package,
        archive: &[u8],
    ) -> Result<crate::permissions::PackageIdentity> {
        if !package.archive_matches(archive) {
            return Err(AnimationError::Integrity(
                "package/archive identity mismatch".into(),
            ));
        }
        self.permission_identity(package)
    }
    /// Verification binds the fully validated immutable archive bytes. A URL,
    /// publisher string, matching name or descriptor alone grants no trust.
    pub fn verify(&self, package: &Package) -> PackageIdentity {
        let id = package.manifest().id.clone();
        let digest = package.digest().to_owned();
        let verified_ilium = self.inventory.contains(&(id.clone(), digest.clone()));
        PackageIdentity {
            id,
            digest,
            verified_ilium,
        }
    }
}
