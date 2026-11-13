//! Bounded, compiled regular-expression watches over visible screen text.
//!
//! A connection subscribes a revisioned set of patterns; the server matches
//! them against each registered terminal pane's visible grid and sends back
//! only per-pattern booleans (`matched` / `unknown`). Screen text never leaves
//! the server through this path, and an unobservable screen is reported as
//! `unknown`, never as a negative match.
//!
//! Text and regions: the matched text is `vt100`'s visible contents -- rows
//! with trailing blanks trimmed, soft-wrapped rows joined with their
//! continuation (so a wrapped message still matches whole), trailing blank
//! rows dropped. `FirstLines(n)` / `LastLines(n)` count those logical lines,
//! so "last line" is the last printed line, not a blank row at the bottom.
//! No scrollback is ever read.
//!
//! Cost and cadence: a pane is evaluated when its screen changes, at most
//! once per `EVALUATION_FLOOR` (50 ms), plus a `BUSY_RETRY_DELAY` retry when
//! the parser lock was busy; an idle pane costs nothing. Each evaluation is
//! bounded by `MAX_SCREEN_WATCHES` linear-time regexes over at most
//! `MAX_SCREEN_BYTES` of text, and only changed answers are sent. Watches are
//! per connection, so two attached clients with watches do the work twice.
//! This is deliberately more responsive than the detection loop's adaptive
//! poll; ARCHITECTURE.md records the deviation.
//!
//! Lifetimes: one `JoinSet` per connection owns the current revision's
//! observer task; replacing the revision aborts the previous one, and closing
//! the connection drops the set. Panes created after a subscription are
//! covered because the client re-sends its watch set, with a new revision, on
//! every tree snapshot (the pane runtime is registered before that snapshot
//! is broadcast).

use std::sync::Arc;

use ilium_core::status_rule_expression::ScreenRegion;
use ilium_core::NodeId;
use ilium_ipc::{ServerEvent, StatusScreenObservation, StatusScreenWatchRejection};
use regex::{Regex, RegexBuilder};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::ipc::direct_events::DirectEventSender;
use crate::state::ServerState;

/// Caps shared with `ilium_core::status_rule_expression::validate` (16
/// distinct enabled patterns, 256 characters) and the client's watch
/// collection; change all three together.
pub const MAX_SCREEN_WATCHES: usize = 16;
pub const MAX_PATTERN_CHARACTERS: usize = 256;
/// Largest visible text (UTF-8 bytes, after vt100 trims trailing blanks of
/// each row and trailing blank rows) still evaluated; larger screens are
/// reported as unknown rather than truncated, because truncation could hide
/// the very evidence a pattern looks for.
pub const MAX_SCREEN_BYTES: usize = 64 * 1024;
/// Largest grid (rows x columns) whose cells are scanned at all. It bounds
/// the preflight scan, not the text: a 600 x 400 pane on a large monitor is
/// still evaluated when its text fits `MAX_SCREEN_BYTES`.
pub const MAX_SCREEN_CELLS: usize = 256 * 1024;
/// Compiled-program and lazy-DFA memory ceilings per pattern; `regex` is a
/// linear-time engine, so these bound memory, not matching time.
const REGEX_SIZE_LIMIT_BYTES: usize = 1024 * 1024;
/// Floor between two evaluations of the same pane while output streams, so
/// regex work is bounded per pane instead of running on every PTY update.
const EVALUATION_FLOOR: std::time::Duration = std::time::Duration::from_millis(50);
/// Retry delay when the parser lock was busy; a real screen change wakes the
/// watcher earlier through the change signal.
const BUSY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(150);
/// Per-revision queue between pane watchers and the connection writer.
const OBSERVATION_QUEUE_CAPACITY: usize = 64;

#[derive(Clone, Debug)]
pub struct ScreenWatchSpec {
    pub id: u16,
    pub pattern: String,
    pub ignore_case: bool,
    pub region: ScreenRegion,
}

impl From<&ilium_ipc::StatusScreenWatch> for ScreenWatchSpec {
    fn from(watch: &ilium_ipc::StatusScreenWatch) -> Self {
        Self {
            id: watch.id,
            pattern: watch.pattern.pattern.clone(),
            ignore_case: watch.pattern.ignore_case,
            region: watch.pattern.region,
        }
    }
}

