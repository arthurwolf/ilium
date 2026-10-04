use super::astro::{self, julian_date, parse_utc};
use super::settings::{Lens, Projection, StarStyle, StartFrom, TimeSpeed};
use super::*;
use crate::debug::{render_frame, Rendered};
use crate::location::GeoLocation;
use crate::raster::DitherMode;
use std::path::PathBuf;
use std::time::Duration;

fn env(latitude: f64, longitude: f64) -> SceneEnv {
    SceneEnv {
        resources: crate::resources::test_resources(),
        location: GeoLocation::new("test", latitude, longitude),
        cache_dir: PathBuf::from("/nonexistent/stars-never-used"),
        gpu: None,
        saved_runtime: std::sync::Arc::new(crate::minecraft::saved_runtime::SavedRuntime::new()),
        palette: Default::default(),
    }
}

fn fixed(text: &str) -> StarsSettings {
    StarsSettings {
        start_from: StartFrom::FixedTime,
        start_datetime: text.to_owned(),
        ..StarsSettings::default()
    }
}

/// Only stars: no decorations, mono dots, so every lit dot is a star.
fn bare(text: &str) -> StarsSettings {
    StarsSettings {
        star_style: StarStyle::Monochrome,
        milky_way: false,
        horizon: false,
        cardinal_marks: false,
        constellation_lines: false,
        moon: false,
        planets: false,
        ..fixed(text)
    }
}

fn draw(
    settings: &StarsSettings,
    latitude: f64,
    longitude: f64,
    width: u16,
    height: u16,
    seconds: u64,
) -> Rendered {
    let mut scene = StarsScene::new(settings, &env(latitude, longitude));
    render_frame(&mut scene, width, height, Duration::from_secs(seconds))
}

fn lit_at(rendered: &Rendered, x: f64, y: f64, radius: i64) -> bool {
    let (cx, cy) = (x.floor() as i64, y.floor() as i64);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let (px, py) = (cx + dx, cy + dy);
            if px < 0
                || py < 0
                || px as usize >= rendered.raster.width
                || py as usize >= rendered.raster.height
            {
                continue;
            }
            if rendered.raster.dots[py as usize * rendered.raster.width + px as usize] > 0.5 {
                return true;
            }
        }
    }
    false
}

/// Horizon direction of a J2000 position for the scene's own clock and place.
fn horizon_direction(text: &str, latitude: f64, longitude: f64, ra: f64, dec: f64) -> astro::Vec3 {
    let jd = julian_date(parse_utc(text).unwrap());
    let horizon = horizon_matrix(latitude, local_sidereal_degrees(jd, longitude));
    let matrix = mat_mul(&horizon, &precession_matrix(jd));
    mat_vec(&matrix, &astro::equatorial_vector(ra, dec))
}

const WHEN: &str = "2026-03-15 21:30";
const PARIS: (f64, f64) = (48.8566, 2.3522);

#[test]
fn rendering_is_deterministic_and_draws_stars() {
    let settings = fixed(WHEN);
    let first = draw(&settings, PARIS.0, PARIS.1, 100, 40, 0);
    let second = draw(&settings, PARIS.0, PARIS.1, 100, 40, 0);
    assert_eq!(first.raster.dots, second.raster.dots);
    assert_eq!(first.cell_colors, second.cell_colors);
    assert!(first.lit_dots() > 200, "{}", first.lit_dots());
    // Every lit cell of a coloured scene has a colour of its own, not only the tint.
    assert_eq!(first.cell_colors.len(), 100 * 40);
}

#[test]
fn dome_confines_the_sky_to_a_circle_and_draws_more_stars_with_a_deeper_limit() {
    let mut settings = StarsSettings {
        horizon: true,
        ..bare(WHEN)
    };
    let shallow = draw(&settings, PARIS.0, PARIS.1, 120, 48, 0);
    settings.magnitude_limit_tenths = 60;
    let deep = draw(&settings, PARIS.0, PARIS.1, 120, 48, 0);
    assert!(
        deep.lit_dots() > shallow.lit_dots() + 200,
        "{} vs {}",
        deep.lit_dots(),
        shallow.lit_dots()
    );
    let radius = 96.0 - 4.5;
    for y in 0..deep.raster.height {
        for x in 0..deep.raster.width {
            if deep.raster.dots[y * deep.raster.width + x] > 0.0 {
                let distance = (x as f64 + 0.5 - 120.0).hypot(y as f64 + 0.5 - 96.0);
                assert!(
                    distance <= radius + 1.5,
                    "dot outside the horizon ring at {x},{y}"
                );
            }
        }
    }
    // About half the sky is above the horizon: compare with the whole catalogue.
    let above = deep.lit_dots();
    let below_limit = catalog()
        .stars
        .iter()
        .filter(|s| s.magnitude <= 6.0)
        .count();
    assert!(
        above * 10 > below_limit * 3 && above < below_limit,
        "{above} of {below_limit}"
    );
}

