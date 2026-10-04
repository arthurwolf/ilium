#![cfg(feature = "v8-runtime")]
//! Pure binary preflight plus actual, explicitly qualified private helper IPC.
use ilium_animation_js::{
    engine::{ArraySpec, ServiceAuthority, TypedArrayKind}, // Bind the real native activation explicitly before helper seed or creation.
    helper::validate_seed_planes,
};
use std::collections::BTreeMap;
#[test]
fn binary_seed_preflight_accepts_all_four_kinds_and_48_planes_without_raising_byte_caps() {
    let kinds = [
        TypedArrayKind::U8,
        TypedArrayKind::F32,
        TypedArrayKind::U16,
        TypedArrayKind::U32,
    ];
    let mut arrays = Vec::new();
    let mut planes = BTreeMap::new();
    let mut expected = 0;
    for index in 0..48 {
        let kind = kinds[index % 4];
        let width = match kind {
            TypedArrayKind::U8 => 1,
            TypedArrayKind::U16 => 2,
            _ => 4,
        };
        let name = format!("input_{index}");
        arrays.push(ArraySpec {
            name: name.clone(),
            kind,
            elements: 3,
        });
        planes.insert(name, vec![0; 3 * width]);
        expected += 3 * width;
    }
    assert_eq!(
        validate_seed_planes(&arrays, &planes, expected).unwrap(),
        expected
    );
    assert!(validate_seed_planes(&arrays, &planes, expected - 1).is_err());
    arrays.push(ArraySpec {
        name: "input_48".into(),
        kind: TypedArrayKind::U8,
        elements: 1,
    });
    planes.insert("input_48".into(), vec![0]);
    assert!(validate_seed_planes(&arrays, &planes, 8 * 1024 * 1024).is_err());
}
#[test]
fn malformed_seed_names_shape_duplicate_overflow_and_hidden_planes_fail_closed() {
    let arrays = [ArraySpec {
        name: "work_data".into(),
        kind: TypedArrayKind::F32,
        elements: 4,
    }];
    let mut planes = BTreeMap::from([("work_data".into(), vec![0; 16])]);
    assert_eq!(validate_seed_planes(&arrays, &planes, 16).unwrap(), 16);
    assert!(validate_seed_planes(&arrays, &planes, 15).is_err());
    assert!(validate_seed_planes(&[arrays[0].clone(), arrays[0].clone()], &planes, 32).is_err());
    planes.insert("extra".into(), vec![]);
    assert!(validate_seed_planes(&arrays, &planes, 32).is_err());
    planes.remove("extra");
    planes.get_mut("work_data").unwrap().pop();
    assert!(validate_seed_planes(&arrays, &planes, 32).is_err());
    for name in ["", "../data", "data.dot", "data-0"] {
        assert!(validate_seed_planes(
            &[ArraySpec {
                name: name.into(),
                kind: TypedArrayKind::U8,
                elements: 0
            }],
            &BTreeMap::from([(name.into(), vec![])]),
            32
        )
        .is_err());
    }
    assert!(validate_seed_planes(
        &[ArraySpec {
            name: "work_data".into(),
            kind: TypedArrayKind::U32,
            elements: usize::MAX
        }],
        &BTreeMap::from([("work_data".into(), vec![])]),
        usize::MAX
    )
    .is_err());
    assert_eq!(validate_seed_planes(&[], &BTreeMap::new(), 0).unwrap(), 0);
}

