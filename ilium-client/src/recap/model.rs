//! Typed recap result: per-pane state, validated text and pointers, and the
//! parser for the model's JSON answer. The model never decides states or goal
//! phases; those come from evidence. The model only writes prose.

use serde::Deserialize;

use super::evidence::{GoalRecord, RecapGoalState};

pub const SHORT_WORD_LIMIT: usize = 40;
pub const LONG_WORD_LIMIT: usize = 250;
pub const SHORT_TIMEOUT_SECONDS: u64 = 10;
pub const LONG_TIMEOUT_SECONDS: u64 = 60;
pub const POINTERS_PER_TEXT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecapLength {
    Short,
    Long,
}

impl RecapLength {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::Long => "long",
        }
    }

    pub const fn word_limit(self) -> usize {
        match self {
            Self::Short => SHORT_WORD_LIMIT,
            Self::Long => LONG_WORD_LIMIT,
        }
    }

    pub const fn timeout_seconds(self) -> u64 {
        match self {
            Self::Short => SHORT_TIMEOUT_SECONDS,
            Self::Long => LONG_TIMEOUT_SECONDS,
        }
    }
}

/// A file or resource the recap refers to. Only pointers that survived
/// validation reach a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecapPointer {
    pub path: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecapText {
    pub text: String,
    pub pointers: Vec<RecapPointer>,
    /// Pointers the model proposed that failed validation.
    pub rejected_pointers: usize,
}

/// Why a pane has no model text, or why its text is only partial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecapPaneState {
    /// Evidence was summarized and the model answered.
    Summarized,
    /// The pane has no current activity or evidence to describe.
    Idle,
    /// Evidence exists but its location is protected, so nothing was read.
    Denied,
    /// No verified evidence source exists for this pane.
    Unavailable,
    /// The pane or its session changed while inference ran.
    Stale,
    /// Inference or parsing failed. Deterministic fields are still present.
    Failed,
}

impl RecapPaneState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Summarized => "summarized",
            Self::Idle => "idle",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
            Self::Stale => "stale",
            Self::Failed => "failed",
        }
    }
}

/// Deterministic facts about one pane, rendered next to the model text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRecapFacts {
    pub goals: Vec<GoalRecord>,
    pub goals_omitted: usize,
    /// Seconds since the newest evidence was written, when known.
    pub evidence_age_seconds: Option<u64>,
    pub evidence_stale: bool,
    pub window_truncated: bool,
    pub redactions: usize,
}

