//! F-080 techniques: the six summarising techniques, their prompts, cleaning and validation (user doc Remote compaction, "Techniques").

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{AgentActivity, ErrorCode, PluginError};
use serde_json::json;

use crate::common::*;

const CLAUDE_REPLY: &str = "<analysis>thinking out loud</analysis>\n<summary>\n1. Primary Request and Intent:\n   build it\n7. Pending Tasks:\n   - none\n8. Current Work:\n   testing\n</summary>";
const TECHNIQUE_IDS: [&str; 6] = [
    "claude_code",
    "codex",
    "opencode",
    "gemini_cli",
    "best_of_all_worlds",
    "custom",
];

fn host_with_technique(technique: &str, custom_prompt: &str) -> FakeHost {
    let mut host = server_host();
    for name in ["claude_technique", "codex_technique", "other_technique"] {
        host.settings()
            .set(&format!("remote_compaction.{name}"), json!(technique))
            .expect("setting accepted");
    }
    host.settings()
        .set("remote_compaction.custom_prompt", json!(custom_prompt))
        .expect("setting accepted");
    host
}

fn first_request(host: &FakeHost) -> (String, String) {
    let requests = host.inference_requests();
    let request = &requests.first().expect("an inference request").1;
    let system = request
        .messages
        .iter()
        .find(|message| message.role == "system")
        .map(|message| message.content.clone())
        .unwrap_or_default();
    let user = request
        .messages
        .iter()
        .find(|message| message.role == "user")
        .map(|message| message.content.clone())
        .unwrap_or_default();
    (system, user)
}

fn run(
    technique: &str,
    custom_prompt: &str,
    reply: &str,
    input: &[u8],
) -> (FakeHost, Result<Vec<u8>, PluginError>) {
    let mut host = host_with_technique(technique, custom_prompt);
    host.script_inference(vec![Ok(reply.to_string())]);
    let result = host.run_job("compact", input.to_vec());
    (host, result)
}

/// REQ: F-080
#[test]
fn technique_080_every_technique_prompt_carries_the_guards_and_leaves_no_placeholder() {
    for technique in TECHNIQUE_IDS {
        let (host, _) = run(
            technique,
            "Write a haiku about {{conversation}}.",
            CLAUDE_REPLY,
            b"[turn 1 | user]\nhello",
        );
        let (system, user) = first_request(&host);
        assert!(system.contains("automated system operation"), "{technique}");
        assert!(system.contains("Do not call any tools"), "{technique}");
        assert!(system.contains("untrusted historical data"), "{technique}");
        assert!(user.contains("hello"), "{technique}");
        assert!(!system.contains("{{"), "{technique}");
    }
}

/// REQ: F-080
#[test]
fn technique_080_claude_code_prompt_names_the_nine_section_summary() {
    let (host, _) = run("claude_code", "", CLAUDE_REPLY, b"hello");
    assert!(first_request(&host)
        .0
        .contains("Primary Request and Intent"));
}

/// REQ: F-080
#[test]
fn technique_080_opencode_prompt_uses_the_objective_work_state_next_move_layout() {
    let (host, _) = run(
        "opencode",
        "",
        "## Objective\n- x\n## Work State\n### Active\n- y\n## Next Move\n1. z",
        b"hello",
    );
    assert!(first_request(&host).0.contains("## Objective"));
}

/// REQ: F-080
#[test]
fn technique_080_custom_text_is_used_verbatim_with_the_conversation_placeholder_replaced() {
    let (host, _) = run(
        "custom",
        "Summarize {{conversation}} as <b>bullets</b> & {{other}}.",
        CLAUDE_REPLY,
        b"hello",
    );
    let (system, user) = first_request(&host);
    assert!(system.contains(
        "Summarize the conversation provided in the user message as <b>bullets</b> & {{other}}."
    ));
    assert!(user.contains("Write the summary now, following the instructions above."));
}

/// REQ: F-080
#[test]
fn technique_080_custom_without_text_falls_back_to_the_claude_code_prompt() {
    for empty in ["", "  \n"] {
        let (host, _) = run("custom", empty, CLAUDE_REPLY, b"hello");
        assert!(
            first_request(&host)
                .0
                .contains("Primary Request and Intent"),
            "{empty:?}"
        );
    }
}

