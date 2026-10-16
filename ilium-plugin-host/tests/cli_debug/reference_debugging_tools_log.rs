//! reference-debugging-tools.md, "The log": line format, thresholds, rate limit, rotation,
//! retention and `ilium plugin logs` (DEV-650..667).

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use ilium_plugin_api::{Side, Tier};
use ilium_plugin_host::telemetry::{
    disabled_log, format_local_time, format_log_line, mark_reinstalled, mark_uninstalled,
    plugin_log_dir, purge_expired_logs, resolve_threshold, restart_log, truncate_message,
    LogConfig, LogLevel, LogSide, PluginLog, LOG_MESSAGE_MAX_BYTES, LOG_ROTATE_BYTES,
};
use proptest::prelude::*;

use crate::common::{
    env, error_code, log_line, record_types, records_of, run, scratch, write_log,
};

const PLUGIN: &str = "example.notes";
const AT: &str = "2030-01-01 09:14:02";

fn file_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).map(|text| text.lines().map(str::to_owned).collect()).unwrap_or_default()
}

fn open_log(dir: &Path, config: LogConfig) -> PluginLog {
    PluginLog::open(dir, PLUGIN, config).expect("open plugin log")
}

// ---------------------------------------------------------------- DEV-650

/// REQ: DEV-650
#[test]
fn dev_650_line_matches_the_documented_example() {
    let line = format_log_line(AT, LogLevel::Info, LogSide::Server, PLUGIN, "activate: ready");
    assert_eq!(line, "2030-01-01 09:14:02 info  server example.notes activate: ready");
}

/// REQ: DEV-650
#[test]
fn dev_650_host_line_uses_the_side_host() {
    let line = format_log_line(
        "2030-01-01 09:15:40",
        LogLevel::Error,
        LogSide::Host,
        PLUGIN,
        "restarting in 1 s (failure 1 of 3 in 10 minutes)",
    );
    assert_eq!(
        line,
        "2030-01-01 09:15:40 error host   example.notes restarting in 1 s (failure 1 of 3 in 10 minutes)"
    );
}

/// REQ: DEV-650
#[test]
fn dev_650_level_is_padded_to_5_and_side_to_6() {
    let warn = format_log_line(AT, LogLevel::Warn, LogSide::Scene, PLUGIN, "m");
    assert_eq!(&warn[20..25], "warn ");
    assert_eq!(&warn[26..32], "scene ");
    let trace = format_log_line(AT, LogLevel::Trace, LogSide::Client, PLUGIN, "m");
    assert_eq!(&trace[20..25], "trace");
    assert_eq!(&trace[26..32], "client");
}

/// REQ: DEV-650
#[test]
fn dev_650_newline_in_message_is_written_as_backslash_n() {
    let line = format_log_line(AT, LogLevel::Info, LogSide::Server, PLUGIN, "a\nb\nc");
    assert!(!line.contains('\n'));
    assert!(line.ends_with("a\\nb\\nc"));
}

proptest! {
    // REQ: DEV-650
    #[test]
    fn dev_650_every_line_has_the_fixed_columns(message in "[ -~\n]{0,60}", level in 0usize..5, side in 0usize..4) {
        let level = LogLevel::ALL[level];
        let side = [LogSide::Server, LogSide::Client, LogSide::Scene, LogSide::Host][side];
        let line = format_log_line(AT, level, side, PLUGIN, &message);
        prop_assert!(line.starts_with(AT));
        prop_assert!(!line.contains('\n'));
        prop_assert_eq!(&line[20..25], format!("{:<5}", level.as_str()));
        prop_assert_eq!(&line[26..32], format!("{:<6}", side.as_str()));
        prop_assert_eq!(&line[33..33 + PLUGIN.len()], PLUGIN);
        prop_assert!(line.ends_with(&message.replace('\n', "\\n")));
    }
}

/// REQ: DEV-650
#[test]
fn dev_650_host_write_stores_a_line_with_side_host() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    assert!(log.write_host(AT, 0, LogLevel::Warn, "events dropped"));
    let lines = file_lines(&scratch.path().join(format!("{PLUGIN}.log")));
    assert_eq!(lines, vec![format!("{AT} warn  host   {PLUGIN} events dropped")]);
}

