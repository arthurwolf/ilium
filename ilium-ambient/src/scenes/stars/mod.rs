//! "Stars overhead": the sky above the shared observer location, exactly as it
//! is at the simulated time, as if there were no Sun, no atmosphere and no
//! light pollution. Yale Bright Star Catalogue positions are rotated into the
//! local horizon frame with real sidereal time (see `astro`), optionally with
//! the Moon and the five naked-eye planets, the Milky Way and constellation
//! figures, and drawn as Braille dots.
//!
//! The scene is a pure function of `(settings, location, Frame)`: it owns no
//! threads, does no I/O and reads the clock only through `Frame`.

pub(crate) mod astro;
mod canvas;
pub(crate) mod catalog;
mod settings;
mod view;

pub use settings::StarsSettings;

use crate::style::ScenePalette;
use astro::{
    galactic_axes, horizon_matrix, julian_date, local_sidereal_degrees, mat_mul, mat_vec,
    moon_sight, normalize, planet_sight, precession_matrix, sun_direction, Mat3, Planet, Vec3,
    IDENTITY,
};
use canvas::{Canvas, Rgb, BLOB, DISC, PLUS, SINGLE};
use catalog::{catalog, color_of_index, Catalog};
use settings::{StarSize, StarStyle, StartFrom};
use view::View;

use crate::control::SceneSettings;
use crate::raster::{hash, smoothstep, Raster};
use crate::scene::{Frame, Scene, SceneEnv};
use std::time::{SystemTime, UNIX_EPOCH};

/// Constellation lines reach this many magnitudes past the magnitude limit,
/// so a figure is not cut off at its faintest corner.
const LINE_MAGNITUDE_SLACK: f32 = 0.6;
const STAR_LINE_INTENSITY: f32 = 0.42;
const HORIZON_INTENSITY: f32 = 0.55;
const NEUTRAL_TINT: Rgb = [205, 214, 238];
const LINE_TINT: Rgb = [92, 128, 190];
const HORIZON_TINT: Rgb = [110, 150, 175];
const COMPASS_TINT: Rgb = [235, 150, 120];
const MILKY_WAY_TINT: Rgb = [165, 175, 215];
const MOON_TINT: Rgb = [238, 234, 214];
/// The real Moon is 0.5 degrees wide, under one dot; it is drawn at least this
/// many dots in radius so that its phase can be read.
const MOON_MIN_RADIUS: f64 = 3.4;

/// The Milky Way layer is rebuilt when the panel size changes or the sky has
/// turned by about a tenth of a degree (this many simulated seconds); in
/// between the cached sparse layer is replayed. It is the costliest layer.
const MILKY_WAY_REBUILD_SECONDS: f64 = 20.0;

#[derive(Default)]
struct MilkyWayLayer {
    key: Option<(usize, usize, i64, bool)>,
    /// Lit dots as (x, y, intensity).
    dots: Vec<(u32, u32, f32)>,
}

pub struct StarsScene {
    settings: StarsSettings,
    latitude: f64,
    longitude: f64,
    fixed_start_unix: Option<f64>,
    catalog: &'static Catalog,
    star_colors: Vec<Rgb>,
    palette: ScenePalette,
    /// Panel position of each catalogue star this frame (None: not visible).
    projected: Vec<Option<(f64, f64)>>,
    strongest: Vec<f32>,
    last_sky_unix: Option<f64>,
    milky_way: MilkyWayLayer,
    #[cfg(test)]
    milky_way_builds: usize,
}

