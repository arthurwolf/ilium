//! Explicitly selected actual-helper qualification. Synthetic provider bytes are
//! local fixture data; helper, broker, original CPU/IO receipts and ACK are real.
use super::*;
use crate::{
    engine::{ArraySpec, CompletionState, CreateState, TypedArrayKind},
    helper::HelperLimits,
    manifest::AnimationMode,
    native_draw::DrawLimits,
    permissions::{Capability, Ceiling, HttpMethod, PermissionBroker, Right, Scope, UserChoice},
    runtime::InstancePreparation,
    surface::{Data, Format, Mode, Shape, Surface, Update},
    trust::TrustVerifier,
    TRUSTED_BOOTSTRAP,
};
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaLimits, ShutdownMode,
};
use std::{
    collections::{BTreeSet, VecDeque},
    io::{Cursor, Write},
    path::PathBuf,
    sync::mpsc,
};

const USGS_URL: &str = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_day.geojson";
const USGS_BODY: &[u8] = br#"{"type":"FeatureCollection","features":[]}"#;
const OSM_ORIGIN: &str = "https://tile.openstreetmap.org";
const OSM_URL: &str = "https://tile.openstreetmap.org/0/0/0.png";
const OPEN_SCRIPT: &str = r#"
export function plan(){return {output:{mode:'pixels',format:'gray32',update:'replace'},fps:30,inputs:{},permissions:[{request_id:'net',id:'network.http',scope:{kind:'network',origins:['https://earthquake.usgs.gov'],methods:['GET']},required:true,reason:'Read the synthetic offline USGS qualification body'}]}}
export async function create(host){
 const opened=await host.sources.earthquakes.open({bounds:{west:-180,east:180,south:-90,north:90},max_entities:2,max_hz:1,fields:[]});
 if(!opened.ok || opened.value.status().state!=='ready' || !opened.value.latest().ok) throw Error('source_open_ack');
 return {render(context,frame){frame.gray.fill(0);frame.present()},dispose(){}};
}
"#;

