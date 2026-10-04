//! Real decoder/provider calls with synthetic pixels; no native pixel acceptance.
use super::*;
use crate::minecraft::{chunk, evidence::MapId, loader::LoadedWindow, nbt, tours};
use crate::voxel_landscape::assets::{
    budget::ByteBudget, identity::OriginKind, review::fixture_origin,
};
use std::sync::{atomic::AtomicBool, Arc};

fn tint(budget: &ByteBudget, cancel: Cancel<'_>) -> NativeTint {
    fn pixels(budget: &ByteBudget, cancel: Cancel<'_>) -> MapPixels {
        let rgb = (0..MAP_PIXELS)
            .map(|index| [index as u8, (index >> 8) as u8, 17])
            .collect();
        MapPixels {
            rgb,
            evidence: ColormapEvidence {
                origin: fixture_origin(OriginKind::DiagnosticFixture),
                source_sha256: Digest256::of(b"synthetic encoded image"),
                rgba_sha256: Digest256::of(b"synthetic decoded image"),
                dimensions: [256, 256],
            },
            _reservation: budget
                .reserve((MAP_PIXELS * 3 + 8192) as u64, cancel)
                .unwrap(),
        }
    }
    NativeTint {
        registry: Manifest::parse().unwrap(),
        grass: pixels(budget, cancel),
        foliage: pixels(budget, cancel),
        budget: budget.clone(),
        swamp: SwampNoise::new(),
        _reservation: budget.reserve(REGISTRY_CHARGE, cancel).unwrap(),
    }
}
fn fields(values: Vec<(&str, nbt::Tag)>) -> nbt::Compound {
    values
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
}
fn string(value: &str) -> nbt::Tag {
    nbt::Tag::String(value.into())
}
fn list(kind: u8, values: Vec<nbt::Tag>) -> nbt::Tag {
    nbt::Tag::List { kind, values }
}
fn map(version: i32, biome: Option<&str>, two_heights: bool) -> Arc<PreparedMap> {
    use nbt::Tag;
    let sections = (-4..=19)
        .map(|y| {
            let mut section = fields(vec![
                ("Y", Tag::Byte(y)),
                (
                    "block_states",
                    Tag::Compound(fields(vec![(
                        "palette",
                        list(
                            10,
                            vec![Tag::Compound(fields(vec![(
                                "Name",
                                string("minecraft:stone"),
                            )]))],
                        ),
                    )])),
                ),
            ]);
            if let Some(biome) = biome {
                let mut values = fields(vec![("palette", list(8, vec![string(biome)]))]);
                if two_heights && y == 4 {
                    values.insert(
                        "palette".into(),
                        list(8, vec![string(biome), string("minecraft:desert")]),
                    );
                    values.insert(
                        "data".into(),
                        Tag::LongArray(vec![0xffff_ffff_ffff_0000_u64 as i64]),
                    );
                }
                section.insert("biomes".into(), Tag::Compound(values));
            }
            Tag::Compound(section)
        })
        .collect();
    let body = fields(vec![
        ("xPos", Tag::Int(-1)),
        ("zPos", Tag::Int(-2)),
        ("Status", string("full")),
        (
            if version <= 2836 {
                "Sections"
            } else {
                "sections"
            },
            list(10, sections),
        ),
    ]);
    let mut root = if version <= 2836 {
        fields(vec![("Level", Tag::Compound(body))])
    } else {
        body
    };
    root.insert("DataVersion".into(), Tag::Int(version));
    let chunk = chunk::decode(
        &nbt::Document {
            name: "synthetic tint fixture".into(),
            root,
        },
        [-1, -2],
        chunk::Limits::default(),
        &|| false,
    )
    .unwrap();
    let mut loaded = LoadedWindow::default();
    loaded.chunks.insert([-1, -2], Arc::new(chunk));
    loaded.coverage.chunks.insert([-1, -2]);
    Arc::new(
        PreparedMap::new(
            Source {
                map: MapId([7; 16]),
                generation: 4,
            },
            0,
            Arc::new(loaded),
            Vec::new(),
            &mut tours::Budget::new(u64::MAX, &|| false),
        )
        .unwrap(),
    )
}
fn request(map: &PreparedMap, kind: ColorKind) -> Request {
    Request {
        source: map.source(),
        java_position: [-16, 64, -32],
        kind,
        climate_policy: ClimatePolicy::Require1193,
    }
}
#[test]
fn climate_inventory_remains_exact_and_does_not_alias_old_or_mod_names() {
    let manifest = Manifest::parse().unwrap();
    assert_eq!(manifest.rows.len(), 63);
    let plains = manifest.climate("minecraft:plains").unwrap();
    assert_eq!(plains.temperature.to_bits(), 0.8_f32.to_bits());
    assert_eq!(plains.downfall.to_bits(), 0.4_f32.to_bits());
    assert_eq!(plains.effects.water_color, 4_159_204);
    for missing in [
        "mod:plains",
        "plains",
        "minecraft:unknown",
        "minecraft:extreme_hills",
    ] {
        assert!(matches!(
            manifest.climate(missing),
            Err(Error::UnknownBiome)
        ));
    }
}
#[test]
fn original_native_foliage_constants_are_not_an_approximate_palette() {
    assert_eq!(FoliageConstant::Evergreen.rgb(), [97, 153, 97]);
    assert_eq!(FoliageConstant::Birch.rgb(), [128, 167, 85]);
    assert_eq!(FoliageConstant::Default.rgb(), [72, 181, 24]);
    assert_eq!(FoliageConstant::Mangrove.rgb(), [146, 198, 72]);
}
#[test]
fn saved_quart_coordinates_and_native_float_rounding_reach_provider_output() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(8 << 20).unwrap();
    let provider = tint(&budget, cancel);
    let map = map(3218, Some("minecraft:plains"), true);
    let low = provider
        .sample_kind(&map, request(&map, ColorKind::Grass), cancel)
        .unwrap();
    // 0.8f32 promoted to f64 gives a column of50, not decimal-f64's51.
    assert_eq!(low.rgb, [50, 173, 17]);
    assert_eq!(low.version_relation, VersionRelation::Java1193);
    assert_eq!(low.biome, "minecraft:plains");
    assert!(low.colormap.is_some());
    let mut high = request(&map, ColorKind::Grass);
    high.java_position[1] = 68;
    let high = provider.sample_kind(&map, high, cancel).unwrap();
    assert_eq!(high.biome, "minecraft:desert");
    assert_eq!(high.rgb, [0, 255, 17]);
    assert_eq!(high.multiplier(), [0.0, 1.0, 17.0 / 255.0]);
    let chunk = &map.loaded().chunks[&[-1, -2]];
    let BiomeSample::Name(original) = chunk.biome_at(low.java_position) else {
        panic!("fixture biome");
    };
    assert_eq!(low.biome.as_ptr(), original.as_ptr());
    drop(provider);
    assert_eq!(budget.used(), 0);
}
#[test]
fn explicit_biome_colors_override_images_without_losing_climate_provenance() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(8 << 20).unwrap();
    let provider = tint(&budget, cancel);
    let map = map(3218, Some("minecraft:badlands"), false);
    for (kind, expected) in [
        (ColorKind::Grass, 9_470_285),
        (ColorKind::Foliage, 10_387_789),
        (ColorKind::Water, 4_159_204),
    ] {
        let sample = provider
            .sample_kind(&map, request(&map, kind), cancel)
            .unwrap();
        assert_eq!(sample.rgb, rgb(expected));
        assert!(sample.colormap.is_none());
        assert_eq!(
            sample.climate_source.as_str(),
            "data/minecraft/worldgen/biome/badlands.json"
        );
        assert_eq!(
            sample.climate_sha256.to_string(),
            "991292fe5ea122c14697048c4f275f0f81fc39772e0b375daa7e65da60cabb0c"
        );
    }
}
#[test]
fn missing_biomes_unknown_names_and_stale_sources_do_not_invent_tints() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(8 << 20).unwrap();
    let provider = tint(&budget, cancel);
    let absent = map(3218, None, false);
    assert!(matches!(
        provider.sample_kind(&absent, request(&absent, ColorKind::Grass), cancel),
        Err(Error::MissingBiome(MissingBiome::Palette))
    ));
    let unknown = map(3218, Some("mod:plains"), false);
    assert!(matches!(
        provider.sample_kind(&unknown, request(&unknown, ColorKind::Water), cancel),
        Err(Error::UnknownBiome)
    ));
    let mut stale = request(&unknown, ColorKind::Foliage);
    stale.source.generation += 1;
    assert!(matches!(
        provider.sample_kind(&unknown, stale, cancel),
        Err(Error::StaleSource)
    ));
    let mut outside = request(&unknown, ColorKind::Grass);
    outside.java_position[0] = 0;
    assert!(matches!(
        provider.sample_kind(&unknown, outside, cancel),
        Err(Error::MissingChunk)
    ));
    outside.java_position[1] = 320;
    assert!(matches!(
        provider.sample_kind(&unknown, outside, cancel),
        Err(Error::OutsideHeight)
    ));
}
#[test]
fn spatial_grass_modifiers_fail_instead_of_disappearing_into_raw_green() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(8 << 20).unwrap();
    let provider = tint(&budget, cancel);
    for (biome, modifier) in [
        ("minecraft:dark_forest", GrassModifier::DarkForest),
        ("minecraft:swamp", GrassModifier::Swamp),
    ] {
        let map = map(3218, Some(biome), false);
        let result = provider.sample_kind(&map, request(&map, ColorKind::Grass), cancel);
        assert!(matches!(result, Err(Error::GrassModifier(actual)) if actual == modifier));
        assert!(provider
            .sample_kind(&map, request(&map, ColorKind::Water), cancel)
            .is_ok());
    }
}
#[test]
fn snapshot_climate_requires_explicit_versioned_rendering_policy() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(8 << 20).unwrap();
    let provider = tint(&budget, cancel);
    for version in [2834, 2835, 2836] {
        let map = map(version, Some("minecraft:plains"), false);
        assert!(
            matches!(provider.sample_kind(&map, request(&map, ColorKind::Grass), cancel), Err(Error::EarlierClimateNotVerified(actual)) if actual == version)
        );
        let mut enabled = request(&map, ColorKind::Grass);
        enabled.climate_policy = ClimatePolicy::RenderEarlierWith1193;
        let sample = provider.sample_kind(&map, enabled, cancel).unwrap();
        assert_eq!(sample.rgb, [50, 173, 17]);
        assert_eq!(
            sample.version_relation,
            VersionRelation::EarlierSaveWith1193Climate {
                saved_data_version: version
            }
        );
    }
    for version in [2833, 3219] {
        assert!(matches!(
            ClimatePolicy::RenderEarlierWith1193.relation(version),
            Err(Error::UnsupportedDataVersion(_))
        ));
    }
}
#[test]
fn cancelled_sample_returns_no_value_and_does_not_change_accounting() {
    let stop = AtomicBool::new(false);
    let budget = ByteBudget::new(8 << 20).unwrap();
    let provider = tint(&budget, Cancel::new(&stop));
    let map = map(3218, Some("minecraft:plains"), false);
    let used = budget.used();
    stop.store(true, std::sync::atomic::Ordering::Release);
    assert!(matches!(
        provider.sample_kind(&map, request(&map, ColorKind::Grass), Cancel::new(&stop)),
        Err(Error::Asset(AssetError::Cancelled))
    ));
    assert_eq!(budget.used(), used);
}

