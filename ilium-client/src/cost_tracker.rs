//! Per-agent spend derived from transcript statistics, plus the immutable
//! [`CostOverlay`] the tree renders.
//!
//! Finite jobs on the existing shared CPU bank own all derivation; history
//! scans use the same bounded I/O bank. The engine reads already-parsed
//! [`SessionStats`] snapshots that [`crate::session_stats_store`] maintains,
//! prices them, calibrates "a lot" against the configured policy, and rebuilds
//! one overlay value only when an input changed (new statistics, settings, tree
//! shape, history, or the sparkline window sliding forward). Rendering only
//! reads that overlay, so a mouse-move redraw never re-prices anything.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

mod preparation;
pub(crate) use preparation::MAX_PANES;
pub use preparation::{CostTracker, PreparedCostCard};
use std::time::{Duration, Instant};

use ilium_core::{NodeId, NodeKind, Tree, ROOT_ID};

use crate::cost_history::{
    default_cache_path, sorted_quota, sorted_totals, CostHistory, HistorySnapshot,
};
use crate::cost_model::{
    budget_fill, burn_usd_per_hour, calibrate, is_burn_spike, sparkline_glyphs, spend_cells,
    Calibrated, Calibration, CalibrationInputs, CostLevel, CostMetric, ModelPrice, PriceTable,
    QuotaWindow, ScaleBasis, SpendPoint, SPIKE_MIN_QUOTA_PER_HOUR, SPIKE_MIN_USD_PER_HOUR,
};
use crate::cost_settings::CostSettings;
use crate::session_stats::{SessionStats, TokenTotals};
use crate::session_stats_store::SessionStatsStore;

/// Window over which the burn rate shown in the detail card is measured.
pub const BURN_WINDOW_MS: i64 = 15 * 60 * 1000;
/// Cost statistics are re-derived at least this often so the sparkline's
/// window keeps sliding even while an agent is idle.
const REBUILD_INTERVAL: Duration = Duration::from_secs(30);
/// How often the tracker asks for fresh statistics of one pane.
const PANE_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
/// Transcript readers the tracker may keep busy at once.
pub const MAX_CONCURRENT_STATS_WORKERS: usize = 2;

/// Dollars for one model inside one session.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCost {
    pub model: String,
    /// `None` when the model has no known price.
    pub usd: Option<f64>,
    pub tokens: TokenTotals,
}

/// Everything derived from one agent's statistics.
#[derive(Debug, PartialEq)]
pub struct PaneCost {
    pub usd: f64,
    /// Some tokens had no price and no CLI-reported total covered them.
    pub is_lower_bound: bool,
    pub burn_usd_per_hour: f64,
    /// Spike and sparkline follow the metric the figures were derived for.
    pub is_spike: bool,
    /// Spend per sparkline cell, oldest first.
    pub spark_cells: Vec<f64>,
    /// Percentage points of the chosen quota window this agent saw used up.
    pub quota_points: f64,
    pub quota_burn_per_hour: f64,
    /// Whether the transcript records the chosen quota window at all; Claude
    /// Code transcripts do not.
    pub has_quota: bool,
    pub tokens: TokenTotals,
    pub model_costs: Vec<ModelCost>,
    /// The CLI's own running total (Claude Code), which may trail the tokens.
    pub reported_usd: Option<f64>,
    /// Codex rate-limit windows: name and percent used.
    pub quota: Vec<(String, f64)>,
    pub spend_points: Vec<SpendPoint>,
    pub(crate) storage: CostStorage,
}

impl PaneCost {
    pub fn from_stats(
        stats: &SessionStats,
        prices: &PriceTable,
        settings: &CostSettings,
        now_ms: i64,
    ) -> Self {
        let estimate = prices.estimate(&stats.models);
        let reported_usd = stats.cost.as_ref().map(|cost| cost.total_usd);
        let usd = estimate.usd.max(reported_usd.unwrap_or(0.0));
        let is_lower_bound = estimate.is_lower_bound() && reported_usd.is_none();

        let spend_points: Vec<SpendPoint> = stats
            .spend
            .iter()
            .filter_map(|bucket| {
                Some(SpendPoint {
                    at_ms: bucket.minute_ms,
                    usd: prices.cost(&bucket.model, &bucket.tokens)?,
                })
            })
            .collect();
        let window_key = settings.quota_window.key();
        let quota_spend: Vec<SpendPoint> = stats
            .quota_spend
            .iter()
            .filter(|bucket| &*bucket.window == window_key)
            .map(|bucket| SpendPoint {
                at_ms: bucket.minute_ms,
                usd: bucket.percent_points,
            })
            .collect();
        let has_quota = stats.rate_limits.iter().any(|(name, _)| name == window_key);
        let window_ms = i64::from(settings.sparkline_window_minutes) * 60_000;
        let (series, spike_floor) = match settings.metric {
            CostMetric::Dollars => (&spend_points, SPIKE_MIN_USD_PER_HOUR),
            CostMetric::Quota => (&quota_spend, SPIKE_MIN_QUOTA_PER_HOUR),
        };
        let model_costs = stats
            .models
            .iter()
            .map(|usage| ModelCost {
                model: usage.model.clone(),
                usd: prices.cost(&usage.model, &usage.tokens),
                tokens: usage.tokens,
            })
            .collect();
        Self {
            usd,
            is_lower_bound,
            burn_usd_per_hour: burn_usd_per_hour(&spend_points, now_ms, BURN_WINDOW_MS),
            is_spike: is_burn_spike(series, now_ms, window_ms, spike_floor),
            spark_cells: spend_cells(
                series,
                now_ms,
                window_ms,
                usize::from(settings.sparkline_cells),
            ),
            quota_points: quota_spend.iter().map(|point| point.usd).sum(),
            quota_burn_per_hour: burn_usd_per_hour(&quota_spend, now_ms, BURN_WINDOW_MS),
            has_quota,
            tokens: stats.tokens,
            model_costs,
            reported_usd,
            quota: stats
                .rate_limits
                .iter()
                .map(|(name, window)| (name.clone(), window.used_percent))
                .collect(),
            spend_points,
            storage: CostStorage::default(),
        }
    }
}

