//! Coordinates the usage view with the existing bounded transcript store.
use crate::session_stats::SessionStats;
use crate::weekly_usage::ModelWorkload;
use ilium_execution::{Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

pub(crate) struct ReadClaudeQuota {
    home: std::path::PathBuf,
    now_ms: i64,
}
impl Job for ReadClaudeQuota {
    type Output = Result<Option<crate::weekly_usage_quota::QuotaDisplay>, String>;
    type Error = std::convert::Infallible;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        use std::io::Read;
        if context.stop_requested() {
            return Ok(Err("cancelled".into()));
        }
        let path = self
            .home
            .join(".my-ccstatusline")
            .join("quota-observations.json");
        let result = (|| {
            let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
            let mut data = Vec::new();
            file.take(1024 * 1024 + 1)
                .read_to_end(&mut data)
                .map_err(|error| error.to_string())?;
            crate::weekly_usage_quota::parse_claude_snapshot(&data, self.now_ms)
                .map_err(str::to_owned)
        })();
        Ok(result)
    }
}

/// Background scan for sessions that no pane owns.
#[derive(Default)]
pub(crate) struct DiscoveryState {
    pub(crate) active: Option<Receipt<crate::weekly_usage_discovery::DiscoverSessions>>,
    retention: Option<ilium_execution::Retention>,
    /// Range of the last submitted scan.
    range: Option<crate::weekly_usage::UsageRange>,
    started_at: Option<Instant>,
    pub(crate) status: Option<String>,
}

/// How long a finished scan stays current before the next one.
const DISCOVERY_REFRESH: Duration = Duration::from_secs(120);

pub(crate) struct DeriveUsage {
    stats: Arc<SessionStats>,
    periods: Vec<crate::weekly_usage_history::UsagePeriod>,
    end_ms: i64,
    codex: bool,
    prices: crate::cost_model::PriceTable,
    history_before_ms: Option<i64>,
    range_ms: i64,
}
impl Job for DeriveUsage {
    type Output = Option<(
        Vec<ModelWorkload>,
        bool,
        bool,
        Option<crate::weekly_usage_quota::CodexQuota>,
        Option<i64>,
    )>;
    type Error = std::convert::Infallible;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Ok(None);
        }
        let Some((models, truncated, baseline_unknown)) = models_for_periods(
            &self.stats,
            &self.periods,
            RangeWindow {
                end_ms: self.end_ms,
                range_ms: self.range_ms,
            },
            self.codex,
            &self.prices,
            self.history_before_ms,
            || context.stop_requested(),
        ) else {
            return Ok(None);
        };
        let quota = crate::weekly_usage_quota::codex_weekly_reading(&self.stats, self.end_ms);
        Ok((!context.stop_requested()).then_some((
            models,
            truncated,
            baseline_unknown,
            quota,
            self.stats.last_at_ms,
        )))
    }
}
/// The span of time a derivation covers: `range_ms` back from `end_ms`.
#[derive(Clone, Copy)]
struct RangeWindow {
    end_ms: i64,
    range_ms: i64,
}

fn models_for_periods(
    stats: &SessionStats,
    periods: &[crate::weekly_usage_history::UsagePeriod],
    window: RangeWindow,
    codex: bool,
    prices: &crate::cost_model::PriceTable,
    history_before_ms: Option<i64>,
    mut cancelled: impl FnMut() -> bool,
) -> Option<(Vec<ModelWorkload>, bool, bool)> {
    let RangeWindow { end_ms, range_ms } = window;
    let mut models = Vec::new();
    let mut truncated = false;
    let mut baseline_unknown = false;
    // Events of the same session that predate the first observed ownership:
    // kept as a separately labelled historical estimate, never as verified.
    if let Some(before_ms) = history_before_ms {
        let range_start = end_ms.saturating_sub(range_ms);
        if before_ms > range_start {
            let (mut included, limited) =
                crate::weekly_usage::model_workloads(&stats.spend, range_start, before_ms, prices);
            for model in &mut included {
                model.historical = true;
            }
            truncated |= limited;
            merge_models(&mut models, &included, &mut truncated);
        }
    }
    for period in periods {
        if cancelled() {
            return None;
        }
        let start_ms = period.start_ms.max(end_ms.saturating_sub(range_ms));
        let (start_ms, unknown) = if codex {
            crate::weekly_usage::codex_window_start(&stats.spend, start_ms)
        } else {
            (start_ms, false)
        };
        baseline_unknown |= unknown;
        let (included, limited) = crate::weekly_usage::model_workloads(
            &stats.spend,
            start_ms,
            period.end_ms.unwrap_or(end_ms).min(end_ms),
            prices,
        );
        truncated |= limited;
        merge_models(&mut models, &included, &mut truncated);
    }
    Some((models, truncated, baseline_unknown))
}

pub(crate) struct ActiveDerivation {
    pub receipt: Receipt<DeriveUsage>,
    specification: crate::weekly_usage_history::UsageSource,
    source: Weak<SessionStats>,
}

use crate::app::{App, Mode};
use crate::session_stats_store::{LoadState, SessionStatsStore};
use crate::weekly_usage_history::UsageSource;
use crate::weekly_usage_ui::UsageRow;
use ilium_core::{AgentClass, NodeKind};

#[derive(Default)]
pub(crate) struct UsageAcquisition {
    // Separate key space and lifecycle: normal pane statistics must continue
    // invalidating superseded requests immediately.
    store: SessionStatsStore,
    sources: Vec<SourceState>,
    history_revision: Option<u64>,
    row_ids: Vec<crate::weekly_usage_ui::UsageRowId>,
}
impl UsageAcquisition {
    pub(crate) fn cancel(&mut self) {
        self.store.cancel_pending();
    }
    /// Distinct sessions of `class` whose owner is ambiguous (several panes
    /// claim the same transcript at once). Each session counts once, however
    /// many panes or periods point at it; its usage is never assigned to any
    /// of them.
    pub(crate) fn unassigned_sessions(&self, class: &AgentClass, profile: Option<&str>) -> usize {
        let mut seen: Vec<&crate::session_stats_store::StatsRequest> = Vec::new();
        for source in &self.sources {
            let specification = &source.specification;
            if specification.request.class != *class
                || specification.profile.as_deref() != profile
                || !(specification.ambiguous || specification.periods.is_empty())
            {
                continue;
            }
            if !seen.iter().any(|known| {
                crate::weekly_usage_history::same_session(known, &specification.request)
            }) {
                seen.push(&specification.request);
            }
        }
        seen.len()
    }
}
struct SourceState {
    specification: UsageSource,
    models: Vec<ModelWorkload>,
    status: String,
    completed: bool,
    pending: bool,
    refresh_required: bool,
    snapshot: Weak<SessionStats>,
    _retention: Option<ilium_execution::Retention>,
    source_time_ms: Option<i64>,
    quota: Option<crate::weekly_usage_quota::CodexQuota>,
}

