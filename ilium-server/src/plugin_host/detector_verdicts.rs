//! The kernel side of plugin-owned agent classification (capability-agent-signatures.md,
//! HST-450 to HST-455).
//!
//! The detection loop never waits on a detector. For every identified agent pane it hands the
//! current screen to [`DetectorVerdicts::request`] and reads the newest cached verdict back with
//! [`DetectorVerdicts::latest`]. One coalesced job on the plugin runtime thread drains the pending
//! screens through `detector.classify-screen`, one call per pane (HST-451), at most
//! [`MAX_SCREENS_PER_ROUND`] per round starting at a rotating pane, within the 50 ms budget each
//! detection tick shares over all its jobs and rounds (HST-453); screens the budget did not reach
//! are first in line next tick. It stores
//! what came back. A pane without a verdict yet, or
//! whose detector missed its deadline, keeps its previous activity (HST-454): a missing verdict
//! is "no sample", never idle.
//!
//! The verdict is translated into the kernel's own evidence types here, so the detection loop
//! applies a plugin verdict through the same goal-ownership and completion rules as the
//! built-in table, which still classifies when no detector plugin is active.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use ilium_core::{AgentClass, AgentTurn, GoalState};
use ilium_detect::{
    ActivityClassification, ActivityEvidence, GoalClassification, GoalEvidence, GoalEvidenceRule,
};
use ilium_plugin_api::generated::exports::detector::{
    ActivityEvidence as WireEvidence, AgentTurn as WireTurn, GoalEvidence as WireGoal, ScreenCell,
    ScreenRequest, ScreenVerdict,
};
use ilium_plugin_api::Tier;
use ilium_plugin_host::runtime::detectors::{DetectorBatch, DETECTOR_CYCLE_BUDGET};

/// Screens one round serves at most, one `classify-screen` call each.
pub(crate) const MAX_SCREENS_PER_ROUND: usize = 32;

/// Dimmed cells forwarded per screen; the agent-detect plugin skips a screen with more.
const MAX_DIMMED_CELLS: usize = 64 * 1024;

/// Distinct verdict strings the kernel keeps as `&'static str` (goal patterns, interstitial
/// keys). Past the cap a new string is dropped, so a misbehaving detector cannot grow memory.
const MAX_INTERNED: usize = 256;

/// One cached verdict and whether its detector is first party.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CachedVerdict {
    pub(crate) verdict: ScreenVerdict,
    pub(crate) first_party: bool,
}

#[derive(Default)]
struct Book {
    /// The newest screen of each pane that no detector has seen yet.
    pending: BTreeMap<u64, ScreenRequest>,
    /// The newest verdict of each pane.
    latest: BTreeMap<u64, CachedVerdict>,
    /// The pane after which the next batch starts (HST-453).
    cursor: u64,
    /// The first pane a spent budget left unserved: the next batch starts there.
    first_in_line: Option<u64>,
    /// Detector call time spent in the current detection tick, over every job and round.
    spent: Duration,
}

/// Per-pane detector verdicts and the coalesced classification job.
#[derive(Default)]
pub(crate) struct DetectorVerdicts {
    book: Mutex<Book>,
    /// A classification job is queued or running.
    in_flight: AtomicBool,
    /// At least one detector plugin is active; refreshed after every runtime job.
    active: AtomicBool,
}

impl DetectorVerdicts {
    /// Whether a detector plugin classifies; false means the built-in table does.
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Records whether a detector plugin is active. When the last one stops, every cached
    /// verdict is dropped so a later detector never sees stale answers.
    pub(crate) fn set_active(&self, active: bool) {
        if !self.active.swap(active, Ordering::AcqRel) || active {
            return;
        }
        if let Ok(mut book) = self.book.lock() {
            book.pending.clear();
            book.latest.clear();
        }
    }

    /// Queues a pane's screen, replacing an older one of the same pane. True when the caller
    /// must schedule the classification job (none is queued yet).
    pub(crate) fn request(&self, screen: ScreenRequest) -> bool {
        let Ok(mut book) = self.book.lock() else {
            return false;
        };
        let pane = screen.pane;
        let already_answered = book.latest.get(&pane).is_some_and(|cached| {
            cached.verdict.generation == screen.generation
                && cached.verdict.signature == screen.signature
        });
        if already_answered {
            return false;
        }
        book.pending.insert(pane, screen);
        drop(book);
        !self.in_flight.swap(true, Ordering::AcqRel)
    }

