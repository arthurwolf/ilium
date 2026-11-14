#![cfg(feature = "testing")]
//! A real `ilium:anim` component draws through the host `frame-surface` resource: the scene
//! guest links the resource, constructs the renderer, lends a borrowed surface per frame and
//! returns the presented pixels.

use std::collections::BTreeMap;
use std::sync::Arc;

use ilium_plugin_api::generated::anim::Readiness;
use ilium_plugin_api::ProcessKind;
use ilium_plugin_host::calls::ResolvedLimits;
use ilium_plugin_host::engine::{CompiledComponent, EngineConfig, PluginEngine};
use ilium_plugin_host::imports::generated_dispatch::ImportSet;
use ilium_plugin_host::scene_guest::{SceneFrameInput, SceneGuest, SceneTraits};
use ilium_plugin_host::store_data::GuestIdentity;
use ilium_plugin_host::testing::wat::scene_component_text;
use ilium_plugin_host::testing::wat_component;

fn engine() -> Arc<PluginEngine> {
    Arc::new(
        PluginEngine::new(EngineConfig {
            epoch_tick_ms: 10,
            cache_dir: None,
            process: ProcessKind::Client,
        })
        .expect("real engine"),
    )
}

fn compile(engine: &PluginEngine, text: &str) -> CompiledComponent {
    engine
        .compile(&wat_component(text))
        .expect("real component")
}

fn scene(engine: Arc<PluginEngine>, text: &str) -> SceneGuest {
    let compiled = compile(&engine, text);
    SceneGuest::instantiate(
        engine,
        &compiled,
        ResolvedLimits(BTreeMap::new()),
        ImportSet::default(),
        GuestIdentity::anonymous(),
        r#"{"scene":"fixture","settings":{},"palette":[],"mode":"live"}"#,
    )
    .expect("the scene instantiates and constructs its renderer")
}

fn input(columns: u32, rows: u32) -> SceneFrameInput {
    SceneFrameInput {
        time_millis: 40,
        wall_millis: 40,
        now_unix_millis: 1_700_000_000_000,
        columns,
        rows,
        palette_rgb: vec![0x00ff_0000, 0x0000_00ff],
    }
}

#[test]
fn a_wat_scene_draws_its_pixels_through_the_host_surface() {
    let mut guest = scene(engine(), &scene_component_text(200));
    assert_eq!(
        guest.traits(),
        SceneTraits {
            frames_per_second: 7,
            wants_pointer: false,
            wants_occupancy: false,
            uses_cell_colors: false,
        }
    );
    assert_eq!(guest.readiness().expect("readiness"), Readiness::Ready);
    let frame = guest
        .render(&input(3, 2))
        .expect("the frame renders")
        .expect("the scene presented");
    assert_eq!((frame.width, frame.height), (6, 8));
    assert_eq!((frame.columns, frame.rows), (3, 2));
    assert_eq!(frame.pixels, vec![200; 48]);
    assert_eq!(frame.cell_colors, None);
    assert_eq!(guest.status().expect("status"), None);
}

#[test]
fn the_ink_is_the_guests_own_and_follows_resizes() {
    let mut guest = scene(engine(), &scene_component_text(37));
    let small = guest.render(&input(1, 1)).unwrap().unwrap();
    assert_eq!(small.pixels, vec![37; 8]);
    let large = guest.render(&input(40, 10)).unwrap().unwrap();
    assert_eq!((large.width, large.height), (80, 40));
    assert!(large.pixels.iter().all(|pixel| *pixel == 37));
}

#[test]
fn hundreds_of_frames_lend_and_return_their_surface_handles() {
    let mut guest = scene(engine(), &scene_component_text(9));
    for _ in 0..500 {
        assert!(guest.render(&input(4, 3)).expect("renders").is_some());
    }
    assert_eq!(guest.surface().lock().unwrap().frame(), 500);
}

