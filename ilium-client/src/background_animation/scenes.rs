//! Coherent, seekable scenes with fixed geometry prepared once per control set.
//! Every texture is prepared at the actual dot resolution; no frame is enlarged
//! from a coarse grid. Spatial randomness is deterministic and never time-seeded.

use super::{
    raster::{hash, smoothstep, Raster},
    shoreline::{rich_columns, shoreline_rich, RichColumn},
    AnimationKind, AnimationSettings, KelpSettings, MoonlitWaterSettings, QuietPondSettings,
    ShorelineSettings, ShorelineStyle, SleepingRidgeSettings, StoneCausticsSettings,
    TeaSteamSettings, TwoRipplesSettings, WindyHillsideSettings,
};
use std::f32::consts::{PI, TAU};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SceneKey {
    kind: AnimationKind,
    controls: [u16; 4],
    shoreline: Option<ShorelineSettings>,
    quiet_pond: Option<QuietPondSettings>,
    width: usize,
    height: usize,
}

#[derive(Debug, Default)]
pub(super) struct SceneCache {
    key: Option<SceneKey>,
    prepared: PreparedScene,
    #[cfg(test)]
    pub preparations: usize,
}

#[derive(Debug, Default)]
enum PreparedScene {
    #[default]
    Empty,
    Shoreline {
        sand: Vec<f32>,
        columns: Vec<ShoreColumn>,
    },
    ShorelineRich {
        sand: Vec<f32>,
        columns: Vec<RichColumn>,
    },
    Moon {
        base: Vec<f32>,
        columns: Vec<WaterColumn>,
    },
    Ridge {
        base: Vec<f32>,
        clouds: Texture,
        mist: Texture,
        fronts: Vec<f32>,
    },
    Meadow {
        base: Vec<f32>,
        plants: Vec<GrassPlant>,
    },
    Tea {
        base: Vec<f32>,
    },
    Kelp {
        plants: Vec<KelpPlant>,
    },
    Stones {
        surfaces: Vec<StoneSurface>,
        texture: Texture,
    },
    Cloudlets {
        // Per-frame x distances, not cached pixels. Refilled before every draw.
        columns: Vec<[f32; 10]>,
    },
    Ripples {
        phases: Vec<RipplePhase>,
    },
    Pond {
        base: Vec<f32>,
        water: Vec<WaterColumn>,
    },
}

#[derive(Debug, Clone, Copy)]
struct ShoreColumn {
    slope: f32,
    shape_a: (f32, f32),
    shape_b: (f32, f32),
    foam: (f32, f32),
    ripple: (f32, f32),
}

#[derive(Debug, Clone, Copy)]
struct WaterColumn {
    broad: (f32, f32),
    fine: (f32, f32),
}

#[derive(Debug, Clone, Copy)]
struct GrassPlant {
    root: (f32, f32),
    height: f32,
    light: f32,
    phase: f32,
    has_seed_head: bool,
}

#[derive(Debug, Clone, Copy)]
struct KelpPlant {
    root: (f32, f32),
    height: f32,
    phase: f32,
    light: f32,
    leaf_length: f32,
    leaves: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Stone {
    pub center: (f32, f32),
    /// Radius in scene units: large 8..12, small 1..2. One unit is 0.020
    /// viewport heights, preserving the requested relative size populations.
    pub radius_units: f32,
    pub is_small: bool,
    ellipticity: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct StoneSurface {
    pub sample: (f32, f32),
    pub height: f32,
    pub normal: (f32, f32, f32),
    pub base: f32,
    light_gain: f32,
    edge: f32,
}

#[derive(Debug, Clone, Copy)]
struct RipplePhase {
    a: (f32, f32),
    b: (f32, f32),
    attenuation_a: f32,
    attenuation_b: f32,
}

/// A full-resolution periodic scalar field. Bilinear lookup transports a
/// continuous texture without regenerating noise, sites or per-dot randomness.
#[derive(Debug)]
struct Texture {
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl Texture {
    fn new(width: usize, height: usize, mut sample: impl FnMut(f32, f32) -> f32) -> Self {
        let mut values = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                values.push(sample(x as f32 / width as f32, y as f32 / height as f32));
            }
        }
        Self {
            width,
            height,
            values,
        }
    }

    fn sample(&self, u: f32, v: f32) -> f32 {
        let x = u.rem_euclid(1.0) * self.width as f32;
        let y = v.rem_euclid(1.0) * self.height as f32;
        let left = x.floor() as usize % self.width;
        let top = y.floor() as usize % self.height;
        let right = (left + 1) % self.width;
        let bottom = (top + 1) % self.height;
        let fraction_x = x - x.floor();
        let fraction_y = y - y.floor();
        let upper = self.values[top * self.width + left] * (1.0 - fraction_x)
            + self.values[top * self.width + right] * fraction_x;
        let lower = self.values[bottom * self.width + left] * (1.0 - fraction_x)
            + self.values[bottom * self.width + right] * fraction_x;
        upper * (1.0 - fraction_y) + lower * fraction_y
    }
}

fn periodic_noise(u: f32, v: f32, columns: i32, rows: i32, seed: i32) -> f32 {
    let x = u * columns as f32;
    let y = v * rows as f32;
    let left = x.floor() as i32;
    let top = y.floor() as i32;
    let fraction_x = smoothstep(0.0, 1.0, x - x.floor());
    let fraction_y = smoothstep(0.0, 1.0, y - y.floor());
    let corner = |cx: i32, cy: i32| hash(cx.rem_euclid(columns) + seed, cy.rem_euclid(rows));
    let upper = corner(left, top) * (1.0 - fraction_x) + corner(left + 1, top) * fraction_x;
    let lower = corner(left, top + 1) * (1.0 - fraction_x) + corner(left + 1, top + 1) * fraction_x;
    upper * (1.0 - fraction_y) + lower * fraction_y
}

// This bounds only the additional cold-preparation payload, not the textures
// or the packed loop cache. Oversized widths retain the original scalar path.
const RIDGE_NOISE_SCRATCH_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy)]
struct RidgeNoiseAxis {
    first: usize,
    second: usize,
    fraction: f32,
}

impl RidgeNoiseAxis {
    // Callers supply Texture::new's normalized coordinates and Ridge's fixed
    // positive periods. Keep division before this multiplication at each caller.
    fn new(coordinate: f32, period: i32) -> Self {
        let position = coordinate * period as f32;
        let first = position.floor() as i32;
        let fraction = smoothstep(0.0, 1.0, position - position.floor());
        Self {
            first: first.rem_euclid(period) as usize,
            second: (first + 1).rem_euclid(period) as usize,
            fraction,
        }
    }

    fn sample_rows(self, rows: &[f32]) -> f32 {
        let upper = rows[self.first];
        let lower = rows[self.second];
        let fraction_y = self.fraction;
        upper * (1.0 - fraction_y) + lower * fraction_y
    }
}

struct RidgeNoiseColumn {
    broad: [f32; 4],
    fine: [f32; 9],
    mist: [f32; 5],
}

fn ridge_noise_grid<const COLUMNS: usize, const ROWS: usize>(seed: i32) -> [[f32; COLUMNS]; ROWS] {
    let mut grid = [[0.0; COLUMNS]; ROWS];
    for (y, row) in grid.iter_mut().enumerate() {
        for (x, value) in row.iter_mut().enumerate() {
            // These are already wrapped coordinates. Add the seed AFTER
            // wrapping, exactly as periodic_noise's corner closure does.
            *value = hash(x as i32 + seed, y as i32);
        }
    }
    grid
}

fn ridge_noise_horizontal<const COLUMNS: usize, const ROWS: usize>(
    grid: &[[f32; COLUMNS]; ROWS],
    u: f32,
) -> [f32; ROWS] {
    let axis = RidgeNoiseAxis::new(u, COLUMNS as i32);
    let fraction_x = axis.fraction;
    let mut values = [0.0; ROWS];
    for (value, row) in values.iter_mut().zip(grid) {
        // This is the original upper/lower expression, not a reassociated
        // lerp. Its f32 result is reused for every visit to this lattice row.
        *value = row[axis.first] * (1.0 - fraction_x) + row[axis.second] * fraction_x;
    }
    values
}

