//! End-to-end test of the *whole* live agent-detection pipeline: process
//! spawn -> `sysinfo` sees it -> `ilium_detect::identify_agent` matches
//! `"claude"` by real process name -> screen text scraped from a real,
//! live `vt100` parser feed (`ilium-pty`) -> `ilium_detect::classify_activity`
//! -> a `PaneStatus` change reaches a connected IPC client. This is
//! meaningfully different from `ilium-detect`'s own fixture-based unit
//! tests (which only exercise the two pure classification functions in
//! isolation, never the loop/process/IPC plumbing around them) and from
//! `smoke.rs` (which never spawns anything that looks like an agent CLI).
//!
//! Per `CLAUDE.md`: "No test should depend on a real `claude` or `codex`
//! binary being installed -- detection tests run against captured
//! fixture text, never by shelling out to the real CLI." This test
//! doesn't violate that: it never invokes a real `claude`/`codex`
//! binary. Instead, it writes a tiny fake shell script literally named
//! `claude` into a tempdir and spawns it by its *absolute* path, so
//! `ilium_detect::identify_agent`'s real, unmodified process-name
//! matching (`AGENT_SIGNATURES`, checked against `sysinfo`'s real process
//! list -- the kernel names a process after the file it was exec'd from,
//! not the path used to reach it, so an absolute-path-invoked script
//! named `claude` is still reported as `claude`) genuinely finds it,
//! exactly as it would find a real Claude Code process, without this
//! workspace's tests ever shelling out to Anthropic's actual CLI or
//! requiring it to be installed on the machine running this suite.
//!
//! Deliberately an absolute path, not a bare `claude` added to `PATH`:
//! this test's own development turned up that a login shell's rc files
//! (`~/.zshenv`, etc. -- `TerminalOrigin::Command` runs `$SHELL -c
//! "<command_line>"`, and most interactive shells still source at least
//! one rc file even in `-c` mode) can silently re-order or rewrite
//! `PATH`, so a `PATH`-prepended fake binary can lose a race against a
//! *real* `claude` install already on the developer's machine -- exactly
//! the failure mode this whole test exists to never risk. An absolute
//! path sidesteps `PATH`/rc-file resolution entirely and is
//! unconditionally deterministic.
//!
//! The fake script produces the two observable screen-text signals the
//! detector owns: the persistent `Goal:` row plus activity state from
//! (`ilium-detect/src/lib.rs`'s `WORKING_MARKER`, the literal substring
//! `"esc to interrupt"`) for a few seconds, then clearing the screen and
//! printing something else -- simulating a turn finishing. Everything
//! downstream of that (the real `sysinfo` process-tree walk, the real
//! `vt100` screen-text scrape, the real classification, the real
//! `PaneStatusChanged` broadcast) is exactly the production code path,
//! nothing faked except the one external binary name and its output.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ilium_core::{PaneStatus, ROOT_ID};
use ilium_ipc::{read_frame, write_frame, ClientRequest, NewPaneKind, ServerEvent};
use ilium_server::config::DetectionConfig;
use ilium_server::SoundPlayer;

mod common;
use common::{expect_event, TestServer};

/// How long the fake `claude` script prints the `"esc to interrupt"`
/// marker before switching to its idle phase. Long enough to comfortably
/// span at least one real detection-loop tick (`ilium-server::detection`'s
/// `BASE_TICK_INTERVAL` is a fixed 1s -- not configurable via
/// `DetectionConfig`, which only controls how *often a due pane is
/// rechecked*, not the loop's own wake cadence) even under test-runner
/// load.
const WORKING_PHASE_SECONDS: u32 = 4;

/// Generous relative to the fixed ~1s tick granularity above -- covers a
/// slow CI runner without ever being so long a genuine regression would
/// make this test slow to fail.
const WAIT_TIMEOUT: Duration = Duration::from_secs(15);

struct RecordingSoundPlayer {
    calls: Arc<Mutex<Vec<ilium_sound::SoundSettings>>>,
}

impl SoundPlayer for RecordingSoundPlayer {
    fn play(&self, settings: &ilium_sound::SoundSettings) -> Result<(), ilium_sound::SoundError> {
        self.calls.lock().unwrap().push(settings.clone());
        Ok(())
    }
}

