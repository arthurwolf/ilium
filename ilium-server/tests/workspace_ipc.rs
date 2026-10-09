//! Real IPC preflight and rejection against an isolated temporary Git repo.

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::process::Command;
#[cfg(unix)]
use std::process::{Child, Stdio};
use std::time::Duration;

#[cfg(unix)]
use ilium_core::Tree;
use ilium_core::{BuiltinAgentProvider, ROOT_ID};
use ilium_ipc::{
    read_frame, write_frame, ClientRequest, ServerEvent, WorkspaceCreateSpec, WorkspaceCreateStage,
};
use ilium_transport::SessionStream;
#[cfg(unix)]
use ilium_transport::{Liveness, SessionEndpoint};

mod common;
#[cfg(unix)]
use common::wait_until;
use common::{expect_event, read_initial_state, TestServer};

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .expect("Git is available for this integration test");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .expect("Git is available for this integration test");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Git output is UTF-8")
        .trim()
        .to_owned()
}

async fn create_result(
    client: &mut SessionStream,
    request_id: u64,
) -> (ServerEvent, Vec<WorkspaceCreateStage>) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut stages = Vec::new();
        loop {
            let event: ServerEvent = read_frame(client).await.expect("create response frame");
            match &event {
                ServerEvent::WorkspaceCreateProgress {
                    request_id: id,
                    stage,
                } if *id == request_id => stages.push(*stage),
                ServerEvent::WorkspaceCreated { request_id: id, .. }
                | ServerEvent::WorkspaceCreateFailed { request_id: id, .. }
                    if *id == request_id =>
                {
                    return (event, stages)
                }
                _ => {}
            }
        }
    })
    .await
    .expect("create response before timeout")
}

#[cfg(unix)]
#[tokio::test]
async fn live_worktree_status_refreshes_and_replays_after_head_changes() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let linked = server.root.join("status-linked");
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 91,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::New {
                branch: "agent/status".into(),
                base_ref: "main".into(),
                path: linked.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (created, _) = create_result(&mut client, 91).await;
    let ServerEvent::WorkspaceCreated { pane_id, .. } = created else {
        panic!("worktree creation failed: {created:?}");
    };

    std::fs::write(linked.join(".gitignore"), ".local.env\nchanged\n").unwrap();
    write_frame(
        &mut client,
        &ClientRequest::RefreshPaneGitStatus { pane_id },
    )
    .await
    .unwrap();
    let full = expect_event(&mut client, Duration::from_secs(10), |event| {
        matches!(event, ServerEvent::PaneGitStatusChanged { pane_id: id, status }
            if *id == pane_id && status.modified > 0 && status.full_checked_at_unix_millis.is_some())
    }).await;
    let ServerEvent::PaneGitStatusChanged { status: full, .. } = full else {
        unreachable!()
    };
    assert_eq!(full.branch.as_deref(), Some("agent/status"));

    git(&linked, &["switch", "-q", "-c", "agent/renamed"]);
    let cheap = expect_event(&mut client, Duration::from_secs(16), |event| {
        matches!(event, ServerEvent::PaneGitStatusChanged { pane_id: id, status }
            if *id == pane_id && status.branch.as_deref() == Some("agent/renamed"))
    })
    .await;
    let ServerEvent::PaneGitStatusChanged { status: cheap, .. } = cheap else {
        unreachable!()
    };
    assert!(
        cheap.modified > 0,
        "HEAD-only probe lost the full-tier dirty count"
    );

    let mut observer = SessionEndpoint::from_path(&server.socket)
        .connect()
        .await
        .unwrap();
    write_frame(
        &mut observer,
        &ClientRequest::Attach {
            session: "workspace".into(),
        },
    )
    .await
    .unwrap();
    let replay = expect_event(&mut observer, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::PaneGitStatusChanged { pane_id: id, status }
            if *id == pane_id && status.branch.as_deref() == Some("agent/renamed"))
    })
    .await;
    assert!(matches!(replay, ServerEvent::PaneGitStatusChanged { .. }));
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_request_can_derive_a_safe_default_path_on_the_server() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 92,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::NewAtDefaultPath {
                branch: "agent/default-path".into(),
                base_ref: None,
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (created, _) = create_result(&mut client, 92).await;
    assert!(
        matches!(created, ServerEvent::WorkspaceCreated { .. }),
        "default-path creation failed: {created:?}"
    );
    let expected = server
        .project
        .parent()
        .unwrap()
        .join("project.worktrees/agent-default-path");
    assert!(
        expected.is_dir(),
        "expected worktree at {}",
        expected.display()
    );
    assert!(
        git_stdout(&server.project, &["worktree", "list", "--porcelain"])
            .contains(expected.to_str().unwrap())
    );

    // Configured setup has a Linux-only supervision contract.
    #[cfg(target_os = "linux")]
    {
        write_frame(
            &mut client,
            &ClientRequest::CreateAgentInWorkspace {
                request_id: 95,
                parent_group: ROOT_ID,
                provider: BuiltinAgentProvider::Codex,
                spec: WorkspaceCreateSpec::NewAtDefaultPathWithSetup {
                    branch: "agent/configured-path".into(),
                    base_ref: None,
                    setup_command: "printf ready > control-setup".into(),
                },
                initial_input: None,
            },
        )
        .await
        .unwrap();
        let (configured, stages) = create_result(&mut client, 95).await;
        assert!(
            matches!(configured, ServerEvent::WorkspaceCreated { .. }),
            "configured default-path creation failed: {configured:?}"
        );
        assert!(stages.contains(&WorkspaceCreateStage::RunningSetup));
        let configured_path = server
            .project
            .parent()
            .unwrap()
            .join("project.worktrees/agent-configured-path");
        assert_eq!(
            std::fs::read(configured_path.join("control-setup")).unwrap(),
            b"ready"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn existing_worktree_cannot_start_a_second_agent_pane() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let linked = server.root.join("single-agent-worktree");
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 93,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::New {
                branch: "agent/single-pane".into(),
                base_ref: "main".into(),
                path: linked.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (first, _) = create_result(&mut client, 93).await;
    assert!(matches!(first, ServerEvent::WorkspaceCreated { .. }));

    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 94,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::Existing { path: linked },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (second, _) = create_result(&mut client, 94).await;
    assert!(
        matches!(second, ServerEvent::WorkspaceCreateFailed { .. }),
        "a second pane was allowed into an occupied worktree: {second:?}"
    );
    assert_eq!(server.snapshot().await.panes().count(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_existing_worktree_requests_admit_one_agent() {
    let server = IsolatedWorkspaceServer::start().await;
    let linked = server.root.join("concurrent-existing");
    git(
        &server.project,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "agent/concurrent-existing",
            linked.to_str().unwrap(),
            "main",
        ],
    );
    let mut first = server.connect().await;
    let mut second = server.connect().await;
    let first_request = ClientRequest::CreateAgentInWorkspace {
        request_id: 95,
        parent_group: ROOT_ID,
        provider: BuiltinAgentProvider::Codex,
        spec: WorkspaceCreateSpec::Existing {
            path: linked.clone(),
        },
        initial_input: None,
    };
    let second_request = ClientRequest::CreateAgentInWorkspace {
        request_id: 96,
        parent_group: ROOT_ID,
        provider: BuiltinAgentProvider::Codex,
        spec: WorkspaceCreateSpec::Existing { path: linked },
        initial_input: None,
    };
    let (first_send, second_send) = tokio::join!(
        write_frame(&mut first, &first_request),
        write_frame(&mut second, &second_request),
    );
    first_send.unwrap();
    second_send.unwrap();
    let ((first_result, _), (second_result, _)) = tokio::join!(
        create_result(&mut first, 95),
        create_result(&mut second, 96),
    );
    let results = [first_result, second_result];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, ServerEvent::WorkspaceCreated { .. }))
            .count(),
        1,
        "concurrent existing worktree results: {results:?}"
    );
    assert_eq!(server.snapshot().await.panes().count(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn retained_owned_worktree_is_recognized_and_reused_after_pane_close() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let linked = server.root.join("retained-owned");
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 97,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::New {
                branch: "agent/retained-owned".into(),
                base_ref: "main".into(),
                path: linked.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (created, _) = create_result(&mut client, 97).await;
    let ServerEvent::WorkspaceCreated { pane_id, .. } = created else {
        panic!("new worktree failed: {created:?}")
    };
    let workspace_id = server
        .snapshot()
        .await
        .pane_workspace(pane_id)
        .unwrap()
        .workspace_id
        .clone();

    write_frame(&mut client, &ClientRequest::ClosePane { pane_id })
        .await
        .unwrap();
    expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.get(pane_id).is_none()),
    )
    .await;
    assert!(linked.is_dir(), "Keep must preserve the linked checkout");
    write_frame(
        &mut client,
        &ClientRequest::QueryRepoFacts {
            request_id: 98,
            project: ROOT_ID,
        },
    )
    .await
    .unwrap();
    let facts_event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::RepoFactsReported { request_id: 98, .. })
    })
    .await;
    let ServerEvent::RepoFactsReported {
        result: Ok(facts), ..
    } = facts_event
    else {
        panic!("repository facts unavailable: {facts_event:?}")
    };
    let retained = facts
        .worktrees
        .iter()
        .find(|worktree| worktree.path == linked)
        .expect("retained worktree listed");
    assert!(
        retained.created_by_ilium,
        "durable marker must survive close"
    );

    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 99,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::Existing { path: linked },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (reused, _) = create_result(&mut client, 99).await;
    let ServerEvent::WorkspaceCreated {
        pane_id: reused_pane_id,
        ..
    } = reused
    else {
        panic!("retained worktree could not be reused: {reused:?}")
    };
    let tree = server.snapshot().await;
    let workspace = tree.pane_workspace(reused_pane_id).unwrap();
    assert!(workspace.created_by_ilium);
    assert_eq!(workspace.workspace_id, workspace_id);
}