// Keep the cold loops and their stack/workspace inside this helper rather than
// adding them to the shared prepare dispatcher. Release codegen still needs
// inspection; this boundary is not a performance or cross-scene guarantee.
#[inline(never)]
fn prepare_ridge_textures(width: usize, height: usize) -> (Texture, Texture) {
    let mut columns = Vec::<RidgeNoiseColumn>::new();
    if width == 0
        || height == 0
        || width > RIDGE_NOISE_SCRATCH_BYTES / std::mem::size_of::<RidgeNoiseColumn>()
        || columns.try_reserve_exact(width).is_err()
    {
        return (
            Texture::new(width, height, |u, v| {
                periodic_noise(u, v, 5, 4, 37) * 0.76 + periodic_noise(u, v, 11, 9, 94) * 0.24
            }),
            Texture::new(width, height, |u, v| periodic_noise(u, v, 7, 5, 183)),
        );
    }

    let broad_grid = ridge_noise_grid::<5, 4>(37);
    let fine_grid = ridge_noise_grid::<11, 9>(94);
    let mist_grid = ridge_noise_grid::<7, 5>(183);
    for x in 0..width {
        let u = x as f32 / width as f32;
        columns.push(RidgeNoiseColumn {
            broad: ridge_noise_horizontal(&broad_grid, u),
            fine: ridge_noise_horizontal(&fine_grid, u),
            mist: ridge_noise_horizontal(&mist_grid, u),
        });
    }

    // Each output texture keeps its original dimensions, row-major order and
    // full-resolution samples. Build clouds before mist as in the scalar path.
    let mut values = Vec::with_capacity(width * height);
    for y in 0..height {
        let v = y as f32 / height as f32;
        let broad = RidgeNoiseAxis::new(v, 4);
        let fine = RidgeNoiseAxis::new(v, 9);
        for column in &columns {
            values.push(
                broad.sample_rows(&column.broad) * 0.76 + fine.sample_rows(&column.fine) * 0.24,
            );
        }
    }
    let clouds = Texture {
        width,
        height,
        values,
    };

    let mut values = Vec::with_capacity(width * height);
    for y in 0..height {
        let v = y as f32 / height as f32;
        let mist_y = RidgeNoiseAxis::new(v, 5);
        for column in &columns {
            values.push(mist_y.sample_rows(&column.mist));
        }
    }
    let mist = Texture {
        width,
        height,
        values,
    };

    // Nothing from columns or the corner grids enters PreparedScene. This
    // scratch is released before the caller prepares fronts and clones base.
    (clouds, mist)
}

fn traveling(phase: (f32, f32), time: (f32, f32)) -> f32 {
    phase.0 * time.1 - phase.1 * time.0
}

impl SceneCache {
    fn prepare(&mut self, raster: &mut Raster, settings: &AnimationSettings) {
        let key = SceneKey {
            kind: settings.kind,
            controls: settings.scene_sliders().map(|slider| slider.value),
            shoreline: settings.scene_shoreline_key(),
            quiet_pond: (settings.kind == AnimationKind::QuietPond).then_some(settings.quiet_pond),
            width: raster.width,
            height: raster.height,
        };
        if self.key == Some(key) {
            return;
        }
        self.prepared = match settings.kind {
            AnimationKind::Shoreline => {
                let grains = f32::from(settings.shoreline.grain_percent) / 100.0;
                let mut sand = Vec::with_capacity(raster.dots.len());
                for y in 0..raster.height {
                    for x in 0..raster.width {
                        let seed = hash(x as i32, y as i32 + 140);
                        sand.push(if seed > 0.985 { grains * 0.42 } else { 0.012 });
                    }
                }
                if settings.shoreline.style == ShorelineStyle::Rich {
                    PreparedScene::ShorelineRich {
                        sand,
                        columns: rich_columns(raster.width),
                    }
                } else {
                    let columns = (0..raster.width)
                        .map(|x| {
                            let u = (x as f32 + 0.5) / raster.width as f32;
                            ShoreColumn {
                                slope: -0.17 * (u - 0.5),
                                shape_a: (u * 11.0).sin_cos(),
                                shape_b: (u * 23.0).sin_cos(),
                                foam: (u * 41.0).sin_cos(),
                                ripple: (u * 8.0).sin_cos(),
                            }
                        })
                        .collect();
                    PreparedScene::Shoreline { sand, columns }
                }
            }
            AnimationKind::MoonlitWater => {
                prepare_moon(raster, settings.moonlit_water);
                PreparedScene::Moon {
                    base: raster.dots.clone(),
                    columns: water_columns(raster.width, 19.0, 47.0),
                }
            }
            AnimationKind::SleepingRidge => {
                let ridge_settings = settings.sleeping_ridge;
                prepare_ridge(raster, ridge_settings);
                let (clouds, mist) = prepare_ridge_textures(raster.width, raster.height);
                let fronts = (0..raster.width)
                    .map(|x| ridge((x as f32 + 0.5) / raster.width as f32, 2, ridge_settings))
                    .collect();
                PreparedScene::Ridge {
                    base: raster.dots.clone(),
                    clouds,
                    mist,
                    fronts,
                }
            }
            AnimationKind::WindyHillside => {
                raster.curve(raster.width.min(160), 0.20, 0.26, |u| (u, hill(u)));
                PreparedScene::Meadow {
                    base: raster.dots.clone(),
                    plants: grass_plants(raster, settings.windy_hillside),
                }
            }
            AnimationKind::TeaSteam => {
                prepare_cup(raster, settings.tea_steam);
                PreparedScene::Tea {
                    base: raster.dots.clone(),
                }
            }
            AnimationKind::Kelp => PreparedScene::Kelp {
                plants: kelp_plants(raster, settings.kelp),
            },
            AnimationKind::StoneCaustics => {
                let stones = stone_layout(settings.stone_caustics);
                let mut surfaces = Vec::with_capacity(raster.dots.len());
                for y in 0..raster.height {
                    let v = (y as f32 + 0.5) / raster.height as f32;
                    for x in 0..raster.width {
                        let u = (x as f32 + 0.5) / raster.width as f32;
                        let mut surface =
                            stone_surface(u, v, raster.aspect(), &stones, settings.stone_caustics);
                        let seed = hash(x as i32 + 190, y as i32 + 370);
                        if surface.height == 0.0
                            && seed
                                > 1.0 - f32::from(settings.stone_caustics.sand_percent) * 0.00006
                        {
                            surface.base = surface.base.max(0.28);
                        }
                        surfaces.push(surface);
                    }
                }
                let sites = CausticSites::new();
                let texture = Texture::new(raster.width, raster.height, |u, v| sites.sample(u, v));
                PreparedScene::Stones { surfaces, texture }
            }
            AnimationKind::Cloudlets => PreparedScene::Cloudlets {
                columns: vec![[0.0; 10]; raster.width],
            },
            AnimationKind::TwoRipples => PreparedScene::Ripples {
                phases: ripple_phases(raster, settings.two_ripples),
            },
            AnimationKind::QuietPond => {
                prepare_pond(raster, settings.quiet_pond);
                PreparedScene::Pond {
                    base: raster.dots.clone(),
                    water: water_columns(raster.width, 13.0, 35.0),
                }
            }
            // Hosted kinds never reach this cache.
            _ => PreparedScene::Empty,
        };
        self.key = Some(key);
        #[cfg(test)]
        {
            self.preparations += 1;
        }
    }
}

pub(super) fn render(
    raster: &mut Raster,
    cache: &mut SceneCache,
    settings: &AnimationSettings,
    seconds: f64,
) {
    cache.prepare(raster, settings);
    let time = seconds as f32;
    match &mut cache.prepared {
        PreparedScene::Shoreline { sand, columns } => {
            shoreline(raster, settings, time, sand, columns)
        }
        PreparedScene::ShorelineRich { sand, columns } => {
            shoreline_rich(raster, &settings.shoreline, time, sand, columns)
        }
        PreparedScene::Moon { base, columns } => {
            moonlit_water(raster, settings.moonlit_water, time, base, columns)
        }
        PreparedScene::Ridge {
            base,
            clouds,
            mist,
            fronts,
        } => sleeping_ridge(
            raster,
            settings.sleeping_ridge,
            time,
            base,
            clouds,
            mist,
            fronts,
        ),
        PreparedScene::Meadow { base, plants } => {
            windy_hillside(raster, settings.windy_hillside, time, base, plants)
        }
        PreparedScene::Tea { base } => tea_steam(raster, settings.tea_steam, time, base),
        PreparedScene::Kelp { plants } => kelp(raster, settings.kelp, time, plants),
        PreparedScene::Stones { surfaces, texture } => {
            stone_caustics(raster, settings.stone_caustics, time, surfaces, texture)
        }
        PreparedScene::Cloudlets { columns } => cloudlets_prepared(raster, settings, time, columns),
        PreparedScene::Ripples { phases } => {
            two_ripples(raster, settings.two_ripples, time, phases)
        }
        PreparedScene::Pond { base, water } => {
            quiet_pond(raster, settings.quiet_pond, time, base, water)
        }
        PreparedScene::Empty => cloudlets(raster, settings, time),
    }
}

#[derive(Clone, Copy)]
struct ClassicColumnFrame {
    shape: f32,
    fragments: f32,
}

