//! Dotted-path adapter over ilium's existing validated settings methods.

use ilium_inference::{InferenceProviderKind, TitleStyle};
use ilium_sound::{NotificationEvent, SoundEvent, SoundSourceKind};
use ilium_voice::{ReasoningEffort, VadEagerness, VoiceInputMode, VoiceModel, VoiceName};
use serde_json::Value;

use crate::app::{AppearanceRow, EditorRow, SessionRow, SoundRow, TerminalRow};
use crate::config::{
    AgentIdentifierMode, GitBranchLine, GitClosePolicy, GitDefaultBase, GitDefaultWhere,
    LeftPanelSizingMode, MotionLevel, NewPaneDirectory, SessionRecoveryPolicy, SidebarDensity,
    TreeOrder, MAX_BOARD_COLUMN_WIDTH, MAX_CARD_PREVIEW_LINES, MIN_BOARD_COLUMN_WIDTH,
    MIN_CARD_PREVIEW_LINES,
};
use crate::icon_settings::IconTarget;
use crate::theme::ColorScheme;
use crate::trigger_settings::{TriggerAction, TriggerEvent};
use crate::App;

use super::command::{SettingsAction, SettingsCommand, StateDetail};
use super::executor::ExecutionReceipt;

pub fn execute(app: &mut App, command: SettingsCommand) -> Result<ExecutionReceipt, String> {
    match command.action {
        SettingsAction::Get => {
            let snapshot =
                super::snapshot::capture(app, StateDetail::Compact, &Default::default())?;
            let data = serde_json::to_value(snapshot).map_err(|error| error.to_string())?;
            Ok(ExecutionReceipt {
                status: "ok",
                message: ilium_prompts::voice::VOICE_SETTINGS_CURRENT_REDACTED_ILIUM_SETTINGS
                    .to_owned(),
                data: data.get("settings").cloned().unwrap_or(Value::Null),
                terminate_session_after_delivery: false,
            })
        }
        SettingsAction::Set => {
            let path = command
                .path
                .as_deref()
                .ok_or(ilium_prompts::voice::VOICE_SETTINGS_PATH_IS_REQUIRED)?;
            let value = command
                .value
                .ok_or(ilium_prompts::voice::VOICE_SETTINGS_VALUE_IS_REQUIRED)?;
            let checkpoint = configuration_checkpoint(app);
            set_setting(app, path, value)?;
            if let Some(receipt) = configuration_receipt(app, checkpoint, path)? {
                return Ok(receipt);
            }
            Ok(ExecutionReceipt::immediate(ilium_prompts::render_value(
                "voice/settings/updated",
                &serde_json::json!({"v0": (path).to_string()}),
            )))
        }
        SettingsAction::Adjust => {
            let path = command
                .path
                .as_deref()
                .ok_or(ilium_prompts::voice::VOICE_SETTINGS_PATH_IS_REQUIRED)?;
            let direction = command
                .direction
                .ok_or(ilium_prompts::voice::VOICE_EXECUTOR_DIRECTION_IS_REQUIRED)?
                .sign();
            let checkpoint = configuration_checkpoint(app);
            adjust_setting(app, path, direction)?;
            if let Some(receipt) = configuration_receipt(app, checkpoint, path)? {
                return Ok(receipt);
            }
            Ok(ExecutionReceipt::immediate(ilium_prompts::render_value(
                "voice/settings/adjusted",
                &serde_json::json!({"v0": (path).to_string()}),
            )))
        }
        SettingsAction::TestInference => {
            app.request_inference_test();
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_SETTINGS_STARTED_THE_INFERENCE_PROVIDER_TEST,
            ))
        }
        SettingsAction::RefreshModels => {
            app.request_model_refresh();
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_SETTINGS_STARTED_MODEL_DISCOVERY,
            ))
        }
        SettingsAction::PreviewSound => {
            if !app.settings_preview_sound() {
                return Err(app
                    .status_message
                    .clone()
                    .unwrap_or_else(|| "Sound preview request was refused".into()));
            }
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_SETTINGS_REQUESTED_A_SOUND_PREVIEW,
            ))
        }
    }
}

fn configuration_checkpoint(app: &mut App) -> (u64, u64) {
    app.configuration_admission.rejection = None;
    (
        app.configuration_admission.attempts,
        app.configuration_admission.accepted,
    )
}
fn configuration_receipt(
    app: &App,
    before: (u64, u64),
    path: &str,
) -> Result<Option<ExecutionReceipt>, String> {
    if app.configuration_admission.attempts == before.0 {
        return Ok(None);
    }
    if let Some(error) = &app.configuration_admission.rejection {
        return Err(format!("Settings change has unaccepted writes: {error}; any earlier admitted writes remain pending"));
    }
    Ok(Some(ExecutionReceipt::local_write_pending(
        "Settings",
        serde_json::json!({"path":path,"accepted_writes":app.configuration_admission.accepted.saturating_sub(before.1)}),
        app.configuration_admission.accepted,
    )))
}

