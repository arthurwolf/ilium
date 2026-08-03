//! End-to-end test of the *whole* live agent-detection pipeline: process
//! spawn -> `sysinfo` sees it -> `ilium_detect::identify_agent` matches
//! `"codex"` by real process name -> screen text scraped from a real,
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
//! `codex` into a tempdir and spawns it by its *absolute* path, so
//! `ilium_detect::identify_agent`'s real, unmodified process-name
//! matching (`BuiltinAgentProvider::ALL`, checked against `sysinfo`'s real process
//! list -- the kernel names a process after the file it was exec'd from,
//! not the path used to reach it, so an absolute-path-invoked script
//! named `codex` is still reported as `codex`) genuinely finds it,
//! exactly as it would find a real Codex process, without this
//! workspace's tests ever shelling out to OpenAI's actual CLI or
//! requiring it to be installed on the machine running this suite.
//!
//! Deliberately an absolute path, not a bare `codex` added to `PATH`:
//! this test's own development turned up that a login shell's rc files
//! (`~/.zshenv`, etc. -- `TerminalOrigin::Command` runs `$SHELL -c
//! "<command_line>"`, and most interactive shells still source at least
//! one rc file even in `-c` mode) can silently re-order or rewrite
//! `PATH`, so a `PATH`-prepended fake binary can lose a race against a
//! *real* `codex` install already on the developer's machine -- exactly
//! the failure mode this whole test exists to never risk. An absolute
//! path sidesteps `PATH`/rc-file resolution entirely and is
//! unconditionally deterministic.
//!
//! The fake script produces the two observable screen-text signals the
//! detector owns: Codex's wide shared-footer goal suffix plus activity state
//! (`ilium-detect/src/lib.rs`'s `WORKING_MARKER`, the literal substring
//! `"esc to interrupt"`) for a few seconds, then clearing the screen and
//! printing something else -- simulating a turn finishing. Everything
//! downstream of that (the real `sysinfo` process-tree walk, the real
//! `vt100` screen-text scrape, the real classification, the real
//! `PaneStatusChanged` broadcast) is exactly the production code path,
//! nothing faked except the one external binary name and its output.

//! Unix-only: every fixture here is a `/bin/sh` script that emits specific
//! ANSI sequences to imitate a real agent CLI, and the server drives it through
//! a real PTY. Porting the fixtures to `cmd`/PowerShell would test the fixture
//! rewrite as much as the server, so Windows coverage for these paths comes
//! from the cross-platform tests instead. See docs/TODO.md.
#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ilium_core::{NodeId, PaneStatus, ROOT_ID};
use ilium_ipc::{
    read_frame, write_frame, AgentDebugEventKind, ClientRequest, NewPaneKind, ServerEvent,
};
use ilium_server::config::DetectionConfig;
use ilium_server::SoundPlayer;

mod common;
use common::{expect_event, TestServer};

/// How long the fake `codex` script prints the `"esc to interrupt"`
/// marker before switching to its idle phase. Long enough to comfortably
/// span at least one real detection-loop tick (`ilium-server::detection`'s
/// `BASE_TICK_INTERVAL` is a fixed 1s -- not configurable via
/// `DetectionConfig`, which only controls how *often a due pane is
/// rechecked*, not the loop's own wake cadence) even under test-runner
/// load.
const WORKING_PHASE_SECONDS: u32 = 12;

/// Generous relative to the fixed ~1s tick granularity above -- covers a
/// slow CI runner without ever being so long a genuine regression would
/// make this test slow to fail.
const WAIT_TIMEOUT: Duration = Duration::from_secs(30);

struct RecordingSoundPlayer {
    calls: Arc<Mutex<Vec<ilium_sound::SoundSettings>>>,
}

impl SoundPlayer for RecordingSoundPlayer {
    fn play(&self, settings: &ilium_sound::SoundSettings) -> Result<(), ilium_sound::SoundError> {
        self.calls.lock().unwrap().push(settings.clone());
        Ok(())
    }
}