fn merge_models(target: &mut Vec<ModelWorkload>, included: &[ModelWorkload], truncated: &mut bool) {
    for model in included {
        if let Some(existing) = target.iter_mut().find(|existing| {
            existing.model == model.model && existing.historical == model.historical
        }) {
            existing.tokens.input = existing.tokens.input.saturating_add(model.tokens.input);
            existing.tokens.cache_read = existing
                .tokens
                .cache_read
                .saturating_add(model.tokens.cache_read);
            existing.tokens.cache_write = existing
                .tokens
                .cache_write
                .saturating_add(model.tokens.cache_write);
            existing.tokens.output = existing.tokens.output.saturating_add(model.tokens.output);
            existing.tokens.reasoning = existing
                .tokens
                .reasoning
                .saturating_add(model.tokens.reasoning);
            existing.relative_units = existing
                .relative_units
                .zip(model.relative_units)
                .map(|(left, right)| left + right);
            existing.boundary_partial |= model.boundary_partial;
        } else if target.len() < 256 {
            target.push(model.clone());
        } else {
            *truncated = true;
        }
    }
    target.sort_by(|left, right| {
        left.model
            .cmp(&right.model)
            .then(left.historical.cmp(&right.historical))
    });
}

impl App {
    /// Starts and collects the scan for sessions found on disk without a pane.
    fn tick_usage_discovery(
        &self,
        state: &mut crate::weekly_usage_ui::WeeklyUsageState,
        now: Instant,
        now_ms: i64,
    ) -> bool {
        use crate::weekly_usage_discovery::{DiscoverSessions, ProviderRoots};
        let mut changed = false;
        if state
            .discovery
            .range
            .is_some_and(|range| range != state.range)
        {
            if let Some(active) = state.discovery.active.take() {
                active.cancel();
            }
        }
        if let Some(mut receipt) = state.discovery.active.take() {
            match receipt.try_take() {
                JobPoll::Pending => state.discovery.active = Some(receipt),
                JobPoll::Ready(mut retained) => {
                    drop(receipt);
                    let size =
                        match retained.view() {
                            JobOutcome::Finished(Ok(output)) => output
                                .sessions
                                .iter()
                                .map(|found| {
                                    std::mem::size_of::<
                                        crate::weekly_usage_discovery::DiscoveredSession,
                                    >() + found.name.capacity()
                                        + found.session_id.capacity()
                                        + found
                                            .models
                                            .iter()
                                            .map(|model| {
                                                std::mem::size_of::<ModelWorkload>()
                                                    + model.model.capacity()
                                            })
                                            .sum::<usize>()
                                })
                                .sum::<usize>()
                                .saturating_mul(3)
                                .saturating_add(4096),
                            _ => 4096,
                        };
                    let reduced = ilium_execution::retained_result_storage_bytes::<
                        JobOutcome<DiscoverSessions>,
                    >(size)
                    .ok()
                    .is_some_and(|size| retained.try_reduce_to_result_storage(size).is_ok());
                    if reduced {
                        let (outcome, retention) = retained.into_parts();
                        if let JobOutcome::Finished(Ok(output)) = outcome {
                            let mut status =
                                format!("{} more sessions found on disk", output.sessions.len());
                            if output.partial {
                                status.push_str(" · scan limit reached, older sessions skipped");
                            }
                            if output.unreadable > 0 {
                                status.push_str(&format!(
                                    " · {} transcripts unreadable",
                                    output.unreadable
                                ));
                            }
                            state.discovery.status = Some(status);
                            state.discovered = output.sessions;
                            state.discovery.retention = Some(retention);
                            changed = true;
                        }
                    }
                }
                JobPoll::Lost | JobPoll::Taken => {}
            }
        }
        let due = state.discovery.range != Some(state.range)
            || state
                .discovery
                .started_at
                .is_none_or(|at| now.saturating_duration_since(at) >= DISCOVERY_REFRESH);
        if state.discovery.active.is_none() && due {
            let (Some(client), Some(home)) = (
                self.session_stats.execution_client(),
                directories::BaseDirs::new(),
            ) else {
                return changed;
            };
            let sources = &state.acquisition.sources;
            let roots = ProviderRoots::discover(
                home.home_dir(),
                sources.iter().filter_map(|source| {
                    source
                        .specification
                        .request
                        .transcript_hint
                        .as_deref()
                        .map(|path| (&source.specification.request.class, path))
                }),
            );
            let job = DiscoverSessions {
                roots,
                start_ms: now_ms.saturating_sub(state.range.duration_ms()),
                end_ms: now_ms,
                prices: crate::cost_model::PriceTable::with_overrides(&self.cost_settings.prices),
                exclude: sources
                    .iter()
                    .map(|source| {
                        (
                            source.specification.request.class.clone(),
                            source.specification.request.session_id.clone(),
                        )
                    })
                    .collect(),
                archive_projects: vec![self.session_cwd.clone()],
            };
            if let Ok(receipt) = client.try_submit(
                Lane::Io,
                JobCost {
                    input_bytes: 32 * 1024 * 1024,
                    result_bytes: 2 * 1024 * 1024,
                },
                job,
            ) {
                state.discovery.active = Some(receipt);
                state.discovery.range = Some(state.range);
                state.discovery.started_at = Some(now);
            }
        }
        changed
    }

