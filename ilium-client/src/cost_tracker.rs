//! Per-agent spend derived from transcript statistics, plus the immutable
//! [`CostOverlay`] the tree renders.
//!
//! The tracker owns no threads besides the history scan it delegates to
//! [`crate::cost_history`]. It reads the already-parsed
//! [`SessionStats`] snapshots that [`crate::session_stats_store`] maintains,
//! prices them, calibrates "a lot" against the configured policy, and rebuilds
//! one overlay value only when an input changed (new statistics, settings, tree
//! shape, history, or the sparkline window sliding forward). Rendering only
//! reads that overlay, so a mouse-move redraw never re-prices anything.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ilium_core::{NodeId, NodeKind, Tree, ROOT_ID};

use crate::cost_history::{default_cache_path, sorted_totals, CostHistory};
use crate::cost_model::{
    budget_fill, burn_usd_per_hour, calibrate, is_burn_spike, sparkline_glyphs, spend_cells,
    Calibrated, Calibration, CalibrationInputs, CostLevel, ModelPrice, PriceTable, ScaleBasis,
    SpendPoint,
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
#[derive(Debug, Clone, PartialEq)]
pub struct PaneCost {
    pub usd: f64,
    /// Some tokens had no price and no CLI-reported total covered them.
    pub is_lower_bound: bool,
    pub burn_usd_per_hour: f64,
    pub is_spike: bool,
    /// Spend per sparkline cell, oldest first.
    pub spark_cells: Vec<f64>,
    pub tokens: TokenTotals,
    pub model_costs: Vec<ModelCost>,
    /// The CLI's own running total (Claude Code), which may trail the tokens.
    pub reported_usd: Option<f64>,
    /// Codex rate-limit windows: name and percent used.
    pub quota: Vec<(String, f64)>,
    pub spend_points: Vec<SpendPoint>,
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
        let window_ms = i64::from(settings.sparkline_window_minutes) * 60_000;
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
            is_spike: is_burn_spike(&spend_points, now_ms, window_ms),
            spark_cells: spend_cells(
                &spend_points,
                now_ms,
                window_ms,
                usize::from(settings.sparkline_cells),
            ),
            tokens: stats.tokens,
            model_costs,
            reported_usd,
            quota: stats
                .rate_limits
                .iter()
                .map(|(name, window)| (name.clone(), window.used_percent))
                .collect(),
            spend_points,
        }
    }
}

/// What the tree draws for one row.
#[derive(Debug, Clone, PartialEq)]
pub struct RowCost {
    /// `None` while the agent's statistics are still loading.
    pub level: Option<CostLevel>,
    pub usd: f64,
    pub is_lower_bound: bool,
    pub burn_usd_per_hour: f64,
    pub spark: String,
    pub is_spike: bool,
    /// Budget calibration only: the agent has spent more than its budget.
    pub is_over_budget: bool,
    pub is_loading: bool,
}

/// Immutable presentation snapshot consumed by the tree renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct CostOverlay {
    pub settings: CostSettings,
    /// Agent panes, plus project/group rows when totals or ordering need them.
    pub rows: HashMap<NodeId, RowCost>,
    pub total_usd: f64,
    pub total_is_lower_bound: bool,
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
}

impl Default for CostOverlay {
    fn default() -> Self {
        let settings = CostSettings::default();
        Self {
            calibrated: calibrate(settings.calibration, &inputs_for(&settings, &[], None)),
            settings,
            rows: HashMap::new(),
            total_usd: 0.0,
            total_is_lower_bound: false,
            agent_count: 0,
            history_sessions: 0,
            is_history_scanning: false,
            ranks: HashMap::new(),
            rank_epoch: 0,
            details: HashMap::new(),
            peer_totals: Vec::new(),
            history_sorted: Vec::new(),
        }
    }
}

impl CostOverlay {
    /// Dollars of `id`; unknown rows count as free.
    pub fn usd_of(&self, id: NodeId) -> f64 {
        self.rows.get(&id).map_or(0.0, |row| row.usd)
    }
}

fn inputs_for<'a>(
    settings: &CostSettings,
    peer_totals: &'a [f64],
    history_sorted: Option<&'a [f64]>,
) -> CalibrationInputs<'a> {
    CalibrationInputs {
        fixed_cuts: settings.fixed_cuts,
        burn_cuts: settings.burn_cuts,
        budget_usd: settings.budget_usd,
        peer_totals,
        history_sorted,
    }
}

/// One group's rolled-up figures while walking the tree.
#[derive(Debug, Default, Clone, Copy)]
struct Rollup {
    usd: f64,
    burn: f64,
    is_lower_bound: bool,
    is_loading: bool,
    agents: usize,
}