    /// Takes the next batch, at most [`MAX_SCREENS_PER_ROUND`] screens starting at the pane a
    /// spent budget left unserved, or else after the pane the previous batch ended on, and
    /// wrapping around.
    pub(crate) fn take_batch(&self) -> Vec<ScreenRequest> {
        let Ok(mut book) = self.book.lock() else {
            return Vec::new();
        };
        let start = match book.first_in_line.take() {
            Some(pane) => pane,
            None => book.cursor.saturating_add(1),
        };
        let panes: Vec<u64> = book
            .pending
            .range(start..)
            .chain(book.pending.range(..start))
            .map(|(pane, _)| *pane)
            .take(MAX_SCREENS_PER_ROUND)
            .collect();
        if let Some(last) = panes.last() {
            book.cursor = *last;
        }
        panes
            .into_iter()
            .filter_map(|pane| book.pending.remove(&pane))
            .collect()
    }

    /// Stores the verdicts of one round and puts the screens it did not reach back in line,
    /// unless a newer screen of the same pane arrived meanwhile. True when the cycle budget was
    /// spent: the job then ends the cycle with [`Self::end_cycle`].
    pub(crate) fn store(&self, batch: DetectorBatch) -> bool {
        let Ok(mut book) = self.book.lock() else {
            return false;
        };
        let budget_spent = !batch.unserved.is_empty();
        if let Some(first) = batch.unserved.first() {
            book.first_in_line = Some(first.pane);
        }
        for screen in batch.unserved {
            book.pending.entry(screen.pane).or_insert(screen);
        }
        for answer in batch.verdicts {
            let pane = answer.verdict.pane;
            let newer = book
                .latest
                .get(&pane)
                .is_none_or(|cached| cached.verdict.generation <= answer.verdict.generation);
            if newer {
                book.latest.insert(
                    pane,
                    CachedVerdict {
                        verdict: answer.verdict,
                        first_party: answer.tier == Tier::T1,
                    },
                );
            }
        }
        budget_spent
    }

    /// Clears the in-flight mark once the job drained everything; true when screens arrived
    /// meanwhile and the job must run another round.
    pub(crate) fn finish_round(&self) -> bool {
        let has_pending = self
            .book
            .lock()
            .map(|book| !book.pending.is_empty())
            .unwrap_or(false);
        if has_pending {
            return true;
        }
        self.in_flight.store(false, Ordering::Release);
        // A request that raced the store above saw `in_flight` set and did not schedule.
        let raced = self
            .book
            .lock()
            .map(|book| !book.pending.is_empty())
            .unwrap_or(false);
        raced && !self.in_flight.swap(true, Ordering::AcqRel)
    }

    /// Releases the in-flight mark: the cycle budget is spent, or the host stopped. The screens
    /// still pending go to the job the next request schedules.
    pub(crate) fn end_cycle(&self) {
        self.in_flight.store(false, Ordering::Release);
    }

    /// Starts a detection tick: the detector call budget is whole again (HST-453).
    pub(crate) fn start_cycle(&self) {
        if let Ok(mut book) = self.book.lock() {
            book.spent = Duration::ZERO;
        }
    }

    /// Detector call time the current tick may still spend; zero ends every job of the tick.
    pub(crate) fn budget_left(&self) -> Duration {
        self.book
            .lock()
            .map(|book| DETECTOR_CYCLE_BUDGET.saturating_sub(book.spent))
            .unwrap_or(Duration::ZERO)
    }

    /// Counts detector call time against the current tick.
    pub(crate) fn spend(&self, elapsed: Duration) {
        if let Ok(mut book) = self.book.lock() {
            book.spent = book.spent.saturating_add(elapsed);
        }
    }

    /// The newest verdict of a pane for an agent signature.
    pub(crate) fn latest(&self, pane: u64, signature: &str) -> Option<CachedVerdict> {
        let book = self.book.lock().ok()?;
        book.latest
            .get(&pane)
            .filter(|cached| cached.verdict.signature == signature)
            .cloned()
    }

    /// Forgets the panes that are no longer agent panes.
    pub(crate) fn retain_panes(&self, live: &BTreeSet<u64>) {
        if let Ok(mut book) = self.book.lock() {
            book.pending.retain(|pane, _| live.contains(pane));
            book.latest.retain(|pane, _| live.contains(pane));
        }
    }
}

/// The signature id the detector export knows an agent class by.
pub(crate) fn signature_of_class(class: &AgentClass) -> String {
    match class {
        AgentClass::Claude => "claude".to_owned(),
        AgentClass::Codex => "codex".to_owned(),
        AgentClass::Antigravity => "antigravity".to_owned(),
        AgentClass::Other(name) => name.clone(),
    }
}

