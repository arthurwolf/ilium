//! Typed configuration for automatic LLM title and structure work.
//!
//! This module owns only the durable event-to-actions contract. Runtime event
//! detection stays in `render_cache`/IPC, action execution stays in `App`, and
//! presentation stays in `trigger_settings_ui`; keeping those boundaries
//! separate makes defaults and compatibility rules testable without a TUI,
//! server, or inference provider.

use serde::{Deserialize, Serialize};

use ilium_core::NodeId;

/// One concrete occurrence routed from the render-cache boundary into the
/// synchronous app action executor. Global events deliberately carry no pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerOccurrence {
    pub event: TriggerEvent,
    pub pane_id: Option<NodeId>,
}

impl TriggerOccurrence {
    pub const fn global(event: TriggerEvent) -> Self {
        Self {
            event,
            pane_id: None,
        }
    }

    pub const fn for_pane(event: TriggerEvent, pane_id: NodeId) -> Self {
        Self {
            event,
            pane_id: Some(pane_id),
        }
    }
}

/// Maps agent lifecycle transitions through the same classifier used by
/// sound playback, making the two settings surfaces share one definition of
/// “finished”, “started”, “approval”, and “background waiting”.
pub const fn event_for_sound(sound_event: ilium_sound::SoundEvent) -> TriggerEvent {
    match sound_event {
        ilium_sound::SoundEvent::AgentFinished => TriggerEvent::AgentFinishedWork,
        ilium_sound::SoundEvent::ApprovalRequired => TriggerEvent::AgentApprovalRequired,
        ilium_sound::SoundEvent::AgentStarted => TriggerEvent::AgentStartedWorking,
        ilium_sound::SoundEvent::WaitingBackground => TriggerEvent::AgentWaitingBackground,
    }
}

/// A semantic lifecycle event the user can attach automatic actions to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerEvent {
    StartupComplete,
    AgentSessionReady,
    AgentPromptSubmitted,
    AgentStartedWorking,
    AgentWaitingBackground,
    AgentApprovalRequired,
    AgentFinishedWork,
    TerminalActivityCheckpoint,
}

