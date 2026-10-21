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

#[cfg(test)]
mod tests {
    use super::{
        parse_point_reference, parse_service_requirement, resolve, resolve_plan, PlanAction,
        PlanPackage, PlanStep, PluginVersion, ResolveInput, ResolvedPlan, SourcePackage,
        VersionRange,
    };
    use ilium_plugin_api::manifest::{parse_manifest, Diagnostic, InstalledSummary, Manifest};
    use ilium_plugin_api::ApiVersion;

    /// Parses a version that the manual calls valid.
    fn version(text: &str) -> PluginVersion {
        PluginVersion::parse(text).expect("fixture version should parse")
    }

    /// Returns the refusal code of a version text the manual calls invalid.
    fn version_refusal(text: &str) -> String {
        PluginVersion::parse(text)
            .expect_err("fixture version should be refused")
            .code
            .as_str()
            .to_string()
    }

    /// Parses a range that the manual calls valid.
    fn range(text: &str) -> VersionRange {
        VersionRange::parse(text).expect("fixture range should parse")
    }

    /// Returns the refusal code of a range text the manual calls invalid.
    fn range_refusal(text: &str) -> String {
        VersionRange::parse(text)
            .expect_err("fixture range should be refused")
            .code
            .as_str()
            .to_string()
    }

    /// Whether `version_text` satisfies `range_text` with the given prerelease permission.
    fn satisfies(range_text: &str, version_text: &str, allow_prerelease: bool) -> bool {
        range(range_text).matches(&version(version_text), allow_prerelease)
    }

    fn has_code(diagnostics: &[Diagnostic], expected_code: &str) -> bool {
        diagnostics.iter().any(|diagnostic| diagnostic.code == expected_code)
    }