impl StarsScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current
    // palette. When animations become plugins, the plugin constructor receives the
    // current palette and MUST follow it, and `Scene::set_palette` delivers later
    // changes. This scene follows it natively: the catalogue star colours (cached,
    // rebuilt on `set_palette`) and every tint are mapped onto the palette by
    // brightness where they are drawn, so `PaletteScene` skips its generic recolour
    // (`follows_palette`).
    pub fn new(settings: &StarsSettings, env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        let location = env.location.normalized();
        let catalog = catalog();
        Self {
            fixed_start_unix: settings.fixed_start_unix(),
            star_colors: star_colors(catalog, &env.palette),
            palette: env.palette.clone(),
            projected: vec![None; catalog.stars.len()],
            strongest: Vec::new(),
            latitude: location.latitude,
            longitude: location.longitude,
            catalog,
            settings,
            last_sky_unix: None,
            milky_way: MilkyWayLayer::default(),
            #[cfg(test)]
            milky_way_builds: 0,
        }
    }

    /// Simulated UTC time in Unix seconds for a frame. At real speed and no
    /// offset this is exactly `frame.now`.
    fn sky_unix(&self, wall_seconds: f64, now: SystemTime) -> f64 {
        let factor = self.settings.time_speed.factor();
        let now_unix = now.duration_since(UNIX_EPOCH).map_or_else(
            |before| -before.duration().as_secs_f64(),
            |after| after.as_secs_f64(),
        );
        let base = match self.fixed_start_unix {
            Some(start) => start + wall_seconds * factor,
            None => now_unix + wall_seconds * (factor - 1.0),
        };
        base + f64::from(self.settings.time_offset_hours) * 3600.0
    }

    fn magnitude_limit(&self) -> f32 {
        self.settings.magnitude_limit_tenths as f32 / 10.0
    }

    fn is_realistic(&self) -> bool {
        self.settings.star_style == StarStyle::Realistic
    }

    /// Brightness 0..1 of a star of the given magnitude: faint stars sit near
    /// 0.3 so that the dither thins them out, the brightest reach 1.
    fn brightness(&self, magnitude: f32) -> f32 {
        let limit = self.magnitude_limit();
        let fraction = ((limit - magnitude) / (limit + 1.5)).clamp(0.0, 1.0);
        let base = 0.30 + 0.70 * fraction;
        base.powf(100.0 / self.settings.brightness_gamma_percent as f32)
    }

    fn size_bias(&self) -> f32 {
        match self.settings.star_size {
            StarSize::Small => 0.8,
            StarSize::Normal => 0.0,
            StarSize::Large => -0.8,
        }
    }

    fn twinkle_factor(&self, index: usize, altitude_sine: f64, wall_seconds: f64) -> f32 {
        let phase = hash(index as i32, 11);
        let speed = 0.6 + 1.6 * hash(index as i32, 23);
        let wave =
            ((wall_seconds as f32 * speed + phase) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let horizon = 1.0 - smoothstep(0.0, 0.7, altitude_sine as f32);
        let amplitude = 0.16 + 0.34 * horizon;
        1.0 - amplitude * wave
    }

    fn draw_milky_way(
        &mut self,
        canvas: &mut Canvas<'_>,
        view: &View,
        to_horizon_j2000: &Mat3,
        unix: f64,
    ) {
        let key = (
            canvas.width(),
            canvas.height(),
            (unix / MILKY_WAY_REBUILD_SECONDS).floor() as i64,
            self.settings.horizon,
        );
        if self.milky_way.key != Some(key) {
            #[cfg(test)]
            {
                self.milky_way_builds += 1;
            }
            self.milky_way.dots =
                build_milky_way(view, to_horizon_j2000, key.0, key.1, self.settings.horizon);
            self.milky_way.key = Some(key);
        }
        for (x, y, intensity) in &self.milky_way.dots {
            canvas.plot(
                i64::from(*x),
                i64::from(*y),
                *intensity,
                self.palette.recolor(MILKY_WAY_TINT),
            );
        }
    }

    fn draw_horizon(&self, canvas: &mut Canvas<'_>, view: &View) {
        let step = 1.5;
        let count = (360.0 / step) as usize;
        let max_jump = canvas.width() as f64 / 2.0;
        let mut previous: Option<(f64, f64)> = None;
        for index in 0..=count {
            let azimuth = f64::from(index as u32) * step;
            let point = view.project(&astro::altitude_azimuth_vector(0.0, azimuth));
            if let (Some(a), Some(b)) = (previous, point) {
                if (a.0 - b.0).abs() < max_jump {
                    canvas.line(a, b, HORIZON_INTENSITY, self.palette.recolor(HORIZON_TINT));
                }
            }
            previous = point;
        }
    }

    fn draw_compass(&self, canvas: &mut Canvas<'_>, view: &View) {
        for (azimuth, length) in [(0.0, 7.0), (90.0, 4.0), (180.0, 4.0), (270.0, 4.0)] {
            let Some(base) = view.project(&astro::altitude_azimuth_vector(0.0, azimuth)) else {
                continue;
            };
            let Some((dx, dy)) = view.inward_direction(azimuth) else {
                continue;
            };
            let tip = (base.0 + dx * length, base.1 + dy * length);
            canvas.line(base, tip, 0.95, self.palette.recolor(COMPASS_TINT));
            if azimuth == 0.0 {
                // North marker: a small blob just beyond the horizon.
                canvas.cluster(
                    base.0 - dx * 3.0,
                    base.1 - dy * 3.0,
                    PLUS,
                    1.0,
                    self.palette.recolor(COMPASS_TINT),
                );
            }
        }
    }

    fn draw_stars(
        &mut self,
        canvas: &mut Canvas<'_>,
        view: &View,
        to_horizon_j2000: &Mat3,
        wall_seconds: f64,
    ) {
        let limit = self.magnitude_limit();
        let line_limit = limit + LINE_MAGNITUDE_SLACK;
        let realistic = self.is_realistic();
        let colored = realistic && self.settings.star_colors;
        let bias = self.size_bias();
        self.projected.iter_mut().for_each(|slot| *slot = None);
        let mut draw_list: Vec<(usize, f64, f64, f32)> = Vec::new();
        for (index, star) in self.catalog.stars.iter().enumerate() {
            if star.magnitude > line_limit {
                break;
            }
            let direction = mat_vec(to_horizon_j2000, &star.vector);
            if self.settings.horizon && direction[2] <= 0.0 {
                continue;
            }
            let Some((x, y)) = view.project(&direction) else {
                continue;
            };
            self.projected[index] = Some((x, y));
            if star.magnitude <= limit && view.contains(x, y, 0.0) {
                draw_list.push((index, x, y, direction[2] as f32));
            }
        }
        if self.settings.constellation_lines {
            let max_jump = canvas.width() as f64 / 2.0;
            for (a, b) in &self.catalog.lines {
                let (Some(from), Some(to)) = (
                    self.projected.get(usize::from(*a)).copied().flatten(),
                    self.projected.get(usize::from(*b)).copied().flatten(),
                ) else {
                    continue;
                };
                if (from.0 - to.0).abs() < max_jump {
                    canvas.line(
                        from,
                        to,
                        STAR_LINE_INTENSITY,
                        self.palette.recolor(LINE_TINT),
                    );
                }
            }
        }
        for (index, x, y, altitude_sine) in draw_list {
            let star = &self.catalog.stars[index];
            let mut intensity = if realistic {
                self.brightness(star.magnitude)
            } else {
                1.0
            };
            if self.settings.twinkle {
                intensity *= self.twinkle_factor(index, f64::from(altitude_sine), wall_seconds);
            }
            let color = if colored {
                self.star_colors[index]
            } else {
                self.palette.recolor(NEUTRAL_TINT)
            };
            if !realistic {
                canvas.cluster(x, y, SINGLE, intensity, color);
                continue;
            }
            match star.magnitude + bias {
                m if m < -2.2 => canvas.cluster(x, y, DISC, intensity, color),
                m if m < 0.6 => canvas.cluster(x, y, BLOB, intensity, color),
                m if m < 1.5 => canvas.cluster(x, y, PLUS, intensity, color),
                m if m < 2.6 => canvas.pair(x, y, intensity, color),
                _ => canvas.cluster(x, y, SINGLE, intensity, color),
            }
        }
    }

    fn draw_satellites(
        &self,
        canvas: &mut Canvas<'_>,
        view: &View,
        horizon: &Mat3,
        unix: f64,
        lst: f64,
    ) {
        // Synthetic 420 km circular orbits: remove the observer's geocentric
        // position before projecting. Fixed phases are illustrative, not TLEs.
        let observer = astro::equatorial_vector(lst, self.latitude);
        for index in 0..6 {
            let phase = (unix.rem_euclid(5400.0) / 5400.0 * std::f64::consts::TAU)
                + index as f64 * std::f64::consts::FRAC_PI_3;
            let node = index as f64 * 0.73;
            let inclination = (51.6_f64 + index as f64 * 5.0).to_radians();
            let (sin_phase, cos_phase) = phase.sin_cos();
            let (sin_node, cos_node) = node.sin_cos();
            let orbital = [
                cos_node * cos_phase - sin_node * sin_phase * inclination.cos(),
                sin_node * cos_phase + cos_node * sin_phase * inclination.cos(),
                sin_phase * inclination.sin(),
            ];
            let direction = mat_vec(
                horizon,
                &normalize(std::array::from_fn(|axis| {
                    6791.0 * orbital[axis] - 6371.0 * observer[axis]
                })),
            );
            if self.settings.horizon && direction[2] <= 0.0 {
                continue;
            }
            if let Some((x, y)) = view.project(&direction) {
                canvas.cluster(x, y, PLUS, 1.0, self.palette.recolor([210, 235, 215]));
            }
        }
    }

    fn draw_planets(&self, canvas: &mut Canvas<'_>, view: &View, to_horizon_j2000: &Mat3, jd: f64) {
        for planet in Planet::ALL {
            let sight = planet_sight(planet, jd);
            let direction = mat_vec(to_horizon_j2000, &sight.direction);
            if self.settings.horizon && direction[2] <= 0.0 {
                continue;
            }
            let Some((x, y)) = view.project(&direction) else {
                continue;
            };
            let magnitude = sight.magnitude as f32;
            let shape = match magnitude {
                m if m < -2.2 => DISC,
                m if m < 0.6 => BLOB,
                m if m < 1.5 => PLUS,
                _ => SINGLE,
            };
            canvas.cluster(x, y, shape, 1.0, self.palette.recolor(planet_color(planet)));
        }
    }

    fn draw_moon(
        &self,
        canvas: &mut Canvas<'_>,
        view: &View,
        horizon: &Mat3,
        to_horizon_j2000: &Mat3,
        jd: f64,
    ) {
        let moon = moon_sight(jd);
        let geocentric = mat_vec(horizon, &moon.direction);
        // Topocentric parallax lowers the Moon by up to a degree.
        let sin_parallax = moon.horizontal_parallax_deg.to_radians().sin();
        let direction = normalize([geocentric[0], geocentric[1], geocentric[2] - sin_parallax]);
        if self.settings.horizon && direction[2] <= 0.0 {
            return;
        }
        let Some((x, y)) = view.project(&direction) else {
            return;
        };
        // Screen direction of the bright limb: towards the Sun along the sky.
        let sun = mat_vec(to_horizon_j2000, &sun_direction(jd));
        let along = astro::dot(&sun, &direction);
        let toward = normalize([
            sun[0] - along * direction[0],
            sun[1] - along * direction[1],
            sun[2] - along * direction[2],
        ]);
        let nudged = normalize([
            direction[0] + 0.01 * toward[0],
            direction[1] + 0.01 * toward[1],
            direction[2] + 0.01 * toward[2],
        ]);
        let (mut sx, mut sy) = (0.0, -1.0);
        if let Some((nx, ny)) = view.project(&nudged) {
            let length = (nx - x).hypot(ny - y);
            if length > 1e-9 {
                (sx, sy) = ((nx - x) / length, (ny - y) / length);
            }
        }
        let radius = (0.26 * view.dots_per_degree()).max(MOON_MIN_RADIUS);
        let elongation = moon.elongation_deg.to_radians();
        let (sin_psi, cos_psi) = elongation.sin_cos();
        let reach = radius.ceil() as i64 + 1;
        let (center_x, center_y) = (x.floor() as i64, y.floor() as i64);
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let (px, py) = (
                    (center_x + dx) as f64 + 0.5 - x,
                    (center_y + dy) as f64 + 0.5 - y,
                );
                let distance = px.hypot(py) / radius;
                let coverage = ((radius + 0.5 - px.hypot(py)).clamp(0.0, 1.0)) as f32;
                if coverage <= 0.0 {
                    continue;
                }
                let (a, b) = (px / radius, py / radius);
                let toward_sun = a * sx + b * sy;
                let depth = (1.0 - distance * distance).max(0.0).sqrt();
                let lit = smoothstep(-0.08, 0.08, (toward_sun * sin_psi - depth * cos_psi) as f32);
                let intensity = coverage * (0.16 + 0.84 * lit);
                canvas.plot(
                    center_x + dx,
                    center_y + dy,
                    intensity,
                    self.palette.recolor(MOON_TINT),
                );
            }
        }
    }
}

