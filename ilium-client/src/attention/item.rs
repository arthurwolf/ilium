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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionItem {
    pub pane_id: NodeId,
    pub project_id: Option<NodeId>,
    pub title: String,
    pub kind: AttentionKind,
    pub reason: String,
    pub question: Option<String>,
    pub waiting_on: Option<NodeId>,
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
}
