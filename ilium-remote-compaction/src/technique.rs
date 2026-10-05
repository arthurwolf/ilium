//! Prompt rendering, summary cleaning and summary validation per technique.
//! The prompt text lives in `ilium-prompts` templates; this module only picks
//! the template and supplies the variables.

use serde_json::{json, Value};

use crate::error::CompactionError;
use crate::types::Technique;

/// Shortest text accepted as a summary.
const MINIMUM_SUMMARY_CHARS: usize = 20;

/// Variables of one summarizer request.
#[derive(Debug, Clone, Copy, Default)]
pub struct PromptInputs<'a> {
    /// The rendered conversation (or chunk) to summarize.
    pub conversation: &'a str,
    /// Summary of everything before `conversation`, to be updated.
    pub prior_summary: Option<&'a str>,
    /// Code-computed facts (the ledger) the summary must treat as exact.
    pub facts: Option<&'a str>,
    /// Instruction text of the `Custom` technique.
    pub custom_prompt: Option<&'a str>,
    /// Which part of the history `conversation` is, such as `part 2 of 3`.
    pub part_label: Option<&'a str>,
    /// Why the previous reply was rejected, when this is a retry.
    pub retry_problem: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPrompt {
    pub system: String,
    pub user: String,
}

/// `Custom` with no instruction text behaves as `ClaudeCode`.
pub fn effective_technique(technique: Technique, custom_prompt: Option<&str>) -> Technique {
    let has_custom_text = custom_prompt.is_some_and(|text| !text.trim().is_empty());
    if technique == Technique::Custom && !has_custom_text {
        return Technique::ClaudeCode;
    }
    technique
}

fn system_template(technique: Technique) -> &'static str {
    match technique {
        Technique::ClaudeCode => "compaction/claude-code-system",
        Technique::Codex => "compaction/codex-system",
        Technique::Opencode => "compaction/opencode-system",
        Technique::GeminiCli => "compaction/gemini-cli-system",
        Technique::BestOfAllWorlds => "compaction/best-of-all-worlds-system",
        Technique::Custom => "compaction/custom-system",
    }
}

fn technique_flags(technique: Technique) -> Value {
    json!({
        "is_claude_code": technique == Technique::ClaudeCode,
        "is_codex": technique == Technique::Codex,
        "is_opencode": technique == Technique::Opencode,
        "is_gemini_cli": technique == Technique::GeminiCli,
        "is_best_of_all_worlds": technique == Technique::BestOfAllWorlds,
        "is_custom": technique == Technique::Custom,
    })
}

fn merge_into(base: &mut Value, extra: Value) {
    if let (Some(base), Value::Object(extra)) = (base.as_object_mut(), extra) {
        base.extend(extra);
    }
}

fn render_system(
    technique: Technique,
    custom_prompt: Option<&str>,
) -> Result<String, CompactionError> {
    let custom = custom_prompt.unwrap_or_default().replace(
        "{{conversation}}",
        "the conversation provided in the user message",
    );
    let rendered = ilium_prompts::render(
        system_template(technique),
        &json!({ "custom_prompt": custom }),
    )?;
    Ok(rendered.trim().to_string())
}

fn append_retry_note(
    user: &mut String,
    retry_problem: Option<&str>,
) -> Result<(), CompactionError> {
    if let Some(problem) = retry_problem {
        let note = ilium_prompts::render("compaction/retry-note", &json!({ "problem": problem }))?;
        user.push_str(note.trim_end());
    }
    Ok(())
}

/// The system and user prompt of one summarizer request for `technique`.
pub fn render_prompt(
    technique: Technique,
    inputs: &PromptInputs<'_>,
) -> Result<RenderedPrompt, CompactionError> {
    let technique = effective_technique(technique, inputs.custom_prompt);
    let system = render_system(technique, inputs.custom_prompt)?;
    let mut variables = json!({
        "conversation": inputs.conversation,
        "prior_summary": inputs.prior_summary.unwrap_or_default(),
        "facts": inputs.facts.unwrap_or_default(),
        "part_label": inputs.part_label.unwrap_or_default(),
    });
    merge_into(&mut variables, technique_flags(technique));
    let mut user = ilium_prompts::render("compaction/user", &variables)?
        .trim()
        .to_string();
    append_retry_note(&mut user, inputs.retry_problem)?;
    Ok(RenderedPrompt { system, user })
}