    fn assert_refused_with(diagnostics: &[Diagnostic], expected_code: &str) {
        let found: Vec<String> = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str().to_string())
            .collect();
        assert!(
            has_code(diagnostics, expected_code),
            "expected refusal code {expected_code}, found {found:?}"
        );
    }

    fn all_messages(diagnostics: &[Diagnostic]) -> String {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<&str>>()
            .join("\n")
    }

    fn plan_package(
        id: &str,
        version: &str,
        needs: &[(&str, &str, bool)],
        host: Option<&str>,
    ) -> PlanPackage {
        PlanPackage {
            id: id.to_string(),
            version: version.to_string(),
            needs: needs
                .iter()
                .map(|&(need_id, need_range, optional)| {
                    (need_id.to_string(), need_range.to_string(), optional)
                })
                .collect(),
            host: host.map(str::to_string),
        }
    }

    fn source(
        id: &str,
        version: &str,
        sha256: &str,
        channel: &str,
        source_rank: usize,
        needs: &[(&str, &str, bool)],
    ) -> SourcePackage {
        SourcePackage {
            package: plan_package(id, version, needs, None),
            sha256: sha256.to_string(),
            yanked: false,
            channel: channel.to_string(),
            source_rank,
        }
    }

    fn request(
        incoming: Vec<PlanPackage>,
        installed: Vec<PlanPackage>,
        sources: Vec<SourcePackage>,
    ) -> ResolveInput {
        ResolveInput {
            incoming,
            installed,
            sources,
            channel: "stable".to_string(),
            host_release: "0.1.0".to_string(),
            is_update: false,
            pins: Vec::new(),
        }
    }

    fn step(action: PlanAction, id: &str, version: &str) -> PlanStep {
        PlanStep {
            action,
            id: id.to_string(),
            version: version.to_string(),
        }
    }

    /// Resolves a request that the manual says must succeed.
    fn expect_plan(input: &ResolveInput) -> ResolvedPlan {
        resolve_plan(input).expect("resolution should succeed")
    }

    /// Resolves a request that the manual says must be refused, returning its diagnostics.
    fn expect_refusal(input: &ResolveInput) -> Vec<Diagnostic> {
        match resolve_plan(input) {
            Ok(plan) => panic!("resolution should be refused, got {plan:?}"),
            Err(diagnostics) => diagnostics,
        }
    }

    fn api_version() -> ApiVersion {
        ApiVersion {
            major: 0,
            minor: 1,
            patch: 0,
        }
    }

    /// Builds a manifest for `id` whose `requires.plugins` lists each `(plugin id, optional)` pair.
    fn manifest_requiring(id: &str, needs: &[(&str, bool)]) -> Manifest {
        let plugins: Vec<serde_json::Value> = needs
            .iter()
            .map(|&(need_id, optional)| {
                serde_json::json!({ "id": need_id, "version": "*", "optional": optional })
            })
            .collect();
        let text = serde_json::json!({
            "api": "ilium:plugin@0.1.0",
            "id": id,
            "name": "Fixture",
            "version": "1.0.0",
            "publisher": "Example Labs",
            "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" },
            "requires": { "plugins": plugins }
        })
        .to_string();
        parse_manifest(&text).expect("fixture manifest should parse")
    }

    /// REQ: PKG-310
    #[test]
    fn version_parses_major_minor_patch() {
        let parsed = version("1.2.3");
        assert_eq!(parsed.major, 1);
        assert_eq!(parsed.minor, 2);
        assert_eq!(parsed.patch, 3);
        assert!(parsed.pre_release.is_empty());
    }

    /// REQ: PKG-310
    #[test]
    fn version_accepts_the_largest_major_boundary() {
        assert_eq!(version("2147483647.0.0").major, 2147483647);
    }

    /// REQ: PKG-310
    #[test]
    fn version_refuses_a_major_above_the_boundary() {
        assert_eq!(version_refusal("2147483648.0.0"), "manifest.version-invalid");
    }

    /// REQ: PKG-310
    #[test]
    fn version_refuses_a_leading_zero_in_a_number() {
        assert_eq!(version_refusal("01.2.3"), "manifest.version-invalid");
    }

    /// REQ: PKG-310
    #[test]
    fn version_refuses_a_version_without_patch() {
        assert_eq!(version_refusal("1.2"), "manifest.version-invalid");
    }

    /// REQ: PKG-310
    #[test]
    fn version_refuses_empty_text() {
        assert_eq!(version_refusal(""), "manifest.version-invalid");
    }

    /// REQ: PKG-312
    #[test]
    fn version_refuses_a_leading_v() {
        assert_eq!(version_refusal("v1.2.3"), "manifest.version-invalid");
    }

    /// REQ: PKG-311
    #[test]
    fn version_splits_prerelease_identifiers() {
        assert_eq!(
            version("1.0.0-beta.2").pre_release,
            vec!["beta".to_string(), "2".to_string()]
        );
    }

    /// REQ: PKG-311
    #[test]
    fn version_refuses_a_numeric_prerelease_identifier_with_leading_zero() {
        assert_eq!(version_refusal("1.0.0-beta.01"), "manifest.version-invalid");
    }

    /// REQ: PKG-311
    #[test]
    fn version_refuses_an_illegal_prerelease_character() {
        assert_eq!(version_refusal("1.0.0-beta_1"), "manifest.version-invalid");
    }

    /// REQ: PKG-311
    #[test]
    fn version_accepts_exactly_64_characters() {
        let text = format!("1.0.0-{}", "a".repeat(58));
        assert_eq!(text.len(), 64);
        assert_eq!(version(&text).pre_release, vec!["a".repeat(58)]);
    }

    /// REQ: PKG-311
    #[test]
    fn version_refuses_65_characters() {
        let text = format!("1.0.0-{}", "a".repeat(59));
        assert_eq!(text.len(), 65);
        assert_eq!(version_refusal(&text), "manifest.version-invalid");
    }

    /// REQ: PKG-313
    #[test]
    fn build_metadata_is_ignored_in_equality() {
        assert_eq!(version("1.2.3+build.5"), version("1.2.3"));
    }

    /// REQ: PKG-313
    #[test]
    fn build_metadata_does_not_change_ordering() {
        assert!(version("1.0.0+exp.sha.5114f85") == version("1.0.0"));
        assert!(!(version("1.0.0+build.2") < version("1.0.0")));
    }

    /// REQ: PKG-314
    #[test]
    fn precedence_follows_the_documented_ascending_order() {
        let ordered = [
            "0.9.0",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
        ];
        for pair in ordered.windows(2) {
            assert!(
                version(pair[0]) < version(pair[1]),
                "{} should be lower than {}",
                pair[0],
                pair[1]
            );
        }
    }

    /// REQ: PKG-314
    #[test]
    fn prerelease_is_lower_than_the_release_with_the_same_numbers() {
        assert!(version("1.0.0-beta.2") < version("1.0.0"));
    }

    /// REQ: PKG-314
    #[test]
    fn numeric_prerelease_identifiers_compare_as_numbers() {
        assert!(version("1.0.0-beta.2") < version("1.0.0-beta.11"));
    }

    /// REQ: PKG-315
    #[test]
    fn range_accepts_every_documented_comparator_form() {
        for text in [
            "*",
            "=1.2.3",
            "1.2.3",
            ">1.2.3",
            ">=1.2.3",
            "<1.2.3",
            "<=1.2.3",
            "^1.2.3",
            "~1.2.3",
            ">=0.1.0 <1.0.0",
        ] {
            assert!(VersionRange::parse(text).is_ok(), "{text} should parse");
        }
    }

    /// REQ: PKG-315
    #[test]
    fn range_refuses_the_or_operator() {
        assert_eq!(range_refusal("^1.0 || ^2.0"), "manifest.requires-invalid");
    }

    /// REQ: PKG-315
    #[test]
    fn range_refuses_a_comparator_with_a_space_inside() {
        assert_eq!(range_refusal(">= 1.0"), "manifest.requires-invalid");
    }

    /// REQ: PKG-315
    #[test]
    fn range_refuses_empty_text() {
        assert_eq!(range_refusal(""), "manifest.requires-invalid");
    }

    /// REQ: PKG-322
    #[test]
    fn range_accepts_exactly_128_characters() {
        let text = format!(">=1.0.0-{}", "a".repeat(120));
        assert_eq!(text.len(), 128);
        assert!(VersionRange::parse(&text).is_ok());
    }

    /// REQ: PKG-322
    #[test]
    fn range_refuses_129_characters() {
        let text = format!(">=1.0.0-{}", "a".repeat(121));
        assert_eq!(text.len(), 129);
        assert_eq!(range_refusal(&text), "manifest.requires-invalid");
    }

    /// REQ: PKG-316
    #[test]
    fn explicit_comparators_match_their_bounds() {
        assert!(satisfies("=1.2.3", "1.2.3", false));
        assert!(!satisfies("=1.2.3", "1.2.4", false));
        assert!(satisfies(">1.2.3", "1.2.4", false));
        assert!(!satisfies(">1.2.3", "1.2.3", false));
        assert!(satisfies(">=1.2.3", "1.2.3", false));
        assert!(!satisfies(">=1.2.3", "1.2.2", false));
        assert!(satisfies("<1.2.3", "1.2.2", false));
        assert!(!satisfies("<1.2.3", "1.2.3", false));
        assert!(satisfies("<=1.2.3", "1.2.3", false));
        assert!(!satisfies("<=1.2.3", "1.2.4", false));
    }

    /// REQ: PKG-315
    #[test]
    fn space_separated_comparators_are_a_logical_and() {
        assert!(satisfies(">=0.1.0 <1.0.0", "0.1.0", false));
        assert!(satisfies(">=0.1.0 <1.0.0", "0.2.0", false));
        assert!(!satisfies(">=0.1.0 <1.0.0", "0.0.9", false));
        assert!(!satisfies(">=0.1.0 <1.0.0", "1.0.0", false));
    }

    /// REQ: PKG-318
    #[test]
    fn caret_keeps_the_left_most_non_zero_part_for_major_versions() {
        assert!(satisfies("^1.2.3", "1.2.3", false));
        assert!(satisfies("^1.2.3", "1.9.9", false));
        assert!(!satisfies("^1.2.3", "2.0.0", false));
        assert!(!satisfies("^1.2.3", "1.2.2", false));
    }

    /// REQ: PKG-318
    #[test]
    fn caret_keeps_the_minor_for_zero_major_versions() {
        assert!(satisfies("^0.2.3", "0.2.3", false));
        assert!(satisfies("^0.2.3", "0.2.9", false));
        assert!(!satisfies("^0.2.3", "0.3.0", false));
        assert!(!satisfies("^0.2.3", "0.2.2", false));
    }

    /// REQ: PKG-318
    #[test]
    fn caret_keeps_the_patch_for_zero_zero_patch_versions() {
        assert!(satisfies("^0.0.3", "0.0.3", false));
        assert!(!satisfies("^0.0.3", "0.0.4", false));
    }

    /// REQ: PKG-318
    #[test]
    fn caret_with_only_major_and_minor_keeps_the_minor() {
        assert!(satisfies("^0.0", "0.0.0", false));
        assert!(satisfies("^0.0", "0.0.9", false));
        assert!(!satisfies("^0.0", "0.1.0", false));
    }

    /// REQ: PKG-318
    #[test]
    fn caret_with_only_major_keeps_the_major() {
        assert!(satisfies("^0", "0.9.9", false));
        assert!(!satisfies("^0", "1.0.0", false));
    }

    /// REQ: PKG-319
    #[test]
    fn tilde_accepts_patch_changes_only() {
        assert!(satisfies("~1.2.3", "1.2.3", false));
        assert!(satisfies("~1.2.3", "1.2.9", false));
        assert!(!satisfies("~1.2.3", "1.3.0", false));
        assert!(!satisfies("~1.2.3", "1.2.2", false));
    }

    /// REQ: PKG-319
    #[test]
    fn tilde_with_major_and_minor_accepts_patch_changes() {
        assert!(satisfies("~1.2", "1.2.0", false));
        assert!(satisfies("~1.2", "1.2.9", false));
        assert!(!satisfies("~1.2", "1.3.0", false));
    }

    /// REQ: PKG-319
    #[test]
    fn tilde_with_only_major_accepts_minor_changes() {
        assert!(satisfies("~1", "1.0.0", false));
        assert!(satisfies("~1", "1.9.9", false));
        assert!(!satisfies("~1", "2.0.0", false));
    }

    /// REQ: PKG-317
    #[test]
    fn bare_version_is_a_caret_range() {
        assert!(satisfies("1.2.3", "1.9.0", false));
        assert!(!satisfies("1.2.3", "2.0.0", false));
        assert!(satisfies("1.2", "1.5.0", false));
        assert!(!satisfies("1.2", "2.0.0", false));
        assert!(satisfies("1", "1.9.9", false));
        assert!(!satisfies("1", "2.0.0", false));
    }

    /// REQ: PKG-317
    #[test]
    fn bare_zero_minor_version_is_the_same_as_its_caret() {
        assert!(satisfies("0.1.0", "0.1.4", false));
        assert!(!satisfies("0.1.0", "0.2.0", false));
        assert_eq!(
            satisfies("0.1.0", "0.1.4", false),
            satisfies("^0.1.0", "0.1.4", false)
        );
    }

    /// REQ: PKG-318
    #[test]
    fn manual_range_table_matches_the_documented_providers() {
        // Rows from the manual's range table; columns are providers 0.1.0, 0.1.4, 0.2.0, 1.0.0.
        let rows: [(&str, [bool; 4]); 5] = [
            ("0.1.0", [true, true, false, false]),
            ("^0.2", [false, false, true, false]),
            (">=0.1.0 <1.0.0", [true, true, true, false]),
            ("~0.1.2", [false, true, false, false]),
            ("*", [true, true, true, true]),
        ];
        let providers = ["0.1.0", "0.1.4", "0.2.0", "1.0.0"];
        for (range_text, expected) in rows {
            for (index, provider) in providers.iter().enumerate() {
                assert_eq!(
                    satisfies(range_text, provider, false),
                    expected[index],
                    "range {range_text} against provider {provider}"
                );
            }
        }
    }

    /// REQ: PKG-320
    #[test]
    fn star_accepts_every_release_version() {
        assert!(satisfies("*", "0.0.1", false));
        assert!(satisfies("*", "99.0.0", false));
    }

    /// REQ: PKG-321
    #[test]
    fn prerelease_is_refused_on_stable_when_no_comparator_carries_one() {
        assert!(!satisfies("^1.2.0", "1.3.0-beta.1", false));
    }

    /// REQ: PKG-321
    #[test]
    fn prerelease_is_allowed_when_the_resolver_runs_on_beta() {
        assert!(satisfies("^1.2.0", "1.3.0-beta.1", true));
    }

    /// REQ: PKG-321
    #[test]
    fn prerelease_matches_when_a_comparator_has_the_same_base_and_a_prerelease() {
        assert!(satisfies(">=1.3.0-beta.1", "1.3.0-beta.1", false));
        assert!(satisfies(">=1.3.0-beta.1", "1.3.0-beta.2", false));
        assert!(!satisfies(">=1.3.0-beta.1", "1.3.1-beta.1", false));
    }

    /// REQ: PKG-321
    #[test]
    fn release_versions_are_not_affected_by_the_prerelease_flag() {
        assert!(satisfies("^1.2.0", "1.3.0", false));
        assert!(satisfies("^1.2.0", "1.3.0", true));
    }

    /// REQ: PKG-327
    #[test]
    fn service_requirement_full_version_is_a_minimum_caret() {
        let requirement = parse_service_requirement("ilium:inference/provider@0.1.0")
            .expect("full service version should parse");
        assert_eq!(requirement.interface, "ilium:inference/provider");
        assert!(requirement.range.matches(&version("0.1.0"), false));
        assert!(requirement.range.matches(&version("0.1.4"), false));
        assert!(!requirement.range.matches(&version("0.2.0"), false));
    }

    /// REQ: PKG-327
    #[test]
    fn service_requirement_accepts_a_plain_name_with_a_full_version() {
        let requirement = parse_service_requirement("com.example.git/status@1.2.0")
            .expect("full service version should parse");
        assert_eq!(requirement.interface, "com.example.git/status");
        assert!(requirement.range.matches(&version("1.9.0"), false));
        assert!(!requirement.range.matches(&version("2.0.0"), false));
    }

    /// REQ: PKG-327
    #[test]
    fn service_requirement_refuses_a_short_form() {
        let refusal = parse_service_requirement("ilium:inference/provider@0.1")
            .expect_err("short service version should be refused");
        assert_eq!(refusal.code.as_str(), "manifest.service-version");
    }

    /// REQ: PKG-327
    #[test]
    fn service_requirement_refuses_a_caret_tilde_or_range() {
        for text in [
            "ilium:inference/provider@^0.1.0",
            "ilium:inference/provider@~0.1.0",
            "ilium:inference/provider@>=0.1.0",
        ] {
            let refusal = parse_service_requirement(text)
                .expect_err("non-full service version should be refused");
            assert_eq!(refusal.code.as_str(), "manifest.service-version", "{text}");
        }
    }

    /// REQ: PKG-328
    #[test]
    fn point_reference_splits_plugin_point_and_range() {
        let reference = parse_point_reference("ilium.core.smart-copy/after-copy@0.1.0")
            .expect("point reference should parse");
        assert_eq!(reference.plugin, "ilium.core.smart-copy");
        assert_eq!(reference.point, "after-copy");
        assert!(reference.range.matches(&version("0.1.4"), false));
        assert!(!reference.range.matches(&version("0.2.0"), false));
    }

    /// REQ: PKG-328
    #[test]
    fn point_reference_bare_version_is_a_caret_range() {
        let reference = parse_point_reference("com.example.git/status@1")
            .expect("point reference should parse");
        assert!(reference.range.matches(&version("1.9.0"), false));
        assert!(!reference.range.matches(&version("2.0.0"), false));
    }

    /// REQ: PKG-328
    #[test]
    fn point_reference_with_a_bad_range_is_refused_as_a_requires_error() {
        let refusal = parse_point_reference("com.example.git/status@>= 1.0")
            .expect_err("range with a space inside a comparator should be refused");
        assert_eq!(refusal.code.as_str(), "manifest.requires-invalid");
    }

    /// REQ: PKG-340
    #[test]
    fn chosen_source_sha_returns_the_digest_recorded_for_the_plugin() {
        let plan = ResolvedPlan {
            steps: Vec::new(),
            chosen_sources: vec![("com.example.git".to_string(), "sha-git".to_string())],
        };
        assert_eq!(
            plan.chosen_source_sha("com.example.git"),
            Some("sha-git".to_string())
        );
    }

    /// REQ: PKG-340
    #[test]
    fn chosen_source_sha_is_none_for_an_unknown_plugin() {
        let plan = ResolvedPlan {
            steps: Vec::new(),
            chosen_sources: vec![("com.example.git".to_string(), "sha-git".to_string())],
        };
        assert_eq!(plan.chosen_source_sha("com.example.other"), None);
    }

    /// REQ: PKG-340
    #[test]
    fn chosen_source_sha_is_none_for_an_empty_plan() {
        let plan = ResolvedPlan {
            steps: Vec::new(),
            chosen_sources: Vec::new(),
        };
        assert_eq!(plan.chosen_source_sha("com.example.git"), None);
    }

    /// REQ: PKG-330
    #[test]
    fn empty_request_yields_an_empty_plan() {
        let plan = resolve_plan(&request(Vec::new(), Vec::new(), Vec::new()))
            .expect("empty request should resolve");
        assert!(plan.steps.is_empty());
        assert!(plan.chosen_sources.is_empty());
    }

    /// REQ: PKG-338
    #[test]
    fn new_dependency_installs_the_highest_satisfying_version_before_its_dependant() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.2", false)], None)],
            Vec::new(),
            vec![
                source("com.example.git", "1.2.0", "sha-git-1.2.0", "stable", 0, &[]),
                source("com.example.git", "1.3.1", "sha-git-1.3.1", "stable", 0, &[]),
                source("com.example.git", "2.0.0", "sha-git-2.0.0", "stable", 0, &[]),
            ],
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![
                step(PlanAction::Install, "com.example.git", "1.3.1"),
                step(PlanAction::Install, "com.example.reviews", "0.4.0"),
            ]
        );
        assert_eq!(
            plan.chosen_source_sha("com.example.git"),
            Some("sha-git-1.3.1".to_string())
        );
    }

    /// REQ: PKG-339
    #[test]
    fn satisfied_installed_dependency_is_kept_without_change() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.0", false)], None)],
            vec![plan_package("com.example.git", "1.0.0", &[], None)],
            Vec::new(),
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![
                step(PlanAction::Keep, "com.example.git", "1.0.0"),
                step(PlanAction::Install, "com.example.reviews", "0.4.0"),
            ]
        );
    }

    /// REQ: PKG-332
    #[test]
    fn installed_version_is_preferred_over_a_higher_source_when_not_updating() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.0", false)], None)],
            vec![plan_package("com.example.git", "1.0.0", &[], None)],
            vec![source("com.example.git", "1.2.0", "sha-git-1.2.0", "stable", 0, &[])],
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![
                step(PlanAction::Keep, "com.example.git", "1.0.0"),
                step(PlanAction::Install, "com.example.reviews", "0.4.0"),
            ]
        );
    }

    /// REQ: PKG-340
    #[test]
    fn installed_dependency_is_updated_when_a_range_forces_it() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.1", false)], None)],
            vec![plan_package("com.example.git", "1.0.0", &[], None)],
            vec![source("com.example.git", "1.2.0", "sha-git-1.2.0", "stable", 0, &[])],
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![
                step(PlanAction::Update, "com.example.git", "1.2.0"),
                step(PlanAction::Install, "com.example.reviews", "0.4.0"),
            ]
        );
        assert_eq!(
            plan.chosen_source_sha("com.example.git"),
            Some("sha-git-1.2.0".to_string())
        );
    }

    /// REQ: PKG-331
    #[test]
    fn highest_version_satisfying_all_ranges_is_chosen() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.0", false)], None)],
            Vec::new(),
            vec![
                source("com.example.git", "1.0.0", "sha-1.0.0", "stable", 0, &[]),
                source("com.example.git", "1.9.0", "sha-1.9.0", "stable", 0, &[]),
                source("com.example.git", "2.0.0", "sha-2.0.0", "stable", 0, &[]),
            ],
        );
        let plan = expect_plan(&input);
        assert!(plan.steps.contains(&step(PlanAction::Install, "com.example.git", "1.9.0")));
    }

    /// REQ: PKG-331
    #[test]
    fn yanked_version_is_never_chosen_for_a_new_install() {
        let mut yanked = source("com.example.git", "1.9.0", "sha-1.9.0", "stable", 0, &[]);
        yanked.yanked = true;
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.0", false)], None)],
            Vec::new(),
            vec![source("com.example.git", "1.0.0", "sha-1.0.0", "stable", 0, &[]), yanked],
        );
        let plan = expect_plan(&input);
        assert!(plan.steps.contains(&step(PlanAction::Install, "com.example.git", "1.0.0")));
        assert!(!plan.steps.iter().any(|plan_step| plan_step.version == "1.9.0"));
    }

    /// REQ: PKG-336
    #[test]
    fn equal_candidates_prefer_the_lower_source_rank() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.0", false)], None)],
            Vec::new(),
            vec![
                source("com.example.git", "1.2.0", "sha-zzz", "stable", 1, &[]),
                source("com.example.git", "1.2.0", "sha-aaa", "stable", 0, &[]),
            ],
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.chosen_source_sha("com.example.git"),
            Some("sha-aaa".to_string())
        );
    }

    /// REQ: PKG-336
    #[test]
    fn equal_candidates_with_equal_rank_prefer_the_lowest_sha256() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.0", false)], None)],
            Vec::new(),
            vec![
                source("com.example.git", "1.2.0", "sha-bbb", "stable", 0, &[]),
                source("com.example.git", "1.2.0", "sha-aaa", "stable", 0, &[]),
            ],
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.chosen_source_sha("com.example.git"),
            Some("sha-aaa".to_string())
        );
    }

    /// REQ: PKG-333
    #[test]
    fn conflicting_ranges_are_refused_and_list_each_constraint() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^2.0", false)], None)],
            vec![plan_package("com.example.lint", "1.0.0", &[("com.example.git", "^1.0", false)], None)],
            vec![
                source("com.example.git", "1.0.0", "sha-1.0.0", "stable", 0, &[]),
                source("com.example.git", "2.0.0", "sha-2.0.0", "stable", 0, &[]),
            ],
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.conflict");
        let messages = all_messages(&diagnostics);
        assert!(messages.contains("^1.0"), "message should list ^1.0: {messages}");
        assert!(messages.contains("^2.0"), "message should list ^2.0: {messages}");
    }

    /// REQ: PKG-341
    #[test]
    fn conflicting_ranges_name_both_source_plugins() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^2.0", false)], None)],
            vec![plan_package("com.example.lint", "1.0.0", &[("com.example.git", "^1.0", false)], None)],
            vec![
                source("com.example.git", "1.0.0", "sha-1.0.0", "stable", 0, &[]),
                source("com.example.git", "2.0.0", "sha-2.0.0", "stable", 0, &[]),
            ],
        );
        let diagnostics = expect_refusal(&input);
        let messages = all_messages(&diagnostics);
        assert!(messages.contains("com.example.lint"), "{messages}");
        assert!(messages.contains("com.example.reviews"), "{messages}");
    }

    /// REQ: PKG-334
    #[test]
    fn required_plugin_with_no_copy_and_no_source_is_unresolved() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.0", false)], None)],
            Vec::new(),
            Vec::new(),
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.unresolved");
    }

    /// REQ: PKG-334
    #[test]
    fn every_missing_required_id_is_listed() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[
                    ("com.example.missing-one", "^1.0", false),
                    ("com.example.missing-two", "^2.0", false),
                ],
                None,
            )],
            Vec::new(),
            Vec::new(),
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.unresolved");
        let messages = all_messages(&diagnostics);
        assert!(messages.contains("com.example.missing-one"), "{messages}");
        assert!(messages.contains("com.example.missing-two"), "{messages}");
    }

    /// REQ: PKG-344
    #[test]
    fn missing_optional_dependency_does_not_block_or_install() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.theme-pack", "*", true)], None)],
            Vec::new(),
            Vec::new(),
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![step(PlanAction::Install, "com.example.reviews", "0.4.0")]
        );
    }

    /// REQ: PKG-343
    #[test]
    fn hard_cycle_between_packages_is_refused() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "*", false)], None)],
            Vec::new(),
            vec![source(
                "com.example.git",
                "1.0.0",
                "sha-git",
                "stable",
                0,
                &[("com.example.reviews", "*", false)],
            )],
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.cycle");
    }

    /// REQ: PKG-335
    #[test]
    fn cycle_through_an_optional_edge_does_not_block_install() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "*", true)], None)],
            Vec::new(),
            vec![source(
                "com.example.git",
                "1.0.0",
                "sha-git",
                "stable",
                0,
                &[("com.example.reviews", "*", false)],
            )],
        );
        assert!(resolve_plan(&input).is_ok());
    }

    /// REQ: PKG-346
    #[test]
    fn host_range_not_satisfied_by_the_running_release_is_refused() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[], Some(">=9.0.0"))],
            Vec::new(),
            Vec::new(),
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.host-version");
    }

    /// REQ: PKG-325
    #[test]
    fn host_range_satisfied_by_the_running_release_is_accepted() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[], Some(">=0.1.0"))],
            Vec::new(),
            Vec::new(),
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![step(PlanAction::Install, "com.example.reviews", "0.4.0")]
        );
    }

    /// REQ: PKG-347
    #[test]
    fn prerelease_only_source_is_unresolved_on_the_stable_channel() {
        let input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.0", false)], None)],
            Vec::new(),
            vec![source("com.example.git", "1.3.0-beta.1", "sha-beta", "beta", 0, &[])],
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.unresolved");
    }

    /// REQ: PKG-354
    #[test]
    fn prerelease_source_is_offered_on_the_beta_channel() {
        let mut input = request(
            vec![plan_package("com.example.reviews", "0.4.0", &[("com.example.git", "^1.0", false)], None)],
            Vec::new(),
            vec![source("com.example.git", "1.3.0-beta.1", "sha-beta", "beta", 0, &[])],
        );
        input.channel = "beta".to_string();
        let plan = expect_plan(&input);
        assert!(plan.steps.contains(&step(PlanAction::Install, "com.example.git", "1.3.0-beta.1")));
    }

    /// REQ: PKG-348
    #[test]
    fn updating_a_dependency_out_of_an_installed_dependants_range_is_refused() {
        let mut input = request(
            vec![plan_package("com.example.git", "2.0.0", &[], None)],
            vec![plan_package("com.example.reviews", "1.0.0", &[("com.example.git", "^1.0", false)], None)],
            vec![source("com.example.git", "2.0.0", "sha-git-2.0.0", "stable", 0, &[])],
        );
        input.is_update = true;
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.conflict");
    }

    /// REQ: PKG-350
    #[test]
    fn independent_packages_activate_in_plugin_id_byte_order() {
        let input = request(
            vec![
                plan_package("com.example.zeta", "1.0.0", &[], None),
                plan_package("com.example.alpha", "1.0.0", &[], None),
            ],
            Vec::new(),
            Vec::new(),
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![
                step(PlanAction::Install, "com.example.alpha", "1.0.0"),
                step(PlanAction::Install, "com.example.zeta", "1.0.0"),
            ]
        );
    }

    /// REQ: PKG-350
    #[test]
    fn dependency_activates_before_its_dependant() {
        let input = request(
            vec![plan_package("com.example.app", "1.0.0", &[("com.example.lib", "^1.0", false)], None)],
            Vec::new(),
            vec![source("com.example.lib", "1.0.0", "sha-lib", "stable", 0, &[])],
        );
        let plan = expect_plan(&input);
        let lib_index = plan
            .steps
            .iter()
            .position(|plan_step| plan_step.id == "com.example.lib")
            .expect("lib should be in the plan");
        let app_index = plan
            .steps
            .iter()
            .position(|plan_step| plan_step.id == "com.example.app")
            .expect("app should be in the plan");
        assert!(lib_index < app_index);
    }

    /// REQ: PKG-350
    #[test]
    fn activation_order_of_resolve_puts_a_dependency_first() {
        let packages = vec![
            manifest_requiring("com.example.app", &[("com.example.lib", false)]),
            manifest_requiring("com.example.lib", &[]),
        ];
        let plan = resolve(&packages, &[], api_version()).expect("acyclic graph should resolve");
        assert_eq!(
            plan.activation_order,
            vec!["com.example.lib".to_string(), "com.example.app".to_string()]
        );
    }

    /// REQ: PKG-350
    #[test]
    fn resolve_orders_independent_plugins_by_id() {
        let packages = vec![
            manifest_requiring("com.example.zeta", &[]),
            manifest_requiring("com.example.alpha", &[]),
        ];
        let plan = resolve(&packages, &[], api_version()).expect("independent plugins should resolve");
        assert_eq!(
            plan.activation_order,
            vec!["com.example.alpha".to_string(), "com.example.zeta".to_string()]
        );
    }

    /// REQ: PKG-350
    #[test]
    fn resolve_of_no_packages_yields_an_empty_order() {
        let plan = resolve(&[], &[], api_version()).expect("empty set should resolve");
        assert!(plan.activation_order.is_empty());
    }

    /// REQ: PKG-335
    #[test]
    fn hard_cycle_in_resolve_is_refused_as_dependency_cycle() {
        let packages = vec![
            manifest_requiring("com.example.app", &[("com.example.lib", false)]),
            manifest_requiring("com.example.lib", &[("com.example.app", false)]),
        ];
        let diagnostics = resolve(&packages, &[], api_version())
            .expect_err("hard cycle should be refused");
        assert_refused_with(&diagnostics, "dependency.cycle");
        let messages = all_messages(&diagnostics);
        assert!(messages.contains("com.example.app"), "{messages}");
        assert!(messages.contains("com.example.lib"), "{messages}");
    }

    /// REQ: PKG-335
    #[test]
    fn optional_edge_closing_a_loop_is_not_a_cycle() {
        let packages = vec![
            manifest_requiring("com.example.app", &[("com.example.lib", true)]),
            manifest_requiring("com.example.lib", &[("com.example.app", false)]),
        ];
        assert!(resolve(&packages, &[], api_version()).is_ok());
    }

    /// REQ: PKG-334
    #[test]
    fn hard_requirement_missing_everywhere_is_unresolved_in_resolve() {
        let packages = vec![manifest_requiring("com.example.app", &[("com.example.lib", false)])];
        let diagnostics = resolve(&packages, &[], api_version())
            .expect_err("missing hard requirement should be refused");
        assert_refused_with(&diagnostics, "dependency.unresolved");
        assert!(all_messages(&diagnostics).contains("com.example.lib"));
    }

    /// REQ: PKG-334
    #[test]
    fn hard_requirement_satisfied_by_an_installed_copy_resolves() {
        let packages = vec![manifest_requiring("com.example.app", &[("com.example.lib", false)])];
        let installed = vec![InstalledSummary {
            id: "com.example.lib".to_string(),
            version: "1.0.0".to_string(),
            services: Vec::new(),
            settings_owner_tabs: Vec::new(),
        }];
        assert!(resolve(&packages, &installed, api_version()).is_ok());
    }
}
