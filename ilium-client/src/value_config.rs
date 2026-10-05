//! Fresh configuration-domain validation for direct numeric controls.
//! Domain values are returned or assigned only after exact range validation;
//! the settings host retains runtime effects, persistence, and error display.
use crate::config::{
    self, EditorSettings, KanbanBoardSettings, LeftPanelSizingSettings, TerminalSettings,
};
use crate::layout::{
    MAXIMUM_TERMINAL_WIDTH, MAX_TREE_WIDTH, MINIMUM_TERMINAL_WIDTH, MIN_TREE_WIDTH,
};
use crate::value_number::{NumberSpec, NumberValue};

fn whole_number(spec: NumberSpec, text: &str) -> Result<u64, String> {
    let NumberValue::Integer(value) = spec.parse(text)? else {
        return Err("Enter a whole number".into());
    };
    u64::try_from(value).map_err(|_| "Number exceeds this setting's storage range".into())
}
fn small_number(spec: NumberSpec, text: &str) -> Result<u16, String> {
    u16::try_from(whole_number(spec, text)?)
        .map_err(|_| "Number exceeds this setting's storage range".into())
}
fn integer_spec(minimum: u16, maximum: u16) -> NumberSpec {
    NumberSpec::Integer {
        minimum: i128::from(minimum),
        maximum: i128::from(maximum),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelNumber {
    FixedWidth,
    UnfocusedWidth,
    FocusedWidth,
    MinimumTerminalWidth,
}
impl PanelNumber {
    pub const ALL: [Self; 4] = [
        Self::FixedWidth,
        Self::UnfocusedWidth,
        Self::FocusedWidth,
        Self::MinimumTerminalWidth,
    ];
    pub fn spec(self, sizing: &LeftPanelSizingSettings) -> NumberSpec {
        match self {
            Self::FixedWidth => integer_spec(MIN_TREE_WIDTH, MAX_TREE_WIDTH),
            Self::UnfocusedWidth => {
                integer_spec(MIN_TREE_WIDTH, sizing.focused_width.min(MAX_TREE_WIDTH))
            }
            Self::FocusedWidth => {
                integer_spec(sizing.unfocused_width.max(MIN_TREE_WIDTH), MAX_TREE_WIDTH)
            }
            Self::MinimumTerminalWidth => {
                integer_spec(MINIMUM_TERMINAL_WIDTH, MAXIMUM_TERMINAL_WIDTH)
            }
        }
    }
    pub fn value(self, sizing: &LeftPanelSizingSettings) -> u16 {
        match self {
            Self::FixedWidth => sizing.fixed_width,
            Self::UnfocusedWidth => sizing.unfocused_width,
            Self::FocusedWidth => sizing.focused_width,
            Self::MinimumTerminalWidth => sizing.minimum_terminal_width,
        }
    }
    pub fn set(self, sizing: &mut LeftPanelSizingSettings, text: &str) -> Result<(), String> {
        let value = small_number(self.spec(sizing), text)?;
        match self {
            Self::FixedWidth => sizing.fixed_width = value,
            Self::UnfocusedWidth => sizing.unfocused_width = value,
            Self::FocusedWidth => sizing.focused_width = value,
            Self::MinimumTerminalWidth => sizing.minimum_terminal_width = value,
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardNumber {
    CardPreviewLines,
    MinimumColumnWidth,
}
impl BoardNumber {
    pub const ALL: [Self; 2] = [Self::CardPreviewLines, Self::MinimumColumnWidth];
    pub fn spec(self) -> NumberSpec {
        match self {
            Self::CardPreviewLines => integer_spec(
                config::MIN_CARD_PREVIEW_LINES,
                config::MAX_CARD_PREVIEW_LINES,
            ),
            Self::MinimumColumnWidth => integer_spec(
                config::MIN_BOARD_COLUMN_WIDTH,
                config::MAX_BOARD_COLUMN_WIDTH,
            ),
        }
    }
    pub fn value(self, settings: &KanbanBoardSettings) -> u16 {
        match self {
            Self::CardPreviewLines => settings.card_preview_lines,
            Self::MinimumColumnWidth => settings.minimum_column_width,
        }
    }
    pub fn set(self, settings: &mut KanbanBoardSettings, text: &str) -> Result<(), String> {
        let value = small_number(self.spec(), text)?;
        match self {
            Self::CardPreviewLines => settings.card_preview_lines = value,
            Self::MinimumColumnWidth => settings.minimum_column_width = value,
        }
        Ok(())
    }
}

pub fn terminal_scrollback_spec() -> NumberSpec {
    integer_spec(
        TerminalSettings::MIN_SCROLLBACK_BUDGET_MIB,
        TerminalSettings::MAX_SCROLLBACK_BUDGET_MIB,
    )
}
pub fn set_terminal_scrollback(settings: &mut TerminalSettings, text: &str) -> Result<(), String> {
    settings.scrollback_budget_mib = small_number(terminal_scrollback_spec(), text)?;
    Ok(())
}
pub fn terminal_engine_memory_spec() -> NumberSpec {
    NumberSpec::Integer {
        minimum: i128::from(TerminalSettings::MIN_ENGINE_MEMORY_BUDGET_MIB),
        maximum: i128::from(TerminalSettings::MAX_ENGINE_MEMORY_BUDGET_MIB),
    }
}
pub fn set_terminal_engine_memory(
    settings: &mut TerminalSettings,
    text: &str,
) -> Result<(), String> {
    settings.engine_memory_budget_mib =
        u32::try_from(whole_number(terminal_engine_memory_spec(), text)?)
            .map_err(|_| "Number exceeds this setting's storage range".to_owned())?;
    Ok(())
}
pub fn editor_autosave_spec() -> NumberSpec {
    integer_spec(250, 5000)
}
pub fn set_editor_autosave_delay(settings: &mut EditorSettings, text: &str) -> Result<(), String> {
    settings.autosave_delay_ms = small_number(editor_autosave_spec(), text)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarNumber {
    VoiceVolume,
    LastPromptLines,
    ProgressLines,
    CompletedProgressHideAfter,
    ApiPort,
    InferenceTokenBudget,
    WorkingPollSeconds,
    IdlePollSeconds,
    NotificationCoalesce,
}
impl ScalarNumber {
    pub const ALL: [Self; 9] = [
        Self::VoiceVolume,
        Self::LastPromptLines,
        Self::ProgressLines,
        Self::CompletedProgressHideAfter,
        Self::ApiPort,
        Self::InferenceTokenBudget,
        Self::WorkingPollSeconds,
        Self::IdlePollSeconds,
        Self::NotificationCoalesce,
    ];
    pub fn spec(self) -> NumberSpec {
        let (minimum, maximum) = match self {
            Self::VoiceVolume => (0, 100),
            Self::LastPromptLines => (
                i128::from(config::MIN_LAST_PROMPT_MAX_LINES),
                i128::from(config::MAX_LAST_PROMPT_MAX_LINES),
            ),
            Self::ProgressLines => (
                i128::from(config::MIN_PROGRESS_MAX_LINES),
                i128::from(config::MAX_PROGRESS_MAX_LINES),
            ),
            Self::CompletedProgressHideAfter => (0, i128::from(u32::MAX)),
            Self::ApiPort => (1, i128::from(u16::MAX)),
            Self::InferenceTokenBudget => (1, i128::from(u32::MAX)),
            Self::WorkingPollSeconds | Self::IdlePollSeconds => (0, i128::from(i64::MAX)),
            Self::NotificationCoalesce => (
                0,
                i128::from(ilium_sound::NotificationSettings::MAX_TASK_COALESCE_SECONDS),
            ),
        };
        NumberSpec::Integer { minimum, maximum }
    }
    pub fn parse(self, text: &str) -> Result<u64, String> {
        let value = whole_number(self.spec(), text)?;
        if matches!(self, Self::WorkingPollSeconds | Self::IdlePollSeconds)
            && std::time::Instant::now()
                .checked_add(std::time::Duration::from_secs(value))
                .is_none()
        {
            return Err("Poll interval is too large for the platform timer".into());
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_direct_entry_uses_fresh_related_widths_and_keeps_other_policies() {
        let mut sizing = LeftPanelSizingSettings::default();
        let original = sizing;
        PanelNumber::FixedWidth.set(&mut sizing, "37").unwrap();
        assert_eq!(sizing.fixed_width, 37);
        assert_eq!(sizing.mode, original.mode);
        PanelNumber::UnfocusedWidth.set(&mut sizing, "31").unwrap();
        assert!(PanelNumber::FocusedWidth.set(&mut sizing, "30").is_err());
        PanelNumber::FocusedWidth.set(&mut sizing, "40").unwrap();
        assert!(PanelNumber::UnfocusedWidth.set(&mut sizing, "41").is_err());
        PanelNumber::MinimumTerminalWidth
            .set(&mut sizing, "147")
            .unwrap();
        assert_eq!(sizing.minimum_terminal_width, 147);
        assert_eq!(sizing.unfocused_width, 31);
        assert_eq!(sizing.focused_width, 40);
    }
    #[test]
    fn invalid_panel_entry_preserves_every_field() {
        for field in PanelNumber::ALL {
            for text in [
                "",
                "abc",
                "17.5",
                "-1",
                "65536",
                "100000000000000000000000000000000000000",
            ] {
                let mut sizing = LeftPanelSizingSettings::default();
                let before = sizing;
                assert!(field.set(&mut sizing, text).is_err());
                assert_eq!(sizing, before);
            }
        }
    }
    #[test]
    fn numeric_configuration_preserves_intermediates_and_rejects_clamping() {
        let mut terminal = TerminalSettings::default();
        set_terminal_scrollback(&mut terminal, "17").unwrap();
        assert_eq!(terminal.scrollback_budget_mib, 17);
        assert!(set_terminal_scrollback(&mut terminal, "513").is_err());
        assert_eq!(terminal.scrollback_budget_mib, 17);
        let mut editor = EditorSettings::default();
        set_editor_autosave_delay(&mut editor, "1101").unwrap();
        assert_eq!(editor.autosave_delay_ms, 1101);
        assert!(set_editor_autosave_delay(&mut editor, "249").is_err());
        assert_eq!(editor.autosave_delay_ms, 1101);
        let mut board = KanbanBoardSettings::default();
        BoardNumber::CardPreviewLines.set(&mut board, "7").unwrap();
        BoardNumber::MinimumColumnWidth
            .set(&mut board, "37")
            .unwrap();
        assert_eq!(board.card_preview_lines, 7);
        assert_eq!(board.minimum_column_width, 37);
        assert!(BoardNumber::CardPreviewLines.set(&mut board, "11").is_err());
        assert!(BoardNumber::MinimumColumnWidth
            .set(&mut board, "9")
            .is_err());
    }
    #[test]
    fn scalar_inventory_accepts_own_limits_and_rejects_outside_values() {
        for field in ScalarNumber::ALL {
            let NumberSpec::Integer { minimum, maximum } = field.spec() else {
                panic!("integer field");
            };
            assert_eq!(
                field.parse(&minimum.to_string()).unwrap(),
                u64::try_from(minimum).unwrap()
            );
            assert!(field.spec().parse(&maximum.to_string()).is_ok());
            if !matches!(
                field,
                ScalarNumber::WorkingPollSeconds | ScalarNumber::IdlePollSeconds
            ) {
                assert_eq!(
                    field.parse(&maximum.to_string()).unwrap(),
                    u64::try_from(maximum).unwrap()
                );
            }
            assert!(field.parse(&(minimum - 1).to_string()).is_err());
            assert!(field.parse(&(maximum + 1).to_string()).is_err());
            for invalid in ["NaN", "1.1", "", "Infinity", "1e3"] {
                assert!(field.parse(invalid).is_err());
            }
        }
        assert_eq!(ScalarNumber::ApiPort.parse("8873").unwrap(), 8873);
        assert_eq!(
            ScalarNumber::CompletedProgressHideAfter
                .parse("17")
                .unwrap(),
            17
        );
        assert_eq!(ScalarNumber::NotificationCoalesce.parse("17").unwrap(), 17);
    }
}
