//! Real candidate TUI acceptance. Synthetic descriptors and provider fixtures
//! exercise coordination only; no GPU or USB device is opened or reset.
#![cfg(target_os = "linux")]

use ilium_core::{resource_queue::*, NodeId, NodeKind, PaneStatus, SplitOrientation, Tree};
use ilium_ipc::{
    read_frame, resource::*, write_frame, ClientRequest, NewPaneKind, NewPaneWorkingDirectory,
    ServerEvent,
};
use ilium_pty::{PtyCommand, PtySession};
use ilium_transport::{SessionEndpoint, SessionStream};
use std::{path::PathBuf, time::Duration};

const TIMEOUT: Duration = Duration::from_secs(90);
static REQUEST_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn candidate_ilium_binary() -> PathBuf {
    std::env::var_os("ILIUM_RESOURCE_TEST_CLI")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_ilium")))
}

struct Sandbox {
    project: PathBuf,
    config: PathBuf,
    socket_dir: PathBuf,
    env: Vec<(&'static str, PathBuf)>,
    _root: tempfile::TempDir,
}
impl Sandbox {
    fn new() -> Self {
        // The build service executes this binary remotely. Its scratch root
        // may be overridden when the verified binary is exercised on aquarelle.
        let base = std::env::var_os("ILIUM_RESOURCE_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/data/ilium-rq"));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join(".scratch-reaper"), b"").unwrap();
        let root = tempfile::Builder::new()
            .prefix("tui")
            .tempdir_in(base)
            .unwrap();
        let project = root.path().join("project");
        let config = root.path().join("config/ilium");
        let socket_dir = root.path().join("s");
        for directory in [&project, &config, &socket_dir] {
            std::fs::create_dir_all(directory).unwrap();
        }
        std::fs::create_dir_all(project.join(".ilium")).unwrap();
        std::fs::write(
            project.join(".ilium/config.yaml"),
            "project name: QueueTui\n",
        )
        .unwrap();
        let http_port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::write(config.join("config.toml"), format!("[keyboard]\nshortcut_base = \"b\"\n[agent_setup]\nnever_ask_global = true\n[detection]\nworking_poll_seconds = 1\n[debug]\nfile_logging_enabled = true\n[api]\nport = {http_port}\n")).unwrap();
        let mut settings = ilium_client::config::load(&config).unwrap();
        settings
            .agent_setup
            .never_ask_projects
            .push(project.clone());
        settings.triggers.startup_complete.clear();
        settings.ui.left_panel_sizing.mode = ilium_client::config::LeftPanelSizingMode::Fixed;
        settings.ui.left_panel_sizing.fixed_width = 64;
        settings.ui.motion_level = ilium_client::config::MotionLevel::Off;
        ilium_client::config::save_agent_setup_settings(&config, &settings.agent_setup).unwrap();
        ilium_client::config::save_trigger_settings(&config, &settings.triggers).unwrap();
        ilium_client::config::save_ui_settings(&config, &settings.ui).unwrap();
        let env = vec![
            ("XDG_DATA_HOME", root.path().join("data")),
            ("XDG_CONFIG_HOME", root.path().join("config")),
            ("XDG_RUNTIME_DIR", socket_dir.clone()),
            (
                ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                socket_dir.clone(),
            ),
            (ilium_platform::paths::CONFIG_DIR_ENV, config.clone()),
            (
                ilium_platform::runtime_dir::DEBUG_LOG_DIR_ENV,
                root.path().join("d"),
            ),
            (
                ilium_client::AGENT_SETUP_HOME_ENV,
                root.path().join("agent-home"),
            ),
        ];
        Self {
            project: project.canonicalize().unwrap(),
            config,
            socket_dir,
            env,
            _root: root,
        }
    }
    fn command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(candidate_ilium_binary());
        command.current_dir(&self.project).env("SHELL", "/bin/sh");
        for (key, value) in &self.env {
            command.env(key, value);
        }
        for key in [
            ilium_ipc::pane_env::PANE_ID,
            ilium_ipc::pane_env::SESSION_NAME,
            ilium_ipc::pane_env::SESSION_SOCKET,
        ] {
            command.env_remove(key);
        }
        command.kill_on_drop(true);
        command
    }
    fn server_log_excerpt(&self) -> String {
        let debug_root = self._root.path().join("d");
        let mut excerpts = Vec::new();
        if let Ok(session_directories) = std::fs::read_dir(&debug_root) {
            let mut log_paths = Vec::new();
            for session_directory in session_directories.flatten() {
                let Ok(files) = std::fs::read_dir(session_directory.path()) else {
                    continue;
                };
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().is_some_and(|extension| extension == "txt") {
                        log_paths.push(path);
                    }
                }
            }
            log_paths.sort();
            excerpts.extend(log_paths.into_iter().filter_map(|path| {
                let contents = std::fs::read_to_string(&path).ok()?;
                let excerpt = contents.chars().rev().take(12000).collect::<String>();
                let excerpt = excerpt.chars().rev().collect::<String>();
                Some(format!("--- {} ---\n{excerpt}", path.display()))
            }));
        }
        if excerpts.is_empty() {
            return format!("no server logs found under {}", debug_root.display());
        }
        excerpts.join("\n")
    }
    async fn cli(&self, args: &[&str]) -> serde_json::Value {
        let output = tokio::time::timeout(TIMEOUT, self.command().args(args).output())
            .await
            .unwrap()
            .unwrap();
        if !output.status.success() {
            panic!(
                "command {args:?}: {} {}\nserver startup logs:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
                self.server_log_excerpt()
            );
        }
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null)
    }
    async fn connect(&self) -> SessionStream {
        let socket = ilium::session::socket_path_in(&self.socket_dir, &self.project, "default");
        let mut stream = SessionEndpoint::from_path(socket).connect().await.unwrap();
        write_frame(
            &mut stream,
            &ClientRequest::Attach {
                session: "default".into(),
                hello: Default::default(),
            },
        )
        .await
        .unwrap();
        stream
    }
    fn tui(&self) -> PtySession {
        let command = PtyCommand::new(
            candidate_ilium_binary().to_string_lossy().into_owned(),
            &self.project,
            50,
            180,
        )
        .arg("--cwd")
        .arg(self.project.to_string_lossy().into_owned())
        .env("SHELL", "/bin/sh");
        let command = self.env.iter().fold(command, |command, (key, value)| {
            command.env(*key, value.to_string_lossy().into_owned())
        });
        PtySession::spawn(command).unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        // Only this test's canonical project and private socket directory are
        // addressed. The user's installed binary and running server are unused.
        let mut command = std::process::Command::new(candidate_ilium_binary());
        command
            .current_dir(&self.project)
            .args(["kill-session", "default"]);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        for key in [
            ilium_ipc::pane_env::PANE_ID,
            ilium_ipc::pane_env::SESSION_NAME,
            ilium_ipc::pane_env::SESSION_SOCKET,
        ] {
            command.env_remove(key);
        }
        command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Ok(mut child) = command.spawn() {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
    }
}