fn shoreline(
    raster: &mut Raster,
    settings: &AnimationSettings,
    time: f32,
    sand: &[f32],
    columns: &[ShoreColumn],
) {
    let controls = settings.shoreline;
    let phase = (time / f32::from(controls.cycle_seconds)).fract();
    let wash = if phase < 0.26 {
        smoothstep(0.0, 0.26, phase)
    } else if phase < 0.36 {
        1.0
    } else {
        1.0 - smoothstep(0.36, 1.0, phase)
    };
    let maximum_tide = 0.40 + 0.27 * f32::from(controls.reach_percent) / 100.0;
    let tide = 0.40 + (maximum_tide - 0.40) * wash;
    let wet_mark = if phase < 0.36 { tide } else { maximum_tide };
    let wet_persistence = 1.0 - smoothstep(0.68, 1.0, phase);
    let foam_width =
        (1.3 / raster.height as f32).max(0.006) * f32::from(controls.foam_width_percent) / 100.0;
    let phase_a = (-time * 0.33).sin_cos();
    let phase_b = (time * 0.24).sin_cos();
    let foam_phase = (time * 0.65).sin_cos();
    // Shape and foam fragmentation are independent of the raster row. Keep
    // their original arithmetic so cached geometry and golden frames agree.
    let frames: Vec<ClassicColumnFrame> = columns
        .iter()
        .map(|column| ClassicColumnFrame {
            shape: column.slope
                + traveling(column.shape_a, phase_a) * 0.014
                + traveling(column.shape_b, phase_b) * 0.008,
            fragments: 0.64 + 0.36 * smoothstep(-0.55, 0.55, traveling(column.foam, foam_phase)),
        })
        .collect();
    for y in 0..raster.height {
        let v = (y as f32 + 0.5) / raster.height as f32;
        let ripple_phase = (time * 0.90 - v * 54.0).sin_cos();
        for (x, (column, frame)) in columns.iter().zip(&frames).enumerate() {
            let index = y * raster.width + x;
            let shape = frame.shape;
            let distance = v - tide - shape;
            let intensity = if distance.abs() < foam_width {
                let foam = 1.0 - smoothstep(foam_width * 0.15, foam_width, distance.abs());
                foam * frame.fragments
            } else if distance < 0.0 {
                let crest = smoothstep(0.78, 0.99, traveling(column.ripple, ripple_phase));
                0.012 + crest * 0.19 * (0.45 + 0.55 * (1.0 - smoothstep(0.0, 0.35, -distance)))
            } else {
                let wet = (1.0 - smoothstep(wet_mark + shape, wet_mark + shape + 0.025, v))
                    * wet_persistence;
                sand[index] * (1.0 - wet * 0.88)
            };
            raster.dots[index] = intensity.clamp(0.0, 1.0);
        }
    }
}

fn water_columns(width: usize, broad: f32, fine: f32) -> Vec<WaterColumn> {
    (0..width)
        .map(|x| {
            let u = (x as f32 + 0.5) / width as f32;
            WaterColumn {
                broad: (u * broad).sin_cos(),
                fine: (u * fine).sin_cos(),
            }
        })
        .collect()
}

fn prepare_moon(raster: &mut Raster, settings: MoonlitWaterSettings) {
    let aspect = raster.aspect();
    let radius = 0.105 * f32::from(settings.moon_size_percent) / 100.0;
    let antialias = 0.7 / raster.height as f32;
    raster.field(|u, v| {
        if v >= 0.43 {
            return 0.0;
        }
        let distance = ((u - 0.54) * aspect).hypot(v - 0.20);
        1.0 - smoothstep(radius - antialias, radius + antialias, distance)
    });
}

fn moonlit_water(
    raster: &mut Raster,
    settings: MoonlitWaterSettings,
    time: f32,
    base: &[f32],
    columns: &[WaterColumn],
) {
    raster.dots.copy_from_slice(base);
    let strength = f32::from(settings.wave_strength_percent) / 100.0;
    let scale = f32::from(settings.ripple_scale_percent) / 100.0;
    let reflection = f32::from(settings.reflection_width_percent) / 100.0;
    for y in 0..raster.height {
        let v = (y as f32 + 0.5) / raster.height as f32;
        if v < 0.43 {
            continue;
        }
        let depth = (v - 0.43) / 0.57;
        // Perspective increases band spacing toward the viewer. Both phases
        // are transported, while columns supply cached crossing wave normals.
        let perspective = 42.0 * (depth + 0.07).ln();
        let broad = (time * strength * 1.10 - perspective / scale).sin_cos();
        let fine = (-time * strength * 0.73 - depth * 107.0 / scale).sin_cos();
        // The reflection spreads into a moving sheet as it approaches the
        // viewer. Independent depth waves keep it from reading as a fixed
        // vertical pillar, even when the water controls are set low.
        let center = 0.54
            + ((depth * 8.5 - time * strength * 0.62).sin() * 0.045
                + (depth * 19.0 + time * strength * 0.39).sin() * 0.022)
                * depth
                * strength;
        let half_width = (0.018 + depth * depth * 0.22) * reflection;
        for (x, column) in columns.iter().enumerate() {
            let u = (x as f32 + 0.5) / raster.width as f32;
            let wave_a = traveling(column.broad, broad);
            let wave_b = traveling(column.fine, fine);
            let local_center = center
                + wave_b * depth * strength * 0.035
                + (depth * 47.0 + time * strength * 0.8).sin() * depth * strength * 0.012;
            let ribbon = 1.0 - smoothstep(half_width * 0.42, half_width, (u - local_center).abs());
            if ribbon == 0.0 {
                // The reflection product is exactly +0 here; only wavelets can
                // contribute. Skip its crest and fragment work outside the ribbon.
                raster.dots[y * raster.width + x] =
                    smoothstep(0.86, 0.99, wave_a) * 0.10 * (0.3 + depth * 0.7) * strength.min(1.0);
                continue;
            }
            let crest = smoothstep(0.02, 0.72, (wave_a * 0.68 + wave_b * 0.32) * strength);
            let broken_surface = wave_b + (depth * 83.0 - time * strength * 1.7).sin() * 0.34;
            let fragments = 0.18 + 0.82 * smoothstep(-0.40, 0.48, broken_surface);
            let reflection_light = ribbon * (0.12 + crest * 0.76) * fragments;
            let outside_wavelets =
                smoothstep(0.86, 0.99, wave_a) * 0.10 * (0.3 + depth * 0.7) * strength.min(1.0);
            raster.dots[y * raster.width + x] = reflection_light.max(outside_wavelets);
        }
    }
}

fn ridge(u: f32, layer: usize, settings: SleepingRidgeSettings) -> f32 {
    let original = match layer {
        0 => 0.50 + 0.08 * (u * 9.0 + 0.3).sin() + 0.035 * (u * 19.0).sin(),
        1 => 0.64 + 0.08 * (u * 7.0 + 1.8).sin() + 0.035 * (u * 14.0 + 0.4).sin(),
        _ => 0.79 + 0.055 * (u * 8.0 + 4.0).sin() + 0.028 * (u * 17.0).sin(),
    };
    0.91 - (0.91 - original) * f32::from(settings.ridge_height_percent) / 100.0
}

fn prepare_ridge(raster: &mut Raster, settings: SleepingRidgeSettings) {
    raster.field(|u, v| {
        if v > ridge(u, 2, settings) {
            0.008
        } else if v > ridge(u, 1, settings) {
            0.045
        } else if v > ridge(u, 0, settings) {
            0.10
        } else {
            0.0
        }
    });
    for layer in 0..3 {
        raster.curve(
            raster.width.min(180),
            0.22,
            0.42 - layer as f32 * 0.10,
            |u| (u, ridge(u, layer, settings)),
        );
    }
}

fn sleeping_ridge(
    raster: &mut Raster,
    settings: SleepingRidgeSettings,
    time: f32,
    base: &[f32],
    clouds: &Texture,
    mist: &Texture,
    fronts: &[f32],
) {
    raster.dots.copy_from_slice(base);
    let drift = time * 0.025 * f32::from(settings.drift_percent) / 100.0;
    let cover = f32::from(settings.cloud_cover_percent) / 100.0;
    let mist_strength = f32::from(settings.mist_percent) / 100.0;
    for y in 0..raster.height {
        let v = (y as f32 + 0.5) / raster.height as f32;
        let sky_mask = 1.0 - smoothstep(0.34, 0.58, v);
        let mist_mask = 1.0 - smoothstep(0.0, 0.14, (v - 0.70).abs());
        if sky_mask == 0.0 && mist_mask == 0.0 {
            continue;
        }
        let warp = (v * 7.0 - time * 0.20).sin() * 0.028;
        for (x, front) in fronts.iter().enumerate() {
            let u = (x as f32 + 0.5) / raster.width as f32;
            let index = y * raster.width + x;
            if cover > 0.0 && sky_mask > 0.0 {
                let bank = smoothstep(
                    0.76 - cover * 0.48,
                    0.95 - cover * 0.29,
                    clouds.sample(u - drift + warp, v),
                );
                raster.dots[index] = raster.dots[index].max(bank * sky_mask * 0.70);
            }
            if mist_strength > 0.0 && mist_mask > 0.0 && v < *front {
                let valley = smoothstep(0.38, 0.76, mist.sample(u + drift * 0.37, v));
                raster.dots[index] =
                    raster.dots[index].max(valley * mist_mask * mist_strength * 0.68);
            }
        }
    }
}

