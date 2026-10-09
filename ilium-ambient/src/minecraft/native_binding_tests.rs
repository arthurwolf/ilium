use super::*;
use crate::voxel_landscape::assets::{
    animation::MissingAnimation,
    identity::OriginKind,
    review::fixture_origin,
    texture::{fixture_texture, Encoding},
};
use std::{collections::BTreeMap, time::Duration};

fn blue_head() -> native_builtin::Recipe {
    native_builtin::recipe(&super::super::chunk::BlockState {
        name: "minecraft:blue_bed".into(),
        properties: BTreeMap::from([
            ("facing".into(), "south".into()),
            ("part".into(), "head".into()),
            ("occupied".into(), "false".into()),
        ]),
    })
    .unwrap()
    .unwrap()
}

#[test]
fn selected_hd_builtin_atlas_keeps_logical_normalized_uv_layout() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let pixels = [40, 90, 180, 255].repeat(128 * 128);
    let texture = fixture_texture(
        [128, 128],
        &pixels,
        None,
        &MissingAnimation::StaticImage,
        Encoding::SrgbColor,
        fixture_origin(OriginKind::SelectedPack),
        &budget,
    );
    let recipe = blue_head();
    assert_eq!(recipe.atlas_size, [64, 64]);
    assert!(native_builtin::bake_faces(&recipe)
        .iter()
        .flat_map(|face| face.uv)
        .flatten()
        .all(|coordinate| (0.0..=1.0).contains(&coordinate)));
    assert!(texture.sample_color([0.75, 0.25], Duration::ZERO).is_some());
}

#[test]
fn source_authored_atlas_strip_uses_each_square_frame_not_whole_image() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let mut pixels = vec![0; 64 * 128 * 4];
    for y in 0..128 {
        for x in 0..64 {
            let color = if y < 64 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            };
            pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4].copy_from_slice(&color);
        }
    }
    let texture = fixture_texture(
        [64, 128],
        &pixels,
        Some(r#"{"animation":{"width":64,"height":64,"frametime":1}}"#),
        &MissingAnimation::StaticImage,
        Encoding::SrgbColor,
        fixture_origin(OriginKind::SelectedPack),
        &budget,
    );
    assert_eq!(blue_head().atlas_size, [64, 64]);
    assert_eq!(texture.animation().frame_count(), 2);
    let first = texture.sample_color([0.5, 0.5], Duration::ZERO).unwrap();
    let second = texture
        .sample_color([0.5, 0.5], Duration::from_millis(50))
        .unwrap();
    assert_eq!(first.straight(), [1.0, 0.0, 0.0]);
    assert_eq!(second.straight(), [0.0, 0.0, 1.0]);
}

#[test]
fn rectangular_selected_atlas_remains_an_authored_sampler_input() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let pixels = [200, 40, 10, 255].repeat(128 * 64);
    let texture = fixture_texture(
        [128, 64],
        &pixels,
        None,
        &MissingAnimation::StaticImage,
        Encoding::SrgbColor,
        fixture_origin(OriginKind::SelectedPack),
        &budget,
    );
    assert_eq!(blue_head().atlas_size, [64, 64]);
    assert!(texture.sample_color([0.5, 0.5], Duration::ZERO).is_some());
}