fn archive(source: &str, origin: &str) -> Vec<u8> {
    let manifest = json!({
        "api_version":1,"id":"native-source-qualification","name":"Source qualification",
        "version":"1.0.0","entry":"entry.mjs","modes":["live"],
        "settings":{"type":"object","properties":{}},
        "capabilities":[{"id":"network.http","scope":{"origins":[origin],"methods":["GET"]}}],
        "files":[{"path":"entry.mjs","bytes":source.len(),
            "sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}]
    });
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("entry.mjs", options).unwrap();
    zip.write_all(source.as_bytes()).unwrap();
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    zip.finish().unwrap().into_inner()
}

fn rights(origin: &str) -> Ceiling {
    Ceiling {
        permissions: vec![Right {
            id: Capability::NetworkHttp,
            scope: Scope::Network {
                origins: BTreeSet::from([origin.into()]),
                methods: BTreeSet::from([HttpMethod::Get]),
            },
        }],
    }
}

fn instance(bytes: &[u8], quota: QuotaGroup, origin: &str) -> (PackageInstance, CreateState) {
    let helper = PathBuf::from(
        std::env::var_os("ILIUM_ANIMATION_HELPER")
            .expect("exact newly built helper is required for this explicit qualification"),
    );
    assert!(helper.is_absolute(), "helper path must be absolute");
    let ledger_storage = quota.reserve_external_storage(384 * 1024).unwrap();
    let verifier = TrustVerifier::from_release_inventory(Vec::new()).unwrap();
    let settings = json!({});
    let environment = json!({"cell_width":1,"cell_height":1,"dot_width":2,"dot_height":4});
    let verified = PackageInstance::verify(InstancePreparation {
        archive: bytes,
        verifier: &verifier,
        helper_executable: &helper,
        trusted_bootstrap: TRUSTED_BOOTSTRAP,
        settings: &settings,
        mode: AnimationMode::Live,
        environment: &environment,
        host_policy: rights(origin),
        instance_id: 91,
        limits: HelperLimits::default(),
        quota,
    })
    .unwrap();
    // Synthetic native ledger fixture: exact verified principal, no remembered
    // decisions. Restore it through the rights-bearing preparation path; this
    // must not use the restricted zero-right convenience method.
    let ledger = PermissionBroker::new(
        verifier.permission_identity(verified.package()).unwrap(),
        rights(origin),
        rights(origin),
    )
    .unwrap()
    .export_remembered()
    .unwrap();
    let (mut instance, review) = verified.prepare(Some(&ledger)).unwrap();
    drop(ledger);
    drop(ledger_storage);
    assert_eq!(review.items().len(), 1);
    let pending = instance
        .begin_resolution(
            review,
            BTreeMap::from([("net".into(), UserChoice::AllowSession)]),
        )
        .unwrap();
    assert!(pending.remembered_bytes().is_none());
    let result = instance.finish_resolution(pending).unwrap();
    assert!(result.denied_required.is_empty());
    assert!(result.authority_error.is_none());
    assert!(result.creation_error.is_none());
    (instance, result.creation.unwrap())
}

struct NoDns;
impl DnsResolver for NoDns {
    fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<std::net::SocketAddr>> {
        panic!("offline qualification must not perform DNS or production HTTP")
    }
}

fn resources() -> (Execution, Client, QuotaGroup, mpsc::Receiver<()>) {
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 8,
        jobs: 24,
        service_jobs: 0,
        input_bytes: 64 * 1024 * 1024,
        result_bytes: 64 * 1024 * 1024,
        worker_threads: 64,
        worker_bytes: 2 * 1024 * 1024 * 1024,
    });
    let disabled = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let lane = LaneConfig {
        threads: 1,
        queue_slots: 4,
        priority: None,
        resident_bytes_per_thread: 1024 * 1024,
    };
    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: lane,
            io: lane,
            service: disabled,
        },
    )
    .unwrap();
    let (sender, receiver) = mpsc::sync_channel(4);
    let client = execution
        .client(ClientLimits {
            jobs: 20,
            service_jobs: 0,
            input_bytes: 48 * 1024 * 1024,
            result_bytes: 48 * 1024 * 1024,
        })
        .unwrap()
        .with_completion_wake(move || {
            let _ = sender.try_send(());
        });
    (execution, client, quota, receiver)
}

fn wake(rx: &mpsc::Receiver<()>) {
    rx.recv_timeout(Duration::from_secs(3))
        .expect("original finite source bank completion wake");
}