fn hill(u: f32) -> f32 {
    0.53 + 0.23 * (u - 0.42).powi(2) + 0.035 * (u * 7.0).sin()
}

fn grass_plants(raster: &Raster, settings: WindyHillsideSettings) -> Vec<GrassPlant> {
    // Count follows visible dot width, with a ceiling for very large terminals.
    // Ten irregular depth layers replace the former seven-by-42 coarse grid.
    let columns =
        (raster.width / 3).clamp(44, 120) * usize::from(settings.plant_density_percent) / 100;
    let mut plants = Vec::with_capacity(columns * 10);
    for row in 0..10 {
        for column in 0..columns {
            let seed = hash(column as i32, row);
            let x = (column as f32 + 0.15 + seed * 0.70) / columns as f32;
            let depth = row as f32 / 9.0;
            let y = hill(x) + 0.026 + depth * 0.45 + hash(column as i32 + 71, row) * 0.037;
            if y > 1.10 {
                continue;
            }
            plants.push(GrassPlant {
                root: (x, y),
                height: (0.022 + seed * 0.045 + depth * 0.027)
                    * f32::from(settings.plant_height_percent)
                    / 100.0,
                light: 0.36 + depth * 0.43 + seed * 0.10,
                phase: seed * 1.3 + depth * 0.75,
                has_seed_head: hash(column as i32 + 109, row) > 0.83,
            });
        }
    }
    plants
}

fn windy_hillside(
    raster: &mut Raster,
    settings: WindyHillsideSettings,
    time: f32,
    base: &[f32],
    plants: &[GrassPlant],
) {
    raster.dots.copy_from_slice(base);
    let aspect = raster.aspect();
    let strength = f32::from(settings.wind_strength_percent) / 100.0;
    let breadth = f32::from(settings.gust_scale_percent) / 100.0;
    for plant in plants {
        let gust = (plant.root.0 * 10.0 / breadth - time * 1.02 - plant.phase).sin()
            + 0.29 * (plant.root.0 * 4.0 - time * 0.47 - plant.phase * 0.7).sin();
        let bend = (0.008 + gust * 0.026 * strength) / aspect;
        let steps = ((plant.height * raster.height as f32 / 1.7).ceil() as usize).clamp(4, 12);
        let position = |fraction: f32| {
            (
                plant.root.0 + bend * fraction * fraction,
                plant.root.1 - plant.height * fraction,
            )
        };
        raster.curve(steps, 0.16, plant.light, position);
        if plant.has_seed_head {
            let tip = position(1.0);
            for side in [-1.0, 1.0] {
                raster.line(
                    (tip.0 - bend * 0.06, tip.1 + plant.height * 0.12),
                    (tip.0 + side * 0.005 / aspect, tip.1 - plant.height * 0.015),
                    0.13,
                    plant.light * 0.86,
                );
            }
        }
    }
}

fn cup_dimensions(settings: TeaSteamSettings, aspect: f32) -> (f32, f32) {
    let scale = f32::from(settings.cup_size_percent) / 100.0;
    ((0.18 * scale / aspect).min(0.32), 0.14 * scale)
}

fn prepare_cup(raster: &mut Raster, settings: TeaSteamSettings) {
    let aspect = raster.aspect();
    let (half_width, height) = cup_dimensions(settings, aspect);
    let lip_y = 0.74;
    let bottom_y = lip_y + height;
    // Thin ceramic contours, an elliptical rim, a hollow handle and a saucer.
    // The faint body shading is continuous, rather than a block-shaped fill.
    raster.field(|u, v| {
        if !(lip_y..=bottom_y).contains(&v) {
            return 0.0;
        }
        let depth = (v - lip_y) / height;
        let width = half_width * (1.0 - 0.22 * smoothstep(0.0, 1.0, depth));
        let x = (u - 0.5) / width;
        if x.abs() >= 1.0 {
            return 0.0;
        }
        (0.018 + 0.10 * (1.0 - x.abs()) + 0.06 * smoothstep(-0.9, -0.4, -x))
            * (1.0 - smoothstep(0.88, 1.0, depth))
    });
    raster.curve(72, 0.33, 0.93, |fraction| {
        let angle = fraction * TAU;
        (0.5 + half_width * angle.cos(), lip_y + 0.018 * angle.sin())
    });
    raster.curve(64, 0.22, 0.42, |fraction| {
        let angle = fraction * TAU;
        (
            0.5 + half_width * 0.88 * angle.cos(),
            lip_y + 0.012 * angle.sin(),
        )
    });
    for side in [-1.0, 1.0] {
        raster.curve(36, 0.32, 0.87, |fraction| {
            let width = half_width * (1.0 - 0.22 * smoothstep(0.0, 1.0, fraction));
            (0.5 + side * width, lip_y + height * fraction)
        });
    }
    raster.curve(48, 0.32, 0.77, |fraction| {
        (
            0.5 + (fraction - 0.5) * half_width * 1.56,
            bottom_y + (fraction * PI).sin() * 0.013,
        )
    });
    for inner in [false, true] {
        let handle_width = if inner { 0.044 } else { 0.063 } / aspect;
        let handle_height = if inner { height * 0.18 } else { height * 0.30 };
        raster.curve(44, 0.28, if inner { 0.60 } else { 0.82 }, |fraction| {
            let angle = -PI * 0.55 + fraction * PI * 1.10;
            (
                0.5 + half_width * 0.94 + handle_width * angle.cos(),
                lip_y + height * 0.46 + handle_height * angle.sin(),
            )
        });
    }
    raster.curve(72, 0.24, 0.56, |fraction| {
        let angle = fraction * TAU;
        (
            0.5 + half_width * 1.48 * angle.cos(),
            bottom_y + 0.024 + 0.020 * angle.sin(),
        )
    });
    raster.curve(48, 0.16, 0.29, |fraction| {
        (
            0.5 - half_width * 0.73 + half_width * fraction * 0.08,
            lip_y + height * (0.13 + fraction * 0.65),
        )
    });
}

fn tea_steam(raster: &mut Raster, settings: TeaSteamSettings, time: f32, base: &[f32]) {
    raster.dots.copy_from_slice(base);
    let aspect = raster.aspect();
    let (cup_width, _) = cup_dimensions(settings, aspect);
    let curl = f32::from(settings.curl_percent) / 100.0;
    let steam_height = (0.54 * f32::from(settings.steam_height_percent) / 100.0).min(0.70);
    for ribbon in 0..usize::from(settings.steam_ribbons) {
        let seed = hash(ribbon as i32, 841);
        let origin = 0.5 + (seed - 0.5) * cup_width * 1.25;
        let phase = seed * TAU;
        let steps = (raster.height / 3).clamp(24, 72);
        let path = |age: f32| {
            let emission = time * (0.72 + seed * 0.24) - age * (5.2 + seed * 2.4);
            let bend = (age * (4.8 + seed * 2.1) + emission + phase).sin() * 0.074 * age
                + (age * (10.0 + seed * 5.0) - emission * 0.57 + phase * 1.7).sin()
                    * 0.038
                    * age
                    * age;
            (origin + bend * curl / aspect, 0.714 - age * steam_height)
        };
        let mut previous = path(0.0);
        for step in 1..=steps {
            let age = step as f32 / steps as f32;
            let next = path(age);
            let fade = (1.0 - smoothstep(0.52, 1.0, age))
                * smoothstep(0.0, 0.06, age)
                * (0.60 + seed * 0.18);
            raster.line(previous, next, 0.17 + age * 0.38, fade);
            // A faint neighboring filament gives steam a soft edge while
            // keeping the luminous center narrower than the former thick curl.
            let offset = 0.55 / raster.width as f32;
            raster.line(
                (previous.0 + offset, previous.1),
                (next.0 + offset, next.1),
                0.13,
                fade * 0.29,
            );
            previous = next;
        }
    }
}