/// The prompt of the final call that merges the partial summaries of a
/// multi-chunk run into one.
pub fn render_merge_prompt(
    technique: Technique,
    summaries: &[String],
    custom_prompt: Option<&str>,
    retry_problem: Option<&str>,
) -> Result<RenderedPrompt, CompactionError> {
    let technique = effective_technique(technique, custom_prompt);
    let system = render_system(technique, custom_prompt)?;
    let numbered = summaries
        .iter()
        .enumerate()
        .map(|(index, summary)| format!("[part {}]\n{}", index + 1, summary.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut variables = json!({
        "summaries": numbered,
        "part_count": summaries.len().to_string(),
    });
    merge_into(&mut variables, technique_flags(technique));
    let mut user = ilium_prompts::render("compaction/merge-user", &variables)?
        .trim()
        .to_string();
    append_retry_note(&mut user, retry_problem)?;
    Ok(RenderedPrompt { system, user })
}

/// Removes every `<tag>...</tag>` block. An unterminated block removes the
/// rest of the text (the reply was cut off inside it).
fn strip_block(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        output.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        match after_open.find(&close) {
            Some(end) => rest = &after_open[end + close.len()..],
            None => return output,
        }
    }
    output.push_str(rest);
    output
}

/// The text between `<tag>` and `</tag>`; an unterminated block runs to the end.
fn inner_block<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let body = &text[start..];
    Some(body.find(&close).map_or(body, |end| &body[..end]))
}

/// Turns a raw model reply into the summary text that is stored: reasoning
/// blocks (`<analysis>`, `<scratchpad>`) are dropped and the format's own
/// wrapper is removed.
pub fn clean_summary(technique: Technique, raw: &str) -> String {
    let stripped = strip_block(&strip_block(raw, "analysis"), "scratchpad");
    let cleaned = match technique {
        Technique::ClaudeCode | Technique::BestOfAllWorlds => inner_block(&stripped, "summary")
            .unwrap_or(&stripped)
            .to_string(),
        Technique::GeminiCli => match stripped.find("<state_snapshot>") {
            Some(start) => {
                let from_start = &stripped[start..];
                match from_start.find("</state_snapshot>") {
                    Some(end) => from_start[..end + "</state_snapshot>".len()].to_string(),
                    None => from_start.to_string(),
                }
            }
            None => stripped,
        },
        Technique::Codex | Technique::Opencode | Technique::Custom => stripped,
    };
    cleaned.trim().to_string()
}

