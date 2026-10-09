//! Actual-helper replay custody checks. Every accepted authority is issued by
//! PackageInstance and the durable PluginPermissionController, never a fixture.
#![cfg(target_os = "linux")]

use super::*;
use crate::{
    animation_plugins::review_bridge::ReviewBridge,
    filesystem::plugin_permissions::PluginPermissionFiles,
};
#[cfg(feature = "qualification-test-support")]
use ilium_animation_js::permissions::{Capability, HttpMethod, Right, Scope, UserChoice};
use ilium_animation_js::{
    engine::CreateState,
    helper::HelperAuthority,
    manifest::AnimationMode,
    permissions::Ceiling,
    replay::{ClipSpecification, FrozenEvidence, FrozenInputs},
    runtime::InstancePreparation,
    trust::TrustVerifier,
    TRUSTED_BOOTSTRAP,
};
use ilium_execution::{Client, ClientLimits, Execution, ExecutionConfig, LaneConfig, ShutdownMode};
use image::{ImageBuffer, ImageFormat, Rgba};
use std::{
    io::{Cursor, Write},
    path::Path,
    sync::mpsc::{self, Receiver, Sender},
};

const PROCEDURAL_SCRIPT: &str = "export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}} export async function create(){return {render(context,frame){frame.cells.set_cell(0,0,{mask:1});frame.present()},dispose(){}}}";
const FAILING_SCRIPT: &str = "export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}} export async function create(){return {render(){throw Error('producer_fixture_failure')},dispose(){}}}";
const FRAME_BYTES: &[u8] = br#"{"masks":[0],"rgb":[null],"text":[]}"#;
const PRE_RENDER_NATIVE_TEXT_SCRIPT: &str = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}} export async function create(host){return {render(context,frame){const glyph=context.time<0.25?'界':'e\u0301';const result=host.text.spans({frame,x:0,y:0,max_cells:2,spans:[{text:glyph,foreground:{r:255,g:0,b:0},background:{r:0,g:64,b:0},bold:true,italic:true,underline:true}]});if(!result.ok)throw Error(result.error.code);frame.present()},dispose(){}}}"#;
const RECORDED_BUNDLE_VIDEO_SCRIPT: &str = r#"export function plan(){return {fps:2,output:{mode:'pixels',format:'rgba8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}} export async function create(host){const opened=await host.media.video.open({asset:host.assets.bundle,relative_path:'assets/frame.ppm',max_pixels:8,max_fps:2});if(!opened.ok)throw Error(opened.error.code);const video=opened.value;return {render(context,frame){const latest=video.latest();if(!latest.ok||!latest.value)throw Error('recorded_video_frame_missing');const drawn=host.media.images.blit({frame,image:latest.value.image,rectangle:{unit:'pixels',x:0,y:0,width:2,height:4},fit:'stretch'});if(!drawn.ok)throw Error(drawn.error.code);frame.present()},dispose(){}}}"#;
const LIVE_EMPTY_REPLAY_FREEZE_SCRIPT: &str = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}} export async function create(host){const frozen=await host.replay.freeze({sources:[],max_bytes:1024});if(!frozen.ok)throw Error('replay_freeze_refused:'+frozen.error.code);return {render(context,frame){frame.cells.set_cell(0,0,{mask:1});frame.present()},dispose(){}}}"#;
#[cfg(feature = "qualification-test-support")]
const LIVE_SOURCE_SEQUENCE_SCRIPT: &str = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},permissions:[{request_id:'source',id:'network.http',scope:{kind:'network',origins:['https://earthquake.usgs.gov'],methods:['GET']},required:true,reason:'Read the local offline replay qualification response'}],replay:{seed:3,duration_seconds:2,seamless:false}}} export async function create(host){const opened=await host.sources.earthquakes.open({bounds:{west:-180,east:180,south:-90,north:90},max_entities:2,max_hz:4,fields:['magnitude']});if(!opened.ok)throw Error('source_open:'+opened.error.code);const captured=await host.replay.capture_sequence({sources:[opened.value],duration_ms:1500,sample_hz:2,max_frames:3,max_bytes:1048576});if(!captured.ok)throw Error('capture_sequence:'+captured.error.code);return {render(context,frame){const latest=opened.value.latest();if(!latest.ok||!latest.value||!latest.value.entities.length)throw Error('replay_feed_missing');frame.cells.set_cell(0,0,{mask:latest.value.entities[0].magnitude>=3?2:1});frame.present()},dispose(){opened.value.close()}}}"#;
#[cfg(feature = "qualification-test-support")]
const USGS_URL: &str = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_day.geojson";
#[cfg(feature = "qualification-test-support")]
const USGS_BODY_LOW: &[u8] = br#"{"type":"FeatureCollection","features":[{"id":"replay-fixture","type":"Feature","properties":{"mag":1,"time":1000,"place":"low"},"geometry":{"type":"Point","coordinates":[1,1,1]}}]}"#;
#[cfg(feature = "qualification-test-support")]
const USGS_BODY_HIGH: &[u8] = br#"{"type":"FeatureCollection","features":[{"id":"replay-fixture","type":"Feature","properties":{"mag":5,"time":2000,"place":"high"},"geometry":{"type":"Point","coordinates":[1,1,1]}}]}"#;

#[cfg(feature = "qualification-test-support")]
fn prepared_live_source_sequence() -> (
    Fixture,
    ilium_animation_js::runtime::PackageInstance,
    Presentation,
) {
    use ilium_animation_js::native_source_host::OfflineSourceHttp;

    let fixture = Fixture::new_with_capabilities(
        LIVE_SOURCE_SEQUENCE_SCRIPT,
        AnimationMode::PreRendered,
        vec![json!({
            "id":"network.http",
            "scope":{"origins":["https://earthquake.usgs.gov"],"methods":["GET"]}
        })],
    );
    let (mut instance, review) = fixture
        .verified_with_policy(
            AnimationMode::PreRendered,
            Ceiling {
                permissions: vec![Right {
                    id: Capability::NetworkHttp,
                    scope: Scope::Network {
                        origins: std::collections::BTreeSet::from([
                            "https://earthquake.usgs.gov".into()
                        ]),
                        methods: std::collections::BTreeSet::from([HttpMethod::Get]),
                    },
                }],
            },
        )
        .prepare(None)
        .unwrap();
    assert_eq!(review.items().len(), 1);
    let pending = instance
        .begin_resolution(
            review,
            BTreeMap::from([("source".into(), UserChoice::AllowSession)]),
        )
        .unwrap();
    let resolution = instance.finish_resolution(pending).unwrap();
    assert!(resolution.denied_required.is_empty());
    let mut presentation = Presentation::new(
        &mut instance,
        &fixture.request,
        &fixture.quota,
        fixture.resources.clone(),
        Arc::clone(&fixture.state_root),
        None,
        false,
        resolution.accepted_creation().expect("accepted creation"),
        Arc::new({
            let sender = fixture.wake_sender.clone();
            move || {
                let _ = sender.send(());
            }
        }),
    )
    .unwrap();
    presentation.sources.offline_http = Some(
        OfflineSourceHttp::new(
            USGS_URL.into(),
            "93.184.216.34:443".parse().unwrap(),
            vec![
                USGS_BODY_LOW.to_vec(),
                USGS_BODY_HIGH.to_vec(),
                USGS_BODY_LOW.to_vec(),
                USGS_BODY_HIGH.to_vec(),
                USGS_BODY_LOW.to_vec(),
                USGS_BODY_HIGH.to_vec(),
                USGS_BODY_LOW.to_vec(),
                USGS_BODY_HIGH.to_vec(),
            ],
        )
        .unwrap(),
    );
    (fixture, instance, presentation)
}

