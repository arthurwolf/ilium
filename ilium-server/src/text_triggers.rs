//! Server-owned Text Trigger matching. This runs on the PTY-output path, not
//! the client `ScreenUpdate` path, so detached and hidden panes behave exactly
//! like visible panes.
//!
//! # Identity model
//!
//! Instance identity is approximated by visible match counts, not row positions:
//! terminal programs move text without re-sending it and repaint unchanged text
//! elsewhere. The tracker maintains its own `vt100` screen from the pane's bytes
//! and counts each normalized matched text after every output slice.
//!
//! * more visible than admitted: the surplus are new instances and fire;
//! * fewer visible than admitted: each additional missing cohort starts its own
//!   [`SETTLE_WINDOW`] at the scan that first observes that loss. Earlier idle
//!   time while text was visible does not count as absence.
//!
//! This is not perfect semantic identity: a repaint that temporarily duplicates
//! text is indistinguishable from a new identical instance at that scan, and an
//! absence lasting the grace window re-arms even if old text later returns.
//!
//! Newline and byte limits improve sampling of fast output; they do not guarantee
//! observation of every transient screen state (for example cursor-only redraws).

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use ilium_core::{NodeId, NodeKind, PaneStatus};
use ilium_ipc::{TextTrigger, TextTriggerTarget};
use regex::Regex;
use tokio::sync::mpsc;

use crate::ipc::handlers::submit_text_trigger_if_current;
use crate::state::ServerState;
mod matching;
pub use matching::Matcher;

/// Grace period measured from the first scan observing an admission missing.
/// This is a repaint heuristic, not a bound on how long real redraws can take.
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
/// abort-on-drop worker. Full delivery queues apply backpressure instead of
/// discarding an already matched semantic decision.
pub struct TriggerDelivery {
    pub trigger_id: String,
    pub message: String,
    pub settings_revision: u64,
    /// Detection-to-send wait taken from the rule at match time.
    pub delay: Duration,
    /// When the match was detected; the send is due at `detected_at + delay`.
    pub detected_at: Instant,
    // Last field: original string allocations die before their storage lease.
    _retention: std::sync::Arc<ilium_execution::StorageAdmission>,
}

/// Delayed deliveries one pane may hold before the bounded channel applies
/// backpressure to the matcher.
const MAX_PENDING_DELIVERIES: usize = 64;

