//! `App` glue of Settings > Optimization (the compaction optimizer).
//!
//! The scan and the report belong to [`crate::compaction_scan`] and
//! [`crate::compaction_report`]; the writers belong to
//! [`crate::agent_config_writer`]; the layout belongs to
//! [`crate::compaction_ui`]. This module owns what is left: which agent is
//! selected, what the agent's configuration currently says, the pending apply
//! confirmation, the outcome note of the last apply/revert, and the action
//! methods the keyboard and mouse handlers call. Like [`crate::cost_app`], it
//! is a thin `impl App` block over plain state.
//!
//! The tab never starts anything on its own: a scan runs only when the user
//! activates "Scan sessions" or "Re-scan", and the agent's configuration file
//! is written only after the user confirms the modal. Reading the current
//! values is a read of two small files when the tab opens and when a scan
//! starts. Plan, apply and revert are synchronous on the UI thread as well:
//! they touch one small file and wait at most a few seconds for the sidecar
//! lock when another writer holds it.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ilium_compaction_analysis::AgentKind;

use crate::agent_config_writer::{
    self as writer, AgentConfigTarget, ApplyPlan, ApplyRecord, ConfigPaths, WriteError,
};
use crate::app::{App, Mode, SettingsState, SettingsTab};
use crate::compaction_report::{group_thousands, ScanSettingsInput};
use crate::compaction_scan::{self, ScanView};
use crate::compaction_ui::{self, Action};
use crate::cost_model::PriceTable;

/// The two sub-tabs, in display order.
pub const OPTIMIZATION_AGENTS: [AgentKind; 2] = [AgentKind::Codex, AgentKind::ClaudeCode];

/// How often a running scan redraws its progress (the event loop sleeps at
/// most this long while a scan runs).
pub const PROGRESS_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The configuration setting the optimizer writes for `agent`.
pub const fn config_target(agent: AgentKind) -> AgentConfigTarget {
    match agent {
        AgentKind::ClaudeCode => AgentConfigTarget::ClaudeAutoCompactWindow,
        AgentKind::Codex => AgentConfigTarget::CodexAutoCompactTokenLimit,
    }
}

/// Human name of `agent` ("Codex", "Claude Code").
pub const fn agent_label(agent: AgentKind) -> &'static str {
    config_target(agent).label()
}

const fn agent_index(agent: AgentKind) -> usize {
    match agent {
        AgentKind::Codex => 0,
        AgentKind::ClaudeCode => 1,
    }
}

/// What the agent's configuration file says about the compaction setting.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CurrentSetting {
    /// Not read yet.
    #[default]
    Unknown,
    /// The file sets the key.
    Value(u64),
    /// The file exists and does not set the key: the CLI default applies.
    NotSet,
    /// The file does not exist (Ilium never creates it): not set.
    NoFile,
    /// The file could not be read or parsed; the text says why.
    Unreadable(String),
}

impl CurrentSetting {
    /// The configured number, when there is one.
    pub fn value(&self) -> Option<u64> {
        match self {
            Self::Value(value) => Some(*value),
            _ => None,
        }
    }
}

/// Whether a note reports a success or a problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteTone {
    Success,
    Error,
}

/// The outcome of the last apply or revert, shown in the tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub tone: NoteTone,
    pub text: String,
}

/// Per-agent state of the tab that is not part of the scan.
#[derive(Debug, Clone, Default)]
pub struct AgentPanel {
    pub current: CurrentSetting,
    /// Resolved file the setting lives in, when it could be resolved.
    pub config_path: Option<PathBuf>,
    /// An applied, not yet reverted change.
    pub record: Option<ApplyRecord>,
    pub note: Option<Note>,
}

/// An apply waiting for the user's confirmation.
#[derive(Debug, Clone)]
pub struct PendingApply {
    pub agent: AgentKind,
    pub plan: ApplyPlan,
    /// Set when the value comes from beyond the observed compactions: the
    /// support text the confirmation shows with its warning.
    pub extrapolated: Option<String>,
    /// Vertical scroll of the confirmation text.
    pub scroll: u16,
}

/// Where the tab reads and writes. Every field defaults to the real
/// location; tests inject temporary directories so nothing touches the real
/// home directory.
#[derive(Debug, Clone, Default)]
pub struct OptimizationPaths {
    /// Home directory holding `.claude` and `.codex` transcripts.
    pub home: Option<PathBuf>,
    /// Directory of the trace caches.
    pub cache_dir: Option<PathBuf>,
    /// Directory of the revert records (Ilium's data directory).
    pub record_dir: Option<PathBuf>,
    /// Configuration files and lock directory of the two agents.
    pub config: Option<ConfigPaths>,
}

