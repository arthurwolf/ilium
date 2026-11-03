//! Bounded, compiled regular-expression watches over visible screen text.
//! The caller owns watch lifetimes and must never send the screen itself to a
//! client as part of a match result.

use regex::{Regex, RegexBuilder};
use ilium_core::status_rule_expression::ScreenRegion;

pub const MAX_SCREEN_WATCHES: usize = 16;
pub const MAX_PATTERN_CHARACTERS: usize = 256;
pub const MAX_SCREEN_BYTES: usize = 64 * 1024;

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

/// Validate a connection's entire watch set before replacing its previous
/// subscription, so an invalid pattern cannot leave a partial subscription.
pub fn compile_wire(watches: &[ilium_ipc::StatusScreenWatch]) -> Result<Vec<ScreenWatch>, String> {
    if watches.len() > MAX_SCREEN_WATCHES {
        return Err(format!("At most {MAX_SCREEN_WATCHES} screen watches are allowed"));
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

pub fn compile(watches: &[ScreenWatchSpec]) -> Result<Vec<ScreenWatch>, String> {
    if watches.len() > MAX_SCREEN_WATCHES {
        return Err(format!("At most {MAX_SCREEN_WATCHES} screen watches are allowed"));
    }
    let mut ids = std::collections::HashSet::new();
    watches.iter().map(|watch| {
        if !ids.insert(watch.id) {
            return Err(format!("Duplicate screen watch identifier {}", watch.id));
        }
        if watch.pattern.chars().count() > MAX_PATTERN_CHARACTERS {
            return Err(format!("Screen watch {} exceeds the pattern length limit", watch.id));
        }
        let regex = RegexBuilder::new(&watch.pattern)
            .case_insensitive(watch.ignore_case)
            .multi_line(true)
            .size_limit(1024 * 1024)
            .dfa_size_limit(1024 * 1024)
            .build()
            .map_err(|error| format!("Invalid screen watch {}: {error}", watch.id))?;
        Ok(ScreenWatch { id: watch.id, regex, region: watch.region })
    }).collect()
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
    let screen = visible_screen;
    let lines: Vec<_> = screen.lines().collect();
    Some(watches.iter().filter_map(|watch| {
        let selected = match watch.region {
            ScreenRegion::All => std::borrow::Cow::Borrowed(screen),
            ScreenRegion::FirstLines(count) => std::borrow::Cow::Owned(lines[..usize::from(count).min(lines.len())].join("\n")),
            ScreenRegion::LastLines(count) => std::borrow::Cow::Owned(lines[lines.len().saturating_sub(usize::from(count))..].join("\n")),
        };
        watch.regex.is_match(&selected).then_some(watch.id)
    }).collect())
}

/// Observe one current-only screen without exposing its contents. The caller
/// owns the pane/session fence and invokes this again after a change signal.
/// A busy or oversized screen invalidates all answers rather than turning an
/// unavailable match into a negative result.
pub fn observe_screen(
    reader: &ilium_pty::ScreenReader,
    pane_id: ilium_core::NodeId,
    revision: u64,
    last_generation: u64,
    watches: &[ScreenWatch],
) -> (ilium_ipc::StatusScreenObservation, bool) {
    let snapshot = reader.try_text_with_generation_and_limit(MAX_SCREEN_BYTES);
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
    (ilium_ipc::StatusScreenObservation { pane_id, revision, screen_generation, matched, unknown }, busy)
}

/// One connection-owned pane subscription. The caller owns and aborts this
/// future when replacing the revision or closing the connection. Subscribing
/// before the first read avoids losing a change between snapshot and wait;
/// the session identity fence prevents a replacement pane from inheriting
/// observations from its predecessor.
pub async fn watch_pane(
    state: std::sync::Arc<crate::state::ServerState>,
    pane_id: ilium_core::NodeId,
    revision: u64,
    watches: std::sync::Arc<Vec<ScreenWatch>>,
    sender: tokio::sync::mpsc::Sender<ilium_ipc::StatusScreenObservation>,
) {
    let Some((input, reader, mut changed)) = crate::pane::current_terminal_reader(&state, pane_id).await else { return; };
    let mut status = input.subscribe_status();
    let mut generation = 0;
    let mut last_observation = None;
    loop {
        if !crate::pane::terminal_reader_is_current(&state, pane_id, &input).await { break; }
        let (observation, busy) = observe_screen(&reader, pane_id, revision, generation, &watches);
        if !crate::pane::terminal_reader_is_current(&state, pane_id, &input).await { break; }
        generation = observation.screen_generation;
        if last_observation.as_ref() != Some(&observation) {
            last_observation = Some(observation.clone());
            if sender.send(observation).await.is_err() { break; }
        }
        // A watch receiver retains the newest change while output is busy.
        // Limit screen formatting and regex work per pane instead of matching
        // on every PTY update, then observe the latest grid on the next pass.
        tokio::select! {
            () = tokio::time::sleep(std::time::Duration::from_millis(50)) => {},
            () = sender.closed() => break,
        }
        tokio::select! {
            result = changed.changed() => if result.is_err() { break; },
            result = status.changed() => if result.is_err() { break; },
            // A parser lock may become available without a new screen event.
            // Retry only that temporary case; oversized screens wait for a
            // real change instead of repeatedly formatting the same grid.
            () = tokio::time::sleep(std::time::Duration::from_millis(150)), if busy => {},
            () = sender.closed() => break,
        }
    }
    // A closed or replaced terminal must not leave its last successful
    // match visible while the client waits for a fresh tree subscription.
    // The connection revision fences this observation if a new watch set
    // has already replaced this task.
    let _ = sender.send(ilium_ipc::StatusScreenObservation {
        pane_id,
        revision,
        screen_generation: generation,
        matched: Vec::new(),
        unknown: watches.iter().map(|watch| watch.id).collect(),
    }).await;
}

#[cfg(test)]
mod tests {
    use super::{ScreenRegion, ScreenWatchSpec, compile, matching_ids};

    #[test]
    fn screen_watches_match_only_requested_visible_lines() {
        let watches = compile(&[
            ScreenWatchSpec { id: 1, pattern: "error".into(), ignore_case: true, region: ScreenRegion::LastLines(1) },
            ScreenWatchSpec { id: 2, pattern: "error".into(), ignore_case: true, region: ScreenRegion::FirstLines(1) },
        ]).expect("valid patterns");
        assert_eq!(matching_ids(&watches, "Ready\nERROR: failed"), Some(vec![1]));
        assert_eq!(matching_ids(&watches, "Ready\nDone"), Some(vec![]));
    }

    #[test]
    fn screen_watch_matches_change_with_the_visible_grid() {
        let watches = compile(&[
            ScreenWatchSpec { id: 1, pattern: "^ERROR".into(), ignore_case: false, region: ScreenRegion::All },
            ScreenWatchSpec { id: 2, pattern: "ERROR".into(), ignore_case: false, region: ScreenRegion::LastLines(1) },
        ]).expect("valid patterns");
        assert_eq!(matching_ids(&watches, "Ready\nERROR: failed"), Some(vec![1, 2]));
        // A later screen must replace, rather than accumulate, prior matches.
        assert_eq!(matching_ids(&watches, "ERROR: failed\nRecovered"), Some(vec![1]));
        assert_eq!(matching_ids(&watches, "Ready\nRecovered"), Some(vec![]));
    }

    #[test]
    fn invalid_patterns_and_duplicate_ids_are_rejected() {
        let invalid = ScreenWatchSpec { id: 1, pattern: "(".into(), ignore_case: false, region: ScreenRegion::All };
        assert!(compile(&[invalid]).is_err());
        let valid = ScreenWatchSpec { id: 1, pattern: "ok".into(), ignore_case: false, region: ScreenRegion::All };
        assert!(compile(&[valid.clone(), valid]).is_err());
        assert_eq!(matching_ids(&[], "text"), Some(vec![]));
        let watches = compile(&[ScreenWatchSpec { id: 3, pattern: "x".into(), ignore_case: false, region: ScreenRegion::All }]).expect("valid watch");
        assert_eq!(matching_ids(&watches, &"x".repeat(super::MAX_SCREEN_BYTES + 1)), None);
    }

    #[test]
    fn wire_watches_keep_region_and_case_options() {
        use ilium_core::status_rule_expression::ScreenPattern;
        let watches = super::compile_wire(&[ilium_ipc::StatusScreenWatch {
            id: 17,
            pattern: ScreenPattern { pattern: "^error".into(), ignore_case: true, region: ScreenRegion::LastLines(1) },
        }]).unwrap();
        assert_eq!(matching_ids(&watches, "error\nREADY"), Some(vec![]));
        assert_eq!(matching_ids(&watches, "ready\nERROR here"), Some(vec![17]));
    }
}