/// Owns the pane's delivery queue. Each delivery waits until its own due time;
/// a short delay never queues behind a long one. Equal due times keep arrival
/// order. Dropping this future drops every pending delivery with it.
pub async fn run_deliveries(
    state: std::sync::Arc<ServerState>,
    pane_id: NodeId,
    input: ilium_pty::PtyInput,
    mut receiver: mpsc::Receiver<TriggerDelivery>,
) {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    struct Pending(Reverse<(Instant, u64)>, TriggerDelivery);
    impl PartialEq for Pending {
        fn eq(&self, other: &Self) -> bool {
            self.0 == other.0
        }
    }
    impl Eq for Pending {}
    impl PartialOrd for Pending {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for Pending {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.0.cmp(&other.0)
        }
    }

    let mut pending: BinaryHeap<Pending> = BinaryHeap::new();
    let mut sequence = 0u64;
    let mut is_open = true;
    while is_open || !pending.is_empty() {
        let next_due = pending.peek().map(|entry| (entry.0).0 .0);
        let due_sleep = async {
            match next_due {
                Some(due) => tokio::time::sleep_until(tokio::time::Instant::from_std(due)).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            received = receiver.recv(), if is_open && pending.len() < MAX_PENDING_DELIVERIES => match received {
                Some(delivery) => {
                    let due = delivery.detected_at + delivery.delay;
                    sequence += 1;
                    pending.push(Pending(Reverse((due, sequence)), delivery));
                }
                None => is_open = false,
            },
            () = due_sleep => {
                let Some(Pending(_, delivery)) = pending.pop() else { continue };
                deliver(&state, pane_id, &input, delivery).await;
            }
        }
    }
}

async fn deliver(
    state: &ServerState,
    pane_id: NodeId,
    input: &ilium_pty::PtyInput,
    delivery: TriggerDelivery,
) {
    match submit_text_trigger_if_current(
        state,
        pane_id,
        &delivery.trigger_id,
        &delivery.message,
        input,
    )
    .await
    {
        Ok(true) => {
            tracing::debug!(pane_id = pane_id.0, trigger_id = %delivery.trigger_id, settings_revision = delivery.settings_revision, "text trigger semantic input acknowledged")
        }
        Ok(false) => {
            tracing::debug!(pane_id = pane_id.0, trigger_id = %delivery.trigger_id, "text trigger decision cancelled: pane replaced, or rule removed, disabled or edited")
        }
        Err(error) => {
            tracing::warn!(pane_id = pane_id.0, trigger_id = %delivery.trigger_id, %error, "text trigger submission failed")
        }
    }
}

/// Admissions first observed missing at the same scan.
struct MissingAdmissions {
    count: usize,
    since: Instant,
}

/// Admission state of one distinct matched text under one rule.
struct KeyState {
    /// Instances already answered (or deliberately adopted without an answer).
    admitted: usize,
    /// Oldest first. After reconciliation, their sum is
    /// `admitted.saturating_sub(visible_count)`.
    missing: VecDeque<MissingAdmissions>,
}

impl KeyState {
    fn new(admitted: usize) -> Self {
        Self {
            admitted,
            missing: VecDeque::new(),
        }
    }

    /// Expire only losses already observed before the current scan. In
    /// particular, a current erase must not inherit an earlier idle interval.
    fn expire_missing(&mut self, now: Instant) {
        while self
            .missing
            .front()
            .is_some_and(|loss| now.saturating_duration_since(loss.since) >= SETTLE_WINDOW)
        {
            let Some(loss) = self.missing.pop_front() else {
                break;
            };
            self.admitted -= loss.count;
        }
    }

    /// Reconcile after all surplus admissions and prefix-key transfers.
    fn observe_visible(&mut self, visible: usize, now: Instant) {
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
            // Counts cannot identify which missing instance returned or was
            // transferred. Retire the oldest loss first, conservatively keeping
            // the younger deadlines for any admissions that remain missing.
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
        debug_assert_eq!(
            self.missing.iter().map(|loss| loss.count).sum::<usize>(),
            required
        );
    }
}

const COMPILED_CACHE_BYTES: usize = 16 * 1024 * 1024;
const COMPILED_CACHE_ENTRIES: usize = 64;
struct CompiledEntry {
    pattern: String,
    regex: std::sync::Arc<regex_automata::meta::Regex>,
    bytes: usize,
}
#[derive(Default)]
struct CompiledCache {
    entries: VecDeque<CompiledEntry>,
    bytes: usize,
}
impl CompiledCache {
    fn get(&mut self, pattern: &str) -> Option<std::sync::Arc<regex_automata::meta::Regex>> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.pattern == pattern)
        {
            let entry = self.entries.remove(index)?;
            let regex = entry.regex.clone();
            self.entries.push_front(entry);
            return Some(regex);
        }
        // These are the exact regex1.12.3 string-facade builder defaults.
        // Semantic rule admissions are separate and survive computational eviction.
        let regex = regex_automata::meta::Regex::builder()
            .configure(
                regex_automata::meta::Regex::config()
                    .nfa_size_limit(Some(10 * 1024 * 1024))
                    .hybrid_cache_capacity(2 * 1024 * 1024)
                    .match_kind(regex_automata::MatchKind::LeftmostFirst)
                    .utf8_empty(true)
                    .pool_capacity(0),
            )
            .syntax(regex_automata::util::syntax::Config::default().utf8(true))
            .build(pattern)
            .ok()?;
        let bytes = regex
            .memory_usage()
            .saturating_mul(2)
            .saturating_add(pattern.len())
            .saturating_add(512);
        let regex = std::sync::Arc::new(regex);
        // Oversized valid patterns still match using the job's transient working
        // allowance; they simply do not become permanently resident cache entries.
        if bytes > COMPILED_CACHE_BYTES {
            return Some(regex);
        }
        while self.bytes.saturating_add(bytes) > COMPILED_CACHE_BYTES
            || self.entries.len() >= COMPILED_CACHE_ENTRIES
        {
            let evicted = self.entries.pop_back()?;
            self.bytes = self.bytes.saturating_sub(evicted.bytes);
        }
        self.bytes += bytes;
        self.entries.push_front(CompiledEntry {
            pattern: pattern.into(),
            regex: regex.clone(),
            bytes,
        });
        Some(regex)
    }
}
thread_local! {
    // Production callers execute on the fixed shared CPU bank. Its resident
    // declaration retains this TLS allocation through actual thread teardown.
    static COMPILED_REGEX: std::cell::RefCell<CompiledCache> = std::cell::RefCell::new(CompiledCache::default());
}
fn matching_regex(pattern: &str) -> Option<std::sync::Arc<regex_automata::meta::Regex>> {
    COMPILED_REGEX.with(|cache| cache.borrow_mut().get(pattern))
}