#[test]
fn polaris_sits_at_the_expected_place_in_the_dome() {
    // Polaris is at altitude ~ latitude, due north: straight above the centre.
    let settings = StarsSettings {
        horizon: true,
        ..bare(WHEN)
    };
    let rendered = draw(&settings, PARIS.0, PARIS.1, 120, 48, 0);
    let view = View::new(
        &settings.normalized(),
        rendered.raster.width,
        rendered.raster.height,
    );
    let direction = horizon_direction(WHEN, PARIS.0, PARIS.1, 37.954_56, 89.264_1);
    let (altitude, azimuth) = astro::enu_to_altitude_azimuth(&direction);
    assert!((altitude - 48.86).abs() < 1.0, "{altitude}");
    assert!(!(1.5..=358.5).contains(&azimuth), "{azimuth}");
    let (x, y) = view.project(&direction).unwrap();
    // Stereographic: r = 2 tan(theta/2) * scale, scale = extent / 2.
    let extent = 96.0 - 4.5;
    let expected = 2.0 * ((90.0 - altitude).to_radians() / 2.0).tan() * extent / 2.0;
    assert!(((96.0 - y) - expected).abs() < 0.8, "{y} {expected}");
    assert!((x - 120.0).abs() < expected * 0.03 + 0.8, "{x}");
    assert!(lit_at(&rendered, x, y, 1), "Polaris dot missing at {x},{y}");
}

#[test]
fn every_bright_star_is_drawn_where_the_projection_says() {
    for (projection, lens) in [
        (Projection::Dome, Lens::Stereographic),
        (Projection::Dome, Lens::Equidistant),
        (Projection::Panorama, Lens::Stereographic),
        (Projection::Patch, Lens::Stereographic),
    ] {
        let settings = StarsSettings {
            projection,
            lens,
            look_azimuth_degrees: 200,
            look_altitude_degrees: 40,
            field_of_view_degrees: 120,
            magnitude_limit_tenths: 40,
            ..bare(WHEN)
        };
        let rendered = draw(&settings, -33.9, 151.2, 110, 44, 0);
        let view = View::new(
            &settings.normalized(),
            rendered.raster.width,
            rendered.raster.height,
        );
        let mut checked = 0;
        for star in catalog()
            .stars
            .iter()
            .take_while(|star| star.magnitude <= 4.0)
        {
            let vector = mat_vec(
                &mat_mul(
                    &horizon_matrix(
                        -33.9,
                        local_sidereal_degrees(julian_date(parse_utc(WHEN).unwrap()), 151.2),
                    ),
                    &precession_matrix(julian_date(parse_utc(WHEN).unwrap())),
                ),
                &star.vector,
            );
            if vector[2] <= 0.0 {
                continue;
            }
            let Some((x, y)) = view.project(&vector) else {
                continue;
            };
            if !view.contains(x, y, -1.0) {
                continue;
            }
            assert!(
                lit_at(&rendered, x, y, 0),
                "{projection:?} missing star at {x:.1},{y:.1}"
            );
            checked += 1;
        }
        assert!(checked > 20, "{projection:?}: only {checked} stars checked");
    }
}

#[test]
fn stars_below_the_horizon_are_never_drawn() {
    // Sirius is up at 21:00 in January but below the horizon twelve hours later.
    let settings = StarsSettings {
        magnitude_limit_tenths: 30,
        horizon: true,
        ..bare("2026-01-10 21:00")
    };
    let evening = draw(&settings, PARIS.0, PARIS.1, 120, 48, 0);
    let view = View::new(
        &settings.normalized(),
        evening.raster.width,
        evening.raster.height,
    );
    let up = horizon_direction("2026-01-10 21:00", PARIS.0, PARIS.1, 101.287_1, -16.716_1);
    assert!(
        up[2] > 0.2
            && lit_at(
                &evening,
                view.project(&up).unwrap().0,
                view.project(&up).unwrap().1,
                0
            )
    );
    let later = "2026-01-11 09:00";
    let down = horizon_direction(later, PARIS.0, PARIS.1, 101.287_1, -16.716_1);
    assert!(
        down[2] < -0.2,
        "Sirius is below the horizon at {later}: {}",
        down[2]
    );
    let morning = draw(
        &StarsSettings {
            start_datetime: later.to_owned(),
            ..settings.clone()
        },
        PARIS.0,
        PARIS.1,
        120,
        48,
        0,
    );
    // Where a horizon-ignoring dome would put it (mirrored through the horizon) nothing is lit.
    let mirrored = [down[0], down[1], -down[2]];
    let (x, y) = view.project(&mirrored).unwrap();
    assert!(!lit_at(&morning, x, y, 0));
    // No dot anywhere outside the horizon ring.
    let radius = 96.0 - 4.5;
    let outside = (0..morning.raster.width)
        .flat_map(|x| (0..morning.raster.height).map(move |y| (x, y)))
        .filter(|(x, y)| {
            (*x as f64 + 0.5 - 120.0).hypot(*y as f64 + 0.5 - 96.0) > radius + 1.0
                && morning.raster.dots[y * morning.raster.width + x] > 0.9
        })
        .count();
    assert_eq!(outside, 0);
}

#[test]
fn dome_puts_east_on_the_left_with_north_up() {
    // Regulus-like check with a synthetic equatorial star is not needed: use Sirius.
    let settings = StarsSettings {
        magnitude_limit_tenths: 30,
        ..bare("2026-01-10 21:00")
    };
    let rendered = draw(&settings, PARIS.0, PARIS.1, 120, 48, 0);
    let view = View::new(
        &settings.normalized(),
        rendered.raster.width,
        rendered.raster.height,
    );
    let sirius = horizon_direction("2026-01-10 21:00", PARIS.0, PARIS.1, 101.287_1, -16.716_1);
    let (altitude, azimuth) = astro::enu_to_altitude_azimuth(&sirius);
    assert!(
        altitude > 15.0 && (90.0..180.0).contains(&azimuth),
        "Sirius should be in the south-east: {altitude} {azimuth}"
    );
    let (x, y) = view.project(&sirius).unwrap();
    assert!(x < 120.0, "east half is on the left");
    assert!(y > 96.0, "southern half is at the bottom");
    assert!(lit_at(&rendered, x, y, 1));
}

