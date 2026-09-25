//! Server-owned Text Trigger matching. This runs on the PTY-output path, not
//! the client `ScreenUpdate` path, so detached and hidden panes behave exactly
//! like visible panes.
//!
//! # Identity model
//!
//! A trigger fires once per *instance* of matching text, however that text
//! later moves. Row positions are deliberately not part of an instance's
//! identity: terminal programs move text without re-sending it (scrolling,
//! scroll regions, insert/delete line) and re-send unchanged text at other
//! positions (repaints, resizes). Instead the tracker keeps its own `vt100`
//! screen, fed by the pane's bytes, and after every slice of output counts how
//! many times each *matched text* is visible. Per rule and matched text it
//! remembers how many instances were already admitted:
//!
//! * more visible than admitted: the surplus are new instances and fire;
//! * fewer visible than admitted: the instances stay admitted until the
//!   shortfall has lasted [`SETTLE_WINDOW`], so an erase-then-repaint never
//!   re-arms a rule, while text that really went away can trigger again.
//!
//! Output is fed in slices small enough that no line can scroll out of the
//! screen unobserved, so fast log output is still seen.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use ilium_core::{NodeId, NodeKind, PaneStatus};
use ilium_ipc::{TextTrigger, TextTriggerTarget};
use regex::Regex;
use tokio::sync::mpsc;

use crate::ipc::handlers::submit_text_trigger_if_current;
use crate::pane::PaneResource;
use crate::state::ServerState;

/// How long matched text must stay missing before its admitted instances are
/// released and the same text may trigger again. Longer than any repaint gap
/// (an erase and its rewrite are adjacent in the byte stream), shorter than
/// the time an agent needs to react to a reply and show the same text again.
const SETTLE_WINDOW: Duration = Duration::from_millis(1500);

/// After the pane is resized the program repaints its whole view, sometimes
/// with older lines that were not visible before. Newly visible matches in
/// that period are admitted without firing.
const RESIZE_QUIET_WINDOW: Duration = Duration::from_millis(2000);

/// Upper bound of bytes fed between two screen scans.
const MAX_SLICE_BYTES: usize = 8 * 1024;

/// Longest match text kept as an identity key.
const MAX_KEY_CHARS: usize = 512;

/// One bounded output-match job. The forwarder owns the sender and an
/// abort-on-drop worker, so a slow agent composer cannot stall PTY output.
pub struct TriggerDelivery {
    pub trigger_id: String,
    pub message: String,
    pub settings_revision: u64,
}

pub async fn run_deliveries(
    state: std::sync::Arc<ServerState>,
    pane_id: NodeId,
    mut receiver: mpsc::Receiver<TriggerDelivery>,
) {
    while let Some(delivery) = receiver.recv().await {
        if let Err(error) = submit_text_trigger_if_current(
            &state,
            pane_id,
            &delivery.trigger_id,
            &delivery.message,
            delivery.settings_revision,
        )
        .await
        {
            tracing::warn!(pane_id = pane_id.0, trigger_id = %delivery.trigger_id, %error, "text trigger submission failed");
        }
    }
}

/// Admission state of one distinct matched text under one rule.
struct KeyState {
    /// Instances already answered (or deliberately adopted without an answer).
    admitted: usize,
    /// Last moment at least `admitted` instances were visible.
    full_since: Instant,
    /// Instances visible at the previous scan. The screen cannot change
    /// between scans, so this is also what stayed visible until now.
    visible: usize,
}

/// Per-rule instance bookkeeping plus the rule's compiled expression, so the
/// regexp is compiled once per rule revision rather than once per output chunk.
struct RuleTable {
    regexp: String,
    regex: Regex,
    keys: HashMap<String, KeyState>,
}

impl RuleTable {
    fn new(trigger: &TextTrigger) -> Option<Self> {
        Some(Self {
            regexp: trigger.regexp.clone(),
            regex: Regex::new(&trigger.regexp).ok()?,
            keys: HashMap::new(),
        })
    }