/// Why a watch set was refused. `watch_id` names the offending watch when
/// the refusal is about one pattern and is `None` for set-wide limits, so the
/// client can show the error next to the rule that caused it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenWatchError {
    pub watch_id: Option<u16>,
    pub message: String,
}

impl ScreenWatchError {
    fn set_wide(message: String) -> Self {
        Self {
            watch_id: None,
            message,
        }
    }

    fn for_watch(watch_id: u16, message: String) -> Self {
        Self {
            watch_id: Some(watch_id),
            message,
        }
    }

    /// The typed wire rejection for the refused revision.
    pub fn into_rejection(self, revision: u64) -> StatusScreenWatchRejection {
        StatusScreenWatchRejection {
            revision,
            watch_id: self.watch_id,
            message: self.message,
        }
    }
}

fn too_many_watches() -> ScreenWatchError {
    ScreenWatchError::set_wide(format!(
        "At most {MAX_SCREEN_WATCHES} screen watches are allowed"
    ))
}

/// Validate a connection's entire watch set before replacing its previous
/// subscription, so an invalid pattern cannot leave a partial subscription.
pub fn compile_wire(
    watches: &[ilium_ipc::StatusScreenWatch],
) -> Result<Vec<ScreenWatch>, ScreenWatchError> {
    // Checked before converting so an oversized set costs no allocation.
    if watches.len() > MAX_SCREEN_WATCHES {
        return Err(too_many_watches());
    }
    let specs: Vec<_> = watches.iter().map(ScreenWatchSpec::from).collect();
    compile(&specs)
}

#[derive(Debug)]
pub struct ScreenWatch {
    id: u16,
    regex: Regex,
    region: ScreenRegion,
}

pub fn compile(watches: &[ScreenWatchSpec]) -> Result<Vec<ScreenWatch>, ScreenWatchError> {
    if watches.len() > MAX_SCREEN_WATCHES {
        return Err(too_many_watches());
    }
    let mut ids = std::collections::HashSet::new();
    watches
        .iter()
        .map(|watch| {
            if !ids.insert(watch.id) {
                return Err(ScreenWatchError::for_watch(
                    watch.id,
                    format!("Duplicate screen watch identifier {}", watch.id),
                ));
            }
            if watch.pattern.chars().count() > MAX_PATTERN_CHARACTERS {
                return Err(ScreenWatchError::for_watch(
                    watch.id,
                    format!(
                        "Screen watch {} exceeds the {MAX_PATTERN_CHARACTERS}-character pattern limit",
                        watch.id
                    ),
                ));
            }
            let regex = RegexBuilder::new(&watch.pattern)
                .case_insensitive(watch.ignore_case)
                .multi_line(true)
                .size_limit(REGEX_SIZE_LIMIT_BYTES)
                .dfa_size_limit(REGEX_SIZE_LIMIT_BYTES)
                .build()
                .map_err(|error| {
                    ScreenWatchError::for_watch(
                        watch.id,
                        format!("Invalid screen watch {}: {error}", watch.id),
                    )
                })?;
            Ok(ScreenWatch {
                id: watch.id,
                regex,
                region: watch.region,
            })
        })
        .collect()
}

/// Match only a caller-provided visible grid. An oversized screen is unknown,
/// not a false match: truncation could otherwise hide the evidence.
pub fn matching_ids(watches: &[ScreenWatch], visible_screen: &str) -> Option<Vec<u16>> {
    if watches.is_empty() {
        return Some(Vec::new());
    }
    if visible_screen.len() > MAX_SCREEN_BYTES {
        return None;
    }
    let lines: Vec<_> = visible_screen.lines().collect();
    Some(
        watches
            .iter()
            .filter_map(|watch| {
                let selected = match watch.region {
                    ScreenRegion::All => std::borrow::Cow::Borrowed(visible_screen),
                    ScreenRegion::FirstLines(count) => {
                        let end = usize::from(count).min(lines.len());
                        std::borrow::Cow::Owned(lines[..end].join("\n"))
                    }
                    ScreenRegion::LastLines(count) => {
                        let start = lines.len().saturating_sub(usize::from(count));
                        std::borrow::Cow::Owned(lines[start..].join("\n"))
                    }
                };
                watch.regex.is_match(&selected).then_some(watch.id)
            })
            .collect(),
    )
}