#[test]
fn panorama_faces_the_chosen_bearing_and_zoom_moves_stars_apart() {
    let mut settings = StarsSettings {
        projection: Projection::Panorama,
        look_azimuth_degrees: 180,
        field_of_view_degrees: 120,
        ..bare("2026-01-10 21:00")
    };
    let wide = draw(&settings, PARIS.0, PARIS.1, 120, 40, 0);
    let view = View::new(
        &settings.normalized(),
        wide.raster.width,
        wide.raster.height,
    );
    let sirius = horizon_direction("2026-01-10 21:00", PARIS.0, PARIS.1, 101.287_1, -16.716_1);
    let (x, y) = view.project(&sirius).unwrap();
    assert!(
        x > 0.0 && x < 120.0,
        "south-east star is left of centre when facing south: {x}"
    );
    assert!(lit_at(&wide, x, y, 1));
    settings.field_of_view_degrees = 60;
    let narrow = draw(&settings, PARIS.0, PARIS.1, 120, 40, 0);
    assert!(
        narrow.lit_dots() < wide.lit_dots(),
        "zoomed in shows fewer stars"
    );
}

#[test]
fn sky_repeats_after_one_sidereal_day_but_not_after_one_solar_day() {
    let settings = bare(WHEN);
    // Real-time speed: wall time 86164.0905 s = one sidereal day.
    let mut scene = StarsScene::new(&settings, &env(PARIS.0, PARIS.1));
    let start = render_frame(&mut scene, 100, 40, Duration::ZERO);
    let sidereal = render_frame(&mut scene, 100, 40, Duration::from_secs(86_164));
    let solar = render_frame(&mut scene, 100, 40, Duration::from_secs(86_400));
    let overlap = |a: &Rendered, b: &Rendered| {
        a.raster
            .dots
            .iter()
            .zip(&b.raster.dots)
            .filter(|(p, q)| **p > 0.5 && **q > 0.5)
            .count() as f64
            / a.lit_dots().max(1) as f64
    };
    assert!(
        overlap(&start, &sidereal) > 0.93,
        "{}",
        overlap(&start, &sidereal)
    );
    assert!(overlap(&start, &solar) < 0.6, "{}", overlap(&start, &solar));
}

#[test]
fn time_acceleration_turns_the_sky_and_offset_shifts_it() {
    let mut fast = bare(WHEN);
    fast.time_speed = TimeSpeed::X3600;
    let a = draw(&fast, PARIS.0, PARIS.1, 100, 40, 0);
    let b = draw(&fast, PARIS.0, PARIS.1, 100, 40, 1);
    assert_ne!(
        a.raster.dots, b.raster.dots,
        "one second at x3600 is one hour of sky"
    );
    // x3600 for 1 s equals a 1 h offset at real speed.
    let mut offset = bare(WHEN);
    offset.time_offset_hours = 1;
    let shifted = draw(&offset, PARIS.0, PARIS.1, 100, 40, 0);
    assert_eq!(b.raster.dots, shifted.raster.dots);
    // Without acceleration one second changes nothing visible (0.004 degrees).
    let slow_a = draw(&bare(WHEN), PARIS.0, PARIS.1, 100, 40, 0);
    let slow_b = draw(&bare(WHEN), PARIS.0, PARIS.1, 100, 40, 1);
    let same = slow_a
        .raster
        .dots
        .iter()
        .zip(&slow_b.raster.dots)
        .filter(|(p, q)| p == q)
        .count();
    assert!(same as f64 > 0.99 * slow_a.raster.dots.len() as f64);
}

#[test]
fn live_clock_is_used_when_not_fixed_and_frame_time_advances_it() {
    let live = StarsSettings {
        star_style: StarStyle::Monochrome,
        ..StarsSettings::default()
    };
    let mut scene = StarsScene::new(&live, &env(PARIS.0, PARIS.1));
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000);
    assert_eq!(scene.sky_unix(0.0, now), 1_790_000_000.0);
    assert_eq!(
        scene.sky_unix(30.0, now),
        1_790_000_000.0,
        "x1 tracks Frame::now exactly"
    );
    scene.settings.time_speed = TimeSpeed::X60;
    assert_eq!(
        scene.sky_unix(30.0, now + Duration::from_secs(30)),
        1_790_000_000.0 + 30.0 + 30.0 * 59.0
    );
    scene.settings.time_offset_hours = -2;
    assert_eq!(scene.sky_unix(0.0, now), 1_790_000_000.0 - 7200.0);
    scene.fixed_start_unix = Some(1000.0);
    scene.settings.time_offset_hours = 0;
    assert_eq!(scene.sky_unix(10.0, now), 1000.0 + 600.0);
}

#[test]
fn location_changes_the_sky() {
    let settings = bare(WHEN);
    let north = draw(&settings, 60.0, 10.0, 100, 40, 0);
    let south = draw(&settings, -60.0, 10.0, 100, 40, 0);
    let overlap = north
        .raster
        .dots
        .iter()
        .zip(&south.raster.dots)
        .filter(|(p, q)| **p > 0.5 && **q > 0.5)
        .count();
    assert!((overlap as f64) < 0.3 * north.lit_dots() as f64);
}

