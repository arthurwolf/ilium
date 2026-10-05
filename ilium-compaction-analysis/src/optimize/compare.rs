//! The three-way comparison of the report: CLI default | currently configured
//! | recommended, with cost and relative consumption.

use serde::{Deserialize, Serialize};

use crate::replay::{ReworkModel, SimTable};

/// How well a point's cost is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointQuality {
    /// The trigger is a simulated grid point.
    Exact,
    /// Interpolated between two neighbouring grid points.
    Interpolated,
    /// Outside the grid: the nearest grid point is reported.
    OutsideGrid,
}

/// One point of the three-way comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonPoint {
    /// "default", "current" or "recommended".
    pub label: String,
    /// Effective trigger, tokens.
    pub trigger_tokens: u32,
    /// Weighted cost (rework included).
    pub cost: f64,
    /// Dollars, when known.
    pub usd: Option<f64>,
    /// Mean simulated compactions.
    pub compactions: f64,
    /// Cost relative to the recommended point (0 = same).
    pub relative_to_recommended: f64,
    /// Cost relative to the CLI default (the consumption users look at).
    pub relative_to_default: f64,
    /// How the cost was obtained.
    pub quality: PointQuality,
}

/// CLI default | currently configured | recommended, with cost and relative
/// consumption. `current_trigger` is the effective trigger of the configured
/// value (use [`AgentSemantics::setting_to_trigger`]); `None` when nothing is
/// configured.
pub fn three_way_comparison(
    table: &SimTable,
    rework: &ReworkModel,
    default_trigger: u32,
    current_trigger: Option<u32>,
    recommended_trigger: u32,
) -> Vec<ComparisonPoint> {
    let totals = table.total_costs(rework, &|_| true);
    let usd = table.total_usd(rework, &|_| true);
    let compactions = table.total_compactions(&|_| true);
    let at = |trigger: u32| interpolate(&table.triggers, &totals, &usd, &compactions, trigger);
    let recommended = at(recommended_trigger);
    let default = at(default_trigger);
    let mut points = vec![("default", default_trigger)];
    if let Some(current) = current_trigger {
        points.push(("current", current));
    }
    points.push(("recommended", recommended_trigger));
    points
        .into_iter()
        .map(|(label, trigger)| {
            let (cost, usd, compactions, quality) = at(trigger);
            ComparisonPoint {
                label: label.to_string(),
                trigger_tokens: trigger,
                cost,
                usd,
                compactions,
                relative_to_recommended: cost / recommended.0 - 1.0,
                relative_to_default: cost / default.0 - 1.0,
                quality,
            }
        })
        .collect()
}

fn interpolate(
    triggers: &[u32],
    totals: &[f64],
    usd: &[Option<f64>],
    compactions: &[f64],
    trigger: u32,
) -> (f64, Option<f64>, f64, PointQuality) {
    if let Ok(index) = triggers.binary_search(&trigger) {
        return (
            totals[index],
            usd[index],
            compactions[index],
            PointQuality::Exact,
        );
    }
    let upper = triggers.partition_point(|&candidate| candidate < trigger);
    if upper == 0 || upper == triggers.len() {
        let edge = if upper == 0 { 0 } else { triggers.len() - 1 };
        return (
            totals[edge],
            usd[edge],
            compactions[edge],
            PointQuality::OutsideGrid,
        );
    }
    let lower = upper - 1;
    let span = f64::from(triggers[upper] - triggers[lower]);
    let weight = f64::from(trigger - triggers[lower]) / span;
    let mix = |left: f64, right: f64| left + (right - left) * weight;
    let usd_mixed = match (usd[lower], usd[upper]) {
        (Some(left), Some(right)) => Some(mix(left, right)),
        _ => None,
    };
    (
        mix(totals[lower], totals[upper]),
        usd_mixed,
        mix(compactions[lower], compactions[upper]),
        PointQuality::Interpolated,
    )
}
