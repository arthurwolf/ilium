//! Twin of the keyboard test of `ilium-client/src/tree_layout_settings.rs` against the plugin's
//! pure editor state. The kernel dialog tests (cancel restores the layout; the rendering at three
//! widths) need the ratatui `App` and stay with the kernel code until popup views can edit text.

use ilium_plugin_core_layout::tree_layout_rows::{
    EditorKey, EditorState, TreeRowElement, TreeRowLayout,
};

/// REQ: PORT:ilium-client/src/tree_layout_settings.rs::keyboard_moves_title_across_icons_and_protects_visibility
#[test]
fn keyboard_moves_title_across_icons_and_protects_visibility() {
    let mut editor = EditorState::default();
    let mut layout = TreeRowLayout::default();
    assert!(!editor.apply(&mut layout, EditorKey::Enter));
    assert!(editor.apply(&mut layout, EditorKey::Down { shift: false }));
    assert_eq!(layout.order()[1], TreeRowElement::Identity);
    editor.selected = 3;
    assert!(!editor.apply(&mut layout, EditorKey::Space));
    assert!(layout.is_visible(TreeRowElement::Title));
    editor.apply(&mut layout, EditorKey::Preset(2));
    assert_eq!(layout.order()[0], TreeRowElement::Title);
}

#[test]
fn navigation_alone_never_changes_the_layout() {
    let mut editor = EditorState::default();
    let mut layout = TreeRowLayout::default();
    assert!(!editor.apply(&mut layout, EditorKey::Down { shift: false }));
    assert!(!editor.apply(&mut layout, EditorKey::Up { shift: false }));
    assert!(layout.is_default());
}

#[test]
fn shift_moves_the_selected_slot_and_space_hides_an_icon() {
    let mut editor = EditorState::default();
    let mut layout = TreeRowLayout::default();
    assert!(editor.apply(&mut layout, EditorKey::Down { shift: true }));
    assert_eq!(layout.order()[0], TreeRowElement::Objective);
    editor.selected = 0;
    assert!(editor.apply(&mut layout, EditorKey::Space));
    assert!(!layout.is_visible(TreeRowElement::Objective));
    assert!(editor.apply(&mut layout, EditorKey::Space));
    assert!(layout.is_visible(TreeRowElement::Objective));
    assert!(editor.apply(&mut layout, EditorKey::Preset(3)));
    assert!(editor.apply(&mut layout, EditorKey::Reset));
    assert!(layout.is_default());
}