#[cfg(unix)]
#[tokio::test]
async fn close_offer_policy_persists_and_dirty_checkout_is_kept() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let linked = server.root.join("close-offer-owned");
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 301,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::NewWithOptions {
                branch: "agent/close-offer".into(),
                base_ref: "main".into(),
                path: linked.clone(),
                setup_command: String::new(),
                close_policy: ilium_ipc::WorkspaceClosePolicy::OfferRemovalWhenSafe,
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (created, _) = create_result(&mut client, 301).await;
    let ServerEvent::WorkspaceCreated { pane_id, .. } = created else {
        panic!("worktree creation failed: {created:?}");
    };
    let snapshot_path = server.root.join("workspace.json");
    assert!(
        wait_until(
            || std::fs::read(&snapshot_path).ok().is_some_and(|bytes| {
                let Ok(snapshot) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                    return false;
                };
                snapshot["workspace_close_preferences"]
                    .as_array()
                    .is_some_and(|preferences| {
                        preferences.iter().any(|preference| {
                            preference["pane_id"] == pane_id.0
                                && preference["policy"] == "OfferRemovalWhenSafe"
                        })
                    })
            }),
            Duration::from_secs(5),
        )
        .await,
        "close policy was not persisted with the pane"
    );

    std::fs::write(linked.join("untracked-user-file"), "keep me\n").unwrap();
    write_frame(
        &mut client,
        &ClientRequest::QueryWorkspaceCloseOffer {
            request_id: 302,
            pane_id,
        },
    )
    .await
    .unwrap();
    let offer = expect_event(&mut client, Duration::from_secs(10), |event| {
        matches!(
            event,
            ServerEvent::WorkspaceCloseOfferReported {
                request_id: 302,
                ..
            }
        )
    })
    .await;
    assert!(matches!(
        offer,
        ServerEvent::WorkspaceCloseOfferReported {
            pane_id: reported_pane,
            can_offer: false,
            ..
        } if reported_pane == pane_id
    ));

    write_frame(&mut client, &ClientRequest::ClosePane { pane_id })
        .await
        .unwrap();
    expect_event(
        &mut client,
        Duration::from_secs(10),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.get(pane_id).is_none()),
    )
    .await;
    assert_eq!(
        std::fs::read(linked.join("untracked-user-file")).unwrap(),
        b"keep me\n"
    );
}

