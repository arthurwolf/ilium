//! Actual offline OSM scene through the client host and Braille packer.
use super::super::{AnimationFrame, AnimationKind, AnimationSettings};
use std::time::{Duration, Instant};

#[test]
fn openstreetmap_attribution_reaches_client_glyphs_and_leaves_with_the_scene() {
    let mut settings = AnimationSettings {
        kind: AnimationKind::OpenStreetMap,
        ..Default::default()
    };
    let mut frame = AnimationFrame::default();
    frame.render(&settings, 20, 10, Duration::ZERO);
    let text: String = (8..10)
        .flat_map(|y| (0..20).map(move |x| (x, y)))
        .map(|(x, y)| frame.glyph(x, y))
        .collect();
    assert!(
        text.contains("(c) OpenStreetMap contributors / ODbL"),
        "{text}"
    );
    settings.kind = AnimationKind::QuietPond;
    frame.render(&settings, 20, 10, Duration::ZERO);
    assert_ne!(frame.glyph(0, 8), '(');
}

#[test]
fn openstreetmap_shared_palette_and_density_reuse_map_and_zero_brightness_clears_dots() {
    let mut settings = AnimationSettings {
        kind: AnimationKind::OpenStreetMap,
        density_percent: 100,
        ..Default::default()
    };
    settings.ambient.openstreetmap.camera = 0;
    let mut frame = AnimationFrame::default();
    let began = Instant::now();
    let deadline = began + Duration::from_secs(15);
    loop {
        frame.render(&settings, 80, 24, began.elapsed());
        if frame
            .status()
            .is_some_and(|status| status.contains("Paris ·"))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "offline OSM load: {:?}",
            frame.status()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let elapsed = began.elapsed();
    frame.render(&settings, 80, 24, elapsed);
    let original = frame.packed_cells().to_vec();
    assert!(original.iter().any(|bits| *bits != 0));
    assert!(!frame.has_cell_colors(), "OSM uses the shared palette");
    let original_color = settings.foreground_rgb();
    settings.hue_degrees = 120;
    settings.saturation_percent = 100;
    settings.lightness_percent = 50;
    frame.render(&settings, 80, 24, elapsed);
    assert_ne!(original_color, settings.foreground_rgb());
    assert_eq!(original, frame.packed_cells());
    settings.density_percent = 25;
    frame.render(&settings, 80, 24, elapsed);
    assert_ne!(original, frame.packed_cells());
    settings.ambient.openstreetmap.brightness_percent = 0;
    frame.render(&settings, 80, 24, elapsed);
    assert!(frame.packed_cells().iter().all(|bits| *bits == 0));
    assert!(frame.status().unwrap().contains("Paris ·"));
}
