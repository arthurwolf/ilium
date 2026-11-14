use super::test_support::{fake_host, FakeProbe};
use super::*;
use ilium_ambient::ControlValue;
use std::collections::HashSet;
use std::sync::atomic::Ordering;

#[test]
fn loop_mode_defaults_to_sixty_seconds_and_reports_packed_ram() {
    let settings = AnimationSettings::default();
    assert_eq!(settings.playback_mode, AnimationPlaybackMode::Loop);
    assert_eq!(settings.loop_seconds, 60);
    assert_eq!(settings.estimated_loop_bytes(80, 24), 80 * 24 * 30 * 60);
    assert_eq!(settings.normalized().loop_seconds, 60);
}

#[test]
fn hosted_kinds_map_to_the_crate_and_are_live_only() {
    let hosted: Vec<_> = AnimationKind::ALL
        .into_iter()
        .filter_map(AnimationKind::ambient)
        .collect();
    assert_eq!(hosted, ilium_ambient::AmbientKind::ALL.to_vec());
    for kind in AnimationKind::ALL {
        assert_eq!(kind.is_ambient(), kind.ambient().is_some());
        assert_eq!(
            kind.is_live_only(),
            kind.is_ambient() || matches!(kind, AnimationKind::Wikipedia | AnimationKind::Semantic)
        );
        let settings = AnimationSettings {
            kind,
            ..Default::default()
        };
        // Every remaining kind is live-only, so none of them uses the loop cache.
        assert!(!settings.uses_loop_cache());
    }
    assert_eq!(AnimationKind::Images.label(), "Images");
}

#[test]
fn hosted_scene_renders_scene_time_wall_time_and_speed_through_the_host() {
    let probe = FakeProbe::new();
    let mut frame = AnimationFrame::default();
    *frame.host_mut() = fake_host(&probe);
    let settings = AnimationSettings {
        kind: AnimationKind::Spectrum,
        speed_percent: 200,
        ..Default::default()
    };
    // The scene starts at session time 10 s: scene time starts at zero.
    frame.render(&settings, 8, 4, Duration::from_secs(10));
    assert_eq!(probe.last_wall_ms.load(Ordering::SeqCst), 0);
    frame.render(&settings, 8, 4, Duration::from_secs(13));
    assert_eq!(probe.last_wall_ms.load(Ordering::SeqCst), 3000);
    assert_eq!(probe.last_time_ms.load(Ordering::SeqCst), 6000);
    assert_ne!(frame.glyph(0, 0), ' ');
    assert_eq!(probe.constructed.load(Ordering::SeqCst), 1);
}

#[test]
fn hosted_frame_reuses_the_render_inside_one_bucket_and_repacks_for_density() {
    let probe = FakeProbe::new();
    let mut frame = AnimationFrame::default();
    *frame.host_mut() = fake_host(&probe);
    let settings = AnimationSettings {
        kind: AnimationKind::Spectrum,
        ..Default::default()
    };
    let time = Duration::from_secs(1);
    frame.render(&settings, 8, 4, time);
    frame.render(&settings, 8, 4, time);
    assert_eq!(probe.rendered.load(Ordering::SeqCst), 1);
    let sparse = AnimationSettings {
        density_percent: 25,
        ..settings.clone()
    };
    frame.render(&sparse, 8, 4, time);
    assert_eq!(
        probe.rendered.load(Ordering::SeqCst),
        1,
        "density only repacks"
    );
    frame.render(&settings, 8, 4, time + Duration::from_millis(100));
    assert_eq!(probe.rendered.load(Ordering::SeqCst), 2);
}

#[test]
fn a_scene_reporting_cell_colors_colors_the_cells_and_others_do_not() {
    let probe = FakeProbe::new();
    let mut frame = AnimationFrame::default();
    *frame.host_mut() = fake_host(&probe);
    let settings = AnimationSettings {
        kind: AnimationKind::Images,
        density_percent: 100,
        ..Default::default()
    };
    frame.render(&settings, 8, 4, Duration::ZERO);
    assert!(!frame.has_cell_colors());
    assert_eq!(frame.cell_color(0, 0), None);
    probe.uses_colors.store(true, Ordering::SeqCst);
    frame.render(&settings, 8, 4, Duration::from_millis(500));
    assert!(frame.has_cell_colors());
    assert_eq!(frame.cell_color(0, 0), Some((200, 20, 40)));
    assert_eq!(frame.cell_color(1, 0), Some((20, 40, 200)));
    assert_eq!(frame.cell_color(8, 0), None, "outside the field");
    // A plugin scene is drawn by its plugin, so the kernel frame owns no colors and drops the hosted scene.
    frame.render(&AnimationSettings::default(), 8, 4, Duration::ZERO);
    assert!(!frame.has_cell_colors());
    assert_eq!(frame.cell_color(0, 0), None);
    assert_eq!(probe.alive(), 0);
}

