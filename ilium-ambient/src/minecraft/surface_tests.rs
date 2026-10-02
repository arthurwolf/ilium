//! Synthetic-only controls. Sibling module registration is deferred to B4.
use super::{
    chunk::{self, BlockSample, DecodedChunk},
    nbt::{Compound, Document, Tag, Text},
    surface::*,
};
use std::cell::Cell;

fn fields(entries: Vec<(&str, Tag)>) -> Compound {
    entries
        .into_iter()
        .map(|(key, value)| (Text::from(key), value))
        .collect()
}
fn string(value: &str) -> Tag {
    Tag::String(Text::from(value))
}
fn state(name: &str, properties: &[(&str, &str)]) -> Tag {
    Tag::Compound(fields(vec![
        ("Name", string(name)),
        (
            "Properties",
            Tag::Compound(fields(
                properties
                    .iter()
                    .map(|&(key, value)| (key, string(value)))
                    .collect(),
            )),
        ),
    ]))
}
// Build the actual public decoder's NBT input; never invent a Paletted constructor.
fn decoded(version: i32, position: [i32; 2], at: impl Fn([i32; 3]) -> usize) -> DecodedChunk {
    let palette = vec![
        state("minecraft:air", &[]),
        state("minecraft:stone", &[]),
        state("minecraft:cave_air", &[]),
        state("minecraft:water", &[("level", "7")]),
        state(
            "minecraft:oak_slab",
            &[("waterlogged", "true"), ("type", "bottom")],
        ),
        state("unknown_mod:machine", &[("zeta", "雪😀"), ("axis", "z")]),
        state(
            "minecraft:oak_leaves",
            &[("persistent", "true"), ("distance", "2")],
        ),
        state("minecraft:glass", &[]),
        state("minecraft:void_air", &[]),
        state("minecraft:air", &[("custom", "true")]),
        state("minecraft:lava", &[("level", "0")]),
    ];
    let mut sections = Vec::new();
    for section_y in -4_i8..=19 {
        let mut words = vec![0_u64; 256];
        for cell in 0..4096 {
            let block = [
                position[0] * 16 + (cell % 16) as i32,
                i32::from(section_y) * 16 + (cell / 256) as i32,
                position[1] * 16 + ((cell / 16) % 16) as i32,
            ];
            let index = at(block);
            assert!(index < palette.len());
            words[cell / 16] |= (index as u64) << ((cell % 16) * 4);
        }
        sections.push(Tag::Compound(fields(vec![
            ("Y", Tag::Byte(section_y)),
            (
                "block_states",
                Tag::Compound(fields(vec![
                    (
                        "palette",
                        Tag::List {
                            kind: 10,
                            values: palette.clone(),
                        },
                    ),
                    (
                        "data",
                        Tag::LongArray(words.into_iter().map(|word| word as i64).collect()),
                    ),
                ])),
            ),
        ])));
    }
    let wrapped = version <= 2836;
    let body = fields(vec![
        ("xPos", Tag::Int(position[0])),
        ("zPos", Tag::Int(position[1])),
        ("Status", string("full")),
        (
            if wrapped { "Sections" } else { "sections" },
            Tag::List {
                kind: 10,
                values: sections,
            },
        ),
    ]);
    let mut root = if wrapped {
        fields(vec![("Level", Tag::Compound(body))])
    } else {
        body
    };
    root.insert(Text::from("DataVersion"), Tag::Int(version));
    chunk::decode(
        &Document {
            name: Text::from("synthetic"),
            root,
        },
        position,
        chunk::Limits::default(),
        &|| false,
    )
    .unwrap()
}
fn point(x: i32, z: i32) -> Bounds {
    Bounds {
        minimum: [x, z],
        maximum: [x, z],
    }
}