/// Writes an executable POSIX shell script named exactly `claude` into
/// `bin_dir` and returns its absolute path -- never the real system
/// `PATH`, never a real Anthropic binary (see this file's module docs on
/// why this is spawned by absolute path rather than added to `PATH`).
///
/// The script prints a line containing the literal
/// `ilium_detect::classify_activity` "working" marker
/// (`"esc to interrupt"`) once a second for [`WORKING_PHASE_SECONDS`],
/// then clears the screen (`\x1b[2J\x1b[H`) and prints something else --
/// the screen must actually stop *containing* the marker for a real
/// `Idle` reclassification, not merely stop *adding* new instances of
/// it, since `vt100::Screen::contents()` reflects the current visible
/// screen, not an ever-growing scrollback.
fn write_fake_claude_binary(bin_dir: &std::path::Path) -> std::path::PathBuf {
    let script_path = bin_dir.join("claude");
    let script = format!(
        "#!/bin/sh\n\
         i=0\n\
         while [ \"$i\" -lt {WORKING_PHASE_SECONDS} ]; do\n\
         \x20\x20printf 'Goal: prove goal detection end to end\\n'\n\
         \x20\x20printf 'Cogitating (esc to interrupt)\\n'\n\
         \x20\x20i=$((i + 1))\n\
         \x20\x20sleep 1\n\
         done\n\
         printf '\\033[2J\\033[H'\n\
         printf 'Goal: prove goal detection end to end\\n'\n\
         printf 'Done. Ready for the next instruction.\\n'\n\
         sleep 60\n"
    );
    let mut file = std::fs::File::create(&script_path).expect("create fake claude script");
    file.write_all(script.as_bytes())
        .expect("write fake claude script");
    // Executable for the owner is enough -- this script is only ever run
    // by this same test process's own spawned children.
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .expect("chmod fake claude script executable");
    script_path
}

