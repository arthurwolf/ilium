//! One attention item and the stable fingerprint of the evidence behind it.

use ilium_core::NodeId;
use serde::{Deserialize, Serialize};

/// Why an agent needs the user. Declaration order is the priority order:
/// a question outranks being stuck, which outranks waiting, which outranks a
/// plain change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    Question,
    Stuck,
    WaitingOn,
    Changed,
}

impl AttentionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Question => "Question",
            Self::Stuck => "Stuck",
            Self::WaitingOn => "Waiting",
            Self::Changed => "Changed",
        }
    }

    /// Parses the kind names the LLM is instructed to use.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "question" | "asking" => Some(Self::Question),
            "stuck" | "failing" => Some(Self::Stuck),
            "waiting_on" | "waiting" | "waitingon" => Some(Self::WaitingOn),
            "changed" | "change" => Some(Self::Changed),
            _ => None,
        }
    }
}

/// Where an item came from. An LLM item is annotation on top of detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionSource {
    Instant,
    Llm,
}

/// Longest briefing field kept, in characters.
pub const MAXIMUM_BRIEFING_CHARACTERS: usize = 300;

/// Plain-words account of why the user was brought to an agent, shown in the
/// briefing box at the top of the agent pane. Every field is optional because
/// the LLM may omit any of them and an instant-only item carries only what
/// detection knows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Briefing {
    /// One or two sentences on what the agent is doing or has done.
    pub status_summary: Option<String>,
    /// Why this agent needs the user.
    pub why_here: Option<String>,
    /// The short next step the user probably wants to take.
    pub suggested_action: Option<String>,
    /// True when the text was written by the LLM; false when it is the
    /// deterministic wording derived from agent detection.
    pub from_llm: bool,
}

impl Briefing {
    pub fn is_empty(&self) -> bool {
        self.status_summary.is_none() && self.why_here.is_none() && self.suggested_action.is_none()
    }

    /// Keeps this briefing's fields and fills the missing ones from `other`.
    /// The result counts as LLM wording only when this briefing already was.
    pub fn filled_from(mut self, other: &Briefing) -> Briefing {
        if self.status_summary.is_none() {
            self.status_summary = other.status_summary.clone();
        }
        if self.why_here.is_none() {
            self.why_here = other.why_here.clone();
        }
        if self.suggested_action.is_none() {
            self.suggested_action = other.suggested_action.clone();
        }
        if self.is_empty() {
            self.from_llm = false;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionItem {
    pub pane_id: NodeId,
    pub project_id: Option<NodeId>,
    pub title: String,
    pub kind: AttentionKind,
    pub reason: String,
    pub question: Option<String>,
    pub waiting_on: Option<NodeId>,
    /// Why the user is brought here, for the briefing box. Absent in stores
    /// written before the briefing existed.
    #[serde(default)]
    pub briefing: Briefing,
    /// 0..=100; only orders items of the same kind.
    pub priority: u8,
    pub source: AttentionSource,
    /// Fingerprint of the pane evidence this item was computed from.
    pub fingerprint: u64,
    /// Refresh generation that produced the item.
    pub generation: u64,
    /// Unix milliseconds when the item first appeared.
    pub created_ms: u64,
}

/// FNV-1a over length-prefixed parts. Unlike `DefaultHasher` it is stable across
/// runs and builds, so persisted fingerprints still compare after a restart.
pub fn fingerprint_parts(parts: &[&[u8]]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for part in parts {
        for byte in (part.len() as u64).to_le_bytes().iter().chain(part.iter()) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_order_is_question_stuck_waiting_changed() {
        assert!(AttentionKind::Question < AttentionKind::Stuck);
        assert!(AttentionKind::Stuck < AttentionKind::WaitingOn);
        assert!(AttentionKind::WaitingOn < AttentionKind::Changed);
    }

    #[test]
    fn kind_parse_accepts_aliases_and_rejects_unknown() {
        assert_eq!(
            AttentionKind::parse(" Question "),
            Some(AttentionKind::Question)
        );
        assert_eq!(AttentionKind::parse("failing"), Some(AttentionKind::Stuck));
        assert_eq!(
            AttentionKind::parse("waiting_on"),
            Some(AttentionKind::WaitingOn)
        );
        assert_eq!(AttentionKind::parse("bogus"), None);
    }

    #[test]
    fn fingerprint_is_stable_and_boundary_sensitive() {
        assert_eq!(
            fingerprint_parts(&[b"ab", b"c"]),
            fingerprint_parts(&[b"ab", b"c"])
        );
        assert_ne!(
            fingerprint_parts(&[b"ab", b"c"]),
            fingerprint_parts(&[b"a", b"bc"])
        );
        // Pinned value guards against accidental algorithm changes, which would
        // silently invalidate every persisted queue.
        assert_eq!(fingerprint_parts(&[]), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn filled_from_keeps_own_fields_and_clears_the_llm_flag_when_empty() {
        let own = Briefing {
            why_here: Some("own".into()),
            from_llm: true,
            ..Briefing::default()
        };
        let other = Briefing {
            why_here: Some("other".into()),
            suggested_action: Some("act".into()),
            ..Briefing::default()
        };
        let merged = own.filled_from(&other);
        assert_eq!(merged.why_here.as_deref(), Some("own"));
        assert_eq!(merged.suggested_action.as_deref(), Some("act"));
        let empty = Briefing {
            from_llm: true,
            ..Briefing::default()
        }
        .filled_from(&Briefing::default());
        assert!(!empty.from_llm);
    }
}
