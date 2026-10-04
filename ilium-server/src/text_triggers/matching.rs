//! Resumable finite CPU ownership of one ordered trigger engine.
use super::*;
use ilium_execution::{Job, JobContext, JobCost, JobOutcome, JobPoll, StorageAdmission};
use std::sync::Arc;
const MIB: usize = 1024 * 1024;
const COST: JobCost = JobCost {
    input_bytes: 64 * MIB,
    result_bytes: MIB,
};
const STEP_MATCHES: usize = 128;

const MAX_PROJECTION_BYTES: usize = 64 * MIB;
struct ScreenProjection {
    input: ilium_pty::PtyInput,
    reader: ilium_pty::ScreenReader,
    geometry: (u16, u16),
    resize_epoch: u64,
    capacity: usize,
    // Admitted before formatting; follows the projected bytes into Operation.
    storage: Arc<StorageAdmission>,
}
pub(super) struct ProjectedScreen {
    pub input: ilium_pty::PtyInput,
    pub reader: ilium_pty::ScreenReader,
    pub resize_epoch: u64,
    pub bytes: Vec<u8>,
    pub geometry: (u16, u16),
    pub storage: Arc<StorageAdmission>,
}
impl Job for ScreenProjection {
    type Output = Option<ProjectedScreen>;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, String> {
        if context.stop_requested() {
            return Err("Text Trigger resync preparation cancelled".into());
        }
        self.reader
            .try_with_screen_and_resize_epoch(|screen, epoch| {
                if (screen.size(), epoch) != (self.geometry, self.resize_epoch) {
                    return Ok(None);
                }
                let bytes = screen.state_formatted();
                if bytes.capacity() > self.capacity {
                    return Err(
                        "Text Trigger formatted-screen capacity exceeded its preflight declaration"
                            .into(),
                    );
                }
                Ok(Some(ProjectedScreen {
                    input: self.input,
                    reader: self.reader.clone(),
                    resize_epoch: self.resize_epoch,
                    bytes,
                    geometry: self.geometry,
                    storage: self.storage,
                }))
            })
            .unwrap_or(Ok(None))
    }
}
/// Formatting is exclusively admitted CPU work. Never uses the retained
/// pre-resize screen; replacement and geometry are checked before publication.
pub(super) async fn project_current_screen(
    state: &ServerState,
    pane_id: NodeId,
    client: &crate::execution::ExecutionClient,
) -> Result<Option<ProjectedScreen>, String> {
    // This deadline covers registry capture, storage/CPU admission and job
    // completion. Dropping run_reserved cancels its owned receipt; it never
    // replays input or releases the physical job's custody prematurely.
    tokio::time::timeout(
        Duration::from_secs(2),
        project_current_screen_inner(state, pane_id, client),
    )
    .await
    .map_err(|_| "Text Trigger resync preparation exceeded its 2-second deadline".to_owned())?
}

async fn project_current_screen_inner(
    state: &ServerState,
    pane_id: NodeId,
    client: &crate::execution::ExecutionClient,
) -> Result<Option<ProjectedScreen>, String> {
    let Some((input, reader, mut changed)) =
        crate::pane::current_terminal_reader(state, pane_id).await
    else {
        return Ok(None);
    };
    let mut status = input.subscribe_status();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if !crate::pane::terminal_reader_is_current(state, pane_id, &input).await {
            return Ok(None);
        }
        if let Some((geometry, resize_epoch)) =
            reader.try_with_screen_and_resize_epoch(|screen, epoch| (screen.size(), epoch))
        {
            // Locked vt100 cells hold at most22 UTF-8 bytes.512 bytes/cell
            // covers attributes/cursor escapes plus Vec growth; state_formatted
            // contains visible cells and input modes, never title/OSC strings.
            let capacity = usize::from(geometry.0)
                .checked_mul(usize::from(geometry.1))
                .and_then(|cells| cells.checked_mul(512))
                .and_then(|bytes| bytes.checked_add(usize::from(geometry.0) * 128 + 4096))
                .filter(|bytes| *bytes <= MAX_PROJECTION_BYTES)
                .ok_or_else(|| {
                    "Text Trigger resync frame exceeds its64MiB preparation limit".to_owned()
                })?;
            let storage = client
                .reserve_storage(capacity)
                .await
                .map_err(|reason| format!("Text Trigger resync storage admission: {reason:?}"))?;
            let reservation = client
                .reserve(
                    ilium_execution::Lane::Cpu,
                    JobCost {
                        input_bytes: 64 * MIB,
                        result_bytes: 64 * MIB,
                    },
                )
                .await
                .map_err(|reason| format!("Text Trigger resync CPU admission: {reason:?}"))?;
            let result = client
                .run_reserved(
                    reservation,
                    ScreenProjection {
                        input: input.clone(),
                        reader: reader.clone(),
                        geometry,
                        resize_epoch,
                        capacity,
                        storage,
                    },
                )
                .await
                .map_err(|error| format!("Text Trigger resync preparation: {error}"))?;
            let (projected, hold) = result.into_parts();
            drop(hold);
            if let Some(projected) = projected {
                if !crate::pane::terminal_reader_is_current(state, pane_id, &input).await {
                    return Ok(None);
                }
                if reader.try_with_screen_and_resize_epoch(|screen, epoch| (screen.size(), epoch))
                    == Some((projected.geometry, projected.resize_epoch))
                {
                    return Ok(Some(projected));
                }
            }
        }
        if !matches!(input.status(), ilium_pty::OwnerStatus::Running) {
            return Ok(None);
        }
        tokio::select! {
            result = changed.changed() => { if result.is_err() { return Ok(None); } }
            result = status.changed() => { if result.is_err() { return Ok(None); } }
            () = tokio::time::sleep_until(deadline) => return Ok(None),
        }
    }
}

