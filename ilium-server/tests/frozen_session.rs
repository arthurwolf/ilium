//! End-to-end frozen provider session recovery through the server IPC path.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

#[cfg(unix)]
use std::process::Command;

use ilium_core::{BuiltinAgentProvider, NodeId, ROOT_ID};
use ilium_ipc::{
    read_frame, write_frame, ClientRequest, NewPaneKind, ServerEvent, WorkspaceCreateSpec,
};
use ilium_server::config::DetectionConfig;

mod common;
use common::{read_initial_state, TestServer};
use ilium_test_fixtures::{install, FixtureBehavior};
use ilium_transport::SessionStream;

const SESSION_ID: &str = "00000000-0000-4000-8000-000000000123";

static FIXTURE_PATH_LOCK: Mutex<()> = Mutex::new(());

struct FixturePath {
    previous: Option<OsString>,
    previous_zdotdir: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl FixturePath {
    /// Puts the fixture directory first on `PATH` and points `ZDOTDIR` at it.
    ///
    /// The pane shell is `$SHELL -c`, and a user `~/.zshenv` can prepend
    /// `~/.local/bin`-style directories, so the real `claude` on the machine
    /// would win over the fixture. An empty `ZDOTDIR` (the fixture directory
    /// holds no `.zshenv`) keeps the pane's `PATH` exactly as the test set it.
    fn prepend(directory: &Path) -> Self {
        let lock = FIXTURE_PATH_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = std::env::var_os("PATH");
        let previous_zdotdir = std::env::var_os("ZDOTDIR");
        let mut search_paths = vec![directory.to_path_buf()];
        if let Some(previous) = previous.as_deref() {
            search_paths.extend(std::env::split_paths(previous));
        }
        let path = std::env::join_paths(search_paths)
            .expect("fixture and original directories produce a valid PATH");
        std::env::set_var("PATH", path);
        std::env::set_var("ZDOTDIR", directory);
        Self {
            previous,
            previous_zdotdir,
            _lock: lock,
        }
    }
}

impl Drop for FixturePath {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var("PATH", previous),
            None => std::env::remove_var("PATH"),
        }
        match self.previous_zdotdir.take() {
            Some(previous) => std::env::set_var("ZDOTDIR", previous),
            None => std::env::remove_var("ZDOTDIR"),
        }
    }
}

async fn attach(server: &TestServer, session_name: &str) -> SessionStream {
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: session_name.to_string(),
            hello: Default::default(),
        },
    )
    .await
    .expect("send Attach");
    read_initial_state(&mut client, Duration::from_secs(5)).await;
    client
}

async fn create_detected_claude_pane(client: &mut SessionStream) -> NodeId {
    write_frame(
        client,
        &ClientRequest::NewPane {
            request_id: 0,
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(format!("claude --resume {SESSION_ID}")),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("send NewPane");

    let mut tree = None;
    let mut session_ids = HashMap::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match read_frame::<ServerEvent, _>(client)
                .await
                .expect("read pane creation event")
            {
                ServerEvent::TreeSnapshot(snapshot) if snapshot.panes().count() == 1 => {
                    tree = Some(snapshot);
                }
                ServerEvent::PaneSessionIdResolved {
                    pane_id,
                    session_id,
                    ..
                } => {
                    session_ids.insert(pane_id, session_id);
                }
                _ => {}
            }

            if let Some(tree) = tree.as_ref() {
                let pane_id = tree.pane_ids_in_tree_order()[0];
                if session_ids.get(&pane_id).is_some_and(|id| id == SESSION_ID) {
                    return pane_id;
                }
            }
        }
    })
    .await
    .expect("Claude fixture is detected with its requested session ID")
}

