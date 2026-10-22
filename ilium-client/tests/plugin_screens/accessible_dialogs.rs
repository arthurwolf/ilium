//! Host-owned dialogs stay readable without colour (USR-261): risk as text, full fingerprint at 80 columns.

use crate::support::*;

/// REQ: USR-261
#[test]
fn usr_261_a_high_risk_row_shows_the_word_high_and_an_unchecked_box() {
    let mut rig = ScreensRig::new(80, 30);
    rig.open_review_dialog("com.example.wild", &["terminal.write"]);
    let row = row_with(&rig.screen_text(), "terminal.write");
    assert!(row.contains("HIGH") && row.contains("[ ]"), "{row}");
}

/// REQ: USR-261
#[test]
fn usr_261_a_full_fingerprint_wraps_without_truncation_at_80_columns() {
    let mut rig = ScreensRig::new(80, 30);
    let fingerprint = "ab".repeat(32);
    rig.open_trust_prompt("Example Publisher", &fingerprint);
    let screen = rig.screen_text();
    assert!(
        screen.lines().all(|line| line.chars().count() <= 80),
        "{screen}"
    );
    let squashed: String = screen
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();
    assert!(squashed.contains(&fingerprint), "{screen}");
}
