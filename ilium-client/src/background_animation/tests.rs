use super::*;
use std::collections::HashSet;

fn snapshot_settings(settings: AnimationSettings, seconds: u64) -> Vec<char> {
    let mut frame = AnimationFrame::default();
    frame.render(&settings, 64, 24, Duration::from_secs(seconds));
    (0..24)
        .flat_map(|y| (0..64).map(move |x| (x, y)))
        .map(|(x, y)| frame.glyph(x, y))
        .collect()
}

fn snapshot(kind: AnimationKind, seconds: u64, density: u16) -> Vec<char> {
    snapshot_settings(
        AnimationSettings {
            kind,
            density_percent: density,
            ..Default::default()
        },
        seconds,
    )
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
        "quiet_pond",
    ];
    for (kind, id) in AnimationKind::ALL.into_iter().zip(expected) {
        assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{id}\""));
        assert_eq!(
            serde_json::from_str::<AnimationKind>(&format!("\"{id}\"")).unwrap(),
            kind
        );
        assert!(!kind.label().is_empty());
        assert!(!kind.description().is_empty());
        assert!(!kind.label().to_lowercase().contains("metaball"));
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
    assert_eq!(settings.foreground_rgb(), (153, 153, 153));
    let normalized = AnimationSettings {
        speed_percent: 0,
        density_percent: u16::MAX,
        hue_degrees: u16::MAX,
        lightness_percent: u16::MAX,
        saturation_percent: u16::MAX,
        ..settings
    }
    .normalized();
    assert_eq!(
        (normalized.speed_percent, normalized.density_percent),
        (25, 100)
    );
    assert_eq!(
        (
            normalized.hue_degrees,
            normalized.lightness_percent,
            normalized.saturation_percent
        ),
        (359, 100, 100)
    );
    for kind in AnimationKind::ALL {
        let mut settings = AnimationSettings {
            kind,
            ..Default::default()
        };
        for row in 17..=20 {
            let slider = settings.slider(row).unwrap();
            settings.set_slider_value(row, u16::MAX);
            assert_eq!(settings.slider(row).unwrap().value, slider.maximum);
            settings.set_slider_value(row, 0);
            assert_eq!(settings.slider(row).unwrap().value, slider.minimum);
        }
    }
}

#[test]
fn hsl_conversion_has_exact_endpoints_and_achromatic_defaults() {
    let base = AnimationSettings::default();
    for hue in [0, 60, 120, 180, 240, 359] {
        assert_eq!(
            AnimationSettings {
                hue_degrees: hue,
                ..base
            }
            .foreground_rgb(),
            (153, 153, 153)
        );
    }
    for (hue, rgb) in [(0, (255, 0, 0)), (120, (0, 255, 0)), (240, (0, 0, 255))] {
        let settings = AnimationSettings {
            hue_degrees: hue,
            saturation_percent: 100,
            lightness_percent: 50,
            ..base
        };
        assert_eq!(settings.foreground_rgb(), rgb);
        assert_eq!(
            AnimationSettings {
                lightness_percent: 0,
                ..settings
            }
            .foreground_rgb(),
            (0, 0, 0)
        );
        assert_eq!(
            AnimationSettings {
                lightness_percent: 100,
                ..settings
            }
            .foreground_rgb(),
            (255, 255, 255)
        );
    }
}

#[test]
fn scene_specific_values_survive_switching_and_serialization() {
    let mut settings = AnimationSettings::default();
    for (index, kind) in AnimationKind::ALL.into_iter().enumerate() {
        settings.kind = kind;
        for row in 17..=20 {
            let slider = settings.slider(row).unwrap();
            settings.set_slider_value(
                row,
                if index % 2 == 0 {
                    slider.minimum
                } else {
                    slider.maximum
                },
            );
        }
    }
    let restored: AnimationSettings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(restored, settings);
    for (index, kind) in AnimationKind::ALL.into_iter().enumerate() {
        settings.kind = kind;
        for slider in settings.scene_sliders() {
            assert_eq!(
                slider.value,
                if index % 2 == 0 {
                    slider.minimum
                } else {
                    slider.maximum
                }
            );
        }
    }
}

