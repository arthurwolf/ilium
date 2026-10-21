//! Platform rules (reference-platform-support.md): sandbox availability, path normalisation,
//! tick gaps, process termination and precompiled artifact selection.

use std::time::Duration;

use ilium_plugin_api::platform::Platform;
use ilium_plugin_api::Diagnostic;
use ilium_plugin_api::Tier;

/// Whether the tier's sandbox is available on the platform; `sandbox.unavailable` otherwise.
pub fn check_sandbox(_platform: Platform, _tier: Tier) -> Result<(), Diagnostic> {
    unimplemented!("host::platform_rules::check_sandbox")
}

/// Normalises a plugin-supplied path to the platform's form.
pub fn normalize_plugin_path(_platform: Platform, _path: &str) -> String {
    unimplemented!("host::platform_rules::normalize_plugin_path")
}

/// Whether a gap between scheduler ticks is treated as a suspend and not charged.
pub fn tick_gap_ignored(_gap: Duration) -> bool {
    unimplemented!("host::platform_rules::tick_gap_ignored")
}

/// How child processes are terminated on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminationKind {
    /// SIGTERM, then SIGKILL after the grace.
    SigtermThenSigkill,
    /// A Windows job object.
    JobObject,
}

/// The termination plan of a platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminationPlan {
    /// The mechanism.
    pub kind: TerminationKind,
    /// The grace before the forced kill.
    pub grace: Duration,
    /// Children are killed with the process (job object).
    pub children_in_job: bool,
}

/// The termination plan of a platform.
pub fn termination_plan(_platform: Platform) -> TerminationPlan {
    unimplemented!("host::platform_rules::termination_plan")
}

/// The target triples the host ships precompiled artifacts for.
pub fn shipped_triples() -> Vec<String> {
    unimplemented!("host::platform_rules::shipped_triples")
}

/// One artifact of a package: precompiled for a triple, or the portable module.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WasmArtifact {
    /// A precompiled `.cwasm` for a target triple.
    Cwasm(String),
    /// The portable `.wasm` module.
    Wasm,
}

impl WasmArtifact {
    /// A precompiled artifact for `triple`.
    pub fn cwasm(_triple: &str) -> WasmArtifact {
        unimplemented!("host::platform_rules::WasmArtifact::cwasm")
    }

    /// The portable module.
    pub fn wasm() -> WasmArtifact {
        unimplemented!("host::platform_rules::WasmArtifact::wasm")
    }
}

/// The artifact chosen for the host triple and whether a fallback was logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    /// The artifact to load.
    pub artifact: WasmArtifact,
    /// A fallback to the portable module was logged.
    pub logged_fallback: bool,
    /// The log code of the fallback (empty when none).
    pub log_code: String,
}

/// Picks the artifact for `host_triple` from the package's artifacts.
pub fn select_precompiled(_host_triple: &str, _artifacts: &[WasmArtifact]) -> Selected {
    unimplemented!("host::platform_rules::select_precompiled")
}

#[cfg(test)]
mod tests {
    use super::{
        check_sandbox, normalize_plugin_path, select_precompiled, shipped_triples,
        termination_plan, tick_gap_ignored, TerminationKind, TerminationPlan, WasmArtifact,
    };
    use ilium_plugin_api::{Diagnostic, Platform, Severity, Tier};
    use std::time::Duration;

    const SANDBOX_UNAVAILABLE_CODE: &str = "sandbox.unavailable";
    const CWASM_INCOMPATIBLE_CODE: &str = "host.cwasm-incompatible";
    const LINUX_X86_64_TRIPLE: &str = "x86_64-unknown-linux-gnu";
    const LINUX_AARCH64_TRIPLE: &str = "aarch64-unknown-linux-gnu";
    const MACOS_AARCH64_TRIPLE: &str = "aarch64-apple-darwin";
    const WINDOWS_X86_64_TRIPLE: &str = "x86_64-pc-windows-msvc";

    /// Asserts that a T2 or T3 load on `platform` is refused with `sandbox.unavailable`
    /// and returns the diagnostic for further checks.
    fn refused_sandbox(platform: Platform, tier: Tier) -> Diagnostic {
        let diagnostic = check_sandbox(platform, tier).expect_err(
            "T2 and T3 must be refused on macOS and Windows",
        );
        assert_eq!(diagnostic.code.as_str(), SANDBOX_UNAVAILABLE_CODE);
        assert_eq!(diagnostic.severity, Severity::Error);
        diagnostic
    }

    /// REQ: HST-380
    #[test]
    fn check_sandbox_refuses_t2_on_macos_naming_the_platform() {
        let diagnostic = refused_sandbox(Platform::MacOs, Tier::T2);
        assert!(
            diagnostic.message.contains("macos"),
            "message must name the platform, got {:?}",
            diagnostic.message
        );
    }