// Setup supervision requires Linux non-reaping child/process-group observation.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn configured_setup_runs_before_agent_start_and_failure_retains_worktree() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let ready = server.root.join("setup-ready");
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 100,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::NewWithSetup {
                branch: "agent/setup-ready".into(),
                base_ref: "main".into(),
                path: ready.clone(),
                setup_command: "printf ready > setup-result".into(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (created, stages) = create_result(&mut client, 100).await;
    assert!(
        matches!(created, ServerEvent::WorkspaceCreated { .. }),
        "{created:?}"
    );
    assert_eq!(
        stages,
        [
            WorkspaceCreateStage::CreatingWorktree,
            WorkspaceCreateStage::Preparing,
            WorkspaceCreateStage::RunningSetup,
            WorkspaceCreateStage::Starting,
        ]
    );
    assert_eq!(
        std::fs::read_to_string(ready.join("setup-result")).unwrap(),
        "ready"
    );

    let failed = server.root.join("setup-failed");
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 101,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::NewWithSetup {
                branch: "agent/setup-failed".into(),
                base_ref: "main".into(),
                path: failed.clone(),
                setup_command: "printf preserved > setup-result; printf token-value >&2; exit 17"
                    .into(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (result, stages) = create_result(&mut client, 101).await;
    let ServerEvent::WorkspaceCreateFailed { error, .. } = result else {
        panic!("failing setup unexpectedly succeeded: {result:?}")
    };
    assert!(error.contains("setup failed"), "{error}");
    assert!(error.contains("worktree retained"), "{error}");
    assert!(
        !error.contains("token-value"),
        "setup output leaked into IPC"
    );
    assert!(stages.contains(&WorkspaceCreateStage::RunningSetup));
    assert_eq!(
        std::fs::read_to_string(failed.join("setup-result")).unwrap(),
        "preserved"
    );
    assert_eq!(server.snapshot().await.panes().count(), 1);

    let rejected = server.root.join("setup-rejected");
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 102,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::NewWithSetup {
                branch: "agent/setup-rejected".into(),
                base_ref: "main".into(),
                path: rejected.clone(),
                setup_command: "x".repeat(8 * 1024 + 1),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (result, _) = create_result(&mut client, 102).await;
    assert!(
        matches!(result, ServerEvent::WorkspaceCreateFailed { error, .. } if error.contains("8192")),
        "oversized setup was accepted"
    );
    assert!(!rejected.exists());
}

/// Repeatable, isolated 49-worktree load for the PERFORMANCE.md process-CPU
/// protocol. Run explicitly with `--ignored --nocapture`; the normal test
/// suite does not need to spend twenty seconds sampling process counters.
#[cfg(unix)]
#[tokio::test]
#[ignore = "manual 49-pane pidstat measurement"]
async fn measure_live_git_status_with_49_workspace_panes() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    for index in 0..49 {
        let branch = format!("agent/perf-{index:02}");
        let linked = server.root.join(format!("perf-{index:02}"));
        let request_id = 1000 + index;
        write_frame(
            &mut client,
            &ClientRequest::CreateAgentInWorkspace {
                request_id,
                parent_group: ROOT_ID,
                provider: BuiltinAgentProvider::Codex,
                spec: WorkspaceCreateSpec::New {
                    branch,
                    base_ref: "main".into(),
                    path: linked,
                },
                initial_input: None,
            },
        )
        .await
        .unwrap();
        let (created, _) = create_result(&mut client, request_id).await;
        assert!(
            matches!(created, ServerEvent::WorkspaceCreated { .. }),
            "workspace {index} failed: {created:?}"
        );
    }

    let output = tokio::task::spawn_blocking({
        let process_id = server.child.id();
        move || {
            Command::new("pidstat")
                .args(["-u", "-w", "-p", &process_id.to_string(), "1", "20"])
                .output()
        }
    })
    .await
    .unwrap()
    .expect("pidstat available for the documented performance protocol");
    assert!(output.status.success(), "pidstat failed: {output:?}");
    eprintln!(
        "isolated_server_pid={} workspace_panes=49\n{}",
        server.child.id(),
        String::from_utf8_lossy(&output.stdout)
    );
}

/// The worktree request launches a provider by its command name. Keep PATH
/// local to a child server so no concurrent test can invoke a real provider.
#[cfg(unix)]
struct IsolatedWorkspaceServer {
    child: Child,
    _directory: tempfile::TempDir,
    root: PathBuf,
    project: PathBuf,
    socket: PathBuf,
    launch_record: PathBuf,
}

#[cfg(unix)]
impl Drop for IsolatedWorkspaceServer {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(unix)]
impl IsolatedWorkspaceServer {
    async fn start() -> Self {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::Builder::new()
            .prefix("iw")
            .tempdir_in("/tmp")
            .expect("temporary project and server state");
        let root =
            ilium_platform::paths::canonicalize(directory.path()).expect("canonical fixture root");
        let project = root.join("project");
        let bin = root.join("bin");
        let config = root.join("config");
        let home = root.join("home");
        let socket = root.join("workspace.sock");
        let snapshot = root.join("workspace.json");
        let log = root.join("workspace.log");
        let launch_record = root.join("launch-record");
        for path in [&project, &bin, &config, &home] {
            std::fs::create_dir(path).expect("isolated server directory");
        }
        let port_reservation = std::net::TcpListener::bind(("127.0.0.1", 0))
            .expect("reserve an isolated HTTP API port");
        let http_api_port = port_reservation.local_addr().unwrap().port();
        drop(port_reservation);
        std::fs::write(
            config.join("config.toml"),
            format!("[notifications]\nenabled = false\n[api]\nport = {http_api_port}\n"),
        )
        .expect("isolated server config");
        let fake_codex = bin.join("codex");
        let script = format!(
            "#!/bin/sh\nprintf '%s|%s\\n' \"$PWD\" \"$ILIUM_WORKTREE\" >> '{}'\nwhile IFS= read -r line; do :; done\n",
            launch_record.display()
        );
        std::fs::write(&fake_codex, script).expect("fake codex script");
        let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&fake_codex, permissions).unwrap();

        git(&project, &["init", "-q", "-b", "main"]);
        git(&project, &["config", "user.name", "Ilium Test"]);
        git(&project, &["config", "user.email", "ilium@example.invalid"]);
        std::fs::write(project.join(".gitignore"), ".local.env\n").unwrap();
        std::fs::write(project.join(".worktreeinclude"), ".local.env\n").unwrap();
        std::fs::write(project.join(".local.env"), "isolated fixture\n").unwrap();
        git(&project, &["add", ".gitignore", ".worktreeinclude"]);
        git(&project, &["commit", "-q", "-m", "initial"]);

        let search_path =
            std::env::join_paths([bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")])
                .expect("isolated provider and system command path");
        let server_binary = std::env::var("CARGO_BIN_EXE_ilium-server")
            .expect("Cargo provides the ilium-server binary path to integration tests");
        let child = Command::new(server_binary)
            .args([
                "--session-name",
                "workspace",
                "--socket-path",
                socket.to_str().unwrap(),
                "--snapshot-path",
                snapshot.to_str().unwrap(),
                "--session-cwd",
                project.to_str().unwrap(),
                "--log-path",
                log.to_str().unwrap(),
            ])
            .env("PATH", search_path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &config)
            .env(ilium_server::paths::CONFIG_DIR_ENV, &config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("isolated ilium-server process");
        let server = Self {
            child,
            _directory: directory,
            root,
            project,
            socket,
            launch_record,
        };
        let endpoint = SessionEndpoint::from_path(&server.socket);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while endpoint.probe_liveness() != Liveness::Live {
            assert!(
                tokio::time::Instant::now() < deadline,
                "isolated server did not bind"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        server
    }

    async fn connect(&self) -> SessionStream {
        let mut stream = SessionEndpoint::from_path(&self.socket)
            .connect()
            .await
            .expect("connect to isolated server");
        write_frame(
            &mut stream,
            &ClientRequest::Attach {
                session: "workspace".into(),
            },
        )
        .await
        .expect("attach isolated server");
        read_initial_state(&mut stream, Duration::from_secs(5)).await;
        stream
    }

    async fn snapshot(&self) -> Tree {
        let mut stream = SessionEndpoint::from_path(&self.socket)
            .connect()
            .await
            .expect("connect snapshot observer");
        write_frame(
            &mut stream,
            &ClientRequest::Attach {
                session: "workspace".into(),
            },
        )
        .await
        .expect("attach snapshot observer");
        read_initial_state(&mut stream, Duration::from_secs(5))
            .await
            .0
    }
}

#[tokio::test]
async fn repo_facts_and_invalid_branch_rejection_leave_no_pane_or_worktree() {
    let server = TestServer::start("workspace-ipc").await;
    git(&server.project_cwd, &["init", "-q", "-b", "main"]);
    git(&server.project_cwd, &["config", "user.name", "Ilium Test"]);
    git(
        &server.project_cwd,
        &["config", "user.email", "ilium@example.invalid"],
    );
    git(
        &server.project_cwd,
        &["commit", "-q", "--allow-empty", "-m", "initial"],
    );
    std::fs::write(server.project_cwd.join(".gitmodules"), "").unwrap();
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "workspace-ipc".into(),
        },
    )
    .await
    .unwrap();
    read_initial_state(&mut client, Duration::from_secs(5)).await;

    write_frame(
        &mut client,
        &ClientRequest::QueryRepoFacts {
            request_id: 11,
            project: ROOT_ID,
        },
    )
    .await
    .unwrap();
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::RepoFactsReported { request_id: 11, .. })
    })
    .await;
    let ServerEvent::RepoFactsReported { result, .. } = event else {
        unreachable!()
    };
    let facts = result.expect("repository facts");
    assert_eq!(facts.checkout_root, server.project_cwd);
    assert_eq!(facts.default_base_ref, "main");
    assert!(facts.local_branches.contains(&"main".to_string()));
    assert!(facts.has_gitmodules);

    let path = server
        .project_cwd
        .parent()
        .unwrap()
        .join("invalid-branch-worktree");
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 12,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::New {
                branch: "-invalid".into(),
                base_ref: "main".into(),
                path: path.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::WorkspaceCreateFailed { request_id: 12, .. }
        )
    })
    .await;
    let ServerEvent::WorkspaceCreateFailed { error, .. } = event else {
        unreachable!()
    };
    assert!(error.contains("branch"), "{error}");
    assert!(!path.exists());
    assert_eq!(
        ilium_git::list_worktrees(&server.project_cwd)
            .await
            .unwrap()
            .len(),
        1
    );
    let mut observer = server.connect().await;
    write_frame(
        &mut observer,
        &ClientRequest::Attach {
            session: "workspace-ipc".into(),
        },
    )
    .await
    .unwrap();
    let (tree, _) = read_initial_state(&mut observer, Duration::from_secs(5)).await;
    assert_eq!(tree.panes().count(), 0);
}