// ---------------------------------------------------------------- DEV-651

/// REQ: DEV-651
#[test]
fn dev_651_limit_constant_is_16_kib() {
    assert_eq!(LOG_MESSAGE_MAX_BYTES, 16_384);
}

/// REQ: DEV-651
#[test]
fn dev_651_message_of_exactly_16384_bytes_is_unchanged() {
    let message = "a".repeat(16_384);
    assert_eq!(truncate_message(&message), message);
}

/// REQ: DEV-651
#[test]
fn dev_651_message_of_16385_bytes_is_cut_and_marked() {
    let message = "a".repeat(16_385);
    let cut = truncate_message(&message);
    assert!(cut.ends_with("…[truncated]"));
    assert!(cut.len() <= 16_384, "cut message is {} bytes", cut.len());
    assert_eq!(cut.len(), 16_384, "the cut keeps as many bytes as the limit allows");
}

/// REQ: DEV-651
#[test]
fn dev_651_cut_falls_on_a_whole_utf8_character() {
    let message = format!("{}{}", "a".repeat(16_368), "€".repeat(10));
    let cut = truncate_message(&message);
    assert!(cut.ends_with("…[truncated]"));
    let body = cut.strip_suffix("…[truncated]").expect("suffix");
    assert_eq!(body, "a".repeat(16_368));
    assert!(cut.len() <= 16_384);
}

/// REQ: DEV-651
#[test]
fn dev_651_cut_keeps_whole_multibyte_characters_when_they_fit() {
    let message = "€".repeat(6_000);
    let cut = truncate_message(&message);
    let body = cut.strip_suffix("…[truncated]").expect("suffix");
    assert!(body.chars().all(|c| c == '€'));
    assert!(cut.len() <= 16_384);
    assert!(cut.len() + 3 > 16_384, "one more character would not fit");
}

proptest! {
    // REQ: DEV-651
    #[test]
    fn dev_651_truncated_message_never_exceeds_the_limit(extra in 0usize..2_000, ch in prop::sample::select(vec!['a', 'é', '€', '😀'])) {
        let message: String = std::iter::repeat(ch).take(16_384 / ch.len_utf8() + extra).collect();
        let cut = truncate_message(&message);
        prop_assert!(cut.len() <= 16_384);
        if message.len() > 16_384 {
            prop_assert!(cut.ends_with("…[truncated]"));
        } else {
            prop_assert_eq!(cut, message);
        }
    }
}

// ---------------------------------------------------------------- DEV-652

/// REQ: DEV-652
#[test]
fn dev_652_levels_are_ordered_trace_debug_info_warn_error() {
    let names: Vec<&str> = LogLevel::ALL.iter().map(|level| level.as_str()).collect();
    assert_eq!(names, ["trace", "debug", "info", "warn", "error"]);
    for pair in LogLevel::ALL.windows(2) {
        assert!(pair[0] < pair[1]);
    }
}

/// REQ: DEV-652
#[test]
fn dev_652_line_is_written_when_level_is_at_or_above_the_threshold() {
    for threshold in LogLevel::ALL {
        for level in LogLevel::ALL {
            let scratch = scratch();
            let config = LogConfig { threshold, ..LogConfig::default() };
            let mut log = open_log(scratch.path(), config);
            let stored = log.write_plugin(AT, 0, Side::Server, level, "m");
            let lines = file_lines(&scratch.path().join(format!("{PLUGIN}.log")));
            assert_eq!(stored, level >= threshold, "threshold {threshold:?} level {level:?}");
            assert_eq!(lines.len(), usize::from(level >= threshold));
        }
    }
}

// ---------------------------------------------------------------- DEV-653

/// REQ: DEV-653
#[test]
fn dev_653_default_threshold_is_info() {
    assert_eq!(LogConfig::default().threshold, LogLevel::Info);
}

/// REQ: DEV-653
#[test]
fn dev_653_dev_install_defaults_to_debug() {
    assert_eq!(LogConfig::for_dev_install().threshold, LogLevel::Debug);
}

