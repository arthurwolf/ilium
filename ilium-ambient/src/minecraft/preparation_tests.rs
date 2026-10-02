//! Synthetic controls go through the real decoder; no filesystem/map reads.
use super::*;
use crate::minecraft::{chunk, coverage::Coverage, nbt};
use std::{collections::BTreeMap, sync::Arc};

fn fields(entries: Vec<(&str, nbt::Tag)>) -> nbt::Compound {
    entries
        .into_iter()
        .map(|(name, value)| (name.into(), value))
        .collect()
}
fn loaded() -> loader::LoadedWindow {
    use nbt::Tag;
    let palette = ["air", "grass_block", "grass"].map(|name| {
        let mut state = fields(vec![(
            "Name",
            Tag::String(format!("minecraft:{name}").as_str().into()),
        )]);
        if name == "grass_block" {
            state.insert(
                "Properties".into(),
                Tag::Compound(fields(vec![("snowy", Tag::String("false".into()))])),
            );
        }
        Tag::Compound(state)
    });
    let sections = (-4_i8..=19)
        .map(|y| {
            let mut words = vec![0_u64; 256];
            for cell in 0..4096 {
                let height = i32::from(y) * 16 + (cell / 256) as i32;
                let index = if height == 0 {
                    1
                } else if height == 1 && cell % 4 == 0 && (cell / 16) % 4 == 0 {
                    2
                } else {
                    0
                };
                words[cell / 16] |= index << ((cell % 16) * 4);
            }
            Tag::Compound(fields(vec![
                ("Y", Tag::Byte(y)),
                (
                    "block_states",
                    Tag::Compound(fields(vec![
                        (
                            "palette",
                            Tag::List {
                                kind: 10,
                                values: palette.to_vec(),
                            },
                        ),
                        (
                            "data",
                            Tag::LongArray(words.into_iter().map(|word| word as i64).collect()),
                        ),
                    ])),
                ),
                (
                    "biomes",
                    Tag::Compound(fields(vec![(
                        "palette",
                        Tag::List {
                            kind: 8,
                            values: vec![Tag::String("minecraft:the_end".into())],
                        },
                    )])),
                ),
            ]))
        })
        .collect();
    let document = nbt::Document {
        name: "".into(),
        root: fields(vec![
            ("DataVersion", Tag::Int(3218)),
            ("xPos", Tag::Int(0)),
            ("zPos", Tag::Int(0)),
            ("Status", Tag::String("full".into())),
            (
                "sections",
                Tag::List {
                    kind: 10,
                    values: sections,
                },
            ),
        ]),
    };
    loader::LoadedWindow {
        chunks: BTreeMap::from([(
            [0, 0],
            Arc::new(
                chunk::decode(&document, [0, 0], chunk::Limits::default(), &|| false).unwrap(),
            ),
        )]),
        coverage: Coverage::default(),
        ..loader::LoadedWindow::default()
    }
}
fn source() -> evidence::Source {
    evidence::Source {
        map: evidence::MapId([17; 16]),
        generation: 7,
    }
}

fn retry_candidates(count: usize) -> Vec<windows::Candidate> {
    (0..count)
        .map(|index| windows::Candidate {
            center: [index as i32, 0],
            requested: [[index as i32, 0]].into(),
            bounds: core(),
        })
        .collect()
}

#[test]
fn retries_rejections_in_order_and_stops_at_first_whole_window() {
    let candidates = retry_candidates(4);
    let mut attempts = Vec::new();
    let output = load_candidates_with(&candidates, &|| false, |candidate| {
        attempts.push(candidate.center);
        if attempts.len() < 3 {
            return Err(Error::SummaryLimit);
        }
        finish_window(loaded(), source(), core(), 0, Limits::default(), &|| false)
    })
    .unwrap();
    assert_eq!(attempts, [[0, 0], [1, 0], [2, 0]]);
    assert_eq!(output.selected_index, Some(2));
    assert_eq!(output.rejected.len(), 2);
    assert!(matches!(output.rejected[0].error, Error::SummaryLimit));
    assert_eq!(output.window.unwrap().source, source());
}

#[test]
fn exhausted_candidate_subset_preserves_errors_and_empty_subset_does_no_work() {
    let output = load_candidates_with(&retry_candidates(3), &|| false, |_| {
        Err(Error::Loader(loader::Error::Limit))
    })
    .unwrap();
    assert!(output.window.is_none());
    assert_eq!(output.selected_index, None);
    assert_eq!(output.rejected.len(), 3);
    assert_eq!(output.rejected[2].center, [2, 0]);
    let empty = load_candidates_with(&[], &|| false, |_| panic!("empty list accessed")).unwrap();
    assert!(empty.window.is_none() && empty.rejected.is_empty());
}

