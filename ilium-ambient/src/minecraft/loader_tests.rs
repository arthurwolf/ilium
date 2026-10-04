use super::super::nbt;
use super::*;
use std::{cell::Cell, collections::BTreeSet};

fn decoded(position: [i32; 2], status: &str, sections: &[i8]) -> chunk::DecodedChunk {
    let section_tags = sections
        .iter()
        .map(|y| {
            nbt::Tag::Compound(
                [
                    ("Y".into(), nbt::Tag::Byte(*y)),
                    (
                        "block_states".into(),
                        nbt::Tag::Compound(
                            [(
                                "palette".into(),
                                nbt::Tag::List {
                                    kind: 10,
                                    values: vec![nbt::Tag::Compound(
                                        [(
                                            "Name".into(),
                                            nbt::Tag::String("minecraft:stone".into()),
                                        )]
                                        .into(),
                                    )],
                                },
                            )]
                            .into(),
                        ),
                    ),
                ]
                .into(),
            )
        })
        .collect();
    let document = nbt::Document {
        name: "".into(),
        root: [
            ("DataVersion".into(), nbt::Tag::Int(3218)),
            ("xPos".into(), nbt::Tag::Int(position[0])),
            ("zPos".into(), nbt::Tag::Int(position[1])),
            ("Status".into(), nbt::Tag::String(status.into())),
            (
                "sections".into(),
                nbt::Tag::List {
                    kind: 10,
                    values: section_tags,
                },
            ),
        ]
        .into(),
    };
    chunk::decode(&document, position, chunk::Limits::default(), &|| false).unwrap()
}

#[test]
fn only_complete_saved_chunks_enter_coverage() {
    let requested = [[-1, 0], [0, 0], [1, 0], [2, 0], [3, 0]].into();
    let result = load_with(&requested, Limits::default(), &|| false, |position| {
        Ok(match position[0] {
            -1 => Some(decoded(
                position,
                "minecraft:full",
                &(-4..=19).collect::<Vec<_>>(),
            )),
            0 => Some(decoded(position, "noise", &(-4..=19).collect::<Vec<_>>())),
            1 => Some(decoded(position, "full", &[-4, 19])),
            2 => None,
            _ => return Err(ReadError::Region(region::Error::Changed)),
        })
    })
    .unwrap();
    assert_eq!(result.chunks.len(), 1);
    assert_eq!(result.coverage.chunks, BTreeSet::from([[-1, 0]]));
    assert_eq!(result.rejected_chunks, 4);
    assert_eq!(result.issues.len(), 4);
    assert_eq!(result.issues[0].reason, Rejection::ProtoChunk);
    assert_eq!(result.issues[1].reason, Rejection::IncompleteSections);
    assert_eq!(result.issues[2].reason, Rejection::Absent);
}

#[test]
fn oversized_request_is_rejected_before_reading() {
    let calls = Cell::new(0);
    let result = load_with(
        &[[0, 0], [1, 0]].into(),
        Limits {
            max_chunks: 1,
            ..Limits::default()
        },
        &|| false,
        |_| {
            calls.set(calls.get() + 1);
            Ok(None)
        },
    );
    assert!(matches!(result, Err(Error::Limit)));
    assert_eq!(calls.get(), 0);
}

#[test]
fn cancellation_discards_the_partial_window() {
    let calls = Cell::new(0);
    let result = load_with(
        &[[0, 0], [1, 0]].into(),
        Limits::default(),
        &|| calls.get() > 0,
        |position| {
            calls.set(calls.get() + 1);
            Ok(Some(decoded(
                position,
                "full",
                &(-4..=19).collect::<Vec<_>>(),
            )))
        },
    );
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(calls.get(), 1);
}

#[test]
fn decoder_cancellation_is_not_a_corrupt_chunk_issue() {
    let result = load_with(&[[0, 0]].into(), Limits::default(), &|| false, |_| {
        Err(ReadError::Chunk(chunk::Error::Cancelled))
    });
    assert!(matches!(result, Err(Error::Cancelled)));
}

#[test]
fn nbt_cancellation_wrappers_discard_previous_loaded_chunks() {
    for nested in [false, true] {
        let calls = Cell::new(0);
        let result = load_with(
            &[[0, 0], [1, 0]].into(),
            Limits::default(),
            &|| false,
            |position| {
                calls.set(calls.get() + 1);
                if position[0] == 0 {
                    return Ok(Some(decoded(
                        position,
                        "full",
                        &(-4..=19).collect::<Vec<_>>(),
                    )));
                }
                let error = region::Error::Nbt(nbt::Error {
                    offset: 27,
                    reason: "cancelled",
                });
                Err(if nested {
                    ReadError::Chunk(chunk::Error::Region(error))
                } else {
                    ReadError::Region(error)
                })
            },
        );
        assert_eq!(calls.get(), 2);
        assert!(matches!(result, Err(Error::Cancelled)));
    }
}