impl OptimizationPaths {
    fn home(&self) -> Option<PathBuf> {
        self.home.clone().or_else(|| {
            directories::BaseDirs::new().map(|directories| directories.home_dir().to_path_buf())
        })
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        self.cache_dir
            .clone()
            .or_else(compaction_scan::default_cache_dir)
    }

    /// Ilium's data directory, like the animation plugins' (`~/.local/share/ilium`).
    fn record_dir(&self) -> Option<PathBuf> {
        self.record_dir.clone().or_else(|| {
            directories::ProjectDirs::from("", "", "ilium")
                .map(|dirs| dirs.data_dir().join("compaction-optimizer"))
        })
    }

    fn config(&self, project_roots: &[PathBuf]) -> Result<ConfigPaths, WriteError> {
        let mut paths = match &self.config {
            Some(paths) => paths.clone(),
            None => ConfigPaths::system()?,
        };
        for root in project_roots {
            if !paths.project_dirs.contains(root) {
                paths.project_dirs.push(root.clone());
            }
        }
        Ok(paths)
    }
}

/// Client-side state of the tab, one value on `App`.
#[derive(Debug)]
pub struct OptimizationState {
    pub selected_agent: AgentKind,
    pub pending_apply: Option<PendingApply>,
    pub paths: OptimizationPaths,
    panels: [AgentPanel; 2],
    /// Whether the tab was the visible Settings tab at the last sync; the
    /// current values are re-read on every hidden -> visible transition.
    is_tab_visible: bool,
}

impl Default for OptimizationState {
    fn default() -> Self {
        Self {
            selected_agent: AgentKind::Codex,
            pending_apply: None,
            paths: OptimizationPaths::default(),
            panels: [AgentPanel::default(), AgentPanel::default()],
            is_tab_visible: false,
        }
    }
}

impl OptimizationState {
    pub fn panel(&self, agent: AgentKind) -> &AgentPanel {
        &self.panels[agent_index(agent)]
    }

    pub fn panel_mut(&mut self, agent: AgentKind) -> &mut AgentPanel {
        &mut self.panels[agent_index(agent)]
    }
}

/// Text of a writer error for the tab. A stale file gets its own wording: the
/// user re-reviews instead of retrying blindly.
pub fn describe_write_error(error: &WriteError) -> String {
    match error {
        WriteError::Stale { path, reason } => format!(
            "Nothing was written: {} changed since it was read ({reason}). Review the new state and apply again.",
            path.display()
        ),
        WriteError::Missing { path } => format!(
            "{} does not exist. Ilium never creates the agent's configuration file; run the agent once or create the file, then apply again.",
            path.display()
        ),
        other => format!("Nothing was written: {other}."),
    }
}

impl App {
    /// The selected agent's scan settings, filled from the configuration and
    /// the Agent Cost tab's price overrides.
    fn optimization_scan_settings(&self, agent: AgentKind) -> ScanSettingsInput {
        ScanSettingsInput {
            current_setting_value: self.optimization.panel(agent).current.value(),
            price_table: PriceTable::with_overrides(&self.cost_settings.prices),
        }
    }

    pub(crate) fn optimization_config_paths(&self) -> Result<ConfigPaths, WriteError> {
        self.optimization
            .paths
            .config(&self.agent_setup_project_roots())
    }

    /// Re-reads both agents' configuration values and revert records.
    pub fn refresh_optimization_settings(&mut self) {
        let paths = self.optimization_config_paths();
        let record_dir = self.optimization.paths.record_dir();
        for agent in OPTIMIZATION_AGENTS {
            let target = config_target(agent);
            let (current, config_path) = match &paths {
                Ok(paths) => match writer::read_current(target, paths) {
                    Ok(setting) => (
                        setting
                            .value
                            .map_or(CurrentSetting::NotSet, CurrentSetting::Value),
                        Some(setting.path),
                    ),
                    Err(WriteError::Missing { path }) => (CurrentSetting::NoFile, Some(path)),
                    Err(error) => (CurrentSetting::Unreadable(error.to_string()), None),
                },
                Err(error) => (CurrentSetting::Unreadable(error.to_string()), None),
            };
            let record = record_dir
                .as_deref()
                .and_then(|directory| writer::load_record(directory, target).ok().flatten());
            let panel = self.optimization.panel_mut(agent);
            panel.current = current;
            panel.config_path = config_path;
            panel.record = record;
        }
    }

