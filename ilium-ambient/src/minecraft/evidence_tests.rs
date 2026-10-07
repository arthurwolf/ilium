//! Synthetic NBT goes through the production decoder; no saves or models opened.
use super::super::{
    chunk,
    nbt::{Compound, Document, Tag, Text},
};
use super::*;
use std::cell::Cell;

fn state(name: &str, properties: &[(&str, &str)]) -> BlockState {
    BlockState {
        name: name.into(),
        properties: properties
            .iter()
            .map(|&(key, value)| (key.into(), value.into()))
            .collect(),
    }
}
fn palette() -> Vec<BlockState> {
    [
        ("air", vec![]),
        ("grass_block", vec![("snowy", "false")]),
        ("dirt", vec![]),
        ("grass", vec![]),
        ("oak_log", vec![("axis", "y")]),
        (
            "oak_leaves",
            vec![("distance", "1"), ("persistent", "false")],
        ),
        ("sand", vec![]),
        ("dead_bush", vec![]),
        ("snow", vec![("layers", "1")]),
        ("stone", vec![]),
        ("gravel", vec![]),
        ("water", vec![("level", "7")]),
        ("lily_pad", vec![]),
        ("clay", vec![]),
        ("cobblestone", vec![]),
        ("mossy_cobblestone", vec![]),
        ("sandstone", vec![]),
        ("cut_sandstone", vec![]),
        ("chiseled_sandstone", vec![]),
        ("prismarine", vec![]),
        ("prismarine_bricks", vec![]),
        ("sea_lantern", vec![]),
        ("fixture_unknown", vec![("zeta", "雪😀"), ("axis", "z")]),
        ("cave_air", vec![]),
        ("lava", vec![("level", "0")]),
        ("ice", vec![]),
        ("oak_log", vec![("axis", "x")]),
        (
            "birch_leaves",
            vec![("distance", "1"), ("persistent", "false")],
        ),
        ("snow", vec![("layers", "9")]),
        ("water", vec![("level", "16")]),
        ("cobblestone", vec![("invented", "true")]),
        ("air", vec![("invented", "true")]),
        ("oak_planks", vec![]),
        (
            "oak_door",
            vec![
                ("facing", "east"),
                ("half", "lower"),
                ("hinge", "left"),
                ("open", "false"),
                ("powered", "false"),
            ],
        ),
        (
            "oak_door",
            vec![
                ("facing", "east"),
                ("half", "upper"),
                ("hinge", "left"),
                ("open", "false"),
                ("powered", "false"),
            ],
        ),
        (
            "oak_door",
            vec![
                ("facing", "east"),
                ("half", "middle"),
                ("hinge", "left"),
                ("open", "false"),
                ("powered", "false"),
            ],
        ),
        (
            "oak_fence",
            vec![
                ("east", "false"),
                ("north", "false"),
                ("south", "false"),
                ("waterlogged", "false"),
                ("west", "false"),
            ],
        ),
        (
            "red_bed",
            vec![("facing", "east"), ("occupied", "false"), ("part", "foot")],
        ),
        (
            "red_bed",
            vec![("facing", "east"), ("occupied", "false"), ("part", "head")],
        ),
        ("mycelium", vec![("snowy", "false")]),
        ("brown_mushroom", vec![]),
        ("red_mushroom", vec![]),
        (
            "brown_mushroom_block",
            vec![
                ("down", "false"),
                ("east", "false"),
                ("north", "false"),
                ("south", "false"),
                ("up", "true"),
                ("west", "false"),
            ],
        ),
        ("red_sand", vec![]),
        ("terracotta", vec![]),
        ("orange_terracotta", vec![]),
        ("yellow_terracotta", vec![]),
        ("jungle_log", vec![("axis", "y")]),
        (
            "jungle_leaves",
            vec![("distance", "1"), ("persistent", "false")],
        ),
        ("spruce_log", vec![("axis", "y")]),
        (
            "spruce_leaves",
            vec![("distance", "1"), ("persistent", "false")],
        ),
    ]
    .into_iter()
    .map(|(name, properties)| state(&format!("minecraft:{name}"), &properties))
    .collect()
}
fn fields(items: Vec<(&str, Tag)>) -> Compound {
    items
        .into_iter()
        .map(|(key, value)| (Text::from(key), value))
        .collect()
}
fn decoded(
    version: i32,
    position: [i32; 2],
    palette: &[BlockState],
    at: impl Fn(i32, i32, i32) -> usize,
) -> chunk::DecodedChunk {
    let tags: Vec<_> = palette
        .iter()
        .map(|state| {
            Tag::Compound(fields(vec![
                ("Name", Tag::String(state.name.as_str().into())),
                (
                    "Properties",
                    Tag::Compound(
                        state
                            .properties
                            .iter()
                            .map(|(key, value)| {
                                (key.as_str().into(), Tag::String(value.as_str().into()))
                            })
                            .collect(),
                    ),
                ),
            ]))
        })
        .collect();
    let bits = ((usize::BITS - (palette.len() - 1).leading_zeros()) as usize).max(4);
    let per_word = 64 / bits;
    let sections = (-4_i8..=19)
        .map(|y| {
            let mut words = vec![0_u64; 4096_usize.div_ceil(per_word)];
            for index in 0..4096 {
                let value = at(
                    (index % 16) as i32,
                    i32::from(y) * 16 + (index / 256) as i32,
                    ((index / 16) % 16) as i32,
                );
                assert!(value < palette.len());
                words[index / per_word] |= (value as u64) << ((index % per_word) * bits);
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
                                values: tags.clone(),
                            },
                        ),
                        (
                            "data",
                            Tag::LongArray(words.into_iter().map(|word| word as i64).collect()),
                        ),
                    ])),
                ),
                // Deliberately contradictory metadata: block evidence must ignore it.
                (
                    "biomes",
                    Tag::Compound(fields(vec![(
                        "palette",
                        Tag::List {
                            kind: 8,
                            values: vec![Tag::String("minecraft:the_end".into())],
                        },
                    )])),
                ),
            ]))
        })
        .collect();
    let wrapped = version <= 2836;
    let body = fields(vec![
        ("xPos", Tag::Int(position[0])),
        ("zPos", Tag::Int(position[1])),
        ("Status", Tag::String("full".into())),
        (
            "Structures",
            Tag::Compound(fields(vec![("fake_monument", Tag::Int(1))])),
        ),
        (
            if wrapped { "Sections" } else { "sections" },
            Tag::List {
                kind: 10,
                values: sections,
            },
        ),
    ]);
    let mut root = if wrapped {
        fields(vec![("Level", Tag::Compound(body))])
    } else {
        body
    };
    root.insert("DataVersion".into(), Tag::Int(version));
    chunk::decode(
        &Document {
            name: "synthetic".into(),
            root,
        },
        position,
        chunk::Limits::default(),
        &|| false,
    )
    .unwrap()
}
fn source() -> Source {
    Source {
        map: MapId([7; 16]),
        generation: 11,
    }
}
fn bounds(position: [i32; 2]) -> surface::Bounds {
    surface::Bounds {
        minimum: position.map(|value| value * 16),
        maximum: position.map(|value| value * 16 + 15),
    }
}
fn survey(chunks: &[chunk::DecodedChunk], core: surface::Bounds) -> Report<'_> {
    let view = SurfaceWindow::overworld(
        core,
        0,
        chunks,
        surface::Limits::default(),
        &mut Work::new(100_000, &|| false),
    )
    .unwrap();
    analyze(
        &view,
        source(),
        Limits::default(),
        &mut Work::new(2_000_000, &|| false),
    )
    .unwrap()
}
fn has(report: &Report<'_>, category: Category) -> bool {
    report
        .targets
        .iter()
        .any(|target| target.key.category == category)
}

