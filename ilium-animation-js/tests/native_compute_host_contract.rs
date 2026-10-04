#![cfg(all(feature = "v8-runtime", feature = "native-host"))]
//! Real helper/finite-bank qualification; never a synthetic service response.
use ilium_ambient::resources::AmbientResources;
use ilium_animation_js::{
    engine::{ArraySpec, CreateState, TypedArrayKind},
    helper::HelperLimits,
    manifest::AnimationMode,
    native_compute_host::NativeComputeHost,
    native_draw::DrawLimits,
    native_draw_host::NativeDrawHost,
    native_media::MediaLimits,
    permissions::Ceiling,
    runtime::{InstancePreparation, PackageInstance},
    surface::{ColourSpace, Data, Format, FrameMeta, Mode, Planes, Shape, Surface, Update},
    trust::TrustVerifier,
};
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
};
use ilium_platform::owned_worker::StopToken;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
fn archive(source: &str, id: &str) -> Vec<u8> {
    let manifest = json!({"api_version":1,"id":id,"name":"Native compute integration fixture","version":"1.0.0","entry":"entry.mjs","modes":["live"],"settings":{"type":"object","properties":{}},"files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}]});
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("entry.mjs", options).unwrap();
    archive.write_all(source.as_bytes()).unwrap();
    archive.start_file("manifest.json", options).unwrap();
    archive
        .write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    archive.finish().unwrap().into_inner()
}
fn instance(bytes: &[u8], quota: QuotaGroup, id: u64) -> (PackageInstance, CreateState) {
    let verifier = TrustVerifier::from_release_inventory(Vec::new()).unwrap();
    let helper = PathBuf::from(
        std::env::var_os("ILIUM_ANIMATION_HELPER")
            .expect("qualification needs the exact newly built helper"),
    );
    let settings = json!({});
    let environment = json!({"cell_width":1,"cell_height":1,"dot_width":2,"dot_height":4});
    let verified = PackageInstance::verify(InstancePreparation {
        archive: bytes,
        verifier: &verifier,
        helper_executable: &helper,
        trusted_bootstrap: ilium_animation_js::TRUSTED_BOOTSTRAP,
        settings: &settings,
        mode: AnimationMode::Live,
        environment: &environment,
        host_policy: Ceiling {
            permissions: Vec::new(),
        },
        instance_id: id,
        limits: HelperLimits::default(),
        quota,
    })
    .unwrap();
    let (mut instance, review) = verified.prepare_without_rights().unwrap();
    assert!(review.items().is_empty());
    let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
    assert!(pending.remembered_bytes().is_none());
    assert!(pending.persistence_error().is_none());
    let resolution = instance.finish_resolution(pending).unwrap();
    assert!(resolution.denied_required.is_empty());
    assert!(resolution.authority_error.is_none());
    assert!(resolution.creation_error.is_none());
    (instance, resolution.creation.unwrap())
}
fn resources() -> (Execution, AmbientResources, QuotaGroup, mpsc::Receiver<()>) {
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 8,
        jobs: 16,
        service_jobs: 0,
        input_bytes: 32 * 1024 * 1024,
        result_bytes: 32 * 1024 * 1024,
        worker_threads: 64,
        worker_bytes: 2 * 1024 * 1024 * 1024,
    });
    let disabled = LaneConfig {
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
                queue_slots: 4,
                priority: None,
                resident_bytes_per_thread: 1024 * 1024,
            },
            io: disabled.clone(),
            service: disabled,
        },
    )
    .unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let client = execution
        .client(ClientLimits {
            jobs: 8,
            service_jobs: 0,
            input_bytes: 16 * 1024 * 1024,
            result_bytes: 16 * 1024 * 1024,
        })
        .unwrap()
        .with_completion_wake(move || {
            let _ = sender.try_send(());
        });
    (execution, AmbientResources::new(client), quota, receiver)
}
#[test]
#[ignore = "requires exact built helper, delegated sandbox and actual V8/native finite bank"]
fn actual_helper_async_fft_result_close_and_guarded_surface_frame() {
    let source = r#"export function plan(){return {output:{mode:'pixels',format:'gray32',update:'replace'},fps:30,inputs:{},permissions:[]}}
export async function create(host){
 const opened=await host.compute.submit({kernel:'fft',input:new Float32Array([1,0,0,0]),parameters:{window:0,complex:0,inverse:0},max_bytes:32,timeout_ms:2000});
 if(!opened.ok)throw Error('native_submit_failed');
 const result=await opened.value.result();
 if(!result.ok||!(result.value instanceof Float32Array)||result.value.length!==8)throw Error('native_result_failed');
 const expected=[1,0,1,0,1,0,1,0];for(let i=0;i<8;i++)if(Math.abs(result.value[i]-expected[i])>1e-6)throw Error('native_fft_bits');
 opened.value.close();
 return {render(context,frame){frame.gray.fill(result.value[0]);frame.present()},dispose(){}};
}"#;
    let bytes = archive(source, "native-compute-host-contract");
    let (mut execution, resources, quota, wake) = resources();
    let _fixture = quota.reserve_external_storage(bytes.len() + 65536).unwrap();
    let (mut instance, mut creation) = instance(&bytes, quota.clone(), 31);
    assert_eq!(creation, CreateState::Pending);
    let mut host = NativeComputeHost::new(
        resources.clone(),
        quota.clone(),
        instance.engine_limits().clone(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut observed_wake = false;
    for _ in 0..8 {
        let requests = instance.requests().unwrap();
        if requests.is_empty() {
            if creation == CreateState::Ready {
                break;
            }
            wake.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("actual finite completion wake");
            observed_wake = true;
            host.on_completion_wake(&mut instance).unwrap();
        } else {
            for request in requests {
                assert!(host.dispatch(&mut instance, request).unwrap().is_none());
            }
        }
        creation = instance.pump().unwrap();
    }
    assert!(
        observed_wake,
        "native result must come from the real finite receipt"
    );
    assert_eq!(creation, CreateState::Ready);
    assert!(instance.requests().unwrap().is_empty());
    assert_eq!(
        host.snapshots(&mut instance).unwrap().metadata(),
        &json!([]),
        "actual closed handle was terminally forgotten"
    );
    let shape = Shape {
        cell_width: 1,
        cell_height: 1,
        mode: Mode::Pixels,
        format: Format::Gray32,
        update: Update::Replace,
        cell_rgb: false,
        colour_space: ColourSpace::Srgb,
    };
    let layout = shape.layout().unwrap();
    let _surface = quota
        .reserve_external_storage(
            layout.canonical_bytes * 2 + layout.handoff_bytes * 2 + layout.dots * 64 + 65536,
        )
        .unwrap();
    let mut surface = Surface::new(31, 7, shape).unwrap();
    let seed = surface.begin(1).unwrap();
    let work = match seed.data {
        Data::F32(values) => values
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect::<Vec<_>>(),
        _ => panic!("fixture gray32 shape"),
    };
    let seed_specs = [ArraySpec {
        name: "work_data".into(),
        kind: TypedArrayKind::F32,
        elements: layout.elements,
    }];
    instance.seed_frame(&json!({"frame":{"key":seed.key,"shape":shape,"reset":seed.reset,"invalid_rects":seed.invalid_rects,"input_specs":[]},"services":[]}),&seed_specs,&BTreeMap::from([("work_data".into(),work)])).unwrap();
    let specs = [
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
        elements: layout.samples,
    })
    .collect::<Vec<_>>();
    let retained = instance
        .render(
            &json!({"time":0,"wall":0,"delta":0.025,"wall_delta":0.025,"inputs":{},"_ilium_frame":{"key":seed.key,"shape":shape}}),
            &specs,
        )
        .unwrap();
    let (mut output, _retained) = retained.into_parts();
    let metadata = FrameMeta::parse(&serde_json::to_vec(&output.metadata).unwrap()).unwrap();
    let data = output
        .planes
        .remove("data")
        .unwrap()
        .chunks_exact(4)
        .map(|bytes| f32::from_ne_bytes(bytes.try_into().unwrap()))
        .collect();
    let order = output
        .planes
        .remove("order")
        .unwrap()
        .chunks_exact(4)
        .map(|bytes| u32::from_ne_bytes(bytes.try_into().unwrap()))
        .collect();
    let planes = Planes {
        data: Data::F32(data),
        touch: output.planes.remove("touch").unwrap(),
        order,
        cell_rgb: None,
        colour_touch: None,
        colour_order: None,
    };
    let mut drawing =
        NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default()).unwrap();
    let outcome = drawing
        .finish(
            &mut instance,
            &mut surface,
            metadata,
            planes,
            &StopToken::default(),
            |snapshot, _| {
                match snapshot.data() {
                    Data::F32(values) => {
                        assert!(values.iter().all(|value| (*value - 1.).abs() < 1e-6))
                    }
                    _ => panic!("fixture gray32 output"),
                };
                Ok(())
            },
        )
        .unwrap();
    assert!(outcome.accepted);
    instance.accept_frame(true).unwrap();
    host.revoke();
    assert!(
        host.is_drained(),
        "original FFT receipt and genuine closed handle already settled"
    );
    let stopped = instance.stop();
    assert!(stopped.authority_error.is_none());
    assert!(stopped.cancellation.is_ok());
    drop(drawing);
    drop(host);
    drop(instance);
    drop(resources);
    execution.request_shutdown(ShutdownMode::Drain);
    let report = execution
        .join_until_background(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(
        report.remaining_workers, 0,
        "actual finite workers must exit"
    );
}
#[test]
#[ignore = "requires exact built helper, delegated sandbox and actual V8"]
fn arbitrary_guest_wire_id_cannot_reconstruct_a_native_compute_handle() {
    let source = r#"export function plan(){return {output:{mode:'pixels',format:'gray32',update:'replace'},fps:30,inputs:{},permissions:[]}}export async function create(){await __ilium_dispatch('compute.result',{id:'1',kind:'compute'});return {render(){},dispose(){}}}"#;
    let bytes = archive(source, "native-compute-foreign-id");
    let (mut execution, resources, quota, _wake) = resources();
    let (mut instance, creation) = instance(&bytes, quota.clone(), 32);
    assert_eq!(creation, CreateState::Pending);
    let mut host =
        NativeComputeHost::new(resources.clone(), quota, instance.engine_limits().clone()).unwrap();
    let mut requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    let error = match host.dispatch(&mut instance, requests.pop().unwrap()) {
        Err(error) => error,
        Ok(_) => panic!("foreign guest id must not reconstruct a native handle"),
    };
    assert!(error.to_string().contains("unknown original handle"));
    assert_eq!(
        host.snapshots(&mut instance).unwrap().metadata(),
        &json!([])
    );
    host.revoke();
    let stopped = instance.stop();
    assert!(stopped.authority_error.is_none());
    assert!(stopped.cancellation.is_ok());
    drop(host);
    drop(instance);
    drop(resources);
    execution.request_shutdown(ShutdownMode::Drain);
    let report = execution
        .join_until_background(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(
        report.remaining_workers, 0,
        "actual finite workers must exit"
    );
}