#[test]
fn every_scene_is_deterministic_nonempty_distinct_and_animated() {
    let mut unique = HashSet::new();
    for kind in AnimationKind::ALL {
        let first = snapshot(kind, 7, 100);
        assert_eq!(first, snapshot(kind, 7, 100), "{kind:?}");
        assert!(first.iter().any(|c| *c != ' '), "{kind:?} is empty");
        assert!(first
            .iter()
            .all(|c| *c == ' ' || ('\u{2801}'..='\u{28ff}').contains(c)));
        assert!(unique.insert(first), "{kind:?} duplicates another scene");
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
fn every_named_scene_slider_changes_the_rendered_scene() {
    for kind in AnimationKind::ALL {
        for row in 17..=20 {
            let mut low = AnimationSettings {
                kind,
                density_percent: 100,
                ..Default::default()
            };
            let mut high = low;
            let slider = low.slider(row).unwrap();
            low.set_slider_value(row, slider.minimum);
            high.set_slider_value(row, slider.maximum);
            assert_ne!(
                snapshot_settings(low, 7),
                snapshot_settings(high, 7),
                "{kind:?} {} has no visible effect",
                slider.label
            );
        }
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
                ..Default::default()
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
                    assert_eq!(
                        low_bits & bits(high.glyph(x, y)),
                        low_bits,
                        "{kind:?} {dither:?}"
                    );
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
                    ..Default::default()
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
    for (x, y, bit) in [
        (0, 0, 0),
        (0, 1, 1),
        (0, 2, 2),
        (1, 0, 3),
        (1, 1, 4),
        (1, 2, 5),
        (0, 3, 6),
        (1, 3, 7),
    ] {
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
        (AnimationKind::TeaSteam, (10, 18, 56, 24)),
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
fn identical_requests_reuse_buffers_and_palette_changes_reuse_geometry() {
    let mut frame = AnimationFrame::default();
    let settings = AnimationSettings {
        kind: AnimationKind::StoneCaustics,
        ..Default::default()
    };
    let time = Duration::from_secs(2);
    frame.render(&settings, 64, 24, time);
    let raster_pointer = frame.raster.dots.as_ptr();
    let cells_pointer = frame.cells.as_ptr();
    let cells = frame.cells.clone();
    assert_eq!(frame.scene_cache.preparations, 1);
    for next in [
        settings,
        AnimationSettings {
            lightness_percent: 35,
            hue_degrees: 120,
            saturation_percent: 50,
            ..settings
        },
    ] {
        frame.render(&next, 64, 24, time);
        assert_eq!(frame.cells, cells);
        assert_eq!(frame.raster.dots.as_ptr(), raster_pointer);
        assert_eq!(frame.cells.as_ptr(), cells_pointer);
        assert_eq!(frame.scene_cache.preparations, 1);
    }
    frame.render(&settings, 64, 24, Duration::from_secs(3));
    assert_eq!(
        frame.scene_cache.preparations, 1,
        "motion must not rebuild stone geometry"
    );
    frame.render(&settings, 60, 20, time);
    assert_eq!(frame.raster.dots.as_ptr(), raster_pointer);
    assert_eq!(frame.cells.as_ptr(), cells_pointer);
    assert_eq!(frame.scene_cache.preparations, 2);
}

#[test]
fn ripple_crests_survive_destructive_interference_as_continuous_arcs() {
    let mut frame = AnimationFrame::default();
    let settings = AnimationSettings {
        kind: AnimationKind::TwoRipples,
        density_percent: 100,
        ..Default::default()
    };
    let separation = f32::from(settings.two_ripples.source_separation_percent) / 100.0;
    let frequency = 28.0 * 100.0 / f32::from(settings.two_ripples.wavelength_percent);
    for seconds in [0, 2] {
        frame.render(&settings, 120, 48, Duration::from_secs(seconds));
        let radius = (std::f32::consts::FRAC_PI_2 + seconds as f32 * 1.35) / frequency;
        for angle_index in 0..24 {
            let angle = angle_index as f32 * std::f32::consts::TAU / 24.0;
            let u = 0.5 - separation * 0.5 + radius * angle.cos() / frame.raster.aspect();
            let v = 0.44 + radius * angle.sin();
            let x = (u * frame.raster.width as f32) as usize;
            let y = (v * frame.raster.height as f32) as usize;
            assert!(
                frame.raster.dots[y * frame.raster.width + x] >= 0.40,
                "missing crest at {seconds}s, angle {angle_index}"
            );
        }
    }
}

#[test]
fn stone_layout_has_the_two_requested_size_populations() {
    let stones = scenes::stone_layout(StoneCausticsSettings::default());
    assert!(stones.iter().any(|stone| stone.is_small));
    assert!(stones.iter().any(|stone| !stone.is_small));
    for stone in stones {
        assert!((if stone.is_small {
            1.0..=2.0
        } else {
            8.0..=12.0
        })
        .contains(&stone.radius_units));
        assert!((0.0..=1.0).contains(&stone.center.0));
        assert!((0.0..=1.0).contains(&stone.center.1));
    }
}

#[test]
fn actual_dome_normals_project_light_in_opposite_directions_on_opposite_slopes() {
    let settings = StoneCausticsSettings::default();
    let stones = scenes::stone_layout(settings);
    let stone = stones[0];
    let offset = stone.radius_units * 0.020 * 0.40;
    let left_u = stone.center.0 - offset;
    let right_u = stone.center.0 + offset;
    // A single stone isolates the geometric contract from deliberate overlap.
    let left = scenes::stone_surface(left_u, stone.center.1, 1.0, &[stone], settings);
    let right = scenes::stone_surface(right_u, stone.center.1, 1.0, &[stone], settings);
    assert!(left.height > 0.0 && right.height > 0.0);
    assert!(left.normal.0 < 0.0 && right.normal.0 > 0.0);
    assert!(left.sample.0 < left_u && right.sample.0 > right_u);
    assert!(left.base >= 0.10 && right.base >= 0.10);
}

#[test]
fn replacing_the_final_scene_reads_its_previous_id_and_writes_the_new_id() {
    let kind: AnimationKind = serde_json::from_str("\"breathing_mountain\"").unwrap();
    assert_eq!(kind, AnimationKind::QuietPond);
    assert_eq!(serde_json::to_string(&kind).unwrap(), "\"quiet_pond\"");
}

#[test]
fn zero_kelp_current_freezes_stems_and_ribbon_twisting() {
    let settings = AnimationSettings {
        kind: AnimationKind::Kelp,
        kelp: KelpSettings {
            current_strength_percent: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        snapshot_settings(settings, 0),
        snapshot_settings(settings, 13)
    );
}