#[test]
fn native_seeded_blend_uses_all_twenty_five_block_samples_and_integer_rgb_mean() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(8 << 20).unwrap();
    let provider = tint(&budget, cancel);
    let map = map(3218, Some("minecraft:plains"), false);
    let request = NativeRequest {
        source: map.source(),
        java_position: [-8, 64, -24],
        kind: ColorKind::Water,
        climate_policy: ClimatePolicy::RenderEarlierWith1193,
        world_seed: Some(0),
        blend_radius: 2,
    };
    let result = provider.sample_native(&map, request, cancel).unwrap();
    assert_eq!(result.rgb, rgb(4_159_204));
    assert_eq!(result.contributions.len(), 25);
    assert!(result
        .contributions
        .iter()
        .all(|part| part.sample.biome == "minecraft:plains"));
    assert_eq!(
        result.contributions.first().unwrap().sample.java_position,
        [-10, 64, -26]
    );
    assert_eq!(
        result.contributions.last().unwrap().sample.java_position,
        [-6, 64, -22]
    );
    assert!(result
        .contributions
        .iter()
        .all(|part| part.sample.colormap.is_none()));
}

#[test]
fn modern_profile_keeps_snapshot_relation_and_rejects_missing_seed_or_unsupported_noise() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(8 << 20).unwrap();
    let provider = tint(&budget, cancel);
    let dark_map = map(2834, Some("minecraft:dark_forest"), false);
    let mut request = NativeRequest {
        source: dark_map.source(),
        java_position: [-8, 64, -24],
        kind: ColorKind::Grass,
        climate_policy: ClimatePolicy::default(),
        world_seed: None,
        blend_radius: 0,
    };
    assert!(matches!(
        provider.sample_native(&dark_map, request, cancel),
        Err(Error::MissingSeed)
    ));
    request.world_seed = Some(-1);
    let result = provider.sample_native(&dark_map, request, cancel).unwrap();
    assert_eq!(result.contributions.len(), 1);
    assert_eq!(
        result.contributions[0].sample.version_relation,
        VersionRelation::EarlierSaveWith1193Climate {
            saved_data_version: 2834
        }
    );
    let swamp = map(2834, Some("minecraft:swamp"), false);
    request.source = swamp.source();
    let swamp_color = provider.sample_native(&swamp, request, cancel).unwrap();
    assert_eq!(swamp_color.rgb, SwampNoise::new().grass_rgb(-8, -24));
    request.blend_radius = 8;
    assert!(matches!(
        provider.sample_native(&swamp, request, cancel),
        Err(Error::BlendRadius)
    ));
}