/// REQ: DEV-653
#[test]
fn dev_653_debug_and_trace_lines_at_info_are_dropped_not_stored() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    assert!(!log.write_plugin(AT, 0, Side::Server, LogLevel::Debug, "d"));
    assert!(!log.write_plugin(AT, 0, Side::Server, LogLevel::Trace, "t"));
    assert!(log.write_plugin(AT, 0, Side::Server, LogLevel::Info, "i"));
    let lines = file_lines(&scratch.path().join(format!("{PLUGIN}.log")));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].ends_with(" i"));
}

/// REQ: DEV-653
#[test]
fn dev_653_dropped_lines_do_not_count_against_the_rate_budget() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    for _ in 0..500 {
        log.write_plugin(AT, 0, Side::Server, LogLevel::Debug, "d");
    }
    assert!(log.write_plugin(AT, 0, Side::Server, LogLevel::Info, "i"));
}

// ---------------------------------------------------------------- DEV-654

/// REQ: DEV-654
#[test]
fn dev_654_environment_variable_sets_the_threshold() {
    assert_eq!(resolve_threshold(None, Some("warn"), false), LogLevel::Warn);
}

/// REQ: DEV-654
#[test]
fn dev_654_command_sets_the_threshold() {
    assert_eq!(resolve_threshold(Some(LogLevel::Error), None, false), LogLevel::Error);
}

/// REQ: DEV-654
#[test]
fn dev_654_command_wins_over_the_environment_variable() {
    assert_eq!(resolve_threshold(Some(LogLevel::Debug), Some("error"), false), LogLevel::Debug);
}

/// REQ: DEV-654
#[test]
fn dev_654_environment_variable_wins_over_the_dev_default() {
    assert_eq!(resolve_threshold(None, Some("error"), true), LogLevel::Error);
}

/// REQ: DEV-654, DEV-653
#[test]
fn dev_654_without_command_or_variable_the_default_applies() {
    assert_eq!(resolve_threshold(None, None, false), LogLevel::Info);
    assert_eq!(resolve_threshold(None, None, true), LogLevel::Debug);
}

// ---------------------------------------------------------------- DEV-655

/// REQ: DEV-655
#[test]
fn dev_655_restart_message_is_exact_at_level_warn() {
    let (level, message) = restart_log(1, 1, 3);
    assert_eq!(level, LogLevel::Warn);
    assert_eq!(message, "restarting in 1 s (failure 1 of 3 in 10 minutes)");
}

/// REQ: DEV-655
#[test]
fn dev_655_restart_message_carries_the_delay_and_failure_count_of_that_restart() {
    let (_, message) = restart_log(4, 2, 3);
    assert_eq!(message, "restarting in 4 s (failure 2 of 3 in 10 minutes)");
}

/// REQ: DEV-655
#[test]
fn dev_655_last_failure_logs_disabled_after_3_failures() {
    let (_, message) = disabled_log(3);
    assert_eq!(message, "disabled after 3 failures in 10 minutes");
}

// ---------------------------------------------------------------- DEV-656

fn stored_count(dir: &Path) -> usize {
    file_lines(&dir.join(format!("{PLUGIN}.log"))).len()
}

/// REQ: DEV-656
#[test]
fn dev_656_ordinary_plugin_stores_200_lines_in_one_second() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    let stored = (0..250).filter(|n| log.write_plugin(AT, 100 + n, Side::Server, LogLevel::Info, "m")).count();
    assert_eq!(stored, 200);
    assert_eq!(stored_count(scratch.path()), 200);
}

/// REQ: DEV-656
#[test]
fn dev_656_line_201_is_dropped_and_line_200_is_kept() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    for n in 0..199 {
        assert!(log.write_plugin(AT, n, Side::Server, LogLevel::Info, "m"));
    }
    assert!(log.write_plugin(AT, 500, Side::Server, LogLevel::Info, "200th"));
    assert!(!log.write_plugin(AT, 501, Side::Server, LogLevel::Info, "201st"));
}

/// REQ: DEV-656
#[test]
fn dev_656_trusted_t1_plugin_gets_1000_lines_per_second() {
    assert_eq!(LogConfig::for_tier(Tier::T1).rate_per_second, 1_000);
    assert_eq!(LogConfig::for_tier(Tier::T2).rate_per_second, 200);
    assert_eq!(LogConfig::for_tier(Tier::T3).rate_per_second, 200);
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::for_tier(Tier::T1));
    let stored = (0..1_100).filter(|n| log.write_plugin(AT, n % 900, Side::Server, LogLevel::Info, "m")).count();
    assert_eq!(stored, 1_000);
}

