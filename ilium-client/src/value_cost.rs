//! Exact cost-dialog catalogs and fresh validation without persistence side effects.
use crate::cost_model::{Calibration, CostMetric, QuotaWindow};
use crate::cost_settings::{
    CostRow, CostSettings, CostVisibility, BURN_PRESETS, FIXED_PRESETS, QUOTA_BURN_PRESETS,
    QUOTA_FIXED_PRESETS,
};
use crate::value_dialog::{ChoiceDialogState, ChoiceOption, NumberDialogState, ValueDialogState};

pub struct CostValueTarget {
    pub row: CostRow,
    metric: CostMetric,
    calibration: Calibration,
    custom_cuts: Option<[f64; 4]>,
}

pub fn is_choice(row: CostRow) -> bool {
    matches!(
        row,
        CostRow::Metric(_)
            | CostRow::QuotaWindow
            | CostRow::Calibration(_)
            | CostRow::FixedPreset
            | CostRow::BurnPreset
            | CostRow::Visibility(_)
    )
}

impl CostValueTarget {
    pub fn new(settings: &CostSettings, row: CostRow) -> Result<Self, String> {
        if !CostRow::rows(settings).contains(&row)
            || (settings.number_spec(row).is_none() && !is_choice(row))
        {
            return Err("This cost row does not offer a value dialog".into());
        }
        let custom_cuts = match row {
            CostRow::FixedPreset => Some(settings.active_fixed_cuts()),
            CostRow::BurnPreset => Some(settings.active_burn_cuts()),
            _ => None,
        };
        Ok(Self {
            row,
            metric: settings.metric,
            calibration: settings.calibration,
            custom_cuts,
        })
    }

    pub fn title(&self) -> String {
        match self.row {
            CostRow::Metric(_) => "Cost metric".into(),
            CostRow::Calibration(_) => "Cost rating".into(),
            CostRow::QuotaWindow => "Quota window".into(),
            CostRow::FixedPreset => "Fixed thresholds".into(),
            CostRow::BurnPreset => "Burn thresholds".into(),
            CostRow::HistoryDays => "History days".into(),
            CostRow::Budget => "Per-agent budget".into(),
            CostRow::SparklineWindow => "Sparkline window (minutes)".into(),
            CostRow::SparklineCells => "Sparkline cells".into(),
            CostRow::Visibility(display) => format!("{} visibility", display.label()),
            _ => "Cost setting".into(),
        }
    }

    fn validate(&self, settings: &CostSettings) -> Result<(), String> {
        if settings.metric != self.metric
            || settings.calibration != self.calibration
            || !CostRow::rows(settings).contains(&self.row)
        {
            return Err("The cost configuration changed; reopen its value dialog".into());
        }
        Ok(())
    }

