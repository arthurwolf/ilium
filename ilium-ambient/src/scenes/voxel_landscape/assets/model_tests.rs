//! Synthetic model/state contract regressions: no source-pack art or native world acceptance.
use super::{
    block_state::BlockState,
    budget::{ByteBudget, Cancel, Limits},
    compatibility::DefinitionSet,
    error::AssetError,
    identity::{Label, OriginKind, ResourceId, SourceBlob},
    layers::{ResourceKey, ResourceKind},
    metadata::Document,
    models::{oriented_quad, DefinitionInput, DefinitionProvider, Direction, ModelCompiler},
    review::fixture_origin,
};
use std::{
    cell::Cell,
    sync::atomic::{AtomicBool, Ordering},
};

struct CountingDefinitions<'a> {
    inner: &'a DefinitionSet,
    blockstate_reads: Cell<usize>,
    model_reads: Cell<usize>,
}
struct SwitchingDefinitions<'a> {
    sources: [&'a DefinitionSet; 3],
    active: Cell<usize>,
    blockstate_reads: Cell<usize>,
    model_reads: Cell<usize>,
}
impl DefinitionProvider for SwitchingDefinitions<'_> {
    fn definition(
        &self,
        key: &ResourceKey,
        cancel: Cancel<'_>,
    ) -> super::error::Result<Option<DefinitionInput>> {
        match key.kind {
            ResourceKind::Blockstate => {
                self.blockstate_reads.set(self.blockstate_reads.get() + 1);
            }
            ResourceKind::Model => self.model_reads.set(self.model_reads.get() + 1),
            _ => {}
        }
        self.sources[self.active.get()].definition(key, cancel)
    }
}

