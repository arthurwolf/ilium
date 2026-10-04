//! Synthetic on-disk Java metadata/chunks; no user saves or placeholder domain.
use super::super::{
    evidence,
    nbt::{self, Tag},
};
use super::*;
use std::{cell::Cell, io::Write};

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("saves");
    std::fs::create_dir(&root).unwrap();
    let root = paths::canonicalize(&root).unwrap();
    let storage = temporary.path().join("history");
    (temporary, root, storage)
}
fn limits() -> pipeline::Limits {
    pipeline::Limits {
        windows: super::super::windows::Limits {
            radius_chunks: 0,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn fields(entries: Vec<(&str, Tag)>) -> nbt::Compound {
    entries
        .into_iter()
        .map(|(name, value)| (name.into(), value))
        .collect()
}
fn text(output: &mut Vec<u8>, value: &nbt::Text) {
    let value = value.to_utf8().unwrap();
    assert!(value.is_ascii(), "synthetic fixture ASCII names");
    output.extend((value.len() as u16).to_be_bytes());
    output.extend(value.as_bytes());
}
fn kind(value: &Tag) -> u8 {
    match value {
        Tag::Byte(_) => 1,
        Tag::Int(_) => 3,
        Tag::Long(_) => 4,
        Tag::String(_) => 8,
        Tag::List { .. } => 9,
        Tag::Compound(_) => 10,
        Tag::LongArray(_) => 12,
        _ => panic!("unused fixture NBT type"),
    }
}
fn payload(output: &mut Vec<u8>, value: &Tag) {
    match value {
        Tag::Byte(value) => output.push(*value as u8),
        Tag::Int(value) => output.extend(value.to_be_bytes()),
        Tag::Long(value) => output.extend(value.to_be_bytes()),
        Tag::String(value) => text(output, value),
        Tag::List { kind, values } => {
            output.push(*kind);
            output.extend((values.len() as i32).to_be_bytes());
            for value in values {
                payload(output, value);
            }
        }
        Tag::Compound(values) => {
            for (name, value) in values {
                output.push(kind(value));
                text(output, name);
                payload(output, value);
            }
            output.push(0);
        }
        Tag::LongArray(values) => {
            output.extend((values.len() as i32).to_be_bytes());
            for value in values {
                output.extend(value.to_be_bytes());
            }
        }
        _ => panic!("unused fixture NBT type"),
    }
}
fn encode(root: nbt::Compound) -> Vec<u8> {
    let mut bytes = vec![10, 0, 0];
    payload(&mut bytes, &Tag::Compound(root));
    bytes
}
fn save(root: &Path, name: &str, played: i64, terrain: bool) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir(&path).unwrap();
    std::fs::create_dir(path.join("region")).unwrap();
    metadata_file(&path, played);
    if terrain {
        terrain_file(&path);
    }
    paths::canonicalize(&path).unwrap()
}
fn metadata_file(path: &Path, played: i64) {
    metadata_file_seed(path, played, None);
}
fn metadata_file_seed(path: &Path, played: i64, world_seed: Option<i64>) {
    let mut data = fields(vec![
        ("DataVersion", Tag::Int(3218)),
        ("LastPlayed", Tag::Long(played)),
        ("LevelName", Tag::String("Synthetic fixture".into())),
    ]);
    if let Some(seed) = world_seed {
        data.insert(
            "WorldGenSettings".into(),
            Tag::Compound(fields(vec![("seed", Tag::Long(seed))])),
        );
    }
    let bytes = encode(fields(vec![("Data", Tag::Compound(data))]));
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes).unwrap();
    std::fs::write(path.join("level.dat"), encoder.finish().unwrap()).unwrap();
}
fn terrain_file(path: &Path) {
    let palette = ["air", "grass_block", "grass"].map(|name| {
        let mut value = fields(vec![(
            "Name",
            Tag::String(format!("minecraft:{name}").as_str().into()),
        )]);
        if name == "grass_block" {
            value.insert(
                "Properties".into(),
                Tag::Compound(fields(vec![("snowy", Tag::String("false".into()))])),
            );
        }
        Tag::Compound(value)
    });
    let sections = (-4_i8..=19)
        .map(|y| {
            let mut words = vec![0_u64; 256];
            for cell in 0..4096 {
                let height = i32::from(y) * 16 + (cell / 256) as i32;
                let index = if height == 0 {
                    1
                } else if height == 1 && cell % 4 == 0 && (cell / 16) % 4 == 0 {
                    2
                } else {
                    0
                };
                words[cell / 16] |= index << ((cell % 16) * 4);
            }
            Tag::Compound(fields(vec![
                ("Y", Tag::Byte(y)),
                (
                    "block_states",
                    Tag::Compound(fields(vec![
                        (
                            "palette",
                            Tag::List {
                                kind: 10,
                                values: palette.to_vec(),
                            },
                        ),
                        (
                            "data",
                            Tag::LongArray(words.into_iter().map(|word| word as i64).collect()),
                        ),
                    ])),
                ),
            ]))
        })
        .collect();
    let document = encode(fields(vec![
        ("DataVersion", Tag::Int(3218)),
        ("xPos", Tag::Int(0)),
        ("zPos", Tag::Int(0)),
        ("Status", Tag::String("full".into())),
        (
            "sections",
            Tag::List {
                kind: 10,
                values: sections,
            },
        ),
    ]));
    let length = document.len() + 1;
    let sectors = (length + 4).div_ceil(4096);
    assert!(sectors <= 255);
    let mut bytes = vec![0; 8192 + sectors * 4096];
    bytes[..4].copy_from_slice(&((2_u32 << 8) | sectors as u32).to_be_bytes());
    bytes[8192..8196].copy_from_slice(&(length as u32).to_be_bytes());
    bytes[8196] = 3;
    bytes[8197..8197 + document.len()].copy_from_slice(&document);
    std::fs::write(path.join("region/r.0.0.mca"), bytes).unwrap();
}

#[test]
fn invalid_paths_generation_limits_and_initial_cancel_fail_before_storage_work() {
    let (_temporary, root, storage) = fixture();
    assert!(matches!(
        prepare(&root, &storage, 0, limits(), &|| false),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        prepare(Path::new("relative"), &storage, 1, limits(), &|| false),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        prepare(&root, Path::new("relative"), 1, limits(), &|| false),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        prepare(&root, &root.join("history"), 1, limits(), &|| false),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        prepare(
            &root,
            &storage,
            1,
            pipeline::Limits {
                maps: 0,
                ..limits()
            },
            &|| false
        ),
        Err(Error::Pipeline(pipeline::Error::Limits))
    ));
    assert!(matches!(
        prepare(&root, &storage, 1, limits(), &|| true),
        Err(Error::Cancelled)
    ));
    assert!(!storage.exists());
}
#[test]
fn missing_root_preserves_previous_binding_and_history_instead_of_retiring_it() {
    let (temporary, root, storage) = fixture();
    let path = save(&root, "one", 1, false);
    let repository = Repository::new(storage.clone()).unwrap();
    let initial = repository
        .bind_catalog(0, &root, &[path], &|| false)
        .unwrap();
    std::fs::rename(&root, temporary.path().join("retained-offline-saves")).unwrap();
    let result = prepare(&root, &storage, 4, limits(), &|| false).unwrap();
    assert_eq!(result.availability, Availability::MissingRoot);
    assert_eq!(result.snapshot, initial.snapshot);
    assert!(result.maps.is_empty());
    assert!(!result.catalog_complete);
    assert_eq!(repository.load(&|| false).unwrap(), initial.snapshot);
}
#[test]
fn raw_directories_are_bound_before_metadata_and_corrupt_maps_remain_reported() {
    let (_temporary, root, storage) = fixture();
    let bad = save(&root, "bad", 1, false);
    std::fs::write(bad.join("level.dat"), b"synthetic corrupt metadata").unwrap();
    let unrelated = root.join("directory without metadata");
    std::fs::create_dir(&unrelated).unwrap();
    std::fs::write(root.join("not a directory"), b"synthetic regular file").unwrap();
    let first = prepare(&root, &storage, 1, limits(), &|| false).unwrap();
    assert_eq!(first.availability, Availability::NoMetadata);
    assert_eq!(first.bindings.len(), 2);
    assert_eq!(first.metadata.rejected_maps, 1);
    assert_eq!(first.metadata.issues[0].directory, bad);
    let identifier = first
        .bindings
        .iter()
        .find(|binding| binding.directory == bad)
        .unwrap()
        .map;
    metadata_file(&bad, 2);
    let again = prepare(&root, &storage, 2, limits(), &|| false).unwrap();
    assert_eq!(again.availability, Availability::NoQualifiedWindows);
    assert_eq!(
        again
            .bindings
            .iter()
            .find(|binding| binding.directory == bad)
            .unwrap()
            .map,
        identifier
    );
    assert_eq!(again.snapshot.revision(), first.snapshot.revision());
    assert_eq!(again.reports.len(), 1);
    assert_eq!(again.snapshot.history(), History::default());
}
#[test]
fn genuine_synthetic_saved_chunk_is_prepared_without_cloning_or_world_writes() {
    let (_temporary, root, storage) = fixture();
    let path = save(&root, "real synthetic chunk", 100, true);
    let metadata = std::fs::read(path.join("level.dat")).unwrap();
    let terrain = std::fs::read(path.join("region/r.0.0.mca")).unwrap();
    let result = prepare(&root, &storage, 7, limits(), &|| false).unwrap();
    assert_eq!(result.availability, Availability::Ready);
    assert_eq!(result.maps.len(), 1);
    assert_eq!(
        result.maps[0].source(),
        evidence::Source {
            map: result.bindings[0].map,
            generation: 7
        }
    );
    assert_eq!(Arc::strong_count(&result.maps[0]), 1);
    let chunk = &result.maps[0].loaded().chunks[&[0, 0]];
    assert_eq!(Arc::strong_count(chunk), 1);
    assert!(!result.maps[0].targets().is_empty());
    assert!(result.maps[0]
        .targets()
        .iter()
        .all(|target| target.source == result.maps[0].source()));
    assert_eq!(
        result.snapshot.history(),
        History::default(),
        "preparation never earns appearance credit"
    );
    assert_eq!(std::fs::read(path.join("level.dat")).unwrap(), metadata);
    assert_eq!(
        std::fs::read(path.join("region/r.0.0.mca")).unwrap(),
        terrain
    );
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 2);
}
#[test]
fn catalog_reorder_and_generation_rebuild_preserve_directory_ids() {
    let (_temporary, root, storage) = fixture();
    let old = save(&root, "old", 1, true);
    let newer = save(&root, "newer", 2, true);
    let first = prepare(&root, &storage, 1, limits(), &|| false).unwrap();
    assert_eq!(first.maps.len(), 2);
    let identities: BTreeMap<_, _> = first
        .bindings
        .iter()
        .map(|binding| (binding.directory.clone(), binding.map))
        .collect();
    metadata_file(&old, 3);
    let second = prepare(&root, &storage, 2, limits(), &|| false).unwrap();
    assert_eq!(second.metadata.maps[0].directory, old);
    assert_eq!(second.maps[0].source().map, identities[&old]);
    assert_eq!(second.maps[1].source().map, identities[&newer]);
    assert!(second.maps.iter().all(|map| map.source().generation == 2));
    assert_eq!(first.snapshot, second.snapshot);
}
#[test]
fn excessive_or_partially_failed_inventory_never_publishes_bindings() {
    let (_temporary, root, storage) = fixture();
    for index in 0..=history_store::MAX_ADMISSION {
        std::fs::create_dir(root.join(format!("directory-{index}"))).unwrap();
    }
    assert!(matches!(
        prepare(&root, &storage, 1, limits(), &|| false),
        Err(Error::Limit(_))
    ));
    assert!(!storage.join("history.json").exists());
    let handle = NoFollowDirectory::open_root(&root).unwrap();
    let generation = secure_fs::directory_generation(&root).unwrap();
    let partial = std::fs::read_dir(&root)
        .unwrap()
        .take(1)
        .chain(std::iter::once(Err(io::Error::other(
            "synthetic partial read_dir failure",
        ))));
    assert!(matches!(
        collect_inventory(&root, &handle, generation, partial, &|| false),
        Err(Error::Io(_))
    ));
}
#[test]
fn replaced_source_after_binding_fails_the_entire_prepared_output() {
    let (temporary, root, storage) = fixture();
    let path = save(&root, "moving", 1, true);
    let replaced = Cell::new(false);
    let callback = || {
        if storage.join("history.json").exists() && !replaced.replace(true) {
            std::fs::rename(&path, temporary.path().join("original-retained")).unwrap();
            std::fs::create_dir(&path).unwrap();
        }
        false
    };
    assert!(matches!(
        prepare(&root, &storage, 1, limits(), &callback),
        Err(Error::Changed { .. })
    ));
    assert!(replaced.get());
    let result = prepare(&root, &storage, 2, limits(), &|| false).unwrap();
    assert_eq!(result.snapshot.bindings().len(), 2);
    assert_eq!(result.snapshot.history(), History::default());
}
#[test]
fn corrupt_persistence_remains_an_error_even_when_saves_root_is_missing() {
    let (temporary, _root, storage) = fixture();
    let repository = Repository::new(storage.clone()).unwrap();
    repository.load(&|| false).unwrap();
    std::fs::write(storage.join("history.json"), b"synthetic corrupt state").unwrap();
    assert!(matches!(
        prepare(
            &temporary.path().join("absent-root"),
            &storage,
            1,
            limits(),
            &|| false
        ),
        Err(Error::Persistence(_))
    ));
    assert_eq!(
        std::fs::read(storage.join("history.json")).unwrap(),
        b"synthetic corrupt state"
    );
}
#[test]
fn cancelled_after_full_binding_returns_no_maps_or_appearance_credit() {
    let (_temporary, root, storage) = fixture();
    save(&root, "one", 1, true);
    assert!(matches!(
        prepare(&root, &storage, 1, limits(), &|| storage
            .join("history.json")
            .exists()),
        Err(Error::Cancelled)
    ));
    let saved = Repository::new(storage).unwrap().load(&|| false).unwrap();
    assert_eq!(saved.bindings().len(), 1);
    assert_eq!(saved.history(), History::default());
}
#[test]
fn quarter_block_route_midpoints_are_map_specific_checked_and_deduplicated() {
    let first = MapId([1; 16]);
    let second = MapId([2; 16]);
    let mut value = serde_json::to_value(History::default()).unwrap();
    value["completed"] = serde_json::json!(1);
    for (index, (map, endpoints)) in [
        (first, [[-256, -128], [0, 0]]),
        (first, [[-256, -128], [0, 0]]),
        (second, [[512, 512], [1024, 1024]]),
    ]
    .into_iter()
    .enumerate()
    {
        value["routes"][index] =
            serde_json::json!({"route":{"map":map,"endpoints":endpoints},"run":1});
    }
    let history = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(recent_centers(history, first).unwrap(), vec![[-2, -1]]);
    assert_eq!(recent_centers(history, second).unwrap(), vec![[12, 12]]);
    value["routes"][0]["route"]["endpoints"] =
        serde_json::json!([[i64::MAX, i64::MAX], [i64::MAX, i64::MAX]]);
    assert!(matches!(
        recent_centers(serde_json::from_value(value).unwrap(), first),
        Err(Error::Invalid(_))
    ));
}