impl PaneCost {
    /// Whether the agent has a figure under `metric`.
    pub fn is_available(&self, metric: CostMetric) -> bool {
        match metric {
            CostMetric::Dollars => true,
            CostMetric::Quota => self.has_quota,
        }
    }

    /// The agent's total in the unit of `metric`.
    pub fn amount(&self, metric: CostMetric) -> f64 {
        match metric {
            CostMetric::Dollars => self.usd,
            CostMetric::Quota => self.quota_points,
        }
    }

    /// The agent's recent rate per hour in the unit of `metric`.
    pub fn burn(&self, metric: CostMetric) -> f64 {
        match metric {
            CostMetric::Dollars => self.burn_usd_per_hour,
            CostMetric::Quota => self.quota_burn_per_hour,
        }
    }
}

/// What the tree draws for one row.
#[derive(Debug, Clone, PartialEq)]
pub struct RowCost {
    /// `None` while the agent's statistics are still loading, or when the
    /// chosen metric has no figure for this agent.
    pub level: Option<CostLevel>,
    /// Total in the overlay's metric: dollars, or quota percentage points.
    pub amount: f64,
    pub is_lower_bound: bool,
    /// Rate per hour in the overlay's metric.
    pub burn_per_hour: f64,
    pub spark: String,
    pub is_spike: bool,
    /// Budget calibration only: the agent has spent more than its budget.
    pub is_over_budget: bool,
    pub is_loading: bool,
    /// The agent is loaded but the chosen metric does not cover it (a Claude
    /// Code agent under the quota metric).
    pub is_unavailable: bool,
}

/// Immutable presentation snapshot consumed by the tree renderer.
#[derive(Debug, PartialEq)]
pub struct CostOverlay {
    pub settings: CostSettings,
    /// What `amount` and every total below are measured in.
    pub metric: CostMetric,
    /// Agent panes, plus project/group rows when totals or ordering need them.
    pub rows: HashMap<NodeId, RowCost>,
    pub total_amount: f64,
    pub total_is_lower_bound: bool,
    /// Agents that have a figure under the metric.
    pub agent_count: usize,
    pub calibrated: Calibrated,
    pub history_sessions: usize,
    pub is_history_scanning: bool,
    /// Per-node spend, the sort key of `TreeOrder::CostDescending`.
    pub ranks: HashMap<NodeId, f64>,
    /// Changes exactly when `ranks` does, so order caches can key on it.
    pub rank_epoch: u64,
    /// Full per-agent breakdown for the detail card.
    pub details: HashMap<NodeId, Arc<PaneCost>>,
    /// Totals of every tracked agent, for previewing the peer-relative scale.
    pub peer_totals: Vec<f64>,
    /// Ascending totals of past sessions, for previewing the history scale.
    pub history_sorted: Vec<f64>,
    pub overlay_revision: u64,
    pub(crate) cards: HashMap<NodeId, Arc<PreparedCostCard>>,
    pub(crate) storage: CostStorage,
}

impl Default for CostOverlay {
    fn default() -> Self {
        let settings = CostSettings::default();
        Self {
            calibrated: calibrate(settings.calibration, &inputs_for(&settings, &[], None)),
            metric: settings.metric,
            settings,
            rows: HashMap::new(),
            total_amount: 0.0,
            total_is_lower_bound: false,
            agent_count: 0,
            history_sessions: 0,
            is_history_scanning: false,
            ranks: HashMap::new(),
            rank_epoch: 0,
            details: HashMap::new(),
            peer_totals: Vec::new(),
            history_sorted: Vec::new(),
            overlay_revision: 0,
            cards: HashMap::new(),
            storage: CostStorage::default(),
        }
    }
}

impl CostOverlay {
    /// Amount of `id` in the overlay's metric; unknown rows count as free.
    pub fn prepared_card(&self, id: NodeId) -> Option<Arc<PreparedCostCard>> {
        self.cards.get(&id).map(Arc::clone)
    }

    pub fn amount_of(&self, id: NodeId) -> f64 {
        self.rows.get(&id).map_or(0.0, |row| row.amount)
    }
}

fn inputs_for<'a>(
    settings: &CostSettings,
    peer_totals: &'a [f64],
    history_sorted: Option<&'a [f64]>,
) -> CalibrationInputs<'a> {
    CalibrationInputs {
        fixed_cuts: settings.active_fixed_cuts(),
        burn_cuts: settings.active_burn_cuts(),
        budget: settings.active_budget(),
        peer_totals,
        history_sorted,
    }
}

