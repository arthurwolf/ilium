//! Instance identity and admission for text triggers.
//!
//! Instance identity is approximated by visible match counts, not row positions: terminal programs
//! move text without re-sending it and repaint unchanged text elsewhere. Each normalized matched
//! text is counted after every screen scan.
//!
//! * more visible than admitted: the surplus are new instances and fire;
//! * fewer visible than admitted: each additional missing cohort starts its own settle window at
//!   the scan that first observes that loss.
//!
//! Moved from `ilium-server/src/text_triggers.rs` (`KeyState`, `RuleTable`, `normalize_key`,
//! `target_matches`, `slice_end`, the delivery ordering). Time is milliseconds since an arbitrary
//! epoch (`u64`) so the module has no clock and stays wasm-safe; the screen arrives as logical
//! lines, so no terminal emulator is needed here.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use regex::Regex;

use crate::config::{Rule, Target};

/// Grace period measured from the first scan observing an admission missing.
pub const SETTLE_WINDOW_MS: u64 = 1500;
/// After a resize the program repaints; newly visible matches in that period are adopted silently.
pub const RESIZE_QUIET_WINDOW_MS: u64 = 2000;
/// Upper bound of bytes fed between two screen scans.
pub const MAX_SLICE_BYTES: usize = 8 * 1024;
/// Longest match text kept as an identity key.
pub const MAX_KEY_CHARS: usize = 512;
/// Delayed deliveries one pane may hold.
pub const MAX_PENDING_DELIVERIES: usize = 64;

/// What the pane currently is, as far as target filtering cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    PlainShell,
    Agent,
    Other,
}

/// Whether a rule with `target` may observe a pane of `kind`.
pub fn target_matches(target: Target, kind: PaneKind) -> bool {
    matches!(
        (target, kind),
        (Target::Both, PaneKind::PlainShell | PaneKind::Agent)
            | (Target::Agents, PaneKind::Agent)
            | (Target::Terminals, PaneKind::PlainShell)
    )
}