fn completed_route(map: MapId) -> History {
    let mut value = serde_json::to_value(History::default()).unwrap();
    value["completed"] = serde_json::json!(1);
    value["routes"][0] =
        serde_json::json!({"route":{"map":map,"endpoints":[[-96,0],[96,0]]},"run":1});
    serde_json::from_value(value).unwrap()
}

#[test]
fn retained_route_center_reaches_actual_window_exclusion_for_only_its_map() {
    let (_temporary, root, storage) = fixture();
    let visited = save(&root, "visited", 2, true);
    let unvisited = save(&root, "unvisited", 1, true);
    let first = prepare(&root, &storage, 1, limits(), &|| false).unwrap();
    let visited_id = first
        .bindings
        .iter()
        .find(|binding| binding.directory == visited)
        .unwrap()
        .map;
    let unvisited_id = first
        .bindings
        .iter()
        .find(|binding| binding.directory == unvisited)
        .unwrap()
        .map;
    let history = completed_route(visited_id);
    Repository::new(storage.clone())
        .unwrap()
        .commit_history(first.snapshot.revision(), history, &|| false)
        .unwrap();
    let second = prepare(&root, &storage, 2, limits(), &|| false).unwrap();
    assert_eq!(second.availability, Availability::Ready);
    assert_eq!(second.maps.len(), 1);
    assert_eq!(second.maps[0].source().map, unvisited_id);
    assert_eq!(second.snapshot.history(), history);
    assert_eq!(second.reports.len(), 2);
    assert_eq!(
        second
            .reports
            .iter()
            .find(|report| report.directory == visited)
            .unwrap()
            .header_candidates,
        0
    );
}

