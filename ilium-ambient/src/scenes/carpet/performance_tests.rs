use super::*; // Exercise the production renderer against the actual crate Raster.
fn sphere_population(count: usize, radius: f32) -> Vec<Body> {
    // Synthetic bounded population, not a claim about authored mode state.
    (0..count)
        .map(|index| {
            let point = [
                0.02 + (index % 32) as f32 * 0.03,
                0.02 + (index / 32) as f32 * 0.03,
            ];
            Body {
                from: point,
                to: point,
                radius,
                height: 0.075,
            }
        })
        .collect() // Preserve every requested sphere.
} // End the synthetic sphere fixture.
fn dense_population() -> Vec<Body> {
    // Reproduce the addendum's distinct crossing-capsule population.
    (0..MAX_BODIES)
        .map(|index| {
            let t = index as f32 / (MAX_BODIES - 1) as f32;
            Body {
                from: [0.0, 0.1 + 0.8 * t],
                to: [1.0, 0.9 - 0.8 * t],
                radius: 0.15,
                height: 0.08 + (index % 17) as f32 * 0.002,
            }
        })
        .collect() // These are not deduplicated geometries.
} // End the dense fixture.
fn image(width: usize, height: usize) -> Raster {
    let mut raster = Raster::default();
    raster.resize(width, height);
    raster
} // Allocate only test-owned raster storage.
fn exact_height(bodies: &[Body], point: [f32; 2], softness: f32) -> f32 {
    bodies
        .iter()
        .map(|body| body.height_at(point, softness))
        .fold(0.0, f32::max)
} // Independent unpruned model oracle.
fn assert_exact_field(renderer: &Renderer, bodies: &[Body], softness: f32) {
    // Check every lattice vertex, including outside all supports.
    for y in 0..SIDE {
        // Include both ground boundaries.
        for x in 0..SIDE {
            // Do not sample only centers or known winners.
            let expected = exact_height(
                bodies,
                [x as f32 / GRID as f32, y as f32 / GRID as f32],
                softness,
            ); // Rebuild the scalar maximum from all original records.
            assert_eq!(
                renderer.heights[y * SIDE + x].to_bits(),
                expected.to_bits(),
                "vertex {x},{y}"
            ); // Require the exact f32 lattice result, not a visual tolerance.
        } // Finish the oracle row.
    } // Finish all vertices.
} // End the independent field gate.
#[test] // A cache hit must restore actual pixels rather than assuming caller buffer identity.
fn unchanged_frames_restore_pixels_without_repeating_strokes() {
    // Cover raw and canonical input reuse.
    let mut bodies = sphere_population(6, 0.12); // Representative bounded moving-mode population.
    let mut renderer = Renderer::default();
    let mut raster = image(480, 320);
    let options = RenderOptions {
        height_scale: 1.0,
        ..Default::default()
    }; // Use the primary's rendering scale.
    renderer.render(&mut raster, &bodies, &options);
    let expected = raster.dots.clone(); // Save a fully rendered first frame.
    let pointers = (
        renderer.heights.as_ptr(),
        renderer.cached_dots.as_ptr(),
        renderer.raw_input.as_ptr(),
    ); // Track persistent allocations across exact reuse.
    raster.dots.fill(f32::NAN);
    renderer.render(&mut raster, &bodies, &options); // The caller may clear or overwrite its buffer independently.
    assert_eq!(raster.dots, expected);
    assert!(renderer.stats().frame_cache_hit); // Completed pixels must be restored exactly.
    assert_eq!(
        (
            renderer.stats().body_evaluations,
            renderer.stats().node_tests,
            renderer.stats().samples,
            renderer.stats().segments,
            renderer.stats().raster_visits_bound
        ),
        (0, 0, 0, 0, 0)
    ); // A copy must not hide repeated geometry work.
    assert_eq!(renderer.stats().copied_dots, 480 * 320); // Account for the real copy operation.
    assert_eq!(
        pointers,
        (
            renderer.heights.as_ptr(),
            renderer.cached_dots.as_ptr(),
            renderer.raw_input.as_ptr()
        )
    ); // Warm exact-input reuse must not replace these buffers.
    bodies.reverse();
    renderer.render(&mut raster, &bodies, &options); // Input ordering is not geometric identity.
    assert!(renderer.stats().frame_cache_hit);
    assert_eq!(raster.dots, expected); // Canonical equivalence also reuses completed ink.
} // End the exact cache gate.
#[test] // The reported benchmark's single increase must not trigger a dense rebuild.
fn dense_height_increase_only_evaluates_the_changed_support() {
    // Preserve all 4096 unique contributors.
    let mut bodies = dense_population();
    bodies[0].height = 0.2; // Match the original benchmark's warm state.
    let options = RenderOptions {
        spacing_in_dots: 2.0,
        ..Default::default()
    };
    let mut renderer = Renderer::default();
    let mut raster = image(480, 320); // Match its raster and density.
    renderer.render(&mut raster, &bodies, &options);
    let before = renderer.heights.clone(); // Retain the established maximum.
    bodies[0].height += 0.0001;
    renderer.render(&mut raster, &bodies, &options); // Change only the original benchmark operand.
    let stats = renderer.stats();
    assert_eq!(stats.unique_bodies, 4096);
    assert!(stats.monotone_update);
    assert_eq!(stats.node_tests, 0); // The complete population remains represented without a hierarchy rebuild.
    assert!(stats.body_evaluations <= SIDE * SIDE); // At most one full support rectangle may be sampled.
    for y in 0..SIDE {
        for x in 0..SIDE {
            let index = y * SIDE + x;
            let added = bodies[0].height_at(
                [x as f32 / GRID as f32, y as f32 / GRID as f32],
                options.softness,
            );
            assert_eq!(
                renderer.heights[index].to_bits(),
                before[index].max(added).to_bits()
            );
        }
    } // Verify the exact monotone maximum update at every vertex.
} // End the measured-failure regression.
#[test] // General repair must expose hidden contributors and remove old footprints.
fn removals_decreases_motion_additions_and_softness_match_the_scalar_oracle() {
    // Exercise both old and new compact support regions.
    let mut bodies = sphere_population(8, 0.035);
    bodies.push(Body {
        from: [0.5, 0.5],
        to: [0.5, 0.5],
        radius: 0.12,
        height: 0.09,
    }); // Establish a lower central bump.
    bodies.push(Body {
        from: [0.4, 0.5],
        to: [0.6, 0.5],
        radius: 0.12,
        height: 0.1875,
    }); // Place a higher capsule over it.
    let mut renderer = Renderer::default();
    let mut raster = image(160, 96);
    let mut options = RenderOptions {
        height_scale: 1.0,
        ..Default::default()
    }; // Use realistic supplied height and scale ranges.
    for stage in 0..10 {
        // Retain the same renderer across all invalidating transitions.
        match stage {
            // Apply one independently meaningful transition per stage.
            1 => bodies[9].height = 0.04, // A decrease must reveal the lower central body.
            2 => {
                bodies[9].from = [0.8, 0.8];
                bodies[9].to = [0.9, 0.8];
            } // Movement must erase the old capsule footprint.
            3 => {
                bodies.remove(8);
            } // Removal must not leave the lower bump cached.
            4 => bodies.push(Body {
                from: [0.0, 0.0],
                to: [0.1, 0.0],
                radius: 0.24,
                height: 0.075,
            }), // Add a boundary capsule at the primary's maximum radius.
            5 => bodies[0].radius = 0.192, // A larger radius changes both support admission and winning heights.
            6 => bodies[0].radius = 0.005, // Shrinking support must erase the formerly wider bump.
            7 => options.softness = 1.0,   // Shape changes invalidate all affected envelopes.
            8 => bodies.clear(), // The final empty population must return exactly to ground.
            9 => bodies.push(Body {
                from: [f32::NAN, 0.5],
                to: [0.5, 0.5],
                radius: 0.1,
                height: 0.1,
            }), // Repeated invalid records must not defeat exact raw-request reuse.
            _ => {}              // Stage zero establishes the initial field.
        } // Finish the requested transition.
        renderer.render(&mut raster, &bodies, &options);
        assert_exact_field(&renderer, &bodies, options.softness); // Compare every repaired and untouched lattice vertex against all supplied bodies.
        if stage == 9 {
            renderer.render(&mut raster, &bodies, &options);
            assert!(renderer.stats().frame_cache_hit);
            assert_eq!(renderer.stats().invalid_bodies, 1);
        } // Stable NaN bits retain accurate invalid counts and cached pixels.
    } // Finish all maintenance transitions.
} // End the winner-recovery gate.
#[test] // Conservative hierarchy rejection must not lose late or crowded contributors.
fn hierarchy_matches_unpruned_dense_and_mixed_capsule_queries() {
    // Probe geometries beyond the favorable single-increase benchmark.
    let mut mixed = Vec::new(); // Assemble a deterministic adversarial population.
    let mut state = 7_u32; // Fix the source so failures are recoverable.
    for index in 0..4096 {
        // Keep the full admitted population.
        let mut coordinate = || {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 8) as f32 / 16777215.0
        }; // Generate finite ground endpoints without a new dependency.
        let from = [coordinate(), coordinate()];
        let to = if index % 3 == 0 {
            from
        } else {
            [coordinate(), coordinate()]
        }; // Mix spheres and genuinely distinct capsules.
        mixed.push(Body {
            from,
            to,
            radius: [0.005, 0.024, 0.15, 0.192, 0.24][index % 5],
            height: 0.025 + (index % 23) as f32 * 0.007,
        }); // Include the preserved DVD and maximum radius ranges.
    } // Finish the mixed population.
    mixed[4095] = Body {
        from: [0.5, 0.5],
        to: [0.5, 0.5],
        radius: 0.005,
        height: 1.0,
    }; // A late tiny, tall contributor may never be shortlisted away.
    for bodies in [dense_population(), mixed] {
        // Test both dense crossing geometry and mixed supports.
        let prepared: Vec<_> = bodies
            .iter()
            .filter_map(|body| Prepared::new(*body))
            .collect();
        let mut tree = EnvelopeTreeIndex::default();
        assert!(tree.build(&prepared, None)); // Build the actual production hierarchy with every record.
        let mut hint = usize::MAX;
        let mut evaluations = 0;
        let mut node_tests = 0; // Track actual query work independently of drawing.
        for index in 0..257 {
            // Include the center explicitly and a deterministic sweep of other locations.
            let point = if index == 256 {
                [0.5, 0.5]
            } else {
                [
                    (index * 37 % 257) as f32 / 256.0,
                    (index * 61 % 257) as f32 / 256.0,
                ]
            }; // Query ground edges and interior, not just support centers.
            let actual = tree.sample(
                &prepared,
                point,
                0.5,
                &mut hint,
                &mut evaluations,
                &mut node_tests,
            ); // Use the same conservative traversal as general repairs.
            assert_eq!(
                actual.to_bits(),
                exact_height(&bodies, point, 0.5).to_bits(),
                "query {index}"
            ); // Demand the exact scalar maximum rather than a tolerance.
        } // Finish all queries.
        let capacity = tree.storage();
        assert!(tree.build(&prepared, None));
        assert_eq!(tree.storage(), capacity); // Warm hierarchy rebuilding retains node and permutation capacity.
        assert!(evaluations <= 257 * 4096);
        assert!(node_tests <= 257 * 1023); // Each contributor and preorder node is visited at most once per query.
    } // Finish both complete-population fixtures.
} // End the hierarchy correctness gate.
#[test] // Bounding-axis normalization must remain safe for subnormal segment lengths.
fn tiny_segments_and_exact_sphere_specialization_preserve_samples() {
    // Guard the zero-length optimization and the hierarchy's f64 axis normalization.
    let bodies = [
        Body {
            from: [0.0, 0.0],
            to: [f32::from_bits(1), f32::from_bits(1)],
            radius: 0.024,
            height: 0.075,
        },
        Body {
            from: [0.5, 0.5],
            to: [0.5, 0.5],
            radius: 0.192,
            height: 0.075,
        },
    ]; // Include both a nonzero subnormal segment and a true sphere.
    let prepared: Vec<_> = bodies
        .iter()
        .filter_map(|body| Prepared::new(*body))
        .collect();
    let mut tree = EnvelopeTreeIndex::default();
    assert!(tree.build(&prepared, None)); // Use the unchanged model normalization.
    for softness in [0.0, 0.5, 1.0] {
        // Cover both compact polynomial endpoints and their blend.
        for y in 0..33 {
            for x in 0..33 {
                let point = [x as f32 / 32.0, y as f32 / 32.0];
                let mut hint = usize::MAX;
                let mut evaluations = 0;
                let mut tests = 0;
                assert_eq!(
                    tree.sample(
                        &prepared,
                        point,
                        softness,
                        &mut hint,
                        &mut evaluations,
                        &mut tests
                    )
                    .to_bits(),
                    exact_height(&bodies, point, softness).to_bits()
                );
                assert_eq!(
                    sample_body(prepared[1], point, softness).to_bits(),
                    prepared[1].sample(point, softness).to_bits()
                );
            }
        } // Compare complete query results and specialized sphere samples independently.
    } // Finish softness controls.
    assert_eq!(bodies[1].normalized().unwrap().radius, 0.192);
    assert_eq!(
        RenderOptions {
            height_scale: 2.5,
            ..Default::default()
        }
        .normalized()
        .height_scale,
        2.5
    ); // Preserve both primary corrections explicitly.
} // End the numeric edge gate.
#[test] // All-moving real-sized sphere populations need bounded compact-support work.
fn moving_1024_spheres_use_support_work_without_hierarchy_queries() {
    // This is a synthetic production-sized fixture, not a mode simulation.
    let mut bodies = sphere_population(1024, 0.012);
    let mut renderer = Renderer::default();
    let mut raster = image(160, 96);
    let options = RenderOptions {
        height_scale: 1.0,
        ..Default::default()
    }; // Keep body count and density unchanged.
    renderer.render(&mut raster, &bodies, &options); // Establish the previous supports.
    for body in &mut bodies {
        body.from[1] += 0.001;
        body.to = body.from;
    } // Move every requested sphere, not just one test operand.
    renderer.render(&mut raster, &bodies, &options);
    let stats = renderer.stats(); // Exercise actual changing-geometry maintenance.
    assert_eq!(stats.unique_bodies, 1024);
    assert!(!stats.monotone_update);
    assert_eq!(stats.node_tests, 0); // Compact-support reconstruction is selected without dropping contributors.
    let bound: usize = renderer
        .bodies
        .iter()
        .map(|body| {
            let (low, high) = lattice_bounds(*body);
            (high[0] - low[0] + 1) * (high[1] - low[1] + 1)
        })
        .sum(); // Independently reconstruct the support-area work ceiling.
    assert!(stats.body_evaluations <= bound && bound <= 1_000_000); // Check actual analytic evaluations, not a scheduling-dependent time threshold.
    for index in 0..128 {
        let point = [
            (index * 17 % 257) as f32 / 256.0,
            (index * 29 % 257) as f32 / 256.0,
        ];
        assert_eq!(
            renderer.heights
                [(point[1] * GRID as f32) as usize * SIDE + (point[0] * GRID as f32) as usize]
                .to_bits(),
            exact_height(&bodies, point, options.softness).to_bits()
        );
    } // Selected exact lattice vertices still include all 1024 bodies.
} // End the compact-support performance regression.
#[test] // Raster reuse must include every presentation control and both dimensions.
fn view_changes_and_softness_have_distinct_cache_ownership() {
    // Geometry state is independent of presentation-only controls.
    let bodies = sphere_population(6, 0.12);
    let mut renderer = Renderer::default();
    let mut raster = image(160, 96);
    let base = RenderOptions::default(); // Keep the source population fixed.
    for control in 0..10 {
        // Exercise one invalidator at a time from a known completed base frame.
        raster.resize(160, 96);
        renderer.render(&mut raster, &bodies, &base);
        let mut options = base; // Reestablish the exact reference key before each change.
        match control {
            0 => options.yaw = 65.0,
            1 => options.pitch = 55.0,
            2 => options.zoom = 1.3,
            3 => options.hatch_direction = 31.0,
            4 => options.spacing_in_dots = 8.0,
            5 => options.line_width = 2.0,
            6 => options.height_scale = 2.5,
            7 => options.softness = 1.0,
            8 => raster.resize(162, 96),
            _ => raster.resize(160, 100),
        } // No cache-affecting public option is omitted.
        renderer.render(&mut raster, &bodies, &options);
        assert!(!renderer.stats().frame_cache_hit, "control {control}"); // A changed key must not copy stale pixels.
        assert_eq!(renderer.stats().rebuilt, control == 7, "control {control}"); // Only softness changes the camera-independent field.
        renderer.render(&mut raster, &bodies, &options);
        assert!(renderer.stats().frame_cache_hit); // Every completed replacement key is reusable.
    } // Finish all cache invalidators.
} // End the presentation ownership gate.
#[test] // Interrupted field or ink state must never satisfy a later cache hit.
fn cancelled_render_forces_coherent_recovery() {
    // Exercise the same cancellation entry used by the owned worker.
    let bodies = sphere_population(6, 0.12);
    let options = RenderOptions::default();
    let mut renderer = Renderer::default();
    let mut raster = image(160, 96); // Establish a valid prior result.
    renderer.render(&mut raster, &bodies, &options);
    let expected = raster.dots.clone(); // Preserve the known complete frame.
    renderer.heights.fill(0.9);
    raster.dots.fill(0.3); // Inject partial scratch corruption so recovery must replace every invalid vertex, not merely invalidate a key.
    let stop = std::sync::atomic::AtomicBool::new(true);
    assert!(!renderer.render_checked(&mut raster, &bodies, &options, Some(&stop))); // An already cancelled request cannot publish cached or newly computed ink.
    assert!(renderer.cached_key.is_none() && renderer.softness.is_none()); // Both field and image validity must be revoked.
    stop.store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(renderer.render_checked(&mut raster, &bodies, &options, Some(&stop))); // A subsequent live request must recover fully.
    assert_eq!(raster.dots, expected);
    assert!(!renderer.stats().frame_cache_hit); // Recovery is a coherent render, not reuse of interrupted state.
} // End the cancellation recovery gate.
#[test] // Keep timing assertions separate from functional correctness tests.
#[ignore = "explicit release measurement; wall time and work counters require controlled host-load interpretation"] // The test completing is not a responsiveness acceptance receipt.
fn correction_measurement() {
    // Distinguish cold, one-body, all-moving, camera-only, and completed-frame reuse.
    if cfg!(debug_assertions) {
        panic!("run this measurement in release mode");
    } // Debug execution is not useful responsiveness evidence.
    let frames = std::env::var("CARPET_BENCH_FRAMES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(32)
        .clamp(8, 256); // Permit longer explicit runs without unbounded test work.
    let mut capsules = sphere_population(160, 0.02);
    for body in &mut capsules {
        body.to = [
            (body.from[0] + 0.04).min(1.0),
            (body.from[1] + 0.03).min(1.0),
        ];
    } // Synthetic short chess-like capsules, not authored chess state.
    let fixtures = [
        ("hunters_synthetic_6", sphere_population(6, 0.12)),
        ("spheres_synthetic_1024", sphere_population(1024, 0.012)),
        (
            "large_spheres_synthetic_1024",
            sphere_population(1024, 0.12),
        ),
        ("short_capsules_synthetic_160", capsules),
        ("dense_crossing_4096", dense_population()),
    ]; // Include favorable production-sized and adversarial dense populations.
    for (name, original) in fixtures {
        // Keep populations distinct in the emitted measurements.
        for path in [
            "cold",
            "height_up",
            "height_down",
            "one_move",
            "all_move",
            "camera",
            "hit",
        ] {
            // Do not label a single increasing height as general changing geometry.
            let mut bodies = original.clone();
            bodies[0].height = 0.2;
            let baseline = bodies.clone(); // Match the recovered benchmark's initial high body while preserving all other contributors.
            let mut renderer = Renderer::default();
            let mut raster = image(480, 320);
            let mut options = RenderOptions {
                spacing_in_dots: 2.0,
                height_scale: 1.0,
                ..Default::default()
            }; // Measure dense hatching at the primary's scale.
            renderer.render(&mut raster, &bodies, &options);
            let mut times = Vec::with_capacity(frames); // Warm the non-cold paths before collecting samples.
            for frame in 0..frames {
                // Bound each measurement batch explicitly.
                match path {
                    // Change exactly the source or presentation dimension named by this path.
                    "height_up" => bodies[0].height = 0.2 + (frame + 1) as f32 * 0.0001, // Monotone direct-maximum updates.
                    "height_down" => bodies[0].height = 0.2 - (frame + 1) as f32 * 0.0001, // A previous winner may expose other bodies.
                    "one_move" | "all_move" => {
                        let count = if path == "all_move" { bodies.len() } else { 1 };
                        let offset = ((frame % 7) + 1) as f32 * 0.0002;
                        for index in 0..count {
                            bodies[index].from[1] = (baseline[index].from[1] + offset).min(1.0);
                            bodies[index].to[1] = (baseline[index].to[1] + offset).min(1.0);
                        }
                    } // Move complete geometry while retaining every record.
                    "camera" => options.yaw = 45.0 + (frame + 1) as f32 * 0.25, // Height-cache reuse is not a completed-raster hit.
                    _ => {} // Cold and exact-hit paths keep the source unchanged.
                } // Finish this sample's setup outside its render timer.
                let start = std::time::Instant::now();
                if path == "cold" {
                    renderer = Renderer::default();
                }
                renderer.render(&mut raster, &bodies, &options);
                times.push(start.elapsed().as_micros());
                std::hint::black_box(&raster.dots); // Include renderer allocation and destruction only in the explicitly cold path.
            } // Finish the timed calls.
            times.sort_unstable();
            let p95 = (frames * 95).div_ceil(100) - 1; // Wall percentiles include scheduling delay; counters describe the final rendered path.
            eprintln!(
                "C1 {name} {path} frames={frames} wall_median_us={} wall_p95_us={} wall_max_us={} stats={:?}",
                times[frames / 2],
                times[p95],
                times[frames - 1],
                renderer.stats()
            ); // Emit measurements without claiming that an unconfigured time gate passed.
        } // Finish all work paths for this population.
    } // Finish all synthetic populations.
} // End the corrected release measurement.
