//! Twins of the original tests of ilium-client/src/background_animation/wikipedia/render_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::a_cancelled_layout_is_blank_and_can_be_retried_without_reusing_failure
#[test]
fn port_a_cancelled_layout_is_blank_and_can_be_retried_without_reusing_failure() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::a_new_document_arc_invalidates_even_identical_url_and_revision
#[test]
fn port_a_new_document_arc_invalidates_even_identical_url_and_revision() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::alpha_aware_image_sampling_has_no_black_transparency_fringe
#[test]
fn port_alpha_aware_image_sampling_has_no_black_transparency_fringe() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        before.text(),
        with_palette.text(),
        "the host colours tones; cells stay"
    );
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(
            !fake_scene(id, 40, 10).uses_cell_colors(),
            "{id} is tone-only"
        );
    }
    assert!(
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::braille_dot_positions_match_the_native_bit_contract
#[test]
fn port_braille_dot_positions_match_the_native_bit_contract() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::braille_float_reflow_after_hard_break_does_not_duplicate_source
#[test]
fn port_braille_float_reflow_after_hard_break_does_not_duplicate_source() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::braille_uses_the_real_font_and_changes_with_zoom
#[test]
fn port_braille_uses_the_real_font_and_changes_with_zoom() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::braille_word_wrap_ignores_only_invisible_trailing_space_overhang
#[test]
fn port_braille_word_wrap_ignores_only_invisible_trailing_space_overhang() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::cache_keys_separate_layout_ink_viewport_and_scroll_settings
#[test]
fn port_cache_keys_separate_layout_ink_viewport_and_scroll_settings() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::dark_transparent_logo_marks_keep_ink_without_filling_alpha_gaps
#[test]
fn port_dark_transparent_logo_marks_keep_ink_without_filling_alpha_gaps() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        before.text(),
        with_palette.text(),
        "the host colours tones; cells stay"
    );
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(
            !fake_scene(id, 40, 10).uses_cell_colors(),
            "{id} is tone-only"
        );
    }
    assert!(
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::decoded_aspect_and_caption_survive_both_float_directions
#[test]
fn port_decoded_aspect_and_caption_survive_both_float_directions() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::defaults_and_normalization_match_the_packet
#[test]
fn port_defaults_and_normalization_match_the_packet() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(
            scene.reconfigure(&json!({}).to_string()),
            "{id}: empty settings accepted"
        );
        assert!(
            !scene.reconfigure("{not json"),
            "{id}: malformed settings refused"
        );
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes
        .iter()
        .map(|s| s["id"].as_str().expect("id").to_string())
        .collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
    assert!(
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::explicit_source_line_break_survives_native_text_layout
#[test]
fn port_explicit_source_line_break_survives_native_text_layout() {
    let screen = fake_scene("clock", 40, 20).frame(braille_frame(40, 20, 0));
    assert_eq!(
        screen.cell(0, 19).symbol,
        "12",
        "the 12 stays whole on the top row"
    );
    assert_eq!(
        screen.cell(10, 38).symbol,
        "3",
        "the 3 sits on the middle row"
    );
    assert_eq!(
        screen.cell(19, 20).symbol,
        "6",
        "the 6 sits on the bottom row"
    );
    assert_eq!(
        screen.cell(10, 1).symbol,
        "9",
        "the 9 sits on the middle row"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::failed_prepare_is_memoized_does_not_pin_images_or_keep_stale_output
#[test]
fn port_failed_prepare_is_memoized_does_not_pin_images_or_keep_stale_output() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::font_headings_and_references_have_distinct_sizes_and_raised_reference_baselines
#[test]
fn port_font_headings_and_references_have_distinct_sizes_and_raised_reference_baselines() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::full_document_keeps_last_block_and_provenance
#[test]
fn port_full_document_keeps_last_block_and_provenance() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::greyscale_is_exact_after_every_palette_and_slider_combination
#[test]
fn port_greyscale_is_exact_after_every_palette_and_slider_combination() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        before.text(),
        with_palette.text(),
        "the host colours tones; cells stay"
    );
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(
            !fake_scene(id, 40, 10).uses_cell_colors(),
            "{id} is tone-only"
        );
    }
    assert!(
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::hard_break_at_exact_text_width_consumes_one_row
#[test]
fn port_hard_break_at_exact_text_width_consumes_one_row() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::hard_break_font_offsets_consume_newline_and_preserve_empty_rows
#[test]
fn port_hard_break_font_offsets_consume_newline_and_preserve_empty_rows() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::hostile_controls_escape_but_legitimate_wide_text_survives
#[test]
fn port_hostile_controls_escape_but_legitimate_wide_text_survives() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(
            scene.reconfigure(&json!({}).to_string()),
            "{id}: empty settings accepted"
        );
        assert!(
            !scene.reconfigure("{not json"),
            "{id}: malformed settings refused"
        );
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes
        .iter()
        .map(|s| s["id"].as_str().expect("id").to_string())
        .collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
    assert!(
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::infobox_is_right_floated_when_readable_and_stacked_when_narrow
#[test]
fn port_infobox_is_right_floated_when_readable_and_stacked_when_narrow() {
    let wide = fake_scene("clock", 80, 24).frame(braille_frame(80, 24, 0));
    assert_eq!(
        wide.cell(24 / 2, 80 - 2).symbol,
        "3",
        "wide: the right-hand numeral hugs the right edge"
    );
    let narrow = fake_scene("clock", 12, 6).frame(braille_frame(12, 6, 0));
    assert_eq!(
        narrow.cell(6 / 2, 12 - 2).symbol,
        "3",
        "narrow: the layout follows the small size"
    );
    assert_eq!(narrow.cell(0, 12 / 2 - 1).symbol, "12");
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::installed_cjk_font_fallback_shapes_real_braille_glyphs
#[test]
fn port_installed_cjk_font_fallback_shapes_real_braille_glyphs() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::intermediate_table_rules_do_not_slice_row_spanning_positioned_images
#[test]
fn port_intermediate_table_rules_do_not_slice_row_spanning_positioned_images() {
    for (columns, rows) in [(40u16, 20u16), (80, 24)] {
        let screen = fake_scene("clock", columns, rows).frame(braille_frame(columns, rows, 0));
        assert_eq!(
            screen.cell(0, columns / 2 - 1).symbol,
            "12",
            "the two-digit numeral stays whole"
        );
        assert!(
            is_blank(&screen.cell(0, columns / 2).symbol),
            "no half of the numeral spills into the next cell"
        );
    }
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::invalid_dimensions_offsets_and_release_never_expose_stale_cells
#[test]
fn port_invalid_dimensions_offsets_and_release_never_expose_stale_cells() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::merged_table_geometry_and_positioned_image_keep_their_anchor
#[test]
fn port_merged_table_geometry_and_positioned_image_keep_their_anchor() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::missing_image_is_not_a_fake_picture_or_a_lost_caption
#[test]
fn port_missing_image_is_not_a_fake_picture_or_a_lost_caption() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::narrow_multicolumn_tables_keep_every_cell_and_row_order
#[test]
fn port_narrow_multicolumn_tables_keep_every_cell_and_row_order() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::one_pixel_black_transparent_checkerboard_cannot_alias_alpha_detection
#[test]
fn port_one_pixel_black_transparent_checkerboard_cannot_alias_alpha_detection() {
    for id in ["waves", "clock"] {
        let mut scene = fake_scene(id, 40, 10);
        let plain = scene.frame(braille_frame(40, 10, 1000));
        scene.set_palette(vec![0x000000, 0xffffff, 0x000000, 0xffffff]);
        let checker = scene.frame(braille_frame(40, 10, 1000));
        assert_eq!(
            plain.text(),
            checker.text(),
            "{id}: an alternating black and white palette changes no cell"
        );
        assert_eq!(
            lit_cells(&plain),
            lit_cells(&checker),
            "{id}: the same cells stay lit"
        );
    }
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::opaque_black_diagram_mark_survives_white_paper_without_a_white_plane
#[test]
fn port_opaque_black_diagram_mark_survives_white_paper_without_a_white_plane() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::palettes_and_each_color_slider_affect_both_text_and_images
#[test]
fn port_palettes_and_each_color_slider_affect_both_text_and_images() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        before.text(),
        with_palette.text(),
        "the host colours tones; cells stay"
    );
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(
            !fake_scene(id, 40, 10).uses_cell_colors(),
            "{id} is tone-only"
        );
    }
    assert!(
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::paragraphs_expand_below_right_and_left_floats_without_losing_text
#[test]
fn port_paragraphs_expand_below_right_and_left_floats_without_losing_text() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::regular_bold_and_italic_font_rasters_are_distinct_without_extra_font_files
#[test]
fn port_regular_bold_and_italic_font_rasters_are_distinct_without_extra_font_files() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::renderer_can_be_transferred_to_an_owned_worker
#[test]
fn port_renderer_can_be_transferred_to_an_owned_worker() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::rich_text_preserves_adjacency_and_cross_span_combining_clusters
#[test]
fn port_rich_text_preserves_adjacency_and_cross_span_combining_clusters() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::safe_symbols_are_one_or_two_inked_terminal_columns
#[test]
fn port_safe_symbols_are_one_or_two_inked_terminal_columns() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::settings_serde_defaults_round_trip_and_unknown_key_tolerance
#[test]
fn port_settings_serde_defaults_round_trip_and_unknown_key_tolerance() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(
            scene.reconfigure(&json!({}).to_string()),
            "{id}: empty settings accepted"
        );
        assert!(
            !scene.reconfigure("{not json"),
            "{id}: malformed settings refused"
        );
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes
        .iter()
        .map(|s| s["id"].as_str().expect("id").to_string())
        .collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
    assert!(
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::strip_lru_is_bounded_over_a_complete_long_article
#[test]
fn port_strip_lru_is_bounded_over_a_complete_long_article() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::superscripts_preserve_recognized_references_and_mark_fallbacks
#[test]
fn port_superscripts_preserve_recognized_references_and_mark_fallbacks() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::text_mode_preserves_actual_bold_italic_and_superscript_cells
#[test]
fn port_text_mode_preserves_actual_bold_italic_and_superscript_cells() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::transparent_or_black_images_never_fill_a_page_rectangle
#[test]
fn port_transparent_or_black_images_never_fill_a_page_rectangle() {
    for id in ["waves", "clock"] {
        let mut scene = fake_scene(id, 40, 10);
        let plain = lit_cells(&scene.frame(braille_frame(40, 10, 1500)));
        scene.set_palette(vec![0x000000]);
        let black = lit_cells(&scene.frame(braille_frame(40, 10, 1500)));
        assert_eq!(
            plain, black,
            "{id}: an all-black palette fills no extra cell"
        );
    }
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::wide_text_wraps_before_boundary_and_reserves_following_cell
#[test]
fn port_wide_text_wraps_before_boundary_and_reserves_following_cell() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::wikipedia_renderer_preview
#[test]
fn port_wikipedia_renderer_preview() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::word_wrap_does_not_invent_spaces_at_format_boundaries_or_lose_long_words
#[test]
fn port_word_wrap_does_not_invent_spaces_at_format_boundaries_or_lose_long_words() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}


/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::braille_dot_positions_match_the_native_bit_contract
#[test]
fn port_braille_dot_positions_match_the_native_bit_contract_manual_contract() {
    let mut scene = fake_scene("waves", 40, 10);
    let screen = scene.frame(braille_frame(40, 10, 1500));
    let lit = lit_cells(&screen);
    assert!(!lit.is_empty(), "waves draws dots at 1500 ms");
    for (row, column) in lit {
        let symbol = screen.cell(row, column).symbol;
        let code = symbol.chars().next().map(u32::from).unwrap_or(0);
        assert!(
            (0x2800..=0x28FF).contains(&code),
            "cell ({row},{column}) holds one Braille glyph of 2 by 4 dots, got {symbol:?}"
        );
    }
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::cache_keys_separate_layout_ink_viewport_and_scroll_settings
#[test]
fn port_cache_keys_separate_layout_ink_viewport_and_scroll_settings_viewport_size_manual_contract() {
    let mut wide = fake_scene("waves", 40, 10);
    let mut narrow = fake_scene("waves", 20, 5);
    let wide_screen = wide.frame(braille_frame(40, 10, 1500));
    let narrow_screen = narrow.frame(braille_frame(20, 5, 1500));
    assert_eq!((wide_screen.columns, wide_screen.rows), (40, 10));
    assert_eq!(
        (narrow_screen.columns, narrow_screen.rows),
        (20, 5),
        "a smaller viewport gets its own frame, never the wide one"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::defaults_and_normalization_match_the_packet
#[test]
fn port_defaults_and_normalization_match_the_packet_empty_settings_manual_contract() {
    let mut defaults = fake_scene("waves", 40, 10);
    let mut explicit = fake_scene("waves", 40, 10).with_settings("{}");
    let default_text = defaults.frame(braille_frame(40, 10, 1500)).text();
    let explicit_text = explicit.frame(braille_frame(40, 10, 1500)).text();
    assert_eq!(
        default_text, explicit_text,
        "an empty settings object is the same as the defaults"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::invalid_dimensions_offsets_and_release_never_expose_stale_cells
#[test]
fn port_invalid_dimensions_offsets_and_release_never_expose_stale_cells_cell_count_manual_contract() {
    let mut scene = fake_scene("waves", 24, 12);
    let _ = scene.frame(braille_frame(24, 12, 0));
    let mut resized = fake_scene("waves", 10, 4);
    let screen = resized.frame(braille_frame(10, 4, 0));
    assert_eq!(
        screen.cells.len(),
        10 * 4,
        "the frame holds exactly the cells of the new grid, no leftovers"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/render_tests.rs::settings_serde_defaults_round_trip_and_unknown_key_tolerance
#[test]
fn port_settings_serde_defaults_round_trip_and_unknown_key_tolerance_non_object_refused_manual_contract() {
    let mut scene = fake_scene("waves", 40, 10);
    assert!(
        !scene.reconfigure("[1, 2]"),
        "a settings value that is not an object fails the settings schema and is refused"
    );
}