#[test]
fn reconfigure_and_palette_reach_the_renderer() {
    let mut guest = scene(engine(), &scene_component_text(1));
    assert!(guest.reconfigure(r#"{"speed":2}"#).expect("reconfigure"));
    guest.set_palette(&[0x0012_3456]).expect("palette");
    assert_eq!(guest.traits().frames_per_second, 7);
}

#[test]
fn a_scene_that_traps_while_rendering_reports_a_panic_trap() {
    let text =
        scene_component_text(5).replace("(call $present (local.get $surface))", "unreachable");
    assert_ne!(text, scene_component_text(5));
    let mut guest = scene(engine(), &text);
    let trap = guest.render(&input(2, 2)).expect_err("the guest traps");
    assert_eq!(trap.code.as_str(), "trap.panic");
    assert_eq!(trap.function, "[method]scene-renderer.render");
}

#[test]
fn a_frame_without_present_is_dropped_not_shown() {
    let text = scene_component_text(5).replace("(call $present (local.get $surface))", "");
    let mut guest = scene(engine(), &text);
    assert_eq!(guest.render(&input(2, 2)).expect("renders"), None);
}

#[test]
fn an_empty_field_makes_no_guest_call() {
    let mut guest = scene(engine(), &scene_component_text(5));
    assert_eq!(guest.render(&input(0, 9)).expect("no call"), None);
    assert_eq!(guest.surface().lock().unwrap().frame(), 0);
}

#[test]
fn a_component_without_a_renderer_does_not_become_a_scene() {
    let engine = engine();
    let compiled = compile(&engine, "(component)");
    let refused = SceneGuest::instantiate(
        engine,
        &compiled,
        ResolvedLimits(BTreeMap::new()),
        ImportSet::default(),
        GuestIdentity::anonymous(),
        "{}",
    );
    assert!(refused.is_err());
}

/// Equips a scene the way the client does (`wiring::scene`) and runs one frame.
fn equipped_scene(
    engine: Arc<PluginEngine>,
    text: &str,
    granted: &[&str],
    observer: Option<(f64, f64)>,
) -> Result<SceneGuest, ilium_plugin_host::HostError> {
    use ilium_plugin_host::wiring::scene::{SceneImportServices, SceneInstanceRequest};
    let compiled = compile(&engine, text);
    let granted: Vec<String> = granted.iter().map(|grant| (*grant).to_owned()).collect();
    let limits = ResolvedLimits(BTreeMap::new());
    let equipped = SceneImportServices::default().equip(
        &SceneInstanceRequest {
            plugin: "test.scene",
            granted: &granted,
            manifest: &serde_json::Value::Null,
            observer,
        },
        &engine,
        &compiled,
        &limits,
    );
    SceneGuest::instantiate(
        engine,
        &compiled,
        limits,
        equipped.imports,
        equipped.identity,
        "{}",
    )
    .map(|guest| guest.with_background(equipped.background))
}

#[test]
fn an_equipped_scene_pumps_its_background_and_draws() {
    let mut guest = equipped_scene(
        engine(),
        &scene_component_text(9),
        &["jobs.run", "http:api.example.org"],
        None,
    )
    .expect("equipped scene starts");
    let pump = guest.pump_background().expect("pump");
    assert_eq!(pump.jobs_started, 0);
    assert!(!guest.handles_events());
    let frame = guest
        .render(&input(1, 1))
        .expect("renders")
        .expect("presented");
    assert!(frame.pixels.iter().all(|pixel| *pixel == 9));
}

#[test]
fn the_location_import_answers_the_observer_position_to_a_granted_scene() {
    let text = ilium_plugin_host::testing::wat::scene_component_with_location_text();
    let mut located = equipped_scene(
        engine(),
        &text,
        &["location.observer"],
        Some((-33.87, 151.21)),
    )
    .expect("granted scene starts");
    assert_eq!(
        located.status().expect("status"),
        Some("located".to_owned())
    );
    let mut unset = equipped_scene(engine(), &text, &["location.observer"], None)
        .expect("granted scene starts");
    assert_eq!(
        unset.status().expect("status"),
        Some("unlocated".to_owned())
    );
    let refused = equipped_scene(engine(), &text, &[], Some((-33.87, 151.21)));
    let error = refused.expect_err("an ungranted location import is refused");
    assert!(error.to_string().contains("import-ungranted"), "{error}");
}