fn seed(instance: &mut PackageInstance, descriptor: &ProjectedFeedDescriptor, quota: &QuotaGroup) {
    let copied = instance
        .copy_native_source_feed_snapshots(&[descriptor.metadata().clone()])
        .unwrap();
    assert_eq!(
        copied.metadata()[0]["revision"],
        descriptor.metadata()["revision"]
    );
    let shape = Shape {
        cell_width: 1,
        cell_height: 1,
        mode: Mode::Pixels,
        format: Format::Gray32,
        update: Update::Replace,
        cell_rgb: false,
        colour_space: crate::surface::ColourSpace::Srgb,
    };
    let layout = shape.layout().unwrap();
    let _admission = quota
        .reserve_external_storage(
            layout.canonical_bytes * 2 + layout.handoff_bytes * 2 + layout.dots * 64 + 65536,
        )
        .unwrap();
    let mut surface = Surface::new(91, 7, shape).unwrap();
    let frame = surface.begin(1).unwrap();
    let work = match frame.data {
        Data::F32(values) => values.into_iter().flat_map(f32::to_ne_bytes).collect(),
        _ => panic!("gray32 source qualification surface"),
    };
    let spec = [ArraySpec {
        name: "work_data".into(),
        kind: TypedArrayKind::F32,
        elements: layout.elements,
    }];
    instance
        .seed_frame(
            &json!({"frame":{"key":frame.key,"shape":shape,"reset":frame.reset,
                "invalid_rects":frame.invalid_rects,"input_specs":[]},
                "services":[descriptor.metadata()]}),
            &spec,
            &BTreeMap::from([("work_data".into(), work)]),
        )
        .unwrap();
    // Exercise the actual frame handoff and reject this fixture frame: no
    // native presentation owner has published it. Supply the native surface
    // key and shape exactly as production rendering does, then release seed
    // ownership before the next source revision without claiming emission.
    let rendered = instance
        .render(
            &json!({"_ilium_frame": {"key": frame.key, "shape": shape}}),
            &[
                ArraySpec {
                    name: "work_data".into(),
                    kind: TypedArrayKind::F32,
                    elements: layout.elements,
                },
                ArraySpec {
                    name: "data".into(),
                    kind: TypedArrayKind::F32,
                    elements: layout.elements,
                },
                ArraySpec {
                    name: "work_touch".into(),
                    kind: TypedArrayKind::U8,
                    elements: layout.samples,
                },
                ArraySpec {
                    name: "touch".into(),
                    kind: TypedArrayKind::U8,
                    elements: layout.samples,
                },
                ArraySpec {
                    name: "work_order".into(),
                    kind: TypedArrayKind::U32,
                    elements: layout.samples,
                },
                ArraySpec {
                    name: "order".into(),
                    kind: TypedArrayKind::U32,
                    elements: layout.samples,
                },
            ],
        )
        .unwrap();
    assert_eq!(rendered.output.metadata["presented"], true);
    assert!(rendered.output.metadata["error"].is_null());
    assert_eq!(rendered.output.planes["data"].len(), layout.elements * 4);
    instance.accept_frame(false).unwrap();
    drop(rendered);
}