/// One group's rolled-up figures while walking the tree.
#[derive(Debug, Default, Clone, Copy)]
struct Rollup {
    amount: f64,
    burn: f64,
    is_lower_bound: bool,
    is_loading: bool,
    /// Agents with a figure under the metric.
    agents: usize,
}

struct PaneEntry {
    cost: Arc<PaneCost>,
    /// A weak identity keeps the old allocation address from being reused.
    source: Weak<SessionStats>,
}

/// Inputs of one [`CostTracker::tick`].
pub struct TickInput<'a> {
    pub settings: &'a CostSettings,
    pub tree: &'a Tree,
    pub tree_version: u64,
    pub store: &'a SessionStatsStore,
    /// Agent panes whose spend is tracked.
    pub agent_panes: &'a [NodeId],
    pub now: Instant,
    pub now_ms: i64,
    pub home: Option<&'a Path>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct CostStorage(Option<Arc<ilium_execution::StorageAdmission>>);
impl Drop for CostStorage {
    fn drop(&mut self) {
        let _ = self.0.take();
    }
}
impl PartialEq for CostStorage {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
trait CostTopology {
    fn is_pane(&self, id: NodeId) -> bool;
    fn children(&self, id: NodeId) -> Option<&[NodeId]>;
}
impl CostTopology for Tree {
    fn is_pane(&self, id: NodeId) -> bool {
        self.get(id)
            .is_some_and(|node| matches!(node.kind, NodeKind::Pane { .. }))
    }
    fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        self.children_of(id).ok()
    }
}
trait CostStatistics {
    fn stats(&self, id: NodeId) -> Option<&Arc<SessionStats>>;
}
impl CostStatistics for SessionStatsStore {
    fn stats(&self, id: NodeId) -> Option<&Arc<SessionStats>> {
        self.entry(id).and_then(|entry| entry.stats.as_ref())
    }
}
struct DerivationInput<'a> {
    settings: &'a CostSettings,
    tree: &'a dyn CostTopology,
    tree_version: u64,
    store: &'a dyn CostStatistics,
    agent_panes: &'a [NodeId],
    now: Instant,
    now_ms: i64,
}
impl<'a> From<&'a TickInput<'a>> for DerivationInput<'a> {
    fn from(input: &'a TickInput<'a>) -> Self {
        Self {
            settings: input.settings,
            tree: input.tree,
            tree_version: input.tree_version,
            store: input.store,
            agent_panes: input.agent_panes,
            now: input.now,
            now_ms: input.now_ms,
        }
    }
}
struct DerivedCosts {
    history: HistorySnapshot,
    panes: HashMap<NodeId, PaneEntry>,
    prices: PriceTable,
    price_overrides: BTreeMap<String, ModelPrice>,
    overlay: Arc<CostOverlay>,
    built_for: Option<BuiltFor>,
    last_rebuild: Option<Instant>,
    history_revision: u64,
    /// Per-session history under `history_for`'s metric and window.
    sorted_history: Vec<f64>,
    history_for: (CostMetric, QuotaWindow),
    /// Settings every `PaneCost` was derived under; a change recomputes all.
    pane_key: (CostMetric, QuotaWindow, u32, u8),
    capture_storage: CostStorage,
}

/// The inputs an overlay was last built from; any difference forces a rebuild.
#[derive(Debug, Clone, PartialEq)]
struct BuiltFor {
    settings: CostSettings,
    tree_version: u64,
    panes_changed: u64,
    history_revision: u64,
    scanning: bool,
}

impl Default for DerivedCosts {
    fn default() -> Self {
        Self {
            history: HistorySnapshot::default(),
            panes: HashMap::new(),
            prices: PriceTable::default(),
            price_overrides: BTreeMap::new(),
            overlay: Arc::new(CostOverlay::default()),
            built_for: None,
            last_rebuild: None,
            history_revision: 0,
            sorted_history: Vec::new(),
            history_for: (CostMetric::Dollars, QuotaWindow::Primary),
            pane_key: (CostMetric::Dollars, QuotaWindow::Primary, 0, 0),
            capture_storage: CostStorage::default(),
        }
    }
}

impl std::fmt::Debug for DerivedCosts {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CostTracker")
            .field("panes", &self.panes.len())
            .finish()
    }
}

