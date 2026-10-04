//! Real client/server/PTY proof with synthetic recommendations and offline scenes.
//! All configuration, sessions and agent setup targets belong to this fixture.
#![cfg(unix)]

use ilium_client::connection::Connection;
use ilium_core::animation_recommendation::{
    AnimationParameter, AnimationRecommendation, AnimationValue, PlanAnimationEntry,
    RecommendedRestructurePlan, ResourcePolicy, ANIMATION_RECOMMENDATION_VERSION,
};
use ilium_core::{RestructureNode, RestructurePlan, Tree};
use ilium_ipc::{ClientRequest, ServerEvent};
use ilium_pty::{PtyCommand, PtySession};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

struct Fixture {
    _root: tempfile::TempDir,
    _runtime: tempfile::TempDir,
    project: PathBuf,
    binary: String,
    environment: Vec<(&'static str, PathBuf)>,
    killed: bool,
}

impl Fixture {
    fn new(scope: &str, enabled: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        // The endpoint name includes the project digest; keep its parent short.
        let short_root = if Path::new("/ram").is_dir() {
            "/ram"
        } else {
            "/tmp"
        };
        let runtime = tempfile::Builder::new()
            .prefix("sm")
            .tempdir_in(short_root)
            .unwrap();
        let project = root.path().join("project");
        let config = root.path().join("config/ilium");
        std::fs::create_dir_all(project.join(".ilium")).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            project.join(".ilium/config.yaml"),
            format!(
                "project name: SemanticNative\nanimation:\n  kind: semantic\n  enabled: {enabled}\n  semantic_scope: {scope}\n  playback_mode: live\n"
            ),
        )
        .unwrap();
        let mut settings = ilium_client::config::ClientConfig::default();
        settings.agent_setup.never_ask_global = true;
        settings
            .agent_setup
            .never_ask_projects
            .push(project.clone());
        settings.onboarding.finish();
        settings.onboarding.ai_enabled = false;
        settings.triggers.startup_complete.clear();
        settings.keyboard.shortcut_base = ilium_client::keymap::ShortcutBase::B;
        settings.ui.motion_level = ilium_client::config::MotionLevel::Full;
        ilium_client::config::save_agent_setup_settings(&config, &settings.agent_setup).unwrap();
        ilium_client::config::save_onboarding_progress(&config, &settings.onboarding).unwrap();
        ilium_client::config::save_trigger_settings(&config, &settings.triggers).unwrap();
        ilium_client::config::save_keyboard_settings(&config, &settings.keyboard).unwrap();
        ilium_client::config::save_ui_settings(&config, &settings.ui).unwrap();
        let environment = vec![
            ("XDG_CONFIG_HOME", root.path().join("config")),
            ("XDG_DATA_HOME", root.path().join("data")),
            ("XDG_RUNTIME_DIR", runtime.path().to_path_buf()),
            (
                ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                runtime.path().join("ilium"),
            ),
            (ilium_platform::paths::CONFIG_DIR_ENV, config),
            (
                ilium_platform::runtime_dir::DEBUG_LOG_DIR_ENV,
                root.path().join("logs"),
            ),
            (
                ilium_client::AGENT_SETUP_HOME_ENV,
                root.path().join("agent-home"),
            ),
        ];
        Self {
            _root: root,
            _runtime: runtime,
            project,
            binary: std::env::var("ILIUM_PTY_SMOKE_BINARY")
                .unwrap_or_else(|_| env!("CARGO_BIN_EXE_ilium").to_owned()),
            environment,
            killed: false,
        }
    }

    fn command(&self, arguments: &[&str]) -> std::process::Command {
        let mut command = std::process::Command::new(&self.binary);
        command.args(arguments).current_dir(&self.project);
        for (name, path) in &self.environment {
            command.env(name, path);
        }
        command
    }

    fn start(&self) -> PtySession {
        let output = self.command(&["new-pane", "--", "cat"]).output().unwrap();
        assert!(output.status.success(), "new-pane: {output:?}");
        let command = PtyCommand::new(&self.binary, &self.project, 44, 140)
            .arg("--cwd")
            .arg(self.project.to_string_lossy().into_owned());
        let command = self
            .environment
            .iter()
            .fold(command, |command, (name, path)| {
                command.env(*name, path.to_string_lossy().into_owned())
            });
        PtySession::spawn(command).unwrap()
    }