fn set_setting(app: &mut App, path: &str, value: Value) -> Result<(), String> {
    match path {
        "ui.left_panel_sizing_mode" => {
            app.settings_set_left_panel_sizing_mode(parse_left_panel_sizing_mode(string(&value)?)?);
        }
        "ui.left_panel_fixed_width" => {
            let target = left_panel_width(&value)?;
            app.settings_set_fixed_panel_width(target);
        }
        "ui.left_panel_unfocused_width" => {
            let target = left_panel_width(&value)?;
            if target > app.ui_settings.left_panel_sizing.focused_width {
                return Err(ilium_prompts::voice::VOICE_SETTINGS_UNFOCUSED_WIDTH_CANNOT_EXCEED_FOCUSED_WIDTH.to_owned());
            }
            app.settings_set_unfocused_panel_width(target);
        }
        "ui.left_panel_focused_width" => {
            let target = left_panel_width(&value)?;
            if target < app.ui_settings.left_panel_sizing.unfocused_width {
                return Err(ilium_prompts::voice::VOICE_SETTINGS_FOCUSED_WIDTH_CANNOT_BE_SMALLER_THAN_UNFOCUSED.to_owned());
            }
            app.settings_set_focused_panel_width(target);
        }
        "ui.left_panel_minimum_terminal_width" => {
            let target = u16::try_from(unsigned(&value)?).map_err(|_| {
                ilium_prompts::voice::VOICE_SETTINGS_MINIMUM_TERMINAL_WIDTH_IS_OUTSIDE_ILIUM_S
            })?;
            if !(crate::layout::MINIMUM_TERMINAL_WIDTH..=crate::layout::MAXIMUM_TERMINAL_WIDTH)
                .contains(&target)
            {
                return Err(
                    ilium_prompts::voice::VOICE_SETTINGS_MINIMUM_TERMINAL_WIDTH_IS_OUTSIDE_ILIUM_S
                        .to_owned(),
                );
            }
            app.settings_set_minimum_terminal_width(target);
        }
        "ui.tree_order" => {
            app.settings_set_tree_order(parse_tree_order(string(&value)?)?);
        }
        "ui.tree_row_management_controls" => {
            if app.ui_settings.show_tree_row_management_controls != boolean(&value)? {
                app.settings_toggle_tree_row_management_controls();
            }
        }
        "ui.agent_identifier_mode" => {
            let target = parse_agent_identifier_mode(string(&value)?)?;
            for _ in 0..4 {
                if app.ui_settings.agent_identifiers.mode == target {
                    break;
                }
                app.settings_adjust_agent_identifier_mode(1);
            }
            ensure_reached(app.ui_settings.agent_identifiers.mode == target)?;
        }
        "ui.color_scheme" => {
            let target = match normalized(string(&value)?).as_str() {
                "dark" => ColorScheme::Dark,
                "light" => ColorScheme::Light,
                _ => {
                    return Err(
                        ilium_prompts::voice::VOICE_SETTINGS_COLOR_SCHEME_MUST_BE_DARK_OR_LIGHT
                            .to_owned(),
                    )
                }
            };
            if app.ui_settings.color_scheme != target {
                app.settings_toggle_color_scheme();
            }
        }
        "ui.motion_level" => {
            let target = parse_motion_level(string(&value)?)?;
            for _ in 0..3 {
                if app.ui_settings.motion_level == target {
                    break;
                }
                app.settings_adjust_row(AppearanceRow::MotionLevel, 1);
            }
            ensure_reached(app.ui_settings.motion_level == target)?;
        }
        "ui.sidebar_density" => {
            let target = parse_sidebar_density(string(&value)?)?;
            for _ in 0..3 {
                if app.ui_settings.sidebar_density == target {
                    break;
                }
                app.settings_adjust_row(AppearanceRow::SidebarDensity, 1);
            }
            ensure_reached(app.ui_settings.sidebar_density == target)?;
        }
        "ui.task_progress_style" => {
            let name = normalized(string(&value)?);
            let target = crate::icon_settings::TASK_PROGRESS_STYLE_NAMES
                .iter()
                .position(|candidate| normalized(candidate) == name)
                .ok_or(
                    ilium_prompts::voice::VOICE_SETTINGS_TASK_PROGRESS_STYLE_MUST_BE_BRAILLE_BLOCKS,
                )?;
            for _ in 0..=crate::icon_settings::TASK_PROGRESS_STYLE_NAMES.len() {
                if crate::icon_settings::task_progress_preset_index(
                    &app.ui_settings.icons.task_progress_frames,
                ) == Some(target)
                {
                    break;
                }
                app.settings_adjust_row(AppearanceRow::ProgressFillStyle, 1);
            }
            ensure_reached(
                crate::icon_settings::task_progress_preset_index(
                    &app.ui_settings.icons.task_progress_frames,
                ) == Some(target),
            )?;
        }
        "ui.stable_glyphs" => {
            if app.ui_settings.use_stable_glyphs != boolean(&value)? {
                app.settings_toggle_stable_glyphs();
            }
        }
        "ui.agent_debug_menu_enabled" => {
            if app.ui_settings.agent_debug_menu_enabled != boolean(&value)? {
                app.settings_toggle_agent_debug_menu();
            }
        }
        path if path.starts_with("ui.icons.") => {
            // The guard above already proved the prefix is present, so this can never miss.
            let key = path
                .strip_prefix("ui.icons.")
                .expect("guarded by starts_with(\"ui.icons.\") above");
            let target = IconTarget::from_key(key).ok_or_else(|| {
                ilium_prompts::render_value(
                    "voice/settings/unknown-configurable-icon",
                    &serde_json::json!({"v0": format!("{:?}", key)}),
                )
            })?;
            app.settings_set_icon(target, string(&value)?.to_owned());
        }
        "terminal.scrollback_budget_mib" => {
            let target = u16::try_from(unsigned(&value)?).map_err(|_| {
                ilium_prompts::voice::VOICE_SETTINGS_SCROLLBACK_BUDGET_IS_TOO_LARGE.to_owned()
            })?;
            if !(crate::config::TerminalSettings::MIN_SCROLLBACK_BUDGET_MIB
                ..=crate::config::TerminalSettings::MAX_SCROLLBACK_BUDGET_MIB)
                .contains(&target)
                || target % 4 != 0
            {
                return Err(
                    ilium_prompts::voice::VOICE_SETTINGS_SCROLLBACK_BUDGET_MUST_BE_4_512_MIB
                        .to_owned(),
                );
            }
            // Stepping moves the value by a fixed 4 MiB per adjustment (see
            // `stepped_scrollback_budget_mib`), but a value loaded from a hand-edited
            // config.toml is only range-checked at load time, not 4-MiB-aligned -- if its
            // residue mod 4 differs from `target`'s (always 0 here), repeatedly stepping by
            // 4 never lands on `target` and the old unconditional `while` loop spun forever
            // oscillating between two values. Bound the loop to the most steps a full sweep
            // of the allowed range could ever need, and fail loudly instead of hanging.
            let max_steps = usize::from(
                (crate::config::TerminalSettings::MAX_SCROLLBACK_BUDGET_MIB
                    - crate::config::TerminalSettings::MIN_SCROLLBACK_BUDGET_MIB)
                    / 4
                    + 1,
            );
            for _ in 0..max_steps {
                if app.terminal_settings.scrollback_budget_mib == target {
                    break;
                }
                let direction = if app.terminal_settings.scrollback_budget_mib < target {
                    1
                } else {
                    -1
                };
                app.settings_adjust_terminal_row(TerminalRow::ScrollbackBudget, direction);
            }
            ensure_reached(app.terminal_settings.scrollback_budget_mib == target)?;
        }
        "terminal.engine_memory_budget_mib" => {
            let target = u32::try_from(unsigned(&value)?)
                .map_err(|_| "Engine memory budget is too large".to_owned())?;
            if !(crate::config::TerminalSettings::MIN_ENGINE_MEMORY_BUDGET_MIB
                ..=crate::config::TerminalSettings::MAX_ENGINE_MEMORY_BUDGET_MIB)
                .contains(&target)
                || target % 256 != 0
            {
                return Err("Engine memory budget must be 256-16384 MiB in steps of 256".to_owned());
            }
            // Bounded like the scrollback loop: a hand-edited unaligned value
            // never lands on `target` by fixed steps.
            let max_steps = ((crate::config::TerminalSettings::MAX_ENGINE_MEMORY_BUDGET_MIB
                - crate::config::TerminalSettings::MIN_ENGINE_MEMORY_BUDGET_MIB)
                / 256
                + 1) as usize;
            for _ in 0..max_steps {
                if app.terminal_settings.engine_memory_budget_mib == target {
                    break;
                }
                let direction = if app.terminal_settings.engine_memory_budget_mib < target {
                    1
                } else {
                    -1
                };
                app.settings_adjust_terminal_row(TerminalRow::EngineMemoryBudget, direction);
            }
            ensure_reached(app.terminal_settings.engine_memory_budget_mib == target)?;
        }
        "terminal.new_pane_directory" => {
            let target = parse_new_pane_directory(string(&value)?)?;
            for _ in 0..3 {
                if app.terminal_settings.new_pane_directory == target {
                    break;
                }
                app.settings_adjust_terminal_row(TerminalRow::NewPaneDirectory, 1);
            }
            ensure_reached(app.terminal_settings.new_pane_directory == target)?;
        }
        "terminal.smart_copy_light" => {
            let target = boolean(&value)?;
            if app.terminal_settings.smart_copy_light != target {
                app.settings_adjust_terminal_row(TerminalRow::SmartCopyLight, 1);
            }
            ensure_reached(app.terminal_settings.smart_copy_light == target)?;
        }
        "terminal.smart_copy_light_key" => {
            let target = match string(&value)?.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => crate::config::SmartCopyLightKey::Control,
                "alt" | "option" => crate::config::SmartCopyLightKey::Alt,
                "shift" => crate::config::SmartCopyLightKey::Shift,
                other => {
                    return Err(format!(
                        "terminal.smart_copy_light_key {other:?} must be ctrl, alt or shift"
                    ))
                }
            };
            for _ in 0..crate::config::SmartCopyLightKey::ALL.len() {
                if app.terminal_settings.smart_copy_light_key == target {
                    break;
                }
                app.settings_adjust_terminal_row(TerminalRow::SmartCopyLightKey, 1);
            }
            ensure_reached(app.terminal_settings.smart_copy_light_key == target)?;
        }
        "editor.line_numbers" => set_editor_toggle(
            app,
            EditorRow::LineNumbers,
            app.editor_settings.show_line_numbers,
            boolean(&value)?,
        ),
        "editor.minimap" => set_editor_toggle(
            app,
            EditorRow::Minimap,
            app.editor_settings.show_minimap,
            boolean(&value)?,
        ),
        "editor.autosave" => set_editor_toggle(
            app,
            EditorRow::Autosave,
            app.editor_settings.autosave_enabled,
            boolean(&value)?,
        ),
        "editor.markdown_rendered_by_default" => set_editor_toggle(
            app,
            EditorRow::MarkdownDefault,
            app.editor_settings.markdown_rendered_by_default,
            boolean(&value)?,
        ),
        "editor.autosave_delay_ms" => {
            let target = u16::try_from(unsigned(&value)?).map_err(|_| {
                ilium_prompts::voice::VOICE_SETTINGS_AUTOSAVE_DELAY_IS_TOO_LARGE.to_owned()
            })?;
            if ![250, 500, 1000, 2000, 5000].contains(&target) {
                return Err(
                    ilium_prompts::voice::VOICE_SETTINGS_AUTOSAVE_DELAY_MUST_BE_250_500_1000
                        .to_owned(),
                );
            }
            for _ in 0..5 {
                if app.editor_settings.autosave_delay_ms == target {
                    break;
                }
                app.settings_adjust_editor_row(EditorRow::AutosaveDelay, 1);
            }
            ensure_reached(app.editor_settings.autosave_delay_ms == target)?;
        }
        "session.recovery_policy" => {
            let target = parse_recovery_policy(string(&value)?)?;
            for _ in 0..3 {
                if app.session_settings.recovery_policy == target {
                    break;
                }
                app.settings_adjust_session_row(SessionRow::RecoveryPolicy, 1);
            }
            ensure_reached(app.session_settings.recovery_policy == target)?;
        }
        "session.backups_enabled" => {
            let target = boolean(&value)?;
            if app.session_settings.backups_enabled != target {
                app.settings_adjust_session_row(SessionRow::BackupsEnabled, 1);
            }
            ensure_reached(app.session_settings.backups_enabled == target)?;
        }
        "keyboard.shortcut_base" => {
            let shortcut = crate::keymap::ShortcutBase::parse(string(&value)?).ok_or(
                ilium_prompts::voice::VOICE_SETTINGS_SHORTCUT_BASE_MUST_BE_ONE_ASCII_LETTER,
            )?;
            app.settings_set_shortcut_base(shortcut);
        }
        "keyboard.preset" => {
            let preset =
                match normalized(string(&value)?).as_str() {
                    "screen" | "gnu_screen" => crate::keymap::KeymapPreset::Screen,
                    "tmux" => crate::keymap::KeymapPreset::Tmux,
                    _ => return Err(
                        ilium_prompts::voice::VOICE_SETTINGS_KEYBOARD_PRESET_MUST_BE_SCREEN_OR_TMUX
                            .to_owned(),
                    ),
                };
            app.settings_apply_keymap_preset(preset);
        }
        path if path.starts_with("keyboard.bindings.") => {
            // The guard above already proved the prefix is present, so this can never miss.
            let action_name = path
                .strip_prefix("keyboard.bindings.")
                .expect("guarded by starts_with(\"keyboard.bindings.\") above");
            let action = crate::keymap::action_from_name(action_name).ok_or_else(|| {
                ilium_prompts::render_value(
                    "voice/settings/unknown-keyboard-action",
                    &serde_json::json!({"v0": format!("{:?}", action_name)}),
                )
            })?;
            let key = crate::keymap::BindingKey::parse_config_value(string(&value)?).ok_or(
                ilium_prompts::voice::VOICE_SETTINGS_KEYBOARD_BINDING_MUST_BE_ONE_PRINTABLE_KEY,
            )?;
            app.settings_assign_key(action, key);
            let reached = app
                .keybindings
                .iter()
                .any(|binding| binding.action == action && binding.key == key);
            if !reached {
                return Err(app.status_message.clone().unwrap_or_else(|| {
                    ilium_prompts::voice::VOICE_SETTINGS_KEYBOARD_BINDING_WAS_REJECTED.to_owned()
                }));
            }
        }
        "kanban_board.card_preview_lines" => {
            let target = u16::try_from(unsigned(&value)?).map_err(|_| {
                ilium_prompts::voice::VOICE_SETTINGS_CARD_PREVIEW_LINE_COUNT_IS_TOO_LARGE.to_owned()
            })?;
            if !(MIN_CARD_PREVIEW_LINES..=MAX_CARD_PREVIEW_LINES).contains(&target) {
                return Err(
                    ilium_prompts::voice::VOICE_SETTINGS_CARD_PREVIEW_LINES_MUST_BE_BETWEEN_1
                        .to_owned(),
                );
            }
            while app.kanban_board_settings.card_preview_lines != target {
                let direction = if app.kanban_board_settings.card_preview_lines < target {
                    1
                } else {
                    -1
                };
                app.settings_adjust_card_preview_lines(direction);
            }
        }
        "kanban_board.minimum_column_width" => {
            let target = u16::try_from(unsigned(&value)?).map_err(|_| {
                ilium_prompts::voice::VOICE_SETTINGS_COLUMN_WIDTH_IS_TOO_LARGE.to_owned()
            })?;
            if !(MIN_BOARD_COLUMN_WIDTH..=MAX_BOARD_COLUMN_WIDTH).contains(&target) {
                return Err(
                    ilium_prompts::voice::VOICE_SETTINGS_MINIMUM_COLUMN_WIDTH_MUST_BE_BETWEEN_10
                        .to_owned(),
                );
            }
            while app.kanban_board_settings.minimum_column_width != target {
                let direction = if app.kanban_board_settings.minimum_column_width < target {
                    1
                } else {
                    -1
                };
                app.settings_adjust_board_column_width(direction);
            }
        }
        "sound.source" => {
            let target =
                match normalized(string(&value)?).as_str() {
                    "system_beep" | "beep" => SoundSourceKind::SystemBeep,
                    "sound_file" | "file" => SoundSourceKind::SoundFile,
                    "bundled_chirping" | "bundled" => SoundSourceKind::BundledChirping,
                    "generated" | "custom" => SoundSourceKind::Generated,
                    "muted" | "off" => SoundSourceKind::Muted,
                    _ => return Err(
                        ilium_prompts::voice::VOICE_SETTINGS_SOUND_SOURCE_MUST_BE_SYSTEM_BEEP_OR
                            .to_owned(),
                    ),
                };
            if app.sound_settings.source != target {
                app.settings_select_sound_source(target);
            }
        }
        "sound.file" => {
            let requested = string(&value)?;
            let index = app
                .sound_discovery
                .sounds
                .iter()
                .position(|sound| sound.path.to_string_lossy() == requested)
                .ok_or_else(|| {
                    ilium_prompts::voice::VOICE_SETTINGS_SOUND_FILE_IS_NOT_IN_ILIUM_S.to_owned()
                })?;
            app.settings_select_sound_file(index);
        }
        "sound.events.agent_finished" => {
            set_sound_event(app, SoundEvent::AgentFinished, boolean(&value)?)
        }
        "sound.events.approval_required" => {
            set_sound_event(app, SoundEvent::ApprovalRequired, boolean(&value)?)
        }
        "sound.events.agent_started" => {
            set_sound_event(app, SoundEvent::AgentStarted, boolean(&value)?)
        }
        "sound.events.waiting_background" => {
            set_sound_event(app, SoundEvent::WaitingBackground, boolean(&value)?)
        }
        "sound.events.task_succeeded" => {
            set_sound_event(app, SoundEvent::TaskSucceeded, boolean(&value)?)
        }
        "sound.events.task_failed" => {
            set_sound_event(app, SoundEvent::TaskFailed, boolean(&value)?)
        }
        "notifications.enabled" => {
            let mut notifications = app.notification_settings;
            notifications.enabled = boolean(&value)?;
            app.apply_and_persist_notification_settings(notifications);
        }
        "notifications.agent_finished" => {
            set_notification_event(app, NotificationEvent::AgentFinished, boolean(&value)?)
        }
        "notifications.approval_required" => {
            set_notification_event(app, NotificationEvent::ApprovalRequired, boolean(&value)?)
        }
        "notifications.task_succeeded" => {
            set_notification_event(app, NotificationEvent::TaskSucceeded, boolean(&value)?)
        }
        "notifications.task_failed" => {
            set_notification_event(app, NotificationEvent::TaskFailed, boolean(&value)?)
        }
        "notifications.suppress_redundant_task_outcomes" => {
            let mut notifications = app.notification_settings;
            notifications.suppress_redundant_task_outcomes = boolean(&value)?;
            app.apply_and_persist_notification_settings(notifications);
        }
        "notifications.task_coalesce_seconds" => {
            let seconds = u32::try_from(unsigned(&value)?)
                .unwrap_or(ilium_sound::NotificationSettings::MAX_TASK_COALESCE_SECONDS);
            let mut notifications = app.notification_settings;
            notifications.task_coalesce_seconds = seconds;
            app.apply_and_persist_notification_settings(notifications.normalized());
        }
        path if path.starts_with("triggers.") => {
            // The guard above already proved the prefix is present, so this can never miss.
            let event_key = path
                .strip_prefix("triggers.")
                .expect("guarded by starts_with(\"triggers.\") above");
            let event = TriggerEvent::from_key(event_key).ok_or_else(|| {
                ilium_prompts::render_value(
                    "voice/settings/unknown-trigger-event",
                    &serde_json::json!({"v0": format!("{:?}", event_key)}),
                )
            })?;
            let values = value
                .as_array()
                .ok_or(ilium_prompts::voice::VOICE_SETTINGS_TRIGGER_ACTIONS_MUST_BE_AN_ARRAY_OF)?;
            let mut actions = Vec::with_capacity(values.len());
            for value in values {
                let action_key = string(value)?;
                let action = TriggerAction::from_key(action_key).ok_or_else(|| {
                    ilium_prompts::render_value(
                        "voice/settings/unknown-trigger-action",
                        &serde_json::json!({"v0": format!("{:?}", action_key)}),
                    )
                })?;
                actions.push(action);
            }
            let mut settings = app.trigger_settings.clone();
            settings.set_actions(event, actions);
            app.apply_and_persist_trigger_settings(settings);
        }
        "inference.provider" => {
            let provider = parse_inference_provider(string(&value)?)?;
            app.settings_select_inference_provider(provider);
        }
        "inference.restructure_prompt_token_limit" => {
            let limit = u32::try_from(unsigned(&value)?)
                .map_err(|_| "Restructure token budget exceeds 4294967295".to_string())?;
            if limit == 0 {
                return Err("Restructure token budget must be greater than zero".into());
            }
            let mut settings = app.inference_settings.clone();
            settings.restructure_prompt_token_limit = limit;
            app.apply_and_persist_inference_settings(settings);
        }
        "inference.title_style" => {
            app.settings_select_title_style(parse_title_style(string(&value)?)?);
        }
        "inference.kilo_gateway.model" => {
            let model = string(&value)?.trim();
            if !app
                .kilo_gateway_models
                .iter()
                .any(|candidate| candidate == model)
            {
                return Err(ilium_prompts::render_value(
                    "voice/settings/kilo-gateway-model-v0-is-not-in",
                    &serde_json::json!({"v0": format!("{:?}", model)}),
                ));
            }
            update_inference(
                app,
                |settings, value| settings.kilo_gateway.model = value,
                model,
            )
        }
        "inference.ollama.url" => update_inference(
            app,
            |settings, value| settings.ollama.base_url = value,
            string(&value)?,
        ),
        "inference.ollama.model" => update_inference(
            app,
            |settings, value| settings.ollama.model = value,
            string(&value)?,
        ),
        "inference.openai.url" => update_inference(
            app,
            |settings, value| settings.openai.base_url = value,
            string(&value)?,
        ),
        "inference.openai.api_key" => update_inference(
            app,
            |settings, value| settings.openai.api_key = value,
            string(&value)?,
        ),
        "inference.openai.model" => update_inference(
            app,
            |settings, value| settings.openai.model = value,
            string(&value)?,
        ),
        "inference.anthropic.url" => update_inference(
            app,
            |settings, value| settings.anthropic.base_url = value,
            string(&value)?,
        ),
        "inference.anthropic.api_key" => update_inference(
            app,
            |settings, value| settings.anthropic.api_key = value,
            string(&value)?,
        ),
        "inference.anthropic.model" => update_inference(
            app,
            |settings, value| settings.anthropic.model = value,
            string(&value)?,
        ),
        "inference.openrouter.api_key" => update_inference(
            app,
            |settings, value| settings.openrouter.api_key = value,
            string(&value)?,
        ),
        "inference.openrouter.model" => update_inference(
            app,
            |settings, value| settings.openrouter.model = value,
            string(&value)?,
        ),
        "voice.enabled" => app.set_voice_control_enabled(boolean(&value)?),
        "voice.api_key" => update_voice(app, |settings| {
            settings.api_key = string(&value)?.trim().to_owned();
            Ok(())
        })?,
        "voice.model" => update_voice(app, |settings| {
            settings.model = parse_voice_model(string(&value)?)?;
            Ok(())
        })?,
        "voice.voice" => update_voice(app, |settings| {
            settings.voice = parse_voice_name(string(&value)?)?;
            Ok(())
        })?,
        "voice.reasoning_effort" => update_voice(app, |settings| {
            settings.reasoning_effort = parse_reasoning_effort(string(&value)?)?;
            Ok(())
        })?,
        "voice.input_mode" => update_voice(app, |settings| {
            settings.input_mode = parse_voice_input_mode(string(&value)?)?;
            Ok(())
        })?,
        "voice.vad_eagerness" => update_voice(app, |settings| {
            settings.vad_eagerness = parse_vad_eagerness(string(&value)?)?;
            Ok(())
        })?,
        "voice.input_device" => update_voice(app, |settings| {
            settings.input_device_name = optional_string(&value)?;
            Ok(())
        })?,
        "voice.output_device" => update_voice(app, |settings| {
            settings.output_device_name = optional_string(&value)?;
            Ok(())
        })?,
        "voice.output_volume_percent" => update_voice(app, |settings| {
            settings.output_volume_percent = u8::try_from(unsigned(&value)?).map_err(|_| {
                ilium_prompts::voice::VOICE_SETTINGS_VOICE_OUTPUT_VOLUME_MUST_BE_BETWEEN_0
                    .to_owned()
            })?;
            if settings.output_volume_percent > 100 {
                return Err(
                    ilium_prompts::voice::VOICE_SETTINGS_VOICE_OUTPUT_VOLUME_MUST_BE_BETWEEN_0
                        .to_owned(),
                );
            }
            Ok(())
        })?,
        "voice.confirm_terminal_submissions" => update_voice(app, |settings| {
            settings.confirm_terminal_submissions = boolean(&value)?;
            Ok(())
        })?,
        "voice.pause_media_while_active" => update_voice(app, |settings| {
            settings.pause_media_while_active = boolean(&value)?;
            Ok(())
        })?,
        "voice.custom_prompt" => update_voice(app, |settings| {
            settings.custom_prompt = string(&value)?.to_owned();
            Ok(())
        })?,
        "reset_planning.monitor_claude" => {
            if app.reset_planning_settings.monitor_claude != boolean(&value)? {
                app.settings_adjust_reset_planning_row(0);
            }
        }
        "reset_planning.monitor_codex" => {
            if app.reset_planning_settings.monitor_codex != boolean(&value)? {
                app.settings_adjust_reset_planning_row(1);
            }
        }
        "reset_planning.time_style" => {
            let target = match normalized(string(&value)?).as_str() {
                "exact" => crate::reset_planning::ResetTimeStyle::Exact,
                "human" => crate::reset_planning::ResetTimeStyle::Human,
                _ => return Err(
                    ilium_prompts::voice::VOICE_SETTINGS_RESET_PLANNING_TIME_STYLE_MUST_BE_EXACT
                        .to_owned(),
                ),
            };
            if app.reset_planning_settings.time_style != target {
                app.settings_adjust_reset_planning_row(2);
            }
        }
        "debug.file_logging_enabled" => {
            if app.debug_settings.file_logging_enabled != boolean(&value)? {
                app.settings_toggle_file_logging();
            }
        }
        path if path.starts_with("git.") => {
            let mut settings = app.git_settings.clone();
            match path {
                "git.default_where" => {
                    settings.default_where = match normalized(string(&value)?).as_str() {
                        "here" => GitDefaultWhere::Here,
                        "new_worktree" => GitDefaultWhere::NewWorktree,
                        "existing_worktree" => GitDefaultWhere::ExistingWorktree,
                        _ => return Err(
                            ilium_prompts::voice::VOICE_SETTINGS_GIT_DEFAULT_WHERE_MUST_BE_HERE_NEW
                                .into(),
                        ),
                    };
                }
                "git.branch_prefix" => settings.branch_prefix = string(&value)?.to_owned(),
                "git.worktree_location_template" => {
                    settings.worktree_location_template = string(&value)?.to_owned();
                }
                "git.default_base" => {
                    settings.default_base = match normalized(string(&value)?).as_str() {
                        "current" => GitDefaultBase::Current,
                        "default_branch" => GitDefaultBase::DefaultBranch,
                        _ => {
                            return Err(ilium_prompts::voice::VOICE_SETTINGS_GIT_DEFAULT_BASE_MUST_BE_CURRENT_OR.into())
                        }
                    };
                }
                "git.branch_line" => {
                    settings.branch_line = match normalized(string(&value)?).as_str() {
                        "worktree_only" => GitBranchLine::WorktreeOnly,
                        "off" => GitBranchLine::Off,
                        _ => return Err(ilium_prompts::voice::VOICE_SETTINGS_GIT_BRANCH_LINE_MUST_BE_WORKTREE_ONLY.into()),
                    };
                }
                "git.setup_command" => settings.setup_command = string(&value)?.to_owned(),
                "git.default_close_policy" => {
                    settings.default_close_policy =
                        match normalized(string(&value)?).as_str() {
                            "keep" => GitClosePolicy::Keep,
                            "offer_removal_when_safe" => GitClosePolicy::OfferRemovalWhenSafe,
                            _ => return Err(
                                ilium_prompts::voice::VOICE_SETTINGS_GIT_DEFAULT_CLOSE_POLICY_MUST_BE_KEEP
                                    .into(),
                            ),
                        };
                }
                _ => {
                    return Err(ilium_prompts::render_value(
                        "voice/settings/unknown-or-read-only-setting-path",
                        &serde_json::json!({"v0": format!("{:?}", path)}),
                    ))
                }
            }
            app.apply_and_persist_git_settings(settings)?;
        }
        _ => {
            return Err(ilium_prompts::render_value(
                "voice/settings/unknown-or-read-only-setting-path",
                &serde_json::json!({"v0": format!("{:?}", path)}),
            ))
        }
    }
    Ok(())
}

