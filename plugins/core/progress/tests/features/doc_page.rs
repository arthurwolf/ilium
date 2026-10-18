//! The page and the baseline user documents it cites: tables, counts, limits and figures are
//! checked by parsing them (a row that is only checkable by reading a page is parsed in a `doc_` test).

use ilium_plugin_sdk::testing::assert_golden;

use crate::feature_support::*;

/// Rows of the behaviour contract table: `(id, rule, value, source)`.
fn behaviour_rows() -> Vec<[String; 4]> {
    let page = page_text();
    let start = page
        .find("| Id | Rule | Exact value or text | Baseline source |")
        .expect("behaviour table");
    page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect();
            [
                cells[0].clone(),
                cells[1].clone(),
                cells[2].clone(),
                cells[3].clone(),
            ]
        })
        .collect()
}

fn row(id: &str) -> [String; 4] {
    behaviour_rows()
        .into_iter()
        .find(|row| row[0] == id)
        .unwrap_or_else(|| panic!("row {id}"))
}

/// REQ: progress-B01, progress-B02, progress-B03, progress-B04, progress-B05
#[test]
fn doc_progress_b01_the_behaviour_table_holds_fifteen_rows_in_order() {
    let ids: Vec<String> = behaviour_rows()
        .into_iter()
        .map(|row| row[0].clone())
        .collect();
    let expected: Vec<String> = (1..=15)
        .map(|number| format!("progress-B{number:02}"))
        .collect();
    assert_eq!(ids, expected);
}

/// REQ: progress-B01
#[test]
fn doc_progress_b01_exit_statuses_match_the_cli_reference_table() {
    let value = &row("progress-B01")[2];
    assert_eq!(
        value,
        "0 done, 2 usage or rejected (for example `monitor-ambiguous`), 3 task error, 4 monitor failed (outcome unknown), 5 monitor replaced or cleared, 6 `--timeout-seconds` elapsed while the task is still running (`outcome` `still-running`)"
    );
    let reference = user_doc("cli-reference.md");
    for line in [
        "| 0 | `done` | The task reported `done`. |",
        "| 3 | `error` | The task reported `error`. |",
        "| 4 | `monitor-failed` |",
        "| 5 | `cleared`, `no-monitor` |",
        "| 6 | `still-running` | `--timeout-seconds` elapsed. |",
    ] {
        assert!(reference.contains(line), "{line}");
    }
}

/// REQ: progress-B02
#[test]
fn doc_progress_b02_outcome_per_exit_is_the_documented_mapping() {
    assert_eq!(
        row("progress-B02")[2],
        "0 `done`, 3 `error`, 4 `monitor-failed`, 5 `cleared` or `no-monitor`, 6 `still-running`"
    );
}

/// REQ: progress-B03
#[test]
fn doc_progress_b03_the_next_texts_are_the_six_documented_sentences() {
    let value = &row("progress-B03")[2];
    for text in [
        "The task finished. Verify its output, then continue.",
        "The task reported failure. Inspect its log or error, fix, and retry.",
        "The probe stopped working, so the task outcome is unknown. Check the task directly; do not assume success.",
        "The monitor was cleared, or is not registered on this pane. Check `ilium progress status`.",
        "This pane has no progress monitor. Register one with `ilium progress set`.",
        "The task is still running. Run the same wait command again to keep waiting.",
        " When you have handled it, remove this monitor with `ilium progress clear --monitor-id <id>`.",
    ] {
        assert!(value.contains(text), "{text}");
    }
}

/// REQ: progress-B04
#[test]
fn doc_progress_b04_composer_notice_values_are_the_three_documented_words() {
    let page = page_text();
    assert!(page.contains("\"composer_notice\":\"suppressed|may-also-arrive|none\""));
    let value = &row("progress-B04")[2];
    assert!(value.contains("`suppressed` while a wait takes the outcome"));
    assert!(value.contains("`may-also-arrive` for a settled outcome without a wait"));
    assert!(value.contains("`none` otherwise"));
}

/// REQ: progress-B05
#[test]
fn doc_progress_b05_the_limit_is_eight_monitors_per_pane() {
    assert!(row("progress-B05")[2]
        .starts_with("At most 8. The 9th live monitor is rejected with code `too-many-monitors`."));
    assert!(user_doc("cli-reference.md").contains("A pane holds up to 8 monitors at once"));
}

/// REQ: progress-B06
#[test]
fn doc_progress_b06_interval_default_and_range() {
    assert_eq!(row("progress-B06")[2], "Default 1, range 1 to 86400");
    assert!(user_doc("cli-reference.md").contains("| `--interval-seconds <n>` | 1 | How often the probe re-runs, from 1 to 86400 (24 hours). |"));
}