/// Where the next scan should happen: after enough newlines that nothing can scroll away unseen,
/// or after a fixed byte budget.
pub fn slice_end(bytes: &[u8], start: usize, newline_budget: usize) -> usize {
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

/// Whitespace runs collapse and digit runs become `#`, so a ticking counter or a re-wrapped gap
/// does not turn one instance into another.
pub fn normalize_key(text: &str) -> String {
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

/// Admissions first observed missing at the same scan.
struct MissingAdmissions {
    count: usize,
    since: u64,
}

/// Admission state of one distinct matched text under one rule.
struct KeyState {
    /// Instances already answered (or deliberately adopted without an answer).
    admitted: usize,
    /// Oldest first; after reconciliation their sum is `admitted - visible` (saturating).
    missing: VecDeque<MissingAdmissions>,
}

impl KeyState {
    fn new(admitted: usize) -> Self {
        Self {
            admitted,
            missing: VecDeque::new(),
        }
    }

    /// Expire only losses already observed before the current scan.
    fn expire_missing(&mut self, now: u64) {
        while self
            .missing
            .front()
            .is_some_and(|loss| now.saturating_sub(loss.since) >= SETTLE_WINDOW_MS)
        {
            let Some(loss) = self.missing.pop_front() else {
                break;
            };
            self.admitted -= loss.count;
        }
    }

    /// Reconcile after all surplus admissions and prefix-key transfers.
    fn observe_visible(&mut self, visible: usize, now: u64) {
        let required = self.admitted.saturating_sub(visible);
        let tracked: usize = self.missing.iter().map(|loss| loss.count).sum();
        if required > tracked {
            let count = required - tracked;
            if let Some(last) = self.missing.back_mut().filter(|loss| loss.since == now) {
                last.count += count;
            } else {
                self.missing
                    .push_back(MissingAdmissions { count, since: now });
            }
        } else {
            // Counts cannot identify which missing instance returned: retire the oldest first.
            let mut returned = tracked - required;
            while returned > 0 {
                let Some(first) = self.missing.front_mut() else {
                    break;
                };
                let count = returned.min(first.count);
                first.count -= count;
                returned -= count;
                if first.count == 0 {
                    self.missing.pop_front();
                }
            }
        }
    }
}

/// Semantic per-rule admission state.
pub struct RuleTable {
    regexp: String,
    compiled: Option<Regex>,
    keys: HashMap<String, KeyState>,
}

impl RuleTable {
    pub fn new(rule: &Rule) -> Self {
        Self {
            regexp: rule.regexp.clone(),
            compiled: Regex::new(&rule.regexp).ok(),
            keys: HashMap::new(),
        }
    }

    pub fn regexp(&self) -> &str {
        &self.regexp
    }

    /// Counts each normalized matched text over the logical lines.
    pub fn count_matches(&self, lines: &[String]) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        let Some(regex) = &self.compiled else {
            return counts;
        };
        for line in lines {
            for found in regex.find_iter(line) {
                let key = normalize_key(found.as_str());
                if !key.is_empty() {
                    *counts.entry(key).or_insert(0) += 1;
                }
            }
        }
        counts
    }

    /// Adopts what is visible now as already answered.
    pub fn seed(&mut self, counts: &HashMap<String, usize>) {
        for (key, &count) in counts {
            self.keys.insert(key.clone(), KeyState::new(count));
        }
    }

    /// Returns how many new instances must be answered.
    pub fn observe(
        &mut self,
        counts: &HashMap<String, usize>,
        now: u64,
        is_eligible: bool,
        is_quiet: bool,
    ) -> usize {
        for state in self.keys.values_mut() {
            state.expire_missing(now);
        }
        let mut fires = 0;
        for (key, &count) in counts {
            let admitted = self.keys.get(key).map_or(0, |state| state.admitted);
            if count <= admitted {
                continue;
            }
            let mut surplus = count - admitted;
            // A match that grows while it streams is the same instance under a longer key.
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
            // Ineligible surplus stays unadmitted so it fires once the pane becomes eligible; a
            // resize repaint is adopted silently.
            let admitted_now = if is_quiet || is_eligible { surplus } else { 0 };
            if is_eligible && !is_quiet {
                fires += surplus;
            }
            if adopted + admitted_now > 0 {
                let state = self
                    .keys
                    .entry(key.clone())
                    .or_insert_with(|| KeyState::new(0));
                state.admitted += adopted + admitted_now;
            }
        }
        for (key, state) in &mut self.keys {
            state.observe_visible(counts.get(key).copied().unwrap_or(0), now);
        }
        self.keys.retain(|_, state| state.admitted > 0);
        fires
    }

    /// Time spent on the alternate screen does not expire main-screen admissions.
    pub fn touch(&mut self, now: u64) {
        for state in self.keys.values_mut() {
            for loss in &mut state.missing {
                loss.since = now;
            }
        }
    }
}

/// One pane's trigger memory: the main and alternate screens are remembered separately.
#[derive(Default)]
pub struct Tracker {
    tables: [HashMap<String, RuleTable>; 2],
    is_alternate: bool,
    is_alternate_table_fresh: bool,
    has_started: bool,
    quiet_until: Option<u64>,
}

impl Tracker {
    /// A resize starts the quiet window once the tracker has seen output.
    pub fn resized(&mut self, now: u64) {
        if self.has_started {
            self.quiet_until = Some(now + RESIZE_QUIET_WINDOW_MS);
        }
    }

    /// Scans `lines` and returns the id of the rule for every new instance, one entry per instance.
    pub fn evaluate(
        &mut self,
        rules: &[Rule],
        lines: &[String],
        is_alternate: bool,
        kind: PaneKind,
        now: u64,
    ) -> Vec<String> {
        let enabled: Vec<&Rule> = rules.iter().filter(|rule| rule.enabled).collect();
        let mut fired = Vec::new();
        if enabled.is_empty() {
            self.tables = [HashMap::new(), HashMap::new()];
            self.has_started = true;
            return fired;
        }
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
        let is_quiet = self.quiet_until.is_some_and(|until| now < until);
        // Rules added while a pane already shows matching text adopt that text; a pane's first
        // sight of a rule adopts nothing.
        let seeds_new_rules =
            self.has_started && !(self.is_alternate && self.is_alternate_table_fresh);
        let table = &mut self.tables[usize::from(is_alternate)];
        table.retain(|id, _| enabled.iter().any(|rule| &rule.id == id));
        for rule in enabled {
            if table
                .get(&rule.id)
                .is_some_and(|existing| existing.regexp() != rule.regexp)
            {
                table.remove(&rule.id);
            }
            let is_new_rule = !table.contains_key(&rule.id);
            if is_new_rule {
                table.insert(rule.id.clone(), RuleTable::new(rule));
            }
            let Some(entry) = table.get_mut(&rule.id) else {
                continue;
            };
            let counts = entry.count_matches(lines);
            if is_new_rule && seeds_new_rules {
                entry.seed(&counts);
                continue;
            }
            let fires = entry.observe(&counts, now, target_matches(rule.target, kind), is_quiet);
            fired.extend(std::iter::repeat_n(rule.id.clone(), fires));
        }
        self.is_alternate_table_fresh = false;
        self.has_started = true;
        fired
    }
}

/// One due delivery: the send is due at `detected_at_ms + delay_ms`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub rule_id: String,
    pub message: String,
    pub detected_at_ms: u64,
    pub delay_ms: u64,
}

