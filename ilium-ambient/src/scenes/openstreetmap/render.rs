//! Pure local-metre geometry rasterization; no sources, timers or I/O.
use super::settings::OpenStreetMapSettings;
use crate::raster::Raster;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapLayer {
    Roads,
    Buildings,
    Water,
    GreenSpace,
    Railways,
    PointsOfInterest,
}
#[derive(Debug, Clone)]
pub enum MapShape {
    Line(Vec<[f64; 2]>),
    Area {
        outer: Vec<[f64; 2]>,
        holes: Vec<Vec<[f64; 2]>>,
    },
    Point([f64; 2]),
}
#[derive(Debug, Clone)]
pub struct MapFeature {
    pub layer: MapLayer,
    pub shape: MapShape,
}

pub fn draw_map(
    raster: &mut Raster,
    features: &[MapFeature],
    settings: &OpenStreetMapSettings,
    seconds: f64,
) {
    raster.dots.fill(0.0);
    if raster.width == 0 || raster.height == 0 || settings.brightness_percent <= 0 {
        return;
    }
    let width = f64::from(settings.map_width_meters.clamp(300, 2000));
    let height = width * raster.height as f64 / raster.width as f64;
    let camera = camera_offset(settings, seconds);
    let brightness = settings.brightness_percent.clamp(0, 200) as f32 / 100.;
    let radius = 0.45 * settings.line_width_percent.clamp(25, 200) as f32 / 100.;
    for feature in features {
        if !is_enabled(feature.layer, settings) {
            continue;
        }
        let intensity = match feature.layer {
            MapLayer::Roads => 0.95,
            MapLayer::Railways => 0.7,
            MapLayer::Buildings => 0.55,
            MapLayer::Water => 0.45,
            MapLayer::GreenSpace => 0.24,
            MapLayer::PointsOfInterest => 0.95,
        } * brightness;
        let project = |point: [f64; 2]| {
            [
                (point[0] - camera[0]) / width + 0.5,
                0.5 - (point[1] - camera[1]) / height,
            ]
        };
        match &feature.shape {
            MapShape::Line(points) => draw_line(raster, points, &project, radius, intensity),
            MapShape::Point(point) => {
                let point = project(*point);
                if point
                    .iter()
                    .all(|p| p.is_finite() && (0.0..=1.0).contains(p))
                {
                    raster.line(
                        (point[0] as f32, point[1] as f32),
                        (point[0] as f32, point[1] as f32),
                        radius.max(0.65),
                        intensity.min(1.),
                    );
                }
            }
            MapShape::Area { outer, holes } => {
                fill_area(
                    raster,
                    outer,
                    holes,
                    &project,
                    (intensity
                        * if feature.layer == MapLayer::Buildings {
                            0.3
                        } else {
                            1.
                        })
                    .min(1.),
                );
                draw_line(raster, outer, &project, radius, intensity.min(1.));
                for hole in holes {
                    draw_line(raster, hole, &project, radius, intensity.min(1.));
                }
            }
        }
    }
}

pub fn camera_offset(settings: &OpenStreetMapSettings, seconds: f64) -> [f64; 2] {
    if settings.camera == 0 || settings.camera_speed <= 0 {
        return [0., 0.];
    }
    let seconds = if seconds.is_finite() {
        seconds.max(0.)
    } else {
        0.
    };
    let phase = (seconds * f64::from(settings.camera_speed.clamp(0, 300)) / 100. / 120.).fract()
        * std::f64::consts::TAU;
    let amplitude = (f64::from(settings.map_width_meters.clamp(300, 2000)) * 0.12).min(150.);
    match settings.camera {
        1 => [amplitude * phase.cos(), amplitude * phase.sin()],
        _ => [amplitude * phase.sin(), 0.],
    }
}

fn is_enabled(layer: MapLayer, s: &OpenStreetMapSettings) -> bool {
    match layer {
        MapLayer::Roads => s.roads,
        MapLayer::Buildings => s.buildings,
        MapLayer::Water => s.water,
        MapLayer::GreenSpace => s.green_space,
        MapLayer::Railways => s.railways,
        MapLayer::PointsOfInterest => s.points_of_interest,
    }
}

fn draw_line(
    raster: &mut Raster,
    points: &[[f64; 2]],
    project: &impl Fn([f64; 2]) -> [f64; 2],
    radius: f32,
    intensity: f32,
) {
    for segment in points.windows(2) {
        let a = project(segment[0]);
        let b = project(segment[1]);
        if let Some((a, b)) = clip_segment(a, b) {
            raster.line(
                (a[0] as f32, a[1] as f32),
                (b[0] as f32, b[1] as f32),
                radius,
                intensity.min(1.),
            );
        }
    }
}

