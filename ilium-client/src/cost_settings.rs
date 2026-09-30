//! User settings for the agent-spend indicators (`[cost]` in `config.toml`).
//!
//! The model is deliberately data-only: every display option is an
//! independent [`DisplayOption`] (enabled, plus *when* it is visible), and the
//! settings tab walks [`CostRow::rows`] so keyboard, mouse and rendering can
//! never disagree about which rows exist. All value changes go through
//! [`CostSettings::adjust`], a pure function the tests exercise directly.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cost_model::{Calibration, ModelPrice, MAX_WINDOW_MINUTES};

/// When a display option is drawn for a tree entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostVisibility {
    /// On every entry, all the time.
    Always,
    /// Only while the pointer is over that entry.
    #[default]
    Hover,
}

impl CostVisibility {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Always => "Always visible",
            Self::Hover => "Only when hovering the entry",
        }
    }

    pub const fn toggled(self) -> Self {
        match self {
            Self::Always => Self::Hover,
            Self::Hover => Self::Always,
        }
    }
}

/// One selectable indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostDisplay {
    /// One coloured height glyph `▂`.
    LevelGlyph,
    /// Height glyph plus the dollar figure `▂ $1.30`.
    LevelDollars,
    /// Five-cell fill meter `▰▰▱▱▱`.
    Meter,
    /// Spend-over-time sparkline `▁▂▅▇▃`.
    Sparkline,
    /// Project and group rows show the sum of their agents.
    GroupTotals,
    /// A card beside the tree with the agent's full cost breakdown.
    DetailCard,
    /// One total line above the tree.
    HeaderTotal,
    /// `↑` when the agent is spending much faster than usual.
    BurnMarker,
}

impl CostDisplay {
    pub const ALL: [Self; 8] = [
        Self::LevelGlyph,
        Self::LevelDollars,
        Self::Meter,
        Self::Sparkline,
        Self::GroupTotals,
        Self::DetailCard,
        Self::HeaderTotal,
        Self::BurnMarker,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::LevelGlyph => "Level glyph",
            Self::LevelDollars => "Level glyph and dollars",
            Self::Meter => "Five-cell meter",
            Self::Sparkline => "Burn sparkline",
            Self::GroupTotals => "Group and project totals",
            Self::DetailCard => "Detail card",
            Self::HeaderTotal => "Total line above the tree",
            Self::BurnMarker => "Burn spike marker",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::LevelGlyph => {
                "One height glyph per agent, coloured green to red. Quietest option; no numbers."
            }
            Self::LevelDollars => {
                "The level glyph followed by the estimated dollars spent. Exact number next to the heat."
            }
            Self::Meter => {
                "Five cells that fill as the agent gets more expensive. Easiest to read at a glance."
            }
            Self::Sparkline => {
                "Spend over the configured window, one bar per time slice. Shows trend, not just total."
            }
            Self::GroupTotals => {
                "Project and group rows show the sum of everything beneath them, to find which project burns money."
            }
            Self::DetailCard => {
                "A card beside the tree with the agent's dollars, what it was rated against, burn rate, token classes, cost per model and quota. Hover shows it for the hovered agent; always keeps it on the selected one."
            }
            Self::HeaderTotal => {
                "A single line above the tree with the total estimated spend of every agent."
            }
            Self::BurnMarker => {
                "An up arrow beside the indicator while spending runs at least twice the recent average."
            }
        }
    }

    /// Stable ID of this option's help topic (see `settings_help`).
    pub const fn help_id(self) -> &'static str {
        match self {
            Self::LevelGlyph => "COST-10",
            Self::LevelDollars => "COST-11",
            Self::Meter => "COST-12",
            Self::Sparkline => "COST-13",
            Self::GroupTotals => "COST-14",
            Self::DetailCard => "COST-15",
            Self::HeaderTotal => "COST-16",
            Self::BurnMarker => "COST-17",
        }
    }
}