pub(super) struct Operation {
    pub context: Arc<EvaluationContext>,
    pub bytes: Vec<u8>,
    pub geometry: (u16, u16),
    pub replace: bool,
    pub now: Instant,
    // Payload storage is admitted before cloning and follows every job clone.
    pub _storage: Arc<StorageAdmission>,
}
#[derive(Default)]
pub struct Matcher {
    state: Option<Engine>,
    bootstrap: Option<Storage>,
    pending: Option<Arc<Operation>>,
    pending_delivery: Option<(usize, usize)>,
    failed: Option<String>,
    pub(super) needs_resync: bool,
}
struct Engine {
    tracker: TriggerTracker,
    #[cfg(test)]
    execution_thread: Option<std::thread::ThreadId>,
    phase: Phase,
    offset: usize,
    empty_scan_done: bool,
    initialized: bool,
    max_rows: usize,
    max_columns: usize,
    osc: OscBudget,
    storage: Storage,
}
impl Default for Engine {
    fn default() -> Self {
        Self {
            tracker: TriggerTracker::default(),
            #[cfg(test)]
            execution_thread: None,
            phase: Phase::Feed,
            offset: 0,
            empty_scan_done: false,
            initialized: false,
            max_rows: 24,
            max_columns: 80,
            osc: OscBudget::default(),
            storage: Storage::default(),
        }
    }
}
#[derive(Default)]
struct Storage {
    leases: Vec<(usize, Arc<StorageAdmission>)>,
    bytes: usize,
    requested: usize,
}
impl Storage {
    fn ensure(
        &mut self,
        required: usize,
        client: &crate::execution::ExecutionClient,
    ) -> Result<(), ilium_execution::RejectReason> {
        if required <= self.bytes {
            return Ok(());
        }
        // Geometric declarations bound lease metadata and avoid one lease per
        // key. No per-pane fixed compiled-regex lease or semantic key clipping.
        let target = required.max(self.bytes.saturating_mul(2)).max(64 * 1024);
        let increment = target.saturating_sub(self.bytes).saturating_add(256);
        self.requested = increment;
        let lease = client.try_reserve_storage(increment)?;
        self.bytes = self.bytes.saturating_add(increment);
        self.leases.push((increment, lease));
        Ok(())
    }
    fn shrink(&mut self, required: usize) {
        while let Some((bytes, _)) = self.leases.last() {
            if self.bytes.saturating_sub(*bytes) < required.max(64 * 1024) {
                break;
            }
            self.bytes -= *bytes;
            self.leases.pop();
        }
    }
}
/// Conservative high-water for vte0.15's private std OSC Vec. Installed
/// vte terminates OSC on BEL/CAN/SUB/ESC; C0 bytes otherwise leave it active.
#[derive(Default, Clone, Copy)]
struct OscBudget {
    escaped: bool,
    active: bool,
    current: usize,
    peak: usize,
}
impl OscBudget {
    fn advance(mut self, bytes: &[u8]) -> Self {
        for &byte in bytes {
            if self.active {
                match byte {
                    7 | 0x18 | 0x1a | 0x1b => {
                        self.active = false;
                        self.current = 0;
                        self.escaped = byte == 0x1b;
                    }
                    _ => {
                        self.current = self.current.saturating_add(1);
                        self.peak = self.peak.max(self.current);
                    }
                }
            } else if self.escaped {
                match byte {
                    b']' => {
                        self.active = true;
                        self.escaped = false;
                    }
                    0x1b => {}
                    0x18 | 0x1a => self.escaped = false,
                    0..=0x1f | 0x7f..=0xff => {}
                    _ => self.escaped = false,
                }
            } else if byte == 0x1b {
                self.escaped = true;
            }
        }
        self
    }
}
enum Phase {
    Feed,
    Scan(Scan),
}
struct Scan {
    lines: Vec<String>,
    rules: Vec<usize>,
    rule: usize,
    seeds: bool,
    quiet: bool,
    counts: Counts,
}
#[derive(Default)]
struct Counts {
    values: HashMap<String, usize>,
    line: usize,
    start: usize,
    last_end: Option<usize>,
}
struct StepJob {
    engine: Option<Engine>,
    bootstrap: Option<Storage>,
    operation: Arc<Operation>,
    client: crate::execution::ExecutionClient,
}
struct StepOutput {
    engine: Option<Engine>,
    decision: Option<(usize, usize)>,
    done: bool,
    blocked: Option<(ilium_execution::RejectReason, usize)>,
}
impl Job for StepJob {
    type Output = StepOutput;
    type Error = std::convert::Infallible;
    fn run(self, context: JobContext) -> Result<StepOutput, Self::Error> {
        let mut engine = match self.engine {
            Some(engine) => engine,
            None => {
                // Startup has no uncharged parser allocation held while waiting.
                let bootstrap = 64 * 1024 + 24 * 80 * 128 + 24 * 256;
                let storage = match self.bootstrap {
                    Some(storage) => storage,
                    None => {
                        let lease = match self.client.try_reserve_storage(bootstrap) {
                            Ok(lease) => lease,
                            Err(reason) => {
                                return Ok(StepOutput {
                                    engine: None,
                                    decision: None,
                                    done: false,
                                    blocked: Some((reason, bootstrap)),
                                })
                            }
                        };
                        Storage {
                            leases: vec![(bootstrap, lease)],
                            bytes: bootstrap,
                            requested: 0,
                        }
                    }
                };
                Engine {
                    storage,
                    ..Engine::default()
                }
            }
        };
        if context.stop_requested() {
            return Ok(StepOutput {
                engine: Some(engine),
                decision: None,
                done: false,
                blocked: Some((ilium_execution::RejectReason::Closed, 0)),
            });
        }
        #[cfg(test)]
        {
            engine.execution_thread = Some(std::thread::current().id());
        }
        let result = engine.advance(&self.operation, &self.client);
        Ok(match result {
            Ok((decision, done)) => StepOutput {
                engine: Some(engine),
                decision,
                done,
                blocked: None,
            },
            Err(reason) => {
                let requested = engine.storage.requested;
                StepOutput {
                    engine: Some(engine),
                    decision: None,
                    done: false,
                    blocked: Some((reason, requested)),
                }
            }
        })
    }
}
fn table_bytes(table: &HashMap<String, RuleTable>) -> usize {
    let mut bytes = table.capacity().saturating_mul(256);
    for (id, rule) in table {
        bytes = bytes
            .saturating_add(id.capacity())
            .saturating_add(rule.regexp.capacity())
            .saturating_add(rule.keys.capacity().saturating_mul(256));
        for (key, state) in &rule.keys {
            bytes = bytes.saturating_add(key.capacity()).saturating_add(
                state
                    .missing
                    .capacity()
                    .saturating_mul(std::mem::size_of::<MissingAdmissions>()),
            );
        }
    }
    bytes
}
impl Engine {
    fn bytes(&self) -> usize {
        let mut bytes = self
            .max_rows
            .saturating_mul(self.max_columns)
            .saturating_mul(128)
            .saturating_add(self.max_rows.saturating_mul(256))
            .saturating_add(64 * 1024)
            .saturating_add(self.osc.peak.saturating_mul(2));
        for table in &self.tracker.tables {
            bytes = bytes.saturating_add(table_bytes(table));
        }
        if let Phase::Scan(scan) = &self.phase {
            bytes = bytes
                .saturating_add(
                    scan.lines
                        .capacity()
                        .saturating_mul(std::mem::size_of::<String>()),
                )
                .saturating_add(
                    scan.rules
                        .capacity()
                        .saturating_mul(std::mem::size_of::<usize>()),
                )
                .saturating_add(scan.counts.values.capacity().saturating_mul(256));
            for line in &scan.lines {
                bytes = bytes.saturating_add(line.capacity());
            }
            for key in scan.counts.values.keys() {
                bytes = bytes.saturating_add(key.capacity());
            }
        }
        bytes
    }
    fn advance(
        &mut self,
        operation: &Operation,
        client: &crate::execution::ExecutionClient,
    ) -> Result<(Option<(usize, usize)>, bool), ilium_execution::RejectReason> {
        if !self.initialized {
            self.max_rows = self.max_rows.max(usize::from(operation.geometry.0));
            self.max_columns = self.max_columns.max(usize::from(operation.geometry.1));
            self.storage.ensure(
                self.bytes()
                    .saturating_add(operation.bytes.len().saturating_mul(4)),
                client,
            )?;
            if operation.replace {
                let osc = OscBudget::default().advance(&operation.bytes);
                self.storage.ensure(
                    self.bytes().saturating_add(osc.peak.saturating_mul(2)),
                    client,
                )?;
                self.tracker.replace_screen(
                    operation.geometry.0,
                    operation.geometry.1,
                    &operation.bytes,
                );
                self.osc = osc;
            } else {
                self.tracker
                    .resize(operation.geometry.0, operation.geometry.1, operation.now);
            }
            self.initialized = true;
        }
        if matches!(self.phase, Phase::Feed) {
            return self.feed_slice(operation, client);
        }
        self.scan_rule(operation, client)
    }
    fn feed_slice(
        &mut self,
        operation: &Operation,
        client: &crate::execution::ExecutionClient,
    ) -> Result<(Option<(usize, usize)>, bool), ilium_execution::RejectReason> {
        let bytes = if operation.replace {
            &[][..]
        } else {
            operation.bytes.as_slice()
        };
        let enabled = operation
            .context
            .settings
            .triggers
            .iter()
            .enumerate()
            .filter_map(|(index, trigger)| trigger.enabled.then_some(index))
            .collect::<Vec<_>>();
        if enabled.is_empty() {
            let osc = self.osc.advance(&bytes[self.offset..]);
            self.storage.ensure(
                self.bytes().saturating_add(osc.peak.saturating_mul(2)),
                client,
            )?;
            self.tracker.shadow.process(&bytes[self.offset..]);
            self.osc = osc;
            self.tracker.tables = [HashMap::new(), HashMap::new()];
            self.tracker.has_started = true;
            return Ok((None, true));
        }
        if self.offset >= bytes.len() && (!bytes.is_empty() || self.empty_scan_done) {
            self.tracker.has_started = true;
            self.storage.shrink(self.bytes());
            return Ok((None, true));
        }
        // Reserve line construction/array metadata before parsing another slice.
        let needed = self
            .bytes()
            .saturating_add(
                self.max_rows
                    .saturating_mul(self.max_columns)
                    .saturating_mul(64),
            )
            .saturating_add(enabled.capacity().saturating_mul(8));
        self.storage.ensure(needed, client)?;
        if bytes.is_empty() {
            self.empty_scan_done = true;
        } else {
            let end = slice_end(
                bytes,
                self.offset,
                usize::from(self.tracker.shadow.screen().size().0 / 2).max(1),
            );
            let osc = self.osc.advance(&bytes[self.offset..end]);
            self.storage
                .ensure(needed.saturating_add(osc.peak.saturating_mul(2)), client)?;
            self.tracker.shadow.process(&bytes[self.offset..end]);
            self.osc = osc;
            self.offset = end;
        }
        let alternate = self.tracker.shadow.screen().alternate_screen();
        if alternate != self.tracker.is_alternate {
            self.tracker.is_alternate = alternate;
            if alternate {
                self.tracker.tables[1].clear();
                self.tracker.is_alternate_table_fresh = true;
            } else {
                self.tracker.tables[0]
                    .values_mut()
                    .for_each(|table| table.touch(operation.now));
            }
        }
        let seeds =
            self.tracker.has_started && !(alternate && self.tracker.is_alternate_table_fresh);
        self.tracker.tables[usize::from(alternate)].retain(|id, _| {
            enabled
                .iter()
                .any(|index| operation.context.settings.triggers[*index].id == *id)
        });
        self.phase = Phase::Scan(Scan {
            lines: logical_lines(self.tracker.shadow.screen()),
            rules: enabled,
            rule: 0,
            seeds,
            quiet: self
                .tracker
                .quiet_until
                .is_some_and(|until| operation.now < until),
            counts: Counts::default(),
        });
        Ok((None, false))
    }
    fn scan_rule(
        &mut self,
        operation: &Operation,
        client: &crate::execution::ExecutionClient,
    ) -> Result<(Option<(usize, usize)>, bool), ilium_execution::RejectReason> {
        // Compute conservative persistent growth before touching semantic state.
        let base = self.bytes();
        let Phase::Scan(scan) = &mut self.phase else {
            return Ok((None, false));
        };
        if scan.rule >= scan.rules.len() {
            self.tracker.is_alternate_table_fresh = false;
            self.phase = Phase::Feed;
            return Ok((None, false));
        }
        let index = scan.rules[scan.rule];
        let trigger = &operation.context.settings.triggers[index];
        let Some(regex) = matching_regex(&trigger.regexp) else {
            scan.rule += 1;
            scan.counts = Counts::default();
            return Ok((None, false));
        };
        let mut cache = regex.create_cache();
        for _ in 0..STEP_MATCHES {
            if scan.counts.line >= scan.lines.len() {
                break;
            }
            let line = &scan.lines[scan.counts.line];
            let mut input = regex_automata::Input::new(line).range(scan.counts.start..);
            let mut found = regex.search_with(&mut cache, &input);
            // Exact Searcher empty-overlap policy, with resumable scalar state.
            if found.is_some_and(|matched| {
                matched.is_empty() && Some(matched.end()) == scan.counts.last_end
            }) {
                input.set_start(input.start().saturating_add(1));
                found = regex.search_with(&mut cache, &input);
            }
            let Some(found) = found else {
                scan.counts.line += 1;
                scan.counts.start = 0;
                scan.counts.last_end = None;
                continue;
            };
            let key = normalize_key(&line[found.range()]);
            if !key.is_empty() && !scan.counts.values.contains_key(&key) {
                // Account HashMap growth before insertion. On refusal no cursor
                // advances: the exact current match is found again on resumption.
                let extra = scan
                    .counts
                    .values
                    .len()
                    .saturating_add(1)
                    .saturating_mul(8192)
                    .saturating_add(key.capacity().saturating_mul(4));
                self.storage.ensure(base.saturating_add(extra), client)?;
            }
            if !key.is_empty() {
                *scan.counts.values.entry(key).or_insert(0) += 1;
            }
            scan.counts.start = found.end();
            scan.counts.last_end = Some(found.end());
        }
        if scan.counts.line < scan.lines.len() {
            return Ok((None, false));
        }
        let table = &mut self.tracker.tables[usize::from(self.tracker.is_alternate)];
        let old_keys = table.get(&trigger.id).map_or(0, |rule| rule.keys.len());
        let missing_growth = table.get(&trigger.id).map_or(0, |rule| {
            rule.keys.values().fold(0usize, |bytes, state| {
                bytes.saturating_add(
                    state
                        .missing
                        .capacity()
                        .max(4)
                        .saturating_mul(2)
                        .saturating_mul(std::mem::size_of::<MissingAdmissions>()),
                )
            })
        });
        let additional = scan.counts.values.keys().fold(
            old_keys
                .saturating_mul(1024)
                .saturating_add(missing_growth)
                .saturating_add(trigger.id.capacity())
                .saturating_add(trigger.regexp.capacity())
                .saturating_add(4096),
            |bytes, key| bytes.saturating_add(key.capacity()).saturating_add(4096),
        );
        self.storage
            .ensure(base.saturating_add(additional), client)?;
        if table
            .get(&trigger.id)
            .is_some_and(|rule| rule.regexp != trigger.regexp)
        {
            table.remove(&trigger.id);
        }
        let new = !table.contains_key(&trigger.id);
        let rule = table
            .entry(trigger.id.clone())
            .or_insert_with(|| RuleTable::new(trigger));
        let fires = if new && scan.seeds {
            rule.seed(&scan.counts.values);
            0
        } else {
            rule.observe(
                &scan.counts.values,
                operation.now,
                target_matches(trigger.target, &operation.context.status),
                scan.quiet,
            )
        };
        scan.rule += 1;
        scan.counts = Counts::default();
        Ok((Some((index, fires)), false))
    }
}