#[tokio::test]
async fn a_real_process_named_claude_preserves_a_visible_goal_through_the_whole_pipeline() {
    let fake_bin_dir = tempfile::tempdir().expect("create tempdir for the fake claude binary");
    let fake_claude_path = write_fake_claude_binary(fake_bin_dir.path());

    // Short poll intervals so a real `Working -> Idle` transition shows up
    // within this test's timeout instead of the real default's 5s/45s
    // cadence -- the detection loop's own wake cadence is still the fixed
    // ~1s `BASE_TICK_INTERVAL` either way (see `WORKING_PHASE_SECONDS`'s
    // doc comment), so this doesn't make the loop busy-poll, just makes a
    // due pane eligible for recheck almost every tick.
    let detection_config = DetectionConfig {
        working_poll_interval: Duration::from_millis(200),
        idle_poll_interval: Duration::from_millis(200),
    };
    let sound_calls = Arc::new(Mutex::new(Vec::new()));
    let mut initially_disabled_sound = ilium_sound::SoundSettings::default();
    initially_disabled_sound.events.agent_finished = false;
    let mut server = TestServer::start_with_sound_player(
        "live-agent-detection-test",
        detection_config,
        initially_disabled_sound,
        Arc::new(RecordingSoundPlayer {
            calls: Arc::clone(&sound_calls),
        }),
    )
    .await;
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "live-agent-detection-test".to_string(),
        },
    )
    .await
    .expect("write Attach request");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    write_frame(
        &mut client,
        &ClientRequest::UpdateSoundSettings {
            settings: ilium_sound::SoundSettings::default(),
        },
    )
    .await
    .expect("enable finished sounds through the live IPC settings path");

    // `TerminalOrigin::Command` runs `$SHELL -c "<command_line>"` (see
    // `ilium-server::pane::spawn_terminal_session`); passing the fake
    // script's absolute path as the whole command line means the real
    // process that ends up running is *exactly* this fake script --
    // named, and therefore reported by `sysinfo`/`identify_agent`,
    // exactly `claude` (see this file's module docs on why an absolute
    // path is used here instead of relying on `PATH`).
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(fake_claude_path.to_string_lossy().to_string()),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("write NewPane request");
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches TreeSnapshot");
    };
    let default_group = tree
        .children_of(ROOT_ID)
        .expect("root is a group")
        .first()
        .copied()
        .expect("a default group should have been created for the pane");
    let pane_id = tree.children_of(default_group).expect("group has children")[0];

    // Structural assertion #1: the real detection loop, walking the real
    // `sysinfo` process tree, must identify the spawned process as a
    // Claude Code agent (by real process name, via
    // `ilium_detect::AGENT_SIGNATURES`) and classify it `Working` (by
    // real `vt100` screen-text scrape, via `ilium_detect::classify_activity`
    // matching the literal `"esc to interrupt"` marker this fake script
    // prints), and preserve its visible `Goal:` row as `AgentWithGoal` --
    // broadcast as a real `PaneStatusChanged` event to this
    // real connected IPC client.
    let working_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneStatusChanged { pane_id: changed_id, status }
                if *changed_id == pane_id
                    && matches!(
                        status,
                        PaneStatus::AgentWithGoal(_, ilium_core::AgentActivity::Working)
                    )
        )
    })
    .await;
    let ServerEvent::PaneStatusChanged { status, .. } = &working_event else {
        unreachable!("predicate only matches PaneStatusChanged");
    };
    assert!(
        matches!(
            status,
            PaneStatus::AgentWithGoal(ilium_core::AgentClass::Claude, _)
        ),
        "expected the real process tree walk to identify this pane as Claude, got {status:?}"
    );
    assert!(
        sound_calls.lock().unwrap().is_empty(),
        "the first Working classification is not a completed turn and must stay silent"
    );

    // Structural assertion #2: once the script clears the screen and
    // stops printing the marker, the real, unmodified pipeline must
    // reclassify the same pane `Done`, not plain `Idle` -- this client
    // never sent `SetPaneFocus` for this pane, so
    // `ilium-server::detection::promote_to_done` must turn the raw
    // "just went idle" verdict `classify_activity` reports into the
    // stateful "finished, unseen" one the tree/UI actually renders as a
    // bell. Regression coverage for the bug where this pane sat at plain
    // `Idle` forever because nothing between `classify_activity` and the
    // tree ever remembered the pane had just been `Working`.
    let done_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneStatusChanged { pane_id: changed_id, status }
                if *changed_id == pane_id
                    && matches!(
                        status,
                        PaneStatus::AgentWithGoal(
                            ilium_core::AgentClass::Claude,
                            ilium_core::AgentActivity::Idle | ilium_core::AgentActivity::Done
                        )
                    )
        )
    })
    .await;
    let ServerEvent::PaneStatusChanged { status, .. } = done_event else {
        unreachable!("predicate only matches PaneStatusChanged");
    };
    assert_eq!(
        status,
        PaneStatus::AgentWithGoal(
            ilium_core::AgentClass::Claude,
            ilium_core::AgentActivity::Done
        ),
        "expected a real Working -> Done transition while preserving its goal, got {status:?}"
    );

    let sound_dispatched = common::wait_until(
        || sound_calls.lock().unwrap().len() == 1,
        Duration::from_secs(5),
    )
    .await;
    assert!(
        sound_dispatched,
        "expected exactly one server-owned sound after the real Working -> Done transition"
    );
    assert_eq!(
        sound_calls.lock().unwrap()[0].source,
        ilium_sound::SoundSourceKind::SystemBeep
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

/// Writes an executable POSIX shell script named exactly `name` (same
/// absolute-path-spawn rationale as [`write_fake_claude_binary`]) that just
/// idles -- the argument-based session-ID-discovery tests below only care about
/// `crate::session_id::discover`'s first admissible source (an explicit resume argument on
/// the command line), not activity classification, so unlike
/// [`write_fake_claude_binary`] neither ever needs to print the "esc to
/// interrupt" working marker.
fn write_idle_fake_agent_binary(bin_dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let script_path = bin_dir.join(name);
    let script = "#!/bin/sh\nsleep 60\n";
    let mut file = std::fs::File::create(&script_path).expect("create fake agent script");
    file.write_all(script.as_bytes())
        .expect("write fake agent script");
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .expect("chmod fake agent script executable");
    script_path
}

/// Writes a resumed-agent fixture whose exact process both exposes a resume
/// ID in argv and keeps that same transcript open. This recreates the real
/// transition window where `/resume` has invalidated argv but the old file
/// descriptor has not been replaced yet.
fn write_resumed_transcript_holding_fake_agent_binary(
    bin_dir: &std::path::Path,
    name: &str,
) -> std::path::PathBuf {
    let script_path = bin_dir.join(name);
    let script = "#!/bin/sh\nexec 3<\"$3\"\nsleep 60\n";
    let mut file = std::fs::File::create(&script_path).expect("create fake resumed agent script");
    file.write_all(script.as_bytes())
        .expect("write fake resumed agent script");
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .expect("chmod fake resumed agent script executable");
    script_path
}

/// Writes a fake agent that keeps one caller-supplied transcript open while
/// it idles. The descriptor is inherited by `sleep`, while the script process
/// itself also retains it as the exact PID found by agent detection.
fn write_transcript_holding_fake_agent_binary(
    bin_dir: &std::path::Path,
    name: &str,
) -> std::path::PathBuf {
    let script_path = bin_dir.join(name);
    let script = "#!/bin/sh\nexec 3<\"$1\"\nsleep 60\n";
    let mut file = std::fs::File::create(&script_path).expect("create fake agent script");
    file.write_all(script.as_bytes())
        .expect("write fake agent script");
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .expect("chmod fake agent script executable");
    script_path
}

fn write_verified_claude_transcript(server: &TestServer, session_id: &str) -> std::path::PathBuf {
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
    let directory = server.home_dir.join(".claude/projects").join(slug);
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{session_id}.jsonl"));
    std::fs::write(
        &path,
        serde_json::json!({
            "type": "user",
            "sessionId": session_id,
            "cwd": server.project_cwd,
            "message": {"content": "integration test prompt"}
        })
        .to_string(),
    )
    .unwrap();
    path
}

fn write_verified_codex_transcript(server: &TestServer, session_id: &str) -> std::path::PathBuf {
    let directory = server.home_dir.join(".codex/sessions/2026/07/14");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("rollout-2026-07-14T12-00-00-{session_id}.jsonl"));
    std::fs::write(
        &path,
        serde_json::json!({
            "type": "session_meta",
            "payload": {"id": session_id, "cwd": server.project_cwd}
        })
        .to_string(),
    )
    .unwrap();
    path
}