/// Writes the transcript Claude would keep for a session started in `cwd`.
/// Detection only accepts a session whose transcript names the pane's project,
/// so a worktree pane needs its own transcript under the worktree's slug.
fn write_verified_claude_transcript(server: &TestServer, session_id: &str, cwd: &Path) {
    let resolved_cwd =
        ilium_platform::paths::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let slug: String = resolved_cwd
        .to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let directory = server.home_dir.join(".claude").join("projects").join(slug);
    std::fs::create_dir_all(&directory).expect("create Claude transcript directory");
    std::fs::write(
        directory.join(format!("{session_id}.jsonl")),
        serde_json::json!({
            "type": "user",
            "sessionId": session_id,
            "cwd": cwd,
            "message": {"content": "integration test prompt"}
        })
        .to_string()
            + "\n",
    )
    .expect("write verified Claude transcript");
}

#[cfg(unix)]
fn run_git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("run Git fixture command");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Parent and child index of a pane: the slot a replacement must reoccupy.
fn tree_slot(tree: &ilium_core::Tree, pane_id: NodeId) -> Option<(NodeId, usize)> {
    let parent = tree.parent_of(pane_id)?;
    let index = tree
        .children_of(parent)
        .ok()?
        .iter()
        .position(|child| *child == pane_id)?;
    Some((parent, index))
}

async fn wait_for_detected_replacement(
    client: &mut SessionStream,
    previous_pane_id: NodeId,
) -> (NodeId, ilium_core::Tree) {
    let mut replacement_tree = None;
    let mut session_ids = HashMap::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match read_frame::<ServerEvent, _>(client)
                .await
                .expect("read unfreeze result")
            {
                ServerEvent::Error { message } => {
                    panic!("server rejected UnfreezePane: {message}");
                }
                ServerEvent::TreeSnapshot(tree)
                    if tree.get(previous_pane_id).is_none() && tree.panes().count() == 1 =>
                {
                    replacement_tree = Some(tree);
                }
                ServerEvent::PaneSessionIdResolved {
                    pane_id,
                    session_id,
                    ..
                } => {
                    session_ids.insert(pane_id, session_id);
                }
                _ => {}
            }

            if let Some(tree) = replacement_tree.as_ref() {
                let replacement_id = tree.pane_ids_in_tree_order()[0];
                if replacement_id != previous_pane_id
                    && session_ids
                        .get(&replacement_id)
                        .is_some_and(|id| id == SESSION_ID)
                {
                    return (replacement_id, tree.clone());
                }
            }
        }
    })
    .await
    .expect("unfreeze starts a replacement with the same provider session ID")
}

