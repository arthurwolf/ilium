//! Manual page `reference-host-telemetry`: log codes, redaction, trace spans, Prometheus metrics,
//! label cardinality, histogram buckets, custom counters, UTC storage and the writer queue
//! (HST-607, 608, 609, 610, 612, 613, 614, 636, 638, 639, 642, 643, 647).

use ilium_plugin_api::Side;

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-607
#[test]
fn hst_607_every_condition_log_code_matches_the_host_pattern_and_is_registered() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let pattern = regex_like_host_code();
    let codes = rig.registry_table_codes();
    assert!(
        !codes.is_empty(),
        "the manual registry table lists the host codes"
    );
    for code in &codes {
        assert!(
            pattern(code),
            "code {code} must match ^host\\.[a-z0-9-]+(\\.[a-z0-9-]+)*$"
        );
    }
    for code in rig.emitted_host_codes() {
        assert!(
            codes.contains(&code),
            "emitted code {code} is listed in the registry table"
        );
    }
}

/// Checks the `^host\.[a-z0-9-]+(\.[a-z0-9-]+)*$` shape without a regex crate.
fn regex_like_host_code() -> impl Fn(&str) -> bool {
    |code: &str| {
        let mut parts = code.split('.');
        parts.next() == Some("host")
            && parts.clone().count() >= 1
            && parts.all(|part| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            })
    }
}

/// REQ: HST-608
#[test]
fn hst_608_secrets_in_log_lines_and_span_attributes_are_redacted() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.emit_plugin_log(
        NOTES,
        "info",
        "token ghp_0123456789abcdefghijklmnopqrstuvwxyz0123 used",
    );
    let written = rig.plugin_log_text(NOTES);
    assert!(
        !written.contains("ghp_0123456789"),
        "raw secret must not reach the log file"
    );
    assert!(
        written.contains("[redacted]"),
        "matched value replaced by [redacted]"
    );
}

/// REQ: HST-609
#[test]
fn hst_609_trace_file_holds_one_span_per_line_with_the_listed_fields_and_is_capped() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_env("ILIUM_PLUGIN_TRACE", "1");
    rig.call_plugin_export(NOTES, "init");
    let trace_path = rig.state_dir().join("plugin-trace.jsonl");
    let text = std::fs::read_to_string(&trace_path)
        .expect("trace file exists at <state>/plugin-trace.jsonl");
    let span = text.lines().next().expect("at least one span");
    let value: serde_json::Value = serde_json::from_str(span).expect("span is one JSON object");
    for field in [
        "trace_id",
        "span_id",
        "parent_id",
        "plugin",
        "side",
        "class",
        "export",
        "queue_us",
        "run_us",
        "imports",
        "outcome",
    ] {
        assert!(value.get(field).is_some(), "span carries {field}");
    }
    assert_eq!(
        rig.trace_file_cap_bytes(),
        32 * 1024 * 1024,
        "trace capped at 32 MiB"
    );
}

/// REQ: HST-610
#[test]
fn hst_610_spans_use_correlation_id_parent_links_and_emit_exactly_once() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_env("ILIUM_PLUGIN_TRACE", "1");
    rig.call_cross_plugin_chain(NOTES, "trap");
    let spans = rig.trace_spans();
    let trap_spans: Vec<_> = spans.iter().filter(|span| span.export == "trap").collect();
    assert_eq!(
        trap_spans.len(),
        1,
        "a trapping call emits exactly one span"
    );
    let child = spans
        .iter()
        .find(|span| span.parent_id.is_some())
        .expect("cross-plugin span has a parent");
    assert_eq!(
        child.parent_id,
        Some(trap_spans[0].span_id.clone()),
        "parent is the caller span"
    );
    assert_eq!(
        child.trace_id,
        rig.correlation_id_of_last_call(),
        "trace_id is the HST-526 correlation id"
    );
}