impl Delivery {
    pub fn due_ms(&self) -> u64 {
        self.detected_at_ms.saturating_add(self.delay_ms)
    }
}

/// Orders deliveries by their own due time (a short delay never queues behind a long one); equal
/// due times keep arrival order. Refuses past [`MAX_PENDING_DELIVERIES`].
#[derive(Default)]
pub struct DeliveryQueue {
    heap: BinaryHeap<Reverse<(u64, u64, usize)>>,
    items: Vec<Option<Delivery>>,
    sequence: u64,
}

impl DeliveryQueue {
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Returns the delivery back when the queue is full (backpressure).
    pub fn push(&mut self, delivery: Delivery) -> Result<(), Delivery> {
        if self.heap.len() >= MAX_PENDING_DELIVERIES {
            return Err(delivery);
        }
        self.sequence += 1;
        let slot = self
            .items
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.items.len());
        let due = delivery.due_ms();
        if slot == self.items.len() {
            self.items.push(Some(delivery));
        } else {
            self.items[slot] = Some(delivery);
        }
        self.heap.push(Reverse((due, self.sequence, slot)));
        Ok(())
    }

    /// The due time of the next delivery.
    pub fn next_due_ms(&self) -> Option<u64> {
        self.heap.peek().map(|Reverse((due, _, _))| *due)
    }

    /// Removes and returns every delivery due at or before `now_ms`, in due order.
    pub fn pop_due(&mut self, now_ms: u64) -> Vec<Delivery> {
        let mut due = Vec::new();
        while self
            .heap
            .peek()
            .is_some_and(|Reverse((when, _, _))| *when <= now_ms)
        {
            let Some(Reverse((_, _, slot))) = self.heap.pop() else {
                break;
            };
            if let Some(delivery) = self.items[slot].take() {
                due.push(delivery);
            }
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, regexp: &str) -> Rule {
        Rule {
            id: id.to_string(),
            regexp: regexp.to_string(),
            message: "go".to_string(),
            ..Rule::default()
        }
    }

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| line.to_string()).collect()
    }

    fn scan(tracker: &mut Tracker, rules: &[Rule], text: &[&str], now: u64) -> Vec<String> {
        tracker.evaluate(rules, &lines(text), false, PaneKind::PlainShell, now)
    }

    #[test]
    fn keys_collapse_whitespace_and_digit_runs() {
        assert_eq!(normalize_key("  retry   in 30s "), "retry in #s");
        assert_eq!(normalize_key("a\t\tb"), "a b");
        assert_eq!(normalize_key("   "), "");
        assert_eq!(
            normalize_key(&"x".repeat(2000)).chars().count(),
            MAX_KEY_CHARS
        );
    }

    #[test]
    fn target_filters_agents_and_plain_shells() {
        use PaneKind::*;
        assert!(target_matches(Target::Both, PlainShell) && target_matches(Target::Both, Agent));
        assert!(!target_matches(Target::Both, Other));
        assert!(
            target_matches(Target::Agents, Agent) && !target_matches(Target::Agents, PlainShell)
        );
        assert!(
            target_matches(Target::Terminals, PlainShell)
                && !target_matches(Target::Terminals, Agent)
        );
    }

    #[test]
    fn slices_end_at_the_newline_budget_or_the_byte_cap() {
        assert_eq!(slice_end(b"a\nb\nc\nd", 0, 2), 4);
        assert_eq!(slice_end(b"abc", 0, 5), 3);
        let long = vec![b'x'; MAX_SLICE_BYTES * 2];
        assert_eq!(slice_end(&long, 0, 5), MAX_SLICE_BYTES);
        assert_eq!(slice_end(&long, MAX_SLICE_BYTES, 5), MAX_SLICE_BYTES * 2);
    }

    #[test]
    fn a_new_instance_fires_once_however_long_it_stays_visible() {
        let rules = [rule("r", "ready$")];
        let mut tracker = Tracker::default();
        assert!(scan(&mut tracker, &rules, &["boot"], 0).is_empty());
        assert_eq!(scan(&mut tracker, &rules, &["boot", "ready"], 100), ["r"]);
        assert!(scan(&mut tracker, &rules, &["boot", "ready"], 200).is_empty());
        assert!(scan(&mut tracker, &rules, &["boot", "ready"], 10_000).is_empty());
    }

    #[test]
    fn a_clear_and_redraw_within_the_settle_window_does_not_refire() {
        let rules = [rule("r", "ready$")];
        let mut tracker = Tracker::default();
        scan(&mut tracker, &rules, &[], 0);
        assert_eq!(scan(&mut tracker, &rules, &["ready"], 10), ["r"]);
        assert!(scan(&mut tracker, &rules, &[], 20).is_empty());
        assert!(scan(&mut tracker, &rules, &["ready"], 20 + SETTLE_WINDOW_MS - 1).is_empty());
    }

    #[test]
    fn a_match_missing_for_the_settle_window_fires_again_when_it_returns() {
        let rules = [rule("r", "ready$")];
        let mut tracker = Tracker::default();
        scan(&mut tracker, &rules, &[], 0);
        assert_eq!(scan(&mut tracker, &rules, &["ready"], 10), ["r"]);
        scan(&mut tracker, &rules, &[], 20);
        assert_eq!(
            scan(&mut tracker, &rules, &["ready"], 20 + SETTLE_WINDOW_MS + 50),
            ["r"]
        );
    }

    #[test]
    fn two_identical_instances_fire_twice_and_digits_do_not_make_new_instances() {
        let rules = [rule("r", r"retry in \d+s")];
        let mut tracker = Tracker::default();
        scan(&mut tracker, &rules, &[], 0);
        assert_eq!(
            scan(&mut tracker, &rules, &["retry in 30s", "retry in 29s"], 10),
            ["r", "r"]
        );
        assert!(scan(&mut tracker, &rules, &["retry in 28s", "retry in 27s"], 20).is_empty());
    }

    #[test]
    fn a_growing_match_is_the_same_instance_under_a_longer_key() {
        let rules = [rule("r", "done.*")];
        let mut tracker = Tracker::default();
        scan(&mut tracker, &rules, &[], 0);
        assert_eq!(scan(&mut tracker, &rules, &["done"], 10), ["r"]);
        assert!(scan(&mut tracker, &rules, &["done now"], 20).is_empty());
    }

    #[test]
    fn an_ineligible_pane_fires_once_it_becomes_eligible() {
        let rules = [Rule {
            target: Target::Agents,
            ..rule("r", "ready$")
        }];
        let mut tracker = Tracker::default();
        tracker.evaluate(&rules, &[], false, PaneKind::PlainShell, 0);
        assert!(tracker
            .evaluate(&rules, &lines(&["ready"]), false, PaneKind::PlainShell, 10)
            .is_empty());
        assert_eq!(
            tracker.evaluate(&rules, &lines(&["ready"]), false, PaneKind::Agent, 20),
            ["r"]
        );
    }

    #[test]
    fn a_rule_added_while_text_is_visible_adopts_it_and_a_changed_regexp_resets() {
        let mut tracker = Tracker::default();
        let first = [rule("a", "alpha")];
        scan(&mut tracker, &first, &["alpha beta"], 0);
        let both = [rule("a", "alpha"), rule("b", "beta")];
        assert!(scan(&mut tracker, &both, &["alpha beta"], 10).is_empty());
        let edited = [rule("a", "alpha"), rule("b", "bet.")];
        assert!(scan(&mut tracker, &edited, &["alpha beta"], 20).is_empty());
    }

    #[test]
    fn disabled_and_removed_rules_never_fire_and_forget_their_state() {
        let mut off = rule("r", "ready$");
        off.enabled = false;
        let mut tracker = Tracker::default();
        scan(&mut tracker, &[off], &[], 0);
        assert!(scan(&mut tracker, &[rule("r", "ready$")], &["ready"], 10).is_empty());
    }

    #[test]
    fn a_resize_repaint_inside_the_quiet_window_is_adopted_without_firing() {
        let rules = [rule("r", "ready$")];
        let mut tracker = Tracker::default();
        scan(&mut tracker, &rules, &[], 0);
        tracker.resized(100);
        assert!(scan(&mut tracker, &rules, &["ready"], 200).is_empty());
        assert!(scan(
            &mut tracker,
            &rules,
            &["ready"],
            100 + RESIZE_QUIET_WINDOW_MS + 5
        )
        .is_empty());
    }

    #[test]
    fn the_alternate_screen_keeps_its_own_table() {
        let rules = [rule("r", "ready$")];
        let mut tracker = Tracker::default();
        scan(&mut tracker, &rules, &[], 0);
        assert_eq!(scan(&mut tracker, &rules, &["ready"], 10), ["r"]);
        // A fresh alternate screen is a new screen: the same text there is a new instance.
        assert_eq!(
            tracker.evaluate(&rules, &lines(&["ready"]), true, PaneKind::PlainShell, 20),
            ["r"]
        );
        // Leaving it restores the main text unchanged, which is not new output.
        assert!(tracker
            .evaluate(&rules, &lines(&["ready"]), false, PaneKind::PlainShell, 30)
            .is_empty());
    }

    #[test]
    fn deliveries_leave_in_due_order_and_equal_due_times_keep_arrival_order() {
        let mut queue = DeliveryQueue::default();
        let delivery = |id: &str, detected, delay| Delivery {
            rule_id: id.to_string(),
            message: id.to_string(),
            detected_at_ms: detected,
            delay_ms: delay,
        };
        queue.push(delivery("slow", 0, 5000)).unwrap();
        queue.push(delivery("first", 0, 1000)).unwrap();
        queue.push(delivery("second", 500, 500)).unwrap();
        assert_eq!(queue.next_due_ms(), Some(1000));
        assert!(queue.pop_due(999).is_empty());
        let ids: Vec<String> = queue
            .pop_due(1000)
            .into_iter()
            .map(|item| item.rule_id)
            .collect();
        assert_eq!(ids, ["first", "second"]);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.pop_due(10_000)[0].rule_id, "slow");
        assert!(queue.is_empty());
    }

    #[test]
    fn a_full_queue_hands_the_delivery_back() {
        let mut queue = DeliveryQueue::default();
        for index in 0..MAX_PENDING_DELIVERIES {
            let item = Delivery {
                rule_id: format!("r{index}"),
                message: String::new(),
                detected_at_ms: 0,
                delay_ms: 10,
            };
            queue.push(item).unwrap();
        }
        let extra = Delivery {
            rule_id: "extra".into(),
            message: String::new(),
            detected_at_ms: 0,
            delay_ms: 10,
        };
        assert_eq!(queue.push(extra.clone()), Err(extra));
        queue.pop_due(10);
        assert!(queue.is_empty());
    }
}