#[tokio::test]
async fn invalid_include_rolls_back_new_checkout_branch_and_pane() {
    let server = TestServer::start("workspace-include-rollback").await;
    git(&server.project_cwd, &["init", "-q", "-b", "main"]);
    git(&server.project_cwd, &["config", "user.name", "Ilium Test"]);
    git(
        &server.project_cwd,
        &["config", "user.email", "ilium@example.invalid"],
    );
    git(
        &server.project_cwd,
        &["commit", "-q", "--allow-empty", "-m", "initial"],
    );
    // The copier's 64 KiB limit rejects this before it can select any file.
    std::fs::write(
        server.project_cwd.join(".worktreeinclude"),
        vec![b'a'; 65_537],
    )
    .unwrap();
    let linked_parent = tempfile::tempdir().expect("isolated linked-worktree parent");
    let linked = ilium_platform::paths::canonicalize(linked_parent.path())
        .unwrap()
        .join("include-rollback");
    assert!(
        !linked.exists(),
        "fixture checkout path unexpectedly exists"
    );
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "workspace-include-rollback".into(),
        },
    )
    .await
    .unwrap();
    read_initial_state(&mut client, Duration::from_secs(5)).await;
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 31,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::New {
                branch: "agent/include-rollback".into(),
                base_ref: "main".into(),
                path: linked.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let event = expect_event(&mut client, Duration::from_secs(10), |event| {
        matches!(
            event,
            ServerEvent::WorkspaceCreateFailed { request_id: 31, .. }
        )
    })
    .await;
    let ServerEvent::WorkspaceCreateFailed { error, .. } = event else {
        unreachable!()
    };
    assert!(error.contains("include"), "{error}");
    if error.contains("new worktree and branch rolled back") {
        assert!(
            !linked.exists(),
            "rollback reported success with a checkout"
        );
        assert_eq!(
            git_stdout(
                &server.project_cwd,
                &["branch", "--list", "agent/include-rollback"]
            ),
            ""
        );
        assert_eq!(
            ilium_git::list_worktrees(&server.project_cwd)
                .await
                .unwrap()
                .len(),
            1
        );
    } else {
        assert!(
            error.contains("process custody cannot be proved")
                || error.contains("processes still use its directory"),
            "{error}"
        );
        assert!(linked.exists(), "unsafe rollback lost the checkout");
        assert_ne!(
            git_stdout(
                &server.project_cwd,
                &["branch", "--list", "agent/include-rollback"]
            ),
            ""
        );
        assert_eq!(
            ilium_git::list_worktrees(&server.project_cwd)
                .await
                .unwrap()
                .len(),
            2
        );
    }
    let mut observer = server.connect().await;
    write_frame(
        &mut observer,
        &ClientRequest::Attach {
            session: "workspace-include-rollback".into(),
        },
    )
    .await
    .unwrap();
    let (tree, _) = read_initial_state(&mut observer, Duration::from_secs(5)).await;
    assert_eq!(tree.panes().count(), 0);
}