#[test]
fn selected_render_band_preserves_exact_covered_cells_and_does_not_infer_air_above() {
    let chunks = [decoded(3218, [0, 0], |position| match position[1] {
        63 => 1,
        64 => 7,
        65 => 6,
        200 => 1,
        _ => 0,
    })];
    let view = window(&chunks, point(0, 0), 0);
    let mut work = Work::new(10_000, &|| false);
    let mut cells = Vec::new();
    let scan = view
        .visit_band([62, 65], &mut work, |cell| {
            cells.push(cell);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        scan,
        Scan {
            columns: 1,
            samples: 4,
            non_air: 3
        }
    );
    assert_eq!(
        cells
            .iter()
            .map(|cell| cell.position[1])
            .collect::<Vec<_>>(),
        [65, 64, 63]
    );
    assert!(cells.iter().all(|cell| !cell.only_air_above));
    for cell in cells {
        let BlockSample::State(original) = chunks[0].block_at(cell.position) else {
            panic!()
        };
        assert!(std::ptr::eq(cell.state, original));
    }
    assert_eq!(
        view.top_at([0, 0], &mut work).unwrap().unwrap().position[1],
        200
    );
}

#[test]
fn selected_render_band_refuses_bad_domain_and_cancels_transactionally() {
    let chunks = [decoded(3218, [0, 0], |_| 1)];
    let view = window(&chunks, point(0, 0), 0);
    for heights in [[MIN_Y - 1, 0], [0, MAX_Y + 1], [65, 64]] {
        let mut work = Work::new(10_000, &|| false);
        assert!(matches!(
            view.visit_band(heights, &mut work, |_| panic!("invalid band callback")),
            Err(Error::InvalidBounds)
        ));
    }
    let cancelled = Cell::new(false);
    let cancel = || cancelled.get();
    let mut work = Work::new(10_000, &cancel);
    let mut partial = Vec::new();
    let result = view.visit_band([64, 65], &mut work, |cell| {
        partial.push(cell);
        cancelled.set(true);
        Ok(())
    });
    assert_eq!(result, Err(Error::Cancelled));
    assert_eq!(partial.len(), 1, "caller must discard this partial result");
}

#[test]
fn surface_render_band_spans_low_ground_and_high_structures_without_per_column_clipping() {
    let chunks = [decoded(3218, [0, 0], |[x, y, _]| {
        usize::from((x == 0 && y == 64) || (x == 1 && (80..=120).contains(&y)))
    })];
    let view = window(
        &chunks,
        Bounds {
            minimum: [0, 0],
            maximum: [1, 0],
        },
        0,
    );
    let mut work = Work::new(10_000, &|| false);
    let heights = view.surface_band(24, &mut work).unwrap().unwrap();
    assert_eq!(heights, [40, 120]);
    let mut cells = Vec::new();
    view.visit_band(heights, &mut work, |cell| {
        cells.push(cell.position);
        Ok(())
    })
    .unwrap();
    assert!(
        cells.contains(&[1, 80, 0]),
        "tall wall below its own highest cell survives"
    );
    assert!(cells.contains(&[0, 64, 0]));
}

#[test]
fn surface_render_band_clamps_domain_and_distinguishes_saved_empty_columns() {
    let chunks = [decoded(3218, [0, 0], |[_, y, _]| usize::from(y == MIN_Y))];
    let view = window(&chunks, point(0, 0), 0);
    let mut work = Work::new(10_000, &|| false);
    assert_eq!(
        view.surface_band(24, &mut work).unwrap(),
        Some([MIN_Y, MIN_Y])
    );
    assert!(matches!(
        view.surface_band(65, &mut work),
        Err(Error::InvalidBounds)
    ));
    let empty = [decoded(3218, [0, 0], |_| 0)];
    assert_eq!(
        window(&empty, point(0, 0), 0)
            .surface_band(24, &mut work)
            .unwrap(),
        None
    );
}
fn window(chunks: &[DecodedChunk], core: Bounds, halo: u16) -> SurfaceWindow<'_> {
    SurfaceWindow::overworld(
        core,
        halo,
        chunks,
        Limits::default(),
        &mut Work::new(100_000, &|| false),
    )
    .unwrap()
}