    fn count_matches(&self, lines: &[String]) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        for line in lines {
            for found in self.regex.find_iter(line) {
                let key = normalize_key(found.as_str());
                if !key.is_empty() {
                    *counts.entry(key).or_insert(0) += 1;
                }
            }
        }
        counts
    }

    /// Adopts what is visible now as already answered.
    fn seed(&mut self, counts: &HashMap<String, usize>, now: Instant) {
        for (key, &count) in counts {
            self.keys.insert(
                key.clone(),
                KeyState {
                    admitted: count,
                    full_since: now,
                    visible: count,
                },
            );
        }
    }

    /// Returns how many new instances must be answered.
    fn observe(
        &mut self,
        counts: &HashMap<String, usize>,
        now: Instant,
        is_eligible: bool,
        is_quiet: bool,
    ) -> usize {
        // A shortfall that lasted through the quiet time before this output
        // means those instances are really gone.
        for state in self.keys.values_mut() {
            if state.visible < state.admitted
                && now.saturating_duration_since(state.full_since) >= SETTLE_WINDOW
            {
                state.admitted = state.visible;
                state.full_since = now;
            }
        }
        let mut fires = 0;
        for (key, &count) in counts {
            let admitted = self.keys.get(key).map_or(0, |state| state.admitted);
            if count <= admitted {
                continue;
            }
            let mut surplus = count - admitted;
            // A match that grows while it streams (`foo` then `foo bar` for a
            // greedy expression) is the same instance under a longer key.
            // Take over admissions from a related key that lost visibility.
            let mut adopted = 0;
            for (other, state) in &mut self.keys {
                if surplus == 0 {
                    break;
                }
                if other == key
                    || !(other.starts_with(key.as_str()) || key.starts_with(other.as_str()))
                {
                    continue;
                }
                let other_count = counts.get(other).copied().unwrap_or(0);
                if state.admitted <= other_count {
                    continue;
                }
                let moved = surplus.min(state.admitted - other_count);
                state.admitted -= moved;
                surplus -= moved;
                adopted += moved;
            }
            // Ineligible surplus stays unadmitted so it fires once the pane
            // becomes eligible; a resize repaint is adopted silently.
            let admitted_now = if is_quiet || is_eligible { surplus } else { 0 };
            if is_eligible && !is_quiet {
                fires += surplus;
            }
            if adopted + admitted_now > 0 {
                let state = self.keys.entry(key.clone()).or_insert(KeyState {
                    admitted: 0,
                    full_since: now,
                    visible: 0,
                });
                state.admitted += adopted + admitted_now;
            }
        }
        for (key, state) in &mut self.keys {
            state.visible = counts.get(key).copied().unwrap_or(0);
            if state.visible >= state.admitted {
                state.full_since = now;
            }
        }
        self.keys.retain(|_, state| state.admitted > 0);
        fires
    }

    fn touch(&mut self, now: Instant) {
        for state in self.keys.values_mut() {
            state.full_since = now;
        }
    }
}

/// Whitespace runs collapse and digit runs become `#`, so a ticking counter
/// or a re-wrapped gap does not turn one instance into another.
fn normalize_key(text: &str) -> String {
    let mut key = String::new();
    let mut previous_was_space = true;
    let mut previous_was_digit = false;
    for character in text.chars() {
        if character.is_whitespace() {
            if !previous_was_space {
                key.push(' ');
            }
            previous_was_space = true;
            previous_was_digit = false;
        } else if character.is_ascii_digit() {
            if !previous_was_digit {
                key.push('#');
            }
            previous_was_space = false;
            previous_was_digit = true;
        } else {
            key.push(character);
            previous_was_space = false;
            previous_was_digit = false;
        }
        if key.chars().count() >= MAX_KEY_CHARS {
            break;
        }
    }
    key.truncate(key.trim_end().len());
    key
}