impl TriggerEvent {
    /// Canonical settings-page order: global lifecycle first, then the agent
    /// turn lifecycle, then the lower-frequency plain-terminal checkpoint.
    pub const ALL: [Self; 8] = [
        Self::StartupComplete,
        Self::AgentSessionReady,
        Self::AgentPromptSubmitted,
        Self::AgentStartedWorking,
        Self::AgentWaitingBackground,
        Self::AgentApprovalRequired,
        Self::AgentFinishedWork,
        Self::TerminalActivityCheckpoint,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::StartupComplete => "Startup complete",
            Self::AgentSessionReady => "Agent session ready",
            Self::AgentPromptSubmitted => "Agent receives a prompt",
            Self::AgentStartedWorking => "Agent starts working",
            Self::AgentWaitingBackground => "Agent waits on background work",
            Self::AgentApprovalRequired => "Agent needs approval",
            Self::AgentFinishedWork => "Agent finishes work",
            Self::TerminalActivityCheckpoint => "Terminal activity checkpoint",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::StartupComplete => "startup_complete",
            Self::AgentSessionReady => "agent_session_ready",
            Self::AgentPromptSubmitted => "agent_prompt_submitted",
            Self::AgentStartedWorking => "agent_started_working",
            Self::AgentWaitingBackground => "agent_waiting_background",
            Self::AgentApprovalRequired => "agent_approval_required",
            Self::AgentFinishedWork => "agent_finished_work",
            Self::TerminalActivityCheckpoint => "terminal_activity_checkpoint",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|event| event.key() == key)
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::StartupComplete => {
                "After the initial tree, restored panes, transcripts, and terminal replay are loaded."
            }
            Self::AgentSessionReady => {
                "When a detected agent and its verified transcript identity are both available."
            }
            Self::AgentPromptSubmitted => {
                "After Enter succeeds for keyboard, scheduled, queued, or newly-created-agent input."
            }
            Self::AgentStartedWorking => {
                "The same semantic transition used by the optional agent-started sound."
            }
            Self::AgentWaitingBackground => {
                "When work pauses for background tasks while the agent remains active."
            }
            Self::AgentApprovalRequired => {
                "The same semantic transition used by the approval-required sound."
            }
            Self::AgentFinishedWork => {
                "The authoritative completion transition that produces the finished-work alert."
            }
            Self::TerminalActivityCheckpoint => {
                "Every second submitted plain-shell command, preserving the existing low-noise cadence."
            }
        }
    }

    pub const fn glyph(self) -> &'static str {
        match self {
            Self::StartupComplete => "◆",
            Self::AgentSessionReady => "◈",
            Self::AgentPromptSubmitted => "↳",
            Self::AgentStartedWorking => "▶",
            Self::AgentWaitingBackground => "◌",
            Self::AgentApprovalRequired => "!",
            Self::AgentFinishedWork => "✓",
            Self::TerminalActivityCheckpoint => ">_",
        }
    }

    pub const fn scope_label(self) -> &'static str {
        match self {
            Self::StartupComplete => "GLOBAL",
            Self::AgentSessionReady
            | Self::AgentPromptSubmitted
            | Self::AgentStartedWorking
            | Self::AgentWaitingBackground
            | Self::AgentApprovalRequired
            | Self::AgentFinishedWork => "AGENT + PROJECT",
            Self::TerminalActivityCheckpoint => "TERMINAL + PROJECT",
        }
    }

    /// Actions meaningful for this event. Global startup has no originating
    /// element/project; every pane event can address its element, its owning
    /// project, or the whole workspace.
    pub const fn available_actions(self) -> &'static [TriggerAction] {
        match self {
            Self::StartupComplete => &STARTUP_ACTIONS,
            _ => &ELEMENT_ACTIONS,
        }
    }
}

/// One LLM-backed operation selected for a trigger event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerAction {
    RetitleElement,
    RestructureProject,
    RestructureAllProjects,
}

impl TriggerAction {
    pub const ALL: [Self; 3] = [
        Self::RetitleElement,
        Self::RestructureProject,
        Self::RestructureAllProjects,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::RetitleElement => "Retitle element",
            Self::RestructureProject => "Restructure project",
            Self::RestructureAllProjects => "Restructure all projects",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::RetitleElement => "retitle_element",
            Self::RestructureProject => "restructure_project",
            Self::RestructureAllProjects => "restructure_all_projects",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.key() == key)
    }

    pub const fn compact_label(self) -> &'static str {
        match self {
            Self::RetitleElement => "Retitle",
            Self::RestructureProject => "Project",
            Self::RestructureAllProjects => "All projects",
        }
    }

    pub const fn glyph(self) -> &'static str {
        match self {
            Self::RetitleElement => "✎",
            Self::RestructureProject => "↻",
            Self::RestructureAllProjects => "⟳",
        }
    }
}

const STARTUP_ACTIONS: [TriggerAction; 1] = [TriggerAction::RestructureAllProjects];
const ELEMENT_ACTIONS: [TriggerAction; 3] = TriggerAction::ALL;

/// Persisted `[triggers]` table. Explicit fields keep hand-edited TOML clear
/// and stable while [`actions_for`](Self::actions_for) gives the runtime one
/// registry-style access point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TriggerSettings {
    pub startup_complete: Vec<TriggerAction>,
    pub agent_session_ready: Vec<TriggerAction>,
    pub agent_prompt_submitted: Vec<TriggerAction>,
    pub agent_started_working: Vec<TriggerAction>,
    pub agent_waiting_background: Vec<TriggerAction>,
    pub agent_approval_required: Vec<TriggerAction>,
    pub agent_finished_work: Vec<TriggerAction>,
    pub terminal_activity_checkpoint: Vec<TriggerAction>,
}