fn adjust_setting(app: &mut App, path: &str, direction: i32) -> Result<(), String> {
    match path {
        "ui.left_panel_sizing_mode" => app.settings_adjust_left_panel_sizing_mode(direction),
        "ui.left_panel_fixed_width" => app.settings_adjust_fixed_panel_width(direction),
        "ui.left_panel_unfocused_width" => app.settings_adjust_unfocused_panel_width(direction),
        "ui.left_panel_focused_width" => app.settings_adjust_focused_panel_width(direction),
        "ui.left_panel_minimum_terminal_width" => {
            app.settings_adjust_minimum_terminal_width(direction)
        }
        "ui.tree_order" => app.settings_adjust_tree_order(direction),
        "ui.tree_row_management_controls" => app.settings_toggle_tree_row_management_controls(),
        "ui.agent_identifier_mode" => app.settings_adjust_agent_identifier_mode(direction),
        "ui.motion_level" => app.settings_adjust_row(AppearanceRow::MotionLevel, direction),
        "ui.sidebar_density" => app.settings_adjust_row(AppearanceRow::SidebarDensity, direction),
        "ui.task_progress_style" => {
            app.settings_adjust_row(AppearanceRow::ProgressFillStyle, direction)
        }
        "terminal.scrollback_budget_mib" => {
            app.settings_adjust_terminal_row(TerminalRow::ScrollbackBudget, direction)
        }
        "terminal.engine_memory_budget_mib" => {
            app.settings_adjust_terminal_row(TerminalRow::EngineMemoryBudget, direction)
        }
        "terminal.new_pane_directory" => {
            app.settings_adjust_terminal_row(TerminalRow::NewPaneDirectory, direction)
        }
        "terminal.smart_copy_light" => {
            app.settings_adjust_terminal_row(TerminalRow::SmartCopyLight, direction)
        }
        "terminal.smart_copy_light_key" => {
            app.settings_adjust_terminal_row(TerminalRow::SmartCopyLightKey, direction)
        }
        "editor.autosave_delay_ms" => {
            app.settings_adjust_editor_row(EditorRow::AutosaveDelay, direction)
        }
        "session.recovery_policy" => {
            app.settings_adjust_session_row(SessionRow::RecoveryPolicy, direction)
        }
        "session.backups_enabled" => {
            app.settings_adjust_session_row(SessionRow::BackupsEnabled, direction)
        }
        "keyboard.shortcut_base" => app.settings_adjust_shortcut_base(direction),
        "kanban_board.card_preview_lines" => app.settings_adjust_card_preview_lines(direction),
        "kanban_board.minimum_column_width" => app.settings_adjust_board_column_width(direction),
        "sound.file" => app.settings_adjust_sound_row(SoundRow::File, direction),
        "inference.kilo_gateway.model" => app.settings_adjust_kilo_gateway_model(direction),
        "inference.openai.model" => app.settings_adjust_openai_model(direction),
        "voice.model" => {
            app.settings_adjust_voice_row(crate::voice_settings::VoiceRow::Model, direction)
        }
        "voice.voice" => {
            app.settings_adjust_voice_row(crate::voice_settings::VoiceRow::Voice, direction)
        }
        "voice.reasoning_effort" => app
            .settings_adjust_voice_row(crate::voice_settings::VoiceRow::ReasoningEffort, direction),
        "voice.input_mode" => {
            app.settings_adjust_voice_row(crate::voice_settings::VoiceRow::InputMode, direction)
        }
        "voice.vad_eagerness" => {
            app.settings_adjust_voice_row(crate::voice_settings::VoiceRow::VadEagerness, direction)
        }
        "voice.input_device" => {
            app.settings_adjust_voice_row(crate::voice_settings::VoiceRow::InputDevice, direction)
        }
        "voice.output_device" => {
            app.settings_adjust_voice_row(crate::voice_settings::VoiceRow::OutputDevice, direction)
        }
        "voice.output_volume_percent" => {
            app.settings_adjust_voice_row(crate::voice_settings::VoiceRow::OutputVolume, direction)
        }
        "voice.confirm_terminal_submissions" => app.settings_adjust_voice_row(
            crate::voice_settings::VoiceRow::ConfirmTerminalSubmissions,
            direction,
        ),
        "voice.pause_media_while_active" => app.settings_adjust_voice_row(
            crate::voice_settings::VoiceRow::PauseMediaWhileActive,
            direction,
        ),
        "reset_planning.monitor_claude" => app.settings_adjust_reset_planning_row(0),
        "reset_planning.monitor_codex" => app.settings_adjust_reset_planning_row(1),
        "reset_planning.time_style" => app.settings_adjust_reset_planning_row(2),
        _ => {
            return Err(ilium_prompts::render_value(
                "voice/settings/setting-v0-is-not-adjustable-use-set",
                &serde_json::json!({"v0": format!("{:?}", path)}),
            ))
        }
    }
    Ok(())
}

