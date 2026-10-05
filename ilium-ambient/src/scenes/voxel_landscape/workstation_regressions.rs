//! Private workstation regressions. Diagnostic fixtures never establish selected-pack pixels.
use super::super::{
    assets::{
        bank::TextureBankBuilder,
        layers::{LayeredPack, MountLayout},
        models::{oriented_quad, DefinitionInput, DefinitionProvider, Direction, ModelQuad},
        review::{fixture_origin, fixture_review},
        source::{LocalTree, SourceLimits},
        texture::{fixture_texture, Encoding},
    },
    surface_state_geometry as geometry,
    surface_village_assembly::workstation_cases,
};
use super::*;
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

const PACKET_SHA: &str = "498480766813eab5c1ae1cea3ae2a18770611583ef823055b6e3cfb28fe6df27";
const IDS: [&str; 13] = [
    "blast_furnace",
    "smoker",
    "cartography_table",
    "brewing_stand",
    "composter",
    "barrel",
    "fletching_table",
    "cauldron",
    "lectern",
    "stonecutter",
    "loom",
    "smithing_table",
    "grindstone",
];
const MISSING: [&str; 9] = [
    "blast_furnace",
    "smoker",
    "cartography_table",
    "brewing_stand",
    "lectern",
    "stonecutter",
    "loom",
    "smithing_table",
    "grindstone",
];
const FACES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const FACINGS: [&str; 4] = ["north", "east", "south", "west"];
const DEDICATED: [&str; 13] = [
    "front front_on side top",
    "bottom front front_on side top",
    "side1 side2 side3 top",
    "@ base",
    "bottom compost ready side top",
    "bottom side top top_open",
    "front side top",
    "bottom inner side top",
    "base front sides top",
    "bottom saw side top",
    "bottom front side top",
    "bottom front side top",
    "pivot round side",
];

fn id(name: &str) -> ResourceId {
    ResourceId::parse(name).unwrap()
}
fn state(block: &str, properties: &[(&str, &str)]) -> BlockState {
    BlockState::new(
        id(&format!("minecraft:{block}")),
        properties
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap()
}
fn packet() -> String {
    let path = std::env::var_os("ILIUM_WORKSTATION_PACKET")
        .expect("set ILIUM_WORKSTATION_PACKET to the captured prompt.md");
    let bytes = fs::read(path).unwrap();
    assert_eq!(bytes.len(), 915_488);
    assert_eq!(Digest256::of(&bytes).to_string(), PACKET_SHA);
    String::from_utf8(bytes).unwrap()
}
fn inventory() -> Value {
    let text = packet();
    let body = text
        .split_once("## Exact selected-pack member inventory\n```json\n")
        .unwrap()
        .1;
    serde_json::from_str(body.split_once("\n```").unwrap().0).unwrap()
}
fn definitions(budget: &ByteBudget, cancel: Cancel<'_>, generated: bool) -> DefinitionSet {
    let mut result = DefinitionSet::new(
        fixture_origin(OriginKind::OriginalCompatibilityGeometry),
        Limits::default(),
        budget.clone(),
    )
    .unwrap();
    result.install_geometry_templates(cancel).unwrap();
    for block in IDS {
        add_compatibility(
            &mut result,
            &id(&format!("minecraft:{block}")),
            false,
            false,
            generated,
            cancel,
        )
        .unwrap();
    }
    result
}
fn matrix() -> Vec<BlockState> {
    let mut result = Vec::new();
    for facing in FACINGS {
        for lit in ["false", "true"] {
            for block in ["blast_furnace", "smoker"] {
                result.push(state(block, &[("facing", facing), ("lit", lit)]));
            }
        }
        for book in ["false", "true"] {
            for powered in ["false", "true"] {
                result.push(state(
                    "lectern",
                    &[("facing", facing), ("has_book", book), ("powered", powered)],
                ));
            }
        }
        for block in ["loom", "stonecutter"] {
            result.push(state(block, &[("facing", facing)]));
        }
        for face in ["floor", "wall", "ceiling"] {
            result.push(state("grindstone", &[("facing", facing), ("face", face)]));
        }
    }
    for facing in ["up", "down", "north", "east", "south", "west"] {
        for open in ["false", "true"] {
            result.push(state("barrel", &[("facing", facing), ("open", open)]));
        }
    }
    for level in 0..=8 {
        result.push(state("composter", &[("level", &level.to_string())]));
    }
    for mask in 0..8 {
        let flag = |bit| {
            if mask & (1 << bit) == 0 {
                "false"
            } else {
                "true"
            }
        };
        result.push(state(
            "brewing_stand",
            &[
                ("has_bottle_0", flag(0)),
                ("has_bottle_1", flag(1)),
                ("has_bottle_2", flag(2)),
            ],
        ));
    }
    for block in [
        "cartography_table",
        "smithing_table",
        "fletching_table",
        "cauldron",
    ] {
        result.push(state(block, &[]));
    }
    assert_eq!(result.iter().collect::<BTreeSet<_>>().len(), 85);
    result
}
fn unsupported(state: &BlockState, native_bottles: bool) -> bool {
    state.property("has_book") == Some("true")
        || (!native_bottles
            && (0..3).any(|slot| state.property(&format!("has_bottle_{slot}")) == Some("true")))
}
fn textures(value: &NormalizedState) -> BTreeSet<String> {
    value
        .applications
        .iter()
        .flat_map(|(_, model)| &model.quads)
        .map(|quad| quad.texture.to_string())
        .collect()
}
fn oriented(value: &NormalizedState) -> Vec<ModelQuad> {
    value
        .applications
        .iter()
        .flat_map(|(application, model)| {
            model
                .quads
                .iter()
                .map(move |quad| oriented_quad(quad, application).unwrap())
        })
        .collect()
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}
fn base_model(block: &str) -> Value {
    geometry::definitions(&format!("minecraft:{block}"))
        .unwrap()
        .models[&format!("minecraft:block/{block}")]
        .clone()
}

macro_rules! force {
    ($test:ident, $block:literal, $models:literal) => {
        #[test]
        fn $test() {
            let value = geometry::definitions(concat!("minecraft:", $block));
            println!("{}", json!({"type":"workstation-forcing","id":concat!("minecraft:",$block),"authored":value.is_some()}));
            let value = value.expect("missing authored workstation factory");
            assert_eq!(value.models.len(), $models);
        }
    };
}
force!(forcing_blast_furnace, "blast_furnace", 2);
force!(forcing_smoker, "smoker", 2);
force!(forcing_cartography_table, "cartography_table", 1);
force!(forcing_brewing_stand, "brewing_stand", 4);
force!(forcing_composter, "composter", 9);
force!(forcing_barrel, "barrel", 2);
force!(forcing_fletching_table, "fletching_table", 1);
force!(forcing_cauldron, "cauldron", 1);
force!(forcing_lectern, "lectern", 1);
force!(forcing_stonecutter, "stonecutter", 1);
force!(forcing_loom, "loom", 1);
force!(forcing_smithing_table, "smithing_table", 1);
force!(forcing_grindstone, "grindstone", 1);

#[test]
fn actual_kit_placement_produces_260_workplaces_300_cells_and_31_states() {
    let cases = workstation_cases();
    let mut counts = BTreeMap::new();
    let mut states = BTreeSet::new();
    for case in &cases {
        *counts
            .entry((case.style, case.profession, case.turns))
            .or_insert(0) += 1;
        assert!(!case.source.is_empty());
        assert_eq!(case.state.id().as_str(), case.profession);
        states.insert(case.state.clone());
    }
    assert_eq!(counts.len(), 260);
    assert_eq!(cases.len(), 300);
    assert_eq!(states.len(), 31);
    for ((_, job, _), count) in counts {
        assert_eq!(count, if job == "minecraft:barrel" { 3 } else { 1 });
    }
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(64 << 20).unwrap();
    let defs = definitions(&budget, cancel, true);
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    for case in cases {
        let value = compiler
            .compile_state(&case.state, case.position, 97, cancel)
            .unwrap();
        assert!(!oriented(&value).is_empty(), "{}", case.state.canonical());
    }
    let per_id: BTreeMap<_, _> = IDS
        .into_iter()
        .map(|block| {
            (
                block,
                states
                    .iter()
                    .filter(|state| state.id().parts().1 == block)
                    .count(),
            )
        })
        .collect();
    for block in IDS {
        assert_eq!(
            per_id[block],
            if [
                "blast_furnace",
                "smoker",
                "lectern",
                "stonecutter",
                "loom",
                "grindstone"
            ]
            .contains(&block)
            {
                4
            } else {
                1
            }
        );
    }
    drop(compiler);
    drop(defs);
    assert_eq!(budget.used(), 0);
}

#[test]
fn all_85_inputs_have_exact_supported_or_unsupported_outcomes() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(64 << 20).unwrap();
    let defs = definitions(&budget, cancel, true);
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    let mut good = 0;
    for state in matrix() {
        let compiled = compiler.compile_state(&state, [0; 3], 0, cancel);
        if unsupported(&state, false) {
            assert!(
                matches!(compiled, Err(AssetError::InvalidMetadata(_))),
                "{} must not become empty geometry",
                state.canonical()
            );
            continue;
        }
        let compiled = compiled.unwrap();
        good += 1;
        let quads = oriented(&compiled);
        assert!(!quads.is_empty());
        for quad in quads {
            assert!(quad
                .points
                .iter()
                .flatten()
                .all(|p| p.is_finite() && (-1e-7..=1.0000001).contains(p)));
            assert!(quad
                .uv
                .iter()
                .flatten()
                .all(|v| v.is_finite() && (-1e-7..=1.0000001).contains(v)));
            close(quad.normal.iter().map(|v| f64::from(*v).powi(2)).sum(), 1.0);
        }
    }
    assert_eq!(good, 70);
    drop(compiler);
    drop(defs);
    assert_eq!(budget.used(), 0);
}