#[tokio::test]
async fn partial_include_copy_retains_prior_files_and_worktree() {
    let server = TestServer::start("workspace-partial-include").await;
    git(&server.project_cwd, &["init", "-q", "-b", "main"]);
    git(&server.project_cwd, &["config", "user.name", "Ilium Test"]);
    git(
        &server.project_cwd,
        &["config", "user.email", "ilium@example.invalid"],
    );
    std::fs::write(server.project_cwd.join("b"), "base b").unwrap();
    git(&server.project_cwd, &["add", "b"]);
    git(&server.project_cwd, &["commit", "-qm", "initial"]);
    std::fs::write(server.project_cwd.join("a"), "copied a").unwrap();
    std::fs::write(server.project_cwd.join(".worktreeinclude"), "a\nb\n").unwrap();
    let linked_holder = tempfile::tempdir().unwrap();
    let linked = ilium_platform::paths::canonicalize(linked_holder.path())
        .unwrap()
        .join("partial-include");
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "workspace-partial-include".into(),
        },
    )
    .await
    .unwrap();
    read_initial_state(&mut client, Duration::from_secs(5)).await;
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 103,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::New {
                branch: "agent/partial-include".into(),
                base_ref: "main".into(),
                path: linked.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (result, _) = create_result(&mut client, 103).await;
    let ServerEvent::WorkspaceCreateFailed { error, .. } = result else {
        panic!("copy collision unexpectedly succeeded: {result:?}")
    };
    assert!(error.contains("worktree retained"), "{error}");
    assert_eq!(
        std::fs::read_to_string(linked.join("a")).unwrap(),
        "copied a"
    );
    assert_eq!(std::fs::read_to_string(linked.join("b")).unwrap(), "base b");
    let mut observer = server.connect().await;
    write_frame(
        &mut observer,
        &ClientRequest::Attach {
            session: "workspace-partial-include".into(),
        },
    )
    .await
    .unwrap();
    let (tree, _) = read_initial_state(&mut observer, Duration::from_secs(5)).await;
    assert_eq!(tree.panes().count(), 0);
}

