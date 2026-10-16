//! Twins of ilium-client/src/ascii_chart.rs: terminal line-chart plotting and resampling.
//!
//! NaN samples travel as JSON null. The chart reply carries `lines` (right-trimmed), `raw_lines`, `rows`
//! (row count), `gutter` and `kinds` (the distinct cell kinds drawn: "line:<series>", "axis", "label").

use serde_json::{json, Value};

use crate::common::server;

fn plot(series: Value, config: Value) -> Value {
    server("chart.plot", json!({ "series": series, "config": config }))
}

fn resample(values: Value, width: u64, downsample: &str) -> Value {
    server("chart.resample", json!({ "values": values, "width": width, "downsample": downsample }))
}

fn lines(items: &[&str]) -> Value {
    json!(items)
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::a_custom_height_rescales_the_rows
#[test]
fn port_ascii_chart_a_custom_height_rescales_the_rows() {
    let reply = plot(json!([[2.,1.,1.,2.,-2.,5.,7.,11.,3.,7.,1.]]), json!({"height":4,"offset":3}));
    assert_eq!(reply["lines"], lines(&[
        " 11.00 ┤      ╭╮",
        "  7.75 ┤    ╭─╯│╭╮",
        "  4.50 ┼╮ ╭╮│  ╰╯│",
        "  1.25 ┤╰─╯││    ╰",
        " -2.00 ┤   ╰╯",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::a_target_width_interpolates_like_the_reference
#[test]
fn port_ascii_chart_a_target_width_interpolates_like_the_reference() {
    let reply = plot(json!([[0.3189989805,0.149949026,0.30142492354,0.195129182935,0.3142492354,0.1674974513,0.3142492354,0.1474974513,0.3047974513]]), json!({"width":30,"height":5}));
    assert_eq!(reply["lines"], lines(&[
        " 0.32 ┼╮            ╭─╮     ╭╮     ╭",
        " 0.29 ┤╰╮    ╭─╮   ╭╯ │    ╭╯│     │",
        " 0.26 ┤ │   ╭╯ ╰╮ ╭╯  ╰╮  ╭╯ ╰╮   ╭╯",
        " 0.23 ┤ ╰╮ ╭╯   ╰╮│    ╰╮╭╯   ╰╮ ╭╯",
        " 0.20 ┤  ╰╮│     ╰╯     ╰╯     │╭╯",
        " 0.16 ┤   ╰╯                   ╰╯",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::flat_series_draw_a_single_row
#[test]
fn port_ascii_chart_flat_series_draw_a_single_row() {
    let reply = plot(json!([[1.0,1.0,1.0,1.0,1.0]]), json!({}));
    assert_eq!(reply["lines"], lines(&[
        " 1.00 ┼────",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::flat_series_draw_a_single_row
#[test]
fn port_ascii_chart_flat_series_draw_a_single_row_zero() {
    let reply = plot(json!([[0.0,0.0,0.0,0.0,0.0]]), json!({}));
    assert_eq!(reply["lines"], lines(&[
        " 0.00 ┼────",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::fractional_ranges_use_more_decimals
#[test]
fn port_ascii_chart_fractional_ranges_use_more_decimals() {
    let reply = plot(json!([[0.2,0.1,0.2,2.,-0.9,0.7,0.91,0.3,0.7,0.4,0.5]]), json!({}));
    assert_eq!(reply["lines"], lines(&[
        "  2.00 ┤  ╭╮ ╭╮",
        "  0.55 ┼──╯│╭╯╰───",
        " -0.90 ┤   ╰╯",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::fractional_ranges_use_more_decimals
#[test]
fn port_ascii_chart_fractional_ranges_use_more_decimals_small() {
    let reply = plot(json!([[0.01,0.004,0.003,0.0042,0.0083,0.0033,0.0079]]), json!({}));
    assert_eq!(reply["lines"], lines(&[
        " 0.010 ┼╮",
        " 0.009 ┤│",
        " 0.008 ┤│  ╭╮╭",
        " 0.007 ┤│  │││",
        " 0.006 ┤│  │││",
        " 0.005 ┤│  │││",
        " 0.004 ┤╰╮╭╯││",
        " 0.003 ┤ ╰╯ ╰╯",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::large_values_drop_the_decimals
#[test]
fn port_ascii_chart_large_values_drop_the_decimals() {
    let reply = plot(json!([[192.,431.,112.,449.,-122.,375.,782.,123.,911.,1711.,172.]]), json!({"height":10}));
    assert_eq!(reply["lines"], lines(&[
        " 1711 ┤        ╭╮",
        " 1528 ┤        ││",
        " 1344 ┤        ││",
        " 1161 ┤        ││",
        "  978 ┤       ╭╯│",
        "  794 ┤     ╭╮│ │",
        "  611 ┤     │││ │",
        "  428 ┤╭╮╭╮╭╯││ │",
        "  245 ┼╯╰╯││ ╰╯ ╰",
        "   61 ┤   ││",
        " -122 ┤   ╰╯",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::matches_the_reference_for_a_mixed_series
#[test]
fn port_ascii_chart_matches_the_reference_for_a_mixed_series() {
    let reply = plot(json!([[2.,1.,1.,2.,-2.,5.,7.,11.,3.,7.,1.]]), json!({}));
    assert_eq!(reply["lines"], lines(&[
        " 11.00 ┤      ╭╮",
        " 10.00 ┤      ││",
        "  9.00 ┤      ││",
        "  8.00 ┤      ││",
        "  7.00 ┤     ╭╯│╭╮",
        "  6.00 ┤     │ │││",
        "  5.00 ┤    ╭╯ │││",
        "  4.00 ┤    │  │││",
        "  3.00 ┤    │  ╰╯│",
        "  2.00 ┼╮ ╭╮│    │",
        "  1.00 ┤╰─╯││    ╰",
        "  0.00 ┤   ││",
        " -1.00 ┤   ││",
        " -2.00 ┤   ╰╯",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::nan_values_leave_gaps
#[test]
fn port_ascii_chart_nan_values_leave_gaps() {
    let reply = plot(json!([[0.1,0.2,0.3,null,0.5,0.6,0.7,null,null,0.9,1.0]]), json!({}));
    assert_eq!(reply["lines"], lines(&[
        " 1.00 ┤         ╭",
        " 0.90 ┤        ─╯",
        " 0.80 ┤",
        " 0.70 ┤     ╭─",
        " 0.60 ┤    ╭╯",
        " 0.50 ┤   ─╯",
        " 0.40 ┤",
        " 0.30 ┤ ╭─",
        " 0.20 ┤╭╯",
        " 0.10 ┼╯",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::negative_only_series_keep_their_sign
#[test]
fn port_ascii_chart_negative_only_series_keep_their_sign() {
    let reply = plot(json!([[-5.,-2.,-3.,-4.,0.,-5.,-6.,-7.,-8.,0.,-9.,-3.,-5.,-2.,-9.,-3.,-1.]]), json!({}));
    assert_eq!(reply["lines"], lines(&[
        "  0.00 ┤   ╭╮   ╭╮",
        " -1.00 ┤   ││   ││     ╭",
        " -2.00 ┤╭╮ ││   ││  ╭╮ │",
        " -3.00 ┤│╰╮││   ││╭╮││╭╯",
        " -4.00 ┤│ ╰╯│   │││││││",
        " -5.00 ┼╯   ╰╮  │││╰╯││",
        " -6.00 ┤     ╰╮ │││  ││",
        " -7.00 ┤      ╰╮│││  ││",
        " -8.00 ┤       ╰╯││  ││",
        " -9.00 ┤         ╰╯  ╰╯",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::several_series_share_one_grid_and_report_their_index
#[test]
fn port_ascii_chart_several_series_share_one_grid_and_report_their_index() {
    let reply = plot(json!([[0.,1.,2.,3.,3.,3.,2.,0.],[5.,4.,2.,1.,4.,6.,6.]]), json!({}));
    assert_eq!(reply["lines"], lines(&[
        " 6.00 ┤    ╭─",
        " 5.00 ┼╮   │",
        " 4.00 ┤╰╮ ╭╯",
        " 3.00 ┤ │╭│─╮",
        " 2.00 ┤ ╰╮│ ╰╮",
        " 1.00 ┤╭╯╰╯  │",
        " 0.00 ┼╯     ╰",
    ]));
    let kinds = reply["kinds"].as_array().cloned().unwrap_or_default();
    for expected in ["line:0", "line:1", "axis", "label"] {
        assert!(kinds.contains(&json!(expected)), "{kinds:?}");
    }
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::short_series_are_padded_with_gaps_when_a_width_is_given
#[test]
fn port_ascii_chart_short_series_are_padded_with_gaps_when_a_width_is_given() {
    let reply = plot(json!([[0.,0.,2.,2.,null],[1.,1.,1.,1.,1.,1.,1.],[null,null,null,0.,0.,2.,2.]]), json!({}));
    assert_eq!(reply["lines"], lines(&[
        " 2.00 ┤ ╭──╭─",
        " 1.00 ┼────│─",
        " 0.00 ┼─╯──╯",
    ]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::a_label_formatter_replaces_the_number_format
#[test]
fn port_ascii_chart_a_label_formatter_replaces_the_number_format() {
    let reply = plot(
        json!([[1000., 3000., 5000.]]),
        json!({ "height": 2, "label_formatter": { "divide_by": 1000.0, "decimals": 0, "suffix": "k" } }),
    );
    let lines = reply["lines"].as_array().cloned().unwrap_or_default();
    assert!(lines[0].as_str().unwrap_or("").contains("5k"), "{lines:?}");
    assert!(lines[2].as_str().unwrap_or("").contains("1k"), "{lines:?}");
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::empty_or_all_nan_input_draws_nothing
#[test]
fn port_ascii_chart_empty_or_all_nan_input_draws_nothing() {
    for series in [json!([]), json!([[]]), json!([[null, null, null, null]])] {
        let reply = plot(series, json!({}));
        assert_eq!(reply["rows"], json!(0), "{reply}");
    }
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::gutter_counts_the_label_and_axis_columns
#[test]
fn port_ascii_chart_gutter_counts_the_label_and_axis_columns() {
    let reply = plot(json!([[1., 2., 3.]]), json!({}));
    // " 3.00 ┤" is 7 display columns; the first data column follows it.
    assert_eq!(reply["gutter"], json!(7));
    assert_eq!(reply["axis_char"], json!("┤"));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::include_zero_extends_the_axis_to_zero
#[test]
fn port_ascii_chart_include_zero_extends_the_axis_to_zero() {
    let reply = plot(json!([[100., 110., 120.]]), json!({ "height": 4, "include_zero": true }));
    let lines = reply["raw_lines"].as_array().cloned().unwrap_or_default();
    assert!(lines.last().and_then(Value::as_str).unwrap_or("").contains('0'), "{lines:?}");
    assert_eq!(lines.len(), 5);
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::max_downsampling_keeps_a_spike_that_interpolation_can_miss
#[test]
fn port_ascii_chart_max_downsampling_keeps_a_spike_that_interpolation_can_miss() {
    let mut values = vec![0.0; 100];
    values[51] = 50.0;
    let peak = |reply: &Value| reply.as_array().cloned().unwrap_or_default().iter().filter_map(Value::as_f64).fold(0.0, f64::max);
    let interpolated = resample(json!(values), 10, "interpolate");
    let maxed = resample(json!(values), 10, "max");
    assert!(peak(&maxed) >= 50.0);
    assert!(peak(&interpolated) < 50.0);
    assert_eq!(maxed.as_array().map(Vec::len), Some(10));
    assert_eq!(resample(json!([2.0, 4.0, 6.0, 8.0]), 2, "mean"), json!([3.0, 7.0]));
}

/// REQ: PORT:ilium-client/src/ascii_chart.rs::stretching_keeps_the_endpoints
#[test]
fn port_ascii_chart_stretching_keeps_the_endpoints() {
    assert_eq!(resample(json!([1.0, 5.0]), 5, "interpolate"), json!([1.0, 2.0, 3.0, 4.0, 5.0]));
    assert_eq!(resample(json!([7.0]), 3, "max"), json!([7.0, 7.0, 7.0]));
}
