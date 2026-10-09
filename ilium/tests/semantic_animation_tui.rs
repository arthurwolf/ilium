//! Real client/server/PTY proof with synthetic recommendations and offline scenes.
//! All configuration, sessions and agent setup targets belong to this fixture.
#![cfg(unix)]

use ilium_ambient::voxel_landscape::pack_profiles::FULL_PACKS;
use ilium_client::background_animation::{AnimationKind, AnimationPlaybackMode, AnimationSettings};
use ilium_client::connection::Connection;
use ilium_core::animation_recommendation::{
    AnimationParameter, AnimationRecommendation, AnimationValue, PlanAnimationEntry,
    RecommendedRestructurePlan, ResourcePolicy, ANIMATION_RECOMMENDATION_VERSION,
};
use ilium_core::{RestructureNode, RestructurePlan, Tree};
use ilium_ipc::{ClientRequest, ServerEvent};
use ilium_pty::{PtyCommand, PtySession};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

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
        // Keep the fixture server away from the user's shared HTTP API port.
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let http_port = listener.local_addr().unwrap().port();
        drop(listener);
        let server_config_path = config.join("config.toml");
        let mut server_config = fs::read_to_string(&server_config_path).unwrap();
        assert!(!server_config.contains("[api]"));
        server_config.push_str(&format!("\n[api]\nport = {http_port}\n"));
        fs::write(server_config_path, server_config).unwrap();
        let environment = vec![
            ("XDG_CONFIG_HOME", root.path().join("config")),
            ("XDG_DATA_HOME", root.path().join("data")),
            ("XDG_RUNTIME_DIR", runtime.path().to_path_buf()),
            (
                ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                runtime.path().join("ilium"),
            ),
            ("XDG_CACHE_HOME", root.path().join("cache")),
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

#[derive(Debug)]
struct BrailleCapture {
    file_name: String,
    sha256: String,
    dot_pixels: usize,
    color_count: usize,
    rows: u16,
    columns: u16,
}

impl BrailleCapture {
    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "file_name": self.file_name.as_str(),
            "sha256": self.sha256.as_str(),
            "dot_pixels": self.dot_pixels,
            "color_count": self.color_count,
            "rows": self.rows,
            "columns": self.columns,
        })
    }
}

fn required_native_path(name: &str) -> PathBuf {
    let value =
        std::env::var_os(name).unwrap_or_else(|| panic!("set {name} for this native proof"));
    let path = PathBuf::from(value);
    assert!(path.is_absolute(), "{name} must be absolute: {path:?}");
    path
}

fn file_sha256(path: &Path) -> String {
    let mut file = fs::File::open(path).unwrap_or_else(|error| panic!("open {path:?}: {error}"));
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    format!("{:x}", digest.finalize())
}

/// Hashes names and metadata for the save inputs the reader consumes. File bytes
/// are not copied into the evidence artifact, and the save directory stays read-only.
fn saved_input_manifest(root: &Path) -> (usize, usize, String) {
    let mut files = Vec::new();
    let mut maps = 0;
    let mut region_files = 0;
    for entry in
        fs::read_dir(root).unwrap_or_else(|error| panic!("read saves root {root:?}: {error}"))
    {
        let entry = entry.unwrap();
        let save = entry.path();
        if !entry.file_type().unwrap().is_dir() || !save.join("level.dat").is_file() {
            continue;
        }
        maps += 1;
        files.push(save.join("level.dat"));
        let region = save.join("region");
        let Ok(entries) = fs::read_dir(region) else {
            continue;
        };
        for entry in entries {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_file()
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("mca" | "mcc")
                )
            {
                region_files += 1;
                files.push(path);
            }
        }
    }
    files.sort();
    let mut digest = Sha256::new();
    for path in &files {
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(
            metadata.file_type().is_file(),
            "save input must remain a regular file: {path:?}"
        );
        let modified = metadata
            .modified()
            .unwrap_or(UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        digest.update(path.as_os_str().as_encoded_bytes());
        digest.update(metadata.len().to_le_bytes());
        digest.update(modified.to_le_bytes());
    }
    (maps, region_files, format!("{:x}", digest.finalize()))
}