fn kelp_plants(raster: &Raster, settings: KelpSettings) -> Vec<KelpPlant> {
    let count =
        (raster.width / 14).clamp(10, 26) * usize::from(settings.plant_density_percent) / 100;
    let clustering = f32::from(settings.cluster_percent) / 100.0;
    let centers = [0.17, 0.49, 0.82];
    (0..count)
        .map(|index| {
            let seed = hash(index as i32, 75);
            let uniform_x = (index as f32 + 0.2 + seed * 0.6) / count as f32;
            let cluster = (hash(index as i32, 329) * 3.0) as usize;
            let clustered_x = centers[cluster.min(2)] + (hash(index as i32, 471) - 0.5) * 0.23;
            KelpPlant {
                root: (
                    uniform_x * (1.0 - clustering) + clustered_x * clustering,
                    1.025 + hash(index as i32, 482) * 0.075,
                ),
                height: 0.36 + seed * 0.60,
                phase: hash(index as i32, 204) * TAU,
                light: 0.34 + seed * 0.52,
                leaf_length: (0.045 + hash(index as i32, 832) * 0.055)
                    * f32::from(settings.leaf_length_percent)
                    / 100.0,
                leaves: 7 + (hash(index as i32, 281) * 7.0) as usize,
            }
        })
        .collect()
}

fn kelp_position(
    plant: &KelpPlant,
    fraction: f32,
    time: f32,
    strength: f32,
    aspect: f32,
) -> (f32, f32) {
    let current = (time * 0.62 - fraction * 2.8 + plant.phase).sin() * 0.070
        + (time * 0.33 - fraction * 5.2 + plant.phase * 1.4).sin() * 0.030;
    (
        plant.root.0 + current * fraction * fraction * strength / aspect,
        plant.root.1 - plant.height * fraction,
    )
}

fn kelp(raster: &mut Raster, settings: KelpSettings, time: f32, plants: &[KelpPlant]) {
    let aspect = raster.aspect();
    let strength = f32::from(settings.current_strength_percent) / 100.0;
    // Roots begin below the viewport. There is deliberately no floor stroke.
    for plant in plants {
        let position = |fraction| kelp_position(plant, fraction, time, strength, aspect);
        raster.curve(28, 0.26, plant.light, position);
        for leaf in 1..=plant.leaves {
            let fraction = leaf as f32 / (plant.leaves + 2) as f32;
            let base = position(fraction);
            let side = if leaf % 2 == 0 { 1.0 } else { -1.0 };
            let length = plant.leaf_length * (1.0 - fraction * 0.40);
            let current = (time * 0.62 - fraction * 2.8 + plant.phase).sin() * strength;
            let twist = (time * 0.83 * strength - fraction * 4.5 + plant.phase).sin();
            let steps = ((length * raster.height as f32 / 2.0) as usize).clamp(5, 12);
            let light = plant.light * (0.83 + 0.17 * twist.abs());
            let positions = |along: f32| {
                let taper = (along * PI).sin();
                let x = base.0
                    + (side * length * along + current * length * along * along * 0.54) / aspect;
                let y = base.1 - length * (along * 0.46 + along * along * 0.22);
                [-1.0, 1.0]
                    .map(|edge| (x, y + edge * length * 0.13 * taper * (0.45 + twist * 0.35)))
            };
            let mut previous = positions(0.0);
            for step in 1..=steps {
                let next = positions(step as f32 / steps as f32);
                // Both edges use the original sample fractions. Max blending
                // of finite intensities makes their interleaving order-neutral.
                for (from, to) in previous.into_iter().zip(next) {
                    raster.line(from, to, 0.19, light);
                }
                previous = next;
            }
        }
    }
}

pub(super) fn stone_layout(settings: StoneCausticsSettings) -> Vec<Stone> {
    let clusters = [(0.18, 0.28), (0.62, 0.31), (0.42, 0.76), (0.87, 0.77)];
    let small_count = usize::from(settings.small_stones_percent) * 28 / 100;
    let mut stones = Vec::with_capacity(8 + small_count);
    for index in 0..8 + small_count {
        let is_small = index >= 8;
        let cluster = index % clusters.len();
        let seed = hash(index as i32, 1003);
        let angle = hash(index as i32, 491) * TAU;
        let spread = if is_small {
            0.065 + seed * 0.15
        } else {
            0.025 + seed * 0.10
        };
        let center = (
            (clusters[cluster].0 + angle.cos() * spread).clamp(0.04, 0.96),
            (clusters[cluster].1 + angle.sin() * spread).clamp(0.05, 0.95),
        );
        stones.push(Stone {
            center,
            radius_units: if is_small {
                1.0 + seed
            } else {
                8.0 + seed * 4.0
            },
            is_small,
            ellipticity: 0.82 + hash(index as i32, 932) * 0.38,
        });
    }
    stones
}

pub(super) fn stone_surface(
    u: f32,
    v: f32,
    aspect: f32,
    stones: &[Stone],
    settings: StoneCausticsSettings,
) -> StoneSurface {
    let mut surface = StoneSurface {
        sample: (u, v),
        normal: (0.0, 0.0, 1.0),
        light_gain: 0.20,
        ..Default::default()
    };
    let relief = f32::from(settings.dome_height_percent) / 100.0;
    for stone in stones {
        let radius = stone.radius_units * 0.020;
        let dx = (u - stone.center.0) * aspect / radius;
        let dy = (v - stone.center.1) * stone.ellipticity / radius;
        let squared_distance = dx * dx + dy * dy;
        if squared_distance >= 1.08 {
            continue;
        }
        let distance = squared_distance.sqrt();
        let edge = (1.0 - (distance - 1.0).abs() / if stone.is_small { 0.14 } else { 0.055 })
            .max(0.0)
            * 0.35;
        if squared_distance >= 1.0 {
            if surface.height == 0.0 {
                surface.edge = surface.edge.max(edge);
            }
            continue;
        }
        let dome = (1.0 - squared_distance).sqrt();
        let height = radius * dome * relief;
        // The visible surface is the highest dome, not the last stone in the
        // list. This keeps overlapping clusters independent of storage order.
        if height <= surface.height {
            continue;
        }
        let gradient_x = -relief * dx / dome.max(0.07);
        let gradient_y = -relief * dy * stone.ellipticity / dome.max(0.07);
        let inverse_length = (gradient_x * gradient_x + gradient_y * gradient_y + 1.0)
            .sqrt()
            .recip();
        let normal = (
            -gradient_x * inverse_length,
            -gradient_y * inverse_length,
            inverse_length,
        );
        let diffuse = (normal.0 * -0.30 + normal.1 * -0.38 + normal.2 * 0.86).max(0.0);
        surface.height = height;
        surface.edge = edge;
        surface.normal = normal;
        // A normal-dependent projection bends the light in different
        // directions on opposing slopes, unlike the old uniform dome shift.
        surface.sample = (
            u + (normal.0 * radius * 0.95 + height * 0.25) / aspect,
            v + normal.1 * radius * 0.95 - height * 0.15,
        );
        surface.base = 0.095 + dome * 0.055 + diffuse * 0.055;
        surface.light_gain = 0.40 + diffuse * 0.52;
    }
    surface
}

/// Full translated Voronoi sites needed by Texture::new's normalized domain.
/// Evaluate each original f32 site expression once, including gx/gy before the
/// additions. Translating a wrapped, pre-rounded site would not be bit-exact.
struct CausticSites {
    sites: [[(f32, f32); 10]; 8],
}

impl CausticSites {
    fn new() -> Self {
        let mut sites = [[(0.0, 0.0); 10]; 8];
        for (row, site_row) in sites.iter_mut().enumerate() {
            let gy = row as i32 - 1;
            for (column, site) in site_row.iter_mut().enumerate() {
                let gx = column as i32 - 1;
                *site = (
                    gx as f32 + 0.22 + hash(gx.rem_euclid(8), gy.rem_euclid(6)) * 0.56,
                    gy as f32 + 0.22 + hash(gx.rem_euclid(8) + 88, gy.rem_euclid(6)) * 0.56,
                );
            }
        }
        Self { sites }
    }

    fn sample(&self, u: f32, v: f32) -> f32 {
        let sample_x = u * 8.0;
        let sample_y = v * 6.0;
        // Keep the exact scalar behavior for out-of-domain/rounded endpoints.
        // All ordinary Texture::new samples fall in this half-open rectangle.
        if !(0.0..8.0).contains(&sample_x) || !(0.0..6.0).contains(&sample_y) {
            return caustic_texture(u, v);
        }
        let cell_x = sample_x.floor() as usize;
        let cell_y = sample_y.floor() as usize;
        let mut nearest = f32::INFINITY;
        let mut second = f32::INFINITY;
        // Table index is signed lattice coordinate + 1. These slices enumerate
        // gy = cell_y-1..=cell_y+1, then gx = cell_x-1..=cell_x+1, exactly as R04.
        for row in &self.sites[cell_y..cell_y + 3] {
            for &(site_x, site_y) in &row[cell_x..cell_x + 3] {
                let distance = (sample_x - site_x).powi(2) + (sample_y - site_y).powi(2);
                if distance < nearest {
                    second = nearest;
                    nearest = distance;
                } else if distance < second {
                    second = distance;
                }
            }
        }
        let separation = (second - nearest) / (nearest.sqrt() + second.sqrt() + 0.0001);
        1.0 - smoothstep(0.015, 0.072, separation)
    }
}