/// REQ: DEV-656
#[test]
fn dev_656_dropped_lines_are_summarised_once_per_second() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    for n in 0..250 {
        log.write_plugin(AT, 100 + n, Side::Server, LogLevel::Info, "m");
    }
    log.tick(AT, 1_100);
    log.tick(AT, 1_200);
    let lines = file_lines(&scratch.path().join(format!("{PLUGIN}.log")));
    let summaries: Vec<&String> = lines.iter().filter(|line| line.contains("lines dropped")).collect();
    assert_eq!(summaries.len(), 1, "exactly one summary line");
    assert_eq!(summaries[0], &format!("{AT} warn  host   {PLUGIN} 50 lines dropped in the last second"));
}

/// REQ: DEV-656
#[test]
fn dev_656_no_summary_line_when_nothing_was_dropped() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    for n in 0..10 {
        log.write_plugin(AT, 100 + n, Side::Server, LogLevel::Info, "m");
    }
    log.tick(AT, 1_100);
    assert_eq!(stored_count(scratch.path()), 10);
}

/// REQ: DEV-656
#[test]
fn dev_656_summary_line_does_not_use_the_rate_budget() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    for n in 0..250 {
        log.write_plugin(AT, 100 + n % 800, Side::Server, LogLevel::Info, "m");
    }
    log.tick(AT, 1_100);
    let stored = (0..200).filter(|n| log.write_plugin(AT, 1_100 + n, Side::Server, LogLevel::Info, "next")).count();
    assert_eq!(stored, 200, "a full budget remains after the summary line");
}

// ---------------------------------------------------------------- DEV-657

/// REQ: DEV-657
#[test]
fn dev_657_full_queue_writes_one_error_line_with_the_count() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    log.queue_full(AT, 0, 5);
    let lines = file_lines(&scratch.path().join(format!("{PLUGIN}.log")));
    assert_eq!(lines, vec![format!("{AT} error host   {PLUGIN} event queue full: dropped 5 events")]);
}

/// REQ: DEV-657
#[test]
fn dev_657_message_carries_the_given_drop_count() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    log.queue_full(AT, 0, 1_234);
    let lines = file_lines(&scratch.path().join(format!("{PLUGIN}.log")));
    assert!(lines[0].ends_with("event queue full: dropped 1234 events"), "{}", lines[0]);
}

// ---------------------------------------------------------------- DEV-658

/// REQ: DEV-658
#[test]
fn dev_658_plugin_lines_never_carry_the_side_host() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    for side in Side::ALL {
        log.write_plugin(AT, 0, side, LogLevel::Info, "hello");
    }
    for line in file_lines(&scratch.path().join(format!("{PLUGIN}.log"))) {
        assert_ne!(&line[26..32], "host  ", "plugin wrote a host line: {line}");
    }
}

/// REQ: DEV-658
#[test]
fn dev_658_message_that_imitates_a_host_line_stays_on_the_plugin_side() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    log.write_plugin(AT, 0, Side::Client, LogLevel::Info, "host   fake");
    let lines = file_lines(&scratch.path().join(format!("{PLUGIN}.log")));
    assert_eq!(&lines[0][26..32], "client");
}

// ---------------------------------------------------------------- DEV-660

fn rotation_message(n: usize) -> String {
    format!("{n:05}{}", "x".repeat(16_000))
}

fn first_number(path: &Path) -> Option<usize> {
    file_lines(path).first().and_then(|line| line.split_whitespace().nth(5)).and_then(|m| m[..5].parse().ok())
}

fn last_number(path: &Path) -> Option<usize> {
    file_lines(path).last().and_then(|line| line.split_whitespace().nth(5)).and_then(|m| m[..5].parse().ok())
}

/// REQ: DEV-660
#[test]
fn dev_660_rotation_size_is_4_mib() {
    assert_eq!(LOG_ROTATE_BYTES, 4 * 1024 * 1024);
}