#[test]
fn retry_limits_and_all_cancellation_paths_stop_without_partial_output() {
    use std::cell::Cell;
    assert!(matches!(
        load_candidates_with(&retry_candidates(17), &|| false, |_| panic!("over cap")),
        Err(Error::CandidateLimit)
    ));
    assert!(matches!(
        load_candidates_with(&retry_candidates(1), &|| true, |_| panic!("cancelled")),
        Err(Error::Loader(loader::Error::Cancelled))
    ));
    for error in [
        Error::Loader(loader::Error::Cancelled),
        Error::Surface(surface::Error::Cancelled),
        Error::Evidence(evidence::Error::Surface(surface::Error::Cancelled)),
    ] {
        let mut error = Some(error);
        let mut calls = 0;
        assert!(matches!(
            load_candidates_with(&retry_candidates(2), &|| false, |_| {
                calls += 1;
                Err(error.take().expect("cancelled attempt was retried"))
            }),
            Err(Error::Loader(loader::Error::Cancelled))
        ));
        assert_eq!(calls, 1);
    }
    let stop = Cell::new(false);
    assert!(matches!(
        load_candidates_with(&retry_candidates(2), &|| stop.get(), |_| {
            let output = finish_window(loaded(), source(), core(), 0, Limits::default(), &|| false);
            stop.set(true);
            output
        }),
        Err(Error::Loader(loader::Error::Cancelled))
    ));
}

#[test]
fn public_retry_rejects_invalid_context_and_reports_absent_candidate_subset() {
    let directory = tempfile::tempdir().unwrap();
    let absent = directory.path().join("never-created-region");
    let mut candidates = retry_candidates(2);
    candidates[1].bounds = surface::Bounds {
        minimum: [16, 0],
        maximum: [31, 15],
    };
    let run = |source, candidates: &[windows::Candidate], cancel: &dyn Fn() -> bool| {
        load_candidates(
            &absent,
            source,
            candidates,
            loader::Limits::default(),
            Limits::default(),
            cancel,
        )
    };
    assert!(matches!(
        run(
            evidence::Source {
                generation: 0,
                ..source()
            },
            &[],
            &|| false
        ),
        Err(Error::InvalidGeneration)
    ));
    assert!(matches!(
        run(source(), &candidates, &|| true),
        Err(Error::Loader(loader::Error::Cancelled))
    ));
    assert!(matches!(
        run(source(), &retry_candidates(17), &|| false),
        Err(Error::CandidateLimit)
    ));
    let output = run(source(), &candidates, &|| false).unwrap();
    assert!(output.window.is_none());
    assert_eq!(output.selected_index, None);
    assert_eq!(output.rejected.len(), 2);
    for (candidate, rejected) in candidates.iter().zip(output.rejected) {
        assert_eq!(rejected.center, candidate.center);
        assert!(matches!(
            rejected.error,
            Error::Surface(surface::Error::Unqualified { .. })
        ));
    }
    assert!(!absent.exists());
}
fn core() -> surface::Bounds {
    surface::Bounds {
        minimum: [0, 0],
        maximum: [15, 15],
    }
}

#[test]
fn candidate_loading_checks_generation_bounds_and_cancel_before_file_access() {
    let directory = tempfile::tempdir().unwrap();
    let mut candidate = windows::Candidate {
        center: [0, 0],
        requested: [[0, 0]].into(),
        bounds: core(),
    };
    let run = |candidate: &windows::Candidate, source, cancelled: &dyn Fn() -> bool| {
        load_candidate(
            directory.path(),
            source,
            candidate,
            loader::Limits::default(),
            Limits::default(),
            cancelled,
        )
    };
    assert!(matches!(
        run(&candidate, source(), &|| true),
        Err(Error::Loader(loader::Error::Cancelled))
    ));
    assert!(matches!(
        run(
            &candidate,
            evidence::Source {
                generation: 0,
                ..source()
            },
            &|| false
        ),
        Err(Error::InvalidGeneration)
    ));
    candidate.bounds.maximum[0] = 14;
    assert!(matches!(
        run(&candidate, source(), &|| false),
        Err(Error::InvalidCandidate)
    ));
    candidate.bounds = core();
    candidate.center = [1, 0];
    assert!(matches!(
        run(&candidate, source(), &|| false),
        Err(Error::InvalidCandidate)
    ));
    candidate.center = [0, 0];
    // An absent region stays unqualified, despite a forged complete header set.
    assert!(matches!(
        run(&candidate, source(), &|| false),
        Err(Error::Surface(surface::Error::Unqualified { .. }))
    ));
}

#[test]
fn candidate_loading_rejects_holes_and_oversize_sets_before_file_access() {
    let directory = tempfile::tempdir().unwrap();
    let mut requested = std::collections::BTreeSet::new();
    for x in -1..=1 {
        for z in -1..=1 {
            requested.insert([x, z]);
        }
    }
    requested.remove(&[0, 0]);
    let candidate = windows::Candidate {
        center: [0, 0],
        requested,
        bounds: surface::Bounds {
            minimum: [-16, -16],
            maximum: [31, 31],
        },
    };
    assert!(matches!(
        load_candidate(
            directory.path(),
            source(),
            &candidate,
            loader::Limits::default(),
            Limits::default(),
            &|| false
        ),
        Err(Error::InvalidCandidate)
    ));
    assert!(matches!(
        load_candidate(
            directory.path(),
            source(),
            &candidate,
            loader::Limits {
                max_chunks: 1,
                ..loader::Limits::default()
            },
            Limits::default(),
            &|| false
        ),
        Err(Error::Loader(loader::Error::Limit))
    ));
}

