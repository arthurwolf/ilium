use super::test_support::{fake_host, FakeProbe};
use super::*;
use ilium_ambient::ControlValue;
use std::collections::HashSet;
use std::sync::atomic::Ordering;

/// The built-in deterministic scenes (everything except the hosted kinds).
fn legacy_kinds() -> impl Iterator<Item = AnimationKind> {
    AnimationKind::ALL
        .into_iter()
        .filter(|kind| !kind.is_live_only())
}

fn slider_bounds(control: &ilium_ambient::Control) -> (i32, i32) {
    match control.kind {
        ilium_ambient::ControlKind::Slider { min, max, .. } => (min, max),
        _ => panic!("{} is not a slider", control.id),
    }
}

/// The numeric controls of the selected built-in scene; the shoreline's
/// style choice is covered by its own tests.
fn slider_controls(settings: &AnimationSettings) -> Vec<ilium_ambient::Control> {
    settings
        .scene_controls()
        .into_iter()
        .filter(|control| matches!(control.kind, ilium_ambient::ControlKind::Slider { .. }))
        .collect()
}

#[test]
fn loop_mode_defaults_to_sixty_seconds_and_reports_packed_ram() {
    let settings = AnimationSettings::default();
    assert_eq!(settings.playback_mode, AnimationPlaybackMode::Loop);
    assert_eq!(settings.loop_seconds, 60);
    assert_eq!(settings.estimated_loop_bytes(80, 24), 80 * 24 * 30 * 60);
    assert_eq!(settings.normalized().loop_seconds, 60);
}

#[test]
fn loop_cache_builds_incrementally_and_wraps_completed_frames() {
    let settings = AnimationSettings {
        loop_seconds: 1,
        ..Default::default()
    };
    let mut cache = AnimationLoopCache::default();
    cache.begin(&settings, 4, 2);
    assert_eq!(cache.status().total_frames, 30);
    assert!(cache.status().estimated_bytes > 0);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !cache.step(&settings, 4, 2, 12) {
        assert!(
            std::time::Instant::now() < deadline,
            "cache builder must complete"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(cache.status().is_ready);
    assert_eq!(cache.frame_index(Duration::from_secs(1)), 0);
    assert_eq!(cache.frame_index(Duration::from_millis(999)), 29);
}

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
            kind.is_ambient() || kind == AnimationKind::Wikipedia
        );
        let settings = AnimationSettings {
            kind,
            ..Default::default()
        };
        // Built-in scenes default to the loop cache; hosted kinds never use it.
        assert_eq!(settings.uses_loop_cache(), !kind.is_live_only());
    }
    assert_eq!(AnimationKind::Images.label(), "Images");
}

