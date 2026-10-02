//! Synthetic model/state contract regressions: no source-pack art or native world acceptance.
use super::{
    block_state::BlockState,
    budget::{ByteBudget, Cancel, Limits},
    compatibility::DefinitionSet,
    identity::{Label, OriginKind, ResourceId},
    models::{oriented_quad, Direction, ModelCompiler},
    review::fixture_origin,
};
use std::sync::atomic::AtomicBool;
fn definitions(budget: &ByteBudget, cancel: Cancel<'_>) -> DefinitionSet {
    let mut defs = DefinitionSet::new(
        fixture_origin(OriginKind::DiagnosticFixture),
        Limits::default(),
        budget.clone(),
    )
    .unwrap();
    defs.install_geometry_templates(cancel).unwrap();
    defs
}
fn reason() -> Label {
    Label::new("Original synthetic model regression fixture").unwrap()
}
#[test]
fn inherited_texture_variables_are_resolved_after_child_overrides() {
    let budget = ByteBudget::new(64 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    defs.insert_model("test:block/base",&serde_json::json!({"parent":"block/cube_all","textures":{"all":"#selected","selected":"test:old"}}),reason(),cancel).unwrap();
    defs.insert_model(
        "test:block/child",
        &serde_json::json!({"parent":"test:block/base","textures":{"selected":"test:child"}}),
        reason(),
        cancel,
    )
    .unwrap();
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    let model = compiler
        .compile_model(&ResourceId::parse("test:block/child").unwrap(), cancel)
        .unwrap();
    assert_eq!(model.quads.len(), 6);
    assert!(model
        .quads
        .iter()
        .all(|q| q.texture.as_str() == "test:child"));
    assert!(model
        .quads
        .iter()
        .all(|q| q.complete_boundary == Some(q.face)));
    assert_eq!(model.origins.len(), 4);
    assert!(model.origins.iter().all(|o| o.compatibility.is_some()));
    let reused = compiler.compile_model(&model.id, cancel).unwrap();
    assert!(std::sync::Arc::ptr_eq(&model, &reused));
}
#[test]
fn child_elements_replace_parent_elements_instead_of_appending_hidden_cubes() {
    let budget = ByteBudget::new(64 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    defs.insert_model("test:block/empty",&serde_json::json!({"parent":"block/cube_all","textures":{"all":"test:stone"},"elements":[]}),reason(),cancel).unwrap();
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    assert!(compiler
        .compile_model(&ResourceId::parse("test:block/empty").unwrap(), cancel)
        .unwrap()
        .quads
        .is_empty());
}
#[test]
fn parent_cycles_texture_cycles_unknown_loaders_and_missing_definitions_are_errors() {
    let budget = ByteBudget::new(64 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    for (id, value) in [
        ("test:block/a", serde_json::json!({"parent":"test:block/b"})),
        ("test:block/b", serde_json::json!({"parent":"test:block/a"})),
        (
            "test:block/loop",
            serde_json::json!({"parent":"block/cube_all","textures":{"all":"#other","other":"#all"}}),
        ),
        (
            "test:block/custom",
            serde_json::json!({"loader":"downloaded:code"}),
        ),
    ] {
        defs.insert_model(id, &value, reason(), cancel).unwrap();
    }
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    for id in [
        "test:block/a",
        "test:block/loop",
        "test:block/custom",
        "test:block/absent",
    ] {
        assert!(compiler
            .compile_model(&ResourceId::parse(id).unwrap(), cancel)
            .is_err());
    }
}
#[test]
fn log_axis_rotations_move_end_grain_not_only_the_semantic_name() {
    let budget = ByteBudget::new(64 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    defs.bind_column(
        ResourceId::parse("test:birch_log").unwrap(),
        ResourceId::parse("test:block/birch_bark").unwrap(),
        ResourceId::parse("test:block/birch_end").unwrap(),
        reason(),
        cancel,
    )
    .unwrap();
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    for (axis, expected) in [
        ("y", [0.0, 0.0, 1.0]),
        ("x", [1.0, 0.0, 0.0]),
        ("z", [0.0, -1.0, 0.0]),
    ] {
        let state = BlockState::new(
            ResourceId::parse("test:birch_log").unwrap(),
            [
                ("axis".into(), axis.into()),
                ("waterlogged".into(), "false".into()),
            ],
        )
        .unwrap();
        let compiled = compiler
            .compile_state(&state, [-1, 7, 80], 7, cancel)
            .unwrap();
        assert_eq!(compiled.state, state);
        let (application, model) = &compiled.applications[0];
        let top = model
            .quads
            .iter()
            .find(|q| q.face == Direction::Up)
            .unwrap();
        assert_eq!(top.texture.as_str(), "test:block/birch_end");
        let transformed = oriented_quad(top, application).unwrap();
        assert!((0..3).all(|i| (transformed.normal[i] - expected[i]).abs() < 1e-6));
    }
}
#[test]
fn grass_top_bottom_side_and_tinted_overlay_remain_distinct_bindings() {
    let budget = ByteBudget::new(64 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    let id = |text| ResourceId::parse(text).unwrap();
    defs.bind_grass(
        id("test:grass"),
        id("test:grass_top"),
        id("test:dirt"),
        id("test:grass_side"),
        Some(id("test:grass_side_overlay")),
        reason(),
        cancel,
    )
    .unwrap();
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    let state = compiler
        .compile_state(
            &BlockState::new(id("test:grass"), []).unwrap(),
            [0; 3],
            0,
            cancel,
        )
        .unwrap();
    let quads = &state.applications[0].1.quads;
    assert_eq!(quads.len(), 10);
    assert_eq!(
        quads
            .iter()
            .find(|q| q.face == Direction::Up)
            .unwrap()
            .texture,
        id("test:grass_top")
    );
    assert_eq!(
        quads
            .iter()
            .find(|q| q.face == Direction::Down)
            .unwrap()
            .texture,
        id("test:dirt")
    );
    assert_eq!(
        quads
            .iter()
            .filter(|q| q.texture == id("test:grass_side_overlay") && q.tint_index == Some(0))
            .count(),
        4
    );
    assert!(quads
        .iter()
        .filter(|q| q.texture == id("test:grass_side"))
        .all(|q| q.tint_index.is_none()));
}
#[test]
fn cross_planes_and_partial_cuboids_do_not_claim_complete_opaque_boundaries() {
    let budget = ByteBudget::new(64 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    defs.insert_model(
        "test:block/flower",
        &serde_json::json!({"parent":"block/cross","textures":{"cross":"test:flower"}}),
        reason(),
        cancel,
    )
    .unwrap();
    defs.insert_model("test:block/slab",&serde_json::json!({"elements":[{"from":[0,0,0],"to":[16,8,16],"faces":{"up":{"texture":"test:stone"},"north":{"texture":"test:stone","cullface":"north"}}}]}),reason(),cancel).unwrap();
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    let cross = compiler
        .compile_model(&ResourceId::parse("test:block/flower").unwrap(), cancel)
        .unwrap();
    assert_eq!(cross.quads.len(), 4);
    assert!(cross
        .quads
        .iter()
        .all(|q| q.complete_boundary.is_none() && q.cull_face.is_none() && !q.shade));
    let slab = compiler
        .compile_model(&ResourceId::parse("test:block/slab").unwrap(), cancel)
        .unwrap();
    assert!(slab.quads.iter().all(|q| q.complete_boundary.is_none()));
    let top = slab.quads.iter().find(|q| q.face == Direction::Up).unwrap();
    assert!(top.points.iter().all(|p| p[2] == 0.5));
    let north = slab
        .quads
        .iter()
        .find(|q| q.face == Direction::North)
        .unwrap();
    assert_eq!(north.uv[0], [0.0, 0.5]);
    assert_eq!(north.uv[2], [1.0, 1.0]);
}
#[test]
fn uv_rotation_and_uvlock_are_actual_coordinate_operations() {
    let budget = ByteBudget::new(64 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    defs.insert_model("test:block/uv",&serde_json::json!({"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"test:uv","uv":[0,0,8,16],"rotation":90}}}]}),reason(),cancel).unwrap();
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    let model = compiler
        .compile_model(&ResourceId::parse("test:block/uv").unwrap(), cancel)
        .unwrap();
    assert_eq!(model.quads[0].uv[0], [0.5, 0.0]);
    assert_eq!(model.quads[0].uv[2], [0.0, 1.0]);
    let mut application = super::block_state::ModelApplication {
        model: model.id.clone(),
        x_turns: 0,
        y_turns: 1,
        uvlock: false,
        weight: 1,
    };
    let unlocked = oriented_quad(&model.quads[0], &application).unwrap();
    application.uvlock = true;
    let locked = oriented_quad(&model.quads[0], &application).unwrap();
    assert_eq!(unlocked.points, locked.points);
    assert_ne!(unlocked.uv, locked.uv);
    assert_eq!(unlocked.uv, model.quads[0].uv);
}
#[test]
fn duplicate_persisted_properties_are_not_silently_collapsed() {
    assert!(serde_json::from_str::<BlockState>(
        r#"{"id":"minecraft:oak_log","properties":{"axis":"x","axis":"z"}}"#
    )
    .is_err());
    assert!(serde_json::from_str::<BlockState>(
        r#"{"id":"minecraft:oak_log","properties":{"axis":"X"}}"#
    )
    .is_err());
}