/// REQ: DEV-660
#[test]
fn dev_660_active_file_rotates_to_log_1_when_it_reaches_4_mib() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    for n in 0..300 {
        log.write_plugin(AT, n as u64 * 1_000, Side::Server, LogLevel::Info, &rotation_message(n));
    }
    let rotated = scratch.path().join(format!("{PLUGIN}.log.1"));
    let size = std::fs::metadata(&rotated).expect("log.1 exists after rotation").len();
    assert!(size >= LOG_ROTATE_BYTES, "rotated file is {size} bytes");
    let active = std::fs::metadata(scratch.path().join(format!("{PLUGIN}.log"))).expect("new active file").len();
    assert!(active < LOG_ROTATE_BYTES);
}

/// REQ: DEV-660
#[test]
fn dev_660_five_files_are_kept_and_the_oldest_is_deleted() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    for n in 0..1_700 {
        log.write_plugin(AT, n as u64 * 1_000, Side::Server, LogLevel::Info, &rotation_message(n));
    }
    for suffix in ["", ".1", ".2", ".3", ".4"] {
        assert!(scratch.path().join(format!("{PLUGIN}.log{suffix}")).exists(), "missing .log{suffix}");
    }
    assert!(!scratch.path().join(format!("{PLUGIN}.log.5")).exists());
    assert!(first_number(&scratch.path().join(format!("{PLUGIN}.log.4"))).unwrap_or(0) > 0, "oldest lines are gone");
}

/// REQ: DEV-660
#[test]
fn dev_660_files_hold_consecutive_runs_oldest_in_the_highest_number() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    for n in 0..1_700 {
        log.write_plugin(AT, n as u64 * 1_000, Side::Server, LogLevel::Info, &rotation_message(n));
    }
    let names = [".log.4", ".log.3", ".log.2", ".log.1", ".log"];
    for pair in names.windows(2) {
        let older = last_number(&scratch.path().join(format!("{PLUGIN}{}", pair[0]))).expect("older file has lines");
        let newer = first_number(&scratch.path().join(format!("{PLUGIN}{}", pair[1]))).expect("newer file has lines");
        assert_eq!(older + 1, newer, "{} then {}", pair[0], pair[1]);
    }
    assert_eq!(last_number(&scratch.path().join(format!("{PLUGIN}.log"))), Some(1_699));
}

/// REQ: DEV-660
#[test]
fn dev_660_no_rotation_below_4_mib() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    for n in 0..100 {
        log.write_plugin(AT, n as u64 * 1_000, Side::Server, LogLevel::Info, &rotation_message(n));
    }
    assert!(!scratch.path().join(format!("{PLUGIN}.log.1")).exists());
}

// ---------------------------------------------------------------- DEV-661

const SEVEN_DAYS_MS: u64 = 7 * 24 * 3_600_000;

fn logs_dir_with_log(root: &Path, plugin: &str) {
    std::fs::create_dir_all(root).expect("logs dir");
    std::fs::write(root.join(format!("{plugin}.log")), "x\n").expect("log file");
}

/// REQ: DEV-661
#[test]
fn dev_661_logs_are_kept_just_before_seven_days() {
    let scratch = scratch();
    logs_dir_with_log(scratch.path(), PLUGIN);
    mark_uninstalled(scratch.path(), PLUGIN, 1_000);
    purge_expired_logs(scratch.path(), 1_000 + SEVEN_DAYS_MS - 1);
    assert!(scratch.path().join(format!("{PLUGIN}.log")).exists());
}

/// REQ: DEV-661
#[test]
fn dev_661_logs_are_deleted_seven_days_after_the_uninstall() {
    let scratch = scratch();
    logs_dir_with_log(scratch.path(), PLUGIN);
    mark_uninstalled(scratch.path(), PLUGIN, 1_000);
    purge_expired_logs(scratch.path(), 1_000 + SEVEN_DAYS_MS);
    assert!(!scratch.path().join(format!("{PLUGIN}.log")).exists());
}

/// REQ: DEV-661
#[test]
fn dev_661_reinstall_within_seven_days_keeps_the_logs() {
    let scratch = scratch();
    logs_dir_with_log(scratch.path(), PLUGIN);
    mark_uninstalled(scratch.path(), PLUGIN, 1_000);
    mark_reinstalled(scratch.path(), PLUGIN);
    purge_expired_logs(scratch.path(), 1_000 + 2 * SEVEN_DAYS_MS);
    assert!(scratch.path().join(format!("{PLUGIN}.log")).exists());
}

