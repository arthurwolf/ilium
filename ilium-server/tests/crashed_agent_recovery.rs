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
            project_pane_signals(status, None, false, None).now,
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

async fn recovery_after_unflushed_crash(provider: &str, class: AgentClass, nested_shell: bool) {
    let session_name = format!("crash-{provider}-{nested_shell}");
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
                NewPaneKind::Command(fixture.path.to_string_lossy().into_owned())
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
        write_frame(
            &mut observer,
            &ClientRequest::Attach {
                session: format!("crash-{provider}-{nested_shell}"),
            },
        )
        .await
        .expect("read historical prompt after manual shell command");
        let (tree, _) = read_initial_state(&mut observer, WAIT).await;
        assert_recovery(&tree, pane_id, &class, &prompt);
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
    recovery_after_unflushed_crash("codex", AgentClass::Codex, false).await;
}

#[tokio::test]
async fn crashed_claude_retains_identity_session_and_exact_unflushed_prompt() {
    recovery_after_unflushed_crash("claude", AgentClass::Claude, false).await;
}

#[tokio::test]
async fn nested_codex_crash_preserves_prompt_and_fences_automation_from_shell() {
    recovery_after_unflushed_crash("codex", AgentClass::Codex, true).await;
}

#[tokio::test]
async fn nested_claude_crash_preserves_prompt_and_fences_automation_from_shell() {
    recovery_after_unflushed_crash("claude", AgentClass::Claude, true).await;
}
