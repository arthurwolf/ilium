//! Actual native encoder-to-V8 completion; synthetic cells exercise wire shape.
//! This does not qualify native world-handle authorization or host dispatch.
use super::*;
use crate::world_region::{collect_region, BlockStateView, RegionLimits, RegionSpec};
use crate::world_region_encoding::encode_region;

fn instrumented_bootstrap() -> String {
    let bootstrap = crate::TRUSTED_BOOTSTRAP;
    let end = bootstrap.rfind("})();").unwrap();
    let mut source = String::with_capacity(bootstrap.len() + 128);
    source.push_str(&bootstrap[..end]);
    source.push_str("globalThis.__ilium_contract_region = world_region_result;\n");
    source.push_str(&bootstrap[end..]);
    source
}

#[test]
fn actual_native_encoded_region_reaches_v8_as_u16_and_preserves_palette_semantics() {
    let (_serial, quota) = super::inventory_contracts::fixture_lock();
    let properties = [
        BTreeMap::from([("level".to_owned(), "7".to_owned())]),
        BTreeMap::new(),
        BTreeMap::from([("level".to_owned(), "9".to_owned())]),
    ];
    let baseline_bytes = quota.snapshot().worker_bytes;
    let fixture_admission = quota.reserve_external_storage(4096).unwrap();
    let identity = "b".repeat(64);
    let spec = RegionSpec {
        origin: [-1, 0, -2],
        size: [2, 1, 2],
        max_bytes: 4096,
    };
    let region = collect_region(
        spec,
        RegionLimits {
            cells: 4,
            palette: 3,
            work: 65536,
        },
        || false,
        |bytes| {
            quota
                .reserve_external_storage(bytes)
                .map_err(|_| crate::world_region::RegionError::Admission)
        },
        |position| {
            let palette = match position {
                [-1, 0, -2] | [-1, 0, -1] => 0,
                [0, 0, -2] => 1,
                [0, 0, -1] => 2,
                _ => panic!("collector accessed a cell outside the requested fixture"),
            };
            Ok(Some(BlockStateView {
                name: if palette == 1 {
                    "minecraft:stone"
                } else {
                    "minecraft:water"
                },
                properties: &properties[palette],
            }))
        },
    )
    .unwrap();
    let encoded = encode_region(
        region,
        &identity,
        4096,
        65536,
        || false,
        |bytes| {
            quota
                .reserve_external_storage(bytes)
                .map_err(|_| crate::world_region::RegionError::Admission)
        },
    )
    .unwrap();
    assert_eq!(encoded.blocks(), [0, 0, 1, 0, 0, 0, 2, 0]);
    let before_refusal = quota.snapshot().worker_bytes;
    let foreign = QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 1,
        input_bytes: 4096,
        result_bytes: 4096,
        worker_threads: 1,
        worker_bytes: 65536,
    });
    let foreign_before = foreign.snapshot().worker_bytes;
    assert!(matches!(
        crate::native_world_region::copy_region_response(
            &encoded,
            &EngineLimits::default(),
            foreign.clone()
        ),
        Err(AnimationError::PermissionDenied(_))
    ));
    assert_eq!(foreign.snapshot().worker_bytes, foreign_before);
    for limits in [
        EngineLimits {
            json_bytes: 1,
            ..EngineLimits::default()
        },
        EngineLimits {
            backing_bytes: 1,
            ..EngineLimits::default()
        },
    ] {
        assert!(matches!(
            crate::native_world_region::copy_region_response(&encoded, &limits, quota.clone()),
            Err(AnimationError::Budget(_))
        ));
        assert_eq!(quota.snapshot().worker_bytes, before_refusal);
    }
    let exhausting = quota
        .reserve_external_storage(2048 * 1024 * 1024 - quota.snapshot().worker_bytes - 32)
        .unwrap();
    let exhausted = quota.snapshot().worker_bytes;
    assert!(matches!(
        crate::native_world_region::copy_region_response(
            &encoded,
            &EngineLimits::default(),
            quota.clone()
        ),
        Err(AnimationError::Budget(_))
    ));
    assert_eq!(quota.snapshot().worker_bytes, exhausted);
    drop(exhausting);
    assert_eq!(quota.snapshot().worker_bytes, before_refusal);
    let native_bytes = quota.snapshot().worker_bytes;
    let response = crate::native_world_region::copy_region_response(
        &encoded,
        &EngineLimits::default(),
        quota.clone(),
    )
    .unwrap();
    assert!(quota.snapshot().worker_bytes > native_bytes);
    assert_ne!(response.planes()["b0"].as_ptr(), encoded.blocks().as_ptr());
    drop(encoded);
    drop(properties);
    drop(fixture_admission);
    // The original native allocation is gone; the independently admitted
    // completion still owns exactly the bytes delivered to the guest below.
    assert!(quota.snapshot().worker_bytes > baseline_bytes);
    assert_eq!(response.planes()["b0"], [0, 0, 1, 0, 0, 0, 2, 0]);
    let source = r#"