/// Enabled flag plus visibility for one [`CostDisplay`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayOption {
    pub enabled: bool,
    pub visibility: CostVisibility,
}

impl Default for DisplayOption {
    fn default() -> Self {
        Self {
            enabled: false,
            visibility: CostVisibility::Hover,
        }
    }
}

impl DisplayOption {
    /// Whether the option draws for an entry that is (`is_hovered`) or is not
    /// under the pointer.
    pub fn is_shown(self, is_hovered: bool) -> bool {
        self.enabled && (self.visibility == CostVisibility::Always || is_hovered)
    }
}

/// Fixed-band presets, each four ascending dollar totals.
pub const FIXED_PRESETS: [[f64; 4]; 5] = [
    [0.25, 1.0, 4.0, 10.0],
    [0.5, 2.0, 8.0, 25.0],
    [1.0, 5.0, 20.0, 50.0],
    [2.0, 10.0, 40.0, 100.0],
    [5.0, 25.0, 100.0, 250.0],
];
/// Burn-rate presets, each four ascending dollars-per-hour cut points.
pub const BURN_PRESETS: [[f64; 4]; 4] = [
    [0.25, 1.0, 3.0, 8.0],
    [0.5, 2.0, 5.0, 15.0],
    [1.0, 4.0, 10.0, 30.0],
    [2.0, 8.0, 20.0, 60.0],
];
pub const BUDGET_STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
pub const HISTORY_DAY_STEPS: [u16; 8] = [7, 14, 30, 60, 90, 180, 365, 730];
/// Sparkline windows in minutes, five minutes up to thirty days.
pub const WINDOW_MINUTE_STEPS: [u32; 20] = [
    5, 10, 15, 30, 45, 60, 90, 120, 180, 240, 360, 480, 720, 1080, 1440, 2880, 4320, 10_080,
    20_160, 43_200,
];
pub const SPARKLINE_CELL_RANGE: (u8, u8) = (4, 24);
pub const MAX_HISTORY_DAYS: u16 = 3650;

/// Everything `[cost]` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CostSettings {
    pub calibration: Calibration,
    pub fixed_cuts: [f64; 4],
    pub burn_cuts: [f64; 4],
    pub budget_usd: f64,
    /// How far back the history calibration reads past sessions.
    pub history_days: u16,
    /// Time covered by the sparkline; six hours by default.
    pub sparkline_window_minutes: u32,
    pub sparkline_cells: u8,
    pub level_glyph: DisplayOption,
    pub level_dollars: DisplayOption,
    pub meter: DisplayOption,
    pub sparkline: DisplayOption,
    pub group_totals: DisplayOption,
    pub detail_card: DisplayOption,
    pub header_total: DisplayOption,
    pub burn_marker: DisplayOption,
    /// Order every tree level by descending cost.
    pub sort_by_cost: bool,
    /// Per-model price overrides (`$` per million tokens), keyed by model
    /// name prefix. Useful for models the built-in table does not know.
    pub prices: BTreeMap<String, ModelPrice>,
}

impl Default for CostSettings {
    fn default() -> Self {
        Self {
            calibration: Calibration::OwnHistory,
            fixed_cuts: FIXED_PRESETS[2],
            burn_cuts: BURN_PRESETS[1],
            budget_usd: 10.0,
            history_days: 90,
            sparkline_window_minutes: 360,
            sparkline_cells: 8,
            level_glyph: DisplayOption::default(),
            level_dollars: DisplayOption::default(),
            meter: DisplayOption {
                enabled: true,
                visibility: CostVisibility::Hover,
            },
            sparkline: DisplayOption::default(),
            group_totals: DisplayOption::default(),
            detail_card: DisplayOption::default(),
            header_total: DisplayOption::default(),
            burn_marker: DisplayOption::default(),
            sort_by_cost: false,
            prices: BTreeMap::new(),
        }
    }
}