/// The `screen-request` of one identified pane's snapshot.
pub(crate) fn screen_request(
    pane: u64,
    class: &AgentClass,
    snapshot: &ilium_pty::ScreenSnapshot,
) -> ScreenRequest {
    let (row, column) = snapshot.cursor_position;
    ScreenRequest {
        pane,
        generation: snapshot.generation,
        signature: signature_of_class(class),
        text: snapshot.text.clone(),
        cursor: Some(ScreenCell { row, column }),
        dimmed: snapshot
            .dimmed_cells
            .iter()
            .take(MAX_DIMMED_CELLS)
            .map(|&(row, column)| ScreenCell { row, column })
            .collect(),
    }
}

/// The kernel evidence a verdict carries, ready for the same goal-ownership reduction the
/// built-in table feeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerdictEvidence {
    pub(crate) activity: ActivityClassification,
    pub(crate) goal: GoalClassification,
    pub(crate) is_fresh_agent_screen: bool,
    pub(crate) interstitial_prompt_response: Option<&'static str>,
}

/// Translates a cached verdict. An interstitial response, which the loop types into the pane,
/// is honoured only from a first-party detector.
pub(crate) fn evidence_of(cached: &CachedVerdict) -> VerdictEvidence {
    let verdict = &cached.verdict;
    VerdictEvidence {
        activity: ActivityClassification {
            turn: turn_of(verdict.turn),
            evidence: activity_evidence_of(verdict.evidence),
            matched_line: verdict.evidence_line.clone(),
        },
        goal: GoalClassification {
            evidence: goal_of(verdict.goal),
            matched_line: verdict.goal_line.clone(),
            matched_rule: verdict.goal_rule.as_deref().and_then(goal_rule_of),
            matched_pattern: verdict.goal_pattern.as_deref().and_then(intern),
        },
        is_fresh_agent_screen: verdict.fresh_screen,
        interstitial_prompt_response: cached
            .first_party
            .then(|| verdict.interstitial_response.as_deref().and_then(intern))
            .flatten(),
    }
}

fn turn_of(turn: WireTurn) -> AgentTurn {
    match turn {
        WireTurn::Working => AgentTurn::Working,
        WireTurn::WaitingApproval => AgentTurn::WaitingApproval,
        WireTurn::WaitingSubagents => AgentTurn::WaitingSubagents,
        WireTurn::Settling => AgentTurn::Settling,
        WireTurn::Idle => AgentTurn::Idle,
    }
}

fn activity_evidence_of(evidence: WireEvidence) -> ActivityEvidence {
    match evidence {
        WireEvidence::InterruptMarker => ActivityEvidence::InterruptMarker,
        WireEvidence::GenericLiveStatus => ActivityEvidence::GenericLiveStatus,
        WireEvidence::ClaudeLiveStatus => ActivityEvidence::ClaudeLiveStatus,
        WireEvidence::CodexLiveStatus => ActivityEvidence::CodexLiveStatus,
        WireEvidence::BackgroundWait => ActivityEvidence::BackgroundWait,
        WireEvidence::BackgroundTaskWait => ActivityEvidence::BackgroundTaskWait,
        WireEvidence::ConfirmationPrompt => ActivityEvidence::ConfirmationPrompt,
        WireEvidence::SelectionPrompt => ActivityEvidence::SelectionPrompt,
        WireEvidence::FolderTrustPrompt => ActivityEvidence::FolderTrustPrompt,
        WireEvidence::NoActiveMarker => ActivityEvidence::NoActiveMarker,
    }
}

fn goal_of(goal: WireGoal) -> GoalEvidence {
    match goal {
        WireGoal::Active => GoalEvidence::State(GoalState::Active),
        WireGoal::Paused => GoalEvidence::State(GoalState::Paused),
        WireGoal::Blocked => GoalEvidence::State(GoalState::Blocked),
        WireGoal::UsageLimited => GoalEvidence::State(GoalState::UsageLimited),
        WireGoal::Reached => GoalEvidence::State(GoalState::Reached),
        WireGoal::Inactive => GoalEvidence::Inactive,
        WireGoal::Unknown => GoalEvidence::Unknown,
    }
}