#[test]
fn biome_evidence_roles_require_supported_minecraft_state_schemas() {
    let mushroom_block = state(
        "minecraft:brown_mushroom_block",
        &[
            ("down", "false"),
            ("east", "false"),
            ("north", "false"),
            ("south", "false"),
            ("up", "true"),
            ("west", "false"),
        ],
    );
    assert_eq!(role(&mushroom_block), Role::Mushroom);
    assert_eq!(
        role(&state("minecraft:brown_mushroom_block", &[("up", "true")])),
        Role::Unknown
    );
    assert_eq!(
        role(&state("minecraft:mycelium", &[("snowy", "false")])),
        Role::Mycelium
    );
    assert_eq!(
        role(&state("minecraft:mycelium", &[("invented", "true")])),
        Role::Unknown
    );
    assert_eq!(
        role(&state("minecraft:orange_terracotta", &[])),
        Role::Terracotta
    );
    assert_eq!(
        role(&state(
            "minecraft:orange_terracotta",
            &[("invented", "true")]
        )),
        Role::Unknown
    );
}

#[test]
fn mycelium_with_distributed_mushrooms_forms_a_distinct_biome_target() {
    let palette = palette();
    let mycelium = palette
        .iter()
        .position(|state| state.name == "minecraft:mycelium")
        .unwrap();
    let mushroom = palette
        .iter()
        .position(|state| state.name == "minecraft:brown_mushroom")
        .unwrap();
    let mushroom_block = palette
        .iter()
        .position(|state| state.name == "minecraft:brown_mushroom_block")
        .unwrap();
    let mushroom_columns = [
        (2, 2),
        (3, 2),
        (10, 2),
        (11, 2),
        (2, 10),
        (3, 10),
        (10, 10),
        (11, 10),
    ];
    let mushroom_block_columns = &mushroom_columns[4..];
    let clustered_mushroom_columns = [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (0, 1),
        (1, 1),
        (2, 1),
        (3, 1),
    ];
    for version in [2834, 2835, 2836, 3218] {
        let chunk = decoded(version, [0, 0], &palette, |x, y, z| {
            if y == 68 && mushroom_block_columns.contains(&(x, z)) {
                mushroom_block
            } else if y == 65 && mushroom_columns[..4].contains(&(x, z)) {
                mushroom
            } else if y == 64 {
                mycelium
            } else if y < 64 {
                2
            } else {
                0
            }
        });
        let chunks = [chunk];
        let report = survey(&chunks, bounds([0, 0]));
        let target = report.targets.iter().find(|target| {
            target.key.category == Category::MushroomFieldsLike
                && target.key.category.kind() == Kind::Biome
                && target.anchor.block.state.name == "minecraft:mycelium"
                && target.corroboration.block.state.name == "minecraft:brown_mushroom"
        });
        assert!(
            target.is_some(),
            "distributed mycelium and mushroom evidence should create a target at DataVersion {version}"
        );
        let target = target.unwrap();
        assert_eq!(target.key.category, Category::MushroomFieldsLike);
        assert_eq!(target.support.primary_columns, 256);
        assert!(target.support.secondary_columns >= 8);

        let clustered = [decoded(version, [0, 0], &palette, |x, y, z| {
            if y == 65 && clustered_mushroom_columns.contains(&(x, z)) {
                mushroom
            } else if y == 64 {
                mycelium
            } else if y < 64 {
                2
            } else {
                0
            }
        })];
        assert!(
            !has(
                &survey(&clustered, bounds([0, 0])),
                Category::MushroomFieldsLike
            ),
            "mushrooms confined to one sector must not qualify at DataVersion {version}"
        );
    }
}

