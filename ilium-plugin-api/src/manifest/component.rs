//! Checks that compare a manifest with what a built component really imports and exports
//! (reference-manifest.md PKG-042..PKG-045, reference-abi-evolution.md section 4,
//! reference-host-runtime.md). Skel-api addition to plan 1.3 for ABI-006, ABI-018, ABI-022..ABI-024.

use super::{Diagnostic, Manifest, ValidationContext};
use crate::side::Side;

/// What a component declares, read from its binary by the host or the CLI
/// (reference-abi-evolution.md "The version appears in three places").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentSummary {
    /// Imported interfaces as `package:name/interface@version`, for example
    /// `ilium:host/tree@0.1.0` or `wasi:sockets/tcp@0.2.0` (reference-host-runtime.md).
    pub imports: Vec<String>,
    /// Exported interface names without package, for example `lifecycle` or `detector`
    /// (reference-manifest.md "Components and exports").
    pub exports: Vec<String>,
    /// Payloads of every `ilium:api-version` custom section, in file order (ABI-004).
    pub api_sections: Vec<Vec<u8>>,
    /// Unstable items the component imports, as feature names (reference-abi-evolution.md section 3).
    pub unstable_imports: Vec<String>,
}

/// Compares one side's component with the manifest: export list (PKG-043), API section
/// (ABI-004), imports against grants (ABI-023), forbidden WASI (`component.wasi-forbidden`)
/// and unstable imports (ABI-022). Returns every finding in refusal order.
pub fn check_component(
    manifest: &Manifest,
    side: Side,
    component: &ComponentSummary,
    _context: &ValidationContext,
) -> Vec<Diagnostic> {
    use super::validate::finding;
    use std::collections::BTreeSet;
    let declared = match side {
        Side::Server => &manifest.server,
        Side::Client => &manifest.client,
        Side::Scene => &manifest.scene,
    };
    let path = format!("/{}", side.as_str());
    let Some(declared) = declared else {
        return vec![finding(
            "manifest.schema",
            path,
            "Component side is not declared.",
            6,
        )];
    };
    // The package validation order specifies section before export checks.
    let section = match component.api_sections.as_slice() {
        [] => {
            return vec![finding(
                "package.api-section-missing",
                path,
                "Component has no ilium:api-version section.",
                6,
            )];
        }
        [section] => section,
        _ => {
            return vec![finding(
                "package.api-section-repeated",
                path,
                "Component has more than one ilium:api-version section.",
                6,
            )];
        }
    };
    if section.len() != 6 {
        return vec![finding(
            "package.api-section-invalid",
            path,
            format!(
                "API version section is {} bytes, expected 6.",
                section.len()
            ),
            6,
        )];
    }
    let expected = manifest
        .api
        .strip_prefix("ilium:plugin@")
        .and_then(|text| semver::Version::parse(text).ok());
    let actual = [
        u16::from_be_bytes([section[0], section[1]]),
        u16::from_be_bytes([section[2], section[3]]),
        u16::from_be_bytes([section[4], section[5]]),
    ];
    if expected.is_none_or(|version| {
        [version.major, version.minor, version.patch] != actual.map(u64::from)
    }) {
        return vec![finding(
            "manifest.api-version",
            path,
            "Component API section differs from manifest api.",
            6,
        )];
    }
    let required: &[&str] = if side == Side::Scene {
        &["lifecycle", "renderer"]
    } else {
        &["lifecycle", "handler"]
    };
    let expected: BTreeSet<_> = required
        .iter()
        .copied()
        .chain(declared.exports.iter().map(String::as_str))
        .collect();
    let actual: BTreeSet<_> = component.exports.iter().map(String::as_str).collect();
    if actual != expected || component.exports.len() != actual.len() {
        return vec![finding(
            "manifest.exports-mismatch",
            format!("{path}/exports"),
            format!(
                "Missing: {:?}. Extra: {:?}.",
                expected.difference(&actual).collect::<Vec<_>>(),
                actual.difference(&expected).collect::<Vec<_>>()
            ),
            6,
        )];
    }
    let grants = match side {
        Side::Server => &manifest.capabilities.server,
        Side::Client => &manifest.capabilities.client,
        Side::Scene => &manifest.capabilities.scene,
    };
    let mut errors = Vec::new();
    for import in &component.imports {
        if import.starts_with("wasi:sockets/") || import.starts_with("wasi:http/") {
            errors.push(finding(
                "component.wasi-forbidden",
                &path,
                format!("Component imports forbidden interface {import}."),
                6,
            ));
            continue;
        }
        if import.starts_with("wasi:") {
            if ![
                "wasi:cli/",
                "wasi:clocks/",
                "wasi:random/",
                "wasi:io/",
                "wasi:filesystem/",
            ]
            .iter()
            .any(|prefix| import.starts_with(prefix))
            {
                errors.push(finding(
                    "component.import-unknown",
                    &path,
                    format!("Unknown WASI interface {import}."),
                    6,
                ));
            }
            continue;
        }
        let always = [
            "log",
            "clock",
            "timers",
            "storage",
            "settings",
            "codec",
            "messages",
            "fs",
            "journal",
            "intents-host",
        ]
        .iter()
        .any(|name| import == &format!("ilium:host/{name}@0.1.0"));
        if always
            || (side == Side::Scene
                && ["ilium:anim/surface@0.1.0", "ilium:anim/fonts@0.1.0"]
                    .contains(&import.as_str()))
        {
            continue;
        }
        let known = crate::grants::table()
            .iter()
            .any(|info| info.world_interfaces.contains(&import.as_str()));
        let requested = grants
            .iter()
            .filter_map(|grant| crate::grants::info(grant.family()))
            .any(|info| {
                info.sides.contains(&side) && info.world_interfaces.contains(&import.as_str())
            });
        if !known {
            errors.push(finding(
                "component.import-unknown",
                &path,
                format!("Unknown host interface {import}."),
                6,
            ));
        } else if !requested {
            errors.push(finding(
                "manifest.import-not-requested",
                &path,
                format!("Component imports {import}, which no requested capability unlocks."),
                6,
            ));
        }
    }
    for feature in &component.unstable_imports {
        if !manifest.unstable.contains(feature) {
            errors.push(finding(
                "manifest.unstable-missing",
                &path,
                format!("Component imports unstable feature {feature}, absent from unstable list."),
                6,
            ));
        }
    }
    errors.truncate(100);
    errors
}