#[test]
fn concurrent_history_revision_prevents_stale_controller_snapshot_delivery() {
    let (_temporary, root, storage) = fixture();
    save(&root, "one", 1, true);
    let committed = Cell::new(false);
    let callback = || {
        if storage.join("history.json").exists() && !committed.replace(true) {
            let repository = Repository::new(storage.clone()).unwrap();
            let snapshot = repository.load(&|| false).unwrap();
            repository
                .commit_history(
                    snapshot.revision(),
                    completed_route(snapshot.bindings()[0].map),
                    &|| false,
                )
                .unwrap();
        }
        false
    };
    match prepare(&root, &storage, 1, limits(), &callback) {
        Err(Error::Persistence(history_store::Error::Conflict { current })) => {
            assert_eq!(current.revision(), 2);
            assert_eq!(current.history().completed(), 1);
        }
        other => panic!("expected current-snapshot conflict, got {other:?}"),
    }
    assert!(committed.get());
}

#[test]
fn empty_existing_root_is_a_complete_empty_inventory_and_only_ignores_files() {
    let (_temporary, root, storage) = fixture();
    std::fs::write(root.join("synthetic unrelated file"), b"preserve this").unwrap();
    let result = prepare(&root, &storage, 1, limits(), &|| false).unwrap();
    assert_eq!(result.availability, Availability::EmptyRoot);
    assert!(result.inventory_complete);
    assert!(result.catalog_complete);
    assert!(result.bindings.is_empty());
    assert_eq!(result.snapshot.revision(), 0);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn entry_cap_counts_ignored_files_and_aborts_before_repository_creation() {
    let (_temporary, root, storage) = fixture();
    for index in 0..=MAX_DIRECTORY_ENTRIES {
        std::fs::write(root.join(format!("synthetic-file-{index}")), b"").unwrap();
    }
    assert!(matches!(
        prepare(&root, &storage, 1, limits(), &|| false),
        Err(Error::Limit("saves root directory entries"))
    ));
    assert!(!storage.exists());
}

#[test]
fn valid_metadata_without_region_is_reported_per_map_and_preserves_qualified_peer() {
    let (_temporary, root, storage) = fixture();
    let fresh = save(&root, "fresh unexplored world", 2, false);
    std::fs::remove_dir(fresh.join("region")).unwrap();
    let qualified = save(&root, "qualified peer", 1, true);
    let result = prepare(&root, &storage, 1, limits(), &|| false).unwrap();
    assert_eq!(result.availability, Availability::Ready);
    assert_eq!(result.bindings.len(), 2);
    assert_eq!(result.metadata.maps.len(), 2);
    assert_eq!(result.maps.len(), 1);
    let qualified_id = result
        .bindings
        .iter()
        .find(|binding| binding.directory == qualified)
        .unwrap()
        .map;
    assert_eq!(result.maps[0].source().map, qualified_id);
    assert_eq!(result.reports.len(), 2);
    assert!(result
        .reports
        .iter()
        .find(|report| report.directory == fresh)
        .unwrap()
        .error
        .is_some());
    assert!(
        !fresh.join("region").exists(),
        "service never creates unexplored terrain"
    );
}

#[test]
fn only_unexplored_world_is_no_qualified_windows_not_missing_metadata() {
    let (_temporary, root, storage) = fixture();
    let fresh = save(&root, "fresh", 1, false);
    std::fs::remove_dir(fresh.join("region")).unwrap();
    let result = prepare(&root, &storage, 1, limits(), &|| false).unwrap();
    assert_eq!(result.availability, Availability::NoQualifiedWindows);
    assert_eq!(result.metadata.maps.len(), 1);
    assert_eq!(result.reports.len(), 1);
    assert!(result.reports[0].error.is_some());
    assert!(result.maps.is_empty());
    assert_eq!(result.snapshot.history(), History::default());
}

#[test]
fn existing_region_file_is_rejected_without_world_writes_or_history_credit() {
    let (_temporary, root, storage) = fixture();
    let invalid = save(&root, "invalid region shape", 2, false);
    std::fs::remove_dir(invalid.join("region")).unwrap();
    std::fs::write(invalid.join("region"), b"preserve synthetic region file").unwrap();
    save(&root, "qualified peer", 1, true);
    assert!(matches!(
        prepare(&root, &storage, 1, limits(), &|| false),
        Err(Error::Changed { directory, .. }) if directory == invalid
    ));
    assert_eq!(
        std::fs::read(invalid.join("region")).unwrap(),
        b"preserve synthetic region file"
    );
    let snapshot = Repository::new(storage).unwrap().load(&|| false).unwrap();
    assert_eq!(snapshot.bindings().len(), 2);
    assert_eq!(snapshot.history(), History::default());
}

#[test]
fn canonical_seed_remains_bound_to_prepared_map_identity_without_world_writes() {
    for seed in [Some(i64::MIN + 1), Some(0), None] {
        let (_temporary, root, storage) = fixture();
        let path = save(&root, "synthetic seeded world", 1, true);
        metadata_file_seed(&path, 1, seed);
        let before = std::fs::read(path.join("level.dat")).unwrap();
        let prepared = prepare(&root, &storage, 7, limits(), &|| false).unwrap();
        assert_eq!(prepared.maps.len(), 1);
        let identity = prepared.maps[0].source().map;
        assert_eq!(prepared.world_seeds.len(), 1);
        assert_eq!(prepared.world_seeds.get(&identity), Some(&seed));
        assert_eq!(prepared.metadata.maps[0].metadata.world_seed, seed);
        assert_eq!(std::fs::read(path.join("level.dat")).unwrap(), before);
    }
}

#[test]
fn projected_source_reaches_chunk_qualification_with_the_same_bound_root_spelling() {
    use super::super::{coverage::Line, projected_source, source_footprint};
    use crate::voxel_landscape::assets::budget::{ByteBudget, Cancel};
    use std::sync::atomic::AtomicBool;

    let (_temporary, root, storage) = fixture();
    let path = save(&root, "projected source root", 100, true);
    let metadata = std::fs::read(path.join("level.dat")).unwrap();
    let terrain = std::fs::read(path.join("region/r.0.0.mca")).unwrap();
    let prepared = prepare(&root, &storage, 7, limits(), &|| false).unwrap();
    assert_eq!(prepared.maps.len(), 1);
    let base = &prepared.maps[0];
    let bound = &prepared.bindings[0];
    bound.verify(&root).unwrap();
    let account = ByteBudget::new(1 << 30).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let request = source_footprint::request(
        Line {
            start: [8.0, 8.0],
            end: [9.0, 8.0],
        },
        63.0,
        [1, 1],
        1024.0,
        &account,
        cancel,
    )
    .unwrap();
    assert!(request.support_chunks().contains(&[0, 0]));
    assert!(request.support_chunks().len() > 1);
    // The fixture intentionally has only one chunk. A matching bound root
    // must reach the ordinary missing-chunk refusal, not fail path identity.
    assert!(matches!(
        projected_source::qualify(base, bound, &root, &request, &account, cancel, &|| false),
        Err(projected_source::Error::Unqualified { missing, .. }) if missing > 0
    ));
    assert_eq!(std::fs::read(path.join("level.dat")).unwrap(), metadata);
    assert_eq!(
        std::fs::read(path.join("region/r.0.0.mca")).unwrap(),
        terrain
    );
    assert_eq!(prepared.snapshot.history(), History::default());
}
