//! Port-compat parity requirements (COR-000, COR-012 to COR-015): terminal scrollback budget,
//! task-outcome notification suppression, text-trigger delivery and worktree setup commands.
//! Each requirement has an `a` test (specified behaviour) and a `b` test (refusal or edge case).
//! Every scenario runs on the plugin build through the public `ServerKernel` drivers.

use ilium_plugin_api::kernel::KernelFacade;
use ilium_plugin_host::lifecycle::StartMode;

use crate::common::*;

/// One `[[text_triggers.triggers]]` rule in the documented TOML shape (src/docs/automation.md).
fn text_trigger_config(enabled: bool, regexp: &str, message: &str, delay_seconds: u32) -> String {
    format!(
        "[[text_triggers.triggers]]\n\
         id = \"req-gap-port-compat\"\n\
         enabled = {enabled}\n\
         regexp = \"{regexp}\"\n\
         message = \"{message}\"\n\
         target = \"terminals\"\n\
         sample_text = \"{regexp}\"\n\
         delay_seconds = {delay_seconds}\n"
    )
}

/// REQ: COR-000
#[test]
fn cor_000_scrollback_default_budget_keeps_newest_output_and_drops_oldest_a() {
    let kernel = world(StartMode::Normal);
    assert_eq!(
        kernel.setting("terminal", "scrollback_budget_mib").as_deref(),
        Some("8")
    );
    let pane = terminal(&kernel, None, "flood", &["cat"]);
    // About 21 MB of numbered lines, more than the default 8 MiB budget.
    let output: String = (0..21_000)
        .map(|index| format!("line {index:06} {}\r\n", "x".repeat(1000)))
        .collect();
    kernel.feed_output(pane, output.as_bytes());
    kernel.settle();
    let history = kernel
        .panes()
        .history(pane, u32::MAX)
        .expect("the pane has a history");
    let joined = history.join("\n");
    assert!(joined.contains("line 020999"), "newest line is missing");
    assert!(
        !joined.contains("line 000000"),
        "oldest line is still retained beyond the budget"
    );
    for accepted in ["4", "512"] {
        assert_eq!(
            kernel.apply_setting("terminal", "scrollback_budget_mib", accepted),
            Ok(()),
            "{accepted}"
        );
    }
}

/// REQ: COR-000
#[test]
fn cor_000_scrollback_budget_write_of_six_is_refused_and_keeps_default_b() {
    let kernel = world(StartMode::Normal);
    let error = kernel
        .apply_setting("terminal", "scrollback_budget_mib", "6")
        .expect_err("a budget of 6 MiB is outside the step of 4 and is refused");
    assert!(error.contains("invalid-argument"), "{error}");
    assert_eq!(
        kernel.setting("terminal", "scrollback_budget_mib").as_deref(),
        Some("8")
    );
}

/// REQ: COR-012
#[test]
fn cor_012_idle_agent_task_outcome_is_suppressed_while_setting_is_on_a() {
    let kernel = world(StartMode::Normal);
    assert_eq!(
        kernel.apply_setting("notifications", "suppress_redundant_task_outcomes", "true"),
        Ok(())
    );
    // The driver has no idle-state input; the agent pane is named so the classifier sees an agent.
    let agent = terminal(&kernel, None, "claude", &["claude"]);
    let alerts_before = kernel.plugin_log("notifications").len();
    kernel.feed_output(agent, b"Task finished\r\n");
    kernel.settle();
    assert_eq!(
        kernel.plugin_log("notifications").len(),
        alerts_before,
        "a sound or toast reached an idle agent pane while suppression is on"
    );
}

/// REQ: COR-012
#[test]
fn cor_012_plain_shell_alerts_and_turning_suppression_off_lets_agent_alert_b() {
    let kernel = world(StartMode::Normal);
    assert_eq!(
        kernel.apply_setting("notifications", "suppress_redundant_task_outcomes", "true"),
        Ok(())
    );
    let shell = terminal(&kernel, None, "shell", &["sh"]);
    let shell_alerts_before = kernel.plugin_log("notifications").len();
    kernel.feed_output(shell, b"Task finished\r\n");
    kernel.settle();
    assert!(
        kernel.plugin_log("notifications").len() > shell_alerts_before,
        "a pane with no agent was suppressed"
    );

    assert_eq!(
        kernel.apply_setting("notifications", "suppress_redundant_task_outcomes", "false"),
        Ok(())
    );
    let agent = terminal(&kernel, None, "claude", &["claude"]);
    let agent_alerts_before = kernel.plugin_log("notifications").len();
    kernel.feed_output(agent, b"Task finished\r\n");
    kernel.settle();
    assert!(
        kernel.plugin_log("notifications").len() > agent_alerts_before,
        "the agent pane did not alert after suppression was switched off"
    );
}

