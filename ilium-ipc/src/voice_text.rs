//! Wire types for typed "spoken" sentences: text delivered into a running
//! voice session as if the microphone had produced it.
//!
//! The voice session itself lives in an attached `ilium-client` (it owns the
//! audio devices and the provider connection), so `ilium voice say` cannot
//! reach it directly. The one-shot CLI submits the sentences to the server,
//! the server offers them to voice-capable clients one at a time, and the
//! accepting client reports the outcome back. These types are the payloads of
//! that exchange; the messages that carry them are
//! `ClientRequest::{RegisterVoiceTextReceiver, SubmitVoiceText,
//! AnswerVoiceText}` and `ServerEvent::{VoiceTextOffered, VoiceTextResult}`.
//!
//! This crate stays provider-neutral: nothing here names OpenAI or an audio
//! device, and the session phase is a small closed vocabulary rather than the
//! voice crate's own state type.

use serde::{Deserialize, Serialize};

/// Most sentences one request may carry.
pub const MAX_VOICE_TEXT_SENTENCES: usize = 32;
/// Most Unicode scalar values one sentence may carry (after trimming). A
/// spoken utterance is short; the bound keeps a runaway pipe from becoming
/// one enormous model turn.
pub const MAX_VOICE_TEXT_SENTENCE_CHARS: usize = 4_000;

/// Why a request to say something to the voice session was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceTextRejectionCode {
    /// The request itself is malformed (no sentences, an empty or oversized
    /// sentence, too many sentences).
    InvalidRequest,
    /// No attached client can host a voice session (only headless or older
    /// clients are connected, or none at all).
    NoVoiceClient,
    /// Voice control is switched off and the request did not ask to start it.
    VoiceOff,
    /// Voice control is enabled but its session is not running (failed to
    /// start or ended); the message carries the reason.
    VoiceUnavailable,
    /// A client was offered the text and never answered in time.
    ClientUnresponsive,
}

/// A structured refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceTextRejection {
    pub code: VoiceTextRejectionCode,
    pub message: String,
}

impl VoiceTextRejection {
    pub fn new(code: VoiceTextRejectionCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Where the voice session was when it took the text. Acceptance means the
/// text was handed to the live session, not that the model has answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceTextPhase {
    /// The session is starting or reconnecting; the text waits in its queue.
    Connecting,
    Listening,
    Recording,
    Thinking,
    Speaking,
}

/// Positive acknowledgement from the client that owns the voice session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceTextAccepted {
    pub sentence_count: u32,
    pub phase: VoiceTextPhase,
    /// True when this request switched voice control on.
    pub started_voice: bool,
}

/// The correlated outcome of one request.
pub type VoiceTextResult = Result<VoiceTextAccepted, VoiceTextRejection>;

/// Trims every sentence and enforces the request limits. Both the CLI (before
/// sending) and the server (before offering) call this, so a request that
/// reaches a client is always well-formed.
pub fn normalize_voice_sentences(
    sentences: Vec<String>,
) -> Result<Vec<String>, VoiceTextRejection> {
    let invalid =
        |message: String| VoiceTextRejection::new(VoiceTextRejectionCode::InvalidRequest, message);
    if sentences.is_empty() {
        return Err(invalid("no sentences to say".to_owned()));
    }
    if sentences.len() > MAX_VOICE_TEXT_SENTENCES {
        return Err(invalid(format!(
            "{} sentences exceed the limit of {MAX_VOICE_TEXT_SENTENCES} per request",
            sentences.len()
        )));
    }
    let mut normalized = Vec::with_capacity(sentences.len());
    for (index, sentence) in sentences.into_iter().enumerate() {
        let trimmed = sentence.trim();
        if trimmed.is_empty() {
            return Err(invalid(format!("sentence {} is empty", index + 1)));
        }
        let length = trimmed.chars().count();
        if length > MAX_VOICE_TEXT_SENTENCE_CHARS {
            return Err(invalid(format!(
                "sentence {} has {length} characters, over the limit of {MAX_VOICE_TEXT_SENTENCE_CHARS}",
                index + 1
            )));
        }
        normalized.push(trimmed.to_owned());
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_are_trimmed_and_kept_in_order() {
        let normalized =
            normalize_voice_sentences(vec!["  open settings ".to_owned(), "close it\n".to_owned()])
                .expect("valid sentences");
        assert_eq!(normalized, ["open settings", "close it"]);
    }

    #[test]
    fn malformed_requests_are_rejected_with_the_invalid_request_code() {
        let too_long = "x".repeat(MAX_VOICE_TEXT_SENTENCE_CHARS + 1);
        let too_many = vec!["hi".to_owned(); MAX_VOICE_TEXT_SENTENCES + 1];
        for candidate in [
            Vec::new(),
            vec!["   ".to_owned()],
            vec!["fine".to_owned(), String::new()],
            vec![too_long],
            too_many,
        ] {
            let rejection = normalize_voice_sentences(candidate).expect_err("must be rejected");
            assert_eq!(rejection.code, VoiceTextRejectionCode::InvalidRequest);
        }
    }

    #[test]
    fn the_length_limit_counts_characters_not_bytes() {
        let at_limit = "é".repeat(MAX_VOICE_TEXT_SENTENCE_CHARS);
        assert!(normalize_voice_sentences(vec![at_limit]).is_ok());
    }
}
