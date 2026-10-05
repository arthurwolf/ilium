use super::*;
use std::cell::Cell;

#[test]
fn native_palette_keeps_distinct_properties_and_x_fast_y_up_cells() {
    let first = BTreeMap::from([(String::from("level"), String::from("7"))]);
    let second = BTreeMap::from([(String::from("level"), String::from("9"))]);
    let a = BlockStateView {
        name: "minecraft:water",
        properties: &first,
    };
    let b = BlockStateView {
        name: "minecraft:water",
        properties: &second,
    };
    let spec = RegionSpec {
        origin: [-16, -64, 32],
        size: [2, 2, 2],
        max_bytes: 4096,
    };
    let volume = collect_region(
        spec,
        RegionLimits {
            cells: 8,
            palette: 2,
            work: 4096,
        },
        || false,
        |_| Ok(()),
        |position| {
            Ok(Some(
                if matches!(position, [-15, -64, 32] | [-16, -63, 33]) {
                    b
                } else {
                    a
                },
            ))
        },
    )
    .unwrap();
    assert_eq!(volume.blocks, [0, 1, 0, 0, 0, 0, 1, 0]);
    assert_eq!(volume.palette, [a, b]);
    assert!(std::ptr::eq(volume.palette[0].properties, &first));
    assert!(std::ptr::eq(volume.palette[1].properties, &second));
    assert_eq!(volume.origin, spec.origin);
    assert_eq!(volume.size, spec.size);
}

#[test]
fn original_admission_refusal_prevents_cell_access() {
    let reads = Cell::new(0);
    let result = collect_region(
        RegionSpec {
            origin: [0; 3],
            size: [1; 3],
            max_bytes: 4096,
        },
        RegionLimits {
            cells: 1,
            palette: 1,
            work: 4096,
        },
        || false,
        |_| Err::<(), _>(RegionError::Admission),
        |_| {
            reads.set(reads.get() + 1);
            Ok(None)
        },
    );
    assert!(matches!(result, Err(RegionError::Admission)));
    assert_eq!(reads.get(), 0);
}

#[test]
fn missing_decoded_cell_is_refused_without_fabricating_air() {
    let result = collect_region(
        RegionSpec {
            origin: [-1, 0, 0],
            size: [1; 3],
            max_bytes: 4096,
        },
        RegionLimits {
            cells: 1,
            palette: 1,
            work: 4096,
        },
        || false,
        |_| Ok(()),
        |_| Ok(None),
    );
    assert!(matches!(result, Err(RegionError::MissingCell([-1, 0, 0]))));
}

#[test]
fn endpoint_overflow_refuses_before_admission_or_source_access() {
    let admitted = Cell::new(false);
    let result = collect_region(
        RegionSpec {
            origin: [i32::MAX, 0, 0],
            size: [2, 1, 1],
            max_bytes: 4096,
        },
        RegionLimits {
            cells: 2,
            palette: 2,
            work: 4096,
        },
        || false,
        |_| {
            admitted.set(true);
            Ok(())
        },
        |_| Ok(None),
    );
    assert!(matches!(result, Err(RegionError::Extent)));
    assert!(!admitted.get());
}

#[test]
fn cancelled_request_refuses_before_admission_or_source_access() {
    let admitted = Cell::new(false);
    let result = collect_region(
        RegionSpec {
            origin: [0; 3],
            size: [1; 3],
            max_bytes: 4096,
        },
        RegionLimits {
            cells: 1,
            palette: 1,
            work: 4096,
        },
        || true,
        |_| {
            admitted.set(true);
            Ok(())
        },
        |_| Ok(None),
    );
    assert!(matches!(result, Err(RegionError::Cancelled)));
    assert!(!admitted.get());
}

struct TestAdmission<'a>(&'a Cell<usize>);
impl Drop for TestAdmission<'_> {
    fn drop(&mut self) {
        self.0.set(0);
    }
}