/// REQ: DEV-661
#[test]
fn dev_661_installed_plugins_logs_are_never_purged() {
    let scratch = scratch();
    logs_dir_with_log(scratch.path(), PLUGIN);
    logs_dir_with_log(scratch.path(), "example.other");
    mark_uninstalled(scratch.path(), "example.other", 0);
    purge_expired_logs(scratch.path(), 3 * SEVEN_DAYS_MS);
    assert!(scratch.path().join(format!("{PLUGIN}.log")).exists());
    assert!(!scratch.path().join("example.other.log").exists());
}

// ---------------------------------------------------------------- DEV-663

/// REQ: DEV-663
#[test]
fn dev_663_log_folder_is_created_owner_only() {
    let scratch = scratch();
    let dir = scratch.path().join("plugins").join("logs").join("default");
    let _log = open_log(&dir, LogConfig::default());
    let mode = std::fs::metadata(&dir).expect("log folder exists").permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
}

/// REQ: DEV-663
#[test]
fn dev_663_log_files_are_not_group_or_world_accessible() {
    let scratch = scratch();
    let dir = scratch.path().join("logs");
    let mut log = open_log(&dir, LogConfig::default());
    log.write_plugin(AT, 0, Side::Server, LogLevel::Info, "m");
    let mode = std::fs::metadata(dir.join(format!("{PLUGIN}.log"))).expect("log file").permissions().mode();
    assert_eq!(mode & 0o077, 0);
}

// ---------------------------------------------------------------- DEV-664

/// REQ: DEV-664
#[test]
fn dev_664_host_lines_for_dropped_events_hold_counts_only() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    log.queue_full(AT, 0, 3);
    let text = std::fs::read_to_string(scratch.path().join(format!("{PLUGIN}.log"))).expect("log file");
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("dropped 3 events"));
    assert!(!text.contains("payload"));
}

/// REQ: DEV-664
#[test]
fn dev_664_plugin_written_data_is_stored_as_written() {
    let scratch = scratch();
    let mut log = open_log(scratch.path(), LogConfig::default());
    log.write_plugin(AT, 0, Side::Server, LogLevel::Info, "count=3");
    let lines = file_lines(&scratch.path().join(format!("{PLUGIN}.log")));
    assert!(lines[0].ends_with("count=3"));
}

// ---------------------------------------------------------------- DEV-665 .. DEV-667 (CLI)

fn sample_lines() -> Vec<String> {
    vec![
        log_line("2030-01-01 09:14:02", "info", "server", PLUGIN, "activate: ready"),
        log_line("2030-01-01 09:14:09", "warn", "server", PLUGIN, "storage near quota: 912 KiB of 1 MiB"),
        log_line("2030-01-01 09:15:40", "error", "host", PLUGIN, "restarting in 1 s (failure 1 of 3 in 10 minutes)"),
        log_line("2030-01-01 09:16:00", "debug", "client", PLUGIN, "render done"),
    ]
}

fn messages(output: &ilium_plugin_host::cli::CliOutput) -> Vec<String> {
    records_of(output, "log")
        .iter()
        .map(|record| record["message"].as_str().unwrap_or("").to_owned())
        .collect()
}

/// REQ: DEV-665
#[test]
fn dev_665_level_shows_lines_at_or_above_it() {
    let scratch = scratch();
    let env = env(scratch.path());
    write_log(&env, PLUGIN, "", &sample_lines());
    let output = run(&env, &["logs", PLUGIN, "--json", "--level", "warn"]);
    assert_eq!(messages(&output).len(), 2);
    let levels: Vec<&str> = records_of(&output, "log").iter().map(|r| r["level"].as_str().unwrap_or("")).collect();
    assert_eq!(levels, ["warn", "error"]);
}

/// REQ: DEV-665
#[test]
fn dev_665_level_debug_shows_everything_but_trace() {
    let scratch = scratch();
    let env = env(scratch.path());
    let mut lines = sample_lines();
    lines.push(log_line("2030-01-01 09:17:00", "trace", "client", PLUGIN, "t"));
    write_log(&env, PLUGIN, "", &lines);
    let output = run(&env, &["logs", PLUGIN, "--json", "--level", "debug"]);
    assert_eq!(messages(&output).len(), 4);
}

