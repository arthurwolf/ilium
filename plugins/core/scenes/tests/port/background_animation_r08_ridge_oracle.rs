//! Twins of the original tests of ilium-client/src/background_animation/r08_ridge_oracle.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_oracle.rs::r08_ridge_key_and_owner_lifecycle
#[test]
fn port_r08_ridge_key_and_owner_lifecycle() {
    let mut scene = fake_scene("waves", 24, 12);
    let small = scene.frame(braille_frame(24, 12, 0));
    assert_eq!((small.columns, small.rows), (24, 12));
    assert!(
        scene.reconfigure("{}"),
        "an empty settings object is accepted in place"
    );
    let mut resized = fake_scene("waves", 10, 4);
    let screen = resized.frame(braille_frame(10, 4, 0));
    assert_eq!(
        (screen.columns, screen.rows),
        (10, 4),
        "a new generation uses its own size, never a stale one"
    );
    for id in MANIFEST_SCENES {
        let fps = u64::from(fake_scene(id, 40, 10).frames_per_second());
        let cap = scene_entry(id)["frame_rate_cap"].as_u64().unwrap();
        assert!(fps >= 1 && fps <= cap, "{id}: {fps} within cap {cap}");
    }
    assert!(!fake_scene("clock", 40, 10).wants_occupancy());
    assert!(
        scenes_page().to_lowercase().contains("ridge"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_oracle.rs::r08_ridge_oracle_rejects_noise_mutations
#[test]
fn port_r08_ridge_oracle_rejects_noise_mutations() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(
            first.text(),
            second.text(),
            "{id}: identical input must give an identical frame"
        );
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(
        scenes_page().to_lowercase().contains("ridge"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_oracle.rs::r08_ridge_public_frames_pack_and_normalize
#[test]
fn port_r08_ridge_public_frames_pack_and_normalize() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        let before = scene.frame(braille_frame(40, 10, 1500));
        assert!(
            scene.reconfigure(&json!({}).to_string()),
            "{id}: empty settings accepted"
        );
        assert!(
            !scene.reconfigure("{not json"),
            "{id}: malformed settings refused"
        );
        let after = scene.frame(braille_frame(40, 10, 1500));
        assert_eq!(
            before.text(),
            after.text(),
            "{id}: normalised settings keep the frame"
        );
        for row in 0..after.rows {
            for column in 0..after.columns {
                let symbol = after.cell(row, column).symbol;
                assert!(symbol.chars().all(|ch| ch == ' ' || ch.is_ascii_digit() || ('\u{2800}'..='\u{28FF}').contains(&ch)), "cell ({row},{column}) {symbol:?} is blank, a numeral of the glyph plane or a Braille pattern");
            }
        }
    }
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_oracle.rs::r08_ridge_scalar_and_retained_payload_matrix
#[test]
fn port_r08_ridge_scalar_and_retained_payload_matrix() {
    for id in MANIFEST_SCENES {
        let mut retained = fake_scene(id, 40, 10);
        for time in [0u64, 500, 1500, 500] {
            let scalar = fake_scene(id, 40, 10).frame(braille_frame(40, 10, time));
            let kept = retained.frame(braille_frame(40, 10, time));
            assert_eq!(
                scalar.text(),
                kept.text(),
                "{id} at {time} ms: retained state equals a scalar fresh render"
            );
        }
    }
}