#[test]
#[cfg(feature = "qualification-test-support")]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn client_replay_sequence_renders_captured_feed_revisions_after_helper_retirement() {
    let (fixture, mut instance, mut presentation) = prepared_live_source_sequence();

    let mut ready = false;
    for _ in 0..800 {
        while fixture.wake_receiver.try_recv().is_ok() {
            presentation
                .sources
                .on_completion_wake(&mut instance, &mut presentation.drawing)
                .unwrap();
        }
        presentation.dispatch_requests(&mut instance).unwrap();
        presentation.creation = instance.pump().unwrap();
        if presentation.creation == CreateState::Ready {
            presentation.dispatch_requests(&mut instance).unwrap();
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready, "the real client replay capture did not settle");
    let recording = presentation
        .sources
        .recordings
        .values()
        .next()
        .expect("client dispatcher must retain its native sequence");
    let sequence = recording
        .sequence
        .as_ref()
        .expect("capture_sequence must retain a source timeline");
    assert!(
        sequence.frames().len() >= 2,
        "a later source revision is required"
    );
    assert_eq!(
        sequence.frames()[0].snapshots()[0].family(),
        ilium_animation_js::replay::InputFamily::Earthquakes
    );
    assert!(sequence.frames()[1].snapshots()[0].revision() > 1);
    let captured_magnitudes: Vec<_> = sequence
        .frames()
        .iter()
        .filter_map(|frame| {
            frame.snapshots()[0].value().metadata()["entities"][0]["magnitude"].as_f64()
        })
        .collect();
    assert!(captured_magnitudes.contains(&1.0));
    assert!(captured_magnitudes.contains(&5.0));
    let certification = instance
        .certify_source_sequence_replay(&recording.frozen, sequence, &recording.sequence_captures)
        .expect("actual client-captured source timeline must satisfy the pre-render certificate");
    let verifier = ilium_animation_js::release::verifier().unwrap();
    let package_identity = verifier.verify(instance.package());
    let evidence = FrozenEvidence::from_native(fixture.quota.clone(), &package_identity, &[])
        .expect("source replay has no recorded-video evidence");
    let appearance_digest: [u8; 32] =
        Sha256::digest(serde_json::to_vec(instance.settings()).unwrap()).into();
    let source_spec = ClipSpec::from_accepted(
        &fixture.quota,
        ClipSpecification {
            package: instance.package(),
            verifier: &verifier,
            plan: instance.plan(),
            settings: instance.settings(),
            shape: shape(
                instance.plan(),
                fixture.request.width,
                fixture.request.height,
            )
            .unwrap(),
            backend: "native-v8",
            api_version: instance.package().manifest().api_version,
            appearance_digest,
            certification,
            frozen: Arc::clone(&recording.frozen),
            source_sequence: Some(Arc::clone(
                recording
                    .sequence
                    .as_ref()
                    .expect("captured native sequence"),
            )),
            evidence,
        },
    )
    .expect("captured source timeline must form an accepted ClipSpec");
    assert!(
        !source_spec.can_stream_procedural(),
        "native source replay must not enter the source-free disk cache"
    );
    let frozen = presentation
        .sources
        .frozen_inputs_for(
            &instance,
            recording.frozen.recording().unwrap(),
            fixture.quota.clone(),
            1024 * 1024,
        )
        .expect("the captured initial feed must resolve through the presentation owner");
    assert_eq!(frozen.snapshots().len(), 1);
    assert_eq!(
        frozen.snapshots()[0].family(),
        ilium_animation_js::replay::InputFamily::Earthquakes
    );
    let (authority, authorization) = instance
        .retain_procedural_replay_authorization()
        .expect("the original pre-render activation authorizes its captured clip");
    let instance = Arc::new(Mutex::new(instance));
    let presentation = Arc::new(Mutex::new(presentation));
    let cache = Arc::new(ReplayCache::new(fixture.quota.clone(), ReplayLimits::default()).unwrap());
    let store = fixture.store();
    match cache.begin_streaming(
        Arc::clone(&source_spec),
        authority.clone(),
        Arc::clone(&authorization),
        StopToken::default(),
        Arc::clone(&store),
        || panic!("a source-backed clip must be refused before owner admission"),
    ) {
        Err(ilium_animation_js::error::AnimationError::PermissionDenied(_)) => {}
        Err(error) => panic!("source-backed streaming refusal was unexpected: {error}"),
        Ok(_) => panic!("source-backed clips must be rejected by the cache API"),
    }
    let mut receipt = fixture
        .client
        .try_reserve(
            Lane::Cpu,
            JobCost {
                input_bytes: 1024 * 1024,
                result_bytes: 1024 * 1024,
            },
        )
        .unwrap()
        .submit(PreRenderJob {
            request: fixture.request.clone(),
            instance: Arc::clone(&instance),
            presentation: Arc::clone(&presentation),
            cache: Arc::clone(&cache),
            store: Arc::clone(&store),
            spec: Arc::clone(&source_spec),
            authority: authority.clone(),
            authorization: Arc::clone(&authorization),
            quota: fixture.quota.clone(),
            stop: StopToken::default(),
        })
        .unwrap();
    let outcome = loop {
        fixture.await_wake();
        if let JobPoll::Ready(outcome) = receipt.try_take() {
            break outcome;
        }
    };
    assert!(matches!(outcome.view(), JobOutcome::Finished(Ok(_))));
    assert!(instance.lock().unwrap().is_physically_retired());
    assert!(presentation_is_drained(&presentation.lock().unwrap()));
    match cache.open_procedural_from_disk(
        Arc::clone(&source_spec),
        authority.clone(),
        Arc::clone(&authorization),
        &store,
    ) {
        Err(ilium_animation_js::error::AnimationError::PermissionDenied(_)) => {}
        Err(error) => panic!("source-backed cold-open refusal was unexpected: {error}"),
        Ok(_) => panic!("source-backed clips must be rejected by the cold disk API"),
    }

    let clip = match cache
        .begin(
            Arc::clone(&source_spec),
            authority.clone(),
            Arc::clone(&authorization),
            StopToken::default(),
            || {
                Err(ilium_animation_js::error::AnimationError::Runtime(
                    "source replay must already be cached".into(),
                ))
            },
        )
        .unwrap()
    {
        Preparation::Cached(clip) => clip,
        Preparation::Started(_) | Preparation::InProgress => {
            panic!("source replay was not published to the in-memory cache")
        }
    };
    match clip.archive_procedural(&store, &StopToken::default()) {
        Err(ilium_animation_js::error::AnimationError::PermissionDenied(_)) => {}
        Err(error) => panic!("source-backed archive refusal was unexpected: {error}"),
        Ok(_) => panic!("source-backed clips must be rejected by the archive API"),
    }
    let mut player = ReplayPlayer::new(
        clip,
        authority,
        authorization,
        PlaybackSettings {
            now: Duration::ZERO,
            speed: 1.0,
            mode: PlaybackMode::Once,
            max_leases: 1,
            stop: StopToken::default(),
        },
    )
    .unwrap();
    let mut rendered_masks = Vec::new();
    for frame_index in 0..source_spec.frame_count() {
        let now = Duration::from_millis((frame_index * 500 + 1) as u64);
        let (playback, _) = player.sample(now).unwrap();
        let Playback::Frame(lease) = playback else {
            panic!("source replay ended before its accepted frame count");
        };
        rendered_masks.push(lease.packed().unwrap().masks[0]);
    }
    assert!(
        rendered_masks.contains(&1),
        "low magnitude should render mask 1"
    );
    assert!(
        rendered_masks.contains(&2),
        "high magnitude should render mask 2"
    );
}