export async function create() {
    const reply = await __ilium_dispatch('fixture.native_region', {});
    const region = __ilium_contract_region(reply.value, {identity:'b'.repeat(64)});
    globalThis.region_result = {
        typed: region.blocks instanceof Uint16Array,
        length: region.blocks.length,
        byteLength: region.blocks.byteLength,
        blocks: Array.from(region.blocks),
        origin: region.origin,
        size: region.size,
        identity: region.identity,
        palette: region.palette,
        frozen: [region, region.origin, region.size, region.palette,
            ...region.palette, ...region.palette.map(state => state.properties)].every(Object.isFrozen),
    };
    return {render(){}, dispose(){}};
}
"#;
    let mut native = super::boundary_tests::engine(
        source,
        &instrumented_bootstrap(),
        EngineLimits::default(),
        quota,
    );
    assert_eq!(
        native
            .start_create(&serde_json::json!({}), &serde_json::json!({}))
            .unwrap(),
        CreateState::Pending
    );
    let request = native.take_requests().unwrap().remove(0);
    assert_eq!(request.method, "fixture.native_region");
    assert_eq!(
        native
            .complete_service_request(request.id, request.authority, response)
            .unwrap(),
        CompletionState::Delivered
    );
    assert_eq!(native.pump().unwrap(), CreateState::Ready);
    let result = native.evaluate_json("region_result").unwrap();
    assert_eq!(result["typed"], true);
    assert_eq!(result["length"], 4);
    assert_eq!(result["byteLength"], 8);
    assert_eq!(result["blocks"], serde_json::json!([0, 1, 0, 2]));
    assert_eq!(result["origin"], serde_json::json!([-1, 0, -2]));
    assert_eq!(result["size"], serde_json::json!([2, 1, 2]));
    assert_eq!(result["identity"], identity);
    assert_eq!(result["palette"][0]["properties"]["level"], "7");
    assert_eq!(result["palette"][1]["name"], "minecraft:stone");
    assert_eq!(result["palette"][2]["properties"]["level"], "9");
    assert_eq!(result["frozen"], true);
    assert!(!native.is_invalid());
}

#[test]
fn original_generated_world_region_survives_source_close_and_reaches_v8() {
    use crate::native_worlds::{GeneratedWorldSettings, WorldService};
    use ilium_ambient::resources::AmbientResources;
    use ilium_execution::{ClientLimits, Execution, ExecutionConfig, LaneConfig};
    let (_serial, quota) = super::inventory_contracts::fixture_lock();
    let zero = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: LaneConfig {
                threads: 1,
                queue_slots: 1,
                priority: None,
                resident_bytes_per_thread: 1024,
            },
            io: zero,
            service: zero,
        },
    )
    .unwrap();
    let resources = AmbientResources::new(
        execution
            .client(ClientLimits {
                jobs: 2,
                service_jobs: 0,
                input_bytes: 1024,
                result_bytes: 1024,
            })
            .unwrap(),
    );
    let mut worlds = WorldService::new(resources, quota.clone(), 71).unwrap();
    let handle = worlds
        .insert_generated(GeneratedWorldSettings::default())
        .unwrap();
    let identity = worlds.world_identity(handle).unwrap().hex();
    let encoded = worlds
        .region(
            handle,
            RegionSpec {
                origin: [-1, 0, -2],
                size: [2, 1, 2],
                max_bytes: 4096,
            },
            RegionLimits {
                cells: 4,
                palette: 4,
                work: 65536,
            },
            65536,
            &ilium_platform::owned_worker::StopToken::default(),
        )
        .unwrap();
    let response = crate::native_world_region::copy_region_response(
        &encoded,
        &EngineLimits::default(),
        quota.clone(),
    )
    .unwrap();
    assert_eq!(
        response.metadata()["value"]["palette"][0]["name"],
        "ilium:generated/basalt"
    );
    worlds.close_world(handle).unwrap();
    drop(encoded);
    drop(worlds);
    let source = r#"
export async function create() {
    const reply = await __ilium_dispatch('fixture.original_generated_region', {});
    const region = __ilium_contract_region(reply.value, {identity:reply.value.identity});
    globalThis.region_result = {
        typed:region.blocks instanceof Uint16Array, blocks:Array.from(region.blocks),
        identity:region.identity, palette:region.palette, origin:region.origin,
        size:region.size, frozen:Object.isFrozen(region.palette[0].properties),
    };
    return {render(){}, dispose(){}};
}
"#;
    let mut engine = super::boundary_tests::engine(
        source,
        &instrumented_bootstrap(),
        EngineLimits::default(),
        quota,
    );
    assert_eq!(
        engine
            .start_create(&serde_json::json!({}), &serde_json::json!({}))
            .unwrap(),
        CreateState::Pending
    );
    let request = engine.take_requests().unwrap().remove(0);
    assert_eq!(request.method, "fixture.original_generated_region");
    assert_eq!(
        engine
            .complete_service_request(request.id, request.authority, response)
            .unwrap(),
        CompletionState::Delivered
    );
    assert_eq!(engine.pump().unwrap(), CreateState::Ready);
    let value = engine.evaluate_json("region_result").unwrap();
    assert_eq!(value["typed"], true);
    assert_eq!(value["blocks"], serde_json::json!([0, 0, 0, 0]));
    assert_eq!(value["identity"], identity);
    assert_eq!(value["palette"][0]["name"], "ilium:generated/basalt");
    assert_eq!(value["palette"][0]["properties"], serde_json::json!({}));
    assert_eq!(value["origin"], serde_json::json!([-1, 0, -2]));
    assert_eq!(value["size"], serde_json::json!([2, 1, 2]));
    assert_eq!(value["frozen"], true);
}