/// End-to-end test of `ilium-server::session_id`'s live wiring into the
/// detection loop: a real spawned process (named `claude`, exactly like
/// [`a_real_process_named_claude_drives_working_to_idle_through_the_whole_pipeline`])
/// invoked with a `--resume <uuid>` argument and holding that transcript open
/// must broadcast that exact uuid as a real `ServerEvent::PaneSessionIdResolved`
/// to a real connected IPC client. It then proves the old descriptor is
/// quarantined after `/resume`, covering both admissible process-bound sources
/// and the in-process transition lifecycle end to end.
#[tokio::test]
async fn a_resumed_claude_processs_session_id_is_discovered_and_broadcast() {
    let fake_bin_dir = tempfile::tempdir().expect("create tempdir for the fake claude binary");
    let fake_claude_path =
        write_resumed_transcript_holding_fake_agent_binary(fake_bin_dir.path(), "claude");
    let resumed_session_id = "95fd0645-3331-408b-a7e5-36e6007bfb78";

    let detection_config = DetectionConfig {
        working_poll_interval: Duration::from_millis(200),
        idle_poll_interval: Duration::from_millis(200),
    };
    let mut server =
        TestServer::start_with_detection_config("live-session-id-discovery-test", detection_config)
            .await;
    let transcript_path = write_verified_claude_transcript(&server, resumed_session_id);
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "live-session-id-discovery-test".to_string(),
        },
    )
    .await
    .expect("write Attach request");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    // Same `$SHELL -c "<command_line>"` mechanism as the `Working`/`Idle`
    // test above. The real spawned process exposes the resume ID in argv and
    // owns the verified transcript descriptor, rather than supplying either
    // signal through a fixture inside the discovery implementation.
    let command_line = format!(
        "{} --resume {resumed_session_id} {}",
        fake_claude_path.display(),
        transcript_path.display()
    );
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(command_line.clone()),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("write NewPane request");
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches TreeSnapshot");
    };
    let default_group = tree
        .children_of(ROOT_ID)
        .expect("root is a group")
        .first()
        .copied()
        .expect("a default group should have been created for the pane");
    let pane_id = tree.children_of(default_group).expect("group has children")[0];

    let resolved_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneSessionIdResolved { pane_id: changed_id, .. }
                if *changed_id == pane_id
        )
    })
    .await;
    let ServerEvent::PaneSessionIdResolved { session_id, .. } = resolved_event else {
        unreachable!("predicate only matches PaneSessionIdResolved");
    };
    assert_eq!(
        session_id, resumed_session_id,
        "expected the real process's --resume argument to be discovered verbatim"
    );

    write_frame(
        &mut client,
        &ClientRequest::SetSessionPaneTitle {
            pane_id,
            expected_session_id: resumed_session_id.to_string(),
            expected_title_generation: 0,
            title: "Title From The Old Session".to_string(),
            short_title: Some("Old Session".to_string()),
            title_source: ilium_core::PaneTitleSource::Automatic,
        },
    )
    .await
    .expect("set an inferred title for the old session");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::TreeSnapshot(tree)
                if tree.get(pane_id).is_some_and(|node| node.name == "Title From The Old Session")
        )
    })
    .await;

    // `/clear` preserves this fake process's resolved session ID, exactly
    // the case where an ID-only title compare-and-set would let an old LLM
    // worker put the stale title back. The real input path must reset both
    // displayed title forms and advance the independent title generation.
    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"/clear\r".to_vec(),
        },
    )
    .await
    .expect("submit /clear to the live fake agent");
    let clear_event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::PaneSessionTitleCleared {
                pane_id: changed_id,
                title_generation: 1,
            } if *changed_id == pane_id
        )
    })
    .await;
    assert!(matches!(
        clear_event,
        ServerEvent::PaneSessionTitleCleared { .. }
    ));
    let reset_after_clear = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::TreeSnapshot(tree)
                if tree.get(pane_id).is_some_and(|node| {
                    node.name == command_line && node.short_name.is_none()
                })
        )
    })
    .await;
    assert!(matches!(reset_after_clear, ServerEvent::TreeSnapshot(_)));

    write_frame(
        &mut client,
        &ClientRequest::SetSessionPaneTitle {
            pane_id,
            expected_session_id: resumed_session_id.to_string(),
            expected_title_generation: 0,
            title: "Stale Clear Result Must Not Return".to_string(),
            short_title: Some("Stale Clear".to_string()),
            title_source: ilium_core::PaneTitleSource::Automatic,
        },
    )
    .await
    .expect("send the pre-clear title-worker result");
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "live-session-id-discovery-test".to_string(),
        },
    )
    .await
    .expect("request authoritative tree after the stale clear result");
    let tree_after_stale_clear = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree_after_stale_clear) = tree_after_stale_clear else {
        unreachable!("predicate only matches TreeSnapshot");
    };
    assert_eq!(
        tree_after_stale_clear.get(pane_id).unwrap().name,
        command_line
    );
    assert_eq!(
        tree_after_stale_clear.get(pane_id).unwrap().short_name,
        None
    );

    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"/resume\r".to_vec(),
        },
    )
    .await
    .expect("submit an in-process session transition");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::PaneSessionIdCleared { pane_id: changed_id, .. } if *changed_id == pane_id)
    })
    .await;
    // The old transcript is deliberately still open on the exact same PID.
    // It must remain quarantined instead of immediately rebinding the ID and
    // retriggering a title request on the next fixed one-second detection tick.
    let rebound_old_session = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event: ServerEvent = read_frame(&mut client)
                .await
                .expect("read event while checking invalidated-session quarantine");
            if matches!(
                event,
                ServerEvent::PaneSessionIdResolved {
                    pane_id: changed_id,
                    session_id,
                    ..
                } if changed_id == pane_id && session_id == resumed_session_id
            ) {
                return;
            }
        }
    })
    .await;
    assert!(
        rebound_old_session.is_err(),
        "the same PID's still-open old transcript must not restore an invalidated session ID"
    );

    // Recreate the IPC ordering race directly: a worker result tagged with
    // the cleared ID arrives after invalidation. The server's compare-and-set
    // must reject it even if the sending client had not processed the clear.
    write_frame(
        &mut client,
        &ClientRequest::SetSessionPaneTitle {
            pane_id,
            expected_session_id: resumed_session_id.to_string(),
            expected_title_generation: 0,
            title: "Stale Result Must Not Return".to_string(),
            short_title: Some("Stale Result".to_string()),
            title_source: ilium_core::PaneTitleSource::Automatic,
        },
    )
    .await
    .expect("send a stale session-title result");
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "live-session-id-discovery-test".to_string(),
        },
    )
    .await
    .expect("request authoritative tree after stale title");
    let authoritative_tree = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(authoritative_tree) = authoritative_tree else {
        unreachable!("predicate only matches TreeSnapshot");
    };
    assert_eq!(authoritative_tree.get(pane_id).unwrap().name, command_line);

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