fn set_editor_toggle(app: &mut App, row: EditorRow, current: bool, target: bool) {
    if current != target {
        app.settings_adjust_editor_row(row, 1);
    }
}

fn set_sound_event(app: &mut App, event: SoundEvent, target: bool) {
    if app.sound_settings.events.is_enabled(event) != target {
        app.settings_toggle_sound_event(event);
    }
}

fn set_notification_event(app: &mut App, event: NotificationEvent, target: bool) {
    if app.notification_settings.event_flag(event) != target {
        let mut notifications = app.notification_settings;
        notifications.toggle(event);
        app.apply_and_persist_notification_settings(notifications);
    }
}

fn update_inference(
    app: &mut App,
    update: impl FnOnce(&mut ilium_inference::InferenceSettings, String),
    value: &str,
) {
    let mut settings = app.inference_settings.clone();
    // The interactive editor (`App::settings_commit_inference_field`) trims every
    // inference field before storing it; do the same here so a control command
    // pasting an API key or URL with stray whitespace cannot persist a value the
    // interactive path would have rejected (a trailing newline in an API key
    // silently breaks provider authentication).
    update(&mut settings, value.trim().to_owned());
    app.apply_and_persist_inference_settings(settings);
}

fn update_voice(
    app: &mut App,
    update: impl FnOnce(&mut crate::config::VoiceSettings) -> Result<(), String>,
) -> Result<(), String> {
    let mut settings = app.voice_settings.clone();
    update(&mut settings)?;
    app.apply_and_persist_voice_settings(settings);
    Ok(())
}