    fn presets(&self) -> &'static [[f64; 4]] {
        match (self.row, self.metric) {
            (CostRow::FixedPreset, CostMetric::Dollars) => &FIXED_PRESETS,
            (CostRow::FixedPreset, CostMetric::Quota) => &QUOTA_FIXED_PRESETS,
            (CostRow::BurnPreset, CostMetric::Dollars) => &BURN_PRESETS,
            (CostRow::BurnPreset, CostMetric::Quota) => &QUOTA_BURN_PRESETS,
            _ => &[],
        }
    }

    pub fn dialog(&self, settings: &CostSettings) -> Result<ValueDialogState, String> {
        self.validate(settings)?;
        if let Some(number) = settings.number_text(self.row) {
            return Ok(ValueDialogState::Number(NumberDialogState::new(
                self.title(),
                number,
            )));
        }
        let (mut entries, selected): (Vec<(String, String)>, String) = match self.row {
            CostRow::Metric(_) => (
                CostMetric::ALL
                    .into_iter()
                    .map(|metric| (metric_id(metric).into(), metric.label().into()))
                    .collect(),
                metric_id(settings.metric).into(),
            ),
            CostRow::Calibration(_) => (
                Calibration::ALL
                    .into_iter()
                    .map(|calibration| {
                        (
                            calibration_id(calibration).into(),
                            calibration.label().into(),
                        )
                    })
                    .collect(),
                calibration_id(settings.calibration).into(),
            ),
            CostRow::QuotaWindow => (
                QuotaWindow::ALL
                    .into_iter()
                    .map(|window| (window.key().into(), window.label().into()))
                    .collect(),
                settings.quota_window.key().into(),
            ),
            CostRow::Visibility(display) => (
                vec![
                    ("always".into(), CostVisibility::Always.label().into()),
                    ("hover".into(), CostVisibility::Hover.label().into()),
                ],
                match settings.option(display).visibility {
                    CostVisibility::Always => "always",
                    CostVisibility::Hover => "hover",
                }
                .into(),
            ),
            CostRow::FixedPreset | CostRow::BurnPreset => {
                let current = self.custom_cuts.ok_or("Missing cost thresholds")?;
                let presets = self.presets();
                let selected = presets
                    .iter()
                    .position(|cuts| *cuts == current)
                    .map_or_else(|| "custom".into(), |index| index.to_string());
                let entries = presets
                    .iter()
                    .enumerate()
                    .map(|(index, cuts)| {
                        (
                            index.to_string(),
                            format!(
                                "{} / {} / {} / {} {}",
                                cuts[0],
                                cuts[1],
                                cuts[2],
                                cuts[3],
                                if self.metric == CostMetric::Dollars {
                                    "$"
                                } else {
                                    "%"
                                }
                            ),
                        )
                    })
                    .collect();
                (entries, selected)
            }
            _ => return Err("This cost control does not offer a choice list".into()),
        };
        if selected == "custom" {
            let cuts = self.custom_cuts.ok_or("Missing custom thresholds")?;
            entries.push((
                "custom".into(),
                format!(
                    "Current custom: {} / {} / {} / {}",
                    cuts[0], cuts[1], cuts[2], cuts[3]
                ),
            ));
        }
        let options = entries
            .into_iter()
            .map(|(id, label)| ChoiceOption {
                id,
                label,
                disabled_reason: None,
            })
            .collect();
        Ok(ValueDialogState::Choice(ChoiceDialogState::new(
            self.title(),
            options,
            Some(selected),
        )?))
    }

    pub fn set_number(&self, settings: &mut CostSettings, text: &str) -> Result<(), String> {
        self.validate(settings)?;
        settings.set_number(self.row, text)
    }

    pub fn set_choice(&self, settings: &mut CostSettings, id: &str) -> Result<(), String> {
        self.validate(settings)?;
        match self.row {
            CostRow::Metric(_) => {
                settings.metric = CostMetric::ALL
                    .into_iter()
                    .find(|metric| metric_id(*metric) == id)
                    .ok_or("Unknown cost metric")?;
            }
            CostRow::Calibration(_) => {
                settings.calibration = Calibration::ALL
                    .into_iter()
                    .find(|calibration| calibration_id(*calibration) == id)
                    .ok_or("Unknown cost rating")?;
            }
            CostRow::QuotaWindow => {
                settings.quota_window = QuotaWindow::ALL
                    .into_iter()
                    .find(|window| window.key() == id)
                    .ok_or("Unknown quota window")?
            }
            CostRow::Visibility(display) => {
                settings.set_visibility(
                    display,
                    match id {
                        "always" => CostVisibility::Always,
                        "hover" => CostVisibility::Hover,
                        _ => return Err("Unknown visibility option".into()),
                    },
                );
            }
            CostRow::FixedPreset | CostRow::BurnPreset => {
                let cuts = if id == "custom" {
                    let opened = self.custom_cuts.ok_or("Missing custom thresholds")?;
                    let current = if self.row == CostRow::FixedPreset {
                        settings.active_fixed_cuts()
                    } else {
                        settings.active_burn_cuts()
                    };
                    if current != opened {
                        return Err("The custom thresholds changed; reopen this list".into());
                    }
                    opened
                } else {
                    let index = id
                        .parse::<usize>()
                        .map_err(|_| "Unknown threshold preset")?;
                    if index.to_string() != id {
                        return Err("Unknown threshold preset".into());
                    }
                    *self
                        .presets()
                        .get(index)
                        .ok_or("Unknown threshold preset")?
                };
                match (self.row, self.metric) {
                    (CostRow::FixedPreset, CostMetric::Dollars) => settings.fixed_cuts = cuts,
                    (CostRow::FixedPreset, CostMetric::Quota) => settings.quota_fixed_cuts = cuts,
                    (CostRow::BurnPreset, CostMetric::Dollars) => settings.burn_cuts = cuts,
                    (CostRow::BurnPreset, CostMetric::Quota) => settings.quota_burn_cuts = cuts,
                    _ => return Err("Threshold destination changed".into()),
                }
            }
            _ => return Err("This cost setting is not a choice".into()),
        }
        Ok(())
    }
}