#[test]
#[ignore = "run explicitly with the matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_original_source_feed_open_ack_refresh_seed_and_retirement() {
    let script = OPEN_SCRIPT;
    let bytes = archive(script, "https://earthquake.usgs.gov");
    let (mut execution, client, quota, rx) = resources();
    let _archive_admission = quota.reserve_external_storage(bytes.len() + 65536).unwrap();
    let (mut instance, creation) = instance(&bytes, quota.clone(), "https://earthquake.usgs.gov");
    assert_eq!(creation, CreateState::Pending);
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    let request = requests.into_iter().next().unwrap();
    assert_eq!(request.method, "sources.earthquakes.open");
    let _body_admission = quota
        .reserve_external_storage(2 * USGS_BODY.len() + 4096)
        .unwrap();
    let offline = Arc::new(OfflineSourceHttp {
        exact_url: USGS_URL.into(),
        public_address: "93.184.216.34:443".parse().unwrap(),
        bodies: Mutex::new(VecDeque::from([USGS_BODY.to_vec(), USGS_BODY.to_vec()])),
    });
    let mut actor = NativeSourceHost::start(
        &mut instance,
        request,
        SourceClock {
            monotonic_ms: 1000,
            epoch_ms: 1_700_000_000_000,
        },
        SourceActorEnvironment {
            client: client.clone(),
            dns: Arc::new(NoDns),
            cadence: Arc::new(SourceCadence::new(quota.clone()).unwrap()),
            limits: SourceActorLimits {
                pages: 8,
                encoded_bytes: 16 * 1024 * 1024,
                media: MediaLimits::default(),
            },
            credentials: None,
            offline_http: Some(Arc::clone(&offline)),
        },
    )
    .unwrap();
    let mut completed = None;
    for _ in 0..8 {
        wake(&rx);
        match actor.on_completion_wake(&mut instance).unwrap() {
            Some(SourceEvent::Complete(value)) => {
                completed = Some(value);
                break;
            }
            Some(SourceEvent::Failed(value)) => panic!("real source actor failed: {}", value.code),
            Some(SourceEvent::Lost) => panic!("source physical receipt lost"),
            None => {}
        }
    }
    let completion = completed.expect("real initial CPU/IO/CPU source completion");
    assert!(actor.is_drained());
    let mut drawing =
        NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default()).unwrap();
    let native_id = "source-feed-qualification";
    let (value, mut descriptor) = completion
        .copy_feed_open_result(&mut instance, &mut drawing, native_id)
        .unwrap();
    assert_eq!(descriptor.metadata()["revision"], 1);
    assert_eq!(descriptor.metadata()["status"]["state"], "ready");
    let prepared = completion
        .prepare_feed_transfer(&instance, &client, StopToken::default())
        .unwrap();
    assert_eq!(
        completion.publish(&mut instance, value).unwrap(),
        CompletionState::Delivered
    );
    completion.settle(&mut instance).unwrap();
    let mut feed = NativeSourceFeedHost::from_delivered(
        completion.into_feed_run(),
        client.clone(),
        Arc::new(NoDns),
        None,
        prepared,
    );
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    seed(&mut instance, &descriptor, &quota);
    let first_capture = feed.capture_latest(&instance, 2 * 1024 * 1024).unwrap();
    let first_summary = first_capture.summary(&instance).unwrap();
    assert_eq!(
        first_summary.family,
        crate::replay::InputFamily::Earthquakes
    );
    assert_eq!(first_summary.source_revision, 1);
    assert!(!first_capture.replay_lineage().is_empty());
    let first_frozen = crate::replay::FrozenInputSnapshot::from_native(
        first_summary.family,
        native_id,
        first_summary.source_revision,
        first_capture.to_frozen_service_value(&instance).unwrap(),
    )
    .unwrap();
    first_capture
        .verify_frozen_snapshot(&instance, &first_frozen)
        .unwrap();
    let frozen_recording = crate::replay::FrozenInputs::from_capture(
        quota.clone(),
        "native-feed-recording",
        vec![first_frozen],
        2 * 1024 * 1024,
        Some(1_700_000_000_000),
        "native feed source capture",
        first_capture.replay_lineage(),
    )
    .unwrap();
    assert_eq!(frozen_recording.snapshots().len(), 1);
    assert_eq!(
        frozen_recording.snapshots()[0].revision(),
        first_summary.source_revision
    );

    let due = feed.next_due_ms().unwrap().unwrap();
    assert!(feed
        .begin_due(
            &instance,
            SourceClock {
                monotonic_ms: due.saturating_add(1),
                epoch_ms: 1_700_000_000_000_i64 + due as i64,
            }
        )
        .unwrap());
    assert!(
        feed.capture_latest(&instance, 2 * 1024 * 1024).is_err(),
        "capture must refuse while a real feed refresh is unsettled"
    );
    let mut refreshed = None;
    for _ in 0..8 {
        wake(&rx);
        match feed.on_completion_wake(&mut instance).unwrap() {
            Some(SourceFeedEvent::Complete) => {
                refreshed = Some(feed.take_completion().unwrap());
                break;
            }
            Some(SourceFeedEvent::Failed(code)) => panic!("real feed refresh failed: {code}"),
            Some(SourceFeedEvent::Lost) => panic!("feed physical receipt lost"),
            None => {}
        }
    }
    let refresh = refreshed.expect("real refresh CPU/IO/CPU completion");
    let next = refresh
        .project(&mut instance, &mut drawing, native_id, 2)
        .unwrap();
    let mut proposed = Some(next);
    refresh
        .deliver(&mut instance, || {
            descriptor = proposed.take().unwrap();
        })
        .unwrap();
    refresh.settle(&mut instance).unwrap();
    feed.resume(refresh).unwrap();
    assert_eq!(descriptor.metadata()["revision"], 2);
    seed(&mut instance, &descriptor, &quota);
    let second_capture = feed.capture_latest(&instance, 2 * 1024 * 1024).unwrap();
    let second_summary = second_capture.summary(&instance).unwrap();
    assert_eq!(
        second_summary.family,
        crate::replay::InputFamily::Earthquakes
    );
    assert_eq!(second_summary.source_revision, 2);
    assert_eq!(second_summary.payload_digest, first_summary.payload_digest);
    assert_eq!(
        second_capture.replay_lineage().len(),
        first_capture.replay_lineage().len()
    );
    let second_frozen = crate::replay::FrozenInputSnapshot::from_native(
        second_summary.family,
        native_id,
        second_summary.source_revision,
        second_capture.to_frozen_service_value(&instance).unwrap(),
    )
    .unwrap();
    second_capture
        .verify_frozen_snapshot(&instance, &second_frozen)
        .unwrap();
    let sequence = crate::replay::FrozenSourceSequence::from_native(
        quota.clone(),
        2_000,
        vec![
            crate::replay::FrozenSourceFrame::from_native(0, frozen_recording.snapshots().to_vec()),
            crate::replay::FrozenSourceFrame::from_native(1_000, vec![second_frozen]),
        ],
        8,
        2 * 1024 * 1024,
    )
    .unwrap();
    assert_eq!(sequence.frame_at(0).unwrap().snapshots()[0].revision(), 1);
    assert_eq!(
        sequence.frame_at(1_000).unwrap().snapshots()[0].revision(),
        2
    );
    assert!(
        offline.bodies.lock().unwrap().is_empty(),
        "two real IO jobs consumed exactly two local bodies"
    );

    feed.cancel();
    assert!(feed.is_drained());
    drop(feed);
    drop(actor);
    let stopped = instance.stop();
    assert!(stopped.authority_error.is_none());
    stopped.cancellation.unwrap();
    assert!(instance.is_physically_retired());
    assert!(first_capture.summary(&instance).is_err());
    assert!(second_capture.summary(&instance).is_err());
    execution.request_shutdown(ShutdownMode::Drain);
    let joined = execution
        .join_until_background(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(joined.remaining_workers, 0);
    assert_eq!(
        joined
            .health
            .lanes
            .iter()
            .map(|lane| lane.joined)
            .sum::<usize>(),
        2
    );
}