/// Observe one current-only screen without exposing its contents. The caller
/// owns the pane/session fence and invokes this again after a change signal.
/// A busy or oversized screen invalidates all answers rather than turning an
/// unavailable match into a negative result.
pub fn observe_screen(
    reader: &ilium_pty::ScreenReader,
    pane_id: NodeId,
    revision: u64,
    last_generation: u64,
    watches: &[ScreenWatch],
) -> (StatusScreenObservation, bool) {
    let snapshot = reader.try_text_with_generation_and_limits(MAX_SCREEN_CELLS, MAX_SCREEN_BYTES);
    let busy = snapshot.is_none();
    let (screen_generation, matches) = match snapshot {
        Some((generation, Ok(text))) => (generation, matching_ids(watches, &text)),
        Some((generation, Err(_))) => (generation, None),
        None => (last_generation, None),
    };
    let (matched, unknown) = match matches {
        Some(matched) => (matched, Vec::new()),
        None => (Vec::new(), watches.iter().map(|watch| watch.id).collect()),
    };
    (
        StatusScreenObservation {
            pane_id,
            revision,
            screen_generation,
            matched,
            unknown,
        },
        busy,
    )
}

/// Change-only emission compares results, not screen generations: a pane
/// that streams output without changing any answer must not flood the
/// connection with identical observations. Generations stay monotonic across
/// the observations that are sent, so the client's ordering fence holds.
pub fn results_changed(
    previous: Option<&StatusScreenObservation>,
    next: &StatusScreenObservation,
) -> bool {
    previous
        .is_none_or(|previous| previous.matched != next.matched || previous.unknown != next.unknown)
}

/// The observation a closed or replaced terminal leaves behind: every watch
/// unknown, so a stale match cannot stay visible while the client waits for
/// a fresh tree subscription.
fn all_unknown(
    pane_id: NodeId,
    revision: u64,
    screen_generation: u64,
    watches: &[ScreenWatch],
) -> StatusScreenObservation {
    StatusScreenObservation {
        pane_id,
        revision,
        screen_generation,
        matched: Vec::new(),
        unknown: watches.iter().map(|watch| watch.id).collect(),
    }
}

/// One connection-owned pane subscription. The caller owns and aborts this
/// future when replacing the revision or closing the connection. Subscribing
/// before the first read avoids losing a change between snapshot and wait;
/// the session identity fence prevents a replacement pane from inheriting
/// observations from its predecessor.
pub async fn watch_pane(
    state: Arc<ServerState>,
    pane_id: NodeId,
    revision: u64,
    watches: Arc<Vec<ScreenWatch>>,
    sender: mpsc::Sender<StatusScreenObservation>,
) {
    let Some((input, reader, mut changed)) =
        crate::pane::current_terminal_reader(&state, pane_id).await
    else {
        return;
    };
    let mut status = input.subscribe_status();
    let mut generation = 0;
    let mut last_observation: Option<StatusScreenObservation> = None;
    loop {
        if !crate::pane::terminal_reader_is_current(&state, pane_id, &input).await {
            break;
        }
        let (observation, busy) = observe_screen(&reader, pane_id, revision, generation, &watches);
        if !crate::pane::terminal_reader_is_current(&state, pane_id, &input).await {
            break;
        }
        generation = observation.screen_generation;
        if results_changed(last_observation.as_ref(), &observation) {
            last_observation = Some(observation.clone());
            if sender.send(observation).await.is_err() {
                break;
            }
        }
        // A watch receiver retains the newest change while output is busy.
        // Limit screen formatting and regex work per pane instead of matching
        // on every PTY update, then observe the latest grid on the next pass.
        tokio::select! {
            () = tokio::time::sleep(EVALUATION_FLOOR) => {},
            () = sender.closed() => break,
        }
        tokio::select! {
            result = changed.changed() => if result.is_err() { break; },
            result = status.changed() => if result.is_err() { break; },
            // A parser lock may become available without a new screen event.
            // Retry only that temporary case; oversized screens wait for a
            // real change instead of repeatedly formatting the same grid.
            () = tokio::time::sleep(BUSY_RETRY_DELAY), if busy => {},
            () = sender.closed() => break,
        }
    }
    // The connection revision fences this observation if a new watch set
    // has already replaced this task.
    let _ = sender
        .send(all_unknown(pane_id, revision, generation, &watches))
        .await;
}

