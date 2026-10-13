//! The manifest model, its validation and its diagnostics (reference-manifest.md,
//! reference-manifest-schema.md).
//!
//! JSON keys are snake_case as in the manual. Every optional key has a serde default so a
//! minimal manifest deserializes; the checks of the manual live in [`validate`], not in serde.

mod component;
mod diagnostic;
mod syntax;
mod validate;

pub use component::{check_component, ComponentSummary};
pub use diagnostic::{Diagnostic, Stage};
pub use syntax::{
    parse_activation, parse_chord, parse_when, ActivationTrigger, Chord, WhenExpr, ACTIVATION_KINDS,
};
pub use validate::{
    json_schema, parse_manifest, validate, validate_json, CheckMode, CorePluginSummary,
    InstalledSummary, ValidationContext,
};

use serde::{Deserialize, Serialize};

use crate::grants::Grant;
use crate::limits::Limits;

/// A parsed `manifest.json` (reference-manifest.md "Top-level keys").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Contract identity, exactly `ilium:plugin@0.1.0` (reference-manifest.md PKG-013).
    pub api: String,
    /// Plugin id, 2 to 8 `.`-separated segments (reference-manifest.md PKG-014).
    pub id: String,
    /// Display name (reference-manifest.md PKG-015).
    pub name: String,
    /// SemVer 2.0.0 version (reference-manifest.md PKG-016).
    pub version: String,
    /// Publisher display name (reference-manifest.md PKG-017).
    pub publisher: String,
    /// One-line description (reference-manifest.md PKG-018).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// SPDX licence expression (reference-manifest.md PKG-019).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Tabler icon name (reference-manifest.md PKG-020).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Registry categories (reference-manifest.md PKG-021).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    /// Search keywords (reference-manifest.md PKG-022).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// `https://` home page (reference-manifest.md PKG-023).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// `https://` source repository (reference-manifest.md PKG-023).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// The server component (reference-manifest.md PKG-024).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<ComponentDecl>,
    /// The client component (reference-manifest.md PKG-024).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<ComponentDecl>,
    /// The scene component (reference-manifest.md PKG-024).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<ComponentDecl>,
    /// One grant list per side (reference-manifest.md PKG-025).
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Activation triggers; empty means lazy (reference-manifest.md PKG-026).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activation: Vec<String>,
    /// Host range, services and plugins (reference-dependency-resolution.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<Requires>,
    /// Replaced core plugins (reference-manifest.md PKG-028).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replaces: Vec<String>,
    /// Plugin ids allowed to send `messages.peer` messages (reference-manifest.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepts_messages_from: Vec<String>,
    /// Core plugin may be replaced (reference-manifest.md PKG-029).
    #[serde(default)]
    pub replaceable: bool,
    /// Replaceable surface of a core plugin (reference-manifest.md PKG-029).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface: Vec<String>,
    /// Static contributions (reference-manifest.md PKG-030).
    #[serde(default)]
    pub contributes: Contributes,
    /// Requested limits, clamped to the tier ceiling (reference-manifest.md PKG-031).
    #[serde(default)]
    pub limits: Limits,
    /// Data version, default 1 (reference-manifest.md PKG-032).
    #[serde(default = "default_data_version")]
    pub data_version: u32,
    /// `standard` or `first-party` (reference-manifest.md PKG-033).
    #[serde(default)]
    pub limits_tier: LimitsTier,
    /// Unstable features used (reference-manifest.md PKG-034).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unstable: Vec<String>,
    /// File inventory of a packed package (reference-package-format.md "The inventory").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileEntry>,
}

/// The default of `data_version` (reference-manifest.md PKG-032).
pub const DEFAULT_DATA_VERSION: u32 = 1;

fn default_data_version() -> u32 {
    DEFAULT_DATA_VERSION
}

/// One side's component: `{ "entry", "world", "exports" }` (reference-manifest.md
/// "Components and exports"). The Rust field `component` is the JSON key `entry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentDecl {
    /// File name of the component, JSON key `entry` (reference-manifest.md PKG-040).
    #[serde(rename = "entry")]
    pub component: String,
    /// The world the component targets (reference-manifest.md PKG-041).
    pub world: String,
    /// Optional exports the component has; required ones are never listed (reference-manifest.md PKG-042).
    #[serde(default)]
    pub exports: Vec<String>,
}

/// One grant list per side (reference-manifest.md "The capabilities object").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Capabilities {
    /// Grants of the server side (reference-capabilities.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server: Vec<Grant>,
    /// Grants of the client side (reference-capabilities.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client: Vec<Grant>,
    /// Grants of the scene side (reference-capabilities.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scene: Vec<Grant>,
}

/// The `requires` object (reference-dependency-resolution.md "The requires object").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Requires {
    /// Host version range (reference-dependency-resolution.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Services as `<interface>@<range>` strings; the range defaults to `*`
    /// (reference-dependency-resolution.md). Parse one with [`ServiceRequirement::parse`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,
    /// Required plugins (reference-dependency-resolution.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<PluginRequirement>,
}

/// One parsed `requires.services` entry (reference-dependency-resolution.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRequirement {
    /// Service interface name (reference-dependency-resolution.md).
    pub name: String,
    /// Version range, `*` when omitted (reference-dependency-resolution.md).
    pub version: String,
}

impl ServiceRequirement {
    /// Parses `<interface>@<range>`; a bad entry is `manifest.requires-invalid`
    /// (reference-dependency-resolution.md).
    pub fn parse(text: &str) -> Result<ServiceRequirement, Diagnostic> {
        unimplemented!("api::manifest::ServiceRequirement::parse")
    }
}

/// One `requires.plugins` entry (reference-dependency-resolution.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRequirement {
    /// Required plugin id (reference-dependency-resolution.md).
    pub id: String,
    /// Version range, default `*` (reference-dependency-resolution.md).
    #[serde(default = "any_version")]
    pub version: String,
    /// Whether the dependency never blocks activation, default `false` (reference-dependency-resolution.md).
    #[serde(default)]
    pub optional: bool,
}

/// The range that matches every version (reference-dependency-resolution.md).
pub const ANY_VERSION: &str = "*";

fn any_version() -> String {
    ANY_VERSION.to_owned()
}

/// Every key of reference-manifest.md "Contributes keys" and "New contributes keys", typed as
/// `serde_json::Value` per key in the skeleton; the implementation replaces each with a typed record.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Contributes(pub std::collections::BTreeMap<String, serde_json::Value>);

/// The limits ceiling class (reference-manifest.md PKG-033).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LimitsTier {
    /// Third-party ceilings (reference-manifest.md PKG-033).
    #[default]
    Standard,
    /// T1 ceilings (reference-manifest.md PKG-033).
    FirstParty,
}

/// One inventory entry of a packed package (reference-package-format.md "The inventory").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Path inside the package (reference-package-format.md).
    pub path: String,
    /// Length in bytes (reference-package-format.md).
    pub length: u64,
    /// Lower-case hex sha256 (reference-package-format.md).
    pub sha256: String,
}
