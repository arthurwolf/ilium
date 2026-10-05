use super::*;
use crate::app::{FocusTarget, SettingsState, SettingsTab};
use crate::background_animation::{AmbientHost, AnimationPlaybackMode};
use crate::semantic_animation::{ProposedParameter, ProposedRecommendation};
use ilium_ambient::{AmbientKind, AmbientSettings, ControlValue, Frame, Scene};
use ilium_core::animation_recommendation::{
    PlanAnimationEntry, RecommendedRestructurePlan, ResourcePolicy,
};
use ilium_core::{PaneContentKind, RestructureNode, RestructurePlan, Tree};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
fn recommendation(
    kind: AnimationKind,
    parameters: Vec<(&str, ControlValue)>,
) -> AnimationRecommendation {
    let raw = ProposedRecommendation {
        kind: serde_json::to_value(kind)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned(),
        resources: ResourcePolicy::Catalog,
        parameters: parameters
            .into_iter()
            .map(|(id, value)| ProposedParameter {
                id: id.into(),
                value,
            })
            .collect(),
    };
    crate::semantic_animation::validate_recommendation(&raw, &AnimationSettings::default()).unwrap()
}
fn paris() -> AnimationRecommendation {
    recommendation(
        AnimationKind::OpenStreetMap,
        vec![
            ("source", ControlValue::Index(0)),
            ("place", ControlValue::Index(0)),
            ("tour", ControlValue::Index(0)),
        ],
    )
}
fn carpet(mode: usize) -> AnimationRecommendation {
    recommendation(
        AnimationKind::Carpet,
        vec![("carpet_mode", ControlValue::Index(mode))],
    )
}
fn snapshot_recommendation(
    tree: &mut Tree,
    id: NodeId,
    recommendation: Option<AnimationRecommendation>,
    generation: u64,
) {
    let mut document = serde_json::to_value(&*tree).unwrap();
    let node = document["nodes"]
        .as_object_mut()
        .unwrap()
        .get_mut(&id.0.to_string())
        .unwrap();
    node["inferred_animation"] = serde_json::to_value(recommendation).unwrap();
    node["animation_generation"] = serde_json::json!(generation);
    *tree = serde_json::from_value(document).unwrap();
}
struct Fixture {
    app: App,
    project: NodeId,
    group: NodeId,
    pane: NodeId,
    other_pane: NodeId,
    other_path: PathBuf,
}
fn fixture() -> Fixture {
    let path = std::env::temp_dir().join("ilium-semantic-fixture-a");
    let other_path = std::env::temp_dir().join("ilium-semantic-fixture-b");
    let mut app = App::new("semantic-fixture".into(), path.clone());
    let project = app.tree.add_project(path).unwrap();
    let group = app.tree.add_group(project, "group").unwrap();
    let pane = app
        .tree
        .add_pane(group, "pane", PaneContentKind::Terminal)
        .unwrap();
    let other_project = app.tree.add_project(other_path.clone()).unwrap();
    let other_pane = app
        .tree
        .add_pane(other_project, "other", PaneContentKind::Terminal)
        .unwrap();
    snapshot_recommendation(&mut app.tree, project, Some(paris()), 0);
    snapshot_recommendation(&mut app.tree, group, Some(carpet(0)), 0);
    snapshot_recommendation(&mut app.tree, pane, Some(carpet(1)), 0);
    snapshot_recommendation(&mut app.tree, other_project, Some(carpet(7)), 0);
    snapshot_recommendation(&mut app.tree, other_pane, Some(carpet(8)), 0);
    app.animation_settings.kind = AnimationKind::Semantic;
    app.animation_settings.playback_mode = AnimationPlaybackMode::Live;
    app.select_node(pane);
    app.set_screen_area(Rect::new(0, 0, 80, 24));
    Fixture {
        app,
        project,
        group,
        pane,
        other_pane,
        other_path,
    }
}
#[derive(Default)]
struct Probe {
    alive: AtomicUsize,
    builds: Mutex<Vec<(AmbientKind, AmbientSettings)>>,
    status: Mutex<String>,
}
struct FakeScene {
    probe: Arc<Probe>,
    fps: u32,
}
impl Drop for FakeScene {
    fn drop(&mut self) {
        self.probe.alive.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Scene for FakeScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        frame.raster.dots.fill(0.8);
        frame.cell_colors.fill([120, 180, 220]);
    }
    fn frames_per_second(&self) -> u32 {
        self.fps
    }
    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn status(&self) -> Option<String> {
        Some(self.probe.status.lock().unwrap().clone())
    }
}
fn install_probe(app: &mut App) -> Arc<Probe> {
    let probe = Arc::new(Probe::default());
    *probe.status.lock().unwrap() =
        "fixture provider; observed unknown; received fixture; TAIL_MARKER".into();
    let shared = Arc::clone(&probe);
    *app.animation_frame.host_mut() =
        AmbientHost::with_factory(Box::new(move |kind, settings, _env| {
            shared.builds.lock().unwrap().push((kind, settings.clone()));
            assert_eq!(shared.alive.fetch_add(1, Ordering::SeqCst), 0);
            Box::new(FakeScene {
                probe: Arc::clone(&shared),
                fps: if kind == AmbientKind::OpenStreetMap {
                    7
                } else {
                    11
                },
            })
        }));
    probe
}
fn compose(app: &mut App, seconds: u64) -> Buffer {
    let mut buffer = Buffer::empty(app.layout.screen_area);
    crate::background_composition::compose_ready_for_test(
        &mut buffer,
        app,
        Duration::from_secs(seconds),
    );
    buffer
}
#[test]
fn semantic_project_entry_group_and_pane_fallback_are_local() {
    let mut fixture = fixture();
    fixture.app.focus = FocusTarget::Pane;
    fixture.app.right_panel_target = RightPanelTarget::Pane {
        pane_id: fixture.other_pane,
    };
    assert_eq!(
        fixture.app.effective_animation_kind(),
        Some(AnimationKind::OpenStreetMap)
    );
    let authored = fixture.app.animation_settings.clone();
    fixture.app.animation_settings.semantic_scope = SemanticScope::Entry;
    assert_eq!(
        fixture
            .app
            .effective_animation_settings()
            .unwrap()
            .ambient
            .carpet
            .mode,
        1
    );
    fixture.app.select_node(fixture.group);
    assert_eq!(
        fixture
            .app
            .effective_animation_settings()
            .unwrap()
            .ambient
            .carpet
            .mode,
        0
    );
    let mut virtual_path = fixture.app.path_to(fixture.group);
    virtual_path.push(NodeId(u64::MAX));
    fixture.app.select_tree_path(virtual_path);
    assert_eq!(
        fixture
            .app
            .effective_animation_settings()
            .unwrap()
            .ambient
            .carpet
            .mode,
        0
    );
    fixture.app.tree_state.select(Vec::new());
    // The authored animation is one global setting: the other project is
    // resolved against the same settings, with no per-project load or rebind.
    let _ = &fixture.other_path;
    assert_eq!(
        fixture
            .app
            .effective_animation_settings()
            .unwrap()
            .ambient
            .carpet
            .mode,
        8
    );
    assert!(fixture.app.take_pending_restructure_requests().is_empty());
    assert_eq!(
        fixture.app.animation_settings.appearance,
        authored.appearance
    );
    assert!(!fixture.app.animation_settings.enabled);
}
#[test]
fn semantic_cache_revalidates_snapshots_without_rebuilding_equivalent_hosts() {
    let mut fixture = fixture();
    fixture.app.animation_settings.enabled = true;
    let probe = install_probe(&mut fixture.app);
    let first = fixture.app.effective_animation_settings().unwrap();
    let repeated = fixture.app.effective_animation_settings().unwrap();
    assert!(Rc::ptr_eq(&first, &repeated));
    compose(&mut fixture.app, 1);
    fixture.app.tree = fixture.app.tree.clone();
    fixture.app.bump_tree_version();
    fixture.app.reconcile_animation_presentation();
    let revalidated = fixture.app.effective_animation_settings().unwrap();
    assert!(!Rc::ptr_eq(&first, &revalidated));
    assert_eq!(*first, *revalidated);
    compose(&mut fixture.app, 2);
    assert_eq!(probe.builds.lock().unwrap().len(), 1);
    snapshot_recommendation(&mut fixture.app.tree, fixture.project, Some(carpet(7)), 1);
    compose(&mut fixture.app, 3);
    assert_eq!(probe.alive.load(Ordering::SeqCst), 1);
    assert_eq!(
        probe.builds.lock().unwrap().last().unwrap().1.carpet.mode,
        7
    );
}
#[test]
fn semantic_invalid_or_missing_recommendation_releases_field_host_and_timer() {
    let mut fixture = fixture();
    fixture.app.animation_settings.enabled = true;
    let probe = install_probe(&mut fixture.app);
    compose(&mut fixture.app, 1);
    assert!(fixture
        .app
        .animation_frame
        .packed_cells()
        .iter()
        .any(|cell| *cell != 0));
    let authored = fixture.app.animation_settings.clone();
    let mut invalid = carpet(1);
    let AnimationValue::Choice { label, .. } = &mut invalid.parameters[0].value else {
        panic!("choice fixture")
    };
    label.push_str(" changed");
    snapshot_recommendation(&mut fixture.app.tree, fixture.project, Some(invalid), 0);
    fixture.app.reconcile_animation_presentation();
    fixture.app.animation_frame.settle_for_test();
    assert!(fixture.app.effective_animation_settings().is_none());
    assert!(fixture.app.semantic_animation_error().is_some());
    assert_eq!(probe.alive.load(Ordering::SeqCst), 0);
    assert!(fixture
        .app
        .animation_frame
        .packed_cells()
        .iter()
        .all(|cell| *cell == 0));
    assert_eq!(fixture.app.animation_frame.cache_status().resident_bytes, 0);
    assert_eq!(
        crate::background_composition::animation_frame_delay(&fixture.app, Duration::ZERO),
        None
    );
    snapshot_recommendation(&mut fixture.app.tree, fixture.project, None, 1);
    fixture.app.reconcile_animation_presentation();
    assert!(fixture
        .app
        .semantic_animation_error()
        .unwrap()
        .contains("no project recommendation"));
    assert_eq!(fixture.app.animation_settings, authored);
}
#[test]
fn semantic_disabled_background_preview_and_concrete_cadence_are_consistent() {
    let mut fixture = fixture();
    let probe = install_probe(&mut fixture.app);
    compose(&mut fixture.app, 1);
    assert!(probe.builds.lock().unwrap().is_empty());
    assert_eq!(
        crate::background_composition::animation_frame_delay(&fixture.app, Duration::ZERO),
        None
    );
    fixture.app.mode = Mode::Settings(SettingsState {
        tab: SettingsTab::Animations,
        ..Default::default()
    });
    compose(&mut fixture.app, 2);
    assert_eq!(fixture.app.animation_frames_per_second(), 7);
    assert!(fixture.app.animation_row_context().scene_uses_cell_colors);
    assert!(!fixture.app.animation_settings.enabled);
    fixture.app.animation_settings.enabled = true;
    fixture.app.mode = Mode::Normal;
    compose(&mut fixture.app, 3);
    assert_eq!(probe.builds.lock().unwrap().len(), 1);
    fixture.app.animation_settings.fps_limit = 3;
    compose(&mut fixture.app, 4);
    assert_eq!(fixture.app.animation_frames_per_second(), 3);
    fixture.app.animation_settings.enabled = false;
    compose(&mut fixture.app, 5);
    assert_eq!(probe.alive.load(Ordering::SeqCst), 0);
}
#[test]
fn semantic_paris_attribution_and_wikipedia_policy_use_effective_kind() {
    let mut fixture = fixture();
    fixture.app.animation_settings.enabled = true;
    let probe = install_probe(&mut fixture.app);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| crate::ui::draw(frame, &mut fixture.app))
        .unwrap();
    fixture.app.animation_frame.settle_for_test();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains(crate::layout::OSM_ATTRIBUTION));
    assert!(!fixture.app.layout.osm_attribution_area.is_empty());
    let builds = probe.builds.lock().unwrap();
    let osm = &builds.last().unwrap().1.openstreetmap;
    assert_eq!((osm.source, osm.place, osm.tour), (0, 0, 0));
    drop(builds);
    snapshot_recommendation(
        &mut fixture.app.tree,
        fixture.project,
        Some(recommendation(AnimationKind::Wikipedia, vec![])),
        1,
    );
    fixture.app.reconcile_animation_presentation();
    assert_eq!(fixture.app.animation_frames_per_second(), 4);
    fixture.app.animation_frame.settle_for_test();
    assert!(fixture.app.layout.osm_attribution_area.is_empty());
    assert_eq!(probe.alive.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.app.animation_settings.kind, AnimationKind::Semantic);
}
#[test]
fn semantic_authored_video_reuse_and_missing_input_are_revalidated() {
    let mut fixture = fixture();
    fixture.app.animation_settings.enabled = true;
    fixture.app.animation_settings.ambient.video.source = "fixture-video-never-opened.mp4".into();
    let raw = ProposedRecommendation {
        kind: "video".into(),
        resources: ResourcePolicy::Authored,
        parameters: vec![],
    };
    let video =
        crate::semantic_animation::validate_recommendation(&raw, &fixture.app.animation_settings)
            .unwrap();
    snapshot_recommendation(&mut fixture.app.tree, fixture.project, Some(video), 0);
    let probe = install_probe(&mut fixture.app);
    compose(&mut fixture.app, 1);
    assert_eq!(
        probe.builds.lock().unwrap().last().unwrap().1.video.source,
        fixture.app.animation_settings.ambient.video.source
    );
    fixture.app.animation_settings.ambient.video.source.clear();
    fixture.app.reconcile_animation_presentation();
    assert!(fixture.app.effective_animation_settings().is_none());
    fixture.app.animation_frame.settle_for_test();
    assert_eq!(probe.alive.load(Ordering::SeqCst), 0);
    assert!(fixture.app.take_pending_restructure_requests().is_empty());
}
#[test]
fn semantic_undo_restores_effective_choice_and_stale_apply_preserves_it() {
    let mut fixture = fixture();
    let previous = fixture.app.tree.clone();
    let revisions = previous
        .project_activity_revisions(fixture.project)
        .unwrap();
    let recommendation = carpet(7);
    let plan = RecommendedRestructurePlan {
        structure: RestructurePlan {
            children: vec![RestructureNode::Pane {
                id: fixture.pane,
                title: "Clock work".into(),
                short_title: None,
                icon: None,
            }],
        },
        expected_animation_generation: 0,
        project: recommendation.clone(),
        entries: vec![PlanAnimationEntry {
            path: vec![0],
            recommendation,
        }],
    };
    let mut server_tree = previous.clone();
    server_tree
        .apply_recommended_project_restructure(fixture.project, plan.clone(), &revisions)
        .unwrap();
    fixture.app.tree = server_tree.clone();
    fixture.app.bump_tree_version();
    assert_eq!(
        fixture
            .app
            .effective_animation_settings()
            .unwrap()
            .ambient
            .carpet
            .mode,
        7
    );
    let accepted = server_tree.clone();
    assert!(server_tree
        .apply_recommended_project_restructure(fixture.project, plan, &revisions)
        .is_err());
    assert_eq!(server_tree, accepted);
    server_tree
        .restore_project_from(fixture.project, &previous)
        .unwrap();
    fixture.app.tree = server_tree;
    fixture.app.bump_tree_version();
    assert_eq!(
        fixture.app.effective_animation_kind(),
        Some(AnimationKind::OpenStreetMap)
    );
    assert_eq!(
        fixture
            .app
            .tree
            .project_animation_generation(fixture.project)
            .unwrap(),
        2
    );
}
#[test]
fn semantic_status_help_preserves_full_report_after_host_release() {
    let mut fixture = fixture();
    fixture.app.mode = Mode::Settings(SettingsState {
        tab: SettingsTab::Animations,
        ..Default::default()
    });
    let probe = install_probe(&mut fixture.app);
    let report = format!(
        "{}\nUNIQUE_REPORT_TAIL",
        "complete fixture status\n".repeat(80)
    );
    *probe.status.lock().unwrap() = report.clone();
    compose(&mut fixture.app, 1);
    let expected = fixture.app.animation_row_context().scene_status.unwrap();
    assert!(expected.contains(&report));
    fixture.app.push_modal(Mode::SettingsHelp(
        crate::settings_help::dialog::SettingsHelpState::new(
            "AN-37",
            1,
            crate::config::MotionLevel::Off,
        ),
    ));
    compose(&mut fixture.app, 2);
    assert_eq!(probe.alive.load(Ordering::SeqCst), 0);
    let Mode::SettingsHelp(state) = &fixture.app.mode else {
        panic!("help fixture")
    };
    assert_eq!(
        state.captured_scene_status.as_deref(),
        Some(expected.as_str())
    );
}
#[test]
fn animation_settings_are_global_and_selecting_projects_never_rebinds_them() {
    let home = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let settings = AnimationSettings {
        enabled: true,
        kind: AnimationKind::MoonlitWater,
        hue_degrees: 17,
        ..Default::default()
    };
    crate::project_config::set_animation(home.path(), settings.clone()).unwrap();
    // A project's own old animation block must be ignored entirely.
    crate::project_config::set_animation(
        second.path(),
        AnimationSettings {
            kind: AnimationKind::MoonlitWater,
            hue_degrees: 219,
            ..Default::default()
        },
    )
    .unwrap();
    let mut app = App::new("global-animation".into(), home.path().to_path_buf());
    let first_id = app.tree.add_project(first.path().to_path_buf()).unwrap();
    let second_id = app.tree.add_project(second.path().to_path_buf()).unwrap();
    app.install_animation_project_settings(
        home.path().to_path_buf(),
        Ok(crate::project_config::load(home.path()).unwrap().animation),
    );
    for project in [first_id, second_id, first_id] {
        app.select_node(project);
        app.synchronize_animation_project_settings();
        app.settle_filesystem_for_test();
        assert_eq!(app.animation_settings, settings.normalized());
        assert_eq!(app.semantic_animation_error(), None);
    }
    // An edit made while another project is selected lands in the one global
    // file and in no project.
    app.select_node(second_id);
    app.settings_select_animation_scene(AnimationKind::Semantic);
    app.settle_filesystem_for_test();
    let saved = crate::project_config::load(home.path()).unwrap().animation;
    assert_eq!(saved.kind, AnimationKind::Semantic);
    assert_eq!(saved.hue_degrees, 17);
    assert!(!first.path().join(".ilium/config.yaml").exists());
    assert_eq!(
        crate::project_config::load(second.path())
            .unwrap()
            .animation
            .hue_degrees,
        219,
        "a project's old block is neither read nor rewritten"
    );
    // A failed save keeps the previous settings and says so.
    let blocker = home.path().join("not-a-directory");
    std::fs::write(&blocker, b"fixture").unwrap();
    app.animation_home = blocker.clone();
    app.install_animation_project_settings(blocker.clone(), Ok(settings.clone()));
    let before = app.animation_settings.clone();
    app.settings_select_animation_scene(AnimationKind::Semantic);
    app.settle_filesystem_for_test();
    assert_eq!(app.animation_settings, before);
    assert!(app
        .status_message
        .as_deref()
        .is_some_and(|message| message.contains("Could not save")));
    assert_eq!(std::fs::read(blocker).unwrap(), b"fixture");
}