#[test]
fn catalog_has_unique_serializable_scenes_in_user_order() {
    let expected = [
        "night_lights",
        "clouds",
        "video",
        "spectrum",
        "images",
        "plugin_scene",
        "wikipedia",
        "open_street_map",
        "semantic",
    ];
    assert_eq!(expected.len(), AnimationKind::ALL.len());
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
        AnimationKind::ALL.len()
    );
}

#[test]
fn settings_defaults_and_untrusted_ranges_are_normalized() {
    let settings: AnimationSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings, AnimationSettings::default());
    assert!(!settings.enabled);
    assert_eq!(settings.kind, AnimationKind::PluginScene);
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
}

#[test]
fn hsl_conversion_has_exact_endpoints_and_achromatic_defaults() {
    let base = AnimationSettings::default();
    for hue in [0, 60, 120, 180, 240, 359] {
        assert_eq!(
            AnimationSettings {
                hue_degrees: hue,
                ..base.clone()
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
            ..base.clone()
        };
        assert_eq!(settings.foreground_rgb(), rgb);
        assert_eq!(
            AnimationSettings {
                lightness_percent: 0,
                ..settings.clone()
            }
            .foreground_rgb(),
            (0, 0, 0)
        );
        assert_eq!(
            AnimationSettings {
                lightness_percent: 100,
                ..settings.clone()
            }
            .foreground_rgb(),
            (255, 255, 255)
        );
    }
}

#[test]
fn common_controls_round_trip_every_shared_setting() {
    let mut settings = AnimationSettings::default();
    for (id, value, check) in [
        (
            "background",
            ControlValue::Bool(true),
            settings_enabled as fn(&AnimationSettings) -> bool,
        ),
        ("speed", ControlValue::Number(150), |s| {
            s.speed_percent == 150
        }),
        ("density", ControlValue::Number(80), |s| {
            s.density_percent == 80
        }),
        ("dither", ControlValue::Index(1), |s| {
            s.dither == DitherMode::Stippled
        }),
        ("lightness", ControlValue::Number(35), |s| {
            s.lightness_percent == 35
        }),
        ("hue", ControlValue::Number(275), |s| s.hue_degrees == 275),
        ("saturation", ControlValue::Number(40), |s| {
            s.saturation_percent == 40
        }),
        ("playback", ControlValue::Index(1), |s| {
            s.playback_mode == AnimationPlaybackMode::Live
        }),
        ("loop_seconds", ControlValue::Number(90), |s| {
            s.loop_seconds == 90
        }),
    ] {
        assert_eq!(
            settings.set_common_control(id, value.clone()),
            Ok(true),
            "{id}"
        );
        assert!(check(&settings), "{id}");
        assert_eq!(
            settings.set_common_control(id, value),
            Ok(false),
            "{id} unchanged"
        );
        assert!(settings.common_control(id).is_some());
    }
    assert!(common_control_ids()
        .iter()
        .all(|id| settings.common_control(id).is_some()));
    assert_eq!(settings.common_control("bogus"), None);
    assert_eq!(
        settings.set_common_control("speed", ControlValue::Number(9999)),
        Ok(true)
    );
    assert_eq!(settings.speed_percent, 300);
    assert_eq!(
        settings.set_common_control("speed", ControlValue::Bool(true)),
        Ok(false),
        "a value of the wrong type changes nothing"
    );
}

fn settings_enabled(settings: &AnimationSettings) -> bool {
    settings.enabled
}

#[test]
fn hosted_controls_route_through_the_crate_and_report_errors() {
    let mut settings = AnimationSettings {
        kind: AnimationKind::Spectrum,
        ..Default::default()
    };
    assert_eq!(
        settings.scene_controls(),
        settings
            .ambient
            .controls(ilium_ambient::AmbientKind::Spectrum)
    );
    // Unknown ids are not an error; the crate decides what is.
    assert_eq!(
        settings.set_scene_control("definitely_not_a_control", ControlValue::Number(1)),
        settings.ambient.clone().set_control(
            ilium_ambient::AmbientKind::Spectrum,
            "definitely_not_a_control",
            ControlValue::Number(1)
        )
    );
}

#[test]
fn geometry_handles_empty_tiny_resize_and_outside_queries() {
    let probe = FakeProbe::new();
    let mut frame = AnimationFrame::default();
    *frame.host_mut() = fake_host(&probe);
    // Hosted kinds go through the fake so the test needs no network or ffmpeg.
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
        frame.pack(PackKey::plain(100, DitherMode::Ordered));
        assert_eq!(frame.glyph(0, 0) as u32, 0x2800 + (1 << bit));
    }
}