impl PaneRecapFacts {
    /// Goal-state line such as `Goal done: ship recap`. Built here, never by
    /// the model, so completion is never inferred from prose.
    pub fn goal_lines(&self) -> Vec<String> {
        self.goals
            .iter()
            .map(|goal| match goal.state {
                RecapGoalState::Done => format!("Goal done: {}", goal.objective),
                RecapGoalState::Active => format!("Goal active: {}", goal.objective),
                RecapGoalState::Paused => format!("Goal paused: {}", goal.objective),
                RecapGoalState::Blocked => format!("Goal blocked: {}", goal.objective),
                RecapGoalState::Unknown => format!("Goal state unknown: {}", goal.objective),
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRecap {
    pub pane_id: String,
    pub pane_name: String,
    pub state: RecapPaneState,
    pub facts: PaneRecapFacts,
    pub short: Option<RecapText>,
    pub long: Option<RecapText>,
    /// Label for the failure, when `state` is `Failed` or partial.
    pub failure: Option<String>,
}

/// Words are whitespace-separated tokens. Markdown bullets count as words,
/// which keeps the count conservative.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelResponseError {
    NotJson(String),
    MissingLength(&'static str),
    EmptyText(&'static str),
    TooLong {
        length: &'static str,
        words: usize,
        limit: usize,
    },
}

impl std::fmt::Display for ModelResponseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotJson(detail) => {
                write!(formatter, "model answer is not the expected JSON: {detail}")
            }
            Self::MissingLength(length) => write!(formatter, "model answer has no {length} text"),
            Self::EmptyText(length) => write!(formatter, "model {length} text is empty"),
            Self::TooLong {
                length,
                words,
                limit,
            } => {
                write!(
                    formatter,
                    "model {length} text has {words} words, limit {limit}"
                )
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawResponse {
    short: Option<RawText>,
    long: Option<RawText>,
}

#[derive(Debug, Deserialize)]
struct RawText {
    text: String,
    #[serde(default)]
    pointers: Vec<RawPointer>,
}

#[derive(Debug, Deserialize)]
struct RawPointer {
    path: String,
    #[serde(default)]
    note: String,
}

/// Parsed model answer before pointer validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRecapResponse {
    pub short: RecapText,
    pub long: RecapText,
}

/// Parses `{"short": {...}, "long": {...}}`. Length limits are enforced here;
/// an answer over the limit is rejected, not cut, so the model text is never
/// silently truncated mid-sentence.
pub fn parse_recap_response(raw: &str) -> Result<ParsedRecapResponse, ModelResponseError> {
    let json = extract_json_object(raw).unwrap_or(raw);
    let response: RawResponse = serde_json::from_str(json)
        .map_err(|error| ModelResponseError::NotJson(error.to_string()))?;
    Ok(ParsedRecapResponse {
        short: validate_text(response.short, RecapLength::Short)?,
        long: validate_text(response.long, RecapLength::Long)?,
    })
}

fn validate_text(
    raw: Option<RawText>,
    length: RecapLength,
) -> Result<RecapText, ModelResponseError> {
    let name = length.name();
    let raw = raw.ok_or(ModelResponseError::MissingLength(name))?;
    let text = raw.text.trim().to_string();
    if text.is_empty() {
        return Err(ModelResponseError::EmptyText(name));
    }
    let words = word_count(&text);
    if words > length.word_limit() {
        return Err(ModelResponseError::TooLong {
            length: name,
            words,
            limit: length.word_limit(),
        });
    }
    let pointers = raw
        .pointers
        .into_iter()
        .filter(|pointer| !pointer.path.trim().is_empty())
        .take(POINTERS_PER_TEXT)
        .map(|pointer| RecapPointer {
            path: pointer.path.trim().to_string(),
            note: pointer.note.trim().to_string(),
        })
        .collect();
    Ok(RecapText {
        text,
        pointers,
        rejected_pointers: 0,
    })
}

/// Models sometimes wrap JSON in a code fence or prose. Takes the outermost
/// object span; `None` when there is no brace pair.
fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end > start).then(|| &raw[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(words: usize) -> String {
        vec!["word"; words].join(" ")
    }

    #[test]
    fn parses_both_lengths_inside_a_code_fence() {
        let raw = format!(
            "```json\n{{\"short\": {{\"text\": \"{}\", \"pointers\": []}}, \"long\": {{\"text\": \"{}\", \"pointers\": [{{\"path\": \"src/lib.rs\", \"note\": \"edited\"}}]}}}}\n```",
            text_of(10),
            text_of(100)
        );
        let parsed = parse_recap_response(&raw).expect("valid");
        assert_eq!(word_count(&parsed.short.text), 10);
        assert_eq!(parsed.long.pointers[0].path, "src/lib.rs");
    }

    #[test]
    fn rejects_text_over_the_word_limit_instead_of_truncating() {
        let raw = format!(
            "{{\"short\": {{\"text\": \"{}\"}}, \"long\": {{\"text\": \"ok\"}}}}",
            text_of(SHORT_WORD_LIMIT + 1)
        );
        assert_eq!(
            parse_recap_response(&raw),
            Err(ModelResponseError::TooLong {
                length: "short",
                words: SHORT_WORD_LIMIT + 1,
                limit: SHORT_WORD_LIMIT,
            })
        );
    }

    #[test]
    fn rejects_missing_empty_and_malformed_answers() {
        assert!(matches!(
            parse_recap_response("{\"short\": {\"text\": \"a\"}}"),
            Err(ModelResponseError::MissingLength("long"))
        ));
        assert!(matches!(
            parse_recap_response("{\"short\": {\"text\": \"  \"}, \"long\": {\"text\": \"a\"}}"),
            Err(ModelResponseError::EmptyText("short"))
        ));
        assert!(matches!(
            parse_recap_response("no json here"),
            Err(ModelResponseError::NotJson(_))
        ));
    }

    #[test]
    fn goal_lines_come_from_recorded_state_only() {
        let facts = PaneRecapFacts {
            goals: vec![
                GoalRecord {
                    objective: "ship recap".into(),
                    state: RecapGoalState::Done,
                },
                GoalRecord {
                    objective: "old goal".into(),
                    state: RecapGoalState::Unknown,
                },
            ],
            goals_omitted: 0,
            evidence_age_seconds: Some(5),
            evidence_stale: false,
            window_truncated: false,
            redactions: 0,
        };
        assert_eq!(
            facts.goal_lines(),
            vec![
                "Goal done: ship recap".to_string(),
                "Goal state unknown: old goal".to_string()
            ]
        );
    }
}
