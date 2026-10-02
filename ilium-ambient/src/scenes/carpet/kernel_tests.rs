use super::*;

fn sphere() -> Body {
    Body {
        from: [0.5, 0.5],
        to: [0.5, 0.5],
        radius: 0.12,
        height: 0.2,
    }
}
fn raster(w: usize, h: usize) -> Raster {
    let mut r = Raster::default();
    r.resize(w, h);
    r
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 3e-6, "{a} != {b}");
}
fn peak(r: &Raster, p: [f64; 2]) -> f32 {
    let x = (p[0] * r.width as f64) as isize;
    let y = (p[1] * r.height as f64) as isize;
    let mut value: f32 = 0.0;
    for yy in (y - 2).max(0)..=(y + 2).min(r.height as isize - 1) {
        for xx in (x - 2).max(0)..=(x + 2).min(r.width as isize - 1) {
            value = value.max(r.dots[yy as usize * r.width + xx as usize]);
        }
    }
    value
}
fn dense() -> Vec<Body> {
    (0..MAX_BODIES)
        .map(|i| {
            let t = i as f32 / (MAX_BODIES - 1) as f32;
            Body {
                from: [0.0, 0.1 + 0.8 * t],
                to: [1.0, 0.9 - 0.8 * t],
                radius: 0.15,
                height: 0.08 + (i % 17) as f32 * 0.002,
            }
        })
        .collect()
}

#[test]
fn analytic_sphere_capsule_and_compact_boundary() {
    let b = Body {
        radius: 0.1,
        ..sphere()
    };
    near(b.height_at([0.5, 0.5], 0.0), 0.2);
    near(b.height_at([0.55, 0.5], 0.0), 0.2 * 0.75 * 0.75);
    near(b.height_at([0.55, 0.5], 1.0), 0.2 * 0.75 * 0.75 * 0.75);
    near(b.height_at([0.6, 0.5], 0.5), 0.0);
    let tube = Body {
        from: [0.25, 0.5],
        to: [0.75, 0.5],
        ..b
    };
    near(tube.height_at([0.4, 0.5], 0.0), 0.2);
    near(
        tube.height_at([0.2, 0.5], 0.0),
        b.height_at([0.55, 0.5], 0.0),
    );
    near(tube.height_at([0.9, 0.5], 0.0), 0.0);
    let reverse = Body {
        from: tube.to,
        to: tube.from,
        ..tube
    };
    near(
        reverse.height_at([0.31, 0.57], 0.5),
        tube.height_at([0.31, 0.57], 0.5),
    );
    assert!(b.height_at([0.5999, 0.5], 0.0) < 1e-6);
    assert_eq!(tube.support(), Some(([0.15, 0.4], [0.85, 0.6])));
}

#[test]
fn fitted_camera_round_trips_corners_and_narrow_viewports() {
    for (w, h) in [(1, 1), (1, 2048), (2048, 1), (160, 96), (480, 320)] {
        for yaw in [0.0, 45.0, 137.0, 270.0] {
            for pitch in [5.0, 35.26439, 85.0] {
                let c = Camera::new(
                    w,
                    h,
                    &RenderOptions {
                        yaw,
                        pitch,
                        ..Default::default()
                    },
                )
                .unwrap();
                for p in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.19, 0.73]] {
                    let uv = c.project(p, 0.0).unwrap();
                    assert!(uv.into_iter().all(|v| (0.0..=1.0).contains(&v)));
                    let q = c.inverse_ground(uv).unwrap();
                    near(p[0], q[0]);
                    near(p[1], q[1]);
                    assert_eq!(c.project(p, 0.0).unwrap()[0], c.project(p, 1.0).unwrap()[0]);
                }
                assert!(c.inverse_ground([-0.1, 0.5]).is_none());
                assert!(c.inverse_ground([f64::NAN, 0.5]).is_none());
                assert!(c.project([f32::NAN, 0.5], 0.0).is_none());
            }
        }
    }
    let c = Camera::new(480, 320, &RenderOptions::default()).unwrap();
    assert!(c.inverse_ground([0.0, 0.0]).is_none());
}

#[test]
fn actual_flat_raster_has_parallel_equally_spaced_full_plane_lines() {
    let o = RenderOptions {
        yaw: 0.0,
        pitch: 30.0,
        spacing_in_dots: 8.0,
        ..Default::default()
    };
    let c = Camera::new(160, 96, &o).unwrap();
    let mut actual = raster(160, 96);
    render(&mut actual, &[], &o);
    let mut expected = raster(160, 96);
    let count = (0.5 * c.scale * c.sine_pitch / o.spacing_in_dots).floor() as i32;
    for k in -count..=count {
        let y = (c.origin[1] + k as f32 * o.spacing_in_dots) / 96.0;
        expected.line(
            ((80.0 - c.scale * 0.5) / 160.0, y),
            ((80.0 + c.scale * 0.5) / 160.0, y),
            0.5,
            1.0,
        );
    }
    assert!(actual.dots.iter().any(|v| *v > 0.8));
    assert!(actual
        .dots
        .iter()
        .zip(&expected.dots)
        .all(|(a, b)| (a - b).abs() < 0.001));
    let endpoints = [
        c.project([0.0, 0.5], 0.0).unwrap(),
        c.project([1.0, 0.5], 0.0).unwrap(),
    ];
    assert!(endpoints[1][0] - endpoints[0][0] > 0.7);
    let mut tilted = raster(160, 96);
    render(
        &mut tilted,
        &[],
        &RenderOptions {
            hatch_direction: 53.0,
            ..o
        },
    );
    assert_ne!(tilted.dots, actual.dots);
}

