//! `chart.*` operations: plotting and resampling of text line charts.

use std::cell::Cell;

use serde_json::{json, Value};

use super::{float, numbers};
use crate::chart::{plot, resample, CellKind, ChartConfig, Downsample};

thread_local! {
    static FORMAT: Cell<(f64, usize, &'static str)> = const { Cell::new((1.0, 2, "")) };
}

fn formatted_label(value: f64) -> String {
    let (divide_by, decimals, suffix) = FORMAT.with(Cell::get);
    format!("{:.decimals$}{suffix}", value / divide_by)
}

fn downsample(name: &str) -> Downsample {
    match name {
        "max" => Downsample::Max,
        "mean" => Downsample::Mean,
        _ => Downsample::Interpolate,
    }
}

fn usize_field(config: &Value, key: &str) -> usize {
    config.get(key).and_then(Value::as_u64).unwrap_or(0) as usize
}

pub(super) fn run(name: &str, request: &Value) -> Value {
    match name {
        "plot" => plot_reply(request),
        "resample" => {
            let values = numbers(&request["values"]);
            let width = usize_field(request, "width");
            let mode = downsample(request["downsample"].as_str().unwrap_or("interpolate"));
            Value::Array(resample(&values, width, mode).into_iter().map(float).collect())
        }
        _ => Value::Null,
    }
}

fn plot_reply(request: &Value) -> Value {
    let series: Vec<Vec<f64>> = request["series"]
        .as_array()
        .map(|items| items.iter().map(numbers).collect())
        .unwrap_or_default();
    let config = &request["config"];
    let mut chart_config = ChartConfig {
        width: usize_field(config, "width"),
        height: usize_field(config, "height"),
        offset: usize_field(config, "offset"),
        downsample: downsample(config["downsample"].as_str().unwrap_or("interpolate")),
        include_zero: config["include_zero"].as_bool().unwrap_or(false),
        label_formatter: None,
    };
    if let Some(formatter) = config.get("label_formatter").filter(|value| value.is_object()) {
        let suffix: &'static str = match formatter["suffix"].as_str().unwrap_or("") {
            "k" => "k",
            "M" => "M",
            "%" => "%",
            _ => "",
        };
        FORMAT.with(|cell| {
            cell.set((
                formatter["divide_by"].as_f64().unwrap_or(1.0),
                formatter["decimals"].as_u64().unwrap_or(2) as usize,
                suffix,
            ));
        });
        chart_config.label_formatter = Some(formatted_label);
    }
    let chart = plot(&series, &chart_config);
    let raw_lines = chart.to_lines();
    let lines: Vec<String> = raw_lines
        .iter()
        .map(|line| line.trim_end().to_string())
        .collect();
    let mut kinds: Vec<String> = Vec::new();
    for cell in chart.rows.iter().flatten() {
        let kind = match cell.kind {
            CellKind::Blank => continue,
            CellKind::Label => "label".to_string(),
            CellKind::Axis => "axis".to_string(),
            CellKind::Line(index) => format!("line:{index}"),
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    json!({
        "lines": lines,
        "raw_lines": raw_lines,
        "rows": chart.rows.len(),
        "gutter": chart.gutter,
        "kinds": kinds,
    })
}