#[test]
fn whole_face_materials_are_semantic_not_all_in_one_textures() {
    for (block, expected) in [
        (
            "blast_furnace",
            [
                "blast_furnace_top",
                "blast_furnace_top",
                "blast_furnace_front",
                "blast_furnace_side",
                "blast_furnace_side",
                "blast_furnace_side",
            ],
        ),
        (
            "smoker",
            [
                "smoker_bottom",
                "smoker_top",
                "smoker_front",
                "smoker_side",
                "smoker_side",
                "smoker_side",
            ],
        ),
        (
            "cartography_table",
            [
                "dark_oak_planks",
                "cartography_table_top",
                "cartography_table_side1",
                "cartography_table_side3",
                "cartography_table_side2",
                "cartography_table_side3",
            ],
        ),
        (
            "loom",
            [
                "loom_bottom",
                "loom_top",
                "loom_front",
                "loom_side",
                "loom_side",
                "loom_side",
            ],
        ),
        (
            "smithing_table",
            [
                "smithing_table_bottom",
                "smithing_table_top",
                "smithing_table_front",
                "smithing_table_front",
                "smithing_table_side",
                "smithing_table_side",
            ],
        ),
    ] {
        let model = base_model(block);
        assert_eq!(model["elements"].as_array().unwrap().len(), 1);
        assert_eq!(model["elements"][0]["from"], json!([0, 0, 0]));
        assert_eq!(model["elements"][0]["to"], json!([16, 16, 16]));
        for (face, texture) in FACES.into_iter().zip(expected) {
            assert_eq!(
                model["elements"][0]["faces"][face]["texture"],
                format!("minecraft:block/{texture}")
            );
            assert_eq!(
                model["elements"][0]["faces"][face]["uv"],
                json!([0, 0, 16, 16])
            );
        }
    }
    for block in ["blast_furnace", "smoker"] {
        let defs = geometry::definitions(&format!("minecraft:{block}")).unwrap();
        let lit = &defs.models[&format!("minecraft:block/{block}_on")];
        let dark = &defs.models[&format!("minecraft:block/{block}")];
        for face in FACES {
            if face == "north" {
                assert_eq!(
                    lit["elements"][0]["faces"][face]["texture"],
                    format!("minecraft:block/{block}_front_on")
                );
            } else {
                assert_eq!(
                    lit["elements"][0]["faces"][face],
                    dark["elements"][0]["faces"][face]
                );
            }
        }
    }
}

fn numeric_uv(value: &serde_json::Value) -> [f64; 4] {
    let entries = value.as_array().expect("UV must remain a numeric array");
    assert_eq!(entries.len(), 4, "UV must retain four coordinates");
    std::array::from_fn(|index| {
        entries[index]
            .as_f64()
            .expect("UV coordinate must be numeric")
    })
}