#[test]
fn spheres_and_capsules_really_lift_ink_and_remove_the_flat_center() {
    let o = RenderOptions {
        yaw: 0.0,
        spacing_in_dots: 24.0,
        ..Default::default()
    };
    let c = Camera::new(320, 200, &o).unwrap();
    let mut flat = raster(320, 200);
    render(&mut flat, &[], &o);
    for b in [
        sphere(),
        Body {
            from: [0.3, 0.5],
            to: [0.7, 0.5],
            ..sphere()
        },
    ] {
        let mut raised = raster(320, 200);
        render(&mut raised, &[b], &o);
        let top = c.project([0.5, 0.5], b.height).unwrap();
        assert!(peak(&raised, top) > 0.5);
        assert!(peak(&flat, top) < 0.05);
        assert!(peak(&raised, c.project([0.5, 0.5], 0.0).unwrap()) < 0.1);
        assert_ne!(raised.dots, flat.dots);
    }
}

#[test]
fn cached_maximum_matches_every_reference_vertex_and_the_spatial_error_bound() {
    let bodies: Vec<_> = (0..17)
        .map(|i| Body {
            from: [0.1 + (i % 5) as f32 * 0.18, 0.2 + (i / 5) as f32 * 0.18],
            to: [0.7, 0.55],
            radius: 0.005 + i as f32 * 0.009,
            height: 0.005 + i as f32 * 0.009,
        })
        .collect();
    let mut r = Renderer::default();
    r.prepare(&bodies, 0.5);
    let exact = |p| {
        bodies
            .iter()
            .map(|b| b.height_at(p, 0.5))
            .fold(0.0, f32::max)
    };
    for y in 0..SIDE {
        for x in 0..SIDE {
            near(
                r.heights[y * SIDE + x],
                exact([x as f32 / GRID as f32, y as f32 / GRID as f32]),
            );
        }
    }
    for i in 0..127 {
        let p = [(i * 37 % 127) as f32 / 127.0, (i * 61 % 131) as f32 / 131.0];
        assert!((r.height(p) - exact(p)).abs() <= 2.0_f32.sqrt() / GRID as f32 + 3e-6);
    }
    let tiny = Body {
        from: [128.5 / 256.0; 2],
        to: [128.5 / 256.0; 2],
        radius: 0.005,
        height: 0.005,
    };
    r.prepare(&[tiny], 0.5);
    assert!(r.height(tiny.from) > 0.001);
}

#[test]
fn all_4096_unique_overlapping_capsules_have_bounded_work_without_shortlists() {
    let mut bodies = dense();
    bodies[MAX_BODIES - 1] = Body {
        from: [0.05, 0.05],
        to: [0.95, 0.05],
        radius: 0.15,
        height: 0.6,
    };
    let mut r = Renderer::default();
    let mut image = raster(480, 320);
    r.render(
        &mut image,
        &bodies,
        &RenderOptions {
            spacing_in_dots: 2.0,
            ..Default::default()
        },
    );
    let stats = r.stats();
    assert_eq!(stats.unique_bodies, MAX_BODIES);
    assert!(!stats.rejected);
    assert!(stats.tile_tests <= MAX_BODIES * TILES * TILES);
    assert!(stats.body_evaluations < MAX_BODIES * SIDE * SIDE / 4);
    assert!(stats.samples <= stats.hatch_lines * 726);
    assert!(stats.raster_visits_bound <= stats.segments as u64 * 121);
    for (x, y) in [(128, 13), (128, 128), (30, 80), (220, 180)] {
        let p = [x as f32 / GRID as f32, y as f32 / GRID as f32];
        near(
            r.height(p),
            bodies
                .iter()
                .map(|b| b.height_at(p, 0.5))
                .fold(0.0, f32::max),
        );
    }
    r.render(&mut image, &bodies, &RenderOptions::default());
    assert!(!r.stats().rebuilt);
    assert_eq!(r.stats().body_evaluations, 0);
}

