//! Worker-only lookup of locally installed private test packs. The registry
//! stores paths and mounting metadata; artwork remains outside the executable.
use super::{
    assets::{
        budget::Cancel,
        error::{AssetError, Result},
        identity::AssetPath,
    },
    pack_profiles,
    settings::{PackSourceSettings, VoxelLandscapeSettings},
};
use crate::control::SceneSettings;
use ilium_platform::secure_fs::NoFollowDirectory;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, ffi::OsStr, io::Read, path::Path};

const MAX_REGISTRY_BYTES: u64 = 65_536;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledPack {
    pub id: String,
    pub source: PackSourceSettings,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackRegistry {
    pub schema: u32,
    #[serde(deserialize_with = "deserialize_profiles")]
    pub profiles: Vec<InstalledPack>,
}

// Old registry files can contain eleven entries. Discard only retired records
// on read; never resolve their paths, expose them as profiles, or rewrite the file.
fn deserialize_profiles<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<InstalledPack>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut profiles = Vec::<InstalledPack>::deserialize(deserializer)?;
    if profiles.len() > pack_profiles::FULL_PACKS.len() + pack_profiles::RETIRED_PACK_IDS.len() {
        return Err(serde::de::Error::custom("too many installed pack records"));
    }
    let mut seen = BTreeSet::new();
    if profiles.iter().any(|entry| !seen.insert(&entry.id)) {
        return Err(serde::de::Error::custom("repeated installed pack"));
    }
    profiles.retain(|entry| !pack_profiles::RETIRED_PACK_IDS.contains(&entry.id.as_str()));
    Ok(profiles)
}