#[test]
fn cropped_uvs_and_original_shape_parts_are_not_generic_cubes() {
    let brew = base_model("brewing_stand");
    let elements = brew["elements"].as_array().unwrap();
    assert_eq!(elements.len(), 4);
    assert_eq!(elements[0]["from"], json!([7, 0, 7]));
    assert_eq!(elements[0]["to"], json!([9, 14, 9]));
    for (index, top, side) in [
        (1, [8, 4, 14, 10], [9, 14, 15, 16]),
        (2, [2, 2, 8, 8], [2, 14, 8, 16]),
        (3, [2, 8, 8, 14], [2, 14, 8, 16]),
    ] {
        assert_eq!(elements[index]["to"][1], 2);
        assert_eq!(
            numeric_uv(&elements[index]["faces"]["up"]["uv"]),
            top.map(f64::from)
        );
        assert_eq!(
            numeric_uv(&elements[index]["faces"]["north"]["uv"]),
            side.map(f64::from)
        );
        assert_eq!(
            elements[index]["faces"]["up"]["texture"],
            "minecraft:block/brewing_stand_base"
        );
    }
    let defs = geometry::definitions("minecraft:brewing_stand").unwrap();
    for slot in 0..3 {
        let arm =
            &defs.models[&format!("minecraft:block/brewing_stand_empty{slot}")]["elements"][0];
        let (negative, positive) = if slot == 0 {
            ("west", "east")
        } else {
            ("north", "south")
        };
        assert_eq!(
            numeric_uv(&arm["faces"][negative]["uv"]),
            [6.0, 14.0, 8.0, 16.0]
        );
        assert_eq!(
            numeric_uv(&arm["faces"][positive]["uv"]),
            [6.0, 2.0, 8.0, 4.0]
        );
        assert_eq!(numeric_uv(&arm["faces"]["up"]["uv"]), [7.0, 2.0, 9.0, 16.0]);
        assert_eq!(
            arm["faces"]["up"]["rotation"].as_i64().unwrap_or(0),
            if slot == 0 { 90 } else { 0 }
        );
        assert!(!defs
            .models
            .contains_key(&format!("minecraft:block/brewing_stand_bottle{slot}")));
    }
    let lectern = base_model("lectern");
    assert_eq!(lectern["elements"].as_array().unwrap().len(), 3);
    assert_eq!(lectern["elements"][1]["from"], json!([5, 2, 6]));
    assert_eq!(lectern["elements"][1]["to"], json!([11, 12, 10]));
    assert_eq!(
        lectern["elements"][2]["rotation"],
        json!({"origin":[8,12,8],"axis":"x","angle":-22.5,"rescale":false})
    );
    assert_eq!(
        numeric_uv(&lectern["elements"][2]["faces"]["up"]["uv"]),
        [1.0, 0.0, 15.0, 12.0]
    );
    assert_eq!(lectern["elements"][2]["faces"]["up"]["rotation"], 180);
    assert_eq!(
        numeric_uv(&lectern["elements"][1]["faces"]["west"]["uv"]),
        [2.0, 8.0, 12.0, 12.0]
    );
    let saw = base_model("stonecutter");
    assert_eq!(saw["elements"][0]["to"], json!([16, 8, 16]));
    assert_eq!(
        numeric_uv(&saw["elements"][0]["faces"]["east"]["uv"]),
        [0.0, 8.0, 16.0, 16.0]
    );
    assert_eq!(saw["elements"][1]["from"], json!([1, 8, 8]));
    assert_eq!(saw["elements"][1]["to"], json!([15, 16, 8]));
    assert_eq!(
        numeric_uv(&saw["elements"][1]["faces"]["north"]["uv"]),
        [1.0, 8.0, 15.0, 16.0]
    );
    assert_eq!(
        numeric_uv(&saw["elements"][1]["faces"]["south"]["uv"]),
        [15.0, 8.0, 1.0, 16.0]
    );
    let grind = base_model("grindstone");
    assert_eq!(grind["elements"].as_array().unwrap().len(), 7);
    for (index, uv) in [(4, [3, 10, 9, 12]), (5, [0, 2, 12, 10]), (6, [3, 0, 9, 2])] {
        assert_eq!(
            grind["elements"][index]["faces"]["east"]["texture"],
            "minecraft:block/grindstone_side"
        );
        assert_eq!(
            numeric_uv(&grind["elements"][index]["faces"]["east"]["uv"]),
            uv.map(f64::from)
        );
        assert_eq!(
            grind["elements"][index]["faces"]["north"]["texture"],
            "minecraft:block/grindstone_round"
        );
    }
    assert!(grind["elements"][4]["faces"].get("up").is_none());
    assert!(grind["elements"][6]["faces"].get("down").is_none());
    for index in [0, 1] {
        assert_eq!(
            grind["elements"][index]["faces"]["north"]["texture"],
            "minecraft:block/stripped_spruce_log"
        );
    }
}

#[test]
fn compiler_rotates_fronts_slopes_blades_and_grindstone_supports_together() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(64 << 20).unwrap();
    let defs = definitions(&budget, cancel, true);
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    for (turn, facing) in FACINGS.into_iter().enumerate() {
        let normal = [
            [0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [-1.0, 0.0, 0.0],
        ][turn];
        for block in ["blast_furnace", "smoker", "loom"] {
            let props = if block == "loom" {
                vec![("facing", facing)]
            } else {
                vec![("facing", facing), ("lit", "false")]
            };
            let value = compiler
                .compile_state(&state(block, &props), [0; 3], 0, cancel)
                .unwrap();
            let front = oriented(&value)
                .into_iter()
                .find(|q| q.texture.as_str() == format!("minecraft:block/{block}_front"))
                .unwrap();
            for (actual, expected) in front.normal.into_iter().zip(normal) {
                close(f64::from(actual), expected);
            }
        }
        let lectern = compiler
            .compile_state(
                &state("lectern", &[("facing", facing), ("has_book", "false")]),
                [0; 3],
                0,
                cancel,
            )
            .unwrap();
        let (application, model) = &lectern.applications[0];
        assert!(!application.uvlock);
        let top = model
            .quads
            .iter()
            .find(|q| q.texture.as_str() == "minecraft:block/lectern_top")
            .unwrap();
        let turned = oriented_quad(top, application).unwrap();
        assert_eq!(top.uv, turned.uv);
        close(f64::from(turned.normal[2]), 22.5_f64.to_radians().cos());
        close(
            f64::from(turned.normal[0]).hypot(f64::from(turned.normal[1])),
            22.5_f64.to_radians().sin(),
        );
        let cutter = compiler
            .compile_state(
                &state("stonecutter", &[("facing", facing)]),
                [0; 3],
                0,
                cancel,
            )
            .unwrap();
        let blades: Vec<_> = oriented(&cutter)
            .into_iter()
            .filter(|q| q.texture.as_str() == "minecraft:block/stonecutter_saw")
            .collect();
        assert_eq!(blades.len(), 2);
        for axis in 0..3 {
            close(
                f64::from(blades[0].normal[axis]),
                -f64::from(blades[1].normal[axis]),
            );
        }
        for (face, expected) in [
            ("floor", [0.0, 0.0, -1.0]),
            ("ceiling", [0.0, 0.0, 1.0]),
            ("wall", normal.map(|v| -v)),
        ] {
            let value = compiler
                .compile_state(
                    &state("grindstone", &[("facing", facing), ("face", face)]),
                    [0; 3],
                    0,
                    cancel,
                )
                .unwrap();
            assert_eq!(oriented(&value).len(), 40);
            let foot = oriented(&value)
                .into_iter()
                .find(|q| q.element == 0 && q.face == Direction::Down)
                .unwrap();
            for (actual, expected) in foot.normal.into_iter().zip(expected) {
                close(f64::from(actual), expected);
            }
            let axis = expected.iter().position(|v| v.abs() > 0.5).unwrap();
            for point in foot.points {
                close(point[axis], if expected[axis] < 0.0 { 0.0 } else { 1.0 });
            }
        }
    }
}