#[test]
fn coordinate_mismatch_never_qualifies_a_different_chunk() {
    let result = load_with(&[[0, 0]].into(), Limits::default(), &|| false, |_| {
        Ok(Some(decoded(
            [1, 0],
            "full",
            &(-4..=19).collect::<Vec<_>>(),
        )))
    })
    .unwrap();
    assert!(result.coverage.chunks.is_empty());
    assert_eq!(result.rejected_chunks, 1);
    assert_eq!(result.issues[0].reason, Rejection::CoordinateMismatch);
}

#[test]
fn rejection_details_are_bounded_but_counts_are_complete() {
    let requested = (0..90).map(|x| [x, 0]).collect();
    let result = load_with(&requested, Limits::default(), &|| false, |_| Ok(None)).unwrap();
    assert_eq!(result.rejected_chunks, 90);
    assert_eq!(result.issues.len(), 64);
}

#[test]
fn invalid_section_range_never_calls_the_reader() {
    let result = load_with(
        &[[0, 0]].into(),
        Limits {
            min_section: 19,
            max_section: -4,
            ..Limits::default()
        },
        &|| false,
        |_| panic!("unexpected read"),
    );
    assert!(matches!(result, Err(Error::InvalidSections)));
}

#[test]
fn unaddressable_chunk_coordinates_are_rejected_before_io() {
    for position in [[i32::MAX, 0], [0, i32::MIN]] {
        let result = load_with(&[position].into(), Limits::default(), &|| false, |_| {
            panic!("unaddressable chunk must not be read")
        });
        assert!(matches!(result, Err(Error::InvalidCoordinates)));
    }
}

#[test]
fn requested_square_uses_floor_coordinates_and_has_no_generation_semantics() {
    assert_eq!(requested_square([-1, -16], 0).unwrap(), [[-1, -1]].into());
    let requested = requested_square([-1, 16], 5).unwrap();
    assert_eq!(requested.len(), 121);
    assert!(requested.contains(&[-6, -4]));
    assert!(requested.contains(&[4, 6]));
    assert!(!requested.contains(&[5, 6]));
}

#[test]
fn requested_square_rejects_large_or_unaddressable_windows() {
    assert!(matches!(requested_square([0, 0], 6), Err(Error::Limit)));
    assert!(matches!(requested_square([0, 0], 255), Err(Error::Limit)));
    assert!(matches!(
        requested_square([i32::MIN, 0], 1),
        Err(Error::InvalidCoordinates)
    ));
    assert!(matches!(
        requested_square([i32::MAX, 0], 1),
        Err(Error::InvalidCoordinates)
    ));
    assert_eq!(requested_square([i32::MIN, i32::MAX], 0).unwrap().len(), 1);
}

#[test]
fn retained_storage_limit_discards_the_whole_window() {
    let calls = Cell::new(0);
    let result = load_with(
        &[[0, 0], [1, 0]].into(),
        Limits {
            max_storage_charge: std::mem::size_of::<LoadedWindow>() + 1,
            ..Limits::default()
        },
        &|| false,
        |position| {
            calls.set(calls.get() + 1);
            Ok(Some(decoded(
                position,
                "full",
                &(-4..=19).collect::<Vec<_>>(),
            )))
        },
    );
    assert!(matches!(result, Err(Error::StorageLimit)));
    assert_eq!(calls.get(), 0);
}

#[test]
fn invalid_storage_budget_never_calls_the_reader() {
    for max_storage_charge in [0, (128 << 20) + 1] {
        let result = load_with(
            &[[0, 0]].into(),
            Limits {
                max_storage_charge,
                ..Limits::default()
            },
            &|| false,
            |_| panic!("invalid storage budget must fail before I/O"),
        );
        assert!(matches!(result, Err(Error::StorageLimit)));
    }
}

#[test]
fn aggregate_storage_admission_stops_before_reading_later_chunks() {
    // Each chunk separately fits; admitting many must exhaust the same budget.
    let limits = Limits {
        max_storage_charge: 512 << 10,
        ..Limits::default()
    };
    let sections = (-4..=19).collect::<Vec<_>>();
    let one = load_with(&[[0, 0]].into(), limits, &|| false, |p| {
        Ok(Some(decoded(p, "full", &sections)))
    })
    .unwrap();
    assert_eq!(one.chunks.len(), 1);
    let calls = Cell::new(0);
    let result = load_with(
        &(0..128).map(|x| [x, 0]).collect(),
        limits,
        &|| false,
        |p| {
            calls.set(calls.get() + 1);
            Ok(Some(decoded(p, "full", &sections)))
        },
    );
    assert!(matches!(result, Err(Error::StorageLimit)));
    assert!((2..128).contains(&calls.get()));
    assert!(matches!(
        load_with(
            &BTreeSet::new(),
            Limits {
                max_storage_charge: 1,
                ..Limits::default()
            },
            &|| false,
            |_| panic!("empty request")
        ),
        Err(Error::StorageLimit)
    ));
}