impl PackRegistry {
    fn selected(&self, id: &str) -> Result<&PackSourceSettings> {
        if self.schema != 1 || self.profiles.len() > pack_profiles::FULL_PACKS.len() {
            return Err(AssetError::InvalidMetadata(
                "invalid installed pack registry version or count".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        for entry in &self.profiles {
            if !pack_profiles::FULL_PACKS
                .iter()
                .any(|profile| profile.id == entry.id)
                || !seen.insert(&entry.id)
            {
                return Err(AssetError::InvalidMetadata(
                    "unknown or repeated installed pack".into(),
                ));
            }
            validate_source(&entry.source, &entry.id)?;
        }
        self.profiles
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| &entry.source)
            .ok_or_else(|| AssetError::InvalidPath(format!("selected pack {id} is not installed")))
    }
}

fn validate_source(source: &PackSourceSettings, id: &str) -> Result<()> {
    for path in [&source.path, &source.addon_path] {
        if path.len() > 4096
            || path.chars().any(char::is_control)
            || (!path.is_empty() && !Path::new(path).is_absolute())
        {
            return Err(AssetError::InvalidPath(
                "installed pack requires an absolute local path".into(),
            ));
        }
    }
    if source.path.is_empty()
        || source.mount > 1
        || source.addon_mount > 1
        || source.edition > 1
        || (source.edition == 1 && id != "plasticator")
        || (source.duplicate_last_wins && id != "textureless")
        || (!source.addon_path.is_empty() && id != "textureless")
        || source.format_major > i32::MAX as u32
        || source.format_minor > i32::MAX as u32
    {
        return Err(AssetError::InvalidMetadata(
            "invalid registered pack mount".into(),
        ));
    }
    if !source.root.is_empty() {
        AssetPath::parse(&source.root)?;
    }
    Ok(())
}

pub fn resolve_registered(
    settings: &VoxelLandscapeSettings,
    cache_dir: &Path,
    cancel: Cancel<'_>,
) -> Result<VoxelLandscapeSettings> {
    cancel.check()?;
    let settings = settings.normalized();
    let profile = pack_profiles::profile(settings.pack_profile)?;
    if !settings.pack_path.is_empty() {
        validate_source(&settings.source_settings(), profile.id)?;
        return Ok(settings.clone());
    }
    let directory = NoFollowDirectory::open_root(cache_dir)
        .and_then(|root| root.open_directory(OsStr::new("voxel-packs")))
        .map_err(|error| AssetError::InvalidPath(format!("installed texture packs: {error}")))?;
    let file = directory
        .open_regular(OsStr::new("sources.json"))
        .map_err(|error| {
            AssetError::InvalidPath(format!("installed texture pack registry: {error}"))
        })?;
    let mut bytes = Vec::new();
    file.take(MAX_REGISTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AssetError::InvalidMetadata(format!("pack registry read: {error}")))?;
    cancel.check()?;
    if bytes.len() as u64 > MAX_REGISTRY_BYTES {
        return Err(AssetError::Limit {
            resource: "installed pack registry",
            requested: bytes.len() as u64,
            limit: MAX_REGISTRY_BYTES,
        });
    }
    let registry: PackRegistry = serde_json::from_slice(&bytes)
        .map_err(|error| AssetError::InvalidMetadata(format!("pack registry: {error}")))?;
    let mut resolved = settings.clone();
    resolved.apply_source_settings(registry.selected(profile.id)?);
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, path: &str) -> InstalledPack {
        let mut source = VoxelLandscapeSettings::default().source_settings();
        source.path = path.into();
        InstalledPack {
            id: id.into(),
            source,
        }
    }

    #[test]
    fn registered_selection_is_by_identity_and_preserves_mount_metadata() {
        let mut goodvibes = entry("goodvibes", "/private/art");
        goodvibes.source.mount = 1;
        let registry = PackRegistry {
            schema: 1,
            profiles: vec![entry("faithful32", "/private/32.zip"), goodvibes],
        };
        assert_eq!(registry.selected("goodvibes").unwrap().mount, 1);
        assert_eq!(
            registry.selected("faithful32").unwrap().path,
            "/private/32.zip"
        );
        assert!(registry.selected("faithful64").is_err());
    }

    #[test]
    fn malformed_or_ambiguous_registry_never_selects_another_pack() {
        for profiles in [
            vec![entry("goodvibes", "/a"), entry("goodvibes", "/b")],
            vec![entry("unknown", "/a")],
            vec![entry("goodvibes", "relative")],
        ] {
            assert!(PackRegistry {
                schema: 1,
                profiles
            }
            .selected("goodvibes")
            .is_err());
        }
        let mut entry = entry("goodvibes", "/a");
        entry.source.duplicate_last_wins = true;
        assert!(PackRegistry {
            schema: 1,
            profiles: vec![entry]
        }
        .selected("goodvibes")
        .is_err());
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn entry(id: &str) -> InstalledPack {
        let mut source = VoxelLandscapeSettings::default().source_settings();
        source.path = format!("/packs/{id}.zip");
        InstalledPack {
            id: id.into(),
            source,
        }
    }

    #[test]
    fn legacy_registry_projects_only_eight_retained_sources_without_editing_input() {
        let mut profiles: Vec<_> = pack_profiles::RETIRED_PACK_IDS
            .into_iter()
            .map(entry)
            .collect();
        profiles.extend(
            pack_profiles::FULL_PACKS
                .iter()
                .map(|profile| entry(profile.id)),
        );
        let bytes = serde_json::to_vec(&PackRegistry {
            schema: 1,
            profiles,
        })
        .unwrap();
        let registry: PackRegistry = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(registry.profiles.len(), 8);
        for profile in pack_profiles::FULL_PACKS {
            assert_eq!(
                registry.selected(profile.id).unwrap().path,
                format!("/packs/{}.zip", profile.id)
            );
        }
        for retired in pack_profiles::RETIRED_PACK_IDS {
            assert!(registry.selected(retired).is_err());
            assert!(!registry
                .profiles
                .iter()
                .any(|profile| profile.id == retired));
        }
        let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(original["profiles"].as_array().unwrap().len(), 11);
        let projected = serde_json::to_value(&registry).unwrap();
        assert_eq!(projected["profiles"].as_array().unwrap().len(), 8);
    }

    #[test]
    fn retired_registry_sources_are_inert_but_unknowns_and_duplicates_still_fail() {
        let mut retired = entry("jicklus");
        retired.source.path = "relative-never-opened".into();
        let registry: PackRegistry = serde_json::from_value(
            serde_json::to_value(PackRegistry {
                schema: 1,
                profiles: vec![retired, entry("goodvibes")],
            })
            .unwrap(),
        )
        .unwrap();
        assert!(registry.selected("goodvibes").is_ok());
        for profiles in [
            vec![entry("goodvibes"), entry("goodvibes")],
            vec![entry("jicklus"), entry("jicklus")],
        ] {
            assert!(serde_json::from_value::<PackRegistry>(
                serde_json::to_value(PackRegistry {
                    schema: 1,
                    profiles
                })
                .unwrap()
            )
            .is_err());
        }
        let registry: PackRegistry = serde_json::from_value(
            serde_json::to_value(PackRegistry {
                schema: 1,
                profiles: vec![entry("goodvibes"), entry("unknown")],
            })
            .unwrap(),
        )
        .unwrap();
        assert!(registry.selected("goodvibes").is_err());
    }

    #[test]
    fn raw_legacy_selection_migrates_before_custom_source_or_installed_lookup() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("voxel-packs");
        std::fs::create_dir(&directory).unwrap();
        let registry = serde_json::to_vec(&PackRegistry {
            schema: 1,
            profiles: vec![entry("jicklus"), entry("goodvibes")],
        })
        .unwrap();
        let path = directory.join("sources.json");
        std::fs::write(&path, &registry).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let legacy = VoxelLandscapeSettings {
            pack_profile_version: 0,
            pack_profile: 0,
            pack_path: "/packs/private.zip".into(),
            ..Default::default()
        };
        let resolved = resolve_registered(&legacy, root.path(), cancel).unwrap();
        assert_eq!(resolved.pack_profile, 0);
        assert_eq!(resolved.pack_profile_version, 1);
        assert_eq!(resolved.pack_path, "/packs/goodvibes.zip");
        assert_eq!(
            resolved.pack_custom_sources["jicklus"].path,
            "/packs/private.zip"
        );
        assert_eq!(std::fs::read(path).unwrap(), registry);
        let legacy = VoxelLandscapeSettings {
            pack_profile_version: 0,
            pack_profile: 8,
            pack_path: "/packs/custom-faithful32.zip".into(),
            ..Default::default()
        };
        let resolved = resolve_registered(&legacy, root.path(), cancel).unwrap();
        assert_eq!(resolved.pack_profile, 5);
        assert_eq!(resolved.pack_path, "/packs/custom-faithful32.zip");
    }
}