    /// Tells the tab whether it is the visible Settings tab; reads the
    /// current values when it just became visible.
    pub fn optimization_sync_visibility(&mut self, is_visible: bool) {
        if is_visible && !self.optimization.is_tab_visible {
            self.refresh_optimization_settings();
        }
        self.optimization.is_tab_visible = is_visible;
    }

    /// Starts a scan of `agent` (never automatic). Returns whether a worker
    /// was started.
    pub fn optimization_start_scan(&mut self, agent: AgentKind) -> bool {
        if self.compaction_optimizer.is_scanning(agent) {
            return false;
        }
        self.refresh_optimization_settings();
        self.optimization.panel_mut(agent).note = None;
        let Some(home) = self.optimization.paths.home() else {
            self.optimization.panel_mut(agent).note = Some(Note {
                tone: NoteTone::Error,
                text:
                    "The home directory could not be determined, so no transcripts can be listed."
                        .to_owned(),
            });
            return false;
        };
        let settings = self.optimization_scan_settings(agent);
        let cache_dir = self.optimization.paths.cache_dir();
        self.compaction_optimizer
            .start_scan(agent, home, cache_dir, settings)
    }

    /// Asks the running scan of `agent` to stop; the previous report stays.
    pub fn optimization_cancel_scan(&mut self, agent: AgentKind) {
        self.compaction_optimizer.cancel_scan(agent);
    }

    /// Prepares the apply of the report's recommendation and opens the
    /// confirmation. A failing plan is reported in the tab instead.
    pub fn optimization_begin_apply(&mut self, agent: AgentKind) {
        self.optimization_begin_apply_of(agent, false);
    }

    /// [`Self::optimization_begin_apply`] for the simulated optimum that no
    /// observed compaction supports (the card's `extrapolated_alternative`).
    pub fn optimization_begin_apply_of(&mut self, agent: AgentKind, is_alternative: bool) {
        let Some(card) = self
            .compaction_optimizer
            .report(agent)
            .and_then(|report| report.recommendation.as_ref())
        else {
            return;
        };
        let (new_value, extrapolated) = if is_alternative {
            let Some(alternative) = &card.extrapolated_alternative else {
                return;
            };
            (
                alternative.setting_value,
                Some(alternative.observed_support_text.clone()),
            )
        } else {
            (
                u64::from(card.setting_value),
                card.extrapolated
                    .then(|| card.observed_support_text.clone()),
            )
        };
        let target = config_target(agent);
        let plan = self
            .optimization_config_paths()
            .and_then(|paths| writer::plan_apply(target, &paths, new_value));
        match plan {
            Ok(plan) => {
                self.optimization.panel_mut(agent).note = None;
                self.optimization.pending_apply = Some(PendingApply {
                    agent,
                    plan,
                    extrapolated,
                    scroll: 0,
                });
            }
            Err(error) => {
                self.optimization.panel_mut(agent).note = Some(Note {
                    tone: NoteTone::Error,
                    text: describe_write_error(&error),
                });
            }
        }
    }

    /// Closes the confirmation without writing anything.
    pub fn optimization_cancel_apply(&mut self) {
        self.optimization.pending_apply = None;
    }

    /// Writes the planned change. The outcome lands in the agent's note.
    pub fn optimization_confirm_apply(&mut self) {
        let Some(pending) = self.optimization.pending_apply.take() else {
            return;
        };
        let agent = pending.agent;
        let outcome = match self.optimization.paths.record_dir() {
            Some(directory) => writer::apply(&pending.plan, &directory),
            None => Err(WriteError::HomeUnavailable),
        };
        let note = match outcome {
            Ok(receipt) if receipt.changed => Note {
                tone: NoteTone::Success,
                text: format!(
                    "Applied {} = {} in {} (was {}). Only new {} sessions use it; running sessions keep their old limit.",
                    pending.plan.target.key(),
                    group_thousands(pending.plan.new_value),
                    pending.plan.path.display(),
                    pending
                        .plan
                        .old_value
                        .map_or_else(|| "not set".to_owned(), group_thousands),
                    agent_label(agent),
                ),
            },
            Ok(_) => Note {
                tone: NoteTone::Success,
                text: format!(
                    "{} already held {}; nothing was written.",
                    pending.plan.path.display(),
                    group_thousands(pending.plan.new_value)
                ),
            },
            Err(error) => Note {
                tone: NoteTone::Error,
                text: describe_write_error(&error),
            },
        };
        self.refresh_optimization_settings();
        self.optimization.panel_mut(agent).note = Some(note);
    }