/// Git may still be running a post-checkout hook when the requester drops its
/// socket. The server owns the remaining operation and rolls back only after
/// Git returns and the checkout proves pristine, including ignored paths.
#[cfg(unix)]
#[tokio::test]
async fn disconnect_during_worktree_add_finishes_git_then_rolls_back() {
    use std::os::unix::fs::PermissionsExt;

    let server = IsolatedWorkspaceServer::start().await;
    let linked = server.root.join("disconnected-linked");
    let hook_ready = server.root.join("hook-ready");
    let hook_release = server.root.join("hook-release");
    let hook = server.project.join(".git/hooks/post-checkout");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nprintf ready > '{}'\nwhile [ ! -e '{}' ]; do sleep 0.05; done\n",
            hook_ready.display(),
            hook_release.display()
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&hook, permissions).unwrap();

    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 41,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::New {
                branch: "agent/disconnected".into(),
                base_ref: "main".into(),
                path: linked.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    assert!(
        wait_until(|| hook_ready.exists(), Duration::from_secs(10)).await,
        "Git did not reach the post-checkout hook"
    );
    drop(client);
    tokio::time::sleep(Duration::from_millis(100)).await;
    std::fs::write(&hook_release, b"release").unwrap();

    let common_dir = ilium_git::discover(&server.project)
        .await
        .unwrap()
        .common_dir;
    assert!(
        wait_until(
            || matches!(
                ilium_platform::process_control::try_workspace_repository_lease(&common_dir),
                Ok(Some(_))
            ),
            Duration::from_secs(15)
        )
        .await,
        "disconnected creation did not finish its repository transaction"
    );
    let branch = git_stdout(&server.project, &["branch", "--list", "agent/disconnected"]);
    if linked.exists() {
        assert!(!branch.is_empty(), "retained checkout lost its branch");
        let head = ilium_git::head_paths(&linked).await.unwrap().head;
        assert!(
            head.parent()
                .unwrap()
                .join("ilium-workspace-owner.json")
                .is_file(),
            "retained checkout lost ownership"
        );
    } else {
        assert!(
            branch.is_empty(),
            "rolled-back checkout retained its branch"
        );
    }
    assert_eq!(server.snapshot().await.panes().count(), 0);
    assert!(
        !server.launch_record.exists(),
        "provider started after disconnect"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn isolated_server_creates_attaches_and_removes_only_owned_worktree() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let linked = server.root.join("linked");
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 21,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::New {
                branch: "agent/isolated".into(),
                base_ref: "main".into(),
                path: linked.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (created, stages) = create_result(&mut client, 21).await;
    assert_eq!(
        stages,
        [
            WorkspaceCreateStage::CreatingWorktree,
            WorkspaceCreateStage::Preparing,
            WorkspaceCreateStage::Starting,
        ]
    );
    let ServerEvent::WorkspaceCreated { pane_id, .. } = created else {
        panic!("new worktree failed: {created:?}")
    };
    assert_eq!(
        git_stdout(&linked, &["branch", "--show-current"]),
        "agent/isolated"
    );
    assert_eq!(
        std::fs::read(linked.join(".local.env")).unwrap(),
        b"isolated fixture\n"
    );
    let marker_parent = PathBuf::from(git_stdout(&linked, &["rev-parse", "--git-path", "HEAD"]));
    let marker_path = marker_parent
        .parent()
        .unwrap()
        .join("ilium-workspace-owner.json");
    let marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&marker_path).unwrap()).unwrap();
    assert_eq!(marker["worktree_root"], linked.to_str().unwrap());
    assert_eq!(marker["branch"], "agent/isolated");
    assert!(marker["workspace_id"]
        .as_str()
        .is_some_and(|id| !id.is_empty()));
    assert_eq!(marker["custody_revision"], 1);
    let custody_tickets = std::fs::read_dir(marker_parent.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("ilium-workspace-custody-")
        })
        .count();
    assert_eq!(custody_tickets, 1, "launch must publish one custody ticket");

    assert!(
        common::wait_until(
            || std::fs::read_to_string(&server.launch_record).is_ok_and(
                |lines| lines.contains(&format!("{}|{}", linked.display(), linked.display()))
            ),
            Duration::from_secs(5),
        )
        .await,
        "fake Codex did not record the worktree cwd and ILIUM_WORKTREE"
    );
    let tree = server.snapshot().await;
    assert_eq!(tree.pane_cwd(pane_id), Some(linked.as_path()));
    let workspace = tree.pane_workspace(pane_id).expect("workspace on pane");
    assert_eq!(workspace.worktree_root, linked);
    assert_eq!(
        workspace.workspace_id.as_deref(),
        marker["workspace_id"].as_str()
    );
    assert!(workspace.created_by_ilium);

    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 22,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::New {
                branch: "agent/isolated".into(),
                base_ref: "main".into(),
                path: server.root.join("duplicate"),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let duplicate = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::WorkspaceCreateFailed { request_id: 22, .. }
        )
    })
    .await;
    let ServerEvent::WorkspaceCreateFailed { error, .. } = duplicate else {
        unreachable!()
    };
    assert!(error.contains("already exists"), "{error}");
    assert!(!server.root.join("duplicate").exists());

    let foreign = server.root.join("foreign-linked");
    git(
        &server.project,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "agent/foreign",
            foreign.to_str().unwrap(),
            "main",
        ],
    );
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 23,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::Existing {
                path: foreign.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let attached = expect_event(&mut client, Duration::from_secs(10), |event| {
        matches!(event, ServerEvent::WorkspaceCreated { request_id: 23, .. })
    })
    .await;
    let ServerEvent::WorkspaceCreated {
        pane_id: attached_pane_id,
        ..
    } = attached
    else {
        unreachable!()
    };
    let tree = server.snapshot().await;
    assert_eq!(tree.panes().count(), 2);
    assert_eq!(tree.pane_cwd(attached_pane_id), Some(foreign.as_path()));
    assert!(
        !tree
            .pane_workspace(attached_pane_id)
            .unwrap()
            .created_by_ilium
    );

    write_frame(
        &mut client,
        &ClientRequest::RemoveWorkspace {
            request_id: 24,
            pane_id: attached_pane_id,
            force_path: Some(foreign.clone()),
            remove_branch: false,
        },
    )
    .await
    .unwrap();
    let foreign_removal = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::WorkspaceRemovalBlocked { request_id: 24, .. }
        )
    })
    .await;
    let ServerEvent::WorkspaceRemovalBlocked { reasons, .. } = foreign_removal else {
        unreachable!()
    };
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("not created by Ilium")),
        "{reasons:?}"
    );
    assert!(linked.exists());
    assert!(foreign.exists());

    write_frame(
        &mut client,
        &ClientRequest::ClosePane {
            pane_id: attached_pane_id,
        },
    )
    .await
    .unwrap();
    let snapshot = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if tree.get(attached_pane_id).is_none())
    })
    .await;
    assert!(matches!(snapshot, ServerEvent::TreeSnapshot(_)));
    // The copied .local.env is ignored by Git status but still user data.
    write_frame(
        &mut client,
        &ClientRequest::RemoveWorkspace {
            request_id: 27,
            pane_id,
            force_path: None,
            remove_branch: false,
        },
    )
    .await
    .unwrap();
    let ignored = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::WorkspaceRemovalBlocked { request_id: 27, .. }
        )
    })
    .await;
    let ServerEvent::WorkspaceRemovalBlocked { reasons, .. } = ignored else {
        unreachable!()
    };
    assert!(
        reasons.iter().any(|reason| reason.contains("ignored")),
        "{reasons:?}"
    );
    assert!(linked.join(".local.env").exists());
    std::fs::write(linked.join("new-file"), "unsaved work\n").unwrap();
    write_frame(
        &mut client,
        &ClientRequest::RemoveWorkspace {
            request_id: 25,
            pane_id,
            force_path: None,
            remove_branch: false,
        },
    )
    .await
    .unwrap();
    let dirty = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::WorkspaceRemovalBlocked { request_id: 25, .. }
        )
    })
    .await;
    let ServerEvent::WorkspaceRemovalBlocked { reasons, .. } = dirty else {
        unreachable!()
    };
    assert!(
        reasons.iter().any(|reason| reason.contains("untracked")),
        "{reasons:?}"
    );
    assert!(linked.exists());
    assert!(marker_path.exists());

    write_frame(
        &mut client,
        &ClientRequest::RemoveWorkspace {
            request_id: 26,
            pane_id,
            force_path: Some(linked.clone()),
            remove_branch: true,
        },
    )
    .await
    .unwrap();
    let removed = expect_event(&mut client, Duration::from_secs(15), |event| {
        matches!(
            event,
            ServerEvent::WorkspaceRemoved { request_id: 26, .. }
                | ServerEvent::WorkspaceRemovalBlocked { request_id: 26, .. }
        )
    })
    .await;
    if let ServerEvent::WorkspaceRemovalBlocked { reasons, .. } = &removed {
        // Some hosts restrict procfs cwd reads even for a same-user service
        // manager. Force must fail closed when process custody is unknown.
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("process directory probe is unavailable")),
            "{removed:?}"
        );
        assert!(linked.exists());
        assert!(marker_path.exists());
        assert!(foreign.exists());
        assert!(!git_stdout(&server.project, &["branch", "--list", "agent/isolated"]).is_empty());
        return;
    }
    assert!(
        matches!(removed, ServerEvent::WorkspaceRemoved { .. }),
        "{removed:?}"
    );
    assert!(!linked.exists());
    assert!(!marker_path.exists());
    assert!(
        foreign.exists(),
        "Ilium must leave foreign worktrees intact"
    );
    assert!(
        git_stdout(&server.project, &["branch", "--list", "agent/isolated"]).is_empty(),
        "explicit path-confirmed force removal should delete the requested branch"
    );
    assert_eq!(
        ilium_git::list_worktrees(&server.project)
            .await
            .unwrap()
            .len(),
        2
    );
    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("stop isolated server session");
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_clients_cannot_create_two_worktrees_on_one_branch() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut first = server.connect().await;
    let mut second = server.connect().await;
    let first_path = server.root.join("first-linked");
    let second_path = server.root.join("second-linked");
    let first_request = ClientRequest::CreateAgentInWorkspace {
        request_id: 41,
        parent_group: ROOT_ID,
        provider: BuiltinAgentProvider::Codex,
        spec: WorkspaceCreateSpec::New {
            branch: "agent/race".into(),
            base_ref: "main".into(),
            path: first_path.clone(),
        },
        initial_input: None,
    };
    let second_request = ClientRequest::CreateAgentInWorkspace {
        request_id: 42,
        parent_group: ROOT_ID,
        provider: BuiltinAgentProvider::Codex,
        spec: WorkspaceCreateSpec::New {
            branch: "agent/race".into(),
            base_ref: "main".into(),
            path: second_path.clone(),
        },
        initial_input: None,
    };
    let (first_send, second_send) = tokio::join!(
        write_frame(&mut first, &first_request),
        write_frame(&mut second, &second_request),
    );
    first_send.expect("submit first concurrent create");
    second_send.expect("submit second concurrent create");
    let ((first_outcome, _), (second_outcome, _)) = tokio::join!(
        create_result(&mut first, 41),
        create_result(&mut second, 42),
    );
    let outcomes = [&first_outcome, &second_outcome];
    assert_eq!(
        outcomes
            .iter()
            .filter(|event| matches!(event, ServerEvent::WorkspaceCreated { .. }))
            .count(),
        1,
        "{outcomes:?}"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|event| matches!(event, ServerEvent::WorkspaceCreateFailed { .. }))
            .count(),
        1,
        "{outcomes:?}"
    );
    let failure = outcomes
        .iter()
        .find_map(|event| match event {
            ServerEvent::WorkspaceCreateFailed { error, .. } => Some(error),
            _ => None,
        })
        .unwrap();
    assert!(failure.contains("already exists"), "{failure}");
    assert_ne!(first_path.exists(), second_path.exists());
    let worktrees = ilium_git::list_worktrees(&server.project).await.unwrap();
    assert_eq!(worktrees.len(), 2);
    assert_eq!(
        worktrees
            .iter()
            .filter(|worktree| worktree.branch.as_deref() == Some("agent/race"))
            .count(),
        1,
    );
    let tree = server.snapshot().await;
    assert_eq!(tree.panes().count(), 1);
    write_frame(&mut first, &ClientRequest::KillSession)
        .await
        .expect("stop isolated server session");
}