/// The goal predicate family a verdict names (the agent-detect plugin's `goal_rule_name`).
fn goal_rule_of(name: &str) -> Option<GoalEvidenceRule> {
    Some(match name {
        "codex-footer-goal-segment" => GoalEvidenceRule::CodexFooterGoalSegment,
        "codex-metadata-footer-without-goal" => GoalEvidenceRule::CodexMetadataFooterWithoutGoal,
        "claude-goal-indicator" => GoalEvidenceRule::ClaudeGoalIndicator,
        "claude-paused-notice" => GoalEvidenceRule::ClaudePausedNotice,
        "claude-achieved-notice" => GoalEvidenceRule::ClaudeAchievedNotice,
        "claude-failed-notice" => GoalEvidenceRule::ClaudeFailedNotice,
        "claude-cleared-notice" => GoalEvidenceRule::ClaudeClearedNotice,
        "claude-indicator-without-goal" => GoalEvidenceRule::ClaudeIndicatorWithoutGoal,
        _ => return None,
    })
}

/// A bounded `&'static str` for a verdict string the pane state keeps; `None` past the cap.
fn intern(text: &str) -> Option<&'static str> {
    static TABLE: OnceLock<Mutex<BTreeSet<&'static str>>> = OnceLock::new();
    let mut table = TABLE.get_or_init(Default::default).lock().ok()?;
    if let Some(known) = table.get(text) {
        return Some(known);
    }
    if table.len() >= MAX_INTERNED || text.len() > 256 {
        return None;
    }
    let leaked: &'static str = Box::leak(text.to_owned().into_boxed_str());
    table.insert(leaked);
    Some(leaked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_host::runtime::detectors::DetectorVerdict;

    fn screen(pane: u64, generation: u64) -> ScreenRequest {
        ScreenRequest {
            pane,
            generation,
            signature: "codex".into(),
            text: String::new(),
            cursor: None,
            dimmed: Vec::new(),
        }
    }

    fn verdict(pane: u64, generation: u64) -> ScreenVerdict {
        ScreenVerdict {
            pane,
            generation,
            signature: "codex".into(),
            turn: WireTurn::WaitingSubagents,
            evidence: WireEvidence::BackgroundTaskWait,
            evidence_line: Some("2 background tasks".into()),
            goal: WireGoal::Paused,
            goal_line: Some("Goal paused".into()),
            goal_rule: Some("claude-paused-notice".into()),
            goal_pattern: Some("Goal paused".into()),
            fresh_screen: true,
            interstitial_response: Some("\r".into()),
            prompt_ready: false,
        }
    }

    fn batch(tier: Tier, verdicts: Vec<ScreenVerdict>) -> DetectorBatch {
        DetectorBatch {
            verdicts: verdicts
                .into_iter()
                .map(|verdict| DetectorVerdict {
                    plugin: "ilium.core.agent-detect".into(),
                    tier,
                    verdict,
                })
                .collect(),
            failures: Vec::new(),
            unserved: Vec::new(),
        }
    }

    /// REQ: HST-451
    #[test]
    fn requests_coalesce_into_one_job_and_the_newest_screen_wins() {
        let verdicts = DetectorVerdicts::default();
        assert!(verdicts.request(screen(1, 1)));
        assert!(!verdicts.request(screen(1, 2)));
        assert!(!verdicts.request(screen(2, 1)));
        let taken = verdicts.take_batch();
        assert_eq!(
            taken
                .iter()
                .map(|s| (s.pane, s.generation))
                .collect::<Vec<_>>(),
            vec![(1, 2), (2, 1)]
        );
        assert!(!verdicts.finish_round());
        assert!(verdicts.request(screen(3, 1)));
    }

    /// REQ: HST-453
    #[test]
    fn a_batch_is_capped_and_the_next_one_starts_after_the_last_pane() {
        let verdicts = DetectorVerdicts::default();
        for pane in 1..=40 {
            verdicts.request(screen(pane, 1));
        }
        let first = verdicts.take_batch();
        assert_eq!(first.len(), MAX_SCREENS_PER_ROUND);
        assert_eq!(first[0].pane, 1);
        assert!(verdicts.finish_round(), "eight screens still wait");
        for pane in 1..=4 {
            verdicts.request(screen(pane, 2));
        }
        let second = verdicts.take_batch();
        assert_eq!(second[0].pane, 33, "the rotation resumes after pane 32");
        assert_eq!(second.len(), 12);
    }

    /// REQ: HST-453
    #[test]
    fn screens_a_spent_budget_left_unserved_are_first_in_line() {
        let verdicts = DetectorVerdicts::default();
        for pane in [1, 2, 3, 4] {
            verdicts.request(screen(pane, 1));
        }
        let taken = verdicts.take_batch();
        assert_eq!(taken.len(), 4);
        verdicts.request(screen(4, 2));
        let spent = verdicts.store(DetectorBatch {
            verdicts: Vec::new(),
            failures: Vec::new(),
            unserved: taken[2..].to_vec(),
        });
        assert!(spent);
        verdicts.end_cycle();
        verdicts.request(screen(1, 2));
        let next = verdicts.take_batch();
        assert_eq!(
            next.iter()
                .map(|s| (s.pane, s.generation))
                .collect::<Vec<_>>(),
            vec![(3, 1), (4, 2), (1, 2)],
            "pane 3 leads, pane 4 keeps its newer screen"
        );
    }

    /// REQ: HST-453
    #[test]
    fn one_tick_shares_one_budget_over_every_round() {
        let verdicts = DetectorVerdicts::default();
        assert_eq!(verdicts.budget_left(), DETECTOR_CYCLE_BUDGET);
        verdicts.spend(Duration::from_millis(30));
        verdicts.spend(Duration::from_millis(30));
        assert_eq!(verdicts.budget_left(), Duration::ZERO);
        verdicts.start_cycle();
        assert_eq!(verdicts.budget_left(), DETECTOR_CYCLE_BUDGET);
    }

    /// REQ: HST-454
    #[test]
    fn a_missing_verdict_is_no_sample_and_an_older_one_never_replaces_a_newer_one() {
        let verdicts = DetectorVerdicts::default();
        assert_eq!(verdicts.latest(1, "codex"), None);
        verdicts.store(batch(Tier::T1, vec![verdict(1, 5)]));
        verdicts.store(batch(Tier::T1, vec![verdict(1, 4)]));
        assert_eq!(
            verdicts.latest(1, "codex").map(|c| c.verdict.generation),
            Some(5)
        );
        assert_eq!(verdicts.latest(1, "claude"), None);
        assert!(
            !verdicts.request(screen(1, 5)),
            "an answered frame is not asked again"
        );
        verdicts.retain_panes(&BTreeSet::new());
        assert_eq!(verdicts.latest(1, "codex"), None);
    }

    #[test]
    fn a_verdict_translates_into_kernel_evidence_and_only_t1_may_type_keys() {
        let verdicts = DetectorVerdicts::default();
        verdicts.store(batch(Tier::T1, vec![verdict(1, 1)]));
        verdicts.store(batch(Tier::T3, vec![verdict(2, 1)]));
        let first_party = evidence_of(&verdicts.latest(1, "codex").expect("cached"));
        assert_eq!(first_party.activity.turn, AgentTurn::WaitingSubagents);
        assert_eq!(
            first_party.activity.evidence,
            ActivityEvidence::BackgroundTaskWait
        );
        assert_eq!(
            first_party.goal.evidence,
            GoalEvidence::State(GoalState::Paused)
        );
        assert_eq!(
            first_party.goal.matched_rule,
            Some(GoalEvidenceRule::ClaudePausedNotice)
        );
        assert_eq!(first_party.goal.matched_pattern, Some("Goal paused"));
        assert!(first_party.is_fresh_agent_screen);
        assert_eq!(first_party.interstitial_prompt_response, Some("\r"));
        let third_party = evidence_of(&verdicts.latest(2, "codex").expect("cached"));
        assert_eq!(third_party.interstitial_prompt_response, None);
    }

    #[test]
    fn stopping_the_last_detector_drops_every_cached_verdict() {
        let verdicts = DetectorVerdicts::default();
        verdicts.set_active(true);
        verdicts.store(batch(Tier::T1, vec![verdict(1, 1)]));
        verdicts.set_active(false);
        assert!(!verdicts.is_active());
        assert_eq!(verdicts.latest(1, "codex"), None);
    }

    #[test]
    fn a_screen_request_carries_cursor_dimmed_cells_and_the_signature() {
        let snapshot = ilium_pty::ScreenSnapshot {
            generation: 9,
            text: "› ".into(),
            cursor_position: (3, 2),
            dimmed_cells: vec![(3, 4)],
        };
        let request = screen_request(7, &AgentClass::Other("example".into()), &snapshot);
        assert_eq!((request.pane, request.generation), (7, 9));
        assert_eq!(request.signature, "example");
        assert_eq!(request.cursor, Some(ScreenCell { row: 3, column: 2 }));
        assert_eq!(request.dimmed, vec![ScreenCell { row: 3, column: 4 }]);
    }
}