/// Checks a cleaned summary for the headings the technique requires. The
/// required set is deliberately small so format drift in a model's wording
/// does not discard a good summary.
pub fn validate_summary(technique: Technique, cleaned: &str) -> Result<(), String> {
    if cleaned.trim().chars().count() < MINIMUM_SUMMARY_CHARS {
        return Err("the reply was empty or too short to be a summary".to_string());
    }
    let lowered = cleaned.to_lowercase();
    let missing: Vec<&str> = match technique {
        Technique::ClaudeCode | Technique::BestOfAllWorlds => {
            ["primary request", "pending tasks", "current work"]
                .into_iter()
                .filter(|heading| !lowered.contains(heading))
                .collect()
        }
        Technique::Opencode => ["## objective", "## work state", "## next move"]
            .into_iter()
            .filter(|heading| !lowered.contains(heading))
            .collect(),
        Technique::GeminiCli => ["<state_snapshot>", "</state_snapshot>", "<task_state>"]
            .into_iter()
            .filter(|tag| !lowered.contains(tag))
            .collect(),
        Technique::Codex | Technique::Custom => Vec::new(),
    };
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "required parts are missing: {}",
            missing.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_REPLY: &str = "<analysis>thinking out loud</analysis>\n<summary>\n1. Primary Request and Intent:\n   build it\n7. Pending Tasks:\n   - none\n8. Current Work:\n   testing\n</summary>";

    fn inputs() -> PromptInputs<'static> {
        PromptInputs {
            conversation: "[turn 1 | user]\nhello",
            ..PromptInputs::default()
        }
    }

    #[test]
    fn every_technique_renders_with_the_system_operation_and_untrusted_guard() {
        for technique in Technique::ALL {
            let prompt = render_prompt(
                technique,
                &PromptInputs {
                    custom_prompt: Some("Write a haiku about {{conversation}}."),
                    ..inputs()
                },
            )
            .unwrap_or_else(|error| panic!("{technique:?}: {error}"));
            assert!(
                prompt.system.contains("automated system operation"),
                "{technique:?} lacks the system-operation guard"
            );
            assert!(
                prompt.system.contains("Do not call any tools"),
                "{technique:?}"
            );
            assert!(
                prompt.system.contains("untrusted historical data"),
                "{technique:?}"
            );
            assert!(prompt.user.contains("hello"), "{technique:?}");
            assert!(
                !prompt.system.contains("{{"),
                "{technique:?} left a placeholder"
            );
        }
    }

    #[test]
    fn custom_prompt_text_is_used_verbatim_and_conversation_placeholder_is_replaced() {
        let prompt = render_prompt(
            Technique::Custom,
            &PromptInputs {
                custom_prompt: Some("Summarize {{conversation}} as <b>bullets</b> & {{other}}."),
                ..inputs()
            },
        )
        .expect("renders");
        assert!(prompt.system.contains(
            "Summarize the conversation provided in the user message as <b>bullets</b> & {{other}}."
        ));
        assert!(prompt
            .user
            .contains("Write the summary now, following the instructions above."));
    }

    #[test]
    fn custom_without_text_falls_back_to_claude_code() {
        assert_eq!(
            effective_technique(Technique::Custom, Some("  \n")),
            Technique::ClaudeCode
        );
        assert_eq!(
            effective_technique(Technique::Custom, None),
            Technique::ClaudeCode
        );
        assert_eq!(
            effective_technique(Technique::Codex, None),
            Technique::Codex
        );
        let prompt = render_prompt(Technique::Custom, &inputs()).expect("renders");
        assert!(prompt.system.contains("Primary Request and Intent"));
    }

    #[test]
    fn prior_summary_facts_part_label_and_retry_note_reach_the_user_prompt() {
        let prompt = render_prompt(
            Technique::BestOfAllWorlds,
            &PromptInputs {
                prior_summary: Some("OLD SUMMARY"),
                facts: Some("Files read:\n- /a"),
                part_label: Some("part 2 of 3"),
                retry_problem: Some("it was empty"),
                ..inputs()
            },
        )
        .expect("renders");
        assert!(prompt
            .user
            .contains("<prior-summary>\nOLD SUMMARY\n</prior-summary>"));
        assert!(prompt.user.contains("<facts>\nFiles read:\n- /a\n</facts>"));
        assert!(prompt.user.contains("part 2 of 3"));
        assert!(prompt
            .user
            .ends_with("Reply again with the complete summary in exactly the required format."));
        let plain = render_prompt(Technique::Codex, &inputs()).expect("renders");
        assert!(!plain.user.contains("<prior-summary>"));
        assert!(!plain.user.contains("<facts>"));
    }

    #[test]
    fn merge_prompt_numbers_the_partial_summaries() {
        let prompt = render_merge_prompt(
            Technique::Opencode,
            &["first".to_string(), "second".to_string()],
            None,
            None,
        )
        .expect("renders");
        assert!(prompt.user.contains("[part 1]\nfirst\n\n[part 2]\nsecond"));
        assert!(prompt.user.contains("in 2 parts"));
        assert!(prompt.system.contains("## Objective"));
    }

    #[test]
    fn claude_reply_is_stripped_of_analysis_and_summary_tags() {
        let cleaned = clean_summary(Technique::ClaudeCode, CLAUDE_REPLY);
        assert!(cleaned.starts_with("1. Primary Request and Intent:"));
        assert!(!cleaned.contains("<analysis>") && !cleaned.contains("<summary>"));
        assert!(!cleaned.contains("thinking out loud"));
        assert_eq!(validate_summary(Technique::ClaudeCode, &cleaned), Ok(()));
    }

    #[test]
    fn unterminated_blocks_and_scratchpads_are_handled() {
        let cut = "<analysis>never closed 1. Primary Request";
        assert_eq!(clean_summary(Technique::ClaudeCode, cut), "");
        let no_close = "<summary>\n1. Primary Request and Intent: x\nPending Tasks\nCurrent Work";
        assert!(clean_summary(Technique::BestOfAllWorlds, no_close).starts_with("1. Primary"));
        let gemini = "<scratchpad>notes</scratchpad>\n<state_snapshot>\n<overall_goal>g</overall_goal>\n<task_state>t</task_state>\n</state_snapshot>\ntrailing chatter";
        let cleaned = clean_summary(Technique::GeminiCli, gemini);
        assert!(cleaned.starts_with("<state_snapshot>") && cleaned.ends_with("</state_snapshot>"));
        assert_eq!(validate_summary(Technique::GeminiCli, &cleaned), Ok(()));
    }

    #[test]
    fn validators_name_what_is_missing() {
        let error = validate_summary(
            Technique::ClaudeCode,
            "1. Primary Request and Intent: x and more text here",
        )
        .expect_err("pending tasks and current work are absent");
        assert!(error.contains("pending tasks") && error.contains("current work"));
        assert!(validate_summary(Technique::Codex, "short").is_err());
        assert!(validate_summary(
            Technique::Codex,
            "A perfectly fine plain-text handoff summary."
        )
        .is_ok());
        let opencode = "## Objective\n- x\n## Work State\n### Active\n- y\n## Next Move\n1. z";
        assert!(validate_summary(Technique::Opencode, opencode).is_ok());
        assert!(
            validate_summary(Technique::Opencode, "## Objective\n- x and some more words").is_err()
        );
        assert!(validate_summary(
            Technique::GeminiCli,
            "<state_snapshot>half open and long enough"
        )
        .is_err());
    }
}