/// New retained-target tests use only this file's isolated server and fake provider.
#[cfg(unix)]
async fn b21_inventory(
    client: &mut SessionStream,
    request_id: u64,
) -> ilium_ipc::WorkspaceInventory {
    write_frame(
        client,
        &ClientRequest::QueryWorkspaceInventory {
            request_id,
            project: ROOT_ID,
        },
    )
    .await
    .unwrap();
    let event = expect_event(client, Duration::from_secs(35), |event| matches!(event, ServerEvent::WorkspaceInventoryReported { request_id: id, .. } if *id == request_id)).await;
    let ServerEvent::WorkspaceInventoryReported { result, .. } = event else {
        unreachable!()
    };
    result.expect("retained inventory")
}
#[cfg(unix)]
async fn b21_retained_target(
    server: &IsolatedWorkspaceServer,
    client: &mut SessionStream,
    request_id: u64,
) -> ilium_ipc::WorkspacePruneTarget {
    let path = server.root.join(format!("retained-{request_id}"));
    // Copy the ignored file first, then collide with the tracked include file.
    // Partial-copy retention works on macOS too, unlike configured setup.
    std::fs::write(
        server.project.join(".worktreeinclude"),
        ".local.env\n.worktreeinclude\n",
    )
    .unwrap();
    write_frame(
        client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Codex,
            spec: WorkspaceCreateSpec::New {
                branch: format!("agent/retained-{request_id}"),
                base_ref: "main".into(),
                path: path.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .unwrap();
    let (result, stages) = create_result(client, request_id).await;
    let ServerEvent::WorkspaceCreateFailed { error, .. } = result else {
        panic!("partial include copy unexpectedly succeeded: {result:?}");
    };
    assert!(error.contains("worktree retained"), "{error}");
    assert!(
        !stages.contains(&WorkspaceCreateStage::RunningSetup),
        "retained security fixture must not depend on platform-specific setup: {stages:?}"
    );
    assert!(path.is_dir());
    assert_eq!(
        std::fs::read(path.join(".local.env")).unwrap(),
        b"isolated fixture\n"
    );
    assert_eq!(
        std::fs::read_to_string(path.join(".worktreeinclude")).unwrap(),
        ".local.env\n"
    );
    assert_eq!(server.snapshot().await.panes().count(), 0);
    b21_inventory(client, request_id + 1)
        .await
        .entries
        .into_iter()
        .find(|entry| entry.path == path)
        .expect("retained row")
        .target
        .expect("complete target")
}
#[cfg(unix)]
async fn b21_prune(
    client: &mut SessionStream,
    request_id: u64,
    target: ilium_ipc::WorkspacePruneTarget,
    mode: ilium_ipc::WorkspacePruneMode,
) -> ilium_ipc::WorkspacePruneResult {
    write_frame(
        client,
        &ClientRequest::PruneWorkspace {
            request_id,
            project: ROOT_ID,
            target,
            mode,
            branch_policy: ilium_ipc::WorkspacePruneBranchPolicy::Keep,
        },
    )
    .await
    .unwrap();
    let event = expect_event(client, Duration::from_secs(35), |event| matches!(event, ServerEvent::WorkspacePruneCompleted { request_id: id, .. } if *id == request_id)).await;
    let ServerEvent::WorkspacePruneCompleted { result, .. } = event else {
        unreachable!()
    };
    result
}
#[cfg(unix)]
#[tokio::test]
async fn b21_retained_inventory_and_safe_prune_preserve_ignored_files() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let target = b21_retained_target(&server, &mut client, 801).await;
    let path = target.worktree_root.clone();
    let result = b21_prune(
        &mut client,
        803,
        target,
        ilium_ipc::WorkspacePruneMode::Safe,
    )
    .await;
    assert_eq!(result.outcome, ilium_ipc::WorkspacePruneOutcome::Blocked);
    assert!(!result.mutation_attempted);
    assert!(
        result
            .reasons
            .iter()
            .any(|reason| reason.contains("ignored")),
        "{result:?}"
    );
    assert_eq!(
        std::fs::read(path.join(".local.env")).unwrap(),
        b"isolated fixture\n"
    );
}
#[cfg(unix)]
#[tokio::test]
async fn retained_worktree_with_a_folder_root_cannot_be_pruned() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let target = b21_retained_target(&server, &mut client, 841).await;
    let path = target.worktree_root.clone();
    write_frame(
        &mut client,
        &ClientRequest::NewFolder {
            parent_group: ROOT_ID,
            path: path.clone(),
        },
    )
    .await
    .unwrap();
    expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let result = b21_prune(
        &mut client,
        843,
        target,
        ilium_ipc::WorkspacePruneMode::Safe,
    )
    .await;
    assert_eq!(result.outcome, ilium_ipc::WorkspacePruneOutcome::Blocked);
    assert!(!result.mutation_attempted);
    assert!(
        result
            .reasons
            .iter()
            .any(|reason| reason.contains("protected")),
        "{result:?}"
    );
    assert!(path.join(".local.env").is_file());
}
#[cfg(unix)]
#[tokio::test]
async fn retained_worktree_with_an_open_editor_cannot_be_pruned() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let target = b21_retained_target(&server, &mut client, 851).await;
    let path = target.worktree_root.clone();
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: ilium_ipc::NewPaneKind::Editor(path.join(".local.env")),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let result = b21_prune(
        &mut client,
        853,
        target,
        ilium_ipc::WorkspacePruneMode::Safe,
    )
    .await;
    assert_eq!(result.outcome, ilium_ipc::WorkspacePruneOutcome::Blocked);
    assert!(!result.mutation_attempted);
    assert!(
        result
            .reasons
            .iter()
            .any(|reason| reason.contains("Ilium panes")),
        "{result:?}"
    );
    assert!(path.join(".local.env").is_file());
}
#[cfg(unix)]
#[tokio::test]
async fn b21_discard_requires_the_exact_confirmed_path() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let target = b21_retained_target(&server, &mut client, 811).await;
    let path = target.worktree_root.clone();
    let result = b21_prune(
        &mut client,
        813,
        target,
        ilium_ipc::WorkspacePruneMode::DiscardFiles {
            confirmed_path: server.project.clone(),
        },
    )
    .await;
    assert_eq!(result.outcome, ilium_ipc::WorkspacePruneOutcome::Blocked);
    assert!(!result.mutation_attempted);
    assert!(
        result
            .reasons
            .iter()
            .any(|reason| reason.contains("exact worktree path")),
        "{result:?}"
    );
    assert!(path.join(".local.env").is_file());
}
#[cfg(unix)]
#[tokio::test]
async fn b21_stale_head_cannot_be_discarded_by_an_old_inventory_target() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let target = b21_retained_target(&server, &mut client, 821).await;
    let path = target.worktree_root.clone();
    git(
        &path,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "new work since confirmation",
        ],
    );
    let new_head = git_stdout(&path, &["rev-parse", "HEAD"]);
    let result = b21_prune(
        &mut client,
        823,
        target,
        ilium_ipc::WorkspacePruneMode::DiscardFiles {
            confirmed_path: path.clone(),
        },
    )
    .await;
    assert_eq!(result.outcome, ilium_ipc::WorkspacePruneOutcome::Blocked);
    assert!(!result.mutation_attempted);
    assert!(
        result
            .reasons
            .iter()
            .any(|reason| reason.contains("stale target")),
        "{result:?}"
    );
    assert_eq!(git_stdout(&path, &["rev-parse", "HEAD"]), new_head);
    assert!(path.join(".local.env").is_file());
}
#[cfg(unix)]
#[tokio::test]
async fn b21_forged_owner_uuid_cannot_authorize_prune() {
    let server = IsolatedWorkspaceServer::start().await;
    let mut client = server.connect().await;
    let mut target = b21_retained_target(&server, &mut client, 831).await;
    let path = target.worktree_root.clone();
    target.workspace_id = "00000000-0000-4000-8000-000000000001".into();
    let result = b21_prune(
        &mut client,
        833,
        target,
        ilium_ipc::WorkspacePruneMode::DiscardFiles {
            confirmed_path: path.clone(),
        },
    )
    .await;
    assert_eq!(result.outcome, ilium_ipc::WorkspacePruneOutcome::Blocked);
    assert!(!result.mutation_attempted);
    assert!(path.is_dir());
}
#[tokio::test]
async fn b21_head_base_uses_creation_oid_not_the_agents_current_head() {
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main");
    std::fs::create_dir(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "user.name", "Ilium Test"]);
    git(&main, &["config", "user.email", "ilium@example.invalid"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let creation = ilium_git::resolve_commit(&main, "HEAD").await.unwrap();
    let linked = directory.path().join("linked");
    ilium_git::create_worktree(&main, &linked, "agent/base-safety", &creation)
        .await
        .unwrap();
    git(
        &linked,
        &["commit", "-q", "--allow-empty", "-m", "unmerged agent work"],
    );
    let agent_tip = ilium_git::resolve_commit(&linked, "HEAD").await.unwrap();
    let (_, safety_tip) = ilium_git::removal_base(&linked, "HEAD", &creation)
        .await
        .unwrap();
    assert_eq!(safety_tip, creation);
    assert!(
        !ilium_git::commit_is_ancestor(&main, &agent_tip, &safety_tip)
            .await
            .unwrap()
    );
    let (qualified, _) = ilium_git::qualified_creation_base(&main, "HEAD")
        .await
        .unwrap();
    assert_eq!(qualified, "refs/heads/main");
}
#[tokio::test]
async fn b21_branch_deletion_retains_a_moved_tip() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "-q", "-b", "main"]);
    git(directory.path(), &["config", "user.name", "Ilium Test"]);
    git(
        directory.path(),
        &["config", "user.email", "ilium@example.invalid"],
    );
    git(
        directory.path(),
        &["commit", "-q", "--allow-empty", "-m", "base"],
    );
    let old_tip = ilium_git::resolve_commit(directory.path(), "HEAD")
        .await
        .unwrap();
    git(directory.path(), &["branch", "agent/keep"]);
    git(
        directory.path(),
        &["commit", "-q", "--allow-empty", "-m", "new main"],
    );
    let new_tip = ilium_git::resolve_commit(directory.path(), "HEAD")
        .await
        .unwrap();
    git(directory.path(), &["branch", "-f", "agent/keep", &new_tip]);
    assert!(ilium_git::delete_removal_branch(
        directory.path(),
        "agent/keep",
        &old_tip,
        "refs/heads/main",
        &old_tip
    )
    .await
    .is_err());
    assert_eq!(
        ilium_git::branch_tip(directory.path(), "agent/keep")
            .await
            .unwrap(),
        Some(new_tip)
    );
}