    pub(crate) fn tick_weekly_usage(&mut self, now: Instant) -> bool {
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.sync_weekly_usage_history(now_ms);
        if !matches!(self.mode, Mode::WeeklyUsage(_)) {
            return false;
        }
        let current_candidates: Vec<_> = self
            .tree
            .panes()
            .filter_map(|node| {
                let NodeKind::Pane { status, .. } = &node.kind else {
                    return None;
                };
                let class = status.known_agent_state()?.class.clone();
                if !matches!(class, AgentClass::Claude | AgentClass::Codex) {
                    return None;
                }
                // Historical recovery identifies a transcript, but it does not
                // establish that this pane still owns future transcript events.
                let binding_since_ms = if matches!(status, ilium_core::PaneStatus::Agent(_)) {
                    self.agent_binding_observed_at_ms.get(&node.id).copied()
                } else {
                    None
                };
                Some((
                    node.id,
                    node.name.clone(),
                    class,
                    self.session_stats_request(node.id),
                    binding_since_ms,
                ))
            })
            .collect();
        let mut candidates = Vec::new();
        for (pane_id, title, current_class, request, binding_since_ms) in current_candidates {
            let current_profile = self
                .custom_agent_panes
                .get(&pane_id)
                .map(|identity| identity.profile_id.clone());
            // The current section first, then every section this pane ran
            // before (another provider or another custom profile).
            let mut sections = vec![(current_class.clone(), current_profile.clone())];
            for section in self.weekly_usage_history.sections_of(pane_id, now_ms) {
                if matches!(section.0, AgentClass::Claude | AgentClass::Codex)
                    && !sections.contains(&section)
                {
                    sections.push(section);
                }
            }
            for (class, profile) in sections {
                let historical_provider = class != current_class || profile != current_profile;
                {
                    candidates.push((
                        pane_id,
                        title.clone(),
                        class,
                        profile,
                        if historical_provider {
                            None
                        } else {
                            request.clone()
                        },
                        if historical_provider {
                            None
                        } else {
                            binding_since_ms
                        },
                        historical_provider,
                    ));
                }
            }
        }
        let Mode::WeeklyUsage(mut state) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return false;
        };
        let mut changed = false;
        self.weekly_usage_range = state.range;
        state.history_capacity_limited = self
            .weekly_usage_history
            .coverage_floor_ms
            .is_some_and(|floor| floor > now_ms.saturating_sub(state.range.duration_ms()));
        let row_ids: Vec<_> = candidates
            .iter()
            .map(|(pane_id, _, class, profile, _, _, _)| {
                crate::weekly_usage_ui::UsageRowId::new(*pane_id, class, profile.as_deref())
            })
            .collect();
        if state.acquisition.history_revision != Some(self.weekly_usage_history.revision)
            || state.acquisition.row_ids != row_ids
        {
            // Release presentation copies before retiring their storage leases.
            state.rows.clear();
            changed = true;
            let specifications: Vec<_> = candidates
                .iter()
                .flat_map(|(pane_id, _, class, profile, _, _, _)| {
                    self.weekly_usage_history
                        .sources(*pane_id, class, profile.as_deref(), now_ms)
                })
                .collect();
            let previous_count = state.acquisition.sources.len();
            state
                .acquisition
                .sources
                .retain(|source| specifications.contains(&source.specification));
            changed |= previous_count != state.acquisition.sources.len();
            for specification in specifications {
                if !state
                    .acquisition
                    .sources
                    .iter()
                    .any(|source| source.specification == specification)
                {
                    let completed = specification.periods.is_empty();
                    state.acquisition.sources.push(SourceState {
                        specification,
                        models: Vec::new(),
                        status: "loading".into(),
                        completed,
                        pending: false,
                        refresh_required: false,
                        snapshot: Weak::new(),
                        _retention: None,
                        source_time_ms: None,
                        quota: None,
                    });
                    changed = true;
                }
            }
            state.acquisition.history_revision = Some(self.weekly_usage_history.revision);
            state.acquisition.row_ids = row_ids;
        }
        if let Some(client) = self.session_stats.execution_client() {
            state.acquisition.store.configure_execution(client);
        }
        let contexts = state
            .acquisition
            .sources
            .iter()
            .map(|source| {
                (
                    source.specification.key,
                    source.specification.request.clone(),
                )
            })
            .collect();
        changed |= state.acquisition.store.reconcile_contexts(&contexts);
        changed |= state.acquisition.store.drain_events();
        if let Some(mut receipt) = state.quota_active.take() {
            match receipt.try_take() {
                JobPoll::Pending => state.quota_active = Some(receipt),
                JobPoll::Ready(mut retained) => {
                    drop(receipt);
                    let reduced = ilium_execution::retained_result_storage_bytes::<
                        JobOutcome<ReadClaudeQuota>,
                    >(4096)
                    .ok()
                    .is_some_and(|size| retained.try_reduce_to_result_storage(size).is_ok());
                    if reduced {
                        let (outcome, retention) = retained.into_parts();
                        match outcome {
                            JobOutcome::Finished(Ok(Ok(Some(quota)))) => {
                                state.claude_quota = Some(quota);
                                state.quota_retention = Some(retention);
                            }
                            JobOutcome::Finished(Ok(Ok(None))) => {
                                state.claude_quota = None;
                                state.quota_retention = None;
                                state.quota_status = Some("account reading unavailable".into());
                            }
                            JobOutcome::Finished(Ok(Err(reason))) => {
                                state.claude_quota = None;
                                state.quota_retention = None;
                                state.quota_status =
                                    Some(format!("account reading unavailable · {reason}"));
                            }
                            _ => {
                                state.claude_quota = None;
                                state.quota_retention = None;
                                state.quota_status = Some("account reading unavailable".into());
                            }
                        }
                        changed = true;
                    } else {
                        state.quota_started = false;
                    }
                }
                JobPoll::Lost | JobPoll::Taken => {
                    state.quota_status = Some("account reading unavailable".into());
                    changed = true;
                }
            }
        }
        if state
            .claude_quota
            .as_ref()
            .is_some_and(|quota| now_ms >= quota.reset_at.saturating_mul(1000))
        {
            state.claude_quota = None;
            state.quota_retention = None;
            state.quota_status = Some("account reading expired · reset reached".into());
            if state.quota_active.is_none() {
                state.quota_started = false;
            }
            changed = true;
        }
        if let Some(quota) = state.claude_quota.as_mut() {
            if quota.freshness == "recent confirmed reading"
                && now_ms.saturating_sub(quota.recorded_at_ms) > 5 * 60 * 1000
            {
                quota.freshness = "stale confirmed reading";
                changed = true;
            }
        }
        if state.quota_active.is_none()
            && state
                .quota_started_at
                .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_secs(60))
        {
            state.quota_started = false;
        }
        if !state.quota_started {
            if let (Some(client), Some(home)) = (
                self.session_stats.execution_client(),
                directories::BaseDirs::new(),
            ) {
                let job = ReadClaudeQuota {
                    home: home.home_dir().to_path_buf(),
                    now_ms,
                };
                if let Ok(receipt) = client.try_submit(
                    Lane::Io,
                    JobCost {
                        input_bytes: 2 * 1024 * 1024,
                        result_bytes: 8192,
                    },
                    job,
                ) {
                    state.quota_active = Some(receipt);
                    state.quota_started = true;
                    state.quota_started_at = Some(now);
                }
            }
        }
        changed |= self.tick_usage_discovery(&mut state, now, now_ms);
        if state.derived_range != Some(state.range) {
            // A different span changes every total: drop in-flight work and
            // force each source to derive again from its already-loaded stats.
            if let Some(active) = state.active.take() {
                active.receipt.cancel();
            }
            for source in &mut state.acquisition.sources {
                source.snapshot = Weak::new();
                source.pending = false;
                source.completed = source.specification.periods.is_empty();
            }
            state.derived_range = Some(state.range);
            changed = true;
        }
        if state.active.as_ref().is_some_and(|active| {
            !state
                .acquisition
                .sources
                .iter()
                .any(|source| source.specification == active.specification)
        }) {
            if let Some(active) = state.active.take() {
                active.receipt.cancel();
            }
        }
        if let Some(mut active) = state.active.take() {
            match active.receipt.try_take() {
                JobPoll::Pending => state.active = Some(active),
                JobPoll::Ready(mut retained) => {
                    state.rows.clear();
                    changed = true;
                    drop(active.receipt);
                    // Charge the compact source plus its presentation copy.
                    let result_size = match retained.view() {
                        JobOutcome::Finished(Ok(Some((models, _, _, _, _)))) => 4096usize
                            .saturating_add(
                                models
                                    .iter()
                                    .map(|model| {
                                        std::mem::size_of::<ModelWorkload>()
                                            + model.model.capacity()
                                    })
                                    .sum::<usize>()
                                    .saturating_mul(3),
                            ),
                        _ => 4096,
                    };
                    let reduced = ilium_execution::retained_result_storage_bytes::<
                        JobOutcome<DeriveUsage>,
                    >(result_size)
                    .ok()
                    .is_some_and(|size| retained.try_reduce_to_result_storage(size).is_ok());
                    if reduced {
                        let (outcome, retention) = retained.into_parts();
                        if let JobOutcome::Finished(Ok(Some((
                            models,
                            truncated,
                            baseline_unknown,
                            quota,
                            source_time_ms,
                        )))) = outcome
                        {
                            if let Some(source) = state
                                .acquisition
                                .sources
                                .iter_mut()
                                .find(|source| source.specification == active.specification)
                            {
                                if state
                                    .acquisition
                                    .store
                                    .matching_entry(
                                        source.specification.key,
                                        &source.specification.request,
                                    )
                                    .and_then(|entry| entry.stats.as_ref())
                                    .is_some_and(|stats| {
                                        Weak::ptr_eq(&active.source, &Arc::downgrade(stats))
                                    })
                                {
                                    source.models = models;
                                    source.quota = quota;
                                    source.source_time_ms = source_time_ms;
                                    source._retention = Some(retention);
                                    source.snapshot = active.source;
                                    source.status = "partial · verified ownership intervals".into();
                                    if truncated {
                                        source.status.push_str(" · model limit reached");
                                    }
                                    if baseline_unknown {
                                        source.status.push_str(
                                            " · ownership-boundary counter interval excluded",
                                        );
                                    }
                                    if source
                                        .models
                                        .iter()
                                        .any(|model| model.relative_units.is_none())
                                    {
                                        source.status.push_str(" · unknown model weight");
                                    }
                                    source.completed = true;
                                    source.pending = false;
                                    source.refresh_required = false;
                                    changed = true;
                                }
                            }
                        }
                    }
                }
                JobPoll::Lost | JobPoll::Taken => {}
            }
        }
        let mut derive_candidate = None;
        for source in &mut state.acquisition.sources {
            if source.specification.periods.is_empty() {
                continue;
            }
            let key = source.specification.key;
            let request = &source.specification.request;
            let in_flight = state.acquisition.store.matching_in_flight(key, request);
            if in_flight && source.pending || source.refresh_required && !source.pending {
                continue;
            }
            if let Some(entry) = state.acquisition.store.matching_entry(key, request) {
                match &entry.state {
                    LoadState::Ready => {
                        if let Some(stats) = &entry.stats {
                            if !Weak::ptr_eq(&source.snapshot, &Arc::downgrade(stats)) {
                                if state.active.is_none() && derive_candidate.is_none() {
                                    derive_candidate =
                                        Some((source.specification.clone(), Arc::clone(stats)));
                                }
                                continue;
                            }
                            source.completed = true;
                            source.pending = false;
                            source.refresh_required = false;
                        }
                    }
                    LoadState::Unavailable(reason) => {
                        let status = format!("unavailable · {reason}");
                        changed |= source.status != status;
                        source.status = status;
                        source.completed = true;
                        source.pending = false;
                        source.refresh_required = false;
                    }
                    LoadState::Idle | LoadState::Loading { .. } => {}
                }
            }
        }
        if let Some((specification, stats)) = derive_candidate {
            if let Some(client) = self.session_stats.execution_client() {
                let source = Arc::downgrade(&stats);
                let job = DeriveUsage {
                    stats,
                    periods: specification.periods.clone(),
                    end_ms: now_ms,
                    codex: specification.request.class == AgentClass::Codex,
                    prices: crate::cost_model::PriceTable::with_overrides(
                        &self.cost_settings.prices,
                    ),
                    history_before_ms: (!specification.ambiguous)
                        .then(|| specification.periods.iter().map(|p| p.start_ms).min())
                        .flatten(),
                    range_ms: state.range.duration_ms(),
                };
                if let Ok(receipt) = client.try_submit(
                    Lane::Cpu,
                    JobCost {
                        input_bytes: 1024 * 1024,
                        result_bytes: 512 * 1024,
                    },
                    job,
                ) {
                    state.active = Some(ActiveDerivation {
                        receipt,
                        specification,
                        source,
                    });
                }
            }
        }
        if state
            .acquisition
            .sources
            .iter()
            .all(|source| source.completed)
        {
            match state.last_cycle {
                None => state.last_cycle = Some(now),
                Some(last) if now.saturating_duration_since(last) >= Duration::from_secs(30) => {
                    for source in &mut state.acquisition.sources {
                        if !source.specification.periods.is_empty() {
                            source.completed = false;
                            source.refresh_required = true;
                        }
                    }
                    state.last_cycle = None;
                }
                _ => {}
            }
        }
        if state.active.is_none() {
            if let Some(source) = state
                .acquisition
                .sources
                .iter_mut()
                .find(|source| !source.completed && !source.pending)
            {
                source.pending = state.acquisition.store.request_refresh_interactive(
                    source.specification.key,
                    source.specification.request.clone(),
                    now,
                );
            }
        }
        let pane_rows = state
            .rows
            .iter()
            .filter(|row| row.session_id.is_none())
            .count();
        if changed || pane_rows != candidates.len() {
            state.rows.clear();
            for (pane_id, title, class, profile, request, binding_since_ms, historical_provider) in
                &candidates
            {
                let sources: Vec<_> = state
                    .acquisition
                    .sources
                    .iter()
                    .filter(|source| {
                        source.specification.pane_id == *pane_id
                            && source.specification.request.class == *class
                            && source.specification.profile == *profile
                    })
                    .collect();
                let mut models = Vec::new();
                let mut truncated = false;
                for source in &sources {
                    merge_models(&mut models, &source.models, &mut truncated);
                }
                let mut status = if sources.is_empty() {
                    if *historical_provider {
                        "ownership history unavailable"
                    } else if request.is_none() {
                        "session binding unavailable"
                    } else if binding_since_ms.is_none() {
                        "ownership period unavailable"
                    } else {
                        "loading"
                    }
                    .to_owned()
                } else if sources
                    .iter()
                    .all(|source| source.specification.periods.is_empty())
                {
                    "duplicate session ownership · unassigned".into()
                } else if sources
                    .iter()
                    .any(|source| source.status.starts_with("partial"))
                {
                    "partial · verified ownership intervals".into()
                } else if let Some(source) = sources
                    .iter()
                    .find(|source| source.status.starts_with("unavailable"))
                {
                    source.status.clone()
                } else {
                    "loading".into()
                };
                if *historical_provider {
                    status.push_str(" · closed ownership · no current account binding");
                }
                if sources.iter().any(|source| source.specification.ambiguous)
                    && !status.starts_with("duplicate")
                {
                    status.push_str(" · overlapping ownership excluded");
                }
                if sources.iter().any(|source| !source.completed) && !models.is_empty() {
                    status.push_str(" · refreshing");
                }
                if sources
                    .iter()
                    .any(|source| source.status.starts_with("unavailable"))
                    && !models.is_empty()
                {
                    status.push_str(" · stale: some sessions unavailable");
                }
                if sources.iter().any(|source| {
                    source
                        .status
                        .contains("ownership-boundary counter interval excluded")
                }) {
                    status.push_str(" · ownership-boundary counter interval excluded");
                }
                if truncated
                    || sources
                        .iter()
                        .any(|source| source.status.contains("model limit reached"))
                {
                    status.push_str(" · model limit reached");
                }
                if models.iter().any(|model| model.relative_units.is_none()) {
                    status.push_str(" · unknown model weight");
                }
                let source_time_ms = sources
                    .iter()
                    .filter_map(|source| source.source_time_ms)
                    .max();
                // A custom agent runs on its own backend: the provider's
                // account quota never describes it.
                let codex_quota = sources
                    .iter()
                    .filter(|_| profile.is_none())
                    .filter(|source| request.as_ref() == Some(&source.specification.request))
                    .filter_map(|source| source.quota.as_ref())
                    .filter(|quota| now_ms < quota.reset_at.saturating_mul(1000))
                    .max_by_key(|quota| quota.observed_at_ms)
                    .cloned();
                state.rows.push(UsageRow {
                    pane_id: *pane_id,
                    title: title.clone(),
                    class: class.clone(),
                    profile: profile.as_ref().map(|id| {
                        let known = self
                            .custom_agents
                            .profiles
                            .iter()
                            .find(|candidate| candidate.id == *id);
                        crate::weekly_usage_ui::ProfileLabel {
                            id: id.clone(),
                            name: known.map_or_else(|| id.clone(), |profile| profile.name.clone()),
                            icon: known
                                .map_or_else(|| "✦".to_owned(), |profile| profile.icon.clone()),
                        }
                    }),
                    models,
                    status,
                    binding_since_ms: *binding_since_ms,
                    source_time_ms,
                    codex_quota,
                    historical_provider: *historical_provider,
                    session_id: None,
                });
            }
            let owned: Vec<(AgentClass, String)> = state
                .acquisition
                .sources
                .iter()
                .map(|source| {
                    (
                        source.specification.request.class.clone(),
                        source.specification.request.session_id.clone(),
                    )
                })
                .collect();
            let found_rows: Vec<UsageRow> = state
                .discovered
                .iter()
                .filter(|found| {
                    !owned
                        .iter()
                        .any(|(class, id)| *class == found.class && *id == found.session_id)
                })
                .map(|found| UsageRow {
                    pane_id: ilium_core::NodeId(crate::weekly_usage_discovery::row_key(
                        &found.session_id,
                    )),
                    title: found.name.clone(),
                    class: found.class.clone(),
                    profile: found
                        .proxy_backed
                        .then(|| crate::weekly_usage_ui::ProfileLabel {
                            id: crate::weekly_usage_discovery::PROXY_PROFILE_ID.into(),
                            name: "Kilo Proxy".into(),
                            icon: "🧠".into(),
                        }),
                    models: found.models.clone(),
                    status: "partial · found in the transcript store · no open Ilium pane".into(),
                    binding_since_ms: None,
                    source_time_ms: found.last_ms,
                    codex_quota: None,
                    historical_provider: false,
                    session_id: Some(found.session_id.clone()),
                })
                .collect();
            state.rows.extend(found_rows);
        } else {
            // Tree titles can change without a new transcript or binding.
            for row in &mut state.rows {
                if let Some((_, title, _, _, _, _, _)) = candidates.iter().find(|candidate| {
                    candidate.0 == row.pane_id
                        && candidate.2 == row.class
                        && candidate.3.as_deref() == row.profile.as_ref().map(|p| p.id.as_str())
                }) {
                    if row.title != *title {
                        row.title.clone_from(title);
                        changed = true;
                    }
                }
                if row
                    .codex_quota
                    .as_ref()
                    .is_some_and(|quota| now_ms >= quota.reset_at.saturating_mul(1000))
                {
                    row.codex_quota = None;
                    changed = true;
                }
            }
        }
        state.rows.sort_by(|left, right| {
            let left_total: f64 = left
                .models
                .iter()
                .filter_map(|model| model.relative_units)
                .sum();
            let right_total: f64 = right
                .models
                .iter()
                .filter_map(|model| model.relative_units)
                .sum();
            // Built-in Claude, built-in Codex, then custom agents by profile.
            let section_order = |row: &UsageRow| {
                (
                    row.profile.is_some(),
                    u8::from(row.class != AgentClass::Claude),
                    row.profile.as_ref().map(|profile| profile.name.clone()),
                )
            };
            section_order(left)
                .cmp(&section_order(right))
                .then_with(|| right_total.total_cmp(&left_total))
                .then_with(|| left.title.cmp(&right.title))
                .then_with(|| left.pane_id.cmp(&right.pane_id))
        });
        if state
            .selected
            .is_none_or(|id| !state.rows.iter().any(|row| row.id() == id))
        {
            state.selected = state.rows.first().map(UsageRow::id);
        }
        self.mode = Mode::WeeklyUsage(state);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weekly_usage_provider_switch_keeps_two_rows_with_current_tree_title() {
        use ilium_core::{AgentActivity, PaneContentKind, PaneStatus, ROOT_ID};
        let mut app = App::new("weekly provider switch".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "agents").unwrap();
        let pane = app
            .tree
            .add_pane(group, "old title", PaneContentKind::Terminal)
            .unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None),
            )
            .unwrap();
        app.agent_session_ids.insert(pane, "claude-session".into());
        app.agent_binding_observed_at_ms
            .insert(pane, now_ms - 300_000);
        app.sync_weekly_usage_history(now_ms - 200_000);
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Working, None),
            )
            .unwrap();
        app.agent_session_ids.insert(pane, "codex-session".into());
        app.agent_binding_observed_at_ms
            .insert(pane, now_ms - 100_000);
        app.sync_weekly_usage_history(now_ms);
        app.tree
            .rename_node(pane, "current title", None, None)
            .unwrap();
        app.action_open_weekly_usage();
        app.tick_weekly_usage(Instant::now());
        let Mode::WeeklyUsage(state) = &app.mode else {
            panic!("usage view should stay open")
        };
        assert_eq!(state.rows.len(), 2);
        assert!(state
            .rows
            .iter()
            .all(|row| row.title == "current title" && row.pane_id == pane));
        assert_eq!(state.acquisition.sources.len(), 2);
        let claude = state
            .rows
            .iter()
            .find(|row| row.class == AgentClass::Claude)
            .unwrap();
        let codex = state
            .rows
            .iter()
            .find(|row| row.class == AgentClass::Codex)
            .unwrap();
        assert!(claude.historical_provider);
        assert!(claude.status.contains("no current account binding"));
        assert!(claude.codex_quota.is_none());
        assert!(!codex.historical_provider);
        assert_eq!(codex.binding_since_ms, Some(now_ms - 100_000));
        assert_ne!(claude.id(), codex.id());
    }
    #[test]
    fn a_custom_agent_pane_gets_its_own_row_and_closed_periods_keep_it() {
        use ilium_core::custom_agent::CustomAgentIdentity;
        use ilium_core::{
            AgentActivity, BuiltinAgentProvider, PaneContentKind, PaneStatus, ROOT_ID,
        };
        let mut app = App::new("weekly custom agent".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "agents").unwrap();
        let pane = app
            .tree
            .add_pane(group, "tuned", PaneContentKind::Terminal)
            .unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None),
            )
            .unwrap();
        app.custom_agent_panes.insert(
            pane,
            CustomAgentIdentity {
                profile_id: "tuned-id".into(),
                base_provider: BuiltinAgentProvider::Claude,
            },
        );
        app.agent_session_ids.insert(pane, "custom-session".into());
        app.agent_binding_observed_at_ms
            .insert(pane, now_ms - 300_000);
        app.sync_weekly_usage_history(now_ms - 200_000);
        app.action_open_weekly_usage();
        app.tick_weekly_usage(Instant::now());
        let Mode::WeeklyUsage(state) = &app.mode else {
            panic!("usage view should stay open")
        };
        assert_eq!(state.rows.len(), 1);
        let row = &state.rows[0];
        assert_eq!(
            row.profile.as_ref().map(|p| p.id.as_str()),
            Some("tuned-id")
        );
        assert!(row.codex_quota.is_none());
        assert_eq!(state.acquisition.sources.len(), 1);
        assert_eq!(
            state.acquisition.sources[0]
                .specification
                .profile
                .as_deref(),
            Some("tuned-id")
        );
        // The pane switches back to the built-in agent: the closed custom
        // period stays in its own section next to the new built-in one.
        app.custom_agent_panes.remove(&pane);
        app.agent_session_ids.insert(pane, "plain-session".into());
        app.agent_binding_observed_at_ms
            .insert(pane, now_ms - 100_000);
        app.sync_weekly_usage_history(now_ms);
        app.tick_weekly_usage(Instant::now());
        let Mode::WeeklyUsage(state) = &app.mode else {
            panic!("usage view should stay open")
        };
        let mut profiles: Vec<_> = state
            .rows
            .iter()
            .map(|row| row.profile.as_ref().map(|p| p.id.clone()))
            .collect();
        profiles.sort();
        assert_eq!(profiles, vec![None, Some("tuned-id".to_owned())]);
        let custom = state.rows.iter().find(|row| row.profile.is_some()).unwrap();
        assert!(custom.historical_provider);
        assert_eq!(
            state
                .acquisition
                .unassigned_sessions(&AgentClass::Claude, Some("tuned-id")),
            0
        );
    }

    #[test]
    fn changing_the_range_makes_every_source_derive_again_and_is_remembered() {
        use crate::weekly_usage::UsageRange;
        use ilium_core::{AgentActivity, PaneContentKind, PaneStatus, ROOT_ID};
        let mut app = App::new("weekly range change".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "agents").unwrap();
        let pane = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None),
            )
            .unwrap();
        app.agent_session_ids.insert(pane, "claude-session".into());
        app.agent_binding_observed_at_ms
            .insert(pane, now_ms - 300_000);
        app.sync_weekly_usage_history(now_ms - 200_000);
        app.action_open_weekly_usage();
        app.tick_weekly_usage(Instant::now());
        let Mode::WeeklyUsage(state) = &mut app.mode else {
            panic!("usage view should stay open")
        };
        assert_eq!(state.derived_range, Some(UsageRange::Days7));
        assert!(!state.acquisition.sources.is_empty());
        // Pretend the sources finished deriving for the old range.
        for source in &mut state.acquisition.sources {
            source.completed = true;
        }
        state.range = UsageRange::Hours6;
        app.tick_weekly_usage(Instant::now());
        let Mode::WeeklyUsage(state) = &app.mode else {
            panic!("usage view should stay open")
        };
        assert_eq!(state.derived_range, Some(UsageRange::Hours6));
        assert!(
            state
                .acquisition
                .sources
                .iter()
                .all(|source| !source.completed
                    || source.specification.periods.is_empty()
                    // A pane without a transcript may legitimately settle
                    // as unavailable within the same tick.
                    || source.status.starts_with("unavailable")),
            "every source with periods must derive again for the new range"
        );
        assert_eq!(app.weekly_usage_range, UsageRange::Hours6);
    }

    #[test]
    fn codex_sub_agent_spend_inside_an_ownership_period_is_kept_with_its_own_model() {
        use crate::session_stats::{SpendBucket, TokenTotals};
        use crate::weekly_usage_history::UsagePeriod;
        let bucket = |minute_ms, model: &str, output| SpendBucket {
            minute_ms,
            model: model.into(),
            tokens: TokenTotals {
                output,
                ..TokenTotals::default()
            },
        };
        // Minute 1 straddles the ownership start, so it is the excluded
        // baseline; later minutes hold the parent's and a sub-agent's spend.
        let stats = SessionStats {
            spend: vec![
                bucket(60_000, "gpt-6.1-sol", 1_000),
                bucket(180_000, "gpt-6.1-sol", 10),
                bucket(180_000, "gpt-6.1-luna", 7),
                bucket(240_000, "gpt-6.1-luna", 3),
            ],
            ..SessionStats::default()
        };
        let periods = [UsagePeriod {
            start_ms: 90_000,
            end_ms: None,
        }];
        let (models, _, baseline_unknown) = models_for_periods(
            &stats,
            &periods,
            RangeWindow {
                end_ms: 400_000,
                range_ms: crate::weekly_usage::UsageRange::Days7.duration_ms(),
            },
            true,
            &crate::cost_model::PriceTable::default(),
            None,
            || false,
        )
        .unwrap();
        assert!(baseline_unknown);
        let output = |name: &str| {
            models
                .iter()
                .find(|model| &*model.model == name)
                .map_or(0, |model| model.tokens.output)
        };
        assert_eq!(output("gpt-6.1-sol"), 10, "pre-ownership minute excluded");
        assert_eq!(output("gpt-6.1-luna"), 10, "sub-agent spend is attributed");
    }
    #[test]
    fn weekly_usage_aggregates_closed_and_renewed_periods_without_the_gap() {
        use crate::session_stats::{SpendBucket, TokenTotals};
        use crate::weekly_usage_history::UsagePeriod;
        let stats = SessionStats {
            spend: [(120_000, 5), (240_000, 100), (360_000, 7)]
                .into_iter()
                .map(|(minute_ms, output)| SpendBucket {
                    minute_ms,
                    model: "claude-sonnet-4-5".into(),
                    tokens: TokenTotals {
                        output,
                        ..TokenTotals::default()
                    },
                })
                .collect(),
            ..SessionStats::default()
        };
        let periods = [
            UsagePeriod {
                start_ms: 100_000,
                end_ms: Some(200_000),
            },
            UsagePeriod {
                start_ms: 300_000,
                end_ms: None,
            },
        ];
        let (models, truncated, _) = models_for_periods(
            &stats,
            &periods,
            RangeWindow {
                end_ms: 500_000,
                range_ms: crate::weekly_usage::UsageRange::Days7.duration_ms(),
            },
            false,
            &crate::cost_model::PriceTable::default(),
            None,
            || false,
        )
        .unwrap();
        assert!(!truncated);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].tokens.output, 12);
        assert!(models[0].relative_units.is_some_and(|units| units > 0.0));
        assert!(models_for_periods(
            &stats,
            &periods,
            RangeWindow {
                end_ms: 500_000,
                range_ms: crate::weekly_usage::UsageRange::Days7.duration_ms()
            },
            false,
            &crate::cost_model::PriceTable::default(),
            None,
            || true,
        )
        .is_none());
    }
    #[test]
    fn the_selected_range_limits_which_events_are_counted() {
        use crate::session_stats::{SpendBucket, TokenTotals};
        use crate::weekly_usage::UsageRange;
        use crate::weekly_usage_history::UsagePeriod;
        let hour = 60 * 60 * 1000_i64;
        let end_ms = 40 * 24 * hour;
        let bucket = |minute_ms: i64, output: u64| SpendBucket {
            minute_ms,
            model: "claude-sonnet-4-5".into(),
            tokens: TokenTotals {
                output,
                ..TokenTotals::default()
            },
        };
        let stats = SessionStats {
            spend: vec![
                bucket(end_ms - 2 * hour, 1),
                bucket(end_ms - 20 * hour, 10),
                bucket(end_ms - 10 * 24 * hour, 100),
                bucket(end_ms - 25 * 24 * hour, 1000),
            ],
            ..SessionStats::default()
        };
        let periods = [UsagePeriod {
            start_ms: 0,
            end_ms: None,
        }];
        let prices = crate::cost_model::PriceTable::default();
        let output_for = |range: UsageRange| -> u64 {
            let (models, _, _) = models_for_periods(
                &stats,
                &periods,
                RangeWindow {
                    end_ms,
                    range_ms: range.duration_ms(),
                },
                false,
                &prices,
                None,
                || false,
            )
            .unwrap();
            models.iter().map(|model| model.tokens.output).sum()
        };
        assert_eq!(output_for(UsageRange::Hours6), 1);
        assert_eq!(output_for(UsageRange::Day1), 11);
        assert_eq!(output_for(UsageRange::Days7), 11);
        assert_eq!(output_for(UsageRange::Days14), 111);
        assert_eq!(output_for(UsageRange::Days30), 1111);
    }

    #[test]
    fn events_before_first_observation_become_a_labelled_historical_estimate() {
        use crate::session_stats::{SpendBucket, TokenTotals};
        use crate::weekly_usage_history::UsagePeriod;
        let day = 24 * 60 * 60 * 1000_i64;
        let end_ms = 10 * day;
        let bucket = |minute_ms: i64, output: u64| SpendBucket {
            minute_ms,
            model: "claude-sonnet-4-5".into(),
            tokens: TokenTotals {
                output,
                ..TokenTotals::default()
            },
        };
        let stats = SessionStats {
            // Day 5 predates the observation; day 9 is verified; day 1 is
            // older than the rolling week and must be ignored.
            spend: vec![bucket(day, 100), bucket(5 * day, 10), bucket(9 * day, 20)],
            ..SessionStats::default()
        };
        let periods = [UsagePeriod {
            start_ms: 8 * day,
            end_ms: None,
        }];
        let prices = crate::cost_model::PriceTable::default();
        let (models, _, _) = models_for_periods(
            &stats,
            &periods,
            RangeWindow {
                end_ms,
                range_ms: crate::weekly_usage::UsageRange::Days7.duration_ms(),
            },
            false,
            &prices,
            Some(8 * day),
            || false,
        )
        .unwrap();
        let verified: u64 = models
            .iter()
            .filter(|m| !m.historical)
            .map(|m| m.tokens.output)
            .sum();
        let historical: u64 = models
            .iter()
            .filter(|m| m.historical)
            .map(|m| m.tokens.output)
            .sum();
        assert_eq!(verified, 20);
        assert_eq!(historical, 10);
        let (without, _, _) = models_for_periods(
            &stats,
            &periods,
            RangeWindow {
                end_ms,
                range_ms: crate::weekly_usage::UsageRange::Days7.duration_ms(),
            },
            false,
            &prices,
            None,
            || false,
        )
        .unwrap();
        assert!(without.iter().all(|m| !m.historical));
    }
    #[test]
    fn pricing_overrides_weight_models_missing_from_the_built_in_table() {
        use crate::session_stats::{SpendBucket, TokenTotals};
        let stats = SessionStats {
            spend: vec![SpendBucket {
                minute_ms: 0,
                model: "vendor-private-model".into(),
                tokens: TokenTotals {
                    output: 1_000_000,
                    ..TokenTotals::default()
                },
            }],
            ..SessionStats::default()
        };
        let period = [crate::weekly_usage_history::UsagePeriod {
            start_ms: 0,
            end_ms: None,
        }];
        let default = crate::cost_model::PriceTable::default();
        let (unweighted, _, _) = models_for_periods(
            &stats,
            &period,
            RangeWindow {
                end_ms: 600_000,
                range_ms: crate::weekly_usage::UsageRange::Days7.duration_ms(),
            },
            false,
            &default,
            None,
            || false,
        )
        .unwrap();
        assert!(unweighted[0].relative_units.is_none());
        let mut overrides = std::collections::BTreeMap::new();
        overrides.insert(
            "vendor-private".to_string(),
            crate::cost_model::ModelPrice {
                input: 1.0,
                output: 2.0,
                cache_read: 0.1,
                cache_write: 1.0,
            },
        );
        let table = crate::cost_model::PriceTable::with_overrides(&overrides);
        let (weighted, _, _) = models_for_periods(
            &stats,
            &period,
            RangeWindow {
                end_ms: 600_000,
                range_ms: crate::weekly_usage::UsageRange::Days7.duration_ms(),
            },
            false,
            &table,
            None,
            || false,
        )
        .unwrap();
        assert!(weighted[0].relative_units.is_some_and(|units| units > 0.0));
    }
    #[test]
    fn duplicate_transcript_bindings_remain_ambiguous_across_projects() {
        let first = crate::session_stats_store::StatsRequest {
            transcript_hint: None,
            class: AgentClass::Codex,
            session_id: "session".into(),
            project_path: "/a".into(),
            home: "/home/test".into(),
        };
        let mut second = first.clone();
        second.project_path = "/b".into();
        assert!(crate::weekly_usage_history::same_session(&first, &second));
        second.session_id = "other".into();
        assert!(!crate::weekly_usage_history::same_session(&first, &second));
        second.session_id = first.session_id.clone();
        second.class = AgentClass::Claude;
        assert!(!crate::weekly_usage_history::same_session(&first, &second));
    }

    #[test]
    fn duplicate_panes_are_unassigned_until_only_one_owner_remains() {
        use ilium_core::{AgentActivity, AgentState, PaneContentKind, PaneStatus, ROOT_ID};

        let mut app = App::new("weekly duplicate test".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "group").unwrap();
        let first = app
            .tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = app
            .tree
            .add_pane(group, "second", PaneContentKind::Terminal)
            .unwrap();
        for pane in [first, second] {
            app.tree
                .set_pane_status(
                    pane,
                    PaneStatus::Agent(AgentState::from_activity(
                        AgentClass::Codex,
                        AgentActivity::Working,
                        None,
                    )),
                )
                .unwrap();
            app.agent_session_ids.insert(pane, "same-session".into());
            app.agent_binding_observed_at_ms.insert(pane, 1);
        }
        app.action_open_weekly_usage();
        assert!(app.tick_weekly_usage(Instant::now()));
        let Mode::WeeklyUsage(state) = &app.mode else {
            panic!("weekly usage view should remain open");
        };
        assert_eq!(state.rows.len(), 2);
        assert!(state.rows.iter().all(|row| {
            row.status == "duplicate session ownership · unassigned" && row.models.is_empty()
        }));

        app.tree
            .set_pane_status(second, PaneStatus::PlainShell)
            .unwrap();
        assert!(app.tick_weekly_usage(Instant::now()));
        let Mode::WeeklyUsage(state) = &app.mode else {
            panic!("weekly usage view should remain open");
        };
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].pane_id, first);
        assert_ne!(
            state.rows[0].status,
            "duplicate session ownership · unassigned"
        );

        app.tree
            .rename_node(first, "new title", None, None)
            .unwrap();
        assert!(app.tick_weekly_usage(Instant::now()));
        let Mode::WeeklyUsage(state) = &app.mode else {
            panic!("weekly usage view should remain open");
        };
        assert_eq!(state.rows[0].title, "new title");
    }

    #[test]
    fn weekly_usage_retains_closed_verified_history_when_agent_is_unavailable() {
        use ilium_core::{
            AgentActivity, AgentAvailability, AgentExitOutcome, AgentProcessKey, AgentRecovery,
            AgentState, PaneContentKind, PaneStatus, ROOT_ID,
        };

        let mut app = App::new("weekly ownership test".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "group").unwrap();
        let pane = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        let last_known_state =
            AgentState::from_activity(AgentClass::Codex, AgentActivity::Working, None);
        app.tree
            .set_pane_status(pane, PaneStatus::Agent(last_known_state.clone()))
            .unwrap();
        app.agent_session_ids.insert(pane, "session".into());
        app.agent_binding_observed_at_ms.insert(pane, 1);
        app.action_open_weekly_usage();
        app.tick_weekly_usage(Instant::now());

        app.tree
            .set_pane_status(
                pane,
                PaneStatus::AgentUnavailable(Box::new(AgentRecovery {
                    last_known_state,
                    process: AgentProcessKey {
                        class: AgentClass::Codex,
                        process_id: 99,
                        started_at_unix_seconds: 1,
                    },
                    availability: AgentAvailability::Exited(AgentExitOutcome::Unknown),
                    signal_name: None,
                    session_id: Some("session".into()),
                    last_prompt: None,
                    previous_exact_prompt: None,
                    latest_prompt_unavailable: false,
                })),
            )
            .unwrap();
        app.tick_weekly_usage(Instant::now());
        let Mode::WeeklyUsage(state) = &app.mode else {
            panic!("weekly usage view should remain open");
        };
        assert_eq!(state.rows.len(), 1);
        assert!(state.rows[0].binding_since_ms.is_none());
        assert!(!state.acquisition.sources.is_empty());
        assert!(state
            .acquisition
            .sources
            .iter()
            .flat_map(|source| &source.specification.periods)
            .all(|period| period.end_ms.is_some()));
    }

    #[test]
    fn tree_inventory_includes_more_agents_than_the_statistics_cache() {
        use ilium_core::{AgentActivity, AgentState, PaneContentKind, PaneStatus, ROOT_ID};

        let mut app = App::new("weekly inventory test".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "group").unwrap();
        for index in 0..20 {
            let pane = app
                .tree
                .add_pane(group, format!("agent {index}"), PaneContentKind::Terminal)
                .unwrap();
            app.tree
                .set_pane_status(
                    pane,
                    PaneStatus::Agent(AgentState::from_activity(
                        AgentClass::Codex,
                        AgentActivity::Working,
                        None,
                    )),
                )
                .unwrap();
            app.agent_session_ids
                .insert(pane, format!("session-{index}"));
            app.agent_binding_observed_at_ms.insert(pane, 1);
        }
        app.action_open_weekly_usage();
        assert!(app.tick_weekly_usage(Instant::now()));
        let Mode::WeeklyUsage(state) = &app.mode else {
            panic!("weekly usage view should remain open");
        };
        assert_eq!(state.rows.len(), 20);
        assert!(state.rows.iter().all(|row| row.status == "loading"));
    }
}
