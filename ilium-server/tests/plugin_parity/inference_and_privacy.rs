//! Baseline scenarios of src/docs/inference-and-privacy.md: F-085 AI admission gate and privacy.
//! "New installs enable AI titles and tree organization through Kilo Gateway, but automatic
//! requests stay paused until the first-run guided setup is finished."

use crate::common::*;

/// REQ: F-085, COR-001
#[test]
fn f_085_automatic_requests_are_paused_until_onboarding_is_finished() {
    in_both_modes(|kernel| {
        assert!(!kernel.ai_request_admitted("title"));
    });
}

/// REQ: F-085, COR-001
#[test]
fn f_085_finishing_onboarding_admits_automatic_requests() {
    in_both_modes(|kernel| {
        kernel.finish_onboarding(true);
        assert!(kernel.ai_request_admitted("title"));
    });
}

/// REQ: F-085, COR-001
#[test]
fn f_085_choosing_skip_in_setup_keeps_automatic_requests_off() {
    in_both_modes(|kernel| {
        kernel.finish_onboarding(false);
        assert!(!kernel.ai_request_admitted("title"));
        assert!(!kernel.ai_request_admitted("tree-organization"));
    });
}

/// REQ: F-085, COR-001
#[test]
fn f_085_default_provider_is_kilo_gateway() {
    in_both_modes(|kernel| {
        assert_eq!(
            kernel.setting("inference", "selected_provider").as_deref(),
            Some("kilo_gateway")
        );
    });
}

/// REQ: F-085, COR-001
#[test]
fn f_085_selected_provider_accepts_the_five_documented_providers_only() {
    in_both_modes(|kernel| {
        for value in [
            "kilo_gateway",
            "ollama",
            "open_ai",
            "anthropic",
            "open_router",
        ] {
            assert_eq!(
                kernel.apply_setting("inference", "selected_provider", value),
                Ok(()),
                "{value}"
            );
        }
        assert!(kernel
            .apply_setting("inference", "selected_provider", "telepathy")
            .is_err());
    });
}