fn write_member(root: &Path, path: &str, bytes: &[u8]) {
    assert!(!Path::new(path).is_absolute());
    assert!(!Path::new(path)
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir)));
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .unwrap();
    file.write_all(bytes).unwrap();
}
fn mounted_fixture(root: &Path, budget: &ByteBudget, cancel: Cancel<'_>) -> LayeredPack {
    let source = LocalTree::open(
        root,
        BTreeMap::new(),
        SourceLimits::default(),
        budget,
        cancel,
    )
    .unwrap();
    LayeredPack::mount(
        fixture_review(),
        Arc::new(source),
        None,
        MountLayout::Java,
        OriginKind::DiagnosticFixture,
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap()
}
fn native_model_fixture(
    profile: &Value,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> (tempfile::TempDir, Option<LayeredPack>) {
    let root = tempfile::tempdir().unwrap();
    let block_models: Vec<_> = profile["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|member| {
            member["path"]
                .as_str()
                .is_some_and(|path| path.starts_with("assets/minecraft/models/block/"))
        })
        .collect();
    if block_models.is_empty() {
        return (root, None);
    }
    for member in block_models {
        let path = member["path"].as_str().unwrap();
        let bytes = member["utf8_content"].as_str().unwrap().as_bytes();
        assert_eq!(
            Digest256::of(bytes).to_string(),
            member["sha256"].as_str().unwrap()
        );
        write_member(root.path(), path, bytes);
    }
    assert!(!root.path().join("assets/minecraft/blockstates").exists());
    let mounted = mounted_fixture(root.path(), budget, cancel);
    (root, Some(mounted))
}

#[test]
fn canonical_selected_model_only_precedence_covers_all_nine_and_saved() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(64 << 20).unwrap();
    let root = tempfile::tempdir().unwrap();
    let marker = serde_json::to_vec(&json!({"elements":[{"from":[0,0,0],"to":[2,3,4],"faces":{"north":{"texture":"diagnostic:block/marker"}}}]})).unwrap();
    for block in MISSING {
        write_member(
            root.path(),
            &format!("assets/minecraft/models/block/{block}.json"),
            &marker,
        );
    }
    let packs = [mounted_fixture(root.path(), &budget, cancel)];
    let states: BTreeSet<_> = workstation_cases()
        .into_iter()
        .map(|case| case.state)
        .collect();
    for generated in [false, true] {
        let defs = definitions(&budget, cancel, generated);
        let sources = DefinitionSources::new(&packs, &[], Some(&defs)).unwrap();
        let mut compiler = ModelCompiler::new(&sources, Limits::default(), budget.clone()).unwrap();
        for block in MISSING {
            assert!(!uses_generated_material_geometry(
                &id(&format!("minecraft:{block}")),
                !generated
            ));
            let state = states
                .iter()
                .find(|state| state.id().parts().1 == block)
                .unwrap();
            let value = compiler.compile_state(state, [0; 3], 0, cancel).unwrap();
            assert!(value.state_origin.compatibility.is_some());
            let (_, model) = value
                .applications
                .iter()
                .find(|(app, _)| app.model.as_str() == format!("minecraft:block/{block}"))
                .unwrap();
            assert_eq!(model.quads.len(), 1);
            assert_eq!(model.quads[0].texture.as_str(), "diagnostic:block/marker");
            assert_eq!(model.origins[0].sha256, Digest256::of(&marker));
            assert!(model.origins[0].compatibility.is_none());
            assert_eq!(model.origins[0].origin.kind, OriginKind::DiagnosticFixture);
        }
    }
    drop(packs);
    assert_eq!(budget.used(), 0);
}

