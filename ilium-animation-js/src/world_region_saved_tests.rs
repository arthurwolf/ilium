use super::*;
use ilium_ambient::minecraft::{
    chunk,
    nbt::{Document, Tag, Text},
};
use ilium_execution::QuotaLimits;
fn field(name: &str, value: Tag) -> (Text, Tag) {
    (Text::from(name), value)
}
// Synthetic NBT exercises the real validated native decoder, not mirrored structs.
fn decoded(position: [i32; 2], level: &str) -> Arc<DecodedChunk> {
    let state = Tag::Compound(BTreeMap::from([
        field("Name", Tag::String(Text::from("minecraft:water"))),
        field(
            "Properties",
            Tag::Compound(BTreeMap::from([field(
                "level",
                Tag::String(Text::from(level)),
            )])),
        ),
    ]));
    let section = Tag::Compound(BTreeMap::from([
        field("Y", Tag::Byte(0)),
        field(
            "block_states",
            Tag::Compound(BTreeMap::from([field(
                "palette",
                Tag::List {
                    kind: 10,
                    values: vec![state],
                },
            )])),
        ),
    ]));
    let document = Document {
        name: Text::from(""),
        root: BTreeMap::from([
            field("DataVersion", Tag::Int(3105)),
            field("xPos", Tag::Int(position[0])),
            field("zPos", Tag::Int(position[1])),
            field("Status", Tag::String(Text::from("full"))),
            field(
                "sections",
                Tag::List {
                    kind: 10,
                    values: vec![section],
                },
            ),
        ]),
    };
    Arc::new(chunk::decode(&document, position, chunk::Limits::default(), &|| false).unwrap())
}
fn quota(bytes: usize) -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 1,
        input_bytes: bytes,
        result_bytes: bytes,
        worker_threads: 1,
        worker_bytes: bytes,
    })
}
fn spec() -> RegionSpec {
    RegionSpec {
        origin: [-1, 0, 0],
        size: [2, 1, 1],
        max_bytes: 4096,
    }
}
fn limits() -> RegionLimits {
    RegionLimits {
        cells: 8,
        palette: 8,
        work: 4096,
    }
}
#[test]
fn actual_decoded_chunk_palette_and_original_quota_survive_through_output() {
    let chunks = BTreeMap::from([
        ([-1, 0], decoded([-1, 0], "7")),
        ([0, 0], decoded([0, 0], "9")),
    ]);
    let q = quota(65536);
    let baseline = q.snapshot().worker_bytes;
    let volume = collect_saved_region(&chunks, &q, spec(), limits(), || false).unwrap();
    assert_eq!(volume.blocks, [0, 1]);
    assert_eq!(
        volume.palette[0]
            .properties
            .get("level")
            .map(String::as_str),
        Some("7")
    );
    assert_eq!(
        volume.palette[1]
            .properties
            .get("level")
            .map(String::as_str),
        Some("9")
    );
    let chunk::BlockSample::State(first) = chunks[&[-1, 0]].block_at([-1, 0, 0]) else {
        panic!("decoded source")
    };
    assert!(std::ptr::eq(
        volume.palette[0].properties,
        &first.properties
    ));
    assert!(q.snapshot().worker_bytes > baseline);
    drop(volume);
    assert_eq!(q.snapshot().worker_bytes, baseline);
}
#[test]
fn missing_native_chunk_refuses_whole_volume_and_releases_original_charge() {
    let chunks = BTreeMap::from([([-1, 0], decoded([-1, 0], "7"))]);
    let q = quota(65536);
    let baseline = q.snapshot().worker_bytes;
    let result = collect_saved_region(&chunks, &q, spec(), limits(), || false);
    assert!(matches!(result, Err(RegionError::MissingCell([0, 0, 0]))));
    assert_eq!(q.snapshot().worker_bytes, baseline);
}
#[test]
fn original_quota_refusal_keeps_native_source_untouched() {
    let chunk = decoded([-1, 0], "7");
    let original = Arc::clone(&chunk);
    let chunks = BTreeMap::from([([-1, 0], chunk)]);
    let q = quota(1);
    assert!(matches!(
        collect_saved_region(&chunks, &q, spec(), limits(), || false),
        Err(RegionError::Admission)
    ));
    assert!(Arc::ptr_eq(&chunks[&[-1, 0]], &original));
    assert_eq!(q.snapshot().worker_bytes, 0);
}
#[test]
fn missing_native_section_is_not_air() {
    let chunks = BTreeMap::from([([-1, 0], decoded([-1, 0], "7"))]);
    let q = quota(65536);
    let spec = RegionSpec {
        origin: [-1, 16, 0],
        size: [1; 3],
        max_bytes: 4096,
    };
    assert!(matches!(
        collect_saved_region(&chunks, &q, spec, limits(), || false),
        Err(RegionError::MissingCell([-1, 16, 0]))
    ));
    assert_eq!(q.snapshot().worker_bytes, 0);
}

