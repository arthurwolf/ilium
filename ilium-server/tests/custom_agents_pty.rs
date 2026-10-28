//! Real server/PTY test with isolated fake provider executables and non-secret sentinels.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use ilium_core::custom_agent::{
    CustomAgentIdentity, CustomAgentProfile, CustomAgentSettings, EnvironmentEntry,
};
use ilium_core::{BuiltinAgentProvider, ROOT_ID};
use ilium_ipc::{
    write_frame, ClientRequest, NewPaneKind, NewPaneWorkingDirectory, ServerEvent,
    WorkspaceCreateSpec,
};
use ilium_transport::{Liveness, SessionEndpoint};

mod common;

struct OwnedServer(Child);
impl Drop for OwnedServer {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[tokio::test]
async fn custom_agent_real_pty_preserves_literal_environment_for_both_providers() {
    let directory = tempfile::Builder::new()
        .prefix("custom-agent-pty-")
        .tempdir()
        .unwrap();
    let root = directory.path();
    let bin = root.join("bin");
    let config = root.join("config");
    let project = root.join("project");
    let capture = root.join("capture");
    for path in [&bin, &config, &project, &capture] {
        std::fs::create_dir(path).unwrap();
    }
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    ] {
        assert!(Command::new("git")
            .current_dir(&project)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    let socket = root.join("s.sock");
    let snapshot = root.join("snapshot.json");
    let log = root.join("server.log");
    let profiles = [
        (
            "one",
            BuiltinAgentProvider::Claude,
            "literal 'one' = ☃\nline",
        ),
        ("two", BuiltinAgentProvider::Claude, "other profile"),
        ("three", BuiltinAgentProvider::Codex, "codex profile"),
        ("four", BuiltinAgentProvider::Claude, "new worktree profile"),
        (
            "five",
            BuiltinAgentProvider::Codex,
            "existing worktree profile",
        ),
    ];
    let profiles: Vec<_> = profiles
        .into_iter()
        .map(|(id, base_provider, value)| CustomAgentProfile {
            id: id.into(),
            name: id.into(),
            icon: "✦".into(),
            base_provider,
            environment: vec![
                EnvironmentEntry {
                    key: "PROFILE_TAG".into(),
                    value: id.into(),
                },
                EnvironmentEntry {
                    key: "SENTINEL_VALUE".into(),
                    value: value.into(),
                },
                EnvironmentEntry {
                    key: "EMPTY_VALUE".into(),
                    value: String::new(),
                },
            ],
        })
        .collect();
    #[derive(serde::Serialize)]
    struct Document {
        custom_agents: CustomAgentSettings,
    }
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let config_text = format!(
        "[debug]\nfile_logging_enabled = true\n[api]\nport = {port}\n{}",
        toml::to_string(&Document {
            custom_agents: CustomAgentSettings {
                profiles: profiles.clone()
            }
        })
        .unwrap()
    );
    std::fs::write(config.join("config.toml"), config_text).unwrap();
    std::fs::set_permissions(
        config.join("config.toml"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    for executable in ["claude", "codex"] {
        let path = bin.join(executable);
        std::fs::write(&path, "#!/bin/sh\nprintf '%s' \"$SENTINEL_VALUE\" > \"$CAPTURE_DIR/$PROFILE_TAG\"\nprintf '%s' \"${EMPTY_VALUE+x}:$EMPTY_VALUE\" > \"$CAPTURE_DIR/$PROFILE_TAG.empty\"\nprintf '%s' \"${ILIUM_WORKTREE-unset}|${ILIUM_BRANCH-unset}\" > \"$CAPTURE_DIR/$PROFILE_TAG.identity\"\nwhile IFS= read -r line; do :; done\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let search_path =
        std::env::join_paths([bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
    let binary = std::env::var("CARGO_BIN_EXE_ilium-server").unwrap();
    let child = Command::new(&binary)
        .args([
            "--session-name",
            "custom-agent-test",
            "--socket-path",
            socket.to_str().unwrap(),
            "--snapshot-path",
            snapshot.to_str().unwrap(),
            "--session-cwd",
            project.to_str().unwrap(),
            "--log-path",
            log.to_str().unwrap(),
        ])
        .env("PATH", &search_path)
        .env("SHELL", "/bin/sh")
        .env("CAPTURE_DIR", &capture)
        .env("ILIUM_WORKTREE", "stale-parent-worktree")
        .env("ILIUM_BRANCH", "stale-parent-branch")
        .env(ilium_server::paths::CONFIG_DIR_ENV, &config)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut server = OwnedServer(child);
    let endpoint = SessionEndpoint::from_path(&socket);
    assert!(
        common::wait_until(
            || endpoint.probe_liveness() == Liveness::Live,
            Duration::from_secs(15)
        )
        .await
    );
    let mut client = endpoint.connect().await.unwrap();
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "custom-agent-test".into(),
            hello: Default::default(),
        },
    )
    .await
    .unwrap();
    common::read_initial_state(&mut client, Duration::from_secs(10)).await;
    let mut first_pane = None;
    for (index, profile) in profiles.iter().take(3).enumerate() {
        let request_id = index as u64 + 1;
        let request = ClientRequest::NewPane {
            request_id,
            parent_group: ROOT_ID,
            kind: NewPaneKind::CustomAgent {
                identity: CustomAgentIdentity {
                    profile_id: profile.id.clone(),
                    base_provider: profile.base_provider,
                },
                initial_input: None,
            },
            working_directory: NewPaneWorkingDirectory::ProjectRoot,
        };
        assert!(!format!("{request:?}").contains(&profile.environment[1].value));
        write_frame(&mut client, &request).await.unwrap();
        let event = common::expect_event(&mut client, Duration::from_secs(20), |event| matches!(event, ServerEvent::PaneCreateFinished { request_id: id, .. } if *id == request_id)).await;
        match event {
            ServerEvent::PaneCreateFinished {
                result: Ok(pane_id),
                ..
            } => {
                if index == 0 {
                    first_pane = Some(pane_id);
                }
            }
            other => panic!("profile launch failed: {other:?}"),
        }
    }
    let linked = root.join("linked");
    for (offset, profile) in profiles.iter().skip(3).enumerate() {
        let request_id = offset as u64 + 4;
        let spec = if offset == 0 {
            WorkspaceCreateSpec::New {
                branch: "agent/profile-test".into(),
                base_ref: "main".into(),
                path: linked.clone(),
            }
        } else {
            WorkspaceCreateSpec::Existing {
                path: linked.clone(),
            }
        };
        let request = ClientRequest::CreateCustomAgentInWorkspace {
            request_id,
            parent_group: ROOT_ID,
            identity: CustomAgentIdentity {
                profile_id: profile.id.clone(),
                base_provider: profile.base_provider,
            },
            spec,
            initial_input: None,
        };
        assert!(!format!("{request:?}").contains(&profile.environment[1].value));
        write_frame(&mut client, &request).await.unwrap();
        let event = common::expect_event(&mut client, Duration::from_secs(40), |event| matches!(event, ServerEvent::WorkspaceCreated { request_id: id, .. } | ServerEvent::WorkspaceCreateFailed { request_id: id, .. } if *id == request_id)).await;
        assert!(
            matches!(event, ServerEvent::WorkspaceCreated { .. }),
            "profile worktree launch failed: {event:?}"
        );
    }
    for profile in &profiles {
        let output = capture.join(&profile.id);
        assert!(
            common::wait_until(
                || output.exists()
                    && capture.join(format!("{}.empty", profile.id)).exists()
                    && capture.join(format!("{}.identity", profile.id)).exists(),
                Duration::from_secs(15)
            )
            .await
        );
        assert!(
            std::fs::read_to_string(output).unwrap() == profile.environment[1].value,
            "agent received the wrong literal environment value"
        );
        assert_eq!(
            std::fs::read_to_string(capture.join(format!("{}.empty", profile.id))).unwrap(),
            "x:"
        );
        let identity =
            std::fs::read_to_string(capture.join(format!("{}.identity", profile.id))).unwrap();
        if profile.id == "four" || profile.id == "five" {
            assert_eq!(identity, format!("{}|unset", linked.display()));
        } else {
            assert_eq!(identity, "unset|unset");
        }
    }
    let first_pane = first_pane.expect("first profile launched");
    let mut changed_profiles = profiles.clone();
    changed_profiles[0].environment[1].value = "updated value".into();
    let changed_config = format!(
        "[debug]\nfile_logging_enabled = true\n[api]\nport = {port}\n{}",
        toml::to_string(&Document {
            custom_agents: CustomAgentSettings {
                profiles: changed_profiles.clone()
            }
        })
        .unwrap()
    );
    std::fs::write(config.join("config.toml"), changed_config).unwrap();
    assert_eq!(
        std::fs::read_to_string(capture.join("one")).unwrap(),
        profiles[0].environment[1].value
    );
    write_frame(
        &mut client,
        &ClientRequest::ReplacePaneWithCommand {
            pane_id: first_pane,
            command_line: "claude".into(),
        },
    )
    .await
    .unwrap();
    assert!(
        common::wait_until(
            || std::fs::read_to_string(capture.join("one"))
                .is_ok_and(|value| value == "updated value"),
            Duration::from_secs(20)
        )
        .await,
        "replacement did not receive the updated profile value"
    );
    let replacement = common::expect_event(&mut client, Duration::from_secs(20), |event| matches!(event, ServerEvent::PaneCustomAgentIdentity { pane_id, identity: Some(identity) } if *pane_id != first_pane && identity.profile_id == "one")).await;
    let ServerEvent::PaneCustomAgentIdentity {
        pane_id: replacement_pane,
        ..
    } = replacement
    else {
        unreachable!()
    };
    changed_profiles.remove(0);
    let missing_config = format!(
        "[debug]\nfile_logging_enabled = true\n[api]\nport = {port}\n{}",
        toml::to_string(&Document {
            custom_agents: CustomAgentSettings {
                profiles: changed_profiles
            }
        })
        .unwrap()
    );
    std::fs::write(config.join("config.toml"), missing_config).unwrap();
    write_frame(
        &mut client,
        &ClientRequest::ReplacePaneWithCommand {
            pane_id: replacement_pane,
            command_line: "claude".into(),
        },
    )
    .await
    .unwrap();
    let error = common::expect_event(&mut client, Duration::from_secs(20), |event| {
        matches!(event, ServerEvent::Error { .. })
    })
    .await;
    assert!(
        matches!(error, ServerEvent::Error { message } if message.contains("profile")),
        "missing profile must report an actionable error"
    );
    assert!(
        common::wait_until(
            || std::fs::read_to_string(&snapshot)
                .is_ok_and(|text| profiles.iter().all(|profile| text.contains(&profile.id))),
            Duration::from_secs(15)
        )
        .await
    );
    let snapshot_text =
        std::fs::read_to_string(&snapshot).expect("session snapshot written before shutdown");
    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    assert!(
        common::wait_until(
            || server.0.try_wait().ok().flatten().is_some(),
            Duration::from_secs(15)
        )
        .await
    );
    let log_text = std::fs::read_to_string(&log).expect("enabled server log written on shutdown");
    for profile in &profiles {
        assert!(snapshot_text.contains(&profile.id));
        assert!(!snapshot_text.contains(&profile.environment[1].value));
        assert!(!log_text.contains(&profile.environment[1].value));
    }
    // KillSession removes its snapshot, so restore the captured bytes to simulate
    // the next launch after an unclean shutdown while still cleaning up PTYs.
    std::fs::write(&snapshot, &snapshot_text).unwrap();
    let restore_log = root.join("restore.log");
    let child = Command::new(&binary)
        .args(["--session-name", "custom-agent-test", "--socket-path", socket.to_str().unwrap(), "--snapshot-path", snapshot.to_str().unwrap(), "--session-cwd", project.to_str().unwrap(), "--log-path", restore_log.to_str().unwrap()])
        .env("PATH", &search_path).env("SHELL", "/bin/sh").env("CAPTURE_DIR", &capture)
        .env(ilium_server::paths::CONFIG_DIR_ENV, &config)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let mut restored_server = OwnedServer(child);
    assert!(common::wait_until(|| endpoint.probe_liveness() == Liveness::Live, Duration::from_secs(15)).await);
    let mut restored_client = endpoint.connect().await.unwrap();
    write_frame(&mut restored_client, &ClientRequest::Attach { session: "custom-agent-test".into(), hello: Default::default() }).await.unwrap();
    let (tree, events) = common::read_initial_state(&mut restored_client, Duration::from_secs(15)).await;
    assert!(tree.get(replacement_pane).is_some(), "missing profile must retain its recoverable pane");
    assert!(events.iter().any(|event| matches!(event, ServerEvent::PaneCustomAgentIdentity { pane_id, identity: Some(identity) } if *pane_id == replacement_pane && identity.profile_id == "one")), "missing profile identity must remain visible after restore");
    assert!(events.iter().any(|event| matches!(event, ServerEvent::BlockingErrorsSnapshot { errors } if errors.iter().any(|error| error.message.contains("profile")))), "restore must report the missing profile");
    write_frame(&mut restored_client, &ClientRequest::KillSession).await.unwrap();
    assert!(common::wait_until(|| restored_server.0.try_wait().ok().flatten().is_some(), Duration::from_secs(15)).await);
}