#[test]
#[ignore = "requires built helper, delegated Linux sandbox and actual V8"]
fn actual_helper_binary_seed_roundtrip_detaches_and_reseeds_without_json_pixels() {
    use ilium_animation_js::{
        helper::{HelperAuthority, HelperLimits, HelperSession},
        manifest::AnimationMode,
        package::{Package, PackageLimits},
    };
    use ilium_execution::{QuotaGroup, QuotaLimits};
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::{
        io::{Cursor, Write},
        path::PathBuf,
    };
    let source = "export function plan(){return {output:{mode:'pixels',format:'gray32',update:'replace'},fps:30,inputs:{},permissions:[]}}; let old;export async function create(host){return {render(context,frame){host.status.log('info','seeded render');if(old&&old.byteLength!==0)throw Error('old_not_detached');old=frame.gray;frame.gray[0]=0.75;frame.present()},dispose(){}}}";
    let manifest = json!({"api_version":1,"id":"helper-seed-contract","name":"Helper seed contract","version":"1.0.0","entry":"entry.mjs","modes":["live"],"settings":{"type":"object","properties":{}},"files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}]});
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("entry.mjs", options).unwrap();
    archive.write_all(source.as_bytes()).unwrap();
    archive.start_file("manifest.json", options).unwrap();
    archive
        .write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    let archive = archive.finish().unwrap().into_inner();
    let package = Package::from_bytes(&archive, PackageLimits::default()).unwrap();
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 32,
        worker_bytes: 1024 * 1024 * 1024,
    });
    let executable = PathBuf::from(
        std::env::var_os("ILIUM_ANIMATION_HELPER")
            .expect("qualification needs ILIUM_ANIMATION_HELPER"),
    );
    let mut helper = HelperSession::launch(
        &executable,
        &archive,
        include_str!("../src/bootstrap.js"),
        HelperAuthority {
            package_digest: package.digest().into(),
            instance_id: 1,
            plan_generation: 1,
            authorization_epoch: 1,
        },
        HelperLimits::default(),
        quota.clone(),
    )
    .unwrap();
    helper
        .bind_service_authority(ServiceAuthority {
            instance_id: 1,
            plan_generation: 1,
            authorization_epoch: 1,
        })
        .unwrap(); // Immutable launch identity is insufficient for seed/create authority.
    let plan = helper
        .plan(&json!({}), AnimationMode::Live, &json!({}))
        .unwrap();
    let _ = helper.start_create(&json!({}), &plan).unwrap();
    let shape = json!({"cell_width":2,"cell_height":1,"mode":"pixels","format":"gray32","update":"replace","cell_rgb":false,"colour_space":"srgb"});
    let seed_specs = [ArraySpec {
        name: "work_data".into(),
        kind: TypedArrayKind::F32,
        elements: 16,
    }];
    let seed_planes = BTreeMap::from([("work_data".into(), vec![0; 64])]);
    let output_specs = [
        ("work_data", TypedArrayKind::F32),
        ("data", TypedArrayKind::F32),
        ("work_touch", TypedArrayKind::U8),
        ("touch", TypedArrayKind::U8),
        ("work_order", TypedArrayKind::U32),
        ("order", TypedArrayKind::U32),
    ]
    .into_iter()
    .map(|(name, kind)| ArraySpec {
        name: name.into(),
        kind,
        elements: 16,
    })
    .collect::<Vec<_>>();
    for sequence in 1..=2 {
        let key = json!({"instance_id":"1","revision":"1","base_version":"0","sequence":sequence.to_string()});
        helper.seed_frame(&json!({"frame":{"key":key,"shape":shape,"reset":true,"invalid_rects":[],"input_specs":[]}}),
            &seed_specs,&seed_planes).unwrap();
        let frame = helper.render(&json!({"time":0,"wall":0,"delta":0.025,"inputs":{},"_ilium_frame":{"key":key,"shape":shape}}),&output_specs).unwrap();
        assert_eq!(frame.metadata["presented"], true);
        assert_eq!(frame.metadata["error"], serde_json::Value::Null);
        assert_eq!(
            f32::from_ne_bytes(frame.planes["data"][..4].try_into().unwrap()),
            0.75
        );
        assert!(frame.planes["touch"].iter().all(|value| *value == 1));
        assert!(helper.take_requests().is_empty());
        let status = helper.take_status().unwrap();
        assert_eq!(status.value["records"][0]["message"], "seeded render");
        assert!(frame.metadata.get("status").is_none());
        helper.accept_frame(true).unwrap();
    }
    helper.dispose().unwrap();
}