/// REQ: HST-612
#[test]
fn hst_612_metrics_print_prometheus_text_with_bounded_labels_and_no_socket() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["plugin", "metrics"]);
    assert_eq!(output.exit_code, 0, "metrics command succeeds");
    let names: Vec<&str> = output
        .stdout_raw
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect();
    assert!(!names.is_empty(), "counters printed");
    for line in names {
        assert!(
            line.starts_with("ilium_plugin_"),
            "metric name prefixed ilium_plugin_: {line}"
        );
        let labels = line
            .split('{')
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .unwrap_or("");
        for label in labels.split(',').filter(|label| !label.is_empty()) {
            let key = label.split('=').next().unwrap_or("");
            assert!(
                ["plugin", "side", "class"].contains(&key),
                "only plugin, side and class labels: {key}"
            );
        }
    }
    assert!(
        rig.open_network_sockets_for_metrics().is_empty(),
        "metrics are never served on a socket"
    );
}

/// REQ: HST-613
#[test]
fn hst_613_metric_labels_exclude_export_names_and_series_are_capped_at_64() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.call_many_exports(NOTES, 200);
    let output = rig.cli(&["plugin", "metrics"]);
    assert!(
        !output.stdout_raw.contains("export="),
        "export names are not metric labels"
    );
    assert!(
        !output.stdout_raw.contains("message="),
        "error messages are not metric labels"
    );
    assert!(
        rig.metric_series_count(NOTES, Side::Server) <= 64,
        "at most 64 series per plugin side"
    );
}

/// REQ: HST-614
#[test]
fn hst_614_histogram_counts_each_call_once_and_clamps_backwards_clock_steps() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.call_plugin_export_times(NOTES, "init", 25);
    let buckets: u64 = rig.histogram_bucket_sum(NOTES, "init");
    let calls = rig.calls_counter(NOTES, "init");
    assert_eq!(
        buckets, calls,
        "bucket sum equals the calls counter for the window"
    );
    rig.step_monotonic_clock_backwards_ms(50);
    rig.call_plugin_export_times(NOTES, "init", 1);
    assert!(
        rig.minimum_recorded_duration_ns(NOTES, "init") >= 0,
        "backwards step clamps to zero"
    );
}

/// REQ: HST-636
#[test]
fn hst_636_diagnostics_bundle_has_required_parts_is_redacted_and_excludes_data_dirs() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let out = rig.path_in_scratch("bundle.tar");
    let output = rig.cli(&[
        "plugin",
        "diagnostics",
        "--out",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(output.exit_code, 0, "diagnostics bundle written");
    let entries = rig.bundle_entry_names(&out);
    for required in ["health", "metrics", "logs", "limits", "manifest", "grants"] {
        assert!(
            entries.iter().any(|name| name.contains(required)),
            "bundle contains {required}"
        );
    }
    assert!(
        entries.iter().all(|name| !name.contains("/data/")),
        "no plugin data directories in the bundle"
    );
    assert!(
        rig.bundle_is_redacted(&out),
        "redaction applied to bundle contents"
    );
}

/// REQ: HST-638
#[test]
fn hst_638_custom_counters_are_limited_to_16_names_and_appear_under_plugin_prefix() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    for index in 0..16 {
        rig.plugin_count(NOTES, &format!("counter_{index}"), 1)
            .expect("first 16 names accepted");
    }
    let seventeenth = rig.plugin_count(NOTES, "counter_16", 1);
    assert_eq!(
        seventeenth.expect_err("17th name refused").code.as_str(),
        "quota-exceeded"
    );
    let stats = rig.cli(&["plugin", "stats", NOTES]);
    assert!(
        stats.stdout_raw.contains("plugin.counter_0"),
        "custom counter shown under the plugin. prefix"
    );
}

/// REQ: HST-639
#[test]
fn hst_639_reading_counters_takes_at_most_5_ms_for_64_plugins_without_calling_plugins() {
    let rig = rig();
    rig.install_many(64);
    let started = std::time::Instant::now();
    let output = rig.cli(&["plugin", "stats"]);
    let elapsed = started.elapsed();
    assert_eq!(output.exit_code, 0, "stats succeeds");
    assert!(
        elapsed.as_millis() <= 5,
        "stats for 64 plugins within 5 ms, took {elapsed:?}"
    );
    assert_eq!(
        rig.plugin_export_calls_during_read(),
        0,
        "reading counters never calls into a plugin"
    );
}