#[test]
fn all_observed_layouts_preserve_exact_states_caves_fluids_and_cell_heights() {
    for version in [2834, 2835, 2836, 3218] {
        let chunks = [decoded(version, [-1, -2], |[_, y, _]| match y {
            -64 => 1,
            -20 => 2,
            -19 => 8,
            61 => 3,
            62 => 4,
            63 => 5,
            64 => 6,
            65 => 7,
            66 => 9,
            67 => 10,
            _ => 0,
        })];
        let view = window(&chunks, point(-1, -17), 0);
        let mut work = Work::new(10_000, &|| false);
        let top = view.top_at([-1, -17], &mut work).unwrap().unwrap();
        assert_eq!(top.position, [-1, 67, -17]);
        assert_eq!(top.state.name, "minecraft:lava");
        let mut observations = Vec::new();
        let scan = view
            .visit_non_air(&mut work, |block| {
                observations.push(block);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            scan,
            Scan {
                columns: 1,
                samples: 384,
                non_air: 8
            }
        );
        assert_eq!(
            observations
                .iter()
                .filter(|block| block.only_air_above)
                .count(),
            1
        );
        assert_eq!(observations[0], top);
        let expected: Vec<_> = (MIN_Y..=MAX_Y)
            .rev()
            .filter_map(|y| {
                let position = [-1, y, -17];
                match chunks[0].block_at(position) {
                    BlockSample::State(state) if !state.is_air() => Some((position, state)),
                    _ => None,
                }
            })
            .collect();
        assert_eq!(
            observations
                .iter()
                .map(|block| (block.position, block.state))
                .collect::<Vec<_>>(),
            expected
        );
        for block in &observations {
            let BlockSample::State(original) = chunks[0].block_at(block.position) else {
                panic!()
            };
            assert!(std::ptr::eq(block.state, original));
        }
        for (y, name) in [(-20, "minecraft:cave_air"), (-19, "minecraft:void_air")] {
            assert!(
                matches!(view.sample([-1, y, -17]), Sample::State(state) if state.name == name)
            );
        }
        let Sample::State(slab) = view.sample([-1, 62, -17]) else {
            panic!()
        };
        assert_eq!(slab.properties["waterlogged"], "true");
        assert_eq!(slab.properties["type"], "bottom");
        let Sample::State(machine) = view.sample([-1, 63, -17]) else {
            panic!()
        };
        assert_eq!(
            machine
                .properties
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["axis", "zeta"]
        );
        assert_eq!(machine.properties["zeta"], "雪😀");
        let Sample::State(water) = view.sample([-1, 61, -17]) else {
            panic!()
        };
        assert_eq!(water.properties["level"], "7");
    }
}

#[test]
fn missing_full_and_proto_states_are_distinct_and_excluded_from_coverage() {
    let original = decoded(3218, [0, 0], |_| 0);
    let mut chunks = vec![original.clone(); 5];
    for (index, chunk) in chunks.iter_mut().enumerate() {
        chunk.identity.position = [index as i32, 0];
    }
    chunks[1].status = Some("minecraft:features".into());
    chunks[2].sections.remove(&0);
    chunks[3].sections.get_mut(&0).unwrap().block_states = None;
    chunks[4].sections_present = false;
    let view = window(
        &chunks,
        Bounds {
            minimum: [0, 0],
            maximum: [80, 0],
        },
        0,
    );
    assert!(matches!(view.sample([16, 0, 0]), Sample::State(state) if state.is_air()));
    for (x, reason) in [
        (32, Missing::Section),
        (48, Missing::BlockStates),
        (64, Missing::SectionList),
        (80, Missing::Chunk),
    ] {
        assert_eq!(view.sample([x, 0, 0]), Sample::Missing(reason));
    }
    let mut work = Work::new(100_000, &|| false);
    assert_eq!(
        view.qualified_coverage(&mut work).unwrap().chunks,
        [[0, 0]].into()
    );
    let mut calls = 0;
    assert_eq!(
        view.visit_non_air(&mut work, |_| {
            calls += 1;
            Ok(())
        }),
        Err(Error::Unqualified {
            position: [1, 0],
            reason: Rejection::NotFull
        })
    );
    assert_eq!(calls, 0);
    for (x, reason) in [
        (32, Rejection::MissingSection(0)),
        (48, Rejection::MissingBlockStates(0)),
        (64, Rejection::MissingSectionList),
        (80, Rejection::MissingChunk),
    ] {
        assert_eq!(
            view.top_at([x, 0], &mut work),
            Err(Error::Unqualified {
                position: [x / 16, 0],
                reason
            })
        );
    }
    for status in [
        None,
        Some("noise"),
        Some("custom:full"),
        Some("full"),
        Some("minecraft:full"),
    ] {
        let mut chunk = original.clone();
        chunk.status = status.map(str::to_owned);
        let chunks = [chunk];
        let complete = window(&chunks, point(0, 0), 0)
            .require_complete(&mut work)
            .is_ok();
        assert_eq!(complete, matches!(status, Some("full" | "minecraft:full")));
    }
}

#[test]
fn negative_halo_and_missing_neighbor_are_checked_before_callbacks() {
    let chunks = [decoded(2835, [-1, -1], |_| 1)];
    let view = window(&chunks, point(-1, -1), 1);
    assert_eq!(
        view.bounds(),
        Bounds {
            minimum: [-2, -2],
            maximum: [0, 0]
        }
    );
    let mut work = Work::new(10_000, &|| false);
    let coverage = view.qualified_coverage(&mut work).unwrap();
    assert!(coverage.contains_view([-8.0, -8.0], 7.9));
    assert!(!coverage.contains_view([-8.0, -8.0], 8.0));
    assert!(view.top_at([-1, -1], &mut work).unwrap().is_some());
    assert!(matches!(
        view.visit_non_air(&mut work, |_| panic!("halo was not qualified")),
        Err(Error::Unqualified {
            reason: Rejection::MissingChunk,
            ..
        })
    ));
}

#[test]
fn sky_void_and_explicit_empty_columns_are_not_missing_terrain() {
    let chunks = [decoded(3218, [0, 0], |_| 0)];
    let view = window(&chunks, point(0, 0), 0);
    let mut work = Work::new(10_000, &|| false);
    assert_eq!(view.sample([0, 320, 0]), Sample::AboveOverworld);
    assert_eq!(view.sample([0, -65, 0]), Sample::BelowOverworld);
    assert_eq!(view.sample([1, 320, 0]), Sample::OutsideWindow);
    assert_eq!(view.top_at([0, 0], &mut work).unwrap(), None);
    assert_eq!(
        view.visit_non_air(&mut work, |_| panic!()).unwrap().non_air,
        0
    );
    let empty = window(&[], point(0, 0), 0);
    assert_eq!(empty.sample([0, 320, 0]), Sample::AboveOverworld);
    assert_eq!(empty.sample([0, 0, 0]), Sample::Missing(Missing::Chunk));
    assert!(empty.require_complete(&mut work).is_err());
}

#[test]
fn no_culling_and_stable_order_across_negative_chunk_seam() {
    let chunks = [decoded(3218, [0, 0], |_| 7), decoded(2834, [-1, 0], |_| 6)];
    let view = window(
        &chunks,
        Bounds {
            minimum: [-1, 0],
            maximum: [0, 0],
        },
        0,
    );
    let mut seen = Vec::new();
    let scan = view
        .visit_non_air(&mut Work::new(10_000, &|| false), |block| {
            seen.push((block.position, block.only_air_above));
            Ok(())
        })
        .unwrap();
    assert_eq!(scan.non_air, 768);
    assert_eq!(seen[0], ([-1, 319, 0], true));
    assert_eq!(seen[383], ([-1, -64, 0], false));
    assert_eq!(seen[384], ([0, 319, 0], true));
    assert_eq!(seen[767], ([0, -64, 0], false));
}

#[test]
fn duplicate_version_and_vertical_domain_conflicts_fail_explicitly() {
    let original = decoded(3218, [0, 0], |_| 0);
    let core = point(0, 0);
    let mut work = Work::new(100_000, &|| false);
    assert!(matches!(
        SurfaceWindow::overworld(
            core,
            0,
            &[original.clone(), original.clone()],
            Limits::default(),
            &mut work
        ),
        Err(Error::DuplicateChunk([0, 0]))
    ));
    for version in [2833, 3219] {
        let mut chunk = original.clone();
        chunk.identity.data_version = version;
        assert!(matches!(
            SurfaceWindow::overworld(core, 0, &[chunk], Limits::default(), &mut work),
            Err(Error::Unqualified {
                reason: Rejection::UnsupportedVersion(_),
                ..
            })
        ));
    }
    for y in [-5, 20] {
        let mut chunk = original.clone();
        chunk.sections.insert(y, original.sections[&0].clone());
        assert!(matches!(
            SurfaceWindow::overworld(core, 0, &[chunk], Limits::default(), &mut work),
            Err(Error::Unqualified {
                reason: Rejection::OutsideDomainSection(_),
                ..
            })
        ));
    }
    let mut chunk = original;
    chunk.sections.insert(
        20,
        chunk::Section {
            block_states: None,
            biomes: None,
        },
    );
    window(&[chunk], core, 0)
        .require_complete(&mut work)
        .unwrap();
}

#[test]
fn bounds_input_memory_and_work_limits_are_testable() {
    let chunks = [decoded(3218, [0, 0], |_| 0)];
    for limits in [
        Limits {
            max_chunks: 0,
            ..Limits::default()
        },
        Limits {
            max_columns: 0,
            ..Limits::default()
        },
        Limits {
            max_owned_bytes: 0,
            ..Limits::default()
        },
    ] {
        let mut work = Work::new(10_000, &|| false);
        assert!(matches!(
            SurfaceWindow::overworld(point(0, 0), 0, &chunks, limits, &mut work),
            Err(Error::Limit(_))
        ));
        assert_eq!(work.used(), 0);
    }
    for core in [
        Bounds {
            minimum: [1, 0],
            maximum: [0, 0],
        },
        Bounds {
            minimum: [i32::MIN, 0],
            maximum: [i32::MAX, 0],
        },
    ] {
        assert!(matches!(
            SurfaceWindow::overworld(
                core,
                0,
                &chunks,
                Limits::default(),
                &mut Work::new(10_000, &|| false)
            ),
            Err(Error::InvalidBounds)
        ));
    }
    assert!(matches!(
        SurfaceWindow::overworld(
            point(0, 0),
            0,
            &chunks,
            Limits::default(),
            &mut Work::new(0, &|| false)
        ),
        Err(Error::Limit("work units"))
    ));
    let view = window(&chunks, point(0, 0), 0);
    let tight = Limits {
        max_owned_bytes: view.owned_bytes(),
        ..Limits::default()
    };
    assert!(
        SurfaceWindow::overworld(
            point(0, 0),
            0,
            &chunks,
            tight,
            &mut Work::new(10_000, &|| false)
        )
        .unwrap()
        .owned_bytes()
            <= tight.max_owned_bytes
    );
    let too_small = Limits {
        max_owned_bytes: view.owned_bytes() - 1,
        ..Limits::default()
    };
    assert!(matches!(
        SurfaceWindow::overworld(
            point(0, 0),
            0,
            &chunks,
            too_small,
            &mut Work::new(10_000, &|| false)
        ),
        Err(Error::Limit(_))
    ));
    for (columns, admitted) in [(8, false), (9, true)] {
        let limits = Limits {
            max_columns: columns,
            ..Limits::default()
        };
        assert_eq!(
            SurfaceWindow::overworld(
                point(8, 8),
                1,
                &chunks,
                limits,
                &mut Work::new(10_000, &|| false)
            )
            .is_ok(),
            admitted
        );
    }
    for edge in [i32::MIN, i32::MAX] {
        let chunks = [decoded(3218, [edge.div_euclid(16), 0], |_| 0)];
        let view = window(&chunks, point(edge, 0), 0);
        assert!(matches!(view.sample([edge, 0, 0]), Sample::State(_)));
        assert!(matches!(
            SurfaceWindow::overworld(
                point(edge, 0),
                1,
                &chunks,
                Limits::default(),
                &mut Work::new(10_000, &|| false)
            ),
            Err(Error::InvalidBounds)
        ));
    }
}

#[test]
fn budget_and_callback_failure_do_not_publish_success_or_refund_work() {
    let chunks = [decoded(3218, [0, 0], |_| 1)];
    let view = window(&chunks, point(0, 0), 0);
    let mut work = Work::new(5, &|| false);
    let mut calls = 0;
    assert_eq!(
        view.visit_non_air(&mut work, |_| {
            calls += 1;
            Ok(())
        }),
        Err(Error::Limit("work units"))
    );
    assert_eq!((work.used(), calls), (5, 4));
    assert!(view.require_complete(&mut work).is_err());
    assert_eq!(work.used(), 5);
    assert_eq!(
        view.visit_non_air(&mut Work::new(10_000, &|| false), |_| Err(Error::Limit(
            "consumer"
        ))),
        Err(Error::Limit("consumer"))
    );
}

#[test]
fn cancellation_before_during_and_after_last_observation_is_explicit() {
    let chunks = [decoded(3218, [0, 0], |[_, y, _]| usize::from(y == MIN_Y))];
    assert!(matches!(
        SurfaceWindow::overworld(
            point(0, 0),
            0,
            &chunks,
            Limits::default(),
            &mut Work::new(10_000, &|| true)
        ),
        Err(Error::Cancelled)
    ));
    let build_checks = Cell::new(0);
    let build_cancel = || {
        build_checks.set(build_checks.get() + 1);
        build_checks.get() >= 3
    };
    assert!(matches!(
        SurfaceWindow::overworld(
            point(0, 0),
            0,
            &chunks,
            Limits::default(),
            &mut Work::new(10_000, &build_cancel)
        ),
        Err(Error::Cancelled)
    ));
    let view = window(&chunks, point(0, 0), 0);
    let checks = Cell::new(0);
    let cancel = || {
        checks.set(checks.get() + 1);
        checks.get() >= 10
    };
    assert_eq!(
        view.top_at([0, 0], &mut Work::new(10_000, &cancel)),
        Err(Error::Cancelled)
    );
    let stopped = Cell::new(false);
    let cancel = || stopped.get();
    let mut emitted = 0;
    assert_eq!(
        view.visit_non_air(&mut Work::new(10_000, &cancel), |_| {
            emitted += 1;
            stopped.set(true);
            Ok(())
        }),
        Err(Error::Cancelled)
    );
    assert_eq!(emitted, 1);
    assert!(matches!(
        view.qualified_coverage(&mut Work::new(10_000, &|| true)),
        Err(Error::Cancelled)
    ));
    assert_eq!(
        view.require_complete(&mut Work::new(10_000, &|| true)),
        Err(Error::Cancelled)
    );
}

// Regression additions for the Arc-backed loader/reference constructor boundary.
#[test]
fn loader_references_keep_state_identity_and_admit_explicit_121_chunk_bounds() {
    use std::sync::Arc;
    let template = decoded(3218, [0, 0], |[_, y, _]| usize::from(y == 64) * 5);
    let mut loaded = super::loader::LoadedWindow::default();
    for z in -5..=5 {
        for x in -5..=5 {
            // Fixture construction only; the production constructor never clones.
            let mut chunk = template.clone();
            chunk.identity.position = [x, z];
            loaded.chunks.insert([x, z], Arc::new(chunk));
            loaded.coverage.chunks.insert([x, z]);
        }
    }
    let core = Bounds {
        minimum: [-79, -79],
        maximum: [94, 94],
    };
    let limits = Limits {
        max_chunks: 128,
        max_columns: 30_976,
        max_owned_bytes: 16_384,
    };
    assert!(matches!(
        SurfaceWindow::overworld_refs(
            core,
            1,
            loaded.chunks.values().map(Arc::as_ref),
            Limits::default(),
            &mut Work::new(1000, &|| false)
        ),
        Err(Error::Limit("input chunks"))
    ));
    let mut work = Work::new(1000, &|| false);
    let view = SurfaceWindow::overworld_refs(
        core,
        1,
        loaded.chunks.values().map(Arc::as_ref),
        limits,
        &mut work,
    )
    .unwrap();
    assert_eq!(work.used(), 121);
    view.require_complete(&mut work).unwrap();
    assert_eq!(
        view.bounds(),
        Bounds {
            minimum: [-80, -80],
            maximum: [95, 95]
        }
    );
    assert_eq!(
        view.qualified_coverage(&mut work).unwrap().chunks,
        loaded.coverage.chunks
    );
    for (position, chunk) in &loaded.chunks {
        assert_eq!(Arc::strong_count(chunk), 1);
        let position = [position[0] * 16, 64, position[1] * 16];
        let Sample::State(borrowed) = view.sample(position) else {
            panic!()
        };
        let BlockSample::State(original) = chunk.block_at(position) else {
            panic!()
        };
        assert!(std::ptr::eq(borrowed, original));
        assert_eq!(borrowed.properties["zeta"], "雪😀");
    }
    for limits in [
        Limits {
            max_chunks: 120,
            ..limits
        },
        Limits {
            max_columns: 30_975,
            ..limits
        },
        Limits {
            max_owned_bytes: 0,
            ..limits
        },
    ] {
        assert!(matches!(
            SurfaceWindow::overworld_refs(
                core,
                1,
                loaded.chunks.values().map(Arc::as_ref),
                limits,
                &mut Work::new(1000, &|| false)
            ),
            Err(Error::Limit(_))
        ));
    }
}

// Deliberately violates ExactSizeIterator; safe code must still stay bounded.
struct WrongLength<'a> {
    item: &'a DecodedChunk,
    actual: usize,
    declared: usize,
    polls: &'a Cell<usize>,
}
impl<'a> Iterator for WrongLength<'a> {
    type Item = &'a DecodedChunk;
    fn next(&mut self) -> Option<Self::Item> {
        self.polls.set(self.polls.get() + 1);
        if self.actual == 0 {
            return None;
        }
        self.actual -= 1;
        Some(self.item)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.declared, Some(self.declared))
    }
}
impl ExactSizeIterator for WrongLength<'_> {}