    async fn control(&self) -> (Connection, u32) {
        let project = ilium_platform::paths::canonicalize(&self.project).unwrap();
        let socket_dir = &self.environment[3].1;
        let socket = ilium::session::socket_path_in(socket_dir, &project, "default");
        let stream = ilium_transport::SessionEndpoint::from_path(&socket)
            .connect()
            .await
            .unwrap();
        let process_id = stream.peer_process_id().unwrap();
        verify_loaded_process(
            process_id,
            &Path::new(&self.binary)
                .parent()
                .unwrap()
                .join("ilium-server"),
        );
        drop(stream);
        (
            Connection::connect(&socket, "default".into())
                .await
                .unwrap(),
            process_id,
        )
    }

    fn kill(&mut self) {
        let output = self.command(&["kill-session", "default"]).output().unwrap();
        assert!(output.status.success(), "kill-session: {output:?}");
        self.killed = true;
    }
}

fn verify_loaded_process(process_id: u32, expected: &Path) {
    let loaded = ilium_platform::process_info::executable_path(process_id).unwrap();
    let expected = ilium_platform::paths::canonicalize(expected).unwrap();
    assert_eq!(
        loaded, expected,
        "isolated process must use the matching candidate binary"
    );
    let bytes = std::fs::read(&loaded).unwrap();
    println!(
        "{}",
        serde_json::json!({
            "type": "artifact", "process_id": process_id, "executable": loaded,
            "sha256": format!("{:x}", Sha256::digest(&bytes)), "bytes": bytes.len(),
            "evidence": "fresh owned process, resolved executable path, unchanged owned candidate file"
        })
    );
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.killed {
            let _ = self.command(&["kill-session", "default"]).output();
        }
    }
}