#[test]
#[cfg(feature = "qualification-test-support")]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn client_replay_sequence_cancellation_drains_source_and_discards_partial_recording() {
    let (fixture, mut instance, mut presentation) = prepared_live_source_sequence();
    let mut capture_id = None;
    for _ in 0..800 {
        while fixture.wake_receiver.try_recv().is_ok() {
            presentation
                .sources
                .on_completion_wake(&mut instance, &mut presentation.drawing)
                .unwrap();
        }
        presentation.dispatch_requests(&mut instance).unwrap();
        presentation.creation = instance.pump().unwrap();
        capture_id = presentation
            .sources
            .pending_sequences
            .iter()
            .find(|(_, pending)| pending.frames.len() >= 2)
            .map(|(id, _)| *id);
        if capture_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let capture_id = capture_id.expect("capture must retain partial frames before cancellation");
    presentation
        .sources
        .pending_sequences
        .get(&capture_id)
        .expect("pending source capture")
        .request
        .stop_token()
        .stop();

    let deadline = Instant::now() + Duration::from_secs(10);
    let drained = loop {
        while fixture.wake_receiver.try_recv().is_ok() {
            presentation
                .sources
                .on_completion_wake(&mut instance, &mut presentation.drawing)
                .unwrap();
        }
        presentation
            .sources
            .advance_sequence_captures(&mut instance, &mut presentation.drawing)
            .unwrap();
        let source_drained = presentation
            .sources
            .feeds
            .values()
            .all(|feed| feed.host.is_drained() && feed.terminal.is_none());
        if !presentation
            .sources
            .pending_sequences
            .contains_key(&capture_id)
            && source_drained
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        drained,
        "cancelled capture must wait for source worker drain"
    );
    assert!(presentation.sources.recordings.is_empty());
    assert!(presentation.sources.pending_sequences.is_empty());
    assert!(
        instance.pump().is_err(),
        "the helper must settle the cancelled capture request"
    );

    presentation.sources.cancel_all();
    instance.retire_helper().unwrap();
    assert!(instance.is_physically_retired());
}

#[test]
fn presentation_settles_two_pure_create_yields_before_procedural_replay() {
    let script = "export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}} export async function create(host){for(let i=0;i<2;i++){const value=await host.tasks.yield();if(!value.ok)throw Error(value.error.code)}return {render(context,frame){frame.cells.set_cell(0,0,{mask:1});frame.present()},dispose(){}}}";
    let fixture = Fixture::new(script, AnimationMode::PreRendered);
    let (mut instance, review) = fixture
        .verified(AnimationMode::PreRendered)
        .prepare_without_rights()
        .unwrap();
    let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
    let resolution = instance.finish_resolution(pending).unwrap();
    assert_eq!(resolution.accepted_creation(), Some(CreateState::Pending));
    let mut presentation = Presentation::new(
        &mut instance,
        &fixture.request,
        &fixture.quota,
        fixture.resources.clone(),
        Arc::clone(&fixture.state_root),
        None,
        fixture
            .request
            .settings
            .plugin
            .selected
            .as_ref()
            .is_some_and(|selected| selected.mode == AnimationMode::Live),
        CreateState::Pending,
        Arc::new({
            let sender = fixture.wake_sender.clone();
            move || {
                let _ = sender.send(());
            }
        }),
    )
    .unwrap();
    presentation.dispatch_requests(&mut instance).unwrap();
    for index in 0..2 {
        let due = presentation
            .tasks
            .next_due()
            .expect("real deferred task turn");
        let wait = due.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
        assert!(presentation
            .tasks
            .on_due(&mut instance, Instant::now())
            .unwrap());
        presentation.creation = instance.pump().unwrap();
        presentation.dispatch_requests(&mut instance).unwrap();
        assert_eq!(
            presentation.creation,
            if index == 0 {
                CreateState::Pending
            } else {
                CreateState::Ready
            }
        );
    }
    assert!(presentation.tasks.next_due().is_none());
    assert!(instance.certify_procedural_replay().is_ok());
    revoke_presentation(&mut presentation);
    assert!(presentation.tasks.is_drained());
    let stopped = instance.stop();
    assert!(stopped.authority_error.is_none());
    assert!(stopped.cancellation.is_ok());
}