#[test]
fn realistic_style_scales_stars_by_magnitude_and_tints_them() {
    let settings = StarsSettings {
        milky_way: false,
        horizon: false,
        cardinal_marks: false,
        constellation_lines: false,
        moon: false,
        planets: false,
        ..fixed("2026-01-10 21:00")
    };
    let rendered = draw(&settings, PARIS.0, PARIS.1, 120, 48, 0);
    let view = View::new(
        &settings.normalized(),
        rendered.raster.width,
        rendered.raster.height,
    );
    let sirius = horizon_direction("2026-01-10 21:00", PARIS.0, PARIS.1, 101.287_1, -16.716_1);
    let (x, y) = view.project(&sirius).unwrap();
    let (cx, cy) = (x.floor() as usize, y.floor() as usize);
    let width = rendered.raster.width;
    let block: usize = (cy - 1..=cy + 1)
        .flat_map(|row| (cx - 1..=cx + 1).map(move |column| (row, column)))
        .filter(|(row, column)| rendered.raster.dots[row * width + column] > 0.3)
        .count();
    assert_eq!(block, 9, "the brightest star is a 3x3 blob");
    // A faint star is a single dot with lower intensity than Sirius.
    let center = rendered.raster.dots[cy * width + cx];
    assert!(center > 0.99);
    let faint: Vec<f32> = rendered
        .raster
        .dots
        .iter()
        .copied()
        .filter(|d| *d > 0.0 && *d < 0.6)
        .collect();
    assert!(faint.len() > 50, "faint stars are dim");
    // Cell colours differ between blue-white and orange stars.
    let colors: std::collections::HashSet<[u8; 3]> = rendered.cell_colors.iter().copied().collect();
    assert!(colors.len() > 6, "{} distinct colours", colors.len());
    let mono = StarsSettings {
        star_style: StarStyle::Monochrome,
        ..settings.clone()
    };
    let mono_scene = StarsScene::new(&mono, &env(0.0, 0.0));
    assert!(!mono_scene.uses_cell_colors());
    let mut plain = settings.clone();
    plain.star_colors = false;
    assert!(!StarsScene::new(&plain, &env(0.0, 0.0)).uses_cell_colors());
    assert!(StarsScene::new(&settings, &env(0.0, 0.0)).uses_cell_colors());
}

#[test]
fn brightness_follows_magnitude_and_gamma() {
    let mut scene = StarsScene::new(&StarsSettings::default(), &env(0.0, 0.0));
    let (bright, mid, faint) = (
        scene.brightness(-1.4),
        scene.brightness(3.0),
        scene.brightness(5.0),
    );
    assert!(bright > 0.98 && bright > mid && mid > faint && (0.29..0.31).contains(&faint));
    scene.settings.brightness_gamma_percent = 200;
    assert!(
        scene.brightness(3.0) > mid,
        "higher gamma brightens faint stars"
    );
    scene.settings.brightness_gamma_percent = 50;
    assert!(scene.brightness(3.0) < mid);
}

#[test]
fn twinkle_is_subtle_deterministic_and_stronger_at_the_horizon() {
    let scene = StarsScene::new(&StarsSettings::default(), &env(0.0, 0.0));
    for index in [0, 5, 77, 4000] {
        for step in 0..50 {
            let t = f64::from(step) * 0.13;
            let high = scene.twinkle_factor(index, 0.95, t);
            let low = scene.twinkle_factor(index, 0.05, t);
            assert!((0.79..=1.0).contains(&high), "{high}");
            assert!((0.49..=1.0).contains(&low), "{low}");
            assert_eq!(high, scene.twinkle_factor(index, 0.95, t));
        }
    }
    let spread = |altitude: f64| {
        let values: Vec<f32> = (0..200)
            .map(|k| scene.twinkle_factor(9, altitude, f64::from(k) * 0.05))
            .collect();
        values.iter().copied().fold(0.0_f32, f32::max)
            - values.iter().copied().fold(1.0_f32, f32::min)
    };
    assert!(spread(0.05) > spread(0.95));
    let mut twinkling = bare(WHEN);
    twinkling.twinkle = true;
    let a = draw(&twinkling, PARIS.0, PARIS.1, 100, 40, 0);
    let b = draw(&twinkling, PARIS.0, PARIS.1, 100, 40, 1);
    assert_eq!(a.raster.dots.len(), b.raster.dots.len());
    assert!(
        a.raster
            .dots
            .iter()
            .zip(&b.raster.dots)
            .any(|(p, q)| p != q),
        "twinkle changes intensities over wall time"
    );
}

#[test]
fn milky_way_is_a_faint_band_that_follows_the_galactic_plane() {
    let mut settings = bare("2026-07-15 23:00");
    settings.milky_way = true;
    settings.magnitude_limit_tenths = 10;
    let rendered = draw(&settings, 20.0, -100.0, 120, 48, 0);
    let dots: Vec<f32> = rendered
        .raster
        .dots
        .iter()
        .copied()
        .filter(|d| *d > 0.0)
        .collect();
    assert!(dots.len() > 1500, "{} milky way dots", dots.len());
    assert!(dots.iter().all(|d| *d <= 0.5) || dots.iter().filter(|d| **d > 0.5).count() < 40);
    // Compare brightness on the plane with the galactic pole direction.
    let jd = julian_date(parse_utc("2026-07-15 23:00").unwrap());
    let matrix = mat_mul(
        &horizon_matrix(20.0, local_sidereal_degrees(jd, -100.0)),
        &precession_matrix(jd),
    );
    let (pole, centre) = galactic_axes();
    let (pole, centre) = (mat_vec(&matrix, &pole), mat_vec(&matrix, &centre));
    let third = astro::cross(&pole, &centre);
    let on_plane = milky_way_intensity(&centre, &pole, &centre, &third);
    let anticentre = milky_way_intensity(
        &[-centre[0], -centre[1], -centre[2]],
        &pole,
        &centre,
        &third,
    );
    assert!(
        on_plane > 0.02 && on_plane > anticentre,
        "{on_plane} vs {anticentre}"
    );
    assert_eq!(milky_way_intensity(&pole, &pole, &centre, &third), 0.0);
}

