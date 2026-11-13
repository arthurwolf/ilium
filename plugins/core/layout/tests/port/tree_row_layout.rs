//! Twins of the model tests of `ilium-client/src/tree_row_layout.rs`. The rendering test of the
//! kernel (`composition_keeps_graphemes_and_styles_in_bounds`) stays with the kernel drawing code.

use ilium_plugin_core_layout::tree_layout_rows::{TreeRowElement, TreeRowLayout};

/// REQ: PORT:ilium-client/src/tree_row_layout.rs::normalizes_partial_duplicate_and_hidden_title
#[test]
fn normalizes_partial_duplicate_and_hidden_title() {
    let layout = TreeRowLayout {
        order: vec![TreeRowElement::Title, TreeRowElement::Title],
        hidden: vec![TreeRowElement::Title, TreeRowElement::Activity],
    }
    .normalized();
    assert_eq!(layout.order().len(), 4);
    assert!(layout.is_visible(TreeRowElement::Title));
    assert!(!layout.is_visible(TreeRowElement::Activity));
}

/// REQ: PORT:ilium-client/src/tree_row_layout.rs::right_slots_anchor_and_title_retains_space
#[test]
fn right_slots_anchor_and_title_retains_space() {
    let layout = TreeRowLayout::all_right();
    assert_eq!(
        layout.placements(3, 3, 2, 20),
        vec![
            (TreeRowElement::Title, 0, 12),
            (TreeRowElement::Identity, 12, 3),
            (TreeRowElement::Objective, 15, 3),
            (TreeRowElement::Activity, 18, 2)
        ]
    );
    assert_eq!(
        layout.placements(3, 3, 2, 2)[0],
        (TreeRowElement::Title, 0, 2)
    );
}

/// REQ: PORT:ilium-client/src/tree_row_layout.rs::serde_round_trip_preserves_order_and_visibility
#[test]
fn serde_round_trip_preserves_order_and_visibility() {
    let layout = TreeRowLayout {
        hidden: vec![TreeRowElement::Objective],
        ..TreeRowLayout::mixed()
    };
    let encoded = serde_json::to_string(&layout).unwrap();
    assert_eq!(
        serde_json::from_str::<TreeRowLayout>(&encoded).unwrap(),
        layout
    );
}

#[test]
fn placements_never_exceed_the_width_and_every_visible_slot_is_placed() {
    for width in 0..40usize {
        for layout in [
            TreeRowLayout::default(),
            TreeRowLayout::all_right(),
            TreeRowLayout::mixed(),
        ] {
            let placements = layout.placements(3, 3, 2, width);
            let end = placements
                .iter()
                .map(|(_, start, size)| start + size)
                .max()
                .unwrap_or(0);
            assert!(end <= width, "width {width}: ends at {end}");
            assert_eq!(placements.len(), 4);
        }
    }
}
