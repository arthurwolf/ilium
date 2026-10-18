//! Plugin sides (reference-manifest.md "Components and exports").

use serde::{Deserialize, Serialize};

use crate::error::UnknownName;

/// The side a component runs on (reference-manifest.md PKG-024). Mirrors the generated
/// `plugin-side` of `ilium:types` with the manifest's names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    /// The server component, world `ilium:server/base@0.1.0` (reference-manifest.md PKG-041).
    Server,
    /// The client component, world `ilium:client/base@0.1.0` (reference-manifest.md PKG-041).
    Client,
    /// The scene component, world `ilium:anim/base@0.1.0` (reference-manifest.md PKG-041).
    Scene,
}

impl Side {
    /// Every side in manifest order (reference-manifest.md PKG-024).
    pub const ALL: [Side; 3] = [Side::Server, Side::Client, Side::Scene];

    /// The manifest key of the side (reference-manifest.md PKG-024).
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Server => "server",
            Side::Client => "client",
            Side::Scene => "scene",
        }
    }
}

impl std::fmt::Display for Side {
    /// `server`, `client` or `scene` (reference-manifest.md PKG-024).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for Side {
    type Err = UnknownName;

    /// Parses `server`, `client` or `scene` (reference-manifest.md PKG-024).
    fn from_str(text: &str) -> Result<Side, UnknownName> {
        Side::ALL
            .into_iter()
            .find(|side| side.as_str() == text)
            .ok_or_else(|| UnknownName::new("side", text))
    }
}