#[test]
fn duplicates_reordering_camera_changes_and_softness_have_correct_cache_ownership() {
    let mut bodies = vec![sphere(); MAX_BODIES];
    bodies[0].height = 0.3;
    let mut r = Renderer::default();
    let mut image = raster(160, 96);
    r.render(&mut image, &bodies, &RenderOptions::default());
    assert_eq!(r.stats().unique_bodies, 1);
    bodies.reverse();
    r.render(
        &mut image,
        &bodies,
        &RenderOptions {
            yaw: 80.0,
            ..Default::default()
        },
    );
    assert!(!r.stats().rebuilt);
    let capacities = (
        r.bodies.capacity(),
        r.next.capacity(),
        r.heights.capacity(),
        r.dirty.capacity(), // C1 replaces the old minima cache with a fixed dirty-tile mask.
    );
    r.render(
        &mut image,
        &bodies,
        &RenderOptions {
            softness: 1.0,
            ..Default::default()
        },
    );
    assert!(r.stats().rebuilt);
    assert_eq!(
        capacities,
        (
            r.bodies.capacity(),
            r.next.capacity(),
            r.heights.capacity(),
            r.dirty.capacity() // C1 retains the same warm-capacity assertion for the replacement mask.
        )
    );
    r.render(&mut image, &[], &RenderOptions::default());
    assert!(r.stats().rebuilt);
    assert!(r.heights.iter().all(|v| *v == 0.0));
    // Signed zero must not defeat the geometry-first, height-second ordering.
    r.prepare(
        &[
            Body {
                from: [-0.0, 0.5],
                to: [-0.0, 0.5],
                ..sphere()
            },
            Body {
                from: [0.0, 0.5],
                to: [0.0, 0.5],
                height: 0.3,
                ..sphere()
            },
        ],
        0.5,
    );
    assert_eq!(r.bodies.len(), 1);
    near(r.height([0.0, 0.5]), 0.3);
}

#[test]
fn zero_narrow_extreme_nonfinite_and_malformed_inputs_are_safe() {
    let bad = Body {
        from: [f32::NAN, 0.0],
        ..sphere()
    };
    assert!(bad.support().is_none());
    assert_eq!(bad.height_at([0.5; 2], 0.0), 0.0);
    for b in [
        Body {
            radius: 0.0,
            ..sphere()
        },
        Body {
            height: -1.0,
            ..sphere()
        },
        Body {
            radius: f32::INFINITY,
            ..sphere()
        },
        Body {
            height: f32::NAN,
            ..sphere()
        },
        Body {
            to: [2.0, 0.5],
            ..sphere()
        },
    ] {
        assert!(b.normalized().is_none());
    }
    let big = Body {
        radius: f32::MAX,
        height: f32::MAX,
        ..sphere()
    }
    .normalized()
    .unwrap();
    assert_eq!(
        (big.radius, big.height),
        (super::super::model::MAX_RADIUS, 1.0)
    );
    for (w, h) in [(0, 0), (0, 7), (1, 1), (1, 90), (90, 1), (2, 4)] {
        for value in [f32::NAN, f32::INFINITY, -f32::MAX, f32::MAX, 0.0] {
            let o = RenderOptions {
                yaw: value,
                pitch: value,
                zoom: value,
                hatch_direction: value,
                spacing_in_dots: value,
                line_width: value,
                height_scale: value,
                softness: value,
            };
            let mut image = raster(w, h);
            let mut r = Renderer::default();
            r.render(&mut image, &[bad, sphere()], &o);
            assert!(image
                .dots
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        }
    }
    let mut r = Renderer::default();
    let mut invalid = Raster {
        width: usize::MAX,
        height: 2,
        ..Raster::default()
    };
    r.render(&mut invalid, &[], &RenderOptions::default());
    assert!(r.stats().rejected);
    let mut image = raster(2, 4);
    r.render(
        &mut image,
        &vec![sphere(); MAX_BODIES + 1],
        &RenderOptions::default(),
    );
    assert!(r.stats().rejected);
    assert!(image.dots.iter().all(|v| *v == 0.0));
}

#[test]
#[ignore = "run explicitly in release mode; prints rebuild and cache-hit costs"]
fn release_measurement() {
    if cfg!(debug_assertions) {
        panic!("use cargo test --release");
    }
    let mut r = Renderer::default();
    let mut image = raster(480, 320);
    let mut bodies = dense();
    let o = RenderOptions {
        spacing_in_dots: 2.0,
        ..Default::default()
    };
    let mut times = Vec::new();
    for i in 0..34 {
        bodies[0].height = 0.2 + i as f32 * 0.0001;
        let start = std::time::Instant::now();
        r.render(&mut image, &bodies, &o);
        if i >= 2 {
            times.push(start.elapsed().as_micros());
        }
        std::hint::black_box(&image.dots);
    }
    times.sort_unstable();
    eprintln!(
        "4096 unique capsules, 480x320 dots, rebuild median={}us p95={}us max={}us stats={:?}",
        times[16],
        times[30],
        times[31],
        r.stats()
    );
    let start = std::time::Instant::now();
    r.render(&mut image, &bodies, &o);
    eprintln!(
        "cache-hit={}us stats={:?}",
        start.elapsed().as_micros(),
        r.stats()
    );
}