impl CostSettings {
    pub fn option(&self, display: CostDisplay) -> DisplayOption {
        match display {
            CostDisplay::LevelGlyph => self.level_glyph,
            CostDisplay::LevelDollars => self.level_dollars,
            CostDisplay::Meter => self.meter,
            CostDisplay::Sparkline => self.sparkline,
            CostDisplay::GroupTotals => self.group_totals,
            CostDisplay::DetailCard => self.detail_card,
            CostDisplay::HeaderTotal => self.header_total,
            CostDisplay::BurnMarker => self.burn_marker,
        }
    }

    fn option_mut(&mut self, display: CostDisplay) -> &mut DisplayOption {
        match display {
            CostDisplay::LevelGlyph => &mut self.level_glyph,
            CostDisplay::LevelDollars => &mut self.level_dollars,
            CostDisplay::Meter => &mut self.meter,
            CostDisplay::Sparkline => &mut self.sparkline,
            CostDisplay::GroupTotals => &mut self.group_totals,
            CostDisplay::DetailCard => &mut self.detail_card,
            CostDisplay::HeaderTotal => &mut self.header_total,
            CostDisplay::BurnMarker => &mut self.burn_marker,
        }
    }

    /// Whether any indicator is enabled, i.e. whether cost must be tracked.
    pub fn is_any_enabled(&self) -> bool {
        self.sort_by_cost || CostDisplay::ALL.iter().any(|d| self.option(*d).enabled)
    }

    /// Repairs out-of-range values from a hand-edited file.
    pub fn sanitized(mut self) -> Self {
        let defaults = Self::default();
        if !is_ascending_positive(&self.fixed_cuts) {
            self.fixed_cuts = defaults.fixed_cuts;
        }
        if !is_ascending_positive(&self.burn_cuts) {
            self.burn_cuts = defaults.burn_cuts;
        }
        if !self.budget_usd.is_finite() || self.budget_usd < 0.01 {
            self.budget_usd = defaults.budget_usd;
        }
        self.history_days = self.history_days.clamp(1, MAX_HISTORY_DAYS);
        self.sparkline_window_minutes = self.sparkline_window_minutes.clamp(1, MAX_WINDOW_MINUTES);
        self.sparkline_cells = self
            .sparkline_cells
            .clamp(SPARKLINE_CELL_RANGE.0, SPARKLINE_CELL_RANGE.1);
        self.prices.retain(|_, price| {
            [
                price.input,
                price.output,
                price.cache_read,
                price.cache_write,
            ]
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0)
        });
        self
    }

    /// Sets the sparkline window to an exact number of minutes.
    pub fn set_window_minutes(&mut self, minutes: u32) {
        self.sparkline_window_minutes = minutes.clamp(1, MAX_WINDOW_MINUTES);
    }

    /// Applies one interaction to `row`. `direction` is `-1`, `0` (activate)
    /// or `1`. Returns whether anything changed.
    pub fn adjust(&mut self, row: CostRow, direction: i32) -> bool {
        let before = self.clone();
        match row {
            CostRow::Calibration(calibration) => self.calibration = calibration,
            CostRow::FixedPreset => {
                self.fixed_cuts = step_preset(&FIXED_PRESETS, self.fixed_cuts, direction)
            }
            CostRow::BurnPreset => {
                self.burn_cuts = step_preset(&BURN_PRESETS, self.burn_cuts, direction)
            }
            CostRow::Budget => {
                self.budget_usd = step_ladder(&BUDGET_STEPS, self.budget_usd, direction)
            }
            CostRow::HistoryDays => {
                self.history_days = step_ladder(&HISTORY_DAY_STEPS, self.history_days, direction)
            }
            CostRow::Display(display) => {
                let option = self.option_mut(display);
                option.enabled = !option.enabled;
            }
            CostRow::Visibility(display) => {
                let option = self.option_mut(display);
                option.visibility = option.visibility.toggled();
            }
            CostRow::SparklineWindow => {
                self.sparkline_window_minutes = step_ladder(
                    &WINDOW_MINUTE_STEPS,
                    self.sparkline_window_minutes,
                    direction,
                )
            }
            CostRow::SparklineCells => {
                let step = if direction < 0 { -1 } else { 1 };
                let cells = i16::from(self.sparkline_cells) + step;
                self.sparkline_cells = cells.clamp(
                    i16::from(SPARKLINE_CELL_RANGE.0),
                    i16::from(SPARKLINE_CELL_RANGE.1),
                ) as u8;
            }
            CostRow::SortByCost => self.sort_by_cost = !self.sort_by_cost,
        }
        *self != before
    }
}