#[test]
#[ignore = "run explicitly with the matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_source_feed_close_requires_original_control_ack() {
    let script = OPEN_SCRIPT.replace(
        " return {render(context,frame)",
        " opened.value.close(); return {render(context,frame)",
    );
    assert!(script.contains("opened.value.close()"));
    let bytes = archive(&script, "https://earthquake.usgs.gov");
    let (mut execution, client, quota, rx) = resources();
    let _archive_admission = quota.reserve_external_storage(bytes.len() + 65536).unwrap();
    let _body_admission = quota
        .reserve_external_storage(USGS_BODY.len() + 4096)
        .unwrap();
    let (mut instance, creation) = instance(&bytes, quota.clone(), "https://earthquake.usgs.gov");
    assert_eq!(creation, CreateState::Pending);
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    let offline = Arc::new(OfflineSourceHttp {
        exact_url: USGS_URL.into(),
        public_address: "93.184.216.34:443".parse().unwrap(),
        bodies: Mutex::new(VecDeque::from([USGS_BODY.to_vec()])),
    });
    let mut actor = NativeSourceHost::start(
        &mut instance,
        requests.into_iter().next().unwrap(),
        SourceClock {
            monotonic_ms: 1000,
            epoch_ms: 1_700_000_000_000,
        },
        SourceActorEnvironment {
            client: client.clone(),
            dns: Arc::new(NoDns),
            cadence: Arc::new(SourceCadence::new(quota.clone()).unwrap()),
            limits: SourceActorLimits {
                pages: 8,
                encoded_bytes: 16 * 1024 * 1024,
                media: MediaLimits::default(),
            },
            credentials: None,
            offline_http: Some(Arc::clone(&offline)),
        },
    )
    .unwrap();
    let mut completed = None;
    for _ in 0..8 {
        wake(&rx);
        match actor.on_completion_wake(&mut instance).unwrap() {
            Some(SourceEvent::Complete(value)) => {
                completed = Some(value);
                break;
            }
            Some(SourceEvent::Failed(value)) => panic!("source open failed: {}", value.code),
            Some(SourceEvent::Lost) => panic!("source opener physical receipt lost"),
            None => {}
        }
    }
    let completion = completed.expect("real original source opener terminal");
    let mut drawing =
        NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default()).unwrap();
    let native_id = "source-feed-qualification";
    let (value, descriptor) = completion
        .copy_feed_open_result(&mut instance, &mut drawing, native_id)
        .unwrap();
    let prepared = completion
        .prepare_feed_transfer(&instance, &client, StopToken::default())
        .unwrap();
    assert_eq!(
        completion.publish(&mut instance, value).unwrap(),
        CompletionState::Delivered
    );
    completion.settle(&mut instance).unwrap();
    let mut feed = NativeSourceFeedHost::from_delivered(
        completion.into_feed_run(),
        client.clone(),
        Arc::new(NoDns),
        None,
        prepared,
    );
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    let control = instance.requests().unwrap();
    assert_eq!(
        control.len(),
        1,
        "only the real helper may issue close control"
    );
    let control = control.into_iter().next().unwrap();
    assert_eq!(control.method, "sources.earthquakes.close");
    assert_eq!(control.payload.metadata()["id"], native_id);
    assert_eq!(
        control.payload.metadata()["kind"],
        descriptor.metadata()["kind"]
    );
    instance
        .authorize_native_source_feed_close(&control)
        .unwrap();
    feed.cancel();
    assert!(feed.is_drained());
    let next_revision = descriptor.metadata()["revision"].as_u64().unwrap() + 1;
    let closed = json!({"id":native_id,"kind":descriptor.metadata()["kind"],
        "revision":next_revision,"status":{"state":"closed"}});
    let value = ServiceValue::copy_from_host(
        &json!({"ok":true,"value":closed}),
        &[],
        &BTreeMap::new(),
        instance.engine_limits(),
        quota.clone(),
    )
    .unwrap();
    assert_eq!(
        instance
            .complete_native_source_feed_close(&control, value)
            .unwrap(),
        CompletionState::Delivered
    );
    assert!(instance.requests().unwrap().is_empty());
    drop(feed);
    drop(actor);
    let stopped = instance.stop();
    assert!(stopped.authority_error.is_none());
    stopped.cancellation.unwrap();
    assert!(instance.is_physically_retired());
    execution.request_shutdown(ShutdownMode::Drain);
    let joined = execution
        .join_until_background(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(joined.remaining_workers, 0);
    assert_eq!(
        joined
            .health
            .lanes
            .iter()
            .map(|lane| lane.joined)
            .sum::<usize>(),
        2
    );
}