/// Sparse list of Milky Way dots for a panel of `width` x `height` dots.
fn build_milky_way(
    view: &View,
    to_horizon_j2000: &Mat3,
    width: usize,
    height: usize,
    clip_horizon: bool,
) -> Vec<(u32, u32, f32)> {
    let (pole, centre) = galactic_axes();
    let pole = mat_vec(to_horizon_j2000, &pole);
    let centre = mat_vec(to_horizon_j2000, &centre);
    let third = astro::cross(&pole, &centre);
    let mut dots = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let Some(direction) = view.unproject(x as f64 + 0.5, y as f64 + 0.5) else {
                continue;
            };
            if clip_horizon && direction[2] <= 0.0 {
                continue;
            }
            let intensity = milky_way_intensity(&direction, &pole, &centre, &third);
            if intensity > 0.01 {
                dots.push((x as u32, y as u32, intensity));
            }
        }
    }
    dots
}

fn star_colors(catalog: &Catalog, palette: &ScenePalette) -> Vec<Rgb> {
    catalog
        .stars
        .iter()
        .map(|star| palette.recolor(color_of_index(star.color_index)))
        .collect()
}

fn planet_color(planet: Planet) -> Rgb {
    match planet {
        Planet::Mercury => [200, 195, 190],
        Planet::Venus => [255, 250, 225],
        Planet::Mars => [255, 140, 90],
        Planet::Jupiter => [255, 235, 200],
        Planet::Saturn => [240, 215, 150],
    }
}