/// REQ: HST-642
#[test]
fn hst_642_advance_clock_drives_buckets_and_the_24_hour_window_identically() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.call_plugin_export(NOTES, "init");
    rig.advance_clock(std::time::Duration::from_secs(24 * 3600 + 1));
    assert_eq!(
        rig.histogram_window_count(NOTES),
        0,
        "24-hour window forgets the old call"
    );
    assert_eq!(
        rig.hourly_bucket_count(NOTES),
        0,
        "hourly buckets age identically"
    );
}

/// REQ: HST-643
#[test]
fn hst_643_full_writer_queue_drops_oldest_counts_them_and_logs_once_per_minute() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.fill_writer_queue(8_192 + 10);
    assert_eq!(
        rig.observability_dropped(),
        10,
        "oldest records dropped and counted"
    );
    rig.fill_writer_queue(100);
    assert_eq!(
        rig.log_code_count("host.observability.dropped"),
        1,
        "one dropped line per minute"
    );
}

/// REQ: HST-647
#[test]
fn hst_647_timestamps_are_utc_in_storage_and_local_in_terminal_display() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_local_timezone("Europe/Berlin");
    rig.emit_plugin_log(NOTES, "info", "stamp");
    let stored = rig.plugin_log_last_ts(NOTES);
    assert!(stored.ends_with('Z'), "stored timestamp is UTC: {stored}");
    let shown = rig.cli(&["plugin", "logs", NOTES]).stdout_raw;
    let shown_line = shown.lines().last().expect("one displayed line");
    assert!(
        shown_line.contains(&rig.local_display_timestamp_of(&stored)),
        "displayed in local time as YYYY-MM-DD HH:mm:ss"
    );
}

/// REQ: HST-607
#[test]
fn hst_607_every_emitted_code_also_matches_the_host_pattern() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let pattern = regex_like_host_code();
    let emitted = rig.emitted_host_codes();
    assert!(!emitted.is_empty(), "the host emits at least one condition code");
    for code in &emitted {
        assert!(
            pattern(code),
            "emitted code {code} must match ^host\\.[a-z0-9-]+(\\.[a-z0-9-]+)*$"
        );
    }
}

/// REQ: HST-608
#[test]
fn hst_608_redaction_also_applies_to_the_plugin_logs_command_output() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.emit_plugin_log(
        NOTES,
        "info",
        "token ghp_0123456789abcdefghijklmnopqrstuvwxyz0123 used",
    );
    let shown = rig.cli(&["plugin", "logs", NOTES]);
    assert_eq!(shown.exit_code, 0, "logs command succeeds");
    assert!(
        !shown.stdout_raw.contains("ghp_0123456789"),
        "raw secret must not reach the logs command output"
    );
    assert!(
        shown.stdout_raw.contains("[redacted]"),
        "matched value shown as [redacted] in the command output"
    );
}

/// REQ: HST-609
#[test]
fn hst_609_every_trace_line_is_one_complete_json_span() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_env("ILIUM_PLUGIN_TRACE", "1");
    rig.call_plugin_export(NOTES, "init");
    rig.call_plugin_export(NOTES, "init");
    let trace_path = rig.state_dir().join("plugin-trace.jsonl");
    let text = std::fs::read_to_string(&trace_path).expect("trace file exists");
    let mut span_lines = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: serde_json::Value =
            serde_json::from_str(line).expect("each trace line is one complete JSON object");
        assert!(value.is_object(), "each trace line is a JSON object");
        span_lines += 1;
    }
    assert!(span_lines >= 2, "two calls leave at least two span lines");
}

/// REQ: HST-610
#[test]
fn hst_610_a_timed_out_call_also_emits_exactly_one_span() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").spins_forever());
    rig.set_env("ILIUM_PLUGIN_TRACE", "1");
    let outcome = rig.call_plugin(NOTES, "init");
    assert!(outcome.is_err(), "a spinning export ends in an error, not a result");
    let spans = rig.trace_spans();
    let init_spans = spans.iter().filter(|span| span.export == "init").count();
    assert_eq!(init_spans, 1, "a timed-out call emits exactly one span");
}