#[test]
fn constellation_lines_horizon_and_compass_add_dots() {
    let plain = bare(WHEN);
    let base = draw(&plain, PARIS.0, PARIS.1, 100, 40, 0);
    for (name, mutate) in [
        (
            "lines",
            (|s: &mut StarsSettings| s.constellation_lines = true) as fn(&mut StarsSettings),
        ),
        ("horizon", |s| s.horizon = true),
        ("compass", |s| s.cardinal_marks = true),
    ] {
        let mut settings = plain.clone();
        mutate(&mut settings);
        let more = draw(&settings, PARIS.0, PARIS.1, 100, 40, 0);
        let count = |r: &Rendered| r.raster.dots.iter().filter(|d| **d > 0.0).count();
        assert!(
            count(&more) > count(&base) + 15,
            "{name}: {} vs {}",
            count(&more),
            count(&base)
        );
    }
}

#[test]
fn horizon_ring_lies_on_the_circle() {
    let mut settings = bare(WHEN);
    settings.horizon = true;
    settings.magnitude_limit_tenths = 10;
    let rendered = draw(&settings, PARIS.0, PARIS.1, 120, 48, 0);
    let radius = 96.0 - 4.5;
    let mut ring = 0;
    for y in 0..rendered.raster.height {
        for x in 0..rendered.raster.width {
            if rendered.raster.dots[y * rendered.raster.width + x] >= 0.5 {
                let distance = (x as f64 + 0.5 - 120.0).hypot(y as f64 + 0.5 - 96.0);
                if (distance - radius).abs() < 1.2 {
                    ring += 1;
                }
            }
        }
    }
    assert!(ring > 300, "{ring}");
}

#[test]
fn moon_phase_is_drawn_lit_when_full_and_dark_when_new() {
    let moon_only = |text: &str| StarsSettings {
        magnitude_limit_tenths: 10,
        star_style: StarStyle::Monochrome,
        milky_way: false,
        horizon: false,
        cardinal_marks: false,
        constellation_lines: false,
        planets: false,
        ..fixed(text)
    };
    let moon_dots = |text: &str, latitude: f64, longitude: f64| -> usize {
        let settings = moon_only(text);
        let with = draw(&settings, latitude, longitude, 100, 40, 0);
        let mut without = settings.clone();
        without.moon = false;
        let baseline = draw(&without, latitude, longitude, 100, 40, 0);
        with.raster
            .dots
            .iter()
            .zip(&baseline.raster.dots)
            .filter(|(a, b)| **a > 0.5 && **b <= 0.5)
            .count()
    };
    // Full moon 2024-01-25 17:54 UTC: high for an observer near 90 degrees east.
    let full = moon_dots("2024-01-25 17:54", 10.0, 92.0);
    // New moon 2024-01-11 11:57 UTC: high for an observer near longitude 0, south.
    let new = moon_dots("2024-01-11 11:57", -21.0, 0.0);
    // First quarter 2024-01-18 03:53 UTC: half lit, seen from longitude ~ -60 at dusk-ish.
    assert!(full >= 15, "full moon lit dots {full}");
    assert!(new <= full / 3, "new moon {new} vs full {full}");
}

#[test]
fn planets_appear_where_the_ephemeris_puts_them() {
    // Jupiter at opposition (2023-11-03) is up at local midnight; observer at 0 E.
    let text = "2023-11-04 00:00";
    let settings = StarsSettings {
        star_style: StarStyle::Monochrome,
        magnitude_limit_tenths: 10,
        milky_way: false,
        horizon: false,
        cardinal_marks: false,
        constellation_lines: false,
        moon: false,
        ..fixed(text)
    };
    let (latitude, longitude) = (35.0, 0.0);
    let rendered = draw(&settings, latitude, longitude, 120, 48, 0);
    let view = View::new(
        &settings.normalized(),
        rendered.raster.width,
        rendered.raster.height,
    );
    let jd = julian_date(parse_utc(text).unwrap());
    let matrix = mat_mul(
        &horizon_matrix(latitude, local_sidereal_degrees(jd, longitude)),
        &precession_matrix(jd),
    );
    let mut seen = 0;
    for planet in [Planet::Jupiter, Planet::Mars, Planet::Saturn, Planet::Venus] {
        let direction = mat_vec(&matrix, &planet_sight(planet, jd).direction);
        if direction[2] > 0.1 {
            let (x, y) = view.project(&direction).unwrap();
            assert!(
                lit_at(&rendered, x, y, 1),
                "{} missing at {x:.1},{y:.1}",
                planet.name()
            );
            seen += 1;
        }
    }
    assert!(
        seen >= 2,
        "expected at least two planets above the horizon, saw {seen}"
    );
    let mut off = settings.clone();
    off.planets = false;
    assert!(draw(&off, latitude, longitude, 120, 48, 0).lit_dots() < rendered.lit_dots());
}