#[test]
fn red_sand_and_terracotta_with_dry_plants_form_a_distinct_biome_target() {
    let palette = palette();
    let red_sand = palette
        .iter()
        .position(|state| state.name == "minecraft:red_sand")
        .unwrap();
    let terracotta = palette
        .iter()
        .position(|state| state.name == "minecraft:orange_terracotta")
        .unwrap();
    let dead_bush = palette
        .iter()
        .position(|state| state.name == "minecraft:dead_bush")
        .unwrap();
    let plant_columns = [
        (2, 2),
        (3, 2),
        (10, 2),
        (11, 2),
        (2, 10),
        (3, 10),
        (10, 10),
        (11, 10),
    ];
    let clustered_plant_columns = [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (0, 1),
        (1, 1),
        (2, 1),
        (3, 1),
    ];
    for version in [2834, 2835, 2836, 3218] {
        let chunk = decoded(version, [0, 0], &palette, |x, y, z| {
            if y == 65 && plant_columns.contains(&(x, z)) {
                dead_bush
            } else if y == 64 && (x + z) % 3 == 0 && !plant_columns.contains(&(x, z)) {
                terracotta
            } else if y == 64 {
                red_sand
            } else if y < 64 {
                2
            } else {
                0
            }
        });
        let chunks = [chunk];
        let report = survey(&chunks, bounds([0, 0]));
        let target = report.targets.iter().find(|target| {
            target.key.category == Category::BadlandsLikeSurface
                && target.key.category.kind() == Kind::Biome
                && matches!(
                    target.anchor.block.state.name.as_str(),
                    "minecraft:red_sand" | "minecraft:orange_terracotta"
                )
                && target.corroboration.block.state.name == "minecraft:dead_bush"
        });
        assert!(
            target.is_some(),
            "red sand, terracotta and dry plant evidence should create a target at DataVersion {version}"
        );
        let target = target.unwrap();
        assert_eq!(target.key.category, Category::BadlandsLikeSurface);
        assert!(target.support.secondary_columns >= 8);

        let clustered = [decoded(version, [0, 0], &palette, |x, y, z| {
            if y == 65 && clustered_plant_columns.contains(&(x, z)) {
                dead_bush
            } else if y == 64 && (x + z) % 3 == 0 && !clustered_plant_columns.contains(&(x, z)) {
                terracotta
            } else if y == 64 {
                red_sand
            } else if y < 64 {
                2
            } else {
                0
            }
        })];
        assert!(
            !has(
                &survey(&clustered, bounds([0, 0])),
                Category::BadlandsLikeSurface
            ),
            "dry plants confined to one sector must not qualify at DataVersion {version}"
        );
    }
}