#[test]
fn cross_tile_native_models_share_geometry_but_preserve_exact_face_owners() {
    use crate::minecraft::{loader, render_cells, saved_binding, surface, tours};
    use crate::voxel_landscape::assets::pixels::fixture_png;
    use std::sync::atomic::AtomicBool;
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(256 << 20).unwrap();
    let png = fixture_png(1, 1, &[80, 90, 100, 255]);
    let state = br#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#;
    let model = br##"{"textures":{"all":"minecraft:block/stone"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"down":{"texture":"#all"},"up":{"texture":"#all"},"north":{"texture":"#all"},"south":{"texture":"#all"},"west":{"texture":"#all"},"east":{"texture":"#all"}}}]}"##;
    let sources = native_assets::tests::fixture(
        &[("assets/minecraft/textures/block/dirt.png", png.as_slice())],
        &[
            ("assets/minecraft/blockstates/stone.json", state.as_slice()),
            ("assets/minecraft/models/block/stone.json", model.as_slice()),
            ("assets/minecraft/textures/block/stone.png", png.as_slice()),
        ],
        &budget,
        cancel,
    );
    let session = NativeSourceSession {
        sources,
        layers: RenderLayers::load(true).unwrap(),
        budget: budget.clone(),
        _ids_charge: budget.reserve(8 << 20, cancel).unwrap(),
    };
    let expected_source = session.sources.provenance().clone();
    let seed_cells = saved_binding::tests::cells("minecraft:stone", &[]);
    let template = seed_cells.map.loaded().chunks.values().next().unwrap();
    let mut loaded = loader::LoadedWindow::default();
    for position in [[-1, 2], [0, 2]] {
        let mut decoded = (**template).clone();
        decoded.identity.position = position;
        loaded.chunks.insert(position, Arc::new(decoded));
        loaded.coverage.chunks.insert(position);
    }
    let map = Arc::new(
        PreparedMap::new(
            seed_cells.map.source(),
            0,
            Arc::new(loaded),
            Vec::new(),
            &mut tours::Budget::new(u64::MAX, &|| false),
        )
        .unwrap(),
    );
    let tile_cells = |x: i32| render_cells::RenderCells {
        map: Arc::clone(&map),
        core: surface::Bounds {
            minimum: [x.div_euclid(16) * 16, 32],
            maximum: [x.div_euclid(16) * 16 + 15, 47],
        },
        heights: [319, 319],
        positions: vec![[x, 319, 32]],
        work_used: 0,
        storage_charge: 0,
    };
    let binding = |x| {
        saved_binding::prepare_accounted(
            &tile_cells(x),
            saved_binding::Limits::default(),
            &budget,
            cancel,
            &|| false,
        )
        .unwrap()
    };
    let definitions = session.definitions().unwrap();
    let mut compiler = ModelCompiler::new(&definitions, session.limits(), budget.clone()).unwrap();
    let mut ids = BTreeSet::new();
    session
        .collect_required(&binding(-1), 7, &mut ids, &mut compiler, cancel)
        .unwrap();
    session
        .collect_required(&binding(0), 7, &mut ids, &mut compiler, cancel)
        .unwrap();
    drop(compiler);
    let shared = session.import(ids, cancel).unwrap();
    let definitions = shared.definitions().unwrap();
    let mut compiler = ModelCompiler::new(&definitions, shared.limits(), budget.clone()).unwrap();
    let first = prepare_native_shared(binding(-1), &shared, &mut compiler, 7, 0, cancel)
        .unwrap()
        .into_tile();
    let second = prepare_native_shared(binding(0), &shared, &mut compiler, 7, 0, cancel)
        .unwrap()
        .into_tile();
    assert_eq!(first.mesh.faces.len(), 6);
    assert_eq!(second.mesh.faces.len(), 6);
    for tile in [&first, &second] {
        assert_eq!(tile.source_profile, expected_source.profile);
        assert_eq!(
            tile.native_archive_sha256,
            expected_source.native_archive_sha256
        );
        assert_eq!(
            tile.selected_archive_sha256,
            expected_source.selected_archive_sha256
        );
    }
    assert!(first
        .mesh
        .faces
        .iter()
        .all(|face| face.owner.position == [-1, 32, 319]));
    assert!(second
        .mesh
        .faces
        .iter()
        .all(|face| face.owner.position == [0, 32, 319]));
    let first_model = &first.mesh.faces[0].model;
    let second_model = &second.mesh.faces[0].model;
    assert_eq!(first_model.bank, second_model.bank);
    assert_eq!(first_model.state, second_model.state);
    assert_eq!(first_model.quads.len(), second_model.quads.len());
    assert!(
        Arc::ptr_eq(first_model, second_model),
        "adjacent saved tiles retained duplicate identical bound models and reservations"
    );
    let key = {
        let reuse = shared.model_reuse.lock().unwrap();
        assert_eq!(reuse.entries.len(), 1);
        let key = reuse.entries.keys().next().unwrap().clone();
        assert!(Arc::ptr_eq(&reuse.get(&key).unwrap(), first_model));
        let mut changed = key.clone();
        changed.0.origin.push(1);
        assert!(reuse.get(&changed).is_none());
        changed = key.clone();
        changed.0.choices[0].origins.push(1);
        assert!(reuse.get(&changed).is_none());
        changed = key.clone();
        changed.1.push((0, [1, 2, 3]));
        assert!(reuse.get(&changed).is_none());
        assert!(ModelReuse::default().get(&key).is_none());
        key
    };
    let mut different = key.clone();
    different.0.origin.push(2);
    let tiny = ByteBudget::new(1).unwrap();
    shared
        .model_reuse
        .lock()
        .unwrap()
        .remember(&different, first_model, &tiny, cancel)
        .unwrap();
    assert_eq!(tiny.used(), 0);
    assert!(shared.model_reuse.lock().unwrap().get(&different).is_none());
    let cancelled = AtomicBool::new(true);
    assert!(matches!(
        shared.model_reuse.lock().unwrap().remember(
            &different,
            first_model,
            &budget,
            Cancel::new(&cancelled)
        ),
        Err(AssetError::Cancelled)
    ));
    let weak = Arc::downgrade(first_model);
    drop(first);
    drop(second);
    assert!(weak.upgrade().is_none());
    let before_prune = budget.used();
    shared.model_reuse.lock().unwrap().prune();
    assert!(shared.model_reuse.lock().unwrap().entries.is_empty());
    assert!(budget.used() < before_prune);
}