    /// REQ: HST-380
    #[test]
    fn check_sandbox_refuses_t3_on_macos_naming_the_platform() {
        let diagnostic = refused_sandbox(Platform::MacOs, Tier::T3);
        assert!(
            diagnostic.message.contains("macos"),
            "message must name the platform, got {:?}",
            diagnostic.message
        );
    }

    /// REQ: HST-380
    #[test]
    fn check_sandbox_refuses_t2_on_windows_naming_the_platform() {
        let diagnostic = refused_sandbox(Platform::Windows, Tier::T2);
        assert!(
            diagnostic.message.contains("windows"),
            "message must name the platform, got {:?}",
            diagnostic.message
        );
    }

    /// REQ: HST-380
    #[test]
    fn check_sandbox_refuses_t3_on_windows_naming_the_platform() {
        let diagnostic = refused_sandbox(Platform::Windows, Tier::T3);
        assert!(
            diagnostic.message.contains("windows"),
            "message must name the platform, got {:?}",
            diagnostic.message
        );
    }

    /// REQ: HST-380
    #[test]
    fn check_sandbox_allows_t2_and_t3_on_linux() {
        assert_eq!(check_sandbox(Platform::Linux, Tier::T2), Ok(()));
        assert_eq!(check_sandbox(Platform::Linux, Tier::T3), Ok(()));
    }

    /// REQ: HST-381
    #[test]
    fn check_sandbox_allows_t1_on_every_platform() {
        for platform in Platform::ALL {
            assert_eq!(
                check_sandbox(platform, Tier::T1),
                Ok(()),
                "T1 must load on {platform}"
            );
        }
    }

    /// REQ: HST-384
    #[test]
    fn normalize_plugin_path_turns_windows_backslashes_into_slashes() {
        assert_eq!(
            normalize_plugin_path(Platform::Windows, r"C:\plugins\data\file.txt"),
            "C:/plugins/data/file.txt"
        );
    }

    /// REQ: HST-384
    #[test]
    fn normalize_plugin_path_handles_mixed_separators_on_windows() {
        assert_eq!(
            normalize_plugin_path(Platform::Windows, r"a\b/c\d"),
            "a/b/c/d"
        );
    }

    /// REQ: HST-384
    #[test]
    fn normalize_plugin_path_keeps_trailing_separator_on_windows() {
        assert_eq!(normalize_plugin_path(Platform::Windows, r"dir\"), "dir/");
    }

    /// REQ: HST-384
    #[test]
    fn normalize_plugin_path_keeps_slash_paths_on_linux_and_macos() {
        assert_eq!(
            normalize_plugin_path(Platform::Linux, "/home/user/data/x.json"),
            "/home/user/data/x.json"
        );
        assert_eq!(
            normalize_plugin_path(Platform::MacOs, "/Users/user/data/x.json"),
            "/Users/user/data/x.json"
        );
    }

    /// REQ: HST-384
    #[test]
    fn normalize_plugin_path_returns_empty_input_unchanged_on_every_platform() {
        for platform in Platform::ALL {
            assert_eq!(
                normalize_plugin_path(platform, ""),
                "",
                "empty path must stay empty on {platform}"
            );
        }
    }

    /// REQ: HST-386
    #[test]
    fn tick_gap_ignored_is_false_for_zero_and_short_gaps() {
        assert!(!tick_gap_ignored(Duration::ZERO));
        assert!(!tick_gap_ignored(Duration::from_millis(500)));
    }

    /// REQ: HST-386
    #[test]
    fn tick_gap_ignored_is_false_at_exactly_one_second() {
        assert!(
            !tick_gap_ignored(Duration::from_secs(1)),
            "a gap of exactly one second is not more than one second"
        );
    }

    /// REQ: HST-386
    #[test]
    fn tick_gap_ignored_is_true_just_above_one_second() {
        assert!(tick_gap_ignored(Duration::from_secs(1) + Duration::from_nanos(1)));
    }

    /// REQ: HST-386
    #[test]
    fn tick_gap_ignored_is_true_for_a_long_suspend_gap() {
        assert!(tick_gap_ignored(Duration::from_secs(3600)));
    }

    /// REQ: HST-385
    #[test]
    fn termination_plan_on_linux_is_sigterm_then_sigkill_after_two_seconds() {
        assert_eq!(
            termination_plan(Platform::Linux),
            TerminationPlan {
                kind: TerminationKind::SigtermThenSigkill,
                grace: Duration::from_secs(2),
                children_in_job: false,
            }
        );
    }

    /// REQ: HST-385
    #[test]
    fn termination_plan_on_macos_is_sigterm_then_sigkill_after_two_seconds() {
        assert_eq!(
            termination_plan(Platform::MacOs),
            TerminationPlan {
                kind: TerminationKind::SigtermThenSigkill,
                grace: Duration::from_secs(2),
                children_in_job: false,
            }
        );
    }

    /// REQ: HST-385
    #[test]
    fn termination_plan_on_windows_uses_job_object_with_children_inside() {
        assert_eq!(
            termination_plan(Platform::Windows),
            TerminationPlan {
                kind: TerminationKind::JobObject,
                grace: Duration::from_secs(2),
                children_in_job: true,
            }
        );
    }

    /// REQ: HST-383
    #[test]
    fn shipped_triples_lists_exactly_the_four_documented_triples() {
        let mut triples = shipped_triples();
        triples.sort();
        let mut expected = vec![
            LINUX_X86_64_TRIPLE.to_string(),
            LINUX_AARCH64_TRIPLE.to_string(),
            MACOS_AARCH64_TRIPLE.to_string(),
            WINDOWS_X86_64_TRIPLE.to_string(),
        ];
        expected.sort();
        assert_eq!(triples, expected);
    }

    /// REQ: HST-383
    #[test]
    fn shipped_triples_has_no_duplicates() {
        let triples = shipped_triples();
        let mut unique = triples.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), triples.len());
    }