    /// Restores the value recorded by the last apply of `agent`.
    pub fn optimization_revert(&mut self, agent: AgentKind) {
        let Some(record) = self.optimization.panel(agent).record.clone() else {
            return;
        };
        let outcome = match (
            self.optimization_config_paths(),
            self.optimization.paths.record_dir(),
        ) {
            (Ok(paths), Some(directory)) => writer::revert(&record, &paths, &directory),
            (Err(error), _) => Err(error),
            (_, None) => Err(WriteError::HomeUnavailable),
        };
        let note = match outcome {
            Ok(receipt) => Note {
                tone: NoteTone::Success,
                text: match receipt.restored_value {
                    Some(value) => format!(
                        "Reverted: {} is {} again in {}. Only new sessions are affected.",
                        record.target.key(),
                        group_thousands(value),
                        receipt.path.display()
                    ),
                    None => format!(
                        "Reverted: {} was removed from {} (it was not set before).",
                        record.target.key(),
                        receipt.path.display()
                    ),
                },
            },
            Err(error) => Note {
                tone: NoteTone::Error,
                text: describe_write_error(&error).replace("Nothing was written", "Not reverted"),
            },
        };
        self.refresh_optimization_settings();
        self.optimization.panel_mut(agent).note = Some(note);
    }

    /// Runs one UI action of the tab.
    pub(crate) fn optimization_run(&mut self, state: &mut SettingsState, action: Action) {
        match action {
            Action::SelectAgent(agent) => {
                if self.optimization.selected_agent != agent {
                    self.optimization.selected_agent = agent;
                    state.scroll = 0;
                }
            }
            Action::Scan => {
                self.optimization_start_scan(self.optimization.selected_agent);
                state.scroll = 0;
            }
            Action::Cancel => self.optimization_cancel_scan(self.optimization.selected_agent),
            Action::Apply => self.optimization_begin_apply(self.optimization.selected_agent),
            Action::ApplyExtrapolated => {
                self.optimization_begin_apply_of(self.optimization.selected_agent, true);
            }
            Action::Revert => self.optimization_revert(self.optimization.selected_agent),
        }
    }

    /// Handles a key while the Optimization tab is the active Settings tab.
    /// Returns whether the key was consumed (`Esc`, `q`, `Tab` and the other
    /// global keys are not, unless a scan or the confirmation owns them).
    pub(crate) fn optimization_key(&mut self, state: &mut SettingsState, key: KeyEvent) -> bool {
        let content =
            crate::settings_ui::compute_layout_for_mode(self.layout.screen_area, self, state)
                .content_area;
        if self.optimization.pending_apply.is_some() {
            self.optimization_modal_key(key, self.layout.screen_area);
            return true;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        let agent = self.optimization.selected_agent;
        let is_scanning = self.compaction_optimizer.is_scanning(agent);
        let page = compaction_ui::page_height(content);
        let consumed = match key.code {
            KeyCode::Esc if is_scanning => {
                self.optimization_cancel_scan(agent);
                true
            }
            KeyCode::Left | KeyCode::Char('h' | '[') => {
                self.optimization_run(state, Action::SelectAgent(adjacent_agent(agent, -1)));
                true
            }
            KeyCode::Right | KeyCode::Char('l' | ']') => {
                self.optimization_run(state, Action::SelectAgent(adjacent_agent(agent, 1)));
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.scroll = state.scroll.saturating_sub(1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.scroll = state.scroll.saturating_add(1);
                true
            }
            KeyCode::PageUp => {
                state.scroll = state.scroll.saturating_sub(page);
                true
            }
            KeyCode::PageDown => {
                state.scroll = state.scroll.saturating_add(page);
                true
            }
            KeyCode::Home => {
                state.scroll = 0;
                true
            }
            KeyCode::End => {
                state.scroll = u16::MAX;
                true
            }
            KeyCode::Char('s' | 'S') => {
                self.optimization_run(state, Action::Scan);
                true
            }
            KeyCode::Char('c' | 'C') if is_scanning => {
                self.optimization_run(state, Action::Cancel);
                true
            }
            KeyCode::Char('a' | 'A') => {
                self.optimization_run(state, Action::Apply);
                true
            }
            KeyCode::Char('e' | 'E') => {
                self.optimization_run(state, Action::ApplyExtrapolated);
                true
            }
            KeyCode::Char('r' | 'R') => {
                self.optimization_run(state, Action::Revert);
                true
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(action) = compaction_ui::primary_action(self) {
                    self.optimization_run(state, action);
                }
                true
            }
            _ => false,
        };
        if consumed {
            state.scroll = state.scroll.min(compaction_ui::max_scroll(self, content));
        }
        consumed
    }

    /// Keyboard contract of the confirmation: Enter or Y applies, Esc or N
    /// cancels, arrows and page keys scroll the text.
    fn optimization_modal_key(&mut self, key: KeyEvent, screen: ratatui::layout::Rect) {
        let Some(pending) = self.optimization.pending_apply.as_ref() else {
            return;
        };
        let page = compaction_ui::modal_page_height(screen);
        let max_scroll = compaction_ui::modal_max_scroll(pending, screen);
        let Some(pending) = self.optimization.pending_apply.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('n' | 'N') => self.optimization_cancel_apply(),
            KeyCode::Enter | KeyCode::Char('y' | 'Y') => self.optimization_confirm_apply(),
            KeyCode::Up | KeyCode::Char('k') => pending.scroll = pending.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                pending.scroll = pending.scroll.saturating_add(1).min(max_scroll);
            }
            KeyCode::PageUp => pending.scroll = pending.scroll.saturating_sub(page),
            KeyCode::PageDown => {
                pending.scroll = pending.scroll.saturating_add(page).min(max_scroll)
            }
            _ => {}
        }
    }

