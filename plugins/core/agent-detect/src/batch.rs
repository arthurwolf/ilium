//! The `ilium.core.agent-detect/screen-verdicts` service: every due pane's screen in one call.
//!
//! The kernel detection loop must never wait on a plugin per pane per tick (HST-450, HST-451,
//! HST-455). It instead sends the screens of all due panes in one `classify-batch` request off
//! the loop, keeps the replies in a verdict cache keyed by pane, screen generation and signature,
//! and reads only that cache inside the tick; a missing or stale verdict is "no sample", never
//! idle. This module is the plugin half of that contract: a pure function from the request text
//! to the reply text, carrying everything the WIT `classification` record cannot (the provider
//! turn, the predicate that matched and its line, the goal rule and pattern, a fresh
//! conversation, the interstitial answer and composer readiness at the cursor).
//!
//! Request: `{"screens": [{"pane": u64, "generation": u64, "signature": str, "text": str,
//! "cursor": [row, column] | null, "dimmed": [[row, column], ...]}]}`.
//! Reply: `{"verdicts": [{"pane", "generation", "signature", "turn", "evidence",
//! "evidence_line", "goal", "goal_line", "goal_rule", "goal_pattern", "fresh_screen",
//! "interstitial_response", "prompt_ready"}]}` in request order, or `{"error": {...}}`.

use serde_json::{json, Map, Value};

use crate::screen::{
    self, ActivityEvidence, AgentClass, AgentTurn, GoalEvidence, GoalEvidenceRule, GoalState,
};

/// The service method of a batch.
pub const CLASSIFY_BATCH: &str = "classify-batch";
/// Most screens one batch may carry: the detection loop's own cap on due panes per tick.
pub const MAX_SCREENS: usize = 32;
/// Largest screen text one entry may carry, in bytes (the loop's snapshot cap).
pub const MAX_SCREEN_BYTES: usize = 2 * 1024 * 1024;
/// Most dimmed cells one entry may carry.
pub const MAX_DIMMED_CELLS: usize = 64 * 1024;

/// One screen of a batch, as parsed from the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenRequest {
    pub pane: u64,
    pub generation: u64,
    pub signature: String,
    pub text: String,
    pub cursor: Option<(u16, u16)>,
    pub dimmed: Vec<(u16, u16)>,
}

/// Answers one service call of the classify service.
pub fn handle(method: &str, payload: &str) -> String {
    if method != CLASSIFY_BATCH {
        return error_reply("unsupported", &format!("unknown method {method}"));
    }
    match parse_request(payload) {
        Ok(screens) => {
            let verdicts: Vec<Value> = screens.iter().map(verdict_of).collect();
            json!({ "verdicts": verdicts }).to_string()
        }
        Err(message) => error_reply("invalid-argument", &message),
    }
}

fn error_reply(code: &str, message: &str) -> String {
    json!({ "error": { "code": code, "message": message } }).to_string()
}

fn cell(value: &Value) -> Option<(u16, u16)> {
    let pair = value.as_array()?;
    if pair.len() != 2 {
        return None;
    }
    let row = u16::try_from(pair[0].as_u64()?).ok()?;
    let column = u16::try_from(pair[1].as_u64()?).ok()?;
    Some((row, column))
}

/// Parses and bounds a request. Any malformed entry refuses the whole batch, so a reply never
/// silently drops a pane the kernel is waiting on.
pub fn parse_request(payload: &str) -> Result<Vec<ScreenRequest>, String> {
    let request: Value =
        serde_json::from_str(payload).map_err(|error| format!("not JSON: {error}"))?;
    let screens = request
        .get("screens")
        .and_then(Value::as_array)
        .ok_or("the request has no screens list")?;
    if screens.len() > MAX_SCREENS {
        return Err(format!(
            "{} screens is over the {MAX_SCREENS} screen cap",
            screens.len()
        ));
    }
    screens
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let field = |name: &str| entry.get(name);
            let pane = field("pane")
                .and_then(Value::as_u64)
                .ok_or(format!("screen {index} has no pane"))?;
            let generation = field("generation")
                .and_then(Value::as_u64)
                .ok_or(format!("screen {index} has no generation"))?;
            let signature = field("signature")
                .and_then(Value::as_str)
                .filter(|signature| !signature.is_empty())
                .ok_or(format!("screen {index} has no signature"))?
                .to_owned();
            let text = field("text")
                .and_then(Value::as_str)
                .ok_or(format!("screen {index} has no text"))?;
            if text.len() > MAX_SCREEN_BYTES {
                return Err(format!(
                    "screen {index} is over the {MAX_SCREEN_BYTES} byte cap"
                ));
            }
            let cursor = match field("cursor") {
                None | Some(Value::Null) => None,
                Some(value) => Some(cell(value).ok_or(format!("screen {index} has a bad cursor"))?),
            };
            let dimmed = match field("dimmed") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(cells)) if cells.len() <= MAX_DIMMED_CELLS => cells
                    .iter()
                    .map(cell)
                    .collect::<Option<Vec<_>>>()
                    .ok_or(format!("screen {index} has a bad dimmed cell"))?,
                Some(_) => return Err(format!("screen {index} has a bad dimmed list")),
            };
            Ok(ScreenRequest {
                pane,
                generation,
                signature,
                text: text.to_owned(),
                cursor,
                dimmed,
            })
        })
        .collect()
}

