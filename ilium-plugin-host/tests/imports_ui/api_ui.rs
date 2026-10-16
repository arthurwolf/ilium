//! `ilium:host/ui-host` (api-ui.md, api-popovers.md): panels, popups, widget state, theme, text
//! width and surfaces. A view's last committed tree is scripted through the fake kernel.

use ilium_plugin_api::generated::host::ui_host::HostUiHost;
use ilium_plugin_api::generated::types::layout::Constraint;
use ilium_plugin_api::generated::types::ui::{
    AnchorSide, Density, IconMode, KeyAnchor, PixelFormat, PopupAnchor, PopupOptions, ViewTarget, WidgetState,
};
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &["ui.panels"];

fn panel_view() -> ViewTarget {
    ViewTarget::Panel("notes".to_owned())
}

fn options() -> PopupOptions {
    PopupOptions { modal: false, pinnable: true, dismiss_on_leave: false, close_on_escape: true }
}

fn centered() -> PopupAnchor {
    PopupAnchor::Centered((Constraint::Percentage(50), Constraint::Percentage(50)))
}

/// A rig whose panel `notes` is open and whose committed tree holds the given popup keys, a
/// button `delete`, a list `notes-list`, a text input `title` and a surface `canvas` (rgb8, 4x4).
fn seeded(popups: &[&str]) -> Rig {
    let rig = rig(GRANTS);
    rig.kernel.declare_panel("notes");
    rig.kernel.commit_view_tree(
        "notes",
        &[("button", "delete"), ("list", "notes-list"), ("text-input", "title"), ("surface:rgb8:4x4", "canvas")],
        popups,
    );
    rig
}

/// REQ: WIT:ilium:host/ui-host.open-panel
#[test]
fn wit_ui_host_open_panel_shows_a_declared_panel() {
    let mut rig = seeded(&[]);
    rig.host.ui_host().open_panel("notes".to_owned()).expect("opens");
    assert!(rig.kernel.open_panels().contains(&"notes".to_owned()));
}

