//! Synthetic source-routing controls, not proof that the pinned game JAR was read.
use super::*;
use crate::voxel_landscape::assets::{
    animation::{AnimationPlan, PixelRect},
    archive::synthetic_zip,
    block_state::BlockState,
    layers::ResourceKind,
    models::{DefinitionProvider, ModelCompiler},
    review::fixture_review,
};
use std::{sync::atomic::AtomicBool, time::Duration};

fn goodvibes_fixture(
    selected: &[(&str, &[u8])],
    native: &[(&str, &[u8])],
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> NativeSources {
    let mut sources = fixture(
        &[("assets/minecraft/textures/block/stone.png", b"placeholder")],
        native,
        budget,
        cancel,
    );
    let zip = synthetic_zip(selected, false, false);
    let bytes = SourceBytes::from_slice(&zip, 1 << 20, budget, cancel).unwrap();
    let source =
        Arc::new(ZipSource::open(bytes, None, SourceLimits::default(), budget, cancel).unwrap());
    let mut review = fixture_review();
    review.pack = ResourceId::parse("ilium-pack:goodvibes").unwrap();
    review.edition = SourceEdition::ExtractedWorldArt;
    sources.packs[0] = LayeredPack::mount(
        review,
        source,
        None,
        MountLayout::ExtractedJava,
        OriginKind::SelectedPack,
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap();
    sources
}

#[test]
fn selected_goodvibes_water_requests_use_exact_aliases_and_animation_pins() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(64 << 20).unwrap();
    let sources = goodvibes_fixture(
        &[
            ("textures/block/water_still.png", b"still"),
            ("textures/block/water_flow.png", b"flow"),
        ],
        &[("assets/minecraft/textures/block/water_still.png", b"native")],
        &budget,
        cancel,
    );

    for (name, digest, image, frame, count, ticks) in [
        (
            "water_still",
            "f35e3a02b81bb359bf3eced106c12324f5523d9fb85e13599887205c0e513247",
            [512, 16384],
            [512, 512],
            32_usize,
            2_u32,
        ),
        (
            "water_flow",
            "75994f61cfd8a4e56480010e91b1df098ab71c70ba373d858effe9d3a2613f66",
            [1021, 16384],
            [1021, 1024],
            16_usize,
            1_u32,
        ),
    ] {
        let id = ResourceId::parse(&format!("minecraft:block/{name}")).unwrap();
        let requests = sources
            .texture_requests(std::slice::from_ref(&id), cancel)
            .unwrap();
        let candidates = &requests.as_slice()[0].candidates;
        assert_eq!(candidates.len(), 3);
        assert!(matches!(
            &candidates[0].location,
            TextureLocation::Resource { id: found, .. } if found == &id
        ));
        assert!(matches!(
            &candidates[1].location,
            TextureLocation::Literal { path, .. }
                if path.as_str() == format!("textures/block/{name}.png")
        ));
        for candidate in &candidates[..2] {
            assert_eq!(
                candidate.expected_source_sha256,
                Some(Digest256::try_from(digest.to_owned()).unwrap())
            );
            assert_eq!(candidate.expected_image.dimensions, Some(image));
            let ScheduleSource::AutomaticJavaWithMissing(missing) = &candidate.schedule else {
                panic!("GoodVibes water candidate must have pinned missing-metadata playback");
            };
            let plan =
                AnimationPlan::build(image, None, missing, &Limits::default(), &budget, cancel)
                    .unwrap();
            assert_eq!(plan.frame_count(), count);
            assert!(!plan.authored_schedule());
            for index in 0..count {
                let sample = plan.at(Duration::from_millis(u64::from(index as u32 * ticks) * 50));
                assert_eq!(
                    sample.current,
                    PixelRect {
                        x: 0,
                        y: index as u32 * frame[1],
                        width: frame[0],
                        height: frame[1],
                    }
                );
                assert_eq!(sample.blend, 0.0);
            }
            let cycle_ms = count as u64 * u64::from(ticks) * 50;
            assert_eq!(plan.at(Duration::from_millis(cycle_ms)).current.y, 0);
        }
        assert!(matches!(
            candidates[2].schedule,
            ScheduleSource::AutomaticJava
        ));
        assert!(candidates[2].expected_source_sha256.is_none());
    }

    let stone = ResourceId::parse("minecraft:block/stone").unwrap();
    let requests = sources
        .texture_requests(std::slice::from_ref(&stone), cancel)
        .unwrap();
    let stone_candidates = &requests.as_slice()[0].candidates;
    assert_eq!(stone_candidates.len(), 3);
    assert!(matches!(
        &stone_candidates[1].location,
        TextureLocation::Literal { path, .. }
            if path.as_str() == "textures/block/stone.png"
    ));

    let farmland = ResourceId::parse("minecraft:block/farmland").unwrap();
    let requests = sources
        .texture_requests(std::slice::from_ref(&farmland), cancel)
        .unwrap();
    let farmland_candidates = &requests.as_slice()[0].candidates;
    assert_eq!(farmland_candidates.len(), 4);
    assert!(matches!(
        &farmland_candidates[1].location,
        TextureLocation::Literal { path, .. }
            if path.as_str() == "textures/block/farmland.png"
    ));
    assert!(matches!(
        &farmland_candidates[2].location,
        TextureLocation::Literal { path, .. }
            if path.as_str() == "textures/block/farmland_moist.png"
    ));
}

fn mount(
    entries: &[(&str, &[u8])],
    id: &str,
    role: OriginKind,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> LayeredPack {
    let zip = synthetic_zip(entries, false, false);
    let bytes = SourceBytes::from_slice(&zip, 1 << 20, budget, cancel).unwrap();
    let source =
        Arc::new(ZipSource::open(bytes, None, SourceLimits::default(), budget, cancel).unwrap());
    let mut review = fixture_review();
    review.pack = ResourceId::parse(id).unwrap();
    LayeredPack::mount(
        review,
        source,
        None,
        MountLayout::Java,
        role,
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap()
}
pub(crate) fn fixture(
    selected: &[(&str, &[u8])],
    native: &[(&str, &[u8])],
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> NativeSources {
    NativeSources {
        packs: vec![
            mount(
                selected,
                "fixture:selected",
                OriginKind::SelectedPack,
                budget,
                cancel,
            ),
            mount(
                native,
                "fixture:native",
                OriginKind::ExplicitFullPackFallback,
                budget,
                cancel,
            ),
        ],
        native_index: 1,
        provenance: Provenance {
            profile: "synthetic source-routing fixture",
            native_archive_sha256: Digest256::of(b"not native"),
            selected_archive_sha256: None,
            selected_override: true,
        },
        selected_duplicates: Vec::new(),
        immutable_definition_sources: true,
        _duplicates_reservation: budget.reserve(0, cancel).unwrap(),
        limits: Limits::default(),
        budget: budget.clone(),
        _reservation: budget.reserve(32 * 1024, cancel).unwrap(),
    }
}
#[test]
fn source_route_preserves_winner_and_never_aliases_missing_resources() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(32 << 20).unwrap();
    let sources = fixture(
        &[("assets/minecraft/textures/block/dirt.png", b"selected")],
        &[
            ("assets/minecraft/textures/block/dirt.png", b"native"),
            ("assets/minecraft/textures/block/stone.png", b"native stone"),
        ],
        &budget,
        cancel,
    );
    assert!(sources.immutable_definition_sources());
    let key = |name| ResourceKey {
        kind: ResourceKind::Texture,
        id: ResourceId::parse(name).unwrap(),
    };
    let dirt = sources
        .resolve(&key("minecraft:block/dirt"), cancel)
        .unwrap()
        .unwrap();
    assert_eq!(dirt.bytes(), b"selected");
    assert_eq!(dirt.origin().kind, OriginKind::SelectedPack);
    assert_eq!(dirt.digest(), Digest256::of(b"selected"));
    let stone = sources
        .resolve(&key("minecraft:block/stone"), cancel)
        .unwrap()
        .unwrap();
    assert_eq!(stone.bytes(), b"native stone");
    assert_eq!(stone.origin().kind, OriginKind::ExplicitFullPackFallback);
    assert!(sources
        .resolve(&key("minecraft:block/mud"), cancel)
        .unwrap()
        .is_none());
    drop(dirt);
    drop(stone);
    drop(sources);
    assert_eq!(budget.used(), 0);
}
#[test]
fn native_parent_and_selected_child_compile_without_compatibility_geometry() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(64 << 20).unwrap();
    let sources = fixture(
        &[("assets/minecraft/blockstates/stone.json", br#"{"variants":{"":{"model":"minecraft:block/child"}}}"#),
          ("assets/minecraft/models/block/child.json", br##"{"parent":"minecraft:block/base","textures":{"top":"minecraft:block/selected"}}"##)],
        &[("assets/minecraft/models/block/base.json", br##"{"textures":{"top":"minecraft:block/native"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#top"}}}]}"##)],
        &budget, cancel,
    );
    let definitions = sources.definitions().unwrap();
    let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
    let state = BlockState::new(ResourceId::parse("minecraft:stone").unwrap(), []).unwrap();
    let compiled = compiler
        .compile_state(&state, [-17, 32, -64], 0, cancel)
        .unwrap();
    assert_eq!(compiled.state, state);
    assert_eq!(compiled.applications.len(), 1);
    let model = &compiled.applications[0].1;
    assert_eq!(model.quads.len(), 1);
    assert_eq!(model.quads[0].texture.as_str(), "minecraft:block/selected");
    assert!(model
        .origins
        .iter()
        .all(|origin| origin.compatibility.is_none()));
}
#[test]
fn malformed_selected_definition_is_not_replaced_by_native_definition() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(16 << 20).unwrap();
    let sources = fixture(
        &[("assets/minecraft/models/block/stone.json", b"{")],
        &[(
            "assets/minecraft/models/block/stone.json",
            br#"{"elements":[]}"#,
        )],
        &budget,
        cancel,
    );
    let key = ResourceKey {
        kind: ResourceKind::Model,
        id: ResourceId::parse("minecraft:block/stone").unwrap(),
    };
    assert!(sources
        .definitions()
        .unwrap()
        .definition(&key, cancel)
        .is_err());
}
#[test]
fn exact_texture_candidates_preserve_identity_and_budget_lifetime() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(16 << 20).unwrap();
    let sources = fixture(&[("selected", b"x")], &[("native", b"y")], &budget, cancel);
    let id = ResourceId::parse("minecraft:block/mud").unwrap();
    let used = budget.used();
    let requests = sources
        .texture_requests(std::slice::from_ref(&id), cancel)
        .unwrap();
    let request = &requests.as_slice()[0];
    assert_eq!(request.requirement.id, id);
    assert_eq!(
        request.requirement.origin,
        RequiredOrigin::SelectedOrExplicitFullPackFallback
    );
    assert_eq!(request.candidates.len(), 2);
    assert_eq!(request.candidates[0].pack.as_str(), "fixture:selected");
    assert_eq!(request.candidates[1].pack.as_str(), "fixture:native");
    for candidate in &request.candidates {
        assert!(
            matches!(&candidate.location, TextureLocation::Resource { id: actual, alias_reason: None } if actual == &id)
        );
    }
    assert!(budget.used() > used);
    drop(requests);
    assert_eq!(budget.used(), used);
    assert!(matches!(
        sources.texture_requests(&[id.clone(), id], cancel),
        Err(AssetError::Duplicate(_))
    ));
    assert_eq!(budget.used(), used);
}
#[test]
fn climate_reads_cannot_be_shadowed_by_selected_art() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(16 << 20).unwrap();
    let path = "data/minecraft/worldgen/biome/plains.json";
    let sources = fixture(
        &[(path, b"selected fake climate")],
        &[(path, b"native climate")],
        &budget,
        cancel,
    );
    assert_eq!(
        sources
            .native_biome(&ResourceId::parse("minecraft:plains").unwrap(), cancel)
            .unwrap()
            .unwrap()
            .bytes(),
        b"native climate"
    );
    assert!(sources
        .native_biome(&ResourceId::parse("mod:plains").unwrap(), cancel)
        .is_err());
}
#[test]
fn lexical_path_selection_preserves_authored_whitespace_and_rejects_bad_paths() {
    let path = std::env::temp_dir().join("installed native 雪.jar ");
    let authored = path.to_str().unwrap();
    assert_eq!(jar_path(authored).unwrap(), path);
    for bad in ["relative.jar", "bad\0.jar"] {
        assert!(jar_path(bad).is_err());
    }
    assert!(jar_path(&"x".repeat(4097)).is_err());
    if let Some(root) = ilium_platform::minecraft::java_directory() {
        assert_eq!(
            jar_path("").unwrap(),
            root.join("versions/1.19.3/1.19.3.jar")
        );
    }
}
#[test]
fn production_entry_rejects_a_fake_jar_and_releases_all_charges() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("1.19.3.jar");
    std::fs::write(&path, synthetic_zip(&[("assets/x", b"fake")], false, false)).unwrap();
    let before = std::fs::read(&path).unwrap();
    let stop = AtomicBool::new(false);
    let budget = ByteBudget::new(16 << 20).unwrap();
    assert!(matches!(
        NativeSources::open(
            &path,
            None,
            Limits::default(),
            budget.clone(),
            Cancel::new(&stop)
        ),
        Err(Error::Asset(AssetError::Integrity { .. }))
    ));
    assert_eq!(budget.used(), 0);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn cancelled_entry_does_not_open_a_source_and_tiny_budget_is_explicit() {
    let stop = AtomicBool::new(true);
    let budget = ByteBudget::new(16 << 20).unwrap();
    assert!(matches!(
        NativeSources::open(
            Path::new("not even absolute"),
            None,
            Limits::default(),
            budget.clone(),
            Cancel::new(&stop)
        ),
        Err(Error::Asset(AssetError::Cancelled))
    ));
    assert_eq!(budget.used(), 0);
    let stop = AtomicBool::new(false);
    let tiny = ByteBudget::new(1).unwrap();
    assert!(matches!(
        NativeSources::open(
            Path::new("not opened"),
            None,
            Limits::default(),
            tiny.clone(),
            Cancel::new(&stop)
        ),
        Err(Error::Asset(AssetError::Limit { .. }))
    ));
    assert_eq!(tiny.used(), 0);
}