/// Liang–Barsky clipping bounds raster arithmetic even for far-away OSM ways.
fn clip_segment(a: [f64; 2], b: [f64; 2]) -> Option<([f64; 2], [f64; 2])> {
    if a.iter().chain(b.iter()).any(|p| !p.is_finite()) {
        return None;
    }
    let delta = [b[0] - a[0], b[1] - a[1]];
    let p = [-delta[0], delta[0], -delta[1], delta[1]];
    let q = [a[0] + 0.02, 1.02 - a[0], a[1] + 0.02, 1.02 - a[1]];
    let mut lo: f64 = 0.;
    let mut hi: f64 = 1.;
    for (p, q) in p.into_iter().zip(q) {
        if p == 0. {
            if q < 0. {
                return None;
            }
            continue;
        }
        let t = q / p;
        if p < 0. {
            lo = lo.max(t);
        } else {
            hi = hi.min(t);
        }
        if lo > hi {
            return None;
        }
    }
    Some((
        [a[0] + lo * delta[0], a[1] + lo * delta[1]],
        [a[0] + hi * delta[0], a[1] + hi * delta[1]],
    ))
}

/// Even-odd scanlines across outer and inner rings preserve real polygon holes.
/// Only explicitly closed complete rings are filled; incomplete ways stay lines.
fn fill_area(
    raster: &mut Raster,
    outer: &[[f64; 2]],
    holes: &[Vec<[f64; 2]>],
    project: &impl Fn([f64; 2]) -> [f64; 2],
    intensity: f32,
) {
    if outer.len() < 4 || outer.first() != outer.last() {
        return;
    }
    let rings: Vec<Vec<[f64; 2]>> = std::iter::once(outer)
        .chain(holes.iter().map(Vec::as_slice))
        .filter(|ring| ring.len() >= 4 && ring.first() == ring.last())
        .map(|ring| ring.iter().copied().map(project).collect())
        .collect();
    if rings.iter().flatten().flatten().any(|p| !p.is_finite()) {
        return;
    }
    let ymin = rings[0].iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let ymax = rings[0]
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);
    let first = (ymin * raster.height as f64).floor().max(0.) as usize;
    let last = (ymax * raster.height as f64).ceil().max(0.) as usize;
    let mut crossings = Vec::new();
    for y in first.min(raster.height)..last.min(raster.height) {
        let scan = (y as f64 + 0.5) / raster.height as f64;
        crossings.clear();
        for ring in &rings {
            for edge in ring.windows(2) {
                let (a, b) = (edge[0], edge[1]);
                if (a[1] <= scan && b[1] > scan) || (b[1] <= scan && a[1] > scan) {
                    crossings.push(a[0] + (scan - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                }
            }
        }
        crossings.sort_by(f64::total_cmp);
        for span in crossings.chunks_exact(2) {
            let start = (span[0] * raster.width as f64 - 0.5).ceil().max(0.) as usize;
            let end = (span[1] * raster.width as f64 - 0.5).ceil().max(0.) as usize;
            for x in start.min(raster.width)..end.min(raster.width) {
                let dot = &mut raster.dots[y * raster.width + x];
                *dot = dot.max(intensity);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn raster() -> Raster {
        let mut r = Raster::default();
        r.resize(100, 100);
        r
    }
    fn polygon(layer: MapLayer) -> MapFeature {
        MapFeature {
            layer,
            shape: MapShape::Area {
                outer: vec![
                    [-400., -400.],
                    [400., -400.],
                    [400., 400.],
                    [-400., 400.],
                    [-400., -400.],
                ],
                holes: vec![vec![
                    [-100., -100.],
                    [100., -100.],
                    [100., 100.],
                    [-100., 100.],
                    [-100., -100.],
                ]],
            },
        }
    }
    fn fixed() -> OpenStreetMapSettings {
        OpenStreetMapSettings {
            camera: 0,
            map_width_meters: 1000,
            ..Default::default()
        }
    }
    #[test]
    fn water_polygon_has_real_empty_hole() {
        let mut r = raster();
        draw_map(&mut r, &[polygon(MapLayer::Water)], &fixed(), 0.);
        assert!(r.dots[20 * 100 + 20] > 0.);
        assert_eq!(r.dots[50 * 100 + 50], 0.);
        assert_eq!(r.dots[0], 0.);
    }
    #[test]
    fn disabling_each_layer_removes_only_its_geometry() {
        for layer in [
            MapLayer::Roads,
            MapLayer::Buildings,
            MapLayer::Water,
            MapLayer::GreenSpace,
            MapLayer::Railways,
            MapLayer::PointsOfInterest,
        ] {
            let mut s = fixed();
            s.points_of_interest = true;
            let shape = if layer == MapLayer::PointsOfInterest {
                MapShape::Point([0., 0.])
            } else {
                MapShape::Line(vec![[-350., 0.], [350., 0.]])
            };
            let features = vec![MapFeature { layer, shape }];
            let mut r = raster();
            draw_map(&mut r, &features, &s, 0.);
            assert!(r.dots.iter().any(|v| *v > 0.), "{layer:?}");
            match layer {
                MapLayer::Roads => s.roads = false,
                MapLayer::Buildings => s.buildings = false,
                MapLayer::Water => s.water = false,
                MapLayer::GreenSpace => s.green_space = false,
                MapLayer::Railways => s.railways = false,
                MapLayer::PointsOfInterest => s.points_of_interest = false,
            }
            draw_map(&mut r, &features, &s, 0.);
            assert!(r.dots.iter().all(|v| *v == 0.), "{layer:?}");
        }
    }
    #[test]
    fn fixed_camera_stays_stable_pan_changes_map_and_zero_speed_holds() {
        let f = vec![MapFeature {
            layer: MapLayer::Roads,
            shape: MapShape::Line(vec![[-300., -300.], [300., 300.]]),
        }];
        let mut s = fixed();
        let mut a = raster();
        let mut b = raster();
        draw_map(&mut a, &f, &s, 0.);
        draw_map(&mut b, &f, &s, 30.);
        assert_eq!(a.dots, b.dots);
        s.camera = 1;
        draw_map(&mut a, &f, &s, 0.);
        draw_map(&mut b, &f, &s, 30.);
        assert!(a.dots != b.dots, "panning must move the map");
        s.camera_speed = 0;
        draw_map(&mut a, &f, &s, 0.);
        draw_map(&mut b, &f, &s, 30.);
        assert_eq!(a.dots, b.dots);
    }
    #[test]
    fn mixed_layers_retain_unrelated_dots_and_east_west_pan_is_periodic() {
        let f = vec![
            MapFeature {
                layer: MapLayer::Roads,
                shape: MapShape::Line(vec![[-300., 300.], [300., 300.]]),
            },
            MapFeature {
                layer: MapLayer::Railways,
                shape: MapShape::Line(vec![[-300., -300.], [300., -300.]]),
            },
        ];
        let mut s = fixed();
        let mut r = raster();
        draw_map(&mut r, &f, &s, 0.);
        assert!(r.dots[20 * 100 + 50] > 0. && r.dots[80 * 100 + 50] > 0.);
        s.roads = false;
        draw_map(&mut r, &f, &s, 0.);
        assert_eq!(r.dots[20 * 100 + 50], 0.);
        assert!(r.dots[80 * 100 + 50] > 0.);
        s.camera = 2;
        let mut first = raster();
        draw_map(&mut first, &f, &s, 0.);
        draw_map(&mut r, &f, &s, 30.);
        assert!(first.dots != r.dots);
        draw_map(&mut r, &f, &s, 120.);
        assert_eq!(first.dots, r.dots);
    }
    #[test]
    fn clipped_crossing_lines_and_polygons_draw_without_inventing_open_area() {
        let mut s = fixed();
        s.line_width_percent = 25;
        let mut r = raster();
        let line = MapFeature {
            layer: MapLayer::Roads,
            shape: MapShape::Line(vec![[-5000., 0.], [5000., 0.]]),
        };
        draw_map(&mut r, &[line], &s, 0.);
        assert!(r.dots.iter().any(|v| *v > 0.));
        let outer = vec![
            [-2000., -2000.],
            [2000., -2000.],
            [2000., 2000.],
            [-2000., 2000.],
            [-2000., -2000.],
        ];
        let mut reversed = outer.clone();
        reversed.reverse();
        for ring in [outer, reversed] {
            let f = MapFeature {
                layer: MapLayer::Water,
                shape: MapShape::Area {
                    outer: ring,
                    holes: vec![],
                },
            };
            draw_map(&mut r, &[f], &s, 0.);
            assert!(r.dots.iter().all(|v| *v > 0.));
        }
        let f = MapFeature {
            layer: MapLayer::Water,
            shape: MapShape::Area {
                outer: vec![[-300., -300.], [300., -300.], [300., 300.]],
                holes: vec![],
            },
        };
        draw_map(&mut r, &[f], &s, 0.);
        assert_eq!(r.dots[50 * 100 + 50], 0.);
        for (w, h) in [(0, 12), (16, 0), (1, 1)] {
            r.resize(w, h);
            draw_map(&mut r, &[], &s, 0.);
            assert!(r.dots.iter().all(|v| v.is_finite()));
        }
    }
    #[test]
    fn brightness_zero_clears_dots_and_thin_map_is_finite_for_tiny_viewports() {
        let features = vec![polygon(MapLayer::Buildings)];
        let mut s = fixed();
        let mut r = raster();
        draw_map(&mut r, &features, &s, 0.);
        assert!(r.dots.iter().any(|v| *v > 0.));
        s.brightness_percent = 0;
        draw_map(&mut r, &features, &s, 0.);
        assert!(r.dots.iter().all(|v| *v == 0.));
        for (w, h) in [(0, 0), (2, 4), (160, 96)] {
            r.resize(w, h);
            draw_map(&mut r, &features, &fixed(), 0.);
            assert!(r.dots.iter().all(|v| v.is_finite() && *v >= 0. && *v <= 1.));
        }
    }
}