/// Smooth value noise in 0..1.
fn value_noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (smoothstep(0.0, 1.0, x - xi), smoothstep(0.0, 1.0, y - yi));
    let corner = |dx: i32, dy: i32| hash(xi as i32 + dx, yi as i32 + dy);
    let top = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * fx;
    let bottom = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * fx;
    top + (bottom - top) * fy
}

/// Brightness of the Milky Way in a direction, from its galactic latitude
/// and longitude: a thin bright core inside a wide halo, brightest towards the
/// galactic centre, broken up by cloudy noise.
fn milky_way_intensity(direction: &Vec3, pole: &Vec3, centre: &Vec3, third: &Vec3) -> f32 {
    let latitude = astro::dot(direction, pole)
        .clamp(-1.0, 1.0)
        .asin()
        .to_degrees() as f32;
    if latitude.abs() > 32.0 {
        return 0.0;
    }
    let longitude = astro::dot(direction, third).atan2(astro::dot(direction, centre)) as f32;
    let band = 0.6 * (-(latitude / 9.0).powi(2)).exp() + 0.4 * (-(latitude / 3.5).powi(2)).exp();
    let longitude_degrees = longitude.to_degrees();
    let bulge = 0.42 + 0.58 * (-(longitude_degrees / 55.0).powi(2)).exp();
    // Sample the noise on a circle so it stays continuous across the anticentre.
    let cloud = value_noise(
        longitude.cos() * 11.0 + 40.0,
        longitude.sin() * 11.0 + latitude / 4.0 + 40.0,
    );
    let fine = value_noise(
        longitude.cos() * 27.0 + 7.0,
        longitude.sin() * 27.0 + latitude / 1.8 + 7.0,
    );
    let texture = 0.35 + 0.8 * cloud + 0.35 * fine;
    // Grain fixed to the sky (0.7 degree cells) so the dither looks irregular
    // rather than a regular lattice, and turns with the stars.
    let grain = 0.45
        + 1.1
            * hash(
                (longitude_degrees * 1.4).floor() as i32,
                (latitude * 1.4).floor() as i32 + 1000,
            );
    // Soft cut-off: a faint fringe would only light the sparse lattice points of
    // the ordered dither and look like a regular grid.
    ((0.40 * band * bulge * texture * grain - 0.05) * 1.25).clamp(0.0, 0.5)
}