#[test]
fn cached_packed_playback_releases_a_previous_hosted_scene() {
    use super::test_support::{fake_host, FakeProbe};
    use std::sync::atomic::Ordering;
    let probe = FakeProbe::new();
    probe.uses_colors.store(true, Ordering::SeqCst);
    let mut frame = AnimationFrame {
        host: fake_host(&probe),
        ..Default::default()
    };
    frame.render(
        &AnimationSettings {
            kind: AnimationKind::Images,
            ..Default::default()
        },
        4,
        2,
        Duration::ZERO,
    );
    assert_eq!(probe.alive(), 1);
    frame.load_packed_cells(4, 2, &[255; 8]);
    assert_eq!(
        probe.alive(),
        0,
        "cached built-ins must release video/image workers and colored-scene metadata"
    );
    assert!(!frame.host().uses_cell_colors());
}

#[test]
fn packing_matches_reference_for_every_braille_pattern_and_density_boundary() {
    const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
    for (width, height) in [(1, 1), (3, 5), (17, 7)] {
        let mut frame = AnimationFrame::default();
        frame.resize(width, height);
        for dither in [DitherMode::Ordered, DitherMode::Stippled] {
            for density in [0, 1, 50, 99, 100] {
                for pattern in 0..=255_u8 {
                    for y in 0..usize::from(height) {
                        for x in 0..usize::from(width) {
                            let mask = pattern.wrapping_add((y * usize::from(width) + x) as u8);
                            for (dy, bits) in BITS.iter().enumerate() {
                                for (dx, bit) in bits.iter().enumerate() {
                                    frame.raster.dots
                                        [(y * 4 + dy) * frame.raster.width + x * 2 + dx] =
                                        if mask & bit != 0 { 1.0 } else { 0.0 };
                                }
                            }
                        }
                    }
                    frame.pack(PackKey::plain(density, dither));
                    for y in 0..usize::from(height) {
                        for x in 0..usize::from(width) {
                            let mut expected = 0;
                            for (dy, bits) in BITS.iter().enumerate() {
                                for (dx, bit) in bits.iter().enumerate() {
                                    let rx = x * 2 + dx;
                                    let ry = y * 4 + dy;
                                    let value = frame.raster.dots[ry * frame.raster.width + rx];
                                    if value * (f32::from(density) / 100.0)
                                        > raster::threshold(rx, ry, dither)
                                    {
                                        expected |= bit;
                                    }
                                }
                            }
                            assert_eq!(frame.cells[y * usize::from(width) + x], expected, "size={width}x{height}, pattern={pattern}, density={density}, dither={dither:?}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn density_repacking_matches_original_comparisons_at_float_boundaries() {
    const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
    let mut frame = AnimationFrame::default();
    frame.resize(7, 3);
    for dither in [DitherMode::Ordered, DitherMode::Stippled] {
        for offset in 0..8 {
            for y in 0..frame.raster.height {
                for x in 0..frame.raster.width {
                    let threshold = raster::threshold(x, y, dither);
                    let values = [
                        f32::NAN,
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        -0.0,
                        threshold,
                        f32::from_bits(threshold.to_bits().saturating_sub(1)),
                        f32::from_bits(threshold.to_bits() + 1),
                        1.0,
                    ];
                    frame.raster.dots[y * frame.raster.width + x] =
                        values[(x + y + offset) % values.len()];
                }
            }
            for density in [25, 60, 99, 100] {
                frame.pack(PackKey::plain(density, dither));
                for y in 0..3 {
                    for x in 0..7 {
                        let mut expected = 0;
                        for (dy, row) in BITS.iter().enumerate() {
                            for (dx, bit) in row.iter().enumerate() {
                                let rx = x * 2 + dx;
                                let ry = y * 4 + dy;
                                if frame.raster.dots[ry * frame.raster.width + rx]
                                    * (f32::from(density) / 100.0)
                                    > raster::threshold(rx, ry, dither)
                                {
                                    expected |= bit;
                                }
                            }
                        }
                        assert_eq!(
                            frame.cells[y * 7 + x],
                            expected,
                            "density={density},offset={offset},dither={dither:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn hosted_scene_takes_new_settings_in_place_or_is_rebuilt() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Adapting {
        accept: bool,
        applied: Arc<AtomicUsize>,
    }
    impl ilium_ambient::Scene for Adapting {
        fn render(&mut self, _frame: &mut ilium_ambient::Frame<'_>) {}
        fn reconfigure(&mut self, _settings: &ilium_ambient::AmbientSettings) -> bool {
            if self.accept {
                self.applied.fetch_add(1, Ordering::SeqCst);
            }
            self.accept
        }
    }

    for accept in [true, false] {
        let built = Arc::new(AtomicUsize::new(0));
        let applied = Arc::new(AtomicUsize::new(0));
        let (factory_built, factory_applied) = (built.clone(), applied.clone());
        let mut host = super::AmbientHost::with_factory(Box::new(move |_, _, _| {
            factory_built.fetch_add(1, Ordering::SeqCst);
            Box::new(Adapting {
                accept,
                applied: factory_applied.clone(),
            })
        }));
        let kind = ilium_ambient::AmbientKind::Clouds;
        let mut settings = ilium_ambient::AmbientSettings::default();
        let first = host.sync(kind, &settings, Duration::ZERO);
        // The same settings change nothing.
        assert_eq!(host.sync(kind, &settings, Duration::from_secs(1)), first);
        settings.clouds.contrast_percent += 10;
        let second = host.sync(kind, &settings, Duration::from_secs(5));
        assert_ne!(
            first, second,
            "the render of the old settings is not reused"
        );
        assert_eq!(built.load(Ordering::SeqCst), if accept { 1 } else { 2 });
        assert_eq!(applied.load(Ordering::SeqCst), usize::from(accept));
        // Scene time runs on through an in-place update and restarts on a rebuild.
        let wall = host.wall(Duration::from_secs(5));
        assert_eq!(
            wall,
            if accept {
                Duration::from_secs(5)
            } else {
                Duration::ZERO
            }
        );
        // A scene of another kind is never reconfigured with foreign settings.
        host.sync(
            ilium_ambient::AmbientKind::Spectrum,
            &settings,
            Duration::from_secs(6),
        );
        assert_eq!(built.load(Ordering::SeqCst), if accept { 2 } else { 3 });
    }
}

#[path = "openstreetmap_tests.rs"]
mod openstreetmap_tests;

fn packed_with(key: PackKey, tone: f32) -> Vec<u8> {
    let mut frame = AnimationFrame::default();
    frame.resize(24, 8);
    frame.raster.dots.fill(tone);
    frame.pack(key);
    frame.packed_cells().to_vec()
}

fn lit_bits(cells: &[u8]) -> u32 {
    cells.iter().map(|cell| cell.count_ones()).sum()
}

#[test]
fn every_dither_mode_packs_a_tone_to_about_that_many_dots() {
    let total = (24 * 8 * 8) as f32;
    for mode in DitherMode::ALL {
        for tone in [0.25_f32, 0.5, 0.75] {
            let cells = packed_with(PackKey::plain(100, mode), tone);
            let lit = lit_bits(&cells) as f32 / total;
            // Atkinson drops a quarter of the error; the matrices round in steps.
            assert!((lit - tone).abs() < 0.2, "{mode:?} tone {tone} lit {lit}");
        }
        assert_eq!(
            lit_bits(&packed_with(PackKey::plain(100, mode), 0.0)),
            0,
            "{mode:?}"
        );
    }
}

#[test]
fn dither_modes_make_different_patterns() {
    // Some matrices coincide with each other on a flat tone, but the catalog
    // must offer clearly distinct looks, not relabelled copies.
    let mut patterns = Vec::new();
    for mode in DitherMode::ALL {
        let mut pattern = Vec::new();
        for tone in [0.2_f32, 0.37, 0.6] {
            pattern.extend(packed_with(PackKey::plain(100, mode), tone));
        }
        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }
    }
    assert!(
        patterns.len() >= DitherMode::ALL.len() - 3,
        "only {} distinct patterns for {} modes",
        patterns.len(),
        DitherMode::ALL.len()
    );
}

#[test]
fn pattern_invert_and_contrast_change_which_dots_are_lit() {
    let normal = packed_with(PackKey::plain(100, DitherMode::Ordered), 0.3);
    let inverted = packed_with(
        PackKey {
            invert: true,
            ..PackKey::plain(100, DitherMode::Ordered)
        },
        0.3,
    );
    assert!(lit_bits(&inverted) > lit_bits(&normal) * 2);
    let hard = packed_with(
        PackKey {
            contrast_percent: 200,
            ..PackKey::plain(100, DitherMode::Ordered)
        },
        0.3,
    );
    assert!(lit_bits(&hard) < lit_bits(&normal));
}

#[test]
fn every_common_control_including_the_look_rows_round_trips() {
    for id in common_control_ids() {
        let mut settings = AnimationSettings::default();
        let control = settings
            .common_control(id)
            .unwrap_or_else(|| panic!("{id} resolves"));
        let Some(mut stepped) = control.stepped(1) else {
            continue;
        };
        // Defaults may sit at a slider boundary. Step inward before asserting
        // a change; a clamped step must remain a truthful no-change result.
        if stepped == control.value {
            assert_eq!(settings.set_common_control(id, stepped.clone()), Ok(false));
            stepped = control.stepped(-1).expect("step-capable control");
        }
        assert_eq!(
            settings.set_common_control(id, stepped.clone()),
            Ok(stepped != control.value),
            "{id}"
        );
        assert_eq!(
            settings.common_control(id).map(|row| row.value),
            Some(stepped),
            "{id}"
        );
    }
}

#[test]
fn choosing_a_preset_sets_the_look_and_may_set_dither_and_density() {
    let mut settings = AnimationSettings::default();
    let matrix = ilium_ambient::style::StylePreset::Matrix.index();
    assert_eq!(
        settings.set_common_control("look_preset", ControlValue::Index(matrix)),
        Ok(true)
    );
    assert_eq!(
        settings.appearance.preset,
        ilium_ambient::style::StylePreset::Matrix
    );
    assert_eq!(settings.dither, DitherMode::Lines);
    assert!(settings.appearance.brightness_percent < 100);
    // A hand edit afterwards leaves the preset.
    settings
        .set_common_control("look_brightness", ControlValue::Number(90))
        .unwrap();
    assert_eq!(
        settings.appearance.preset,
        ilium_ambient::style::StylePreset::Custom
    );
}

#[test]
fn look_and_panel_settings_are_global_normalized_and_persist_through_serde() {
    let mut settings = AnimationSettings::default();
    settings.appearance.brightness_percent = 0;
    settings.fps_limit = 999;
    settings.panels = PanelTarget::Right;
    let normalized = settings.normalized();
    assert_eq!(normalized.appearance.brightness_percent, 1);
    assert_eq!(normalized.fps_limit, 30);
    let yaml = serde_json::to_string(&settings).unwrap();
    assert_eq!(
        serde_json::from_str::<AnimationSettings>(&yaml).unwrap(),
        settings
    );
    // One look for every scene: the field is not per scene.
    for kind in AnimationKind::ALL {
        let selected = AnimationSettings {
            kind,
            ..settings.clone()
        };
        assert_eq!(selected.appearance, settings.appearance);
    }
    // Missing keys fall back to the neutral defaults.
    let sparse: AnimationSettings = serde_json::from_str("{}").unwrap();
    assert!(sparse.appearance.is_neutral());
    assert_eq!(sparse.panels, PanelTarget::Both);
    assert_eq!(sparse.fps_limit, 0);
}

#[test]
fn scenes_that_moved_into_the_scenes_plugin_load_as_the_plugin_scene_kind() {
    for old in [
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
        "breathing_mountain",
    ] {
        let kind: AnimationKind = serde_json::from_str(&format!("\"{old}\"")).unwrap();
        assert_eq!(kind, AnimationKind::PluginScene, "{old}");
        assert_eq!(serde_json::to_string(&kind).unwrap(), "\"plugin_scene\"");
    }
}
