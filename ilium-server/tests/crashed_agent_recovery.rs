//! Real detection/IPC recovery after a provider exits before flushing its prompt.
use std::path::PathBuf;
use std::time::Duration;

use ilium_core::{project_pane_signals, AgentClass, NodeId, NodeKind, NowSignal, Tree, ROOT_ID};
use ilium_ipc::{
    read_frame, write_frame, ClientRequest, NewPaneKind, PromptSubmissionSource, ServerEvent,
};
use ilium_server::config::DetectionConfig;
use ilium_test_fixtures::{install, FixtureBehavior};

mod common;
use common::{expect_event, read_initial_state, wait_until, TestServer};

const WAIT: Duration = Duration::from_secs(30);
const SESSION_ID: &str = "840cdeca-c171-4556-8494-28b341e38c14";

fn transcript(server: &TestServer, provider: &str) -> PathBuf {
    let (directory, filename, content) = if provider == "claude" {
        let slug: String = server
            .project_cwd
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
        (
            server.home_dir.join(".claude/projects").join(slug),
            format!("{SESSION_ID}.jsonl"),
            serde_json::json!({"type":"user", "sessionId":SESSION_ID,
                "cwd":server.project_cwd, "message":{"content":"previous submitted prompt"}}),
        )
    } else {
        (
            server.home_dir.join(".codex/sessions/2026/07/14"),
            format!("rollout-2026-07-14T12-00-00-{SESSION_ID}.jsonl"),
            serde_json::json!({"type":"session_meta", "payload":{
                "id":SESSION_ID, "cwd":server.project_cwd}}),
        )
    };
    std::fs::create_dir_all(&directory).expect("isolated transcript directory");
    let path = directory.join(filename);
    std::fs::write(&path, format!("{content}\n")).expect("existing valid provider transcript");
    path
}

fn assert_recovery(tree: &Tree, pane_id: NodeId, class: &AgentClass, prompt: &str) {
    let node = tree
        .get(pane_id)
        .expect("crashed agent pane must remain recoverable");
    let NodeKind::Pane {
        status,
        last_prompt,
        ..
    } = &node.kind
    else {
        panic!("crashed pane changed kind: {:?}", node.kind);
    };
    let agent = status
        .known_agent_state()
        .expect("crashed provider must not become a plain terminal");
    assert_eq!(&agent.class, class, "historical provider identity lost");
    assert!(
        status.agent_state().is_none(),
        "historical provider must not authorize a live composer"
    );
    assert!(
        matches!(
            project_pane_signals(status, &[], false, None).now,
            NowSignal::AgentUnavailable(_)
        ),
        "historical Working evidence must not project as live activity"
    );
    assert!(
        !agent.completion_unread,
        "a crash must not create a success bell"
    );
    assert_eq!(
        last_prompt.as_deref(),
        Some(prompt),
        "authored prompt lost or overwritten by stale transcript"
    );
    let recovery = status.agent_recovery().expect("historical recovery record");
    assert_eq!(recovery.session_id.as_deref(), Some(SESSION_ID));
    assert_eq!(recovery.last_prompt.as_deref(), Some(prompt));
}