/// REQ: progress-B07
#[test]
fn doc_progress_b07_probe_size_limits_match_the_cli_reference() {
    assert_eq!(
        row("progress-B07")[2],
        "Command line 16 KiB, stdout 64 KiB, stderr 16 KiB, `job_id` 256 bytes, `message` 2048 bytes, `details` 8192 bytes, `error` 4096 bytes"
    );
    let reference = user_doc("cli-reference.md");
    assert!(reference.contains("The probe is a shell command line (at most 16 KiB)."));
    assert!(reference.contains("at most 256 bytes"));
    assert!(reference.contains("at most 2048 bytes"));
    assert!(reference.contains("at most 8192 bytes"));
    assert!(reference.contains("at most 4096 bytes"));
    assert!(reference.contains("at most 64 KiB on stdout and 16 KiB on stderr"));
}

/// REQ: progress-B08
#[test]
fn doc_progress_b08_probe_run_is_one_shell_line_with_a_thirty_second_timeout() {
    assert!(row("progress-B08")[2].contains("at most 30 s (`timeout_millis: 30_000`)"));
    assert!(page_text().contains("timeout_millis: 30_000,"));
}

/// REQ: progress-B09
#[test]
fn doc_progress_b09_three_consecutive_failures_fail_the_monitor() {
    assert!(row("progress-B09")[2]
        .starts_with("After 3 consecutive probe failures the monitor becomes `failed`"));
    assert!(user_doc("automation.md")
        .contains("After **three** consecutive failures the monitor is **failed**"));
}

/// REQ: progress-B10
#[test]
fn doc_progress_b10_the_cli_waits_forty_five_seconds_for_the_server_reply() {
    assert_eq!(
        row("progress-B10")[2],
        "The CLI waits up to 45 s for the server reply"
    );
    assert!(user_doc("cli-reference.md")
        .contains("The CLI waits up to 45 seconds for the server's correlated reply."));
}

/// REQ: progress-B11
#[test]
fn doc_progress_b11_same_probe_twice_returns_the_existing_monitor() {
    assert!(row("progress-B11")[2].contains("returns the existing monitor id"));
    assert!(user_doc("cli-reference.md").contains("Registering the exact command of a monitor that is still running returns that monitor instead of adding a second one."));
}

/// REQ: progress-B12
#[test]
fn doc_progress_b12_restore_needs_one_preflight_with_the_same_job_id() {
    assert!(
        row("progress-B12")[2].contains("only after one preflight probe returns the same `job_id`")
    );
    assert!(row("progress-B12")[2].contains("fresh monitor id"));
}

/// REQ: progress-B13
#[test]
fn doc_progress_b13_the_seven_delivery_states_and_the_retry_rules() {
    let value = &row("progress-B13")[2];
    for state in [
        "`not-queued`",
        "`queued`",
        "`attempted`",
        "`delivered-to-pty`",
        "`uncertain`",
        "`not-deliverable`",
        "`collected-by-waiter`",
    ] {
        assert!(value.contains(state), "{state}");
    }
    assert!(value.contains("Only `queued` is retried after a restart."));
    assert!(value.contains("Undelivered outcomes are re-sent every 20 s"));
}

/// REQ: progress-B14
#[test]
fn doc_progress_b14_terminal_result_is_typed_once_and_not_after_a_wait() {
    let value = &row("progress-B14")[2];
    assert_eq!(value, "Typed into the agent composer once (`composer_notice`); not typed when a running wait collected it");
}

/// REQ: progress-B15
#[test]
fn doc_progress_b15_rejection_codes_equal_the_cli_reference_list() {
    let codes: Vec<String> = row("progress-B15")[2]
        .split(", ")
        .map(|code| code.trim_matches('`').to_string())
        .collect();
    assert_eq!(codes.len(), 12);
    let reference = user_doc("cli-reference.md");
    let start = reference
        .find("Rejection codes: ")
        .expect("rejection codes line")
        + "Rejection codes: ".len();
    let line = &reference[start..reference[start..].find('.').map(|end| start + end).unwrap()];
    let documented: Vec<String> = line
        .split(", ")
        .map(|code| code.trim_matches('`').to_string())
        .collect();
    assert_eq!(codes, documented);
}