/// Writes an executable POSIX shell script named exactly `codex` into
/// `bin_dir` and returns its absolute path -- never the real system
/// `PATH`, never a real Codex binary (see this file's module docs on why
/// this is spawned by absolute path rather than added to `PATH`).
///
/// The script prints a line containing the literal
/// `ilium_detect::classify_activity` "working" marker
/// (`"esc to interrupt"`) once a second for [`WORKING_PHASE_SECONDS`],
/// then clears the screen (`\x1b[2J\x1b[H`) and prints something else --
/// the screen must actually stop *containing* the marker for a real
/// `Idle` reclassification, not merely stop *adding* new instances of
/// it, since `vt100::Screen::contents()` reflects the current visible
/// screen, not an ever-growing scrollback.
fn write_fake_codex_binary(bin_dir: &std::path::Path) -> std::path::PathBuf {
    let script_path = bin_dir.join("codex");
    let script = format!(
        "#!/bin/sh\n\
         i=0\n\
         while [ \"$i\" -lt {WORKING_PHASE_SECONDS} ]; do\n\
         \x20\x20printf 'gpt-5.6-sol xhigh · workspace · Working · Pursuing goal (5m)\\n'\n\
         \x20\x20printf 'Cogitating (esc to interrupt)\\n'\n\
         \x20\x20i=$((i + 1))\n\
         \x20\x20sleep 1\n\
         done\n\
         printf '\\033[2J\\033[H'\n\
         printf 'Done. Ready for the next instruction.\\n'\n\
         IFS= read -r queued_prompt\n\
         printf 'queued:<%s>\\n' \"$queued_prompt\"\n\
         sleep 60\n"
    );
    let mut file = std::fs::File::create(&script_path).expect("create fake codex script");
    file.write_all(script.as_bytes())
        .expect("write fake codex script");
    // Executable for the owner is enough -- this script is only ever run
    // by this same test process's own spawned children.
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .expect("chmod fake codex script executable");
    script_path
}

/// Writes a short-lived working phase followed by an idle screen. Unlike the
/// queue fixture above, this process never reads terminal input, isolating the
/// focus acknowledgement from queue-delivery acknowledgement behavior.
fn write_finished_fake_codex_binary(bin_dir: &std::path::Path) -> std::path::PathBuf {
    let script_path = bin_dir.join("codex");
    let script = "#!/bin/sh\n\
                  printf 'Cogitating (esc to interrupt)\\n'\n\
                  sleep 2\n\
                  printf '\\033[2J\\033[HDone. Ready for the next instruction.\\n'\n\
                  sleep 60\n";
    let mut file = std::fs::File::create(&script_path).expect("create finished fake codex");
    file.write_all(script.as_bytes())
        .expect("write finished fake codex");
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .expect("make finished fake codex executable");
    script_path
}

/// Resolves the first pane created through the root shortcut. The server
/// deliberately translates that legacy shortcut into `project -> default ->
/// pane`, so these end-to-end assertions must follow the user-visible tree
/// rather than assuming a group may live directly under the root.
fn first_launch_project_pane(tree: &ilium_core::Tree) -> NodeId {
    let project_id = tree
        .project_ids()
        .into_iter()
        .next()
        .expect("a launch project should exist for the pane");
    let default_group = tree
        .children_of(project_id)
        .expect("project is a container")
        .first()
        .copied()
        .expect("a default group should have been created for the pane");
    tree.children_of(default_group)
        .expect("default group is a container")[0]
}