fn is_ascending_positive(cuts: &[f64; 4]) -> bool {
    cuts.iter().all(|cut| cut.is_finite() && *cut > 0.0) && cuts.windows(2).all(|p| p[0] < p[1])
}

/// Moves to the next/previous ladder value; a value not on the ladder (a
/// hand-edited file) snaps to the nearest one in the requested direction.
/// Activating with `direction == 0` steps forward.
fn step_ladder<T: Copy + PartialOrd>(ladder: &[T], current: T, direction: i32) -> T {
    let forward = direction >= 0;
    if forward {
        ladder
            .iter()
            .copied()
            .find(|value| *value > current)
            .unwrap_or(ladder[0])
    } else {
        ladder
            .iter()
            .rev()
            .copied()
            .find(|value| *value < current)
            .unwrap_or(ladder[ladder.len() - 1])
    }
}

fn step_preset(presets: &[[f64; 4]], current: [f64; 4], direction: i32) -> [f64; 4] {
    let forward = direction >= 0;
    let index = presets.iter().position(|preset| *preset == current);
    match (index, forward) {
        (Some(index), true) => presets[(index + 1) % presets.len()],
        (Some(index), false) => presets[(index + presets.len() - 1) % presets.len()],
        (None, true) => presets
            .iter()
            .copied()
            .find(|preset| preset[0] > current[0])
            .unwrap_or(presets[0]),
        (None, false) => presets
            .iter()
            .rev()
            .copied()
            .find(|preset| preset[0] < current[0])
            .unwrap_or(presets[presets.len() - 1]),
    }
}

/// True when `cuts` equals one of `presets`; otherwise the file holds custom
/// numbers and the settings tab labels them so.
pub fn is_preset(presets: &[[f64; 4]], cuts: [f64; 4]) -> bool {
    presets.contains(&cuts)
}

/// One selectable row of the Cost settings tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostRow {
    /// Radio card choosing how "expensive" is decided.
    Calibration(Calibration),
    FixedPreset,
    HistoryDays,
    Budget,
    BurnPreset,
    /// Enable / disable one indicator.
    Display(CostDisplay),
    /// When that indicator is visible.
    Visibility(CostDisplay),
    SparklineWindow,
    SparklineCells,
    SortByCost,
}

impl CostRow {
    /// Every row in display order. Parameter rows appear only for the active
    /// calibration, directly after the radio cards.
    pub fn rows(settings: &CostSettings) -> Vec<Self> {
        let mut rows: Vec<Self> = Calibration::ALL
            .iter()
            .copied()
            .map(Self::Calibration)
            .collect();
        match settings.calibration {
            Calibration::FixedBands => rows.push(Self::FixedPreset),
            Calibration::PeerRelative => {}
            Calibration::OwnHistory => rows.push(Self::HistoryDays),
            Calibration::Budget => rows.push(Self::Budget),
            Calibration::BurnRate => rows.push(Self::BurnPreset),
        }
        for display in CostDisplay::ALL {
            rows.push(Self::Display(display));
            rows.push(Self::Visibility(display));
        }
        rows.push(Self::SparklineWindow);
        rows.push(Self::SparklineCells);
        rows.push(Self::SortByCost);
        rows
    }