async fn event(
    stream: &mut SessionStream,
    label: &str,
    predicate: impl Fn(&ServerEvent) -> bool,
) -> ServerEvent {
    let mut observed = std::collections::VecDeque::new();
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let event: ServerEvent = read_frame(stream).await.unwrap();
            if observed.len() == 8 { observed.pop_front(); }
            observed.push_back(format!("{:?}", std::mem::discriminant(&event)));
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .unwrap_or_else(|error| panic!("candidate server event {label}: {error}; recent event kinds: {observed:?}"))
}
async fn tree(stream: &mut SessionStream) -> Tree {
    match event(stream, "initial tree snapshot", |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await
    {
        ServerEvent::TreeSnapshot(tree) => tree,
        _ => unreachable!(),
    }
}
async fn screen(tui: &PtySession, text: &str) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !tui.screen_text().contains(text) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "Missing {text:?} in candidate terminal:\n{}",
            tui.screen_text()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
async fn dialog_closed(tui: &PtySession) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while tui.screen_text().contains("[Submit]") {
        assert!(
            tokio::time::Instant::now() < deadline,
            "resource dialog did not finish: {}",
            tui.screen_text()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
fn locate(tui: &PtySession, text: &str) -> (u16, u16) {
    tui.with_screen(|screen| {
        let (rows, columns) = screen.size();
        (0..rows)
            .find_map(|row| {
                let line: String = (0..columns)
                    .filter_map(|column| screen.cell(row, column))
                    .map(|cell| cell.contents())
                    .collect();
                let index = line.find(text)?;
                let column = unicode_width::UnicodeWidthStr::width(&line[..index]);
                Some((u16::try_from(column).unwrap() + 1, row + 1))
            })
            .unwrap_or_else(|| panic!("Missing clickable {text:?}: {}", tui.screen_text()))
    })
}
fn click(tui: &PtySession, text: &str, button: u8) {
    let (column, row) = locate(tui, text);
    tui.write(format!("\x1b[<{button};{column};{row}M\x1b[<{button};{column};{row}m").as_bytes())
        .unwrap();
}
async fn rpc(
    stream: &mut SessionStream,
    project: NodeId,
    pane: Option<NodeId>,
    operation: ResourceOperation,
) -> ResourceReply {
    let mut request = ResourceRequest::new(Some(project), operation);
    request.pane_id = pane;
    request.request_key = Some(format!(
        "{}:{}",
        ilium_platform::resource_fs::resource_clock()
            .unwrap()
            .unix_millis,
        std::sync::atomic::AtomicU64::fetch_add(
            &REQUEST_SEQUENCE,
            1,
            std::sync::atomic::Ordering::Relaxed
        )
    ));
    loop {
        write_frame(
            stream,
            &ClientRequest::Resource {
                request_id: 700,
                request: request.clone(),
            },
        )
        .await
        .unwrap();
        let ServerEvent::ResourceResult { result, .. } = event(stream, "resource RPC response", |event| {
            matches!(
                event,
                ServerEvent::ResourceResult {
                    request_id: 700,
                    ..
                }
            )
        })
        .await
        else {
            unreachable!()
        };
        match result {
            Err(error) if error.code == "busy" => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            result => return result.unwrap(),
        }
    }
}
async fn spawn_agent(
    stream: &mut SessionStream,
    sandbox: &Sandbox,
    project: NodeId,
    name: &str,
) -> NodeId {
    let mut inspection = sandbox.connect().await;
    let existing = tree(&mut inspection).await.pane_ids_in_tree_order();
    drop(inspection);
    let directory = sandbox.project.join(name);
    std::fs::create_dir_all(&directory).unwrap();
    let fixture = ilium_test_fixtures::install(
        &directory,
        "codex",
        &ilium_test_fixtures::FixtureBehavior::WorkingUntilMarker {
            marker_path: directory.join("finish"),
        },
    );
    write_frame(
        stream,
        &ClientRequest::NewPane {
            request_id: 701,
            parent_group: project,
            kind: NewPaneKind::Command(fixture.path.to_string_lossy().into_owned()),
            working_directory: NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let pane = event(stream, "new fake agent detection", |event| matches!(event, ServerEvent::PaneNodeChanged(node) if matches!(node.kind,NodeKind::Pane{status:PaneStatus::Agent(_),..}) && !existing.contains(&node.id) )).await;
    let ServerEvent::PaneNodeChanged(node) = pane else {
        unreachable!()
    };
    write_frame(
        stream,
        &ClientRequest::RenameNode {
            node_id: node.id,
            title: name.into(),
            short_title: None,
            inferred_icon: None,
        },
    )
    .await
    .unwrap();
    event(stream, "fake agent rename snapshot", |event| matches!(event,ServerEvent::TreeSnapshot(tree) if tree.get(node.id).is_some_and(|node|node.name==name))).await;
    node.id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_resource_dialog_queue_wait_and_display_only_grouping() {
    let sandbox = Sandbox::new();
    assert!(
        ilium_client::config::load(&sandbox.config)
            .unwrap()
            .ui
            .resource_queue_grouping,
        "resource grouping defaults on"
    );
    sandbox.cli(&["new-pane", "--", "cat"]).await;
    let mut control = sandbox.connect().await;
    let initial = tree(&mut control).await;
    let project = initial.project_ids()[0];
    let original_pane = initial.pane_ids_in_tree_order()[0];
    let tui = sandbox.tui();
    screen(&tui, "QueueTui").await;
    click(&tui, "QueueTui", 2);
    screen(&tui, "New resource").await;
    click(&tui, "New resource", 0);
    screen(&tui, "Physical identity (optional)").await;
    screen(&tui, "Tab: field").await;
    tui.write(
        b"Synthetic GPU\tgpu\tTerminal acceptance only\tExplicit release\t\ttest-device:tui\r",
    )
    .unwrap();
    dialog_closed(&tui).await;
    screen(&tui, "Synthetic GPU").await;
    // The canonical catalog, not appearance alone, proves dialog persistence.
    let catalog = rpc(&mut control, project, None, ResourceOperation::Snapshot)
        .await
        .catalog;
    let resource_id = catalog
        .links
        .iter()
        .find(|link| link.slug == "gpu")
        .unwrap()
        .resource_id
        .clone();
    assert_eq!(
        catalog.resources[&resource_id]
            .definition
            .physical_identity
            .as_deref(),
        Some("test-device:tui")
    );
    click(&tui, "Synthetic GPU", 0);
    screen(&tui, "No current owner or waiting reservations.").await;
    screen(&tui, "Enforced policy: exclusive").await;
    tui.write(b"e").unwrap();
    screen(&tui, "Local slug (create only)").await;
    tui.write(b" revised\r").unwrap();
    let edit_deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let saved = rpc(&mut control, project, None, ResourceOperation::Snapshot)
            .await
            .catalog;
        if saved.resources[&resource_id].definition.title == "Synthetic GPU revised" {
            break;
        }
        assert!(
            tokio::time::Instant::now() < edit_deadline,
            "edited metadata did not persist: {}",
            tui.screen_text()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tui.write(b"\x1b").unwrap();
    dialog_closed(&tui).await;
    screen(&tui, "Synthetic GPU revised").await;

    // A second actual project server discovers the same account-wide catalog.
    // Its private resource binding is created through the link picker.
    let mut linked = Sandbox::new();
    let shared_data = sandbox
        .env
        .iter()
        .find(|(key, _)| *key == "XDG_DATA_HOME")
        .unwrap()
        .1
        .clone();
    linked
        .env
        .iter_mut()
        .find(|(key, _)| *key == "XDG_DATA_HOME")
        .unwrap()
        .1 = shared_data;
    linked.cli(&["new-pane", "--", "cat"]).await;
    let linked_tui = linked.tui();
    screen(&linked_tui, "QueueTui").await;
    click(&linked_tui, "QueueTui", 2);
    screen(&linked_tui, "Link existing resource").await;
    click(&linked_tui, "Link existing resource", 0);
    screen(&linked_tui, "Search existing resources").await;
    screen(&linked_tui, "Tab: field").await;
    linked_tui
        .write(b"Synthetic GPU revised\tshared-gpu\r")
        .unwrap();
    dialog_closed(&linked_tui).await;
    screen(&linked_tui, "Synthetic GPU revised").await;
    let mut linked_control = linked.connect().await;
    let linked_project = tree(&mut linked_control).await.project_ids()[0];
    let shared = rpc(
        &mut linked_control,
        linked_project,
        None,
        ResourceOperation::Snapshot,
    )
    .await
    .catalog;
    assert_eq!(
        shared.resources.len(),
        1,
        "linking must not create another authority"
    );
    assert!(shared
        .links
        .iter()
        .any(|link| link.slug == "shared-gpu" && link.resource_id == resource_id));
    assert!(
        std::fs::read_to_string(linked.project.join(".ilium/config.yaml"))
            .unwrap()
            .contains("shared-gpu")
    );

    let owner = spawn_agent(&mut control, &sandbox, project, "QueueOwner").await;
    let waiter = spawn_agent(&mut control, &sandbox, project, "QueueWaiter").await;
    let later = spawn_agent(&mut control, &sandbox, project, "QueueLater").await;
    let reserve = |reservation: &str| {
        ResourceOperation::Mutate(ResourceCommand::Reserve {
            resource_id: resource_id.clone(),
            reservation_id: reservation.into(),
            estimate_seconds: 600,
        })
    };
    let owned = rpc(&mut control, project, Some(owner), reserve("tui-owner"))
        .await
        .reservation
        .unwrap();
    let queued = rpc(&mut control, project, Some(waiter), reserve("tui-waiter"))
        .await
        .reservation
        .unwrap();
    let tail = rpc(&mut control, project, Some(later), reserve("tui-later"))
        .await
        .reservation
        .unwrap();
    assert_eq!(owned.state, ReservationState::Owned);
    assert_eq!(queued.state, ReservationState::Queued);
    // Start the actual blocking CLI, which registers the native queue monitor.
    let mut waiting = sandbox
        .command()
        .args([
            "resource",
            "wait",
            "tui-waiter",
            "--agent",
            &waiter.0.to_string(),
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    screen(&tui, "Taken").await;
    click(&tui, "Synthetic GPU revised", 0);
    screen(&tui, "Queue 1").await;
    screen(&tui, "QueueWaiter").await;
    // Open the resource's children, then the waiting pane, through real clicks.
    click(&tui, "Synthetic GPU revised", 0);
    screen(&tui, "⌛ 1.").await;
    screen(&tui, "⌛ 2.").await;
    click(&tui, "QueueWaiter", 0);
    screen(&tui, "queue position 1").await;
    screen(&tui, "▸2").await;
    let (_, strip_row) = locate(&tui, "▸2");
    let colors = tui.with_screen(|screen| {
        let mut colors = Vec::new();
        for column in 0..screen.size().1 {
            let cell = screen.cell(strip_row - 1, column).unwrap();
            let background = cell.bgcolor();
            if background != vt100::Color::Default && !colors.contains(&background) {
                colors.push(background);
            }
        }
        colors
    });
    assert!(
        colors.len() >= 2,
        "native queue strip must render distinct participant colors: {colors:?}"
    );

    // Grouping is a persisted presentation setting. Toggling it must not
    // change core parents, pane identities, or the active reservations.
    click(&tui, "Synthetic GPU revised", 0);
    screen(&tui, "Enforced policy").await;
    tui.write(b"g").unwrap();
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while ilium_client::config::load(&sandbox.config)
        .unwrap()
        .ui
        .resource_queue_grouping
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "grouping toggle was not persisted"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tui.write(b"g").unwrap();
    write_frame(
        &mut control,
        &ClientRequest::RenameNode {
            node_id: original_pane,
            title: "InputWitness".into(),
            short_title: None,
            inferred_icon: None,
        },
    )
    .await
    .unwrap();
    let after = tree(&mut control).await;
    assert_eq!(
        initial.parent_of(original_pane),
        after.parent_of(original_pane)
    );
    assert_eq!(after.parent_of(owner), Some(project));
    assert_eq!(after.parent_of(waiter), Some(project));
    // A real split remains authoritative while queue grouping changes.
    write_frame(
        &mut control,
        &ClientRequest::CreateSplitView {
            parent_group: project,
            name: "QueueSplit".into(),
            orientation: SplitOrientation::Vertical,
            pane_ids: vec![original_pane, waiter],
        },
    )
    .await
    .unwrap();
    let split = tree(&mut control).await;
    assert_eq!(split.parent_of(original_pane), split.parent_of(waiter));
    assert!(split
        .get(split.parent_of(waiter).unwrap())
        .unwrap()
        .is_split_view());
    screen(&tui, "QueueSplit").await;
    click(&tui, "QueueSplit", 0);
    tui.write(b"\x02wl").unwrap();
    screen(&tui, "InputWitness").await;
    click(&tui, "InputWitness", 0);
    tui.write(b"resource-input-witness\r").unwrap();
    screen(&tui, "resource-input-witness").await;
    screen(&tui, "esc to interrupt").await;
    // Distinct input must reach the original second PTY, including while its
    // tree entry is a resource participant reference to a split child.
    click(&tui, "QueueWaiter", 0);
    tui.write(b"resource-agent-input-witness").unwrap();
    pane_output(&mut control, waiter, "resource-agent-input-witness").await;
    screen(&tui, "resource-agent-input-witness").await;
    let cat_position = locate(&tui, "resource-input-witness");
    let agent_position = locate(&tui, "resource-agent-input-witness");
    assert_ne!(cat_position, agent_position);
    let split_id = split.parent_of(waiter).unwrap();
    let before_split = serde_json::to_value(split.get(split_id).unwrap()).unwrap();
    // Toggle while the split is actually selected, then Escape must restore
    // both live children rather than opening a fabricated resource terminal.
    click(&tui, "Synthetic GPU revised", 0);
    screen(&tui, "Enforced policy").await;
    tui.write(b"g").unwrap();
    grouping_saved(&sandbox, false).await;
    tui.write(b"g").unwrap();
    grouping_saved(&sandbox, true).await;
    tui.write(b"\x1b").unwrap();
    screen(&tui, "resource-input-witness").await;
    screen(&tui, "resource-agent-input-witness").await;
    screen(&tui, "esc to interrupt").await;
    assert_eq!(locate(&tui, "resource-input-witness"), cat_position);
    assert_eq!(locate(&tui, "resource-agent-input-witness"), agent_position);
    let mut after_control = sandbox.connect().await;
    let after_grouping = tree(&mut after_control).await;
    assert_eq!(
        serde_json::to_value(after_grouping.get(split_id).unwrap()).unwrap(),
        before_split
    );
    assert_eq!(after_grouping.parent_of(waiter), Some(split_id));
    assert_eq!(after_grouping.parent_of(original_pane), Some(split_id));
    rpc(
        &mut control,
        project,
        Some(owner),
        ResourceOperation::Mutate(ResourceCommand::Release {
            reservation_id: owned.id,
            ownership_generation: owned.ownership_generation,
        }),
    )
    .await;
    let outcome = tokio::time::timeout(TIMEOUT, waiting.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(
        outcome.success(),
        "blocking candidate CLI did not finish on ownership handoff"
    );
    let acquired = rpc(
        &mut control,
        project,
        Some(waiter),
        ResourceOperation::Status {
            reservation_id: queued.id.clone(),
        },
    )
    .await
    .reservation
    .unwrap();
    assert_eq!(acquired.state, ReservationState::Owned);
    rpc(
        &mut control,
        project,
        Some(later),
        ResourceOperation::Mutate(ResourceCommand::Cancel {
            reservation_id: tail.id,
        }),
    )
    .await;
    rpc(
        &mut control,
        project,
        Some(waiter),
        ResourceOperation::Mutate(ResourceCommand::Release {
            reservation_id: queued.id,
            ownership_generation: acquired.ownership_generation,
        }),
    )
    .await;
    screen(&tui, "Free").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_usb_boards_and_interrupted_wait_keep_authority() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["new-pane", "--", "cat"]).await;
    for (name, slug, identity) in [
        ("Synthetic board alpha", "alpha", "test-usb:alpha"),
        ("Synthetic board beta", "beta", "test-usb:beta"),
    ] {
        sandbox
            .cli(&[
                "resource",
                "create",
                "--name",
                name,
                "--slug",
                slug,
                "--kind",
                "usb-board",
                "--device-identity",
                identity,
            ])
            .await;
    }
    let mut control = sandbox.connect().await;
    let project = tree(&mut control).await.project_ids()[0];
    let catalog = rpc(&mut control, project, None, ResourceOperation::Snapshot)
        .await
        .catalog;
    let id = |slug: &str| {
        catalog
            .links
            .iter()
            .find(|link| link.slug == slug)
            .unwrap()
            .resource_id
            .clone()
    };
    let alpha = id("alpha");
    let beta = id("beta");
    assert_ne!(alpha, beta);
    let first = spawn_agent(&mut control, &sandbox, project, "BoardAlphaOwner").await;
    let second = spawn_agent(&mut control, &sandbox, project, "BoardBetaOwner").await;
    let third = spawn_agent(&mut control, &sandbox, project, "BoardAlphaWaiter").await;
    let reserve = |resource_id, reservation: &str| {
        ResourceOperation::Mutate(ResourceCommand::Reserve {
            resource_id,
            reservation_id: reservation.into(),
            estimate_seconds: 600,
        })
    };
    let owned_alpha = rpc(
        &mut control,
        project,
        Some(first),
        reserve(alpha.clone(), "usb-alpha-owner"),
    )
    .await
    .reservation
    .unwrap();
    let owned_beta = rpc(
        &mut control,
        project,
        Some(second),
        reserve(beta.clone(), "usb-beta-owner"),
    )
    .await
    .reservation
    .unwrap();
    assert_eq!(owned_alpha.state, ReservationState::Owned);
    assert_eq!(owned_beta.state, ReservationState::Owned);
    let refused = sandbox
        .command()
        .args([
            "resource",
            "reserve",
            "beta",
            "1m",
            "--agent",
            &first.0.to_string(),
        ])
        .output()
        .await
        .unwrap();
    assert!(
        !refused.status.success(),
        "one invocation must not hold and wait on another resource"
    );
    let refused: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(refused["type"], "error");
    let queued = rpc(
        &mut control,
        project,
        Some(third),
        reserve(alpha.clone(), "usb-alpha-waiter"),
    )
    .await
    .reservation
    .unwrap();
    assert_eq!(queued.state, ReservationState::Queued);
    let tui = sandbox.tui();
    screen(&tui, "Synthetic board alpha").await;
    screen(&tui, "Synthetic board beta").await;
    click(&tui, "Synthetic board alpha", 0);
    screen(&tui, "Enforced policy").await;
    tui.write(b"g").unwrap();
    let grouping_deadline = tokio::time::Instant::now() + TIMEOUT;
    while ilium_client::config::load(&sandbox.config)
        .unwrap()
        .ui
        .resource_queue_grouping
    {
        assert!(
            tokio::time::Instant::now() < grouping_deadline,
            "USB grouping preference was not persisted"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Native waits have separate admission; fill every ordinary monitor slot
    // with finite, side-effect-free probe invocations before attaching one.
    let mut monitor_ids = Vec::new();
    for index in 0..8 {
        let report = serde_json::json!({"job_id":format!("synthetic-usb-capacity-{index}"),"status":"running","percent":0,"indeterminate":true,"message":"Synthetic USB acceptance capacity probe"});
        let command = format!("printf '%s\\n' '{}'", report);
        write_frame(
            &mut control,
            &ClientRequest::SetPaneProgressMonitor {
                request_id: 800 + index,
                pane_id: third,
                command,
                interval_seconds: 3600,
            },
        )
        .await
        .unwrap();
        let ServerEvent::ProgressMonitorSetCompleted { result, .. } = event(&mut control, "generic progress monitor registration", |event| matches!(event,ServerEvent::ProgressMonitorSetCompleted{request_id,..} if *request_id==800+index)).await else { unreachable!() };
        monitor_ids.push(
            result
                .expect("all eight ordinary monitor slots remain supported")
                .monitor_id,
        );
    }
    let wait = PtyCommand::new(
        candidate_ilium_binary().to_string_lossy().into_owned(),
        &sandbox.project,
        12,
        120,
    )
    .arg("resource")
    .arg("wait")
    .arg("usb-alpha-waiter")
    .arg("--agent")
    .arg(third.0.to_string())
    .arg("--project")
    .arg(sandbox.project.to_string_lossy().into_owned());
    let wait = sandbox.env.iter().fold(wait, |command, (key, value)| {
        command.env(*key, value.to_string_lossy().into_owned())
    });
    let mut wait = PtySession::spawn(wait).unwrap();
    event(&mut control, "queued wait registration", |event| matches!(event,ServerEvent::ResourceWaitChanged{pane_id,reservation_id:Some(id),..} if *pane_id==third&&*id==queued.id)).await;
    write_frame(
        &mut control,
        &ClientRequest::GetPaneProgressMonitorStatus {
            request_id: 850,
            pane_id: third,
        },
    )
    .await
    .unwrap();
    let ServerEvent::ProgressMonitorStatusReported { result, .. } = event(&mut control, "progress monitor status", |event| {
        matches!(
            event,
            ServerEvent::ProgressMonitorStatusReported {
                request_id: 850,
                ..
            }
        )
    })
    .await
    else {
        unreachable!()
    };
    let monitors = result.unwrap().progress_monitors;
    assert_eq!(
        monitors.len(),
        8,
        "resource wait must not evict an ordinary monitor"
    );
    assert!(monitors
        .iter()
        .all(|monitor| monitor_ids.contains(&monitor.monitor_id)));
    // The native strip and the saturated ordinary footer must coexist in
    // the same real terminal. Clicking native queue text opens its detail.
    screen(&tui, "BoardAlphaWaiter").await;
    click(&tui, "BoardAlphaWaiter", 0);
    screen(&tui, "queue position 1").await;
    screen(&tui, "8 tasks").await;
    click(&tui, "queue position 1", 0);
    screen(&tui, "Enforced policy").await;
    screen(&tui, "BoardAlphaWaiter").await;
    write_frame(
        &mut control,
        &ClientRequest::UpdateProgressMonitorEnabled { enabled: false },
    )
    .await
    .unwrap();
    click(&tui, "Synthetic board alpha", 0);
    screen(&tui, "Queue 1").await;
    screen(&tui, "BoardAlphaWaiter").await;
    assert!(
        !ilium_client::config::load(&sandbox.config)
            .unwrap()
            .ui
            .resource_queue_grouping
    );
    // Ctrl-C in the wait's actual controlling terminal is an observer
    // interruption, never an implicit reservation cancellation.
    wait.write(b"\x03").unwrap();
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !wait.has_exited() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "interrupted CLI wait did not exit: {}",
            wait.screen_text()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        wait.screen_text().contains("interrupted"),
        "CLI must explain retained reservation on Ctrl-C: {}",
        wait.screen_text()
    );
    assert_eq!(
        rpc(
            &mut control,
            project,
            Some(third),
            ResourceOperation::Status {
                reservation_id: queued.id.clone()
            }
        )
        .await
        .reservation
        .unwrap()
        .state,
        ReservationState::Queued
    );
    rpc(
        &mut control,
        project,
        None,
        ResourceOperation::Mutate(ResourceCommand::SetGate {
            resource_id: alpha.clone(),
            gate: ResourceGate::Unavailable {
                reason: "Synthetic board temporarily disconnected".into(),
            },
        }),
    )
    .await;
    let current = rpc(&mut control, project, None, ResourceOperation::Snapshot)
        .await
        .catalog;
    assert_eq!(
        current.resources[&alpha]
            .reservations
            .iter()
            .find(|claim| claim.id == owned_alpha.id)
            .unwrap()
            .state,
        ReservationState::Owned
    );
    assert_eq!(
        current.resources[&beta]
            .reservations
            .iter()
            .find(|claim| claim.id == owned_beta.id)
            .unwrap()
            .state,
        ReservationState::Owned
    );
    rpc(
        &mut control,
        project,
        Some(third),
        ResourceOperation::Mutate(ResourceCommand::Cancel {
            reservation_id: queued.id,
        }),
    )
    .await;
    rpc(
        &mut control,
        project,
        Some(first),
        ResourceOperation::Mutate(ResourceCommand::Release {
            reservation_id: owned_alpha.id,
            ownership_generation: owned_alpha.ownership_generation,
        }),
    )
    .await;
    rpc(
        &mut control,
        project,
        Some(second),
        ResourceOperation::Mutate(ResourceCommand::Release {
            reservation_id: owned_beta.id,
            ownership_generation: owned_beta.ownership_generation,
        }),
    )
    .await;
    screen(&tui, "Unavailable").await;
    rpc(
        &mut control,
        project,
        None,
        ResourceOperation::Mutate(ResourceCommand::SetGate {
            resource_id: alpha,
            gate: ResourceGate::Open,
        }),
    )
    .await;
    let current = rpc(&mut control, project, None, ResourceOperation::Snapshot)
        .await
        .catalog;
    assert!(current.resources.values().all(|resource| resource
        .reservations
        .iter()
        .all(|claim| !claim.state.active())));
}

/// Each CLI starts its own candidate daemon process. This test deliberately
/// uses only cat panes and synthetic descriptors; it needs no provider or device.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_candidate_server_processes_share_one_authoritative_catalog() {
    let first = Sandbox::new();
    let mut second = Sandbox::new();
    let shared_data = first
        .env
        .iter()
        .find(|(key, _)| *key == "XDG_DATA_HOME")
        .unwrap()
        .1
        .clone();
    second
        .env
        .iter_mut()
        .find(|(key, _)| *key == "XDG_DATA_HOME")
        .unwrap()
        .1 = shared_data;
    assert_ne!(first.socket_dir, second.socket_dir);
    first.cli(&["new-pane", "--", "cat"]).await;
    second.cli(&["new-pane", "--", "cat"]).await;
    first
        .cli(&[
            "resource",
            "create",
            "--name",
            "Synthetic shared board",
            "--slug",
            "board",
            "--kind",
            "usb-board",
            "--device-identity",
            "test-device:independent-processes",
        ])
        .await;
    let first_list = first.cli(&["resource", "list"]).await;
    let id = first_list["resources"][0]["definition"]["id"]
        .as_str()
        .unwrap();
    second
        .cli(&["resource", "link", id, "--slug", "shared-board"])
        .await;
    second
        .cli(&[
            "resource",
            "edit",
            "shared-board",
            "--description",
            "Authored through the second independent server",
        ])
        .await;
    // New CLI connections force authoritative readback from both daemons.
    let read_first = first.cli(&["resource", "show", "board"]).await;
    let read_second = second.cli(&["resource", "show", "shared-board"]).await;
    assert_eq!(
        read_first["resource"]["definition"],
        read_second["resource"]["definition"]
    );
    assert_eq!(
        read_first["resource"]["definition"]["description"],
        "Authored through the second independent server"
    );
    first
        .cli(&[
            "resource",
            "create",
            "--name",
            "Synthetic second board",
            "--slug",
            "board-beta",
            "--kind",
            "usb-board",
            "--device-identity",
            "test-device:independent-beta",
        ])
        .await;
    let beta = first.cli(&["resource", "show", "board-beta"]).await;
    second
        .cli(&[
            "resource",
            "link",
            beta["resource"]["definition"]["id"].as_str().unwrap(),
            "--slug",
            "shared-beta",
        ])
        .await;
    let all = first.cli(&["resource", "list", "--all"]).await;
    assert_eq!(all["resources"].as_array().unwrap().len(), 2);
    assert_eq!(all["links"].as_array().unwrap().len(), 4);
    let mut first_control = first.connect().await;
    let first_project = tree(&mut first_control).await.project_ids()[0];
    let mut second_control = second.connect().await;
    let second_project = tree(&mut second_control).await.project_ids()[0];
    let alpha_agent = spawn_agent(
        &mut first_control,
        &first,
        first_project,
        "CrossBoardAlphaOwner",
    )
    .await;
    let beta_agent = spawn_agent(
        &mut second_control,
        &second,
        second_project,
        "CrossBoardBetaOwner",
    )
    .await;
    let queued_agent = spawn_agent(
        &mut second_control,
        &second,
        second_project,
        "CrossBoardAlphaWaiter",
    )
    .await;
    let alpha_owned = first
        .cli(&[
            "resource",
            "reserve",
            "board",
            "60s",
            "--agent",
            &alpha_agent.0.to_string(),
        ])
        .await;
    let beta_owned = second
        .cli(&[
            "resource",
            "reserve",
            "shared-beta",
            "60s",
            "--agent",
            &beta_agent.0.to_string(),
        ])
        .await;
    assert_eq!(alpha_owned["state"], "owned");
    assert_eq!(beta_owned["state"], "owned");
    let refused = first
        .command()
        .args([
            "resource",
            "reserve",
            "board-beta",
            "60s",
            "--agent",
            &alpha_agent.0.to_string(),
        ])
        .output()
        .await
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    let refusal: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(refusal["type"], "error");
    let queued = second
        .cli(&[
            "resource",
            "reserve",
            "shared-board",
            "60s",
            "--agent",
            &queued_agent.0.to_string(),
        ])
        .await;
    let queued_id = queued["reservation_id"].as_str().unwrap();
    let waiting = blocking_wait(&second, queued_agent, queued_id);
    attached_wait(&mut second_control, queued_agent, queued_id).await;
    first
        .cli(&[
            "resource",
            "gate",
            "board",
            "unavailable",
            "--reason",
            "Synthetic alpha maintenance only",
        ])
        .await;
    let beta_retained = second
        .cli(&[
            "resource",
            "status",
            beta_owned["reservation_id"].as_str().unwrap(),
            "--agent",
            &beta_agent.0.to_string(),
        ])
        .await;
    assert_eq!(beta_retained["state"], "owned");
    second
        .cli(&[
            "resource",
            "cancel",
            queued_id,
            "--agent",
            &queued_agent.0.to_string(),
        ])
        .await;
    completed_wait(waiting, 3).await;
    let alpha_retained = first
        .cli(&[
            "resource",
            "status",
            alpha_owned["reservation_id"].as_str().unwrap(),
            "--agent",
            &alpha_agent.0.to_string(),
        ])
        .await;
    assert_eq!(alpha_retained["state"], "owned");
    first
        .cli(&[
            "resource",
            "release",
            alpha_owned["reservation_id"].as_str().unwrap(),
            "--agent",
            &alpha_agent.0.to_string(),
        ])
        .await;
    second
        .cli(&[
            "resource",
            "release",
            beta_owned["reservation_id"].as_str().unwrap(),
            "--agent",
            &beta_agent.0.to_string(),
        ])
        .await;
    first.cli(&["resource", "gate", "board", "open"]).await;
    assert!(
        std::fs::read_to_string(first.project.join(".ilium/config.yaml"))
            .unwrap()
            .contains("board")
    );
    assert!(
        std::fs::read_to_string(second.project.join(".ilium/config.yaml"))
            .unwrap()
            .contains("shared-board")
    );
}

fn blocking_wait(sandbox: &Sandbox, pane: NodeId, reservation: &str) -> tokio::process::Child {
    sandbox
        .command()
        .args([
            "resource",
            "wait",
            reservation,
            "--agent",
            &pane.0.to_string(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}
async fn completed_wait(child: tokio::process::Child, expected_exit: i32) -> serde_json::Value {
    let output = tokio::time::timeout(TIMEOUT, child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        records.len(),
        1,
        "one correlated terminal result is required"
    );
    records.into_iter().next().unwrap()
}
async fn attached_wait(stream: &mut SessionStream, pane: NodeId, reservation: &str) {
    event(stream, "blocking wait registration", |message| matches!(message, ServerEvent::ResourceWaitChanged { pane_id, reservation_id: Some(id), .. } if *pane_id == pane && id.0 == reservation)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_process_three_agent_fifo_handoffs_and_unavailable_cancelled_wait() {
    let first = Sandbox::new();
    let mut second = Sandbox::new();
    let shared_data = first
        .env
        .iter()
        .find(|(key, _)| *key == "XDG_DATA_HOME")
        .unwrap()
        .1
        .clone();
    second
        .env
        .iter_mut()
        .find(|(key, _)| *key == "XDG_DATA_HOME")
        .unwrap()
        .1 = shared_data;
    first.cli(&["new-pane", "--", "cat"]).await;
    second.cli(&["new-pane", "--", "cat"]).await;
    first
        .cli(&[
            "resource",
            "create",
            "--name",
            "Synthetic FIFO GPU",
            "--slug",
            "gpu",
            "--kind",
            "gpu",
        ])
        .await;
    let resource = first.cli(&["resource", "show", "gpu"]).await;
    let id = resource["resource"]["definition"]["id"].as_str().unwrap();
    second
        .cli(&["resource", "link", id, "--slug", "shared-gpu"])
        .await;
    let mut first_control = first.connect().await;
    let first_project = tree(&mut first_control).await.project_ids()[0];
    let mut second_control = second.connect().await;
    let second_project = tree(&mut second_control).await.project_ids()[0];
    let a = spawn_agent(&mut first_control, &first, first_project, "FifoOwnerA").await;
    let b = spawn_agent(&mut second_control, &second, second_project, "FifoWaiterB").await;
    let c = spawn_agent(&mut second_control, &second, second_project, "FifoWaiterC").await;
    let owner = first
        .cli(&[
            "resource",
            "reserve",
            "gpu",
            "600s",
            "--agent",
            &a.0.to_string(),
        ])
        .await;
    let queued_b = second
        .cli(&[
            "resource",
            "reserve",
            "shared-gpu",
            "300s",
            "--agent",
            &b.0.to_string(),
        ])
        .await;
    let queued_c = second
        .cli(&[
            "resource",
            "reserve",
            "shared-gpu",
            "60s",
            "--agent",
            &c.0.to_string(),
        ])
        .await;
    assert_eq!(owner["state"], "owned");
    assert_eq!(queued_b["queue_position"], 1);
    assert_eq!(queued_c["queue_position"], 2);
    let b_id = queued_b["reservation_id"].as_str().unwrap();
    let c_id = queued_c["reservation_id"].as_str().unwrap();
    let wrong_actor = completed_wait(
        blocking_wait(&second, b, owner["reservation_id"].as_str().unwrap()),
        2,
    )
    .await;
    assert!(wrong_actor["code"] == "actor" || wrong_actor["code"] == "stale_invocation");
    let waiting_b = blocking_wait(&second, b, b_id);
    attached_wait(&mut second_control, b, b_id).await;
    let waiting_c = blocking_wait(&second, c, c_id);
    attached_wait(&mut second_control, c, c_id).await;
    let tui_a = first.tui();
    let tui_b = second.tui();
    for tui in [&tui_a, &tui_b] {
        screen(tui, "Synthetic FIFO GPU").await;
        click(tui, "Synthetic FIFO GPU", 0);
        screen(tui, "FifoWaiterB").await;
        screen(tui, "FifoWaiterC").await;
    }
    first
        .cli(&[
            "resource",
            "release",
            owner["reservation_id"].as_str().unwrap(),
            "--agent",
            &a.0.to_string(),
        ])
        .await;
    assert_eq!(completed_wait(waiting_b, 0).await["reservation_id"], b_id);
    let still_queued = second
        .cli(&["resource", "status", c_id, "--agent", &c.0.to_string()])
        .await;
    assert_eq!(still_queued["state"], "queued");
    assert_eq!(still_queued["queue_position"], 1);
    second
        .cli(&["resource", "release", b_id, "--agent", &b.0.to_string()])
        .await;
    assert_eq!(completed_wait(waiting_c, 0).await["reservation_id"], c_id);
    // An already-owned wait is immediate, and still confirms the same generation.
    let immediate = completed_wait(blocking_wait(&second, c, c_id), 0).await;
    assert_eq!(immediate["state"], "owned");
    second
        .cli(&["resource", "release", c_id, "--agent", &c.0.to_string()])
        .await;
    for tui in [&tui_a, &tui_b] {
        screen(tui, "Free").await;
    }
    // A second round proves a closed gate does not finish a queued observer.
    let owner = first
        .cli(&[
            "resource",
            "reserve",
            "gpu",
            "60s",
            "--agent",
            &a.0.to_string(),
        ])
        .await;
    let queued = second
        .cli(&[
            "resource",
            "reserve",
            "shared-gpu",
            "60s",
            "--agent",
            &b.0.to_string(),
        ])
        .await;
    let reservation = queued["reservation_id"].as_str().unwrap();
    let mut waiting = blocking_wait(&second, b, reservation);
    attached_wait(&mut second_control, b, reservation).await;
    first
        .cli(&[
            "resource",
            "gate",
            "gpu",
            "unavailable",
            "--reason",
            "Synthetic fixture maintenance",
        ])
        .await;
    first
        .cli(&[
            "resource",
            "release",
            owner["reservation_id"].as_str().unwrap(),
            "--agent",
            &a.0.to_string(),
        ])
        .await;
    let retained = second
        .cli(&[
            "resource",
            "status",
            reservation,
            "--agent",
            &b.0.to_string(),
        ])
        .await;
    assert_eq!(retained["state"], "queued");
    assert!(waiting.try_wait().unwrap().is_none());
    second
        .cli(&[
            "resource",
            "cancel",
            reservation,
            "--agent",
            &b.0.to_string(),
        ])
        .await;
    let cancelled = completed_wait(waiting, 3).await;
    assert!(
        cancelled["state"] == "cancelled"
            || cancelled["code"] == "reservation_cancelled"
            || cancelled["code"] == "cancelled"
    );
    first.cli(&["resource", "gate", "gpu", "open"]).await;
    let final_snapshot = rpc(
        &mut second_control,
        second_project,
        None,
        ResourceOperation::Snapshot,
    )
    .await;
    assert!(final_snapshot.active_waits.is_empty());
    assert!(final_snapshot.catalog.resources[&ResourceId(id.into())]
        .reservations
        .iter()
        .all(|claim| !claim.state.active()));
}

fn command_terminal(sandbox: &Sandbox, arguments: &[&str]) -> PtySession {
    let mut command = PtyCommand::new(
        candidate_ilium_binary().to_string_lossy().into_owned(),
        &sandbox.project,
        50,
        180,
    )
    .arg("--cwd")
    .arg(sandbox.project.to_string_lossy().into_owned());
    for argument in arguments {
        command = command.arg(*argument);
    }
    command = command
        .arg("--project")
        .arg(sandbox.project.to_string_lossy().into_owned());
    for (key, value) in &sandbox.env {
        command = command.env(*key, value.to_string_lossy().into_owned());
    }
    PtySession::spawn(command).unwrap()
}
async fn terminal_completed(terminal: &mut PtySession) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !terminal.has_exited() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "candidate command did not exit: {}",
            terminal.screen_text()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interactive_recovery_rejects_stale_confirmation_and_preserves_other_waiters() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["new-pane", "--", "cat"]).await;
    sandbox
        .cli(&[
            "resource",
            "create",
            "--name",
            "Synthetic recovery board",
            "--slug",
            "board",
            "--kind",
            "usb-board",
        ])
        .await;
    let mut control = sandbox.connect().await;
    let project = tree(&mut control).await.project_ids()[0];
    let owner = spawn_agent(&mut control, &sandbox, project, "RecoveryOwner").await;
    let waiter = spawn_agent(&mut control, &sandbox, project, "RecoveryWaiter").await;
    let owned = sandbox
        .cli(&[
            "resource",
            "reserve",
            "board",
            "60s",
            "--agent",
            &owner.0.to_string(),
        ])
        .await;
    let queued = sandbox
        .cli(&[
            "resource",
            "reserve",
            "board",
            "60s",
            "--agent",
            &waiter.0.to_string(),
        ])
        .await;
    let owner_id = owned["reservation_id"].as_str().unwrap();
    let waiter_id = queued["reservation_id"].as_str().unwrap();
    let waiting = blocking_wait(&sandbox, waiter, waiter_id);
    attached_wait(&mut control, waiter, waiter_id).await;
    let snapshot = rpc(&mut control, project, None, ResourceOperation::Snapshot).await;
    let resource_id = snapshot.catalog.resources.keys().next().unwrap().clone();
    let revision = snapshot.catalog.revision.to_string();
    let mut stale = command_terminal(
        &sandbox,
        &[
            "resource",
            "recover",
            "board",
            "--expected-revision",
            &revision,
            "--reason",
            "Synthetic recovery acceptance",
        ],
    );
    screen(&stale, "Type REVOKE").await;
    screen(&stale, "RecoveryOwner").await;
    sandbox
        .cli(&[
            "resource",
            "estimate",
            owner_id,
            "90s",
            "--agent",
            &owner.0.to_string(),
        ])
        .await;
    stale
        .write(format!("REVOKE {}\r", resource_id.0).as_bytes())
        .unwrap();
    terminal_completed(&mut stale).await;
    assert!(
        stale.screen_text().contains("stale_confirmation")
            || stale.screen_text().contains("revision_conflict"),
        "{}",
        stale.screen_text()
    );
    let retained = sandbox
        .cli(&[
            "resource",
            "status",
            owner_id,
            "--agent",
            &owner.0.to_string(),
        ])
        .await;
    assert_eq!(retained["state"], "owned");
    // Headless recovery cannot use the same reason/revision as a bypass.
    let output = sandbox
        .command()
        .args([
            "resource",
            "recover",
            "board",
            "--expected-revision",
            &retained["revision"].as_u64().unwrap().to_string(),
            "--reason",
            "Synthetic headless attempt",
        ])
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let error: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["code"], "interactive_confirmation_required");
    let current = rpc(&mut control, project, None, ResourceOperation::Snapshot).await;
    let mut revoke = command_terminal(
        &sandbox,
        &[
            "resource",
            "recover",
            "board",
            "--expected-revision",
            &current.catalog.revision.to_string(),
            "--reason",
            "Synthetic owner revoked; no real device",
        ],
    );
    screen(&revoke, "Type REVOKE").await;
    revoke
        .write(format!("REVOKE {}\r", resource_id.0).as_bytes())
        .unwrap();
    terminal_completed(&mut revoke).await;
    let after = rpc(&mut control, project, None, ResourceOperation::Snapshot).await;
    let record = &after.catalog.resources[&resource_id];
    assert!(matches!(record.gate, ResourceGate::Paused { .. }));
    assert!(matches!(
        record
            .reservations
            .iter()
            .find(|claim| claim.id.0 == owner_id)
            .unwrap()
            .state,
        ReservationState::Revoked { .. }
    ));
    assert!(matches!(
        record
            .reservations
            .iter()
            .find(|claim| claim.id.0 == waiter_id)
            .unwrap()
            .state,
        ReservationState::Queued
    ));
    assert!(after
        .active_waits
        .iter()
        .any(|(pane, claim)| *pane == waiter && claim.0 == waiter_id));
    let revoked = completed_wait(blocking_wait(&sandbox, owner, owner_id), 3).await;
    assert!(
        revoked["state"] == "revoked"
            || revoked["code"] == "reservation_revoked"
            || revoked["code"] == "revoked"
    );
    let mut clear = command_terminal(
        &sandbox,
        &[
            "resource",
            "recover",
            "board",
            "--expected-revision",
            &after.catalog.revision.to_string(),
            "--reason",
            "Simulated cleanup verified; fixture opens no hardware",
        ],
    );
    screen(&clear, "Type CLEAR").await;
    clear
        .write(format!("CLEAR {}\r", resource_id.0).as_bytes())
        .unwrap();
    terminal_completed(&mut clear).await;
    let acquired = completed_wait(waiting, 0).await;
    assert_eq!(acquired["reservation_id"], waiter_id);
    sandbox
        .cli(&[
            "resource",
            "release",
            waiter_id,
            "--agent",
            &waiter.0.to_string(),
        ])
        .await;
}

async fn grouping_saved(sandbox: &Sandbox, expected: bool) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while ilium_client::config::load(&sandbox.config)
        .unwrap()
        .ui
        .resource_queue_grouping
        != expected
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "grouping did not persist {expected}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn candidate_daemon_crash_keeps_claim_occupied_while_synthetic_workload_survives() {
    struct Workload(std::process::Child);
    impl Drop for Workload {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let sandbox = Sandbox::new();
    sandbox.cli(&["new-pane", "--", "cat"]).await;
    sandbox
        .cli(&[
            "resource",
            "create",
            "--name",
            "Synthetic crash GPU",
            "--slug",
            "gpu",
            "--kind",
            "gpu",
        ])
        .await;
    let mut control = sandbox.connect().await;
    let project = tree(&mut control).await.project_ids()[0];
    let agent = spawn_agent(&mut control, &sandbox, project, "CrashOwner").await;
    let owner = sandbox
        .cli(&[
            "resource",
            "reserve",
            "gpu",
            "1s",
            "--agent",
            &agent.0.to_string(),
        ])
        .await;
    assert_eq!(owner["state"], "owned");
    let before = rpc(&mut control, project, None, ResourceOperation::Snapshot).await;
    let resource_id = before.catalog.resources.keys().next().unwrap().clone();
    let claim = before.catalog.resources[&resource_id].reservations[0].clone();
    let mut workload = Workload(
        std::process::Command::new("/bin/sleep")
            .arg("600")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let endpoint = SessionEndpoint::from_path(ilium::session::socket_path_in(
        &sandbox.socket_dir,
        &sandbox.project,
        "default",
    ));
    let peer = endpoint.connect().await.unwrap();
    let server_pid = peer
        .peer_process_id()
        .expect("the candidate local daemon publishes its process identity");
    assert_ne!(server_pid, std::process::id());
    assert!(workload.0.try_wait().unwrap().is_none());
    // The peer was obtained from this sandbox's private endpoint, never a
    // user's live session. This deliberately skips normal daemon cleanup.
    ilium_platform::process_control::terminate(server_pid).unwrap();
    drop(peer);
    drop(control);
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while endpoint.probe_liveness() == ilium_transport::Liveness::Live {
        assert!(
            tokio::time::Instant::now() < deadline,
            "task-owned crashed daemon listener did not stop"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(workload.0.try_wait().unwrap().is_none());
    // Normal candidate startup recovers only this stale isolated endpoint.
    sandbox.cli(&["new-pane", "--", "cat"]).await;
    let mut restarted = sandbox.connect().await;
    let restarted_project = tree(&mut restarted).await.project_ids()[0];
    assert_ne!(restarted.peer_process_id(), Some(server_pid));
    let after = rpc(
        &mut restarted,
        restarted_project,
        None,
        ResourceOperation::Snapshot,
    )
    .await;
    let retained = after.catalog.resources[&resource_id]
        .reservations
        .iter()
        .find(|reservation| reservation.id == claim.id)
        .unwrap();
    assert!(
        retained.state.occupied(),
        "server failure is not device clearance: {:?}",
        retained.state
    );
    assert_eq!(retained.agent, claim.agent);
    assert_eq!(retained.ownership_generation, claim.ownership_generation);
    assert_eq!(
        after.catalog.resources[&resource_id]
            .reservations
            .iter()
            .filter(|reservation| reservation.state.occupied())
            .count(),
        1
    );
    assert!(
        workload.0.try_wait().unwrap().is_none(),
        "synthetic workload must survive the daemon crash/recovery"
    );
}

async fn pane_output(stream: &mut SessionStream, pane: NodeId, expected: &str) {
    tokio::time::timeout(TIMEOUT, async {
        let mut observed = String::new();
        loop {
            let message: ServerEvent = read_frame(stream).await.unwrap();
            if let ServerEvent::ScreenUpdate { pane_id, bytes, .. } = message {
                if pane_id == pane {
                    observed.push_str(&String::from_utf8_lossy(&bytes));
                    if observed.contains(expected) {
                        return;
                    }
                    assert!(
                        observed.len() < 1_048_576,
                        "bounded test output did not contain input witness"
                    );
                }
            }
        }
    })
    .await
    .expect("input must reach its original split child PTY");
}