/// REQ: F-080
#[test]
fn technique_080_a_plain_codex_run_has_neither_prior_summary_nor_facts_blocks() {
    let (host, _) = run(
        "codex",
        "",
        "A perfectly fine plain-text handoff summary.",
        b"hello",
    );
    let (_, user) = first_request(&host);
    assert!(!user.contains("<prior-summary>"));
}

/// REQ: F-080
#[test]
fn technique_080_a_large_input_is_merged_with_numbered_parts() {
    let mut host = host_with_technique("opencode", "");
    let reply = "## Objective\n- x\n## Work State\n### Active\n- y\n## Next Move\n1. z";
    host.script_inference(vec![
        Ok(reply.to_string()),
        Ok(reply.to_string()),
        Ok(reply.to_string()),
    ]);
    host.settings()
        .set("remote_compaction.summarizer_context_tokens", json!(8_000))
        .expect("setting accepted");
    host.run_job("compact", transcript_of(200_000))
        .expect("job succeeds");
    let users: Vec<String> = host
        .inference_requests()
        .iter()
        .filter_map(|(_, request)| {
            request
                .messages
                .iter()
                .find(|message| message.role == "user")
                .map(|message| message.content.clone())
        })
        .collect();
    let merge = users.last().expect("a merge request");
    assert!(merge.contains("[part 1]\n") && merge.contains("[part 2]\n"));
    assert!(merge.contains(" parts"));
}

/// REQ: F-080
#[test]
fn technique_080_claude_reply_loses_analysis_and_summary_tags() {
    let (_, result) = run("claude_code", "", CLAUDE_REPLY, b"hello");
    let text = String::from_utf8(result.expect("job succeeds")).expect("utf-8");
    assert!(text.starts_with("1. Primary Request and Intent:"));
    assert!(
        !text.contains("<analysis>")
            && !text.contains("<summary>")
            && !text.contains("thinking out loud")
    );
}

/// REQ: F-080
#[test]
fn technique_080_gemini_reply_keeps_only_the_state_snapshot() {
    let reply = "<scratchpad>notes</scratchpad>\n<state_snapshot>\n<overall_goal>g</overall_goal>\n<task_state>t</task_state>\n</state_snapshot>\ntrailing chatter";
    let (_, result) = run("gemini_cli", "", reply, b"hello");
    let text = String::from_utf8(result.expect("job succeeds")).expect("utf-8");
    assert!(text.starts_with("<state_snapshot>") && text.ends_with("</state_snapshot>"));
}

/// REQ: F-080
#[test]
fn technique_080_an_unterminated_analysis_block_cleans_to_nothing_and_is_retried() {
    let mut host = host_with_technique("claude_code", "");
    host.script_inference(vec![
        Ok("<analysis>never closed 1. Primary Request".to_string()),
        Ok(CLAUDE_REPLY.to_string()),
    ]);
    host.run_job("compact", b"hello".to_vec())
        .expect("second reply is valid");
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 2);
    let retry_user = requests[1]
        .1
        .messages
        .iter()
        .find(|message| message.role == "user")
        .expect("user message");
    assert!(retry_user
        .content
        .ends_with("Reply again with the complete summary in exactly the required format."));
}

/// REQ: F-080
#[test]
fn technique_080_the_claude_validator_names_the_missing_sections_in_the_retry_note() {
    let mut host = host_with_technique("claude_code", "");
    host.script_inference(vec![
        Ok("1. Primary Request and Intent: x and more text here".to_string()),
        Ok(CLAUDE_REPLY.to_string()),
    ]);
    host.run_job("compact", b"hello".to_vec())
        .expect("second reply is valid");
    let retry = host.inference_requests()[1]
        .1
        .messages
        .iter()
        .find(|message| message.role == "user")
        .expect("user")
        .content
        .clone();
    assert!(retry.contains("pending tasks") && retry.contains("current work"));
}

