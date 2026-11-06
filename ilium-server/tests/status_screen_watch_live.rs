//! End-to-end screen-watch observation through a real server, a real PTY and
//! the public IPC surface only.
//!
//! The pane runs a fixture executable spawned by absolute path (never through
//! `PATH`, never a real agent CLI) whose visible screen first contains the
//! working marker `Cogitating (esc to interrupt)`, then -- once a marker file
//! appears -- clears and prints a line without it. A raw IPC client, standing
//! in for `ilium-client`, subscribes a `SetStatusScreenWatches` revision and
//! asserts what `StatusScreenObserved` events the server sends back:
//!
//! - a revision subscribed before the pane existed never produces an
//!   observation for it (the server watches the panes registered at
//!   subscription time; the real client re-sends its watch set, with a new
//!   revision, after every tree snapshot -- mirrored here by hand);
//! - after re-subscribing, the first screen that shows the marker reports the
//!   match (an empty grid before the fixture's first line is an honest
//!   negative, never a false match);
//! - the fixture reprints the marker once a second: streaming output that
//!   does not change the answer produces nothing, and
//!   the screen change that removes the marker produces exactly one
//!   observation with the pattern neither matched nor unknown;
//! - closing the pane ends with one observation listing the pattern as
//!   unknown, so a stale match can never outlive its terminal;
//! - the screen text itself never travels inside any of those events.

use std::time::Duration;

use ilium_core::status_rule_expression::{ScreenPattern, ScreenRegion};
use ilium_core::{NodeId, ROOT_ID};
use ilium_ipc::{
    read_frame, write_frame, ClientRequest, NewPaneKind, ServerEvent, StatusScreenObservation,
    StatusScreenWatch,
};
use ilium_server::config::DetectionConfig;
use ilium_test_fixtures::{install, FixtureBehavior};

mod common;
use common::{expect_event, read_initial_state, TestServer};

/// Generous relative to the watcher's 50 ms evaluation floor; bounded so a
/// genuine regression fails in seconds, not minutes.
const WAIT_TIMEOUT: Duration = Duration::from_secs(20);

/// The watch identifier the raw client assigns, as `ilium-client` numbers
/// its distinct enabled patterns from zero.
const WATCH_ID: u16 = 0;

fn marker_watches() -> Vec<StatusScreenWatch> {
    vec![StatusScreenWatch {
        id: WATCH_ID,
        pattern: ScreenPattern {
            // Case-insensitive on purpose: the fixture prints lower-case
            // `esc to interrupt`, so a case-sensitive mismatch here would be
            // a wire-option regression, not a fixture change.
            pattern: "ESC TO INTERRUPT".to_string(),
            ignore_case: true,
            // `vt100` trims trailing blank rows from the visible contents, so
            // the fixture's single printed line is also one of the last three.
            region: ScreenRegion::LastLines(3),
        },
    }]
}

/// Resolves the pane created through the root shortcut (`project -> default
/// -> pane`), following the user-visible tree rather than assuming a group
/// may live directly under the root.
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

fn observation_for(event: &ServerEvent, pane_id: NodeId) -> Option<&StatusScreenObservation> {
    match event {
        ServerEvent::StatusScreenObserved(observation) if observation.pane_id == pane_id => {
            Some(observation)
        }
        _ => None,
    }
}

async fn subscribe(
    client: &mut ilium_transport::SessionStream,
    revision: u64,
    watches: Vec<StatusScreenWatch>,
) {
    write_frame(
        client,
        &ClientRequest::SetStatusScreenWatches { revision, watches },
    )
    .await
    .expect("write SetStatusScreenWatches");
}

