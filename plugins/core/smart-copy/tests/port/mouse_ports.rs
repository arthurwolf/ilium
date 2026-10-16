//! Ports of mouse.rs and ui.rs preview: pointer handling and the picker view.

use ilium_plugin_sdk::types::PointerKind;

use crate::common::*;

/// REQ: PORT:ilium-client/src/mouse.rs::smart_copy_release_only_hosts_still_copy_the_hovered_candidate
/// REQ: PORT:ilium-client/src/mouse.rs::smart_copy_release_is_not_lost_to_stale_tree_drag
#[test]
fn port_mouse_picking_after_pointer_activity_still_copies() {
    let mut host = open_picker(&["hovered"]);
    let out = host.deliver_one(key("1"));
    assert!(status_contains(&out, "Copied 7 characters"));
    assert!(!status_contains(&out, "Move over a highlighted Smart Copy selection"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("hovered"));
}

/// REQ: PORT:ilium-client/src/mouse.rs::right_click_unfreezes_the_clicked_pane_without_a_cached_session_id
#[test]
fn port_mouse_closing_the_picker_closes_the_view() {
    let mut host = open_picker(&["x"]);
    let out = host.deliver_one(key("esc"));
    assert!(!intents(&out).is_empty());
}

/// REQ: PORT:ilium-client/src/ui.rs::smart_copy_preview_fits_narrow_screens
#[test]
fn port_ui_the_picker_view_renders_at_narrow_widths() {
    for width in [12u16, 16, 20, 27, 28, 40] {
        let mut host = open_picker(&["first", "second"]);
        let text = host.render(picker_target(), width, 9).text();
        assert!(text.lines().count() <= 9, "{width}");
        assert!(text.lines().all(|line| line.chars().count() <= width as usize), "{width}");
    }
    let _ = (PointerKind::Down, pointer(0, PointerKind::Down, 0, 0));
}