#[test]
fn brightest_planets_are_bigger_than_faint_stars() {
    // Jupiter (magnitude -2.9 at opposition) is drawn as a 13 dot disc.
    let text = "2023-11-04 00:00";
    let settings = StarsSettings {
        magnitude_limit_tenths: 10,
        milky_way: false,
        horizon: false,
        cardinal_marks: false,
        constellation_lines: false,
        moon: false,
        ..fixed(text)
    };
    let with_planets = draw(&settings, 35.0, 0.0, 120, 48, 0);
    let mut none = settings.clone();
    none.planets = false;
    let without = draw(&none, 35.0, 0.0, 120, 48, 0);
    let extra = with_planets.lit_dots() as i64 - without.lit_dots() as i64;
    assert!(extra >= 13, "{extra}");
}

#[test]
fn status_reports_simulated_time_and_bad_input_only() {
    let live = StarsScene::new(&StarsSettings::default(), &env(0.0, 0.0));
    assert_eq!(live.status(), None);
    let mut simulated = StarsScene::new(&fixed("2026-12-21 22:00"), &env(0.0, 0.0));
    simulated.render(&mut {
        // A frame is needed to know the sky time.
        let mut raster = Raster::default();
        raster.resize(40, 16);
        let colors: &'static mut Vec<Rgb> = Box::leak(Box::new(vec![[0; 3]; 100]));
        let raster: &'static mut Raster = Box::leak(Box::new(raster));
        Frame {
            raster,
            cell_colors: colors,
            width: 20,
            height: 4,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: SystemTime::UNIX_EPOCH,
        }
    });
    assert_eq!(
        simulated.status().as_deref(),
        Some("Sky at 2026-12-21 22:00:00 UTC")
    );
    let bad = StarsScene::new(&fixed("not a date"), &env(0.0, 0.0));
    assert!(bad.status().unwrap().contains("not understood"));
}

#[test]
fn frame_rate_matches_how_fast_the_sky_moves() {
    let mut settings = StarsSettings::default();
    let fps =
        |settings: &StarsSettings| StarsScene::new(settings, &env(0.0, 0.0)).frames_per_second();
    assert_eq!(fps(&settings), 1);
    settings.time_speed = TimeSpeed::X86400;
    assert_eq!(fps(&settings), 12);
    settings.time_speed = TimeSpeed::X1;
    settings.twinkle = true;
    assert_eq!(fps(&settings), 12);
}

#[test]
fn small_panels_and_extreme_locations_do_not_panic() {
    for (latitude, longitude) in [(90.0, 0.0), (-90.0, 180.0), (0.0, -180.0), (89.999, 45.0)] {
        for projection in Projection::ALL {
            for (width, height) in [(1, 1), (2, 1), (5, 3), (40, 3), (3, 30)] {
                let settings = StarsSettings {
                    projection: *projection,
                    twinkle: true,
                    ..fixed(WHEN)
                };
                let rendered = draw(&settings, latitude, longitude, width, height, 3);
                assert_eq!(
                    rendered.raster.dots.len(),
                    usize::from(width) * usize::from(height) * 8
                );
                assert!(rendered
                    .raster
                    .dots
                    .iter()
                    .all(|d| d.is_finite() && (0.0..=1.0).contains(d)));
            }
        }
    }
    let empty = draw(&StarsSettings::default(), 10.0, 10.0, 0, 0, 0);
    assert!(empty.raster.dots.is_empty());
}

#[test]
fn scene_owns_no_worker_and_drops_instantly() {
    let scene = StarsScene::new(&StarsSettings::default(), &env(0.0, 0.0));
    let started = std::time::Instant::now();
    drop(scene);
    assert!(started.elapsed() < Duration::from_millis(50));
}

#[test]
fn registry_builds_the_scene_and_it_is_deterministic_for_a_fixed_clock() {
    use crate::registry::{AmbientKind, AmbientSettings};
    let settings = AmbientSettings {
        stars: fixed(WHEN),
        ..AmbientSettings::default()
    };
    let mut scene = settings.create_scene(
        AmbientKind::Stars,
        &SceneEnv::for_test(
            PathBuf::from("/nonexistent"),
            crate::resources::test_resources(),
        ),
    );
    let a = render_frame(scene.as_mut(), 80, 30, Duration::ZERO);
    assert!(a.lit_dots() > 100);
    assert!(scene.status().is_some());
    assert!(scene.uses_cell_colors());
}