fn indexed_terminal_color(index: u8) -> [u8; 3] {
    const BASIC: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 0, 0],
        [0, 205, 0],
        [205, 205, 0],
        [0, 0, 238],
        [205, 0, 205],
        [0, 205, 205],
        [229, 229, 229],
        [127, 127, 127],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [92, 92, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    match index {
        0..=15 => BASIC[usize::from(index)],
        16..=231 => {
            let value = index - 16;
            let levels = [0, 95, 135, 175, 215, 255];
            [
                levels[usize::from(value / 36)],
                levels[usize::from((value / 6) % 6)],
                levels[usize::from(value % 6)],
            ]
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            [gray, gray, gray]
        }
    }
}

fn capture_braille_ppm(tui: &PtySession, output: &Path, file_name: &str) -> BrailleCapture {
    let (rows, columns, pixels, dot_pixels, colors) = tui
        .try_with_screen(|screen| {
            let (rows, columns) = screen.size();
            let width = usize::from(columns) * 2;
            let height = usize::from(rows) * 4;
            let mut pixels = vec![0_u8; width * height * 3];
            let mut dot_pixels = 0;
            let mut colors = BTreeSet::new();
            // Each set Braille bit maps to its actual 2x4 terminal subcell.
            const DOTS: [(usize, usize, u32); 8] = [
                (0, 0, 0),
                (0, 1, 1),
                (0, 2, 2),
                (1, 0, 3),
                (1, 1, 4),
                (1, 2, 5),
                (0, 3, 6),
                (1, 3, 7),
            ];
            for row in 0..rows {
                for column in 0..columns {
                    let Some(cell) = screen.cell(row, column) else {
                        continue;
                    };
                    let Some(character) = cell.contents().chars().next() else {
                        continue;
                    };
                    let codepoint = u32::from(character);
                    if !(0x2800..=0x28ff).contains(&codepoint) {
                        continue;
                    }
                    let mask = codepoint - 0x2800;
                    let color = match cell.fgcolor() {
                        vt100::Color::Rgb(red, green, blue) => [red, green, blue],
                        vt100::Color::Idx(index) => indexed_terminal_color(index),
                        vt100::Color::Default => [255, 255, 255],
                    };
                    for (dot_x, dot_y, bit) in DOTS {
                        if mask & (1 << bit) == 0 {
                            continue;
                        }
                        let x = usize::from(column) * 2 + dot_x;
                        let y = usize::from(row) * 4 + dot_y;
                        let offset = (y * width + x) * 3;
                        pixels[offset..offset + 3].copy_from_slice(&color);
                        colors.insert(color);
                        dot_pixels += 1;
                    }
                }
            }
            (rows, columns, pixels, dot_pixels, colors.len())
        })
        .expect("the live PTY screen must be readable");
    let width = usize::from(columns) * 2;
    let height = usize::from(rows) * 4;
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    bytes.extend_from_slice(&pixels);
    let path = output.join(file_name);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("create native capture {path:?}: {error}"))
        .write_all(&bytes)
        .unwrap();
    BrailleCapture {
        file_name: file_name.to_owned(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        dot_pixels,
        color_count: colors,
        rows,
        columns,
    }
}