/// REQ: COR-013
#[test]
fn cor_013_match_fires_once_and_fires_again_after_text_was_missing_for_two_seconds_a() {
    let kernel = world_with_config(
        StartMode::Normal,
        &text_trigger_config(true, "Build failed", "y", 0),
    );
    let pane = terminal(&kernel, None, "t", &["cat"]);
    kernel.feed_output(pane, b"Build failed\r\n");
    kernel.settle();
    assert_eq!(kernel.pane_input_log(pane), b"y".to_vec(), "first delivery");

    // The text leaves the screen, stays away for 2 s, then appears again as a new instance.
    kernel.feed_output(pane, b"\x1b[2J\x1b[H");
    kernel.settle();
    kernel.advance_clock_ms(2000);
    kernel.feed_output(pane, b"Build failed\r\n");
    kernel.settle();
    assert_eq!(
        kernel.pane_input_log(pane),
        b"yy".to_vec(),
        "a new instance after 2 s missing did not deliver again"
    );
}

/// REQ: COR-013
#[test]
fn cor_013_repainting_the_same_visible_line_does_not_fire_again_b() {
    let kernel = world_with_config(
        StartMode::Normal,
        &text_trigger_config(true, "Build failed", "y", 0),
    );
    let pane = terminal(&kernel, None, "t", &["cat"]);
    kernel.feed_output(pane, b"Build failed\r\n");
    kernel.settle();
    kernel.feed_output(pane, b"\x1b[1;1HBuild failed");
    kernel.settle();
    assert_eq!(
        kernel.pane_input_log(pane),
        b"y".to_vec(),
        "a repaint of text that stayed visible delivered again"
    );
}

/// REQ: COR-014
#[test]
fn cor_014_delayed_delivery_still_sends_when_the_rule_stays_valid_a() {
    let kernel = world_with_config(
        StartMode::Normal,
        &text_trigger_config(true, "Deploy now", "x", 5),
    );
    let pane = terminal(&kernel, None, "t", &["cat"]);
    kernel.feed_output(pane, b"Deploy now\r\n");
    kernel.settle();
    assert!(
        kernel.pane_input_log(pane).is_empty(),
        "a delayed delivery arrived before its 5 s delay"
    );
    kernel.advance_clock_ms(5000);
    kernel.settle();
    assert_eq!(
        kernel.pane_input_log(pane),
        b"x".to_vec(),
        "the valid, enabled rule did not deliver after 5 s"
    );
}

/// REQ: COR-014
#[test]
fn cor_014_disabling_the_rule_at_two_seconds_cancels_the_pending_send_b() {
    let kernel = world_with_config(
        StartMode::Normal,
        &text_trigger_config(true, "Deploy now", "x", 5),
    );
    let pane = terminal(&kernel, None, "t", &["cat"]);
    kernel.feed_output(pane, b"Deploy now\r\n");
    kernel.settle();
    kernel.advance_clock_ms(2000);
    kernel.write_config_file(&text_trigger_config(false, "Deploy now", "x", 5));
    kernel.settle();
    kernel.advance_clock_ms(3000);
    kernel.settle();
    assert!(
        kernel.pane_input_log(pane).is_empty(),
        "input reached the PTY after the rule was disabled"
    );
}

/// REQ: COR-015
#[test]
fn cor_015_setup_command_read_back_and_8192_byte_limit_accepted_on_linux_a() {
    let configured = "printf setup-marker; exit 1";
    let kernel = world_with_config(
        StartMode::Normal,
        &format!("[git]\nsetup_command = \"{configured}\"\n"),
    );
    assert_eq!(
        kernel.setting("git", "setup_command").as_deref(),
        Some(configured)
    );
    let longest_allowed = "x".repeat(8192);
    assert_eq!(
        kernel.apply_setting("git", "setup_command", &longest_allowed),
        Ok(()),
        "a command of exactly 8192 bytes is refused"
    );
}

/// REQ: COR-015
#[test]
fn cor_015_oversized_nul_and_non_linux_setup_commands_are_refused_b() {
    let kernel = world(StartMode::Normal);
    let oversized = "x".repeat(8193);
    assert!(
        kernel
            .apply_setting("git", "setup_command", &oversized)
            .is_err(),
        "an 8193-byte command was accepted"
    );
    assert!(
        kernel
            .apply_setting("git", "setup_command", "printf a\0b")
            .is_err(),
        "a command containing NUL was accepted"
    );
    kernel.pretend_platform_windows();
    assert!(
        kernel
            .apply_setting("git", "setup_command", "printf marker")
            .is_err(),
        "a non-empty setup command was accepted on a non-Linux platform"
    );
}