/// Visual check: `cargo test -p ilium-ambient sky_preview -- --ignored --nocapture`
#[test]
#[ignore = "prints a preview for a human"]
fn sky_preview() {
    let show = |title: &str,
                settings: &StarsSettings,
                latitude: f64,
                longitude: f64,
                width: u16,
                height: u16,
                density: u16| {
        let rendered = draw(settings, latitude, longitude, width, height, 0);
        println!("=== {title} ({} lit dots)", rendered.lit_dots());
        for line in rendered.braille_lines(density, DitherMode::Ordered) {
            println!("{line}");
        }
    };
    let base = fixed("2026-01-10 21:00");
    show(
        "dome, Paris, realistic",
        &base,
        PARIS.0,
        PARIS.1,
        60,
        24,
        100,
    );
    show(
        "dome, Paris, density 60",
        &base,
        PARIS.0,
        PARIS.1,
        60,
        24,
        60,
    );
    let mono = StarsSettings {
        star_style: StarStyle::Monochrome,
        ..base.clone()
    };
    show("dome, mono", &mono, PARIS.0, PARIS.1, 60, 24, 100);
    let pano = StarsSettings {
        projection: Projection::Panorama,
        look_azimuth_degrees: 180,
        ..base.clone()
    };
    show("panorama south", &pano, PARIS.0, PARIS.1, 100, 20, 100);
    let patch = StarsSettings {
        projection: Projection::Patch,
        look_azimuth_degrees: 200,
        look_altitude_degrees: 40,
        field_of_view_degrees: 60,
        ..base.clone()
    };
    show("patch 60 deg", &patch, PARIS.0, PARIS.1, 80, 24, 100);
    let wide = StarsSettings {
        magnitude_limit_tenths: 50,
        ..base
    };
    show("wide dome 120x30", &wide, -33.9, 151.2, 120, 30, 100);
    let milky = StarsSettings {
        magnitude_limit_tenths: 10,
        constellation_lines: false,
        planets: false,
        moon: false,
        ..fixed("2026-07-15 23:00")
    };
    show(
        "milky way only, July, lat 40",
        &milky,
        40.0,
        0.0,
        70,
        26,
        100,
    );
    let panorama_milky = StarsSettings {
        projection: Projection::Panorama,
        look_azimuth_degrees: 180,
        ..milky
    };
    show(
        "milky way panorama south",
        &panorama_milky,
        40.0,
        0.0,
        100,
        18,
        100,
    );
    // Moon close-ups: aim a 12 degree patch at the Moon for several phases.
    for (label, when, latitude, longitude) in [
        ("full moon", "2024-01-25 17:54", 10.0, 92.0),
        ("first quarter", "2024-01-18 03:53", 20.0, -148.0),
        ("waxing crescent", "2024-01-14 03:00", 20.0, -148.0),
        ("new moon", "2024-01-11 11:57", -21.0, 0.0),
    ] {
        let jd = julian_date(parse_utc(when).unwrap());
        let horizon = horizon_matrix(latitude, local_sidereal_degrees(jd, longitude));
        let enu = mat_vec(&horizon, &moon_sight(jd).direction);
        let (altitude, azimuth) = astro::enu_to_altitude_azimuth(&enu);
        let close = StarsSettings {
            projection: Projection::Patch,
            look_azimuth_degrees: azimuth.round() as i32,
            look_altitude_degrees: altitude.round().clamp(0.0, 90.0) as i32,
            field_of_view_degrees: 14,
            horizon: false,
            cardinal_marks: false,
            magnitude_limit_tenths: 65,
            ..fixed(when)
        };
        show(
            &format!("{label}, alt {altitude:.0} az {azimuth:.0}"),
            &close,
            latitude,
            longitude,
            40,
            10,
            100,
        );
    }
    let planets = StarsSettings {
        projection: Projection::Panorama,
        look_azimuth_degrees: 180,
        field_of_view_degrees: 150,
        ..fixed("2023-11-04 00:00")
    };
    show(
        "panorama south with Jupiter and Saturn, 2023-11-04",
        &planets,
        35.0,
        0.0,
        100,
        20,
        100,
    );
}

/// Timing probe: `cargo test --release -p ilium-ambient frame_time -- --ignored --nocapture`
#[test]
#[ignore = "timing probe"]
fn frame_time() {
    for (width, height) in [(80u16, 24u16), (200, 60), (300, 80)] {
        for projection in Projection::ALL {
            let settings = StarsSettings {
                projection: *projection,
                twinkle: true,
                ..fixed(WHEN)
            };
            let mut scene = StarsScene::new(&settings, &env(PARIS.0, PARIS.1));
            let started = std::time::Instant::now();
            let runs = 20;
            for run in 0..runs {
                render_frame(&mut scene, width, height, Duration::from_secs(run));
            }
            println!(
                "{projection:?} {width}x{height}: {:?} per frame",
                started.elapsed() / runs as u32
            );
        }
    }
}

#[test]
fn milky_way_layer_is_cached_between_nearby_frames_and_rebuilt_on_change() {
    let mut settings = bare(WHEN);
    settings.milky_way = true;
    let mut scene = StarsScene::new(&settings, &env(PARIS.0, PARIS.1));
    let first = render_frame(&mut scene, 80, 30, Duration::from_secs(0));
    assert_eq!(scene.milky_way_builds, 1);
    let second = render_frame(&mut scene, 80, 30, Duration::from_secs(5));
    assert_eq!(
        scene.milky_way_builds, 1,
        "same 20 s bucket reuses the layer"
    );
    assert!(first.lit_dots() > 0 && second.lit_dots() > 0);
    render_frame(&mut scene, 80, 30, Duration::from_secs(45));
    assert_eq!(scene.milky_way_builds, 2, "the sky has turned enough");
    render_frame(&mut scene, 60, 20, Duration::from_secs(45));
    assert_eq!(scene.milky_way_builds, 3, "panel resized");
    // Cached output is identical to a fresh build at the same instant.
    let cached = render_frame(&mut scene, 60, 20, Duration::from_secs(47));
    let mut fresh_scene = StarsScene::new(&settings, &env(PARIS.0, PARIS.1));
    let fresh = render_frame(&mut fresh_scene, 60, 20, Duration::from_secs(45));
    let differing = cached
        .raster
        .dots
        .iter()
        .zip(&fresh.raster.dots)
        .filter(|(a, b)| (**a - **b).abs() > 1e-6)
        .count();
    assert!(
        differing < 14,
        "{differing} dots differ only through 2 s of star motion"
    );
}

