//! Controlled tmux/real-wire acceptance using synthetic, deliberately wrong title proposals.
//! No provider call is made. Missing session/process observations deliberately fail closed;
//! matching-observation eligibility is separately exercised by server unit tests.
#![cfg(unix)]

use ilium_client::connection::{Connection, EventRetention, Received};
use ilium_core::animation_recommendation::{
    AnimationParameter, AnimationRecommendation, AnimationValue, PlanAnimationEntry,
    RecommendedRestructurePlan, ResourcePolicy, ANIMATION_RECOMMENDATION_VERSION,
};
use ilium_core::{AgentClass, Node, NodeId, NodeKind, RestructureNode, RestructurePlan, Tree};
use ilium_ipc::{ClientRequest, PaneTitleObservation, ServerEvent};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TIMEOUT: Duration = Duration::from_secs(30);

struct Fixture {
    root: PathBuf,
    runtime: PathBuf,
    project: PathBuf,
    binary: PathBuf,
    environment: Vec<(&'static str, PathBuf)>,
    tmux_socket: String,
    session_started: bool,
    tmux_started: bool,
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl Fixture {
    fn new() -> Self {
        let evidence = PathBuf::from(
            std::env::var_os("ILIUM_NAMING_EVIDENCE_DIR")
                .expect("set absolute ILIUM_NAMING_EVIDENCE_DIR"),
        );
        assert!(evidence.is_absolute());
        std::fs::create_dir_all(&evidence).unwrap();
        let root = tempfile::Builder::new()
            .prefix("naming-tmux-")
            .tempdir_in(&evidence)
            .unwrap()
            .keep();
        let runtime = tempfile::Builder::new()
            .prefix("nt")
            .tempdir_in(if Path::new("/ram").is_dir() {
                "/ram"
            } else {
                "/tmp"
            })
            .unwrap()
            .keep();
        let project = root.join("project");
        let config = root.join("config/ilium");
        std::fs::create_dir_all(project.join(".ilium")).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            project.join(".ilium/config.yaml"),
            "project name: NamingTitleAcceptance\nanimation:\n  enabled: false\n",
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
        settings.ui.motion_level = ilium_client::config::MotionLevel::Off;
        ilium_client::config::save_agent_setup_settings(&config, &settings.agent_setup).unwrap();
        ilium_client::config::save_onboarding_progress(&config, &settings.onboarding).unwrap();
        ilium_client::config::save_trigger_settings(&config, &settings.triggers).unwrap();
        ilium_client::config::save_ui_settings(&config, &settings.ui).unwrap();
        // Keep the task-created server away from the user's default listener.
        // A selected port is configuration evidence, not listener ownership.
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let http_port = listener.local_addr().unwrap().port();
        assert_ne!(http_port, 8872);
        drop(listener);
        let config_path = config.join("config.toml");
        let existing = std::fs::read_to_string(&config_path).unwrap();
        assert!(!existing.contains("[api]"));
        std::fs::write(
            &config_path,
            format!("{existing}\n[api]\nport = {http_port}\n"),
        )
        .unwrap();
        let port_receipt = root.join("requested-http-port.json");
        std::fs::write(
            &port_receipt,
            serde_json::to_vec_pretty(&serde_json::json!({
                "type": "artifact", "config_path": config_path,
                "table": "api", "port": http_port,
                "listener_ownership": "not established by port selection"
            }))
            .unwrap(),
        )
        .unwrap();
        artifact(&port_receipt, "task-local HTTP port configuration");
        let binary = ilium_platform::paths::canonicalize(&PathBuf::from(
            std::env::var_os("ILIUM_PTY_SMOKE_BINARY")
                .expect("set matching candidate ILIUM_PTY_SMOKE_BINARY"),
        ))
        .unwrap();
        assert!(binary.parent().unwrap().join("ilium-server").is_file());
        let environment = vec![
            ("XDG_CONFIG_HOME", root.join("config")),
            ("XDG_DATA_HOME", root.join("data")),
            ("XDG_RUNTIME_DIR", runtime.clone()),
            (
                ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                runtime.join("ilium"),
            ),
            (ilium_platform::paths::CONFIG_DIR_ENV, config),
            (
                ilium_platform::runtime_dir::DEBUG_LOG_DIR_ENV,
                root.join("logs"),
            ),
            (ilium_client::AGENT_SETUP_HOME_ENV, root.join("agent-home")),
        ];
        let tmux_socket = format!(
            "naming-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        artifact(&root, "retained task-owned state");
        artifact(
            &runtime,
            "retained task-owned runtime; enqueue after consumers released",
        );
        Self {
            root,
            runtime,
            project,
            binary,
            environment,
            tmux_socket,
            session_started: false,
            tmux_started: false,
        }
    }

    fn cli(&self, arguments: &[&str]) -> Output {
        let mut command = Command::new(&self.binary);
        command.args(arguments).current_dir(&self.project);
        for (name, value) in &self.environment {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    fn tmux(&self, arguments: &[&str]) -> Output {
        Command::new("tmux")
            .args(["-L", &self.tmux_socket])
            .args(arguments)
            .output()
            .unwrap()
    }

    fn start(&mut self) {
        let fixtures = self.root.join("fixtures");
        std::fs::create_dir_all(&fixtures).unwrap();
        let agent = ilium_test_fixtures::install(
            &fixtures,
            "codex",
            &ilium_test_fixtures::FixtureBehavior::WorkingUntilMarker {
                marker_path: self.root.join("never-submitted-task"),
            },
        );
        let output = self.cli(&["new-pane", "--", agent.path.to_str().unwrap()]);
        assert!(output.status.success(), "owned agent creation: {output:?}");
        self.session_started = true;
        let output = self.cli(&["new-pane", "--", "cat"]);
        assert!(
            output.status.success(),
            "owned sibling creation: {output:?}"
        );
        let env = self
            .environment
            .iter()
            .map(|(name, value)| shell_quote(&format!("{name}={}", value.display())))
            .collect::<Vec<_>>()
            .join(" ");
        // Keep exec so tmux's pane PID remains the client PID. Retain stderr
        // separately because tmux discards the pane when startup exits early.
        let client_stderr = self.root.join("client-stderr.txt");
        artifact(&client_stderr, "owned client startup and runtime stderr");
        let command = format!(
            "exec env {env} {} --cwd {} 2> {}",
            shell_quote(self.binary.to_str().unwrap()),
            shell_quote(self.project.to_str().unwrap()),
            shell_quote(client_stderr.to_str().unwrap())
        );
        let output = self.tmux(&[
            "new-session",
            "-d",
            "-s",
            "acceptance",
            "-x",
            "160",
            "-y",
            "44",
            &command,
        ]);
        assert!(output.status.success(), "owned tmux launch: {output:?}");
        self.tmux_started = true;
    }

    async fn control(&self) -> Connection {
        let project = ilium_platform::paths::canonicalize(&self.project).unwrap();
        let socket =
            ilium::session::socket_path_in(&self.runtime.join("ilium"), &project, "default");
        let stream = ilium_transport::SessionEndpoint::from_path(&socket)
            .connect()
            .await
            .unwrap();
        verify_loaded_process(
            stream.peer_process_id().unwrap(),
            &self.binary.parent().unwrap().join("ilium-server"),
            &self.root,
        );
        drop(stream);
        Connection::connect(&socket, "default".into())
            .await
            .unwrap()
    }

    fn terminal_text(&self) -> String {
        let output = self.tmux(&["capture-pane", "-e", "-p", "-t", "acceptance:0.0"]);
        if !output.status.success() {
            let path = self.root.join("client-stderr.txt");
            let stderr = std::fs::read_to_string(&path);
            panic!("capture owned terminal: {output:?}; retained client stderr at {path:?}: {stderr:?}");
        }
        String::from_utf8(output.stdout).unwrap()
    }

    fn capture(&self, name: &str) -> String {
        let text = self.terminal_text();
        let path = self.root.join(format!("{name}.terminal.txt"));
        std::fs::write(&path, &text).unwrap();
        artifact(&path, "actual controlled tmux terminal");
        text
    }

    fn stop(&mut self) {
        if self.session_started {
            let output = self.cli(&["kill-session", "default"]);
            assert!(output.status.success(), "owned session stop: {output:?}");
            self.session_started = false;
        }
        if self.tmux_started {
            let output = self.tmux(&["kill-server"]);
            assert!(output.status.success(), "owned tmux stop: {output:?}");
            self.tmux_started = false;
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Failure cleanup is scoped by the retained fixture environment and unique tmux socket.
        if self.session_started {
            let output = self.cli(&["kill-session", "default"]);
            println!(
                "{}",
                serde_json::json!({"type":"result","cleanup":"owned ilium session","success":output.status.success(),"state":self.root})
            );
        }
        if self.tmux_started {
            let output = self.tmux(&["kill-server"]);
            println!(
                "{}",
                serde_json::json!({"type":"result","cleanup":"owned tmux server","success":output.status.success(),"socket":self.tmux_socket})
            );
        }
    }
}

fn artifact(path: &Path, evidence: &str) {
    println!(
        "{}",
        serde_json::json!({"type":"artifact","path":path,"evidence":evidence})
    );
}

fn verify_loaded_process(process_id: u32, expected: &Path, evidence_root: &Path) {
    let loaded = ilium_platform::process_info::executable_path(process_id).unwrap();
    assert_eq!(
        loaded,
        ilium_platform::paths::canonicalize(expected).unwrap()
    );
    let bytes = std::fs::read(&loaded).unwrap();
    let receipt = serde_json::json!({"type":"artifact","process_id":process_id,"executable":loaded,"sha256":format!("{:x}",Sha256::digest(&bytes)),"bytes":bytes.len(),"evidence":"fresh isolated loaded process; candidate file hash"});
    let path = evidence_root.join(format!("loaded-process-{process_id}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    println!("{receipt}");
    artifact(&path, "loaded candidate identity receipt");
}

async fn initial_tree(connection: &mut Connection) -> ilium_client::connection::Received<Tree> {
    tokio::time::timeout(TIMEOUT, async {
        let mut tree = None;
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
            match event {
                ServerEvent::PaneStateSnapshot { tree: value, .. }
                | ServerEvent::TreeSnapshot(value) => {
                    tree = Some(ilium_client::connection::Received::with_retention(
                        value,
                        _event_retention,
                    ))
                }
                ServerEvent::InitialStateSyncComplete => return tree.unwrap(),
                _ => {}
            }
        }
        panic!("initial synchronization disconnected")
    })
    .await
    .unwrap()
}

async fn detected_agent(
    connection: &mut Connection,
    mut tree: Received<Tree>,
) -> (Received<Tree>, NodeId, HashMap<NodeId, EventRetention>) {
    tokio::time::timeout(TIMEOUT, async {
        // Status deltas move decoded allocations into the tree. Keep their
        // credit until a replacement snapshot releases those allocations.
        let mut status_retention = HashMap::new();
        loop {
            if let Some(id) = tree.pane_ids_in_tree_order().into_iter().find(|id| {
                match &tree.get(*id).unwrap().kind {
                    NodeKind::Pane { status, .. } => status
                        .known_agent_state()
                        .is_some_and(|agent| agent.class == AgentClass::Codex),
                    _ => false,
                }
            }) {
                return (tree, id, status_retention);
            }
            let received = connection
                .events
                .recv()
                .await
                .expect("detector disconnected");
            let (event, retention) = received.into_parts();
            match event {
                ServerEvent::TreeSnapshot(value)
                | ServerEvent::PaneStateSnapshot { tree: value, .. } => {
                    tree = Received::with_retention(value, retention);
                    status_retention.clear();
                }
                ServerEvent::PaneStatusChanged { pane_id, status }
                | ServerEvent::PaneDetectedStateChanged {
                    pane_id, status, ..
                } => {
                    tree = tree.map(|mut value| {
                        value.set_pane_status(pane_id, status).unwrap();
                        value
                    });
                    if let Some(retention) = retention {
                        status_retention.insert(pane_id, retention);
                    } else {
                        status_retention.remove(&pane_id);
                    }
                }
                _ => {}
            }
        }
    })
    .await
    .expect("absolute fake codex must be detected")
}

fn observations(tree: &Tree) -> Vec<PaneTitleObservation> {
    tree.pane_ids_in_tree_order()
        .iter()
        .map(|id| {
            let node = tree.get(*id).unwrap();
            let agent_class = match &node.kind {
                NodeKind::Pane { status, .. } => {
                    status.known_agent_state().map(|agent| agent.class.clone())
                }
                _ => None,
            };
            PaneTitleObservation {
                pane_id: *id,
                presentation_revision: node.presentation_revision,
                agent_class,
                session_id: None,
                process_id: None,
                title_generation: 0,
            }
        })
        .collect()
}

fn misleading_plan(tree: &Tree, group: &str) -> RestructurePlan {
    RestructurePlan {
        children: vec![RestructureNode::Group {
            title: group.into(),
            short_title: None,
            icon: None,
            children: tree
                .pane_ids_in_tree_order()
                .iter()
                .map(|id| RestructureNode::Pane {
                    id: *id,
                    title: "Sibling Authentication".into(),
                    short_title: Some("Sibling Auth".into()),
                    icon: Some("🔐".into()),
                })
                .collect(),
        }],
    }
}

fn recommendation() -> AnimationRecommendation {
    AnimationRecommendation {
        version: ANIMATION_RECOMMENDATION_VERSION,
        kind: "carpet".into(),
        resources: ResourcePolicy::Catalog,
        parameters: vec![AnimationParameter {
            id: "carpet_mode".into(),
            value: AnimationValue::Choice {
                index: 1,
                label: "Autonomous Snake".into(),
            },
        }],
    }
}

async fn apply(
    connection: &mut Connection,
    request: ClientRequest,
    group: &str,
) -> ilium_client::connection::Received<Tree> {
    connection.requests.send(request).await.unwrap();
    tokio::time::timeout(TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
            match event {
                ServerEvent::TreeSnapshot(tree)
                    if tree.all_ids().any(|id| tree.get(id).unwrap().name == group) =>
                {
                    return ilium_client::connection::Received::with_retention(
                        tree,
                        _event_retention,
                    )
                }
                ServerEvent::Error { message }
                | ServerEvent::ProjectRestructureRejected { message, .. } => {
                    panic!("restructure rejected: {message}")
                }
                _ => {}
            }
        }
        panic!("apply disconnected")
    })
    .await
    .expect("useful grouping must commit")
}

fn assert_bundle(before: &Node, after: &Node) {
    assert_eq!(after.name, before.name);
    assert_eq!(after.short_name, before.short_name);
    assert_eq!(after.inferred_icon, before.inferred_icon);
    assert_eq!(after.is_name_fixed, before.is_name_fixed);
    assert_eq!(after.presentation_revision, before.presentation_revision);
    let source = |node: &Node| match node.kind {
        NodeKind::Pane { title_source, .. } => title_source,
        _ => panic!("expected pane"),
    };
    assert_eq!(source(after), source(before));
}

fn retain_tree(fixture: &Fixture, name: &str, tree: &Tree) {
    let path = fixture.root.join(format!("{name}.tree.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(tree).unwrap()).unwrap();
    artifact(&path, "authoritative wire tree snapshot");
}

#[tokio::test]
async fn unasked_agent_keeps_title_across_all_restructure_apply_lanes() {
    let mut fixture = Fixture::new();
    fixture.start();
    let mut connection = fixture.control().await;
    let initial = initial_tree(&mut connection).await;
    let (mut tree, agent_id, _status_retention) = detected_agent(&mut connection, initial).await;
    let untouched = tree.get(agent_id).unwrap().clone();
    assert!(
        matches!(
            &untouched.kind,
            NodeKind::Pane {
                last_prompt: None,
                ..
            }
        ),
        "fixture must have no authored task"
    );
    retain_tree(&fixture, "before", &tree);
    tokio::time::timeout(TIMEOUT, async {
        loop {
            if fixture.terminal_text().contains("NamingTitleAcceptance") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    fixture.capture("before");
    let pane_pid = fixture.tmux(&[
        "display-message",
        "-p",
        "-t",
        "acceptance:0.0",
        "#{pane_pid}",
    ]);
    assert!(pane_pid.status.success());
    verify_loaded_process(
        String::from_utf8(pane_pid.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap(),
        &fixture.binary,
        &fixture.root,
    );
    let project_id = tree.project_ids()[0];
    for (lane, group) in [
        (0, "Global safe grouping"),
        (1, "Project safe grouping"),
        (2, "Recommended safe grouping"),
    ] {
        let plan = misleading_plan(&tree, group);
        let title_observations = observations(&tree);
        let request = match lane {
            0 => ClientRequest::ApplyRestructurePlan {
                plan,
                title_observations,
            },
            1 => ClientRequest::ApplyProjectRestructurePlan {
                project_id,
                plan,
                title_observations,
                inference_activity_revisions: tree.project_activity_revisions(project_id).unwrap(),
            },
            _ => ClientRequest::ApplyRecommendedProjectRestructurePlan {
                project_id,
                title_observations,
                inference_activity_revisions: tree.project_activity_revisions(project_id).unwrap(),
                plan: RecommendedRestructurePlan {
                    structure: plan,
                    expected_animation_generation: tree
                        .project_animation_generation(project_id)
                        .unwrap(),
                    project: recommendation(),
                    entries: std::iter::once(PlanAnimationEntry {
                        path: vec![0],
                        recommendation: recommendation(),
                    })
                    .chain(
                        tree.pane_ids_in_tree_order()
                            .iter()
                            .enumerate()
                            .map(|(index, _)| PlanAnimationEntry {
                                path: vec![0, u32::try_from(index).unwrap()],
                                recommendation: recommendation(),
                            }),
                    )
                    .collect(),
                },
            },
        };
        tree = apply(&mut connection, request, group).await;
        assert_bundle(&untouched, tree.get(agent_id).unwrap());
        retain_tree(&fixture, &format!("lane-{lane}"), &tree);
        tokio::time::timeout(TIMEOUT, async {
            loop {
                if fixture.terminal_text().contains(group) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("actual TUI must show committed grouping");
        fixture.capture(&format!("lane-{lane}"));
        println!(
            "{}",
            serde_json::json!({"type":"result","lane":lane,"pane_id":agent_id.0,"title_bundle":"unchanged","structure":"grouping committed","limit":"synthetic proposal, honest absent session/process observations; no model or task submission"})
        );
    }
    let old_observations = observations(&tree);
    connection
        .requests
        .send(ClientRequest::RenameNode {
            node_id: agent_id,
            title: "Manual retained title".into(),
            short_title: Some("Manual short".into()),
            inferred_icon: Some("📌".into()),
        })
        .await
        .unwrap();
    tree = tokio::time::timeout(TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
            match event {
                ServerEvent::TreeSnapshot(value)
                    if value
                        .get(agent_id)
                        .is_some_and(|node| node.name == "Manual retained title") =>
                {
                    return Received::with_retention(value, _event_retention)
                }
                ServerEvent::Error { message } => panic!("manual rename rejected: {message}"),
                _ => {}
            }
        }
        panic!("rename disconnected")
    })
    .await
    .unwrap();
    let manual = tree.get(agent_id).unwrap().clone();
    assert!(manual.is_name_fixed);
    assert_ne!(
        manual.presentation_revision,
        untouched.presentation_revision
    );
    let request = ClientRequest::ApplyProjectRestructurePlan {
        project_id,
        plan: misleading_plan(&tree, "Manual race grouping"),
        title_observations: old_observations,
        inference_activity_revisions: tree.project_activity_revisions(project_id).unwrap(),
    };
    tree = apply(&mut connection, request, "Manual race grouping").await;
    assert_bundle(&manual, tree.get(agent_id).unwrap());
    retain_tree(&fixture, "manual-race", &tree);
    tokio::time::timeout(TIMEOUT, async {
        while !fixture.terminal_text().contains("Manual race grouping") {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    fixture.capture("manual-race");
    println!(
        "{}",
        serde_json::json!({"type":"result","scenario":"manual rename vs old title observation","bundle":"retained","grouping":"committed"})
    );
    fixture.stop();
}
