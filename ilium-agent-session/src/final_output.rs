//! Pure final-reply evidence from a caller-verified main transcript.

use ilium_core::AgentClass;
use serde_json::Value;

#[derive(Default)]
pub struct FinalOutput {
    text: Option<String>,
    latest: Option<String>,
}

impl FinalOutput {
    pub fn observe(&mut self, provider: &AgentClass, entry: &Value) {
        match provider {
            AgentClass::Codex => {
                let payload = &entry["payload"];
                match (entry["type"].as_str(), payload["type"].as_str()) {
                    (Some("event_msg"), Some("task_started" | "turn_aborted")) => {
                        self.text = None;
                        self.latest = None;
                    }
                    (Some("event_msg"), Some("task_complete")) => {
                        self.set(payload["last_agent_message"].as_str().into_iter());
                    }
                    (Some("response_item"), Some("message")) => {
                        if payload["role"].as_str() == Some("user") {
                            self.text = None;
                            self.latest = None;
                        }
                        if payload["role"].as_str() == Some("assistant") {
                            self.latest =
                                bounded_latest(text_parts(&payload["content"], "output_text"));
                        }
                        if payload["role"].as_str() == Some("assistant")
                            && payload["channel"].as_str() == Some("final")
                        {
                            self.set(text_parts(&payload["content"], "output_text"));
                        }
                    }
                    _ => {}
                }
            }
            AgentClass::Claude => match entry["type"].as_str() {
                Some("user") if is_user_request(&entry["message"]["content"]) => {
                    self.text = None;
                    self.latest = None;
                }
                Some("assistant") => {
                    self.text = None;
                    self.latest = bounded_latest(text_parts(&entry["message"]["content"], "text"));
                    if entry["message"]["stop_reason"].as_str() == Some("end_turn") {
                        self.set(text_parts(&entry["message"]["content"], "text"));
                    }
                }
                _ => {}
            },
            _ => {
                self.text = None;
                self.latest = None;
            }
        }
    }

    pub fn latest_text(&self) -> Option<&str> {
        self.latest.as_deref()
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    fn set<'a>(&mut self, parts: impl Iterator<Item = &'a str> + Clone) {
        self.text = bounded_text(parts.clone());
        self.latest = bounded_latest(parts);
    }
}

fn is_user_request(content: &Value) -> bool {
    content.is_string()
        || content.as_array().is_some_and(|parts| {
            parts
                .iter()
                .any(|part| part["type"].as_str() != Some("tool_result"))
        })
}

// Keep the actual ending for question evidence; the final-reply preview
// remains a prefix. Storage is bounded even when a verified record is large.
fn bounded_latest<'a>(parts: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut tail = std::collections::VecDeque::with_capacity(300);
    let mut spaces = 0usize;
    for character in parts.flat_map(|part| part.chars().chain(std::iter::once(' '))) {
        if character.is_whitespace() || character.is_control() {
            spaces = spaces.saturating_add(1).min(300);
            continue;
        }
        for character in std::iter::repeat_n(' ', spaces).chain(std::iter::once(character)) {
            if tail.len() == 300 {
                tail.pop_front();
            }
            tail.push_back(character);
        }
        spaces = 0;
    }
    let text: String = tail.into_iter().collect();
    (!text.trim().is_empty()).then(|| text.trim().to_owned())
}

fn bounded_text<'a>(parts: impl Iterator<Item = &'a str>) -> Option<String> {
    let text: String = parts
        .flat_map(|part| part.chars().chain(std::iter::once(' ')))
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(300)
        .collect();
    (!text.trim().is_empty()).then(|| text.trim().to_owned())
}