    /// REQ: HST-383
    #[test]
    fn wasm_artifact_cwasm_wraps_the_triple() {
        assert_eq!(
            WasmArtifact::cwasm(LINUX_X86_64_TRIPLE),
            WasmArtifact::Cwasm(LINUX_X86_64_TRIPLE.to_string())
        );
    }

    /// REQ: HST-383
    #[test]
    fn wasm_artifact_wasm_is_the_portable_module() {
        assert_eq!(WasmArtifact::wasm(), WasmArtifact::Wasm);
    }

    /// REQ: HST-383
    #[test]
    fn select_precompiled_picks_the_matching_cwasm_without_fallback() {
        let artifacts = vec![
            WasmArtifact::Wasm,
            WasmArtifact::cwasm(LINUX_X86_64_TRIPLE),
            WasmArtifact::cwasm(MACOS_AARCH64_TRIPLE),
        ];
        let selected = select_precompiled(LINUX_X86_64_TRIPLE, &artifacts);
        assert_eq!(selected.artifact, WasmArtifact::cwasm(LINUX_X86_64_TRIPLE));
        assert!(!selected.logged_fallback);
        assert_eq!(selected.log_code, "");
    }

    /// REQ: HST-383
    #[test]
    fn select_precompiled_falls_back_to_wasm_and_logs_when_no_cwasm_matches() {
        let artifacts = vec![
            WasmArtifact::Wasm,
            WasmArtifact::cwasm(LINUX_AARCH64_TRIPLE),
        ];
        let selected = select_precompiled(WINDOWS_X86_64_TRIPLE, &artifacts);
        assert_eq!(selected.artifact, WasmArtifact::Wasm);
        assert!(selected.logged_fallback);
        assert_eq!(selected.log_code, CWASM_INCOMPATIBLE_CODE);
    }

    /// REQ: HST-383
    #[test]
    fn select_precompiled_falls_back_when_only_wasm_is_present() {
        let artifacts = vec![WasmArtifact::Wasm];
        let selected = select_precompiled(LINUX_X86_64_TRIPLE, &artifacts);
        assert_eq!(selected.artifact, WasmArtifact::Wasm);
        assert!(selected.logged_fallback);
        assert_eq!(selected.log_code, CWASM_INCOMPATIBLE_CODE);
    }

    /// REQ: HST-383
    #[test]
    fn select_precompiled_does_not_match_an_empty_host_triple() {
        let artifacts = vec![
            WasmArtifact::Wasm,
            WasmArtifact::cwasm(LINUX_X86_64_TRIPLE),
        ];
        let selected = select_precompiled("", &artifacts);
        assert_eq!(selected.artifact, WasmArtifact::Wasm);
        assert!(selected.logged_fallback);
        assert_eq!(selected.log_code, CWASM_INCOMPATIBLE_CODE);
    }

    /// REQ: HST-383
    #[test]
    fn select_precompiled_with_no_artifacts_falls_back_to_wasm_and_logs() {
        // Assumption: the manual does not define an empty package; the portable module is the
        // only artifact the host can fall back to (HST-383), so the empty case is reported as
        // a fallback with the incompatibility code.
        let selected = select_precompiled(LINUX_X86_64_TRIPLE, &[]);
        assert_eq!(selected.artifact, WasmArtifact::Wasm);
        assert!(selected.logged_fallback);
        assert_eq!(selected.log_code, CWASM_INCOMPATIBLE_CODE);
    }
}