async fn wait_for_saved_routes(
    history_path: &Path,
    minimum_completed_runs: u64,
) -> serde_json::Value {
    tokio::time::timeout(Duration::from_secs(900), async {
        loop {
            if let Ok(bytes) = fs::read(history_path) {
                if let Ok(snapshot) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if snapshot
                        .get("history")
                        .and_then(|history| history.get("completed"))
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0)
                        >= minimum_completed_runs
                    {
                        return snapshot;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "fewer than {minimum_completed_runs} displayed saved-world routes completed; history at {history_path:?}"
        )
    })
}

fn categories_seen(snapshot: &serde_json::Value) -> BTreeSet<String> {
    snapshot
        .get("history")
        .and_then(|history| history.get("seen"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            entry
                .get("key")
                .and_then(|key| key.get("category"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

fn categories_seen_on_run(snapshot: &serde_json::Value, run: u64) -> BTreeSet<String> {
    snapshot
        .get("history")
        .and_then(|history| history.get("seen"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("run").and_then(serde_json::Value::as_u64) == Some(run))
        .filter_map(|entry| {
            entry
                .get("key")
                .and_then(|key| key.get("category"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

fn category_kind(category: &str) -> Option<&'static str> {
    match category {
        "WeatheredMasonry"
        | "OrnamentalSandstone"
        | "PrismarineMasonry"
        | "DwellingLikeConstruction" => Some("structure"),
        "OpenGrassland" | "RootedWoodland" | "SnowySurface" | "DrySandySurface"
        | "VegetatedShore" | "RockyRelief" => Some("biome"),
        _ => None,
    }
}

fn newly_seen_kind_on_run(
    previous: &serde_json::Value,
    current: &serde_json::Value,
    run: u64,
    kind: &str,
) -> bool {
    let previous = categories_seen(previous);
    categories_seen_on_run(current, run)
        .iter()
        .any(|category| category_kind(category) == Some(kind) && !previous.contains(category))
}

fn route_for_run(snapshot: &serde_json::Value, run: u64) -> Option<&serde_json::Value> {
    snapshot
        .get("history")?
        .get("routes")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("run").and_then(serde_json::Value::as_u64) == Some(run))?
        .get("route")
}

#[tokio::test]
#[ignore = "requires real local Minecraft saves and the selected GoodVibes archive"]
async fn actual_saved_world_route_emits_selected_pack_frames_and_preserves_inputs() {
    let goodvibes_profile = FULL_PACKS
        .iter()
        .position(|profile| profile.id == "goodvibes")
        .expect("GoodVibes must remain a selectable full-pack profile");
    assert_eq!(FULL_PACKS[goodvibes_profile].name, "GoodVibes / Acaitart");
    let saves_root = required_native_path("ILIUM_SAVED_MAPS_NATIVE_SAVES");
    let goodvibes_archive = required_native_path("ILIUM_SAVED_MAPS_NATIVE_GOODVIBES_ARCHIVE");
    let goodvibes_root = std::env::var("ILIUM_SAVED_MAPS_NATIVE_GOODVIBES_ROOT")
        .expect("set the GoodVibes archive member root for the native proof");
    let output = required_native_path("ILIUM_SAVED_MAPS_NATIVE_OUTPUT");
    assert!(saves_root.is_dir());
    assert!(goodvibes_archive.is_file());
    assert!(
        !output.exists(),
        "native capture output must be new: {output:?}"
    );
    fs::create_dir(&output).unwrap();
    let (map_count_before, region_count_before, world_before) = saved_input_manifest(&saves_root);
    assert!(
        map_count_before > 0,
        "the selected saves folder must contain Java worlds"
    );
    assert!(
        region_count_before > 0,
        "the selected worlds must contain saved Anvil chunks"
    );
    let pack_before = file_sha256(&goodvibes_archive);

    let mut fixture = Fixture::new("project", true);
    let config_dir = fixture
        .environment
        .iter()
        .find(|(name, _)| *name == ilium_platform::paths::CONFIG_DIR_ENV)
        .map(|(_, path)| path)
        .expect("fixture config directory");
    ilium_client::config::save_debug_settings(
        config_dir,
        &ilium_client::config::DebugSettings {
            file_logging_enabled: true,
        },
    )
    .unwrap();
    let mut settings = AnimationSettings::default();
    settings.enabled = true;
    settings.kind = AnimationKind::VoxelLandscape;
    settings.playback_mode = AnimationPlaybackMode::Live;
    settings.speed_percent = 300;
    settings.density_percent = 100;
    settings.fps_limit = 12;
    settings.ambient.voxel_landscape.saved_maps.source =
        serde_json::from_value(serde_json::json!("saved_maps")).unwrap();
    settings.ambient.voxel_landscape.saved_maps.saves_folder =
        saves_root.to_string_lossy().into_owned();
    settings.ambient.voxel_landscape.pan_speed_percent = 200;
    settings.ambient.voxel_landscape.pack_profile = goodvibes_profile;
    settings.ambient.voxel_landscape.pack_path = goodvibes_archive.to_string_lossy().into_owned();
    settings.ambient.voxel_landscape.pack_root = goodvibes_root.clone();
    ilium_client::project_config::set_animation(&fixture.project, settings.clone()).unwrap();
    let authored = ilium_client::project_config::load(&fixture.project)
        .unwrap()
        .animation;
    assert_eq!(authored.kind, AnimationKind::VoxelLandscape);
    assert_eq!(
        serde_json::to_value(authored.ambient.voxel_landscape.saved_maps.source).unwrap(),
        serde_json::json!("saved_maps")
    );
    assert_eq!(
        authored.ambient.voxel_landscape.saved_maps.saves_folder,
        saves_root.to_string_lossy()
    );
    assert_eq!(
        authored.ambient.voxel_landscape.pack_profile,
        goodvibes_profile
    );
    assert_eq!(
        FULL_PACKS[authored.ambient.voxel_landscape.pack_profile].id, "goodvibes",
        "the configured archive and selected full-pack profile must agree"
    );
    assert_eq!(
        authored.ambient.voxel_landscape.pack_path,
        goodvibes_archive.to_string_lossy()
    );
    assert_eq!(authored.ambient.voxel_landscape.pack_root, goodvibes_root);

    // Keep daemon/client diagnostics outside the fixture TempDir so a failed
    // native run remains inspectable after the fixture is dropped.
    let diagnostic_logs = output.join("client-server-logs");
    fs::create_dir_all(&diagnostic_logs).unwrap();
    for (name, path) in &mut fixture.environment {
        if *name == ilium_platform::runtime_dir::DEBUG_LOG_DIR_ENV {
            *path = diagnostic_logs.clone();
        }
    }

    let mut tui = fixture.start();
    screen_contains(&tui, "SemanticNative").await;
    verify_loaded_process(tui.process_id().unwrap(), Path::new(&fixture.binary));
    let (mut control, server_process_id) = fixture.control().await;
    let _initial_tree = initial_tree(&mut control).await;
    drop(control);
    let initial_timeout = std::env::var("ILIUM_SAVED_MAPS_NATIVE_INITIAL_FRAME_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(300));
    let started = tokio::time::timeout(initial_timeout, async {
        loop {
            if braille_ink(&tui).chars().count() >= 10 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    if started.is_err() {
        let workspace = tui.screen_text();
        fs::write(output.join("workspace-before-timeout.txt"), &workspace).unwrap();
        let settings_result =
            tokio::time::timeout(Duration::from_secs(10), open_animation_settings(&tui)).await;
        let mut settings_screen = tui.screen_text();
        let preview_observation_seconds =
            std::env::var("ILIUM_SAVED_MAPS_NATIVE_PREVIEW_OBSERVATION_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(60)
                .clamp(1, 900);
        let checkpoints = [2, 10, 30, 60, 120, 180, 300, 600, 900]
            .into_iter()
            .filter(|seconds| *seconds < preview_observation_seconds)
            .chain(std::iter::once(preview_observation_seconds))
            .collect::<Vec<_>>();
        let mut previous_seconds = 0;
        for elapsed_seconds in &checkpoints {
            tokio::time::sleep(Duration::from_secs(*elapsed_seconds - previous_seconds)).await;
            previous_seconds = *elapsed_seconds;
            settings_screen = tui.screen_text();
            fs::write(
                output.join(format!("animation-preview-after-{elapsed_seconds}s.txt")),
                &settings_screen,
            )
            .unwrap();
        }
        fs::write(
            output.join("animation-settings-diagnostic.txt"),
            &settings_screen,
        )
        .unwrap();
        fixture.kill();
        tokio::time::timeout(TIMEOUT, async {
            while !tui.has_exited()
                || ilium_platform::process_control::is_running(server_process_id)
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("owned client and server must stop after diagnostic capture");
        let inputs_after = saved_input_manifest(&saves_root);
        let pack_after = file_sha256(&goodvibes_archive);
        assert_eq!(
            (map_count_before, region_count_before, world_before.clone()),
            inputs_after,
            "saved-world input files changed during diagnostic rendering"
        );
        assert_eq!(
            pack_before, pack_after,
            "selected GoodVibes archive changed during diagnostic rendering"
        );
        let diagnostic = serde_json::json!({
            "project_animation": serde_json::to_value(authored).unwrap(),
            "initial_frame_timeout_seconds": initial_timeout.as_secs(),
            "settings_screen_opened": settings_result.is_ok(),
            "debug_file_logging_enabled_in_isolated_fixture": true,
            "settings_preview_observation_seconds": checkpoints,
            "input_save_count": map_count_before,
            "input_region_file_count": region_count_before,
            "input_metadata_snapshot_sha256": inputs_after.2,
            "inputs_unchanged": true,
            "goodvibes_archive_sha256": pack_after,
            "goodvibes_archive_unchanged": true,
            "client_server_logs": diagnostic_logs,
            "workspace_screen": "workspace-before-timeout.txt",
            "animation_settings_screen": "animation-settings-diagnostic.txt",
            "saves_root": saves_root,
            "goodvibes_archive": goodvibes_archive,
        });
        fs::write(
            output.join("diagnostic.json"),
            serde_json::to_vec_pretty(&diagnostic).unwrap(),
        )
        .unwrap();
        panic!(
            "saved scene never emitted Braille pixels; retained workspace, settings, config, and logs under {output:?}:\n{workspace}\nsettings diagnostic:\n{settings_screen}"
        );
    }
    let first = capture_braille_ppm(&tui, &output, "route-start.ppm");
    assert!(
        first.dot_pixels >= 40,
        "saved scene capture is too sparse: {first:?}"
    );
    tokio::time::timeout(Duration::from_secs(30), async {
        let initial = braille_ink(&tui);
        loop {
            if braille_ink(&tui) != initial {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("selected-pack saved-scene raster must move through its route");
    let moving = capture_braille_ppm(&tui, &output, "route-moving.ppm");
    assert_ne!(
        first.sha256, moving.sha256,
        "the emitted saved-world raster must move"
    );
    assert!(
        moving.dot_pixels >= 40 && moving.color_count >= 3,
        "selected-pack colors missing: {moving:?}"
    );

    let history_path = fixture
        ._root
        .path()
        .join("cache/ilium/ambient/minecraft-saved-history/history.json");
    let history_run_1 = wait_for_saved_routes(&history_path, 1).await;
    assert!(
        !categories_seen_on_run(&history_run_1, 1)
            .iter()
            .all(|category| category_kind(category) != Some("biome")),
        "odd run 1 must visibly show a block-derived biome category"
    );

    let history_run_2 = wait_for_saved_routes(&history_path, 2).await;
    assert_ne!(
        route_for_run(&history_run_1, 1),
        route_for_run(&history_run_2, 2),
        "the second tour must move to a different saved-world line or map"
    );
    assert!(
        newly_seen_kind_on_run(&history_run_1, &history_run_2, 2, "structure"),
        "even run 2 must visibly show a previously unseen block-derived structure category"
    );

    let history_run_3 = wait_for_saved_routes(&history_path, 3).await;
    assert_ne!(
        route_for_run(&history_run_2, 2),
        route_for_run(&history_run_3, 3),
        "the third tour must move to a different saved-world line or map"
    );
    assert!(
        newly_seen_kind_on_run(&history_run_2, &history_run_3, 3, "biome"),
        "odd run 3 must visibly show a previously unseen block-derived biome category"
    );
    let completed = history_run_3["history"]["completed"].as_u64().unwrap_or(0);
    assert!(
        completed >= 3,
        "history must credit all three presentation-confirmed routes"
    );
    let finished = capture_braille_ppm(&tui, &output, "route-complete.ppm");
    assert!(
        finished.dot_pixels >= 40,
        "completed route capture is blank: {finished:?}"
    );
    fixture.kill();
    tokio::time::timeout(TIMEOUT, async {
        while !tui.has_exited() || ilium_platform::process_control::is_running(server_process_id) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("owned client and server must stop after graceful kill-session");
    let inputs_stopped = saved_input_manifest(&saves_root);
    let pack_after = file_sha256(&goodvibes_archive);
    assert_eq!(
        (map_count_before, region_count_before, world_before),
        inputs_stopped,
        "saved-world input files changed during read-only rendering"
    );
    assert_eq!(
        pack_before, pack_after,
        "selected GoodVibes archive changed"
    );
    let manifest = serde_json::json!({
        "schema": 1,
        "client_and_server": "actual isolated Ilium binaries over PTY",
        "world_source": "SavedMaps",
        "pack_profile_id": FULL_PACKS[goodvibes_profile].id,
        "pack_profile": FULL_PACKS[goodvibes_profile].name,
        "goodvibes_archive_sha256": pack_after,
        "visible_history_completed_runs": completed,
        "odd_biome_and_even_structure_novelty_verified": true,
        "adjacent_route_transitions_verified": 2,
        "input_map_count": map_count_before,
        "input_region_files": region_count_before,
        "input_metadata_snapshot_sha256": inputs_stopped.2,
        "inputs_unchanged": true,
        "captures": [first.as_json(), moving.as_json(), finished.as_json()],
    });
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("manifest.json"))
        .unwrap()
        .write_all(&serde_json::to_vec_pretty(&manifest).unwrap())
        .unwrap();
    println!(
        "{}",
        serde_json::json!({
            "type":"artifact", "path":output, "manifest":"manifest.json",
            "route_completed":completed >= 3,
            "selected_pack_id":FULL_PACKS[goodvibes_profile].id,
            "selected_pack":FULL_PACKS[goodvibes_profile].name,
            "odd_biome_and_even_structure_novelty_verified":true,
            "inputs_unchanged":true, "captures":3
        })
    );
}