#[test]
fn owned_handoff_retains_exact_chunks_and_ignores_forged_coverage_and_biomes() {
    let mut input = loaded();
    input.coverage.chunks.insert([999, 999]);
    let retained = Arc::clone(&input.chunks[&[0, 0]]);
    let output = finish_window(input, source(), core(), 0, Limits::default(), &|| false).unwrap();
    assert!(Arc::ptr_eq(&retained, &output.loaded.chunks[&[0, 0]]));
    assert_eq!(Arc::strong_count(&retained), 2);
    assert_eq!(output.loaded.coverage.chunks, [[0, 0]].into());
    assert_eq!(output.source, source());
    assert_eq!(output.core, core());
    assert_eq!(output.bounds, core());
    assert_eq!(output.stats.columns, 256);
    assert!(output
        .targets
        .iter()
        .any(|target| target.key.category == evidence::Category::OpenGrassland));
    assert!(output
        .targets
        .iter()
        .all(|target| target.source == source() && target.key.map == source().map));
    assert!(output.work_used > 0);
}

#[test]
fn incomplete_halo_or_input_never_returns_partial_evidence() {
    assert!(matches!(
        finish_window(loaded(), source(), core(), 1, Limits::default(), &|| false),
        Err(Error::Surface(surface::Error::Unqualified { .. }))
    ));
    let mut input = loaded();
    let mut proto = (*input.chunks[&[0, 0]]).clone();
    proto.identity.position = [1, 0];
    proto.status = Some("noise".into());
    input.chunks.insert([1, 0], Arc::new(proto));
    assert!(matches!(
        finish_window(input, source(), core(), 0, Limits::default(), &|| false),
        Err(Error::UnqualifiedInput)
    ));
}

#[test]
fn cancellation_work_and_summary_limits_drop_the_whole_handoff() {
    let input = loaded();
    let retained = Arc::clone(&input.chunks[&[0, 0]]);
    assert!(matches!(
        finish_window(input, source(), core(), 0, Limits::default(), &|| true),
        Err(Error::Surface(surface::Error::Cancelled))
    ));
    assert_eq!(Arc::strong_count(&retained), 1);
    assert!(finish_window(
        loaded(),
        source(),
        core(),
        0,
        Limits {
            work_units: 0,
            ..Limits::default()
        },
        &|| false
    )
    .is_err());
    assert!(matches!(
        finish_window(
            loaded(),
            source(),
            core(),
            0,
            Limits {
                summary_bytes: 0,
                ..Limits::default()
            },
            &|| false
        ),
        Err(Error::SummaryLimit)
    ));
    assert!(matches!(
        finish_window(
            loaded(),
            evidence::Source {
                generation: 0,
                ..source()
            },
            core(),
            0,
            Limits::default(),
            &|| false
        ),
        Err(Error::InvalidGeneration)
    ));
}

#[test]
fn signed_coordinates_and_chunk_lookup_keys_stay_consistent() {
    let mut input = loaded();
    let chunk = input.chunks.remove(&[0, 0]).unwrap();
    let mut translated = (*chunk).clone();
    translated.identity.position = [-1, -2];
    let translated = Arc::new(translated);
    input.chunks.insert([-1, -2], Arc::clone(&translated));
    let bounds = surface::Bounds {
        minimum: [-16, -32],
        maximum: [-1, -17],
    };
    let output = finish_window(input, source(), bounds, 0, Limits::default(), &|| false).unwrap();
    assert!(Arc::ptr_eq(&translated, &output.loaded.chunks[&[-1, -2]]));
    assert_eq!(output.loaded.coverage.chunks, [[-1, -2]].into());
    assert!(!output.targets.is_empty());
    assert!(output.targets.iter().all(|target| {
        target.key.tile == [-1, -2] && bounds.contains([target.key.anchor[0], target.key.anchor[2]])
    }));
    let mut wrong_key = loaded();
    let chunk = wrong_key.chunks.remove(&[0, 0]).unwrap();
    wrong_key.chunks.insert([1, 0], chunk);
    assert!(matches!(
        finish_window(wrong_key, source(), core(), 0, Limits::default(), &|| false),
        Err(Error::UnqualifiedInput)
    ));
    let exact_struct_bytes = std::mem::size_of::<PreparedWindow>();
    assert!(matches!(
        finish_window(
            loaded(),
            source(),
            core(),
            0,
            Limits {
                summary_bytes: exact_struct_bytes,
                ..Limits::default()
            },
            &|| false
        ),
        Err(Error::SummaryLimit)
    ));
}