impl DerivedCosts {
    /// Re-derives whatever changed. Returns whether the overlay was rebuilt.
    fn tick(&mut self, input: &DerivationInput<'_>) -> Result<bool, String> {
        if input.settings.prices != self.price_overrides {
            self.price_overrides = input.settings.prices.clone();
            self.prices = PriceTable::with_overrides(&self.price_overrides);
            self.panes.clear();
            self.sorted_history.clear();
            self.history_revision = u64::MAX;
        }
        if !input.settings.is_any_enabled() {
            // Nothing is drawn, so track nothing; keep the overlay inert.
            let was_active = !self.panes.is_empty() || !self.overlay.rows.is_empty();
            if was_active {
                self.panes.clear();
                let storage = preparation::overlay_storage(input, &self.panes, 0)?;
                let mut overlay = CostOverlay {
                    settings: input.settings.clone(),
                    overlay_revision: self.overlay.overlay_revision.wrapping_add(1),
                    ..CostOverlay::default()
                };
                preparation::prepare_cards(&mut overlay, storage)?;
                self.overlay = Arc::new(overlay);
                self.built_for = None;
            }
            return Ok(was_active);
        }

        let history_for = (input.settings.metric, input.settings.quota_window);
        if self.history.revision() != self.history_revision || self.history_for != history_for {
            self.history_revision = self.history.revision();
            self.history_for = history_for;
            self.sorted_history = match history_for.0 {
                CostMetric::Dollars => sorted_totals(self.history.entries(), &self.prices),
                CostMetric::Quota => sorted_quota(self.history.entries(), history_for.1.key()),
            };
        }

        let pane_key = (
            input.settings.metric,
            input.settings.quota_window,
            input.settings.sparkline_window_minutes,
            input.settings.sparkline_cells,
        );
        let settings_moved = self.pane_key != pane_key;
        self.pane_key = pane_key;
        let window_moved = settings_moved
            || self
                .last_rebuild
                .is_none_or(|last| input.now.duration_since(last) >= REBUILD_INTERVAL);
        let mut panes_changed = self.refresh_panes(input, window_moved)?;

        let alive: HashSet<NodeId> = input.agent_panes.iter().copied().collect();
        let before = self.panes.len();
        self.panes
            .retain(|pane_id, _| alive.contains(pane_id) && input.store.stats(*pane_id).is_some());
        if self.panes.len() != before {
            panes_changed += 1;
        }

        let built_for = BuiltFor {
            settings: input.settings.clone(),
            tree_version: input.tree_version,
            panes_changed: 0,
            history_revision: self.history_revision,
            scanning: self.history.is_scanning(),
        };
        let needs_rebuild = panes_changed > 0
            || window_moved
            || self.built_for.as_ref().is_none_or(|previous| {
                BuiltFor {
                    panes_changed: 0,
                    ..previous.clone()
                } != built_for
            });
        if !needs_rebuild {
            return Ok(false);
        }
        let overlay_storage =
            preparation::overlay_storage(input, &self.panes, self.sorted_history.len())?;
        let mut overlay = self.build_overlay(input);
        overlay.overlay_revision = self.overlay.overlay_revision.wrapping_add(1);
        preparation::prepare_cards(&mut overlay, overlay_storage)?;
        self.overlay = Arc::new(overlay);
        self.built_for = Some(built_for);
        self.last_rebuild = Some(input.now);
        Ok(true)
    }

    /// Recomputes each tracked pane whose statistics snapshot changed (or
    /// whose sparkline window moved). Returns how many panes changed.
    fn refresh_panes(
        &mut self,
        input: &DerivationInput<'_>,
        window_moved: bool,
    ) -> Result<u64, String> {
        let mut changed = 0;
        for pane_id in input.agent_panes {
            let Some(stats) = input.store.stats(*pane_id) else {
                continue;
            };
            let source = Arc::downgrade(stats);
            let is_current = self
                .panes
                .get(pane_id)
                .is_some_and(|entry| Weak::ptr_eq(&entry.source, &source));
            if is_current && !window_moved {
                continue;
            }
            let storage = preparation::pane_storage(stats)?;
            let mut cost = PaneCost::from_stats(stats, &self.prices, input.settings, input.now_ms);
            preparation::check_pane_capacity(&cost, storage.1)?;
            cost.storage = CostStorage(Some(storage.0));
            let cost = Arc::new(cost);
            let was_different = self
                .panes
                .get(pane_id)
                .is_none_or(|entry| *entry.cost != *cost);
            self.panes.insert(*pane_id, PaneEntry { cost, source });
            if was_different || !is_current {
                changed += 1;
            }
        }
        Ok(changed)
    }