#[test]
fn native_chunk_access_clamps_selected_vertical_quart_before_palette_lookup() {
    assert_eq!(lookup_quart([3, -17, -2]), [3, -16, -2]);
    assert_eq!(lookup_quart([3, -16, -2]), [3, -16, -2]);
    assert_eq!(lookup_quart([3, 79, -2]), [3, 79, -2]);
    assert_eq!(lookup_quart([3, 80, -2]), [3, 79, -2]);
}

#[test]
fn officially_renamed_snapshot_biomes_keep_stored_identity_and_native_climate() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let account = ByteBudget::new(8 << 20).unwrap();
    let provider = tint(&account, cancel);
    for (stored, destination) in [
        (
            "minecraft:tall_birch_forest",
            "minecraft:old_growth_birch_forest",
        ),
        ("minecraft:snowy_tundra", "minecraft:snowy_plains"),
        (
            "minecraft:giant_tree_taiga",
            "minecraft:old_growth_pine_taiga",
        ),
        (
            "minecraft:giant_spruce_taiga",
            "minecraft:old_growth_spruce_taiga",
        ),
    ] {
        for version in [2834, 2836] {
            let original = map(version, Some(stored), false);
            let modern = map(3218, Some(destination), false);
            for kind in [ColorKind::Grass, ColorKind::Foliage, ColorKind::Water] {
                let expected = provider
                    .sample_kind(&modern, request(&modern, kind), cancel)
                    .unwrap();
                let mut older = request(&original, kind);
                assert!(
                    matches!(provider.sample_kind(&original, older, cancel), Err(Error::EarlierClimateNotVerified(v)) if v==version)
                );
                older.climate_policy = ClimatePolicy::RenderEarlierWith1193;
                let actual = provider.sample_kind(&original, older, cancel).expect("officially renamed snapshot biome must resolve under the explicit modern rendering policy");
                assert_eq!(actual.biome, stored);
                assert_eq!(actual.rgb, expected.rgb);
                assert_eq!(actual.climate_source, expected.climate_source);
                assert_eq!(actual.climate_sha256, expected.climate_sha256);
                assert_eq!(
                    actual.version_relation,
                    VersionRelation::EarlierSaveWith1193Climate {
                        saved_data_version: version
                    }
                );
                let native = |source| NativeRequest {
                    source,
                    java_position: [-8, 64, -24],
                    kind,
                    climate_policy: ClimatePolicy::RenderEarlierWith1193,
                    world_seed: Some(0),
                    blend_radius: 2,
                };
                let expected_blend = provider
                    .sample_native(&modern, native(modern.source()), cancel)
                    .unwrap();
                let actual_blend = provider
                    .sample_native(&original, native(original.source()), cancel)
                    .unwrap();
                assert_eq!(actual_blend.rgb, expected_blend.rgb);
                assert_eq!(actual_blend.contributions.len(), 25);
                assert!(actual_blend
                    .contributions
                    .iter()
                    .all(|part| part.sample.biome == stored
                        && part.sample.climate_source == expected.climate_source
                        && part.sample.climate_sha256 == expected.climate_sha256));
            }
        }
        let invalid_modern = map(3218, Some(stored), false);
        assert!(matches!(
            provider.sample_kind(
                &invalid_modern,
                request(&invalid_modern, ColorKind::Water),
                cancel
            ),
            Err(Error::UnknownBiome)
        ));
    }
}

#[test]
fn official_biome_render_conversion_is_version_qualified_and_leaves_unknown_names_exact() {
    for version in [2834, 2836, 2837] {
        let relation = ClimatePolicy::RenderEarlierWith1193
            .relation(version)
            .unwrap();
        assert_eq!(
            render_climate_identity("minecraft:tall_birch_forest", relation),
            "minecraft:old_growth_birch_forest"
        );
        for name in [
            "mod:tall_birch_forest",
            "minecraft:unknown",
            "minecraft:plains",
        ] {
            assert_eq!(render_climate_identity(name, relation), name);
        }
    }
    for version in [2838, 2839, 3218] {
        let relation = ClimatePolicy::RenderEarlierWith1193
            .relation(version)
            .unwrap();
        assert_eq!(
            render_climate_identity("minecraft:tall_birch_forest", relation),
            "minecraft:tall_birch_forest"
        );
    }
}