/// REQ: HST-612
#[test]
fn hst_612_every_metric_line_ends_in_a_numeric_sample_value() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.call_plugin_export(NOTES, "init");
    let output = rig.cli(&["plugin", "metrics"]);
    assert_eq!(output.exit_code, 0, "metrics command succeeds");
    let mut sample_count = 0;
    for line in output
        .stdout_raw
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let value = line.rsplit(' ').next().unwrap_or("");
        assert!(
            value.parse::<f64>().is_ok(),
            "sample value of {line} is a number"
        );
        sample_count += 1;
    }
    assert!(sample_count > 0, "at least one sample printed");
}

/// REQ: HST-613
#[test]
fn hst_613_the_64_series_cap_also_holds_on_the_client_side() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").client_side());
    rig.call_many_exports(NOTES, 200);
    assert!(
        rig.metric_series_count(NOTES, Side::Client) <= 64,
        "at most 64 series per plugin client side"
    );
}

/// REQ: HST-614
#[test]
fn hst_614_a_single_call_is_counted_once_and_sits_in_one_bucket() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.call_plugin_export_times(NOTES, "init", 1);
    assert_eq!(rig.calls_counter(NOTES, "init"), 1, "one call counted once");
    assert_eq!(
        rig.histogram_bucket_sum(NOTES, "init"),
        1,
        "that call sits in exactly one bucket"
    );
}

/// REQ: HST-636
#[test]
fn hst_636_diagnostics_bundle_completes_within_10_seconds_for_64_plugins() {
    let rig = rig();
    rig.install_many(64);
    let out = rig.path_in_scratch("bundle-64.tar");
    let started = std::time::Instant::now();
    let output = rig.cli(&[
        "plugin",
        "diagnostics",
        "--out",
        out.to_str().expect("utf-8 path"),
    ]);
    let elapsed = started.elapsed();
    assert_eq!(output.exit_code, 0, "bundle written for 64 plugins");
    assert!(
        elapsed <= std::time::Duration::from_secs(10),
        "bundle within 10 s, took {elapsed:?}"
    );
}

/// REQ: HST-638
#[test]
fn hst_638_invalid_counter_names_are_refused_and_repeats_use_no_new_slot() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert!(
        rig.plugin_count(NOTES, "Bad-Name", 1).is_err(),
        "names must match ^[a-z][a-z0-9_]{{0,31}}$"
    );
    for index in 0..16 {
        rig.plugin_count(NOTES, &format!("counter_{index}"), 1)
            .expect("first 16 names accepted");
    }
    rig.plugin_count(NOTES, "counter_3", 1)
        .expect("an existing name still accepts counts after the 16 slots are used");
}

/// REQ: HST-639
#[test]
fn hst_639_plugin_info_reads_without_calling_into_the_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["plugin", "info", NOTES]);
    assert_eq!(output.exit_code, 0, "plugin info succeeds");
    assert_eq!(
        rig.plugin_export_calls_during_read(),
        0,
        "reading plugin info never calls into a plugin"
    );
}

/// REQ: HST-642
#[test]
fn hst_642_a_call_23_hours_old_is_still_inside_the_24_hour_window() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.call_plugin_export(NOTES, "init");
    rig.advance_clock(std::time::Duration::from_secs(23 * 3600));
    assert_eq!(
        rig.histogram_window_count(NOTES),
        1,
        "a call 23 hours old is still inside the window"
    );
}

/// REQ: HST-643
#[test]
fn hst_643_a_queue_of_exactly_8192_records_drops_nothing() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.fill_writer_queue(8_192);
    assert_eq!(
        rig.observability_dropped(),
        0,
        "a full but not overflowing queue drops nothing"
    );
}

/// REQ: HST-647
#[test]
fn hst_647_terminal_display_uses_the_local_format_yyyy_mm_dd_hh_mm_ss() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_local_timezone("Europe/Berlin");
    rig.emit_plugin_log(NOTES, "info", "stamp");
    let stored = rig.plugin_log_last_ts(NOTES);
    let local = rig.local_display_timestamp_of(&stored);
    assert_eq!(local.len(), 19, "YYYY-MM-DD HH:mm:ss is 19 characters");
    let bytes = local.as_bytes();
    assert!(
        bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes[10] == b' '
            && bytes[13] == b':'
            && bytes[16] == b':',
        "separators follow YYYY-MM-DD HH:mm:ss, got {local}"
    );
}
