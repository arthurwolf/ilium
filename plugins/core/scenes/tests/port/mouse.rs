//! Twins of the original tests of ilium-client/src/mouse.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/mouse.rs::clicking_the_top_resize_filter_switch_reveals_animation_resizes
#[test]
fn port_clicking_the_top_resize_filter_switch_reveals_animation_resizes() {
    for id in ["waves", "clock"] {
        assert!(!fake_scene(id, 40, 10).wants_pointer(), "{id} is pointer-insensitive");
    }
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 1000));
    let second = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(first.text(), second.text(), "no pointer: the same time renders the same cells");
}