#[test]
fn admitted_projection_retains_the_supplied_guard_until_the_output_is_dropped() {
    let properties = BTreeMap::new();
    let charged = Cell::new(0);
    let volume = collect_region(
        RegionSpec {
            origin: [0; 3],
            size: [1; 3],
            max_bytes: 4096,
        },
        RegionLimits {
            cells: 1,
            palette: 1,
            work: 4096,
        },
        || false,
        |bytes| {
            charged.set(bytes);
            Ok(TestAdmission(&charged))
        },
        |_| {
            Ok(Some(BlockStateView {
                name: "minecraft:air",
                properties: &properties,
            }))
        },
    )
    .unwrap();
    assert!(charged.get() > 4096);
    assert_eq!(
        volume.palette[usize::from(volume.blocks[0])].name,
        "minecraft:air"
    );
    drop(volume);
    assert_eq!(charged.get(), 0);
}

#[test]
fn cancellation_after_one_cell_drops_partial_buffers_and_the_supplied_guard() {
    let properties = BTreeMap::new();
    let cancelled = Cell::new(false);
    let charged = Cell::new(0);
    let reads = Cell::new(0);
    let result = collect_region(
        RegionSpec {
            origin: [0; 3],
            size: [2, 1, 1],
            max_bytes: 4096,
        },
        RegionLimits {
            cells: 2,
            palette: 1,
            work: 4096,
        },
        || cancelled.get(),
        |bytes| {
            charged.set(bytes);
            Ok(TestAdmission(&charged))
        },
        |_| {
            reads.set(reads.get() + 1);
            cancelled.set(true);
            Ok(Some(BlockStateView {
                name: "minecraft:air",
                properties: &properties,
            }))
        },
    );
    assert!(matches!(result, Err(RegionError::Cancelled)));
    assert_eq!(reads.get(), 1);
    assert_eq!(charged.get(), 0);
}

#[test]
fn response_byte_ceiling_includes_semantic_metadata_and_releases_admission() {
    let properties = BTreeMap::from([(String::from("level"), String::from("7"))]);
    let charged = Cell::new(0);
    let result = collect_region(
        RegionSpec {
            origin: [0; 3],
            size: [1; 3],
            max_bytes: 258,
        },
        RegionLimits {
            cells: 1,
            palette: 1,
            work: 4096,
        },
        || false,
        |bytes| {
            charged.set(bytes);
            Ok(TestAdmission(&charged))
        },
        |_| {
            Ok(Some(BlockStateView {
                name: "minecraft:water",
                properties: &properties,
            }))
        },
    );
    assert!(matches!(result, Err(RegionError::Bytes)));
    assert_eq!(charged.get(), 0);
}

#[test]
fn distinct_property_state_beyond_palette_limit_refuses_the_entire_projection() {
    let first = BTreeMap::from([(String::from("level"), String::from("7"))]);
    let second = BTreeMap::from([(String::from("level"), String::from("9"))]);
    let result = collect_region(
        RegionSpec {
            origin: [0; 3],
            size: [2, 1, 1],
            max_bytes: 4096,
        },
        RegionLimits {
            cells: 2,
            palette: 1,
            work: 4096,
        },
        || false,
        |_| Ok(()),
        |position| {
            Ok(Some(BlockStateView {
                name: "minecraft:water",
                properties: if position[0] == 0 { &first } else { &second },
            }))
        },
    );
    assert!(matches!(result, Err(RegionError::Palette)));
}

#[test]
fn work_ceiling_refuses_projection_before_unbounded_palette_search() {
    let properties = BTreeMap::new();
    let result = collect_region(
        RegionSpec {
            origin: [0; 3],
            size: [1; 3],
            max_bytes: 4096,
        },
        RegionLimits {
            cells: 1,
            palette: 1,
            work: 1,
        },
        || false,
        |_| Ok(()),
        |_| {
            Ok(Some(BlockStateView {
                name: "minecraft:air",
                properties: &properties,
            }))
        },
    );
    assert!(matches!(result, Err(RegionError::Work)));
}