fn text_parts<'a>(content: &'a Value, kind: &'a str) -> impl Iterator<Item = &'a str> + Clone {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(move |part| part["type"].as_str() == Some(kind))
        .filter_map(|part| part["text"].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn review_question_evidence_uses_the_actual_message_ending() {
        for (message, question) in [
            (
                format!("{} Which target?", "Explanation. ".repeat(60)),
                true,
            ),
            (
                format!("{}? This is the completed result.", "x".repeat(299)),
                false,
            ),
        ] {
            let mut output = FinalOutput::default();
            output.observe(
                &AgentClass::Codex,
                &json!({"type":"response_item","payload":{
                "type":"message","role":"assistant","channel":"final",
                "content":[{"type":"output_text","text":message}]}}),
            );
            assert_eq!(
                output.latest_text().unwrap().trim_end().ends_with('?'),
                question
            );
            assert!(output.latest_text().unwrap().chars().count() <= 300);
        }
    }

    #[test]
    fn claude_array_user_requests_invalidate_previous_replies_but_tool_results_do_not() {
        let mut output = FinalOutput::default();
        output.observe(
            &AgentClass::Claude,
            &json!({"type":"assistant","message":{
            "stop_reason":"end_turn","content":[{"type":"text","text":"Previous answer"}]}}),
        );
        output.observe(
            &AgentClass::Claude,
            &json!({"type":"user","message":{
            "content":[{"type":"tool_result","tool_use_id":"fixture","content":"tool result"}]}}),
        );
        assert_eq!(output.latest_text(), Some("Previous answer"));
        output.observe(
            &AgentClass::Claude,
            &json!({"type":"user","message":{
            "content":[{"type":"text","text":"A new request"}]}}),
        );
        assert_eq!(output.text(), None);
        assert_eq!(output.latest_text(), None);
    }

    #[test]
    fn codex_final_channel_is_distinct_from_commentary_and_prior_turns() {
        let mut output = FinalOutput::default();
        output.observe(
            &AgentClass::Codex,
            &json!({"type":"response_item","payload":{
            "type":"message","role":"assistant","channel":"commentary",
            "content":[{"type":"output_text","text":"Working on fixture"}]}}),
        );
        assert_eq!(output.text(), None);
        output.observe(
            &AgentClass::Codex,
            &json!({"type":"response_item","payload":{
            "type":"message","role":"assistant","channel":"final",
            "content":[{"type":"output_text","text":"Fixture result verified"}]}}),
        );
        assert_eq!(output.text(), Some("Fixture result verified"));
        output.observe(
            &AgentClass::Codex,
            &json!({"type":"event_msg","payload":{"type":"task_started"}}),
        );
        assert_eq!(output.text(), None);
    }

    #[test]
    fn claude_requires_an_end_turn_reply_and_ignores_tool_results() {
        let mut output = FinalOutput::default();
        output.observe(
            &AgentClass::Claude,
            &json!({"type":"assistant","message":{
            "stop_reason":"tool_use","content":[{"type":"text","text":"checking fixture"}]}}),
        );
        assert_eq!(output.text(), None);
        output.observe(
            &AgentClass::Claude,
            &json!({"type":"assistant","message":{
            "stop_reason":"end_turn","content":[{"type":"text","text":"Fixture passed"}]}}),
        );
        assert_eq!(output.text(), Some("Fixture passed"));
        output.observe(
            &AgentClass::Claude,
            &json!({"type":"user","message":{"content":"Next fixture"}}),
        );
        assert_eq!(output.text(), None);
    }

    #[test]
    fn final_reply_is_bounded_and_never_contains_terminal_control_characters() {
        let mut output = FinalOutput::default();
        output.observe(&AgentClass::Codex, &json!({"type":"event_msg","payload":{
            "type":"task_complete","last_agent_message":format!("\u{1b}[2J\n{}", "é".repeat(400))}}));
        let text = output.text().unwrap();
        assert!(text.chars().count() <= 300);
        assert!(!text.chars().any(char::is_control));
    }
    #[test]
    fn latest_message_includes_commentary_without_claiming_a_final_reply() {
        let mut output = FinalOutput::default();
        output.observe(
            &AgentClass::Codex,
            &json!({"type":"response_item","payload":{
            "type":"message","role":"assistant","channel":"commentary",
            "content":[{"type":"output_text","text":"Inspecting the fixture"}]}}),
        );
        assert_eq!(output.latest_text(), Some("Inspecting the fixture"));
        assert_eq!(output.text(), None);
        output.observe(
            &AgentClass::Codex,
            &json!({"type":"event_msg","payload":{"type":"task_started"}}),
        );
        assert_eq!(output.latest_text(), None);
        output.observe(
            &AgentClass::Claude,
            &json!({"type":"assistant","message":{
            "stop_reason":"tool_use","content":[{"type":"text","text":"Checking fixture"}]}}),
        );
        assert_eq!(output.latest_text(), Some("Checking fixture"));
        assert_eq!(output.text(), None);
    }
}
