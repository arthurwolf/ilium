//! Save metadata and relative recency. Header counts remain unqualified terrain.
use super::{nbt, region};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Save {
    pub directory: PathBuf,
    pub metadata: Metadata,
}

#[derive(Debug, Default)]
pub struct Catalog {
    /// Relative LastPlayed order; terrain qualification happens afterwards.
    pub maps: Vec<Save>,
    pub rejected_maps: usize,
    pub issues: Vec<MapIssue>,
}

#[derive(Debug)]
pub struct MapIssue {
    pub directory: PathBuf,
    pub message: String,
}

pub fn discover_metadata(
    saves_root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Catalog, Error> {
    if cancelled() {
        return Err(region::Error::Cancelled.into());
    }
    let mut catalog = Catalog::default();
    for directory in super::discovery::save_folders(saves_root)? {
        match read_metadata(&directory, cancelled) {
            Ok(metadata) => catalog.maps.push(Save {
                directory,
                metadata,
            }),
            Err(Error::Region(region::Error::Cancelled)) => {
                return Err(region::Error::Cancelled.into())
            }
            Err(error) => {
                catalog.rejected_maps += 1;
                if catalog.issues.len() < 64 {
                    catalog.issues.push(MapIssue {
                        directory,
                        message: error.to_string(),
                    });
                }
            }
        }
    }
    rank_recent(&mut catalog.maps);
    Ok(catalog)
}

fn rank_recent(maps: &mut [Save]) {
    maps.sort_by(|left, right| {
        right
            .metadata
            .last_played
            .cmp(&left.metadata.last_played)
            .then_with(|| left.directory.cmp(&right.directory))
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub name: nbt::Text,
    pub data_version: i32,
    pub last_played: i64,
    /// Candidate anchor only. Saved chunk qualification still decides coverage.
    pub spawn_position: Option<[i32; 3]>,
    /// Canonical WorldGenSettings.seed Long; missing/noncanonical values remain
    /// unavailable to seeded rendering, without excluding catalog discovery.
    pub world_seed: Option<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("save metadata I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Region(#[from] region::Error),
    #[error(transparent)]
    Nbt(#[from] nbt::Error),
    #[error("invalid save metadata: {0}")]
    Invalid(&'static str),
}

pub fn metadata(document: &nbt::Document) -> Result<Metadata, Error> {
    let Some(nbt::Tag::Compound(data)) = nbt::get(&document.root, "Data") else {
        return Err(Error::Invalid("missing Data compound"));
    };
    let Some(nbt::Tag::Int(data_version)) = nbt::get(data, "DataVersion") else {
        return Err(Error::Invalid("missing DataVersion"));
    };
    if !(region::MIN_DATA_VERSION..=region::MAX_DATA_VERSION).contains(data_version) {
        return Err(region::Error::DataVersion(*data_version).into());
    }
    let Some(nbt::Tag::Long(last_played)) = nbt::get(data, "LastPlayed") else {
        return Err(Error::Invalid("missing LastPlayed"));
    };
    if *last_played < 0 {
        return Err(Error::Invalid("negative LastPlayed"));
    }
    let Some(nbt::Tag::String(name)) = nbt::get(data, "LevelName") else {
        return Err(Error::Invalid("missing LevelName"));
    };
    let spawn = ["SpawnX", "SpawnY", "SpawnZ"].map(|field| nbt::get(data, field));
    let spawn_position = match spawn {
        [None, None, None] => None,
        [Some(nbt::Tag::Int(x)), Some(nbt::Tag::Int(y)), Some(nbt::Tag::Int(z))] => {
            Some([*x, *y, *z])
        }
        _ => return Err(Error::Invalid("partial or non-Int spawn position")),
    };
    // Canonical Java serialization writes a Long. Its native codec can coerce
    // other numeric tags; this reader deliberately does not claim that parity.
    // Supported modern versions do not use the historical RandomSeed fixer.
    let world_seed = match nbt::get(data, "WorldGenSettings") {
        Some(nbt::Tag::Compound(settings)) => match nbt::get(settings, "seed") {
            Some(nbt::Tag::Long(seed)) => Some(*seed),
            _ => None,
        },
        _ => None,
    };
    Ok(Metadata {
        name: name.clone(),
        data_version: *data_version,
        last_played: *last_played,
        spawn_position,
        world_seed,
    })
}

/// Bounded, read-only Java level.dat (one gzip stream). The surrounding worker
/// owns cancellation; this function does no work on the animation thread.
pub fn read_metadata(save: &Path, cancelled: &dyn Fn() -> bool) -> Result<Metadata, Error> {
    use std::io::Read;
    if cancelled() {
        return Err(region::Error::Cancelled.into());
    }
    let mut file = super::io::open_regular(&save.join("level.dat"))?;
    let limit = region::Limits::default();
    let mut compressed = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        if cancelled() {
            return Err(region::Error::Cancelled.into());
        }
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if count > limit.max_compressed_bytes.saturating_sub(compressed.len()) {
            return Err(Error::Invalid("compressed level.dat exceeds limit"));
        }
        compressed.extend_from_slice(&buffer[..count]);
    }
    let bytes = region::decompress(
        region::Compression::Gzip,
        &compressed,
        limit.nbt.max_bytes,
        cancelled,
    )?;
    let document = nbt::parse_checked(&bytes, limit.nbt, cancelled).map_err(|error| {
        if error.reason == "cancelled" {
            Error::Region(region::Error::Cancelled)
        } else {
            Error::Nbt(error)
        }
    })?;
    metadata(&document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn document(version: i32, played: i64) -> nbt::Document {
        let data = BTreeMap::from([
            (nbt::Text::from("DataVersion"), nbt::Tag::Int(version)),
            (nbt::Text::from("LastPlayed"), nbt::Tag::Long(played)),
            (
                nbt::Text::from("LevelName"),
                nbt::Tag::String(nbt::Text(vec![0x41, 0xd800])),
            ),
        ]);
        nbt::Document {
            name: nbt::Text::from(""),
            root: BTreeMap::from([(nbt::Text::from("Data"), nbt::Tag::Compound(data))]),
        }
    }

    #[test]
    fn old_dates_remain_eligible_and_names_remain_lossless() {
        let value = metadata(&document(2834, 1_630_000_000_000)).unwrap();
        assert_eq!(value.last_played, 1_630_000_000_000);
        assert_eq!(value.name.0, [0x41, 0xd800]);
        assert!(value.name.to_utf8().is_err());
        assert!(metadata(&document(3218, 0)).is_ok());
    }

    #[test]
    fn canonical_world_seed_preserves_all_signed_long_values() {
        for seed in [i64::MIN, -1, 0, 1, i64::MAX] {
            let mut document = document(3218, 0);
            let Some(nbt::Tag::Compound(data)) = document.root.get_mut(&"Data".into()) else {
                panic!("fixture Data");
            };
            data.insert(
                "WorldGenSettings".into(),
                nbt::Tag::Compound(BTreeMap::from([("seed".into(), nbt::Tag::Long(seed))])),
            );
            assert_eq!(metadata(&document).unwrap().world_seed, Some(seed));
        }
    }

    #[test]
    fn absent_or_noncanonical_seed_never_invents_a_render_seed() {
        let mut document = document(3218, 0);
        assert_eq!(metadata(&document).unwrap().world_seed, None);
        let Some(nbt::Tag::Compound(data)) = document.root.get_mut(&"Data".into()) else {
            panic!("fixture Data");
        };
        data.insert("RandomSeed".into(), nbt::Tag::Long(123));
        assert_eq!(metadata(&document).unwrap().world_seed, None);
        for settings in [
            nbt::Tag::Long(7),
            nbt::Tag::Compound(BTreeMap::new()),
            nbt::Tag::Compound(BTreeMap::from([("seed".into(), nbt::Tag::Int(7))])),
            nbt::Tag::Compound(BTreeMap::from([(
                "seed".into(),
                nbt::Tag::String("7".into()),
            )])),
        ] {
            let Some(nbt::Tag::Compound(data)) = document.root.get_mut(&"Data".into()) else {
                panic!("fixture Data");
            };
            data.insert("WorldGenSettings".into(), settings);
            assert_eq!(metadata(&document).unwrap().world_seed, None);
        }
    }

    #[test]
    fn spawn_anchor_preserves_signed_coordinates_and_allows_absence() {
        let mut document = document(3218, 0);
        assert_eq!(metadata(&document).unwrap().spawn_position, None);
        let Some(nbt::Tag::Compound(data)) = document.root.get_mut(&nbt::Text::from("Data")) else {
            panic!("fixture Data");
        };
        for (field, value) in [("SpawnX", -97), ("SpawnY", 74), ("SpawnZ", 127)] {
            data.insert(field.into(), nbt::Tag::Int(value));
        }
        assert_eq!(
            metadata(&document).unwrap().spawn_position,
            Some([-97, 74, 127])
        );
    }

    #[test]
    fn partial_or_mistyped_spawn_anchor_is_not_a_valid_coordinate() {
        let mut document = document(3218, 0);
        let Some(nbt::Tag::Compound(data)) = document.root.get_mut(&nbt::Text::from("Data")) else {
            panic!("fixture Data");
        };
        data.insert("SpawnX".into(), nbt::Tag::Int(0));
        assert!(matches!(metadata(&document), Err(Error::Invalid(_))));
        let Some(nbt::Tag::Compound(data)) = document.root.get_mut(&nbt::Text::from("Data")) else {
            panic!("fixture Data");
        };
        data.insert("SpawnY".into(), nbt::Tag::Long(64));
        data.insert("SpawnZ".into(), nbt::Tag::Int(0));
        assert!(matches!(metadata(&document), Err(Error::Invalid(_))));
    }

    #[test]
    fn rejects_unqualified_versions_and_invalid_timestamps() {
        assert!(metadata(&document(2833, 1)).is_err());
        assert!(metadata(&document(3219, 1)).is_err());
        assert!(metadata(&document(2834, -1)).is_err());
    }

    #[test]
    fn ranks_relative_dates_with_stable_path_ties_without_an_age_cutoff() {
        let mut maps: Vec<Save> = [("b", 200), ("a", 200), ("c", 100)]
            .into_iter()
            .map(|(name, played)| Save {
                directory: name.into(),
                metadata: metadata(&document(2834, played)).unwrap(),
            })
            .collect();
        rank_recent(&mut maps);
        assert_eq!(
            maps.iter()
                .map(|map| map.directory.to_str().unwrap())
                .collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert_eq!(maps.len(), 3);
    }

    #[test]
    fn corrupt_map_is_reported_and_empty_cancel_is_explicit() {
        let directory = tempfile::tempdir().unwrap();
        let bad = directory.path().join("bad map");
        std::fs::create_dir(&bad).unwrap();
        std::fs::create_dir(bad.join("region")).unwrap();
        std::fs::write(bad.join("level.dat"), b"broken").unwrap();
        let result = discover_metadata(directory.path(), &|| false).unwrap();
        assert!(result.maps.is_empty());
        assert_eq!(result.rejected_maps, 1);
        assert_eq!(
            result.issues[0].directory,
            ilium_platform::paths::canonicalize(&bad).unwrap()
        );
        assert!(!result.issues[0].message.is_empty());
        assert!(matches!(
            discover_metadata(directory.path(), &|| true),
            Err(Error::Region(region::Error::Cancelled))
        ));
    }

    #[test]
    fn reads_real_gzip_nbt_without_writing_and_rejects_truncation() {
        use flate2::{write::GzEncoder, Compression};
        use std::io::Write;
        let mut bytes = vec![10, 0, 0, 10, 0, 4];
        bytes.extend(b"Data");
        for (kind, name, payload) in [
            (3, "DataVersion", 2834_i32.to_be_bytes().to_vec()),
            (
                4,
                "LastPlayed",
                1_630_000_000_000_i64.to_be_bytes().to_vec(),
            ),
            (8, "LevelName", vec![0, 1, b'A']),
        ] {
            bytes.push(kind);
            bytes.extend((name.len() as u16).to_be_bytes());
            bytes.extend(name.as_bytes());
            bytes.extend(payload);
        }
        bytes.extend([10, 0, 16]);
        bytes.extend(b"WorldGenSettings");
        bytes.extend([4, 0, 4]);
        bytes.extend(b"seed");
        bytes.extend((-9_223_372_036_854_775_807_i64).to_be_bytes());
        bytes.extend([0, 0, 0]);
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&bytes).unwrap();
        let compressed = encoder.finish().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("level.dat");
        std::fs::write(&path, &compressed).unwrap();
        assert_eq!(
            read_metadata(directory.path(), &|| false).unwrap().name,
            nbt::Text::from("A")
        );
        assert_eq!(
            read_metadata(directory.path(), &|| false)
                .unwrap()
                .world_seed,
            Some(-9_223_372_036_854_775_807)
        );
        assert_eq!(std::fs::read(&path).unwrap(), compressed);
        assert!(matches!(
            read_metadata(directory.path(), &|| true),
            Err(Error::Region(region::Error::Cancelled))
        ));
        std::fs::write(&path, &compressed[..compressed.len() - 1]).unwrap();
        assert!(read_metadata(directory.path(), &|| false).is_err());
    }
}
