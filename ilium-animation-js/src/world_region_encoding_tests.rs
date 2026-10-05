use super::*;
use crate::world_region::{collect_region, BlockStateView, RegionLimits, RegionSpec};
use std::{cell::Cell, collections::BTreeMap};
struct Guard<'a>(&'a Cell<usize>);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
fn source<'a>(
    props: &'a BTreeMap<String, String>,
    drops: &'a Cell<usize>,
) -> BorrowedRegion<'a, Guard<'a>> {
    collect_region(
        RegionSpec {
            origin: [-1, 0, 0],
            size: [2, 1, 2],
            max_bytes: 4096,
        },
        RegionLimits {
            cells: 4,
            palette: 2,
            work: 4096,
        },
        || false,
        |_| Ok(Guard(drops)),
        |position| {
            Ok(Some(BlockStateView {
                name: if position[0] == -1 {
                    "minecraft:water"
                } else {
                    "minecraft:stone"
                },
                properties: props,
            }))
        },
    )
    .unwrap()
}
#[test]
fn typed_region_wire_preserves_palette_escapes_axes_and_owned_buffer_admission() {
    let props = BTreeMap::from([(String::from("note"), String::from("a\"\\\n☃"))]);
    let original_drops = Cell::new(0);
    let encoded_drops = Cell::new(0);
    let value = encode_region(
        source(&props, &original_drops),
        &"a".repeat(64),
        4096,
        4096,
        || false,
        |_| Ok(Guard(&encoded_drops)),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let metadata: serde_json::Value = serde_json::from_slice(value.metadata()).unwrap();
    assert_eq!(metadata["ok"], true);
    assert_eq!(metadata["value"]["origin"], serde_json::json!([-1, 0, 0]));
    assert_eq!(metadata["value"]["size"], serde_json::json!([2, 1, 2]));
    assert_eq!(
        metadata["value"]["blocks"],
        serde_json::json!({"$ilium_binary":"blocks"})
    );
    assert_eq!(metadata["value"]["identity"], "a".repeat(64));
    assert_eq!(metadata["value"]["palette"][0]["name"], "minecraft:water");
    assert_eq!(metadata["value"]["palette"][1]["name"], "minecraft:stone");
    assert_eq!(
        metadata["value"]["palette"][0]["properties"]["note"],
        "a\"\\\n☃"
    );
    assert_eq!(value.blocks(), [0, 0, 1, 0, 0, 0, 1, 0]);
    assert!(value.metadata().len() + value.blocks().len() <= 4096);
    assert_eq!(original_drops.get(), 1);
    assert_eq!(encoded_drops.get(), 0);
    drop(value);
    assert_eq!(encoded_drops.get(), 1);
}
#[test]
fn byte_refusal_happens_before_encoding_admission() {
    let props = BTreeMap::new();
    let drops = Cell::new(0);
    let admitted = Cell::new(false);
    let result = encode_region(
        source(&props, &drops),
        &"a".repeat(64),
        1,
        4096,
        || false,
        |_| {
            admitted.set(true);
            Ok(())
        },
    );
    assert!(matches!(result, Err(RegionError::Bytes)));
    assert!(!admitted.get());
    assert_eq!(drops.get(), 1);
}
#[test]
fn encoder_uses_original_admission_and_never_returns_unowned_buffers() {
    let props = BTreeMap::new();
    let drops = Cell::new(0);
    let result = encode_region(
        source(&props, &drops),
        &"a".repeat(64),
        4096,
        4096,
        || false,
        |_| Err::<(), _>(RegionError::Admission),
    );
    assert!(matches!(result, Err(RegionError::Admission)));
    assert_eq!(drops.get(), 1);
}
#[test]
fn cancelled_serialization_drops_both_admissions_without_a_partial_result() {
    let props = BTreeMap::new();
    let original = Cell::new(0);
    let encoded = Cell::new(0);
    let checks = Cell::new(0);
    let result = encode_region(
        source(&props, &original),
        &"a".repeat(64),
        4096,
        4096,
        || {
            checks.set(checks.get() + 1);
            checks.get() > 4
        },
        |_| Ok(Guard(&encoded)),
    );
    assert!(matches!(result, Err(RegionError::Cancelled)));
    assert_eq!(original.get(), 1);
    assert_eq!(encoded.get(), 1);
}
#[test]
fn finite_encoding_work_is_enforced_and_returns_no_body() {
    let props = BTreeMap::new();
    let original = Cell::new(0);
    let encoded = Cell::new(0);
    let result = encode_region(
        source(&props, &original),
        &"a".repeat(64),
        4096,
        1,
        || false,
        |_| Ok(Guard(&encoded)),
    );
    assert!(matches!(result, Err(RegionError::Work)));
    assert_eq!(original.get(), 1);
    assert_eq!(encoded.get(), 1);
}

#[test]
fn signed_coordinate_endpoint_overflow_is_refused_before_encoding_allocation() {
    let props = BTreeMap::new();
    let drops = Cell::new(0);
    let admitted = Cell::new(false);
    let mut region = source(&props, &drops);
    region.origin[0] = i32::MAX;
    let result = encode_region(
        region,
        &"a".repeat(64),
        4096,
        4096,
        || false,
        |_| {
            admitted.set(true);
            Ok(())
        },
    );
    assert!(matches!(result, Err(RegionError::Extent)));
    assert!(!admitted.get());
    assert_eq!(drops.get(), 1);
}