#[tokio::test]
async fn focusing_a_finished_agent_clears_its_bell_through_live_ipc() {
    let fake_bin_dir = tempfile::tempdir().expect("create tempdir for the finished fake codex");
    let fake_codex_path = write_finished_fake_codex_binary(fake_bin_dir.path());
    let detection_config = DetectionConfig {
        working_poll_interval: Duration::from_millis(100),
        idle_poll_interval: Duration::from_millis(100),
    };
    let mut server =
        TestServer::start_with_detection_config("focus-acknowledgement-test", detection_config)
            .await;
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "focus-acknowledgement-test".to_string(),
        },
    )
    .await
    .expect("attach to the live focus-acknowledgement session");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(fake_codex_path.to_string_lossy().to_string()),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("start the live finished fake Codex agent");
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches TreeSnapshot");
    };
    let pane_id = first_launch_project_pane(&tree);

    let _ = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneStatusChanged { pane_id: changed_id, status }
                if *changed_id == pane_id
                    && matches!(
                        status,
                        PaneStatus::Agent(ilium_core::AgentClass::Codex, ilium_core::AgentActivity::Working)
                    )
        )
    })
    .await;
    let _ = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneStatusChanged { pane_id: changed_id, status }
                if *changed_id == pane_id
                    && matches!(
                        status,
                        PaneStatus::Agent(ilium_core::AgentClass::Codex, ilium_core::AgentActivity::Done)
                    )
        )
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::SetPaneFocus {
            pane_id,
            focused: true,
        },
    )
    .await
    .expect("open the completed agent pane");
    let acknowledged_event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::PaneStatusChanged { pane_id: changed_id, status }
                if *changed_id == pane_id
                    && *status
                        == PaneStatus::Agent(
                            ilium_core::AgentClass::Codex,
                            ilium_core::AgentActivity::Idle,
                        )
        )
    })
    .await;
    assert!(matches!(
        acknowledged_event,
        ServerEvent::PaneStatusChanged { .. }
    ));

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("stop the live focus-acknowledgement session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn a_real_process_named_codex_preserves_its_pursuing_goal_status_through_the_whole_pipeline()
{
    let fake_bin_dir = tempfile::tempdir().expect("create tempdir for the fake codex binary");
    let fake_codex_path = write_fake_codex_binary(fake_bin_dir.path());

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
        &ClientRequest::UpdateAgentDebugMenu { enabled: true },
    )
    .await
    .expect("enable live per-agent debug capture");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::AgentDebugMenuChanged { enabled: true })
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
    // exactly `codex` (see this file's module docs on why an absolute
    // path is used here instead of relying on `PATH`).
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(fake_codex_path.to_string_lossy().to_string()),
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
    let pane_id = first_launch_project_pane(&tree);

    // Focus is set here, before any turn has completed, so
    // `handle_set_pane_focus`'s completion acknowledgement has nothing to
    // acknowledge yet (see `focusing_a_finished_agent_clears_its_bell_through_live_ipc`
    // for the case where focus arrives *after* completion and does clear it).
    // The later Working -> Done transition below must therefore still happen
    // on its own, and the later Done -> Idle transition must come from the
    // queued prompt's delivery gate, not from this early focus call.
    write_frame(
        &mut client,
        &ClientRequest::SetPaneFocus {
            pane_id,
            focused: true,
        },
    )
    .await
    .expect("focus the live fake agent pane");

    write_frame(
        &mut client,
        &ClientRequest::EnqueuePrompt {
            pane_id,
            text: "queued-live-marker".to_string(),
            delivery: ilium_core::PromptQueueDelivery::Once,
        },
    )
    .await
    .expect("enqueue prompt through live IPC");
    let queued_tree = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if tree.prompt_queue_len(pane_id) == Some(1))
    })
    .await;
    assert!(matches!(queued_tree, ServerEvent::TreeSnapshot(_)));

    // Structural assertion #1: the real detection loop, walking the real
    // `sysinfo` process tree, must identify the spawned process as a
    // Codex agent (by real process name, via
    // `ilium_detect::BuiltinAgentProvider::ALL`) and classify it `Working` (by
    // real `vt100` screen-text scrape, via `ilium_detect::classify_activity`
    // matching the literal `"esc to interrupt"` marker this fake script
    // prints), and preserve the `Pursuing goal (5m)` suffix at the end of its
    // shared metadata footer as
    // `AgentWithGoal` --
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
            PaneStatus::AgentWithGoal(ilium_core::AgentClass::Codex, _)
        ),
        "expected the real process tree walk to identify this pane as Codex, got {status:?}"
    );
    assert!(
        sound_calls.lock().unwrap().is_empty(),
        "the first Working classification is not a completed turn and must stay silent"
    );

    // Structural assertion #2: once the script clears the screen, both the
    // working marker and goal footer become temporarily absent. The real,
    // unmodified pipeline must retain goal ownership for the same process and
    // reclassify the pane `Done`, not plain `Idle`, even though it is focused.
    // `ilium-server::detection::promote_to_done` must turn the raw "just went
    // idle" verdict into the durable completed-turn state that drives the
    // sound, bell, and title marker.
    let done_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneStatusChanged { pane_id: changed_id, status }
                if *changed_id == pane_id
                    && matches!(
                        status,
                        PaneStatus::AgentWithGoal(
                            ilium_core::AgentClass::Codex,
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
            ilium_core::AgentClass::Codex,
            ilium_core::AgentActivity::Done
        ),
        "expected a real Working -> Done transition while preserving its goal, got {status:?}"
    );

    // Completion opens the queue gate. Its successful PTY write is fresh
    // input, so the same server-owned status immediately returns to Idle while
    // retaining the goal variant; later idle detection must not resurrect it.
    let acknowledged_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneStatusChanged { pane_id: changed_id, status }
                if *changed_id == pane_id
                    && matches!(
                        status,
                        PaneStatus::AgentWithGoal(
                            ilium_core::AgentClass::Codex,
                            ilium_core::AgentActivity::Idle
                        )
                    )
        )
    })
    .await;
    assert!(matches!(
        acknowledged_event,
        ServerEvent::PaneStatusChanged { .. }
    ));

    let delivered_prompt = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::ScreenUpdate { pane_id: changed_id, bytes, .. }
                if *changed_id == pane_id
                    && String::from_utf8_lossy(bytes).contains("queued:<queued-live-marker>")
        )
    })
    .await;
    assert!(matches!(delivered_prompt, ServerEvent::ScreenUpdate { .. }));

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

    // Wait for one post-input classification to establish the stable Idle
    // baseline. The input acknowledgement changes status synchronously; this
    // poll adds the corresponding change-only detector explanation once.
    let _ = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneDebugEntryAppended {
                pane_id: changed_id,
                entry,
            } if *changed_id == pane_id
                && entry.kind == AgentDebugEventKind::DetectionCycle
                && entry.context.activity == Some(ilium_core::AgentActivity::Idle)
        )
    })
    .await;
    write_frame(
        &mut client,
        &ClientRequest::GetPaneDebugLog {
            pane_id,
            after_sequence: None,
        },
    )
    .await
    .expect("request the complete live debug history");
    let debug_event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::PaneDebugLogSnapshot {
                pane_id: changed_id,
                ..
            } if *changed_id == pane_id
        )
    })
    .await;
    let ServerEvent::PaneDebugLogSnapshot { entries, .. } = debug_event else {
        unreachable!("predicate only matches PaneDebugLogSnapshot");
    };
    for expected_kind in [
        AgentDebugEventKind::DetectionCycle,
        AgentDebugEventKind::PromptQueued,
        AgentDebugEventKind::PromptSubmitted,
        AgentDebugEventKind::QueuedPromptDelivered,
    ] {
        assert!(
            entries.iter().any(|entry| entry.kind == expected_kind),
            "live history should contain {expected_kind:?}: {entries:#?}"
        );
    }
    let detection_entries: Vec<_> = entries
        .iter()
        .filter(|entry| entry.kind == AgentDebugEventKind::DetectionCycle)
        .collect();
    // The point is that repeated polls do not spam decisions -- one entry per
    // real change, not one per tick. The Working/Done/input-acknowledged-Idle
    // progression is always recorded; whether a leading Idle precedes it is a
    // scheduling detail, because the detector can legitimately observe the pane
    // once before the agent's first bytes have been parsed. Pinning an exact
    // count asserts how fast the machine is, not how the server behaves.
    assert!(
        (3..=4).contains(&detection_entries.len()),
        "repeated polls must produce one decision per real change, optionally \
         preceded by an idle observation made before the agent's first output: \
         {detection_entries:#?}"
    );
    assert!(detection_entries.iter().all(|entry| {
        [
            "Agent identity decision",
            "Activity decision",
            "Goal decision",
            "Session identity decision",
            "Applied pane state",
        ]
        .iter()
        .all(|label| entry.fields.iter().any(|field| field.label == *label))
    }));
    assert!(detection_entries.iter().all(|entry| {
        entry.fields.iter().all(|field| {
            !field.label.contains("phase ")
                && !field.label.contains("generation")
                && !field.value.contains("NoActiveMarker")
                && !field.value.contains("AgentWithGoal")
        })
    }));
    assert!(entries.iter().any(|entry| {
        entry.kind == AgentDebugEventKind::PromptSubmitted
            && entry.fields.iter().any(|field| {
                field.label == "submitted input" && field.value == "queued-live-marker"
            })
    }));

    tokio::time::sleep(Duration::from_millis(700)).await;
    write_frame(
        &mut client,
        &ClientRequest::GetPaneDebugLog {
            pane_id,
            after_sequence: None,
        },
    )
    .await
    .expect("request history after several unchanged idle-state polls");
    let later_debug_event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::PaneDebugLogSnapshot {
                pane_id: changed_id,
                ..
            } if *changed_id == pane_id
        )
    })
    .await;
    let ServerEvent::PaneDebugLogSnapshot {
        entries: later_entries,
        ..
    } = later_debug_event
    else {
        unreachable!("predicate only matches PaneDebugLogSnapshot");
    };
    let later_detection_entries: Vec<_> = later_entries
        .iter()
        .filter(|entry| entry.kind == AgentDebugEventKind::DetectionCycle)
        .collect();
    assert_eq!(
        later_detection_entries, detection_entries,
        "unchanged polls must not mutate timestamps, counts, fields, or sequences"
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

/// Writes an executable POSIX shell script named exactly `name` (same
/// absolute-path-spawn rationale as [`write_fake_codex_binary`]) that just
/// idles -- the argument-based session-ID-discovery tests below only care about
/// `ilium_server::session_id::discover_with_trace`'s second admissible source
/// (an explicit resume argument on the command line), not activity
/// classification, so unlike [`write_fake_codex_binary`] neither ever needs
/// to print the "esc to interrupt" working marker.
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

/// Writes a Codex-shaped process that reproduces 0.144.6's `/clear`
/// descriptor lifecycle: the old rollout stays open, `/clear` resets the
/// conversation in place, and the next submitted prompt opens a new rollout
/// without replacing the process.
fn write_clear_transitioning_fake_codex_binary(bin_dir: &std::path::Path) -> std::path::PathBuf {
    let script_path = bin_dir.join("codex");
    let script = "#!/bin/sh\n\
                  exec 3<\"$1\"\n\
                  cleared=0\n\
                  printf 'Done. Ready for the next instruction.\\n'\n\
                  while IFS= read -r submitted_line; do\n\
                  \x20\x20if [ \"$submitted_line\" = \"/clear\" ]; then\n\
                  \x20\x20\x20\x20cleared=1\n\
                  \x20\x20\x20\x20printf '\\033[2J\\033[Hnew conversation started\\n'\n\
                  \x20\x20elif [ \"$cleared\" -eq 1 ]; then\n\
                  \x20\x20\x20\x20exec 4<\"$2\"\n\
                  \x20\x20\x20\x20cleared=0\n\
                  \x20\x20\x20\x20printf '23\\n'\n\
                  \x20\x20fi\n\
                  done\n";
    let mut file = std::fs::File::create(&script_path).expect("create fake clearing Codex script");
    file.write_all(script.as_bytes())
        .expect("write fake clearing Codex script");
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .expect("chmod fake clearing Codex script executable");
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
/// detection loop: a real spawned process (named `claude`, using the same
/// absolute-path spawn mechanism as
/// [`a_real_process_named_codex_preserves_its_pursuing_goal_status_through_the_whole_pipeline`])
/// invoked with a `--resume <uuid>` argument and holding that transcript open
/// must broadcast that exact uuid as a real `ServerEvent::PaneSessionIdResolved`
/// to a real connected IPC client. It then proves `/clear` discards that ID,
/// every old title field and grouping, and the old descriptor before `/resume`
/// repeats the same invalidation contract.
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
    let pane_id = first_launch_project_pane(&tree);
    let project_id = tree
        .project_ancestor(pane_id)
        .expect("the launched pane must belong to a project");
    assert_ne!(tree.parent_of(pane_id), Some(project_id));

    let resolved_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneSessionIdResolved { pane_id: changed_id, .. }
                if *changed_id == pane_id
        )
    })
    .await;
    let ServerEvent::PaneSessionIdResolved {
        session_id,
        process_id,
        ..
    } = resolved_event
    else {
        unreachable!("predicate only matches PaneSessionIdResolved");
    };
    assert_eq!(
        session_id, resumed_session_id,
        "expected the real process's --resume argument to be discovered verbatim"
    );
    assert!(
        process_id.is_some_and(|process_id| process_id > 0),
        "the resolved session event must carry its detected owning process"
    );

    write_frame(
        &mut client,
        &ClientRequest::SetSessionPaneTitle {
            pane_id,
            expected_session_id: resumed_session_id.to_string(),
            expected_title_generation: 0,
            title: "Title From The Old Session".to_string(),
            short_title: Some("Old Session".to_string()),
            inferred_icon: Some("📜".to_string()),
            title_source: ilium_core::PaneTitleSource::UserSpecified,
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

    // `/clear` must be an ilium identity boundary even while this fake Claude
    // process keeps its old transcript descriptor open. A user title belongs
    // to that old conversation too, so every title field must be discarded.
    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"/clear\r".to_vec(),
            submission: None,
        },
    )
    .await
    .expect("submit /clear to the live fake agent");
    let mut saw_session_clear = false;
    let mut saw_title_clear = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !saw_session_clear || !saw_title_clear {
            let event: ServerEvent = read_frame(&mut client)
                .await
                .expect("read a Claude clear transition event");
            match event {
                ServerEvent::PaneSessionIdCleared {
                    pane_id: changed_id,
                    title_generation: 1,
                } if changed_id == pane_id => saw_session_clear = true,
                ServerEvent::PaneSessionTitleCleared {
                    pane_id: changed_id,
                    title_generation: 1,
                } if changed_id == pane_id => saw_title_clear = true,
                _ => {}
            }
        }
    })
    .await
    .expect("timed out waiting for both Claude /clear transitions");
    let reset_after_clear = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::TreeSnapshot(tree)
                if tree.get(pane_id).is_some_and(|node| {
                    node.name == "<new>"
                        && node.short_name.is_none()
                        && node.inferred_icon.is_none()
                        && node.parent == Some(project_id)
                        && matches!(
                            &node.kind,
                            ilium_core::NodeKind::Pane {
                                title_source: ilium_core::PaneTitleSource::Automatic,
                                ..
                            }
                        )
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
            inferred_icon: Some("📜".to_string()),
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
    assert_eq!(tree_after_stale_clear.get(pane_id).unwrap().name, "<new>");
    assert_eq!(
        tree_after_stale_clear.get(pane_id).unwrap().short_name,
        None
    );

    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"/resume\r".to_vec(),
            submission: None,
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
            inferred_icon: Some("📜".to_string()),
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
    assert_eq!(authoritative_tree.get(pane_id).unwrap().name, "<new>");

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
    let pane_id = first_launch_project_pane(&tree);

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

/// Proves the first admissible source end-to-end: an agent with no session
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
    let pane_id = first_launch_project_pane(&tree);

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

/// Reproduces the real Codex 0.144.6 failure mode end to end. `/clear` keeps
/// one process alive and its old transcript open, while the next prompt opens
/// a different rollout. The server must clear the old identity immediately,
/// quarantine it during the empty transition window, then bind the new ID from
/// the same PID even though both descriptors remain open.
#[tokio::test]
async fn codex_clear_rebinds_the_same_process_to_its_new_open_transcript() {
    let fake_bin_dir = tempfile::tempdir().expect("create tempdir for the fake codex binary");
    let fake_codex_path = write_clear_transitioning_fake_codex_binary(fake_bin_dir.path());
    let old_session_id = "7a111111-1111-4111-8111-111111111111";
    let new_session_id = "7a222222-2222-4222-8222-222222222222";
    let detection_config = DetectionConfig {
        working_poll_interval: Duration::from_millis(200),
        idle_poll_interval: Duration::from_millis(200),
    };
    let mut server = TestServer::start_with_agent_debug(
        "live-codex-clear-session-transition-test",
        detection_config,
    )
    .await;
    let old_transcript_path = write_verified_codex_transcript(&server, old_session_id);
    let new_transcript_path = write_verified_codex_transcript(&server, new_session_id);
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "live-codex-clear-session-transition-test".to_string(),
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
                "{} {} {}",
                fake_codex_path.display(),
                old_transcript_path.display(),
                new_transcript_path.display()
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
    let pane_id = first_launch_project_pane(&tree);
    let project_id = tree
        .project_ancestor(pane_id)
        .expect("the launched pane must belong to a project");
    assert_ne!(tree.parent_of(pane_id), Some(project_id));

    let initial_resolution = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneSessionIdResolved {
                pane_id: changed_id,
                session_id,
                ..
            } if *changed_id == pane_id && session_id == old_session_id
        )
    })
    .await;
    let ServerEvent::PaneSessionIdResolved {
        process_id: initial_process_id,
        title_generation: initial_title_generation,
        ..
    } = initial_resolution
    else {
        unreachable!("predicate only matches PaneSessionIdResolved");
    };
    assert!(initial_process_id.is_some());
    assert_eq!(initial_title_generation, 0);

    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"/clear\r".to_vec(),
            submission: None,
        },
    )
    .await
    .expect("submit /clear to the live fake Codex process");

    let mut saw_session_clear = false;
    let mut saw_title_clear = false;
    tokio::time::timeout(WAIT_TIMEOUT, async {
        while !saw_session_clear || !saw_title_clear {
            let event: ServerEvent = read_frame(&mut client)
                .await
                .expect("read a clear transition event");
            match event {
                ServerEvent::PaneSessionIdCleared {
                    pane_id: changed_id,
                    title_generation,
                } if changed_id == pane_id => {
                    assert_eq!(title_generation, 1);
                    saw_session_clear = true;
                }
                ServerEvent::PaneSessionTitleCleared {
                    pane_id: changed_id,
                    title_generation,
                } if changed_id == pane_id => {
                    assert_eq!(title_generation, 1);
                    saw_title_clear = true;
                }
                _ => {}
            }
        }
    })
    .await
    .expect("timed out waiting for both /clear state transitions");

    let reset_after_clear = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::TreeSnapshot(tree)
                if tree.get(pane_id).is_some_and(|node| {
                    node.name == "<new>" && node.parent == Some(project_id)
                })
        )
    })
    .await;
    assert!(matches!(reset_after_clear, ServerEvent::TreeSnapshot(_)));

    // The fake process still holds only the invalidated old transcript until
    // its next prompt, matching the real Codex transition window exactly.
    let rebound_old_session = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event: ServerEvent = read_frame(&mut client)
                .await
                .expect("read event while checking the /clear quarantine");
            if matches!(
                event,
                ServerEvent::PaneSessionIdResolved {
                    pane_id: changed_id,
                    session_id,
                    ..
                } if changed_id == pane_id && session_id == old_session_id
            ) {
                return;
            }
        }
    })
    .await;
    assert!(
        rebound_old_session.is_err(),
        "the old rollout must stay quarantined while Codex waits for its next prompt"
    );

    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"What is 9 + 14?\r".to_vec(),
            submission: None,
        },
    )
    .await
    .expect("submit the first prompt in the new Codex rollout");
    let replacement_resolution = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneSessionIdResolved {
                pane_id: changed_id,
                session_id,
                ..
            } if *changed_id == pane_id && session_id == new_session_id
        )
    })
    .await;
    let ServerEvent::PaneSessionIdResolved {
        process_id: replacement_process_id,
        title_generation: replacement_title_generation,
        ..
    } = replacement_resolution
    else {
        unreachable!("predicate only matches PaneSessionIdResolved");
    };
    assert_eq!(replacement_process_id, initial_process_id);
    assert_eq!(replacement_title_generation, 1);

    write_frame(
        &mut client,
        &ClientRequest::GetPaneDebugLog {
            pane_id,
            after_sequence: None,
        },
    )
    .await
    .expect("request the complete Codex transition history");
    let debug_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneDebugLogSnapshot {
                pane_id: changed_id,
                ..
            } if *changed_id == pane_id
        )
    })
    .await;
    let ServerEvent::PaneDebugLogSnapshot { entries, .. } = debug_event else {
        unreachable!("predicate only matches PaneDebugLogSnapshot");
    };
    let session_clear = entries
        .iter()
        .find(|entry| {
            entry.kind == AgentDebugEventKind::SessionCleared
                && entry.summary.contains("submitted command")
        })
        .expect("history contains the command-driven identity invalidation");
    let replacement = entries
        .iter()
        .find(|entry| {
            entry.kind == AgentDebugEventKind::SessionResolved
                && entry.fields.iter().any(|field| {
                    field.label == "Accepted session ID" && field.value == new_session_id
                })
        })
        .expect("history contains the project-verified replacement identity");
    let unresolved_transition_discovery = entries
        .iter()
        .find(|entry| {
            entry.kind == AgentDebugEventKind::SessionDiscovery
                && entry.summary.contains("no admissible identity")
                && entry.correlation_id == session_clear.correlation_id
        })
        .expect("history contains the unresolved post-clear discovery window");
    let replacement_discovery = entries
        .iter()
        .find(|entry| {
            entry.kind == AgentDebugEventKind::SessionDiscovery
                && entry.summary.contains("project-verified identity")
                && entry.fields.iter().any(|field| {
                    field.label == "Session ID after check" && field.value == new_session_id
                })
        })
        .expect("history contains the complete replacement discovery trace");
    assert!(session_clear.fields.iter().any(|field| {
        field.label == "session ID before invalidation" && field.value == old_session_id
    }));
    assert!(session_clear
        .fields
        .iter()
        .any(|field| { field.label == "submitted transition command" && field.value == "/clear" }));
    assert!(session_clear.fields.iter().any(|field| {
        field.label == "invalidation rule"
            && field.value.contains("Claude or Codex")
            && field.value.contains("fresh ilium session identity")
    }));
    assert!(replacement.fields.iter().any(|field| {
        field.label == "Replaced invalidated session ID" && field.value == old_session_id
    }));
    assert!(unresolved_transition_discovery.fields.iter().any(|field| {
        field.label == "Excluded session IDs"
            && field.value.contains(old_session_id)
            && field.value.contains("invalidated")
    }));
    assert!(unresolved_transition_discovery
        .fields
        .iter()
        .any(|field| { field.label == "Session ID after check" && field.value == "none" }));
    assert!(replacement_discovery
        .fields
        .iter()
        .any(|field| { field.label == "Provider under check" && field.value == "Codex" }));
    assert!(replacement_discovery.fields.iter().any(|field| {
        field.label == "Agent PID under check"
            && initial_process_id.is_some_and(|process_id| field.value == process_id.to_string())
    }));
    assert!(replacement_discovery.fields.iter().any(|field| {
        field.label == "Canonical project boundary"
            && field.value == server.project_cwd.to_string_lossy()
    }));
    assert!(replacement_discovery.fields.iter().any(|field| {
        field.label == "Phase: Open transcript descriptors"
            && field.value.contains(old_session_id)
            && field.value.contains(new_session_id)
    }));
    assert!(session_clear.correlation_id.is_some());
    assert_eq!(replacement.correlation_id, session_clear.correlation_id);
    assert_eq!(
        replacement_discovery.correlation_id,
        session_clear.correlation_id
    );
    assert!(entries.iter().any(|entry| {
        entry.kind == AgentDebugEventKind::PromptSubmitted
            && entry.correlation_id == session_clear.correlation_id
            && entry.fields.iter().any(|field| {
                field.label == "session lifecycle decision" && field.value.contains("Codex")
            })
    }));

    let mut reattached_client = server.connect().await;
    write_frame(
        &mut reattached_client,
        &ClientRequest::Attach {
            session: "live-codex-clear-session-transition-test".to_string(),
        },
    )
    .await
    .expect("reattach after the Codex session transition");
    let replayed_resolution = expect_event(&mut reattached_client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneSessionIdResolved {
                pane_id: changed_id,
                session_id,
                process_id,
                title_generation: 1,
            } if *changed_id == pane_id
                && session_id == new_session_id
                && *process_id == initial_process_id
        )
    })
    .await;
    assert!(matches!(
        replayed_resolution,
        ServerEvent::PaneSessionIdResolved { .. }
    ));

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}