/// The wire name of a provider turn.
pub fn turn_name(turn: AgentTurn) -> &'static str {
    match turn {
        AgentTurn::Working => "working",
        AgentTurn::WaitingApproval => "waiting-approval",
        AgentTurn::WaitingSubagents => "waiting-subagents",
        AgentTurn::Settling => "settling",
        AgentTurn::Idle => "idle",
    }
}

/// The wire name of the predicate that selected an activity.
pub fn evidence_name(evidence: ActivityEvidence) -> &'static str {
    match evidence {
        ActivityEvidence::InterruptMarker => "interrupt-marker",
        ActivityEvidence::GenericLiveStatus => "generic-live-status",
        ActivityEvidence::ClaudeLiveStatus => "claude-live-status",
        ActivityEvidence::CodexLiveStatus => "codex-live-status",
        ActivityEvidence::BackgroundWait => "background-wait",
        ActivityEvidence::BackgroundTaskWait => "background-task-wait",
        ActivityEvidence::ConfirmationPrompt => "confirmation-prompt",
        ActivityEvidence::SelectionPrompt => "selection-prompt",
        ActivityEvidence::FolderTrustPrompt => "folder-trust-prompt",
        ActivityEvidence::NoActiveMarker => "no-active-marker",
    }
}

/// The wire name of goal evidence: a phase, `inactive`, or `unknown` (keep the previous goal).
pub fn goal_name(goal: GoalEvidence) -> &'static str {
    match goal {
        GoalEvidence::State(GoalState::Active) => "active",
        GoalEvidence::State(GoalState::Paused) => "paused",
        GoalEvidence::State(GoalState::Blocked) => "blocked",
        GoalEvidence::State(GoalState::UsageLimited) => "usage-limited",
        GoalEvidence::State(GoalState::Reached) => "reached",
        GoalEvidence::Inactive => "inactive",
        GoalEvidence::Unknown => "unknown",
    }
}

/// The wire name of the goal predicate family.
pub fn goal_rule_name(rule: GoalEvidenceRule) -> &'static str {
    match rule {
        GoalEvidenceRule::CodexFooterGoalSegment => "codex-footer-goal-segment",
        GoalEvidenceRule::CodexMetadataFooterWithoutGoal => "codex-metadata-footer-without-goal",
        GoalEvidenceRule::ClaudeGoalIndicator => "claude-goal-indicator",
        GoalEvidenceRule::ClaudePausedNotice => "claude-paused-notice",
        GoalEvidenceRule::ClaudeAchievedNotice => "claude-achieved-notice",
        GoalEvidenceRule::ClaudeFailedNotice => "claude-failed-notice",
        GoalEvidenceRule::ClaudeClearedNotice => "claude-cleared-notice",
        GoalEvidenceRule::ClaudeIndicatorWithoutGoal => "claude-indicator-without-goal",
    }
}

fn optional_text(text: Option<&str>) -> Value {
    text.map_or(Value::Null, |text| Value::String(text.to_owned()))
}