    /// Handles a left click or wheel turn on the confirmation. Everything
    /// under a modal is inert: the click is always consumed.
    pub(crate) fn optimization_modal_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        screen: ratatui::layout::Rect,
    ) {
        use crossterm::event::{MouseButton, MouseEventKind};
        let position = ratatui::layout::Position::new(mouse.column, mouse.row);
        let Some(pending) = self.optimization.pending_apply.as_mut() else {
            return;
        };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                match compaction_ui::modal_action_at(screen, position) {
                    Some(crate::modal::DialogAction::Confirm) => self.optimization_confirm_apply(),
                    Some(crate::modal::DialogAction::Cancel) => self.optimization_cancel_apply(),
                    None => {}
                }
            }
            MouseEventKind::ScrollUp => pending.scroll = pending.scroll.saturating_sub(3),
            MouseEventKind::ScrollDown => {
                let max_scroll = compaction_ui::modal_max_scroll(pending, screen);
                pending.scroll = pending.scroll.saturating_add(3).min(max_scroll);
            }
            _ => {}
        }
    }

    /// Resolves a left click in the tab's content area. Returns whether the
    /// click landed on a control (and ran it).
    pub(crate) fn optimization_click(
        &mut self,
        state: &mut SettingsState,
        content: ratatui::layout::Rect,
        position: ratatui::layout::Position,
    ) -> bool {
        let Some(action) = compaction_ui::hit(self, content, state.scroll, position) else {
            return false;
        };
        self.optimization_run(state, action);
        state.scroll = state.scroll.min(compaction_ui::max_scroll(self, content));
        true
    }

    /// Per-tick maintenance: applies finished scans and progress. Returns
    /// whether the screen needs a redraw.
    pub(crate) fn tick_compaction(&mut self, now: Instant) -> bool {
        let is_visible = matches!(
            &self.mode,
            Mode::Settings(state) if state.tab == SettingsTab::Optimization
        );
        self.optimization_sync_visibility(is_visible);
        self.compaction_optimizer.drain_events(now) && is_visible
    }

    /// When the event loop must wake next for the scan progress, if a scan
    /// runs and the tab is on screen.
    pub(crate) fn compaction_next_poll(&self, now: Instant) -> Option<Instant> {
        let is_scanning = OPTIMIZATION_AGENTS
            .iter()
            .any(|agent| self.compaction_optimizer.is_scanning(*agent));
        is_scanning.then(|| now + PROGRESS_POLL_INTERVAL)
    }

    /// Whether `view` of the selected agent is a running scan (used by the
    /// header hint of the tab).
    pub fn optimization_is_scanning(&self) -> bool {
        matches!(
            self.compaction_optimizer
                .view(self.optimization.selected_agent),
            ScanView::Listing { .. } | ScanView::Scanning(_)
        )
    }
}

/// The agent `delta` places after `agent`, wrapping.
pub fn adjacent_agent(agent: AgentKind, delta: i32) -> AgentKind {
    let count = OPTIMIZATION_AGENTS.len() as i32;
    let index = OPTIMIZATION_AGENTS
        .iter()
        .position(|candidate| *candidate == agent)
        .unwrap_or(0) as i32;
    OPTIMIZATION_AGENTS[(index + delta).rem_euclid(count) as usize]
}

#[cfg(test)]
mod tests;
