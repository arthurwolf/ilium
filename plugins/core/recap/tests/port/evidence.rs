//! Twins of ilium-client/src/recap/evidence.rs: goals, prompts, screen digest and transcript windows.
//! The host parses the session file into turns (one string per turn here); the plugin keeps the
//! goal lines, bounds and redacts the evidence it sends.

use crate::common::*;
use crate::support::*;

const GOAL_BUDGET_CHARACTERS: usize = 8 * 1024;
const GOAL_OBJECTIVE_CLIP_CHARACTERS: usize = 2_000;
const PROMPT_CLIP_CHARACTERS: usize = 400;

fn objective_of(line: &str, state_word: &str) -> String {
    line.strip_prefix(&format!("Goal {state_word}: "))
        .unwrap_or_else(|| panic!("goal line {line:.60}"))
        .to_string()
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::parses_goal_command_arguments_only_from_goal_records
#[test]
fn port_evidence_parses_goal_command_arguments_only_from_goal_records() {
    let lines = goal_lines(
        &[
            "user: /goal ship the recap",
            "user: <command-args>x</command-args>",
            "user: /goal    ",
        ],
        &[],
    );
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].ends_with(": ship the recap"), "{lines:?}");
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::claude_goal_state_comes_from_met_flag_and_newer_goal_supersedes_active
#[test]
fn port_evidence_claude_goal_state_comes_from_the_met_flag_and_a_newer_goal_supersedes() {
    let lines = goal_lines(
        &["user: hello"],
        &[
            ("first objective".to_string(), "unknown"),
            ("second objective".to_string(), "done"),
        ],
    );
    assert_eq!(
        lines,
        [
            "Goal state unknown: first objective",
            "Goal done: second objective"
        ]
    );
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::codex_goal_status_maps_recorded_phases
#[test]
fn port_evidence_codex_goal_status_maps_recorded_phases() {
    let lines = goal_lines(
        &["user: hello"],
        &[
            ("objective a".to_string(), "paused"),
            ("objective b".to_string(), "done"),
        ],
    );
    assert_eq!(
        lines,
        ["Goal paused: objective a", "Goal done: objective b"]
    );
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::oversize_goal_list_keeps_first_and_newest_and_counts_omitted
#[test]
fn port_evidence_oversize_goal_list_keeps_first_and_newest() {
    let goals: Vec<(String, &str)> = (0..40)
        .map(|index| (format!("objective {index} {}", "x".repeat(400)), "active"))
        .collect();
    let lines = goal_lines(&["user: hello"], &goals);
    assert_eq!(lines.len(), 2, "38 goals are omitted from the middle");
    assert!(objective_of(&lines[0], "active").starts_with("objective 0 "));
    assert!(objective_of(&lines[1], "active").starts_with("objective 39 "));
    assert!(lines
        .iter()
        .all(|line| objective_of(line, "active").chars().count()
            <= GOAL_OBJECTIVE_CLIP_CHARACTERS + 64));
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::oversized_individual_goal_is_clipped_even_below_aggregate_budget
#[test]
fn port_evidence_oversized_individual_goal_is_clipped_below_the_aggregate_budget() {
    let objective = format!(
        "first {} last",
        "x".repeat(GOAL_OBJECTIVE_CLIP_CHARACTERS + 100)
    );
    let lines = goal_lines(&["user: hello"], &[(objective, "active")]);
    assert_eq!(lines.len(), 1);
    let kept = objective_of(&lines[0], "active");
    assert!(kept.starts_with("first ") && kept.ends_with(" last"));
    assert!(kept.contains("characters omitted"));
    assert!(kept.chars().count() <= GOAL_OBJECTIVE_CLIP_CHARACTERS + 64);
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::two_oversized_goals_keep_first_and_newest
#[test]
fn port_evidence_two_oversized_goals_keep_first_and_newest() {
    let first = format!("first {} first-tail", "x".repeat(GOAL_BUDGET_CHARACTERS));
    let newest = format!("newest {} newest-tail", "y".repeat(GOAL_BUDGET_CHARACTERS));
    let lines = goal_lines(&["user: hello"], &[(first, "paused"), (newest, "active")]);
    assert_eq!(lines.len(), 2);
    assert!(objective_of(&lines[0], "paused").starts_with("first "));
    assert!(objective_of(&lines[1], "active").starts_with("newest "));
    assert!(lines.iter().all(|line| line.contains("characters omitted")));
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::clipping_markers_count_towards_the_aggregate_goal_budget
#[test]
fn port_evidence_clipping_markers_count_towards_the_aggregate_goal_budget() {
    let size = GOAL_OBJECTIVE_CLIP_CHARACTERS + 1;
    let goals: Vec<(String, &str)> = vec![
        ("a".repeat(size), "active"),
        ("b".repeat(size), "active"),
        ("c".repeat(size), "active"),
        ("d".repeat(size), "active"),
        ("z".repeat(GOAL_BUDGET_CHARACTERS - 4 * size - 1), "active"),
    ];
    let lines = goal_lines(&["user: hello"], &goals);
    assert_eq!(lines.len(), 2);
    let objectives: Vec<String> = lines
        .iter()
        .map(|line| objective_of(line, "active"))
        .collect();
    assert!(objectives[0].starts_with('a'));
    assert!(objectives[1].starts_with('z'));
    assert!(
        objectives
            .iter()
            .map(|objective| objective.chars().count())
            .sum::<usize>()
            <= GOAL_BUDGET_CHARACTERS
    );
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::injected_notifications_are_not_prompts
#[test]
fn port_evidence_injected_notifications_are_not_prompts() {
    let (mut host, _, _) = start_agent_run(&[
        "user: <system-reminder>internal note</system-reminder>",
        "user: real question about the parser",
    ]);
    let evidence = job_evidence(&mut host);
    assert!(evidence.contains("real question about the parser"));
    assert!(!evidence.contains("internal note"));
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::prompts_are_bounded_and_secrets_are_redacted_before_leaving
#[test]
fn port_evidence_prompts_are_bounded_and_secrets_are_redacted_before_leaving() {
    let long = format!(
        "user: api_key=abcd1234efgh5678 {} {}",
        "p".repeat(900),
        "q".repeat(900)
    );
    let (mut host, _, _) = start_agent_run(&[long.as_str()]);
    let evidence = job_evidence(&mut host);
    assert!(!evidence.contains("abcd1234efgh5678"));
    assert!(evidence.contains("[redacted]"));
    assert!(
        !evidence.contains(&"p".repeat(PROMPT_CLIP_CHARACTERS + 64)),
        "the prompt is clipped"
    );
    assert!(
        !evidence.contains(&"q".repeat(PROMPT_CLIP_CHARACTERS + 64)),
        "the prompt is clipped"
    );
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::screen_tail_keeps_newest_lines_within_budget
#[test]
fn port_evidence_screen_tail_keeps_newest_lines_within_budget() {
    let screen: Vec<String> = (0..100).map(|index| format!("line {index:03}")).collect();
    let refs: Vec<&str> = screen.iter().map(String::as_str).collect();
    let (mut host, _, _) = start_shell_run(&refs);
    let evidence = job_evidence(&mut host);
    assert!(evidence.contains("line 099"));
    assert!(
        evidence.contains("line 060"),
        "the newest 40 lines are kept"
    );
    assert!(!evidence.contains("line 059"));
    assert!(!evidence.contains("line 000"));
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::screen_digest_redacts_and_reports_empty_screen_as_absent
#[test]
fn port_evidence_screen_digest_redacts_and_reports_an_empty_screen_as_idle() {
    let (mut host, _, _) = start_shell_run(&["export GITHUB_TOKEN=supersecretvalue123", "$ ls"]);
    let evidence = job_evidence(&mut host);
    assert!(evidence.contains("[redacted]"));
    assert!(!evidence.contains("supersecretvalue123"));
    let (mut empty, _, invocation) = start_shell_run(&["   ", "  "]);
    assert!(empty.jobs().list().is_empty());
    let records = jsonl(&empty.cli().refresh(&invocation).stdout());
    assert_eq!(first_recap(&records)["state"], "idle");
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::window_reader_reads_whole_small_file_and_skips_oversize_lines
#[test]
fn port_evidence_small_transcript_is_read_whole_and_in_order() {
    let (mut host, _, _) = start_agent_run(&["user: first", "user: third"]);
    let query = host.transcript_queries().pop().expect("a transcript query");
    assert_eq!(query.max_bytes, 512 * 1024);
    let evidence = job_evidence(&mut host);
    let first = evidence.find("first").expect("first turn present");
    let third = evidence.find("third").expect("third turn present");
    assert!(first < third, "turns keep their order");
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::window_reader_drops_partial_edge_lines_when_middle_is_skipped
#[test]
fn port_evidence_long_transcript_never_contains_a_partial_turn() {
    let turns: Vec<String> = (0..200)
        .map(|index| format!("user: line-{index:04}-padding-padding"))
        .collect();
    let refs: Vec<&str> = turns.iter().map(String::as_str).collect();
    let (mut host, _, _) = start_agent_run(&refs);
    let evidence = job_evidence(&mut host);
    assert!(evidence.contains("line-0199"), "the newest turn is kept");
    for (position, _) in evidence.match_indices("line-") {
        let word: String = evidence[position..]
            .chars()
            .take_while(|symbol| !symbol.is_whitespace())
            .collect();
        assert!(word.ends_with("padding-padding"), "partial turn {word}");
    }
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::parses_goal_command_arguments_only_from_goal_records
#[test]
fn port_evidence_transcript_without_a_goal_command_has_no_goal_lines() {
    assert!(goal_lines(&["user: hello"], &[]).is_empty());
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::claude_goal_state_comes_from_met_flag_and_newer_goal_supersedes_active
#[test]
fn port_evidence_single_blocked_goal_is_reported_with_its_recorded_state() {
    let lines = goal_lines(&["user: hello"], &[("only goal".to_string(), "blocked")]);
    assert_eq!(lines, ["Goal blocked: only goal"]);
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::codex_goal_status_maps_recorded_phases
#[test]
fn port_evidence_active_goal_maps_to_the_active_phase_label() {
    let lines = goal_lines(&["user: hello"], &[("objective c".to_string(), "active")]);
    assert_eq!(lines, ["Goal active: objective c"]);
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::oversize_goal_list_keeps_first_and_newest_and_counts_omitted
#[test]
fn port_evidence_oversize_goal_list_drops_the_middle_goals() {
    let goals: Vec<(String, &str)> = (0..40)
        .map(|index| (format!("objective {index} {}", "x".repeat(400)), "active"))
        .collect();
    let lines = goal_lines(&["user: hello"], &goals);
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("objective 20 ")),
        "{lines:?}"
    );
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::oversized_individual_goal_is_clipped_even_below_aggregate_budget
#[test]
fn port_evidence_clipped_goal_never_keeps_a_full_run_of_its_middle() {
    let objective = format!(
        "first {} last",
        "x".repeat(GOAL_OBJECTIVE_CLIP_CHARACTERS + 100)
    );
    let lines = goal_lines(&["user: hello"], &[(objective, "active")]);
    let kept = objective_of(&lines[0], "active");
    assert!(!kept.contains(&"x".repeat(GOAL_OBJECTIVE_CLIP_CHARACTERS)));
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::two_oversized_goals_keep_first_and_newest
#[test]
fn port_evidence_two_oversized_goals_keep_their_tails() {
    let first = format!("first {} first-tail", "x".repeat(GOAL_BUDGET_CHARACTERS));
    let newest = format!("newest {} newest-tail", "y".repeat(GOAL_BUDGET_CHARACTERS));
    let lines = goal_lines(&["user: hello"], &[(first, "paused"), (newest, "active")]);
    assert!(objective_of(&lines[0], "paused").ends_with("first-tail"));
    assert!(objective_of(&lines[1], "active").ends_with("newest-tail"));
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::clipping_markers_count_towards_the_aggregate_goal_budget
#[test]
fn port_evidence_clipped_newest_goal_says_how_much_was_omitted() {
    let size = GOAL_OBJECTIVE_CLIP_CHARACTERS + 1;
    let goals: Vec<(String, &str)> = vec![
        ("a".repeat(size), "active"),
        ("b".repeat(size), "active"),
        ("c".repeat(size), "active"),
        ("d".repeat(size), "active"),
        ("z".repeat(GOAL_BUDGET_CHARACTERS - 4 * size - 1), "active"),
    ];
    let lines = goal_lines(&["user: hello"], &goals);
    assert_eq!(lines.len(), 2);
    assert!(objective_of(&lines[1], "active").contains("characters omitted"));
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::injected_notifications_are_not_prompts
#[test]
fn port_evidence_injected_notice_markup_never_reaches_the_evidence() {
    let (mut host, _, _) = start_agent_run(&[
        "user: <system-reminder>internal note</system-reminder>",
        "user: real question about the parser",
    ]);
    let evidence = job_evidence(&mut host);
    assert!(!evidence.contains("system-reminder"), "{evidence}");
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::prompts_are_bounded_and_secrets_are_redacted_before_leaving
#[test]
fn port_evidence_clipped_prompt_keeps_its_first_characters() {
    let long = format!("user: {}", "p".repeat(900));
    let (mut host, _, _) = start_agent_run(&[long.as_str()]);
    let evidence = job_evidence(&mut host);
    assert!(evidence.contains(&"p".repeat(100)), "{evidence}");
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::screen_tail_keeps_newest_lines_within_budget
#[test]
fn port_evidence_screen_tail_keeps_every_one_of_the_newest_rows() {
    let screen: Vec<String> = (0..100).map(|index| format!("line {index:03}")).collect();
    let refs: Vec<&str> = screen.iter().map(String::as_str).collect();
    let (mut host, _, _) = start_shell_run(&refs);
    let evidence = job_evidence(&mut host);
    for index in 60..100 {
        assert!(evidence.contains(&format!("line {index:03}")), "{index}");
    }
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::screen_digest_redacts_and_reports_empty_screen_as_absent
#[test]
fn port_evidence_screen_digest_keeps_the_key_name_next_to_the_redaction() {
    let (mut host, _, _) = start_shell_run(&["export GITHUB_TOKEN=supersecretvalue123", "$ ls"]);
    let evidence = job_evidence(&mut host);
    assert!(evidence.contains("GITHUB_TOKEN=[redacted]"), "{evidence}");
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::window_reader_reads_whole_small_file_and_skips_oversize_lines
#[test]
fn port_evidence_small_transcript_is_read_once_per_run() {
    let (mut host, _, _) = start_agent_run(&["user: first"]);
    assert_eq!(host.transcript_queries().len(), 1);
}

/// REQ: PORT:ilium-client/src/recap/evidence.rs::window_reader_drops_partial_edge_lines_when_middle_is_skipped
#[test]
fn port_evidence_long_transcript_keeps_the_second_newest_turn() {
    let turns: Vec<String> = (0..200)
        .map(|index| format!("user: line-{index:04}-padding-padding"))
        .collect();
    let refs: Vec<&str> = turns.iter().map(String::as_str).collect();
    let (mut host, _, _) = start_agent_run(&refs);
    let evidence = job_evidence(&mut host);
    assert!(evidence.contains("line-0198"), "{evidence}");
}
