//! `timeline.*` operations: the work ribbon and the time-scale windows.

use serde_json::{json, Value};

use crate::stats_types::{ProgressOutcome, ProgressSpan, SessionStats, WorkKind, WorkMark};
use crate::timeline::{work_ribbon, StatsScale};

fn stats(value: &Value) -> SessionStats {
    let work_marks: Vec<WorkMark> = value["kinds"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|pair| {
                    Some(WorkMark {
                        at_ms: pair.get(0)?.as_i64()?,
                        kind: WorkKind::parse(pair.get(1)?.as_str()?)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let progress_spans = value["spans"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|span| {
                    Some(ProgressSpan {
                        start_ms: span["start_ms"].as_i64()?,
                        end_ms: span["end_ms"].as_i64(),
                        outcome: ProgressOutcome::parse(span["outcome"].as_str()?)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    SessionStats {
        first_at_ms: work_marks.first().map(|mark| mark.at_ms),
        last_at_ms: value["last_ms"].as_i64(),
        work_marks,
        progress_spans,
    }
}

fn scale(name: &str) -> StatsScale {
    match name {
        "day" => StatsScale::Day,
        "six_hours" => StatsScale::SixHours,
        "hour" => StatsScale::Hour,
        "quarter_hour" => StatsScale::QuarterHour,
        "five_minutes" => StatsScale::FiveMinutes,
        _ => StatsScale::All,
    }
}

pub(super) fn run(name: &str, request: &Value) -> Value {
    let stats = stats(&request["stats"]);
    match name {
        "work-ribbon" => {
            let ribbon = work_ribbon(
                &stats,
                request["from"].as_i64().unwrap_or(0),
                request["to"].as_i64().unwrap_or(0),
                request["columns"].as_u64().unwrap_or(0) as usize,
            );
            json!({
                "cells": ribbon.cells.iter().map(|cell| cell.map_or(Value::Null, |cell| json!({
                    "kind": cell.kind.name(),
                    "progress_running": cell.progress_running,
                }))).collect::<Vec<_>>(),
                "outcomes": ribbon.outcomes.iter().map(|outcome| outcome.map_or(Value::Null, |outcome| json!(outcome.name()))).collect::<Vec<_>>(),
                "has_data": ribbon.has_data(),
                "has_running_progress": ribbon.has_running_progress(),
                "kinds_present": ribbon.kinds_present().iter().map(|kind| kind.name()).collect::<Vec<_>>(),
            })
        }
        "scale-window" => scale(request["scale"].as_str().unwrap_or("all"))
            .window(&stats)
            .map_or(Value::Null, |(start, end)| json!([start, end])),
        _ => Value::Null,
    }
}