#[test]
#[ignore = "run explicitly with the matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_source_image_copy_refusal_close_ack_and_revocation() {
    use image::{ImageBuffer, ImageFormat, Rgba};
    let script = r#"
export function plan(){return {output:{mode:'pixels',format:'gray32',update:'replace'},fps:30,inputs:{},permissions:[{request_id:'net',id:'network.http',scope:{kind:'network',origins:['https://tile.openstreetmap.org'],methods:['GET']},required:true,reason:'Read the synthetic offline tile qualification body'}]}}
export async function create(host){
 const tile=await host.sources.osm.tile({x:0,y:0,zoom:0,format:'raster'});
 if(!tile.ok || !tile.value.image || typeof tile.value.image.id!=='string') throw Error('native_tile_image');
 host.media.images.close(tile.value.image);
 return {render(context,frame){frame.gray.fill(0);frame.present()},dispose(){}};
}
"#;
    let bytes = archive(script, OSM_ORIGIN);
    let (mut execution, client, quota, rx) = resources();
    let _archive_admission = quota.reserve_external_storage(bytes.len() + 65536).unwrap();
    let png = ImageBuffer::from_pixel(1, 1, Rgba([19u8, 23, 29, 255]));
    let mut encoded = Cursor::new(Vec::new());
    png.write_to(&mut encoded, ImageFormat::Png).unwrap();
    let body = encoded.into_inner();
    let _body_admission = quota.reserve_external_storage(body.len() + 4096).unwrap();
    let (mut instance, creation) = instance(&bytes, quota.clone(), OSM_ORIGIN);
    assert_eq!(creation, CreateState::Pending);
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    let request = requests.into_iter().next().unwrap();
    assert_eq!(request.method, "sources.osm.tile");
    let offline = Arc::new(OfflineSourceHttp {
        exact_url: OSM_URL.into(),
        public_address: "93.184.216.34:443".parse().unwrap(),
        bodies: Mutex::new(VecDeque::from([body])),
    });
    let mut actor = NativeSourceHost::start(
        &mut instance,
        request,
        SourceClock {
            monotonic_ms: 1000,
            epoch_ms: 1_700_000_000_000,
        },
        SourceActorEnvironment {
            client: client.clone(),
            dns: Arc::new(NoDns),
            cadence: Arc::new(SourceCadence::new(quota.clone()).unwrap()),
            limits: SourceActorLimits {
                pages: 8,
                encoded_bytes: 16 * 1024 * 1024,
                media: MediaLimits::default(),
            },
            credentials: None,
            offline_http: Some(Arc::clone(&offline)),
        },
    )
    .unwrap();
    let mut completed = None;
    for _ in 0..8 {
        wake(&rx);
        match actor.on_completion_wake(&mut instance).unwrap() {
            Some(SourceEvent::Complete(value)) => {
                completed = Some(value);
                break;
            }
            Some(SourceEvent::Failed(value)) => {
                panic!("source image operation failed: {}", value.code)
            }
            Some(SourceEvent::Lost) => panic!("source image physical receipt lost"),
            None => {}
        }
    }
    let completion = completed.expect("original source image CPU/IO/CPU terminal");
    assert!(actor.is_drained());
    let foreign_quota = QuotaGroup::new(QuotaLimits {
        clients: 1,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 0,
        worker_bytes: 64 * 1024 * 1024,
    });
    let mut foreign_drawing =
        NativeDrawHost::new(foreign_quota, MediaLimits::default(), DrawLimits::default()).unwrap();
    assert!(
        completion
            .copy_operation_result(&mut instance, &mut foreign_drawing)
            .is_err(),
        "a copied/foreign draw root cannot import the source image"
    );
    let mut drawing =
        NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default()).unwrap();
    let (value, imported) = completion
        .copy_operation_result(&mut instance, &mut drawing)
        .unwrap();
    assert_eq!(imported.len(), 1);
    let image_id = &imported[0];
    assert!(drawing.owns_source_image(image_id));
    let crate::native_source_host::NativeSourceOutput::Operation(original_output) =
        completion.authorized_output(&instance).unwrap()
    else {
        panic!("native source completion must retain its operation output");
    };
    let original_image = original_output.native_images()[0].clone();
    let repeated_descriptor = drawing
        .retain_source_image(&mut instance, &original_image)
        .unwrap();
    assert_eq!(
        repeated_descriptor["id"].as_str(),
        Some(image_id.as_str()),
        "replay reuse of one admitted image must not allocate duplicate native handles"
    );
    assert_eq!(
        completion.publish(&mut instance, value).unwrap(),
        CompletionState::Delivered
    );
    completion.settle(&mut instance).unwrap();
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    let controls = instance.requests().unwrap();
    assert_eq!(controls.len(), 1);
    let control = controls.into_iter().next().unwrap();
    assert_eq!(control.method, "media.images.close");
    assert_eq!(control.payload.metadata()["id"], image_id.as_str());
    // The original image remains registered until the exact helper control is
    // copied and its native completion returns Delivered.
    assert!(drawing.owns_source_image(image_id));
    let value = ServiceValue::copy_from_host(
        &json!({"ok":true,"value":null}),
        &[],
        &BTreeMap::new(),
        instance.engine_limits(),
        quota.clone(),
    )
    .unwrap();
    assert_eq!(
        instance
            .complete_native_source_image_close(&control, value)
            .unwrap(),
        CompletionState::Delivered
    );
    drawing.release_source_image(image_id).unwrap();
    assert!(!drawing.owns_source_image(image_id));
    assert!(drawing.release_source_image(image_id).is_err());
    assert!(offline.bodies.lock().unwrap().is_empty());
    instance.revoke_activation().unwrap();
    assert!(
        completion.authorized_output(&instance).is_err(),
        "the copied terminal cannot revive source rights after revocation"
    );
    assert!(
        drawing
            .retain_source_image(&mut instance, &original_image)
            .is_err(),
        "a cached image alias cannot cross a revoked activation"
    );
    drop(completion);
    drop(actor);
    drop(drawing);
    let stopped = instance.stop();
    assert!(stopped.authority_error.is_none());
    stopped.cancellation.unwrap();
    assert!(instance.is_physically_retired());
    execution.request_shutdown(ShutdownMode::Drain);
    let joined = execution
        .join_until_background(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(joined.remaining_workers, 0);
    assert_eq!(
        joined
            .health
            .lanes
            .iter()
            .map(|lane| lane.joined)
            .sum::<usize>(),
        2
    );
}