async fn screen_contains(tui: &PtySession, text: &str) {
    tokio::time::timeout(TIMEOUT, async {
        while !tui.screen_text().contains(text) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("missing {text:?} in real PTY:\n{}", tui.screen_text()));
}

fn braille_ink(tui: &PtySession) -> String {
    tui.screen_text()
        .chars()
        .filter(|character| ('\u{2801}'..='\u{28ff}').contains(character))
        .collect()
}

async fn verify_scene_motion(tui: &PtySession) {
    let initial_result = tokio::time::timeout(TIMEOUT, async {
        loop {
            let ink = braille_ink(tui);
            if ink.chars().count() >= 10 {
                return ink;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    let initial = match initial_result {
        Ok(ink) => ink,
        Err(_) => {
            retain_screen(tui, "failed-workspace-no-ink");
            let workspace = tui.screen_text();
            tui.resize(44, 240).unwrap();
            open_animation_settings(tui).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
            retain_screen(tui, "failed-settings-no-ink");
            panic!(
            "concrete scene must paint actual nonempty Braille cells; actual workspace:\n{workspace}\nsettings diagnostic:\n{}",
            tui.screen_text()
        );
        }
    };
    tokio::time::timeout(TIMEOUT, async {
        while braille_ink(tui) == initial {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        retain_screen(tui, "failed-workspace-no-motion");
        panic!(
            "actual scene ink must advance while the normal workspace is visible:\n{}",
            tui.screen_text()
        )
    });
}

async fn initial_tree(connection: &mut Connection) -> ilium_client::connection::Received<Tree> {
    tokio::time::timeout(TIMEOUT, async {
        let mut tree = None;
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
            match event {
                ServerEvent::PaneStateSnapshot { tree: value, .. } => {
                    tree = Some(ilium_client::connection::Received::with_retention(
                        value,
                        _event_retention,
                    ))
                }
                ServerEvent::TreeSnapshot(value) => {
                    tree = Some(ilium_client::connection::Received::with_retention(
                        value,
                        _event_retention,
                    ))
                }
                ServerEvent::InitialStateSyncComplete => return tree.unwrap(),
                _ => {}
            }
        }
        panic!("control disconnected before initial tree");
    })
    .await
    .unwrap()
}

fn recommendation(kind: &str) -> AnimationRecommendation {
    let choices: &[(&str, u32, &str)] = match kind {
        "open_street_map" => &[
            ("source", 0, "World catalogue"),
            ("place", 0, "Paris"),
            ("tour", 0, "Selected place"),
        ],
        "carpet" => &[("carpet_mode", 1, "Autonomous Snake")],
        _ => unreachable!("fixture uses only two offline scenes"),
    };
    let mut recommendation = AnimationRecommendation {
        version: ANIMATION_RECOMMENDATION_VERSION,
        kind: kind.into(),
        resources: ResourcePolicy::Catalog,
        parameters: choices
            .iter()
            .map(|(id, index, label)| AnimationParameter {
                id: (*id).into(),
                value: AnimationValue::Choice {
                    index: *index,
                    label: (*label).into(),
                },
            })
            .collect(),
    };
    // Canonical admission orders OSM fields alphabetically (no dependency axes).
    // Stored choice identities include this order as well as exact labels.
    recommendation
        .parameters
        .sort_by(|left, right| left.id.cmp(&right.id));
    recommendation.validate_shape().unwrap();
    recommendation
}

async fn apply_recommendations(
    connection: &mut Connection,
    tree: &Tree,
) -> ilium_client::connection::Received<Tree> {
    let project = tree.project_ids()[0];
    let pane = tree.pane_ids_in_tree_order()[0];
    let expected_generation = tree.project_animation_generation(project).unwrap() + 1;
    let structure = RestructurePlan {
        children: vec![RestructureNode::Pane {
            id: pane,
            title: "SemanticEntry".into(),
            short_title: None,
            icon: None,
        }],
    };
    connection
        .requests
        .send(ClientRequest::ApplyRecommendedProjectRestructurePlan {
            title_observations: Vec::new(),
            project_id: project,
            inference_activity_revisions: tree.project_activity_revisions(project).unwrap(),
            plan: RecommendedRestructurePlan {
                structure,
                expected_animation_generation: tree.project_animation_generation(project).unwrap(),
                project: recommendation("open_street_map"),
                entries: vec![PlanAnimationEntry {
                    path: vec![0],
                    recommendation: recommendation("carpet"),
                }],
            },
        })
        .await
        .unwrap();
    tokio::time::timeout(TIMEOUT, async {
        let mut updated = None;
        let mut acknowledged = false;
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
            match event {
                ServerEvent::TreeSnapshot(value)
                    if value.project_animation_generation(project).unwrap()
                        == expected_generation =>
                {
                    updated = Some(ilium_client::connection::Received::with_retention(
                        value,
                        _event_retention,
                    ));
                }
                ServerEvent::ProjectRestructureApplied { project_id, .. }
                    if project_id == project =>
                {
                    acknowledged = true;
                }
                ServerEvent::ProjectRestructureRejected { message, .. } => {
                    panic!("apply rejected: {message}")
                }
                _ => {}
            }
            // Direct replies and mutation broadcasts are separate streams;
            // both are required, but their relative arrival is not a contract.
            if acknowledged {
                if let Some(value) = updated.take() {
                    return value;
                }
            }
        }
        panic!("control disconnected before apply acknowledgement");
    })
    .await
    .unwrap()
}

async fn open_animation_settings(tui: &PtySession) {
    tui.write(b"\x02").unwrap();
    screen_contains(tui, "LEADER (press a letter").await;
    retain_screen(tui, "settings-leader");
    tui.write(b":").unwrap();
    screen_contains(tui, "⚙ Settings").await;
    let tabs = ilium_client::app::SettingsTab::ALL;
    let from = tabs
        .iter()
        .position(|tab| *tab == ilium_client::app::SettingsTab::Appearance)
        .unwrap();
    let to = tabs
        .iter()
        .position(|tab| *tab == ilium_client::app::SettingsTab::Animations)
        .unwrap();
    tui.write(&vec![b'\t'; (to + tabs.len() - from) % tabs.len()])
        .unwrap();
    screen_contains(tui, "Look and display — all animations").await;
    screen_contains(tui, "Scene settings").await;
}

fn retain_screen(tui: &PtySession, name: &str) {
    if let Some(directory) = std::env::var_os("ILIUM_SEMANTIC_SCREEN_DIR") {
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        let path = directory.join(format!("{name}.txt"));
        std::fs::write(path, tui.screen_text()).unwrap();
    }
}

#[tokio::test]
async fn project_paris_and_entry_carpet_render_through_actual_recommendation_transport() {
    for (scope, expected) in [("project", "OpenStreetMap"), ("entry", "Carpet")] {
        let mut fixture = Fixture::new(scope, true);
        let authored_before = serde_json::to_value(
            ilium_client::project_config::load(&fixture.project)
                .unwrap()
                .animation,
        )
        .unwrap();
        let mut tui = fixture.start();
        screen_contains(&tui, "SemanticNative").await;
        // fork may return before exec replaces the child's test executable.
        // Actual application output establishes the point at which identity
        // must match; retain the same strict executable-path/hash assertion.
        verify_loaded_process(tui.process_id().unwrap(), Path::new(&fixture.binary));
        let (mut control, server_process_id) = fixture.control().await;
        let before = initial_tree(&mut control).await;
        let accepted = apply_recommendations(&mut control, &before).await;
        assert_eq!(
            accepted
                .get(accepted.project_ids()[0])
                .unwrap()
                .inferred_animation
                .as_ref()
                .unwrap()
                .kind,
            "open_street_map"
        );
        assert_eq!(
            accepted
                .get(accepted.pane_ids_in_tree_order()[0])
                .unwrap()
                .inferred_animation
                .as_ref()
                .unwrap()
                .kind,
            "carpet"
        );
        // The animation-only request supplies no title grant. Select the
        // preserved authored pane label after its old group was removed.
        let preserved_title = accepted
            .get(accepted.pane_ids_in_tree_order()[0])
            .unwrap()
            .name
            .clone();
        // An unchanged pane label can still be visible in the old tree.
        // Wait for the rendered removal before deriving mouse coordinates.
        let removed_names: Vec<_> = before
            .all_ids()
            .filter(|id| accepted.get(*id).is_none())
            .map(|id| before.get(id).unwrap().name.clone())
            .collect();
        assert!(
            !removed_names.is_empty(),
            "fixture must remove its old group"
        );
        tokio::time::timeout(TIMEOUT, async {
            while removed_names.iter().any(|name| {
                tui.screen_text()
                    .lines()
                    .any(|line| line.chars().take(44).collect::<String>().contains(name))
            }) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "old groups still rendered: {removed_names:?}\n{}",
                tui.screen_text()
            )
        });
        screen_contains(&tui, &preserved_title).await;
        let row = tui
            .screen_text()
            .lines()
            .position(|line| {
                line.chars()
                    .take(30)
                    .collect::<String>()
                    .contains(&preserved_title)
            })
            .unwrap()
            + 1;
        tui.write(format!("\x1b[<0;10;{row}M\x1b[<0;10;{row}m\r").as_bytes())
            .unwrap();
        verify_scene_motion(&tui).await;
        retain_screen(&tui, &format!("{scope}-workspace"));
        // Keep the ordinary 140-column motion proof above; the wider settings
        // view must expose the complete owner-bound parameter status below.
        tui.resize(44, 240).unwrap();
        open_animation_settings(&tui).await;
        let owner = if scope == "project" {
            accepted.project_ids()[0]
        } else {
            accepted.pane_ids_in_tree_order()[0]
        };
        screen_contains(&tui, &format!("Semantic {scope} #{}: {expected}", owner.0)).await;
        // The compact status row elides long recommendations. Selecting its
        // read-only row exposes the unabridged value in the wrapped footer.
        let recommendation_row = tui
            .screen_text()
            .lines()
            .position(|line| line.contains("Recommendation ") && line.contains("Semantic "))
            .unwrap()
            + 1;
        tui.write(
            format!("\x1b[<0;100;{recommendation_row}M\x1b[<0;100;{recommendation_row}m")
                .as_bytes(),
        )
        .unwrap();
        screen_contains(
            &tui,
            if scope == "project" {
                "place=Paris"
            } else {
                "carpet_mode=Autonomous Snake"
            },
        )
        .await;
        if scope == "project" {
            screen_contains(&tui, "OpenStreetMap contributors").await;
        }
        retain_screen(&tui, scope);
        let authored = ilium_client::project_config::load(&fixture.project)
            .unwrap()
            .animation;
        assert_eq!(
            authored.kind,
            ilium_client::background_animation::AnimationKind::Semantic
        );
        assert!(authored.enabled);
        assert_eq!(serde_json::to_value(authored).unwrap(), authored_before);
        fixture.kill();
        tokio::time::timeout(TIMEOUT, async {
            while !tui.has_exited()
                || ilium_platform::process_control::is_running(server_process_id)
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("owned client and server must stop after graceful kill-session");
        println!(
            "{}",
            serde_json::json!({"type":"result", "scope":scope, "cleanup":"client and server exited", "server_process_id":server_process_id})
        );
    }
}