/// Visible rows joined across soft wraps into logical lines.
fn logical_lines(screen: &vt100::Screen) -> Vec<String> {
    let (_, columns) = screen.size();
    let mut lines = Vec::new();
    let mut current = String::new();
    for (row, text) in screen.rows(0, columns).enumerate() {
        current.push_str(&text);
        let continues = u16::try_from(row).is_ok_and(|row| screen.row_wrapped(row));
        if !continues {
            lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Where the next scan should happen: after enough newlines that nothing can
/// scroll away unseen, or after a fixed byte budget.
fn slice_end(bytes: &[u8], start: usize, newline_budget: usize) -> usize {
    let limit = bytes.len().min(start + MAX_SLICE_BYTES);
    let mut newlines = 0;
    for (offset, &byte) in bytes[start..limit].iter().enumerate() {
        if byte == b'\n' {
            newlines += 1;
            if newlines >= newline_budget {
                return start + offset + 1;
            }
        }
    }
    limit
}

/// One pane's trigger memory. The forwarder owns it and feeds it every output
/// chunk in order.
#[derive(Default)]
pub struct TriggerTracker {
    shadow: vt100::Parser,
    /// Main-screen and alternate-screen instances are remembered separately:
    /// leaving the alternate screen restores the main text unchanged and must
    /// not read as new output.
    tables: [HashMap<String, RuleTable>; 2],
    is_alternate: bool,
    is_alternate_table_fresh: bool,
    has_started: bool,
    quiet_until: Option<Instant>,
}

impl TriggerTracker {
    pub fn resize(&mut self, rows: u16, cols: u16, now: Instant) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        if self.shadow.screen().size() == (rows, cols) {
            return;
        }
        self.shadow.screen_mut().set_size(rows, cols);
        if self.has_started {
            self.quiet_until = Some(now + RESIZE_QUIET_WINDOW);
        }
    }

    /// Replaces the tracker's screen with the pane's authoritative one after
    /// PTY bytes were lost. Admissions are kept, so text that stayed visible
    /// does not fire again.
    pub fn replace_screen(&mut self, rows: u16, cols: u16, formatted_state: &[u8]) {
        let mut parser = vt100::Parser::new(rows.max(1), cols.max(1), 0);
        parser.process(formatted_state);
        self.shadow = parser;
    }

    /// Feeds `bytes` (an empty slice only rescans) and returns the id of the
    /// trigger for every new instance, one entry per instance.
    pub fn feed(
        &mut self,
        bytes: &[u8],
        triggers: &[TextTrigger],
        status: &PaneStatus,
        now: Instant,
    ) -> Vec<String> {
        let enabled: Vec<&TextTrigger> =
            triggers.iter().filter(|trigger| trigger.enabled).collect();
        let mut fired = Vec::new();
        if enabled.is_empty() {
            self.shadow.process(bytes);
            self.tables = [HashMap::new(), HashMap::new()];
            self.has_started = true;
            return fired;
        }
        if bytes.is_empty() {
            self.evaluate(&enabled, status, now, &mut fired);
        }
        let mut start = 0;
        while start < bytes.len() {
            let (rows, _) = self.shadow.screen().size();
            let newline_budget = usize::from(rows / 2).max(1);
            let end = slice_end(bytes, start, newline_budget);
            self.shadow.process(&bytes[start..end]);
            self.evaluate(&enabled, status, now, &mut fired);
            start = end;
        }
        self.has_started = true;
        fired
    }

    fn evaluate(
        &mut self,
        enabled: &[&TextTrigger],
        status: &PaneStatus,
        now: Instant,
        fired: &mut Vec<String>,
    ) {
        let is_alternate = self.shadow.screen().alternate_screen();
        if is_alternate != self.is_alternate {
            self.is_alternate = is_alternate;
            if is_alternate {
                self.tables[1].clear();
                self.is_alternate_table_fresh = true;
            } else {
                self.tables[0]
                    .values_mut()
                    .for_each(|table| table.touch(now));
            }
        }
        let lines = logical_lines(self.shadow.screen());
        let is_quiet = self.quiet_until.is_some_and(|until| now < until);
        // Rules added while a pane already shows matching text adopt that
        // text; a pane's first sight of a rule adopts nothing.
        let seeds_new_rules =
            self.has_started && !(self.is_alternate && self.is_alternate_table_fresh);
        let table = &mut self.tables[usize::from(is_alternate)];
        table.retain(|id, _| enabled.iter().any(|trigger| &trigger.id == id));
        for trigger in enabled {
            if table
                .get(&trigger.id)
                .is_some_and(|rule| rule.regexp != trigger.regexp)
            {
                table.remove(&trigger.id);
            }
            let is_new_rule = !table.contains_key(&trigger.id);
            if is_new_rule {
                let Some(rule) = RuleTable::new(trigger) else {
                    continue;
                };
                table.insert(trigger.id.clone(), rule);
            }
            let Some(rule) = table.get_mut(&trigger.id) else {
                continue;
            };
            let counts = rule.count_matches(&lines);
            if is_new_rule && seeds_new_rules {
                rule.seed(&counts, now);
                continue;
            }
            let fires = rule.observe(
                &counts,
                now,
                target_matches(trigger.target, status),
                is_quiet,
            );
            fired.extend(std::iter::repeat_n(trigger.id.clone(), fires));
        }
        self.is_alternate_table_fresh = false;
    }
}

struct EvaluationContext {
    status: PaneStatus,
    settings: ilium_ipc::TextTriggerSettings,
    settings_revision: u64,
}

async fn load_context(state: &ServerState, pane_id: NodeId) -> Option<EvaluationContext> {
    let status = state
        .tree
        .read()
        .await
        .get(pane_id)
        .and_then(|node| match &node.kind {
            NodeKind::Pane { status, .. } => Some(status.clone()),
            _ => None,
        })?;
    let accepted = state.text_trigger_settings.read().await;
    Some(EvaluationContext {
        status,
        settings: accepted.settings.clone(),
        settings_revision: accepted.revision,
    })
}

fn deliver(
    pane_id: NodeId,
    context: &EvaluationContext,
    fired: Vec<String>,
    delivery_sender: &mpsc::Sender<TriggerDelivery>,
) {
    for trigger_id in fired {
        let Some(trigger) = context
            .settings
            .triggers
            .iter()
            .find(|trigger| trigger.id == trigger_id)
        else {
            continue;
        };
        if let Err(error) = delivery_sender.try_send(TriggerDelivery {
            trigger_id: trigger.id.clone(),
            message: trigger.message.clone(),
            settings_revision: context.settings_revision,
        }) {
            tracing::warn!(pane_id = pane_id.0, trigger_id = %trigger.id, %error, "text trigger delivery queue is full or closed");
        }
    }
}

pub async fn process_output(
    state: &ServerState,
    pane_id: NodeId,
    tracker: &mut TriggerTracker,
    bytes: &[u8],
    delivery_sender: &mpsc::Sender<TriggerDelivery>,
) {
    let size = state
        .panes
        .read()
        .await
        .get(&pane_id)
        .and_then(|resource| match resource {
            PaneResource::Terminal(runtime) => {
                Some(runtime.session.with_screen(vt100::Screen::size))
            }
            PaneResource::Editor { .. } => None,
        });
    if let Some((rows, cols)) = size {
        tracker.resize(rows, cols, Instant::now());
    }
    let Some(context) = load_context(state, pane_id).await else {
        return;
    };
    let fired = tracker.feed(
        bytes,
        &context.settings.triggers,
        &context.status,
        Instant::now(),
    );
    deliver(pane_id, &context, fired, delivery_sender);
}

/// Called when PTY chunks were dropped: the tracker's screen no longer matches
/// the pane's, so it adopts the pane's real screen and rescans it.
pub async fn resync_after_gap(
    state: &ServerState,
    pane_id: NodeId,
    tracker: &mut TriggerTracker,
    delivery_sender: &mpsc::Sender<TriggerDelivery>,
) {
    let snapshot = state
        .panes
        .read()
        .await
        .get(&pane_id)
        .and_then(|resource| match resource {
            PaneResource::Terminal(runtime) => Some(
                runtime
                    .session
                    .with_screen(|screen| (screen.size(), screen.state_formatted())),
            ),
            PaneResource::Editor { .. } => None,
        });
    let Some(((rows, cols), formatted_state)) = snapshot else {
        return;
    };
    tracker.replace_screen(rows, cols, &formatted_state);
    process_output(state, pane_id, tracker, &[], delivery_sender).await;
}

/// Validates candidates before the detached server replaces its active list.
/// Messages stay literal single lines so each rule has one text stage and one
/// later Enter stage.
pub fn validate_settings(settings: &ilium_ipc::TextTriggerSettings) -> Option<String> {
    let mut ids = HashSet::new();
    for (index, trigger) in settings.triggers.iter().enumerate() {
        if trigger.id.is_empty() {
            return Some(format!("Text Trigger {} has no stable id", index + 1));
        }
        if !ids.insert(&trigger.id) {
            return Some(format!("Text Trigger {} repeats an existing id", index + 1));
        }
        if trigger.regexp.is_empty() {
            return Some(format!(
                "Text Trigger {} regexp must not be empty",
                index + 1
            ));
        }
        if trigger.message.contains(['\r', '\n']) {
            return Some(format!(
                "Text Trigger {} message must be one line",
                index + 1
            ));
        }
        if let Err(error) = Regex::new(&trigger.regexp) {
            return Some(format!(
                "Text Trigger {} regexp is invalid: {error}",
                index + 1
            ));
        }
    }
    None
}

pub(crate) fn target_matches(target: TextTriggerTarget, status: &PaneStatus) -> bool {
    matches!(
        (target, status),
        (
            TextTriggerTarget::Both,
            PaneStatus::PlainShell | PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..),
        ) | (
            TextTriggerTarget::Agents,
            PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..)
        ) | (TextTriggerTarget::Terminals, PaneStatus::PlainShell)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_ipc::{TextTrigger, TextTriggerSettings};

    fn rule(regexp: &str) -> Vec<TextTrigger> {
        vec![TextTrigger {
            id: "rule".to_string(),
            enabled: true,
            regexp: regexp.to_string(),
            message: "reply".to_string(),
            target: TextTriggerTarget::Both,
            sample_text: String::new(),
        }]
    }

    struct Harness {
        tracker: TriggerTracker,
        triggers: Vec<TextTrigger>,
        status: PaneStatus,
        now: Instant,
        fired: usize,
    }

    impl Harness {
        fn new(regexp: &str, rows: u16, cols: u16) -> Self {
            let mut tracker = TriggerTracker::default();
            let now = Instant::now();
            tracker.resize(rows, cols, now);
            Self {
                tracker,
                triggers: rule(regexp),
                status: PaneStatus::PlainShell,
                now,
                fired: 0,
            }
        }

        fn feed(&mut self, bytes: &[u8]) -> usize {
            let fired = self
                .tracker
                .feed(bytes, &self.triggers, &self.status, self.now)
                .len();
            self.fired += fired;
            fired
        }

        fn wait(&mut self, duration: Duration) {
            self.now += duration;
        }

        /// A rescan with no new bytes, as after a quiet moment.
        fn rescan(&mut self) -> usize {
            self.feed(b"")
        }
    }

    #[test]
    fn a_streamed_line_fires_once_when_it_completes_and_scrolls() {
        let mut harness = Harness::new("abracrabdara", 4, 60);
        assert_eq!(harness.feed(b"noise\r\nabracrabdara, may I\r\n"), 1);
        for index in 0..10 {
            assert_eq!(harness.feed(format!("more {index}\r\n").as_bytes()), 0);
        }
        assert_eq!(harness.fired, 1);
    }

    #[test]
    fn a_line_partly_written_matches_only_once_complete_and_once() {
        let mut harness = Harness::new("abracrabdara", 6, 60);
        assert_eq!(harness.feed(b"abracra"), 0);
        assert_eq!(harness.feed(b"bdara, do you"), 1);
        assert_eq!(harness.feed(b" allow me\r\n"), 0);
        assert_eq!(harness.feed(b"next\r\n"), 0);
    }

    #[test]
    fn ink_style_erase_and_rewrite_frames_do_not_refire() {
        let mut harness = Harness::new("abracrabdara", 8, 60);
        assert_eq!(harness.feed(b"header\r\nabracrabdara now\r\nstatus 1"), 1);
        for frame in 2..40 {
            let repaint = format!(
                "\x1b[2K\x1b[1A\x1b[2K\x1b[1A\x1b[2K\x1b[Gheader\r\nabracrabdara now\r\nstatus {frame}"
            );
            assert_eq!(harness.feed(repaint.as_bytes()), 0, "frame {frame}");
        }
    }

    #[test]
    fn an_unterminated_last_line_repainted_by_the_next_frame_fires_once() {
        let mut harness = Harness::new("abracrabdara", 8, 60);
        assert_eq!(harness.feed(b"line1\r\nabracrabdara"), 1);
        assert_eq!(
            harness.feed(b"\x1b[2K\x1b[1A\x1b[Gline1\r\nabracrabdara\r\nline3"),
            0
        );
        assert_eq!(harness.feed(b"\r\nmore\r\n"), 0);
    }

    #[test]
    fn cursor_addressed_output_without_newlines_is_matched() {
        let mut harness = Harness::new("abracrabdara", 6, 40);
        assert_eq!(harness.feed(b"\x1b[2;1Habracrabdara x"), 1);
        assert_eq!(harness.feed(b"\x1b[3;1Hnext line"), 0);
        assert_eq!(harness.feed(b"\x1b[2;1H\x1b[Kabracrabdara x"), 0);
    }

    #[test]
    fn ink_frames_that_grow_and_scroll_the_view_do_not_refire() {
        let mut harness = Harness::new("abracrabdara", 5, 60);
        let mut lines = vec!["abracrabdara now".to_string()];
        assert_eq!(harness.feed(b"abracrabdara now"), 1);
        let mut height: usize = 1;
        for step in 0..12 {
            lines.push(format!("answer {step}"));
            let mut frame = String::new();
            for _ in 0..height.saturating_sub(1) {
                frame.push_str("\x1b[2K\x1b[1A");
            }
            frame.push_str("\x1b[2K\x1b[G");
            frame.push_str(&lines.join("\r\n"));
            height = lines.len();
            assert_eq!(harness.feed(frame.as_bytes()), 0, "step {step}");
        }
    }

    #[test]
    fn scroll_regions_and_scroll_commands_that_move_text_do_not_refire() {
        let mut harness = Harness::new("abracrabdara", 6, 40);
        assert_eq!(harness.feed(b"\x1b[3;1Habracrabdara here"), 1);
        for sequence in [
            &b"\x1b[S"[..],
            b"\x1b[2S",
            b"\x1b[T",
            b"\x1bM",
            b"\x1b[1;6r\x1b[6;1H\x1bD",
            b"\x1b[2;5r\x1b[2;1H\x1bM\x1b[r",
            b"\x1b[2L",
            b"\x1b[M",
        ] {
            assert_eq!(harness.feed(sequence), 0, "{sequence:?}");
        }
    }

    #[test]
    fn a_clear_and_full_redraw_within_the_settle_window_does_not_refire() {
        let mut harness = Harness::new("abracrabdara", 6, 40);
        assert_eq!(harness.feed(b"a\r\nabracrabdara go\r\nb"), 1);
        assert_eq!(harness.feed(b"\x1b[2J\x1b[H"), 0);
        harness.wait(Duration::from_millis(400));
        assert_eq!(harness.feed(b"a\r\nabracrabdara go\r\nb"), 0);
        assert_eq!(harness.fired, 1);
    }

    #[test]
    fn a_match_missing_for_the_settle_window_fires_again_when_it_returns() {
        let mut harness = Harness::new("^trigger-ready$", 4, 40);
        assert_eq!(harness.feed(b"trigger-ready\r\n"), 1);
        assert_eq!(harness.feed(b"\x1b[H\x1b[2Jwaiting"), 0);
        harness.wait(SETTLE_WINDOW + Duration::from_millis(50));
        assert_eq!(harness.rescan(), 0);
        assert_eq!(harness.feed(b"\x1b[H\x1b[2Jtrigger-ready"), 1);
    }

    #[test]
    fn a_match_returning_after_a_silent_gap_longer_than_the_settle_window_fires() {
        let mut harness = Harness::new("^trigger-ready$", 4, 40);
        assert_eq!(harness.feed(b"trigger-ready\r\n"), 1);
        assert_eq!(harness.feed(b"\x1b[H\x1b[2Jwaiting"), 0);
        harness.wait(SETTLE_WINDOW + Duration::from_millis(50));
        assert_eq!(harness.feed(b"\x1b[H\x1b[2Jtrigger-ready"), 1);
    }

    #[test]
    fn a_match_returning_after_a_short_silent_gap_does_not_fire() {
        let mut harness = Harness::new("^trigger-ready$", 4, 40);
        assert_eq!(harness.feed(b"trigger-ready\r\n"), 1);
        assert_eq!(harness.feed(b"\x1b[H\x1b[2Jwaiting"), 0);
        harness.wait(SETTLE_WINDOW / 3);
        assert_eq!(harness.feed(b"\x1b[H\x1b[2Jtrigger-ready"), 0);
    }

    #[test]
    fn simultaneous_identical_instances_each_fire_and_scrolling_keeps_them_answered() {
        let mut harness = Harness::new("abracrabdara", 14, 40);
        assert_eq!(
            harness.feed(b"abracrabdara one\r\nx\r\nabracrabdara two\r\n"),
            2
        );
        assert_eq!(harness.feed(b"x\r\nx\r\nx\r\nx\r\nx\r\nx\r\n"), 0);
        harness.wait(SETTLE_WINDOW * 2);
        assert_eq!(harness.rescan(), 0);
        assert_eq!(harness.feed(b"abracrabdara three\r\n"), 1);
        assert_eq!(harness.fired, 3);
    }

    #[test]
    fn an_instance_that_scrolled_off_frees_its_slot_after_the_settle_window() {
        let mut harness = Harness::new("abracrabdara", 4, 40);
        assert_eq!(harness.feed(b"abracrabdara one\r\n"), 1);
        assert_eq!(harness.feed(b"x\r\nx\r\nx\r\nx\r\n"), 0);
        harness.wait(SETTLE_WINDOW + Duration::from_millis(10));
        assert_eq!(harness.rescan(), 0);
        assert_eq!(harness.feed(b"abracrabdara two\r\n"), 1);
    }

    #[test]
    fn a_growing_greedy_match_is_one_instance() {
        let mut harness = Harness::new("proceed.*", 4, 60);
        assert_eq!(harness.feed(b"proceed"), 1);
        for chunk in [&b" w"[..], b"ith", b" the", b" change?"] {
            assert_eq!(harness.feed(chunk), 0);
        }
    }

    #[test]
    fn a_counter_inside_the_match_does_not_create_new_instances() {
        let mut harness = Harness::new(r"Pursuing goal \(\d+m\)", 4, 60);
        assert_eq!(harness.feed(b"Pursuing goal (1m)"), 1);
        assert_eq!(harness.feed(b"\x1b[2K\x1b[GPursuing goal (2m)"), 0);
        assert_eq!(harness.feed(b"\x1b[2K\x1b[GPursuing goal (13m)"), 0);
    }

    #[test]
    fn matches_split_across_a_soft_wrap_are_found_once_at_any_width() {
        let mut harness = Harness::new("abracrabdara go", 6, 10);
        assert_eq!(harness.feed(b"xxabracrabdara go now"), 1);
        harness.tracker.resize(6, 30, harness.now);
        harness.wait(Duration::from_millis(50));
        assert_eq!(harness.feed(b"\x1b[2J\x1b[Hxxabracrabdara go now"), 0);
    }

    #[test]
    fn resize_repaints_never_fire_including_lines_that_were_not_visible_before() {
        let mut harness = Harness::new("abracrabdara", 3, 40);
        assert_eq!(harness.feed(b"abracrabdara old\r\nl1\r\nl2\r\nl3\r\n"), 1);
        harness.wait(SETTLE_WINDOW * 4);
        harness.tracker.resize(8, 40, harness.now);
        assert_eq!(
            harness.feed(b"\x1b[2J\x1b[Habracrabdara old\r\nl1\r\nl2\r\nl3\r\n"),
            0
        );
    }

    #[test]
    fn a_new_instance_after_the_resize_window_fires() {
        let mut harness = Harness::new("abracrabdara", 6, 40);
        assert_eq!(harness.feed(b"abracrabdara first\r\n"), 1);
        harness.tracker.resize(6, 50, harness.now);
        harness.wait(RESIZE_QUIET_WINDOW + Duration::from_millis(10));
        assert_eq!(harness.feed(b"abracrabdara second\r\n"), 1);
    }

    #[test]
    fn the_alternate_screen_round_trip_does_not_refire_main_screen_text() {
        let mut harness = Harness::new("abracrabdara", 6, 40);
        assert_eq!(harness.feed(b"abracrabdara main\r\n"), 1);
        assert_eq!(harness.feed(b"\x1b[?1049h\x1b[Halternate view"), 0);
        harness.wait(SETTLE_WINDOW * 5);
        assert_eq!(harness.feed(b"more alternate"), 0);
        assert_eq!(harness.feed(b"\x1b[?1049l"), 0);
        assert_eq!(harness.fired, 1);
    }

    #[test]
    fn output_that_scrolls_away_inside_one_chunk_is_still_seen() {
        let mut harness = Harness::new(r"^hit \d+ x$", 4, 40);
        let mut burst = String::from("hit 7 x\r\n");
        for index in 0..200 {
            burst.push_str(&format!("filler {index}\r\n"));
        }
        assert_eq!(harness.feed(burst.as_bytes()), 1);
    }

    #[test]
    fn slice_boundaries_inside_escape_sequences_and_utf8_are_safe() {
        let mut harness = Harness::new("snow\u{2603}man", 4, 40);
        let mut burst = Vec::new();
        for _ in 0..600 {
            burst.extend_from_slice(b"\x1b[1;32mgreen\x1b[0m \xe2\x98\x83\r\n");
        }
        burst.extend_from_slice("snow\u{2603}man\r\n".as_bytes());
        assert_eq!(harness.feed(&burst), 1);
    }

    #[test]
    fn a_rule_added_while_matching_text_is_visible_adopts_it_silently() {
        let mut harness = Harness::new("abracrabdara", 6, 40);
        harness.triggers.clear();
        assert_eq!(harness.feed(b"abracrabdara before\r\n"), 0);
        harness.triggers = rule("abracrabdara");
        assert_eq!(harness.rescan(), 0);
        assert_eq!(harness.feed(b"abracrabdara after\r\n"), 1);
    }

    #[test]
    fn editing_the_regexp_adopts_visible_text_and_still_sees_new_text() {
        let mut harness = Harness::new("alpha", 6, 40);
        assert_eq!(harness.feed(b"alpha\r\nbeta\r\n"), 1);
        harness.triggers[0].regexp = "beta".to_string();
        assert_eq!(harness.rescan(), 0);
        assert_eq!(harness.feed(b"beta again\r\n"), 1);
    }

    #[test]
    fn an_admitted_instance_stays_answered_across_temporary_ineligibility() {
        let mut harness = Harness::new("ready", 6, 40);
        harness.triggers[0].target = TextTriggerTarget::Agents;
        assert_eq!(harness.feed(b"ready\r\n"), 0);
        harness.status = PaneStatus::Agent(
            ilium_core::AgentClass::Codex,
            ilium_core::AgentActivity::Working,
        );
        assert_eq!(harness.rescan(), 1);
        harness.status = PaneStatus::PlainShell;
        assert_eq!(harness.rescan(), 0);
        harness.status = PaneStatus::Agent(
            ilium_core::AgentClass::Codex,
            ilium_core::AgentActivity::Idle,
        );
        assert_eq!(harness.rescan(), 0);
    }

    #[test]
    fn a_pane_restored_after_lost_output_keeps_its_admissions() {
        let mut harness = Harness::new("abracrabdara", 6, 40);
        assert_eq!(harness.feed(b"abracrabdara one\r\n"), 1);
        let mut authoritative = vt100::Parser::new(6, 40, 0);
        authoritative.process(b"abracrabdara one\r\nlost output\r\n");
        let formatted = authoritative.screen().state_formatted();
        harness.tracker.replace_screen(6, 40, &formatted);
        assert_eq!(harness.rescan(), 0);
        authoritative.process(b"abracrabdara two\r\n");
        let formatted = authoritative.screen().state_formatted();
        harness.tracker.replace_screen(6, 40, &formatted);
        assert_eq!(harness.rescan(), 1);
    }

    #[test]
    fn empty_and_whitespace_matches_never_create_instances() {
        let mut harness = Harness::new(r"\s*", 4, 40);
        assert_eq!(harness.feed(b"text   more\r\n"), 0);
    }

    #[test]
    fn keys_normalize_spacing_digits_and_length() {
        assert_eq!(normalize_key("  a   b  "), "a b");
        assert_eq!(normalize_key("try 12 of 345"), "try # of #");
        assert!(normalize_key(&"x".repeat(5000)).chars().count() <= MAX_KEY_CHARS);
    }

    #[test]
    fn validation_rejects_unstable_ids_multiline_messages_and_bad_regexps() {
        let mut trigger = TextTrigger {
            regexp: "[".to_string(),
            ..TextTrigger::default()
        };
        assert!(validate_settings(&TextTriggerSettings {
            triggers: vec![trigger.clone()]
        })
        .is_some());
        trigger.id = "rule-1".to_string();
        trigger.regexp = "ok".to_string();
        trigger.message = "one\ntwo".to_string();
        assert!(validate_settings(&TextTriggerSettings {
            triggers: vec![trigger]
        })
        .is_some());
        let trigger = TextTrigger {
            id: "rule-3".to_string(),
            regexp: "ready".to_string(),
            message: "continue".to_string(),
            ..TextTrigger::default()
        };
        assert!(validate_settings(&TextTriggerSettings {
            triggers: vec![trigger.clone(), trigger]
        })
        .is_some());
        let trigger = TextTrigger {
            id: "rule-2".to_string(),
            message: "ok".to_string(),
            ..TextTrigger::default()
        };
        assert!(validate_settings(&TextTriggerSettings {
            triggers: vec![trigger]
        })
        .is_some());
    }

    #[test]
    fn scope_keeps_agents_and_plain_terminals_distinct() {
        assert!(target_matches(
            TextTriggerTarget::Terminals,
            &PaneStatus::PlainShell
        ));
        assert!(!target_matches(
            TextTriggerTarget::Agents,
            &PaneStatus::PlainShell
        ));
        let agent = PaneStatus::Agent(
            ilium_core::AgentClass::Codex,
            ilium_core::AgentActivity::Working,
        );
        assert!(target_matches(TextTriggerTarget::Agents, &agent));
        assert!(!target_matches(TextTriggerTarget::Terminals, &agent));
    }
}
