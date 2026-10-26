//! Package trust tiers (security.md, glossary.md "Tier").

use serde::{Deserialize, Serialize};

use crate::error::UnknownName;

/// Trust tier of a package: T1 bundled, T2 signed third party, T3 unsigned local (security.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    /// Bundled first-party code, in-process (security.md).
    T1,
    /// Signed third-party packages, sandboxed helper (security.md).
    T2,
    /// Unsigned local and dev packages, sandboxed helper (security.md).
    T3,
}

impl Tier {
    /// Every tier in order (security.md).
    pub const ALL: [Tier; 3] = [Tier::T1, Tier::T2, Tier::T3];

    /// The name the manual uses: `T1`, `T2`, `T3` (security.md).
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::T1 => "T1",
            Tier::T2 => "T2",
            Tier::T3 => "T3",
        }
    }
}

impl std::fmt::Display for Tier {
    /// `T1`, `T2` or `T3` (security.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for Tier {
    type Err = UnknownName;

    /// Parses `T1`, `T2` or `T3` (security.md).
    fn from_str(text: &str) -> Result<Tier, UnknownName> {
        Tier::ALL
            .into_iter()
            .find(|tier| tier.as_str() == text)
            .ok_or_else(|| UnknownName::new("tier", text))
    }
}