/// Same proof as
/// [`a_resumed_claude_processs_session_id_is_discovered_and_broadcast`],
/// for a real process named `codex` invoked with `resume <uuid>` (Codex's
/// positional resume form, distinct from Claude's `--resume <uuid>` flag --
/// see `session_id::from_arguments`). This is the exact asymmetry this
/// feature was reported broken on ("works for Claude, not Codex"): argument
/// and exact-PID open-file evidence are symmetric between the two classes.
/// No environment/directory/newest-transcript fallback
/// exists: if neither process-bound source proves ownership, no ID is emitted.
#[tokio::test]
async fn a_resumed_codex_processs_session_id_is_discovered_and_broadcast() {
    let fake_bin_dir = tempfile::tempdir().expect("create tempdir for the fake codex binary");
    let fake_codex_path = write_idle_fake_agent_binary(fake_bin_dir.path(), "codex");
    let resumed_session_id = "4e8767e0-8b01-4329-bfc6-a6087b1b1f9e";

    let detection_config = DetectionConfig {
        working_poll_interval: Duration::from_millis(200),
        idle_poll_interval: Duration::from_millis(200),
    };
    let mut server = TestServer::start_with_detection_config(
        "live-codex-session-id-discovery-test",
        detection_config,
    )
    .await;
    write_verified_codex_transcript(&server, resumed_session_id);
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "live-codex-session-id-discovery-test".to_string(),
        },
    )
    .await
    .expect("write Attach request");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    let command_line = format!("{} resume {resumed_session_id}", fake_codex_path.display());
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(command_line),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("write NewPane request");
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches TreeSnapshot");
    };
    let default_group = tree
        .children_of(ROOT_ID)
        .expect("root is a group")
        .first()
        .copied()
        .expect("a default group should have been created for the pane");
    let pane_id = tree.children_of(default_group).expect("group has children")[0];

    let resolved_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneSessionIdResolved { pane_id: changed_id, .. }
                if *changed_id == pane_id
        )
    })
    .await;
    let ServerEvent::PaneSessionIdResolved { session_id, .. } = resolved_event else {
        unreachable!("predicate only matches PaneSessionIdResolved");
    };
    assert_eq!(
        session_id, resumed_session_id,
        "expected the real Codex process's positional resume argument to be discovered verbatim"
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

/// Proves the second admissible source end-to-end: an agent with no session
/// ID in argv is resolved only from a project-verified transcript held open by
/// that exact detected process.
#[tokio::test]
async fn a_codex_processs_open_transcript_is_discovered_and_broadcast() {
    let fake_bin_dir = tempfile::tempdir().expect("create tempdir for the fake codex binary");
    let fake_codex_path = write_transcript_holding_fake_agent_binary(fake_bin_dir.path(), "codex");
    let session_id = "6f7a8891-6c0a-4e60-9448-1e63fc74cd82";
    let detection_config = DetectionConfig {
        working_poll_interval: Duration::from_millis(200),
        idle_poll_interval: Duration::from_millis(200),
    };
    let mut server = TestServer::start_with_detection_config(
        "live-open-transcript-discovery-test",
        detection_config,
    )
    .await;
    let transcript_path = write_verified_codex_transcript(&server, session_id);
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "live-open-transcript-discovery-test".to_string(),
        },
    )
    .await
    .expect("write Attach request");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(format!(
                "{} {}",
                fake_codex_path.display(),
                transcript_path.display()
            )),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("write NewPane request");
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches TreeSnapshot");
    };
    let default_group = tree.children_of(ROOT_ID).unwrap()[0];
    let pane_id = tree.children_of(default_group).unwrap()[0];

    let resolved_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneSessionIdResolved { pane_id: changed_id, .. }
                if *changed_id == pane_id
        )
    })
    .await;
    let ServerEvent::PaneSessionIdResolved {
        session_id: resolved_session_id,
        ..
    } = resolved_event
    else {
        unreachable!("predicate only matches PaneSessionIdResolved");
    };
    assert_eq!(resolved_session_id, session_id);

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}
