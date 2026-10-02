use super::*;

fn square(center: [i32; 2], radius: i32) -> BTreeSet<[i32; 2]> {
    (center[0] - radius..=center[0] + radius)
        .flat_map(|x| (center[1] - radius..=center[1] + radius).map(move |z| [x, z]))
        .collect()
}

#[test]
fn anchor_is_tested_off_grid_and_headers_remain_only_candidates() {
    let allocated = square([-7, 13], 5);
    let result = search(&allocated, [-7, 13], &[], Limits::default(), &|| false).unwrap();
    assert!(result.scan_complete);
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].requested, allocated);
    assert_eq!(
        result.candidates[0].bounds,
        Bounds {
            minimum: [-192, 128],
            maximum: [-17, 303]
        }
    );
    let mut missing = allocated;
    missing.remove(&[-12, 8]);
    assert!(
        search(&missing, [-7, 13], &[], Limits::default(), &|| false)
            .unwrap()
            .candidates
            .is_empty()
    );
}

#[test]
fn separated_candidates_rank_by_distance_and_exclude_recent_windows() {
    let mut allocated = square([0, 0], 5);
    allocated.extend(square([-22, 0], 5));
    allocated.extend(square([22, 0], 5));
    let result = search(&allocated, [0, 0], &[], Limits::default(), &|| false).unwrap();
    let centers: Vec<_> = result.candidates.iter().map(|c| c.center).collect();
    assert_eq!(centers, vec![[0, 0], [-22, 0], [22, 0]]);
    for c in &result.candidates {
        assert_eq!(c.requested.len(), 121);
    }
    assert!(result.candidates[1]
        .requested
        .is_disjoint(&result.candidates[2].requested));
    let result = search(
        &allocated,
        [0, 0],
        &[[0, 0], [-22, 0]],
        Limits::default(),
        &|| false,
    )
    .unwrap();
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].center, [22, 0]);
}

#[test]
fn exhausted_work_does_not_claim_absent_terrain_and_cancellation_discards_prefix() {
    let allocated = square([0, 0], 5);
    let result = search(
        &allocated,
        [0, 0],
        &[],
        Limits {
            work_units: 120,
            ..Limits::default()
        },
        &|| false,
    )
    .unwrap();
    assert!(!result.scan_complete);
    assert!(result.candidates.is_empty());
    assert_eq!(result.work_used, 120);
    let calls = std::cell::Cell::new(0);
    assert_eq!(
        search(&allocated, [0, 0], &[], Limits::default(), &|| {
            calls.set(calls.get() + 1);
            calls.get() == 130
        })
        .unwrap_err(),
        Error::Cancelled
    );
}

#[test]
fn invalid_limits_and_unaddressable_candidates_never_overflow() {
    for limits in [
        Limits {
            radius_chunks: 6,
            ..Limits::default()
        },
        Limits {
            candidates: 0,
            ..Limits::default()
        },
        Limits {
            candidates: 17,
            ..Limits::default()
        },
        Limits {
            work_units: 0,
            ..Limits::default()
        },
    ] {
        assert_eq!(
            search(&BTreeSet::new(), [0, 0], &[], limits, &|| false).unwrap_err(),
            Error::Limits
        );
    }
    let allocated = BTreeSet::from([[i32::MIN, i32::MIN], [i32::MAX, i32::MAX]]);
    let result = search(
        &allocated,
        [i32::MAX, i32::MAX],
        &[],
        Limits::default(),
        &|| false,
    )
    .unwrap();
    assert!(result.scan_complete);
    assert!(result.candidates.is_empty());
}

#[test]
fn output_limit_is_distinct_from_scan_completion_and_header_count() {
    let allocated = BTreeSet::from([[0, 0], [1, 0], [2, 0], [3, 0]]);
    let result = search(
        &allocated,
        [9, 0],
        &[],
        Limits {
            radius_chunks: 0,
            candidates: 2,
            ..Limits::default()
        },
        &|| false,
    )
    .unwrap();
    assert!(result.scan_complete);
    assert_eq!(result.header_complete, 4);
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|c| c.center)
            .collect::<Vec<_>>(),
        vec![[3, 0], [2, 0]]
    );
    assert!(result.candidates.iter().all(|c| c.requested.len() == 1));
}

#[test]
fn excludes_overlapping_recent_footprint_but_allows_adjacent_window() {
    let mut allocated = square([0, 0], 5);
    allocated.extend(square([11, 0], 5));
    let result = search(&allocated, [0, 0], &[[1, 0]], Limits::default(), &|| false).unwrap();
    assert!(result.candidates.is_empty());
    let result = search(&allocated, [0, 0], &[[0, 0]], Limits::default(), &|| false).unwrap();
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].center, [11, 0]);
}