async fn recovery_after_unflushed_crash(
    provider: &str,
    class: AgentClass,
    nested_shell: bool,
    interpreted_launcher: bool,
) {
    let session_name = format!("crash-{provider}-{nested_shell}-{interpreted_launcher}");
    let mut server = TestServer::start_with_detection_config(
        &session_name,
        DetectionConfig {
            working_poll_interval: Duration::from_millis(100),
            idle_poll_interval: Duration::from_millis(100),
            auto_answer_interstitial_prompts: false,
        },
    )
    .await;
    let transcript_path = transcript(&server, provider);
    let transcript_before = std::fs::read(&transcript_path).expect("transcript baseline");
    let prompt_path = server.home_dir.join("captured-prompt.txt");
    let fixture = install(
        &server.home_dir,
        provider,
        &FixtureBehavior::CrashAfterSubmittedPrompt {
            prompt_path: prompt_path.clone(),
            transcript_path: Some(transcript_path.clone()),
            exit_code: 42,
            mouse_tracking: true,
        },
    );
    #[cfg(unix)]
    let launcher = interpreted_launcher.then(|| {
        let directory = server.home_dir.join("interpreted-shim");
        std::fs::create_dir(&directory).expect("owned shim directory");
        let path = directory.join(provider);
        let child_exit = server.home_dir.join("native-child-exit.txt");
        let release = server.home_dir.join("release-launcher");
        // A real shell-interpreted script models script-argv shim ancestry
        // without requiring a JS runtime. Kernel name sh is not a provider.
        std::fs::write(&path, format!(
            "#!/bin/sh\n{}\ncode=$?\nprintf '%s' \"$code\" > {}\nwhile [ ! -e {} ]; do sleep 0.05; done\nexit 0\n",
            shell_quote(&fixture.path), shell_quote(&child_exit), shell_quote(&release)
        )).expect("owned interpreted launcher");
        (path, child_exit, release)
    });
    let command = fixture.path.to_string_lossy().into_owned();
    #[cfg(unix)]
    let command = launcher.as_ref().map_or(command, |(path, _, _)| {
        format!("/bin/sh {}", shell_quote(path))
    });
    assert!(fixture.path.is_absolute());
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: session_name.clone(),
        },
    )
    .await
    .expect("attach owner");
    read_initial_state(&mut client, WAIT).await;
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: if nested_shell {
                NewPaneKind::PlainShell
            } else {
                NewPaneKind::Command(command)
            },
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("launch absolute fake provider");
    let created = expect_event(&mut client, WAIT, |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = created else {
        unreachable!()
    };
    let project = tree.project_ids()[0];
    let group = tree.children_of(project).expect("project children")[0];
    let pane_id = tree.children_of(group).expect("group children")[0];
    if nested_shell {
        let quoted_path = fixture.path.to_string_lossy();
        write_frame(
            &mut client,
            &ClientRequest::UserKeyInput {
                pane_id,
                bytes: format!("\"{quoted_path}\"\r").into_bytes(),
                submission: None,
                prompt_epoch: None,
            },
        )
        .await
        .expect("launch fake provider in owned live shell");
    }
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut live_provider = false;
    let mut process_id = None;
    let mut seen = Vec::new();
    tokio::time::timeout(WAIT, async {
        while !live_provider || process_id.is_none() || !parser.screen().bracketed_paste() {
            let event: ServerEvent = read_frame(&mut client).await.expect("startup IPC event");
            seen.push(format!("{event:?}").chars().take(300).collect::<String>());
            match event {
                ServerEvent::ScreenUpdate {
                    pane_id: changed,
                    bytes,
                    ..
                } if changed == pane_id => parser.process(&bytes),
                ServerEvent::PaneDetectedStateChanged {
                    pane_id: changed,
                    status,
                    ..
                } if changed == pane_id => {
                    live_provider = status
                        .agent_state()
                        .is_some_and(|agent| agent.class == class);
                }
                ServerEvent::PaneSessionIdResolved {
                    pane_id: changed,
                    session_id,
                    process_id: owner,
                    ..
                } if changed == pane_id => {
                    assert_eq!(session_id, SESSION_ID);
                    process_id = owner;
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("provider/composer/session not ready: {seen:#?}"));
    let process_id = process_id.expect("verified transcript-owning provider process");
    #[cfg(unix)]
    let native_and_launcher_keys = launcher.as_ref().map(|(path, _, _)| {
        let mut system = sysinfo::System::new();
        ilium_detect::refresh(&mut system);
        let native = system
            .process(sysinfo::Pid::from_u32(process_id))
            .expect("verified native provider still alive before submission");
        assert_eq!(native.name().to_string_lossy(), provider);
        assert_eq!(
            native.cmd().first().map(std::path::Path::new),
            Some(fixture.path.as_path())
        );
        let parent_id = native
            .parent()
            .expect("native provider has actual shim parent");
        let parent = system
            .process(parent_id)
            .expect("actual interpreted shim parent");
        assert_eq!(parent.name().to_string_lossy(), "sh");
        assert!(parent
            .cmd()
            .iter()
            .any(|arg| std::path::Path::new(arg) == path));
        let children = ilium_detect::ProcessChildrenIndex::build(&system);
        let identity = ilium_detect::identify_agent_with_extra(&system, parent_id, &children, &[])
            .expect("native provider is detected through launcher ancestry");
        assert_eq!(
            identity.pid, process_id,
            "interpreted parent must not own the live native CLI"
        );
        assert_eq!(identity.process_tree_depth, 1);
        (
            ilium_core::AgentProcessKey {
                class: class.clone(),
                process_id,
                started_at_unix_seconds: native.start_time(),
            },
            parent_id,
        )
    });
    let queued_marker = server.home_dir.join("queued-must-not-cross-crash.txt");
    if nested_shell || interpreted_launcher {
        // A real semantic producer is queued while the provider is still live.
        // Error exit must neither masquerade as finished nor replay this head
        // into the surviving interactive shell.
        write_frame(
            &mut client,
            &ClientRequest::EnqueuePrompt {
                pane_id,
                text: format!("echo QUEUE_LEAK > \"{}\"", queued_marker.display()),
                delivery: ilium_core::PromptQueueDelivery::Once,
            },
        )
        .await
        .expect("queue a semantic prompt before provider loss");
        expect_event(&mut client, WAIT, |event| matches!(event,
            ServerEvent::TreeSnapshot(tree) if tree.get(pane_id).is_some_and(|node|
                matches!(&node.kind, NodeKind::Pane { prompt_queue, .. } if prompt_queue.len() == 1))
        )).await;
    }
    let prompt = format!(
        "{}\nUnicode café 日本語 🦀\nlast line  ",
        "authored work  ".repeat(600)
    );
    assert!(prompt.len() > 4096);
    let bytes = [b"\x1b[200~".as_slice(), prompt.as_bytes(), b"\x1b[201~"].concat();
    write_frame(
        &mut client,
        &ClientRequest::UserKeyInput {
            pane_id,
            bytes,
            submission: None,
            prompt_epoch: None,
        },
    )
    .await
    .expect("write authored bracketed paste");
    // Mirror the real client: pasting does not submit; the later human Enter
    // is a Keyboard submission rather than a semantic automated producer.
    write_frame(
        &mut client,
        &ClientRequest::UserKeyInput {
            pane_id,
            bytes: b"\r".to_vec(),
            submission: Some(PromptSubmissionSource::Keyboard),
            prompt_epoch: Some(format!("synthetic-crash-{provider}-enter-1")),
        },
    )
    .await
    .expect("submit the authored prompt with human Enter");
    let crash_output = std::cell::RefCell::new(vt100::Parser::new(24, 100, 0));
    expect_event(&mut client, WAIT, |event| {
        if let ServerEvent::ScreenUpdate {
            pane_id: changed,
            bytes,
            ..
        } = event
        {
            if *changed != pane_id {
                return false;
            }
            let mut parser = crash_output.borrow_mut();
            parser.process(bytes);
            parser.screen().contents().contains("FATAL_FIXTURE_CRASH")
        } else {
            false
        }
    })
    .await;
    assert_eq!(
        std::fs::read(&prompt_path).expect("fixture prompt receipt"),
        prompt.as_bytes()
    );
    assert_eq!(
        std::fs::read(&transcript_path).expect("unflushed transcript"),
        transcript_before
    );
    #[cfg(unix)]
    if let Some((_, child_exit, _)) = &launcher {
        assert!(
            wait_until(|| child_exit.is_file(), WAIT).await,
            "launcher did not observe actual native subprocess exit"
        );
        assert_eq!(
            std::fs::read_to_string(child_exit).expect("actual nested child exit receipt"),
            "42"
        );
        // This external fixture receipt is not a server-native wait result;
        // recovery may report Unknown, but must never credit the shim's exit0.
    }
    let mut system = sysinfo::System::new();
    assert!(
        wait_until(
            || {
                ilium_detect::refresh(&mut system);
                system.process(sysinfo::Pid::from_u32(process_id)).is_none()
            },
            WAIT
        )
        .await,
        "verified provider process {process_id} did not disappear"
    );
    let transition = expect_event(&mut client, WAIT, |event| {
        matches!(event,
            ServerEvent::PaneDetectedStateChanged { pane_id: changed, .. } if *changed == pane_id
        )
    })
    .await;
    if let ServerEvent::PaneDetectedStateChanged { status, .. } = transition {
        let agent = status
            .known_agent_state()
            .expect("exit detection must retain recovery identity");
        assert_eq!(agent.class, class);
        assert!(status.agent_state().is_none());
        assert!(status.agent_recovery().is_some());
        assert!(!agent.completion_unread);
    }
    let mut observer = server.connect().await;
    write_frame(
        &mut observer,
        &ClientRequest::Attach {
            session: session_name,
        },
    )
    .await
    .expect("fresh recovery observer");
    let (tree, events) = read_initial_state(&mut observer, WAIT).await;
    assert_recovery(&tree, pane_id, &class, &prompt);
    assert!(
        events.iter().any(|event| matches!(event,
            ServerEvent::PaneSessionIdResolved { pane_id: changed, session_id, .. }
                if *changed == pane_id && session_id == SESSION_ID
        )),
        "fresh observer lost retained session identity: {events:#?}"
    );
    #[cfg(unix)]
    if let Some((native_key, _)) = &native_and_launcher_keys {
        let NodeKind::Pane {
            status,
            prompt_queue,
            ..
        } = &tree.get(pane_id).expect("retained native pane").kind
        else {
            unreachable!()
        };
        assert_eq!(
            &status
                .agent_recovery()
                .expect("original native recovery")
                .process,
            native_key
        );
        assert_eq!(
            prompt_queue.len(),
            1,
            "launcher loss dispatched or discarded queued work"
        );
        assert!(!prompt_queue[0].attempted_delivery);
        write_frame(
            &mut client,
            &ClientRequest::KeyInput {
                pane_id,
                bytes: b"AUTOMATION_MUST_NOT_REACH_SHIM\r".to_vec(),
                submission: Some(PromptSubmissionSource::QueuedPrompt),
            },
        )
        .await
        .expect("request stale automatic input at surviving launcher");
        expect_event(&mut client, WAIT, |event| {
            matches!(event,
                ServerEvent::Error { message } if message.contains("automatic input refused")
            )
        })
        .await;
    }
    if nested_shell {
        let marker = server.home_dir.join("automation-must-not-reach-shell.txt");
        let marker_path = marker.to_string_lossy();
        write_frame(
            &mut client,
            &ClientRequest::KeyInput {
                pane_id,
                bytes: format!("echo leaked > \"{marker_path}\"\r").into_bytes(),
                submission: None,
            },
        )
        .await
        .expect("attempt stale agent automation");
        expect_event(&mut client, WAIT, |event| {
            matches!(event, ServerEvent::Error { message }
                if message.contains("automatic input refused"))
        })
        .await;
        // A direct human shell command is still permitted, and provides an
        // ordering barrier before checking the rejected automated side effect.
        write_frame(
            &mut client,
            &ClientRequest::UserKeyInput {
                pane_id,
                bytes: b"echo MANUAL_SHELL_STILL_AVAILABLE\r".to_vec(),
                submission: Some(PromptSubmissionSource::Keyboard),
                prompt_epoch: None,
            },
        )
        .await
        .expect("manual shell remains available after agent crash");
        let shell_output = std::cell::RefCell::new(vt100::Parser::new(24, 100, 0));
        expect_event(&mut client, WAIT, |event| {
            let ServerEvent::ScreenUpdate {
                pane_id: changed,
                bytes,
                ..
            } = event
            else {
                return false;
            };
            if *changed != pane_id {
                return false;
            }
            let mut parser = shell_output.borrow_mut();
            parser.process(bytes);
            parser
                .screen()
                .contents()
                .lines()
                .any(|line| line.trim() == "MANUAL_SHELL_STILL_AVAILABLE")
        })
        .await;
        assert!(!marker.exists(), "stale automation reached the shell");
        assert!(
            !queued_marker.exists(),
            "queued semantic input crossed crash into shell"
        );
        write_frame(
            &mut observer,
            &ClientRequest::Attach {
                session: format!("crash-{provider}-{nested_shell}-{interpreted_launcher}"),
            },
        )
        .await
        .expect("read historical prompt after manual shell command");
        let (tree, _) = read_initial_state(&mut observer, WAIT).await;
        assert_recovery(&tree, pane_id, &class, &prompt);
        let NodeKind::Pane { prompt_queue, .. } = &tree.get(pane_id).expect("retained pane").kind
        else {
            unreachable!("recovery assertion already checked pane kind");
        };
        assert_eq!(
            prompt_queue.len(),
            1,
            "pending semantic prompt was discarded or dispatched"
        );
        assert!(
            !prompt_queue[0].attempted_delivery,
            "crash falsely attempted queued delivery"
        );
    }
    #[cfg(unix)]
    if let Some((_, _, release)) = &launcher {
        let (native_key, launcher_process_id) = native_and_launcher_keys
            .as_ref()
            .expect("verified owner keys");
        std::fs::write(release, b"owned shim may exit zero now")
            .expect("release only owned launcher");
        let mut system = sysinfo::System::new();
        assert!(
            wait_until(
                || {
                    ilium_detect::refresh(&mut system);
                    system.process(*launcher_process_id).is_none()
                },
                WAIT
            )
            .await,
            "owned interpreted launcher did not exit after release"
        );
        // The original recovery can remain Unknown with no semantic change
        // after shim exit. A direct request's error and subsequent Attach are
        // ordered IPC barriers; do not require an invented status transition.
        write_frame(
            &mut client,
            &ClientRequest::KeyInput {
                pane_id,
                bytes: b"AUTOMATION_MUST_NOT_REPLAY_AFTER_SHIM_EXIT\r".to_vec(),
                submission: Some(PromptSubmissionSource::QueuedPrompt),
            },
        )
        .await
        .expect("request fenced automation after shim exits zero");
        expect_event(&mut client, WAIT, |event| {
            matches!(event,
                ServerEvent::Error { message } if message.contains("automatic input refused")
            )
        })
        .await;
        write_frame(
            &mut client,
            &ClientRequest::Attach {
                session: format!("crash-{provider}-{nested_shell}-{interpreted_launcher}"),
            },
        )
        .await
        .expect("same-connection snapshot barrier after shim exits zero");
        let (tree, events) = read_initial_state(&mut client, WAIT).await;
        assert_recovery(&tree, pane_id, &class, &prompt);
        let NodeKind::Pane {
            status,
            prompt_queue,
            ..
        } = &tree
            .get(pane_id)
            .expect("native recovery after shim exit")
            .kind
        else {
            unreachable!()
        };
        let recovery = status
            .agent_recovery()
            .expect("native recovery after shim exit");
        assert_eq!(
            &recovery.process, native_key,
            "shim exit replaced original native key"
        );
        assert_ne!(
            recovery.availability,
            ilium_core::AgentAvailability::Exited(ilium_core::AgentExitOutcome::ExitCode(0)),
            "shim exit0 was falsely credited as the native provider's exit cause"
        );
        assert_eq!(prompt_queue.len(), 1);
        assert!(!prompt_queue[0].attempted_delivery);
        assert!(!queued_marker.exists());
        assert!(events.iter().any(|event| matches!(event,
            ServerEvent::PaneSessionIdResolved { pane_id: changed, session_id, process_id: owner, .. }
                if *changed == pane_id && session_id == SESSION_ID && *owner == Some(native_key.process_id)
        )), "snapshot lost original native session ownership after shim exit");
    }
    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("stop owned test session");
    tokio::time::timeout(WAIT, &mut server.server_task)
        .await
        .expect("owned server shutdown timeout")
        .expect("owned server task join")
        .expect("owned server shutdown result");
}

#[tokio::test]
async fn crashed_codex_retains_identity_session_and_exact_unflushed_prompt() {
    recovery_after_unflushed_crash("codex", AgentClass::Codex, false, false).await;
}

#[tokio::test]
async fn crashed_claude_retains_identity_session_and_exact_unflushed_prompt() {
    recovery_after_unflushed_crash("claude", AgentClass::Claude, false, false).await;
}

#[tokio::test]
async fn nested_codex_crash_preserves_prompt_and_fences_automation_from_shell() {
    recovery_after_unflushed_crash("codex", AgentClass::Codex, true, false).await;
}

#[tokio::test]
async fn nested_claude_crash_preserves_prompt_and_fences_automation_from_shell() {
    recovery_after_unflushed_crash("claude", AgentClass::Claude, true, false).await;
}

#[cfg(unix)]
struct ReleaseOwnedStall {
    provider_marker: PathBuf,
    drain_marker: PathBuf,
}

#[cfg(unix)]
impl Drop for ReleaseOwnedStall {
    fn drop(&mut self) {
        // Panic cleanup must not strand the marker-driven provider/reader.
        // These exact marker paths were created solely for this test.
        let _ = std::fs::write(&self.provider_marker, b"cleanup");
        let _ = std::fs::write(&self.drain_marker, b"cleanup");
    }
}

/// This fixture is an existing absolute fake provider that runs an owned
/// nonreading child. Nothing in this test invokes a real provider or alters PATH.
#[cfg(unix)]
fn shell_quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(unix)]
async fn add_owned_command(
    client: &mut ilium_transport::SessionStream,
    command: String,
    existing: &[NodeId],
) -> NodeId {
    write_frame(
        client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(command),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("create owned command pane");
    let event = expect_event(client, WAIT, |event| matches!(event,
        ServerEvent::TreeSnapshot(tree) if tree.project_ids().iter().any(|project|
            tree.children_of(*project).is_ok_and(|groups| groups.iter().any(|group|
                tree.children_of(*group).is_ok_and(|panes| panes.iter().any(|pane| !existing.contains(pane))))))
    )).await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!()
    };
    for project in tree.project_ids() {
        for group in tree.children_of(project).expect("project children") {
            for pane in tree.children_of(*group).expect("group children") {
                if !existing.contains(pane) {
                    return *pane;
                }
            }
        }
    }
    unreachable!("predicate proved a new pane exists")
}

#[cfg(unix)]
async fn await_output(
    client: &mut ilium_transport::SessionStream,
    pane_id: NodeId,
    needle: &str,
    timeout: Duration,
) {
    let parser = std::cell::RefCell::new(vt100::Parser::new(24, 120, 0));
    expect_event(client, timeout, |event| {
        let ServerEvent::ScreenUpdate {
            pane_id: changed,
            bytes,
            ..
        } = event
        else {
            return false;
        };
        if *changed != pane_id {
            return false;
        }
        let mut parser = parser.borrow_mut();
        parser.process(bytes);
        parser.screen().contents().contains(needle)
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn in_flight_automated_receipt_cancels_without_enter_or_replay_after_provider_loss() {
    let session_name = "crash-stalled-receipt";
    let mut server = TestServer::start_with_detection_config(
        session_name,
        DetectionConfig {
            working_poll_interval: Duration::from_millis(100),
            idle_poll_interval: Duration::from_millis(100),
            auto_answer_interstitial_prompts: false,
        },
    )
    .await;
    let release = server.home_dir.join("release-provider");
    let drain = server.home_dir.join("release-shell-drain");
    let captured = server.home_dir.join("shell-received.bin");
    let drain_complete = server.home_dir.join("shell-drain-complete");
    let _release_on_failure = ReleaseOwnedStall {
        provider_marker: release.clone(),
        drain_marker: drain.clone(),
    };
    let first_byte = server.home_dir.join("admitted-first-byte.bin");
    let replacement = server.home_dir.join("nonreading.sh");
    std::fs::write(&replacement, format!(
        "#!/bin/sh\nstty raw -echo\nprintf '\\033[?2004hSTALLED_PROVIDER_READY\\r\\n› Send a message\\r\\n'\n/bin/dd bs=1 count=1 of={} 2>/dev/null\nwhile [ ! -e {} ]; do sleep 0.05; done\nexit 42\n",
        shell_quote(&first_byte), shell_quote(&release)
    )).expect("write owned nonreading provider script");
    ilium_platform::secure_fs::restrict_executable_file_to_owner(&replacement)
        .expect("make owned provider script executable");
    let fake = install(
        &server.home_dir,
        "codex",
        &FixtureBehavior::ShellImpersonator {
            intercepted_command: "stalled-provider".to_string(),
            replacement,
        },
    );
    let healthy = install(
        &server.home_dir,
        "healthy-echo",
        &FixtureBehavior::EchoSubmittedLine {
            prefix: "SECOND_PANE_RESPONSIVE".to_string(),
        },
    );
    let mut stalled_client = server.connect().await;
    write_frame(
        &mut stalled_client,
        &ClientRequest::Attach {
            session: session_name.to_string(),
        },
    )
    .await
    .expect("attach stalled-request connection");
    read_initial_state(&mut stalled_client, WAIT).await;
    // The outer owned shell stays alive. Its new reader is deliberately held
    // until the detector has cancelled the old provider's pending receipt.
    // VMIN=0/VTIME=1 gives the reader a concrete EOF after draining the native
    // input buffer. Its completion marker proves capture has finished.
    let outer_shell = server.home_dir.join("owned-return-to-shell.sh");
    std::fs::write(&outer_shell, format!(
        "#!/bin/sh\n{} -c stalled-provider; printf 'RETURNED_TO_SHELL\\r\\n'; while [ ! -e {} ]; do sleep 0.05; done; stty raw -echo min 0 time 1; cat > {}; printf drained > {}; while :; do sleep 1; done\n",
        shell_quote(&fake.path), shell_quote(&drain), shell_quote(&captured), shell_quote(&drain_complete)
    )).expect("write owned return-to-shell script");
    // Keep provider words out of the surviving shell's argv. A `sh -c codex`
    // wrapper is itself intentionally identifiable as Codex by the detector,
    // and would never prove genuine provider disappearance in this scenario.
    let stalled = add_owned_command(
        &mut stalled_client,
        format!("/bin/sh {}", shell_quote(&outer_shell)),
        &[],
    )
    .await;
    let mut ready_screen = vt100::Parser::new(24, 120, 0);
    let mut detected_live = false;
    tokio::time::timeout(WAIT, async {
        while !detected_live
            || !ready_screen
                .screen()
                .contents()
                .contains("STALLED_PROVIDER_READY")
        {
            let event: ServerEvent = read_frame(&mut stalled_client)
                .await
                .expect("stalled provider startup event");
            match event {
                ServerEvent::ScreenUpdate { pane_id, bytes, .. } if pane_id == stalled => {
                    ready_screen.process(&bytes)
                }
                ServerEvent::PaneDetectedStateChanged {
                    pane_id, status, ..
                } if pane_id == stalled => {
                    detected_live = status
                        .agent_state()
                        .is_some_and(|agent| agent.class == AgentClass::Codex);
                }
                _ => {}
            }
        }
    })
    .await
    .expect("owned nonreading provider did not become ready and detected");
    let healthy_pane =
        add_owned_command(&mut stalled_client, shell_quote(&healthy.path), &[stalled]).await;
    let mut responsive_client = server.connect().await;
    write_frame(
        &mut responsive_client,
        &ClientRequest::Attach {
            session: session_name.to_string(),
        },
    )
    .await
    .expect("attach responsive second connection");
    read_initial_state(&mut responsive_client, WAIT).await;
    const BODY_BYTES: usize = 512 * 1024;
    let mut body = vec![b'x'; BODY_BYTES];
    body.push(b'\r');
    write_frame(
        &mut stalled_client,
        &ClientRequest::KeyInput {
            pane_id: stalled,
            bytes: body,
            submission: Some(PromptSubmissionSource::QueuedPrompt),
        },
    )
    .await
    .expect("admit large automated body and Enter");
    // The owned provider reads exactly one byte, then stops reading. Seeing
    // that byte proves admission before we permit provider loss; a sleep
    // cannot order the fresh-process preflight against the release marker.
    // The remaining half-MiB body still cannot fit the stalled PTY buffer.
    assert!(
        wait_until(
            || std::fs::read(&first_byte).is_ok_and(|bytes| bytes == b"x"),
            WAIT
        )
        .await,
        "owned provider did not observe the first admitted automatic byte"
    );
    write_frame(
        &mut responsive_client,
        &ClientRequest::UserKeyInput {
            pane_id: healthy_pane,
            bytes: b"ok\r".to_vec(),
            submission: Some(PromptSubmissionSource::Keyboard),
            prompt_epoch: None,
        },
    )
    .await
    .expect("send independent healthy input");
    await_output(
        &mut responsive_client,
        healthy_pane,
        "SECOND_PANE_RESPONSIVE:<ok>",
        Duration::from_secs(1),
    )
    .await;
    std::fs::write(&release, b"provider exits now").expect("release owned stalled provider");
    // An actual direct error settles the admitted receipt; a timer alone
    // cannot establish that the blocked request was cancelled.
    tokio::time::timeout(WAIT, async {
        loop {
            let event: ServerEvent = read_frame(&mut stalled_client).await.expect("receipt settlement event");
            assert!(!matches!(event, ServerEvent::PanePromptSubmitted {
                pane_id, source: PromptSubmissionSource::QueuedPrompt,
            } if pane_id == stalled), "uncertain receipt fabricated a completed semantic submission");
            if matches!(event, ServerEvent::Error { ref message } if message.contains("automatic input cancelled")) {
                break;
            }
        }
    }).await.expect("admitted automated receipt did not settle on provider loss");
    const HUMAN_BARRIER: &[u8] = b"HUMAN_ORDERING_BARRIER\r";
    write_frame(
        &mut responsive_client,
        &ClientRequest::UserKeyInput {
            pane_id: stalled,
            bytes: HUMAN_BARRIER.to_vec(),
            submission: None,
            prompt_epoch: None,
        },
    )
    .await
    .expect("request manual input after cancelled partial delivery");
    // Partial delivery quarantines this PTY writer. The manual request must
    // settle explicitly, releasing the server's input gate without replaying
    // uncertain bytes or pretending the quarantined stream remains writable.
    let unavailable_seen = std::cell::Cell::new(false);
    expect_event(&mut responsive_client, WAIT, |event| {
        if matches!(event, ServerEvent::PaneDetectedStateChanged { pane_id, status, .. }
            if *pane_id == stalled && status.agent_recovery().is_some() && status.agent_state().is_none())
        {
            unavailable_seen.set(true);
        }
        // The queued manual receipt can settle as Cancelled before the
        // writer publishes WriterFailed. Both reject this exact pane's input;
        // the drained-byte assertions below still prove no manual replay.
        matches!(event, ServerEvent::Error { message }
            if message.contains(&format!("pane {stalled:?}:"))
                && (message.contains("failed to admit input") || message.contains("failed to deliver input"))
                && (message.contains("WriterFailed") || message.contains("OwnerLost")
                    || (message.contains("PTY operation") && message.ends_with(": Cancelled"))))
    })
    .await;
    // Cancellation revokes input before the next process-table refresh has
    // necessarily published disappearance. Await that authoritative event;
    // a direct cancellation error alone is not a recovery snapshot barrier.
    if !unavailable_seen.get() {
        expect_event(&mut responsive_client, WAIT, |event| {
            matches!(event, ServerEvent::PaneDetectedStateChanged { pane_id, status, .. }
                if *pane_id == stalled && status.agent_recovery().is_some() && status.agent_state().is_none())
        })
        .await;
    }
    std::fs::write(&drain, b"shell may read now").expect("release only owned shell reader");
    assert!(
        wait_until(|| drain_complete.is_file(), WAIT).await,
        "owned shell did not finish draining the quarantined PTY input buffer"
    );
    let prefix = std::fs::read(&captured).expect("completed owned shell receipt");
    assert!(
        !prefix.is_empty() && prefix.len() < BODY_BYTES,
        "expected a nonempty partial automated prefix, received {} bytes",
        prefix.len()
    );
    assert!(
        prefix.iter().all(|byte| *byte == b'x'),
        "orphan automated Enter, manual bytes or unexpected replay reached shell: {} bytes",
        prefix.len()
    );
    // The next write on the same formerly-blocked connection is an ordering
    // barrier against its handler, and proves the connection settled too.
    write_frame(
        &mut stalled_client,
        &ClientRequest::Attach {
            session: session_name.to_string(),
        },
    )
    .await
    .expect("reuse formerly blocked client");
    let (tree, events) = read_initial_state(&mut stalled_client, WAIT).await;
    let NodeKind::Pane { status, .. } = &tree.get(stalled).expect("retained stalled pane").kind
    else {
        unreachable!()
    };
    assert!(
        status.agent_recovery().is_some() && status.agent_state().is_none(),
        "partial-write quarantine lost unavailable recovery: {status:?}"
    );
    assert!(!events.iter().any(|event| matches!(event,
        ServerEvent::PanePromptSubmitted { pane_id, source: PromptSubmissionSource::QueuedPrompt, .. } if *pane_id == stalled
    )), "cancelled uncertain input was reported as a completed semantic submission");
    write_frame(&mut responsive_client, &ClientRequest::KillSession)
        .await
        .expect("clean owned server");
    tokio::time::timeout(WAIT, &mut server.server_task)
        .await
        .expect("owned shutdown timeout")
        .expect("owned task join")
        .expect("owned server result");
}

#[cfg(unix)]
#[tokio::test]
async fn codex_interpreted_launcher_loss_retains_native_recovery() {
    recovery_after_unflushed_crash("codex", AgentClass::Codex, false, true).await;
}

#[cfg(unix)]
#[tokio::test]
async fn claude_interpreted_launcher_loss_retains_native_recovery() {
    recovery_after_unflushed_crash("claude", AgentClass::Claude, false, true).await;
}