fn ensure_reached(was_reached: bool) -> Result<(), String> {
    was_reached.then_some(()).ok_or_else(|| {
        ilium_prompts::voice::VOICE_SETTINGS_SETTING_REGISTRY_COULD_NOT_REACH_THE_REQUESTED
            .to_owned()
    })
}

fn boolean(value: &Value) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| ilium_prompts::voice::VOICE_SETTINGS_VALUE_MUST_BE_A_BOOLEAN.to_owned())
}

fn unsigned(value: &Value) -> Result<u64, String> {
    value.as_u64().ok_or_else(|| {
        ilium_prompts::voice::VOICE_SETTINGS_VALUE_MUST_BE_A_NON_NEGATIVE_INTEGER.to_owned()
    })
}

fn string(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .ok_or_else(|| ilium_prompts::voice::VOICE_SETTINGS_VALUE_MUST_BE_A_STRING.to_owned())
}

fn optional_string(value: &Value) -> Result<Option<String>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let value = string(value)?.trim();
    Ok((!value.is_empty()).then(|| value.to_owned()))
}

fn normalized(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

fn left_panel_width(value: &Value) -> Result<u16, String> {
    let width = u16::try_from(unsigned(value)?).map_err(|_| {
        ilium_prompts::voice::VOICE_SETTINGS_LEFT_PANEL_WIDTH_IS_OUTSIDE_ILIUM_S.to_owned()
    })?;
    if !(crate::layout::MIN_TREE_WIDTH..=crate::layout::MAX_TREE_WIDTH).contains(&width) {
        return Err(
            ilium_prompts::voice::VOICE_SETTINGS_LEFT_PANEL_WIDTH_IS_OUTSIDE_ILIUM_S.to_owned(),
        );
    }
    Ok(width)
}

fn parse_left_panel_sizing_mode(value: &str) -> Result<LeftPanelSizingMode, String> {
    match normalized(value).as_str() {
        "fixed" => Ok(LeftPanelSizingMode::Fixed),
        "focus_dependent" => Ok(LeftPanelSizingMode::FocusDependent),
        "width_dependent" | "terminal_width_dependent" => {
            Ok(LeftPanelSizingMode::TerminalWidthDependent)
        }
        _ => Err(
            ilium_prompts::voice::VOICE_SETTINGS_LEFT_PANEL_SIZING_MODE_MUST_BE_FIXED.to_owned(),
        ),
    }
}

fn parse_tree_order(value: &str) -> Result<TreeOrder, String> {
    match normalized(value).as_str() {
        "manual" => Ok(TreeOrder::Manual),
        "type" => Ok(TreeOrder::Type),
        "age_ascending" => Ok(TreeOrder::AgeAscending),
        "age_descending" => Ok(TreeOrder::AgeDescending),
        "name_ascending" | "name_a_z" => Ok(TreeOrder::NameAscending),
        "name_descending" | "name_z_a" => Ok(TreeOrder::NameDescending),
        _ => Err(ilium_prompts::voice::VOICE_SETTINGS_INVALID_TREE_ORDER.to_owned()),
    }
}

fn parse_agent_identifier_mode(value: &str) -> Result<AgentIdentifierMode, String> {
    match normalized(value).as_str() {
        "full_name" => Ok(AgentIdentifierMode::FullName),
        "letter" => Ok(AgentIdentifierMode::Letter),
        "icon" => Ok(AgentIdentifierMode::Icon),
        "hidden" => Ok(AgentIdentifierMode::Hidden),
        _ => Err(ilium_prompts::voice::VOICE_SETTINGS_INVALID_AGENT_IDENTIFIER_MODE.to_owned()),
    }
}

fn parse_motion_level(value: &str) -> Result<MotionLevel, String> {
    match normalized(value).as_str() {
        "full" => Ok(MotionLevel::Full),
        "reduced" => Ok(MotionLevel::Reduced),
        "off" => Ok(MotionLevel::Off),
        _ => Err(
            ilium_prompts::voice::VOICE_SETTINGS_MOTION_LEVEL_MUST_BE_FULL_REDUCED_OR.to_owned(),
        ),
    }
}

fn parse_sidebar_density(value: &str) -> Result<SidebarDensity, String> {
    match normalized(value).as_str() {
        "compact" => Ok(SidebarDensity::Compact),
        "standard" => Ok(SidebarDensity::Standard),
        "comfortable" => Ok(SidebarDensity::Comfortable),
        _ => Err(
            ilium_prompts::voice::VOICE_SETTINGS_SIDEBAR_DENSITY_MUST_BE_COMPACT_STANDARD_OR
                .to_owned(),
        ),
    }
}

fn parse_new_pane_directory(value: &str) -> Result<NewPaneDirectory, String> {
    match normalized(value).as_str() {
        "project_root" => Ok(NewPaneDirectory::ProjectRoot),
        "focused_terminal" => Ok(NewPaneDirectory::FocusedTerminal),
        "last_used" => Ok(NewPaneDirectory::LastUsed),
        _ => Err(ilium_prompts::voice::VOICE_SETTINGS_INVALID_NEW_PANE_DIRECTORY_POLICY.to_owned()),
    }
}

fn parse_recovery_policy(value: &str) -> Result<SessionRecoveryPolicy, String> {
    match normalized(value).as_str() {
        "restore_automatically" => Ok(SessionRecoveryPolicy::RestoreAutomatically),
        "ask_before_restore" => Ok(SessionRecoveryPolicy::AskBeforeRestore),
        "start_fresh" => Ok(SessionRecoveryPolicy::StartFresh),
        _ => Err(ilium_prompts::voice::VOICE_SETTINGS_INVALID_SESSION_RECOVERY_POLICY.to_owned()),
    }
}

fn parse_inference_provider(value: &str) -> Result<InferenceProviderKind, String> {
    match normalized(value).as_str() {
        "kilo" | "kilo_gateway" => Ok(InferenceProviderKind::KiloGateway),
        "ollama" => Ok(InferenceProviderKind::Ollama),
        "openai" => Ok(InferenceProviderKind::OpenAi),
        "anthropic" => Ok(InferenceProviderKind::Anthropic),
        "openrouter" => Ok(InferenceProviderKind::OpenRouter),
        _ => Err(ilium_prompts::voice::VOICE_SETTINGS_INVALID_INFERENCE_PROVIDER.to_owned()),
    }
}

fn parse_title_style(value: &str) -> Result<TitleStyle, String> {
    match normalized(value).as_str() {
        "labeling" | "labelling" => Ok(TitleStyle::Labeling),
        "summarization" | "summarisation" => Ok(TitleStyle::Summarization),
        _ => Err(ilium_prompts::voice::VOICE_SETTINGS_INVALID_TITLE_STYLE.to_owned()),
    }
}

fn parse_voice_model(value: &str) -> Result<VoiceModel, String> {
    let value = normalized(value);
    VoiceModel::ALL
        .into_iter()
        .find(|model| normalized(model.api_name()) == value || normalized(model.label()) == value)
        .ok_or_else(|| ilium_prompts::voice::VOICE_SETTINGS_INVALID_REALTIME_VOICE_MODEL.to_owned())
}

fn parse_voice_name(value: &str) -> Result<VoiceName, String> {
    let value = normalized(value);
    VoiceName::ALL
        .into_iter()
        .find(|voice| normalized(voice.api_name()) == value || normalized(voice.label()) == value)
        .ok_or_else(|| ilium_prompts::voice::VOICE_SETTINGS_INVALID_REALTIME_VOICE.to_owned())
}

fn parse_reasoning_effort(value: &str) -> Result<ReasoningEffort, String> {
    let value = normalized(value);
    ReasoningEffort::ALL
        .into_iter()
        .find(|effort| normalized(effort.api_name()) == value)
        .ok_or_else(|| {
            ilium_prompts::voice::VOICE_SETTINGS_REASONING_EFFORT_MUST_BE_MINIMAL_LOW_OR.to_owned()
        })
}

fn parse_voice_input_mode(value: &str) -> Result<VoiceInputMode, String> {
    match normalized(value).as_str() {
        "semantic_vad" | "vad" => Ok(VoiceInputMode::SemanticVad),
        "push_to_talk" | "ptt" => Ok(VoiceInputMode::PushToTalk),
        _ => Err(
            ilium_prompts::voice::VOICE_SETTINGS_VOICE_INPUT_MODE_MUST_BE_SEMANTIC_VAD.to_owned(),
        ),
    }
}

fn parse_vad_eagerness(value: &str) -> Result<VadEagerness, String> {
    let value = normalized(value);
    VadEagerness::ALL
        .into_iter()
        .find(|eagerness| normalized(eagerness.api_name()) == value)
        .ok_or_else(|| {
            ilium_prompts::voice::VOICE_SETTINGS_VAD_EAGERNESS_MUST_BE_AUTO_LOW_MEDIUM.to_owned()
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use std::path::PathBuf;

    use super::*;

    #[test]
    fn preview_control_reports_actual_refusal_instead_of_queued() {
        let mut app = App::new("preview-refusal".into(), PathBuf::from("/tmp/project"));
        app.outbound_admission = None;
        let command = || SettingsCommand {
            action: SettingsAction::PreviewSound,
            path: None,
            value: None,
            direction: None,
        };
        let failure = execute(&mut app, command()).unwrap_err();
        assert!(failure.contains("Request rejected before admission"));
        app.sound_settings.source = SoundSourceKind::Muted;
        assert_eq!(execute(&mut app, command()).unwrap_err(), "Sound is muted");
        assert!(app.take_outbound_requests().is_empty());
    }

    #[test]
    fn setting_values_are_normalized_without_accepting_unknown_paths() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));

        set_setting(&mut app, "ui.motion_level", json!("reduced")).unwrap();
        assert_eq!(app.ui_settings.motion_level, MotionLevel::Reduced);
        set_setting(&mut app, "ui.task_progress_style", json!("moons")).unwrap();
        assert_eq!(
            crate::icon_settings::task_progress_preset_index(
                &app.ui_settings.icons.task_progress_frames
            ),
            Some(2)
        );
        adjust_setting(&mut app, "ui.task_progress_style", 1).unwrap();
        assert_eq!(
            crate::icon_settings::task_progress_preset_index(
                &app.ui_settings.icons.task_progress_frames
            ),
            Some(3)
        );
        let snapshot =
            super::super::snapshot::capture(&app, StateDetail::Compact, &Default::default())
                .unwrap();
        assert_eq!(snapshot.settings["ui"]["task_progress_style"], "Quarters");
        assert_eq!(snapshot.settings["ui"]["task_progress_frames"][0], "○");
        assert!(snapshot.settings["writable_path_patterns"]
            .as_array()
            .unwrap()
            .contains(&json!("sound.events.task_succeeded")));
        assert!(set_setting(&mut app, "ui.task_progress_style", json!("unknown")).is_err());
        set_setting(&mut app, "voice.confirm_terminal_submissions", json!(true)).unwrap();
        assert!(app.voice_settings.confirm_terminal_submissions);
        set_setting(
            &mut app,
            "triggers.agent_finished_work",
            json!(["restructure_all_projects", "retitle_element"]),
        )
        .unwrap();
        assert_eq!(
            app.trigger_settings.agent_finished_work,
            vec![
                TriggerAction::RetitleElement,
                TriggerAction::RestructureAllProjects,
            ]
        );
        assert!(
            set_setting(&mut app, "triggers.agent_finished_work", json!(["made_up"]),).is_err()
        );
        assert!(set_setting(&mut app, "ui.made_up", json!(true)).is_err());
        set_setting(&mut app, "inference.title_style", json!("Labelling")).unwrap();
        assert_eq!(app.inference_settings.title_style, TitleStyle::Labeling);
        assert!(set_setting(&mut app, "inference.title_style", json!("outline")).is_err());
        assert_eq!(app.inference_settings.title_style, TitleStyle::Labeling);
    }

    #[test]
    fn restructure_token_setting_validates_and_updates_snapshot() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let path = "inference.restructure_prompt_token_limit";
        set_setting(&mut app, path, json!(345678)).unwrap();
        assert_eq!(
            app.inference_settings.restructure_prompt_token_limit,
            345678
        );
        for invalid in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("123"),
            json!(4294967296_u64),
        ] {
            assert!(set_setting(&mut app, path, invalid).is_err());
            assert_eq!(
                app.inference_settings.restructure_prompt_token_limit,
                345678
            );
        }
        app.settings_commit_inference_field(
            crate::app::InferenceSettingField::RestructurePromptTokenLimit,
            "234567".into(),
        );
        assert_eq!(
            app.inference_settings.restructure_prompt_token_limit,
            234567
        );
        app.settings_commit_inference_field(
            crate::app::InferenceSettingField::RestructurePromptTokenLimit,
            "0".into(),
        );
        assert_eq!(
            app.inference_settings.restructure_prompt_token_limit,
            234567
        );
        for invalid in ["-1", "abc", "1.5", "4294967296", ""] {
            app.settings_commit_inference_field(
                crate::app::InferenceSettingField::RestructurePromptTokenLimit,
                invalid.into(),
            );
            assert_eq!(
                app.inference_settings.restructure_prompt_token_limit,
                234567
            );
            assert!(app
                .status_message
                .as_ref()
                .unwrap()
                .contains("positive whole number"));
        }
        let snapshot =
            super::super::snapshot::capture(&app, StateDetail::Compact, &Default::default())
                .unwrap();
        assert_eq!(
            snapshot.settings["inference"]["restructure_prompt_token_limit"],
            234567
        );
        assert!(snapshot.settings["writable_path_patterns"]
            .as_array()
            .unwrap()
            .contains(&json!(path)));
    }

    #[test]
    fn kilo_model_setting_accepts_only_discovered_free_choices() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        app.kilo_gateway_models
            .push("stepfun/step-3.7-flash:free".to_string());

        set_setting(
            &mut app,
            "inference.kilo_gateway.model",
            json!("stepfun/step-3.7-flash:free"),
        )
        .unwrap();
        assert_eq!(
            app.inference_settings.kilo_gateway.model,
            "stepfun/step-3.7-flash:free"
        );
        assert!(set_setting(
            &mut app,
            "inference.kilo_gateway.model",
            json!("provider/paid")
        )
        .is_err());

        adjust_setting(&mut app, "inference.kilo_gateway.model", 1).unwrap();
        assert_eq!(app.inference_settings.kilo_gateway.model, "kilo-auto/free");
    }

    #[test]
    fn left_panel_widths_reject_invalid_values_without_mutating_state() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let original = app.ui_settings.left_panel_sizing;

        assert!(set_setting(&mut app, "ui.left_panel_fixed_width", json!(5)).is_err());
        assert_eq!(
            app.ui_settings.left_panel_sizing, original,
            "a rejected left-panel width must not have a visible side effect"
        );

        assert!(set_setting(
            &mut app,
            "ui.left_panel_focused_width",
            json!(2_147_483_648_u64)
        )
        .is_err());
        assert_eq!(app.ui_settings.left_panel_sizing, original);

        assert!(set_setting(&mut app, "ui.left_panel_focused_width", json!(20)).is_err());
        assert_eq!(app.ui_settings.left_panel_sizing, original);

        set_setting(&mut app, "ui.left_panel_fixed_width", json!(40)).unwrap();
        assert_eq!(app.ui_settings.left_panel_sizing.fixed_width, 40);
        set_setting(
            &mut app,
            "ui.left_panel_sizing_mode",
            json!("width-dependent"),
        )
        .unwrap();
        assert_eq!(
            app.ui_settings.left_panel_sizing.mode,
            LeftPanelSizingMode::TerminalWidthDependent
        );
    }

    #[test]
    fn scrollback_budget_rejects_rather_than_hangs_when_misaligned() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));

        // Config loading only range-checks `scrollback_budget_mib`, not 4 MiB
        // alignment, so a hand-edited config.toml can leave an odd starting value.
        // Stepping by a fixed 4 MiB per adjustment can then never land exactly on a
        // 4-MiB-aligned target; this must return an error promptly instead of
        // spinning forever oscillating between two values.
        app.terminal_settings.scrollback_budget_mib = 5;
        assert!(set_setting(&mut app, "terminal.scrollback_budget_mib", json!(8)).is_err());

        // A reachable, aligned target still works normally.
        app.terminal_settings.scrollback_budget_mib = 32;
        set_setting(&mut app, "terminal.scrollback_budget_mib", json!(64)).unwrap();
        assert_eq!(app.terminal_settings.scrollback_budget_mib, 64);
    }

    #[test]
    fn reset_planning_control_paths_toggle_independently() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        set_setting(&mut app, "reset_planning.monitor_claude", json!(false)).unwrap();
        set_setting(&mut app, "reset_planning.time_style", json!("human")).unwrap();
        assert!(!app.reset_planning_settings.monitor_claude);
        assert!(app.reset_planning_settings.monitor_codex);
        assert_eq!(
            app.reset_planning_settings.time_style,
            crate::reset_planning::ResetTimeStyle::Human
        );
        assert!(set_setting(&mut app, "reset_planning.time_style", json!("soon")).is_err());
        adjust_setting(&mut app, "reset_planning.monitor_codex", 1).unwrap();
        assert!(!app.reset_planning_settings.monitor_codex);
    }
}
