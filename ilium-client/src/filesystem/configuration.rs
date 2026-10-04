//! Ordered typed configuration commands; file locks, merge, serialization,
//! atomic replacement and durability all execute in the finite I/O bank.
use crate::config::{AgentSetupSettings, SessionSettings};
use ilium_execution::{Job, JobContext, JobCost};
use ilium_ipc::{TextTrigger, TextTriggerSettings};
use std::path::PathBuf;

pub enum ConfigurationChange {
    Ui(Box<crate::config::UiSettings>),
    Terminal(crate::config::TerminalSettings),
    Git(crate::config::GitSettings),
    ResetPlanning(crate::reset_planning::ResetPlanningSettings),
    Cost(crate::cost_settings::CostSettings),
    Editor(crate::config::EditorSettings),
    Onboarding(crate::onboarding::progress::OnboardingProgress),
    Keyboard(crate::config::KeyboardSettings),
    Kanban(crate::config::KanbanBoardSettings),
    Sound(ilium_sound::SoundSettings),
    Notifications(ilium_sound::NotificationSettings),
    Inference(Box<ilium_inference::InferenceSettings>),
    Triggers(crate::trigger_settings::TriggerSettings),
    Voice(crate::config::VoiceSettings),
    Debug(crate::config::DebugSettings),
    Api(crate::config::ApiSettings),
    Keymap(
        crate::config::KeyboardSettings,
        Vec<crate::keymap::KeyBinding>,
    ),
    Session {
        previous: SessionSettings,
        desired: SessionSettings,
    },
    AgentSetup {
        previous: AgentSetupSettings,
        desired: AgentSetupSettings,
    },
    TextTriggers {
        previous: TextTriggerSettings,
        desired: TextTriggerSettings,
    },
    TextTriggerEdit {
        previous: Option<TextTrigger>,
        desired: Option<TextTrigger>,
    },
    Animation(Box<crate::background_animation::AnimationSettings>),
    Separators(bool),
}
pub enum ConfigurationSaved {
    Plain,
    Session(SessionSettings),
    AgentSetup(AgentSetupSettings),
    TextTriggers(TextTriggerSettings),
    Animation(Box<crate::background_animation::AnimationSettings>),
    Separators(bool),
}
pub struct ConfigurationFailure {
    pub message: String,
    /// A durability failure may follow successful replacement. Readback is
    /// observation, never a durable-success acknowledgement or retry licence.
    pub observed_session: Option<SessionSettings>,
    pub observed_agent_setup: Option<AgentSetupSettings>,
}
pub struct ConfigurationWrite {
    pub directory: PathBuf,
    pub change: ConfigurationChange,
}
impl ConfigurationWrite {
    pub const COST: JobCost = JobCost {
        input_bytes: 8 * 1024 * 1024,
        result_bytes: 2 * 1024 * 1024,
    };
}
impl Job for ConfigurationWrite {
    type Output = ConfigurationSaved;
    type Error = ConfigurationFailure;
    fn run(self, _context: JobContext) -> Result<Self::Output, Self::Error> {
        use ConfigurationChange::*;
        let directory = &self.directory;
        let result = match self.change {
            Ui(value) => crate::config::save_ui_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Terminal(value) => crate::config::save_terminal_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Git(value) => crate::config::save_git_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            ResetPlanning(value) => crate::config::save_reset_planning_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Cost(value) => crate::config::save_cost_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Editor(value) => crate::config::save_editor_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Onboarding(value) => crate::config::save_onboarding_progress(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Keyboard(value) => crate::config::save_keyboard_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Kanban(value) => crate::config::save_kanban_board_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Sound(value) => crate::config::save_sound_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Notifications(value) => crate::config::save_notification_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Inference(value) => crate::config::save_inference_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Triggers(value) => crate::config::save_trigger_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Voice(value) => crate::config::save_voice_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Debug(value) => crate::config::save_debug_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Api(value) => crate::config::save_api_settings(directory, &value)
                .map(|()| ConfigurationSaved::Plain),
            Keymap(keyboard, bindings) => {
                crate::config::save_keymap_settings(directory, &keyboard, &bindings)
                    .map(|()| ConfigurationSaved::Plain)
            }
            Session { previous, desired } => {
                crate::config::save_session_settings(directory, &previous, &desired)
                    .map(ConfigurationSaved::Session)
            }
            AgentSetup { previous, desired } => {
                crate::config::update_agent_setup_settings(directory, &previous, &desired)
                    .map(ConfigurationSaved::AgentSetup)
            }
            TextTriggers { previous, desired } => {
                crate::config::save_text_trigger_settings(directory, &previous, &desired)
                    .map(|()| ConfigurationSaved::TextTriggers(desired))
            }
            TextTriggerEdit { previous, desired } => crate::config::save_text_trigger_edit(
                directory,
                previous.as_ref(),
                desired.as_ref(),
            )
            .map(ConfigurationSaved::TextTriggers),
            Animation(settings) => {
                return crate::project_config::set_animation(directory, (*settings).clone())
                    .map(|()| ConfigurationSaved::Animation(settings))
                    .map_err(|error| ConfigurationFailure {
                        message: error.to_string(),
                        observed_session: None,
                        observed_agent_setup: None,
                    })
            }
            Separators(value) => {
                return crate::project_config::set_show_project_separators(directory, value)
                    .map(|()| ConfigurationSaved::Separators(value))
                    .map_err(|error| ConfigurationFailure {
                        message: error.to_string(),
                        observed_session: None,
                        observed_agent_setup: None,
                    })
            }
        };
        result.map_err(|error| {
            let observed = crate::config::load(directory).ok();
            ConfigurationFailure {
                message: error.to_string(),
                observed_session: observed.as_ref().map(|config| config.session),
                observed_agent_setup: observed.map(|config| config.agent_setup),
            }
        })
    }
}

impl ConfigurationChange {
    /// Structural fields without Serialize use exact capacity accounting;
    /// serializable fields are normalized through a bounded transport below.
    pub fn checked_bytes(&self) -> Result<usize, String> {
        use ConfigurationChange::*;
        let bytes = match self {
            Ui(value) => {
                std::mem::size_of::<crate::config::UiSettings>()
                    + value.icons.group.capacity()
                    + value.icons.top_level.capacity()
                    + value.icons.project.capacity()
                    + value.icons.split_vertical.capacity()
                    + value.icons.split_horizontal.capacity()
                    + value.icons.folder.capacity()
                    + value.icons.worktree_branch.capacity()
                    + value.icons.terminal.capacity()
                    + value.icons.editor.capacity()
                    + value.icons.board.capacity()
                    + value.icons.claude.capacity()
                    + value.icons.codex.capacity()
                    + value.icons.antigravity.capacity()
                    + value.icons.other_agent.capacity()
                    + value.icons.working.capacity()
                    + value.icons.waiting_background.capacity()
                    + value.icons.background_task_still_running.capacity()
                    + value.icons.waiting_approval.capacity()
                    + value.icons.done.capacity()
                    + value.icons.idle.capacity()
                    + value.icons.agent_unavailable.capacity()
                    + value.icons.goal_active.capacity()
                    + value.icons.goal_paused.capacity()
                    + value.icons.goal_blocked.capacity()
                    + value.icons.goal_usage_limited.capacity()
                    + value.icons.goal_reached.capacity()
                    + value.icons.parked.capacity()
                    + value.icons.task_pending.capacity()
                    + value.icons.task_done.capacity()
                    + value.icons.task_error.capacity()
                    + value.icons.monitor_failed.capacity()
                    + value.icons.scheduled_input.capacity()
                    + value.icons.bookmark.capacity()
                    + value.icons.lock.capacity()
                    + value.icons.toolbar_search.capacity()
                    + value.icons.toolbar_restructure.capacity()
                    + value.icons.toolbar_settings.capacity()
                    + value.icons.row_rename.capacity()
                    + value.icons.row_move_up.capacity()
                    + value.icons.row_move_down.capacity()
                    + value.icons.row_close.capacity()
                    + value.icons.row_retitle.capacity()
                    + value.icons.row_project_restructure.capacity()
                    + value.icons.ask_for_update.capacity()
                    + value.icons.screen_transfer_left.capacity()
                    + value.icons.screen_transfer_right.capacity()
                    + value.icons.screen_transfer_up.capacity()
                    + value.icons.screen_transfer_down.capacity()
                    + value.icons.open_external.capacity()
                    + value.icons.agent_toolbar_close.capacity()
                    + value.icons.agent_toolbar_compact.capacity()
                    + value.icons.agent_toolbar_clear.capacity()
                    + value.icons.agent_toolbar_config.capacity()
                    + value.icons.agent_toolbar_stop.capacity()
                    + value.icons.agent_toolbar_copy_screen.capacity()
                    + value.icons.agent_toolbar_smart_copy.capacity()
                    + value.icons.agent_toolbar_copy_last_message.capacity()
                    + value.icons.agent_toolbar_effort.capacity()
                    + value.icons.agent_toolbar_model.capacity()
                    + value.icons.agent_toolbar_claude_haiku.capacity()
                    + value.icons.agent_toolbar_claude_sonnet.capacity()
                    + value.icons.agent_toolbar_claude_opus.capacity()
                    + value.icons.agent_toolbar_claude_fable.capacity()
                    + value.icons.agent_toolbar_exit.capacity()
                    + value.icons.agent_toolbar_fast.capacity()
                    + value.icons.agent_toolbar_selection.capacity()
                    + value.icons.task_progress_frames.capacity() * std::mem::size_of::<String>()
                    + value
                        .icons
                        .task_progress_frames
                        .iter()
                        .map(String::capacity)
                        .sum::<usize>()
            }
            Keymap(_, bindings) => {
                bindings.capacity() * std::mem::size_of::<crate::keymap::KeyBinding>()
            }
            Animation(value) => {
                serialized_bound(value)?
                    + std::mem::size_of::<crate::background_animation::AnimationSettings>()
            }
            Terminal(_) | Editor(_) | Session { .. } | Keyboard(_) | Kanban(_) | Separators(_) => 0,
            Git(value) => serialized_bound(value)?,
            ResetPlanning(value) => serialized_bound(value)?,
            Cost(value) => serialized_bound(value)?,
            Onboarding(value) => serialized_bound(value)?,
            Sound(value) => serialized_bound(value)?,
            Notifications(value) => serialized_bound(value)?,
            Inference(value) => serialized_bound(value)?,
            Triggers(value) => serialized_bound(value)?,
            Voice(value) => serialized_bound(value)?,
            Debug(value) => serialized_bound(value)?,
            Api(value) => serialized_bound(value)?,
            AgentSetup { previous, desired } => {
                serialized_bound(previous)? + serialized_bound(desired)?
            }
            TextTriggers { previous, desired } => {
                serialized_bound(previous)? + serialized_bound(desired)?
            }
            TextTriggerEdit { previous, desired } => {
                serialized_bound(previous)? + serialized_bound(desired)?
            }
        };
        if bytes > 1024 * 1024 {
            return Err("Configuration command exceeds byte limit".into());
        }
        Ok(bytes + std::mem::size_of::<Self>())
    }
}
/// A bounded serialization sink checks commands without allocating an
/// unbounded transport. Deep-cloned String/Vec snapshots have normalized
/// capacities; the factor covers element/tree overhead, and runtime-only
/// inference proxies are explicitly removed before construction.
fn serialized_bound(value: &impl serde::Serialize) -> Result<usize, String> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|n| *n <= 64 * 1024)
                .ok_or_else(|| std::io::Error::other("configuration transport exceeds 64 KiB"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| "Configuration command exceeds retained limit".to_owned())?;
    Ok(counter.0 * 16)
}