/// REQ: DEV-665
#[test]
fn dev_665_since_takes_an_absolute_time() {
    let scratch = scratch();
    let env = env(scratch.path());
    write_log(&env, PLUGIN, "", &sample_lines());
    let output = run(&env, &["logs", PLUGIN, "--json", "--since", "2030-01-01 09:14:09"]);
    assert_eq!(messages(&output).len(), 3, "the line at the given second is included");
    let output = run(&env, &["logs", PLUGIN, "--json", "--since", "2030-01-01 09:14:10"]);
    assert_eq!(messages(&output).len(), 2);
}

/// REQ: DEV-665
#[test]
fn dev_665_since_takes_a_duration() {
    let scratch = scratch();
    let env = env(scratch.path());
    let old = format_local_time(env.now - Duration::from_secs(20 * 60));
    let recent = format_local_time(env.now - Duration::from_secs(5 * 60));
    write_log(
        &env,
        PLUGIN,
        "",
        &[log_line(&old, "info", "server", PLUGIN, "old"), log_line(&recent, "info", "server", PLUGIN, "recent")],
    );
    let output = run(&env, &["logs", PLUGIN, "--json", "--since", "10m"]);
    assert_eq!(messages(&output), vec!["recent".to_owned()]);
}

/// REQ: DEV-665
#[test]
fn dev_665_level_and_since_combine() {
    let scratch = scratch();
    let env = env(scratch.path());
    write_log(&env, PLUGIN, "", &sample_lines());
    let output = run(&env, &["logs", PLUGIN, "--json", "--level", "warn", "--since", "2030-01-01 09:14:10"]);
    assert_eq!(messages(&output).len(), 1);
}

/// REQ: DEV-666
#[test]
fn dev_666_tail_defaults_to_200_lines() {
    let scratch = scratch();
    let env = env(scratch.path());
    let lines: Vec<String> = (0..250)
        .map(|n| log_line("2030-01-01 09:14:02", "info", "server", PLUGIN, &format!("line {n}")))
        .collect();
    write_log(&env, PLUGIN, "", &lines);
    let output = run(&env, &["logs", PLUGIN, "--json"]);
    let shown = messages(&output);
    assert_eq!(shown.len(), 200);
    assert_eq!(shown.first().map(String::as_str), Some("line 50"));
    assert_eq!(shown.last().map(String::as_str), Some("line 249"));
}

/// REQ: DEV-666
#[test]
fn dev_666_tail_n_shows_the_last_n_lines() {
    let scratch = scratch();
    let env = env(scratch.path());
    write_log(&env, PLUGIN, "", &sample_lines());
    let output = run(&env, &["logs", PLUGIN, "--json", "--tail", "2"]);
    assert_eq!(messages(&output), vec!["restarting in 1 s (failure 1 of 3 in 10 minutes)".to_owned(), "render done".to_owned()]);
}

/// REQ: DEV-666
#[test]
fn dev_666_tail_larger_than_the_log_shows_everything() {
    let scratch = scratch();
    let env = env(scratch.path());
    write_log(&env, PLUGIN, "", &sample_lines());
    let output = run(&env, &["logs", PLUGIN, "--json", "--tail", "1000"]);
    assert_eq!(messages(&output).len(), 4);
}

/// REQ: DEV-667
#[test]
fn dev_667_each_line_becomes_a_log_record_with_the_documented_fields() {
    let scratch = scratch();
    let env = env(scratch.path());
    write_log(&env, PLUGIN, "", &sample_lines()[..1]);
    let output = run(&env, &["logs", PLUGIN, "--json"]);
    let logs = records_of(&output, "log");
    assert_eq!(logs.len(), 1);
    let record = logs[0].as_object().expect("object");
    let mut keys: Vec<&str> = record.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["level", "message", "plugin", "side", "time", "type"]);
    assert_eq!(record["time"], "2030-01-01 09:14:02");
    assert_eq!(record["level"], "info");
    assert_eq!(record["side"], "server");
    assert_eq!(record["plugin"], PLUGIN);
    assert_eq!(record["message"], "activate: ready");
}