#[test]
fn hosted_scene_renders_scene_time_wall_time_and_speed_through_the_host() {
    let probe = FakeProbe::new();
    let mut frame = AnimationFrame::default();
    *frame.host_mut() = fake_host(&probe);
    let settings = AnimationSettings {
        kind: AnimationKind::Pipes,
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
        kind: AnimationKind::Pipes,
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
    // A built-in scene owns no colors and drops the hosted scene.
    frame.render(&AnimationSettings::default(), 8, 4, Duration::ZERO);
    assert!(!frame.has_cell_colors());
    assert_eq!(frame.cell_color(0, 0), None);
    assert_eq!(probe.alive(), 0);
}

#[test]
fn catalog_has_unique_serializable_scenes_in_user_order() {
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
        "pipes",
        "stars",
        "night_lights",
        "clouds",
        "video",
        "spectrum",
        "images",
        "dither_water",
        "atlantic_dusk",
        "cube_clock",
        "box_machine",
        "machine_screen",
        "fbm_clouds",
        "dithered_waves",
        "dithr_patterns",
        "hex_expedition",
        "vector_td",
        "wikipedia",
        "galactic_empires",
        "voxel_landscape",
        "solar_system",
        "topographic_maps",
        "graph",
        "pi",
        "earthquakes",
        "aircraft",
        "boats",
        "chess",
        "open_street_map",
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
    for kind in legacy_kinds() {
        let mut settings = AnimationSettings {
            kind,
            ..Default::default()
        };
        for control in slider_controls(&settings) {
            let (minimum, maximum) = slider_bounds(&control);
            settings
                .set_scene_control(control.id, ControlValue::Number(i32::MAX))
                .unwrap();
            assert_eq!(
                settings.scene_control(control.id).unwrap().value,
                ControlValue::Number(maximum)
            );
            settings
                .set_scene_control(control.id, ControlValue::Number(-5))
                .unwrap();
            assert_eq!(
                settings.scene_control(control.id).unwrap().value,
                ControlValue::Number(minimum)
            );
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
fn scene_specific_values_survive_switching_and_serialization() {
    let mut settings = AnimationSettings::default();
    for (index, kind) in legacy_kinds().enumerate() {
        settings.kind = kind;
        for control in slider_controls(&settings) {
            let (minimum, maximum) = slider_bounds(&control);
            settings
                .set_scene_control(
                    control.id,
                    ControlValue::Number(if index % 2 == 0 { minimum } else { maximum }),
                )
                .unwrap();
        }
    }
    let restored: AnimationSettings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(restored, settings);
    for (index, kind) in legacy_kinds().enumerate() {
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
fn scene_controls_adapt_legacy_sliders_with_stable_ids_and_persisted_names() {
    let mut settings = AnimationSettings {
        kind: AnimationKind::Kelp,
        ..Default::default()
    };
    let controls = settings.scene_controls();
    assert_eq!(
        controls
            .iter()
            .map(|control| control.id)
            .collect::<Vec<_>>(),
        LEGACY_CONTROL_IDS
    );
    assert_eq!(controls[0].label, "Plant density");
    assert_eq!(controls[0].display_value(), "100%");
    assert_eq!(
        settings.set_scene_control("scene_control_0", ControlValue::Number(175)),
        Ok(true)
    );
    assert_eq!(
        settings.set_scene_control("scene_control_0", ControlValue::Number(175)),
        Ok(false)
    );
    assert_eq!(
        settings.set_scene_control("nope", ControlValue::Number(1)),
        Ok(false)
    );
    // The persisted field names did not change.
    let value = serde_json::to_value(&settings).unwrap();
    assert_eq!(value["kelp"]["plant_density_percent"], 175);
    assert!(value.get("static_image").is_none());
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
        kind: AnimationKind::Pipes,
        ..Default::default()
    };
    assert_eq!(
        settings.scene_controls(),
        settings.ambient.controls(ilium_ambient::AmbientKind::Pipes)
    );
    // Unknown ids are not an error; the crate decides what is.
    assert_eq!(
        settings.set_scene_control("definitely_not_a_control", ControlValue::Number(1)),
        settings.ambient.clone().set_control(
            ilium_ambient::AmbientKind::Pipes,
            "definitely_not_a_control",
            ControlValue::Number(1)
        )
    );
}

#[test]
fn every_scene_is_deterministic_nonempty_distinct_and_animated() {
    let mut unique = HashSet::new();
    for kind in legacy_kinds() {
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
    for kind in legacy_kinds() {
        let base = AnimationSettings {
            kind,
            density_percent: 100,
            ..Default::default()
        };
        for control in slider_controls(&base) {
            let (minimum, maximum) = slider_bounds(&control);
            let mut low = base.clone();
            let mut high = base.clone();
            low.set_scene_control(control.id, ControlValue::Number(minimum))
                .unwrap();
            high.set_scene_control(control.id, ControlValue::Number(maximum))
                .unwrap();
            // Some effects (the big wave of a set, clinging foam) only show in
            // part of the wash cycle, so look at several instants.
            assert!(
                [7, 23, 47]
                    .into_iter()
                    .any(|seconds| snapshot_settings(low.clone(), seconds)
                        != snapshot_settings(high.clone(), seconds)),
                "{kind:?} {} has no visible effect",
                control.label
            );
        }
    }
}

#[test]
fn density_only_removes_dots_for_both_dithers() {
    for kind in legacy_kinds() {
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
        settings.clone(),
        AnimationSettings {
            lightness_percent: 35,
            hue_degrees: 120,
            saturation_percent: 50,
            ..settings.clone()
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
        snapshot_settings(settings.clone(), 0),
        snapshot_settings(settings, 13)
    );
}

// ---------------------------------------------------------------------------
// Shoreline Classic golden output. The hashes below were computed from the
// renderer BEFORE the Rich style existed; the Classic style must stay
// byte-identical to them forever.
// ---------------------------------------------------------------------------

fn fnv1a(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn classic_shoreline(reach: u16, foam: u16, grain: u16, cycle: u16) -> AnimationSettings {
    AnimationSettings {
        kind: AnimationKind::Shoreline,
        shoreline: ShorelineSettings {
            style: ShorelineStyle::Classic,
            reach_percent: reach,
            foam_width_percent: foam,
            grain_percent: grain,
            cycle_seconds: cycle,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Hash of every raster dot (exact f32 bits) over all golden times and speeds.
fn shoreline_golden_hash(settings: &AnimationSettings, width: u16, height: u16) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for speed in [100, 175] {
        for seconds in [0.0, 1.7, 6.3, 11.9, 25.4, 90.0] {
            let settings = AnimationSettings {
                speed_percent: speed,
                ..settings.clone()
            };
            let mut frame = AnimationFrame::default();
            frame.render(&settings, width, height, Duration::from_secs_f64(seconds));
            for dot in &frame.raster.dots {
                fnv1a(&mut hash, &dot.to_bits().to_le_bytes());
            }
        }
    }
    hash
}

#[test]
fn shoreline_classic_output_is_byte_identical_to_the_original_renderer() {
    // The golden hashes cover exact f32 bits, and the original renderer's
    // `sin`/`cos` come from the platform libm: Windows and macOS round the last
    // bit differently, so the bits are only pinned where they were recorded.
    if !cfg!(target_os = "linux") {
        return;
    }
    const GOLDEN: [(&str, u16, u16, u64); 9] = [
        ("default", 40, 12, 0xe4f3127ad73b5666),
        ("default", 80, 24, 0x0e1789a538969ed5),
        ("default", 137, 41, 0xeaa97cfd257999eb),
        ("wide", 40, 12, 0x0270f44e5ef65b27),
        ("wide", 80, 24, 0xc89f450f089c30c0),
        ("wide", 137, 41, 0xee9df4d0b8203e6d),
        ("narrow", 40, 12, 0xbe49cf9e0dc950aa),
        ("narrow", 80, 24, 0x83c9589fe8c6e8d6),
        ("narrow", 137, 41, 0xd5e65d0f0f5775a0),
    ];
    for (name, width, height, expected) in GOLDEN {
        let settings = match name {
            "default" => classic_shoreline(100, 75, 20, 12),
            "wide" => classic_shoreline(150, 200, 100, 6),
            _ => classic_shoreline(50, 25, 0, 30),
        };
        assert_eq!(
            shoreline_golden_hash(&settings, width, height),
            expected,
            "Classic shoreline {name} {width}x{height} changed"
        );
    }
}

/// Prints Braille frames of the Rich shoreline for eyeballing:
/// `cargo test -p ilium-client shoreline_preview -- --ignored --nocapture`.
#[test]
#[ignore = "visual aid, prints frames"]
fn shoreline_preview() {
    let width = 100;
    let height = 30;
    let times: Vec<f64> = std::env::var("SHORE_TIMES")
        .unwrap_or_else(|_| "1,3,5,7,9,11".into())
        .split(',')
        .filter_map(|part| part.parse().ok())
        .collect();
    let settings = AnimationSettings {
        kind: AnimationKind::Shoreline,
        density_percent: 100,
        ..Default::default()
    };
    for seconds in times {
        let mut frame = AnimationFrame::default();
        frame.render(&settings, width, height, Duration::from_secs_f64(seconds));
        println!("--- t={seconds}");
        for y in 0..height {
            let line: String = (0..width).map(|x| frame.glyph(x, y)).collect();
            println!("{line}");
        }
    }
}

fn shoreline_settings(edit: impl FnOnce(&mut ShorelineSettings)) -> AnimationSettings {
    let mut settings = AnimationSettings {
        kind: AnimationKind::Shoreline,
        density_percent: 100,
        ..Default::default()
    };
    edit(&mut settings.shoreline);
    settings
}

#[test]
fn shoreline_defaults_to_rich_but_a_saved_mapping_without_style_is_classic() {
    assert_eq!(ShorelineSettings::default().style, ShorelineStyle::Rich);
    assert_eq!(
        AnimationSettings::default().shoreline.style,
        ShorelineStyle::Rich
    );
    // No shoreline mapping at all: a fresh configuration.
    let fresh: AnimationSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(fresh.shoreline.style, ShorelineStyle::Rich);
    // A mapping written before styles existed keeps rendering as before.
    let old: AnimationSettings = serde_json::from_str(
        r#"{"kind":"shoreline","shoreline":{"reach_percent":100,"foam_width_percent":75,
        "grain_percent":20,"cycle_seconds":12}}"#,
    )
    .unwrap();
    assert_eq!(old.shoreline.style, ShorelineStyle::Classic);
    // Exact-bit golden: Linux libm only (see the byte-identical test above).
    if cfg!(target_os = "linux") {
        assert_eq!(
            shoreline_golden_hash(&old, 40, 12),
            0xe4f3127ad73b5666,
            "an old four-key config renders the original pixels"
        );
    }
    // The style is always written, so a save/load round trip is stable.
    let saved = serde_json::to_value(AnimationSettings::default()).unwrap();
    assert_eq!(saved["shoreline"]["style"], "rich");
    let restored: AnimationSettings = serde_json::from_value(saved).unwrap();
    assert_eq!(restored, AnimationSettings::default());
    let explicit: AnimationSettings =
        serde_json::from_str(r#"{"shoreline":{"style":"classic"}}"#).unwrap();
    assert_eq!(explicit.shoreline.style, ShorelineStyle::Classic);
}

#[test]
fn shoreline_controls_list_grows_only_in_the_rich_style() {
    let mut settings = shoreline_settings(|_| {});
    let rich_ids: Vec<_> = settings.scene_controls().iter().map(|c| c.id).collect();
    assert_eq!(rich_ids[..4], LEGACY_CONTROL_IDS);
    assert_eq!(rich_ids[4], "shoreline_style");
    assert_eq!(rich_ids.len(), 4 + 1 + 15);
    assert_eq!(
        rich_ids.iter().collect::<HashSet<_>>().len(),
        rich_ids.len()
    );
    assert_eq!(
        settings.set_scene_control("shoreline_style", ControlValue::Index(0)),
        Ok(true)
    );
    assert_eq!(settings.shoreline.style, ShorelineStyle::Classic);
    let classic_ids: Vec<_> = settings.scene_controls().iter().map(|c| c.id).collect();
    assert_eq!(
        classic_ids.len(),
        5,
        "four legacy sliders plus the style row"
    );
    assert_eq!(classic_ids[4], "shoreline_style");
    assert_eq!(
        settings.set_scene_control("shoreline_style", ControlValue::Index(0)),
        Ok(false)
    );
    // A Rich-only id is inert while its row is hidden from other scenes.
    let mut other = AnimationSettings {
        kind: AnimationKind::Kelp,
        ..Default::default()
    };
    assert_eq!(
        other.set_scene_control("shoreline_chop", ControlValue::Number(5)),
        Ok(false)
    );
    // Edits clamp, and a wrong value type changes nothing.
    let mut rich = shoreline_settings(|_| {});
    assert_eq!(
        rich.set_scene_control("shoreline_chop", ControlValue::Number(9999)),
        Ok(true)
    );
    assert_eq!(rich.shoreline.chop_percent, 100);
    assert_eq!(
        rich.set_scene_control("shoreline_chop", ControlValue::Bool(true)),
        Ok(false)
    );
    // Untrusted saved values are clamped by normalization.
    let wild = shoreline_settings(|shoreline| {
        shoreline.wave_sets = 99;
        shoreline.big_wave_every = 0;
        shoreline.stick_linger_percent = 0;
    })
    .normalized();
    assert_eq!(
        (
            wild.shoreline.wave_sets,
            wild.shoreline.big_wave_every,
            wild.shoreline.stick_linger_percent
        ),
        (4, 2, 5)
    );
}

#[test]
fn rich_shoreline_is_a_pure_function_of_settings_size_and_time() {
    let settings = shoreline_settings(|_| {});
    let times = [0.0, 3.3, 6.1, 9.4, 13.7, 26.0, 71.2];
    let fresh: Vec<Vec<u32>> = times
        .iter()
        .map(|seconds| {
            let mut frame = AnimationFrame::default();
            frame.render(&settings, 61, 19, Duration::from_secs_f64(*seconds));
            frame.raster.dots.iter().map(|dot| dot.to_bits()).collect()
        })
        .collect();
    // The loop-cache builder reuses one frame sequentially; a preview jumps
    // around. Both must agree with a fresh evaluation, in any order.
    let mut reused = AnimationFrame::default();
    for index in [3, 0, 6, 1, 5, 2, 4, 3] {
        reused.render(&settings, 61, 19, Duration::from_secs_f64(times[index]));
        let dots: Vec<u32> = reused.raster.dots.iter().map(|dot| dot.to_bits()).collect();
        assert_eq!(dots, fresh[index], "t={}", times[index]);
    }
    assert_ne!(fresh[0], fresh[3], "the scene moves");
}

#[test]
fn rich_shoreline_looks_different_from_classic_and_style_switch_rerenders() {
    let rich = snapshot_settings(shoreline_settings(|_| {}), 6);
    let classic = snapshot_settings(
        shoreline_settings(|shoreline| shoreline.style = ShorelineStyle::Classic),
        6,
    );
    assert_ne!(rich, classic);
    // One frame object switching style must not serve stale geometry.
    let mut frame = AnimationFrame::default();
    let mut settings = shoreline_settings(|_| {});
    frame.render(&settings, 64, 24, Duration::from_secs(6));
    settings.shoreline.style = ShorelineStyle::Classic;
    frame.render(&settings, 64, 24, Duration::from_secs(6));
    let switched: Vec<char> = (0..24)
        .flat_map(|y| (0..64).map(move |x| (x, y)))
        .map(|(x, y)| frame.glyph(x, y))
        .collect();
    assert_eq!(switched, classic);
}

/// Total light in the sand rows of one Rich frame, with the parts of the
/// scene that would also change the sand held constant.
fn sand_light(settings: &AnimationSettings, seconds: f64) -> f32 {
    let (width, height) = (100_u16, 30_u16);
    let mut frame = AnimationFrame::default();
    frame.render(settings, width, height, Duration::from_secs_f64(seconds));
    let columns = usize::from(width) * 2;
    let rows = usize::from(height) * 4;
    // Below the beach line at rest plus the largest meander: only sand.
    frame.raster.dots[(rows * 3 / 4) * columns..]
        .iter()
        .sum::<f32>()
        + frame.raster.dots[(rows * 3 / 5) * columns..(rows * 3 / 4) * columns]
            .iter()
            .sum::<f32>()
}

#[test]
fn clinging_foam_bits_appear_on_the_sand_after_the_wave_then_vanish() {
    // Isolate the bits: a synchronous beach, identical waves, everything else
    // that could add light to the sand switched off.
    let build = |amount: u16, linger: u16| {
        shoreline_settings(|shoreline| {
            shoreline.swell_angle_degrees = 0;
            shoreline.set_irregularity_percent = 0;
            shoreline.meander_percent = 0;
            shoreline.lace_percent = 0;
            shoreline.backwash_percent = 0;
            shoreline.sparkle_percent = 0;
            shoreline.stick_amount_percent = amount;
            shoreline.stick_linger_percent = linger;
        })
    };
    let cycle = 12.0;
    let series = |amount: u16, linger: u16| -> Vec<f32> {
        let settings = build(amount, linger);
        (0..48)
            .map(|step| sand_light(&settings, f64::from(step) * cycle / 24.0))
            .collect()
    };
    let with_bits = series(100, 20);
    let without = series(0, 20);
    let extra: Vec<f32> = with_bits
        .iter()
        .zip(&without)
        .map(|(with, without)| with - without)
        .collect();
    // During the advance (phase 0..0.3 of every cycle) nothing clings yet.
    for cycle_index in 0..2 {
        for step in 0..6 {
            assert_eq!(
                extra[cycle_index * 24 + step],
                0.0,
                "no bits while the wave is still advancing (step {step})"
            );
        }
    }
    // Some sample of the retreat has bits, and they are gone again before the
    // next wave arrives.
    let peak = extra.iter().copied().fold(0.0, f32::max);
    assert!(peak > 2.0, "bits cling to the sand after the wave: {peak}");
    let visible: Vec<usize> = (0..24).filter(|step| extra[*step] > 0.0).collect();
    assert!(!visible.is_empty());
    assert!(
        extra[24..30].iter().all(|light| *light == 0.0),
        "bits are gone when the next wave has arrived"
    );
    // They come and go within the lifetime the linger control sets.
    let lingering = |linger: u16| {
        let with = series(100, linger);
        with.iter()
            .zip(&without)
            .take(24)
            .filter(|(with, without)| **with > **without)
            .count()
    };
    assert!(
        lingering(60) > lingering(5),
        "longer linger, longer presence"
    );
}

#[test]
fn overhaul_pond_accepts_sixty_four_pads() {
    let mut settings = AnimationSettings {
        kind: AnimationKind::QuietPond,
        ..Default::default()
    };
    settings.quiet_pond.pad_count = 64;
    assert_eq!(settings.normalized().quiet_pond.pad_count, 64);
    let count = settings
        .scene_control("scene_0")
        .or_else(|| settings.scene_controls().into_iter().next())
        .unwrap();
    assert_eq!(slider_bounds(&count).1, 64);
}

#[test]
fn overhaul_cache_has_at_least_thirty_frames_per_second() {
    let settings = AnimationSettings {
        loop_seconds: 1,
        ..Default::default()
    };
    let mut cache = AnimationLoopCache::default();
    cache.begin(&settings, 4, 2);
    assert!(
        cache.status().total_frames >= 30,
        "12 fps cache visibly steps even when fully calculated"
    );
}

#[test]
fn cached_builtin_playback_releases_a_previous_hosted_scene() {
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
        let kind = ilium_ambient::AmbientKind::DitherWater;
        let mut settings = ilium_ambient::AmbientSettings::default();
        let first = host.sync(kind, &settings, Duration::ZERO);
        // The same settings change nothing.
        assert_eq!(host.sync(kind, &settings, Duration::from_secs(1)), first);
        settings.dither_water.seed += 1;
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
            ilium_ambient::AmbientKind::Pipes,
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
    let reference = packed_with(PackKey::plain(100, DitherMode::Ordered), 0.5);
    for mode in DitherMode::ALL.into_iter().skip(1) {
        assert_ne!(
            packed_with(PackKey::plain(100, mode), 0.5),
            reference,
            "{mode:?} must differ from ordered"
        );
    }
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
        let Some(stepped) = control.stepped(1) else {
            continue;
        };
        assert_eq!(
            settings.set_common_control(id, stepped.clone()),
            Ok(true),
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
fn look_changes_never_rebuild_the_loop_cache_but_pattern_changes_do() {
    let mut cache = AnimationLoopCache::default();
    let mut settings = AnimationSettings {
        enabled: true,
        ..Default::default()
    };
    cache.begin(&settings, 20, 8);
    let first = cache.status().total_frames;
    assert!(first > 0);
    settings.appearance.brightness_percent = 30;
    settings.appearance.palette = 3;
    settings.panels = PanelTarget::Left;
    settings.fps_limit = 5;
    cache.begin(&settings, 20, 8);
    assert!(
        cache.status().completed_frames > 0 || cache.status().is_ready || first > 0,
        "same cache identity: nothing restarts"
    );
}

#[path = "live_scene_tests.rs"]
mod live_scene_tests;
