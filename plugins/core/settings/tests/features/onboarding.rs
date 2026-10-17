//! Guided setup (F-089): the plugin owns the flow, the host builds the rows of each step from a
//! settings section as a `schema-form` and saves every change; the plugin writes nothing itself.

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};

use crate::common::*;

fn first_run() -> ViewTarget {
    ViewTarget::OnboardingStep("first-run".to_string())
}

/// REQ: F-089
#[test]
fn onboarding_089_the_manifest_step_has_id_title_and_order() {
    let step = &manifest()["contributes"]["onboarding_steps"][0];
    assert_eq!(step["id"], "first-run");
    assert_eq!(step["title"], "Welcome");
    assert_eq!(step["order"], 10);
}

/// REQ: F-089
#[test]
fn onboarding_089_the_first_run_step_renders_at_three_widths() {
    for width in [40u16, 80, 120] {
        let mut host = client_host();
        let screen = host.render(first_run(), width, 12);
        assert_eq!((screen.columns, screen.rows), (width, 12));
        assert!(!screen.text().trim().is_empty(), "width {width}");
    }
}

/// REQ: F-089
#[test]
fn onboarding_089_the_step_shows_its_welcome_title() {
    let mut host = client_host();
    let text = host.render(first_run(), 80, 12).text();
    assert!(text.contains("Welcome"), "{text}");
}

/// REQ: F-089
#[test]
fn onboarding_089_the_step_shows_its_position_and_the_enter_and_escape_hints() {
    let mut host = client_host();
    let text = host.render(first_run(), 80, 12).text();
    assert!(text.contains("Step ") && text.contains(" of "), "{text}");
    assert!(text.contains("Enter next"), "{text}");
    assert!(text.contains("Esc skip"), "{text}");
}

/// REQ: F-089
#[test]
fn onboarding_089_a_change_in_the_step_form_makes_no_submission_the_host_saves_it() {
    let mut host = client_host();
    host.send_action(
        first_run(),
        "form",
        ActionKind::Changed,
        ActionValue::Flag(true),
    );
    host.settle();
    assert!(host.submitted().is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-089
#[test]
fn onboarding_089_the_settings_page_gets_the_appearance_form() {
    let mut host = client_host();
    let settings = host
        .render(
            ViewTarget::SettingsPage("user-interface".to_string()),
            80,
            12,
        )
        .text();
    assert!(settings.contains("Density"), "{settings}");
}

/// REQ: F-089
#[test]
fn onboarding_089_the_step_view_is_the_same_on_every_render() {
    let mut host = client_host();
    let first = host.render(first_run(), 80, 12);
    let second = host.render(first_run(), 80, 12);
    assert_eq!(first, second);
}