fn scene(category: Category, x: i32, y: i32, z: i32) -> usize {
    use Category::*;
    match category {
        OpenGrassland => {
            if y == 65 && x % 4 == 0 && z % 4 == 0 {
                return 3;
            }
            if y == 64 {
                return 1;
            }
        }
        RootedWoodland => {
            let roots = [(3, 3), (11, 3), (3, 11), (11, 11)];
            if (69..=70).contains(&y)
                && roots
                    .iter()
                    .any(|&(a, b)| (x - a).abs() <= 2 && (z - b).abs() <= 2)
            {
                return 5;
            }
            if (65..=68).contains(&y) && roots.contains(&(x, z)) {
                return 4;
            }
            if y == 64 {
                return 1;
            }
        }
        SnowySurface => {
            if y == 65 && x >= 8 {
                return 8;
            }
            if y == 64 {
                return if x < 8 { 25 } else { 9 };
            }
        }
        DrySandySurface => {
            if y == 65 && x % 5 == 2 && z % 5 == 2 {
                return 7;
            }
            if y == 64 {
                return 6;
            }
        }
        MushroomFieldsLike => {
            if y == 65 && x % 4 == 0 && z % 4 == 0 {
                return 40;
            }
            if y == 64 {
                return 39;
            }
        }
        BadlandsLikeSurface => {
            let planted = x % 4 == 0 && z % 4 == 0;
            if y == 65 && planted {
                return 7;
            }
            if y == 64 {
                return if !planted && (x + z) % 3 == 0 { 45 } else { 43 };
            }
        }
        JungleCanopyLike => {
            // Keep the four canopy patches connected while retaining four
            // spatial sectors for the category's corroborating trunk signal.
            let roots = [(3, 3), (8, 3), (3, 8), (8, 8)];
            if (69..=70).contains(&y)
                && roots
                    .iter()
                    .any(|&(a, b)| (x - a).abs() <= 2 && (z - b).abs() <= 2)
            {
                return 48;
            }
            if (65..=68).contains(&y) && roots.contains(&(x, z)) {
                return 47;
            }
            if y == 64 {
                return 1;
            }
        }
        SpruceSnowfieldLike => {
            let roots = [(3, 3), (11, 3), (3, 11), (11, 11)];
            if (69..=70).contains(&y)
                && roots
                    .iter()
                    .any(|&(a, b)| (x - a).abs() <= 2 && (z - b).abs() <= 2)
            {
                return 50;
            }
            if (65..=68).contains(&y) && roots.contains(&(x, z)) {
                return 49;
            }
            if y == 65 {
                return 8;
            }
            if y == 64 {
                return 1;
            }
        }
        VegetatedShore => {
            if y == 65 && [1, 5].contains(&x) && z % 4 == 2 {
                return 12;
            }
            if y == 64 && x < 8 {
                return 11;
            }
            if y == 63 {
                return 13;
            }
        }
        RockyRelief => {
            if y == 64 + x / 3 {
                return if z % 4 == 0 { 10 } else { 9 };
            }
        }
        WeatheredMasonry | OrnamentalSandstone | DesertTempleLike | PrismarineMasonry
        | OceanMonumentLike => {
            let built = (4..=11).contains(&x) && (4..=11).contains(&z);
            if built
                && matches!(category, PrismarineMasonry | OceanMonumentLike)
                && (65..=67).contains(&y)
            {
                return 11;
            }
            if built && (62..=64).contains(&y) {
                return match category {
                    WeatheredMasonry => {
                        if x % 2 == 0 {
                            15
                        } else {
                            14
                        }
                    }
                    OrnamentalSandstone => match x % 3 {
                        0 => 18,
                        1 => 17,
                        _ => 16,
                    },
                    DesertTempleLike => {
                        if x % 2 == 0 {
                            18
                        } else {
                            17
                        }
                    }
                    _ => {
                        if [(4, 4), (11, 11)].contains(&(x, z)) {
                            21
                        } else if x % 2 == 0 {
                            20
                        } else {
                            19
                        }
                    }
                };
            }
            if y == 60 {
                return 9;
            }
        }
        DwellingLikeConstruction => {}
    }
    0
}