fn actual_service_helper(
    source: &str,
    stamp: ilium_animation_js::engine::ServiceAuthority,
    quota: ilium_execution::QuotaGroup,
) -> ilium_animation_js::helper::HelperSession {
    // Use real helper IPC; no dispatcher mock.
    use ilium_animation_js::{
        helper::{HelperAuthority, HelperLimits, HelperSession},
        package::{Package, PackageLimits},
    }; // Use supplied helper/package owners.
    use serde_json::json; // Keep archive metadata informational.
    use sha2::{Digest, Sha256}; // Bind exact fixture module bytes.
    use std::{
        io::{Cursor, Write},
        path::PathBuf,
    }; // Use the actual built helper path.
    let manifest = json!({"api_version":1,"id":"helper-service-contract","name":"Helper service contract","version":"1.0.0","entry":"entry.mjs","modes":["live"],"settings":{"type":"object","properties":{}},"files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}]}); // Preserve validated source bytes.
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new())); // Build a complete fixture archive.
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored); // Avoid external fixture files.
    archive.start_file("entry.mjs", options).unwrap(); // Store the declared entry module.
    archive.write_all(source.as_bytes()).unwrap(); // Retain source without host evaluation.
    archive.start_file("manifest.json", options).unwrap(); // Store the required manifest.
    archive
        .write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap(); // Preserve the frozen manifest ABI.
    let bytes = archive.finish().unwrap().into_inner(); // Keep actual archive bytes through launch.
    let package = Package::from_bytes(&bytes, PackageLimits::default()).unwrap(); // Use actual package validation/digest.
    let executable = PathBuf::from(
        std::env::var_os("ILIUM_ANIMATION_HELPER")
            .expect("qualification needs ILIUM_ANIMATION_HELPER"),
    ); // Require real native prerequisites.
    let bootstrap = "globalThis.__ilium_host=Object.freeze({http:{request:payload=>__ilium_dispatch('http.request',payload)}});globalThis.__ilium_make_frame=()=>({gray:new Float32Array(4),present(){this.submitted=true;}});globalThis.__ilium_finish_frame=frame=>({metadata:{submitted:!!frame.submitted},planes:{gray:frame.gray}});globalThis.__ilium_accept_frame=()=>{};"; // Transport fixture only; no native HTTP.
    let mut helper = HelperSession::launch(
        &executable,
        &bytes,
        bootstrap,
        HelperAuthority {
            package_digest: package.digest().into(),
            instance_id: stamp.instance_id,
            plan_generation: 1,
            authorization_epoch: 1,
        },
        HelperLimits::default(),
        quota,
    )
    .unwrap(); // Keep session and active stamps distinct.
    helper.bind_service_authority(stamp).unwrap(); // Bind before seed/create.
    assert_eq!(helper.authority().plan_generation, 1); // Preserve immutable session revision.
    assert_eq!(helper.authority().authorization_epoch, 1); // Preserve immutable session epoch.
    helper // Return the actual process owner.
} // Requires parent-run isolated helper.
fn helper_service_quota() -> ilium_execution::QuotaGroup {
    // Preserve original qualification limits.
    ilium_execution::QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 32,
        worker_bytes: 1024 * 1024 * 1024,
    }) // Create no replacement execution bank.
} // Own helper/copies on one finite root.
#[test] // Qualify real V8 and binary IPC.
#[ignore = "requires built helper, delegated Linux sandbox and actual V8"] // Require actual native qualification.
fn actual_helper_binary_requests_completion_ack_and_cancel_wait_for_explicit_pump() {
    // Exercise typed IPC and explicit pumps.
    use ilium_animation_js::engine::{
        CompletionState, CreateState, EngineLimits, ServiceAuthority, ServicePhase, ServiceValue,
    }; // Use actual native payload owners.
    use serde_json::json; // Keep markers separate from bytes.
    let stamp = ServiceAuthority {
        instance_id: 83,
        plan_generation: 4,
        authorization_epoch: 7,
    }; // Fixture stamps confer no permission.
    let quota = helper_service_quota(); // Keep the original quota root.
    let source = "export function plan(){return {}};export async function create(host){const u8=new Uint8Array([9,0,255,9]).subarray(1,3),f32=new Float32Array([9,-0,0.75,9]).subarray(1,3),u16=new Uint16Array([9,0x1234,0xfedc,9]).subarray(1,3),u32=new Uint32Array([9,16777217,4294967295,9]).subarray(1,3);const first=host.http.request({views:{u8,f32,u16,u32}}),second=host.http.request({views:{u8,f32,u16,u32}});u8.fill(7);f32.fill(7);u16.fill(7);u32.fill(7);const copied=await second;if(!copied.ok||!(copied.value.u8 instanceof Uint8Array)||copied.value.u8[1]!==255||!(copied.value.f32 instanceof Float32Array)||!Object.is(copied.value.f32[0],-0)||copied.value.f32[1]!==0.75||!(copied.value.u16 instanceof Uint16Array)||copied.value.u16[1]!==0xfedc||!(copied.value.u32 instanceof Uint32Array)||copied.value.u32[0]!==16777217||copied.value.u32[1]!==4294967295)throw Error('typed_completion');const third=host.http.request({after_copy:copied.value.u8});const copied_first=await first;if(!copied_first.ok)throw Error('first_completion');const cancelled=await third;if(cancelled.ok||cancelled.error.code!=='cancelled')throw Error('structured_cancel');const last=await host.http.request({after_cancel:true});if(last.ok||last.error.code!=='fixture_denied')throw Error('structured_result');let old;return {render(context,frame){if(old&&old.byteLength!==0)throw Error('old_frame_not_detached');if(copied.value.f32.byteLength!==8||copied.value.u8.byteLength!==2||copied.value.f32[1]!==0.75)throw Error('service_copy_detached');old=frame.gray;frame.gray.fill(copied.value.f32[1]);frame.present();},dispose(){}}}"; // Verify typed copies and structured errors.
    let mut helper = actual_service_helper(source, stamp, quota.clone()); // Launch/bind the actual helper.
    assert_eq!(
        helper
            .start_create(
                &json!({}),
                &json!({"generation":999,"authorization_epoch":999})
            )
            .unwrap(),
        CreateState::Pending
    ); // Ignore JSON as native authority.
    let requests = helper.take_requests(); // Drain two actual request pages.
    assert_eq!(requests.len(), 2); // Allow canonical-name reuse.
    assert!(requests[0].id < requests[1].id); // Preserve monotonic request IDs.
    let expected = BTreeMap::from([
        ("b0".into(), vec![0, 255]),
        (
            "b1".into(),
            [(-0.0_f32).to_ne_bytes(), 0.75_f32.to_ne_bytes()].concat(),
        ),
        (
            "b2".into(),
            [0x1234_u16.to_ne_bytes(), 0xfedc_u16.to_ne_bytes()].concat(),
        ),
        (
            "b3".into(),
            [16_777_217_u32.to_ne_bytes(), u32::MAX.to_ne_bytes()].concat(),
        ),
    ]); // Expect logical pre-mutation subviews.
    for request in &requests {
        // Check each native request.
        assert_eq!(request.authority, stamp); // Retain the bound active stamp.
        assert_eq!(request.phase, ServicePhase::Create); // Use native acquisition phase.
        assert_eq!(request.payload.planes(), &expected); // Snapshot before guest mutation.
        assert_eq!(request.payload.arrays().len(), 4); // Preserve four types across IPC.
        assert!(request.remaining_ms() <= EngineLimits::default().preparation_ms); // Never restart request deadlines.
        assert_eq!(
            request.payload.metadata()["views"]["u8"],
            json!({"$ilium_binary":"b0"})
        ); // Use only canonical byte markers.
    } // JSON grants no native authority.
    let arrays = requests[1].payload.arrays().to_vec(); // Reuse verified typed descriptors.
    let metadata = json!({"ok":true,"value":requests[1].payload.metadata()["views"]}); // Keep Result metadata separate.
    let result = ServiceValue::copy_from_host(
        &metadata,
        &arrays,
        &expected,
        &EngineLimits::default(),
        quota.clone(),
    )
    .unwrap(); // Admit a distinct original-root copy.
    let stale = ServiceAuthority {
        authorization_epoch: stamp.authorization_epoch + 1,
        ..stamp
    }; // Change only mutable active epoch.
    assert!(helper
        .complete_service_request(requests[1].id, stale, result.clone())
        .is_err()); // Refuse stale completion authority.
    let foreign = ServiceValue::copy_from_host(
        &metadata,
        &arrays,
        &expected,
        &EngineLimits::default(),
        helper_service_quota(),
    )
    .unwrap(); // Admit a deliberately foreign value.
    assert!(helper
        .complete_service_request(requests[1].id, stamp, foreign)
        .is_err()); // Reject foreign quota custody.
    assert_eq!(
        helper
            .complete_service_request(requests[1].id, stamp, result.clone())
            .unwrap(),
        CompletionState::Delivered
    ); // Copy and acknowledge without JS.
    assert!(helper.take_requests().is_empty()); // Do not run completion reactions.
    assert_eq!(
        helper
            .complete_service_request(requests[0].id, stamp, result.clone())
            .unwrap(),
        CompletionState::Delivered
    ); // Hidden checkpoints would leave undrained work.
    assert!(helper.take_requests().is_empty()); // Keep the second ACK checkpoint-free.
    assert_eq!(helper.pump().unwrap(), CreateState::Pending); // Run reactions only on explicit pump.
    let third = helper.take_requests(); // Drain the continuation's request.
    assert_eq!(third.len(), 1); // Require exactly one follow-up.
    assert!(third[0].id > requests[1].id); // Never recycle native request IDs.
    assert_eq!(third[0].payload.planes()["b0"], vec![0, 255]); // Permit later copies of service buffers.
    let cancelled_alias = third[0].clone(); // Retain a native cancellation alias.
    assert_eq!(
        helper.cancel_request(third[0].id, stamp).unwrap(),
        CompletionState::Cancelled
    ); // Require the actual cancellation ACK.
    assert!(cancelled_alias.is_cancelled()); // Share the native stop token.
    assert!(helper.take_requests().is_empty()); // Do not run cancellation reactions.
    let notifications = helper.take_cancelled_requests(); // Retain terminal notification ownership.
    assert_eq!(notifications.len(), 1); // Notify the issued request once.
    assert_eq!(notifications[0].id, third[0].id); // Preserve exact terminal correlation.
    assert!(!helper.is_physically_retired()); // Notification never proves helper exit.
    assert_eq!(helper.pump().unwrap(), CreateState::Pending); // Pump the structured cancellation Result.
    let last = helper.take_requests(); // Hidden cancel checkpoints would fail here.
    assert_eq!(last.len(), 1); // Require one after-cancel request.
    assert_eq!(last[0].payload.metadata(), &json!({"after_cancel":true})); // Confirm the expected continuation ran.
    let denied = ServiceValue::copy_from_host(&json!({"ok":false,"error":{"code":"fixture_denied","message":"native transport fixture refusal"}}), &[], &BTreeMap::new(), &EngineLimits::default(), quota.clone()).unwrap(); // Fixture error Result; no service success.
    assert_eq!(
        helper
            .complete_service_request(last[0].id, stamp, denied)
            .unwrap(),
        CompletionState::Delivered
    ); // Distinguish delivery from service success.
    assert!(helper.take_requests().is_empty()); // Keep error copying checkpoint-free.
    assert_eq!(helper.pump().unwrap(), CreateState::Ready); // Verify the error on explicit pump.
    for _ in 0..2 {
        // Keep service buffers across frames.
        let frame = helper
            .render(
                &json!({}),
                &[ArraySpec {
                    name: "gray".into(),
                    kind: TypedArrayKind::F32,
                    elements: 4,
                }],
            )
            .unwrap(); // Use real render copying/detachment.
        assert_eq!(frame.metadata, json!({"submitted":true})); // Require all package checks to pass.
        assert_eq!(frame.planes["gray"], 0.75_f32.to_ne_bytes().repeat(4)); // Preserve typed result pixels.
        helper.accept_frame(true).unwrap(); // Separate frame ACK from service ACK.
    } // Drop each actual frame owner.
    helper.dispose().unwrap(); // Require real shutdown and worker joins.
    assert!(helper.is_physically_retired()); // Observe physical retirement proof.
    drop(helper); // Drop the proven-retired helper.
    assert_eq!(quota.snapshot().worker_threads, 0); // Release physically retired workers.
    assert!(quota.snapshot().worker_bytes > 0); // Retain surviving payload admissions.
    drop((
        requests,
        third,
        last,
        notifications,
        cancelled_alias,
        result,
    )); // Drop every actual admitted alias.
    assert_eq!(quota.snapshot().worker_bytes, 0); // Release final admitted payload custody.
} // No native HTTP/grant qualification claimed.
#[test] // Separate helper and activation lifetimes.
#[ignore = "requires built helper, delegated Linux sandbox and actual V8"] // Require real physical retirement.
fn actual_helper_retirement_preserves_native_activation_until_explicit_broker_retire() {
    // Retain native authority independently.
    use ilium_animation_js::{
        engine::{CreateState, ServiceAuthority},
        permissions::{CallPhase, Ceiling, PackageIdentity, PermissionBroker, PermissionPlan},
    }; // Use actual broker and helper APIs.
    use serde_json::json; // Keep JSON outside authority.
    let identity = PackageIdentity::unverified(
        "helper-retirement-contract".into(),
        b"native broker fixture identity",
    )
    .unwrap(); // Isolated broker identity; no package integration.
    let mut broker = PermissionBroker::new(
        identity,
        Ceiling {
            permissions: vec![],
        },
        Ceiling {
            permissions: vec![],
        },
    )
    .unwrap(); // Grant no privileged services.
    let review = broker
        .prepare(
            97,
            3,
            PermissionPlan {
                permissions: vec![],
                demands: vec![],
            },
            BTreeMap::new(),
        )
        .unwrap(); // Prepare an acquisition-free review.
    let activation = broker
        .resolve(review, BTreeMap::new())
        .unwrap()
        .activation
        .unwrap(); // Activate without inventing permission.
    let stamp = ServiceAuthority {
        instance_id: activation.plan.instance_id,
        plan_generation: activation.plan.plan_revision,
        authorization_epoch: activation.plan.authorization_epoch,
    }; // Derive stamp from private activation.
    let quota = helper_service_quota(); // Keep the original helper root.
    let mut helper = actual_service_helper("export function plan(){return {}};export async function create(){return {render(context,frame){frame.present()},dispose(){}}}", stamp, quota.clone()); // Run an actual baseline-only helper.
    assert_eq!(
        helper.start_create(&json!({}), &json!({})).unwrap(),
        CreateState::Ready
    ); // Finish actual package preparation.
    assert!(broker
        .grant(&activation.channel, "_host_channel_check")
        .unwrap()
        .is_none()); // Require a current private channel.
    assert!(broker
        .dispatch(&activation.channel, CallPhase::Async, "cpu", vec![])
        .is_err()); // Refuse fake empty CPU needs.
    assert!(!helper.is_physically_retired()); // Observe the physically live helper.
    helper.cancel().unwrap(); // Execute actual shutdown and joins.
    assert!(helper.is_physically_retired()); // Require complete terminal evidence.
    helper.cancel().unwrap(); // Repeat only proved retirement.
    assert!(helper.pump().is_err()); // Forbid resumed package execution.
    assert!(broker
        .grant(&activation.channel, "_host_channel_check")
        .unwrap()
        .is_none()); // Keep native activation current.
    drop(helper); // Drop the proved-dead process owner.
    assert_eq!(quota.snapshot().worker_threads, 0); // Release actual worker admissions.
    assert_eq!(quota.snapshot().worker_bytes, 0); // No retained payload owners remain.
    assert!(broker
        .grant(&activation.channel, "_host_channel_check")
        .is_ok()); // Keep authority after helper destruction.
    let _invalidation = broker.retire(activation.plan.instance_id); // Revoke native activation explicitly.
    assert!(broker
        .grant(&activation.channel, "_host_channel_check")
        .is_err()); // Reject the now-stale private channel.
} // PackageInstance/failed-join gates remain native.
