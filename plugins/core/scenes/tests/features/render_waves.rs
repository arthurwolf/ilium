//! The Waves sample scene (page sample 1, F-096).
use crate::common::*;

/// REQ: F-096
#[test]
fn waves_096_draws_something_visible() {
    let screen = fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 0));
    assert!(!lit_cells(&screen).is_empty());
}

/// REQ: F-096
#[test]
fn waves_096_changes_over_time() {
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 0));
    let later = scene.frame(braille_frame(40, 10, 1000));
    assert!(equal_share(&first, &later) < 1.0);
}

/// REQ: F-096
#[test]
fn waves_096_boundary_one_millisecond_apart_may_be_equal_but_one_second_differs() {
    let mut scene = fake_scene("waves", 80, 24);
    let a = scene.frame(braille_frame(80, 24, 0));
    let b = scene.frame(braille_frame(80, 24, 1));
    let c = scene.frame(braille_frame(80, 24, 1000));
    assert!(equal_share(&a, &b) >= equal_share(&a, &c));
}

/// REQ: F-096
#[test]
fn waves_096_pattern_is_periodic_with_the_sine_period() {
    // tone = sin(0.2 c + 0.1 r - 2 s): the period in s is pi, so 3142 ms repeats the frame and 1571 ms is the half period.
    let mut scene = fake_scene("waves", 40, 10);
    let start = scene.frame(braille_frame(40, 10, 0));
    let half = scene.frame(braille_frame(40, 10, 1571));
    let full = scene.frame(braille_frame(40, 10, 3142));
    assert!(equal_share(&start, &full) > 0.9);
    assert!(equal_share(&start, &half) < 0.5);
    assert!(equal_share(&start, &full) > equal_share(&start, &half));
}

/// REQ: F-096
#[test]
fn waves_096_golden_at_three_widths() {
    for (columns, rows) in [(40u16, 10u16), (80, 24), (120, 30)] {
        let screen = fake_scene("waves", columns, rows).frame(braille_frame(columns, rows, 0));
        ilium_plugin_sdk::testing::assert_golden(
            &format!("tests/features/golden/waves_{columns}x{rows}.txt"),
            &screen.text(),
        );
    }
}
