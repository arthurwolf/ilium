//! Agent primitive (primitives-agent.md, primitives-agent-transcript.md): views, signatures,
//! toolbar state, prompts, transcripts. Requirement ids PRI-010..015 and the ten WIT functions of
//! `ilium:primitives/agent`.

use ilium_plugin_api::generated::primitives::agent::{
    HostAgent, SessionRef, SignatureAction, SignatureSpec, TranscriptQuery, TranscriptTurn,
    TurnRole,
};
use ilium_plugin_api::generated::types::ids::{NodeId, RequestId};
use ilium_plugin_api::generated::types::tree::{AgentActivity, AgentModel};
use ilium_plugin_api::ErrorCode;
use proptest::prelude::*;

use crate::common::*;

const AGENT_PANE: NodeId = NodeId(20);
const READ: &[&str] = &["agent.read"];
const ALL: &[&str] = &[
    "agent.read",
    "agent.read-prompts",
    "agent.signatures",
    "agent.control",
    "agent.transcript",
];

fn agent_rig(grants: &[&str]) -> Rig {
    let rig = server_rig(grants);
    let mut tree = standard_tree();
    tree.push(agent_node(AGENT_PANE, "claude", AgentActivity::Working));
    rig.kernel.set_tree(tree);
    rig
}

fn signature(id: &str) -> SignatureSpec {
    SignatureSpec {
        id: id.to_owned(),
        process_names: vec!["mytool".to_owned()],
        ..SignatureSpec::default()
    }
}

fn turns(count: usize) -> Vec<TranscriptTurn> {
    (0..count)
        .map(|index| TranscriptTurn {
            role: if index % 2 == 0 {
                TurnRole::User
            } else {
                TurnRole::Assistant
            },
            text: format!("turn-{index}"),
            tool: None,
            at: None,
            tokens: None,
        })
        .collect()
}

fn query(last_turns: u32, max_bytes: u32) -> TranscriptQuery {
    TranscriptQuery {
        pane: AGENT_PANE,
        last_turns,
        max_bytes,
    }
}

// ---------------------------------------------------------------- PRI-010

/// REQ: PRI-010, WIT:ilium:primitives/agent.register-signature
#[test]
fn pri_010_valid_signature_accepted() {
    let rig = agent_rig(ALL);
    assert!(rig
        .host
        .agent()
        .register_signature(signature("mytool"))
        .is_ok());
}

