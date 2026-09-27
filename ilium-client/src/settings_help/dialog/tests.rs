use super::*;
use ratatui::layout::{Position, Rect};

#[test]
fn wide_dialog_gives_the_explanation_and_illustration_separate_columns() {
    let layout = layout(Rect::new(0, 0, 120, 48));

    assert!(!layout.stacked);
    assert!(layout.explanation.width >= 40);
    assert!(layout.illustration.width >= 40);
    assert!(layout.explanation.x < layout.illustration.x);
    assert!(layout.explanation.right() <= layout.illustration.x);
    assert!(layout.footer.y >= layout.explanation.bottom());
    assert!(layout.footer.y >= layout.illustration.bottom());
}

#[test]
fn narrow_dialog_stacks_its_panels_without_clipping_the_footer_area() {
    let layout = layout(Rect::new(0, 0, 80, 24));

    assert!(layout.stacked);
    assert_eq!(layout.explanation.x, layout.illustration.x);
    assert_eq!(layout.explanation.width, layout.illustration.width);
    assert!(layout.explanation.y < layout.illustration.y);
    assert!(layout.footer.y >= layout.illustration.bottom());
}

#[test]
fn dialog_layout_stays_inside_tiny_terminal_bounds() {
    let area = Rect::new(0, 0, 32, 8);
    let layout = layout(area);

    assert!(area.contains(Position::new(layout.popup.x, layout.popup.y)));
    assert!(layout.popup.right() <= area.right());
    assert!(layout.popup.bottom() <= area.bottom());
    assert!(layout.footer.right() <= area.right());
    assert!(layout.footer.bottom() <= area.bottom());
}
