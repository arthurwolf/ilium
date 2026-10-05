//! Single finite CPU owner for cost derivation. Captures carry only topology,
//! immutable statistics identities and bounded settings, never authored nodes.
use super::*;
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, StorageAdmission,
};
use ratatui::text::Line;
const MIB: usize = 1024 * 1024;
const MAX_NODES: usize = 4096;
pub(crate) const MAX_PANES: usize = 128;
const MAX_CAPTURE: usize = 8 * MIB;
const MAX_PANE: usize = 32 * MIB;
const MAX_OVERLAY: usize = 64 * MIB;
const HISTORY_POLL: Duration = Duration::from_secs(1);

/// Prepared on the CPU owner and shared through the last emitted-row owner.
/// A clone of the Arc refers to the same allocation and physical storage lease.
#[derive(Debug, PartialEq)]
pub struct PreparedCostCard {
    pub overlay_revision: u64,
    pub lines: Arc<[Line<'static>]>,
    storage: CostStorage,
}
struct Topology(HashMap<NodeId, (bool, Vec<NodeId>)>);
impl CostTopology for Topology {
    fn is_pane(&self, id: NodeId) -> bool {
        self.0.get(&id).is_some_and(|node| node.0)
    }
    fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        self.0.get(&id).map(|node| node.1.as_slice())
    }
}
struct Statistics(HashMap<NodeId, Arc<SessionStats>>);
impl CostStatistics for Statistics {
    fn stats(&self, id: NodeId) -> Option<&Arc<SessionStats>> {
        self.0.get(&id)
    }
}
struct Identity {
    settings: CostSettings,
    tree_version: u64,
    panes: Vec<(NodeId, Option<Weak<SessionStats>>)>,
    home: Option<PathBuf>,
    history_revision: u64,
    history_epoch: u64,
    history_scanning: bool,
    // Settings, identifiers and their domain-owner derivatives are charged
    // before copying. Keep this last, after all retained fields have dropped.
    storage: Arc<StorageAdmission>,
}
impl Identity {
    fn matches(&self, input: &TickInput<'_>, history: &CostHistory, epoch: u64) -> bool {
        self.history_epoch == epoch
            && self.history_revision == history.revision()
            && self.history_scanning == history.is_scanning()
            && self.settings == *input.settings
            && self.home.as_deref() == input.home
            && self.tree_version == input.tree_version
            && self.panes.len() == input.agent_panes.len()
            && self
                .panes
                .iter()
                .zip(input.agent_panes)
                .all(|((id, source), wanted)| {
                    if id != wanted {
                        return false;
                    }
                    let current = input.store.stats(*id);
                    match (source, current) {
                        (None, None) => true,
                        (Some(old), Some(current)) => old.as_ptr() == Arc::as_ptr(current),
                        _ => false,
                    }
                })
    }
}
struct Capture {
    identity: Arc<Identity>,
    topology: Topology,
    statistics: Statistics,
    agent_panes: Vec<NodeId>,
    now: Instant,
    now_ms: i64,
    history: HistorySnapshot,
}
impl Capture {
    fn new(
        input: &TickInput<'_>,
        history: HistorySnapshot,
        history_epoch: u64,
    ) -> Result<Self, String> {
        let count = input.tree.all_ids().count();
        if count > MAX_NODES || input.agent_panes.len() > MAX_PANES {
            return Err(
                "Cost preparation topology bound exceeded; previous overlay retained".into(),
            );
        }
        let settings = settings_bytes(input.settings);
        if settings > 64 * 1024 || input.settings.prices.len() > 512 {
            return Err(
                "Cost price settings exceed preparation bound; previous overlay retained".into(),
            );
        }
        let edges = input
            .tree
            .all_ids()
            .map(|id| input.tree.children_of(id).map_or(0, <[NodeId]>::len))
            .sum::<usize>();
        let home_bytes = input.home.map_or(0, |path| path.as_os_str().len());
        // Hash tables can round bucket counts up; include control-byte and
        // allocation rounding headroom. Settings have several bounded copies
        // in the price table, BuiltFor and overlay; history has two float arrays.
        let bytes = 4096usize
            .saturating_add(count.saturating_mul(256))
            .saturating_add(edges.saturating_mul(std::mem::size_of::<NodeId>() * 2))
            .saturating_add(input.agent_panes.len().saturating_mul(256))
            .saturating_add(settings.saturating_mul(8))
            .saturating_add(32_768 * 32)
            .saturating_add(home_bytes * 2);
        if bytes > MAX_CAPTURE || home_bytes > 64 * 1024 {
            return Err(
                "Cost preparation capture bytes exceeded; previous overlay retained".into(),
            );
        }
        let storage = Arc::new(
            crate::execution::process_quota()
                .reserve_external_storage(bytes)
                .map_err(|reason| format!("Cost preparation capture admission: {reason:?}"))?,
        );
        let mut topology = HashMap::with_capacity(count);
        for id in input.tree.all_ids() {
            topology.insert(
                id,
                (
                    input.tree.is_pane(id),
                    input.tree.children_of(id).unwrap_or(&[]).to_vec(),
                ),
            );
        }
        let statistics = input
            .agent_panes
            .iter()
            .filter_map(|id| input.store.stats(*id).map(|stats| (*id, Arc::clone(stats))))
            .collect();
        let identity = Arc::new(Identity {
            settings: input.settings.clone(),
            tree_version: input.tree_version,
            panes: input
                .agent_panes
                .iter()
                .map(|id| (*id, input.store.stats(*id).map(Arc::downgrade)))
                .collect(),
            home: input.home.map(Path::to_path_buf),
            history_revision: history.revision(),
            history_epoch,
            history_scanning: history.is_scanning(),
            storage,
        });
        Ok(Self {
            identity,
            topology: Topology(topology),
            statistics: Statistics(statistics),
            agent_panes: input.agent_panes.to_vec(),
            now: input.now,
            now_ms: input.now_ms,
            history,
        })
    }
}
struct Derive {
    engine: DerivedCosts,
    capture: Capture,
}
struct Derived {
    engine: DerivedCosts,
    identity: Arc<Identity>,
    now: Instant,
    result: Result<bool, String>,
}
impl Job for Derive {
    type Output = Derived;
    type Error = std::convert::Infallible;
    fn run(mut self, context: JobContext) -> Result<Derived, Self::Error> {
        let capture = self.capture;
        self.engine.history = capture.history;
        let previous = std::mem::replace(
            &mut self.engine.capture_storage,
            CostStorage(Some(Arc::clone(&capture.identity.storage))),
        );
        let result = if context.stop_requested() {
            Err("Cost preparation cancelled".into())
        } else {
            self.engine.tick(&DerivationInput {
                settings: &capture.identity.settings,
                tree: &capture.topology,
                tree_version: capture.identity.tree_version,
                store: &capture.statistics,
                agent_panes: &capture.agent_panes,
                now: capture.now,
                now_ms: capture.now_ms,
            })
        };
        // Old cached settings and float arrays can exist during replacement.
        // Their prior lease remains alive across every allocation in this pass.
        drop(previous);
        Ok(Derived {
            engine: self.engine,
            identity: capture.identity,
            now: capture.now,
            result,
        })
    }
}
struct Active {
    identity: Arc<Identity>,
    receipt: Receipt<Derive>,
}
/// UI coordinator: nonblocking admission/receipt collection and refresh cadence.
/// All pricing, sorting, calibration, rollups and card formatting run on CPU.
pub struct CostTracker {
    engine: Option<DerivedCosts>,
    history: CostHistory,
    history_cache_path: Option<PathBuf>,
    history_epoch: u64,
    active: Option<Active>,
    published: Arc<CostOverlay>,
    identity: Option<Arc<Identity>>,
    checked_at: Option<Instant>,
    last_requested: HashMap<NodeId, Instant>,
    client: Option<Client>,
    diagnostic: Option<String>,
    closing: bool,
    coordinator_storage: Option<StorageAdmission>,
}
impl Default for CostTracker {
    fn default() -> Self {
        let engine = DerivedCosts::default();
        Self {
            published: Arc::clone(&engine.overlay),
            engine: Some(engine),
            history: CostHistory::default(),
            history_cache_path: default_cache_path(),
            history_epoch: 0,
            active: None,
            identity: None,
            checked_at: None,
            last_requested: HashMap::new(),
            client: {
                #[cfg(test)]
                {
                    Some(crate::execution::test_client())
                }
                #[cfg(not(test))]
                {
                    None
                }
            },
            diagnostic: None,
            closing: false,
            coordinator_storage: None,
        }
    }
}
impl std::fmt::Debug for CostTracker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CostTracker")
            .field("preparing", &self.active.is_some())
            .finish()
    }
}
impl CostTracker {
    pub fn overlay(&self) -> &Arc<CostOverlay> {
        &self.published
    }
    pub fn pane_cost(&self, pane_id: NodeId) -> Option<&PaneCost> {
        self.published.details.get(&pane_id).map(AsRef::as_ref)
    }
    pub(crate) fn history_error(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }
    pub(crate) fn configure_execution(&mut self, client: Client) {
        self.history.configure_execution(client.clone());
        self.client = Some(client);
    }
    pub fn set_history_cache_path(&mut self, path: Option<PathBuf>) {
        if self.history_cache_path == path {
            return;
        }
        self.history_cache_path = path;
        self.history_epoch = self.history_epoch.wrapping_add(1);
        self.history.invalidate_cache();
        if let Some(engine) = &mut self.engine {
            engine.history = HistorySnapshot::default();
        }
        // Epoch fencing rejects an already-running CPU capture independently
        // of whether enabled settings allow the next I/O scan to start.
        self.identity = None;
    }
    pub(crate) fn cancel_pending(&mut self) {
        self.closing = true;
        if let Some(active) = self.active.take() {
            active.receipt.cancel();
            // Ready and not-started CPU outcomes must release their history
            // snapshots while the retirement owner is still accepting work.
        }
        if let Some(engine) = &mut self.engine {
            engine.history = HistorySnapshot::default();
        }
        self.history.cancel_pending();
    }
    pub fn plan_refreshes(
        &mut self,
        agent_panes: &[NodeId],
        busy_workers: usize,
        now: Instant,
    ) -> Vec<NodeId> {
        if self.closing || agent_panes.len() > MAX_PANES {
            return Vec::new();
        }
        if self.coordinator_storage.is_none() {
            match crate::execution::process_quota().reserve_external_storage(16 * 1024) {
                Ok(storage) => self.coordinator_storage = Some(storage),
                Err(reason) => {
                    self.diagnostic = Some(format!("Cost refresh metadata admission: {reason:?}"));
                    return Vec::new();
                }
            }
        }
        let mut budget = MAX_CONCURRENT_STATS_WORKERS.saturating_sub(busy_workers);
        self.last_requested.retain(|id, _| agent_panes.contains(id));
        let mut due = Vec::with_capacity(budget.min(MAX_CONCURRENT_STATS_WORKERS));
        for id in agent_panes.iter().take(MAX_PANES) {
            if budget == 0 {
                break;
            }
            if self
                .last_requested
                .get(id)
                .is_none_or(|last| now.saturating_duration_since(*last) >= PANE_REFRESH_INTERVAL)
            {
                self.last_requested.insert(*id, now);
                due.push(*id);
                budget -= 1;
            }
        }
        due
    }
    pub fn tick(&mut self, input: &TickInput<'_>) -> bool {
        // Release completed I/O debits even when no new CPU job can be admitted.
        let history_changed = self.history.drain_events(input.now);
        let mut changed = self.collect(input) || history_changed;
        if !self.closing
            && input.settings.is_any_enabled()
            && input.settings.calibration == Calibration::OwnHistory
        {
            if let Some(home) = input.home {
                self.history.request_scan(
                    home.to_path_buf(),
                    self.history_cache_path.clone(),
                    input.settings.history_days,
                    input.now,
                );
            }
        }
        if self.closing || self.active.is_some() {
            return changed;
        }
        let Some(client) = &self.client else {
            self.diagnostic = Some("Cost CPU preparation unavailable".into());
            return changed;
        };
        if self.engine.is_none() {
            return changed;
        }
        if !input.settings.is_any_enabled() && self.published.rows.is_empty() {
            return changed;
        }
        let interval = if self.history.is_scanning() {
            HISTORY_POLL
        } else {
            REBUILD_INTERVAL
        };
        if self
            .identity
            .as_ref()
            .is_some_and(|identity| identity.matches(input, &self.history, self.history_epoch))
            && self
                .checked_at
                .is_some_and(|last| input.now.saturating_duration_since(last) < interval)
        {
            return changed;
        }
        let capture = match Capture::new(input, self.history.snapshot(), self.history_epoch) {
            Ok(capture) => capture,
            Err(error) => {
                self.diagnostic = Some(error);
                return changed;
            }
        };
        let reservation = match client.try_reserve_detailed(
            Lane::Cpu,
            JobCost {
                input_bytes: 64 * MIB,
                result_bytes: 4096,
            },
        ) {
            Ok(reservation) => reservation,
            Err(failure) => {
                let diagnostic = format!(
                    "Cost CPU admission: {:?}; previous overlay retained",
                    failure.reason
                );
                // Capture the rejecting ledger's evidence, not a later usage
                // snapshot. Repeated ticks with the same refusal do not flood
                // logs, and the unchanged engine remains available for retry.
                if self.diagnostic.as_deref() != Some(diagnostic.as_str()) {
                    tracing::warn!(
                        reason = ?failure.reason,
                        quota = ?failure.quota,
                        "cost preparation admission refused"
                    );
                }
                self.diagnostic = Some(diagnostic);
                return true;
            }
        };
        let identity = Arc::clone(&capture.identity);
        let Some(engine) = self.engine.take() else {
            return changed;
        };
        let job = Derive { engine, capture };
        match reservation.submit(job) {
            Ok(receipt) => self.active = Some(Active { identity, receipt }),
            Err(rejected) => {
                self.engine = Some(rejected.value.engine);
                self.diagnostic = Some(format!(
                    "Cost CPU admission: {:?}; previous overlay retained",
                    rejected.reason
                ));
                changed = true;
            }
        }
        changed
    }
    fn collect(&mut self, input: &TickInput<'_>) -> bool {
        let Some(active) = &mut self.active else {
            return false;
        };
        let outcome = match active.receipt.try_take() {
            JobPoll::Pending => return false,
            JobPoll::Ready(outcome) => Some(outcome.into_parts().0),
            _ => None,
        };
        let Some(active) = self.active.take() else {
            return false;
        };
        let valid = active
            .identity
            .matches(input, &self.history, self.history_epoch);
        match outcome {
            Some(JobOutcome::Finished(Ok(mut result))) => {
                self.diagnostic = result
                    .result
                    .as_ref()
                    .err()
                    .cloned()
                    .or_else(|| self.history.error().map(str::to_owned));
                let changed = valid && result.result.as_ref().is_ok_and(|changed| *changed);
                if valid && result.result.is_ok() {
                    self.published = Arc::clone(&result.engine.overlay);
                    self.identity = Some(result.identity);
                    self.checked_at = Some(result.now);
                }
                if result.result.is_err() {
                    self.identity = None;
                    result.engine.built_for = None;
                    result.engine.last_rebuild = None;
                }
                if !valid {
                    result.engine.history = HistorySnapshot::default();
                }
                self.engine = Some(result.engine);
                changed
            }
            Some(JobOutcome::NotStarted { job, .. }) => {
                self.engine = Some(job.engine);
                self.diagnostic =
                    Some("Cost CPU job did not start; original owner retained".into());
                false
            }
            _ => {
                let mut engine = DerivedCosts::default();
                engine.overlay = Arc::clone(&self.published);
                self.engine = Some(engine);
                self.identity = None;
                self.diagnostic = Some("Cost CPU worker failed; previous overlay retained".into());
                true
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn settle_for_test(&mut self, input: &TickInput<'_>) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut changed = self.tick(input);
        while self.active.is_some()
            || (input.settings.is_any_enabled()
                && self.identity.as_ref().is_none_or(|identity| {
                    !identity.matches(input, &self.history, self.history_epoch)
                }))
        {
            assert!(
                Instant::now() < deadline,
                "cost CPU receipt timed out: {:?}",
                self.diagnostic
            );
            std::thread::sleep(Duration::from_millis(2));
            changed |= self.tick(input);
        }
        changed
    }
}
impl Drop for CostTracker {
    fn drop(&mut self) {
        self.cancel_pending();
    }
}
fn settings_bytes(settings: &CostSettings) -> usize {
    std::mem::size_of::<CostSettings>()
        + settings
            .prices
            .iter()
            .map(|(name, _)| name.capacity() + 256)
            .sum::<usize>()
}
pub(super) fn pane_storage(stats: &SessionStats) -> Result<(Arc<StorageAdmission>, usize), String> {
    let model_bytes = stats
        .models
        .iter()
        .map(|usage| usage.model.capacity())
        .sum::<usize>();
    let quota_bytes = stats
        .rate_limits
        .iter()
        .map(|(name, _)| name.capacity())
        .sum::<usize>();
    let bytes = 4096usize
        .saturating_add(
            stats
                .models
                .len()
                .saturating_mul(std::mem::size_of::<ModelCost>() * 2),
        )
        .saturating_add(
            stats
                .spend
                .len()
                .saturating_mul(std::mem::size_of::<SpendPoint>() * 2),
        )
        .saturating_add(
            stats
                .quota_spend
                .len()
                .saturating_mul(std::mem::size_of::<SpendPoint>() * 2),
        )
        .saturating_add(
            stats
                .rate_limits
                .len()
                .saturating_mul(std::mem::size_of::<(String, f64)>() * 2),
        )
        .saturating_add(model_bytes)
        .saturating_add(quota_bytes);
    if bytes > MAX_PANE {
        return Err("Derived pane cost exceeds retained byte bound".into());
    }
    let storage = Arc::new(
        crate::execution::process_quota()
            .reserve_external_storage(bytes)
            .map_err(|reason| format!("Derived pane cost admission: {reason:?}"))?,
    );
    Ok((storage, bytes))
}
fn pane_bytes(cost: &PaneCost) -> usize {
    std::mem::size_of::<PaneCost>()
        + cost.spark_cells.capacity() * 8
        + cost.model_costs.capacity() * std::mem::size_of::<ModelCost>()
        + cost
            .model_costs
            .iter()
            .map(|model| model.model.capacity())
            .sum::<usize>()
        + cost.quota.capacity() * std::mem::size_of::<(String, f64)>()
        + cost
            .quota
            .iter()
            .map(|(name, _)| name.capacity())
            .sum::<usize>()
        + cost.spend_points.capacity() * std::mem::size_of::<SpendPoint>()
}
pub(super) fn check_pane_capacity(cost: &PaneCost, bytes: usize) -> Result<(), String> {
    if pane_bytes(cost) > bytes {
        Err("Derived pane cost physical capacity exceeded its admission".into())
    } else {
        Ok(())
    }
}
pub(super) fn overlay_storage(
    input: &DerivationInput<'_>,
    panes: &HashMap<NodeId, PaneEntry>,
    history: usize,
) -> Result<(Arc<StorageAdmission>, usize), String> {
    let text = panes
        .values()
        .map(|entry| {
            let cost = &entry.cost;
            8192usize
                .saturating_add(
                    cost.model_costs
                        .iter()
                        .map(|model| model.model.capacity() * 4)
                        .sum::<usize>(),
                )
                .saturating_add(
                    cost.quota
                        .iter()
                        .map(|(name, _)| name.capacity() * 4 + 2048)
                        .sum::<usize>(),
                )
        })
        .sum::<usize>();
    let bytes = 8192usize
        .saturating_add(input.agent_panes.len() * 1024)
        .saturating_add(MAX_NODES * 512)
        .saturating_add(history * 32)
        .saturating_add(settings_bytes(input.settings) * 2)
        .saturating_add(text);
    if bytes > MAX_OVERLAY {
        return Err("Cost overlay/card physical byte bound exceeded".into());
    }
    let hold = Arc::new(
        crate::execution::process_quota()
            .reserve_external_storage(bytes)
            .map_err(|reason| format!("Cost overlay/card storage admission: {reason:?}"))?,
    );
    Ok((hold, bytes))
}
pub(super) fn prepare_cards(
    overlay: &mut CostOverlay,
    (hold, limit): (Arc<StorageAdmission>, usize),
) -> Result<(), String> {
    overlay.storage = CostStorage(Some(Arc::clone(&hold)));
    if overlay.settings.detail_card.enabled {
        for (id, cost) in &overlay.details {
            let Some(row) = overlay.rows.get(id) else {
                continue;
            };
            let lines = crate::cost_overlay::detail_card_lines(cost, row, overlay);
            overlay.cards.insert(
                *id,
                Arc::new(PreparedCostCard {
                    overlay_revision: overlay.overlay_revision,
                    lines: lines.into(),
                    storage: CostStorage(Some(Arc::clone(&hold))),
                }),
            );
        }
    }
    let card_bytes = overlay
        .cards
        .values()
        .map(|card| {
            card.lines.len() * std::mem::size_of::<Line<'static>>()
                + card
                    .lines
                    .iter()
                    .map(|line| {
                        line.spans.capacity() * std::mem::size_of::<ratatui::text::Span<'static>>()
                            + line
                                .spans
                                .iter()
                                .map(|span| match &span.content {
                                    std::borrow::Cow::Owned(content) => content.capacity(),
                                    _ => 0,
                                })
                                .sum::<usize>()
                    })
                    .sum::<usize>()
        })
        .sum::<usize>();
    let retained = settings_bytes(&overlay.settings)
        + overlay.rows.capacity() * (std::mem::size_of::<(NodeId, RowCost)>() + 32)
        + overlay
            .rows
            .values()
            .map(|row| row.spark.capacity())
            .sum::<usize>()
        + overlay.ranks.capacity() * 64
        + overlay.details.capacity() * 64
        + overlay.cards.capacity() * 64
        + overlay.peer_totals.capacity() * 8
        + overlay.history_sorted.capacity() * 8
        + card_bytes;
    if retained > limit {
        return Err("Cost overlay physical capacity exceeded storage admission".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
    struct Block {
        entered: SyncSender<std::thread::ThreadId>,
        release: Receiver<()>,
    }
    impl Job for Block {
        type Output = ();
        type Error = ();
        fn run(self, _context: JobContext) -> Result<(), ()> {
            self.entered.send(std::thread::current().id()).unwrap();
            self.release.recv().unwrap();
            Ok(())
        }
    }
    fn bank() -> (Execution, Client, QuotaGroup) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 4,
            jobs: 8,
            service_jobs: 0,
            input_bytes: 128 * MIB,
            result_bytes: 8 * MIB,
            worker_threads: 1,
            worker_bytes: 128 * MIB,
        });
        let lane = |threads| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 4 },
            priority: None,
            resident_bytes_per_thread: 64 * MIB,
        };
        let owner = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane(1),
                io: lane(0),
                service: lane(0),
            },
        )
        .unwrap();
        let client = owner
            .client(ClientLimits {
                jobs: 8,
                service_jobs: 0,
                input_bytes: 128 * MIB,
                result_bytes: 8 * MIB,
            })
            .unwrap();
        (owner, client, quota)
    }
    fn fixture() -> (Tree, NodeId, SessionStatsStore, CostSettings) {
        let mut tree = Tree::new();
        let project = tree
            .add_project(std::path::PathBuf::from("/worker879/cost-fixture"))
            .unwrap();
        let group = tree.add_group(project, "agents").unwrap();
        let id = tree
            .add_pane(group, "agent", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let mut stats = SessionStatsStore::default();
        stats.insert_ready_for_test(
            id,
            Arc::new(SessionStats {
                models: vec![crate::session_stats::ModelUsage {
                    model: "claude-haiku-4-5".into(),
                    calls: 1,
                    tokens: TokenTotals {
                        input: 1_000_000,
                        ..TokenTotals::default()
                    },
                }],
                ..SessionStats::default()
            }),
        );
        let settings = CostSettings {
            calibration: Calibration::FixedBands,
            detail_card: crate::cost_settings::DisplayOption {
                enabled: true,
                ..Default::default()
            },
            ..CostSettings::default()
        };
        (tree, id, stats, settings)
    }
    fn input<'a>(
        tree: &'a Tree,
        id: &'a NodeId,
        stats: &'a SessionStatsStore,
        settings: &'a CostSettings,
        now: Instant,
    ) -> TickInput<'a> {
        TickInput {
            settings,
            tree,
            tree_version: 1,
            store: stats,
            agent_panes: std::slice::from_ref(id),
            now,
            now_ms: 0,
            home: None,
        }
    }
    #[test]
    fn completed_history_receipt_drains_before_a_new_cpu_admission_at_original_limits() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 8,
            jobs: 8,
            service_jobs: 0,
            input_bytes: 512 * MIB,
            result_bytes: 64 * MIB,
            worker_threads: 2,
            worker_bytes: 256 * MIB,
        });
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 4,
            priority: None,
            resident_bytes_per_thread: 4 * MIB,
        };
        let mut owner = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane,
                io: lane,
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .unwrap();
        let general = owner
            .client(ClientLimits {
                jobs: 8,
                service_jobs: 0,
                input_bytes: 512 * MIB,
                result_bytes: 64 * MIB,
            })
            .unwrap();
        let statistics = general
            .child(ClientLimits {
                jobs: 8,
                service_jobs: 0,
                input_bytes: 384 * MIB,
                result_bytes: 64 * MIB,
            })
            .unwrap();
        let parser = general
            .child(ClientLimits {
                jobs: 1,
                service_jobs: 0,
                input_bytes: 128 * MIB,
                result_bytes: 1,
            })
            .unwrap();
        let parser_hold = parser
            .try_reserve_external(JobCost {
                input_bytes: 128 * MIB,
                result_bytes: 0,
            })
            .unwrap();
        let stats_hold = statistics
            .try_reserve_external(JobCost {
                input_bytes: 128 * MIB,
                result_bytes: 0,
            })
            .unwrap();
        let home = tempfile::tempdir().unwrap();
        let directory = home.path().join(".claude/projects/fixture");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("one.jsonl"),
            concat!(
                "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n",
                "{\"type\":\"cost-state\",\"sessionId\":\"only\",\"totalCostUSD\":1.0}\n",
                "{\"type\":\"assistant\",\"message\":{\"content\":[]}}\n"
            ),
        )
        .unwrap();
        let (done_tx, done_rx) = sync_channel(1);
        let client = statistics.clone().with_completion_wake(move || {
            let _ = done_tx.try_send(());
        });
        let mut tracker = CostTracker::default();
        tracker.configure_execution(client);
        assert!(tracker
            .history
            .request_scan(home.path().to_path_buf(), None, 30, Instant::now()));
        // This signal comes from the actual finite I/O job's terminal publication.
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(quota.snapshot().input_bytes, 512 * MIB);
        assert_eq!(
            statistics
                .try_reserve(
                    Lane::Cpu,
                    JobCost {
                        input_bytes: 64 * MIB,
                        result_bytes: 4096,
                    }
                )
                .err(),
            Some(ilium_execution::RejectReason::InputBytes)
        );
        let (tree, id, stats, settings) = fixture();
        let input = input(&tree, &id, &stats, &settings, Instant::now());
        tracker.tick(&input);
        assert!(tracker.history.has_result());
        assert_eq!(tracker.history.entries().len(), 1);
        assert!(
            quota.snapshot().input_bytes <= 320 * MIB,
            "coordinator must release the original256MiB receipt before CPU admission"
        );
        tracker.settle_for_test(&input);
        assert_eq!(tracker.engine.as_ref().unwrap().history.entries().len(), 1);
        // App keeps its tracker alive while the execution owner joins. Cancellation
        // must release every original history generation before that join.
        tracker.cancel_pending();
        assert!(tracker.history.entries().is_empty());
        assert!(tracker
            .engine
            .as_ref()
            .unwrap()
            .history
            .entries()
            .is_empty());
        drop(stats_hold);
        drop(parser_hold);
        drop(statistics);
        drop(parser);
        drop(general);
        owner.request_shutdown(ShutdownMode::Drain);
        let report = owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(report.shutdown_complete);
        assert_eq!(report.remaining_workers, 0);
        drop(tracker);
    }

    // Independent fixture with the same original limits as the existing drain
    // test. No process-global bank, extra lane, or larger admission is introduced.
    fn history_reconfiguration_bank() -> (Execution, Client, QuotaGroup) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 8,
            jobs: 8,
            service_jobs: 0,
            input_bytes: 512 * MIB,
            result_bytes: 64 * MIB,
            worker_threads: 2,
            worker_bytes: 256 * MIB,
        });
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 4,
            priority: None,
            resident_bytes_per_thread: 4 * MIB,
        };
        let owner = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane,
                io: lane,
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .unwrap();
        let general = owner
            .client(ClientLimits {
                jobs: 8,
                service_jobs: 0,
                input_bytes: 512 * MIB,
                result_bytes: 64 * MIB,
            })
            .unwrap();
        let statistics = general
            .child(ClientLimits {
                jobs: 8,
                service_jobs: 0,
                input_bytes: 384 * MIB,
                result_bytes: 64 * MIB,
            })
            .unwrap();
        (owner, statistics, quota)
    }
    fn history_reconfiguration_home() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        let directory = home.path().join(".claude/projects/fixture");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("one.jsonl"),
            concat!(
                "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n",
                "{\"type\":\"cost-state\",\"sessionId\":\"only\",\"totalCostUSD\":1.0}\n",
                "{\"type\":\"assistant\",\"message\":{\"content\":[]}}\n"
            ),
        )
        .unwrap();
        home
    }
    #[derive(Clone, Copy)]
    enum IneligibleHistoryTick {
        Disabled,
        FixedBands,
        NoHome,
    }
    fn reconfigured_history_does_not_install_old_receipt(ready: bool, mode: IneligibleHistoryTick) {
        let (mut owner, statistics, quota) = history_reconfiguration_bank();
        let home = history_reconfiguration_home();
        let (tree, id, stats, mut settings) = fixture();
        settings.calibration = Calibration::OwnHistory;
        if matches!(mode, IneligibleHistoryTick::Disabled) {
            settings.sort_by_cost = false;
            for option in [
                &mut settings.level_glyph,
                &mut settings.level_dollars,
                &mut settings.meter,
                &mut settings.sparkline,
                &mut settings.group_totals,
                &mut settings.detail_card,
                &mut settings.header_total,
                &mut settings.burn_marker,
            ] {
                option.enabled = false;
            }
            assert!(!settings.is_any_enabled());
        } else if matches!(mode, IneligibleHistoryTick::FixedBands) {
            settings.calibration = Calibration::FixedBands;
            assert!(settings.is_any_enabled());
        }
        let (done_tx, done_rx) = sync_channel(1);
        let mut tracker = CostTracker::default();
        tracker.configure_execution(statistics.clone());
        tracker
            .history
            .configure_execution(statistics.clone().with_completion_wake(move || {
                let _ = done_tx.try_send(());
            }));
        let mut blocked = None;
        let mut release = None;
        if !ready {
            let (entered_tx, entered_rx) = sync_channel(1);
            let (release_tx, release_rx) = sync_channel(1);
            blocked = Some(
                statistics
                    .try_submit(
                        Lane::Io,
                        JobCost {
                            input_bytes: 4096,
                            result_bytes: 4096,
                        },
                        Block {
                            entered: entered_tx,
                            release: release_rx,
                        },
                    )
                    .unwrap(),
            );
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            release = Some(release_tx);
        }
        assert!(tracker
            .history
            .request_scan(home.path().to_path_buf(), None, 30, Instant::now()));
        assert!(tracker.history.is_scanning());
        if ready {
            // Actual IO publication, before the coordinator has consumed it.
            done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(quota.snapshot().input_bytes, 256 * MIB);
        } else {
            assert_eq!(quota.snapshot().input_bytes, 256 * MIB + 4096);
        }
        let revision = tracker.history.revision();
        tracker.set_history_cache_path(Some(home.path().join("changed-cache.json")));
        assert!(tracker.history.revision() > revision);
        assert!(!tracker.history.has_result());
        assert!(tracker.history.entries().is_empty());
        let mut tick_input = input(&tree, &id, &stats, &settings, Instant::now());
        if !matches!(mode, IneligibleHistoryTick::NoHome) {
            tick_input.home = Some(home.path());
        }
        // This first tick is ineligible for a replacement IO scan. In the
        // queued case it also proves invalidation while original custody lives.
        tracker.tick(&tick_input);
        assert!(!tracker.history.has_result());
        assert!(tracker.history.entries().is_empty());
        if let Some(release) = release {
            release.send(()).unwrap();
            done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            tracker.tick(&tick_input);
        }
        assert!(!tracker.history.is_scanning());
        assert!(!tracker.history.has_result());
        assert!(tracker.history.entries().is_empty());
        // Cache publication by a captured running write is deliberately not a
        // rollback oracle. Only installation under the new identity is fenced.
        tracker.cancel_pending();
        drop(blocked);
        drop(statistics);
        owner.request_shutdown(ShutdownMode::Drain);
        let report = owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(report.shutdown_complete);
        assert_eq!(report.remaining_workers, 0);
        assert_eq!(report.health.lanes[Lane::Cpu as usize].retirement_live, 0);
        drop(tracker);
    }
    #[test]
    fn cache_change_discards_queued_io_when_display_is_disabled() {
        reconfigured_history_does_not_install_old_receipt(false, IneligibleHistoryTick::Disabled);
    }
    #[test]
    fn cache_change_discards_ready_io_when_display_is_disabled() {
        reconfigured_history_does_not_install_old_receipt(true, IneligibleHistoryTick::Disabled);
    }
    #[test]
    fn cache_change_discards_queued_io_under_fixed_bands() {
        reconfigured_history_does_not_install_old_receipt(false, IneligibleHistoryTick::FixedBands);
    }
    #[test]
    fn cache_change_discards_ready_io_under_fixed_bands() {
        reconfigured_history_does_not_install_old_receipt(true, IneligibleHistoryTick::FixedBands);
    }
    #[test]
    fn cache_change_discards_queued_io_without_home() {
        reconfigured_history_does_not_install_old_receipt(false, IneligibleHistoryTick::NoHome);
    }
    #[test]
    fn cache_change_discards_ready_io_without_home() {
        reconfigured_history_does_not_install_old_receipt(true, IneligibleHistoryTick::NoHome);
    }
    #[test]
    fn cache_change_releases_installed_generation_after_independent_last_reader() {
        let (mut owner, statistics, _) = history_reconfiguration_bank();
        let home = history_reconfiguration_home();
        let (done_tx, done_rx) = sync_channel(1);
        let mut tracker = CostTracker::default();
        tracker.configure_execution(statistics.clone());
        tracker
            .history
            .configure_execution(statistics.clone().with_completion_wake(move || {
                let _ = done_tx.try_send(());
            }));
        assert!(tracker
            .history
            .request_scan(home.path().to_path_buf(), None, 30, Instant::now()));
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(tracker.history.drain_events(Instant::now()));
        let snapshot = tracker.history.snapshot();
        let independent_reader = snapshot.clone();
        assert_eq!(snapshot.entries().len(), 1);
        let original_entries = snapshot.entries().as_ptr();
        assert_eq!(independent_reader.entries().as_ptr(), original_entries);
        tracker.engine.as_mut().unwrap().history = snapshot;
        tracker.set_history_cache_path(Some(home.path().join("new-cache.json")));
        assert!(tracker.history.entries().is_empty());
        assert!(tracker
            .engine
            .as_ref()
            .unwrap()
            .history
            .entries()
            .is_empty());
        tracker.cancel_pending();
        assert_eq!(independent_reader.entries().len(), 1);
        assert_eq!(independent_reader.entries().as_ptr(), original_entries);
        assert_eq!(
            owner.monitor().health().lanes[Lane::Cpu as usize].retirement_live,
            1
        );
        drop(independent_reader);
        drop(statistics);
        owner.request_shutdown(ShutdownMode::Drain);
        let report = owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(report.shutdown_complete);
        assert_eq!(report.health.lanes[Lane::Cpu as usize].retirement_live, 0);
        assert_eq!(report.remaining_workers, 0);
        drop(tracker);
    }

    #[test]
    fn ordinary_cancelled_rescan_preserves_the_previous_complete_generation() {
        let (mut owner, statistics, _) = history_reconfiguration_bank();
        let home = history_reconfiguration_home();
        let (done_tx, done_rx) = sync_channel(1);
        let mut tracker = CostTracker::default();
        tracker.configure_execution(statistics.clone());
        tracker
            .history
            .configure_execution(statistics.clone().with_completion_wake(move || {
                let _ = done_tx.try_send(());
            }));
        assert!(tracker
            .history
            .request_scan(home.path().to_path_buf(), None, 30, Instant::now()));
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(tracker.history.drain_events(Instant::now()));
        let original_entries = tracker.history.entries().as_ptr();
        assert_eq!(tracker.history.entries().len(), 1);
        let (entered_tx, entered_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        let blocker = statistics
            .try_submit(
                Lane::Io,
                JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
                Block {
                    entered: entered_tx,
                    release: release_rx,
                },
            )
            .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // Same home/cache with a different day count forces another actual IO
        // receipt without invalidating the previous complete source generation.
        assert!(tracker
            .history
            .request_scan(home.path().to_path_buf(), None, 31, Instant::now()));
        owner.request_shutdown(ShutdownMode::Cancel);
        release_tx.send(()).unwrap();
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(tracker.history.drain_events(Instant::now()));
        assert!(tracker.history.error().is_some());
        assert!(tracker.history.has_result());
        assert_eq!(tracker.history.entries().len(), 1);
        assert_eq!(tracker.history.entries().as_ptr(), original_entries);
        tracker.cancel_pending();
        drop(blocker);
        drop(statistics);
        let report = owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(report.shutdown_complete);
        assert_eq!(report.remaining_workers, 0);
        drop(tracker);
    }

    #[test]
    fn changing_history_cache_path_fences_an_already_captured_cpu_generation() {
        let (tree, id, stats, settings) = fixture();
        let input = input(&tree, &id, &stats, &settings, Instant::now());
        let mut tracker = CostTracker::default();
        let capture =
            Capture::new(&input, tracker.history.snapshot(), tracker.history_epoch).unwrap();
        assert!(capture
            .identity
            .matches(&input, &tracker.history, tracker.history_epoch));
        tracker.set_history_cache_path(Some(PathBuf::from("/worker879/new-history-cache")));
        assert!(!capture
            .identity
            .matches(&input, &tracker.history, tracker.history_epoch));
    }

    #[test]
    fn blocked_cpu_keeps_ui_nonblocking_fences_revisions_and_does_not_starve_time() {
        let (mut owner, client, quota) = bank();
        let (entered_tx, entered_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        let mut blocker = client
            .try_submit(
                Lane::Cpu,
                JobCost {
                    input_bytes: 1024,
                    result_bytes: 1024,
                },
                Block {
                    entered: entered_tx,
                    release: release_rx,
                },
            )
            .unwrap();
        assert_ne!(
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            std::thread::current().id()
        );
        let (tree, id, stats, mut settings) = fixture();
        let mut tracker = CostTracker::default();
        tracker.configure_execution(client);
        let now = Instant::now();
        let started = Instant::now();
        assert!(!tracker.tick(&input(&tree, &id, &stats, &settings, now)));
        assert!(started.elapsed() < Duration::from_millis(100));
        let original = Arc::clone(&tracker.active.as_ref().unwrap().identity);
        for offset in 1..64 {
            tracker.tick(&input(
                &tree,
                &id,
                &stats,
                &settings,
                now + Duration::from_millis(offset),
            ));
        }
        assert!(Arc::ptr_eq(
            &original,
            &tracker.active.as_ref().unwrap().identity
        ));
        assert_eq!(
            quota.snapshot().jobs,
            2,
            "time updates cannot multiply CPU jobs"
        );
        settings.metric = CostMetric::Quota;
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match blocker.try_take() {
                JobPoll::Pending => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(2));
                }
                JobPoll::Ready(_) => break,
                _ => panic!("blocked CPU receipt lost"),
            }
        }
        let stale = Arc::clone(&tracker.published);
        while tracker.active.is_some() {
            assert!(Instant::now() < deadline);
            tracker.collect(&input(&tree, &id, &stats, &settings, now));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            Arc::ptr_eq(&stale, &tracker.published),
            "wrong metric cannot publish"
        );
        assert!(tracker.settle_for_test(&input(&tree, &id, &stats, &settings, now)));
        assert_eq!(tracker.overlay().metric, CostMetric::Quota);
        // A later time alone may arrive while the exact completed revision is
        // awaiting collection; it must not invalidate a valid finished result.
        settings.metric = CostMetric::Dollars;
        tracker.tick(&input(&tree, &id, &stats, &settings, now));
        assert!(tracker.settle_for_test(&input(
            &tree,
            &id,
            &stats,
            &settings,
            now + Duration::from_millis(999)
        )));
        assert_eq!(tracker.overlay().rows[&id].amount, 1.0);
        tracker.cancel_pending();
        drop(tracker);
        drop(blocker);
        owner.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            owner
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
        drop(owner);
        assert_eq!(quota.snapshot().jobs, 0);
    }
    #[test]
    fn acknowledged_rows_keep_exact_card_title_and_storage_after_new_source_and_shutdown() {
        let (mut owner, client, quota) = bank();
        let (tree, id, stats, settings) = fixture();
        let mut tracker = CostTracker::default();
        tracker.configure_execution(client);
        let input = TickInput {
            settings: &settings,
            tree: &tree,
            tree_version: 1,
            store: &stats,
            agent_panes: &[id],
            now: Instant::now(),
            now_ms: 0,
            home: None,
        };
        assert!(tracker.settle_for_test(&input));
        let card = tracker.overlay().prepared_card(id).unwrap();
        assert_eq!(card.overlay_revision, tracker.overlay().overlay_revision);
        let storage = Arc::downgrade(card.storage.0.as_ref().unwrap());
        let old_revision = card.overlay_revision;
        let text = card
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains("$1.00"), "{text}");
        let mut next_settings = settings.clone();
        next_settings.metric = CostMetric::Quota;
        assert!(tracker.settle_for_test(&TickInput {
            settings: &next_settings,
            ..input
        }));
        let newer = tracker.overlay().prepared_card(id).unwrap();
        assert_ne!(newer.overlay_revision, old_revision);
        assert!(!Arc::ptr_eq(&newer, &card));
        let directory = tempfile::tempdir().unwrap();
        let mut app = crate::app::App::new(
            "actual-cost-ack-fixture".into(),
            directory.path().to_path_buf(),
        );
        app.tree = tree;
        app.layout.tree_area = ratatui::layout::Rect::new(0, 0, 80, 24);
        let stale_pointer = crate::tree_ui::TreeNodeHit {
            id,
            row: 99,
            line: 0,
        };
        app.hovered_tree_node = Some(stale_pointer);
        let rows = |card| {
            crate::tree_ui::PaintedTreeRows::cost_row_for_test(
                id,
                7,
                "title actually emitted".into(),
                card,
                settings.detail_card,
                Some(id),
            )
        };
        app.record_composed_tree_rows(rows(Arc::clone(&card)));
        assert!(
            app.commit_emitted_geometry(app.capture_emitted_geometry(1)),
            "first actual ready card requests its first paint"
        );
        let (row, values) = app.emitted_cost_card_target().unwrap();
        assert_eq!(
            row, 7,
            "card anchors to acknowledged title row rather than stale pointer row99"
        );
        assert_eq!(values.cost_title, "title actually emitted");
        assert!(Arc::ptr_eq(values.cost_card.as_ref().unwrap(), &card));
        app.record_composed_tree_rows(rows(Arc::clone(&card)));
        assert!(
            !app.commit_emitted_geometry(app.capture_emitted_geometry(2)),
            "same source Arc/title/row cannot create an ACK redraw loop"
        );
        let acknowledged_rows = Arc::clone(app.composed_tree_rows.as_ref().unwrap());
        app.record_composed_tree_rows(rows(Arc::clone(&newer)));
        assert!(
            app.commit_emitted_geometry(app.capture_emitted_geometry(3)),
            "changed actual CPU source schedules its replacement paint"
        );
        app.record_composed_tree_rows(rows(Arc::clone(&newer)));
        assert!(
            !app.commit_emitted_geometry(app.capture_emitted_geometry(4)),
            "new source settles after one ACK paint"
        );
        let (_, old) = acknowledged_rows
            .cost_card_target(Some(stale_pointer))
            .unwrap();
        assert_eq!(
            old.cost_card.as_ref().unwrap().overlay_revision,
            old_revision
        );
        assert_eq!(old.cost_title, "title actually emitted");
        assert_eq!(
            app.emitted_cost_card_target()
                .unwrap()
                .1
                .cost_card
                .as_ref()
                .unwrap()
                .overlay_revision,
            newer.overlay_revision
        );
        drop(card);
        drop(newer);
        drop(tracker);
        drop(app);
        owner.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            owner
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
        drop(owner);
        assert_eq!(
            quota.snapshot().jobs,
            0,
            "cards do not pin finite job envelopes"
        );
        assert!(
            storage.upgrade().is_some(),
            "retained ACK rows keep actual storage after source/native owner shutdown"
        );
        drop(acknowledged_rows);
        assert!(
            storage.upgrade().is_none(),
            "last ACK row releases the actual card allocation"
        );
    }
    #[test]
    fn oversized_price_capacity_refuses_before_copying_and_keeps_old_overlay() {
        let (tree, id, stats, mut settings) = fixture();
        let mut name = String::with_capacity(128 * 1024);
        name.push_str("model");
        settings.prices.insert(
            name,
            ModelPrice {
                input: 1.0,
                output: 1.0,
                cache_read: 1.0,
                cache_write: 1.0,
            },
        );
        let mut tracker = CostTracker::default();
        let old = Arc::clone(tracker.overlay());
        assert!(!tracker.tick(&TickInput {
            settings: &settings,
            tree: &tree,
            tree_version: 1,
            store: &stats,
            agent_panes: &[id],
            now: Instant::now(),
            now_ms: 0,
            home: None
        }));
        assert!(tracker.active.is_none());
        assert!(Arc::ptr_eq(&old, tracker.overlay()));
        assert!(tracker.history_error().unwrap().contains("settings exceed"));
    }
}