#[test]
fn all_categories_have_spatially_supported_positive_controls() {
    for category in CATEGORIES {
        let chunks = [decoded(3218, [-1, -2], &palette(), |x, y, z| {
            scene(category, x, y, z)
        })];
        let report = survey(&chunks, bounds([-1, -2]));
        let target = report
            .targets
            .iter()
            .find(|target| target.key.category == category)
            .unwrap_or_else(|| panic!("missing positive control for {category:?}"));
        assert!(target.support.columns >= 32);
        assert_eq!(target.key.anchor, target.anchor.block.position);
        assert_eq!(target.key.tile, [-1, -2]);
        assert_eq!(target.key.revision, RULE_REVISION);
        assert_eq!(
            target
                .support
                .footprint
                .iter()
                .map(|word| word.count_ones())
                .sum::<u32>(),
            u32::from(target.support.columns)
        );
        for witness in [Some(target.anchor), Some(target.corroboration)]
            .into_iter()
            .chain(target.landmarks)
            .flatten()
        {
            let chunk::BlockSample::State(original) = chunks[0].block_at(witness.block.position)
            else {
                panic!()
            };
            assert!(std::ptr::eq(witness.block.state, original));
            let [x, _, z] = witness.block.position;
            assert!(included(
                &target.support.footprint,
                (z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize
            ));
        }
        if category == Category::PrismarineMasonry {
            assert!(target.anchor.water_above);
            assert!(!target.anchor.block.only_air_above);
            assert_eq!(target.confidence, Confidence::Supported);
        }
        if category == Category::WeatheredMasonry {
            assert_eq!(target.confidence, Confidence::Corroborated);
        }
    }
}

#[test]
fn specific_patterns_do_not_promote_generic_surfaces_or_masonry() {
    let woodland = [decoded(3218, [0, 0], &palette(), |x, y, z| {
        scene(Category::RootedWoodland, x, y, z)
    })];
    let woodland_report = survey(&woodland, bounds([0, 0]));
    assert!(!has(&woodland_report, Category::JungleCanopyLike));
    assert!(!has(&woodland_report, Category::SpruceSnowfieldLike));

    let sandstone = [decoded(3218, [0, 0], &palette(), |x, y, _z| {
        if (4..=11).contains(&x) && y == 64 {
            17
        } else {
            0
        }
    })];
    assert!(!has(
        &survey(&sandstone, bounds([0, 0])),
        Category::DesertTempleLike
    ));

    let prismarine = [decoded(3218, [0, 0], &palette(), |x, y, z| {
        if (4..=11).contains(&x) && (4..=11).contains(&z) && y == 64 {
            20
        } else {
            0
        }
    })];
    assert!(!has(
        &survey(&prismarine, bounds([0, 0])),
        Category::OceanMonumentLike
    ));
}

#[test]
fn admitted_snapshot_and_root_layouts_ignore_biome_and_structure_metadata() {
    for version in [2834, 2835, 2836, 3218] {
        let chunks = [decoded(version, [0, 0], &palette(), |x, y, z| {
            scene(Category::RootedWoodland, x, y, z)
        })];
        assert!(has(
            &survey(&chunks, bounds([0, 0])),
            Category::RootedWoodland
        ));
        let empty = [decoded(version, [0, 0], &palette(), |_, _, _| 0)];
        let report = survey(&empty, bounds([0, 0]));
        assert!(report.targets.is_empty());
        assert_eq!(report.stats.empty_columns, 256);
    }
}

#[test]
fn lone_decorations_thin_planes_and_narrow_strips_are_not_structures() {
    for category in [
        Category::WeatheredMasonry,
        Category::OrnamentalSandstone,
        Category::PrismarineMasonry,
    ] {
        for shape in 0..3 {
            let chunks = [decoded(3218, [0, 0], &palette(), |x, y, z| {
                let original = scene(category, x, y, z);
                if !matches!(original, 14..=21) {
                    return original;
                }
                if (shape == 0 && (x, y, z) != (6, 64, 6))
                    || (shape == 1 && y != 64)
                    || (shape == 2 && z != 7)
                {
                    return 0;
                }
                original
            })];
            assert!(
                !has(&survey(&chunks, bounds([0, 0])), category),
                "{category:?}/{shape}"
            );
        }
    }
}

#[test]
fn equal_counts_do_not_join_disconnected_ornaments_into_a_structure() {
    let chunks = [decoded(3218, [0, 0], &palette(), |x, y, z| {
        if (62..=64).contains(&y) && (x + z) % 2 == 0 {
            return if x < 8 { 14 } else { 15 };
        }
        0
    })];
    assert!(!has(
        &survey(&chunks, bounds([0, 0])),
        Category::WeatheredMasonry
    ));
}

#[test]
fn forest_requires_separated_rooted_stems_and_matching_local_canopies() {
    for variant in 0..4 {
        let chunks = [decoded(3218, [0, 0], &palette(), |x, y, z| {
            let original = scene(Category::RootedWoodland, x, y, z);
            match (variant, original) {
                (0, 4) => 26,                    // Horizontal logs are not rooted vertical stems.
                (1, 5) => 27, // Nearby but wrong-species crowns do not corroborate.
                (2, 1) => 0,  // Floating trunks have no observed soil attachment.
                (3, 4) if (x, z) != (3, 3) => 0, // One tree is not woodland.
                _ => original,
            }
        })];
        assert!(!has(
            &survey(&chunks, bounds([0, 0])),
            Category::RootedWoodland
        ));
    }
    let one_large_tree = [decoded(3218, [0, 0], &palette(), |x, y, z| {
        if y == 64 {
            return 1;
        }
        if (65..=68).contains(&y) && (7..=8).contains(&x) && (7..=8).contains(&z) {
            return 4;
        }
        if y == 70 && (2..=13).contains(&x) && (2..=13).contains(&z) {
            return 5;
        }
        0
    })];
    assert!(!has(
        &survey(&one_large_tree, bounds([0, 0])),
        Category::RootedWoodland
    ));
}

#[test]
fn malformed_or_modded_evidence_stays_exact_but_does_not_support_rules() {
    for (category, replace, replacement) in [
        (Category::SnowySurface, 8, 28),
        (Category::VegetatedShore, 11, 29),
        (Category::WeatheredMasonry, 14, 30),
    ] {
        let chunks = [decoded(3218, [0, 0], &palette(), |x, y, z| {
            let value = scene(category, x, y, z);
            if value == replace {
                replacement
            } else {
                value
            }
        })];
        let report = survey(&chunks, bounds([0, 0]));
        assert!(!has(&report, category));
        assert!(report.stats.unrecognized_columns > 0);
    }
    let mut states = palette();
    states[22].name = "unknown_mod:machine".into();
    let chunks = [decoded(3218, [0, 0], &states, |_, y, _| match y {
        64 => 22,
        63 => 24,
        62 => 23,
        _ => 0,
    })];
    let report = survey(&chunks, bounds([0, 0]));
    assert!(report.targets.is_empty());
    assert_eq!(report.stats.unrecognized_columns, 256);
    for (y, expected) in [(64, &states[22]), (63, &states[24]), (62, &states[23])] {
        let chunk::BlockSample::State(actual) = chunks[0].block_at([0, y, 0]) else {
            panic!()
        };
        assert_eq!(actual, expected);
    }
    assert_eq!(
        states[22]
            .properties
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["axis", "zeta"]
    );
    for name in [
        "mod:stone_bricks",
        "minecraft:stone_bricks_fake",
        "minecraft:STONE_BRICKS",
    ] {
        assert_eq!(role(&state(name, &[])), Role::Unknown);
    }
}

#[test]
fn roofed_caves_and_deep_water_are_not_silently_promoted_to_surface_targets() {
    for cover in [0, 23, 24] {
        let chunks = [decoded(3218, [0, 0], &palette(), |x, y, z| {
            if cover == 0 && y == 90 {
                return 22;
            }
            if cover == 0 && (65..90).contains(&y) {
                return 23;
            }
            if cover > 0
                && (4..=11).contains(&x)
                && (4..=11).contains(&z)
                && (65..=64 + cover).contains(&y)
            {
                return 11;
            }
            scene(Category::WeatheredMasonry, x, y, z)
        })];
        let report = survey(&chunks, bounds([0, 0]));
        assert_eq!(has(&report, Category::WeatheredMasonry), cover == 23);
        if cover == 24 {
            assert_eq!(report.stats.band_limited_columns, 64);
        }
        if cover == 0 {
            assert_eq!(report.stats.unrecognized_columns, 256);
        }
    }
}

#[test]
fn aquatic_decoration_must_touch_water_and_water_must_touch_banks() {
    for variant in 0..2 {
        let chunks = [decoded(3218, [0, 0], &palette(), |x, y, z| {
            if variant == 1 && y == 64 {
                return 11;
            } // No dry banks anywhere.
            if variant == 0 && y == 64 {
                return 0;
            } // Pads float above dry soil.
            scene(Category::VegetatedShore, x, y, z)
        })];
        assert!(!has(
            &survey(&chunks, bounds([0, 0])),
            Category::VegetatedShore
        ));
    }
}

#[test]
fn partial_proto_and_missing_halo_data_fail_instead_of_returning_empty_success() {
    let original = decoded(3218, [0, 0], &palette(), |x, y, z| {
        scene(Category::OpenGrassland, x, y, z)
    });
    for variant in 0..4 {
        let mut chunk = original.clone();
        match variant {
            0 => {
                chunk.sections.remove(&0);
            }
            1 => chunk.sections.get_mut(&0).unwrap().block_states = None,
            2 => chunk.status = Some("features".into()),
            _ => chunk.sections_present = false,
        }
        let chunks = [chunk];
        let view = SurfaceWindow::overworld(
            bounds([0, 0]),
            0,
            &chunks,
            surface::Limits::default(),
            &mut Work::new(100, &|| false),
        )
        .unwrap();
        assert!(matches!(
            analyze(
                &view,
                source(),
                Limits::default(),
                &mut Work::new(2_000_000, &|| false)
            ),
            Err(Error::Surface(surface::Error::Unqualified { .. }))
        ));
    }
    let chunks = [original];
    let view = SurfaceWindow::overworld(
        bounds([0, 0]),
        1,
        &chunks,
        surface::Limits::default(),
        &mut Work::new(100, &|| false),
    )
    .unwrap();
    assert!(matches!(
        analyze(
            &view,
            source(),
            Limits::default(),
            &mut Work::new(2_000_000, &|| false)
        ),
        Err(Error::Surface(surface::Error::Unqualified { .. }))
    ));
}

#[test]
fn limits_and_one_shot_cancellation_never_publish_partial_reports() {
    let chunks = [decoded(3218, [0, 0], &palette(), |x, y, z| {
        scene(Category::WeatheredMasonry, x, y, z)
    })];
    let view = SurfaceWindow::overworld(
        bounds([0, 0]),
        0,
        &chunks,
        surface::Limits::default(),
        &mut Work::new(100, &|| false),
    )
    .unwrap();
    for limits in [
        Limits {
            max_tiles: 0,
            ..Limits::default()
        },
        Limits {
            max_targets: 0,
            ..Limits::default()
        },
        Limits {
            max_operations: 0,
            ..Limits::default()
        },
        Limits {
            max_owned_bytes: 0,
            ..Limits::default()
        },
        Limits {
            max_targets: HARD_TARGETS + 1,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            analyze(
                &view,
                source(),
                limits,
                &mut Work::new(2_000_000, &|| false)
            ),
            Err(Error::Limit(_))
        ));
    }
    assert!(matches!(
        analyze(
            &view,
            source(),
            Limits::default(),
            &mut Work::new(0, &|| false)
        ),
        Err(Error::Surface(surface::Error::Limit(_)))
    ));
    let checks = Cell::new(0);
    let count = || {
        checks.set(checks.get() + 1);
        false
    };
    let complete = analyze(
        &view,
        source(),
        Limits::default(),
        &mut Work::new(2_000_000, &count),
    )
    .unwrap();
    assert!(complete.stats.operations > 0);
    let total = checks.get();
    for stop_at in [1, 100, total] {
        let checks = Cell::new(0);
        let cancel = || {
            checks.set(checks.get() + 1);
            checks.get() == stop_at
        };
        assert!(matches!(
            analyze(
                &view,
                source(),
                Limits::default(),
                &mut Work::new(2_000_000, &cancel)
            ),
            Err(Error::Surface(surface::Error::Cancelled))
        ));
    }
    let exact = Limits {
        max_operations: complete.stats.operations,
        ..Limits::default()
    };
    assert!(analyze(&view, source(), exact, &mut Work::new(2_000_000, &|| false)).is_ok());
    let short = Limits {
        max_operations: complete.stats.operations - 1,
        ..exact
    };
    assert!(matches!(
        analyze(&view, source(), short, &mut Work::new(2_000_000, &|| false)),
        Err(Error::Limit("operations"))
    ));
}