/// REQ: DEV-667
#[test]
fn dev_667_log_records_are_followed_by_one_summary_record() {
    let scratch = scratch();
    let env = env(scratch.path());
    write_log(&env, PLUGIN, "", &sample_lines());
    let output = run(&env, &["logs", PLUGIN, "--json"]);
    let types = record_types(&output);
    assert_eq!(&types[..4], ["log", "log", "log", "log"]);
    assert_eq!(records_of(&output, "summary").len(), 1);
    assert_eq!(types[types.len() - 2..], ["summary", "result"]);
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-667
#[test]
fn dev_667_message_with_spaces_and_colons_stays_whole() {
    let scratch = scratch();
    let env = env(scratch.path());
    write_log(&env, PLUGIN, "", &sample_lines()[1..2]);
    let output = run(&env, &["logs", PLUGIN, "--json"]);
    assert_eq!(messages(&output), vec!["storage near quota: 912 KiB of 1 MiB".to_owned()]);
}

// ---------------------------------------------------------------- DEV-662

/// REQ: DEV-662
#[test]
fn dev_662_files_are_read_oldest_first_across_rotation() {
    let scratch = scratch();
    let env = env(scratch.path());
    write_log(&env, PLUGIN, ".2", &[log_line("2030-01-01 08:00:00", "info", "server", PLUGIN, "first")]);
    write_log(&env, PLUGIN, ".1", &[log_line("2030-01-01 08:30:00", "info", "server", PLUGIN, "second")]);
    write_log(&env, PLUGIN, "", &[log_line("2030-01-01 09:00:00", "info", "server", PLUGIN, "third")]);
    let output = run(&env, &["logs", PLUGIN, "--json"]);
    assert_eq!(messages(&output), vec!["first".to_owned(), "second".to_owned(), "third".to_owned()]);
}

/// REQ: DEV-662
#[test]
fn dev_662_all_five_files_are_read_in_order() {
    let scratch = scratch();
    let env = env(scratch.path());
    for (suffix, name) in [(".4", "a"), (".3", "b"), (".2", "c"), (".1", "d"), ("", "e")] {
        write_log(&env, PLUGIN, suffix, &[log_line("2030-01-01 08:00:00", "info", "server", PLUGIN, name)]);
    }
    let output = run(&env, &["logs", PLUGIN, "--json"]);
    assert_eq!(messages(&output), ["a", "b", "c", "d", "e"].map(str::to_owned).to_vec());
}

/// REQ: DEV-662
#[test]
fn dev_662_tail_counts_from_the_newest_file() {
    let scratch = scratch();
    let env = env(scratch.path());
    write_log(&env, PLUGIN, ".1", &[log_line("2030-01-01 08:30:00", "info", "server", PLUGIN, "old")]);
    write_log(&env, PLUGIN, "", &[log_line("2030-01-01 09:00:00", "info", "server", PLUGIN, "new")]);
    let output = run(&env, &["logs", PLUGIN, "--json", "--tail", "1"]);
    assert_eq!(messages(&output), vec!["new".to_owned()]);
}

/// REQ: DEV-650, DEV-662
#[test]
fn dev_650_text_output_repeats_the_stored_lines_verbatim() {
    let scratch = scratch();
    let env = env(scratch.path());
    let lines = sample_lines();
    write_log(&env, PLUGIN, "", &lines);
    let output = run(&env, &["logs", PLUGIN]);
    assert_eq!(output.stdout_text, format!("{}\n", lines.join("\n")));
}

/// REQ: DEV-662
#[test]
fn dev_662_logs_of_a_plugin_that_never_logged_fail_with_plugin_not_found_or_are_empty() {
    // MAY: a missing log folder is either an empty result or `plugin.not-found`; neither is a crash.
    let scratch = scratch();
    let env = env(scratch.path());
    let output = run(&env, &["logs", PLUGIN, "--json"]);
    match output.exit_code {
        0 => assert!(records_of(&output, "log").is_empty()),
        _ => assert_eq!(error_code(&output).as_deref(), Some("plugin.not-found")),
    }
}

/// REQ: DEV-662
#[test]
fn dev_662_log_dir_is_under_state_plugins_logs() {
    let dir = plugin_log_dir(Path::new("/state"), Path::new("/work/project"), "default");
    assert!(dir.starts_with("/state/plugins/logs"), "{dir:?}");
}
