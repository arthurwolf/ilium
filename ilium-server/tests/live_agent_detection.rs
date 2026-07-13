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
//! The fake script's only job is producing the one *observable signal*
//! `ilium_detect::classify_activity` actually keys off
//! (`ilium-detect/src/lib.rs`'s `WORKING_MARKER`, the literal substring
//! `"esc to interrupt"`) for a few seconds, then clearing the screen and
//! printing something else -- simulating a turn finishing. Everything
//! downstream of that (the real `sysinfo` process-tree walk, the real
//! `vt100` screen-text scrape, the real classification, the real
//! `PaneStatusChanged` broadcast) is exactly the production code path,
//! nothing faked except the one external binary name and its output.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use ilium_core::{PaneStatus, ROOT_ID};
use ilium_ipc::{write_frame, ClientRequest, NewPaneKind, ServerEvent};
use ilium_server::config::DetectionConfig;

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
         \x20\x20printf 'Cogitating (esc to interrupt)\\n'\n\
         \x20\x20i=$((i + 1))\n\
         \x20\x20sleep 1\n\
         done\n\
         printf '\\033[2J\\033[H'\n\
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
async fn a_real_process_named_claude_drives_working_to_idle_through_the_whole_pipeline() {
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
    let server =
        TestServer::start_with_detection_config("live-agent-detection-test", detection_config)
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
    // prints) -- broadcast as a real `PaneStatusChanged` event to this
    // real connected IPC client.
    let working_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneStatusChanged { pane_id: changed_id, status }
                if *changed_id == pane_id
                    && matches!(status, PaneStatus::Agent(_, ilium_core::AgentActivity::Working))
        )
    })
    .await;
    let ServerEvent::PaneStatusChanged { status, .. } = &working_event else {
        unreachable!("predicate only matches PaneStatusChanged");
    };
    assert!(
        matches!(status, PaneStatus::Agent(ilium_core::AgentClass::Claude, _)),
        "expected the real process tree walk to identify this pane as Claude, got {status:?}"
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
                        PaneStatus::Agent(
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
        PaneStatus::Agent(ilium_core::AgentClass::Claude, ilium_core::AgentActivity::Done),
        "expected a real Working -> Done transition (this client never focused the pane), got {status:?}"
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

/// Writes an executable POSIX shell script named exactly `name` (same
/// absolute-path-spawn rationale as [`write_fake_claude_binary`]) that just
/// idles -- the two session-ID-discovery tests below only care about
/// `crate::session_id::discover`'s tier 1 (an explicit resume argument on
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

/// End-to-end test of `ilium-server::session_id`'s live wiring into the
/// detection loop: a real spawned process (named `claude`, exactly like
/// [`a_real_process_named_claude_drives_working_to_idle_through_the_whole_pipeline`])
/// invoked with a `--resume <uuid>` argument must have that exact uuid
/// picked up by `session_id::discover`'s tier 1 (`from_arguments`, a real
/// `sysinfo::Process::cmd()` read of this real process, not a fixture) and
/// broadcast as a real `ServerEvent::PaneSessionIdResolved` to a real
/// connected IPC client -- proving the gap this whole feature was built to
/// close (the pre-refactor bin crate's session-ID discovery never having
/// been ported into the new client/server split, see `session_id`'s module
/// docs) is actually closed, not just unit-tested in isolation.
#[tokio::test]
async fn a_resumed_claude_processs_session_id_is_discovered_and_broadcast() {
    let fake_bin_dir = tempfile::tempdir().expect("create tempdir for the fake claude binary");
    let fake_claude_path = write_idle_fake_agent_binary(fake_bin_dir.path(), "claude");
    let resumed_session_id = "95fd0645-3331-408b-a7e5-36e6007bfb78";

    let detection_config = DetectionConfig {
        working_poll_interval: Duration::from_millis(200),
        idle_poll_interval: Duration::from_millis(200),
    };
    let server =
        TestServer::start_with_detection_config("live-session-id-discovery-test", detection_config)
            .await;
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
    // test above -- the real spawned process's real argv ends up being
    // exactly `<fake_claude_path> --resume <resumed_session_id>`, so
    // `session_id::discover`'s tier 1 (`from_arguments`, reading this
    // exact process's `sysinfo::Process::cmd()`) has real data to find,
    // not a fixture.
    let command_line = format!(
        "{} --resume {resumed_session_id}",
        fake_claude_path.display()
    );
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(command_line),
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

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

/// Same proof as
/// [`a_resumed_claude_processs_session_id_is_discovered_and_broadcast`],
/// for a real process named `codex` invoked with `resume <uuid>` (Codex's
/// positional resume form, distinct from Claude's `--resume <uuid>` flag --
/// see `session_id::from_arguments`). This is the exact asymmetry this
/// feature was reported broken on ("works for Claude, not Codex"): tiers
/// 1-3 (`from_arguments`/`from_environment`/`from_open_files`) are fully
/// symmetric between the two classes, so a resumed Codex process is
/// discovered exactly like a resumed Claude one -- only tier 4 (the
/// newest-project-transcript fallback, irrelevant here since tier 1 always
/// wins first) is deliberately Claude-only (see `session_id`'s module
/// docs).
#[tokio::test]
async fn a_resumed_codex_processs_session_id_is_discovered_and_broadcast() {
    let fake_bin_dir = tempfile::tempdir().expect("create tempdir for the fake codex binary");
    let fake_codex_path = write_idle_fake_agent_binary(fake_bin_dir.path(), "codex");
    let resumed_session_id = "4e8767e0-8b01-4329-bfc6-a6087b1b1f9e";

    let detection_config = DetectionConfig {
        working_poll_interval: Duration::from_millis(200),
        idle_poll_interval: Duration::from_millis(200),
    };
    let server = TestServer::start_with_detection_config(
        "live-codex-session-id-discovery-test",
        detection_config,
    )
    .await;
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
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}