    fn build_overlay(&self, input: &DerivationInput<'_>) -> CostOverlay {
        let settings = input.settings;
        let metric = settings.metric;
        let peer_totals: Vec<f64> = self
            .panes
            .values()
            .filter(|entry| entry.cost.is_available(metric))
            .map(|entry| entry.cost.amount(metric))
            .collect();
        let history = (!self.sorted_history.is_empty() || self.history.has_result())
            .then_some(self.sorted_history.as_slice());
        let calibrated = calibrate(
            settings.calibration,
            &inputs_for(settings, &peer_totals, history),
        );
        let level_of = |amount: f64, burn: f64| {
            let value = match calibrated.basis {
                ScaleBasis::TotalUsd => amount,
                ScaleBasis::BurnUsdPerHour => burn,
            };
            calibrated.scale.level(value)
        };

        let mut rows: HashMap<NodeId, RowCost> = HashMap::new();
        let mut pane_rollups: HashMap<NodeId, Rollup> = HashMap::new();
        for pane_id in input.agent_panes {
            let Some(entry) = self.panes.get(pane_id) else {
                rows.insert(
                    *pane_id,
                    RowCost {
                        level: None,
                        amount: 0.0,
                        is_lower_bound: false,
                        burn_per_hour: 0.0,
                        spark: String::new(),
                        is_spike: false,
                        is_over_budget: false,
                        is_loading: true,
                        is_unavailable: false,
                    },
                );
                pane_rollups.insert(
                    *pane_id,
                    Rollup {
                        is_loading: true,
                        agents: 1,
                        ..Rollup::default()
                    },
                );
                continue;
            };
            let cost = &entry.cost;
            if !cost.is_available(metric) {
                rows.insert(
                    *pane_id,
                    RowCost {
                        level: None,
                        amount: 0.0,
                        is_lower_bound: false,
                        burn_per_hour: 0.0,
                        spark: String::new(),
                        is_spike: false,
                        is_over_budget: false,
                        is_loading: false,
                        is_unavailable: true,
                    },
                );
                continue;
            }
            let (amount, burn) = (cost.amount(metric), cost.burn(metric));
            // Only dollar totals can be a lower bound (an unpriced model).
            let is_lower_bound = metric == CostMetric::Dollars && cost.is_lower_bound;
            rows.insert(
                *pane_id,
                RowCost {
                    level: Some(level_of(amount, burn)),
                    amount,
                    is_lower_bound,
                    burn_per_hour: burn,
                    spark: sparkline_glyphs(&cost.spark_cells),
                    is_spike: cost.is_spike,
                    is_over_budget: settings.calibration == Calibration::Budget
                        && budget_fill(amount, settings.active_budget()) >= 1.0,
                    is_loading: false,
                    is_unavailable: false,
                },
            );
            pane_rollups.insert(
                *pane_id,
                Rollup {
                    amount,
                    burn,
                    is_lower_bound,
                    is_loading: false,
                    agents: 1,
                },
            );
        }

        let total: Rollup = pane_rollups
            .values()
            .fold(Rollup::default(), |mut sum, pane| {
                sum.amount += pane.amount;
                sum.is_lower_bound |= pane.is_lower_bound;
                sum.agents += pane.agents;
                sum
            });

        if settings.group_totals.enabled || settings.sort_by_cost {
            let mut group_rollups = HashMap::new();
            rollup_groups(input.tree, ROOT_ID, &pane_rollups, &mut group_rollups);
            for (group_id, rollup) in group_rollups {
                if rollup.agents == 0 {
                    continue;
                }
                rows.insert(
                    group_id,
                    RowCost {
                        level: (!rollup.is_loading).then(|| level_of(rollup.amount, rollup.burn)),
                        amount: rollup.amount,
                        is_lower_bound: rollup.is_lower_bound,
                        burn_per_hour: rollup.burn,
                        spark: String::new(),
                        is_spike: false,
                        is_over_budget: false,
                        is_loading: rollup.is_loading,
                        is_unavailable: false,
                    },
                );
            }
        }

        let ranks: HashMap<NodeId, f64> = rows.iter().map(|(id, row)| (*id, row.amount)).collect();
        let rank_epoch = if ranks == self.overlay.ranks {
            self.overlay.rank_epoch
        } else {
            self.overlay.rank_epoch + 1
        };
        CostOverlay {
            settings: settings.clone(),
            metric,
            rows,
            total_amount: total.amount,
            total_is_lower_bound: total.is_lower_bound,
            agent_count: total.agents,
            calibrated,
            history_sessions: self.sorted_history.len(),
            is_history_scanning: self.history.is_scanning(),
            ranks,
            rank_epoch,
            details: self
                .panes
                .iter()
                .map(|(pane_id, entry)| (*pane_id, Arc::clone(&entry.cost)))
                .collect(),
            peer_totals,
            history_sorted: self.sorted_history.clone(),
            overlay_revision: 0,
            cards: HashMap::new(),
            storage: CostStorage::default(),
        }
    }
}