fn caustic_texture(u: f32, v: f32) -> f32 {
    let sample_x = u * 8.0;
    let sample_y = v * 6.0;
    let cell_x = sample_x.floor() as i32;
    let cell_y = sample_y.floor() as i32;
    let mut nearest = f32::INFINITY;
    let mut second = f32::INFINITY;
    for gy in cell_y - 1..=cell_y + 1 {
        for gx in cell_x - 1..=cell_x + 1 {
            let site_x = gx as f32 + 0.22 + hash(gx.rem_euclid(8), gy.rem_euclid(6)) * 0.56;
            let site_y = gy as f32 + 0.22 + hash(gx.rem_euclid(8) + 88, gy.rem_euclid(6)) * 0.56;
            let distance = (sample_x - site_x).powi(2) + (sample_y - site_y).powi(2);
            if distance < nearest {
                second = nearest;
                nearest = distance;
            } else if distance < second {
                second = distance;
            }
        }
    }
    let separation = (second - nearest) / (nearest.sqrt() + second.sqrt() + 0.0001);
    1.0 - smoothstep(0.015, 0.072, separation)
}

fn stone_caustics(
    raster: &mut Raster,
    settings: StoneCausticsSettings,
    time: f32,
    surfaces: &[StoneSurface],
    texture: &Texture,
) {
    let scale = 100.0 / f32::from(settings.caustic_scale_percent);
    let drift_x = time * 0.044;
    let drift_y = time * 0.027;
    for y in 0..raster.height {
        let v = (y as f32 + 0.5) / raster.height as f32;
        let shear = (v * 8.0 - time * 0.34).sin() * 0.014;
        for x in 0..raster.width {
            let index = y * raster.width + x;
            let surface = surfaces[index];
            let light = texture.sample(
                surface.sample.0 * scale - drift_x + shear,
                surface.sample.1 * scale - drift_y,
            );
            raster.dots[index] = (surface.base + light * surface.light_gain)
                .max(surface.edge)
                .min(1.0);
        }
    }
}

fn cloudlets(raster: &mut Raster, settings: &AnimationSettings, time: f32) {
    // Retain the direct renderer for the Empty fallback and existing oracles.
    let mut columns = vec![[0.0; 10]; raster.width];
    cloudlets_prepared(raster, settings, time, &mut columns);
}

fn cloudlets_prepared(
    raster: &mut Raster,
    settings: &AnimationSettings,
    time: f32,
    columns: &mut [[f32; 10]],
) {
    // Both callers allocate this private workspace for the current raster width;
    // SceneKey includes dimensions, so a resized raster prepares a new workspace.
    debug_assert_eq!(columns.len(), raster.width);
    // AnimationFrame normalizes this control to 2..=10. Specializing the small
    // source loop retains its addition order and exposes a fixed loop bound.
    match settings.cloudlets.form_count {
        2 => cloudlets_with_sources::<2>(raster, settings, time, columns),
        3 => cloudlets_with_sources::<3>(raster, settings, time, columns),
        4 => cloudlets_with_sources::<4>(raster, settings, time, columns),
        5 => cloudlets_with_sources::<5>(raster, settings, time, columns),
        6 => cloudlets_with_sources::<6>(raster, settings, time, columns),
        7 => cloudlets_with_sources::<7>(raster, settings, time, columns),
        8 => cloudlets_with_sources::<8>(raster, settings, time, columns),
        9 => cloudlets_with_sources::<9>(raster, settings, time, columns),
        _ => cloudlets_with_sources::<10>(raster, settings, time, columns),
    }
}

fn cloudlets_with_sources<const SOURCE_COUNT: usize>(
    raster: &mut Raster,
    settings: &AnimationSettings,
    time: f32,
    columns: &mut [[f32; 10]],
) {
    let controls = settings.cloudlets;
    let aspect = raster.aspect();
    let orbit = f32::from(controls.orbit_radius_percent) / 100.0;
    let radius_scale = f32::from(controls.form_size_percent) / 100.0;
    let cohesion = f32::from(controls.cohesion_percent) / 100.0;
    let cohesion_scale = cohesion.sqrt();
    let field_low = 1.10 / cohesion_scale;
    let field_high = 1.36 / cohesion_scale;
    let mut sources = [(0.0, 0.0, 0.0, 0.0); SOURCE_COUNT];
    for (index, source) in sources.iter_mut().enumerate() {
        let seed = hash(index as i32, 459);
        let phase = index as f32 * TAU / SOURCE_COUNT as f32;
        let radius = (0.065 + seed * 0.025) * radius_scale;
        let squared_radius = radius * radius;
        *source = (
            0.5 + (time * 0.19 + phase).cos() * 0.25 * orbit,
            0.49 + (time * 0.23 + phase * 1.3).sin() * 0.19 * orbit,
            squared_radius,
            squared_radius * (0.18 + cohesion * 0.08),
        );
    }
    // Preserve each f32 expression and source accumulation order. Only the
    // lifetime of the squared terms changes: x uses W*S rather than
    // W*H*S evaluations; y uses H*S. This is not a sampled/coarse field.
    for (x, column) in columns.iter_mut().enumerate() {
        let u = (x as f32 + 0.5) / raster.width as f32;
        for (index, &(cx, _, _, _)) in sources.iter().enumerate() {
            column[index] = ((u - cx) * aspect).powi(2);
        }
    }
    for y in 0..raster.height {
        let v = (y as f32 + 0.5) / raster.height as f32;
        let mut rows = [0.0; SOURCE_COUNT];
        for (index, &(_, cy, _, _)) in sources.iter().enumerate() {
            rows[index] = (v - cy).powi(2);
        }
        for (x, column) in columns.iter().enumerate() {
            let mut field = 0.0;
            for (index, &(_, _, squared_radius, softened_radius)) in sources.iter().enumerate() {
                let distance_squared = column[index] + rows[index];
                field += squared_radius / (distance_squared + softened_radius);
            }
            // Raster::field applies this outer clamp in the R04 renderer.
            raster.dots[y * raster.width + x] =
                (smoothstep(field_low, field_high, field) * 0.91).clamp(0.0, 1.0);
        }
    }
}

fn ripple_phases(raster: &Raster, settings: TwoRipplesSettings) -> Vec<RipplePhase> {
    let separation = f32::from(settings.source_separation_percent) / 100.0;
    let frequency = 28.0 * 100.0 / f32::from(settings.wavelength_percent);
    let damping = f32::from(settings.damping_percent) / 100.0;
    let mut phases = Vec::with_capacity(raster.dots.len());
    for y in 0..raster.height {
        let v = (y as f32 + 0.5) / raster.height as f32;
        for x in 0..raster.width {
            let u = (x as f32 + 0.5) / raster.width as f32;
            let distance_a = ((u - (0.5 - separation * 0.5)) * raster.aspect()).hypot(v - 0.44);
            let distance_b = ((u - (0.5 + separation * 0.5)) * raster.aspect()).hypot(v - 0.57);
            phases.push(RipplePhase {
                a: (distance_a * frequency).sin_cos(),
                b: (distance_b * frequency + 0.6).sin_cos(),
                attenuation_a: 1.0 / (1.0 + distance_a * damping * 1.5),
                attenuation_b: 1.0 / (1.0 + distance_b * damping * 1.5),
            });
        }
    }
    phases
}

fn two_ripples(
    raster: &mut Raster,
    settings: TwoRipplesSettings,
    time: f32,
    phases: &[RipplePhase],
) {
    let temporal = (time * 1.35).sin_cos();
    let interference = f32::from(settings.interference_percent) / 100.0;
    for (dot, phase) in raster.dots.iter_mut().zip(phases) {
        let first = traveling(phase.a, temporal);
        let second = traveling(phase.b, temporal);
        // Keep every crest; only its intensity responds to the other field.
        // Destructive interference never thresholds away a whole arc.
        let crest_a = smoothstep(0.66, 0.95, first);
        let crest_b = smoothstep(0.66, 0.95, second);
        *dot = (crest_a * (0.64 + second * 0.16 * interference) * phase.attenuation_a
            + crest_b * (0.64 + first * 0.16 * interference) * phase.attenuation_b)
            .min(1.0);
    }
}

fn prepare_pond(raster: &mut Raster, settings: QuietPondSettings) {
    let aspect = raster.aspect();
    let pads = pond_pads(settings, aspect);
    // One scratch allocation for the whole preparation, not one per row.
    let mut row_pads = Vec::with_capacity(pads.len());
    for y in 0..raster.height {
        let v = (y as f32 + 0.5) / raster.height as f32;
        pond_row_candidates(&pads, v, &mut row_pads);
        for x in 0..raster.width {
            let u = (x as f32 + 0.5) / raster.width as f32;
            // Keep the exact leaf shader, sorted overlap order and field clamp.
            raster.dots[y * raster.width + x] = pond_light(&row_pads, aspect, u, v).clamp(0.0, 1.0);
        }
    }
}

