//! Dependency and service resolution across a set of packages (reference-manifest.md PKG-650..660).

use ilium_plugin_api::manifest::{Diagnostic, InstalledSummary, Manifest};
use ilium_plugin_api::ApiVersion;

/// The order in which packages activate, with the service providers each consumer uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionPlan {
    /// Plugin ids in activation order.
    pub activation_order: Vec<String>,
}

/// Resolves requirements; a cycle is `dependency.cycle` (PKG-335).
pub fn resolve(
    _packages: &[Manifest],
    _installed: &[InstalledSummary],
    _host: ApiVersion,
) -> Result<ResolutionPlan, Vec<Diagnostic>> {
    unimplemented!("host::resolve::resolve")
}

/// A plugin version: semver with build metadata ignored in comparisons
/// (reference-dependency-resolution.md PKG-310..312).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginVersion {
    /// Major.
    pub major: u64,
    /// Minor.
    pub minor: u64,
    /// Patch.
    pub patch: u64,
    /// Pre-release identifiers, empty for a release.
    pub pre_release: Vec<String>,
}

impl PluginVersion {
    /// Parses a version; a bad one is `manifest.version-invalid` (PKG-310).
    pub fn parse(_text: &str) -> Result<PluginVersion, Diagnostic> {
        unimplemented!("host::resolve::PluginVersion::parse")
    }
}

/// A version range of a requirement (reference-dependency-resolution.md PKG-313..316).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRange {
    _private: (),
}

impl VersionRange {
    /// Parses a range; a bad one is `manifest.requires-invalid` (PKG-313).
    pub fn parse(_text: &str) -> Result<VersionRange, Diagnostic> {
        unimplemented!("host::resolve::VersionRange::parse")
    }

    /// Whether a version satisfies the range; pre-releases match only when allowed (PKG-315).
    pub fn matches(&self, _version: &PluginVersion, _allow_prerelease: bool) -> bool {
        unimplemented!("host::resolve::VersionRange::matches")
    }
}

/// A parsed `requires.services` entry (PKG-320).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRequirement {
    /// The service interface.
    pub interface: String,
    /// The accepted range.
    pub range: VersionRange,
}

/// Parses `<interface>@<range>` (PKG-320).
pub fn parse_service_requirement(_text: &str) -> Result<ServiceRequirement, Diagnostic> {
    unimplemented!("host::resolve::parse_service_requirement")
}

/// A parsed extension-point reference (PKG-321).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointReference {
    /// The owning plugin.
    pub plugin: String,
    /// The point name.
    pub point: String,
    /// The accepted range.
    pub range: VersionRange,
}

/// Parses `<plugin>/<point>@<range>` (PKG-321).
pub fn parse_point_reference(_text: &str) -> Result<PointReference, Diagnostic> {
    unimplemented!("host::resolve::parse_point_reference")
}

/// One package in a resolution request (PKG-330..348).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanPackage {
    /// The plugin id.
    pub id: String,
    /// The version.
    pub version: String,
    /// Requirements as (plugin id, range, optional).
    pub needs: Vec<(String, String, bool)>,
    /// The host release range, when any.
    pub host: Option<String>,
}

/// One package a source offers (PKG-330..348).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePackage {
    /// The package.
    pub package: PlanPackage,
    /// The package digest.
    pub sha256: String,
    /// Whether the source yanked it.
    pub yanked: bool,
    /// The channel it is published on.
    pub channel: String,
    /// The rank of the source, lower wins.
    pub source_rank: usize,
}

/// A resolution request (PKG-330..348).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveInput {
    /// Packages being installed or updated.
    pub incoming: Vec<PlanPackage>,
    /// Packages already installed.
    pub installed: Vec<PlanPackage>,
    /// Packages the sources offer.
    pub sources: Vec<SourcePackage>,
    /// The selected channel.
    pub channel: String,
    /// The running host release.
    pub host_release: String,
    /// Whether the request is an update.
    pub is_update: bool,
    /// Pinned plugin ids.
    pub pins: Vec<String>,
}

/// What a plan step does (PKG-330).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanAction {
    /// Installs a package.
    Install,
    /// Updates an installed package.
    Update,
    /// Keeps an installed package.
    Keep,
}

/// One step of a resolved plan (PKG-330).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStep {
    /// The action.
    pub action: PlanAction,
    /// The plugin id.
    pub id: String,
    /// The version.
    pub version: String,
}

/// A resolved plan (PKG-330..348).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPlan {
    /// The steps in activation order.
    pub steps: Vec<PlanStep>,
    /// The digest chosen for each plugin id.
    pub chosen_sources: Vec<(String, String)>,
}

impl ResolvedPlan {
    /// The digest of the source chosen for a plugin (PKG-340).
    pub fn chosen_source_sha(&self, _id: &str) -> Option<String> {
        unimplemented!("host::resolve::ResolvedPlan::chosen_source_sha")
    }
}

/// Resolves a request into a plan; conflicts are diagnostics (PKG-330..348).
pub fn resolve_plan(_input: &ResolveInput) -> Result<ResolvedPlan, Vec<Diagnostic>> {
    unimplemented!("host::resolve::resolve_plan")
}