#[test]
fn stable_tiles_source_generations_and_serializable_handoff_preserve_identity() {
    let chunks = [[0, 0], [-1, 0]].map(|position| {
        decoded(3218, position, &palette(), |x, y, z| {
            scene(Category::OpenGrassland, x, y, z)
        })
    });
    let full = survey(
        &chunks,
        surface::Bounds {
            minimum: [-16, 0],
            maximum: [15, 15],
        },
    );
    let cropped = survey(
        &chunks,
        surface::Bounds {
            minimum: [-15, 0],
            maximum: [15, 15],
        },
    );
    assert_eq!(full.targets.len(), 2);
    assert_eq!(cropped.targets.len(), 1);
    assert_eq!(cropped.stats.edge_columns_not_surveyed, 240);
    assert_eq!(cropped.targets[0], full.targets[1]);
    let summary = cropped.targets[0].summary();
    let bytes = serde_json::to_vec(&summary).unwrap();
    assert_eq!(
        serde_json::from_slice::<TargetSummary>(&bytes).unwrap(),
        summary
    );
    let view = SurfaceWindow::overworld(
        bounds([0, 0]),
        0,
        &chunks,
        surface::Limits::default(),
        &mut Work::new(100, &|| false),
    )
    .unwrap();
    let next_source = Source {
        generation: 12,
        ..source()
    };
    let newer = analyze(
        &view,
        next_source,
        Limits::default(),
        &mut Work::new(2_000_000, &|| false),
    )
    .unwrap();
    assert_eq!(newer.targets[0].key, cropped.targets[0].key);
    assert_ne!(newer.targets[0].summary().source, summary.source);
    let other_map = Source {
        map: MapId([8; 16]),
        ..source()
    };
    let other = analyze(
        &view,
        other_map,
        Limits::default(),
        &mut Work::new(2_000_000, &|| false),
    )
    .unwrap();
    assert_ne!(other.targets[0].key, cropped.targets[0].key);
}