/// Sums every pane beneath each container, returning the total of `id`.
fn rollup_groups(
    tree: &dyn CostTopology,
    id: NodeId,
    panes: &HashMap<NodeId, Rollup>,
    groups: &mut HashMap<NodeId, Rollup>,
) -> Rollup {
    if tree.is_pane(id) {
        return panes.get(&id).copied().unwrap_or_default();
    }
    let Some(children) = tree.children(id) else {
        return Rollup::default();
    };
    let mut sum = Rollup::default();
    for child in children {
        let child_rollup = rollup_groups(tree, *child, panes, groups);
        sum.amount += child_rollup.amount;
        sum.burn += child_rollup.burn;
        sum.is_lower_bound |= child_rollup.is_lower_bound;
        sum.is_loading |= child_rollup.is_loading;
        sum.agents += child_rollup.agents;
    }
    if id != ROOT_ID {
        groups.insert(id, sum);
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_stats::{ModelUsage, ReportedCost, SpendBucket};
    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus};

    const MINUTE: i64 = 60_000;

    fn stats_with(models: Vec<ModelUsage>, spend: Vec<SpendBucket>) -> SessionStats {
        SessionStats {
            models,
            spend,
            ..SessionStats::default()
        }
    }

    fn usage(model: &str, input: u64, output: u64) -> ModelUsage {
        ModelUsage {
            model: model.to_owned(),
            calls: 1,
            tokens: TokenTotals {
                input,
                output,
                ..TokenTotals::default()
            },
        }
    }

    fn settings_window(minutes: u32, cells: u8) -> CostSettings {
        CostSettings {
            sparkline_window_minutes: minutes,
            sparkline_cells: cells,
            ..CostSettings::default()
        }
    }

    #[test]
    fn prices_models_and_marks_unknown_ones_as_a_lower_bound() {
        let stats = stats_with(
            vec![
                usage("claude-haiku-4-5", 1_000_000, 0),
                usage("gpt-reserve", 10, 10),
            ],
            Vec::new(),
        );
        let cost =
            PaneCost::from_stats(&stats, &PriceTable::default(), &CostSettings::default(), 0);
        assert!((cost.usd - 1.0).abs() < 1e-9);
        assert!(cost.is_lower_bound);
        assert_eq!(cost.model_costs[1].usd, None);
    }

    #[test]
    fn a_reported_total_wins_when_larger_and_clears_the_lower_bound() {
        let mut stats = stats_with(
            vec![
                usage("claude-haiku-4-5", 1_000_000, 0),
                usage("mystery", 5, 5),
            ],
            Vec::new(),
        );
        stats.cost = Some(ReportedCost {
            total_usd: 3.5,
            lines_added: 0,
            lines_removed: 0,
            api_duration_ms: 0,
            tool_duration_ms: 0,
            has_unknown_model_cost: false,
            model_costs: Vec::new(),
        });
        let cost =
            PaneCost::from_stats(&stats, &PriceTable::default(), &CostSettings::default(), 0);
        assert_eq!(cost.usd, 3.5);
        assert!(!cost.is_lower_bound);
        assert_eq!(cost.reported_usd, Some(3.5));
    }

    #[test]
    fn spend_points_feed_the_burn_rate_and_sparkline_window() {
        let now = 1_000 * MINUTE;
        let spend = vec![
            SpendBucket {
                minute_ms: now - 5 * MINUTE,
                model: Arc::from("claude-haiku-4-5"),
                tokens: TokenTotals {
                    input: 1_000_000,
                    ..TokenTotals::default()
                },
            },
            SpendBucket {
                minute_ms: now - 50 * MINUTE,
                model: Arc::from("claude-haiku-4-5"),
                tokens: TokenTotals {
                    input: 500_000,
                    ..TokenTotals::default()
                },
            },
        ];
        let stats = stats_with(vec![usage("claude-haiku-4-5", 1_500_000, 0)], spend);
        let cost =
            PaneCost::from_stats(&stats, &PriceTable::default(), &settings_window(60, 4), now);
        // $1 in the last 15 minutes = $4/h.
        assert!((cost.burn_usd_per_hour - 4.0).abs() < 1e-6);
        assert_eq!(cost.spark_cells.len(), 4);
        assert!((cost.spark_cells.iter().sum::<f64>() - 1.5).abs() < 1e-9);
        assert!(cost.spark_cells[0] > 0.0 && cost.spark_cells[3] > 0.0);
    }

    #[test]
    fn refresh_planning_respects_cadence_and_the_worker_cap() {
        let mut tracker = CostTracker::default();
        let panes: Vec<NodeId> = (10..16).map(NodeId).collect();
        let start = Instant::now();
        let first = tracker.plan_refreshes(&panes, 0, start);
        assert_eq!(first.len(), MAX_CONCURRENT_STATS_WORKERS);
        assert!(tracker
            .plan_refreshes(&panes, MAX_CONCURRENT_STATS_WORKERS, start)
            .is_empty());
        let next = tracker.plan_refreshes(&panes, 0, start);
        assert_eq!(next.len(), MAX_CONCURRENT_STATS_WORKERS);
        assert!(next.iter().all(|pane| !first.contains(pane)));
        let later = start + PANE_REFRESH_INTERVAL + Duration::from_secs(1);
        assert_eq!(
            tracker.plan_refreshes(&panes[..1], 0, later),
            vec![panes[0]]
        );
    }

    fn agent_tree() -> (Tree, NodeId, NodeId, NodeId) {
        let mut tree = Tree::new();
        let project = tree.add_group(ROOT_ID, "work").unwrap();
        let first = tree
            .add_pane(project, "a", PaneContentKind::Terminal)
            .unwrap();
        let second = tree
            .add_pane(project, "b", PaneContentKind::Terminal)
            .unwrap();
        for pane in [first, second] {
            tree.set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Idle, None),
            )
            .unwrap();
        }
        (tree, project, first, second)
    }

    #[test]
    fn overlay_rolls_group_totals_up_and_keeps_loading_agents_unrated() {
        let (tree, project, first, second) = agent_tree();
        let mut tracker = DerivedCosts::default();
        let settings = CostSettings {
            calibration: Calibration::FixedBands,
            group_totals: crate::cost_settings::DisplayOption {
                enabled: true,
                ..Default::default()
            },
            ..CostSettings::default()
        };
        // Seed the pane table directly: the store is exercised elsewhere.
        let stats = stats_with(vec![usage("claude-haiku-4-5", 6_000_000, 0)], Vec::new());
        tracker.prices = PriceTable::default();
        tracker.panes.insert(
            first,
            PaneEntry {
                cost: Arc::new(PaneCost::from_stats(&stats, &tracker.prices, &settings, 0)),
                source: Weak::new(),
            },
        );
        let input = TickInput {
            settings: &settings,
            tree: &tree,
            tree_version: 1,
            store: &SessionStatsStore::default(),
            agent_panes: &[first, second],
            now: Instant::now(),
            now_ms: 0,
            home: None,
        };
        let overlay = tracker.build_overlay(&DerivationInput::from(&input));
        assert_eq!(overlay.agent_count, 2);
        assert!((overlay.total_amount - 6.0).abs() < 1e-9);
        let rated = &overlay.rows[&first];
        assert_eq!(rated.level.unwrap().index(), 2, "$6 is in the $5..$20 band");
        assert!(overlay.rows[&second].is_loading);
        assert!(overlay.rows[&second].level.is_none());
        let group = &overlay.rows[&project];
        assert!((group.amount - 6.0).abs() < 1e-9);
        assert!(group.is_loading, "a loading child keeps the group unrated");
        assert_eq!(overlay.ranks[&first], 6.0);
    }

    #[test]
    fn disabled_settings_track_nothing() {
        let (tree, _, first, _) = agent_tree();
        let mut tracker = CostTracker::default();
        let mut settings = CostSettings::default();
        settings.meter.enabled = false;
        let input = TickInput {
            settings: &settings,
            tree: &tree,
            tree_version: 1,
            store: &SessionStatsStore::default(),
            agent_panes: &[first],
            now: Instant::now(),
            now_ms: 0,
            home: None,
        };
        assert!(!tracker.tick(&input));
        assert!(tracker.overlay().rows.is_empty());
    }

    #[test]
    fn budget_calibration_flags_agents_over_budget() {
        let (tree, _, first, _) = agent_tree();
        let mut tracker = DerivedCosts::default();
        let settings = CostSettings {
            calibration: Calibration::Budget,
            budget_usd: 2.0,
            ..CostSettings::default()
        };
        let stats = stats_with(vec![usage("claude-haiku-4-5", 3_000_000, 0)], Vec::new());
        tracker.panes.insert(
            first,
            PaneEntry {
                cost: Arc::new(PaneCost::from_stats(&stats, &tracker.prices, &settings, 0)),
                source: Weak::new(),
            },
        );
        let input = TickInput {
            settings: &settings,
            tree: &tree,
            tree_version: 1,
            store: &SessionStatsStore::default(),
            agent_panes: &[first],
            now: Instant::now(),
            now_ms: 0,
            home: None,
        };
        let overlay = tracker.build_overlay(&DerivationInput::from(&input));
        assert!(overlay.rows[&first].is_over_budget);
        assert_eq!(
            overlay.rows[&first].level.unwrap().index(),
            CostLevel::MAX as usize
        );
    }
    fn quota_stats(readings: &[(i64, f64)], reported_window: &str) -> SessionStats {
        use crate::session_stats::{QuotaBucket, RateLimitWindow};
        SessionStats {
            quota_spend: readings
                .iter()
                .map(|(minute_ms, percent_points)| QuotaBucket {
                    minute_ms: *minute_ms,
                    window: Arc::from(reported_window),
                    percent_points: *percent_points,
                })
                .collect(),
            rate_limits: vec![(
                reported_window.to_owned(),
                RateLimitWindow {
                    used_percent: 40.0,
                    window_minutes: Some(300),
                    resets_at_unix: Some(1),
                },
            )],
            ..SessionStats::default()
        }
    }

    fn quota_settings() -> CostSettings {
        CostSettings {
            metric: CostMetric::Quota,
            calibration: Calibration::FixedBands,
            ..CostSettings::default()
        }
    }

    #[test]
    fn quota_metric_sums_the_chosen_window_and_drives_burn_and_sparkline() {
        let now = 1_000 * MINUTE;
        let stats = quota_stats(
            &[(now - 5 * MINUTE, 3.0), (now - 50 * MINUTE, 1.5)],
            "primary",
        );
        let settings = CostSettings {
            sparkline_window_minutes: 60,
            sparkline_cells: 4,
            ..quota_settings()
        };
        let cost = PaneCost::from_stats(&stats, &PriceTable::default(), &settings, now);
        assert!(cost.has_quota);
        assert!((cost.quota_points - 4.5).abs() < 1e-9);
        // 3 points inside the last 15 minutes = 12 points an hour.
        assert!((cost.quota_burn_per_hour - 12.0).abs() < 1e-9);
        assert!((cost.spark_cells.iter().sum::<f64>() - 4.5).abs() < 1e-9);
        assert_eq!(cost.amount(CostMetric::Quota), cost.quota_points);
        assert_eq!(cost.amount(CostMetric::Dollars), cost.usd);

        let other_window = CostSettings {
            quota_window: QuotaWindow::Secondary,
            ..settings
        };
        let cost = PaneCost::from_stats(&stats, &PriceTable::default(), &other_window, now);
        assert!(!cost.has_quota, "no secondary window was ever reported");
        assert_eq!(cost.quota_points, 0.0);
    }

    #[test]
    fn quota_spike_needs_the_quota_floor_not_the_dollar_one() {
        let now = 1_000 * MINUTE;
        // 1 point in ten minutes is 6 points an hour: a spike over a flat past.
        let stats = quota_stats(&[(now - 2 * MINUTE, 1.0)], "primary");
        let settings = CostSettings {
            sparkline_window_minutes: 360,
            ..quota_settings()
        };
        let cost = PaneCost::from_stats(&stats, &PriceTable::default(), &settings, now);
        assert!(cost.is_spike);
        let dollars = CostSettings {
            metric: CostMetric::Dollars,
            ..settings
        };
        let cost = PaneCost::from_stats(&stats, &PriceTable::default(), &dollars, now);
        assert!(!cost.is_spike, "no dollar spend, so nothing spikes");
    }

    #[test]
    fn quota_overlay_rates_codex_agents_and_leaves_claude_agents_unavailable() {
        let (tree, project, first, second) = agent_tree();
        let mut tracker = DerivedCosts::default();
        let settings = CostSettings {
            group_totals: crate::cost_settings::DisplayOption {
                enabled: true,
                ..Default::default()
            },
            ..quota_settings()
        };
        let codex = quota_stats(&[(0, 9.0)], "primary");
        let claude = stats_with(vec![usage("claude-haiku-4-5", 6_000_000, 0)], Vec::new());
        for (pane, stats) in [(first, &codex), (second, &claude)] {
            tracker.panes.insert(
                pane,
                PaneEntry {
                    cost: Arc::new(PaneCost::from_stats(stats, &tracker.prices, &settings, 0)),
                    source: Weak::new(),
                },
            );
        }
        let input = TickInput {
            settings: &settings,
            tree: &tree,
            tree_version: 1,
            store: &SessionStatsStore::default(),
            agent_panes: &[first, second],
            now: Instant::now(),
            now_ms: 0,
            home: None,
        };
        let overlay = tracker.build_overlay(&DerivationInput::from(&input));
        assert_eq!(overlay.metric, CostMetric::Quota);
        let codex_row = &overlay.rows[&first];
        assert_eq!(codex_row.amount, 9.0);
        assert!(!codex_row.is_lower_bound);
        // 9 points sits in the 8..20 band of the default quota bands.
        assert_eq!(codex_row.level.unwrap().index(), 3);
        let claude_row = &overlay.rows[&second];
        assert!(claude_row.is_unavailable && claude_row.level.is_none());
        assert!(!claude_row.is_loading);
        assert_eq!(overlay.agent_count, 1, "only agents with a figure count");
        assert!((overlay.total_amount - 9.0).abs() < 1e-9);
        assert!((overlay.rows[&project].amount - 9.0).abs() < 1e-9);
        assert_eq!(overlay.peer_totals, vec![9.0]);
    }

    #[test]
    fn quota_budget_is_a_share_of_the_window() {
        let (tree, _, first, _) = agent_tree();
        let mut tracker = DerivedCosts::default();
        let settings = CostSettings {
            calibration: Calibration::Budget,
            quota_budget_percent: 5.0,
            ..quota_settings()
        };
        let stats = quota_stats(&[(0, 6.0)], "primary");
        tracker.panes.insert(
            first,
            PaneEntry {
                cost: Arc::new(PaneCost::from_stats(&stats, &tracker.prices, &settings, 0)),
                source: Weak::new(),
            },
        );
        let input = TickInput {
            settings: &settings,
            tree: &tree,
            tree_version: 1,
            store: &SessionStatsStore::default(),
            agent_panes: &[first],
            now: Instant::now(),
            now_ms: 0,
            home: None,
        };
        let overlay = tracker.build_overlay(&DerivationInput::from(&input));
        assert!(overlay.rows[&first].is_over_budget);
    }

    #[test]
    fn changing_the_metric_recomputes_panes_that_were_already_derived() {
        let (tree, _, first, _) = agent_tree();
        let mut tracker = CostTracker::default();
        let mut store = SessionStatsStore::default();
        store.insert_ready_for_test(first, Arc::new(quota_stats(&[(0, 9.0)], "primary")));
        let now = Instant::now();
        let mut settings = CostSettings {
            calibration: Calibration::FixedBands,
            ..CostSettings::default()
        };
        let tick = |tracker: &mut CostTracker, settings: &CostSettings| {
            tracker.settle_for_test(&TickInput {
                settings,
                tree: &tree,
                tree_version: 1,
                store: &store,
                agent_panes: &[first],
                now,
                now_ms: 0,
                home: None,
            })
        };
        assert!(tick(&mut tracker, &settings));
        assert_eq!(
            tracker.overlay().rows[&first].amount,
            0.0,
            "no dollar spend"
        );
        settings.metric = CostMetric::Quota;
        assert!(tick(&mut tracker, &settings));
        assert_eq!(tracker.overlay().rows[&first].amount, 9.0);
    }
    #[test]
    fn invalidated_transcript_cache_removes_previously_derived_costs() {
        let (tree, _, first, _) = agent_tree();
        let mut tracker = CostTracker::default();
        let mut store = SessionStatsStore::default();
        store.insert_ready_for_test(first, Arc::new(quota_stats(&[(0, 9.0)], "primary")));
        let settings = CostSettings {
            calibration: Calibration::FixedBands,
            ..quota_settings()
        };
        let now = Instant::now();
        assert!(tracker.settle_for_test(&TickInput {
            settings: &settings,
            tree: &tree,
            tree_version: 1,
            store: &store,
            agent_panes: &[first],
            now,
            now_ms: 0,
            home: None
        }));
        assert!(tracker.pane_cost(first).is_some());
        store.forget(first);
        assert!(tracker.settle_for_test(&TickInput {
            settings: &settings,
            tree: &tree,
            tree_version: 1,
            store: &store,
            agent_panes: &[first],
            now,
            now_ms: 0,
            home: None
        }));
        assert!(
            tracker.pane_cost(first).is_none(),
            "old session cost cannot remain after transcript ownership disappears"
        );
    }
}