/// The verdict of one screen.
pub fn verdict_of(request: &ScreenRequest) -> Value {
    let class = AgentClass::from_signature(&request.signature);
    let activity = screen::classify_activity_for_agent_detailed(&class, &request.text);
    let goal = screen::goal_evidence_for_agent_detailed(&class, &request.text);
    let prompt_ready = match request.cursor {
        Some((row, column)) => screen::is_agent_prompt_ready_at_cursor(
            &class,
            &request.text,
            row,
            column,
            &request.dimmed,
        ),
        None => screen::is_agent_prompt_ready(&class, &request.text),
    };
    let mut verdict = Map::new();
    verdict.insert("pane".into(), json!(request.pane));
    verdict.insert("generation".into(), json!(request.generation));
    verdict.insert("signature".into(), json!(request.signature));
    verdict.insert("turn".into(), json!(turn_name(activity.turn)));
    verdict.insert("evidence".into(), json!(evidence_name(activity.evidence)));
    verdict.insert(
        "evidence_line".into(),
        optional_text(activity.matched_line.as_deref()),
    );
    verdict.insert("goal".into(), json!(goal_name(goal.evidence)));
    verdict.insert(
        "goal_line".into(),
        optional_text(goal.matched_line.as_deref()),
    );
    verdict.insert(
        "goal_rule".into(),
        optional_text(goal.matched_rule.map(goal_rule_name)),
    );
    verdict.insert("goal_pattern".into(), optional_text(goal.matched_pattern));
    verdict.insert(
        "fresh_screen".into(),
        json!(screen::is_fresh_agent_screen(&class, &request.text)),
    );
    verdict.insert(
        "interstitial_response".into(),
        optional_text(screen::interstitial_prompt_response(&class, &request.text)),
    );
    verdict.insert("prompt_ready".into(), json!(prompt_ready));
    Value::Object(verdict)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(screens: Value) -> String {
        json!({ "screens": screens }).to_string()
    }

    fn reply(payload: &str) -> Value {
        serde_json::from_str(&handle(CLASSIFY_BATCH, payload)).expect("reply is JSON")
    }

    #[test]
    fn a_batch_answers_every_screen_in_request_order() {
        let payload = request(json!([
            { "pane": 7, "generation": 3, "signature": "codex",
              "text": "older transcript\nWorking (esc to interrupt)\nSend a message" },
            { "pane": 9, "generation": 1, "signature": "claude", "text": "$ ready" },
        ]));
        let verdicts = reply(&payload)["verdicts"].clone();
        assert_eq!(verdicts[0]["pane"], 7);
        assert_eq!(verdicts[0]["generation"], 3);
        assert_eq!(verdicts[0]["turn"], "working");
        assert_eq!(verdicts[0]["evidence"], "interrupt-marker");
        assert!(verdicts[0]["evidence_line"]
            .as_str()
            .is_some_and(|line| line.contains("esc to interrupt")));
        assert_eq!(verdicts[1]["pane"], 9);
        assert_eq!(verdicts[1]["turn"], "idle");
        assert_eq!(verdicts[1]["evidence"], "no-active-marker");
        assert_eq!(verdicts[1]["evidence_line"], Value::Null);
    }

    #[test]
    fn a_goal_footer_reports_the_phase_rule_and_line() {
        let text = "› Ask Codex to do anything\n\n  gpt-5 high · Pursuing goal (5m)";
        let verdict = reply(&request(json!([
            { "pane": 1, "generation": 1, "signature": "codex", "text": text }
        ])))["verdicts"][0]
            .clone();
        let direct = screen::goal_evidence_for_agent_detailed(&AgentClass::Codex, text);
        assert_eq!(verdict["goal"], goal_name(direct.evidence));
        assert_eq!(
            verdict["goal_rule"],
            optional_text(direct.matched_rule.map(goal_rule_name))
        );
        assert_eq!(
            verdict["goal_pattern"],
            optional_text(direct.matched_pattern)
        );
    }

    #[test]
    fn an_unknown_signature_gets_unknown_goal_and_no_interstitial() {
        let verdict = reply(&request(json!([
            { "pane": 1, "generation": 1, "signature": "example-agent", "text": "hello" }
        ])))["verdicts"][0]
            .clone();
        assert_eq!(verdict["goal"], "unknown");
        assert_eq!(verdict["interstitial_response"], Value::Null);
        assert_eq!(verdict["fresh_screen"], false);
    }

    #[test]
    fn readiness_follows_the_cursor_when_one_is_given() {
        let text = ">_ OpenAI Codex\n› ";
        let with_cursor = verdict_of(&ScreenRequest {
            pane: 1,
            generation: 1,
            signature: "codex".into(),
            text: text.into(),
            cursor: Some((1, 2)),
            dimmed: Vec::new(),
        });
        assert_eq!(
            with_cursor["prompt_ready"],
            json!(screen::is_agent_prompt_ready_at_cursor(
                &AgentClass::Codex,
                text,
                1,
                2,
                &[]
            ))
        );
    }

    #[test]
    fn a_malformed_entry_refuses_the_whole_batch() {
        for payload in [
            "not json".to_owned(),
            json!({}).to_string(),
            request(json!([{ "pane": 1, "generation": 1, "signature": "", "text": "" }])),
            request(json!([{ "pane": 1, "generation": 1, "signature": "codex" }])),
            request(
                json!([{ "pane": 1, "generation": 1, "signature": "codex", "text": "",
                             "cursor": [1] }]),
            ),
            request(
                json!([{ "pane": 1, "generation": 1, "signature": "codex", "text": "",
                             "dimmed": [[1, 70000]] }]),
            ),
        ] {
            let answer = reply(&payload);
            assert_eq!(answer["error"]["code"], "invalid-argument", "{payload}");
        }
    }

    #[test]
    fn a_batch_over_the_cap_is_refused() {
        let screens: Vec<Value> = (0..=MAX_SCREENS as u64)
            .map(|pane| json!({ "pane": pane, "generation": 1, "signature": "codex", "text": "" }))
            .collect();
        assert_eq!(
            reply(&request(Value::Array(screens)))["error"]["code"],
            "invalid-argument"
        );
    }

    #[test]
    fn an_unknown_method_is_unsupported() {
        let answer: Value = serde_json::from_str(&handle("classify", "{}")).expect("reply is JSON");
        assert_eq!(answer["error"]["code"], "unsupported");
    }

    #[test]
    fn every_wire_name_is_distinct() {
        let turns = [
            AgentTurn::Working,
            AgentTurn::WaitingApproval,
            AgentTurn::WaitingSubagents,
            AgentTurn::Settling,
            AgentTurn::Idle,
        ]
        .map(turn_name);
        let mut unique = turns.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), turns.len());
    }
}