impl Default for TriggerSettings {
    fn default() -> Self {
        Self {
            // Startup is intentionally opt-in: attaching another client or
            // restarting the UI should not spend one LLM call per project by
            // surprise.
            startup_complete: Vec::new(),
            // Session readiness restores the existing automatic first title.
            agent_session_ready: vec![TriggerAction::RetitleElement],
            // A prompt is the earliest meaningful signal that the pane's task
            // may have changed.
            agent_prompt_submitted: vec![TriggerAction::RetitleElement],
            // This normally coalesces behind the prompt-triggered in-flight
            // title, while also providing a retry if transcript persistence
            // lagged behind the Enter event.
            agent_started_working: vec![TriggerAction::RetitleElement],
            agent_waiting_background: Vec::new(),
            agent_approval_required: Vec::new(),
            // Completion refreshes the element title with the finished turn
            // and reorganizes only its owning project, avoiding a global call
            // for every agent.
            agent_finished_work: vec![
                TriggerAction::RetitleElement,
                TriggerAction::RestructureProject,
            ],
            // Preserve the existing every-second-command LLM title behavior,
            // now made visible and configurable.
            terminal_activity_checkpoint: vec![TriggerAction::RetitleElement],
        }
    }
}

impl TriggerSettings {
    pub fn actions_for(&self, event: TriggerEvent) -> &[TriggerAction] {
        match event {
            TriggerEvent::StartupComplete => &self.startup_complete,
            TriggerEvent::AgentSessionReady => &self.agent_session_ready,
            TriggerEvent::AgentPromptSubmitted => &self.agent_prompt_submitted,
            TriggerEvent::AgentStartedWorking => &self.agent_started_working,
            TriggerEvent::AgentWaitingBackground => &self.agent_waiting_background,
            TriggerEvent::AgentApprovalRequired => &self.agent_approval_required,
            TriggerEvent::AgentFinishedWork => &self.agent_finished_work,
            TriggerEvent::TerminalActivityCheckpoint => &self.terminal_activity_checkpoint,
        }
    }

    fn actions_for_mut(&mut self, event: TriggerEvent) -> &mut Vec<TriggerAction> {
        match event {
            TriggerEvent::StartupComplete => &mut self.startup_complete,
            TriggerEvent::AgentSessionReady => &mut self.agent_session_ready,
            TriggerEvent::AgentPromptSubmitted => &mut self.agent_prompt_submitted,
            TriggerEvent::AgentStartedWorking => &mut self.agent_started_working,
            TriggerEvent::AgentWaitingBackground => &mut self.agent_waiting_background,
            TriggerEvent::AgentApprovalRequired => &mut self.agent_approval_required,
            TriggerEvent::AgentFinishedWork => &mut self.agent_finished_work,
            TriggerEvent::TerminalActivityCheckpoint => &mut self.terminal_activity_checkpoint,
        }
    }

    pub fn is_enabled(&self, event: TriggerEvent, action: TriggerAction) -> bool {
        self.actions_for(event).contains(&action)
    }

    /// Replaces one event mapping and applies the same availability,
    /// exclusivity, and ordering rules used by interactive chip toggles.
    pub fn set_actions(&mut self, event: TriggerEvent, actions: Vec<TriggerAction>) {
        *self.actions_for_mut(event) = actions;
        *self = self.clone().normalized();
    }