#[tokio::test]
async fn unfreeze_restarts_the_saved_provider_session_and_recovers_its_identity() {
    let fixtures = tempfile::tempdir().expect("fixture directory");
    install(fixtures.path(), "claude", &FixtureBehavior::Idle);
    let _fixture_path = FixturePath::prepend(fixtures.path());

    let session_name = "frozen-session-recovery-test";
    let detection_config = DetectionConfig {
        working_poll_interval: Duration::from_millis(100),
        idle_poll_interval: Duration::from_millis(100),
        auto_answer_interstitial_prompts: true,
    };
    let mut server = TestServer::start_with_detection_config(session_name, detection_config).await;
    write_verified_claude_transcript(&server, SESSION_ID, &server.project_cwd);
    let mut client = attach(&server, session_name).await;
    let frozen_pane_id = create_detected_claude_pane(&mut client).await;

    write_frame(
        &mut client,
        &ClientRequest::FreezePane {
            pane_id: frozen_pane_id,
            resume_command: format!("claude --resume {SESSION_ID}"),
        },
    )
    .await
    .expect("send FreezePane");
    let event = common::expect_event(&mut client, Duration::from_secs(10), |event| {
        matches!(event, ServerEvent::PaneFrozen { pane_id, .. } if *pane_id == frozen_pane_id)
    })
    .await;
    assert_eq!(
        event,
        ServerEvent::PaneFrozen {
            pane_id: frozen_pane_id,
            result: Ok(()),
        }
    );

    // A restarted client must recover the frozen marker from the server's
    // authoritative origin before offering the unfreeze action again.
    drop(client);
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: session_name.to_string(),
            hello: Default::default(),
        },
    )
    .await
    .expect("reattach after freezing");
    let (initial_tree, initial_events) =
        read_initial_state(&mut client, Duration::from_secs(5)).await;
    assert!(
        initial_events.iter().any(|event| matches!(
            event,
            ServerEvent::PaneFrozen {
                pane_id,
                result: Ok(()),
            } if *pane_id == frozen_pane_id
        )),
        "reattaching must restore the server's frozen-pane marker"
    );
    let frozen_slot = tree_slot(&initial_tree, frozen_pane_id)
        .expect("frozen pane keeps its tree slot before unfreeze");

    write_frame(
        &mut client,
        &ClientRequest::UnfreezePane {
            pane_id: frozen_pane_id,
        },
    )
    .await
    .expect("send UnfreezePane");
    let (replacement_id, replacement_tree) =
        wait_for_detected_replacement(&mut client, frozen_pane_id).await;
    assert_ne!(replacement_id, frozen_pane_id);
    assert_eq!(
        tree_slot(&replacement_tree, replacement_id),
        Some(frozen_slot),
        "the replacement must take the frozen pane's parent and child index"
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("stop test server session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[cfg(unix)]
#[tokio::test]
async fn unfreeze_preserves_worktree_ownership() {
    let fixtures = tempfile::tempdir().expect("fixture directory");
    install(fixtures.path(), "claude", &FixtureBehavior::Idle);
    let _fixture_path = FixturePath::prepend(fixtures.path());

    let session_name = "frozen-worktree-recovery-test";
    let detection_config = DetectionConfig {
        working_poll_interval: Duration::from_millis(100),
        idle_poll_interval: Duration::from_millis(100),
        auto_answer_interstitial_prompts: true,
    };
    let mut server = TestServer::start_with_detection_config(session_name, detection_config).await;
    run_git(&server.project_cwd, &["init", "--initial-branch=main"]);
    run_git(&server.project_cwd, &["config", "user.name", "Ilium test"]);
    run_git(
        &server.project_cwd,
        &["config", "user.email", "ilium-test@example.invalid"],
    );
    std::fs::write(server.project_cwd.join("README.md"), "test worktree\n")
        .expect("write repository fixture");
    run_git(&server.project_cwd, &["add", "README.md"]);
    run_git(&server.project_cwd, &["commit", "-m", "test repository"]);

    let worktree_parent = tempfile::tempdir().expect("worktree parent");
    let worktree_root = worktree_parent.path().join("linked");

    let mut client = attach(&server, session_name).await;
    write_frame(
        &mut client,
        &ClientRequest::CreateAgentInWorkspace {
            request_id: 71,
            parent_group: ROOT_ID,
            provider: BuiltinAgentProvider::Claude,
            spec: WorkspaceCreateSpec::New {
                branch: "agent/frozen-session-test".to_string(),
                base_ref: "main".to_string(),
                path: worktree_root.clone(),
            },
            initial_input: None,
        },
    )
    .await
    .expect("send CreateAgentInWorkspace");
    let created = common::expect_event(&mut client, Duration::from_secs(20), |event| {
        matches!(
            event,
            ServerEvent::WorkspaceCreated { request_id: 71, .. }
                | ServerEvent::WorkspaceCreateFailed { request_id: 71, .. }
        )
    })
    .await;
    let workspace_pane_id = match created {
        ServerEvent::WorkspaceCreated { pane_id, .. } => pane_id,
        ServerEvent::WorkspaceCreateFailed { error, .. } => {
            panic!("could not create worktree agent fixture: {error}");
        }
        _ => unreachable!("event predicate accepts only workspace results"),
    };
    // Resumed identity is independently verified against the provider's
    // transcript in this worktree; argv alone is not proof of session custody.
    write_verified_claude_transcript(&server, SESSION_ID, &worktree_root);

    write_verified_claude_transcript(&server, SESSION_ID, &worktree_root);
    write_frame(
        &mut client,
        &ClientRequest::ReplacePaneWithCommand {
            pane_id: workspace_pane_id,
            command_line: format!("claude --resume {SESSION_ID}"),
        },
    )
    .await
    .expect("send ReplacePaneWithCommand");
    let (agent_pane_id, replacement_tree) =
        wait_for_detected_replacement(&mut client, workspace_pane_id).await;
    assert_ne!(agent_pane_id, workspace_pane_id);
    let original_workspace = replacement_tree
        .pane_workspace(agent_pane_id)
        .expect("replacement retains workspace identity")
        .clone();
    assert_eq!(
        original_workspace.worktree_root,
        std::fs::canonicalize(&worktree_root).unwrap()
    );
    assert_eq!(original_workspace.branch, "agent/frozen-session-test");
    assert!(original_workspace.created_by_ilium);
    assert!(original_workspace.workspace_id.is_some());

    write_frame(
        &mut client,
        &ClientRequest::FreezePane {
            pane_id: agent_pane_id,
            resume_command: format!("claude --resume {SESSION_ID}"),
        },
    )
    .await
    .expect("send FreezePane");
    let event = common::expect_event(&mut client, Duration::from_secs(10), |event| {
        matches!(event, ServerEvent::PaneFrozen { pane_id, .. } if *pane_id == agent_pane_id)
    })
    .await;
    assert_eq!(
        event,
        ServerEvent::PaneFrozen {
            pane_id: agent_pane_id,
            result: Ok(()),
        }
    );

    let (resumed_pane_id, resumed_tree) =
        wait_for_detected_replacement_after_unfreeze(&mut client, agent_pane_id).await;
    assert_ne!(resumed_pane_id, agent_pane_id);
    assert_eq!(
        resumed_tree.pane_workspace(resumed_pane_id),
        Some(&original_workspace)
    );

    write_frame(
        &mut client,
        &ClientRequest::ClosePaneWithWorkspaceDisposition {
            request_id: 72,
            pane_id: resumed_pane_id,
            disposition: ilium_ipc::WorkspaceDisposition::RemoveWorktreeAndBranch,
        },
    )
    .await
    .expect("remove the task-owned worktree after unfreezing");
    let removal = common::expect_event(&mut client, Duration::from_secs(20), |event| {
        matches!(
            event,
            ServerEvent::WorkspaceRemoved { request_id: 72, pane_id }
                | ServerEvent::WorkspaceRemovalBlocked { request_id: 72, pane_id, .. }
                if *pane_id == resumed_pane_id
        )
    })
    .await;
    if let ServerEvent::WorkspaceRemovalBlocked { reasons, .. } = &removal {
        // Restricted procfs cannot establish process custody. Unfreezing must
        // preserve ownership without bypassing this independent safety gate.
        assert!(!reasons.is_empty());
        assert!(
            reasons.iter().all(|reason| {
                reason.contains("cannot inspect cwd of process")
                    && reason.contains("Permission denied")
                    && !reason.contains("REMOVAL OUTCOME UNCERTAIN")
            }),
            "unexpected worktree removal refusal after unfreeze: {removal:?}"
        );
        assert!(worktree_root.is_dir());
        run_git(
            &server.project_cwd,
            &[
                "show-ref",
                "--verify",
                "refs/heads/agent/frozen-session-test",
            ],
        );
    } else {
        assert!(
            matches!(
                removal,
                ServerEvent::WorkspaceRemoved { request_id: 72, .. }
            ),
            "worktree custody remained unresolved after unfreeze: {removal:?}"
        );
        assert!(!worktree_root.exists());
        let branch = Command::new("git")
            .current_dir(&server.project_cwd)
            .args(["branch", "--list", "agent/frozen-session-test"])
            .output()
            .expect("inspect removed fixture branch");
        assert!(branch.status.success());
        assert!(
            branch.stdout.is_empty(),
            "successful removal must remove the owned branch"
        );
    }

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("stop test server session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

async fn wait_for_detected_replacement_after_unfreeze(
    client: &mut SessionStream,
    frozen_pane_id: NodeId,
) -> (NodeId, ilium_core::Tree) {
    write_frame(
        client,
        &ClientRequest::UnfreezePane {
            pane_id: frozen_pane_id,
        },
    )
    .await
    .expect("send UnfreezePane for worktree pane");
    wait_for_detected_replacement(client, frozen_pane_id).await
}