#[test]
fn disconnected_qualifying_components_have_a_deterministic_tie_break() {
    let chunks = [decoded(3218, [0, 0], &palette(), |x, y, z| {
        if (62..=64).contains(&y) && ((x < 6 && z < 6) || (x >= 10 && z >= 10)) {
            return if x % 2 == 0 { 15 } else { 14 };
        }
        0
    })];
    let report = survey(&chunks, bounds([0, 0]));
    assert_eq!(report.stats.qualifying_components, 2);
    assert_eq!(report.targets.len(), 1);
    assert!(report.targets[0].key.anchor[0] < 6);
    assert_eq!(report.targets[0].confidence, Confidence::Supported);
    assert_eq!(report.targets[0].support.columns, 36);
    assert_eq!(report.targets, survey(&chunks, bounds([0, 0])).targets);
}

#[test]
fn signed_block_domain_extremes_do_not_wrap_spatial_evidence() {
    for x in [i32::MIN.div_euclid(16), i32::MAX.div_euclid(16)] {
        let chunks = [decoded(2835, [x, -1], &palette(), |a, y, b| {
            scene(Category::OpenGrassland, a, y, b)
        })];
        let report = survey(&chunks, bounds([x, -1]));
        assert!(has(&report, Category::OpenGrassland));
        assert_eq!(report.targets[0].key.tile, [x, -1]);
    }
}

fn constructed_chunks(base: [i32; 2], variant: u8) -> Vec<chunk::DecodedChunk> {
    let seam = [(base[0] + 1) * 16, (base[1] + 1) * 16];
    let mut chunks = Vec::new();
    for z in base[1]..=base[1] + 1 {
        for x in base[0]..=base[0] + 1 {
            chunks.push(decoded(3218, [x, z], &palette(), |local_x, y, local_z| {
                let gx = x * 16 + local_x;
                let gz = z * 16 + local_z;
                let deck = (seam[0] - 3..=seam[0] + 2).contains(&gx)
                    && (seam[1] - 3..=seam[1] + 2).contains(&gz);
                if variant == 4 && deck && y == 90 {
                    return 9;
                }
                if (gx, gz) == (seam[0] - 1, seam[1] - 1) {
                    if y == 66 {
                        return if variant == 1 { 0 } else { 34 };
                    }
                    if y == 65 {
                        return if variant == 5 { 35 } else { 33 };
                    }
                }
                if variant != 1 && variant != 5 && y == 65 && (gx, gz) == (seam[0], seam[1]) {
                    return 37;
                }
                if variant != 1 && variant != 5 && y == 65 && (gx, gz) == (seam[0] + 1, seam[1]) {
                    return 38;
                }
                if deck && y == 64 {
                    return match variant {
                        2 => 4,                       // natural logs alone do not corroborate a paired door
                        3 if (gx + gz) % 2 != 0 => 0, // disconnected ornament
                        _ => {
                            if (gx + gz) % 5 == 0 {
                                36
                            } else {
                                32
                            }
                        }
                    };
                }
                0
            }));
        }
    }
    chunks
}

fn constructed_report(chunks: &[chunk::DecodedChunk], base: [i32; 2]) -> Report<'_> {
    let core = surface::Bounds {
        minimum: base.map(|coordinate| coordinate * 16),
        maximum: base.map(|coordinate| (coordinate + 2) * 16 - 1),
    };
    survey(chunks, core)
}

#[test]
fn paired_wooden_evidence_survives_positive_and_negative_chunk_seams() {
    for base in [[0, 0], [-1, -1]] {
        let chunks = constructed_chunks(base, 0);
        let report = constructed_report(&chunks, base);
        let target = report
            .targets
            .iter()
            .find(|target| target.key.category == Category::DwellingLikeConstruction)
            .unwrap();
        let seam = [(base[0] + 1) * 16, (base[1] + 1) * 16];
        assert_eq!(target.key.anchor, [seam[0] - 1, 65, seam[1] - 1]);
        assert_eq!(target.key.tile, [base[0], base[1]]);
        assert_eq!(target.support.origin, [seam[0] - 8, seam[1] - 8]);
        assert!(target.support.columns >= 36);
        assert_eq!(report.stats.construction_windows, 9);
        assert_eq!(target.confidence, Confidence::Corroborated);
        for witness in [target.anchor, target.corroboration] {
            let chunk = chunks
                .iter()
                .find(|chunk| {
                    chunk.identity.position
                        == [
                            witness.block.position[0].div_euclid(16),
                            witness.block.position[2].div_euclid(16),
                        ]
                })
                .unwrap();
            let chunk::BlockSample::State(original) = chunk.block_at(witness.block.position) else {
                panic!()
            };
            assert!(std::ptr::eq(original, witness.block.state));
        }
    }
}