#[test]
#[ignore = "run explicitly with the matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_lost_source_receipt_never_claims_terminal_ack_or_retirement() {
    let bytes = archive(OPEN_SCRIPT, "https://earthquake.usgs.gov");
    let (mut execution, client, quota, rx) = resources();
    let _archive_admission = quota.reserve_external_storage(bytes.len() + 65536).unwrap();
    let (mut instance, creation) = instance(&bytes, quota.clone(), "https://earthquake.usgs.gov");
    assert_eq!(creation, CreateState::Pending);
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    let mut actor = NativeSourceHost::start(
        &mut instance,
        requests.into_iter().next().unwrap(),
        SourceClock {
            monotonic_ms: 1000,
            epoch_ms: 1_700_000_000_000,
        },
        SourceActorEnvironment {
            client,
            dns: Arc::new(NoDns),
            cadence: Arc::new(SourceCadence::new(quota.clone()).unwrap()),
            limits: SourceActorLimits {
                pages: 8,
                encoded_bytes: 16 * 1024 * 1024,
                media: MediaLimits::default(),
            },
            credentials: None,
            offline_http: None,
        },
    )
    .unwrap();
    wake(&rx);
    let Some(SourceStage::Cpu { receipt, .. }) = actor.stage.as_mut() else {
        panic!("original CPU receipt absent")
    };
    let JobPoll::Ready(original) = receipt.try_take() else {
        panic!("original completed CPU receipt absent")
    };
    drop(original); // Test-only external consumption makes the owner's receipt Lost.
    assert!(matches!(
        actor.on_completion_wake(&mut instance).unwrap(),
        Some(SourceEvent::Lost)
    ));
    assert!(matches!(
        actor.stage.as_ref(),
        Some(SourceStage::LostCpu { .. })
    ));
    assert!(!actor.is_drained());
    actor.cancel();
    assert!(
        !actor.is_drained(),
        "logical cancellation is no physical proof"
    );
    assert!(instance.requests().unwrap().is_empty());
    drop(actor);
    let stopped = instance.stop();
    assert!(stopped.authority_error.is_none());
    stopped.cancellation.unwrap();
    assert!(instance.is_physically_retired());
    execution.request_shutdown(ShutdownMode::Drain);
    let joined = execution
        .join_until_background(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(joined.remaining_workers, 0);
    assert_eq!(
        joined
            .health
            .lanes
            .iter()
            .map(|lane| lane.joined)
            .sum::<usize>(),
        2
    );
}