    /// Toggles one chip. `None` is a real UI choice represented by an empty
    /// action list, so it cannot coexist with active actions. Project-scoped
    /// and all-project restructuring are mutually exclusive because running
    /// both for one occurrence would duplicate the project call.
    pub fn toggle(&mut self, event: TriggerEvent, action: Option<TriggerAction>) {
        let available = event.available_actions();
        let actions = self.actions_for_mut(event);
        let Some(action) = action.filter(|action| available.contains(action)) else {
            actions.clear();
            return;
        };

        if let Some(index) = actions.iter().position(|candidate| *candidate == action) {
            actions.remove(index);
            return;
        }

        match action {
            TriggerAction::RestructureProject => {
                actions.retain(|candidate| *candidate != TriggerAction::RestructureAllProjects)
            }
            TriggerAction::RestructureAllProjects => {
                actions.retain(|candidate| *candidate != TriggerAction::RestructureProject)
            }
            TriggerAction::RetitleElement => {}
        }
        actions.push(action);
        normalize_actions(actions, available);
    }

    /// Cleans hand-edited TOML by removing duplicates, incompatible actions,
    /// and redundant project/global restructure combinations.
    pub fn normalized(mut self) -> Self {
        for event in TriggerEvent::ALL {
            let available = event.available_actions();
            let actions = self.actions_for_mut(event);
            normalize_actions(actions, available);
        }
        self
    }
}

fn normalize_actions(actions: &mut Vec<TriggerAction>, available: &[TriggerAction]) {
    let has_all_projects = actions.contains(&TriggerAction::RestructureAllProjects);
    actions.retain(|action| {
        available.contains(action)
            && !(has_all_projects && *action == TriggerAction::RestructureProject)
    });
    actions.sort_by_key(|action| {
        TriggerAction::ALL
            .iter()
            .position(|candidate| candidate == action)
            .unwrap_or(usize::MAX)
    });
    actions.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_update_titles_without_global_startup_cost() {
        let settings = TriggerSettings::default();

        assert!(settings
            .actions_for(TriggerEvent::StartupComplete)
            .is_empty());
        assert_eq!(
            settings.actions_for(TriggerEvent::AgentPromptSubmitted),
            &[TriggerAction::RetitleElement]
        );
        assert_eq!(
            settings.actions_for(TriggerEvent::AgentFinishedWork),
            &[
                TriggerAction::RetitleElement,
                TriggerAction::RestructureProject,
            ]
        );
    }

    #[test]
    fn none_clears_actions_and_restructure_scopes_are_exclusive() {
        let mut settings = TriggerSettings::default();
        let event = TriggerEvent::AgentFinishedWork;

        settings.toggle(event, Some(TriggerAction::RestructureAllProjects));
        assert!(!settings.is_enabled(event, TriggerAction::RestructureProject));
        assert!(settings.is_enabled(event, TriggerAction::RestructureAllProjects));
        assert!(settings.is_enabled(event, TriggerAction::RetitleElement));

        settings.toggle(event, None);
        assert!(settings.actions_for(event).is_empty());
    }

    #[test]
    fn normalization_removes_duplicates_and_impossible_startup_actions() {
        let settings = TriggerSettings {
            startup_complete: vec![
                TriggerAction::RetitleElement,
                TriggerAction::RestructureAllProjects,
                TriggerAction::RestructureAllProjects,
            ],
            agent_finished_work: vec![
                TriggerAction::RestructureProject,
                TriggerAction::RestructureAllProjects,
            ],
            ..TriggerSettings::default()
        }
        .normalized();

        assert_eq!(
            settings.startup_complete,
            vec![TriggerAction::RestructureAllProjects]
        );
        assert_eq!(
            settings.agent_finished_work,
            vec![TriggerAction::RestructureAllProjects]
        );
    }

    #[test]
    fn partially_specified_toml_keeps_defaults_for_missing_events() {
        let settings: TriggerSettings =
            toml::from_str("agent_finished_work = [\"restructure_all_projects\"]\n").unwrap();

        assert_eq!(
            settings.agent_finished_work,
            vec![TriggerAction::RestructureAllProjects]
        );
        assert_eq!(
            settings.agent_prompt_submitted,
            vec![TriggerAction::RetitleElement]
        );
    }
}