#[test]
#[ignore = "requires the hash-pinned private workstation packet"]
fn captured_native_models_close_dependencies_for_all_eleven_profiles() {
    let input = inventory();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    for profile in input["profiles"].as_array().unwrap() {
        let name = profile["profile"].as_str().unwrap();
        let budget = ByteBudget::new(64 << 20).unwrap();
        let (root, pack) = native_model_fixture(profile, &budget, cancel);
        let packs: Vec<_> = pack.into_iter().collect();
        let defs = definitions(&budget, cancel, true);
        let sources = DefinitionSources::new(&packs, &[], Some(&defs)).unwrap();
        let mut compiler = ModelCompiler::new(&sources, Limits::default(), budget.clone()).unwrap();
        let mut original = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
        let mut dependencies = BTreeSet::new();
        let mut good = 0;
        for state in matrix() {
            let value = if uses_generated_material_geometry(state.id(), false) {
                original.compile_state(&state, [0; 3], 0, cancel)
            } else {
                compiler.compile_state(&state, [0; 3], 0, cancel)
            };
            if unsupported(&state, name == "whimscape") {
                assert!(
                    matches!(value, Err(AssetError::InvalidMetadata(_))),
                    "{name}: {}",
                    state.canonical()
                );
                continue;
            }
            let value =
                value.unwrap_or_else(|error| panic!("{name}: {}: {error}", state.canonical()));
            good += 1;
            dependencies.extend(textures(&value));
            for (application, model) in &value.applications {
                let member_path = format!(
                    "assets/minecraft/models/{}.json",
                    application.model.parts().1
                );
                let native = profile["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|member| member["path"].as_str() == Some(&member_path));
                if let Some(native) = native {
                    let origin = model
                        .origins
                        .iter()
                        .find(|origin| origin.resource.id == application.model)
                        .unwrap();
                    assert!(origin.compatibility.is_none(), "{name}: {member_path}");
                    assert_eq!(
                        origin.sha256.to_string(),
                        native["sha256"].as_str().unwrap()
                    );
                }
            }
            if name == "whimscape" && state.id().parts().1 == "brewing_stand" {
                assert_eq!(value.applications.len(), 4);
                assert_eq!(
                    textures(&value),
                    BTreeSet::from([
                        "minecraft:block/brewing_stand".to_owned(),
                        "minecraft:block/brewing_stand_rod".to_owned()
                    ])
                );
                for slot in 0..3 {
                    let suffix = if state.property(&format!("has_bottle_{slot}")) == Some("true") {
                        "bottle"
                    } else {
                        "empty"
                    };
                    let companion = format!("minecraft:block/brewing_stand_{suffix}{slot}");
                    assert!(value
                        .applications
                        .iter()
                        .any(|(app, model)| app.model.as_str() == companion
                            && model
                                .origins
                                .iter()
                                .any(|origin| origin.resource.id == app.model
                                    && origin.compatibility.is_none())));
                }
            }
        }
        assert_eq!(good, if name == "whimscape" { 77 } else { 70 });
        if name == "whimscape" {
            assert!(!dependencies.contains("minecraft:block/brewing_stand_base"));
            assert!(dependencies.contains("minecraft:block/stonecutter_other_side"));
            assert!(dependencies.contains("minecraft:block/dark_oak_log"));
        }
        if name == "textureless" {
            assert!(dependencies.contains("minecraft:block/stripped_dark_oak_wood"));
            assert!(!dependencies
                .iter()
                .any(|value| value.ends_with("_inventory")));
        }
        drop(original);
        drop(compiler);
        drop(defs);
        drop(packs);
        drop(root);
        assert_eq!(budget.used(), 0, "{name}");
    }
}

#[test]
#[ignore = "requires the hash-pinned private workstation packet"]
fn filtered_member_inventory_distinguishes_absence_legacy_paths_and_extra_art() {
    let input = inventory();
    let profiles = input["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 11);
    let mut member_count = 0;
    let mut text_count = 0;
    for (profile, expected_members) in profiles
        .iter()
        .zip([63, 69, 88, 9, 8, 238, 52, 79, 62, 62, 162])
    {
        let name = profile["profile"].as_str().unwrap();
        let members = profile["members"].as_array().unwrap();
        assert_eq!(members.len(), expected_members, "{name}");
        member_count += members.len();
        let paths: BTreeSet<_> = members
            .iter()
            .map(|member| member["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths.len(), members.len());
        assert!(!paths.iter().any(|path| path.contains("/blockstates/")));
        for member in members {
            if let Some(text) = member["utf8_content"].as_str() {
                text_count += 1;
                assert_eq!(text.len() as u64, member["bytes"].as_u64().unwrap());
                assert_eq!(
                    Digest256::of(text.as_bytes()).to_string(),
                    member["sha256"].as_str().unwrap()
                );
            }
        }
        let root = profile["mount"]["root"].as_str().unwrap();
        let prefix = if root.is_empty() {
            String::new()
        } else {
            format!("{root}/")
        };
        let mut found = 0;
        for (block, suffixes) in IDS.into_iter().zip(DEDICATED) {
            for suffix in suffixes.split_whitespace() {
                let name = if suffix == "@" {
                    block.to_owned()
                } else {
                    format!("{block}_{suffix}")
                };
                let candidates = [
                    format!("{prefix}assets/minecraft/textures/block/{name}.png"),
                    format!("{prefix}assets/minecraft/textures/blocks/{name}.png"),
                    format!("{prefix}textures/block/{name}.png"),
                ];
                found += usize::from(
                    candidates
                        .iter()
                        .any(|candidate| paths.contains(candidate.as_str())),
                );
            }
        }
        assert_eq!(
            found,
            match name {
                "goodvibes" | "programmerart" => 6,
                "whimscape" => 49,
                "textureless" => 44,
                _ => 50,
            },
            "{name}"
        );
    }
    assert_eq!(member_count, 892);
    assert_eq!(text_count, 65);
    assert_eq!(
        DEDICATED
            .iter()
            .map(|row| row.split_whitespace().count())
            .sum::<usize>(),
        50
    );
}

#[test]
fn malformed_selected_model_and_missing_occupied_art_do_not_become_success() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(64 << 20).unwrap();
    let root = tempfile::tempdir().unwrap();
    write_member(
        root.path(),
        "assets/minecraft/models/block/smoker.json",
        b"{broken",
    );
    let packs = [mounted_fixture(root.path(), &budget, cancel)];
    let defs = definitions(&budget, cancel, true);
    let sources = DefinitionSources::new(&packs, &[], Some(&defs)).unwrap();
    let mut compiler = ModelCompiler::new(&sources, Limits::default(), budget.clone()).unwrap();
    let error = compiler
        .compile_state(
            &state("smoker", &[("facing", "north"), ("lit", "false")]),
            [0; 3],
            0,
            cancel,
        )
        .unwrap_err();
    assert!(matches!(error, AssetError::InvalidMetadata(_)));
    assert!(!geometry_recovery_allowed(&error, false, "whimscape"));
    assert!(geometry_recovery_allowed(&error, true, "whimscape"));
    for error in [
        AssetError::Cancelled,
        AssetError::Allocation,
        AssetError::Limit {
            resource: "working bytes",
            requested: 2,
            limit: 1,
        },
    ] {
        assert!(!geometry_recovery_allowed(&error, true, "jicklus"));
        assert!(!geometry_recovery_allowed(&error, false, "jicklus"));
    }
}

#[test]
fn new_material_requests_preserve_alias_order_and_explicit_fallback_only() {
    for profile in pack_profiles::FULL_PACKS {
        let aliases = explicit_aliases(profile.id).unwrap();
        let selected = profile.review().unwrap().pack;
        let fallback = id("diagnostic:fallback");
        for (block, suffixes) in IDS.into_iter().zip(DEDICATED) {
            for suffix in suffixes.split_whitespace() {
                let path = if suffix == "@" {
                    block.to_owned()
                } else {
                    format!("{block}_{suffix}")
                };
                let resource = id(&format!("minecraft:block/{path}"));
                let only = request(
                    resource.clone(),
                    &selected,
                    &aliases,
                    profile.id == "goodvibes",
                    None,
                    false,
                )
                .unwrap();
                let with = request(
                    resource.clone(),
                    &selected,
                    &aliases,
                    profile.id == "goodvibes",
                    Some(&fallback),
                    false,
                )
                .unwrap();
                assert_eq!(only.requirement.origin, RequiredOrigin::SelectedPack);
                assert_eq!(
                    with.requirement.origin,
                    RequiredOrigin::SelectedOrExplicitFullPackFallback
                );
                assert!(only
                    .candidates
                    .iter()
                    .all(|candidate| candidate.pack == selected));
                let at = with
                    .candidates
                    .iter()
                    .position(|candidate| candidate.pack == fallback)
                    .unwrap();
                assert!(with.candidates[..at]
                    .iter()
                    .all(|candidate| candidate.pack == selected));
                assert!(
                    matches!(&with.candidates[at].location, TextureLocation::Resource { id, alias_reason: Some(_) } if id == &resource)
                );
                for candidate in &with.candidates {
                    if let TextureLocation::Resource { id, alias_reason } = &candidate.location {
                        if id != &resource {
                            assert_eq!(resource.as_str(), "minecraft:block/composter_bottom");
                            assert_eq!(id.as_str(), "minecraft:block/composter_side");
                            assert!(alias_reason.is_some());
                        }
                    }
                    assert!(matches!(
                        &candidate.schedule,
                        ScheduleSource::AutomaticJava | ScheduleSource::NoMetadata
                    ));
                }
                if resource.as_str() == "minecraft:block/composter_bottom" {
                    assert!(at + 1 < with.candidates.len());
                }
            }
        }
    }
}

#[test]
fn current_geometry_bytes_and_generated_prop_selection_are_preserved() {
    let current = include_str!("surface_state_geometry.rs");
    let before = current.split_once("// Canonical fallback names preserve selected model-only overrides as well as blockstates.\n").unwrap().0;
    let after = current
        .split_once("pub fn definitions(id: &str) -> Option<StateGeometry> {")
        .unwrap()
        .1;
    let dispatch = "    if let Some(workstation) = workstation_definitions(block) { // Resolve only the nine new canonical workstation factories.\n        return Some(workstation); // Preserve all other definition branches byte-for-byte.\n    } // Keep the prior material and Saved selection policies in their existing consumers.\n";
    let restored = format!(
        "{before}pub fn definitions(id: &str) -> Option<StateGeometry> {{{}",
        after.replacen(dispatch, "", 1)
    );
    // The captured packet predates separately accepted flora geometry. Pin the
    // current non-workstation baseline and require workstation support to stay
    // behind this narrow dispatch.
    assert_eq!(
        Digest256::of(restored.as_bytes()).to_string(),
        "39903eb4bf3356f58fae7d99da868ac382c511311494d7c23f44007a37e5f45f"
    );
    for block in MISSING {
        assert!(!geometry::is_remaining_generated_material(&format!(
            "minecraft:{block}"
        )));
    }
    for block in [
        "cauldron",
        "composter",
        "chest",
        "bell",
        "campfire",
        "oak_fence_gate",
        "jungle_fence_gate",
        "acacia_fence_gate",
        "spruce_fence_gate",
    ] {
        assert!(uses_generated_material_geometry(
            &id(&format!("minecraft:{block}")),
            false
        ));
        assert!(!uses_generated_material_geometry(
            &id(&format!("minecraft:{block}")),
            true
        ));
    }
}

struct Superseding<'a> {
    source: &'a DefinitionSet,
    revision: &'a AtomicU64,
    calls: AtomicUsize,
}
impl DefinitionProvider for Superseding<'_> {
    fn definition(&self, key: &ResourceKey, cancel: Cancel<'_>) -> Result<Option<DefinitionInput>> {
        let value = self.source.definition(key, cancel)?;
        if self.calls.fetch_add(1, Ordering::AcqRel) == 1 {
            self.revision.fetch_add(1, Ordering::AcqRel);
        }
        Ok(value)
    }
}

#[test]
fn cancellation_supersession_and_budget_exhaustion_cannot_publish_a_workstation() {
    let stop = AtomicBool::new(false);
    let revision = AtomicU64::new(7);
    let cancel = Cancel::for_revision(&stop, &revision, 7);
    let budget = ByteBudget::new(64 << 20).unwrap();
    let defs = definitions(&budget, cancel, true);
    let baseline = budget.used();
    {
        let source = Superseding {
            source: &defs,
            revision: &revision,
            calls: AtomicUsize::new(0),
        };
        let mut compiler = ModelCompiler::new(&source, Limits::default(), budget.clone()).unwrap();
        assert!(matches!(
            compiler.compile_state(
                &state("smoker", &[("facing", "north"), ("lit", "false")]),
                [0; 3],
                0,
                cancel
            ),
            Err(AssetError::Cancelled)
        ));
        assert_eq!(source.calls.load(Ordering::Acquire), 2);
    }
    assert_eq!(budget.used(), baseline);
    revision.store(7, Ordering::Release);
    stop.store(true, Ordering::Release);
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    assert!(matches!(
        compiler.compile_state(&state("smithing_table", &[]), [0; 3], 0, cancel),
        Err(AssetError::Cancelled)
    ));
    stop.store(false, Ordering::Release);
    let retained = compiler
        .compile_state(&state("smithing_table", &[]), [0; 3], 0, cancel)
        .unwrap();
    let before = budget.used();
    let blocker = budget
        .clone()
        .reserve(budget.limit() - before, cancel)
        .unwrap();
    assert!(matches!(
        compiler.compile_state(&state("cartography_table", &[]), [0; 3], 0, cancel),
        Err(AssetError::Limit { .. })
    ));
    assert_eq!(budget.used(), budget.limit());
    drop(blocker);
    assert_eq!(budget.used(), before);
    assert!(retained.uses_budget(&budget));
    assert!(!retained.applications[0].1.quads.is_empty());
    drop(
        compiler
            .compile_state(&state("cartography_table", &[]), [0; 3], 0, cancel)
            .unwrap(),
    );
    let foreign = ByteBudget::new(64 << 20).unwrap();
    let mut wrong = ModelCompiler::new(&defs, Limits::default(), foreign.clone()).unwrap();
    assert!(matches!(
        wrong.compile_state(&state("smithing_table", &[]), [0; 3], 0, cancel),
        Err(AssetError::InvalidMetadata(_))
    ));
    assert_eq!(foreign.used(), 0);
    drop(wrong);
    drop(compiler);
    drop(defs);
    assert!(budget.used() > 0);
    drop(retained);
    assert_eq!(budget.used(), 0);
    let tiny = ByteBudget::new(1).unwrap();
    let mut empty = DefinitionSet::new(
        fixture_origin(OriginKind::DiagnosticFixture),
        Limits::default(),
        tiny.clone(),
    )
    .unwrap();
    assert!(matches!(
        add_compatibility(
            &mut empty,
            &id("minecraft:grindstone"),
            false,
            false,
            true,
            cancel
        ),
        Err(AssetError::Limit { .. })
    ));
    drop(empty);
    assert_eq!(tiny.used(), 0);
}

fn diagnostic_bank(
    value: &NormalizedState,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> TextureBank {
    let mut builder = TextureBankBuilder::new(
        fixture_review(),
        vec![],
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap();
    for name in textures(value) {
        builder
            .insert(
                id(&name),
                fixture_texture(
                    [1, 1],
                    &[127, 193, 61, 255],
                    None,
                    &MissingAnimation::StaticImage,
                    Encoding::SrgbColor,
                    fixture_origin(OriginKind::DiagnosticFixture),
                    budget,
                ),
                cancel,
            )
            .unwrap();
    }
    let bank = builder.finish(vec![], cancel).unwrap();
    assert!(!bank.coverage().required_textures_satisfied);
    bank
}
fn table(value: &NormalizedState, bank: &TextureBank) -> MaterialTable {
    let mut rules = BTreeMap::new();
    for name in textures(value) {
        let resource = id(&name);
        if let Some(texture) = bank
            .resolve(&resource)
            .and_then(|handle| bank.texture(handle))
        {
            rules.insert(
                resource.clone(),
                TextureRenderRule {
                    alpha: texture_alpha(resource.parts().1, texture.image().info().alpha_min),
                    layer: 0,
                    normal_map: None,
                    specular_map: None,
                },
            );
        }
    }
    MaterialTable {
        medium: None,
        rules,
        tints: BTreeMap::new(),
    }
}

#[test]
fn bound_mesh_keeps_exact_cell_owners_bank_and_last_storage_reservation() {
    use ilium_execution::{QuotaGroup, QuotaLimits};
    // Admit default decoder scratch separately from retained fixture/model bytes.
    let bytes = usize::try_from(Limits::default().decoder_scratch_bytes).unwrap() + (64 << 20);
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 0,
        worker_bytes: bytes,
    });
    let storage = Arc::new(quota.reserve_external_storage(bytes).unwrap());
    let budget = ByteBudget::with_storage(u64::try_from(bytes).unwrap(), storage).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let defs = definitions(&budget, cancel, true);
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    let value = compiler
        .compile_state(
            &state("grindstone", &[("face", "floor"), ("facing", "north")]),
            [0; 3],
            0,
            cancel,
        )
        .unwrap();
    let bank = diagnostic_bank(&value, &budget, cancel);
    let policy = table(&value, &bank);
    let empty = MaterialTable {
        medium: None,
        rules: BTreeMap::new(),
        tints: BTreeMap::new(),
    };
    assert!(matches!(
        BoundModel::bind(&value, &bank, &empty, &budget, cancel),
        Err(AssetError::InvalidMetadata(_))
    ));
    let wrong = ByteBudget::new(u64::try_from(bytes).unwrap()).unwrap();
    assert!(BoundModel::bind(&value, &bank, &policy, &wrong, cancel).is_err());
    let bound = Arc::new(BoundModel::bind(&value, &bank, &policy, &budget, cancel).unwrap());
    let instances = BTreeMap::from([
        ([0, 0, 64], Arc::clone(&bound)),
        ([2, 0, 64], Arc::clone(&bound)),
    ]);
    let region = MeshRegion {
        minimum: [0, 0],
        maximum: [4, 1],
    };
    let before = budget.used();
    assert!(matches!(
        PreparedMesh::build(&instances, region, bank.identity(), 1, &budget, cancel),
        Err(AssetError::Limit { .. })
    ));
    assert_eq!(budget.used(), before);
    let mesh =
        PreparedMesh::build(&instances, region, bank.identity(), 1000, &budget, cancel).unwrap();
    assert_eq!(mesh.faces.len(), 80);
    let owners: BTreeSet<_> = mesh.faces.iter().map(|face| face.owner).collect();
    assert_eq!(owners.len(), 80);
    assert_eq!(
        owners
            .iter()
            .map(|owner| owner.position)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([[0, 0, 64], [2, 0, 64]])
    );
    assert!(mesh.uses_budget(&budget));
    assert!(mesh
        .faces
        .iter()
        .all(|face| Arc::ptr_eq(&face.model, &bound)));
    stop.store(true, Ordering::Release);
    assert!(matches!(
        PreparedMesh::build(&instances, region, bank.identity(), 1000, &budget, cancel),
        Err(AssetError::Cancelled)
    ));
    drop(instances);
    drop(bound);
    drop(value);
    drop(compiler);
    drop(defs);
    drop(bank);
    drop(budget);
    assert_eq!(quota.snapshot().worker_bytes, bytes);
    assert!(quota.reserve_external_storage(1).is_err());
    drop(mesh);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn importer_retains_absence_corruption_alias_evidence_and_shared_account_failures() {
    use super::super::assets::pixels::fixture_png;
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    // Retained fixture allowance is separate from the default decoder scratch.
    let budget = ByteBudget::new(Limits::default().decoder_scratch_bytes + (64 << 20)).unwrap();
    let root = tempfile::tempdir().unwrap();
    write_member(
        root.path(),
        "assets/minecraft/textures/block/smoker_front.png",
        b"corrupt, not absent",
    );
    write_member(
        root.path(),
        "fixture.png",
        &fixture_png(1, 1, &[127, 193, 61, 255]),
    );
    let packs = [mounted_fixture(root.path(), &budget, cancel)];
    let importer = TextureImporter::new(&packs, Limits::default(), budget.clone()).unwrap();
    let selected = packs[0].review().pack.clone();
    let mut corrupt = request(
        id("minecraft:block/smoker_front"),
        &selected,
        &BTreeMap::new(),
        false,
        None,
        false,
    )
    .unwrap();
    corrupt.candidates.push(TextureCandidate {
        pack: selected.clone(),
        location: TextureLocation::Literal {
            path: AssetPath::parse("fixture.png").unwrap(),
            evidence: Label::new("diagnostic candidate, not selected artwork").unwrap(),
        },
        schedule: ScheduleSource::NoMetadata,
        expected_source_sha256: None,
        expected_image: ImageExpectations::default(),
    });
    let bad = importer.import(&[corrupt], cancel).unwrap();
    assert_eq!(bad.records[0].candidates.len(), 1);
    assert!(bad.records[0].failure.is_some());
    assert!(bad
        .bank
        .resolve(&id("minecraft:block/smoker_front"))
        .is_none());
    drop(bad);
    let absent = request(
        id("minecraft:block/loom_front"),
        &selected,
        &BTreeMap::new(),
        false,
        None,
        false,
    )
    .unwrap();
    let missing = importer
        .import(std::slice::from_ref(&absent), cancel)
        .unwrap();
    assert_eq!(
        missing.records[0].failure.as_deref(),
        Some("resource absent from every explicit candidate")
    );
    assert!(missing.bank.is_empty());
    drop(missing);
    let mut alias = absent.clone();
    alias.candidates.push(TextureCandidate {
        pack: selected,
        location: TextureLocation::Literal {
            path: AssetPath::parse("fixture.png").unwrap(),
            evidence: Label::new("diagnostic alias only").unwrap(),
        },
        schedule: ScheduleSource::NoMetadata,
        expected_source_sha256: None,
        expected_image: ImageExpectations::default(),
    });
    let resolved = importer.import(&[alias], cancel).unwrap();
    assert!(resolved.records[0].failure.is_none());
    assert!(resolved.records[0].candidates[1].alias_evidence.is_some());
    assert_eq!(
        resolved.records[0].source.as_ref().unwrap().kind,
        OriginKind::DiagnosticFixture
    );
    assert!(!resolved.bank.coverage().required_textures_satisfied);
    drop(resolved);
    let before = budget.used();
    stop.store(true, Ordering::Release);
    assert!(matches!(
        importer.import(std::slice::from_ref(&absent), cancel),
        Err(AssetError::Cancelled)
    ));
    stop.store(false, Ordering::Release);
    assert_eq!(budget.used(), before);
    let blocker = budget
        .clone()
        .reserve(budget.limit() - before, cancel)
        .unwrap();
    assert!(matches!(
        importer.import(&[absent], cancel),
        Err(AssetError::Limit { .. })
    ));
    assert_eq!(budget.used(), budget.limit());
    drop(blocker);
    assert_eq!(budget.used(), before);
    let foreign = ByteBudget::new(64 << 20).unwrap();
    assert!(TextureImporter::new(&packs, Limits::default(), foreign).is_err());
    drop(importer);
    drop(packs);
    assert_eq!(budget.used(), 0);
}

fn private_settings(profile: &Value) -> VoxelLandscapeSettings {
    let mut settings = VoxelLandscapeSettings {
        pack_profile: pack_profiles::FULL_PACKS
            .iter()
            .position(|entry| Some(entry.id) == profile["profile"].as_str())
            .unwrap(),
        ..Default::default()
    };
    let source = serde_json::from_value::<super::super::settings::PackSourceSettings>(
        profile["mount"].clone(),
    )
    .unwrap();
    settings.apply_source_settings(&source);
    settings
}
fn write_receipt(root: &Path, name: &str, value: &Value) {
    let mut bytes = serde_json::to_vec(value).unwrap();
    bytes.push(b'\n');
    assert!(bytes.len() <= 32 << 20);
    write_member(root, name, &bytes);
}

#[test]
#[ignore = "private primary-only archive import and mesh receipts; no raster acceptance"]
fn primary_private_import_and_mesh_receipts() {
    let input = inventory();
    let profiles = input["profiles"].as_array().unwrap();
    let output =
        std::env::var_os("ILIUM_WORKSTATION_RECEIPTS").expect("set a NEW private output directory");
    let output = Path::new(&output);
    fs::create_dir(output).unwrap();
    let fallback_name = std::env::var("ILIUM_WORKSTATION_FALLBACK")
        .expect("name an explicitly reviewed fallback profile, or none");
    let fallback_profile = if fallback_name == "none" {
        None
    } else {
        Some(
            profiles
                .iter()
                .find(|profile| profile["profile"] == fallback_name)
                .expect("fallback must be in the captured eleven profiles"),
        )
    };
    let cases = workstation_cases();
    let generated: BTreeSet<_> = cases.iter().map(|case| case.state.clone()).collect();
    assert_eq!(cases.len(), 300);
    assert_eq!(generated.len(), 31);
    write_receipt(
        output,
        "cases.json",
        &json!({"packet_sha256":PACKET_SHA,"cases":cases.iter().map(|case| json!({
        "style":case.style,"profession_block":case.profession,"turns":case.turns,"source":case.source,
        "position":case.position,"state":case.state.canonical()
    })).collect::<Vec<_>>(),"generated_states":generated.iter().map(BlockState::canonical).collect::<Vec<_>>() }),
    );
    let mut defects = Vec::new();
    for profile in profiles {
        let name = profile["profile"].as_str().unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(SCENE_BUDGET_BYTES).unwrap();
        {
            let mounted =
                pack_sources::mount_selected(&private_settings(profile), budget.clone(), cancel)
                    .unwrap();
            let archive_sha = mounted.source_sha256;
            let review = mounted.pack.review().clone();
            let origin = BlobOrigin {
                pack: review.pack.clone(),
                release: review.release.clone(),
                layer: Label::new("Original Ilium geometry compatibility; not pack-authored model")
                    .unwrap(),
                path: AssetPath::parse("compatibility/root.json").unwrap(),
                review_digest: review.digest().unwrap(),
                kind: OriginKind::OriginalCompatibilityGeometry,
            };
            let mut defs = DefinitionSet::new(origin, Limits::default(), budget.clone()).unwrap();
            defs.install_geometry_templates(cancel).unwrap();
            for block in IDS {
                add_compatibility(
                    &mut defs,
                    &id(&format!("minecraft:{block}")),
                    name == "goodvibes",
                    name == "plasticator",
                    true,
                    cancel,
                )
                .unwrap();
            }
            let mut packs = vec![mounted.pack];
            let mut fallback_sha = None;
            let fallback = fallback_profile.filter(|value| value["profile"] != name);
            if let Some(fallback) = fallback {
                let mounted = pack_sources::mount_reviewed_fallback(
                    &private_settings(fallback),
                    budget.clone(),
                    cancel,
                )
                .unwrap();
                fallback_sha = mounted.source_sha256;
                packs.push(mounted.pack);
            }
            // Full-pack fallback supplies images only, never selected model JSON.
            let sources = DefinitionSources::new(&packs[..1], &[], Some(&defs)).unwrap();
            let mut compiler =
                ModelCompiler::new(&sources, Limits::default(), budget.clone()).unwrap();
            let mut original =
                ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
            let mut compiled = Vec::new();
            let mut state_rows = Vec::new();
            let mut resources = BTreeSet::new();
            for state in matrix() {
                let result = if uses_generated_material_geometry(state.id(), false) {
                    original.compile_state(&state, [0, 0, 64], 97, cancel)
                } else {
                    compiler.compile_state(&state, [0, 0, 64], 97, cancel)
                };
                let expected_unsupported = unsupported(&state, name == "whimscape");
                match result {
                    Err(error) => {
                        if !expected_unsupported {
                            defects.push(format!("{name}: {}: {error}", state.canonical()));
                        }
                        state_rows.push(json!({"state":state.canonical(),"generated":generated.contains(&state),"unsupported":expected_unsupported,"error":error.to_string()}));
                    }
                    Ok(value) => {
                        if expected_unsupported {
                            defects.push(format!(
                                "{name}: unsupported occupancy accepted: {}",
                                state.canonical()
                            ));
                        }
                        resources.extend(textures(&value));
                        state_rows.push(json!({"state":state.canonical(),"generated":generated.contains(&state),"unsupported":false,
                            "state_origin":value.state_origin,"applications":value.applications.iter().map(|(app, model)| json!({
                                "application":app,"origins":model.origins,"quads":model.quads.iter().map(|quad| {
                                    let q = oriented_quad(quad, app).unwrap();
                                    json!({"element":q.element,"face":q.face.name(),"texture":q.texture,"points":q.points,"uv":q.uv,"normal":q.normal,"tint_index":q.tint_index})
                                }).collect::<Vec<_>>()
                            })).collect::<Vec<_>>() }));
                        compiled.push(value);
                    }
                }
            }
            assert_eq!(state_rows.len(), 85);
            let aliases = explicit_aliases(name).unwrap();
            let mut passes = Vec::new();
            for with_fallback in [false, true] {
                if with_fallback && fallback.is_none() {
                    continue;
                }
                let fallback_id = if with_fallback {
                    Some(&packs[1].review().pack)
                } else {
                    None
                };
                let requests: Vec<_> = resources
                    .iter()
                    .map(|resource| {
                        request(
                            id(resource),
                            &review.pack,
                            &aliases,
                            name == "goodvibes",
                            fallback_id,
                            fallback.is_some_and(|profile| profile["profile"] == "goodvibes"),
                        )
                    })
                    .collect::<Result<_>>()
                    .unwrap();
                let importer = TextureImporter::new(
                    if with_fallback { &packs } else { &packs[..1] },
                    Limits::default(),
                    budget.clone(),
                )
                .unwrap();
                let imported = importer.import(&requests, cancel).unwrap();
                let mut meshes = Vec::new();
                for value in &compiled {
                    let mut policy = table(value, &imported.bank);
                    // This tests binding, not biome appearance. Native raster qualification must use production tints.
                    for (_, model) in &value.applications {
                        for index in model.quads.iter().filter_map(|q| q.tint_index) {
                            policy.tints.insert(index, [1.0; 3]);
                        }
                    }
                    let result = BoundModel::bind(value, &imported.bank, &policy, &budget, cancel)
                        .and_then(|model| {
                            PreparedMesh::build(
                                &BTreeMap::from([([0, 0, 64], Arc::new(model))]),
                                MeshRegion {
                                    minimum: [0, 0],
                                    maximum: [1, 1],
                                },
                                imported.bank.identity(),
                                8192,
                                &budget,
                                cancel,
                            )
                        });
                    match result {
                        Ok(mesh) => meshes.push(json!({"state":value.state.canonical(),"faces":mesh.faces.iter().map(|face| json!({
                            "position":face.owner.position,"part":face.owner.part,"face":face.owner.face,"layer":face.owner.layer
                        })).collect::<Vec<_>>() })),
                        Err(error) => {
                            if with_fallback || fallback.is_none() { defects.push(format!("{name}: material {}: {error}", value.state.canonical())); }
                            meshes.push(json!({"state":value.state.canonical(),"error":error.to_string()}));
                        }
                    }
                }
                let final_pass = with_fallback || fallback.is_none();
                if final_pass && !imported.bank.coverage().required_textures_satisfied {
                    defects.push(format!("{name}: unsatisfied final texture requirements"));
                }
                passes.push(json!({"fallback_enabled":with_fallback,"bank":imported.bank.identity(),"coverage":imported.bank.coverage(),
                    "imports":imported.records,"meshes":meshes,"decoded":resources.iter().filter_map(|resource| {
                        let texture = imported.bank.resolve(&id(resource)).and_then(|handle| imported.bank.texture(handle))?;
                        Some(json!({"resource":resource,"alpha_min":texture.image().info().alpha_min,
                            "authored_animation":texture.animation().authored_schedule(),"changing_rectangles":texture.animation().changing_rects()}))
                    }).collect::<Vec<_>>(),"tints":"diagnostic-neutral-not-biome"}));
            }
            write_receipt(
                output,
                &format!("{name}.json"),
                &json!({"schema":1,"packet_sha256":PACKET_SHA,"profile":name,
                "settings":profile["mount"],"archive_sha256":archive_sha,"fallback_profile":fallback.map(|value| &value["profile"]),
                "fallback_archive_sha256":fallback_sha,"review":review,"states":state_rows,"passes":passes,"budget_peak":budget.peak(),
                "qualification":"compile-import-mesh only; native pixels and production biome tints not measured"}),
            );
        }
        assert_eq!(
            budget.used(),
            0,
            "{name}: retained charge after complete fixture release"
        );
    }
    write_receipt(
        output,
        "result.json",
        &json!({"profiles":11,"workplaces":260,"workstation_cells":300,"generated_states":31,"defects":defects,"native_pixels":"not-run"}),
    );
    assert!(
        defects.is_empty(),
        "private import/mesh defects; read result.json"
    );
}
