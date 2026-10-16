//! Accessibility and layout rules checked against the SDK types and the pages
//! (accessibility-and-theming.md, layout.md, ui-model.md).

use ilium_plugin_sdk::types::{ColorRole, Constraint, Density, IconMode, ThemeExtras};

use crate::support::page;

fn role_name(role: ColorRole) -> &'static str {
    match role {
        ColorRole::Foreground => "foreground",
        ColorRole::Muted => "muted",
        ColorRole::Accent => "accent",
        ColorRole::Success => "success",
        ColorRole::Warning => "warning",
        ColorRole::Danger => "danger",
        ColorRole::Info => "info",
        ColorRole::Selection => "selection",
        ColorRole::Border => "border",
    }
}

const ROLES: [ColorRole; 9] = [
    ColorRole::Foreground,
    ColorRole::Muted,
    ColorRole::Accent,
    ColorRole::Success,
    ColorRole::Warning,
    ColorRole::Danger,
    ColorRole::Info,
    ColorRole::Selection,
    ColorRole::Border,
];

/// REQ: AUT-504 WARN
#[test]
fn aut_504_there_are_exactly_nine_colour_roles() {
    let text = page("accessibility-and-theming");
    let line = text
        .lines()
        .find(|line| line.starts_with("enum color-role {"))
        .expect("the page declares enum color-role");
    let inner = line.trim_start_matches("enum color-role {").trim_end_matches('}');
    let declared: Vec<&str> = inner.split(',').map(str::trim).collect();
    assert_eq!(declared.len(), 9);
    assert_eq!(ColorRole::CASE_NAMES, declared.as_slice());
    assert_eq!(ColorRole::CASE_NAMES.len(), 9);
}

/// REQ: AUT-504 WARN
#[test]
fn aut_504_the_accessibility_page_names_every_role() {
    let text = page("accessibility-and-theming");
    for role in ROLES {
        assert!(text.contains(role_name(role)), "role {} missing on the page", role_name(role));
    }
}

/// REQ: AUT-503 WARN
#[test]
fn aut_503_theme_extras_expose_compact_density_and_both_icon_modes() {
    assert_eq!(Density::CASE_NAMES, &["compact", "normal", "comfortable"]);
    assert_eq!(IconMode::CASE_NAMES, &["normal", "stable"]);
    let text = page("accessibility-and-theming");
    assert!(text.contains("enum density { compact, normal, comfortable }"));
}

/// REQ: AUT-502
#[test]
fn aut_502_theme_extras_carry_the_reduced_motion_flag() {
    let text = page("accessibility-and-theming");
    assert!(text.contains(
        "record theme-extras { reduced-motion: bool, density: density, info: u32, selection: u32, border: u32, icon-mode: icon-mode }"
    ));
    assert!(text.contains("A change of the setting delivers `theme-changed`."));
    let extras = ThemeExtras {
        reduced_motion: true,
        density: Density::Normal,
        info: 0,
        selection: 0,
        border: 0,
        icon_mode: IconMode::Normal,
    };
    let off = ThemeExtras { reduced_motion: false, ..extras.clone() };
    assert_ne!(extras, off);
}

/// REQ: AUT-506 WARN, AUT-505 WARN
#[test]
fn aut_506_constraint_cases_match_the_layout_page_table() {
    assert_eq!(
        Constraint::CASE_NAMES,
        &["length", "min", "max", "percentage", "ratio", "fill"]
    );
    let text = page("layout");
    for case in Constraint::CASE_NAMES {
        assert!(text.contains(&format!("| `{case}` |")), "constraint {case} missing from the table");
    }
    assert_eq!(Constraint::Min(30).case_name(), "min");
    assert_eq!(Constraint::Fill(1).case_name(), "fill");
    assert_eq!(Constraint::Percentage(25).case_name(), "percentage");
    assert_ne!(Constraint::Min(29), Constraint::Min(30));
    assert_ne!(Constraint::Min(30), Constraint::Min(31));
}

/// REQ: AUT-505 WARN
#[test]
fn aut_505_the_lint_renders_at_40_columns_by_12_rows() {
    let rules = page("reference-check-and-lint-rules");
    assert!(rules.contains("at 40 columns by 12 rows"));
    let layout = page("layout");
    assert!(layout.contains("which is the size `lint.widget-missing-key` renders at"));
}

/// REQ: AUT-501, AUT-410
#[test]
fn doc_aut_501_ui_model_repeats_the_word_or_glyph_rule() {
    let text = page("ui-model");
    assert!(text.contains("Add a word or glyph to every status"));
}