#[test]
fn live_replay_freeze_acknowledges_an_explicit_empty_source_set() {
    let fixture = Fixture::new(&LIVE_EMPTY_REPLAY_FREEZE_SCRIPT, AnimationMode::Live);
    let (mut instance, review) = fixture
        .verified(AnimationMode::Live)
        .prepare_without_rights()
        .unwrap();
    let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
    let resolution = instance.finish_resolution(pending).unwrap();
    let mut presentation = Presentation::new(
        &mut instance,
        &fixture.request,
        &fixture.quota,
        fixture.resources.clone(),
        Arc::clone(&fixture.state_root),
        None,
        true,
        resolution.accepted_creation().expect("accepted creation"),
        Arc::new(|| {}),
    )
    .unwrap();
    for _ in 0..100 {
        presentation.dispatch_requests(&mut instance).unwrap();
        presentation.creation = instance.pump().unwrap();
        if presentation.creation == CreateState::Ready {
            presentation.dispatch_requests(&mut instance).unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(presentation.creation, CreateState::Ready);
    let recording_id = presentation
        .sources
        .recordings
        .keys()
        .next()
        .expect("native replay freeze retained its recording")
        .clone();
    assert!(presentation
        .sources
        .frozen_inputs_for(
            &instance,
            "replay-capture-foreign",
            fixture.quota.clone(),
            1024,
        )
        .is_err());
    assert!(presentation
        .sources
        .frozen_inputs_for(&instance, &recording_id, fixture.quota.clone(), 0)
        .is_err());
    let frozen = presentation
        .sources
        .frozen_inputs_for(&instance, &recording_id, fixture.quota.clone(), 1024)
        .expect("retained capture must become the inputs used by replay preparation");
    assert_eq!(frozen.recording(), Some(recording_id.as_str()));
    assert!(frozen.snapshots().is_empty());
    revoke_presentation(&mut presentation);
    let stopped = instance.stop();
    assert!(stopped.authority_error.is_none());
    assert!(stopped.cancellation.is_ok());
}

fn archive(source: &str, mode: AnimationMode) -> Vec<u8> {
    archive_with_capabilities(source, mode, Vec::new())
}

fn archive_with_capabilities(
    source: &str,
    mode: AnimationMode,
    capabilities: Vec<serde_json::Value>,
) -> Vec<u8> {
    let mode_name = match mode {
        AnimationMode::Live => "live",
        AnimationMode::PreRendered => "pre_rendered",
    };
    let manifest = json!({
        "api_version":1,"id":"native-replay-custody","name":"Native replay custody",
        "version":"1.0.0","entry":"entry.mjs","modes":[mode_name],
        "capabilities":capabilities,
        "settings":{"type":"object","properties":{}},
        "files":[{"path":"entry.mjs","bytes":source.len(),
            "sha256":format!("{:x}", Sha256::digest(source.as_bytes()))}]
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

/// A labelled synthetic 2x2 RGB PPM in the verified package, not a URL or
/// external path handed to FFmpeg. The real decoder still runs in its cgroup.
fn recorded_video_archive(source: &str) -> Vec<u8> {
    let mut ppm = b"P6\n2 2\n255\n".to_vec();
    ppm.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
    let manifest = json!({
        "api_version":1,"id":"native-replay-custody","name":"Native replay custody",
        "version":"1.0.0","entry":"entry.mjs","modes":["pre_rendered"],
        "settings":{"type":"object","properties":{}},
        "files":[
            {"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))},
            {"path":"assets/frame.ppm","bytes":ppm.len(),"sha256":format!("{:x}",Sha256::digest(&ppm))}
        ]
    });
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("entry.mjs", options).unwrap();
    zip.write_all(source.as_bytes()).unwrap();
    zip.start_file("assets/frame.ppm", options).unwrap();
    zip.write_all(&ppm).unwrap();
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    zip.finish().unwrap().into_inner()
}

fn pinned(path: &Path) -> Arc<PinnedDirectory> {
    Arc::new(
        PinnedDirectory::from_host(Arc::new(NoFollowDirectory::open_root(path).unwrap())).unwrap(),
    )
}

struct Fixture {
    _temporary: tempfile::TempDir,
    ledger_root: Arc<PinnedDirectory>,
    state_root: Arc<PinnedDirectory>,
    clip_root: Arc<PinnedDirectory>,
    quota: QuotaGroup,
    execution: Execution,
    client: Client,
    resources: ilium_ambient::resources::AmbientResources,
    review: Arc<ReviewBridge>,
    wake_sender: Sender<()>,
    wake_receiver: Receiver<()>,
    request: RenderRequest,
    archive: Vec<u8>,
}
impl Fixture {
    fn new(source: &str, mode: AnimationMode) -> Self {
        Self::build(archive(source, mode))
    }

    #[cfg(feature = "qualification-test-support")]
    fn new_with_capabilities(
        source: &str,
        mode: AnimationMode,
        capabilities: Vec<serde_json::Value>,
    ) -> Self {
        Self::build(archive_with_capabilities(source, mode, capabilities))
    }

    fn build(archive: Vec<u8>) -> Self {
        // Presentation's process-wide SourceCadence is bound to this exact
        // original quota root even when this fixture requests no sources.
        let quota = crate::execution::process_quota();
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 8,
            priority: None,
            resident_bytes_per_thread: 64 * 1024,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane,
                io: lane,
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .unwrap();
        let (wake_sender, wake_receiver) = mpsc::channel();
        let completion_sender = wake_sender.clone();
        let client = execution
            .client(ClientLimits {
                jobs: 16,
                service_jobs: 0,
                input_bytes: 128 * 1024 * 1024,
                result_bytes: 128 * 1024 * 1024,
            })
            .unwrap()
            .with_completion_wake(move || {
                let _ = completion_sender.send(());
            });
        let resources = ilium_ambient::resources::AmbientResources::new(client.clone());
        let ui_ready = Arc::new(tokio::sync::Notify::new());
        let review_sender = wake_sender.clone();
        let review = ReviewBridge::new(
            quota.clone(),
            ui_ready,
            Box::new(move || {
                let _ = review_sender.send(());
            }),
        )
        .unwrap();
        review.select(1, true).unwrap();
        let temporary = tempfile::tempdir().unwrap();
        for child in ["ledger", "state", "clips"] {
            std::fs::create_dir(temporary.path().join(child)).unwrap();
        }
        let ledger_root = pinned(&temporary.path().join("ledger"));
        let state_root = pinned(&temporary.path().join("state"));
        let clip_root = pinned(&temporary.path().join("clips"));
        Self {
            _temporary: temporary,
            ledger_root,
            state_root,
            clip_root,
            quota,
            execution,
            client,
            resources,
            review,
            wake_sender,
            wake_receiver,
            request: RenderRequest {
                revision: 1,
                settings: AnimationSettings::default(),
                width: 1,
                height: 1,
                elapsed: Duration::ZERO,
                requested_at: Instant::now(),
                pointer: None,
                occupancy: None,
                occupancy_revision: 0,
            },
            archive,
        }
    }
    fn verified(&self, mode: AnimationMode) -> VerifiedPreparation {
        self.verified_with_policy(
            mode,
            Ceiling {
                permissions: Vec::new(),
            },
        )
    }
    fn verified_with_policy(
        &self,
        mode: AnimationMode,
        host_policy: Ceiling,
    ) -> VerifiedPreparation {
        let helper = std::env::var_os("ILIUM_ANIMATION_HELPER")
            .expect("matching release helper must be supplied explicitly");
        assert!(Path::new(&helper).is_absolute());
        let verifier = TrustVerifier::from_release_inventory(Vec::new()).unwrap();
        let settings = json!({});
        let environment = json!({"cell_width":1,"cell_height":1,"dot_width":2,"dot_height":4});
        let mut limits = HelperLimits::default();
        limits.engine.preparation_ms = limits.engine.preparation_ms.max(1000);
        PackageInstance::verify(InstancePreparation {
            archive: &self.archive,
            verifier: &verifier,
            helper_executable: Path::new(&helper),
            trusted_bootstrap: TRUSTED_BOOTSTRAP,
            settings: &settings,
            mode,
            environment: &environment,
            host_policy,
            instance_id: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
            limits,
            quota: self.quota.clone(),
        })
        .unwrap()
    }
    fn await_wake(&self) {
        self.wake_receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("original finite completion wake");
    }
    fn accepted_controller(&self) -> (PluginPermissionController, CreateState) {
        let sender = self.wake_sender.clone();
        let files = PluginPermissionFiles::new(
            &self.client,
            Arc::clone(&self.ledger_root),
            Arc::new(tokio::sync::Notify::new()),
            Arc::new(move || {
                let _ = sender.send(());
            }),
        )
        .unwrap();
        let mut controller = PluginPermissionController::new(files, &self.client).unwrap();
        controller
            .start(self.verified(AnimationMode::PreRendered), 1)
            .unwrap();
        let review = loop {
            self.await_wake();
            match controller.poll(1) {
                Some(ActivationUpdate::Review(review)) => break review,
                Some(ActivationUpdate::Failed { message, .. }) => panic!("{message}"),
                Some(ActivationUpdate::Finished(_)) => panic!("review was skipped"),
                None => {}
            }
        };
        let invalidation = controller.resolve(1, review, BTreeMap::new()).unwrap();
        apply_before_creation(invalidation).unwrap();
        let resolution = match controller.poll(1) {
            Some(ActivationUpdate::Finished(resolution)) => resolution,
            Some(ActivationUpdate::Failed { message, .. }) => panic!("{message}"),
            _ => panic!("source-free session resolution must be immediate"),
        };
        (
            controller,
            resolution.accepted_creation().expect("accepted creation"),
        )
    }
    fn ready_presentation(
        &self,
        instance: &mut PackageInstance,
        creation: CreateState,
    ) -> Presentation {
        let mut presentation = Presentation::new(
            instance,
            &self.request,
            &self.quota,
            self.resources.clone(),
            Arc::clone(&self.state_root),
            None, // These image/cache fixtures declare no audio demand.
            self.request
                .settings
                .plugin
                .selected
                .as_ref()
                .is_some_and(|selected| selected.mode == AnimationMode::Live),
            creation,
            Arc::new({
                let sender = self.wake_sender.clone();
                move || {
                    let _ = sender.send(());
                }
            }),
        )
        .unwrap();
        for _ in 0..100 {
            presentation.dispatch_requests(instance).unwrap();
            presentation.creation = instance.pump().unwrap();
            if presentation.creation == CreateState::Ready {
                presentation.dispatch_requests(instance).unwrap();
                return presentation;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("actual helper creation did not become ready");
    }
    fn accepted_direct(&self, mode: AnimationMode) -> (PackageInstance, Presentation) {
        let (mut instance, review) = self.verified(mode).prepare_without_rights().unwrap();
        let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
        let resolution = instance.finish_resolution(pending).unwrap();
        let presentation = self.ready_presentation(
            &mut instance,
            resolution.accepted_creation().expect("accepted creation"),
        );
        (instance, presentation)
    }
    fn accepted_spec(&self, instance: &PackageInstance) -> Arc<ClipSpec> {
        let verifier = ilium_animation_js::release::verifier().unwrap();
        let package = verifier.verify(instance.package());
        let frozen = FrozenInputs::from_host(
            self.quota.clone(),
            None,
            &[],
            Sha256::digest([]).into(),
            None,
            "procedural",
            &[],
        )
        .unwrap();
        let evidence = FrozenEvidence::from_native(self.quota.clone(), &package, &[]).unwrap();
        let appearance_digest: [u8; 32] =
            Sha256::digest(serde_json::to_vec(&self.request.settings).unwrap()).into();
        let spec = ClipSpec::from_accepted(
            &self.quota,
            ClipSpecification {
                package: instance.package(),
                verifier: &verifier,
                plan: instance.plan(),
                settings: instance.settings(),
                shape: shape(instance.plan(), self.request.width, self.request.height).unwrap(),
                backend: "native-v8",
                api_version: instance.package().manifest().api_version,
                appearance_digest,
                certification: instance.certify_procedural_replay().unwrap(),
                frozen,
                source_sequence: None,
                evidence,
            },
        )
        .unwrap();
        assert!(spec.can_stream_procedural());
        spec
    }
    fn store(&self) -> Arc<ClipChunkStore> {
        Arc::new(
            ClipChunkStore::new(
                Arc::clone(&self.clip_root),
                self.quota.clone(),
                2 * 1024 * 1024 * 1024,
            )
            .unwrap(),
        )
    }
    fn seed_disk(&self, spec: &ClipSpec) {
        let store = self.store();
        let mut writer = store
            .begin_procedural(&spec.key().hex(), spec.frame_count(), false)
            .unwrap();
        for _ in 0..spec.frame_count() {
            writer.push_frame(FRAME_BYTES).unwrap();
        }
        writer.finish().unwrap();
        assert_eq!(
            store
                .open_procedural(&spec.key().hex())
                .unwrap()
                .frame_count(),
            spec.frame_count()
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.execution.request_shutdown(ShutdownMode::Cancel);
        let joined = self
            .execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            joined.remaining_workers, 0,
            "fixture finite workers must exit"
        );
    }
}

#[derive(Debug)]
struct EmptyJob;
impl Job for EmptyJob {
    type Output = ();
    type Error = ();
    fn run(self, _: JobContext) -> Result<(), ()> {
        Ok(())
    }
}

struct QueueGateJob {
    entered: Sender<()>,
    release: Receiver<()>,
}
impl Job for QueueGateJob {
    type Output = ();
    type Error = ();
    fn run(self, _: JobContext) -> Result<(), ()> {
        self.entered.send(()).map_err(|_| ())?;
        self.release
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| ())?;
        Ok(())
    }
}
fn setup_retention(fixture: &Fixture) -> Retention {
    let mut receipt = fixture
        .client
        .try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: 1,
                result_bytes: 1,
            },
        )
        .unwrap()
        .submit(EmptyJob)
        .unwrap();
    loop {
        fixture.await_wake();
        if let JobPoll::Ready(outcome) = receipt.try_take() {
            let (outcome, retention) = outcome.into_parts();
            assert!(matches!(outcome, JobOutcome::Finished(Ok(()))));
            return retention;
        }
    }
}
fn backend_with_accepted_workflow(fixture: &Fixture) -> PluginBackend {
    let (mut controller, creation) = fixture.accepted_controller();
    let presentation =
        fixture.ready_presentation(controller.package_instance_mut().unwrap(), creation);
    let retention = setup_retention(fixture);
    let mut backend = PluginBackend::new(
        fixture.quota.clone(),
        fixture.resources.clone(),
        Arc::clone(&fixture.review),
        Arc::new(|| {}),
        Arc::new(tokio::sync::Notify::new()),
    );
    backend.workflow = Some(Workflow {
        revision: 1,
        request: fixture.request.clone(),
        controller,
        picker: None,
        audio_picker: None,
        audio_picker_uncertain: false,
        qualified_audio: None,
        presentation: Some(presentation),
        preparation_presentation: None,
        clip_root: Some(Arc::clone(&fixture.clip_root)),
        state_root: Arc::clone(&fixture.state_root),
        preparation: None,
        preparation_retention: None,
        preparation_stop: None,
        preparation_authority: None,
        player: None,
        playback_origin: None,
        last_playback_tick: None,
        last_playback_elapsed: None,
        playback_frozen: false,
        update: None,
        update_applied: true,
        cancellation: None,
        halted: false,
        _setup_retention: retention,
    });
    backend
}
fn await_producer(fixture: &Fixture, backend: &mut PluginBackend) -> Result<(), String> {
    for _ in 0..20 {
        fixture.await_wake();
        let result = backend.on_native_completion();
        if backend
            .workflow
            .as_ref()
            .is_none_or(|workflow| workflow.preparation.is_none())
        {
            return result;
        }
        result?;
    }
    Err("producer did not publish a terminal finite outcome".into())
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn cold_disk_index_retires_accepted_helper_before_player_publication() {
    let fixture = Fixture::new(PROCEDURAL_SCRIPT, AnimationMode::PreRendered);
    let mut backend = backend_with_accepted_workflow(&fixture);
    let spec = fixture.accepted_spec(
        backend
            .workflow
            .as_ref()
            .unwrap()
            .controller
            .package_instance()
            .unwrap(),
    );
    fixture.seed_disk(&spec);
    PluginBackend::start_preparation(
        backend.workflow.as_mut().unwrap(),
        &fixture.quota,
        &fixture.resources,
    )
    .unwrap();
    assert!(backend
        .workflow
        .as_ref()
        .unwrap()
        .preparation_presentation
        .is_some());
    await_producer(&fixture, &mut backend).unwrap();
    let workflow = backend.workflow.as_ref().unwrap();
    assert!(workflow.player.is_some());
    assert!(workflow.preparation_presentation.is_none());
    assert!(workflow
        .controller
        .delegated_instance()
        .unwrap()
        .lock()
        .unwrap()
        .is_physically_retired());
    backend.stop();
    backend.settle_retirement(true).unwrap();
    assert!(backend.is_physically_settled());
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
// Internal PreRenderJob branch: the current client makes one fresh ReplayCache
// per workflow, so public activation cannot yet reach this hit.
fn cached_preparation_branch_retires_second_actual_helper() {
    let prior = Fixture::new(PROCEDURAL_SCRIPT, AnimationMode::PreRendered);
    let (instance, presentation) = prior.accepted_direct(AnimationMode::PreRendered);
    let spec = prior.accepted_spec(&instance);
    let (authority, authorization) = instance.retain_procedural_replay_authorization().unwrap();
    prior.seed_disk(&spec);
    let prior_instance = Arc::new(Mutex::new(instance));
    let prior_presentation = Arc::new(Mutex::new(presentation));
    let prior_owner = PreRenderOwner {
        instance: Arc::clone(&prior_instance),
        presentation: Arc::clone(&prior_presentation),
        quota: prior.quota.clone(),
        stop: StopToken::default(),
        custody: Mutex::new(None),
    };
    prior_owner.retire().unwrap();
    let cache = Arc::new(ReplayCache::new(prior.quota.clone(), ReplayLimits::default()).unwrap());
    cache
        .open_procedural_from_disk(Arc::clone(&spec), authority, authorization, &prior.store())
        .unwrap();

    let current = Fixture::new(PROCEDURAL_SCRIPT, AnimationMode::PreRendered);
    let (instance, presentation) = current.accepted_direct(AnimationMode::PreRendered);
    let current_spec = current.accepted_spec(&instance);
    assert_eq!(spec.key(), current_spec.key());
    let (authority, authorization) = instance.retain_procedural_replay_authorization().unwrap();
    let current_instance = Arc::new(Mutex::new(instance));
    let current_presentation = Arc::new(Mutex::new(presentation));
    assert!(!current_instance.lock().unwrap().is_physically_retired());
    let mut receipt = current
        .client
        .try_reserve(
            Lane::Cpu,
            JobCost {
                input_bytes: 1024 * 1024,
                result_bytes: 1024 * 1024,
            },
        )
        .unwrap()
        .submit(PreRenderJob {
            request: current.request.clone(),
            instance: Arc::clone(&current_instance),
            presentation: Arc::clone(&current_presentation),
            cache,
            store: current.store(),
            spec: current_spec,
            authority,
            authorization,
            quota: current.quota.clone(),
            stop: StopToken::default(),
        })
        .unwrap();
    let outcome = loop {
        current.await_wake();
        if let JobPoll::Ready(outcome) = receipt.try_take() {
            break outcome;
        }
    };
    assert!(matches!(outcome.view(), JobOutcome::Finished(Ok(_))));
    assert!(current_instance.lock().unwrap().is_physically_retired());
    assert!(presentation_is_drained(
        &current_presentation.lock().unwrap()
    ));
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn failed_producer_keeps_original_workflow_until_native_retirement_wake() {
    let fixture = Fixture::new(FAILING_SCRIPT, AnimationMode::PreRendered);
    let mut backend = backend_with_accepted_workflow(&fixture);
    PluginBackend::start_preparation(
        backend.workflow.as_mut().unwrap(),
        &fixture.quota,
        &fixture.resources,
    )
    .unwrap();
    let owned_presentation = Arc::clone(
        backend
            .workflow
            .as_ref()
            .unwrap()
            .preparation_presentation
            .as_ref()
            .unwrap(),
    );
    // Cadence alone cannot discard the original producer/presentation.
    backend.settle_retirement(false).unwrap();
    assert!(backend.workflow.is_some());
    let error = await_producer(&fixture, &mut backend).unwrap_err();
    assert!(error.contains("producer_fixture_failure"), "{error}");
    if backend.workflow.is_some() {
        backend.settle_retirement(true).unwrap();
    }
    assert!(backend.is_physically_settled());
    assert!(presentation_is_drained(&owned_presentation.lock().unwrap()));
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn cancelled_queued_producer_retains_original_workflow_until_not_started_wake() {
    let fixture = Fixture::new(PROCEDURAL_SCRIPT, AnimationMode::PreRendered);
    let mut backend = backend_with_accepted_workflow(&fixture);
    let (entered_sender, entered_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let mut gate = fixture
        .client
        .try_reserve(
            Lane::Cpu,
            JobCost {
                input_bytes: 1,
                result_bytes: 1,
            },
        )
        .unwrap()
        .submit(QueueGateJob {
            entered: entered_sender,
            release: release_receiver,
        })
        .unwrap();
    entered_receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    PluginBackend::start_preparation(
        backend.workflow.as_mut().unwrap(),
        &fixture.quota,
        &fixture.resources,
    )
    .unwrap();
    let presentation = Arc::clone(
        backend
            .workflow
            .as_ref()
            .unwrap()
            .preparation_presentation
            .as_ref()
            .unwrap(),
    );
    backend.stop();
    backend.settle_retirement(false).unwrap();
    assert!(backend.workflow.is_some());
    assert!(backend.workflow.as_ref().unwrap().preparation.is_some());
    release_sender.send(()).unwrap();
    // The fixture gate is a real finite receipt too; consume its original
    // result rather than treating the producer wake as its completion.
    let gate_outcome = loop {
        fixture.await_wake();
        if let JobPoll::Ready(outcome) = gate.try_take() {
            break outcome;
        }
    };
    assert!(matches!(gate_outcome.view(), JobOutcome::Finished(Ok(()))));
    await_producer(&fixture, &mut backend).unwrap();
    if backend.workflow.is_some() {
        backend.settle_retirement(true).unwrap();
    }
    assert!(backend.is_physically_settled());
    assert!(presentation_is_drained(&presentation.lock().unwrap()));
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn decoded_image_allocation_survives_until_physical_helper_exit() {
    let bitmap = ImageBuffer::from_fn(1, 1, |_, _| Rgba([255_u8, 0, 0, 255]));
    let mut encoded = Cursor::new(Vec::new());
    bitmap.write_to(&mut encoded, ImageFormat::Png).unwrap();
    let literal = serde_json::to_string(&encoded.into_inner()).unwrap();
    let script = format!(
        "export function plan(){{return {{fps:30,output:{{mode:'pixels',format:'gray8',update:'replace'}},inputs:{{}},permissions:[]}}}} export async function create(host){{const raw=new Uint8Array({literal});const opened=await host.media.images.decode({{bytes:raw,max_pixels:4}});if(!opened.ok)throw Error('decode_fixture_failure');return {{render(context,frame){{frame.gray.fill(0);frame.present()}},dispose(){{}}}}}}"
    );
    let fixture = Fixture::new(&script, AnimationMode::Live);
    let (instance, presentation) = fixture.accepted_direct(AnimationMode::Live);
    assert!(!presentation.images.is_drained());
    let instance = Arc::new(Mutex::new(instance));
    let presentation = Arc::new(Mutex::new(presentation));
    let owner = PreRenderOwner {
        instance: Arc::clone(&instance),
        presentation: Arc::clone(&presentation),
        quota: fixture.quota.clone(),
        stop: StopToken::default(),
        custody: Mutex::new(None),
    };
    owner.retire().unwrap();
    assert!(instance.lock().unwrap().is_physically_retired());
    assert!(presentation_is_drained(&presentation.lock().unwrap()));
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_live_native_text_publication_keeps_wide_glyph_and_continuation() {
    // Exercise the public family C adapter through the real package review,
    // helper, retained activation, and terminal publication path.
    let script = "export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{}}} export async function create(host){return {render(context,frame){const result=host.text.spans({frame,x:0,y:0,max_cells:2,spans:[{text:'界',foreground:{r:255,g:0,b:0},background:{r:0,g:64,b:0},bold:true,italic:true,underline:true}]});if(!result.ok)throw Error(result.error.code);frame.present()},dispose(){}}}";
    let mut fixture = Fixture::new(script, AnimationMode::Live);
    fixture.request.width = 2;
    fixture.request.settings.source = crate::animation_plugins::AnimationSourceTab::Plugin;
    fixture.request.settings.plugin.selected = Some(crate::animation_plugins::PluginSelection {
        package_id: "native-replay-custody".into(),
        mode: AnimationMode::Live,
        settings: json!({}),
    });
    let (mut instance, mut presentation) = fixture.accepted_direct(AnimationMode::Live);
    let result = presentation.render(&mut instance, &fixture.request, 1, &StopToken::default());
    let instance = Arc::new(Mutex::new(instance));
    let presentation = Arc::new(Mutex::new(presentation));
    let owner = PreRenderOwner {
        instance: Arc::clone(&instance),
        presentation: Arc::clone(&presentation),
        quota: fixture.quota.clone(),
        stop: StopToken::default(),
        custody: Mutex::new(None),
    };
    // Retire physically before asserting, even when the observed failure is
    // publication: an assertion must not abandon the actual helper owner.
    owner.retire().unwrap();
    assert!(instance.lock().unwrap().is_physically_retired());
    assert!(presentation_is_drained(&presentation.lock().unwrap()));
    let frame = result
        .expect("actual native styled text must publish")
        .expect("accepted frame");
    assert_eq!(frame.cells.len(), 2);
    assert_eq!(frame.cells[0].article_symbol.as_deref(), Some("界"));
    assert_eq!(frame.cells[0].article_style, (true, true));
    assert_eq!(frame.cells[0].color, Some((255, 0, 0)));
    assert_eq!(frame.cells[0].article_background, Some((0, 64, 0)));
    assert!(frame.cells[0].article_underline);
    assert!(frame.cells[1].article_is_continuation);
}

fn await_text_producer(fixture: &Fixture, backend: &mut PluginBackend) -> Result<(), String> {
    for _ in 0..20 {
        fixture
            .wake_receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| format!("Original text clip completion wake: {error}"))?;
        let result = backend.on_native_completion();
        if backend
            .workflow
            .as_ref()
            .is_none_or(|workflow| workflow.preparation.is_none())
        {
            return result;
        }
        result?;
    }
    Err("Text clip producer did not publish a terminal finite outcome".into())
}

fn retire_text_backend(fixture: &Fixture, backend: &mut PluginBackend) -> Result<(), String> {
    backend.stop();
    for _ in 0..20 {
        backend.settle_retirement(false)?;
        if backend.is_physically_settled() {
            return Ok(());
        }
        fixture
            .wake_receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| format!("Original text helper retirement wake: {error}"))?;
        backend.settle_retirement(true)?;
        if backend.is_physically_settled() {
            return Ok(());
        }
    }
    Err("Original text helper physical settlement was not proved".into())
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_prerender_native_text_clip_disk_playback_and_revocation() {
    let mut fixture = Fixture::new(PRE_RENDER_NATIVE_TEXT_SCRIPT, AnimationMode::PreRendered);
    fixture.request.width = 2;
    fixture.request.settings.source = crate::animation_plugins::AnimationSourceTab::Plugin;
    fixture.request.settings.plugin.selected = Some(crate::animation_plugins::PluginSelection {
        package_id: "native-replay-custody".into(),
        mode: AnimationMode::PreRendered,
        settings: json!({}),
    });
    let mut backend = backend_with_accepted_workflow(&fixture);
    let (spec, clip_shape) = {
        let instance = backend
            .workflow
            .as_ref()
            .unwrap()
            .controller
            .package_instance()
            .unwrap();
        (
            fixture.accepted_spec(instance),
            shape(
                instance.plan(),
                fixture.request.width,
                fixture.request.height,
            )
            .unwrap(),
        )
    };
    // Gather outcomes without assertions. The original producer retires its
    // helper before exposing a player; cleanup is attempted before any check.
    let observed = (|| -> Result<_, String> {
        PluginBackend::start_preparation(
            backend
                .workflow
                .as_mut()
                .ok_or("Accepted workflow missing")?,
            &fixture.quota,
            &fixture.resources,
        )?;
        await_text_producer(&fixture, &mut backend)?;
        let workflow = backend.workflow.as_ref().ok_or("Clip workflow missing")?;
        let delegated = Arc::clone(
            workflow
                .controller
                .delegated_instance()
                .ok_or("Original delegated replay owner missing")?,
        );
        let helper_retired = delegated
            .lock()
            .map_err(|_| "Original replay owner poisoned")?
            .is_physically_retired();
        let player_ready = workflow.player.is_some();
        let reader = fixture
            .store()
            .open_procedural(&spec.key().hex())
            .map_err(|error| error.to_string())?;
        let disk_count = reader.frame_count();
        let disk_symbols = (0..disk_count)
            .map(|index| {
                reader
                    .procedural_frame(index, clip_shape)
                    .map(|frame| {
                        frame
                            .text
                            .into_iter()
                            .map(|text| text.text)
                            .collect::<Vec<_>>()
                    })
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut first_request = fixture.request.clone();
        first_request.elapsed = Duration::from_millis(1);
        first_request.requested_at = Instant::now();
        let first = backend
            .render(&first_request, 1, &StopToken::default())?
            .ok_or("First native text playback frame missing")?;
        let mut second_request = first_request.clone();
        second_request.elapsed = Duration::from_millis(501);
        second_request.requested_at += Duration::from_millis(500);
        let second = backend
            .render(&second_request, 2, &StopToken::default())?
            .ok_or("Second native text playback frame missing")?;
        let first_receipt = first
            .replay
            .as_ref()
            .ok_or("First original playback lease missing")?
            .receipt()
            .frame;
        let second_receipt = second
            .replay
            .as_ref()
            .ok_or("Second original playback lease missing")?
            .receipt()
            .frame;
        let first_lease_symbols = first
            .replay
            .as_ref()
            .ok_or("First original playback lease missing")?
            .text()
            .map_err(|error| error.to_string())?
            .iter()
            .map(|text| text.text.clone())
            .collect::<Vec<_>>();
        let second_lease_symbols = second
            .replay
            .as_ref()
            .ok_or("Second original playback lease missing")?
            .text()
            .map_err(|error| error.to_string())?
            .iter()
            .map(|text| text.text.clone())
            .collect::<Vec<_>>();
        let revoked = delegated
            .lock()
            .map_err(|_| "Original replay owner poisoned")?
            .revoke_activation()
            .map_err(|error| error.to_string())?
            .is_some();
        let lease_denied = first
            .replay
            .as_ref()
            .is_some_and(|lease| lease.text().is_err());
        let output_denied = second
            .authority
            .begin_output(&HelperAuthority {
                package_digest: second.identity.package_digest.clone(),
                instance_id: second.identity.instance_id,
                plan_generation: second.identity.plan_generation,
                authorization_epoch: second.identity.authorization_epoch,
            })
            .is_err();
        let fresh_playback_denied = backend
            .render(&second_request, 3, &StopToken::default())
            .is_err();
        Ok((
            helper_retired,
            player_ready,
            disk_count,
            disk_symbols,
            first_receipt,
            second_receipt,
            first_lease_symbols,
            second_lease_symbols,
            first,
            second,
            revoked,
            lease_denied,
            output_denied,
            fresh_playback_denied,
        ))
    })();
    let retirement = retire_text_backend(&fixture, &mut backend);
    retirement.expect("Actual text clip owner must physically settle after the scenario");
    let (
        helper_retired,
        player_ready,
        disk_count,
        disk_symbols,
        first_receipt,
        second_receipt,
        first_lease_symbols,
        second_lease_symbols,
        first,
        second,
        revoked,
        lease_denied,
        output_denied,
        fresh_playback_denied,
    ) = observed.expect("Accepted text clip must produce genuine disk and playback results");
    assert!(helper_retired && player_ready);
    assert_eq!(disk_count, 2);
    assert_eq!(disk_symbols, vec![vec!["界"], vec!["e\u{301}"]]);
    assert_eq!((first_receipt, second_receipt), (0, 1));
    assert_eq!(first_lease_symbols, vec!["界"]);
    assert_eq!(second_lease_symbols, vec!["e\u{301}"]);
    assert_eq!(first.cells[0].article_symbol.as_deref(), Some("界"));
    assert!(first.cells[1].article_is_continuation);
    assert_eq!(first.cells[0].article_background, Some((0, 64, 0)));
    assert!(first.cells[0].article_underline);
    assert_eq!(second.cells[0].article_symbol.as_deref(), Some("e\u{301}"));
    assert!(!second.cells[1].article_is_continuation);
    assert!(revoked && lease_denied && output_denied && fresh_playback_denied);
}

#[test]
#[ignore = "requires matching ILIUM_ANIMATION_HELPER, installed FFmpeg and delegated Linux codec sandbox"]
fn actual_recorded_bundle_video_replay_keeps_source_pixels_until_decoder_and_helper_retire() {
    let mut fixture = Fixture::new(RECORDED_BUNDLE_VIDEO_SCRIPT, AnimationMode::PreRendered);
    fixture.archive = recorded_video_archive(RECORDED_BUNDLE_VIDEO_SCRIPT);
    fixture.request.settings.source = crate::animation_plugins::AnimationSourceTab::Plugin;
    fixture.request.settings.plugin.selected = Some(crate::animation_plugins::PluginSelection {
        package_id: "native-replay-custody".into(),
        mode: AnimationMode::PreRendered,
        settings: json!({}),
    });
    let mut backend = backend_with_accepted_workflow(&fixture);
    let recorded = backend
        .workflow
        .as_ref()
        .unwrap()
        .presentation
        .as_ref()
        .unwrap()
        .video
        .recorded_count();
    assert_eq!(
        recorded, 1,
        "one actual bundle Video open must settle during create"
    );
    PluginBackend::start_preparation(
        backend.workflow.as_mut().unwrap(),
        &fixture.quota,
        &fixture.resources,
    )
    .unwrap();
    let outcome = (|| -> Result<_, String> {
        await_producer(&fixture, &mut backend)?;
        let workflow = backend
            .workflow
            .as_ref()
            .ok_or("Recorded workflow missing")?;
        let delegated = Arc::clone(
            workflow
                .controller
                .delegated_instance()
                .ok_or("Original recorded helper owner missing")?,
        );
        let physically_retired = delegated
            .lock()
            .map_err(|_| "Recorded helper owner poisoned")?
            .is_physically_retired();
        let player_ready = workflow.player.is_some();
        let mut request = fixture.request.clone();
        request.elapsed = Duration::from_millis(1);
        request.requested_at = Instant::now();
        let frame = backend
            .render(&request, 1, &StopToken::default())?
            .ok_or("Recorded playback frame missing")?;
        let lease = frame
            .replay
            .as_ref()
            .ok_or("Recorded playback lease missing")?;
        let has_source_pixels = lease
            .packed()
            .map_err(|error| error.to_string())?
            .owners
            .iter()
            .any(Option::is_some);
        let first_frame = lease.receipt().frame == 0;
        let playback_shape = {
            let instance = delegated.lock().map_err(|_| "Recorded owner poisoned")?;
            shape(instance.plan(), request.width, request.height)?
        };
        // A source-bearing packed buffer alone cannot authorize publication.
        assert!(cells_from_packed(
            playback_shape,
            lease.packed().map_err(|error| error.to_string())?,
            &request.settings,
            Duration::ZERO,
        )
        .is_err());
        let decoded = cells_from_replay(playback_shape, lease, &request.settings, Duration::ZERO)?;
        assert_eq!(decoded.len(), frame.cells.len());
        assert_eq!(
            decoded
                .iter()
                .map(|cell| cell.packed_bits)
                .collect::<Vec<_>>(),
            frame
                .cells
                .iter()
                .map(|cell| cell.packed_bits)
                .collect::<Vec<_>>(),
        );
        let revoked = delegated
            .lock()
            .map_err(|_| "Recorded helper owner poisoned")?
            .revoke_activation()
            .map_err(|error| error.to_string())?
            .is_some();
        let stale_lease_denied = lease.packed().is_err();
        assert!(
            cells_from_replay(playback_shape, lease, &request.settings, Duration::ZERO,).is_err(),
            "revoked source authority must also deny cell conversion"
        );
        Ok((
            physically_retired,
            player_ready,
            has_source_pixels,
            first_frame,
            revoked,
            stale_lease_denied,
        ))
    })();
    retire_text_backend(&fixture, &mut backend)
        .expect("Actual recorded Video helper and decoder must physically settle");
    let (
        physically_retired,
        player_ready,
        has_source_pixels,
        first_frame,
        revoked,
        stale_lease_denied,
    ) = outcome.expect("Recorded Video must reach RAM playback");
    assert!(physically_retired && player_ready && has_source_pixels && first_frame);
    assert!(revoked && stale_lease_denied);
}

// Actual helper denial checks use the same accepted-activation fixture.
include!("video_negative_tests.rs");

// Public selected Video travels through the real review and admitted picker.
include!("video_selected_tests.rs");
