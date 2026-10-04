//! Top-down heliocentric view using JPL approximate J2000 orbital elements.
//! Source: https://ssd.jpl.nasa.gov/planets/approx_pos.html (1800–2050).
//! Body radii: NASA planetary fact sheet, equatorial diameters / 2.
mod settings;
use super::stars::astro;
use crate::{
    control::SceneSettings,
    scene::{Frame, Scene, SceneEnv},
    style::ScenePalette,
};
pub use settings::SolarSystemSettings;

const RADII_KM: [f64; 8] = [
    2439.5, 6052.0, 6378.0, 3396.0, 71492.0, 60268.0, 25559.0, 24764.0,
];
const AU_KM: f64 = 149_597_870.7;
const SUN_RADIUS_KM: f64 = 695_700.0;
const OUTER_AU: f64 = 31.0;
const COLORS: [[u8; 3]; 8] = [
    [190, 180, 170],
    [235, 215, 160],
    [105, 165, 245],
    [230, 125, 85],
    [220, 190, 150],
    [220, 205, 160],
    [145, 225, 230],
    [100, 140, 240],
];
pub struct SolarSystemScene {
    settings: SolarSystemSettings,
    orbit_points: Vec<Vec<(f64, f64)>>,
    last_jd: f64,
    palette: ScenePalette,
}
impl SolarSystemScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current
    // palette. When animations become plugins, the plugin constructor receives the
    // current palette and MUST follow it, and `Scene::set_palette` delivers later
    // changes. This scene follows it natively: the sun, planet and backdrop colours
    // are mapped onto the palette by brightness at draw time, so `PaletteScene`
    // skips its generic recolour (`follows_palette`).
    pub fn new(settings: &SolarSystemSettings, env: &SceneEnv) -> Self {
        let mut scene = Self {
            settings: settings.normalized(),
            orbit_points: Vec::new(),
            last_jd: 2451545.0,
            palette: env.palette.clone(),
        };
        scene.orbit_points = (0..8)
            .map(|index| {
                (0..=192)
                    .map(|sample| {
                        scene.project_position(
                            astro::solar_orbit_position(index, sample as f64 / 192.0),
                            1.0,
                            (0.0, 0.0),
                        )
                    })
                    .collect()
            })
            .collect();
        scene
    }
    fn radial_fraction(&self, distance: f64) -> f64 {
        let actual = distance / OUTER_AU;
        let compressed = 0.12 + 0.88 * distance.ln_1p() / OUTER_AU.ln_1p();
        let realism = f64::from(self.settings.distance_realism_percent) / 100.0;
        compressed * (1.0 - realism) + actual * realism
    }
    fn projected(&self, index: usize, jd: f64, extent: f64, center: (f64, f64)) -> (f64, f64) {
        self.project_position(astro::solar_heliocentric(index, jd), extent, center)
    }
    fn project_position(&self, position: [f64; 3], extent: f64, center: (f64, f64)) -> (f64, f64) {
        let radius = position[0].hypot(position[1]);
        let scale = extent * self.radial_fraction(radius) / radius.max(1e-12);
        (
            center.0 + position[0] * scale,
            center.1 - position[1] * scale,
        )
    }
    fn body_radius(&self, radius_km: f64, extent: f64) -> f64 {
        let actual = radius_km / AU_KM / OUTER_AU * extent;
        let exaggerated = if radius_km > SUN_RADIUS_KM * 0.5 {
            extent * 0.06
        } else {
            1.2 + 3.4 * (radius_km / RADII_KM[4]).sqrt()
        };
        let realism = f64::from(self.settings.size_realism_percent) / 100.0;
        (exaggerated * (1.0 - realism) + actual * realism).max(0.7)
    }
}
fn body(frame: &mut Frame<'_>, center: (f64, f64), radius: f64, color: [u8; 3]) {
    let left = (center.0 - radius - 1.0).max(0.0) as usize;
    let top = (center.1 - radius - 1.0).max(0.0) as usize;
    let right = ((center.0 + radius + 1.0).ceil().max(0.0) as usize).min(frame.raster.width);
    let bottom = ((center.1 + radius + 1.0).ceil().max(0.0) as usize).min(frame.raster.height);
    for y in top..bottom {
        for x in left..right {
            let distance = (x as f64 + 0.5 - center.0).hypot(y as f64 + 0.5 - center.1);
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0) as f32;
            if coverage <= 0.0 {
                continue;
            }
            // Opaque discs cover orbit paths beneath them.
            frame.raster.dots[y * frame.raster.width + x] = coverage;
            if let Some(slot) = frame.cell_color_mut((x / 2) as u16, (y / 4) as u16) {
                *slot = color;
            }
        }
    }
}
impl Scene for SolarSystemScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        frame.raster.dots.fill(0.0);
        frame.cell_colors.clear();
        frame.cell_colors.resize(
            usize::from(frame.width) * usize::from(frame.height),
            self.palette.recolor([90, 110, 150]),
        );
        let width = frame.raster.width as f64;
        let height = frame.raster.height as f64;
        if width < 2.0 || height < 2.0 {
            return;
        }
        let center = (width * 0.5, height * 0.5);
        let extent = (width.min(height) * 0.46 - 4.0).max(1.0);
        let jd = 2451545.0 + frame.time.as_secs_f64() * self.settings.days_per_second();
        self.last_jd = jd;
        if self.settings.orbit_paths {
            for (index, points) in self.orbit_points.iter().enumerate() {
                if !self.settings.visible_planets[index] {
                    continue;
                }
                for pair in points.windows(2) {
                    frame.raster.line(
                        (
                            (center.0 + pair[0].0 * extent) as f32 / width as f32,
                            (center.1 + pair[0].1 * extent) as f32 / height as f32,
                        ),
                        (
                            (center.0 + pair[1].0 * extent) as f32 / width as f32,
                            (center.1 + pair[1].1 * extent) as f32 / height as f32,
                        ),
                        0.3,
                        0.35,
                    );
                }
            }
        }
        body(
            frame,
            center,
            self.body_radius(SUN_RADIUS_KM, extent),
            self.palette.recolor([255, 220, 115]),
        );
        for (index, color) in COLORS.iter().enumerate() {
            if self.settings.visible_planets[index] {
                body(
                    frame,
                    self.projected(index, jd, extent, center),
                    self.body_radius(RADII_KM[index], extent),
                    self.palette.recolor(*color),
                );
            }
        }
    }
    fn set_palette(&mut self, palette: &ScenePalette) {
        self.palette = palette.clone();
    }
    fn follows_palette(&self) -> bool {
        true
    }
    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn frames_per_second(&self) -> u32 {
        30
    }
    fn status(&self) -> Option<String> {
        Some(
            if self.last_jd > 2469807.5 {
                "Approximate orbits extrapolated past 2050; one-dot body markers"
            } else {
                "JPL approximate orbits from J2000; minimum one-dot body markers"
            }
            .to_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::render_frame;
    use std::{path::PathBuf, time::Duration};
    #[test]
    fn all_planets_have_controls_and_persist_independently() {
        let mut settings = SolarSystemSettings::default();
        for id in settings::PLANET_IDS {
            assert!(settings
                .set_control(id, crate::ControlValue::Bool(false))
                .unwrap());
        }
        assert_eq!(settings.visible_planets, [false; 8]);
        let restored: SolarSystemSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
    }
    #[test]
    fn distance_realism_preserves_ratios_and_compression_spreads_inner_planets() {
        let env = SceneEnv::for_test(PathBuf::new(), crate::resources::test_resources());
        let real = SolarSystemScene::new(
            &SolarSystemSettings {
                distance_realism_percent: 100,
                ..Default::default()
            },
            &env,
        );
        let compact = SolarSystemScene::new(&SolarSystemSettings::default(), &env);
        assert!((real.radial_fraction(1.0) / real.radial_fraction(5.0) - 0.2).abs() < 1e-10);
        assert!(compact.radial_fraction(1.0) > real.radial_fraction(1.0) * 5.0);
    }
    #[test]
    fn hiding_planets_removes_them_without_rescaling_sun_and_visible_planets() {
        let env = SceneEnv::for_test(PathBuf::new(), crate::resources::test_resources());
        let mut all = SolarSystemScene::new(
            &SolarSystemSettings {
                orbit_paths: false,
                ..Default::default()
            },
            &env,
        );
        let mut none = SolarSystemScene::new(
            &SolarSystemSettings {
                visible_planets: [false; 8],
                orbit_paths: false,
                ..Default::default()
            },
            &env,
        );
        let all = render_frame(&mut all, 120, 50, Duration::ZERO);
        let none = render_frame(&mut none, 120, 50, Duration::ZERO);
        assert!(all.lit_dots() > none.lit_dots());
    }
    #[test]
    fn motion_uses_simulation_speed_and_small_frames_are_safe() {
        let env = SceneEnv::for_test(PathBuf::new(), crate::resources::test_resources());
        let scene = SolarSystemScene::new(&SolarSystemSettings::default(), &env);
        let first = scene.projected(0, 2451545.0, 100.0, (0.0, 0.0));
        let second = scene.projected(0, 2451545.0 + 22.0, 100.0, (0.0, 0.0));
        assert!((first.0 - second.0).hypot(first.1 - second.1) > 5.0);
        for (w, h) in [(0, 0), (1, 1), (10, 3)] {
            let mut scene = SolarSystemScene::new(&SolarSystemSettings::default(), &env);
            let out = render_frame(&mut scene, w, h, Duration::ZERO);
            assert!(out.raster.dots.iter().all(|value| value.is_finite()));
        }
    }
}

#[cfg(test)]
mod orbital_contract_tests {
    use super::*;
    #[test]
    fn all_eight_orbits_are_finite_closed_and_in_expected_distance_order() {
        let expected = [0.387, 0.723, 1.0, 1.524, 5.203, 9.537, 19.189, 30.070];
        for (index, axis) in expected.iter().enumerate() {
            let start = astro::solar_orbit_position(index, 0.0);
            let end = astro::solar_orbit_position(index, 1.0);
            assert!(start.iter().all(|value| value.is_finite()));
            for dimension in 0..3 {
                assert!((start[dimension] - end[dimension]).abs() < 1e-9);
            }
            let other = astro::solar_orbit_position(index, 0.5);
            let length = |position: [f64; 3]| {
                position
                    .iter()
                    .map(|value| value * value)
                    .sum::<f64>()
                    .sqrt()
            };
            assert!(((length(start) + length(other)) * 0.5 - axis).abs() < 0.001);
        }
    }
    #[test]
    fn size_realism_uses_physical_scale_with_minimum_visible_markers() {
        let env = SceneEnv::for_test(
            std::path::PathBuf::new(),
            crate::resources::test_resources(),
        );
        let real = SolarSystemScene::new(
            &SolarSystemSettings {
                size_realism_percent: 100,
                ..Default::default()
            },
            &env,
        );
        let compact = SolarSystemScene::new(&SolarSystemSettings::default(), &env);
        assert!(real.body_radius(SUN_RADIUS_KM, 100.0) < compact.body_radius(SUN_RADIUS_KM, 100.0));
        assert_eq!(real.body_radius(RADII_KM[0], 100.0), 0.7);
    }
}

#[cfg(test)]
mod speed_and_validation_tests {
    use super::*;
    use crate::{debug::render_frame, ControlValue};
    use std::{path::PathBuf, time::Duration};
    #[test]
    fn simulation_speed_changes_motion_with_the_same_frame_clock() {
        let env = SceneEnv::for_test(PathBuf::new(), crate::resources::test_resources());
        let mut slow = SolarSystemScene::new(
            &SolarSystemSettings {
                time_speed: 0,
                orbit_paths: false,
                ..Default::default()
            },
            &env,
        );
        let mut fast = SolarSystemScene::new(
            &SolarSystemSettings {
                time_speed: 5,
                orbit_paths: false,
                ..Default::default()
            },
            &env,
        );
        let slow = render_frame(&mut slow, 120, 50, Duration::from_secs(1));
        let fast = render_frame(&mut fast, 120, 50, Duration::from_secs(1));
        assert_ne!(slow.raster.dots, fast.raster.dots);
        assert!(fast.raster.dots.iter().all(|value| value.is_finite()));
    }
    #[test]
    fn malformed_controls_do_not_mutate_saved_values_and_normalization_bounds_ranges() {
        let mut settings = SolarSystemSettings::default();
        let original = settings.clone();
        assert!(settings
            .set_control("mercury", ControlValue::Number(3))
            .is_err());
        assert!(settings
            .set_control("time_speed", ControlValue::Index(usize::MAX))
            .is_err());
        assert_eq!(settings, original);
        let normalized = SolarSystemSettings {
            distance_realism_percent: -999,
            size_realism_percent: 999,
            time_speed: usize::MAX,
            ..original
        }
        .normalized();
        assert_eq!(normalized.distance_realism_percent, 0);
        assert_eq!(normalized.size_realism_percent, 100);
        assert_eq!(normalized.time_speed, 5);
    }
}

#[cfg(test)]
mod palette_tests {
    use super::*;
    use crate::debug::render_frame;
    use std::{path::PathBuf, time::Duration};
    fn colors(palette: Option<ScenePalette>) -> Vec<[u8; 3]> {
        let env = SceneEnv::for_test(PathBuf::new(), crate::resources::test_resources());
        let mut scene = SolarSystemScene::new(&SolarSystemSettings::default(), &env);
        if let Some(palette) = palette {
            scene.set_palette(&palette);
        }
        render_frame(&mut scene, 120, 50, Duration::ZERO).cell_colors
    }
    #[test]
    fn palette_changes_colours_and_none_is_unchanged() {
        let palette = ScenePalette {
            stops: vec![[10, 200, 20], [250, 30, 30]],
            ..Default::default()
        };
        let env = SceneEnv::for_test(PathBuf::new(), crate::resources::test_resources());
        assert!(SolarSystemScene::new(&SolarSystemSettings::default(), &env).follows_palette());
        let plain = colors(None);
        assert_eq!(plain, colors(Some(ScenePalette::default())));
        assert!(plain.contains(&[255, 220, 115]));
        assert_ne!(plain, colors(Some(palette)));
    }
}
