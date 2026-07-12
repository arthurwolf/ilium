//! End-to-end test of the *whole* live agent-detection pipeline: process
//! spawn -> `sysinfo` sees it -> `illium_detect::identify_agent` matches
//! `"claude"` by real process name -> screen text scraped from a real,
//! live `vt100` parser feed (`illium-pty`) -> `illium_detect::classify_activity`
//! -> a `PaneStatus` change reaches a connected IPC client. This is
//! meaningfully different from `illium-detect`'s own fixture-based unit
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
//! `illium_detect::identify_agent`'s real, unmodified process-name
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
//! `illium_detect::classify_activity` actually keys off
//! (`illium-detect/src/lib.rs`'s `WORKING_MARKER`, the literal substring
//! `"esc to interrupt"`) for a few seconds, then clearing the screen and
//! printing something else -- simulating a turn finishing. Everything
//! downstream of that (the real `sysinfo` process-tree walk, the real
//! `vt100` screen-text scrape, the real classification, the real
//! `PaneStatusChanged` broadcast) is exactly the production code path,
//! nothing faked except the one external binary name and its output.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use illium_core::{PaneStatus, ROOT_ID};
use illium_ipc::{write_frame, ClientRequest, NewPaneKind, ServerEvent};
use illium_server::config::DetectionConfig;

mod common;
use common::{expect_event, TestServer};

/// How long the fake `claude` script prints the `"esc to interrupt"`
/// marker before switching to its idle phase. Long enough to comfortably
/// span at least one real detection-loop tick (`illium-server::detection`'s
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
/// `illium_detect::classify_activity` "working" marker
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
    // `illium-server::pane::spawn_terminal_session`); passing the fake
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
    // `illium_detect::AGENT_SIGNATURES`) and classify it `Working` (by
    // real `vt100` screen-text scrape, via `illium_detect::classify_activity`
    // matching the literal `"esc to interrupt"` marker this fake script
    // prints) -- broadcast as a real `PaneStatusChanged` event to this
    // real connected IPC client.
    let working_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneStatusChanged { pane_id: changed_id, status }
                if *changed_id == pane_id
                    && matches!(status, PaneStatus::Agent(_, illium_core::AgentActivity::Working))
        )
    })
    .await;
    let ServerEvent::PaneStatusChanged { status, .. } = &working_event else {
        unreachable!("predicate only matches PaneStatusChanged");
    };
    assert!(
        matches!(
            status,
            PaneStatus::Agent(illium_core::AgentClass::Claude, _)
        ),
        "expected the real process tree walk to identify this pane as Claude, got {status:?}"
    );

    // Structural assertion #2: once the script clears the screen and
    // stops printing the marker, the real, unmodified pipeline must
    // reclassify the same pane `Idle` (or `Done` -- either is a
    // legitimate "turn finished" verdict from `classify_activity`'s plain
    // "nothing else matched" branch) on its own, with no fixture, no
    // manual state mutation, nothing faked past the one external binary
    // name and its printed output.
    let idle_event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        matches!(
            event,
            ServerEvent::PaneStatusChanged { pane_id: changed_id, status }
                if *changed_id == pane_id
                    && matches!(
                        status,
                        PaneStatus::Agent(
                            illium_core::AgentClass::Claude,
                            illium_core::AgentActivity::Idle | illium_core::AgentActivity::Done
                        )
                    )
        )
    })
    .await;
    let ServerEvent::PaneStatusChanged { status, .. } = idle_event else {
        unreachable!("predicate only matches PaneStatusChanged");
    };
    assert!(
        matches!(
            status,
            PaneStatus::Agent(
                illium_core::AgentClass::Claude,
                illium_core::AgentActivity::Idle | illium_core::AgentActivity::Done
            )
        ),
        "expected a real Working -> Idle/Done transition, got {status:?}"
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}