#[test]
fn horizonless_panorama_draws_catalogue_stars_below_ground() {
    let settings = StarsSettings {
        projection: Projection::Panorama,
        field_of_view_degrees: 180,
        ..bare(WHEN)
    };
    let mut scene = StarsScene::new(&settings, &env(PARIS.0, PARIS.1));
    let rendered = render_frame(&mut scene, 120, 48, Duration::ZERO);
    let jd = julian_date(parse_utc(WHEN).unwrap());
    let matrix = mat_mul(
        &horizon_matrix(PARIS.0, local_sidereal_degrees(jd, PARIS.1)),
        &precession_matrix(jd),
    );
    let mut below = 0;
    for (index, star) in scene.catalog.stars.iter().enumerate() {
        if star.magnitude > scene.magnitude_limit() {
            continue;
        }
        if mat_vec(&matrix, &star.vector)[2] >= -0.1 {
            continue;
        }
        if let Some((x, y)) = scene.projected[index] {
            if x >= 1.0
                && x < rendered.raster.width as f64 - 1.0
                && y >= 1.0
                && y < rendered.raster.height as f64 - 1.0
            {
                assert!(lit_at(&rendered, x, y, 0));
                below += 1;
            }
        }
    }
    assert!(below > 20, "only {below} below-horizon stars");
}

#[test]
fn milky_way_below_horizon_is_retained_only_in_fullscreen_mode() {
    let settings = StarsSettings {
        projection: Projection::Panorama,
        horizon: false,
        ..StarsSettings::default()
    };
    let view = View::new(&settings, 160, 96);
    let full = build_milky_way(&view, &IDENTITY, 160, 96, false);
    let clipped = build_milky_way(&view, &IDENTITY, 160, 96, true);
    assert!(full.len() > clipped.len());
    assert!(full.iter().any(|(_, y, _)| *y > 50));
    assert!(clipped.iter().all(|(_, y, _)| *y < 48));
}

#[test]
fn simulated_satellites_are_optional_persisted_and_raise_cadence() {
    let mut settings = StarsSettings::default();
    assert!(!settings.satellites);
    assert!(settings
        .set_control("satellites", crate::ControlValue::Bool(true))
        .unwrap());
    let restored: StarsSettings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert!(restored.satellites);
    let scene = StarsScene::new(&settings, &env(0.0, 0.0));
    assert_eq!(scene.frames_per_second(), 12);
    let control = settings
        .controls()
        .into_iter()
        .find(|row| row.id == "satellites")
        .unwrap();
    assert!(control.label.contains("Simulated"));
}

#[test]
fn below_horizon_moon_and_planets_can_be_seen_in_fullscreen_panorama() {
    let latitude = PARIS.0;
    let longitude = PARIS.1;
    let mut checked_moon = false;
    let mut checked_planet = false;
    for day in 1..=28 {
        let text = format!("2026-01-{day:02} 12:00");
        let jd = julian_date(parse_utc(&text).unwrap());
        let horizon = horizon_matrix(latitude, local_sidereal_degrees(jd, longitude));
        let to_horizon = mat_mul(&horizon, &precession_matrix(jd));
        let moon = moon_sight(jd);
        let geocentric = mat_vec(&horizon, &moon.direction);
        let moon_direction = normalize([
            geocentric[0],
            geocentric[1],
            geocentric[2] - moon.horizontal_parallax_deg.to_radians().sin(),
        ]);
        let planet_direction = mat_vec(&to_horizon, &planet_sight(Planet::Jupiter, jd).direction);
        for (is_moon, direction) in [(true, moon_direction), (false, planet_direction)] {
            if !(direction[2] < -0.1 && direction[2] > -0.8) {
                continue;
            }
            let (_, azimuth) = astro::enu_to_altitude_azimuth(&direction);
            let settings = StarsSettings {
                projection: Projection::Panorama,
                look_azimuth_degrees: azimuth.round() as i32,
                moon: is_moon,
                planets: !is_moon,
                ..bare(&text)
            };
            let rendered = draw(&settings, latitude, longitude, 120, 48, 0);
            let view = View::new(&settings, rendered.raster.width, rendered.raster.height);
            let (x, y) = view.project(&direction).unwrap();
            assert!(
                lit_at(&rendered, x, y, 5),
                "below-horizon body missing: moon={is_moon} day={day}"
            );
            if is_moon {
                checked_moon = true;
            } else {
                checked_planet = true;
            }
        }
        if checked_moon && checked_planet {
            break;
        }
    }
    assert!(checked_moon && checked_planet);
}

#[test]
fn palette_changes_cell_colours_and_none_is_unchanged() {
    let settings = StarsSettings {
        star_style: StarStyle::Realistic,
        star_colors: true,
        ..fixed("2024-03-01T22:00:00Z")
    };
    let colors = |palette: Option<crate::style::ScenePalette>| {
        let mut scene = StarsScene::new(&settings, &env(48.0, 2.0));
        if let Some(palette) = palette {
            scene.set_palette(&palette);
        }
        assert!(scene.follows_palette());
        render_frame(&mut scene, 100, 40, Duration::from_secs(1)).cell_colors
    };
    let plain = colors(None);
    assert!(!plain.is_empty());
    assert_eq!(plain, colors(Some(Default::default())));
    let palette = crate::style::ScenePalette {
        stops: vec![[10, 200, 20], [250, 30, 30]],
        ..Default::default()
    };
    assert_ne!(plain, colors(Some(palette)));
}