fn metric_id(metric: CostMetric) -> &'static str {
    match metric {
        CostMetric::Dollars => "dollars",
        CostMetric::Quota => "quota",
    }
}

fn calibration_id(calibration: Calibration) -> &'static str {
    match calibration {
        Calibration::FixedBands => "fixed_bands",
        Calibration::PeerRelative => "peer_relative",
        Calibration::OwnHistory => "own_history",
        Calibration::Budget => "budget",
        Calibration::BurnRate => "burn_rate",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_offer_the_full_metric_and_calibration_catalogs() {
        let mut settings = CostSettings::default();
        let metric = CostValueTarget::new(&settings, CostRow::Metric(settings.metric)).unwrap();
        let ValueDialogState::Choice(metric_dialog) = metric.dialog(&settings).unwrap() else {
            panic!("metric should be a choice dialog");
        };
        assert_eq!(metric_dialog.options().len(), CostMetric::ALL.len());
        metric.set_choice(&mut settings, "quota").unwrap();
        assert_eq!(settings.metric, CostMetric::Quota);

        let calibration =
            CostValueTarget::new(&settings, CostRow::Calibration(settings.calibration)).unwrap();
        let ValueDialogState::Choice(calibration_dialog) = calibration.dialog(&settings).unwrap()
        else {
            panic!("calibration should be a choice dialog");
        };
        assert_eq!(calibration_dialog.options().len(), Calibration::ALL.len());
        calibration.set_choice(&mut settings, "burn_rate").unwrap();
        assert_eq!(settings.calibration, Calibration::BurnRate);
        assert!(calibration.set_choice(&mut settings, "unknown").is_err());
    }

    #[test]
    fn custom_thresholds_and_every_preset_are_recoverable_without_cycling() {
        let mut settings = CostSettings {
            calibration: Calibration::FixedBands,
            fixed_cuts: [1.1, 2.2, 3.3, 4.4],
            ..CostSettings::default()
        };
        let target = CostValueTarget::new(&settings, CostRow::FixedPreset).unwrap();
        let ValueDialogState::Choice(dialog) = target.dialog(&settings).unwrap() else {
            panic!("choice");
        };
        assert_eq!(dialog.options().len(), FIXED_PRESETS.len() + 1);
        assert_eq!(dialog.selected_id.as_deref(), Some("custom"));
        let before = settings.clone();
        target.set_choice(&mut settings, "custom").unwrap();
        assert_eq!(settings, before);
        target.set_choice(&mut settings, "4").unwrap();
        assert_eq!(settings.fixed_cuts, FIXED_PRESETS[4]);
        assert!(target.set_choice(&mut settings, "custom").is_err());
    }
    #[test]
    fn budget_rejects_changed_units_and_keeps_exact_valid_intermediates() {
        let mut settings = CostSettings {
            calibration: Calibration::Budget,
            ..CostSettings::default()
        };
        let target = CostValueTarget::new(&settings, CostRow::Budget).unwrap();
        target.set_number(&mut settings, "17.125").unwrap();
        assert_eq!(settings.budget_usd, 17.125);
        settings.metric = CostMetric::Quota;
        let before = settings.clone();
        assert!(target.set_number(&mut settings, "17.125").is_err());
        assert_eq!(settings, before);
    }
    #[test]
    fn hidden_rows_invalid_numbers_and_unknown_options_never_mutate() {
        let mut settings = CostSettings::default();
        assert!(CostValueTarget::new(&settings, CostRow::Budget).is_err());
        let target = CostValueTarget::new(&settings, CostRow::HistoryDays).unwrap();
        let before = settings.clone();
        for input in ["0", "3651", "1.5", ""] {
            assert!(target.set_number(&mut settings, input).is_err());
        }
        assert_eq!(settings, before);
        let target = CostValueTarget::new(
            &settings,
            CostRow::Visibility(crate::cost_settings::CostDisplay::Meter),
        )
        .unwrap();
        assert!(target.set_choice(&mut settings, "invalid").is_err());
        assert_eq!(settings, before);
    }
}