/// REQ: WIT:ilium:host/ui-host.open-panel
#[test]
fn wit_ui_host_open_panel_undeclared_panel_not_found() {
    let mut rig = seeded(&[]);
    assert_eq!(code_of(rig.host.ui_host().open_panel("ghost".to_owned())), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/ui-host.open-panel
#[test]
fn wit_ui_host_open_panel_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["ui.panels"]);
    rig.kernel.declare_panel("notes");
    assert_eq!(code_of(rig.host.ui_host().open_panel("notes".to_owned())), ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/ui-host.close-panel
#[test]
fn wit_ui_host_close_panel_hides_the_panel() {
    let mut rig = seeded(&[]);
    rig.host.ui_host().open_panel("notes".to_owned()).expect("opens");
    rig.host.ui_host().close_panel("notes".to_owned());
    assert!(!rig.kernel.open_panels().contains(&"notes".to_owned()));
}

/// REQ: WIT:ilium:host/ui-host.close-panel
#[test]
fn wit_ui_host_close_panel_not_open_is_a_no_op() {
    let mut rig = seeded(&[]);
    rig.host.ui_host().close_panel("notes".to_owned());
    assert!(rig.host.failures().is_empty());
}

/// REQ: WIT:ilium:host/ui-host.close-panel
#[test]
fn wit_ui_host_close_panel_undeclared_panel_is_a_no_op() {
    let mut rig = seeded(&[]);
    rig.host.ui_host().close_panel("ghost".to_owned());
    assert!(rig.host.failures().is_empty());
}

/// REQ: WIT:ilium:host/ui-host.open-popup
#[test]
fn wit_ui_host_open_popup_shows_the_popup_node_of_the_committed_tree() {
    let mut rig = seeded(&["keys-help"]);
    rig.host.ui_host().open_panel("notes".to_owned()).expect("opens");
    rig.host.ui_host().open_popup("keys-help".to_owned(), PopupAnchor::BelowKey("delete".to_owned())).expect("opens");
    assert!(rig.kernel.open_popups().contains(&"keys-help".to_owned()));
}

/// REQ: WIT:ilium:host/ui-host.open-popup
#[test]
fn wit_ui_host_open_popup_no_matching_node_not_found() {
    let mut rig = seeded(&[]);
    assert_eq!(code_of(rig.host.ui_host().open_popup("keys-help".to_owned(), centered())), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/ui-host.open-popup
#[test]
fn wit_ui_host_open_popup_two_matching_nodes_invalid_argument() {
    let mut rig = seeded(&["dup", "dup"]);
    assert_eq!(code_of(rig.host.ui_host().open_popup("dup".to_owned(), centered())), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/ui-host.open-popup
#[test]
fn wit_ui_host_open_popup_fifth_popup_in_one_view_quota_exceeded() {
    let mut rig = seeded(&["p1", "p2", "p3", "p4", "p5"]);
    for name in ["p1", "p2", "p3", "p4"] {
        rig.host.ui_host().open_popup(name.to_owned(), centered()).expect("within the limit of 4");
    }
    assert_eq!(code_of(rig.host.ui_host().open_popup("p5".to_owned(), centered())), ErrorCode::QuotaExceeded);
}

/// REQ: WIT:ilium:host/ui-host.open-popup
#[test]
fn wit_ui_host_open_popup_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["ui.panels"]);
    assert_eq!(code_of(rig.host.ui_host().open_popup("p".to_owned(), centered())), ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/ui-host.open-popup-with
#[test]
fn wit_ui_host_open_popup_with_anchors_at_a_key() {
    let mut rig = seeded(&["detail"]);
    let anchor = PopupAnchor::AtKey(KeyAnchor { key: "delete".to_owned(), side: AnchorSide::Right });
    rig.host.ui_host().open_popup_with("detail".to_owned(), anchor, options()).expect("opens");
    assert!(rig.kernel.open_popups().contains(&"detail".to_owned()));
    assert_eq!(rig.kernel.popup_options("detail"), Some(options()));
}

/// REQ: WIT:ilium:host/ui-host.open-popup-with
#[test]
fn wit_ui_host_open_popup_with_anchor_key_not_in_the_view_invalid_argument() {
    let mut rig = seeded(&["detail"]);
    let anchor = PopupAnchor::AtKey(KeyAnchor { key: "no-such-key".to_owned(), side: AnchorSide::Below });
    assert_eq!(code_of(rig.host.ui_host().open_popup_with("detail".to_owned(), anchor, options())), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/ui-host.open-popup-with
#[test]
fn wit_ui_host_open_popup_with_unknown_popup_not_found() {
    let mut rig = seeded(&[]);
    assert_eq!(code_of(rig.host.ui_host().open_popup_with("ghost".to_owned(), PopupAnchor::AtPointer, options())), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/ui-host.open-popup-with
#[test]
fn wit_ui_host_open_popup_with_fifth_popup_quota_exceeded() {
    let mut rig = seeded(&["p1", "p2", "p3", "p4", "p5"]);
    for name in ["p1", "p2", "p3", "p4"] {
        rig.host.ui_host().open_popup_with(name.to_owned(), PopupAnchor::AtPointer, options()).expect("within the limit");
    }
    assert_eq!(code_of(rig.host.ui_host().open_popup_with("p5".to_owned(), PopupAnchor::AtPointer, options())), ErrorCode::QuotaExceeded);
}

/// REQ: WIT:ilium:host/ui-host.close-popup
#[test]
fn wit_ui_host_close_popup_hides_it_and_frees_a_slot() {
    let mut rig = seeded(&["p1", "p2", "p3", "p4", "p5"]);
    for name in ["p1", "p2", "p3", "p4"] {
        rig.host.ui_host().open_popup(name.to_owned(), centered()).expect("opens");
    }
    rig.host.ui_host().close_popup("p1".to_owned());
    assert!(!rig.kernel.open_popups().contains(&"p1".to_owned()));
    assert!(rig.host.ui_host().open_popup("p5".to_owned(), centered()).is_ok());
}

/// REQ: WIT:ilium:host/ui-host.close-popup
#[test]
fn wit_ui_host_close_popup_not_open_is_a_no_op() {
    let mut rig = seeded(&["p1"]);
    rig.host.ui_host().close_popup("p1".to_owned());
    rig.host.ui_host().close_popup("ghost".to_owned());
    assert!(rig.host.failures().is_empty());
}

/// REQ: WIT:ilium:host/ui-host.invalidate
#[test]
fn wit_ui_host_invalidate_queues_one_render_of_the_view() {
    let mut rig = seeded(&[]);
    rig.host.ui_host().invalidate(panel_view());
    rig.host.pump(8);
    assert_eq!(rig.kernel.render_count("notes"), 1);
}

/// REQ: WIT:ilium:host/ui-host.invalidate
#[test]
fn wit_ui_host_invalidate_repeated_calls_coalesce_into_one_render() {
    let mut rig = seeded(&[]);
    for _ in 0..5 {
        rig.host.ui_host().invalidate(panel_view());
    }
    rig.host.pump(8);
    assert_eq!(rig.kernel.render_count("notes"), 1);
}

/// REQ: WIT:ilium:host/ui-host.invalidate
#[test]
fn wit_ui_host_invalidate_unknown_view_never_fails() {
    let mut rig = seeded(&[]);
    rig.host.ui_host().invalidate(ViewTarget::Panel("ghost".to_owned()));
    assert!(rig.host.failures().is_empty());
}

/// REQ: WIT:ilium:host/ui-host.widget-state
#[test]
fn wit_ui_host_widget_state_reads_the_host_owned_state() {
    let mut rig = seeded(&[]);
    rig.kernel.set_widget_state("notes", "title", WidgetState::Text("abc".to_owned()));
    assert_eq!(rig.host.ui_host().widget_state(panel_view(), "title".to_owned()), Some(WidgetState::Text("abc".to_owned())));
}

/// REQ: WIT:ilium:host/ui-host.widget-state
#[test]
fn wit_ui_host_widget_state_unknown_key_is_none() {
    let mut rig = seeded(&[]);
    assert_eq!(rig.host.ui_host().widget_state(panel_view(), "ghost".to_owned()), None);
}

/// REQ: WIT:ilium:host/ui-host.widget-state
#[test]
fn wit_ui_host_widget_state_unknown_view_is_none() {
    let mut rig = seeded(&[]);
    assert_eq!(rig.host.ui_host().widget_state(ViewTarget::Panel("ghost".to_owned()), "title".to_owned()), None);
}

/// REQ: WIT:ilium:host/ui-host.set-widget-state
#[test]
fn wit_ui_host_set_widget_state_selects_a_list_row() {
    let mut rig = seeded(&[]);
    rig.host.ui_host().set_widget_state(panel_view(), "notes-list".to_owned(), WidgetState::Selected(Some(0))).expect("sets");
    assert_eq!(rig.host.ui_host().widget_state(panel_view(), "notes-list".to_owned()), Some(WidgetState::Selected(Some(0))));
}

/// REQ: WIT:ilium:host/ui-host.set-widget-state
#[test]
fn wit_ui_host_set_widget_state_unknown_key_not_found() {
    let mut rig = seeded(&[]);
    let result = rig.host.ui_host().set_widget_state(panel_view(), "ghost".to_owned(), WidgetState::Offset(1));
    assert_eq!(code_of(result), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/ui-host.set-widget-state
#[test]
fn wit_ui_host_set_widget_state_offset_on_a_button_invalid_argument() {
    let mut rig = seeded(&[]);
    let result = rig.host.ui_host().set_widget_state(panel_view(), "delete".to_owned(), WidgetState::Offset(1));
    assert_eq!(code_of(result), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/ui-host.display-width
#[test]
fn wit_ui_host_display_width_ascii_counts_one_cell_per_character() {
    let mut rig = seeded(&[]);
    assert_eq!(rig.host.ui_host().display_width("hello".to_owned()), 5);
    assert_eq!(rig.host.ui_host().display_width(String::new()), 0);
}

/// REQ: WIT:ilium:host/ui-host.display-width
#[test]
fn wit_ui_host_display_width_wide_glyphs_count_two_cells() {
    let mut rig = seeded(&[]);
    assert_eq!(rig.host.ui_host().display_width("\u{4f60}\u{597d}".to_owned()), 4);
}

/// REQ: WIT:ilium:host/ui-host.display-width
#[test]
fn wit_ui_host_display_width_combining_marks_add_no_width() {
    let mut rig = seeded(&[]);
    assert_eq!(rig.host.ui_host().display_width("e\u{301}".to_owned()), 1);
}

/// REQ: WIT:ilium:host/ui-host.theme
#[test]
fn wit_ui_host_theme_returns_the_active_tokens() {
    let mut rig = seeded(&[]);
    rig.kernel.set_theme(0x00_11_22_33, true);
    let theme = rig.host.ui_host().theme();
    assert_eq!(theme.foreground, 0x00_11_22_33);
    assert!(theme.is_dark);
}

/// REQ: WIT:ilium:host/ui-host.theme
#[test]
fn wit_ui_host_theme_colours_use_only_the_low_24_bits() {
    let mut rig = seeded(&[]);
    let theme = rig.host.ui_host().theme();
    for colour in [theme.foreground, theme.background, theme.accent, theme.muted, theme.success, theme.warning, theme.danger] {
        assert_eq!(colour >> 24, 0);
    }
}

/// REQ: WIT:ilium:host/ui-host.theme
#[test]
fn wit_ui_host_theme_stable_glyphs_follow_the_icon_mode() {
    let mut rig = seeded(&[]);
    rig.kernel.set_icon_mode(IconMode::Stable);
    assert!(rig.host.ui_host().theme().stable_glyphs);
    rig.kernel.set_icon_mode(IconMode::Normal);
    assert!(!rig.host.ui_host().theme().stable_glyphs);
}

/// REQ: WIT:ilium:host/ui-host.theme-extras
#[test]
fn wit_ui_host_theme_extras_reports_density_and_reduced_motion() {
    let mut rig = seeded(&[]);
    rig.kernel.set_density(Density::Compact);
    rig.kernel.set_reduced_motion(true);
    let extras = rig.host.ui_host().theme_extras();
    assert_eq!(extras.density, Density::Compact);
    assert!(extras.reduced_motion);
}

/// REQ: WIT:ilium:host/ui-host.theme-extras
#[test]
fn wit_ui_host_theme_extras_icon_mode_is_normal_by_default() {
    let mut rig = seeded(&[]);
    assert_eq!(rig.host.ui_host().theme_extras().icon_mode, IconMode::Normal);
}

/// REQ: WIT:ilium:host/ui-host.write-surface
#[test]
fn wit_ui_host_write_surface_writes_whole_rows() {
    let mut rig = seeded(&[]);
    let row = 4 * 3;
    rig.host.ui_host().write_surface(panel_view(), "canvas".to_owned(), 1, PixelFormat::Rgb8, vec![9; row * 2]).expect("writes");
    assert_eq!(rig.kernel.surface_rows("notes", "canvas", 1, 2), vec![9; row * 2]);
}

/// REQ: WIT:ilium:host/ui-host.write-surface
#[test]
fn wit_ui_host_write_surface_partial_row_invalid_argument() {
    let mut rig = seeded(&[]);
    let result = rig.host.ui_host().write_surface(panel_view(), "canvas".to_owned(), 0, PixelFormat::Rgb8, vec![0; 4 * 3 + 1]);
    assert_eq!(code_of(result), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/ui-host.write-surface
#[test]
fn wit_ui_host_write_surface_format_differs_invalid_argument() {
    let mut rig = seeded(&[]);
    let result = rig.host.ui_host().write_surface(panel_view(), "canvas".to_owned(), 0, PixelFormat::Gray8, vec![0; 4]);
    assert_eq!(code_of(result), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/ui-host.write-surface
#[test]
fn wit_ui_host_write_surface_past_the_surface_height_invalid_argument() {
    let mut rig = seeded(&[]);
    let result = rig.host.ui_host().write_surface(panel_view(), "canvas".to_owned(), 3, PixelFormat::Rgb8, vec![0; 4 * 3 * 2]);
    assert_eq!(code_of(result), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/ui-host.write-surface
#[test]
fn wit_ui_host_write_surface_over_sixteen_mib_invalid_argument() {
    let mut rig = seeded(&[]);
    let result = rig.host.ui_host().write_surface(panel_view(), "canvas".to_owned(), 0, PixelFormat::Rgb8, vec![0; 16 * MIB + 3]);
    assert_eq!(code_of(result), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/ui-host.write-surface
#[test]
fn wit_ui_host_write_surface_unknown_key_not_found() {
    let mut rig = seeded(&[]);
    let result = rig.host.ui_host().write_surface(panel_view(), "ghost".to_owned(), 0, PixelFormat::Rgb8, vec![0; 12]);
    assert_eq!(code_of(result), ErrorCode::NotFound);
}