fn pond_row_candidates(
    pads: &[(f32, f32, f32, f32)],
    v: f32,
    candidates: &mut Vec<(f32, f32, f32, f32)>,
) {
    candidates.clear();
    for &pad in pads {
        let (_, cy, radius, _) = pad;
        let dy = (v - cy) * 1.13;
        // Deliberately much wider than the shader's 1.08-radius support.
        // Normalized pond radii are positive normal floats: outside two radii,
        // hypot(dx, dy) / radius cannot approach the 1.08 boundary. Do not
        // tighten this to the shading boundary or replace hypot in pond_light.
        // Retain nonpositive/nonfinite radii or nonfinite vertical differences.
        if !(radius > 0.0 && radius.is_finite() && dy.is_finite() && dy.abs() > radius * 2.0) {
            candidates.push(pad);
        }
    }
}

/// Deterministic leaf positions. Rooted mode is a visual spacing heuristic:
/// rhizome groups constrain reach; bounded candidate selection avoids piling
/// every leaf on one root without claiming a botanical growth simulation.
fn pond_pads(settings: QuietPondSettings, aspect: f32) -> Vec<(f32, f32, f32, f32)> {
    let scale = f32::from(settings.pad_size_percent) / 100.0;
    let count = usize::from(settings.pad_count);
    let mut pads: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(count);
    let groups = count.div_ceil(8).clamp(2, 8);
    for index in 0..count {
        let seed = hash(index as i32, 723);
        let radius = (0.038 + seed * 0.029) * scale;
        let mut center = (
            0.10 + hash(index as i32, 498) * 0.80,
            0.13 + hash(index as i32, 931) * 0.73,
        );
        if settings.natural_placement {
            let group = index % groups;
            let root = (
                0.18 + hash(group as i32, 1943) * 0.64,
                0.19 + hash(group as i32, 2839) * 0.62,
            );
            let mut best_clearance = f32::NEG_INFINITY;
            for candidate in 0..30 {
                let key = (index * 30 + candidate) as i32;
                let angle = hash(key, 1583) * TAU;
                let reach = 0.025 + hash(key, 1709).sqrt() * 0.18;
                let point = (
                    (root.0 + angle.cos() * reach / aspect.max(0.5)).clamp(0.07, 0.93),
                    (root.1 + angle.sin() * reach).clamp(0.08, 0.92),
                );
                let clearance = pads
                    .iter()
                    .map(|&(x, y, r, _)| {
                        ((point.0 - x) * aspect).hypot((point.1 - y) * 1.13) - r - radius
                    })
                    .fold(f32::INFINITY, f32::min);
                if clearance > best_clearance {
                    best_clearance = clearance;
                    center = point;
                }
                if clearance >= radius * 0.08 {
                    break;
                }
            }
        }
        pads.push((center.0, center.1, radius, hash(index as i32, 843) * TAU));
    }
    // Screen depth establishes which opaque leaf covers another.
    pads.sort_by(|left, right| left.1.total_cmp(&right.1));
    pads
}

fn pond_light(pads: &[(f32, f32, f32, f32)], aspect: f32, u: f32, v: f32) -> f32 {
    let mut light: f32 = 0.0;
    for &(cx, cy, radius, angle) in pads {
        let dx = (u - cx) * aspect;
        let dy = (v - cy) * 1.13;
        let distance = dx.hypot(dy) / radius;
        if distance > 1.08 {
            continue;
        }
        let theta = dy.atan2(dx);
        let relative = (theta - angle + PI).rem_euclid(TAU) - PI;
        if relative.abs() < 0.22 && distance > 0.10 {
            continue;
        }
        let edge = (1.0 - (distance - 1.0).abs() / 0.085).max(0.0) * 0.75;
        let body = if distance < 1.0 {
            0.11 + (1.0 - distance) * 0.08
        } else {
            0.0
        };
        let vein = if distance > 0.13 && distance < 0.89 {
            smoothstep(0.994, 1.0, (theta * 9.0 + angle).cos()) * 0.36
        } else {
            0.0
        };
        let leaf = edge.max(body + vein);
        // The front body is opaque even where its own veins are dim. Outside
        // the body, retain the bright rim without erasing uncovered water.
        light = if distance < 1.0 {
            leaf
        } else {
            light.max(leaf)
        };
    }
    light
}

fn quiet_pond(
    raster: &mut Raster,
    settings: QuietPondSettings,
    time: f32,
    base: &[f32],
    columns: &[WaterColumn],
) {
    let drift = time * f32::from(settings.drift_percent) / 100.0;
    let strength = f32::from(settings.ripple_strength_percent) / 100.0;
    for y in 0..raster.height {
        let v = (y as f32 + 0.5) / raster.height as f32;
        let broad = (drift * 0.79 - v * 73.0).sin_cos();
        let fine = (drift * -0.43 - v * 109.0).sin_cos();
        for (x, column) in columns.iter().enumerate() {
            let index = y * raster.width + x;
            if base[index] > 0.0 {
                raster.dots[index] = base[index];
                continue;
            }
            let wave = traveling(column.broad, broad) * 0.74 + traveling(column.fine, fine) * 0.26;
            let glint = smoothstep(0.78, 0.96, wave) * strength * 0.54;
            raster.dots[index] = glint;
        }
    }
}

#[cfg(test)]
mod optimization_fidelity_tests {
    use super::*;
    use crate::background_animation::CloudletSettings;

    fn raster_at(width: usize, height: usize) -> Raster {
        let mut raster = Raster::default();
        raster.resize(width, height);
        raster
    }

    fn assert_identical_dots(expected: &Raster, actual: &Raster) {
        assert_eq!(
            (expected.width, expected.height),
            (actual.width, actual.height)
        );
        for (index, (expected, actual)) in expected.dots.iter().zip(&actual.dots).enumerate() {
            assert_eq!(expected.to_bits(), actual.to_bits(), "dot {index}");
        }
    }

    // These reference bodies are frozen from the supplied pre-optimization
    // working tree (scenes.rs SHA256 156e8f77d8d28e0d2064ea35bcdf37406b93757275a2f4e62b9b6d91e29939bd).

    fn reference_cloudlets(raster: &mut Raster, settings: &AnimationSettings, time: f32) {
        let controls = settings.cloudlets;
        let aspect = raster.aspect();
        let count = usize::from(controls.form_count);
        let orbit = f32::from(controls.orbit_radius_percent) / 100.0;
        let radius_scale = f32::from(controls.form_size_percent) / 100.0;
        let cohesion = f32::from(controls.cohesion_percent) / 100.0;
        let cohesion_scale = cohesion.sqrt();
        let field_low = 1.10 / cohesion_scale;
        let field_high = 1.36 / cohesion_scale;
        let sources: Vec<_> = (0..count)
            .map(|index| {
                let seed = hash(index as i32, 459);
                let phase = index as f32 * TAU / count as f32;
                (
                    0.5 + (time * 0.19 + phase).cos() * 0.25 * orbit,
                    0.49 + (time * 0.23 + phase * 1.3).sin() * 0.19 * orbit,
                    (0.065 + seed * 0.025) * radius_scale,
                )
            })
            .collect();
        raster.field(|u, v| {
            let mut field = 0.0;
            for &(cx, cy, radius) in &sources {
                let distance_squared = ((u - cx) * aspect).powi(2) + (v - cy).powi(2);
                field += radius * radius
                    / (distance_squared + radius * radius * (0.18 + cohesion * 0.08));
            }
            smoothstep(field_low, field_high, field) * 0.91
        });
    }

    fn reference_kelp(
        raster: &mut Raster,
        settings: KelpSettings,
        time: f32,
        plants: &[KelpPlant],
    ) {
        let aspect = raster.aspect();
        let strength = f32::from(settings.current_strength_percent) / 100.0;
        // Roots begin below the viewport. There is deliberately no floor stroke.
        for plant in plants {
            let position = |fraction| kelp_position(plant, fraction, time, strength, aspect);
            raster.curve(28, 0.26, plant.light, position);
            for leaf in 1..=plant.leaves {
                let fraction = leaf as f32 / (plant.leaves + 2) as f32;
                let base = position(fraction);
                let side = if leaf % 2 == 0 { 1.0 } else { -1.0 };
                let length = plant.leaf_length * (1.0 - fraction * 0.40);
                let current = (time * 0.62 - fraction * 2.8 + plant.phase).sin() * strength;
                let twist = (time * 0.83 * strength - fraction * 4.5 + plant.phase).sin();
                let steps = ((length * raster.height as f32 / 2.0) as usize).clamp(5, 12);
                for edge in [-1.0, 1.0] {
                    raster.curve(
                        steps,
                        0.19,
                        plant.light * (0.83 + 0.17 * twist.abs()),
                        |along| {
                            let taper = (along * PI).sin();
                            (
                                base.0
                                    + (side * length * along
                                        + current * length * along * along * 0.54)
                                        / aspect,
                                base.1 - length * (along * 0.46 + along * along * 0.22)
                                    + edge * length * 0.13 * taper * (0.45 + twist * 0.35),
                            )
                        },
                    );
                }
            }
        }
    }

