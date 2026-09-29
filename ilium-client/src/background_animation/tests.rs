use super::*;
use std::collections::HashSet;

fn snapshot(kind: AnimationKind, seconds: u64, density: u16) -> Vec<char> {
    let mut frame = AnimationFrame::default();
    frame.render(
        &AnimationSettings {
            kind,
            density_percent: density,
            ..AnimationSettings::default()
        },
        64,
        24,
        Duration::from_secs(seconds),
    );
    (0..24)
        .flat_map(|y| (0..64).map(move |x| (x, y)))
        .map(|(x, y)| frame.glyph(x, y))
        .collect()
}

#[test]
fn catalog_has_ten_unique_serializable_scenes_in_user_order() {
    let expected = [
        "shoreline",
        "moonlit_water",
        "sleeping_ridge",
        "windy_hillside",
        "tea_steam",
        "kelp",
        "stone_caustics",
        "cloudlets",
        "two_ripples",
        "breathing_mountain",
    ];
    for (kind, id) in AnimationKind::ALL.into_iter().zip(expected) {
        assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{id}\""));
        assert_eq!(
            serde_json::from_str::<AnimationKind>(&format!("\"{id}\"")).unwrap(),
            kind
        );
        assert!(!kind.label().is_empty());
        assert!(!kind.description().is_empty());
    }
    assert_eq!(
        AnimationKind::ALL
            .into_iter()
            .map(AnimationKind::label)
            .collect::<HashSet<_>>()
            .len(),
        10
    );
}

#[test]
fn settings_defaults_and_untrusted_ranges_are_normalized() {
    let settings: AnimationSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings, AnimationSettings::default());
    assert!(!settings.enabled);
    assert_eq!(settings.kind, AnimationKind::Shoreline);
    assert_eq!(settings.speed_percent, 100);
    assert_eq!(settings.density_percent, 60);
    assert_eq!(settings.dither, DitherMode::Ordered);
    let normalized = AnimationSettings {
        speed_percent: 0,
        density_percent: u16::MAX,
        ..settings
    }
    .normalized();
    assert_eq!(normalized.speed_percent, 25);
    assert_eq!(normalized.density_percent, 100);
}

#[test]
fn every_scene_is_deterministic_nonempty_and_distinct() {
    let mut unique = HashSet::new();
    for kind in AnimationKind::ALL {
        let first = snapshot(kind, 7, 100);
        assert_eq!(first, snapshot(kind, 7, 100), "{kind:?}");
        assert!(first.iter().any(|c| *c != ' '), "{kind:?} is empty");
        assert!(first
            .iter()
            .all(|c| *c == ' ' || ('\u{2801}'..='\u{28ff}').contains(c)));
        assert!(unique.insert(first), "{kind:?} duplicates another scene");
    }
}

#[test]
fn every_scene_changes_over_multiple_seconds() {
    for kind in AnimationKind::ALL {
        let initial = snapshot(kind, 0, 100);
        assert!(
            [3, 7, 15]
                .into_iter()
                .any(|seconds| snapshot(kind, seconds, 100) != initial),
            "{kind:?} is static"
        );
    }
}

#[test]
fn density_only_removes_dots_for_both_dithers() {
    for kind in AnimationKind::ALL {
        for dither in [DitherMode::Ordered, DitherMode::Stippled] {
            let mut low = AnimationFrame::default();
            let mut high = AnimationFrame::default();
            let settings = AnimationSettings {
                kind,
                dither,
                density_percent: 25,
                ..AnimationSettings::default()
            };
            low.render(&settings, 40, 16, Duration::from_secs(4));
            high.render(
                &AnimationSettings {
                    density_percent: 100,
                    ..settings
                },
                40,
                16,
                Duration::from_secs(4),
            );
            for y in 0..16 {
                for x in 0..40 {
                    let bits = |c: char| if c == ' ' { 0 } else { c as u32 - 0x2800 };
                    let low_bits = bits(low.glyph(x, y));
                    let high_bits = bits(high.glyph(x, y));
                    assert_eq!(low_bits & high_bits, low_bits, "{kind:?} {dither:?}");
                }
            }
        }
    }
}

#[test]
fn geometry_handles_empty_tiny_resize_and_outside_queries() {
    let mut frame = AnimationFrame::default();
    for kind in AnimationKind::ALL {
        for (width, height) in [(0, 0), (0, 4), (5, 0), (1, 1), (2, 1), (80, 25), (1, 2)] {
            frame.render(
                &AnimationSettings {
                    kind,
                    ..AnimationSettings::default()
                },
                width,
                height,
                Duration::from_secs(5),
            );
            assert_eq!((frame.width(), frame.height()), (width, height));
            assert_eq!(frame.glyph(width, height), ' ');
        }
    }
}

