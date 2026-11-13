//! Side, tier and platform as the test harness and the macro name them.
//!
//! The same three enums exist in `ilium-plugin-api` for the host. The SDK family may not depend
//! on that crate (test plan 1.2), so the guest side has its own copies; both render the same
//! manual strings.

use std::fmt;

/// The side a plugin component runs on (reference-manifest.md PKG-041).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    /// The server component, world `ilium:server`.
    Server,
    /// The client component, world `ilium:client`.
    Client,
    /// The scene component, world `ilium:anim`.
    Scene,
}

impl Side {
    /// The manual name of the side: `server`, `client` or `scene`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Client => "client",
            Self::Scene => "scene",
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The trust tier of a plugin (security.md). The harness default is `T1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Tier {
    /// Bundled first-party code.
    T1,
    /// Signed third-party packages.
    T2,
    /// Unsigned local and dev packages.
    T3,
}

impl Tier {
    /// The manual name of the tier: `T1`, `T2` or `T3`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::T1 => "T1",
            Self::T2 => "T2",
            Self::T3 => "T3",
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The platform a fake host reports to the plugin (reference-test-harness.md "Setup").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    /// Linux.
    Linux,
    /// macOS.
    MacOs,
    /// Windows.
    Windows,
}

impl Platform {
    /// The manual name of the platform: `linux`, `macos` or `windows`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::MacOs => "macos",
            Self::Windows => "windows",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