#[test]
fn decoded_source_and_encoder_share_the_original_native_quota_until_last_buffer_consumer() {
    let chunks = BTreeMap::from([
        ([-1, 0], decoded([-1, 0], "7")),
        ([0, 0], decoded([0, 0], "9")),
    ]);
    let q = quota(65536);
    let baseline = q.snapshot().worker_bytes;
    let volume = collect_saved_region(&chunks, &q, spec(), limits(), || false).unwrap();
    let source_bytes = q.snapshot().worker_bytes;
    assert!(source_bytes > baseline);
    let overlap = std::cell::Cell::new(false);
    let encoded = crate::world_region_encoding::encode_region(
        volume,
        &"b".repeat(64),
        4096,
        4096,
        || false,
        |bytes| {
            assert_eq!(q.snapshot().worker_bytes, source_bytes);
            let lease = q
                .reserve_external_storage(bytes)
                .map_err(|_| RegionError::Admission)?;
            overlap.set(q.snapshot().worker_bytes > source_bytes);
            Ok(lease)
        },
    )
    .unwrap_or_else(|error| panic!("decoded region encoding: {error:?}"));
    assert!(
        overlap.get(),
        "both original-account admissions must coexist while reading source"
    );
    assert!(q.snapshot().worker_bytes > baseline);
    assert_eq!(encoded.blocks(), &[0, 0, 1, 0]);
    let metadata: serde_json::Value = serde_json::from_slice(encoded.metadata()).unwrap();
    assert_eq!(metadata["value"]["origin"], serde_json::json!([-1, 0, 0]));
    assert_eq!(metadata["value"]["size"], serde_json::json!([2, 1, 1]));
    assert_eq!(metadata["value"]["palette"][0]["properties"]["level"], "7");
    assert_eq!(metadata["value"]["palette"][1]["properties"]["level"], "9");
    assert_eq!(metadata["value"]["identity"], "b".repeat(64));
    drop(chunks);
    assert_eq!(encoded.blocks(), &[0, 0, 1, 0]);
    assert!(q.snapshot().worker_bytes > baseline);
    drop(encoded);
    assert_eq!(q.snapshot().worker_bytes, baseline);
}

#[test]
fn original_native_stop_during_encoding_returns_no_body_and_releases_both_account_leases() {
    let chunks = BTreeMap::from([
        ([-1, 0], decoded([-1, 0], "7")),
        ([0, 0], decoded([0, 0], "9")),
    ]);
    let q = quota(65536);
    let stop = ilium_platform::owned_worker::StopToken::default();
    let volume = collect_saved_region(&chunks, &q, spec(), limits(), || stop.is_stopped()).unwrap();
    let mut observations = 0;
    let result = crate::world_region_encoding::encode_region(
        volume,
        &"b".repeat(64),
        4096,
        4096,
        || {
            observations += 1;
            if observations == 5 {
                stop.stop();
            }
            stop.is_stopped()
        },
        |bytes| {
            q.reserve_external_storage(bytes)
                .map_err(|_| RegionError::Admission)
        },
    );
    assert!(matches!(result, Err(RegionError::Cancelled)));
    assert!(stop.is_stopped());
    assert_eq!(q.snapshot().worker_bytes, 0);
    assert_eq!(chunks[&[-1, 0]].identity.position, [-1, 0]);
}

#[test]
fn complete_native_response_retains_only_original_output_charge_after_source_drop() {
    let chunks = BTreeMap::from([
        ([-1, 0], decoded([-1, 0], "7")),
        ([0, 0], decoded([0, 0], "9")),
    ]);
    let q = quota(65536);
    let stop = ilium_platform::owned_worker::StopToken::default();
    let output = encode_saved_region(&chunks, &q, &stop, spec(), limits(), &"b".repeat(64), 4096)
        .unwrap_or_else(|error| panic!("complete native response: {error:?}"));
    assert_eq!(output.blocks(), &[0, 0, 1, 0]);
    let metadata: serde_json::Value = serde_json::from_slice(output.metadata()).unwrap();
    assert_eq!(metadata["value"]["palette"][0]["properties"]["level"], "7");
    assert_eq!(metadata["value"]["palette"][1]["properties"]["level"], "9");
    assert_eq!(metadata["value"]["identity"], "b".repeat(64));
    drop(chunks);
    assert_eq!(output.blocks(), &[0, 0, 1, 0]);
    assert!(q.snapshot().worker_bytes > 0);
    drop(output);
    assert_eq!(q.snapshot().worker_bytes, 0);
}

#[test]
fn original_stop_refuses_complete_response_before_any_charge() {
    let chunks = BTreeMap::from([([-1, 0], decoded([-1, 0], "7"))]);
    let q = quota(65536);
    let stop = ilium_platform::owned_worker::StopToken::default();
    stop.stop();
    let output = encode_saved_region(&chunks, &q, &stop, spec(), limits(), &"b".repeat(64), 4096);
    assert!(matches!(output, Err(RegionError::Cancelled)));
    assert_eq!(q.snapshot().worker_bytes, 0);
}

#[test]
fn overlapping_original_account_refusal_returns_no_complete_response() {
    let chunks = BTreeMap::from([
        ([-1, 0], decoded([-1, 0], "7")),
        ([0, 0], decoded([0, 0], "9")),
    ]);
    // Projection fits independently; the distinct encoded body must coexist with it.
    let q = quota(5000);
    let projection = collect_saved_region(&chunks, &q, spec(), limits(), || false).unwrap();
    drop(projection);
    assert_eq!(q.snapshot().worker_bytes, 0);
    let stop = ilium_platform::owned_worker::StopToken::default();
    let output = encode_saved_region(&chunks, &q, &stop, spec(), limits(), &"b".repeat(64), 4096);
    assert!(matches!(output, Err(RegionError::Admission)));
    assert_eq!(q.snapshot().worker_bytes, 0);
}

#[test]
fn incomplete_original_source_refuses_response_instead_of_encoding_air() {
    let chunks = BTreeMap::from([([-1, 0], decoded([-1, 0], "7"))]);
    let q = quota(65536);
    let stop = ilium_platform::owned_worker::StopToken::default();
    let output = encode_saved_region(&chunks, &q, &stop, spec(), limits(), &"b".repeat(64), 4096);
    assert!(matches!(output, Err(RegionError::MissingCell([0, 0, 0]))));
    assert_eq!(q.snapshot().worker_bytes, 0);
}