#[test]
fn reference_iterator_length_claims_cannot_trigger_unbounded_consumption() {
    let chunk = decoded(3218, [0, 0], |_| 0);
    for (declared, actual, maximum_polls) in [(1, 2, 2), (2, 1, 2), (1, usize::MAX, 2), (257, 1, 0)]
    {
        let polls = Cell::new(0);
        let iterator = WrongLength {
            item: &chunk,
            actual,
            declared,
            polls: &polls,
        };
        assert!(matches!(
            SurfaceWindow::overworld_refs(
                point(0, 0),
                0,
                iterator,
                Limits {
                    max_chunks: usize::MAX,
                    ..Limits::default()
                },
                &mut Work::new(1000, &|| false)
            ),
            Err(Error::Limit(_))
        ));
        assert!(polls.get() <= maximum_polls);
    }
}

#[test]
fn reference_path_keeps_duplicate_version_domain_and_memory_checks() {
    let original = decoded(3218, [0, 0], |_| 0);
    assert!(matches!(
        SurfaceWindow::overworld_refs(
            point(0, 0),
            0,
            [&original, &original],
            Limits::default(),
            &mut Work::new(100, &|| false)
        ),
        Err(Error::DuplicateChunk([0, 0]))
    ));
    for version in [2833, 3219] {
        let mut chunk = original.clone();
        chunk.identity.data_version = version;
        assert!(matches!(
            SurfaceWindow::overworld_refs(
                point(0, 0),
                0,
                [&chunk],
                Limits::default(),
                &mut Work::new(100, &|| false)
            ),
            Err(Error::Unqualified {
                reason: Rejection::UnsupportedVersion(_),
                ..
            })
        ));
    }
    let mut chunk = original.clone();
    chunk.sections.insert(20, original.sections[&0].clone());
    assert!(matches!(
        SurfaceWindow::overworld_refs(
            point(0, 0),
            0,
            [&chunk],
            Limits::default(),
            &mut Work::new(100, &|| false)
        ),
        Err(Error::Unqualified {
            reason: Rejection::OutsideDomainSection(20),
            ..
        })
    ));
    chunk = original.clone();
    chunk.identity.position = [i32::MAX, 0];
    assert!(matches!(
        SurfaceWindow::overworld_refs(
            point(0, 0),
            0,
            [&chunk],
            Limits::default(),
            &mut Work::new(100, &|| false)
        ),
        Err(Error::InvalidBounds)
    ));
    let owned = window(std::slice::from_ref(&original), point(0, 0), 0).owned_bytes();
    let exact = Limits {
        max_owned_bytes: owned,
        ..Limits::default()
    };
    assert!(SurfaceWindow::overworld_refs(
        point(0, 0),
        0,
        [&original],
        exact,
        &mut Work::new(100, &|| false)
    )
    .is_ok());
    assert!(matches!(
        SurfaceWindow::overworld_refs(
            point(0, 0),
            0,
            [&original],
            Limits {
                max_owned_bytes: owned - 1,
                ..exact
            },
            &mut Work::new(100, &|| false)
        ),
        Err(Error::Limit(_))
    ));
}

#[test]
fn reference_constructor_cancellation_and_work_exhaustion_do_not_pull_items() {
    let chunk = decoded(3218, [0, 0], |_| 0);
    for cancelled in [false, true] {
        let polls = Cell::new(0);
        let iterator = WrongLength {
            item: &chunk,
            actual: 1,
            declared: 1,
            polls: &polls,
        };
        assert!(SurfaceWindow::overworld_refs(
            point(0, 0),
            0,
            iterator,
            Limits::default(),
            &mut Work::new(0, &|| cancelled)
        )
        .is_err());
        assert_eq!(polls.get(), 0);
    }
    let stopped = Cell::new(false);
    let cancel = || stopped.get();
    let iterator = [&chunk].into_iter().inspect(|_| stopped.set(true));
    assert!(matches!(
        SurfaceWindow::overworld_refs(
            point(0, 0),
            0,
            iterator,
            Limits::default(),
            &mut Work::new(100, &cancel)
        ),
        Err(Error::Cancelled)
    ));
}