/// REQ: F-080
#[test]
fn technique_080_a_too_short_codex_summary_is_rejected_and_retried() {
    let mut host = host_with_technique("codex", "");
    host.script_inference(vec![
        Ok("short".to_string()),
        Ok("A perfectly fine plain-text handoff summary.".to_string()),
    ]);
    host.run_job("compact", b"hello".to_vec())
        .expect("second reply is valid");
    assert_eq!(host.inference_requests().len(), 2);
}

/// REQ: F-080
#[test]
fn technique_080_a_valid_codex_summary_needs_no_retry() {
    let (host, result) = run(
        "codex",
        "",
        "A perfectly fine plain-text handoff summary.",
        b"hello",
    );
    assert!(result.is_ok());
    assert_eq!(host.inference_requests().len(), 1);
}

/// REQ: F-080
#[test]
fn technique_080_opencode_summary_without_its_three_headings_is_rejected() {
    let mut host = host_with_technique("opencode", "");
    host.script_inference(vec![
        Ok("## Objective\n- x and some more words".to_string()),
        Ok("## Objective\n- x\n## Work State\n### Active\n- y\n## Next Move\n1. z".to_string()),
    ]);
    host.run_job("compact", b"hello".to_vec())
        .expect("second reply is valid");
    assert_eq!(host.inference_requests().len(), 2);
}

/// REQ: F-080
#[test]
fn technique_080_a_half_open_gemini_snapshot_is_rejected() {
    let mut host = host_with_technique("gemini_cli", "");
    let good = "<state_snapshot>\n<overall_goal>g</overall_goal>\n<task_state>t</task_state>\n</state_snapshot>";
    host.script_inference(vec![
        Ok("<state_snapshot>half open and long enough".to_string()),
        Ok(good.to_string()),
    ]);
    host.run_job("compact", b"hello".to_vec())
        .expect("second reply is valid");
    assert_eq!(host.inference_requests().len(), 2);
}

/// REQ: F-080
#[test]
fn technique_080_each_agent_uses_its_own_technique_row_and_others_use_the_shared_row() {
    let mut host = server_host();
    host.settings()
        .set("remote_compaction.claude_technique", json!("opencode"))
        .expect("setting accepted");
    host.settings()
        .set("remote_compaction.codex_technique", json!("gemini_cli"))
        .expect("setting accepted");
    host.settings()
        .set("remote_compaction.other_technique", json!("codex"))
        .expect("setting accepted");
    let claude = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    let codex = add_agent_pane(&mut host, "codex", "codex", AgentActivity::Idle);
    let other = add_agent_pane(&mut host, "gemini", "antigravity", AgentActivity::Idle);
    let mut used = Vec::new();
    for pane in [claude, codex, other] {
        host.script_inference(vec![Ok(
            "A perfectly fine plain-text handoff summary.".to_string()
        )]);
        let _ = host.run_job_for_pane(pane, "compact", b"hello".to_vec());
        used.push(
            host.inference_requests()
                .last()
                .expect("request")
                .1
                .messages[0]
                .content
                .clone(),
        );
    }
    assert!(used[0].contains("## Objective"));
    assert!(used[1].contains("state_snapshot"));
    assert!(!used[2].contains("## Objective") && !used[2].contains("state_snapshot"));
}

/// REQ: F-080
#[test]
fn technique_080_an_empty_reply_every_time_ends_in_the_offline_fallback_not_an_error() {
    let mut host = host_with_technique("claude_code", "");
    host.script_inference(vec![
        Ok(String::new()),
        Ok(String::new()),
        Ok(String::new()),
    ]);
    let result = host.run_job("compact", b"first request\nsecond request".to_vec());
    assert!(result.is_ok());
}

/// REQ: F-080
#[test]
fn technique_080_input_too_long_from_the_provider_triggers_a_smaller_chunk_retry() {
    let mut host = host_with_technique("codex", "");
    let too_long = PluginError {
        code: ErrorCode::InvalidArgument,
        message: "input too long".to_string(),
        retry_after_ms: None,
    };
    host.script_inference(vec![
        Err(too_long),
        Ok("A perfectly fine plain-text handoff summary.".to_string()),
        Ok("A perfectly fine plain-text handoff summary.".to_string()),
    ]);
    host.run_job("compact", transcript_of(100_000))
        .expect("re-chunked run succeeds");
    assert!(host.inference_requests().len() >= 3);
}