    pub fn help_id(self) -> &'static str {
        match self {
            Self::Calibration(Calibration::FixedBands) => "COST-01",
            Self::Calibration(Calibration::PeerRelative) => "COST-02",
            Self::Calibration(Calibration::OwnHistory) => "COST-03",
            Self::Calibration(Calibration::Budget) => "COST-04",
            Self::Calibration(Calibration::BurnRate) => "COST-05",
            Self::FixedPreset => "COST-06",
            Self::HistoryDays => "COST-07",
            Self::Budget => "COST-08",
            Self::BurnPreset => "COST-09",
            Self::Display(display) | Self::Visibility(display) => display.help_id(),
            Self::SparklineWindow => "COST-18",
            Self::SparklineCells => "COST-19",
            Self::SortByCost => "COST-20",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_show_only_the_meter_and_only_on_hover() {
        let settings = CostSettings::default();
        assert_eq!(settings.calibration, Calibration::OwnHistory);
        assert_eq!(settings.sparkline_window_minutes, 360);
        for display in CostDisplay::ALL {
            let option = settings.option(display);
            assert_eq!(option.enabled, display == CostDisplay::Meter, "{display:?}");
            assert_eq!(option.visibility, CostVisibility::Hover, "{display:?}");
        }
        assert!(settings.meter.is_shown(true));
        assert!(!settings.meter.is_shown(false));
        assert!(!settings.level_glyph.is_shown(true));
    }

    #[test]
    fn always_visible_options_show_without_hover() {
        let mut settings = CostSettings::default();
        settings.adjust(CostRow::Visibility(CostDisplay::Meter), 0);
        assert!(settings.meter.is_shown(false));
        settings.adjust(CostRow::Display(CostDisplay::Meter), 0);
        assert!(!settings.meter.is_shown(true));
    }

    #[test]
    fn rows_follow_the_selected_calibration() {
        let mut settings = CostSettings::default();
        assert!(CostRow::rows(&settings).contains(&CostRow::HistoryDays));
        assert!(!CostRow::rows(&settings).contains(&CostRow::Budget));
        settings.adjust(CostRow::Calibration(Calibration::Budget), 0);
        assert!(CostRow::rows(&settings).contains(&CostRow::Budget));
        assert!(!CostRow::rows(&settings).contains(&CostRow::HistoryDays));
        settings.adjust(CostRow::Calibration(Calibration::BurnRate), 0);
        assert!(CostRow::rows(&settings).contains(&CostRow::BurnPreset));
        settings.adjust(CostRow::Calibration(Calibration::FixedBands), 0);
        assert!(CostRow::rows(&settings).contains(&CostRow::FixedPreset));
        settings.adjust(CostRow::Calibration(Calibration::PeerRelative), 0);
        let rows = CostRow::rows(&settings);
        assert_eq!(rows.len(), 5 + 16 + 3);
    }

    #[test]
    fn every_row_has_a_distinct_or_shared_help_id_in_range() {
        let settings = CostSettings::default();
        for calibration in Calibration::ALL {
            let mut settings = settings.clone();
            settings.calibration = calibration;
            for row in CostRow::rows(&settings) {
                let id = row.help_id();
                let number: u32 = id.strip_prefix("COST-").unwrap().parse().unwrap();
                assert!((1..=20).contains(&number), "{id}");
            }
        }
    }

    #[test]
    fn sparkline_window_steps_through_the_ladder_and_snaps_custom_values() {
        let mut settings = CostSettings::default();
        assert!(settings.adjust(CostRow::SparklineWindow, 1));
        assert_eq!(settings.sparkline_window_minutes, 480);
        assert!(settings.adjust(CostRow::SparklineWindow, -1));
        assert_eq!(settings.sparkline_window_minutes, 360);
        settings.sparkline_window_minutes = 100;
        settings.adjust(CostRow::SparklineWindow, 1);
        assert_eq!(settings.sparkline_window_minutes, 120);
        settings.sparkline_window_minutes = 100;
        settings.adjust(CostRow::SparklineWindow, -1);
        assert_eq!(settings.sparkline_window_minutes, 90);
        settings.sparkline_window_minutes = 43_200;
        settings.adjust(CostRow::SparklineWindow, 1);
        assert_eq!(settings.sparkline_window_minutes, 5);
    }

    #[test]
    fn presets_wrap_and_custom_cuts_snap() {
        let mut settings = CostSettings::default();
        assert_eq!(settings.fixed_cuts, FIXED_PRESETS[2]);
        settings.adjust(CostRow::FixedPreset, 1);
        assert_eq!(settings.fixed_cuts, FIXED_PRESETS[3]);
        settings.fixed_cuts = [0.7, 3.0, 9.0, 30.0];
        settings.adjust(CostRow::FixedPreset, 1);
        assert_eq!(settings.fixed_cuts, FIXED_PRESETS[2]);
        settings.fixed_cuts = FIXED_PRESETS[0];
        settings.adjust(CostRow::FixedPreset, -1);
        assert_eq!(settings.fixed_cuts, FIXED_PRESETS[4]);
        assert!(is_preset(&FIXED_PRESETS, FIXED_PRESETS[1]));
        assert!(!is_preset(&FIXED_PRESETS, [1.0, 2.0, 3.0, 4.0]));
    }

    #[test]
    fn sparkline_cells_are_clamped() {
        let mut settings = CostSettings::default();
        settings.sparkline_cells = SPARKLINE_CELL_RANGE.1;
        assert!(!settings.adjust(CostRow::SparklineCells, 1));
        settings.sparkline_cells = SPARKLINE_CELL_RANGE.0;
        assert!(!settings.adjust(CostRow::SparklineCells, -1));
    }

    #[test]
    fn sanitizing_repairs_hand_edited_nonsense() {
        let settings = CostSettings {
            fixed_cuts: [5.0, 1.0, 2.0, 3.0],
            burn_cuts: [0.0, 1.0, 2.0, 3.0],
            budget_usd: f64::NAN,
            history_days: 0,
            sparkline_window_minutes: 0,
            sparkline_cells: 200,
            ..CostSettings::default()
        }
        .sanitized();
        let defaults = CostSettings::default();
        assert_eq!(settings.fixed_cuts, defaults.fixed_cuts);
        assert_eq!(settings.burn_cuts, defaults.burn_cuts);
        assert_eq!(settings.budget_usd, defaults.budget_usd);
        assert_eq!(settings.history_days, 1);
        assert_eq!(settings.sparkline_window_minutes, 1);
        assert_eq!(settings.sparkline_cells, SPARKLINE_CELL_RANGE.1);
    }

    #[test]
    fn toml_round_trip_keeps_nested_options_and_accepts_partial_files() {
        let mut settings = CostSettings::default();
        settings.adjust(CostRow::Display(CostDisplay::Sparkline), 0);
        settings.adjust(CostRow::Visibility(CostDisplay::Sparkline), 0);
        settings.prices.insert(
            "gpt-reserve".to_owned(),
            ModelPrice {
                input: 1.0,
                output: 2.0,
                cache_read: 0.1,
                cache_write: 0.0,
            },
        );
        let text = toml::to_string(&settings).unwrap();
        let loaded: CostSettings = toml::from_str(&text).unwrap();
        assert_eq!(loaded, settings);

        let partial: CostSettings =
            toml::from_str("calibration = \"budget\"\n[meter]\nvisibility = \"always\"\n").unwrap();
        assert_eq!(partial.calibration, Calibration::Budget);
        assert_eq!(partial.meter.visibility, CostVisibility::Always);
        assert!(!partial.meter.enabled);
        assert_eq!(partial.sparkline_window_minutes, 360);
    }
}