struct PaneEntry {
    cost: Arc<PaneCost>,
    /// Address of the statistics snapshot this was derived from.
    source: usize,
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

pub struct CostTracker {
    history: CostHistory,
    history_cache_path: Option<PathBuf>,
    panes: HashMap<NodeId, PaneEntry>,
    last_requested: HashMap<NodeId, Instant>,
    prices: PriceTable,
    price_overrides: BTreeMap<String, ModelPrice>,
    overlay: Arc<CostOverlay>,
    built_for: Option<BuiltFor>,
    last_rebuild: Option<Instant>,
    history_revision: u64,
    sorted_history: Vec<f64>,
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

impl Default for CostTracker {
    fn default() -> Self {
        Self {
            history: CostHistory::default(),
            history_cache_path: default_cache_path(),
            panes: HashMap::new(),
            last_requested: HashMap::new(),
            prices: PriceTable::default(),
            price_overrides: BTreeMap::new(),
            overlay: Arc::new(CostOverlay::default()),
            built_for: None,
            last_rebuild: None,
            history_revision: 0,
            sorted_history: Vec::new(),
        }
    }
}

impl std::fmt::Debug for CostTracker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CostTracker")
            .field("panes", &self.panes.len())
            .finish()
    }
}

impl CostTracker {
    pub fn overlay(&self) -> &Arc<CostOverlay> {
        &self.overlay
    }

    pub fn pane_cost(&self, pane_id: NodeId) -> Option<&PaneCost> {
        self.panes.get(&pane_id).map(|entry| entry.cost.as_ref())
    }

    pub fn prices(&self) -> &PriceTable {
        &self.prices
    }

    /// Overrides where the history cache lives (tests and alternate homes).
    pub fn set_history_cache_path(&mut self, path: Option<PathBuf>) {
        self.history_cache_path = path;
    }

    /// Panes whose statistics should be refreshed now, honouring the
    /// tracker's own cadence and the concurrent-reader cap.
    pub fn plan_refreshes(
        &mut self,
        agent_panes: &[NodeId],
        busy_workers: usize,
        now: Instant,
    ) -> Vec<NodeId> {
        let mut budget = MAX_CONCURRENT_STATS_WORKERS.saturating_sub(busy_workers);
        let mut due = Vec::new();
        let alive: HashSet<NodeId> = agent_panes.iter().copied().collect();
        self.last_requested
            .retain(|pane_id, _| alive.contains(pane_id));
        for pane_id in agent_panes {
            if budget == 0 {
                break;
            }
            let is_due = self
                .last_requested
                .get(pane_id)
                .is_none_or(|last| now.duration_since(*last) >= PANE_REFRESH_INTERVAL);
            if is_due {
                self.last_requested.insert(*pane_id, now);
                due.push(*pane_id);
                budget -= 1;
            }
        }
        due
    }