#[tokio::test]
async fn status_screen_watch_follows_a_live_pane_through_match_change_and_close() {
    let fixture_dir = tempfile::tempdir().expect("tempdir for the fixture executable");
    let finish_marker = fixture_dir.path().join("finish-marker");
    // A non-agent name keeps the detection loop's own agent events out of
    // this test's stream; the screen watch does not depend on agent identity.
    let fixture = install(
        fixture_dir.path(),
        "screenprinter",
        &FixtureBehavior::WorkingUntilMarker {
            marker_path: finish_marker.clone(),
        },
    );

    let mut server =
        TestServer::start_with_detection_config("screen-watch-live", DetectionConfig::default())
            .await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "screen-watch-live".to_string(),
            hello: Default::default(),
        },
    )
    .await
    .expect("write Attach request");
    read_initial_state(&mut client, Duration::from_secs(5)).await;

    // Revision 1 is subscribed while the session has no pane at all.
    subscribe(&mut client, 1, marker_watches()).await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            request_id: 0,
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(fixture.path.to_string_lossy().to_string()),
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

    // The real client re-subscribes after every tree snapshot; do the same.
    // Revision 1 predates the pane, so no observation may ever carry it (zero
    // watched panes means zero observations). The pane's first line may not
    // have reached the grid yet when revision 2 starts, so an honest negative
    // or a busy-parser Unknown may precede the match; the first screen that
    // shows the marker must report it.
    subscribe(&mut client, 2, marker_watches()).await;
    let early_observations = std::sync::Mutex::new(Vec::new());
    let event = expect_event(&mut client, WAIT_TIMEOUT, |event| {
        let Some(observation) = observation_for(event, pane_id) else {
            return false;
        };
        early_observations.lock().unwrap().push(observation.clone());
        observation.matched == vec![WATCH_ID]
    })
    .await;
    let first = observation_for(&event, pane_id).expect("predicate matched");
    assert_eq!(
        first.revision, 2,
        "observations carry the subscribed revision"
    );
    assert!(first.unknown.is_empty(), "a matched screen was readable");
    let first_generation = first.screen_generation;
    // The predicate closure is gone, so the mutex can be consumed outright
    // (no guard is held across the awaits below).
    let early = early_observations.into_inner().unwrap();
    assert!(
        early.iter().all(|observation| observation.revision == 2),
        "a revision subscribed before the pane existed must not observe it: {early:?}"
    );
    for pair in early.windows(2) {
        assert!(
            pair[0].matched != pair[1].matched || pair[0].unknown != pair[1].unknown,
            "consecutive observations with identical answers: {pair:?}"
        );
    }

    // Release the fixture: it clears the screen and prints a line without the
    // marker. Close the pane as soon as a readable screen reports no match.
    // Everything the server sends for this pane from here to the final
    // Unknown is kept for the assertions below.
    std::fs::write(&finish_marker, b"finish").expect("release the fixture into its final screen");
    let mut later: Vec<StatusScreenObservation> = Vec::new();
    let mut close_sent = false;
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            let event: ServerEvent = read_frame(&mut client).await.expect("read a server event");
            // The screen text never rides along with an observation.
            if let ServerEvent::StatusScreenObserved(observation) = &event {
                let rendered = format!("{observation:?}");
                assert!(
                    !rendered.contains("Cogitating") && !rendered.contains("Done."),
                    "observation must carry only booleans, got {rendered}"
                );
            }
            let Some(observation) = observation_for(&event, pane_id) else {
                continue;
            };
            later.push(observation.clone());
            if !close_sent && observation.unknown.is_empty() && observation.matched.is_empty() {
                write_frame(&mut client, &ClientRequest::ClosePane { pane_id })
                    .await
                    .expect("write ClosePane request");
                close_sent = true;
            }
            if close_sent && observation.unknown.contains(&WATCH_ID) {
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("no final Unknown observation after ClosePane; observations so far: {later:?}")
    });

    // Change-only: no two consecutive observations carry the same answers,
    // whatever the screen generation did in between.
    for pair in later.windows(2) {
        assert!(
            pair[0].matched != pair[1].matched || pair[0].unknown != pair[1].unknown,
            "consecutive observations with identical answers: {pair:?}"
        );
    }
    assert!(later.iter().all(|observation| observation.revision == 2));

    // The clear-then-print transition is two screen changes that give the
    // same answer, so exactly one readable "no match" observation must have
    // been emitted, and it is the last readable one before the close.
    let readable: Vec<&StatusScreenObservation> = later
        .iter()
        .filter(|observation| observation.unknown.is_empty())
        .collect();
    let negatives: Vec<&&StatusScreenObservation> = readable
        .iter()
        .filter(|observation| observation.matched.is_empty())
        .collect();
    assert_eq!(
        negatives.len(),
        1,
        "exactly one readable no-match observation between the match and the close: {later:?}"
    );
    let changed: &StatusScreenObservation = negatives[0];
    assert!(
        readable.last().is_some_and(|last| last.matched.is_empty()),
        "the no-match answer is the last readable screen before the close: {later:?}"
    );
    assert!(
        changed.screen_generation > first_generation,
        "a later screen carries a later generation ({} > {first_generation})",
        changed.screen_generation
    );

    // The terminal's final observation lists the pattern as unknown and never
    // rewinds the generation fence.
    let final_observation = later.last().expect("loop ends on the final observation");
    assert!(final_observation.matched.is_empty());
    assert_eq!(final_observation.unknown, vec![WATCH_ID]);
    assert!(final_observation.screen_generation >= changed.screen_generation);

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn status_screen_watch_refusal_is_a_typed_event_naming_the_revision_and_watch() {
    let mut server = TestServer::start("screen-watch-invalid").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "screen-watch-invalid".to_string(),
            hello: Default::default(),
        },
    )
    .await
    .expect("write Attach request");
    read_initial_state(&mut client, Duration::from_secs(5)).await;

    // One valid watch plus one that cannot compile: the whole revision is
    // refused, and the typed event names the revision and the bad watch.
    subscribe(
        &mut client,
        1,
        vec![
            StatusScreenWatch {
                id: 0,
                pattern: ScreenPattern {
                    pattern: "ok".to_string(),
                    ignore_case: false,
                    region: ScreenRegion::All,
                },
            },
            StatusScreenWatch {
                id: 5,
                pattern: ScreenPattern {
                    pattern: "(unclosed".to_string(),
                    ignore_case: false,
                    region: ScreenRegion::All,
                },
            },
        ],
    )
    .await;
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::StatusScreenWatchesRejected(_))
    })
    .await;
    let ServerEvent::StatusScreenWatchesRejected(rejection) = event else {
        unreachable!("predicate only matches StatusScreenWatchesRejected");
    };
    assert_eq!(rejection.revision, 1);
    assert_eq!(rejection.watch_id, Some(5));
    assert!(
        rejection.message.contains("Invalid screen watch 5"),
        "the rejection explains the offending watch: {}",
        rejection.message
    );

    // A set-wide limit is refused without a watch identifier.
    let too_many: Vec<_> = (0..=16u16)
        .map(|id| StatusScreenWatch {
            id,
            pattern: ScreenPattern {
                pattern: "x".to_string(),
                ignore_case: false,
                region: ScreenRegion::All,
            },
        })
        .collect();
    subscribe(&mut client, 2, too_many).await;
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::StatusScreenWatchesRejected(_))
    })
    .await;
    let ServerEvent::StatusScreenWatchesRejected(rejection) = event else {
        unreachable!("predicate only matches StatusScreenWatchesRejected");
    };
    assert_eq!(rejection.revision, 2);
    assert_eq!(rejection.watch_id, None);

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}
