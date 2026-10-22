//! Dedup keys of `send-input` and `agent-action` (reference-events-and-intents.md,
//! requests-and-outcomes.md "Duplicates").

use crate::generated::types::PluginError;

/// A checked dedup key: 1 to 128 characters from `[A-Za-z0-9._:/-]` (ABI-010).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DedupKey(String);

impl DedupKey {
    /// ABI-010: `[A-Za-z0-9._:/-]`, 1..=128 characters; anything else is `invalid-argument`.
    pub fn parse(text: &str) -> Result<DedupKey, PluginError> {
        if text.is_empty() || text.len() > DEDUP_KEY_MAX_CHARS ||
            !text.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte)) {
            return Err(PluginError { code: crate::ErrorCode::InvalidArgument,
                message: "dedup key must contain 1 to 128 ASCII letters, digits or ._:/-".into(), retry_after_ms: None });
        }
        Ok(DedupKey(text.to_owned()))
    }

    /// The key text (ABI-010).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Longest dedup key in characters (ABI-010).
pub const DEDUP_KEY_MAX_CHARS: usize = 128;

/// Characters a dedup key may use besides ASCII letters and digits (ABI-010).
pub const DEDUP_KEY_PUNCTUATION: &str = "._:/-";

/// How long an accepted key stays reserved, on the wall clock, in milliseconds (ABI-034).
pub const DEDUP_WINDOW_MS: u64 = 600_000;

/// ABI-034: a key accepted at `accepted_wall_ms` is still reserved at `now_wall_ms` while
/// `now_wall_ms - accepted_wall_ms < 600,000`. A clock that went backwards keeps it reserved.
pub fn is_reserved(accepted_wall_ms: u64, now_wall_ms: u64) -> bool {
    now_wall_ms.saturating_sub(accepted_wall_ms) < DEDUP_WINDOW_MS
}