impl ConfigurationChange {
    pub fn normalize(self) -> Result<Self, String> {
        use ConfigurationChange::*;
        Ok(match self {
            Git(value) => Git(normalize(&value)?),
            ResetPlanning(value) => ResetPlanning(normalize(&value)?),
            Cost(mut value) => {
                value.prices = value
                    .prices
                    .into_iter()
                    .map(|(key, price)| (key.into_boxed_str().into_string(), price))
                    .collect();
                Cost(value)
            }
            Onboarding(value) => Onboarding(normalize(&value)?),
            Sound(value) => Sound(normalize(&value)?),
            Notifications(value) => Notifications(normalize(&value)?),
            Inference(value) => Inference(Box::new(normalize(&*value)?)),
            Triggers(value) => Triggers(normalize(&value)?),
            Voice(value) => Voice(normalize(&value)?),
            Debug(value) => Debug(normalize(&value)?),
            Api(value) => Api(normalize(&value)?),
            Animation(value) => Animation(normalize(&value)?),
            AgentSetup { previous, desired } => AgentSetup {
                previous: normalize(&previous)?,
                desired: normalize(&desired)?,
            },
            TextTriggers { previous, desired } => TextTriggers {
                previous: normalize(&previous)?,
                desired: normalize(&desired)?,
            },
            TextTriggerEdit { previous, desired } => TextTriggerEdit {
                previous: normalize(&previous)?,
                desired: normalize(&desired)?,
            },
            value => value,
        })
    }
}
fn normalize<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> Result<T, String> {
    serialized_bound(value)?;
    // The preceding sink bounds this allocation and normalized owned fields;
    // serde skipped runtime proxy lists never enter durability commands.
    let bytes = serde_json::to_vec(value)
        .map_err(|_| "Could not encode configuration command".to_owned())?;
    serde_json::from_slice(&bytes)
        .map_err(|_| "Could not normalize configuration command".to_owned())
}

pub struct ProjectRead {
    pub path: PathBuf,
}
impl Job for ProjectRead {
    type Output = crate::background_animation::AnimationSettings;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Err("Project read cancelled".into());
        }
        crate::project_config::load(&self.path)
            .map(|config| config.animation)
            .map_err(|error| error.to_string())
    }
}

/// Serialize only persisted provider configuration from a borrowed source.
/// Runtime paid-proxy catalog records are skipped by serde and never cloned
/// into a pending durability command.
pub(crate) fn inference_snapshot(
    value: &ilium_inference::InferenceSettings,
) -> Result<ConfigurationChange, String> {
    normalize(value).map(|value| ConfigurationChange::Inference(Box::new(value)))
}