    /// Re-derives whatever changed. Returns whether the overlay was rebuilt.
    pub fn tick(&mut self, input: &TickInput<'_>) -> bool {
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
                self.overlay = Arc::new(CostOverlay {
                    settings: input.settings.clone(),
                    ..CostOverlay::default()
                });
                self.built_for = None;
            }
            return was_active;
        }

        self.history.drain_events(input.now);
        if input.settings.calibration == Calibration::OwnHistory {
            if let Some(home) = input.home {
                self.history.request_scan(
                    home.to_path_buf(),
                    self.history_cache_path.clone(),
                    input.settings.history_days,
                    input.now,
                );
            }
        }
        if self.history.revision() != self.history_revision {
            self.history_revision = self.history.revision();
            self.sorted_history = sorted_totals(self.history.entries(), &self.prices);
        }

        let window_moved = self
            .last_rebuild
            .is_none_or(|last| input.now.duration_since(last) >= REBUILD_INTERVAL);
        let mut panes_changed = self.refresh_panes(input, window_moved);

        let alive: HashSet<NodeId> = input.agent_panes.iter().copied().collect();
        let before = self.panes.len();
        self.panes.retain(|pane_id, _| alive.contains(pane_id));
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
            return false;
        }
        self.overlay = Arc::new(self.build_overlay(input));
        self.built_for = Some(built_for);
        self.last_rebuild = Some(input.now);
        true
    }

    /// Recomputes each tracked pane whose statistics snapshot changed (or
    /// whose sparkline window moved). Returns how many panes changed.
    fn refresh_panes(&mut self, input: &TickInput<'_>, window_moved: bool) -> u64 {
        let mut changed = 0;
        for pane_id in input.agent_panes {
            let Some(stats) = input
                .store
                .entry(*pane_id)
                .and_then(|entry| entry.stats.as_ref())
            else {
                continue;
            };
            let source = Arc::as_ptr(stats) as usize;
            let is_current = self
                .panes
                .get(pane_id)
                .is_some_and(|entry| entry.source == source);
            if is_current && !window_moved {
                continue;
            }
            let cost = Arc::new(PaneCost::from_stats(
                stats,
                &self.prices,
                input.settings,
                input.now_ms,
            ));
            let was_different = self
                .panes
                .get(pane_id)
                .is_none_or(|entry| *entry.cost != *cost);
            self.panes.insert(*pane_id, PaneEntry { cost, source });
            if was_different || !is_current {
                changed += 1;
            }
        }
        changed
    }

    fn build_overlay(&self, input: &TickInput<'_>) -> CostOverlay {
        let settings = input.settings;
        let peer_totals: Vec<f64> = self.panes.values().map(|entry| entry.cost.usd).collect();
        let history = (!self.sorted_history.is_empty() || self.history.has_result())
            .then_some(self.sorted_history.as_slice());
        let calibrated = calibrate(
            settings.calibration,
            &inputs_for(settings, &peer_totals, history),
        );
        let level_of = |usd: f64, burn: f64| {
            let value = match calibrated.basis {
                ScaleBasis::TotalUsd => usd,
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
                        usd: 0.0,
                        is_lower_bound: false,
                        burn_usd_per_hour: 0.0,
                        spark: String::new(),
                        is_spike: false,
                        is_over_budget: false,
                        is_loading: true,
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
            rows.insert(
                *pane_id,
                RowCost {
                    level: Some(level_of(cost.usd, cost.burn_usd_per_hour)),
                    usd: cost.usd,
                    is_lower_bound: cost.is_lower_bound,
                    burn_usd_per_hour: cost.burn_usd_per_hour,
                    spark: sparkline_glyphs(&cost.spark_cells),
                    is_spike: cost.is_spike,
                    is_over_budget: settings.calibration == Calibration::Budget
                        && budget_fill(cost.usd, settings.budget_usd) >= 1.0,
                    is_loading: false,
                },
            );
            pane_rollups.insert(
                *pane_id,
                Rollup {
                    usd: cost.usd,
                    burn: cost.burn_usd_per_hour,
                    is_lower_bound: cost.is_lower_bound,
                    is_loading: false,
                    agents: 1,
                },
            );
        }

        let total: Rollup = pane_rollups
            .values()
            .fold(Rollup::default(), |mut sum, pane| {
                sum.usd += pane.usd;
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
                        level: (!rollup.is_loading).then(|| level_of(rollup.usd, rollup.burn)),
                        usd: rollup.usd,
                        is_lower_bound: rollup.is_lower_bound,
                        burn_usd_per_hour: rollup.burn,
                        spark: String::new(),
                        is_spike: false,
                        is_over_budget: false,
                        is_loading: rollup.is_loading,
                    },
                );
            }
        }

        let ranks: HashMap<NodeId, f64> = rows.iter().map(|(id, row)| (*id, row.usd)).collect();
        let rank_epoch = if ranks == self.overlay.ranks {
            self.overlay.rank_epoch
        } else {
            self.overlay.rank_epoch + 1
        };
        CostOverlay {
            settings: settings.clone(),
            rows,
            total_usd: total.usd,
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
        }
    }
}

/// Sums every pane beneath each container, returning the total of `id`.
fn rollup_groups(
    tree: &Tree,
    id: NodeId,
    panes: &HashMap<NodeId, Rollup>,
    groups: &mut HashMap<NodeId, Rollup>,
) -> Rollup {
    let Some(node) = tree.get(id) else {
        return Rollup::default();
    };
    if matches!(node.kind, NodeKind::Pane { .. }) {
        return panes.get(&id).copied().unwrap_or_default();
    }
    let Ok(children) = tree.children_of(id) else {
        return Rollup::default();
    };
    let mut sum = Rollup::default();
    for child in children {
        let child_rollup = rollup_groups(tree, *child, panes, groups);
        sum.usd += child_rollup.usd;
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
        let mut tracker = CostTracker::default();
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
                source: 1,
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
        let overlay = tracker.build_overlay(&input);
        assert_eq!(overlay.agent_count, 2);
        assert!((overlay.total_usd - 6.0).abs() < 1e-9);
        let rated = &overlay.rows[&first];
        assert_eq!(rated.level.unwrap().index(), 2, "$6 is in the $5..$20 band");
        assert!(overlay.rows[&second].is_loading);
        assert!(overlay.rows[&second].level.is_none());
        let group = &overlay.rows[&project];
        assert!((group.usd - 6.0).abs() < 1e-9);
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
        let mut tracker = CostTracker::default();
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
                source: 1,
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
        let overlay = tracker.build_overlay(&input);
        assert!(overlay.rows[&first].is_over_budget);
        assert_eq!(
            overlay.rows[&first].level.unwrap().index(),
            CostLevel::MAX as usize
        );
    }
}
