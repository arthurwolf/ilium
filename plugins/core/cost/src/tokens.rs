//! Token counters shared by the price model and the statistics replies.

/// Token counts normalised across providers: `input` is uncached prompt input, `output` includes `reasoning`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TokenTotals {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
    /// Subset of `output` spent on hidden reasoning, where the provider says.
    #[serde(default)]
    pub reasoning: u64,
}

impl TokenTotals {
    /// Every counted token.
    pub fn total(&self) -> u64 {
        self.input + self.cache_read + self.cache_write + self.output
    }
}

/// Usage attributed to one model over a session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct ModelUsage {
    pub model: String,
    #[serde(default)]
    pub calls: u32,
    pub tokens: TokenTotals,
}