    #[test]
    fn fixed_cloudlet_sources_preserve_every_dot_at_all_legal_counts() {
        for (width, height) in [(2, 4), (31, 17), (160, 96), (480, 320)] {
            for form_count in 2..=10 {
                for (form_size_percent, cohesion_percent, orbit_radius_percent) in
                    [(50, 25, 25), (100, 100, 100), (175, 200, 150)]
                {
                    let settings = AnimationSettings {
                        cloudlets: CloudletSettings {
                            form_count,
                            form_size_percent,
                            cohesion_percent,
                            orbit_radius_percent,
                        },
                        ..Default::default()
                    };
                    for time in [0.0, 1.0 / 12.0, 7.0] {
                        let mut expected = raster_at(width, height);
                        let mut actual = raster_at(width, height);
                        reference_cloudlets(&mut expected, &settings, time);
                        cloudlets(&mut actual, &settings, time);
                        assert_identical_dots(&expected, &actual);
                    }
                }
            }
        }
    }

    #[test]
    fn paired_kelp_edges_preserve_every_dot_including_zero_current() {
        let presets = [
            KelpSettings::default(),
            KelpSettings {
                plant_density_percent: 25,
                current_strength_percent: 0,
                leaf_length_percent: 50,
                cluster_percent: 0,
            },
            KelpSettings {
                plant_density_percent: 200,
                current_strength_percent: 200,
                leaf_length_percent: 175,
                cluster_percent: 100,
            },
            KelpSettings {
                current_strength_percent: 0,
                ..Default::default()
            },
        ];
        for (width, height) in [(2, 4), (31, 17), (160, 96), (480, 320)] {
            for settings in presets {
                let shape = raster_at(width, height);
                let plants = kelp_plants(&shape, settings);
                for time in [0.0, 1.0 / 12.0, 7.0, 24.0] {
                    let mut expected = raster_at(width, height);
                    let mut actual = raster_at(width, height);
                    reference_kelp(&mut expected, settings, time, &plants);
                    kelp(&mut actual, settings, time, &plants);
                    assert_identical_dots(&expected, &actual);
                }
            }
        }
    }
}

#[cfg(test)]
mod moon_ribbon_fidelity_tests {
    use super::*;

    fn reference_moonlit_water(
        raster: &mut Raster,
        settings: MoonlitWaterSettings,
        time: f32,
        base: &[f32],
        columns: &[WaterColumn],
    ) {
        raster.dots.copy_from_slice(base);
        let strength = f32::from(settings.wave_strength_percent) / 100.0;
        let scale = f32::from(settings.ripple_scale_percent) / 100.0;
        let reflection = f32::from(settings.reflection_width_percent) / 100.0;
        for y in 0..raster.height {
            let v = (y as f32 + 0.5) / raster.height as f32;
            if v < 0.43 {
                continue;
            }
            let depth = (v - 0.43) / 0.57;
            // Perspective increases band spacing toward the viewer. Both phases
            // are transported, while columns supply cached crossing wave normals.
            let perspective = 42.0 * (depth + 0.07).ln();
            let broad = (time * strength * 1.10 - perspective / scale).sin_cos();
            let fine = (-time * strength * 0.73 - depth * 107.0 / scale).sin_cos();
            // The reflection spreads into a moving sheet as it approaches the
            // viewer. Independent depth waves keep it from reading as a fixed
            // vertical pillar, even when the water controls are set low.
            let center = 0.54
                + ((depth * 8.5 - time * strength * 0.62).sin() * 0.045
                    + (depth * 19.0 + time * strength * 0.39).sin() * 0.022)
                    * depth
                    * strength;
            let half_width = (0.018 + depth * depth * 0.22) * reflection;
            for (x, column) in columns.iter().enumerate() {
                let u = (x as f32 + 0.5) / raster.width as f32;
                let wave_a = traveling(column.broad, broad);
                let wave_b = traveling(column.fine, fine);
                let local_center = center
                    + wave_b * depth * strength * 0.035
                    + (depth * 47.0 + time * strength * 0.8).sin() * depth * strength * 0.012;
                let ribbon =
                    1.0 - smoothstep(half_width * 0.42, half_width, (u - local_center).abs());
                let crest = smoothstep(0.02, 0.72, (wave_a * 0.68 + wave_b * 0.32) * strength);
                let broken_surface = wave_b + (depth * 83.0 - time * strength * 1.7).sin() * 0.34;
                let fragments = 0.18 + 0.82 * smoothstep(-0.40, 0.48, broken_surface);
                let reflection_light = ribbon * (0.12 + crest * 0.76) * fragments;
                let outside_wavelets =
                    smoothstep(0.86, 0.99, wave_a) * 0.10 * (0.3 + depth * 0.7) * strength.min(1.0);
                raster.dots[y * raster.width + x] = reflection_light.max(outside_wavelets);
            }
        }
    }

    #[test]
    fn zero_ribbon_shortcut_preserves_every_moon_dot() {
        let settings = [
            MoonlitWaterSettings::default(),
            MoonlitWaterSettings {
                wave_strength_percent: 0,
                ripple_scale_percent: 50,
                reflection_width_percent: 25,
                moon_size_percent: 50,
            },
            MoonlitWaterSettings {
                wave_strength_percent: 200,
                ripple_scale_percent: 200,
                reflection_width_percent: 200,
                moon_size_percent: 150,
            },
        ];
        for (width, height) in [(2, 4), (31, 17), (160, 96)] {
            let columns = water_columns(width, 19.0, 47.0);
            for controls in settings {
                let mut prepared = Raster::default();
                prepared.resize(width, height);
                prepare_moon(&mut prepared, controls);
                let base = prepared.dots;
                for time in [0.0, 1.0 / 30.0, 7.0, 24.0] {
                    let mut expected = Raster::default();
                    expected.resize(width, height);
                    expected.dots.fill(0.77);
                    let mut actual = Raster::default();
                    actual.resize(width, height);
                    actual.dots.fill(0.77);
                    reference_moonlit_water(&mut expected, controls, time, &base, &columns);
                    moonlit_water(&mut actual, controls, time, &base, &columns);
                    for (index, (left, right)) in expected.dots.iter().zip(&actual.dots).enumerate()
                    {
                        assert_eq!(
                            left.to_bits(),
                            right.to_bits(),
                            "dot {index}, {width}x{height}, {controls:?}, t={time}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod pond_overhaul_tests {
    use super::*;
    #[test]
    fn rooted_placement_is_repeatable_bounded_and_distinct() {
        let settings = QuietPondSettings {
            pad_count: 64,
            natural_placement: true,
            ..Default::default()
        };
        let pads = pond_pads(settings, 2.0);
        assert_eq!(pads.len(), 64);
        assert_eq!(pads, pond_pads(settings, 2.0));
        assert!(pads
            .iter()
            .all(|&(x, y, radius, _)| (0.07..=0.93).contains(&x)
                && (0.08..=0.92).contains(&y)
                && radius > 0.0));
        assert!(pads.windows(2).all(|pair| pair[0].1 <= pair[1].1));
        assert_ne!(
            pads,
            pond_pads(
                QuietPondSettings {
                    natural_placement: false,
                    ..settings
                },
                2.0
            )
        );
    }
    #[test]
    fn overhaul_front_leaf_hides_the_rear_edge() {
        let rear = (0.3, 0.5, 0.2, PI);
        let front = (0.5, 0.5, 0.25, PI);
        let front_only = pond_light(&[front], 1.0, 0.5, 0.5);
        assert_eq!(
            pond_light(&[rear, front], 1.0, 0.5, 0.5),
            front_only,
            "rear leaf edge must not show through the front leaf center"
        );
    }
}

#[cfg(test)]
pub(super) mod r05_cloudlet_tests {
    include!("r05_cloudlet_tests.rs");
}

#[cfg(test)]
pub(super) mod r06_pond_tests {
    include!("r06_pond_tests.rs");
}

#[cfg(test)]
pub(super) mod r07_caustic_tests {
    include!("r07_caustic_tests.rs");
}

#[cfg(test)] // R08 original-only oracle and shared fidelity tests.
pub(super) mod r08_ridge_oracle {
    // Descendant access to current scene internals.
    include!("r08_ridge_oracle.rs"); // Same complete file in both arms.
} // End R08 common tests.

#[cfg(test)] // R08 tests requiring the exact B1 private helper.
mod r08_ridge_candidate {
    // Candidate-only registration.
    include!("r08_ridge_candidate.rs"); // Complete helper and mutation tests.
} // End R08 candidate tests.