/// Semantic per-rule admission state. Replaceable compiled expressions live
/// in the CPU owner's bounded cache, independently of main/alternate tables.
struct RuleTable {
    regexp: String,
    keys: HashMap<String, KeyState>,
}

impl RuleTable {
    fn new(trigger: &TextTrigger) -> Self {
        Self {
            regexp: trigger.regexp.clone(),
            keys: HashMap::new(),
        }
    }

    #[cfg(test)]
    fn count_matches(&self, lines: &[String]) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        let Some(regex) = matching_regex(&self.regexp) else {
            return counts;
        };
        // Explicit cache bypasses meta's internal per-CPU cache pool. Only
        // one current rule's working cache exists during sequential evaluation.
        let mut cache = regex.create_cache();
        for line in lines {
            let mut matches =
                regex_automata::util::iter::Searcher::new(regex_automata::Input::new(line));
            while let Some(found) =
                matches.advance(|input| Ok(regex.search_with(&mut cache, input)))
            {
                let key = normalize_key(&line[found.range()]);
                if !key.is_empty() {
                    *counts.entry(key).or_insert(0) += 1;
                }
            }
        }
        counts
    }

    /// Adopts what is visible now as already answered.
    fn seed(&mut self, counts: &HashMap<String, usize>) {
        for (key, &count) in counts {
            self.keys.insert(key.clone(), KeyState::new(count));
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
        // Use only previously observed losses here. New losses are timestamped
        // below, after applying this scan's admissions and key transfers.
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

    fn touch(&mut self, now: Instant) {
        for state in self.keys.values_mut() {
            // Preserve the existing main-screen restoration policy: time spent
            // on the alternate screen does not expire main-screen admissions.
            for loss in &mut state.missing {
                loss.since = now;
            }
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
    #[cfg(test)]
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

    #[cfg(test)]
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
                let rule = RuleTable::new(trigger);
                table.insert(trigger.id.clone(), rule);
            }
            let Some(rule) = table.get_mut(&trigger.id) else {
                continue;
            };
            let counts = rule.count_matches(&lines);
            if is_new_rule && seeds_new_rules {
                rule.seed(&counts);
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
    _storage: std::sync::Arc<ilium_execution::StorageAdmission>,
}

pub(crate) fn execution_client(
    state: &ServerState,
) -> Result<crate::execution::ExecutionClient, String> {
    if let Some(execution) = state.execution.get() {
        return Ok(execution.client.clone());
    }
    #[cfg(test)]
    return Ok(crate::execution::test_general_client());
    #[cfg(not(test))]
    Err("Text Trigger execution owner is unavailable".to_owned())
}

/// Includes collection/string capacity, rather than the encoded wire length.
pub(crate) fn settings_bytes(settings: &ilium_ipc::TextTriggerSettings) -> Option<usize> {
    if settings.triggers.len() > 4096 {
        return None;
    }
    let mut bytes = std::mem::size_of_val(settings).checked_add(
        settings
            .triggers
            .capacity()
            .checked_mul(std::mem::size_of::<TextTrigger>())?,
    )?;
    for trigger in &settings.triggers {
        bytes = bytes
            .checked_add(trigger.id.capacity())?
            .checked_add(trigger.regexp.capacity())?
            .checked_add(trigger.message.capacity())?
            .checked_add(trigger.sample_text.capacity())?;
    }
    Some(bytes)
}

/// Validation executes on a real CPU bank. The admitted original settings
/// allocation transfers to a small, exact lifetime charge before publication.
pub(crate) struct AcceptedCandidate {
    pub(crate) settings: ilium_ipc::TextTriggerSettings,
    pub(crate) retention: std::sync::Arc<ilium_execution::StorageAdmission>,
    #[cfg(test)]
    pub(crate) validation_thread: std::thread::ThreadId,
}

pub(crate) async fn validate_in_worker(
    state: &ServerState,
    settings: ilium_ipc::TextTriggerSettings,
    storage: Option<std::sync::Arc<ilium_execution::StorageAdmission>>,
) -> Result<AcceptedCandidate, String> {
    const MAX_SETTINGS_BYTES: usize = 16 * 1024 * 1024;
    let bytes = settings_bytes(&settings)
        .filter(|bytes| *bytes <= MAX_SETTINGS_BYTES)
        .ok_or_else(|| {
            "Text Trigger settings exceed the 16 MiB retained allocation limit".to_owned()
        })?;
    let client = execution_client(state)?;
    let storage = match storage {
        Some(storage) => storage,
        None => client
            .reserve_storage(bytes.max(256))
            .await
            .map_err(|error| format!("Text Trigger retained settings admission: {error:?}"))?,
    };
    let reservation = client
        .reserve(
            ilium_execution::Lane::Cpu,
            ilium_execution::JobCost {
                input_bytes: 64 * 1024 * 1024,
                result_bytes: 32 * 1024 * 1024,
            },
        )
        .await
        .map_err(|error| format!("Text Trigger validation admission: {error:?}"))?;
    let validated = client
        .run_reserved(reservation, move |_context| {
            let error = validate_settings(&settings);
            Ok::<_, std::convert::Infallible>((
                settings,
                error,
                storage,
                std::thread::current().id(),
            ))
        })
        .await
        .map_err(|error| format!("Text Trigger validation failed: {error}"))?;
    if let Some(message) = &validated.view().1 {
        return Err(message.clone());
    }
    let ((settings, _, retention, validation_thread), validation_charge) = validated.into_parts();
    #[cfg(not(test))]
    let _ = validation_thread;
    let settings = AcceptedCandidate {
        settings,
        retention,
        #[cfg(test)]
        validation_thread,
    };
    drop(validation_charge);
    Ok(settings)
}

async fn load_context(state: &ServerState, pane_id: NodeId) -> Option<EvaluationContext> {
    use ilium_core::AllocationSize;
    let client = execution_client(state).ok()?;
    loop {
        let settings_size = {
            let accepted = state.text_trigger_settings.read().await;
            settings_bytes(&accepted.settings)?
        };
        let status_size = {
            let tree = state.tree.read().await;
            match &tree.get(pane_id)?.kind {
                NodeKind::Pane { status, .. } => status.retained_bytes(),
                _ => return None,
            }
        };
        let size = settings_size
            .saturating_add(status_size)
            .saturating_add(4096);
        let storage = client.reserve_storage(size).await.ok()?;
        let (settings, settings_revision) = {
            let accepted = state.text_trigger_settings.read().await;
            if settings_bytes(&accepted.settings)? > settings_size {
                continue;
            }
            (accepted.settings.clone(), accepted.revision)
        };
        let status = {
            let tree = state.tree.read().await;
            match &tree.get(pane_id)?.kind {
                NodeKind::Pane { status, .. } if status.retained_bytes() <= status_size => {
                    status.clone()
                }
                NodeKind::Pane { .. } => continue,
                _ => return None,
            }
        };
        return Some(EvaluationContext {
            status,
            settings,
            settings_revision,
            _storage: storage,
        });
    }
}

async fn enqueue_delivery(
    client: &crate::execution::ExecutionClient,
    trigger: &TextTrigger,
    settings_revision: u64,
    sender: &mpsc::Sender<TriggerDelivery>,
) -> Result<(), String> {
    let bytes = std::mem::size_of::<TriggerDelivery>()
        .checked_add(128)
        .and_then(|bytes| bytes.checked_add(trigger.id.capacity()))
        .and_then(|bytes| bytes.checked_add(trigger.message.capacity()))
        .ok_or_else(|| "Text Trigger delivery allocation overflow".to_owned())?;
    let retention = client
        .reserve_storage(bytes)
        .await
        .map_err(|error| format!("Text Trigger delivery storage admission: {error:?}"))?;
    // Admission precedes cloning. The queue and active semantic writer retain
    // the same allocation charge; awaiting a full queue holds no registry lock.
    sender
        .send(TriggerDelivery {
            trigger_id: trigger.id.clone(),
            message: trigger.message.clone(),
            settings_revision,
            delay: Duration::from_secs(u64::from(
                trigger
                    .delay_seconds
                    .min(ilium_ipc::MAX_TEXT_TRIGGER_DELAY_SECONDS),
            )),
            detected_at: Instant::now(),
            _retention: retention,
        })
        .await
        .map_err(|_| "Text Trigger delivery owner closed".to_owned())
}

pub async fn process_output(
    state: &ServerState,
    pane_id: NodeId,
    tracker: &mut Matcher,
    bytes: &[u8],
    delivery_sender: &mpsc::Sender<TriggerDelivery>,
) {
    let now = Instant::now();
    let client = match execution_client(state) {
        Ok(client) => client,
        Err(error) => {
            report_matching_error(state, pane_id, error);
            return;
        }
    };
    if let Some(pending) = tracker.pending_operation() {
        if let Err(error) = tracker.apply(pending, &client, delivery_sender).await {
            report_matching_error(state, pane_id, error);
            return;
        }
    }
    let replace = tracker.needs_resync;
    let (source, geometry, storage, origin) = if replace {
        match matching::project_current_screen(state, pane_id, &client).await {
            Ok(Some(projected)) => (
                projected.bytes,
                projected.geometry,
                projected.storage,
                Some((projected.input, projected.reader, projected.resize_epoch)),
            ),
            Ok(None) => {
                tracker.needs_resync = true;
                return;
            }
            Err(error) => {
                report_matching_error(state, pane_id, error);
                return;
            }
        }
    } else {
        let Some(geometry) =
            crate::pane::read_current_terminal_screen(state, pane_id, vt100::Screen::size).await
        else {
            tracker.needs_resync = true;
            return;
        };
        let size = bytes
            .len()
            .saturating_add(std::mem::size_of::<matching::Operation>())
            .saturating_add(4096);
        let storage = match client.reserve_storage(size).await {
            Ok(storage) => storage,
            Err(error) => {
                report_matching_error(
                    state,
                    pane_id,
                    format!("Text Trigger original feed admission: {error:?}"),
                );
                return;
            }
        };
        (bytes.to_vec(), geometry, storage, None)
    };
    let Some(context) = load_context(state, pane_id).await else {
        return;
    };
    if let Some((input, reader, resize_epoch)) = &origin {
        // load_context awaited after screen preparation. Revalidate the
        // originating session and resize epoch at operation acceptance;
        // dimensions alone cannot detect an away-and-back resize.
        if !crate::pane::terminal_reader_is_current(state, pane_id, input).await
            || reader.try_with_screen_and_resize_epoch(|screen, epoch| (screen.size(), epoch))
                != Some((geometry, *resize_epoch))
        {
            tracker.needs_resync = true;
            return;
        }
    }
    let operation = std::sync::Arc::new(matching::Operation {
        context: std::sync::Arc::new(context),
        bytes: source,
        geometry,
        replace,
        now,
        _storage: storage,
    });
    if let Err(error) = tracker.apply(operation, &client, delivery_sender).await {
        report_matching_error(state, pane_id, error);
    }
}
fn report_matching_error(state: &ServerState, pane_id: NodeId, error: String) {
    tracing::error!(pane_id=pane_id.0,%error,"Text Trigger matching did not settle; original state was not reset");
    state.broadcast(ilium_ipc::ServerEvent::Error {
        message: format!("Text Trigger matching for pane{}: {error}", pane_id.0),
    });
}
/// Sampling gaps adopt the authoritative screen while retaining admissions.
/// Transient matches lost by the broadcast ring remain explicitly unverified.
pub async fn resync_after_gap(
    state: &ServerState,
    pane_id: NodeId,
    tracker: &mut Matcher,
    delivery_sender: &mpsc::Sender<TriggerDelivery>,
) {
    tracker.needs_resync = true;
    process_output(state, pane_id, tracker, &[], delivery_sender).await;
}

/// Validates candidates before the detached server replaces its active list.
/// Messages stay literal single lines so each rule has one text stage and one
/// later Enter stage.
pub fn validate_settings(settings: &ilium_ipc::TextTriggerSettings) -> Option<String> {
    if let Err(message) = settings.validate_identities() {
        return Some(message);
    }
    for (index, trigger) in settings.triggers.iter().enumerate() {
        if trigger.regexp.is_empty() {
            return Some(format!(
                "Text Trigger {} regexp must not be empty",
                index + 1
            ));
        }
        if trigger.delay_seconds > ilium_ipc::MAX_TEXT_TRIGGER_DELAY_SECONDS {
            return Some(format!(
                "Text Trigger {} delay must be at most {} seconds",
                index + 1,
                ilium_ipc::MAX_TEXT_TRIGGER_DELAY_SECONDS
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
            PaneStatus::PlainShell | PaneStatus::Agent(..),
        ) | (TextTriggerTarget::Agents, PaneStatus::Agent(..))
            | (TextTriggerTarget::Terminals, PaneStatus::PlainShell)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_ipc::{TextTrigger, TextTriggerSettings};

    #[tokio::test]
    async fn a_full_delivery_queue_preserves_matched_decisions_and_literal_fifo_messages() {
        let client = crate::execution::test_general_client();
        let mut first = TextTrigger {
            id: "first".into(),
            message: "literal first".into(),
            ..TextTrigger::default()
        };
        let (sender, mut receiver) = mpsc::channel(1);
        enqueue_delivery(&client, &first, 7, &sender).await.unwrap();
        first.id = "second".into();
        first.message = "literal second".into();
        let (entered_sender, entered_receiver) = tokio::sync::oneshot::channel();
        let owned_sender = sender.clone();
        let mut pending = tokio::spawn(async move {
            entered_sender.send(()).unwrap();
            enqueue_delivery(&client, &first, 8, &owned_sender).await
        });
        entered_receiver.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut pending)
                .await
                .is_err()
        );
        let admitted = receiver.recv().await.unwrap();
        assert_eq!(admitted.trigger_id, "first");
        assert_eq!(admitted.message, "literal first");
        assert_eq!(admitted.settings_revision, 7);
        pending.await.unwrap().unwrap();
        let admitted = receiver.recv().await.unwrap();
        assert_eq!(admitted.trigger_id, "second");
        assert_eq!(admitted.message, "literal second");
        assert_eq!(admitted.settings_revision, 8);
    }

    fn rule(regexp: &str) -> Vec<TextTrigger> {
        vec![TextTrigger {
            id: "rule".to_string(),
            enabled: true,
            regexp: regexp.to_string(),
            message: "reply".to_string(),
            target: TextTriggerTarget::Both,
            sample_text: String::new(),
            delay_seconds: 0,
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
    fn explicit_matching_cache_preserves_string_regex_defaults() {
        let patterns = [
            r"",
            r"foo|foobar",
            r"\b\w+\b",
            r"(?i)ä+",
            r"(?m)^.*$",
            r"a*?",
            r"(?s).+",
            r"(?-u:[a-z]+)",
            r"[\p{Greek}\p{Emoji}]+",
        ];
        let lines = ["", "foobar foo", "äÄ aaa", "αβ γ  🐈", "a\nb\r\n", "é.a"];
        for pattern in patterns {
            let facade = regex::Regex::new(pattern).unwrap();
            let direct = matching_regex(pattern).unwrap();
            let mut cache = direct.create_cache();
            for line in lines {
                let expected = facade
                    .find_iter(line)
                    .map(|matched| matched.range())
                    .collect::<Vec<_>>();
                let mut matches =
                    regex_automata::util::iter::Searcher::new(regex_automata::Input::new(line));
                let mut actual = Vec::new();
                while let Some(matched) =
                    matches.advance(|input| Ok(direct.search_with(&mut cache, input)))
                {
                    actual.push(matched.range());
                }
                assert_eq!(actual, expected, "pattern={pattern:?}, line={line:?}");
            }
        }
    }

    #[test]
    fn compiled_cache_evicts_computation_without_resetting_semantic_keys() {
        let mut cache = CompiledCache::default();
        let original = cache.get("needle").unwrap();
        for index in 0..COMPILED_CACHE_ENTRIES + 8 {
            cache.get(&format!("pattern{index}")).unwrap();
        }
        assert!(cache.entries.len() <= COMPILED_CACHE_ENTRIES);
        assert!(cache.bytes <= COMPILED_CACHE_BYTES);
        let replacement = cache.get("needle").unwrap();
        assert!(!std::sync::Arc::ptr_eq(&original, &replacement));
        let mut tracker = TriggerTracker::default();
        let trigger = TextTrigger {
            id: "cache-rule".into(),
            enabled: true,
            regexp: "needle".into(),
            message: "reply".into(),
            sample_text: String::new(),
            delay_seconds: 0,
            target: TextTriggerTarget::Both,
        };
        let now = Instant::now();
        assert_eq!(
            tracker.feed(
                b"needle",
                std::slice::from_ref(&trigger),
                &PaneStatus::PlainShell,
                now
            ),
            vec!["cache-rule"]
        );
        COMPILED_REGEX.with(|cache| *cache.borrow_mut() = CompiledCache::default());
        assert!(tracker
            .feed(
                &[],
                &[trigger],
                &PaneStatus::PlainShell,
                now + Duration::from_millis(1)
            )
            .is_empty());
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
        harness.status = PaneStatus::from_activity(
            ilium_core::AgentClass::Codex,
            ilium_core::AgentActivity::Working,
            None,
        );
        assert_eq!(harness.rescan(), 1);
        harness.status = PaneStatus::PlainShell;
        assert_eq!(harness.rescan(), 0);
        harness.status = PaneStatus::from_activity(
            ilium_core::AgentClass::Codex,
            ilium_core::AgentActivity::Idle,
            None,
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
    fn relocating_one_occurrence_after_idle_does_not_refire() {
        let mut harness = Harness::new("abracrabdara", 6, 60);
        assert_eq!(harness.feed(b"\x1b[5;1Habracrabdara, may I proceed?"), 1);
        let mut previous_row = 5;
        for row in [4, 3, 2] {
            harness.wait(SETTLE_WINDOW * 4);
            let erase = format!("\x1b[{previous_row};1H\x1b[2K");
            assert_eq!(harness.feed(erase.as_bytes()), 0);
            let repaint = format!("\x1b[{row};1Habracrabdara, may I proceed?");
            // Both feeds use the same Instant; the observed absence is zero.
            assert_eq!(harness.feed(repaint.as_bytes()), 0, "row {row}");
            previous_row = row;
        }
        assert_eq!(harness.fired, 1);
    }

    #[test]
    fn one_feed_split_after_newlines_does_not_expire_a_fresh_loss() {
        let mut harness = Harness::new("abracrabdara", 6, 60);
        assert_eq!(harness.feed(b"\x1b[3;1Habracrabdara, may I proceed?"), 1);
        harness.wait(SETTLE_WINDOW * 4);
        let erase = b"\x1b[3;1H\x1b[2K\r\n\r\n\r\n";
        let mut repaint = erase.to_vec();
        repaint.extend_from_slice(b"\x1b[2;1Habracrabdara, may I proceed?");
        assert_eq!(slice_end(&repaint, 0, 3), erase.len());
        assert_eq!(harness.feed(&repaint), 0);
        assert_eq!(harness.fired, 1);
    }

    #[test]
    fn one_feed_split_at_byte_budget_does_not_expire_a_fresh_loss() {
        let mut harness = Harness::new("abracrabdara", 6, 60);
        assert_eq!(harness.feed(b"\x1b[3;1Habracrabdara, may I proceed?"), 1);
        harness.wait(SETTLE_WINDOW * 4);
        let mut repaint = b"\x1b[3;1H\x1b[2K".to_vec();
        // NUL padding is inert screen output, and creates an exact byte boundary.
        repaint.resize(MAX_SLICE_BYTES, 0);
        repaint.extend_from_slice(b"\x1b[2;1Habracrabdara, may I proceed?");
        assert_eq!(slice_end(&repaint, 0, 3), MAX_SLICE_BYTES);
        assert_eq!(harness.feed(&repaint), 0);
        assert_eq!(harness.fired, 1);
    }

    #[test]
    fn idle_before_erasure_is_not_part_of_the_missing_window() {
        let mut harness = Harness::new("^trigger-ready$", 4, 40);
        assert_eq!(harness.feed(b"trigger-ready"), 1);
        harness.wait(SETTLE_WINDOW * 4);
        assert_eq!(harness.feed(b"\x1b[H\x1b[2Jwaiting"), 0);
        harness.wait(SETTLE_WINDOW - Duration::from_millis(1));
        assert_eq!(harness.feed(b"\x1b[H\x1b[2Jtrigger-ready"), 0);
        assert_eq!(harness.fired, 1);
    }

    #[test]
    fn a_full_observed_absence_rearms_without_an_intermediate_rescan() {
        let mut harness = Harness::new("^trigger-ready$", 4, 40);
        assert_eq!(harness.feed(b"trigger-ready"), 1);
        harness.wait(SETTLE_WINDOW * 4);
        assert_eq!(harness.feed(b"\x1b[H\x1b[2Jwaiting"), 0);
        harness.wait(SETTLE_WINDOW);
        assert_eq!(harness.feed(b"\x1b[H\x1b[2Jtrigger-ready"), 1);
        assert_eq!(harness.fired, 2);
    }

    #[test]
    fn independent_identical_lines_still_each_fire_without_a_cooldown() {
        let mut harness = Harness::new("abracrabdara", 10, 60);
        assert_eq!(harness.feed(b"abracrabdara\r\nabracrabdara\r\n"), 2);
        assert_eq!(harness.feed(b"abracrabdara\r\n"), 1);
        harness.wait(SETTLE_WINDOW * 4);
        assert_eq!(harness.feed(b"abracrabdara\r\n"), 1);
        assert_eq!(harness.fired, 4);
    }

    #[test]
    fn a_return_cancels_its_loss_before_another_erase() {
        let mut harness = Harness::new("abracrabdara", 4, 60);
        assert_eq!(harness.feed(b"abracrabdara"), 1);
        for _ in 0..3 {
            harness.wait(SETTLE_WINDOW * 4);
            assert_eq!(harness.feed(b"\x1b[H\x1b[2J"), 0);
            harness.wait(SETTLE_WINDOW / 2);
            assert_eq!(harness.feed(b"abracrabdara"), 0);
        }
        assert_eq!(harness.fired, 1);
    }

    #[test]
    fn later_losses_do_not_inherit_the_first_loss_deadline() {
        let mut harness = Harness::new("abracrabdara", 6, 60);
        assert_eq!(
            harness.feed(b"\x1b[2;1Habracrabdara\x1b[4;1Habracrabdara"),
            2
        );
        assert_eq!(harness.feed(b"\x1b[2;1H\x1b[2K"), 0);
        harness.wait(SETTLE_WINDOW / 2);
        assert_eq!(harness.feed(b"\x1b[4;1H\x1b[2K"), 0);
        harness.wait(SETTLE_WINDOW / 2);
        assert_eq!(harness.rescan(), 0);
        let state = &harness.tracker.tables[0]["rule"].keys["abracrabdara"];
        assert_eq!(state.admitted, 1);
        assert_eq!(harness.feed(b"\x1b[2;1Habracrabdara"), 0);
        assert_eq!(harness.feed(b"\x1b[4;1Habracrabdara"), 1);
        assert_eq!(harness.fired, 3);
    }

    #[test]
    fn an_ambiguous_return_retires_the_oldest_loss_first() {
        let now = Instant::now();
        let mut state = KeyState::new(2);
        state.observe_visible(1, now);
        state.observe_visible(0, now + SETTLE_WINDOW / 2);
        state.observe_visible(1, now + SETTLE_WINDOW * 3 / 4);
        state.expire_missing(now + SETTLE_WINDOW);
        assert_eq!(state.admitted, 2);
        assert_eq!(state.missing.len(), 1);
        assert_eq!(
            state.missing.front().unwrap().since,
            now + SETTLE_WINDOW / 2
        );
        state.observe_visible(2, now + SETTLE_WINDOW);
        assert!(state.missing.is_empty());
    }

    #[test]
    fn a_greedy_key_can_still_adopt_a_fresh_loss_after_idle() {
        let mut harness = Harness::new("proceed.*", 6, 60);
        assert_eq!(harness.feed(b"proceed"), 1);
        harness.wait(SETTLE_WINDOW * 4);
        assert_eq!(harness.feed(b"\x1b[2K\x1b[G"), 0);
        assert_eq!(harness.feed(b"proceed with the change?"), 0);
        assert_eq!(harness.feed(b"\r\nproceed with the change?"), 1);
        assert_eq!(harness.fired, 2);
    }

    #[test]
    fn main_screen_missing_admissions_do_not_age_on_the_alternate_screen() {
        let mut harness = Harness::new("abracrabdara", 6, 60);
        assert_eq!(harness.feed(b"abracrabdara"), 1);
        assert_eq!(harness.feed(b"\x1b[H\x1b[2J"), 0);
        assert_eq!(harness.feed(b"\x1b[?1049halternate"), 0);
        harness.wait(SETTLE_WINDOW * 4);
        assert_eq!(harness.feed(b"\x1b[?1049l"), 0);
        assert_eq!(harness.feed(b"\x1b[Habracrabdara"), 0);
        assert_eq!(harness.fired, 1);
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
        let agent = PaneStatus::from_activity(
            ilium_core::AgentClass::Codex,
            ilium_core::AgentActivity::Working,
            None,
        );
        assert!(target_matches(TextTriggerTarget::Agents, &agent));
        assert!(!target_matches(TextTriggerTarget::Terminals, &agent));
    }
}
