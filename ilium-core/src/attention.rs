//! Shared attention priorities and stable evidence identity; no I/O.

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