/// Replace a connection's screen-watch revision. The previous revision's
/// observer is aborted first so its late observations cannot be attributed
/// to the new rules. An empty set spawns nothing: zero watches cost zero
/// work on the server. Returns whether an observer task now runs.
pub(crate) fn replace_revision(
    observers: &mut JoinSet<()>,
    state: Arc<ServerState>,
    revision: u64,
    compiled: Vec<ScreenWatch>,
    replies: DirectEventSender,
) -> bool {
    observers.abort_all();
    while observers.try_join_next().is_some() {}
    if compiled.is_empty() {
        return false;
    }
    observers.spawn(observe_registered_panes(
        state,
        revision,
        Arc::new(compiled),
        replies,
    ));
    true
}

/// Watch every terminal pane registered at subscription time for one
/// revision and forward each pane's change-only observations to the
/// connection. Dropping this future (abort on replacement or disconnect)
/// drops the per-pane watchers with it.
async fn observe_registered_panes(
    state: Arc<ServerState>,
    revision: u64,
    watches: Arc<Vec<ScreenWatch>>,
    replies: DirectEventSender,
) {
    let pane_ids: Vec<NodeId> = state.panes.read().await.keys().copied().collect();
    let (sender, mut receiver) = mpsc::channel(OBSERVATION_QUEUE_CAPACITY);
    let mut pane_watchers = JoinSet::new();
    for pane_id in pane_ids {
        pane_watchers.spawn(watch_pane(
            Arc::clone(&state),
            pane_id,
            revision,
            Arc::clone(&watches),
            sender.clone(),
        ));
    }
    // Only the watchers hold senders now, so the loop ends once every pane
    // watcher has finished (pane closed or replaced).
    drop(sender);
    while let Some(observation) = receiver.recv().await {
        if replies
            .send(ServerEvent::StatusScreenObserved(observation))
            .await
            .is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compile, matching_ids, replace_revision, results_changed, ScreenRegion, ScreenWatchSpec,
        StatusScreenObservation,
    };
    use ilium_core::NodeId;
    use std::sync::Arc;

    fn spec(id: u16, pattern: &str, ignore_case: bool, region: ScreenRegion) -> ScreenWatchSpec {
        ScreenWatchSpec {
            id,
            pattern: pattern.into(),
            ignore_case,
            region,
        }
    }

    #[test]
    fn screen_watches_match_only_requested_visible_lines() {
        let watches = compile(&[
            spec(1, "error", true, ScreenRegion::LastLines(1)),
            spec(2, "error", true, ScreenRegion::FirstLines(1)),
        ])
        .expect("valid patterns");
        assert_eq!(
            matching_ids(&watches, "Ready\nERROR: failed"),
            Some(vec![1])
        );
        assert_eq!(matching_ids(&watches, "Ready\nDone"), Some(vec![]));
    }

    #[test]
    fn screen_watch_matches_change_with_the_visible_grid() {
        let watches = compile(&[
            spec(1, "^ERROR", false, ScreenRegion::All),
            spec(2, "ERROR", false, ScreenRegion::LastLines(1)),
        ])
        .expect("valid patterns");
        assert_eq!(
            matching_ids(&watches, "Ready\nERROR: failed"),
            Some(vec![1, 2])
        );
        // A later screen must replace, rather than accumulate, prior matches.
        assert_eq!(
            matching_ids(&watches, "ERROR: failed\nRecovered"),
            Some(vec![1])
        );
        assert_eq!(matching_ids(&watches, "Ready\nRecovered"), Some(vec![]));
    }

    #[test]
    fn regions_wider_than_the_screen_and_empty_screens_are_safe() {
        let watches = compile(&[
            spec(1, "x", false, ScreenRegion::FirstLines(50)),
            spec(2, "x", false, ScreenRegion::LastLines(50)),
            spec(3, "^$", false, ScreenRegion::All),
        ])
        .expect("valid patterns");
        assert_eq!(matching_ids(&watches, "a\nx"), Some(vec![1, 2]));
        // An empty grid matches only the pattern that accepts emptiness.
        assert_eq!(matching_ids(&watches, ""), Some(vec![3]));
    }

    #[test]
    fn invalid_patterns_and_duplicate_ids_are_rejected() {
        let invalid = spec(1, "(", false, ScreenRegion::All);
        assert!(compile(&[invalid]).is_err());
        let valid = spec(1, "ok", false, ScreenRegion::All);
        assert!(compile(&[valid.clone(), valid]).is_err());
        assert_eq!(matching_ids(&[], "text"), Some(vec![]));
        let watches = compile(&[spec(3, "x", false, ScreenRegion::All)]).expect("valid watch");
        assert_eq!(
            matching_ids(&watches, &"x".repeat(super::MAX_SCREEN_BYTES + 1)),
            None
        );
    }

    #[test]
    fn refusals_name_the_offending_watch_or_the_whole_set() {
        let error = compile(&[
            spec(2, "fine", false, ScreenRegion::All),
            spec(7, "(", false, ScreenRegion::All),
        ])
        .expect_err("an unclosed group cannot compile");
        assert_eq!(error.watch_id, Some(7));
        assert!(error.message.contains("Invalid screen watch 7"));

        let long_pattern = "a".repeat(super::MAX_PATTERN_CHARACTERS + 1);
        let error =
            compile(&[spec(3, &long_pattern, false, ScreenRegion::All)]).expect_err("too long");
        assert_eq!(error.watch_id, Some(3));

        let duplicate = spec(4, "ok", false, ScreenRegion::All);
        let error = compile(&[duplicate.clone(), duplicate]).expect_err("duplicate id");
        assert_eq!(error.watch_id, Some(4));

        let too_many: Vec<_> = (0..=super::MAX_SCREEN_WATCHES as u16)
            .map(|id| spec(id, "x", false, ScreenRegion::All))
            .collect();
        let error = compile(&too_many).expect_err("too many watches");
        assert_eq!(error.watch_id, None);

        let rejection = error.into_rejection(12);
        assert_eq!(rejection.revision, 12);
        assert_eq!(rejection.watch_id, None);
        assert!(rejection.message.contains("16"));
    }

    #[test]
    fn regions_count_logical_lines_so_soft_wraps_do_not_split_a_match() {
        // `vt100` joins a soft-wrapped row with its continuation and drops
        // trailing blank rows; the observer matches that logical text, so a
        // wrapped message still matches whole and the last line is the last
        // printed one, not a blank row at the bottom of the grid.
        let mut parser = vt100::Parser::new(4, 10, 0);
        parser.process(b"first\r\nERROR: build failed");
        let text = parser.screen().contents();
        let watches = compile(&[
            spec(1, "build failed", false, ScreenRegion::LastLines(1)),
            spec(2, "first", false, ScreenRegion::FirstLines(1)),
            spec(3, "first", false, ScreenRegion::LastLines(1)),
        ])
        .expect("valid patterns");
        assert_eq!(matching_ids(&watches, &text), Some(vec![1, 2]));
    }

    #[test]
    fn pattern_length_and_watch_count_caps_are_enforced() {
        let long_pattern = "a".repeat(super::MAX_PATTERN_CHARACTERS + 1);
        assert!(compile(&[spec(1, &long_pattern, false, ScreenRegion::All)]).is_err());
        let at_limit = "a".repeat(super::MAX_PATTERN_CHARACTERS);
        assert!(compile(&[spec(1, &at_limit, false, ScreenRegion::All)]).is_ok());
        let too_many: Vec<_> = (0..=super::MAX_SCREEN_WATCHES as u16)
            .map(|id| spec(id, "x", false, ScreenRegion::All))
            .collect();
        assert!(compile(&too_many).is_err());
        assert!(compile(&too_many[..super::MAX_SCREEN_WATCHES]).is_ok());
        // Nested repetition that would explode a backtracking engine compiles
        // (or is refused by the size limit) but never hangs matching.
        let watches =
            compile(&[spec(1, "(a+)+$", false, ScreenRegion::All)]).expect("linear engine");
        assert_eq!(
            matching_ids(&watches, &format!("{}b", "a".repeat(5000))),
            Some(vec![])
        );
    }

    #[test]
    fn wire_watches_keep_region_and_case_options() {
        use ilium_core::status_rule_expression::ScreenPattern;
        let watches = super::compile_wire(&[ilium_ipc::StatusScreenWatch {
            id: 17,
            pattern: ScreenPattern {
                pattern: "^error".into(),
                ignore_case: true,
                region: ScreenRegion::LastLines(1),
            },
        }])
        .unwrap();
        assert_eq!(matching_ids(&watches, "error\nREADY"), Some(vec![]));
        assert_eq!(matching_ids(&watches, "ready\nERROR here"), Some(vec![17]));
    }

    #[test]
    fn wire_watch_count_cap_is_enforced_before_compiling() {
        use ilium_core::status_rule_expression::ScreenPattern;
        let watches: Vec<_> = (0..=super::MAX_SCREEN_WATCHES as u16)
            .map(|id| ilium_ipc::StatusScreenWatch {
                id,
                pattern: ScreenPattern {
                    pattern: "x".into(),
                    ignore_case: false,
                    region: ScreenRegion::All,
                },
            })
            .collect();
        assert!(super::compile_wire(&watches).is_err());
        assert!(super::compile_wire(&[]).unwrap().is_empty());
    }

    fn observation(generation: u64, matched: &[u16], unknown: &[u16]) -> StatusScreenObservation {
        StatusScreenObservation {
            pane_id: NodeId(4),
            revision: 1,
            screen_generation: generation,
            matched: matched.to_vec(),
            unknown: unknown.to_vec(),
        }
    }

    #[test]
    fn observations_are_emitted_only_when_results_change() {
        let first = observation(1, &[0], &[]);
        assert!(results_changed(None, &first));
        // Streaming output advances the generation without changing answers.
        assert!(!results_changed(Some(&first), &observation(9, &[0], &[])));
        assert!(results_changed(Some(&first), &observation(10, &[], &[])));
        assert!(results_changed(Some(&first), &observation(11, &[], &[0])));
        let unknown = observation(12, &[], &[0]);
        assert!(!results_changed(
            Some(&unknown),
            &observation(13, &[], &[0])
        ));
    }

    #[test]
    fn all_unknown_lists_every_watch_and_no_match() {
        let watches = compile(&[
            spec(3, "a", false, ScreenRegion::All),
            spec(9, "b", false, ScreenRegion::All),
        ])
        .expect("valid patterns");
        let final_observation = super::all_unknown(NodeId(2), 5, 7, &watches);
        assert_eq!(final_observation.pane_id, NodeId(2));
        assert_eq!(final_observation.revision, 5);
        assert_eq!(final_observation.screen_generation, 7);
        assert!(final_observation.matched.is_empty());
        assert_eq!(final_observation.unknown, vec![3, 9]);
    }

    #[tokio::test]
    async fn an_empty_watch_set_spawns_no_observer_and_aborts_the_previous_one() {
        let directory = tempfile::tempdir().expect("tempdir");
        let state = Arc::new(crate::state::test_server_state(&directory));
        let (replies, mut reply_events) = crate::ipc::direct_events::DirectEventSender::channel(8);
        let mut observers = tokio::task::JoinSet::new();

        // A populated revision spawns exactly one observer even with no panes
        // registered; it then idles with nothing to report.
        let watches = compile(&[spec(0, "x", false, ScreenRegion::All)]).expect("valid watch");
        assert!(replace_revision(
            &mut observers,
            Arc::clone(&state),
            1,
            watches,
            replies.clone()
        ));
        assert_eq!(observers.len(), 1);

        // Replacing it with an empty set is zero work: the previous observer
        // is aborted and nothing new is spawned, so draining the set yields at
        // most that one task (finished or cancelled, never a running one).
        assert!(!replace_revision(
            &mut observers,
            Arc::clone(&state),
            2,
            Vec::new(),
            replies.clone()
        ));
        let mut drained = 0;
        while let Some(result) = observers.join_next().await {
            drained += 1;
            assert!(
                result.is_ok() || result.as_ref().is_err_and(|error| error.is_cancelled()),
                "the replaced observer may only finish or be cancelled: {result:?}"
            );
        }
        assert!(drained <= 1, "an empty set must not spawn an observer");
        assert!(observers.is_empty());
        // `replies` is still held by this test, so a closed channel cannot pass
        // for silence: the receiver must genuinely time out.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), reply_events.recv())
                .await
                .is_err(),
            "no pane means no observation, and an empty set must never report"
        );
        drop(replies);
    }
}
