//! Capability grants and the grant table (reference-capabilities.md).

use serde::{Deserialize, Serialize};

use crate::error::UnknownName;
use crate::manifest::Diagnostic;
use crate::side::Side;
use crate::tier::Tier;

/// One requested capability, for example `tree.read` or `http:api.example.com`
/// (reference-capabilities.md).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Grant(String);

/// Risk class of a grant: normal grants are low, `H` grants are high (reference-capabilities.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Risk {
    /// Low risk (reference-capabilities.md).
    Low,
    /// Medium risk (reference-capabilities.md).
    Medium,
    /// High risk, asked for once at first activation for T1 (reference-capabilities.md).
    High,
}

impl Risk {
    /// Every risk class, lowest first (reference-capabilities.md).
    pub const ALL: [Risk; 3] = [Risk::Low, Risk::Medium, Risk::High];

    /// `low`, `medium` or `high` (reference-capabilities.md).
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
        }
    }
}

impl std::fmt::Display for Risk {
    /// `low`, `medium` or `high` (reference-capabilities.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for Risk {
    type Err = UnknownName;

    /// Parses `low`, `medium` or `high` (reference-capabilities.md).
    fn from_str(text: &str) -> Result<Risk, UnknownName> {
        Risk::ALL
            .into_iter()
            .find(|risk| risk.as_str() == text)
            .ok_or_else(|| UnknownName::new("risk", text))
    }
}

/// One row of the grant table (reference-capabilities.md "Capability table").
#[derive(Debug, Clone)]
pub struct GrantInfo {
    /// The grant family, for example `http` or `tree.read`.
    pub family: &'static str,
    /// Whether the grant takes an argument after `:`.
    pub takes_argument: bool,
    /// Risk class.
    pub risk: Risk,
    /// Tiers that may request it.
    pub tiers: &'static [Tier],
    /// Sides that may request it.
    pub sides: &'static [Side],
    /// The unstable feature that gates it, if any (ABI-022).
    pub unstable: Option<&'static str>,
    /// The WIT interfaces the grant unlocks (ABI-023).
    pub world_interfaces: &'static [&'static str],
}

impl Grant {
    /// Parses a grant string; an unknown family is `manifest.grant-unknown` (ABI-023).
    pub fn parse(_text: &str) -> Result<Grant, Diagnostic> {
        unimplemented!("api::grants::Grant::parse")
    }

    /// The grant text as written in the manifest (reference-capabilities.md).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The family part before `:` (reference-capabilities.md).
    pub fn family(&self) -> &str {
        unimplemented!("api::grants::Grant::family")
    }

    /// The argument after `:`, if any (reference-capabilities.md).
    pub fn argument(&self) -> Option<&str> {
        unimplemented!("api::grants::Grant::argument")
    }

    /// Whether this grant allows what `requested` asks for, for example `*.example.com`
    /// covering `api.example.com` (reference-capabilities.md).
    pub fn covers(&self, _requested: &Grant) -> bool {
        unimplemented!("api::grants::Grant::covers")
    }
}

/// The grant table of this API version (reference-capabilities.md).
pub fn table() -> &'static [GrantInfo] {
    unimplemented!("api::grants::table")
}

/// The table row of a grant family (reference-capabilities.md).
pub fn info(_family: &str) -> Option<&'static GrantInfo> {
    unimplemented!("api::grants::info")
}

/// Returns the diagnostic code when the grant is not allowed for this tier and side
/// (reference-manifest-schema.md `manifest.grant-tier`, `manifest.grant-side`).
pub fn check_grant(_grant: &Grant, _tier: Tier, _side: Side) -> Result<(), Diagnostic> {
    unimplemented!("api::grants::check_grant")
}

/// The highest risk of a grant list (reference-capabilities.md).
pub fn risk_of(_grants: &[Grant]) -> Risk {
    unimplemented!("api::grants::risk_of")
}

/// Whether a grant name is high risk (reference-capabilities.md, api-panes.md PAN-032).
pub fn is_high_risk(_grant: &str) -> bool {
    unimplemented!("api::grants::is_high_risk")
}