#[test]
fn braille_bit_layout_is_exact() {
    let positions = [
        (0, 0, 0),
        (0, 1, 1),
        (0, 2, 2),
        (1, 0, 3),
        (1, 1, 4),
        (1, 2, 5),
        (0, 3, 6),
        (1, 3, 7),
    ];
    for (x, y, bit) in positions {
        let mut frame = AnimationFrame::default();
        frame.resize(1, 1);
        frame.raster.dots[y * 2 + x] = 1.0;
        frame.pack(100, DitherMode::Ordered);
        assert_eq!(frame.glyph(0, 0) as u32, 0x2800 + (1 << bit));
    }
}

#[test]
fn moon_and_cup_stationary_components_do_not_move() {
    for (kind, rectangle) in [
        (AnimationKind::MoonlitWater, (24, 0, 42, 10)),
        (AnimationKind::TeaSteam, (19, 20, 47, 24)),
    ] {
        let a = snapshot(kind, 0, 100);
        let b = snapshot(kind, 13, 100);
        let (left, top, right, bottom) = rectangle;
        for y in top..bottom {
            for x in left..right {
                assert_eq!(
                    a[y * 64 + x],
                    b[y * 64 + x],
                    "{kind:?} static geometry at {x},{y}"
                );
            }
        }
    }
}

#[test]
fn identical_frame_requests_preserve_buffers_and_pixels() {
    let mut frame = AnimationFrame::default();
    let settings = AnimationSettings::default();
    let time = Duration::from_secs(2);
    frame.render(&settings, 64, 24, time);
    let raster_pointer = frame.raster.dots.as_ptr();
    let cells_pointer = frame.cells.as_ptr();
    let cells = frame.cells.clone();
    frame.render(&settings, 64, 24, time);
    assert_eq!(frame.cells, cells);
    assert_eq!(frame.raster.dots.as_ptr(), raster_pointer);
    assert_eq!(frame.cells.as_ptr(), cells_pointer);
    frame.render(&settings, 60, 20, time);
    assert_eq!(frame.raster.dots.as_ptr(), raster_pointer);
    assert_eq!(frame.cells.as_ptr(), cells_pointer);
}

#[test]
fn ripple_crests_survive_destructive_interference_as_continuous_arcs() {
    let mut frame = AnimationFrame::default();
    let settings = AnimationSettings {
        kind: AnimationKind::TwoRipples,
        density_percent: 100,
        ..AnimationSettings::default()
    };
    for seconds in [0, 8] {
        frame.render(&settings, 96, 36, Duration::from_secs(seconds));
        // Follow the first outward-traveling crest around the left source.
        // The former combined-height threshold erased substantial arc sections
        // whenever the right source's wave opposed this crest.
        let radius = (std::f32::consts::FRAC_PI_2 + seconds as f32 * 0.24) / 22.0;
        for angle_index in 0..24 {
            let angle = angle_index as f32 * std::f32::consts::TAU / 24.0;
            let u = 0.24 + radius * angle.cos() / frame.raster.aspect();
            let v = 0.44 + radius * angle.sin();
            let x = (u * frame.raster.width as f32) as usize;
            let y = (v * frame.raster.height as f32) as usize;
            assert!(
                frame.raster.dots[y * frame.raster.width + x] >= 0.40,
                "missing crest arc at {seconds}s, angle {angle_index}"
            );
        }
    }
}

#[test]
fn stone_centers_remain_shaded_when_the_moving_light_leaves() {
    let mut frame = AnimationFrame::default();
    let settings = AnimationSettings {
        kind: AnimationKind::StoneCaustics,
        ..AnimationSettings::default()
    };
    for seconds in [0, 8, 13] {
        frame.render(&settings, 96, 36, Duration::from_secs(seconds));
        for (u, v) in [
            (0.12, 0.25),
            (0.44, 0.22),
            (0.78, 0.26),
            (0.27, 0.61),
            (0.63, 0.59),
        ] {
            let x = (u * frame.raster.width as f32) as usize;
            let y = (v * frame.raster.height as f32) as usize;
            assert!(
                frame.raster.dots[y * frame.raster.width + x] >= 0.11,
                "stone at {u},{v} disappears without caustic light at {seconds}s"
            );
        }
    }
}