#[test]
fn paired_object_needs_connected_construction_and_complete_valid_halves() {
    for variant in [1, 2, 3, 4, 5] {
        let chunks = constructed_chunks([0, 0], variant);
        let report = constructed_report(&chunks, [0, 0]);
        assert!(
            !has(&report, Category::DwellingLikeConstruction),
            "variant {variant}"
        );
    }
}

#[test]
fn constructed_scan_is_bounded_and_transactional() {
    let chunks = constructed_chunks([0, 0], 0);
    let core = surface::Bounds {
        minimum: [0, 0],
        maximum: [31, 31],
    };
    let view = SurfaceWindow::overworld(
        core,
        0,
        &chunks,
        surface::Limits::default(),
        &mut Work::new(100, &|| false),
    )
    .unwrap();
    let limits = Limits {
        max_construction_windows: 8,
        ..Limits::default()
    };
    assert!(matches!(
        analyze(
            &view,
            source(),
            limits,
            &mut Work::new(1_000_000, &|| false)
        ),
        Err(Error::Limit("construction windows"))
    ));
    let stopped = Cell::new(false);
    let cancel = || stopped.get();
    let mut work = Work::new(1_000_000, &cancel);
    let seen = Cell::new(0);
    let cancellation = || {
        seen.set(seen.get() + 1);
        seen.get() > 2000
    };
    assert!(matches!(
        analyze(
            &view,
            source(),
            Limits::default(),
            &mut Work::new(1_000_000, &cancellation)
        ),
        Err(Error::Surface(surface::Error::Cancelled))
    ));
    stopped.set(true);
    assert!(matches!(
        analyze(&view, source(), Limits::default(), &mut work),
        Err(Error::Surface(surface::Error::Cancelled))
    ));
}

fn roof_fixture_with_axis(detached: bool, axis: &str) -> Vec<chunk::DecodedChunk> {
    let mut states = palette();
    states.push(state("minecraft:spruce_log", &[("axis", axis)]));
    let roof_state = states.len() - 1;
    vec![decoded(3218, [0, 0], &states, |x, y, z| {
        if !(3..=7).contains(&x) || !(3..=7).contains(&z) {
            return 0;
        }
        let roof_y = if detached { 90 } else { 86 - (x - 5).abs() };
        if y == roof_y {
            return roof_state;
        }
        if y == 80 {
            return 32;
        }
        if y == 81 && (x, z) == (4, 5) {
            return 37;
        }
        if y == 81 && (x, z) == (5, 5) {
            return 38;
        }
        let original_roof_y = 86 - (x - 5).abs();
        if (x == 3 || x == 7 || z == 3 || z == 7) && (81..original_roof_y).contains(&y) {
            return 14;
        }
        0
    })]
}
fn roof_fixture(detached: bool) -> Vec<chunk::DecodedChunk> {
    roof_fixture_with_axis(detached, "z")
}
#[test]
fn attached_horizontal_roof_ridge_is_inside_classified_dwelling_bounds() {
    let chunks = roof_fixture(false);
    let report = survey(&chunks, bounds([0, 0]));
    let target = report
        .targets
        .iter()
        .find(|t| t.key.category == Category::DwellingLikeConstruction)
        .unwrap();
    assert_eq!(target.key.anchor, [4, 81, 5]);
    assert_eq!(target.support.columns, 25);
    assert_eq!(
        target.support.maximum[1], 86,
        "Connected native timber ridge must belong to the displayed construction bounds"
    );
}
#[test]
fn disconnected_horizontal_logs_do_not_expand_dwelling_bounds() {
    let chunks = roof_fixture(true);
    let report = survey(&chunks, bounds([0, 0]));
    let target = report
        .targets
        .iter()
        .find(|t| t.key.category == Category::DwellingLikeConstruction)
        .unwrap();
    assert_eq!(target.key.anchor, [4, 81, 5]);
    assert_eq!(target.support.columns, 25);
    assert_eq!(target.support.maximum[1], 85);
}

#[test]
fn vertical_timber_does_not_expand_dwelling_bounds() {
    let chunks = roof_fixture_with_axis(false, "y");
    let report = survey(&chunks, bounds([0, 0]));
    let target = report
        .targets
        .iter()
        .find(|target| target.key.category == Category::DwellingLikeConstruction)
        .unwrap();
    assert_eq!(target.support.maximum[1], 85);
}
#[test]
fn malformed_timber_does_not_expand_dwelling_bounds() {
    let chunks = roof_fixture_with_axis(false, "diagonal");
    let report = survey(&chunks, bounds([0, 0]));
    let target = report
        .targets
        .iter()
        .find(|target| target.key.category == Category::DwellingLikeConstruction)
        .unwrap();
    assert_eq!(target.support.maximum[1], 85);
}