/// REQ: PRI-010
#[test]
fn pri_010_empty_id_invalid_argument() {
    let rig = agent_rig(ALL);
    assert_eq!(
        error_of(rig.host.agent().register_signature(signature(""))),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-010
#[test]
fn pri_010_id_of_64_chars_accepted() {
    let rig = agent_rig(ALL);
    assert!(rig
        .host
        .agent()
        .register_signature(signature(&"a".repeat(64)))
        .is_ok());
}

/// REQ: PRI-010
#[test]
fn pri_010_id_of_65_chars_invalid_argument() {
    let rig = agent_rig(ALL);
    let result = rig
        .host
        .agent()
        .register_signature(signature(&"a".repeat(65)));
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: PRI-010
#[test]
fn pri_010_id_with_forbidden_characters_invalid_argument() {
    let rig = agent_rig(ALL);
    for id in ["Upper", "under_score", "dot.ted", "sp ace"] {
        let result = rig.host.agent().register_signature(signature(id));
        assert_eq!(
            error_of(result),
            Some(ErrorCode::InvalidArgument),
            "id {id:?}"
        );
    }
}

/// REQ: PRI-010
#[test]
fn pri_010_no_process_names_invalid_argument() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.process_names.clear();
    assert_eq!(
        error_of(rig.host.agent().register_signature(spec)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-010
#[test]
fn pri_010_32_process_names_accepted() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.process_names = (0..32).map(|index| format!("tool{index}")).collect();
    assert!(rig.host.agent().register_signature(spec).is_ok());
}

/// REQ: PRI-010
#[test]
fn pri_010_33_process_names_invalid_argument() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.process_names = (0..33).map(|index| format!("tool{index}")).collect();
    assert_eq!(
        error_of(rig.host.agent().register_signature(spec)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-010
#[test]
fn pri_010_empty_process_name_invalid_argument() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.process_names = vec![String::new()];
    assert_eq!(
        error_of(rig.host.agent().register_signature(spec)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-010
#[test]
fn pri_010_process_name_of_255_bytes_accepted_256_invalid_argument() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.process_names = vec!["n".repeat(255)];
    assert!(rig.host.agent().register_signature(spec).is_ok());
    let mut spec = signature("y");
    spec.process_names = vec!["n".repeat(256)];
    assert_eq!(
        error_of(rig.host.agent().register_signature(spec)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-010
#[test]
fn pri_010_relative_process_name_with_slash_invalid_argument() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.process_names = vec!["bin/tool".to_owned()];
    assert_eq!(
        error_of(rig.host.agent().register_signature(spec)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-010
#[test]
fn pri_010_absolute_process_name_with_slash_accepted() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.process_names = vec!["/usr/bin/tool".to_owned()];
    assert!(rig.host.agent().register_signature(spec).is_ok());
}

/// REQ: PRI-010
#[test]
fn pri_010_32_aliases_accepted_33_invalid_argument() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.aliases = (0..32).map(|index| format!("a{index}")).collect();
    assert!(rig.host.agent().register_signature(spec).is_ok());
    let mut spec = signature("y");
    spec.aliases = (0..33).map(|index| format!("a{index}")).collect();
    assert_eq!(
        error_of(rig.host.agent().register_signature(spec)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-010
#[test]
fn pri_010_unknown_icon_invalid_argument() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.icon = Some("no-such-tabler-icon-name".to_owned());
    assert_eq!(
        error_of(rig.host.agent().register_signature(spec)),
        Some(ErrorCode::InvalidArgument)
    );
}

fn action(index: usize, keys: String) -> SignatureAction {
    SignatureAction {
        id: format!("act-{index}"),
        keys,
        allowed_when: None,
    }
}

/// REQ: PRI-010
#[test]
fn pri_010_32_actions_accepted_33_invalid_argument() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.actions = (0..32).map(|index| action(index, "k".into())).collect();
    assert!(rig.host.agent().register_signature(spec).is_ok());
    let mut spec = signature("y");
    spec.actions = (0..33).map(|index| action(index, "k".into())).collect();
    assert_eq!(
        error_of(rig.host.agent().register_signature(spec)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-010
#[test]
fn pri_010_action_keys_1_kib_accepted_over_invalid_argument() {
    let rig = agent_rig(ALL);
    let mut spec = signature("x");
    spec.actions = vec![action(0, "k".repeat(KIB))];
    assert!(rig.host.agent().register_signature(spec).is_ok());
    let mut spec = signature("y");
    spec.actions = vec![action(0, "k".repeat(KIB + 1))];
    assert_eq!(
        error_of(rig.host.agent().register_signature(spec)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-010
#[test]
fn pri_010_id_the_manifest_contributes_conflict() {
    let rig = agent_rig(ALL);
    rig.kernel.contribute_signature_in_manifest("from-manifest");
    let result = rig
        .host
        .agent()
        .register_signature(signature("from-manifest"));
    assert_eq!(error_of(result), Some(ErrorCode::Conflict));
}

/// REQ: PRI-010
#[test]
fn pri_010_reregistering_an_own_runtime_signature_replaces_it() {
    let rig = agent_rig(ALL);
    rig.host
        .agent()
        .register_signature(signature("again"))
        .expect("first");
    assert!(rig
        .host
        .agent()
        .register_signature(signature("again"))
        .is_ok());
}

/// REQ: PRI-010, WIT:ilium:primitives/agent.register-signature
#[test]
fn pri_010_register_without_grant_not_permitted() {
    let rig = agent_rig(READ);
    assert_eq!(
        error_of(rig.host.agent().register_signature(signature("x"))),
        Some(ErrorCode::NotPermitted)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    // REQ: PRI-010
    #[test]
    fn pri_010_signature_id_is_valid_iff_charset_and_length_hold(id in "[a-zA-Z0-9_.-]{0,70}") {
        let rig = agent_rig(ALL);
        let valid = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        prop_assert_eq!(rig.host.agent().register_signature(signature(&id)).is_ok(), valid);
    }
}

// ---------------------------------------------------------------- PRI-011

fn overlap_rig() -> Rig {
    let rig = agent_rig(ALL);
    let mut user = signature("user-rule");
    user.process_names = vec!["mytool".to_owned()];
    rig.kernel.set_user_signature(user);
    rig.host
        .agent()
        .register_signature(signature("plugin-rule"))
        .expect("plugin rule");
    rig
}

/// REQ: PRI-011
#[test]
fn pri_011_user_signature_wins_over_plugin_signature() {
    let rig = overlap_rig();
    assert_eq!(
        rig.kernel.identify_process_name("mytool").as_deref(),
        Some("user-rule")
    );
}

/// REQ: PRI-011
#[test]
fn pri_011_plugin_signature_wins_over_built_in() {
    let rig = agent_rig(ALL);
    let mut spec = signature("my-claude");
    spec.process_names = vec!["claude".to_owned()];
    rig.host
        .agent()
        .register_signature(spec)
        .expect("plugin rule");
    let winner = rig
        .kernel
        .identify_process_name("claude")
        .expect("a signature");
    assert!(winner.ends_with("my-claude"), "winner {winner}");
}

/// REQ: PRI-011
#[test]
fn pri_011_overlap_is_logged_once_as_a_warning() {
    let rig = overlap_rig();
    let _ = rig.kernel.identify_process_name("mytool");
    let warnings: Vec<_> = rig
        .host
        .logs()
        .into_iter()
        .filter(|line| line.level == "warn" && line.message.contains("mytool"))
        .collect();
    assert_eq!(warnings.len(), 1, "{}", log_text(&rig.host));
}

/// REQ: PRI-011
#[test]
fn pri_011_no_overlap_logs_no_warning() {
    let rig = agent_rig(ALL);
    rig.host
        .agent()
        .register_signature(signature("solo"))
        .expect("rule");
    let _ = rig.kernel.identify_process_name("mytool");
    assert_eq!(log_count(&rig.host, "overlap"), 0);
}

// ---------------------------------------------------------------- PRI-012

/// REQ: PRI-012, WIT:ilium:primitives/agent.set-toolbar-state
#[test]
fn pri_012_own_button_on_existing_pane_accepted() {
    let rig = agent_rig(ALL);
    rig.kernel.contribute_toolbar_button("mine");
    let result = rig
        .host
        .agent()
        .set_toolbar_state("mine".into(), Some(AGENT_PANE), true, None);
    assert!(result.is_ok());
}

/// REQ: PRI-012
#[test]
fn pri_012_button_of_another_plugin_not_found() {
    let rig = agent_rig(ALL);
    rig.kernel
        .contribute_toolbar_button_for("other.plugin", "theirs");
    let result = rig
        .host
        .agent()
        .set_toolbar_state("theirs".into(), None, true, None);
    assert_eq!(error_of(result), Some(ErrorCode::NotFound));
}

/// REQ: PRI-012
#[test]
fn pri_012_missing_pane_not_found() {
    let rig = agent_rig(ALL);
    rig.kernel.contribute_toolbar_button("mine");
    let result = rig
        .host
        .agent()
        .set_toolbar_state("mine".into(), Some(MISSING_PANE), true, None);
    assert_eq!(error_of(result), Some(ErrorCode::NotFound));
}

/// REQ: PRI-012
#[test]
fn pri_012_without_grant_not_permitted() {
    let rig = agent_rig(READ);
    rig.kernel.contribute_toolbar_button("mine");
    let result = rig
        .host
        .agent()
        .set_toolbar_state("mine".into(), None, true, None);
    assert_eq!(error_of(result), Some(ErrorCode::NotPermitted));
}

/// REQ: PRI-012
#[test]
fn pri_012_label_of_32_cells_accepted_33_invalid_argument() {
    let rig = agent_rig(ALL);
    rig.kernel.contribute_toolbar_button("mine");
    let ok = rig
        .host
        .agent()
        .set_toolbar_state("mine".into(), None, true, Some("l".repeat(32)));
    assert!(ok.is_ok());
    let bad = rig
        .host
        .agent()
        .set_toolbar_state("mine".into(), None, true, Some("l".repeat(33)));
    assert_eq!(error_of(bad), Some(ErrorCode::InvalidArgument));
}

/// REQ: PRI-012
#[test]
fn pri_012_pane_state_is_dropped_when_the_pane_closes() {
    let rig = agent_rig(ALL);
    rig.kernel.contribute_toolbar_button("mine");
    rig.host
        .agent()
        .set_toolbar_state("mine".into(), Some(AGENT_PANE), false, None)
        .expect("state");
    rig.kernel.remove_node(AGENT_PANE);
    assert!(rig.kernel.toolbar_state("mine", Some(AGENT_PANE)).is_none());
}

// ---------------------------------------------------------------- PRI-013

/// REQ: PRI-013, WIT:ilium:primitives/agent.agents
#[test]
fn pri_013_agents_without_grant_is_empty() {
    let rig = agent_rig(&[]);
    assert!(rig.host.agent().agents().is_empty());
    assert!(
        log_count(&rig.host, "agent.read") >= 1,
        "{}",
        log_text(&rig.host)
    );
}

/// REQ: PRI-013, WIT:ilium:primitives/agent.agents
#[test]
fn pri_013_agents_with_grant_lists_identified_panes() {
    let rig = agent_rig(READ);
    let panes = rig.host.agent().agents();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].pane, AGENT_PANE);
    assert_eq!(panes[0].view.signature, "claude");
}

/// REQ: PRI-013, WIT:ilium:primitives/agent.view
#[test]
fn pri_013_view_without_grant_is_none() {
    let rig = agent_rig(&[]);
    assert!(rig.host.agent().view(AGENT_PANE).is_none());
}

/// REQ: WIT:ilium:primitives/agent.view
#[test]
fn wit_agent_view_returns_signature_and_activity() {
    let rig = agent_rig(READ);
    let view = rig.host.agent().view(AGENT_PANE).expect("view");
    assert_eq!(view.signature, "claude");
    assert_eq!(view.activity, AgentActivity::Working);
}

/// REQ: WIT:ilium:primitives/agent.view
#[test]
fn wit_agent_view_of_a_plain_terminal_is_none() {
    let rig = agent_rig(READ);
    assert!(rig.host.agent().view(PANE).is_none());
}

/// REQ: PRI-013, WIT:ilium:primitives/agent.session
#[test]
fn pri_013_session_without_grant_is_none() {
    let rig = agent_rig(&[]);
    rig.kernel.set_agent_session(
        AGENT_PANE,
        SessionRef {
            scope: "agent-sessions:claude".into(),
            path: "p".into(),
            session_id: "s".into(),
        },
    );
    assert!(rig.host.agent().session(AGENT_PANE).is_none());
}

/// REQ: WIT:ilium:primitives/agent.session
#[test]
fn wit_agent_session_returns_the_reference_for_a_scoped_signature() {
    let rig = agent_rig(READ);
    let reference = SessionRef {
        scope: "agent-sessions:claude".into(),
        path: "p".into(),
        session_id: "s-1".into(),
    };
    rig.kernel.set_agent_session(AGENT_PANE, reference.clone());
    assert_eq!(rig.host.agent().session(AGENT_PANE), Some(reference));
}

/// REQ: WIT:ilium:primitives/agent.session
#[test]
fn wit_agent_session_without_session_scope_is_none() {
    let rig = agent_rig(READ);
    assert!(rig.host.agent().session(AGENT_PANE).is_none());
}

/// REQ: PRI-013, WIT:ilium:primitives/agent.last-prompt
#[test]
fn pri_013_last_prompt_without_the_prompts_grant_is_none() {
    let rig = agent_rig(READ);
    rig.kernel.set_last_prompt(AGENT_PANE, "fix the bug");
    assert!(rig.host.agent().last_prompt(AGENT_PANE).is_none());
}

/// REQ: WIT:ilium:primitives/agent.last-prompt
#[test]
fn wit_agent_last_prompt_with_the_prompts_grant_returns_the_text() {
    let rig = agent_rig(&["agent.read", "agent.read-prompts"]);
    rig.kernel.set_last_prompt(AGENT_PANE, "fix the bug");
    assert_eq!(
        rig.host.agent().last_prompt(AGENT_PANE).as_deref(),
        Some("fix the bug")
    );
}

/// REQ: WIT:ilium:primitives/agent.model
#[test]
fn wit_agent_model_returns_the_model_record() {
    let rig = agent_rig(READ);
    let model = AgentModel {
        model: "opus".into(),
        provider: Some("anthropic".into()),
        context_window: Some(200_000),
        source: "screen".into(),
    };
    rig.kernel.set_agent_model(AGENT_PANE, model.clone());
    assert_eq!(rig.host.agent().model(AGENT_PANE), Some(model));
}

/// REQ: WIT:ilium:primitives/agent.model
#[test]
fn wit_agent_model_of_a_plain_terminal_is_none() {
    let rig = agent_rig(READ);
    assert!(rig.host.agent().model(PANE).is_none());
}

// ---------------------------------------------------------------- PRI-015

/// REQ: PRI-015, WIT:ilium:primitives/agent.prompt-epoch
#[test]
fn pri_015_restored_pane_reports_no_prompt_epoch() {
    let rig = agent_rig(&["agent.read", "agent.read-prompts"]);
    assert!(rig.host.agent().prompt_epoch(RESTORED_PANE).is_none());
}

/// REQ: PRI-015
#[test]
fn pri_015_unidentified_pane_has_no_activity() {
    let rig = agent_rig(READ);
    assert!(rig.host.agent().view(PANE).is_none());
}

/// REQ: PRI-015
#[test]
fn pri_015_restored_pane_reports_no_view() {
    let rig = agent_rig(READ);
    assert!(rig.host.agent().view(RESTORED_PANE).is_none());
}

/// REQ: WIT:ilium:primitives/agent.prompt-epoch
#[test]
fn wit_agent_prompt_epoch_returns_the_counter_of_an_identified_pane() {
    let rig = agent_rig(&["agent.read", "agent.read-prompts"]);
    rig.kernel.set_prompt_epoch(AGENT_PANE, 7);
    assert_eq!(rig.host.agent().prompt_epoch(AGENT_PANE), Some(7));
}

// ---------------------------------------------------------------- PRI-014 and transcripts

fn read_turns(rig: &mut Rig, query: TranscriptQuery) -> Vec<TranscriptTurn> {
    let request = rig
        .host
        .agent()
        .transcript(query)
        .expect("request accepted");
    rig.host.pump(8);
    rig.host
        .agent()
        .take_transcript(request)
        .expect("transcript completed")
}

/// REQ: PRI-014, WIT:ilium:primitives/agent.transcript, WIT:ilium:primitives/agent.take-transcript
#[test]
fn pri_014_600_turns_return_the_newest_512() {
    let mut rig = agent_rig(ALL);
    rig.kernel.set_agent_transcript(AGENT_PANE, turns(600));
    let got = read_turns(&mut rig, query(512, (4 * MIB) as u32));
    assert_eq!(got.len(), 512);
    assert_eq!(got.last().map(|turn| turn.text.as_str()), Some("turn-599"));
    assert_eq!(got.first().map(|turn| turn.text.as_str()), Some("turn-88"));
}

/// REQ: PRI-014
#[test]
fn pri_014_exactly_512_turns_are_all_returned() {
    let mut rig = agent_rig(ALL);
    rig.kernel.set_agent_transcript(AGENT_PANE, turns(512));
    assert_eq!(
        read_turns(&mut rig, query(512, (4 * MIB) as u32)).len(),
        512
    );
}

/// REQ: PRI-014
#[test]
fn pri_014_513_turns_drop_the_oldest_one() {
    let mut rig = agent_rig(ALL);
    rig.kernel.set_agent_transcript(AGENT_PANE, turns(513));
    let got = read_turns(&mut rig, query(512, (4 * MIB) as u32));
    assert_eq!(got.first().map(|turn| turn.text.as_str()), Some("turn-1"));
}

/// REQ: PRI-014
#[test]
fn pri_014_byte_cap_drops_the_oldest_turns() {
    let mut rig = agent_rig(ALL);
    let big: Vec<TranscriptTurn> = (0..10)
        .map(|index| TranscriptTurn {
            role: TurnRole::Assistant,
            text: format!("{index}{}", "x".repeat(MIB)),
            tool: None,
            at: None,
            tokens: None,
        })
        .collect();
    rig.kernel.set_agent_transcript(AGENT_PANE, big);
    let got = read_turns(&mut rig, query(512, (4 * MIB) as u32));
    let bytes: usize = got.iter().map(|turn| turn.text.len()).sum();
    assert!(bytes <= 4 * MIB, "{bytes} bytes");
    assert!(got.last().expect("turns").text.starts_with('9'));
    assert!(got.len() < 10);
}

/// REQ: PRI-014, WIT:ilium:primitives/agent.transcript
#[test]
fn pri_014_last_turns_over_512_invalid_argument() {
    let rig = agent_rig(ALL);
    assert_eq!(
        error_of(rig.host.agent().transcript(query(513, 1_024))),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-014
#[test]
fn pri_014_max_bytes_over_4_mib_invalid_argument() {
    let rig = agent_rig(ALL);
    let result = rig.host.agent().transcript(query(10, (4 * MIB) as u32 + 1));
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: PRI-014
#[test]
fn pri_014_max_bytes_exactly_4_mib_accepted() {
    let rig = agent_rig(ALL);
    assert!(rig
        .host
        .agent()
        .transcript(query(10, (4 * MIB) as u32))
        .is_ok());
}

/// REQ: PRI-014
#[test]
fn pri_014_last_turns_exactly_512_accepted() {
    let rig = agent_rig(ALL);
    assert!(rig.host.agent().transcript(query(512, 1_024)).is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    // REQ: PRI-014
    #[test]
    fn pri_014_returned_turns_never_exceed_512_and_are_the_newest(total in 1usize..900) {
        let mut rig = agent_rig(ALL);
        rig.kernel.set_agent_transcript(AGENT_PANE, turns(total));
        let got = read_turns(&mut rig, query(512, (4 * MIB) as u32));
        prop_assert_eq!(got.len(), total.min(512));
        let newest = format!("turn-{}", total - 1);
        prop_assert_eq!(got.last().map(|turn| turn.text.clone()), Some(newest));
    }
}

/// REQ: WIT:ilium:primitives/agent.transcript
#[test]
fn wit_agent_transcript_without_grant_not_permitted() {
    let rig = agent_rig(READ);
    assert_eq!(
        error_of(rig.host.agent().transcript(query(5, 1_024))),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:primitives/agent.transcript
#[test]
fn wit_agent_transcript_signature_without_session_format_unsupported() {
    let rig = agent_rig(ALL);
    rig.kernel.set_agent_session_format(AGENT_PANE, false);
    assert_eq!(
        error_of(rig.host.agent().transcript(query(5, 1_024))),
        Some(ErrorCode::Unsupported)
    );
}

/// REQ: WIT:ilium:primitives/agent.transcript
#[test]
fn wit_agent_transcript_pane_without_session_yet_not_found() {
    let rig = agent_rig(ALL);
    rig.kernel.set_agent_session_format(AGENT_PANE, true);
    assert_eq!(
        error_of(rig.host.agent().transcript(query(5, 1_024))),
        Some(ErrorCode::NotFound)
    );
}

/// REQ: WIT:ilium:primitives/agent.take-transcript
#[test]
fn wit_agent_take_transcript_unknown_request_is_none() {
    let rig = agent_rig(ALL);
    assert!(rig
        .host
        .agent()
        .take_transcript(RequestId(31_337))
        .is_none());
}

/// REQ: WIT:ilium:primitives/agent.take-transcript
#[test]
fn wit_agent_take_transcript_second_take_is_none() {
    let mut rig = agent_rig(ALL);
    rig.kernel.set_agent_transcript(AGENT_PANE, turns(3));
    let request = rig
        .host
        .agent()
        .transcript(query(3, 4_096))
        .expect("request");
    rig.host.pump(8);
    assert!(rig.host.agent().take_transcript(request).is_some());
    assert!(rig.host.agent().take_transcript(request).is_none());
}