struct CancelReceipt(ilium_execution::Receipt<StepJob>);
impl Drop for CancelReceipt {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl Matcher {
    pub(super) fn pending_operation(&self) -> Option<Arc<Operation>> {
        self.pending.clone()
    }
    pub(super) async fn apply(
        &mut self,
        operation: Arc<Operation>,
        client: &crate::execution::ExecutionClient,
        sender: &mpsc::Sender<TriggerDelivery>,
    ) -> Result<(), String> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| !Arc::ptr_eq(pending, &operation))
        {
            return Err(
                "Previous Text Trigger scan remains retained; new feed refused before admission"
                    .into(),
            );
        }
        self.pending = Some(operation.clone());
        let notification = client.completion_notification();
        loop {
            // An admitted semantic decision survives transient delivery failure.
            // Decrement only after the ordered sender accepted this instance.
            while let Some((index, count)) = self.pending_delivery {
                if count == 0 {
                    self.pending_delivery = None;
                    break;
                }
                let trigger = &operation.context.settings.triggers[index];
                super::enqueue_delivery(
                    client,
                    trigger,
                    operation.context.settings_revision,
                    sender,
                )
                .await?;
                self.pending_delivery = Some((index, count - 1));
            }
            let reservation = client
                .reserve(ilium_execution::Lane::Cpu, COST)
                .await
                .map_err(|reason| format!("Text Trigger CPU admission: {reason:?}"))?;
            let job = StepJob {
                engine: self.state.take(),
                bootstrap: self.bootstrap.take(),
                operation: operation.clone(),
                client: client.clone(),
            };
            let receipt = match reservation.submit(job) {
                Ok(receipt) => receipt,
                Err(rejected) => {
                    self.state = rejected.value.engine;
                    self.bootstrap = rejected.value.bootstrap;
                    return Err(format!(
                        "Text Trigger CPU publication: {:?}",
                        rejected.reason
                    ));
                }
            };
            // Every owned allocation now has its job or persistent-state debit.
            let mut receipt = CancelReceipt(receipt);
            let outcome = loop {
                let ready = notification.notified();
                tokio::pin!(ready);
                ready.as_mut().enable();
                match receipt.0.try_take() {
                    JobPoll::Pending => ready.await,
                    JobPoll::Ready(outcome) => break outcome,
                    JobPoll::Lost | JobPoll::Taken => {
                        let error =
                            "Text Trigger engine completion lost; state was not reset".to_string();
                        self.failed = Some(error.clone());
                        return Err(error);
                    }
                }
            };
            let (outcome, hold) = outcome.into_parts();
            let output = match outcome {
                JobOutcome::Finished(Ok(output)) => output,
                JobOutcome::Finished(Err(never)) => match never {},
                JobOutcome::NotStarted { job, reason } => {
                    self.state = job.engine;
                    self.bootstrap = job.bootstrap;
                    return Err(format!("Text Trigger engine cancelled before execution: {reason:?}; original state retained"));
                }
                JobOutcome::Panicked => {
                    let error="Text Trigger matching panicked; consumed state is indeterminate and was not recreated".to_string();
                    self.failed = Some(error.clone());
                    return Err(error);
                }
            };
            self.state = output.engine;
            // Persistent state and operation allocations have independent
            // storage custody; do not occupy finite result/input credit while
            // awaiting growth admission or the ordered durable delivery owner.
            drop(hold);
            if let Some((reason, requested)) = output.blocked {
                if reason == ilium_execution::RejectReason::Busy {
                    tokio::task::yield_now().await;
                    continue;
                }
                if reason != ilium_execution::RejectReason::WorkerBytes {
                    return Err(format!(
                        "Text Trigger state admission: {reason:?}; original scan retained"
                    ));
                }
                loop {
                    let released = notification.notified();
                    tokio::pin!(released);
                    released.as_mut().enable();
                    match client.try_reserve_storage(requested) {
                        Ok(lease) => {
                            if let Some(engine) = &mut self.state {
                                engine.storage.bytes =
                                    engine.storage.bytes.saturating_add(requested);
                                engine.storage.leases.push((requested, lease));
                            } else {
                                self.bootstrap = Some(Storage {
                                    leases: vec![(requested, lease)],
                                    bytes: requested,
                                    requested: 0,
                                });
                            }
                            break;
                        }
                        Err(ilium_execution::RejectReason::Busy) => tokio::task::yield_now().await,
                        Err(ilium_execution::RejectReason::WorkerBytes) => released.await,
                        Err(reason) => {
                            return Err(format!(
                                "Text Trigger growth admission: {reason:?}; original scan retained"
                            ))
                        }
                    }
                }
                continue;
            }
            self.pending_delivery = output.decision;
            if output.done {
                if let Some(engine) = &mut self.state {
                    engine.initialized = false;
                    engine.offset = 0;
                    engine.empty_scan_done = false;
                    engine.phase = Phase::Feed;
                    engine.storage.shrink(engine.bytes());
                }
                self.needs_resync = false;
                self.pending = None;
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn operation(
        client: &crate::execution::ExecutionClient,
        bytes: &[u8],
        now: Instant,
        geometry: (u16, u16),
        replace: bool,
    ) -> Arc<Operation> {
        let settings = ilium_ipc::TextTriggerSettings {
            triggers: vec![TextTrigger {
                id: "rule".into(),
                regexp: "needle".into(),
                message: "reply".into(),
                ..TextTrigger::default()
            }],
        };
        let retained = client
            .reserve_storage(super::super::settings_bytes(&settings).unwrap() + 4096)
            .await
            .unwrap();
        let payload = client.reserve_storage(bytes.len() + 4096).await.unwrap();
        Arc::new(Operation {
            context: Arc::new(EvaluationContext {
                status: PaneStatus::PlainShell,
                settings,
                settings_revision: 1,
                _storage: retained,
            }),
            bytes: bytes.to_vec(),
            geometry,
            replace,
            now,
            _storage: payload,
        })
    }
    #[tokio::test]
    async fn ordered_cpu_steps_preserve_split_escape_sequences_and_resize_quiet_policy() {
        let client = crate::execution::test_general_client();
        let (sender, mut receiver) = mpsc::channel(64);
        let mut matcher = Matcher::default();
        let mut original = TriggerTracker::default();
        let now = Instant::now();
        let parts: [&[u8]; 5] = [
            b"\x1b[2;",
            b"1Hnee",
            b"dle",
            b"\r\nnext",
            b"\x1b[2J\x1b[Hneedle",
        ];
        let mut expected = 0;
        for (index, part) in parts.into_iter().enumerate() {
            let operation = operation(
                &client,
                part,
                now + Duration::from_millis(index as u64),
                (4, 40),
                false,
            )
            .await;
            original.resize(4, 40, operation.now);
            expected += original
                .feed(
                    part,
                    &operation.context.settings.triggers,
                    &operation.context.status,
                    operation.now,
                )
                .len();
            matcher.apply(operation, &client, &sender).await.unwrap();
            assert!(matcher.pending.is_none());
        }
        let mut actual = 0;
        while let Ok(delivery) = receiver.try_recv() {
            assert_eq!(delivery.trigger_id, "rule");
            actual += 1;
        }
        assert_eq!(actual, expected);
        assert_eq!(
            matcher
                .state
                .as_ref()
                .unwrap()
                .tracker
                .shadow
                .screen()
                .contents(),
            original.shadow.screen().contents()
        );
        let resized = operation(
            &client,
            b"\x1b[2J\x1b[Hneedle\r\nneedle",
            now + Duration::from_millis(100),
            (5, 50),
            false,
        )
        .await;
        original.resize(5, 50, resized.now);
        let expected = original
            .feed(
                &resized.bytes,
                &resized.context.settings.triggers,
                &resized.context.status,
                resized.now,
            )
            .len();
        matcher.apply(resized, &client, &sender).await.unwrap();
        let mut actual = 0;
        while receiver.try_recv().is_ok() {
            actual += 1;
        }
        assert_eq!(actual, expected);
        assert_eq!(actual, 0);
    }
    #[tokio::test]
    async fn replay_resync_retains_rule_admissions_and_settle_clock() {
        let client = crate::execution::test_general_client();
        let (sender, mut receiver) = mpsc::channel(64);
        let mut matcher = Matcher::default();
        let now = Instant::now();
        matcher
            .apply(
                operation(&client, b"needle", now, (4, 40), false).await,
                &client,
                &sender,
            )
            .await
            .unwrap();
        assert_eq!(receiver.recv().await.unwrap().trigger_id, "rule");
        matcher.needs_resync = true;
        assert!(matcher.needs_resync);
        matcher
            .apply(
                operation(
                    &client,
                    b"needle",
                    now + Duration::from_millis(100),
                    (4, 40),
                    true,
                )
                .await,
                &client,
                &sender,
            )
            .await
            .unwrap();
        assert!(!matcher.needs_resync);
        assert!(receiver.try_recv().is_err());
        matcher
            .apply(
                operation(
                    &client,
                    b"\x1b[2J",
                    now + Duration::from_millis(200),
                    (4, 40),
                    false,
                )
                .await,
                &client,
                &sender,
            )
            .await
            .unwrap();
        matcher
            .apply(
                operation(
                    &client,
                    b"needle",
                    now + Duration::from_millis(1800),
                    (4, 40),
                    false,
                )
                .await,
                &client,
                &sender,
            )
            .await
            .unwrap();
        assert_eq!(receiver.recv().await.unwrap().trigger_id, "rule");
    }
    #[test]
    fn split_osc_highwater_tracks_library_termination_and_retained_capacity() {
        let budget = OscBudget::default().advance(b"\x1b]").advance(b"0;abcdef");
        assert!(budget.active);
        assert!(budget.peak >= 8);
        let peak = budget.peak;
        let closed = budget.advance(b"\x1b[2Jplain output");
        assert!(!closed.active);
        assert_eq!(closed.peak, peak);
        let next = closed.advance(b"\x1b\0]0;xy\x07");
        assert!(!next.active);
        assert_eq!(next.peak, peak);
        assert!(!budget.advance(b"\x18").active);
        assert!(!budget.advance(b"\x1a").active);
    }
    #[tokio::test]
    async fn refused_delivery_retains_decision_and_resumes_without_duplicate_matching() {
        let client = crate::execution::test_general_client();
        let now = Instant::now();
        let (sender, receiver) = mpsc::channel(64);
        let mut matcher = Matcher::default();
        matcher
            .apply(
                operation(&client, b"initial", now, (4, 40), false).await,
                &client,
                &sender,
            )
            .await
            .unwrap();
        drop(receiver);
        let feed = operation(
            &client,
            b"\rneedle",
            now + Duration::from_secs(3),
            (4, 40),
            false,
        )
        .await;
        assert!(matcher.apply(feed.clone(), &client, &sender).await.is_err());
        assert!(Arc::ptr_eq(
            matcher.pending_operation().as_ref().unwrap(),
            &feed
        ));
        assert_eq!(matcher.pending_delivery, Some((0, 1)));
        let (retry, mut receiver) = mpsc::channel(64);
        matcher.apply(feed, &client, &retry).await.unwrap();
        assert_eq!(receiver.recv().await.unwrap().trigger_id, "rule");
        assert!(receiver.try_recv().is_err());
        assert!(matcher.pending_delivery.is_none());
        assert!(matcher.pending_operation().is_none());
    }
    #[tokio::test]
    async fn cancelled_queued_step_preserves_original_engine_and_bytes() {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, Lane, LaneConfig, QuotaGroup, QuotaLimits,
            ShutdownMode, SkipReason,
        };
        let client = crate::execution::test_general_client();
        let operation = operation(&client, b"\x1b[Hneedle", Instant::now(), (4, 40), false).await;
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 3,
            service_jobs: 0,
            input_bytes: 128 * MIB,
            result_bytes: 4 * MIB,
            worker_threads: 1,
            worker_bytes: 128 * MIB,
        });
        let lane = |threads| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 2 },
            priority: None,
            resident_bytes_per_thread: if threads == 0 { 0 } else { 32 * MIB },
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut execution = loop {
            match Execution::start(
                quota.clone(),
                ExecutionConfig {
                    cpu: lane(1),
                    io: lane(0),
                    service: lane(0),
                },
            ) {
                Ok(execution) => break execution,
                Err(ilium_execution::StartError::Admission(
                    ilium_execution::RejectReason::Busy,
                )) if Instant::now() < deadline => std::thread::yield_now(),
                Err(error) => panic!("isolated matcher test bootstrap: {error:?}"),
            }
        };
        let completed = Arc::new(tokio::sync::Notify::new());
        let wake = completed.clone();
        let bank = execution
            .client(ClientLimits {
                jobs: 3,
                service_jobs: 0,
                input_bytes: 128 * MIB,
                result_bytes: 4 * MIB,
            })
            .unwrap()
            .with_completion_wake(move || wake.notify_one());
        let (started_sender, started_receiver) = std::sync::mpsc::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let _blocking = bank
            .try_reserve(
                Lane::Cpu,
                JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
            )
            .unwrap()
            .submit(move |_context: JobContext| -> Result<(), String> {
                started_sender.send(()).unwrap();
                release_receiver
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|error| error.to_string())
            })
            .unwrap();
        started_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let mut engine = Engine::default();
        engine.tracker.shadow.process(b"old engine");
        let previous = engine.tracker.shadow.screen().contents();
        let mut receipt = bank
            .try_reserve(Lane::Cpu, COST)
            .unwrap()
            .submit(StepJob {
                engine: Some(engine),
                bootstrap: None,
                operation: operation.clone(),
                client,
            })
            .unwrap();
        receipt.cancel();
        // The UI-facing producer returned while the only CPU owner was blocked.
        assert!(matches!(receipt.try_take(), JobPoll::Pending));
        release_sender.send(()).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let ready = completed.notified();
                tokio::pin!(ready);
                ready.as_mut().enable();
                match receipt.try_take() {
                    JobPoll::Ready(outcome) => break outcome,
                    JobPoll::Pending => ready.await,
                    _ => panic!("queued matcher completion lost"),
                }
            }
        })
        .await
        .unwrap();
        let (outcome, hold) = outcome.into_parts();
        let JobOutcome::NotStarted { job, reason } = outcome else {
            panic!("cancelled queued step must not run");
        };
        assert_eq!(reason, SkipReason::Cancelled);
        assert!(Arc::ptr_eq(&job.operation, &operation));
        assert_eq!(job.operation.bytes, b"\x1b[Hneedle");
        assert_eq!(job.operation.now, operation.now);
        assert_eq!(
            job.engine
                .as_ref()
                .unwrap()
                .tracker
                .shadow
                .screen()
                .contents(),
            previous
        );
        drop(job);
        drop(hold);
        drop(receipt);
        execution.request_shutdown(ShutdownMode::Drain);
        let report = tokio::task::spawn_blocking(move || {
            execution.join_until_background(Instant::now() + Duration::from_secs(5))
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(report.remaining_workers, 0);
    }
    #[tokio::test]
    async fn matching_runs_on_a_real_cpu_owner_without_retaining_finite_job_credit() {
        let mut client = crate::execution::test_general_client();
        let deadline = Instant::now() + Duration::from_secs(5);
        client.foundation = loop {
            match client.foundation.child(ilium_execution::ClientLimits {
                jobs: 2,
                service_jobs: 0,
                input_bytes: 64 * MIB,
                result_bytes: MIB,
            }) {
                Ok(child) => break child,
                Err(ilium_execution::RejectReason::Busy) if Instant::now() < deadline => {
                    std::thread::yield_now()
                }
                Err(reason) => panic!("matching test identity: {reason:?}"),
            }
        };
        let (sender, _receiver) = mpsc::channel(64);
        let mut matcher = Matcher::default();
        matcher
            .apply(
                operation(&client, b"not matching", Instant::now(), (4, 40), false).await,
                &client,
                &sender,
            )
            .await
            .unwrap();
        assert!(!matcher.state.as_ref().unwrap().storage.leases.is_empty());
        assert!(matcher.pending.is_none());
        assert_ne!(
            matcher.state.as_ref().unwrap().execution_thread.unwrap(),
            std::thread::current().id()
        );
        let released = client.completion_notification();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let ready = released.notified();
                tokio::pin!(ready);
                ready.as_mut().enable();
                if client.foundation.usage().jobs == 0 {
                    break;
                }
                ready.await;
            }
        })
        .await
        .unwrap();
        let usage = client.foundation.usage();
        assert_eq!(usage.jobs, 0);
        assert_eq!(usage.input_bytes, 0);
        assert_eq!(usage.result_bytes, 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn same_size_native_resize_rejects_a_prepared_screen_projection() {
        let client = crate::execution::test_general_client();
        let mut session = ilium_pty::PtySession::spawn(
            ilium_pty::PtyCommand::new("sh", std::env::temp_dir(), 24, 80)
                .arg("-c")
                .arg("printf 'PROJECTION_READY\\n'; exec sleep 60"),
        )
        .unwrap();
        let reader = session.current_screen_reader();
        let ready = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if reader.try_with_screen(|screen| screen.contents().contains("PROJECTION_READY"))
                    == Some(true)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        let capacity = 24 * 80 * 512 + 24 * 128 + 4096;
        let storage = client.reserve_storage(capacity).await.unwrap();
        let projection = ScreenProjection {
            input: session.input_handle(),
            reader,
            geometry: (24, 80),
            resize_epoch: 0,
            capacity,
            storage,
        };
        // The prepared geometry is unchanged, but native owner receipt commits
        // a different epoch before the CPU job is allowed to inspect it.
        let resize = session.resize(24, 80);
        let result = client
            .run(
                ilium_execution::Lane::Cpu,
                JobCost {
                    input_bytes: 64 * MIB,
                    result_bytes: 64 * MIB,
                },
                projection,
            )
            .await;
        let shutdown = session.shutdown_blocking(Duration::from_secs(2));
        drop(session);
        let shutdown = shutdown.unwrap();
        assert!(shutdown.pending.is_empty() && shutdown.panicked.is_empty());
        assert!(ready.is_ok() && resize.is_ok());
        assert!(
            result.unwrap().view().is_none(),
            "projection prepared before same-size resize must be rejected"
        );
    }
}
