//! `model.*` operations: price table, level scales, calibration, formatting and spend series.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::{float, number, numbers};
use crate::model::{
    budget_fill, burn_usd_per_hour, calibrate, format_amount, format_quota, format_rate,
    format_tokens, format_usd, format_usd_per_hour, format_window, is_burn_spike, meter_string,
    parse_window_minutes, percent_rank, percentile, spend_cells, sparkline_glyphs, Calibration,
    CalibrationInputs, CalibrationNote, CostLevel, CostMetric, LevelScale, ModelPrice, PriceTable,
    QuotaWindow, ScaleBasis, SpendPoint,
};
use crate::tokens::{ModelUsage, TokenTotals};

fn points(value: &Value) -> Vec<SpendPoint> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| SpendPoint {
                    at_ms: item["at_ms"].as_i64().unwrap_or(0),
                    usd: item["usd"].as_f64().unwrap_or(0.0),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn cuts(value: &Value) -> [f64; 4] {
    let list = numbers(value);
    let mut cuts = [0.0; 4];
    for (slot, cut) in cuts.iter_mut().zip(list) {
        *slot = cut;
    }
    cuts
}

fn enum_field<T: serde::de::DeserializeOwned + Default>(value: &Value) -> T {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

fn price_table(overrides: &Value) -> PriceTable {
    let map: BTreeMap<String, ModelPrice> = serde_json::from_value(overrides.clone()).unwrap_or_default();
    PriceTable::with_overrides(&map)
}

fn tokens(value: &Value) -> TokenTotals {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

fn level(value: &Value) -> Option<CostLevel> {
    value.as_u64().map(|index| CostLevel::new(index as usize))
}

pub(super) fn run(name: &str, request: &Value) -> Value {
    match name {
        "format-usd" => json!(format_usd(
            number(request, "usd"),
            request["lower_bound"].as_bool().unwrap_or(false)
        )),
        "format-amount" => json!(format_amount(
            enum_field::<CostMetric>(&request["metric"]),
            number(request, "value"),
            request["lower_bound"].as_bool().unwrap_or(false)
        )),
        "format-quota" => json!(format_quota(number(request, "value"))),
        "format-rate" => json!(format_rate(
            enum_field::<CostMetric>(&request["metric"]),
            number(request, "value")
        )),
        "format-usd-per-hour" => json!(format_usd_per_hour(number(request, "usd"))),
        "format-tokens" => json!(format_tokens(request["tokens"].as_u64().unwrap_or(0))),
        "format-window" => json!(format_window(request["minutes"].as_u64().unwrap_or(0) as u32)),
        "parse-window-minutes" => parse_window_minutes(request["text"].as_str().unwrap_or(""))
            .map_or(Value::Null, |minutes| json!(minutes)),
        "quota-window-key" => json!(enum_field::<QuotaWindow>(&request["window"]).key()),
        "budget-fill" => float(budget_fill(number(request, "total"), number(request, "budget"))),
        "burn_usd_per_hour" => float(burn_usd_per_hour(
            &points(&request["points"]),
            request["now_ms"].as_i64().unwrap_or(0),
            request["window_ms"].as_i64().unwrap_or(0),
        )),
        "is-burn-spike" => json!(is_burn_spike(
            &points(&request["points"]),
            request["now_ms"].as_i64().unwrap_or(0),
            request["baseline_window_ms"].as_i64().unwrap_or(0),
            number(request, "minimum_per_hour"),
        )),
        "spend-cells" => Value::Array(
            spend_cells(
                &points(&request["points"]),
                request["now_ms"].as_i64().unwrap_or(0),
                request["window_ms"].as_i64().unwrap_or(0),
                request["cells"].as_u64().unwrap_or(0) as usize,
            )
            .into_iter()
            .map(float)
            .collect(),
        ),
        "sparkline-glyphs" => json!(sparkline_glyphs(&numbers(&request["values"]))),
        "percentile" => float(percentile(&numbers(&request["sorted"]), number(request, "fraction"))),
        "percent-rank" => float(percent_rank(&numbers(&request["sorted"]), number(request, "value"))),
        "scale-level" => json!(LevelScale {
            cuts: cuts(&request["cuts"])
        }
        .level(number(request, "value"))
        .index()),
        "level-meter" => json!(meter_string(level(&request["level"]))),
        "meter-string" => json!(meter_string(level(&request["level"]))),
        "level-glyph" => level(&request["level"]).map_or(Value::Null, |level| json!(level.glyph().to_string())),
        "price-lookup" => price_table(&request["overrides"])
            .lookup(request["model"].as_str().unwrap_or(""))
            .map_or(Value::Null, |price| json!(price)),
        "price-cost" => price_table(&request["overrides"])
            .cost(request["model"].as_str().unwrap_or(""), &tokens(&request["tokens"]))
            .map_or(Value::Null, float),
        "price-estimate" => {
            let models: Vec<ModelUsage> =
                serde_json::from_value(request["models"].clone()).unwrap_or_default();
            let estimate = price_table(&request["overrides"]).estimate(&models);
            json!({
                "usd": estimate.usd,
                "unpriced_tokens": estimate.unpriced_tokens,
                "is_lower_bound": estimate.is_lower_bound(),
            })
        }
        "calibrate" => {
            let peers = numbers(&request["peer_totals"]);
            let history = request.get("history_sorted").filter(|v| !v.is_null()).map(numbers);
            let inputs = CalibrationInputs {
                fixed_cuts: cuts(&request["fixed_cuts"]),
                burn_cuts: cuts(&request["burn_cuts"]),
                budget: number(request, "budget"),
                peer_totals: &peers,
                history_sorted: history.as_deref(),
            };
            let calibrated = calibrate(enum_field::<Calibration>(&request["calibration"]), &inputs);
            json!({
                "cuts": calibrated.scale.cuts,
                "basis": match calibrated.basis {
                    ScaleBasis::TotalUsd => "total_usd",
                    ScaleBasis::BurnUsdPerHour => "burn_usd_per_hour",
                },
                "note": match calibrated.note {
                    CalibrationNote::Ready => "ready",
                    CalibrationNote::HistoryUnavailable => "history_unavailable",
                    CalibrationNote::NoPeers => "no_peers",
                },
            })
        }
        _ => Value::Null,
    }
}
