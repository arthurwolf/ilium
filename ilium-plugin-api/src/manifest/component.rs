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
    _manifest: &Manifest,
    _side: Side,
    _component: &ComponentSummary,
    _context: &ValidationContext,
) -> Vec<Diagnostic> {
    unimplemented!("api::manifest::check_component")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Severity;
    use crate::manifest::{CheckMode, ValidationContext};
    use crate::platform::Platform;
    use crate::tier::Tier;

    /// The API section payload of contract version 0.1.0 (ABI-004, HST-085).
    const API_SECTION_0_1_0: [u8; 6] = [0x00, 0x00, 0x00, 0x01, 0x00, 0x00];

    /// The four unstable features of version 0.1.0 (ABI-022).
    const UNSTABLE_FEATURES_0_1_0: [&str; 4] =
        ["ui-calendar", "plugin-services", "gpu-kernels", "websocket"];

    fn context() -> ValidationContext {
        ValidationContext {
            tier: Tier::T2,
            mode: CheckMode::Install,
            platform: Platform::Linux,
            installed: Vec::new(),
            core_plugins: Vec::new(),
            unstable_features: UNSTABLE_FEATURES_0_1_0
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        }
    }

    /// A manifest whose server component lists `server_exports` and whose server side requests
    /// `server_grants`, with `unstable` listing the unstable features it uses.
    fn manifest_with(
        server_exports: &[&str],
        server_grants: &[&str],
        unstable: &[&str],
    ) -> Manifest {
        let value = serde_json::json!({
            "api": "ilium:plugin@0.1.0",
            "id": "ilium.example",
            "name": "Example",
            "version": "1.0.0",
            "publisher": "Example Publisher",
            "server": {
                "entry": "server.wasm",
                "world": "ilium:server/base@0.1.0",
                "exports": server_exports,
            },
            "capabilities": { "server": server_grants },
            "unstable": unstable,
        });
        serde_json::from_value(value).expect("test manifest JSON matches the Manifest schema")
    }

    /// A component with the 0.1.0 API section and no imports or exports.
    fn valid_component() -> ComponentSummary {
        ComponentSummary {
            api_sections: vec![API_SECTION_0_1_0.to_vec()],
            ..ComponentSummary::default()
        }
    }

    fn run(manifest: &Manifest, component: &ComponentSummary) -> Vec<Diagnostic> {
        check_component(manifest, Side::Server, component, &context())
    }

    fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect()
    }

    /// REQ: ABI-004
    #[test]
    fn valid_component_with_exact_api_section_has_no_findings() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = valid_component();

        assert_eq!(run(&manifest, &component), Vec::new());
    }

    /// REQ: ABI-004
    #[test]
    fn empty_component_without_api_section_is_refused_as_missing_section() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary::default();

        assert_eq!(
            codes(&run(&manifest, &component)),
            vec!["package.api-section-missing"]
        );
    }

    /// REQ: ABI-004
    #[test]
    fn api_section_for_another_major_version_is_refused_as_api_version() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            api_sections: vec![vec![0x00, 0x00, 0x00, 0x02, 0x00, 0x00]],
            ..ComponentSummary::default()
        };

        assert_eq!(codes(&run(&manifest, &component)), vec!["manifest.api-version"]);
    }

    /// REQ: ABI-004
    #[test]
    fn api_section_differing_only_in_patch_is_refused_as_api_version() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            api_sections: vec![vec![0x00, 0x00, 0x00, 0x01, 0x00, 0x01]],
            ..ComponentSummary::default()
        };

        assert_eq!(codes(&run(&manifest, &component)), vec!["manifest.api-version"]);
    }

    /// REQ: HST-085
    #[test]
    fn api_section_of_five_bytes_is_refused_as_invalid_section() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            api_sections: vec![vec![0x00, 0x00, 0x00, 0x01, 0x00]],
            ..ComponentSummary::default()
        };

        assert_eq!(
            codes(&run(&manifest, &component)),
            vec!["package.api-section-invalid"]
        );
    }

    /// REQ: HST-085
    #[test]
    fn empty_api_section_payload_is_refused_as_invalid_section() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            api_sections: vec![Vec::new()],
            ..ComponentSummary::default()
        };

        assert_eq!(
            codes(&run(&manifest, &component)),
            vec!["package.api-section-invalid"]
        );
    }

    /// REQ: ABI-004
    #[test]
    fn two_api_sections_are_refused_as_repeated_section() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            api_sections: vec![API_SECTION_0_1_0.to_vec(), API_SECTION_0_1_0.to_vec()],
            ..ComponentSummary::default()
        };

        assert_eq!(
            codes(&run(&manifest, &component)),
            vec!["package.api-section-repeated"]
        );
    }

    /// REQ: PKG-043
    #[test]
    fn exports_equal_to_manifest_list_have_no_findings() {
        let manifest = manifest_with(&["detector", "migrator"], &[], &[]);
        let component = ComponentSummary {
            exports: vec!["detector".to_owned(), "migrator".to_owned()],
            ..valid_component()
        };

        assert_eq!(run(&manifest, &component), Vec::new());
    }

    /// REQ: PKG-043
    #[test]
    fn export_order_does_not_matter_when_names_are_equal() {
        let manifest = manifest_with(&["migrator", "detector"], &[], &[]);
        let component = ComponentSummary {
            exports: vec!["detector".to_owned(), "migrator".to_owned()],
            ..valid_component()
        };

        assert_eq!(run(&manifest, &component), Vec::new());
    }

    /// REQ: ABI-006
    #[test]
    fn export_present_in_component_but_not_in_manifest_is_refused() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            exports: vec!["migrator".to_owned()],
            ..valid_component()
        };

        assert_eq!(
            codes(&run(&manifest, &component)),
            vec!["manifest.exports-mismatch"]
        );
    }

    /// REQ: ABI-006
    #[test]
    fn export_listed_in_manifest_but_absent_from_component_is_refused() {
        let manifest = manifest_with(&["migrator"], &[], &[]);
        let component = valid_component();

        assert_eq!(
            codes(&run(&manifest, &component)),
            vec!["manifest.exports-mismatch"]
        );
    }

    /// REQ: HST-072
    #[test]
    fn import_of_interface_no_requested_grant_unlocks_is_refused() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            imports: vec!["ilium:host/clipboard@0.1.0".to_owned()],
            ..valid_component()
        };

        assert_eq!(
            codes(&run(&manifest, &component)),
            vec!["manifest.import-not-requested"]
        );
    }

    /// REQ: ABI-023
    #[test]
    fn import_of_interface_unlocked_by_requested_grant_is_accepted() {
        let manifest = manifest_with(&[], &["tree.read"], &[]);
        let component = ComponentSummary {
            imports: vec!["ilium:host/tree@0.1.0".to_owned()],
            ..valid_component()
        };

        assert_eq!(run(&manifest, &component), Vec::new());
    }

    /// REQ: ABI-023
    #[test]
    fn import_of_always_granted_interface_needs_no_grant() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            imports: vec!["ilium:host/log@0.1.0".to_owned()],
            ..valid_component()
        };

        assert_eq!(run(&manifest, &component), Vec::new());
    }

    /// REQ: ABI-024
    #[test]
    fn wasi_sockets_import_is_refused_as_forbidden() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            imports: vec!["wasi:sockets/tcp@0.2.0".to_owned()],
            ..valid_component()
        };

        assert!(codes(&run(&manifest, &component)).contains(&"component.wasi-forbidden"));
    }

    /// REQ: ABI-024
    #[test]
    fn wasi_http_import_is_refused_as_forbidden() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            imports: vec!["wasi:http/outgoing-handler@0.2.0".to_owned()],
            ..valid_component()
        };

        assert!(codes(&run(&manifest, &component)).contains(&"component.wasi-forbidden"));
    }

    /// REQ: ABI-022
    #[test]
    fn unstable_import_not_listed_in_manifest_is_refused() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            unstable_imports: vec!["ui-calendar".to_owned()],
            ..valid_component()
        };

        assert_eq!(
            codes(&run(&manifest, &component)),
            vec!["manifest.unstable-missing"]
        );
    }

    /// REQ: ABI-022
    #[test]
    fn unstable_import_listed_in_manifest_is_accepted() {
        let manifest = manifest_with(&[], &[], &["ui-calendar"]);
        let component = ComponentSummary {
            unstable_imports: vec!["ui-calendar".to_owned()],
            ..valid_component()
        };

        assert_eq!(run(&manifest, &component), Vec::new());
    }

    /// REQ: HST-084
    #[test]
    fn findings_come_in_refusal_order_with_error_severity() {
        let manifest = manifest_with(&[], &[], &[]);
        let component = ComponentSummary {
            exports: vec!["migrator".to_owned()],
            api_sections: vec![vec![0x00, 0x00, 0x00, 0x02, 0x00, 0x00]],
            unstable_imports: vec!["ui-calendar".to_owned()],
            ..ComponentSummary::default()
        };
        let diagnostics = run(&manifest, &component);

        assert_eq!(
            codes(&diagnostics),
            vec![
                "manifest.exports-mismatch",
                "manifest.api-version",
                "manifest.unstable-missing",
            ]
        );
        assert!(diagnostics
            .iter()
            .all(|diagnostic| matches!(diagnostic.severity, Severity::Error)));
    }
}