#[test]
fn mutable_provider_reloads_changed_blockstate_and_resets_model_cache_per_tile() {
    let budget = ByteBudget::new(128 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut sources = Vec::new();
    for (model, value) in [
        (
            "test:block/first",
            serde_json::json!({"elements":[{"from":[0,0,0],"to":[16,16,16],
                "faces":{"up":{"texture":"test:first"}}}]}),
        ),
        (
            "test:block/second",
            serde_json::json!({"elements":[{"from":[0,0,0],"to":[16,16,16],
                "faces":{"up":{"texture":"test:second"}}}]}),
        ),
        (
            "test:block/second",
            serde_json::json!({"loader":"test:unsupported"}),
        ),
    ] {
        let mut defs = DefinitionSet::new(
            fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap();
        defs.insert_model(model, &value, reason(), cancel).unwrap();
        defs.insert(
            ResourceKey {
                kind: ResourceKind::Blockstate,
                id: ResourceId::parse("test:live").unwrap(),
            },
            &serde_json::json!({"variants":{"":{"model":model}}}),
            reason(),
            cancel,
        )
        .unwrap();
        sources.push(defs);
    }
    let source_baseline = budget.used();
    {
        let switching = SwitchingDefinitions {
            sources: [&sources[0], &sources[1], &sources[2]],
            active: Cell::new(0),
            blockstate_reads: Cell::new(0),
            model_reads: Cell::new(0),
        };
        let mut compiler =
            ModelCompiler::new(&switching, Limits::default(), budget.clone()).unwrap();
        let state = BlockState::new(ResourceId::parse("test:live").unwrap(), []).unwrap();
        let first = compiler
            .compile_state(&state, [0, 64, 0], 17, cancel)
            .unwrap();
        assert_eq!(first.applications[0].1.id.as_str(), "test:block/first");
        switching.active.set(1);
        let second = compiler
            .compile_state(&state, [1, 64, 0], 17, cancel)
            .unwrap();
        assert_eq!(second.applications[0].1.id.as_str(), "test:block/second");
        assert_eq!(
            second.applications[0].1.quads[0].texture.as_str(),
            "test:second"
        );
        assert_ne!(first.state_origin.sha256, second.state_origin.sha256);
        assert_eq!(switching.blockstate_reads.get(), 2);
        compiler.reset_for_next_tile();
        switching.active.set(2);
        assert!(matches!(
            compiler.compile_state(&state, [2, 64, 0], 17, cancel),
            Err(super::error::AssetError::Unsupported(_))
        ));
        assert_eq!(switching.blockstate_reads.get(), 3);
        assert_eq!(switching.model_reads.get(), 3);
    }
    assert_eq!(budget.used(), source_baseline);
    drop(sources);
    assert_eq!(budget.used(), 0);
}
impl DefinitionProvider for CountingDefinitions<'_> {
    fn definition(
        &self,
        key: &ResourceKey,
        cancel: Cancel<'_>,
    ) -> super::error::Result<Option<DefinitionInput>> {
        match key.kind {
            ResourceKind::Blockstate => {
                self.blockstate_reads.set(self.blockstate_reads.get() + 1);
            }
            ResourceKind::Model => self.model_reads.set(self.model_reads.get() + 1),
            _ => {}
        }
        self.inner.definition(key, cancel)
    }
}

#[test]
fn retained_compiler_reads_each_definition_once_but_selects_each_anchor_and_seed() {
    let budget = ByteBudget::new(128 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = DefinitionSet::new(
        fixture_origin(OriginKind::DiagnosticFixture),
        Limits::default(),
        budget.clone(),
    )
    .unwrap();
    for (name, texture) in [
        ("test:block/first", "test:first"),
        ("test:block/second", "test:second"),
        ("test:block/side", "test:side"),
    ] {
        defs.insert_model(
            name,
            &serde_json::json!({"elements":[{"from":[0,0,0],"to":[16,16,16],
                "faces":{"up":{"texture":texture}}}]}),
            reason(),
            cancel,
        )
        .unwrap();
    }
    let id = ResourceId::parse("test:choice").unwrap();
    defs.insert(
        ResourceKey {
            kind: ResourceKind::Blockstate,
            id: id.clone(),
        },
        &serde_json::json!({"multipart":[
            {"apply":[{"model":"test:block/first"},{"model":"test:block/second"}]},
            {"when":{"east":"true"},"apply":{"model":"test:block/side"}}
        ]}),
        reason(),
        cancel,
    )
    .unwrap();
    let source_baseline = budget.used();
    {
        let counted = CountingDefinitions {
            inner: &defs,
            blockstate_reads: Cell::new(0),
            model_reads: Cell::new(0),
        };
        let mut compiler = ModelCompiler::new(&counted, Limits::default(), budget.clone()).unwrap();
        compiler.enable_immutable_source_cache();
        let mut selected = std::collections::BTreeSet::new();
        let mut seed_changes_choice = false;
        let mut origin = None;
        for x in -48..48 {
            let state = BlockState::new(
                id.clone(),
                [(
                    "east".into(),
                    if x % 2 == 0 { "true" } else { "false" }.into(),
                )],
            )
            .unwrap();
            let anchor = [x, 72, -7];
            let first = compiler.compile_state(&state, anchor, 17, cancel).unwrap();
            let second = compiler.compile_state(&state, anchor, 18, cancel).unwrap();
            assert_eq!(first.applications.len(), if x % 2 == 0 { 2 } else { 1 });
            assert_eq!(second.applications.len(), first.applications.len());
            if x % 2 == 0 {
                assert_eq!(first.applications[1].1.id.as_str(), "test:block/side");
            }
            selected.insert(first.applications[0].1.id.as_str().to_owned());
            seed_changes_choice |= first.applications[0].1.id != second.applications[0].1.id;
            if let Some(expected) = origin {
                assert_eq!(first.state_origin.sha256, expected);
            } else {
                origin = Some(first.state_origin.sha256);
            }
        }
        assert_eq!(selected.len(), 2);
        assert!(seed_changes_choice);
        assert_eq!(counted.blockstate_reads.get(), 1);
        assert_eq!(counted.model_reads.get(), 3);
        stop.store(true, Ordering::Release);
        let cancelled = compiler.compile_state(
            &BlockState::new(id, [("east".into(), "true".into())]).unwrap(),
            [0, 72, 0],
            17,
            cancel,
        );
        assert!(matches!(
            cancelled,
            Err(super::error::AssetError::Cancelled)
        ));
    }
    assert_eq!(budget.used(), source_baseline);
    drop(defs);
    assert_eq!(budget.used(), 0);
}

#[test]
fn route_cache_recycles_at_capacity_without_losing_selected_models_or_charges() {
    let budget = ByteBudget::new(128 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = DefinitionSet::new(
        fixture_origin(OriginKind::DiagnosticFixture),
        Limits::default(),
        budget.clone(),
    )
    .unwrap();
    for index in 0..3 {
        let block = format!("test:recycle_{index}");
        let model = format!("test:block/recycle_{index}");
        let texture = format!("test:color_{index}");
        defs.insert_model(
            &model,
            &serde_json::json!({"elements":[{"from":[0,0,0],"to":[16,16,16],
                "faces":{"up":{"texture":texture}}}]}),
            reason(),
            cancel,
        )
        .unwrap();
        defs.insert(
            ResourceKey {
                kind: ResourceKind::Blockstate,
                id: ResourceId::parse(&block).unwrap(),
            },
            &serde_json::json!({"variants":{"":{"model":model}}}),
            reason(),
            cancel,
        )
        .unwrap();
    }
    let source_baseline = budget.used();
    let limits = Limits {
        textures: 2,
        ..Limits::default()
    };
    {
        let mut compiler = ModelCompiler::new(&defs, limits, budget.clone()).unwrap();
        compiler.enable_immutable_source_cache();
        for index in [0, 1, 2, 0] {
            let state = BlockState::new(
                ResourceId::parse(&format!("test:recycle_{index}")).unwrap(),
                [],
            )
            .unwrap();
            let compiled = compiler
                .compile_state(&state, [index, 64, -1], 17, cancel)
                .unwrap();
            assert_eq!(
                compiled.applications[0].1.quads[0].texture.as_str(),
                format!("test:color_{index}")
            );
        }
    }
    assert_eq!(budget.used(), source_baseline);
    drop(defs);
    assert_eq!(budget.used(), 0);
}
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
fn native_element_comments_preserve_geometry_without_relaxing_semantic_fields() {
    let budget = ByteBudget::new(64 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    let baseline = serde_json::json!({
        "elements": [{
            "from": [0, 0, 0], "to": [16, 16, 16],
            "faces": {"up": {"texture": "test:stone"}}
        }]
    });
    let mut commented = baseline.clone();
    commented["elements"][0]["__comment"] = "Center post".into();
    let mut unsupported = commented.clone();
    unsupported["elements"][0]["renderer_behavior"] = "executable".into();
    for (name, document) in [
        ("test:block/comment_baseline", baseline),
        ("test:block/commented", commented),
        ("test:block/comment_unknown", unsupported),
    ] {
        defs.insert_model(name, &document, reason(), cancel)
            .unwrap();
    }
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    let baseline = compiler
        .compile_model(
            &ResourceId::parse("test:block/comment_baseline").unwrap(),
            cancel,
        )
        .unwrap();
    let commented = compiler
        .compile_model(&ResourceId::parse("test:block/commented").unwrap(), cancel)
        .unwrap();
    assert_eq!(commented.quads, baseline.quads);
    assert!(compiler
        .compile_model(
            &ResourceId::parse("test:block/comment_unknown").unwrap(),
            cancel
        )
        .is_err());
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

#[test]
fn pinned_1193_exporter_labels_in_parent_preserve_selected_child_faces_and_origin() {
    let budget = ByteBudget::new(128 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    let baseline_parent = serde_json::json!({
        "textures": {"torch": "test:old_torch"},
        "elements": [{
            "from": [0, 0, 0], "to": [16, 16, 16],
            "faces": {"north": {"texture": "#torch"}}
        }]
    });
    let mut labeled_parent = baseline_parent.clone();
    labeled_parent["format_version"] = "1.21.11".into();
    labeled_parent["groups"] = serde_json::json!([{"name":"exporter group","children":[0]}]);
    labeled_parent["__createdwith"] = "opl's Model Maker".into();
    labeled_parent["elements"][0]["shade_direction_override"] = "up".into();
    for (name, value) in [
        ("test:block/parent_plain", baseline_parent),
        ("test:block/parent_labeled", labeled_parent),
        (
            "test:block/child_plain",
            serde_json::json!({"parent":"test:block/parent_plain","textures":{"torch":"test:selected_torch"}}),
        ),
        (
            "test:block/child_labeled",
            serde_json::json!({"parent":"test:block/parent_labeled","textures":{"torch":"test:selected_torch"}}),
        ),
    ] {
        defs.insert_model(name, &value, reason(), cancel).unwrap();
    }
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    let plain = compiler
        .compile_model(
            &ResourceId::parse("test:block/child_plain").unwrap(),
            cancel,
        )
        .unwrap();
    let labeled = compiler
        .compile_model(
            &ResourceId::parse("test:block/child_labeled").unwrap(),
            cancel,
        )
        .unwrap();
    assert_eq!(labeled.quads, plain.quads);
    assert_eq!(labeled.quads[0].texture.as_str(), "test:selected_torch");
    assert_eq!(labeled.origins.len(), 2);
    assert_ne!(labeled.origins[0].sha256, plain.origins[0].sha256);
    for field in [
        "format_version",
        "groups",
        "__createdwith",
        "shade_direction_override",
    ] {
        assert!(labeled
            .ignored_non_world_fields
            .iter()
            .any(|record| record.as_str() == field));
    }
}

#[test]
fn pinned_1193_exporter_labels_do_not_disable_active_model_guards() {
    let budget = ByteBudget::new(128 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    let base = serde_json::json!({
        "format_version": "1.21.11",
        "elements": [{
            "from": [0, 0, 0], "to": [16, 16, 16],
            "faces": {"north": {"texture": "test:stone"}}
        }]
    });
    let mut unknown = base.clone();
    unknown["unreviewed_renderer_behavior"] = "execute".into();
    let mut loader = base.clone();
    loader["loader"] = "test:custom".into();
    let mut render_type = base.clone();
    render_type["render_type"] = "test:custom".into();
    for (name, value) in [
        ("test:block/unknown_field", unknown),
        ("test:block/custom_loader", loader),
        ("test:block/custom_render_type", render_type),
    ] {
        defs.insert_model(name, &value, reason(), cancel).unwrap();
    }
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    for name in [
        "test:block/unknown_field",
        "test:block/custom_loader",
        "test:block/custom_render_type",
    ] {
        assert!(matches!(
            compiler.compile_model(&ResourceId::parse(name).unwrap(), cancel),
            Err(AssetError::Unsupported(_))
        ));
    }
}

#[test]
fn selected_jicklus_duplicate_lower_variant_remains_rejected() {
    // Exact retained Jicklus blockstates/tall_grass.json bytes, including its
    // two distinct authored `lower` entries. Do not silently choose either.
    const SOURCE: &str = "{\n  \"variants\": {\n    \"lower\": [\n\t{\"model\": \"minecraft:block/tall_grass_bottom\"},\n\t{\"model\": \"minecraft:block/tall_grassbottom2\"}\n\t],\n\t \"lower\": [\n\t{\"model\": \"minecraft:block/tall_grass_top\"},\n\t{\"model\": \"minecraft:block/tall_grass_top2\"}\n\t]\n  }\n}";
    let budget = ByteBudget::new(64 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let limits = Limits::default();
    let source = SourceBlob::new(
        SOURCE.as_bytes().to_vec(),
        fixture_origin(OriginKind::DiagnosticFixture),
        None,
        &limits,
        &budget,
        cancel,
    )
    .unwrap();
    assert!(Document::parse(&source, &limits, &budget, cancel).is_err());
}

#[test]
fn selected_jicklus_planes_keep_twelve_visible_faces_without_manufactured_thickness() {
    let budget = ByteBudget::new(128 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    let value =
        serde_json::from_str(include_str!("model_fixtures/jicklus-glow-lichen.json")).unwrap();
    defs.insert_model("test:block/jicklus_lichen", &value, reason(), cancel)
        .unwrap();
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    let model = compiler
        .compile_model(
            &ResourceId::parse("test:block/jicklus_lichen").unwrap(),
            cancel,
        )
        .unwrap();
    assert_eq!(model.quads.len(), 12);
    for element in 0..6 {
        let quads: Vec<_> = model
            .quads
            .iter()
            .filter(|quad| quad.element == element)
            .collect();
        assert_eq!(quads.len(), 2);
        assert_eq!(quads[0].normal.map(|value| -value), quads[1].normal);
        assert!(quads
            .iter()
            .all(|quad| quad.texture.as_str() == "minecraft:block/glow_lichen_side"));
        let axis = if element % 2 == 0 { 1 } else { 0 };
        let plane = quads[0].points[0][axis];
        assert!(quads
            .iter()
            .flat_map(|quad| quad.points)
            .all(|point| point[axis] == plane));
    }
    assert_eq!(model.origins.len(), 1);
}

#[test]
fn pinned_1193_ignores_selected_light_hint_but_preserves_emissive_texture_and_shade() {
    let budget = ByteBudget::new(128 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    let hinted: serde_json::Value =
        serde_json::from_str(include_str!("model_fixtures/whimscape-glow-lichen.json")).unwrap();
    let mut plain = hinted.clone();
    plain["elements"][1]
        .as_object_mut()
        .unwrap()
        .remove("light_emission");
    for (name, value) in [
        ("test:block/hinted_lichen", hinted),
        ("test:block/plain_lichen", plain),
    ] {
        defs.insert_model(name, &value, reason(), cancel).unwrap();
    }
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    let hinted = compiler
        .compile_model(
            &ResourceId::parse("test:block/hinted_lichen").unwrap(),
            cancel,
        )
        .unwrap();
    let plain = compiler
        .compile_model(
            &ResourceId::parse("test:block/plain_lichen").unwrap(),
            cancel,
        )
        .unwrap();
    assert_eq!(hinted.quads.len(), 4);
    assert_eq!(hinted.quads, plain.quads);
    assert_eq!(
        hinted
            .quads
            .iter()
            .filter(|quad| !quad.shade
                && quad.texture.as_str() == "minecraft:block/glow_lichen_emissive")
            .count(),
        2
    );
    assert_ne!(hinted.origins[0].sha256, plain.origins[0].sha256);
    assert!(hinted
        .ignored_non_world_fields
        .iter()
        .any(|field| field.as_str() == "light_emission"));
}

#[test]
fn zero_area_faces_still_validate_cull_uv_and_unknown_render_fields() {
    let budget = ByteBudget::new(128 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut defs = definitions(&budget, cancel);
    for (index, face) in [
        serde_json::json!({"texture":"test:stone","cullface":"sideways"}),
        serde_json::json!({"texture":"test:stone","uv":[0,0,16]}),
        serde_json::json!({"texture":"test:stone","unknown_render_behavior":true}),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("test:block/invalid_plane_{index}");
        let value =
            serde_json::json!({"elements":[{"from":[0,0,0],"to":[16,16,0],"faces":{"east":face}}]});
        defs.insert_model(&id, &value, reason(), cancel).unwrap();
    }
    let mut compiler = ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap();
    for (index, expected) in [
        "unknown model face",
        "four UV coordinates required",
        "metadata field unknown_render_behavior",
    ]
    .into_iter()
    .enumerate()
    {
        let error = compiler
            .compile_model(
                &ResourceId::parse(&format!("test:block/invalid_plane_{index}")).unwrap(),
                cancel,
            )
            .unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "unexpected rejection: {error}"
        );
    }
}
mod uvlock_regressions {
    // Keep this correction's independent oracles together without changing existing regressions.
    use super::super::block_state::{ModelApplication, StateDefinition}; // Exercise both parsed applications and the public orientation guard.
    use super::super::models::ModelQuad;
    // Reuse the exact fixture/provider APIs already exercised by the enclosing module.
    // Compare complete transformed quads after checking UVs independently.
    use super::*;
    // The columns are Down, Up, North, South, West, East; rows are x_turns * 4 + y_turns.
    // Codes describe unit-square coordinates: 0=(u,v), 1=(1-v,u), 2=(1-u,1-v), 3=(v,1-u).
    // These literal results use integer Java rotations X:(x,z,-y), then Y:(-z,y,x), with nominal face tangents.
    fn uv_turn_table() -> [[u8; 6]; 16] {
        // Return a frozen oracle without consulting production geometry or rotation helpers.
        [
            // Enumerate every admitted pair of blockstate quarter turns.
            [0, 0, 0, 0, 0, 0], // x=0, y=0.
            [3, 1, 0, 0, 0, 0], // x=0, y=1.
            [2, 2, 0, 0, 0, 0], // x=0, y=2.
            [1, 3, 0, 0, 0, 0], // x=0, y=3.
            [0, 2, 2, 0, 3, 1], // x=1, y=0.
            [0, 2, 1, 1, 3, 1], // x=1, y=1.
            [0, 2, 0, 2, 3, 1], // x=1, y=2.
            [0, 2, 3, 3, 3, 1], // x=1, y=3.
            [0, 0, 2, 2, 2, 2], // x=2, y=0.
            [1, 3, 2, 2, 2, 2], // x=2, y=1.
            [2, 2, 2, 2, 2, 2], // x=2, y=2.
            [3, 1, 2, 2, 2, 2], // x=2, y=3.
            [2, 0, 2, 0, 1, 3], // x=3, y=0.
            [2, 0, 3, 3, 1, 3], // x=3, y=1.
            [2, 0, 0, 2, 1, 3], // x=3, y=2.
            [2, 0, 1, 1, 1, 3], // x=3, y=3.
        ] // End the complete 16-by-six UV oracle.
    } // End the UV oracle table.
    fn target_face_table() -> [[usize; 6]; 16] {
        // Independently freeze nominal destinations for complete-boundary controls.
        [
            // Use the same declared six-face ordering as the UV oracle.
            [0, 1, 2, 3, 4, 5], // x=0, y=0.
            [0, 1, 5, 4, 2, 3], // x=0, y=1.
            [0, 1, 3, 2, 5, 4], // x=0, y=2.
            [0, 1, 4, 5, 3, 2], // x=0, y=3.
            [3, 2, 0, 1, 4, 5], // x=1, y=0.
            [4, 5, 0, 1, 2, 3], // x=1, y=1.
            [2, 3, 0, 1, 5, 4], // x=1, y=2.
            [5, 4, 0, 1, 3, 2], // x=1, y=3.
            [1, 0, 3, 2, 4, 5], // x=2, y=0.
            [1, 0, 4, 5, 2, 3], // x=2, y=1.
            [1, 0, 2, 3, 5, 4], // x=2, y=2.
            [1, 0, 5, 4, 3, 2], // x=2, y=3.
            [2, 3, 1, 0, 4, 5], // x=3, y=0.
            [5, 4, 1, 0, 2, 3], // x=3, y=1.
            [3, 2, 1, 0, 5, 4], // x=3, y=2.
            [4, 5, 1, 0, 3, 2], // x=3, y=3.
        ] // End the complete nominal-direction oracle.
    } // End the destination table.
    fn expected_locked_uv(
        // Transform authored coordinates using only the frozen two-dimensional oracle.
        source_uv: [[f32; 2]; 4], // Accept already assigned per-vertex source coordinates.
        face: Direction, // Select the authored nominal face independently of physical tilt.
        x_turns: u8,     // Select an admitted X quarter turn.
        y_turns: u8,     // Select an admitted Y quarter turn.
    ) -> [[f32; 2]; 4] {
        // Return independently expected destination texture coordinates.
        let row = usize::from(x_turns) * 4 + usize::from(y_turns); // Flatten the complete orientation matrix.
        let turns = uv_turn_table()[row][face.index()]; // Read a literal expectation instead of deriving it from output normals.
        source_uv.map(|[u, v]| match turns {
            // Preserve each vertex's authored crop, flip, and face rotation.
            0 => [u, v], // Identity leaves both authored texture coordinates intact.
            1 => [1.0 - v, u], // A positive UV quarter turn rotates around the unit texture center.
            2 => [1.0 - u, 1.0 - v], // A UV half turn reverses both texture coordinates.
            3 => [v, 1.0 - u], // A negative UV quarter turn rotates around the unit texture center.
            _ => unreachable!("the frozen UV oracle contains only quarter turns"), // Reject an accidental oracle edit.
        }) // Finish all four independent corner expectations.
    } // End the UV expectation helper.
    fn assert_uv_eq(actual: [[f32; 2]; 4], expected: [[f32; 2]; 4]) {
        // Allow only trigonometric roundoff in tested output.
        for (actual, expected) in actual
            .into_iter()
            .flatten()
            .zip(expected.into_iter().flatten())
        {
            // Check all eight coordinates.
            assert!(
                (actual - expected).abs() < 1e-6,
                "UV {actual} != expected {expected}"
            ); // Also reject NaN through the failed comparison.
        } // Finish the numeric UV check.
    } // End the UV comparison helper.
    fn assert_orientation(
        // Assert UV semantics first, then compare every remaining field with an unlocked transform.
        quad: &ModelQuad,   // Borrow an immutable compiled source quad.
        model: &ResourceId, // Retain the actual compiled model identity in each application.
        x_turns: u8,        // Specify the blockstate X quarter turn.
        y_turns: u8,        // Specify the blockstate Y quarter turn.
    ) -> ModelQuad {
        // Return the checked locked quad for additional boundary or native checks.
        let application = ModelApplication {
            // Construct an admitted application using the existing public type.
            model: model.clone(), // Preserve the selected model identifier.
            x_turns,              // Preserve the tested X orientation.
            y_turns,              // Preserve the tested Y orientation.
            uvlock: false,        // Obtain the geometry/material control without UV locking.
            weight: 1,            // Use the normal single-application weight.
        }; // Finish the unlocked application.
        let unlocked = oriented_quad(quad, &application).unwrap(); // The existing unlocked geometry path must remain admitted.
        assert_eq!(unlocked.uv, quad.uv); // An unlocked application must retain authored texture coordinates.
        let application = ModelApplication {
            uvlock: true,
            ..application
        }; // Change only the UV-lock request.
        let locked = oriented_quad(quad, &application).unwrap_or_else(|error| {
            // Make the original tilted-normal rejection a hard regression failure.
            panic!(
                "face {:?}, element {}, x={x_turns}, y={y_turns}: {error}",
                quad.face, quad.element
            ); // Identify the exact failed nominal case.
        }); // Finish the actual locked transformation.
        let expected_uv = expected_locked_uv(quad.uv, quad.face, x_turns, y_turns); // Consult the independent table, never the actual result.
        assert_uv_eq(locked.uv, expected_uv); // Reject no-op locking, wrong direction, wrong face, and wrong crop pivot.
        let expected = ModelQuad {
            uv: locked.uv,
            ..unlocked
        }; // Exclude only the independently checked UV field from the control comparison.
        assert_eq!(locked, expected); // Preserve geometry, normals, culls, boundaries, tint, shade, material, element, and nominal face.
        assert_eq!(locked.face, quad.face); // Keep authored face identity distinct from the destination normal.
        if x_turns == 0 && y_turns == 0 {
            // Explicitly cover UV-lock identity even when the element is physically tilted.
            assert_eq!(locked.uv, quad.uv); // Identity may neither reject, unflip, rotate, nor recenter the authored UVs.
        } // Finish the identity-specific contract.
        locked // Return the verified result for caller-specific assertions.
    } // End the complete orientation assertion.
    fn uv_rectangle(flip: usize) -> [i32; 4] {
        // Keep independent U/V flips of an asymmetric partial crop explicit.
        match flip {
            // Use a crop whose midpoint differs from the unit texture center on both axes.
            0 => [3, 5, 11, 14], // Preserve both authored axes.
            1 => [11, 5, 3, 14], // Reverse only the U axis.
            2 => [3, 14, 11, 5], // Reverse only the V axis.
            3 => [11, 14, 3, 5], // Reverse both authored axes.
            _ => unreachable!("four independent flip combinations"), // Reject an invalid fixture index.
        } // End the four crop variants.
    } // End the literal crop helper.
    fn authored_uv(rectangle: [i32; 4], face_turns: usize) -> [[f32; 2]; 4] {
        // Check importer face assignment before testing blockstate locking.
        let [u0, v0, u1, v1] = rectangle.map(|coordinate| coordinate as f32 / 16.0); // Convert the authored texel rectangle exactly.
        let corners = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]; // Record the four authored rectangle corners directly.
        std::array::from_fn(|index| corners[(index + face_turns) % 4]) // Apply only the fixture's explicitly authored face rotation.
    } // End the source-coordinate oracle.
    fn plane_geometry(plane: usize) -> ([i32; 3], [i32; 3], [Direction; 2]) {
        // Cover all six nominal directions with zero-thickness geometry.
        match plane {
            // Keep each plane partial and asymmetric so it cannot claim a complete block boundary.
            0 => ([2, 3, 5], [13, 12, 5], [Direction::North, Direction::South]), // Pair the two sides of an XY plane.
            1 => ([4, 2, 3], [4, 14, 11], [Direction::West, Direction::East]), // Pair the two sides of a YZ plane.
            2 => ([3, 6, 2], [12, 6, 14], [Direction::Down, Direction::Up]), // Pair the two sides of an XZ plane.
            _ => unreachable!("three paired nominal planes"), // Reject an invalid fixture index.
        } // End the independent plane definitions.
    } // End the plane helper.
    fn tilted_fixture(axis: &str, angle: f64, rescale: bool) -> serde_json::Value {
        // Build one finite model containing every UV/nominal-face combination.
        let mut elements = Vec::new(); // Retain only 48 fixture elements, below the production 256-element limit.
        for element in 0..48 {
            // Expand three paired planes through four face rotations and four independent flips.
            let plane = element / 16; // Select the nominal plane without coupling it to the element rotation axis.
            let face_turns = (element % 16) / 4; // Cover face rotations 0, 90, 180, and 270 degrees.
            let rectangle = uv_rectangle(element % 4); // Cover normal, U-flipped, V-flipped, and doubly flipped partial UVs.
            let (from, to, directions) = plane_geometry(plane); // Use exact zero-thickness authored bounds.
            let mut faces = serde_json::Map::new(); // Author both visible sides independently.
            for direction in directions {
                // Preserve the paired nominal face identities.
                faces.insert(
                    direction.name().into(),
                    serde_json::json!({ // Use only currently supported face metadata.
                        "texture": format!("test:uv_plane_{plane}"), // Distinguish each nominal plane's selected texture.
                        "uv": rectangle, // Preserve the explicit partial rectangle and its signed extents.
                        "rotation": face_turns * 90, // Retain the independent authored face rotation.
                        "tintindex": plane, // Distinguish and verify the three authored tint indices.
                        "cullface": direction.name() // Demand conservative removal of partial/tilted cull hints.
                    }),
                ); // Finish one authored face.
            } // Finish both sides of this zero-thickness element.
            elements.push(
                serde_json::json!({ // Add the complete supported element definition.
                    "from": from, // Preserve the independent lower bounds.
                    "to": to, // Preserve the coincident plane coordinate without manufacturing thickness.
                    "rotation": {"origin": [4, 7, 8], "axis": axis, "angle": angle, "rescale": rescale}, // Use an asymmetric pivot and the tested element transform.
                    "shade": plane % 2 == 0, // Verify both shade states across otherwise comparable geometry.
                    "faces": faces // Preserve both visible nominal faces and all material metadata.
                }),
            ); // Finish the current synthetic element.
        } // Finish the bounded fixture expansion.
        serde_json::json!({"elements": elements}) // Return a model using only supplied importer contracts.
    } // End the tilted fixture builder.
    fn check_tilted_case(axis: &str, angle: f64, rescale: bool) -> usize {
        // Isolate ownership and quota checks for each complete transform case.
        let budget = ByteBudget::new(32 << 20).unwrap(); // Use a finite test-owned account below existing production ceilings.
        let stop = AtomicBool::new(false); // Own this case's cancellation lifetime.
        let cancel = Cancel::new(&stop); // Pass the actual cancellation token through every preparation operation.
        let mut defs = DefinitionSet::new(
            fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap(); // Use the existing explicit diagnostic provider.
        let model_id = ResourceId::parse("test:block/uv_tilt_matrix").unwrap(); // Retain one stable model identity for this scoped case.
        defs.insert_model(
            model_id.as_str(),
            &tilted_fixture(axis, angle, rescale),
            reason(),
            cancel,
        )
        .unwrap(); // Compile real parsed metadata rather than hand-built output quads.
        let source_baseline = budget.used(); // Record the real source-owner charge before compiler/cache allocations.
        {
            // Scope every cached model and compiler reservation inside the source lifetime.
            let mut compiler =
                ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap(); // Reuse the same admitted source account.
            let model = compiler.compile_model(&model_id, cancel).unwrap(); // Exercise actual element parsing and normalization.
            assert_eq!(model.quads.len(), 96); // Retain exactly two visible sides for all 48 zero-thickness elements.
            assert!(model.uses_budget(&budget)); // Keep normalized model ownership attached to the original account.
            assert_eq!(model.origins.len(), 1); // Preserve the fixture's complete one-model provenance chain.
            let original_quads = model.quads.clone(); // Snapshot all immutable cached geometry and authored attributes.
            let original_origins = serde_json::to_value(&model.origins).unwrap(); // Snapshot every provenance field, including the source digest.
            let original_ignored = model.ignored_non_world_fields.clone(); // Preserve metadata review records during orientation.
            let cached = compiler.compile_model(&model_id, cancel).unwrap(); // Exercise retained model cache identity.
            assert!(std::sync::Arc::ptr_eq(&model, &cached)); // Orientation must not replace the cached compiled model.
            assert!(model.quads.iter().any(|quad| quad
                .normal
                .iter()
                .filter(|component| component.abs() > 0.1)
                .count()
                > 1)); // Confirm the fixture actually contains non-cardinal physical normals.
            for quad in &model.quads {
                // Validate independent source expectations before any blockstate transformation.
                let element = usize::from(quad.element); // Decode the fixture's declared element layout.
                let plane = element / 16; // Locate the independently authored nominal plane.
                assert!(plane_geometry(plane).2.contains(&quad.face)); // Retain both original nominal face labels.
                assert_eq!(
                    quad.uv,
                    authored_uv(uv_rectangle(element % 4), (element % 16) / 4)
                ); // Preserve each explicit crop, flip, and face rotation exactly.
                assert_eq!(quad.texture.as_str(), format!("test:uv_plane_{plane}")); // Preserve the original selected texture identity.
                assert_eq!(quad.tint_index, Some(plane as u16)); // Preserve each source tint index.
                assert_eq!(quad.shade, plane % 2 == 0); // Preserve both authored shade states.
                assert!(quad.cull_face.is_none() && quad.complete_boundary.is_none());
                // Partial planes must never become complete occluding boundaries.
            } // Finish validation of all original normalized quads.
            let retained_before = budget.used(); // Record retained charges immediately before the constant-size orientation work.
            for sample in 0..96 * 16 {
                // Cover all 1,536 nominal/UV/application combinations in this element-transform case.
                let quad = &model.quads[sample / 16]; // Visit every compiled plane side with each blockstate orientation.
                let orientation = sample % 16; // Select the independently enumerated application row.
                assert_orientation(
                    quad,
                    &model_id,
                    (orientation / 4) as u8,
                    (orientation % 4) as u8,
                ); // Enforce independent UVs and complete unlocked-field equality.
            } // Finish every valid blockstate X/Y combination.
            assert_eq!(budget.used(), retained_before); // UV locking must neither acquire another account nor retain new reservations.
            assert_eq!(model.quads, original_quads); // Preserve the complete cached model after repeated transformations.
            assert_eq!(
                serde_json::to_value(&model.origins).unwrap(),
                original_origins
            ); // Preserve origins, source hashes, and compatibility labels.
            assert_eq!(model.ignored_non_world_fields, original_ignored); // Preserve all metadata review evidence.
            stop.store(true, Ordering::Release); // Cancel after cache population to exercise the cache-hit guard.
            assert!(matches!(
                compiler.compile_model(&model_id, cancel),
                Err(AssetError::Cancelled)
            )); // A warm model cache must not bypass cancellation.
            assert_eq!(budget.used(), retained_before); // Cancellation must leave retained-owner accounting unchanged.
        } // Release the compiler, both model handles, and all retained model charges.
        assert_eq!(budget.used(), source_baseline); // All compiler/cache reservations must return to the original source baseline.
        drop(defs); // Release the exact fixture source owner.
        assert_eq!(budget.used(), 0); // The scoped fixture must leave no retained account charge.
        96 * 16 // Report the exact number of independent locked/unlocked comparisons performed by this case.
    } // End one complete element-transform case.
    #[test] // Register the regression that forces the supplied tilted-normal implementation RED.
    fn tilted_uvlock_covers_nominal_faces_all_rotations_flips_crops_and_rescale() {
        // Cover each admitted nonzero element tilt and both rescale modes.
        let mut comparisons = 0; // Count the actual executed corpus rather than estimating coverage from fixture presence.
        for case in 0..24 {
            // Enumerate three axes, four signed angles, and two rescale states without nested expansion.
            let axis = ["x", "y", "z"][case / 8]; // Select an independent element rotation axis.
            let angle = [-45.0, -22.5, 22.5, 45.0][(case / 2) % 4]; // Include both signs and the nearest-cardinal tie cases.
            comparisons += check_tilted_case(axis, angle, case % 2 != 0); // Execute the complete nominal/UV/blockstate matrix for this case.
        } // Finish all nonzero admitted element transforms.
        assert_eq!(comparisons, 36_864); // Require the full 24-by-96-by-16 corpus to execute.
    } // End the comprehensive tilted-element regression.
    #[test] // Register fixed numeric cases that are readable without executing either oracle helper.
    fn uvlock_literal_coordinates_fix_texture_center_flip_and_face_rotation() {
        // Distinguish a true UV coordinate transform from corner permutation or a no-op.
        let source = [
            [0.6875, 0.3125],
            [0.6875, 0.875],
            [0.1875, 0.875],
            [0.1875, 0.3125],
        ]; // Freeze crop [3,5,11,14] with an authored 90-degree face rotation.
        let expected = [
            [0.6875, 0.6875],
            [0.125, 0.6875],
            [0.125, 0.1875],
            [0.6875, 0.1875],
        ]; // Freeze the Up/y=90 unit-center result independently.
        assert_eq!(expected_locked_uv(source, Direction::Up, 0, 1), expected); // Anchor the table convention to explicit asymmetric numeric coordinates.
        let source = [[0.75, 0.75], [0.625, 0.75], [0.625, 1.0], [0.75, 1.0]]; // Freeze the supplied main-vine leaf1 north flipped rectangle.
        let expected = [[0.25, 0.25], [0.375, 0.25], [0.375, 0.0], [0.25, 0.0]]; // Freeze its up-facing x=270 nominal UV result.
        assert_eq!(expected_locked_uv(source, Direction::North, 3, 0), expected);
        // Ensure native east/west no-op cases cannot mask the required up-side transform.
    } // End the literal numeric oracle checks.
    #[test] // Register an axis-aligned control across the same complete orientation matrix.
    fn axis_aligned_default_uvs_keep_all_complete_boundaries_and_cullfaces() {
        // Preserve established cube behavior while admitting tilted geometry.
        let budget = ByteBudget::new(32 << 20).unwrap(); // Use one finite account for source and normalized cube ownership.
        let stop = AtomicBool::new(false); // Keep the control's preparation lifetime live.
        let cancel = Cancel::new(&stop); // Use the normal compiler cancellation token.
        let mut defs = DefinitionSet::new(
            fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap(); // Retain explicit diagnostic provenance.
        let mut faces = serde_json::Map::new(); // Author all six complete boundaries without explicit UV rectangles.
        for face in Direction::ALL {
            // Exercise every default face mapping and authored cull direction.
            faces.insert(
                face.name().into(),
                serde_json::json!({"texture":"test:complete_cube","cullface":face.name()}),
            ); // Leave UVs absent to test the existing default-UV path.
        } // Finish the full axis-aligned cube source.
        let model_id = ResourceId::parse("test:block/uv_complete_cube").unwrap(); // Preserve a separate control model identity.
        defs.insert_model(
            model_id.as_str(),
            &serde_json::json!({"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":faces}]}),
            reason(),
            cancel,
        )
        .unwrap(); // Normalize genuine full-boundary geometry.
        let source_baseline = budget.used(); // Record the source-only account charge.
        {
            // Scope retained compiler and model owners.
            let mut compiler =
                ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap(); // Share the original fixture account.
            let model = compiler.compile_model(&model_id, cancel).unwrap(); // Compile the default-UV control with the real importer.
            assert_eq!(model.quads.len(), 6); // Retain every complete cube face.
            for sample in 0..6 * 16 {
                // Exercise all 96 complete-face/application combinations.
                let quad = &model.quads[sample / 16]; // Select one original full boundary.
                let row = sample % 16; // Select one admitted X/Y quarter-turn pair.
                assert_eq!(quad.uv, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]); // Preserve the six existing default unit-square mappings.
                assert_eq!(quad.complete_boundary, Some(quad.face)); // Require a genuine complete source boundary.
                assert_eq!(quad.cull_face, Some(quad.face)); // Preserve its valid authored cull hint.
                let locked = assert_orientation(quad, &model_id, (row / 4) as u8, (row % 4) as u8); // Check table-based UVs and complete unlocked-field equality.
                let target = Direction::ALL[target_face_table()[row][quad.face.index()]]; // Read the frozen nominal-direction oracle.
                assert_eq!(locked.complete_boundary, Some(target)); // Rotate the complete-boundary identity exactly once.
                assert_eq!(locked.cull_face, Some(target)); // Rotate the valid cull direction with the same blockstate transform.
                for (actual, expected) in locked.normal.into_iter().zip(target.step()) {
                    // Independently check destination axis normals.
                    assert!((actual - expected as f32).abs() < 1e-6); // Permit floating-point right-angle roundoff only.
                } // Finish the complete face's normal assertion.
            } // Finish all axis-aligned controls.
        } // Release the cube and its compiler/cache reservations.
        assert_eq!(budget.used(), source_baseline); // Preserve source-owner accounting after the complete-boundary matrix.
        drop(defs); // Release the control source.
        assert_eq!(budget.used(), 0); // Leave no retained reservations after the control completes.
    } // End the axis-aligned preservation regression.
    fn small_tilted_fixture() -> serde_json::Value {
        // Keep validation and low-budget controls small enough to reach the intended guard.
        serde_json::json!({"elements":[{ // Define one supported zero-thickness element with two explicit faces.
            "from":[2,3,5], "to":[13,12,5], // Keep the geometry partial and nondegenerate on its visible sides.
            "rotation":{"origin":[4,7,8],"axis":"x","angle":-22.5}, // Reproduce the admitted class of physical tilt.
            "faces":{ // Author explicit crop and flip metadata on both plane sides.
                "north":{"texture":"test:small_tilt","uv":[11,5,3,14]}, // Retain a flipped partial north-side crop.
                "south":{"texture":"test:small_tilt","uv":[3,5,11,14]} // Retain the independently authored opposite side.
            } // Finish the visible plane faces.
        }]}) // Return the complete small source model.
    } // End the small fixture helper.
    #[test] // Register public and parsed application guards alongside signed normalization.
    fn uvlock_keeps_invalid_application_rotations_rejected_and_signed_rotations_normalized() {
        // Preserve explicit rotation validation before UV locking.
        let budget = ByteBudget::new(32 << 20).unwrap(); // Share a finite source/compiler account for this guard test.
        let stop = AtomicBool::new(false); // Keep valid fixture preparation uncancelled.
        let cancel = Cancel::new(&stop); // Use the existing cancellation token type.
        let mut defs = DefinitionSet::new(
            fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap(); // Preserve diagnostic fixture provenance.
        let model_id = ResourceId::parse("test:block/uv_rotation_guard").unwrap(); // Use one stable model for parsed and direct application checks.
        defs.insert_model(model_id.as_str(), &small_tilted_fixture(), reason(), cancel)
            .unwrap(); // Reach orientation through the real element compiler.
        let source_baseline = budget.used(); // Preserve the source-only ownership baseline.
        {
            // Scope compiled model reservations independently of the source.
            let mut compiler =
                ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap(); // Reuse the admitted source account.
            let model = compiler.compile_model(&model_id, cancel).unwrap(); // Obtain actual tilted quads for all guard checks.
            let before = budget.used(); // Record the unchanged retained model/cache charge.
            let mut sensitive = model.quads[0].clone(); // Retain genuine compiled tilted geometry while probing representable UV identity edge cases.
            sensitive.uv = [
                [f32::from_bits(1), -0.0],
                [-f32::from_bits(1), 0.0],
                [1.0, f32::from_bits(2)],
                [f32::MIN_POSITIVE, -f32::MIN_POSITIVE],
            ]; // Include subnormals and signed zero that subtract/add around 0.5 can erase.
            let locked = assert_orientation(&sensitive, &model_id, 0, 0); // Preserve geometry/normal/material equality while exercising UV-lock identity.
            assert_eq!(
                locked.uv.map(|pair| pair.map(f32::to_bits)),
                sensitive.uv.map(|pair| pair.map(f32::to_bits))
            ); // Require every UV bit to survive the identity path, including negative zero.
            for case in 0..8 {
                // Cross invalid X/Y byte values with both UV-lock states.
                let (x_turns, y_turns) = [(4, 0), (0, 4), (255, 0), (0, 255)][case / 2]; // Cover both axes and extreme unchecked inputs.
                let application = ModelApplication {
                    model: model_id.clone(),
                    x_turns,
                    y_turns,
                    uvlock: case % 2 != 0,
                    weight: 1,
                }; // Preserve the public type's exact input surface.
                assert!(matches!(
                    oriented_quad(&model.quads[0], &application),
                    Err(AssetError::InvalidMetadata(_))
                )); // Invalid direct rotations must fail before either lock path.
            } // Finish every unchecked-rotation control.
            let state =
                BlockState::new(ResourceId::parse("test:uv_rotation_guard").unwrap(), []).unwrap(); // Use a real semantic blockstate for parsed selectors.
            let definition = StateDefinition::parse(&serde_json::json!({"variants":{"":{"model":model_id.as_str(),"x":-90,"y":450,"uvlock":true}}})).unwrap(); // Preserve modulo normalization of signed quarter-turn degrees.
            let applications = definition.select(&state, [7, 64, -3], 17).unwrap(); // Exercise the real application parser and selector.
            assert_eq!(applications.len(), 1); // Preserve the single selected model application.
            assert_eq!((applications[0].x_turns, applications[0].y_turns), (3, 1)); // Negative and wrapped turns must normalize without changing orientation semantics.
            assert!(applications[0].uvlock); // Preserve the parsed UV-lock request.
            let locked = oriented_quad(&model.quads[0], &applications[0]).unwrap(); // Apply the exact parsed application to an actual tilted face.
            assert_uv_eq(
                locked.uv,
                expected_locked_uv(model.quads[0].uv, model.quads[0].face, 3, 1),
            ); // Check parsed signed rotations against the same independent nominal oracle.
            for rotation in [
                serde_json::json!({"x":90.5}),
                serde_json::json!({"y":90.5}),
                serde_json::json!({"x":45}),
                serde_json::json!({"y":45}),
                serde_json::json!({"x":2147483648_i64}),
            ] {
                // Preserve fractional, non-quarter, and signed-range rejections.
                let mut application = rotation.as_object().unwrap().clone(); // Retain the deliberately invalid authored field exactly.
                application.insert("model".into(), model_id.as_str().into()); // Add only the required valid selected model identifier.
                application.insert("uvlock".into(), true.into()); // Ensure UV-lock presence cannot relax the parser guard.
                assert!(
                    StateDefinition::parse(&serde_json::json!({"variants":{"":application}}))
                        .is_err()
                ); // Reject invalid parsed rotations before model use.
            } // Finish the malformed application controls.
            assert_eq!(budget.used(), before); // Successful and rejected orientations must preserve retained-owner accounting.
        } // Release all normalized model/cache owners.
        assert_eq!(budget.used(), source_baseline); // Preserve source ownership after every guard path.
        drop(defs); // Release the admitted fixture source.
        assert_eq!(budget.used(), 0); // Leave no retained charge after guard validation.
    } // End application validation preservation.
    #[test] // Register a real bounded failure path without increasing any production limit.
    fn uvlock_preparation_cancellation_and_budget_failure_release_temporary_charges() {
        // Preserve explicit failures through the original compiler preparation boundary.
        let budget = ByteBudget::new(1 << 20).unwrap(); // Deliberately admit less than the compiler's existing four-MiB work reservation.
        let stop = AtomicBool::new(false); // Allow the small source to be admitted before cancellation checks.
        let cancel = Cancel::new(&stop); // Share one cancellation token for the source and compiler.
        let mut defs = DefinitionSet::new(
            fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap(); // Keep the finite low-budget source account explicit.
        let model_id = ResourceId::parse("test:block/uv_low_budget").unwrap(); // Retain an exact identity for both failure attempts.
        defs.insert_model(model_id.as_str(), &small_tilted_fixture(), reason(), cancel)
            .unwrap(); // Keep source metadata small enough to reach compiler work admission.
        let source_baseline = budget.used(); // Record the admitted source charge before attempting either failure path.
        {
            // Scope every temporary compiler owner.
            let mut compiler =
                ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap(); // Preserve the original low-budget account.
            stop.store(true, Ordering::Release); // Cancel before uncached model preparation.
            assert!(matches!(
                compiler.compile_model(&model_id, cancel),
                Err(AssetError::Cancelled)
            )); // Cancellation must win before source loading or work admission.
            assert_eq!(budget.used(), source_baseline); // Cancelled preparation must not retain new charges.
            stop.store(false, Ordering::Release); // Re-enable preparation to expose the genuine finite work limit.
            assert!(matches!(
                compiler.compile_model(&model_id, cancel),
                Err(AssetError::Limit {
                    resource: "working bytes",
                    ..
                })
            )); // Preserve bounded failure instead of bypassing work admission for tilted models.
            assert_eq!(budget.used(), source_baseline); // Failed preparation must release parsed metadata and all temporary reservations.
        } // Release the empty compiler and any cache bookkeeping.
        assert_eq!(budget.used(), source_baseline); // Keep source ownership unchanged after the compiler drops.
        drop(defs); // Release the final fixture source owner.
        assert_eq!(budget.used(), 0); // Require full accounting recovery after both failure paths.
    } // End the budget and cancellation regression.
    #[test] // Register all authored rotation-domain guards through the real model compiler.
    fn uvlock_keeps_invalid_element_and_face_rotations_rejected_without_retained_charges() {
        // Preserve explicit metadata failures independently of blockstate application validation.
        let budget = ByteBudget::new(32 << 20).unwrap(); // Share one finite account across every malformed source and compiler attempt.
        let stop = AtomicBool::new(false); // Keep validation uncancelled so each intended metadata guard is reached.
        let cancel = Cancel::new(&stop); // Use the actual preparation cancellation token.
        let mut defs = DefinitionSet::new(
            fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap(); // Retain explicit diagnostic ownership for all malformed inputs.
        for case in 0..7 {
            // Give each distinct invalid authored field its own immutable model identity.
            let mut value = small_tilted_fixture(); // Begin with otherwise valid explicit-UV tilted geometry.
            match case {
                // Corrupt exactly one active rotation field in each source.
                0 => value["elements"][0]["rotation"]["angle"] = 30.into(), // Reject an unsupported non-quarter element tilt.
                1 => value["elements"][0]["rotation"]["angle"] = 90.into(), // Keep blockstate quarter-turn permissions separate from element-angle permissions.
                2 => value["elements"][0]["rotation"]["axis"] = "w".into(), // Reject an unknown element rotation axis.
                3 => value["elements"][0]["faces"]["north"]["rotation"] = 45.into(), // Reject a non-quarter face-UV rotation.
                4 => value["elements"][0]["faces"]["north"]["rotation"] = 360.into(), // Preserve the existing explicit face-rotation range rather than applying state modulo rules.
                5 => value["elements"][0]["faces"]["north"]["rotation"] = 90.5.into(), // Reject a fractional authored face-UV rotation.
                6 => value["elements"][0]["rotation"]["rescale"] = "true".into(), // Keep rescale strictly boolean while adding tilted UV-lock support.
                _ => unreachable!("seven invalid authored rotation cases"), // Reject an accidental fixture-range change.
            } // Finish the one-field corruption.
            defs.insert_model(
                &format!("test:block/uv_invalid_authored_{case}"),
                &value,
                reason(),
                cancel,
            )
            .unwrap(); // Preserve a distinct recoverable source identity for each guard.
        } // Finish source admission without compiling invalid geometry.
        let source_baseline = budget.used(); // Record all seven admitted source-owner charges.
        {
            // Scope every normalization attempt and its temporary reservations.
            let mut compiler =
                ModelCompiler::new(&defs, Limits::default(), budget.clone()).unwrap(); // Reuse the original source account and unchanged limits.
            let expected_messages = [
                "unsupported element rotation angle",
                "unsupported element rotation angle",
                "rotation axis",
                "face UV rotation must be 0/90/180/270",
                "face UV rotation must be 0/90/180/270",
                "expected unsigned 32-bit integer",
                "expected JSON boolean",
            ]; // Require the intended guard rather than a missing-definition or unrelated error.
            for (case, expected_message) in expected_messages.into_iter().enumerate() {
                // Compile every independently admitted malformed source.
                let model_id =
                    ResourceId::parse(&format!("test:block/uv_invalid_authored_{case}")).unwrap(); // Request the exact fixture identity without replacing any definition.
                let error = compiler.compile_model(&model_id, cancel).unwrap_err(); // Reach the real element or face metadata validator.
                assert!(matches!(&error, AssetError::InvalidMetadata(_))); // Preserve explicit typed rejection for every malformed authored rotation.
                assert!(
                    error.to_string().contains(expected_message),
                    "unexpected authored-rotation rejection: {error}"
                ); // Verify each failure originates in the intended semantic guard.
                assert_eq!(budget.used(), source_baseline); // Release parsed metadata, work admission, and every temporary model charge after each failure.
            } // Finish all seven real importer rejections.
        } // Release the empty compiler and any cache bookkeeping.
        assert_eq!(budget.used(), source_baseline); // Keep source ownership unchanged after all failed normalization attempts.
        drop(defs); // Release the exact seven diagnostic source owners.
        assert_eq!(budget.used(), 0); // Require complete accounting recovery after invalid authored metadata.
    } // End authored rotation validation preservation.
      // Exact packet JSON is diagnostic evidence; no source-pack pixels are bundled.
    fn vine_sources() -> [(&'static str, &'static str, &'static str); 5] {
        // List exact members and digests.
        [
            // Preserve the packet's state-then-model order.
            (
                // The ten-clause multipart blockstate.
                "minecraft:vine", // Canonical blockstate resource.
                include_str!("model_fixtures/uvlock-00-vine.json"), // Retain original bytes.
                "0c79b856dde3d66dfce34790a6685c950ec5c5293344803a63eec1aa457ea2dd", // Original SHA256.
            ), // Finish the state record.
            (
                // The base eight-quad vine.
                "minecraft:block/vine", // Canonical model resource.
                include_str!("model_fixtures/uvlock-01-vine.json"), // Retain original bytes.
                "7248c35300a064510be90f6fa0610e046fd72bd6911333a1372f6735a651f31d", // Original SHA256.
            ), // Finish the base model.
            (
                // The first six-quad alternative.
                "minecraft:block/vine_alt_1", // Canonical model resource.
                include_str!("model_fixtures/uvlock-02-vine_alt_1.json"), // Retain original bytes.
                "3ced10c9d02fb5841c6ea0002711f726d71aee2c2d3538399f1c21ac32ca013f", // Original SHA256.
            ), // Finish the first alternative.
            (
                // The second six-quad alternative.
                "minecraft:block/vine_alt_2", // Canonical model resource.
                include_str!("model_fixtures/uvlock-03-vine_alt_2.json"), // Retain original bytes.
                "8c6a7e14c916d6358cdda68843b589e76da29b8b599cda703555f4017efee7e2", // Original SHA256.
            ), // Finish the second alternative.
            (
                // The third six-quad alternative.
                "minecraft:block/vine_alt_3", // Canonical model resource.
                include_str!("model_fixtures/uvlock-04-vine_alt_3.json"), // Retain original bytes.
                "7e9b149724661d4806e9719dabaa4122896d6739036b430aae5bb83dd6ffc2b9", // Original SHA256.
            ), // Finish the third alternative.
        ] // Return the complete fixed fixture inventory.
    } // Finish the fixture manifest.
    struct VineDefinitions {
        // Use the real source-blob/document path in this test provider.
        budget: ByteBudget, // Charge every parsed member to the compiler's existing account.
    } // Finish the test provider's state.
    impl DefinitionProvider for VineDefinitions {
        // Supply definitions without reserializing their JSON.
        fn definition(
            // Implement the existing provider contract.
            &self,              // Borrow the shared test account.
            key: &ResourceKey,  // Resolve the exact requested resource kind and name.
            cancel: Cancel<'_>, // Preserve caller cancellation.
        ) -> super::super::error::Result<Option<DefinitionInput>> {
            // Return actual importer inputs.
            cancel.check()?; // Reject cancelled reads before allocating source bytes.
            let members = vine_sources(); // Borrow the fixed five-member inventory.
            let found = members.iter().enumerate().find(|(index, member)| {
                // Match name and kind.
                let kind = if *index == 0 {
                    ResourceKind::Blockstate
                } else {
                    ResourceKind::Model
                }; // Keep kinds distinct.
                key.kind == kind && key.id.as_str() == member.0 // Reject cross-kind aliases.
            }); // Finish bounded lookup.
            let Some((_, (_, source, hash))) = found else {
                return Ok(None);
            }; // Preserve real absence.
            let expected = super::super::identity::Digest256::try_from((*hash).to_owned())?; // Validate the digest.
            let limits = Limits::default(); // Preserve all existing format limits.
            let blob = SourceBlob::new(
                // Admit original bytes through the real source implementation.
                source.as_bytes().to_vec(), // Copy only the requested fixed member.
                fixture_origin(OriginKind::DiagnosticFixture), // Never claim selected artwork acceptance.
                Some(expected), // Detect any fixture byte drift, including terminal newlines.
                &limits,        // Apply existing encoded-byte limits.
                &self.budget,   // Reuse the actual compiler account.
                cancel,         // Keep admission cancellable.
            )?; // Propagate integrity, resource, and cancellation failures.
            let document = Document::parse(&blob, &limits, &self.budget, cancel)?; // Retain original SHA256.
            Ok(Some(DefinitionInput {
                document,
                compatibility: None,
            })) // Do not invent fallback provenance.
        } // Finish the provider method.
    } // Finish the provider implementation.
    #[test] // Exercise every supplied alternative and actual blockstate orientation.
    fn uvlock_exact_vines_keep_authored_planes_and_multipart_selection() {
        // Reproduce the native trigger.
        let budget = ByteBudget::new(64 << 20).unwrap(); // Use one ordinary bounded account.
        let stop = AtomicBool::new(false); // Own test cancellation.
        let cancel = Cancel::new(&stop); // Borrow the cancellation state.
        let provider = VineDefinitions {
            budget: budget.clone(),
        }; // Keep source and compiler accounts equal.
        let members = vine_sources(); // Keep expected original identities in one place.
        let mut compiler =
            ModelCompiler::new(&provider, Limits::default(), budget.clone()).unwrap(); // Use the real compiler.
        let orientations = [
            (0_u8, 0_u8, false),
            (0, 1, true),
            (0, 2, true),
            (0, 3, true),
            (3, 0, true),
        ]; // North, east, south, west, up.
        let mut checked_quads = 0_usize; // Count actual authored quads, not model facades.
        let mut checked_tilted = 0_usize; // Disclose the non-axis-aligned denominator.
        for (index, (id, _, hash)) in members.iter().enumerate().skip(1) {
            // Force all four alternatives directly.
            let model = compiler
                .compile_model(&ResourceId::parse(id).unwrap(), cancel)
                .unwrap(); // Compile actual JSON.
            assert_eq!(model.quads.len(), if index == 1 { 8 } else { 6 }); // Preserve both sides of every plane.
            assert_eq!(model.origins.len(), 1); // No substitute parent or compatibility model is introduced.
            assert_eq!(model.origins[0].resource.id, model.id); // Keep the authored model identity.
            assert_eq!(model.origins[0].resource.kind, ResourceKind::Model); // Keep model custody typed.
            assert_eq!(model.origins[0].sha256.to_string(), *hash); // Keep the exact original-byte hash.
            assert_eq!(
                model.origins[0].origin,
                fixture_origin(OriginKind::DiagnosticFixture)
            ); // Preserve the honest source role.
            assert!(model.origins[0].compatibility.is_none()); // Never hide rejection behind compatibility geometry.
            let before_quads = model.quads.clone(); // Snapshot immutable cached geometry.
            let before_origins = serde_json::to_value(&model.origins).unwrap(); // Snapshot complete provenance.
            let before_bytes = budget.used(); // Snapshot the actual retained charge.
            for quad in &model.quads {
                // Exercise every face of this actual alternative.
                assert!(!quad.shade); // Preserve the source's disabled directional shading.
                assert_eq!(quad.tint_index, Some(0)); // Preserve authored biome tint selection.
                assert_eq!(quad.cull_face, None); // Keep partial planes uncullable.
                assert_eq!(quad.complete_boundary, None); // Never invent an opaque block boundary.
                let expected_texture = if quad.element == 0 {
                    "minecraft:block/vine"
                } else {
                    "minecraft:block/vine_leaves"
                }; // Preserve selected texture references.
                assert_eq!(quad.texture.as_str(), expected_texture); // Check both stem and leaf material IDs.
                if quad.element != 0 {
                    // Require physically tilted leaf normals.
                    assert_eq!(
                        quad.normal
                            .iter()
                            .filter(|value| value.abs() > 1e-6)
                            .count(),
                        2
                    ); // Detect flattened leaves.
                    checked_tilted += 1; // Count each authored tilted face exactly once.
                } // Finish physical tilt checks.
                for (x_turns, y_turns, uvlock) in orientations {
                    // Include native unlocked north and locked up.
                    let application = ModelApplication {
                        model: model.id.clone(),
                        x_turns,
                        y_turns,
                        uvlock,
                        weight: 1,
                    }; // Keep exact native turns.
                    let actual = oriented_quad(quad, &application).unwrap(); // Original source rejects tilted locked faces here.
                    let mut plain_application = application.clone(); // Prepare identical geometric orientation.
                    plain_application.uvlock = false; // Change only the lock flag for the preservation control.
                    let mut expected = oriented_quad(quad, &plain_application).unwrap(); // Preserve every non-UV field.
                    if x_turns == 3 && quad.face == Direction::North {
                        // Independently known upward north-face transform.
                        expected.uv = quad.uv.map(|[u, v]| [1.0 - u, 1.0 - v]); // Preserve the native reversed U endpoints.
                    } // Other native N/S charts use the identity mapping.
                    assert_eq!(actual, expected); // Check UVs and the entire geometric/material/culling record.
                } // Finish this face's five actual orientations.
                checked_quads += 1; // Count each emitted authored face once.
            } // Finish actual face coverage.
            assert_eq!(model.quads, before_quads); // The shared cached model must remain unchanged.
            assert_eq!(
                serde_json::to_value(&model.origins).unwrap(),
                before_origins
            ); // Keep every origin field unchanged.
            assert_eq!(budget.used(), before_bytes); // Orientation must not retain new resource charges.
        } // Finish all four independently forced alternatives.
        assert_eq!((checked_quads, checked_tilted), (26, 18)); // Pin actual total and tilted-face denominators.
        let base = compiler
            .compile_model(&ResourceId::parse(members[1].0).unwrap(), cancel)
            .unwrap(); // Reuse the exact main vine.
        let leaf = base
            .quads
            .iter()
            .find(|quad| quad.element == 1 && quad.face == Direction::North)
            .unwrap(); // Select leaf1's flipped north face.
        assert_eq!(
            leaf.uv,
            [[0.75, 0.75], [0.625, 0.75], [0.625, 1.0], [0.75, 1.0]]
        ); // Pin authored crop corner order.
        let application = ModelApplication {
            model: base.id.clone(),
            x_turns: 3,
            y_turns: 0,
            uvlock: true,
            weight: 1,
        }; // Apply native up rotation.
        let locked = oriented_quad(leaf, &application).unwrap(); // Exercise the corrected nominal UV chart.
        assert_eq!(
            locked.uv,
            [[0.25, 0.25], [0.375, 0.25], [0.375, 0.0], [0.25, 0.0]]
        ); // Reject a no-op lock or inverse crop.
        drop(base); // Release the additional caller-held model owner.
        for mask in 0_u32..32 {
            // Exercise all five-boolean native connection states.
            let names = ["north", "east", "south", "west", "up"]; // Match the packet's multipart order.
            let properties = names.iter().enumerate().map(|(bit, name)| {
                // Construct the actual connection predicates.
                (
                    (*name).to_owned(),
                    if mask & (1 << bit) != 0 {
                        "true"
                    } else {
                        "false"
                    }
                    .to_owned(),
                ) // Keep all five explicit booleans.
            }); // Finish state properties.
            let state =
                BlockState::new(ResourceId::parse("minecraft:vine").unwrap(), properties).unwrap(); // Parse the real state.
            let compiled = compiler
                .compile_state(&state, [-3, 72, 9], 17, cancel)
                .unwrap(); // Use real deterministic multipart selection.
            assert_eq!(
                compiled.applications.len(),
                if mask == 0 {
                    5
                } else {
                    mask.count_ones() as usize
                }
            ); // Preserve all-false special behavior.
            assert_eq!(compiled.state_origin.sha256.to_string(), members[0].2); // Keep original blockstate bytes authoritative.
            assert_eq!(
                compiled.state_origin.origin,
                fixture_origin(OriginKind::DiagnosticFixture)
            ); // Preserve diagnostic source custody.
            assert!(compiled.state_origin.compatibility.is_none()); // No replacement blockstate is supplied.
            let mut expected_orientations =
                orientations
                    .into_iter()
                    .enumerate()
                    .filter_map(|(bit, value)| {
                        // Retain admitted clause order.
                        if mask == 0 || mask & (1 << bit) != 0 {
                            Some(value)
                        } else {
                            None
                        } // Include exactly the matching native clauses.
                    }); // Finish independent expected application selection.
            for (actual, model) in &compiled.applications {
                // Bind all selected source alternatives.
                assert_eq!(
                    (actual.x_turns, actual.y_turns, actual.uvlock),
                    expected_orientations.next().unwrap()
                ); // Keep exact application semantics.
                assert!(members[1..]
                    .iter()
                    .any(|member| member.0 == model.id.as_str())); // Reject unlisted replacement models.
                for quad in &model.quads {
                    // Ensure every selected model is actually orientable.
                    oriented_quad(quad, actual).unwrap(); // Selection success alone cannot conceal a binding rejection.
                } // Finish selected-quad admission.
            } // Finish actual selected applications.
            assert!(expected_orientations.next().is_none()); // No native clause was silently dropped.
        } // Finish all native connection states.
        drop(compiler); // Release all retained normalized models and cache charges.
        assert_eq!(budget.used(), 0); // The provider owns no uncharged retained source or model storage.
    } // Finish exact native source regression.
} // End the tilted-element UV-lock regression module.