#[test]
fn storage_charge_accounts_string_capacity_without_changing_states_and_cancels() {
    let mut chunk = decoded([0, 0], "full", &(-4..=19).collect::<Vec<_>>());
    let before = chunk.storage_charge(&|| false).unwrap();
    let old_capacity = chunk.status.as_ref().unwrap().capacity();
    chunk.status.as_mut().unwrap().reserve_exact(65536);
    let new_capacity = chunk.status.as_ref().unwrap().capacity();
    assert_eq!(
        chunk.storage_charge(&|| false).unwrap() - before,
        new_capacity - old_capacity
    );
    assert_eq!(chunk.status.as_deref(), Some("full"));
    let chunk::BlockSample::State(original) = chunk.block_at([0, 0, 0]) else {
        panic!("fixture state")
    };
    let original_pointer = original as *const chunk::BlockState;
    for stop in [1, 2, 25] {
        let calls = Cell::new(0);
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() == stop
        };
        assert!(matches!(
            chunk.storage_charge(&cancel),
            Err(chunk::Error::Cancelled)
        ));
        let chunk::BlockSample::State(current) = chunk.block_at([0, 0, 0]) else {
            panic!("fixture state")
        };
        assert_eq!(current as *const chunk::BlockState, original_pointer);
    }
}

#[test]
fn projected_ceiling_admits_a_larger_request_but_never_invents_missing_chunks() {
    let requested = (0..=128).map(|x| [x, 0]).collect::<BTreeSet<_>>();
    let calls = Cell::new(0);
    assert!(matches!(
        load_with(&requested, Limits::default(), &|| false, |_| {
            calls.set(calls.get() + 1);
            Ok(None)
        }),
        Err(Error::Limit)
    ));
    assert_eq!(calls.get(), 0);
    let result = load_with_ceiling(
        &requested,
        Limits {
            max_chunks: MAX_PROJECTED_CHUNKS,
            max_storage_charge: MAX_PROJECTED_STORAGE_CHARGE,
            ..Limits::default()
        },
        [MAX_PROJECTED_CHUNKS, MAX_PROJECTED_STORAGE_CHARGE],
        &|| false,
        |_| {
            calls.set(calls.get() + 1);
            Ok(None)
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 129);
    assert_eq!(result.rejected_chunks, 129);
    assert!(result.coverage.chunks.is_empty());
    assert_eq!(result.issues.len(), 64);
}

#[test]
fn projected_ceiling_rejects_overflow_before_any_read() {
    let requested = (0..=512).map(|x| [x, 0]).collect::<BTreeSet<_>>();
    let calls = Cell::new(0);
    let result = load_with_ceiling(
        &requested,
        Limits {
            max_chunks: MAX_PROJECTED_CHUNKS,
            max_storage_charge: MAX_PROJECTED_STORAGE_CHARGE,
            ..Limits::default()
        },
        [MAX_PROJECTED_CHUNKS, MAX_PROJECTED_STORAGE_CHARGE],
        &|| false,
        |_| {
            calls.set(calls.get() + 1);
            Ok(None)
        },
    );
    assert!(matches!(result, Err(Error::Limit)));
    assert_eq!(calls.get(), 0);
}

#[test]
fn published_storage_charge_covers_exact_retained_chunk_and_diagnostic_allowance() {
    let requested = BTreeSet::from([[0, 0]]);
    let chunk = decoded([0, 0], "minecraft:full", &(-4..=19).collect::<Vec<_>>());
    let chunk_charge = chunk.storage_charge(&|| false).unwrap();
    let loaded = load_with(&requested, Limits::default(), &|| false, |_| {
        Ok(Some(chunk.clone()))
    })
    .unwrap();
    let overhead = 16
        * (std::mem::size_of::<[i32; 2]>()
            + std::mem::size_of::<Arc<chunk::DecodedChunk>>()
            + 2 * std::mem::size_of::<usize>());
    let expected = std::mem::size_of::<LoadedWindow>()
        + 64 * (std::mem::size_of::<Issue>() + 4 * 256)
        + chunk_charge
        + overhead;
    assert_eq!(loaded.retained_storage_charge, expected);
    assert_eq!(loaded.coverage.chunks, requested);
    assert!(loaded.retained_storage_charge <= Limits::default().max_storage_charge);
}