impl Scene for StarsScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let wall_seconds = frame.wall.as_secs_f64();
        let unix = self.sky_unix(wall_seconds, frame.now);
        self.last_sky_unix = Some(unix);
        let jd = julian_date(unix);
        let lst = local_sidereal_degrees(jd, self.longitude);
        let horizon = horizon_matrix(self.latitude, lst);
        let precession = if self.settings.precession {
            precession_matrix(jd)
        } else {
            IDENTITY
        };
        let to_horizon_j2000 = mat_mul(&horizon, &precession);
        let view = View::new(&self.settings, frame.raster.width, frame.raster.height);
        let use_colors = self.uses_cell_colors();
        let mut strongest = std::mem::take(&mut self.strongest);
        {
            let raster: &mut Raster = frame.raster;
            let mut canvas = Canvas::new(
                raster,
                frame.cell_colors,
                &mut strongest,
                usize::from(frame.width),
                usize::from(frame.height),
                use_colors,
                self.palette.recolor(NEUTRAL_TINT),
            );
            if self.settings.milky_way {
                self.draw_milky_way(&mut canvas, &view, &to_horizon_j2000, unix);
            }
            if self.settings.horizon {
                self.draw_horizon(&mut canvas, &view);
            }
            self.draw_stars(&mut canvas, &view, &to_horizon_j2000, wall_seconds);
            if self.settings.planets {
                self.draw_planets(&mut canvas, &view, &to_horizon_j2000, jd);
            }
            if self.settings.satellites {
                self.draw_satellites(&mut canvas, &view, &horizon, unix, lst);
            }
            if self.settings.moon {
                self.draw_moon(&mut canvas, &view, &horizon, &to_horizon_j2000, jd);
            }
            if self.settings.cardinal_marks {
                self.draw_compass(&mut canvas, &view);
            }
        }
        self.strongest = strongest;
    }

    fn set_palette(&mut self, palette: &ScenePalette) {
        self.palette = palette.clone();
        self.star_colors = star_colors(self.catalog, palette);
        // The cached Milky Way stores intensities only; its tint is applied on replay.
    }

    fn follows_palette(&self) -> bool {
        true
    }

    fn uses_cell_colors(&self) -> bool {
        self.is_realistic() && self.settings.star_colors
    }

    fn frames_per_second(&self) -> u32 {
        if self.settings.twinkle || self.settings.satellites {
            return 12;
        }
        match self.settings.time_speed {
            settings::TimeSpeed::X1 => 1,
            settings::TimeSpeed::X10 => 2,
            settings::TimeSpeed::X60 => 4,
            settings::TimeSpeed::X600 => 8,
            _ => 12,
        }
    }

    fn status(&self) -> Option<String> {
        if self.catalog.stars.is_empty() {
            return Some("Star catalogue could not be read".to_owned());
        }
        if self.settings.has_unusable_start() {
            return Some("Start date not understood, using the real clock".to_owned());
        }
        let simulated = self.settings.time_speed != settings::TimeSpeed::X1
            || self.settings.time_offset_hours != 0
            || self.settings.start_from == StartFrom::FixedTime;
        if !simulated {
            return None;
        }
        self.last_sky_unix
            .map(|unix| format!("Sky at {} UTC", astro::format_utc(unix)))
    }
}

#[cfg(test)]
mod tests;