/// REQ: F-059, COR-011
#[test]
fn doc_059_the_four_settings_have_the_documented_defaults_and_ranges() {
    let page = page_text();
    let start = page
        .find("| Key | Default | Range or values | Baseline source |")
        .expect("settings table");
    let rows: Vec<Vec<String>> = page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0][0], "`[ui] progress_monitor_enabled`");
    assert_eq!(rows[0][1], "true");
    assert_eq!(rows[1][0], "`[ui] progress_max_lines`");
    assert_eq!(rows[1][1], "4");
    assert_eq!(rows[1][2], "Footer lines, 1 to 20");
    assert_eq!(rows[2][0], "`[ui] completed_progress_hide_after_seconds`");
    assert_eq!(rows[2][1], "60");
    assert_eq!(rows[3][0], "`[ui] task_progress_frames`");
    assert_eq!(
        rows[3][1],
        "the built-in preset 0 of the Agent Monitoring tab frame picker (the client default; the other presets are bars, moon phases and quarter circles)"
    );
    assert_eq!(
        rows[3][2],
        "2 to 13 printable frames, all one cell wide or all two cells wide"
    );
}

/// REQ: F-061
#[test]
fn doc_061_figure_one_equals_the_golden_file() {
    let figures = term_blocks(&page_text());
    assert!(figures.len() >= 2);
    assert_golden(&golden_path("progress_footer_50.txt"), &figures[0]);
}

/// REQ: F-060, progress-B01
#[test]
fn doc_060_figure_two_shows_the_registration_record_then_the_wait_record_and_exit_zero() {
    let figures = term_blocks(&page_text());
    let text = &figures[1];
    let registration = text
        .find("{\"type\":\"progress_set\"")
        .expect("progress_set");
    let wait = text
        .find("{\"type\":\"progress_wait\"")
        .expect("progress_wait");
    assert!(registration < wait);
    assert!(text.contains("\"outcome\":\"done\",\n \"exit_code\":0,"));
    let last_line = text.trim_end().lines().last().expect("last line").trim();
    assert_eq!(last_line, "0", "the figure ends with exit status 0");
}

/// REQ: F-060
#[test]
fn doc_060_record_shapes_name_their_fields_in_the_documented_order() {
    let page = page_text();
    assert!(page.contains(
        "{\"type\":\"progress_set\",\"request_id\":<n>,\"pane_id\":<n>,\"monitor_id\":<n>,\"progress\":{\"monitor_id\":<n>,\"report\":{...},\"monitor_health\":{\"state\":\"...\"},\"last_observed_unix_millis\":<n>}}"
    ));
    assert!(page.contains(
        "{\"type\":\"progress_wait\",\"pane_id\":<n>,\"monitor_id\":<n>,\"outcome\":\"done|error|monitor-failed|cleared|no-monitor|still-running\",\"exit_code\":<n>,\"waited_seconds\":<n>,\"composer_notice\":\"suppressed|may-also-arrive|none\",\"next\":\"...\",\"progress\":{...}}"
    ));
}

/// REQ: F-060, F-108
#[test]
fn doc_108_cli_reference_lists_the_wait_alias_with_the_same_arguments() {
    let reference = user_doc("cli-reference.md");
    assert!(reference.contains("ilium progress wait   [<monitor_id>] [--timeout-seconds <n>]"));
    assert!(reference.contains("ilium wait            [<monitor_id>] [--timeout-seconds <n>]"));
}

/// REQ: F-059
#[test]
fn doc_059_the_page_names_the_plugin_id_and_both_sides() {
    let page = page_text();
    assert!(page.contains("The plugin id is `ilium.core.progress` and it runs on both sides"));
    assert!(page.contains("the server side owns the CLI, the probes and the watchers"));
    assert!(page.contains("the client side draws the footer and the status bar segment"));
}

/// REQ: F-059, COR-011
#[test]
fn doc_059_every_page_linked_in_abi_used_exists() {
    let page = page_text();
    let start = page.find("### ABI used").expect("ABI used");
    let end = page[start..]
        .find("### See also")
        .map(|offset| start + offset)
        .expect("See also");
    let mut checked = 0;
    for part in page[start..end].split("(/pages/").skip(1) {
        let stem = part.split(['/', ')']).next().unwrap();
        let path = workspace_root()
            .join("docs/plugins-manual/pages")
            .join(format!("{stem}.md"));
        assert!(path.exists(), "{path:?}");
        checked += 1;
    }
    assert!(checked >= 9);
}

/// REQ: F-059
#[test]
fn doc_059_probe_contract_fields_and_status_values_match_the_cli_reference() {
    let reference = user_doc("cli-reference.md");
    assert!(reference.contains("One of `not-started-yet`, `running`, `error`, `done`."));
    let automation = user_doc("automation.md");
    assert!(automation.contains("`not-started-yet` may move to any status."));
    assert!(automation.contains(
        "`running` may move to `running`, `error` or `done`, but never back to `not-started-yet`."
    ));
    assert!(automation.contains("`error` and `done` are final."));
}
