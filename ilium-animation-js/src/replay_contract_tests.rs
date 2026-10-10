//! Synthetic fixtures exercise pure replay admission, cache, playback and
//! history algorithms. They bypass production certification through private
//! ClipSpec construction; protected-source integration remains unverified.
use super::*;
use crate::{
    engine::{ArraySpec, EngineLimits, ServiceValue, TypedArrayKind},
    error::{AnimationError, Result},
    helper::HelperAuthority,
    manifest::AnimationMode,
    package::{Package, PackageLimits},
    permissions::{Ceiling, Channel, PermissionBroker, PermissionPlan},
    plan::{AnimationPlan, PlanBudget},
    runtime::RetainedFrameAuthority,
    surface::{
        self, Blend, ColourSpace, Command, Data, Format, FrameMeta, Mode, NativeOutput,
        NativePatch, NativeRenderer, NoNativeRenderer, Planes, Rect, Shape, SourceToken, Surface,
        Update,
    },
    trust::TrustVerifier,
};
use ilium_execution::{QuotaGroup, QuotaLimits, StorageAdmission};
use ilium_platform::owned_worker::StopToken;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
fn root(bytes: usize) -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 0,
        worker_bytes: bytes,
    })
}
fn package() -> Package {
    let entry = b"export const fixture = true;";
    let manifest = json!({"api_version":1,"id":"replay-contract","name":"Replay contract","version":"1.0.0","entry":"entry.mjs","modes":["live","pre_rendered"],"settings":{"type":"object","properties":{}},"files":[{"path":"entry.mjs","bytes":entry.len(),"sha256":format!("{:x}",Sha256::digest(entry))}]});
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("entry.mjs", options).unwrap();
    zip.write_all(entry).unwrap();
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    Package::from_bytes(
        &zip.finish().unwrap().into_inner(),
        PackageLimits::default(),
    )
    .unwrap()
}
fn shape() -> Shape {
    Shape {
        cell_width: 1,
        cell_height: 1,
        mode: Mode::Cells,
        format: Format::Mask8,
        update: Update::Replace,
        cell_rgb: false,
        colour_space: ColourSpace::Srgb,
    }
}
fn plan(seamless: bool) -> AnimationPlan {
    AnimationPlan::parse(&json!({"fps":2,"output":{"mode":"cells","format":"mask8","update":"replace"},"inputs":{},"replay":{"seed":3,"duration_seconds":1,"seamless":seamless}}),AnimationMode::PreRendered,PlanBudget::default()).unwrap()
}
#[test]
fn pre_render_plan_can_bind_recording_created_during_create() {
    let parsed = AnimationPlan::parse(
        &json!({
            "fps": 2,
            "output": {"mode":"cells","format":"mask8","update":"replace"},
            "inputs": {"pointer":{"max_hz":1}},
            "replay": {"seed":3,"duration_seconds":1,"seamless":false}
        }),
        AnimationMode::PreRendered,
        PlanBudget::default(),
    );

    assert!(
        parsed.is_ok(),
        "the host recording is created during create(), after plan() returns"
    );
}
#[derive(Default)]
struct History(Mutex<Vec<(u64, Vec<usize>)>>);
impl ReplayHistory for History {
    fn emitted(
        &self,
        _: [u8; 32],
        payload: &[u8],
        receipt: &ReplayReceipt,
        dots: &[usize],
    ) -> Result<()> {
        assert_eq!(payload, b"native fixture evidence");
        self.0
            .lock()
            .unwrap()
            .push((receipt.lease_sequence, dots.to_vec()));
        Ok(())
    }
}

struct OrderedHistory {
    marker: u8,
    fail: bool,
    calls: Arc<Mutex<Vec<u8>>>,
}
impl ReplayHistory for OrderedHistory {
    fn emitted(&self, _: [u8; 32], _: &[u8], _: &ReplayReceipt, _: &[usize]) -> Result<()> {
        self.calls.lock().unwrap().push(self.marker);
        if self.fail {
            return Err(failure("injected uncertain history settlement"));
        }
        Ok(())
    }
}
struct Fixture {
    quota: QuotaGroup,
    package: Package,
    verifier: TrustVerifier,
    evidence: Arc<FrozenEvidence>,
    frozen: Arc<FrozenInputs>,
    history: Arc<History>,
}
impl Fixture {
    fn new(protected: bool, bytes: usize) -> Self {
        let quota = root(bytes);
        let package = package();
        let verifier = TrustVerifier::from_release_inventory(vec![]).unwrap();
        let history = Arc::new(History::default());
        let evidence = FrozenEvidence::from_native(
            quota.clone(),
            &verifier.verify(&package),
            &if protected {
                vec![NativeEvidenceInput {
                    source_digest: [9; 32],
                    lineage: vec![GrantLineage {
                        request_id: "bundle".into(),
                        capability: "images.saved".into(),
                        scope_digest: [8; 32],
                    }],
                    payload: b"native fixture evidence".to_vec(),
                    history: history.clone(),
                }]
            } else {
                vec![]
            },
        )
        .unwrap();
        let frozen = FrozenInputs::from_host(
            quota.clone(),
            None,
            &[],
            [5; 32],
            None,
            "synthetic frozen fixture",
            &[],
        )
        .unwrap();
        Self {
            quota,
            package,
            verifier,
            evidence,
            frozen,
            history,
        }
    }
    fn spec(&self, settings: &Value, seamless: bool) -> Arc<ClipSpec> {
        self.spec_with(&plan(seamless), settings, shape(), self.frozen.clone())
            .unwrap()
    }
    // Pure, module-private playback fixture: bypasses production certification
    // instead of pretending a synthetic digest came from a sealed V8 helper.
    // The negative public admission tests below exercise the real issuer gate.
    fn spec_with(
        &self,
        plan: &AnimationPlan,
        settings: &Value,
        shape: Shape,
        frozen: Arc<FrozenInputs>,
    ) -> Result<Arc<ClipSpec>> {
        if shape.mode != Mode::Cells
            || shape.format != Format::Mask8
            || shape.update != Update::Replace
        {
            return Err(failure("fixture output shape mismatch"));
        }
        let replay = plan
            .replay
            .as_ref()
            .ok_or_else(|| failure("fixture replay absent"))?;
        let required_pointer = plan.inputs.pointer.is_some();
        if required_pointer
            && (!frozen.families.contains(&InputFamily::Pointer)
                || replay.input_recording.as_deref() != frozen.recording.as_deref())
        {
            return Err(failure("fixture input recording absent"));
        }
        let fps = replay.fps.unwrap_or(plan.fps);
        let frames = (fps * replay.duration_seconds).ceil() as usize;
        let package = self.verifier.verify(&self.package);
        let mut lineage = frozen.lineage.clone();
        for entry in self.evidence.entries.values() {
            for item in &entry.lineage {
                if !lineage.contains(item) {
                    lineage.push(item.clone());
                }
            }
        }
        validate_lineage(&lineage)?;
        let material = json!({"package":package.digest(), "plan":plan,
            "settings":settings, "shape":shape, "frozen":frozen.digest,
            "evidence":self.evidence.digest});
        let key = ClipKey(Sha256::digest(bounded_json(&material)?).into());
        Ok(Arc::new(ClipSpec {
            key,
            package,
            shape,
            fps,
            duration: replay.duration_seconds,
            frames,
            seamless: replay.seamless,
            frozen,
            source_sequence: None,
            evidence: self.evidence.clone(),
            lineage,
            recorded_video: false, // This synthetic fixture performs no recorded native video open.
            _metadata: reserve(&self.quota, 1024 * 1024)?,
        }))
    }
    fn authority(&self) -> ReplayAuthority {
        ReplayAuthority {
            package_digest: self.package.digest().into(),
            instance_id: 1,
            revision: 1,
            authorization_epoch: 1,
        }
    }
}
#[test]
fn clips_declaring_frozen_source_families_cannot_use_procedural_storage() {
    let fixture = Fixture::new(false, 32 << 20);
    let frozen = FrozenInputs::from_host(
        fixture.quota.clone(),
        None,
        &[InputFamily::Weather],
        [6; 32],
        None,
        "frozen source family",
        &[],
    )
    .unwrap();
    let spec = fixture
        .spec_with(&plan(false), &json!({}), shape(), frozen)
        .unwrap();

    assert!(
        !spec.can_stream_procedural(),
        "declared frozen source families are not source-free procedural output"
    );
}
struct Authorization {
    epoch: AtomicU64,
    revoked: AtomicBool,
    broker: Arc<Mutex<PermissionBroker>>,
    channel: Channel,
    expected: ReplayAuthority,
}
impl Authorization {
    fn new(fixture: &Fixture) -> Arc<Self> {
        let empty = Ceiling {
            permissions: vec![],
        };
        let mut broker = PermissionBroker::new(
            fixture
                .verifier
                .permission_identity(&fixture.package)
                .unwrap(),
            empty.clone(),
            empty,
        )
        .unwrap();
        let review = broker
            .prepare(
                1,
                1,
                PermissionPlan {
                    permissions: vec![],
                    demands: vec![],
                },
                BTreeMap::new(),
            )
            .unwrap();
        let activation = broker
            .resolve(review, BTreeMap::new())
            .unwrap()
            .activation
            .unwrap();
        let expected = ReplayAuthority {
            package_digest: fixture.package.digest().into(),
            instance_id: activation.plan.instance_id,
            revision: activation.plan.plan_revision,
            authorization_epoch: activation.plan.authorization_epoch,
        };
        Arc::new(Self {
            epoch: AtomicU64::new(expected.authorization_epoch),
            revoked: AtomicBool::new(false),
            broker: Arc::new(Mutex::new(broker)),
            channel: activation.channel,
            expected,
        })
    }
    // This is only a broker/order fixture. The client Presenter's real backend
    // draw and flush tests remain the terminal acceptance path.
    fn flushed_proof(&self, fixture: &Fixture) -> Result<ReplayFlushedProof> {
        self.check(&fixture.authority(), &[], ReplayAccess::Emission)?;
        let frame = HelperAuthority {
            package_digest: fixture.package.digest().into(),
            instance_id: self.expected.instance_id,
            plan_generation: self.expected.revision,
            authorization_epoch: self.expected.authorization_epoch,
        };
        let retained = RetainedFrameAuthority::from_active_test_channel(
            Arc::clone(&self.broker),
            self.channel.clone(),
            frame.clone(),
            fixture.quota.clone(),
        )?;
        let committed = retained.begin_output(&frame)?;
        let mut sink = Cursor::new(Vec::new());
        if let Err(error) = sink
            .write_all(b"fixture host output")
            .and_then(|_| sink.flush())
        {
            committed.backend_failed_uncertain()?;
            return Err(error.into());
        }
        assert_eq!(sink.into_inner(), b"fixture host output");
        committed.backend_flushed()
    }
}
impl ReplayAuthorization for Authorization {
    fn check(
        &self,
        authority: &ReplayAuthority,
        _: &[GrantLineage],
        _: ReplayAccess,
    ) -> Result<()> {
        if self.revoked.load(Ordering::Acquire)
            || self.epoch.load(Ordering::Acquire) != authority.authorization_epoch
            || authority != &self.expected
        {
            return Err(AnimationError::PermissionDenied("fixture revoked".into()));
        }
        let coordinates = self
            .broker
            .lock()
            .unwrap()
            .channel_coordinates(&self.channel)
            .map_err(|error| AnimationError::PermissionDenied(error.to_string()))?;
        if coordinates
            != (
                authority.instance_id,
                authority.revision,
                authority.authorization_epoch,
            )
        {
            return Err(AnimationError::PermissionDenied(
                "fixture channel changed".into(),
            ));
        }
        Ok(())
    }
    fn validate_flushed(
        &self,
        authority: &ReplayAuthority,
        proof: &ReplayFlushedProof,
    ) -> Result<()> {
        if authority != &self.expected || !proof.belongs_to(&self.broker, authority) {
            return Err(AnimationError::PermissionDenied(
                "foreign fixture flush proof".into(),
            ));
        }
        Ok(())
    }
}
struct Owner {
    quota: QuotaGroup,
    charge: Mutex<Option<Arc<StorageAdmission>>>,
    cancelled: AtomicBool,
    retired: AtomicBool,
    refuse_retire: AtomicBool,
}
impl Owner {
    fn new(quota: &QuotaGroup) -> Arc<Self> {
        Arc::new(Self {
            quota: quota.clone(),
            charge: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            refuse_retire: AtomicBool::new(false),
        })
    }
    fn actual_exit(&self) {
        self.retired.store(true, Ordering::Release);
        self.charge.lock().unwrap().take();
    }
}
impl ReplayPreparationOwner for Owner {
    fn quota_group(&self) -> QuotaGroup {
        self.quota.clone()
    }
    fn bind_retirement_custody(&self, storage: Arc<StorageAdmission>) {
        *self.charge.lock().unwrap() = Some(storage);
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    fn retire(&self) -> Result<()> {
        if self.refuse_retire.load(Ordering::Acquire) {
            return Err(AnimationError::Runtime(
                "fixture worker still executing".into(),
            ));
        }
        self.actual_exit();
        Ok(())
    }
    fn is_retired(&self) -> bool {
        self.retired.load(Ordering::Acquire)
    }
}
fn begin(
    cache: &ReplayCache,
    f: &Fixture,
    spec: Arc<ClipSpec>,
    auth: Arc<Authorization>,
    owner: Arc<Owner>,
) -> ClipPreparation {
    match cache
        .begin(spec, f.authority(), auth, StopToken::default(), || {
            Ok(owner)
        })
        .unwrap()
    {
        Preparation::Started(prep) => *prep,
        _ => panic!("fixture expected fresh preparation"),
    }
}
struct Renderer(Option<SourceToken>, u8);
impl NativeRenderer for Renderer {
    fn render(
        &mut self,
        command: &Command,
        _: Shape,
        _: usize,
    ) -> std::result::Result<NativeOutput, surface::SurfaceError> {
        let Command::Blit { target, .. } = command else {
            panic!("fixture command")
        };
        Ok(NativeOutput {
            patches: vec![NativePatch {
                rect: *target,
                data: Data::U8(vec![self.1]),
                state: vec![2],
                owners: vec![self.0; 8],
            }],
            ..NativeOutput::default()
        })
    }
}

struct SplitRenderer {
    owners: Vec<Option<SourceToken>>,
    mask: u8,
}
impl NativeRenderer for SplitRenderer {
    fn render(
        &mut self,
        command: &Command,
        _: Shape,
        _: usize,
    ) -> std::result::Result<NativeOutput, surface::SurfaceError> {
        let Command::Blit { target, .. } = command else {
            panic!("split fixture command")
        };
        Ok(NativeOutput {
            patches: vec![NativePatch {
                rect: *target,
                data: Data::U8(vec![self.mask]),
                state: vec![2],
                owners: self.owners.clone(),
            }],
            ..NativeOutput::default()
        })
    }
}
fn split_snapshot(mask: u8, first: SourceToken, second: SourceToken) -> Surface {
    let mut surface = Surface::new(1, 1, shape()).unwrap();
    let seed = surface.begin(1).unwrap();
    let meta = FrameMeta {
        wire_version: 1,
        key: seed.key,
        shape: shape(),
        presented: true,
        error: None,
        commands: vec![Command::Blit {
            order: 2,
            handle: "split-fixture".into(),
            source: Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            target: Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            blend: Blend::Overwrite,
        }],
    };
    let planes = Planes {
        data: Data::U8(vec![mask]),
        touch: vec![0],
        order: vec![0],
        cell_rgb: None,
        colour_touch: None,
        colour_order: None,
    };
    let mut owners = vec![Some(first); 4];
    owners.extend([Some(second); 4]);
    surface
        .finish(meta, planes, &mut SplitRenderer { owners, mask })
        .unwrap();
    surface
}
fn snapshot(mask: u8, token: Option<SourceToken>) -> Surface {
    let mut surface = Surface::new(1, 1, shape()).unwrap();
    let seed = surface.begin(1).unwrap();
    let mut meta = FrameMeta {
        wire_version: 1,
        key: seed.key,
        shape: shape(),
        presented: true,
        error: None,
        commands: vec![],
    };
    let mut planes = Planes {
        data: Data::U8(vec![mask]),
        touch: vec![1],
        order: vec![1],
        cell_rgb: None,
        colour_touch: None,
        colour_order: None,
    };
    if token.is_some() {
        meta.commands.push(Command::Blit {
            order: 2,
            handle: "fixture".into(),
            source: Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            target: Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            blend: Blend::Overwrite,
        });
        planes.touch[0] = 0;
        planes.order[0] = 0;
        surface
            .finish(meta, planes, &mut Renderer(token, mask))
            .unwrap();
    } else {
        surface.finish(meta, planes, &mut NoNativeRenderer).unwrap();
    }
    surface
}
fn push(prep: &mut ClipPreparation, mask: u8, token: Option<SourceToken>) {
    let ticket = prep.next_sample().unwrap();
    let surface = snapshot(mask, token);
    prep.push_snapshot(ticket, surface.snapshot(), |v, _, _| v, |v, _, _| v >= 0.5)
        .unwrap();
}
fn prepare(
    f: &Fixture,
    cache: &ReplayCache,
    auth: Arc<Authorization>,
    token: Option<SourceToken>,
) -> Arc<ReplayClip> {
    let spec = f.spec(&json!({}), false);
    let mut prep = begin(cache, f, spec, auth, Owner::new(&f.quota));
    push(&mut prep, 1, token);
    push(&mut prep, 255, token);
    prep.finish().unwrap()
}
fn player(
    f: &Fixture,
    clip: Arc<ReplayClip>,
    auth: Arc<Authorization>,
    stop: StopToken,
) -> ReplayPlayer {
    ReplayPlayer::new(
        clip,
        f.authority(),
        auth,
        PlaybackSettings {
            now: Duration::ZERO,
            speed: 1.0,
            mode: PlaybackMode::Repeat,
            max_leases: 3,
            stop,
        },
    )
    .unwrap()
}
fn lease(player: &mut ReplayPlayer, time: f64) -> PlaybackLease {
    match player.sample(Duration::from_secs_f64(time)).unwrap().0 {
        Playback::Frame(lease) => lease,
        _ => panic!("fixture expected frame"),
    }
}
#[test]
fn complete_clip_retirement_cache_singleflight_and_zero_acquisition_playback() {
    let f = Fixture::new(false, 32 << 20);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let auth = Authorization::new(&f);
    let spec = f.spec(&json!({}), false);
    let owner = Owner::new(&f.quota);
    let mut prep = begin(&cache, &f, spec.clone(), auth.clone(), owner.clone());
    assert!(matches!(
        cache
            .begin(
                spec.clone(),
                f.authority(),
                auth.clone(),
                StopToken::default(),
                || panic!("must not acquire twice")
            )
            .unwrap(),
        Preparation::InProgress
    ));
    let sample = prep.next_sample().unwrap();
    assert_eq!((sample.time, sample.wall, sample.delta), (0., 0., 0.));
    let surface = snapshot(1, None);
    prep.push_snapshot(sample, surface.snapshot(), |v, _, _| v, |v, _, _| v > 0.)
        .unwrap();
    push(&mut prep, 255, None);
    let clip = prep.finish().unwrap();
    assert!(owner.is_retired());
    assert_eq!(clip.frame_count(), 2);
    assert!(matches!(
        cache
            .begin(
                spec,
                f.authority(),
                auth.clone(),
                StopToken::default(),
                || panic!("cached playback must not load V8")
            )
            .unwrap(),
        Preparation::Cached(_)
    ));
    let mut player = player(&f, clip, auth, StopToken::default());
    assert_eq!(lease(&mut player, 0.).packed().unwrap().masks, vec![1]);
    assert_eq!(lease(&mut player, 0.75).packed().unwrap().masks, vec![255]);
    assert_eq!(lease(&mut player, 1.).packed().unwrap().masks, vec![1]);
}
#[test]
fn full_admission_precedes_owner_factory_and_finite_count_is_enforced() {
    let f = Fixture::new(false, 2 << 20);
    let spec = f.spec(&json!({}), false);
    let limits = ReplayLimits {
        max_frames: 1,
        ..ReplayLimits::default()
    };
    let cache = ReplayCache::new(f.quota.clone(), limits).unwrap();
    assert!(cache
        .begin(
            spec.clone(),
            f.authority(),
            Authorization::new(&f),
            StopToken::default(),
            || panic!("must reject frame cap first")
        )
        .is_err());
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let remaining = (2 << 20) - f.quota.snapshot().worker_bytes;
    let occupied = f.quota.reserve_external_storage(remaining - 1).unwrap();
    assert!(cache
        .begin(
            spec,
            f.authority(),
            Authorization::new(&f),
            StopToken::default(),
            || panic!("must admit full clip first")
        )
        .is_err());
    drop(occupied);
}
#[test]
fn partial_and_stale_preparation_retain_custody_until_actual_exit() {
    let f = Fixture::new(false, 32 << 20);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let auth = Authorization::new(&f);
    let spec = f.spec(&json!({}), false);
    let owner = Owner::new(&f.quota);
    let mut prep = begin(&cache, &f, spec.clone(), auth.clone(), owner.clone());
    push(&mut prep, 1, None);
    let charged = f.quota.snapshot().worker_bytes;
    let mut current = f.authority();
    current.revision = 2;
    cache.invalidate_preparations(&current).unwrap();
    assert!(prep.next_sample().is_err());
    drop(prep);
    assert!(owner.cancelled.load(Ordering::Acquire));
    assert!(!cache.contains(spec.key()).unwrap());
    assert!(f.quota.snapshot().worker_bytes > charged - 100000);
    assert!(matches!(
        cache
            .begin(
                spec.clone(),
                f.authority(),
                auth,
                StopToken::default(),
                || panic!("retiring preparation retains slot")
            )
            .unwrap(),
        Preparation::InProgress
    ));
    owner.actual_exit();
    cache.collect_retired().unwrap();
    assert!(f.quota.snapshot().worker_bytes < charged);
}
#[test]
fn failed_retirement_cannot_publish_a_clip() {
    let f = Fixture::new(false, 32 << 20);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let spec = f.spec(&json!({}), false);
    let owner = Owner::new(&f.quota);
    owner.refuse_retire.store(true, Ordering::Release);
    let mut prep = begin(
        &cache,
        &f,
        spec.clone(),
        Authorization::new(&f),
        owner.clone(),
    );
    push(&mut prep, 1, None);
    push(&mut prep, 2, None);
    assert!(prep.finish().is_err());
    assert!(!cache.contains(spec.key()).unwrap());
    assert!(owner.cancelled.load(Ordering::Acquire));
    owner.actual_exit();
    cache.collect_retired().unwrap();
}
#[test]
fn frozen_identity_settings_shape_certification_and_live_input_rules() {
    let f = Fixture::new(false, 32 << 20);
    let a = f.spec(&json!({"a":1,"b":2}), false);
    let b = f.spec(&json!({"b":2,"a":1}), false);
    assert_eq!(a.key(), b.key());
    assert_ne!(a.key(), f.spec(&json!({"a":2,"b":2}), false).key());
    let mut altered = shape();
    altered.update = Update::Retain;
    assert!(f
        .spec_with(&plan(false), &json!({}), altered, f.frozen.clone())
        .is_err());
    // Public callers cannot mutate a native certificate. The private issuer
    // refuses a missing loaded helper identity before any spec is accepted.
    assert!(ReplayCertification::sealed_procedural(
        &f.package,
        &plan(false),
        &json!({}),
        ReplayExecutionIdentity {
            ambient_seed: 3,
            bootstrap_digest: [1; 32],
            environment_digest: [2; 32],
            helper_build_digest: [0; 32],
        },
        0
    )
    .is_err());
    let mut live = plan(false);
    live.inputs.pointer = Some(crate::plan::RateDemand { max_hz: 2. });
    assert!(f
        .spec_with(&live, &json!({}), shape(), f.frozen.clone())
        .is_err());
    live.replay.as_mut().unwrap().input_recording = Some("recording".into());
    let frozen = FrozenInputs::from_host(
        f.quota.clone(),
        Some("recording"),
        &[InputFamily::Pointer],
        [7; 32],
        None,
        "fixture capture",
        &[],
    )
    .unwrap();
    assert!(f.spec_with(&live, &json!({}), shape(), frozen).is_ok());
    // This pure frozen-input fixture is not a source-bearing native grant.
    assert!(ReplayCertification::sealed_procedural(
        &f.package,
        &live,
        &json!({}),
        ReplayExecutionIdentity {
            ambient_seed: 3,
            bootstrap_digest: [1; 32],
            environment_digest: [2; 32],
            helper_build_digest: [3; 32],
        },
        0
    )
    .is_err());
}
#[test]
fn history_only_actual_surviving_emission_and_proof_outlives_player_cache() {
    let f = Fixture::new(true, 32 << 20);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let auth = Authorization::new(&f);
    let token = f.evidence.token(0).unwrap();
    let clip = prepare(&f, &cache, auth.clone(), Some(token));
    assert!(f.history.0.lock().unwrap().is_empty());
    let key = clip.key();
    let mut player = player(&f, clip, auth.clone(), StopToken::default());
    let first = lease(&mut player, 0.75);
    let sequence = first.receipt().lease_sequence;
    let mut first_pending = first.prepare_emission(&[0]).unwrap();
    first_pending
        .after_host_emission(auth.flushed_proof(&f).unwrap())
        .unwrap();
    assert!(first_pending.settle().is_ok());
    assert!(f.history.0.lock().unwrap().is_empty());
    let second = lease(&mut player, 0.75);
    assert_ne!(sequence, second.receipt().lease_sequence);
    let mut pending = second.prepare_emission(&[1 | 128]).unwrap();
    assert!(f.history.0.lock().unwrap().is_empty());
    pending
        .after_host_emission(auth.flushed_proof(&f).unwrap())
        .unwrap();
    cache.evict(key).unwrap();
    drop(player);
    drop(cache);
    auth.revoked.store(true, Ordering::Release);
    assert!(pending.settle().is_ok());
    let events = f.history.0.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, vec![0, 7]);
}
#[test]
fn invalid_flush_proof_stays_with_same_owner_and_keeps_proof_admission() {
    let f = Fixture::new(true, 32 << 20);
    let foreign = Fixture::new(true, 32 << 20);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let auth = Authorization::new(&f);
    let foreign_auth = Authorization::new(&foreign);
    let clip = prepare(&f, &cache, auth, Some(f.evidence.token(0).unwrap()));
    let mut player = player(&f, clip, Authorization::new(&f), StopToken::default());
    let mut pending = lease(&mut player, 0.).prepare_emission(&[1]).unwrap();
    let foreign_baseline = foreign.quota.snapshot().worker_bytes;
    let proof = foreign_auth.flushed_proof(&foreign).unwrap();
    let with_proof = foreign.quota.snapshot().worker_bytes;
    assert!(with_proof > foreign_baseline);
    assert!(pending.after_host_emission(proof).is_err());
    assert_eq!(pending.state, EmissionSettlementState::ValidationFailed);
    assert!(pending.proof.is_some());
    assert_eq!(pending.settled_sources, 0);
    assert!(pending.settle().is_err());
    assert_eq!(foreign.quota.snapshot().worker_bytes, with_proof);
    drop(pending);
    assert_eq!(foreign.quota.snapshot().worker_bytes, foreign_baseline);
}

#[test]
fn partial_history_failure_keeps_original_proof_and_remaining_cursor() {
    let mut f = Fixture::new(false, 32 << 20);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let first = SourceToken::from_native(2_001).unwrap();
    let second = SourceToken::from_native(2_002).unwrap();
    let mut entries = BTreeMap::new();
    entries.insert(
        first.evidence_key(),
        EvidenceEntry {
            token: first,
            source: [1; 32],
            lineage: Vec::new(),
            payload: b"first".to_vec(),
            history: Arc::new(OrderedHistory {
                marker: 1,
                fail: false,
                calls: Arc::clone(&calls),
            }),
        },
    );
    entries.insert(
        second.evidence_key(),
        EvidenceEntry {
            token: second,
            source: [2; 32],
            lineage: Vec::new(),
            payload: b"second".to_vec(),
            history: Arc::new(OrderedHistory {
                marker: 2,
                fail: true,
                calls: Arc::clone(&calls),
            }),
        },
    );
    f.evidence = Arc::new(FrozenEvidence {
        entries,
        digest: [4; 32],
        package_digest: f.package.digest().into(),
        quota: f.quota.clone(),
        _storage: reserve(&f.quota, 65_536).unwrap(),
    });
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let auth = Authorization::new(&f);
    let spec = f.spec(&json!({}), false);
    let mut prep = begin(&cache, &f, spec, Arc::clone(&auth), Owner::new(&f.quota));
    for _ in 0..2 {
        let ticket = prep.next_sample().unwrap();
        let surface = split_snapshot(255, first, second);
        prep.push_snapshot(
            ticket,
            surface.snapshot(),
            |value, _, _| value,
            |value, _, _| value >= 0.5,
        )
        .unwrap();
    }
    let clip = prep.finish().unwrap();
    let mut player = player(&f, clip, Arc::clone(&auth), StopToken::default());
    let mut emission = lease(&mut player, 0.).prepare_emission(&[255]).unwrap();
    emission
        .after_host_emission(auth.flushed_proof(&f).unwrap())
        .unwrap();
    let before_failure = f.quota.snapshot().worker_bytes;
    assert!(emission.settle().is_err());
    assert_eq!(*calls.lock().unwrap(), vec![1, 2]);
    assert_eq!(emission.state, EmissionSettlementState::HistoryFailed);
    assert_eq!(emission.settled_sources, 1);
    assert!(emission.proof.is_some());
    assert_eq!(f.quota.snapshot().worker_bytes, before_failure);
    assert!(emission.settle().is_err());
    assert_eq!(*calls.lock().unwrap(), vec![1, 2]);
}

#[test]
fn revocation_epoch_stale_lease_and_false_emission_fail_closed() {
    let f = Fixture::new(true, 32 << 20);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let auth = Authorization::new(&f);
    let clip = prepare(&f, &cache, auth.clone(), Some(f.evidence.token(0).unwrap()));
    let mut player = player(&f, clip, auth.clone(), StopToken::default());
    assert!(lease(&mut player, 0.).prepare_emission(&[2]).is_err());
    let old = lease(&mut player, 0.);
    player.seek(Duration::ZERO, 0.5).unwrap();
    assert!(old.packed().is_err());
    let live = lease(&mut player, 0.);
    auth.epoch.store(8, Ordering::Release);
    assert!(live.packed().is_err());
    assert!(player.sample(Duration::ZERO).is_err());
    assert!(f.history.0.lock().unwrap().is_empty());
}
#[test]
fn pause_speed_seek_backward_overflow_and_parent_stop_ownership() {
    let f = Fixture::new(false, 32 << 20);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let auth = Authorization::new(&f);
    let clip = prepare(&f, &cache, auth.clone(), None);
    let parent = StopToken::default();
    let mut player = player(&f, clip, auth, parent.clone());
    player.pause(Duration::ZERO).unwrap();
    player.seek(Duration::from_secs(5), 0.75).unwrap();
    let (frame, clock) = player.sample(Duration::from_secs(6)).unwrap();
    assert!(clock.suspended);
    let Playback::Frame(frame) = frame else {
        panic!()
    };
    assert_eq!(frame.packed().unwrap().masks, vec![255]);
    drop(frame);
    assert!(player.set_speed(Duration::from_secs(6), f64::NAN).is_err());
    player.resume(Duration::from_secs(6)).unwrap();
    player.set_speed(Duration::from_secs(6), 0.).unwrap();
    assert!(player.sample(Duration::from_secs(5)).is_err());
    assert_eq!(lease(&mut player, 7.).packed().unwrap().masks, vec![255]);
    assert!(player.seek(Duration::from_secs(7), f64::INFINITY).is_err());
    player.set_speed(Duration::from_secs(7), f64::MAX).unwrap();
    assert!(player.sample(Duration::from_secs(10)).is_err());
    player.set_speed(Duration::from_secs(7), 1.).unwrap();
    drop(player);
    assert!(!parent.is_stopped());
}
#[test]
fn seamless_requires_certified_state_and_exact_boundary_ticket() {
    let f = Fixture::new(false, 32 << 20);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let spec = f.spec(&json!({}), true);
    let mut prep = begin(
        &cache,
        &f,
        spec.clone(),
        Authorization::new(&f),
        Owner::new(&f.quota),
    );
    push(&mut prep, 1, None);
    push(&mut prep, 1, None);
    let boundary = prep.boundary_sample().unwrap();
    assert_eq!(boundary.time, 1.);
    let surface = snapshot(1, None);
    prep.verify_boundary(boundary, surface.snapshot(), |v, _, _| v, |v, _, _| v > 0.)
        .unwrap();
    assert!(prep.finish().is_ok());
    assert!(
        ReplayCertification::sealed_procedural(
            &f.package,
            &plan(true),
            &json!({}),
            ReplayExecutionIdentity {
                ambient_seed: 3,
                bootstrap_digest: [1; 32],
                environment_digest: [2; 32],
                helper_build_digest: [3; 32],
            },
            0
        )
        .is_err(),
        "current native issuer cannot certify seamless continuity"
    );
    let other = f.spec(&json!({"other":true}), true);
    let mut prep = begin(
        &cache,
        &f,
        other.clone(),
        Authorization::new(&f),
        Owner::new(&f.quota),
    );
    push(&mut prep, 1, None);
    push(&mut prep, 1, None);
    let boundary = prep.boundary_sample().unwrap();
    let wrong = snapshot(2, None);
    assert!(prep
        .verify_boundary(boundary, wrong.snapshot(), |v, _, _| v, |v, _, _| v > 0.)
        .is_err());
    assert!(prep.finish().is_err());
    assert!(!cache.contains(other.key()).unwrap());
}
#[test]
fn active_lease_slots_and_retained_bytes_survive_cache_eviction() {
    let f = Fixture::new(false, 32 << 20);
    let baseline = f.quota.snapshot().worker_bytes;
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let auth = Authorization::new(&f);
    let clip = prepare(&f, &cache, auth.clone(), None);
    let key = clip.key();
    let mut player = ReplayPlayer::new(
        clip,
        f.authority(),
        auth,
        PlaybackSettings {
            now: Duration::ZERO,
            speed: 1.,
            mode: PlaybackMode::Once,
            max_leases: 1,
            stop: StopToken::default(),
        },
    )
    .unwrap();
    let lease = lease(&mut player, 0.);
    assert!(player.sample(Duration::ZERO).is_err());
    cache.evict(key).unwrap();
    drop(cache);
    assert!(f.quota.snapshot().worker_bytes > baseline);
    assert_eq!(lease.packed().unwrap().masks, vec![1]);
    drop(lease);
    assert!(matches!(
        player.sample(Duration::from_secs(1)).unwrap().0,
        Playback::Ended
    ));
    drop(player);
    assert_eq!(f.quota.snapshot().worker_bytes, baseline);
}
#[test]
fn foreign_native_tickets_and_unretained_evidence_are_rejected() {
    let f = Fixture::new(true, 32 << 20);
    let auth = Authorization::new(&f);
    let first = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let second = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let mut a = begin(
        &first,
        &f,
        f.spec(&json!({}), false),
        auth.clone(),
        Owner::new(&f.quota),
    );
    let mut b = begin(
        &second,
        &f,
        f.spec(&json!({}), false),
        auth.clone(),
        Owner::new(&f.quota),
    );
    let foreign = a.next_sample().unwrap();
    let _own = b.next_sample().unwrap();
    let frame = snapshot(1, Some(f.evidence.token(0).unwrap()));
    assert!(b
        .push_snapshot(foreign, frame.snapshot(), |v, _, _| v, |v, _, _| v > 0.)
        .is_err());
    let outsider = Fixture::new(true, 32 << 20);
    let third = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let mut prep = begin(
        &third,
        &f,
        f.spec(&json!({}), false),
        auth,
        Owner::new(&f.quota),
    );
    let ticket = prep.next_sample().unwrap();
    let wrong = snapshot(1, Some(outsider.evidence.token(0).unwrap()));
    assert!(prep
        .push_snapshot(ticket, wrong.snapshot(), |v, _, _| v, |v, _, _| v > 0.)
        .is_err());
    assert!(f.history.0.lock().unwrap().is_empty());
}
#[test]
fn partial_unknown_cancelled_or_revoked_preparation_never_publishes() {
    let f = Fixture::new(false, 32 << 20);
    let auth = Authorization::new(&f);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let spec = f.spec(&json!({}), false);
    let owner = Owner::new(&f.quota);
    let mut partial = begin(&cache, &f, spec.clone(), auth.clone(), owner.clone());
    push(&mut partial, 1, None);
    assert!(partial.finish().is_err());
    assert!(!cache.contains(spec.key()).unwrap());
    owner.actual_exit();
    cache.collect_retired().unwrap();
    let owner = Owner::new(&f.quota);
    let mut unknown = begin(&cache, &f, spec.clone(), auth.clone(), owner.clone());
    let ticket = unknown.next_sample().unwrap();
    let mut empty = Surface::new(1, 1, shape()).unwrap();
    let seed = empty.begin(1).unwrap();
    empty
        .finish(
            FrameMeta {
                wire_version: 1,
                key: seed.key,
                shape: shape(),
                presented: true,
                error: None,
                commands: vec![],
            },
            Planes {
                data: seed.data,
                touch: vec![3],
                order: vec![1],
                cell_rgb: None,
                colour_touch: None,
                colour_order: None,
            },
            &mut NoNativeRenderer,
        )
        .unwrap();
    assert!(unknown
        .push_snapshot(ticket, empty.snapshot(), |v, _, _| v, |v, _, _| v > 0.)
        .is_err());
    drop(unknown);
    owner.actual_exit();
    cache.collect_retired().unwrap();
    let mut complete = begin(&cache, &f, spec.clone(), auth.clone(), Owner::new(&f.quota));
    push(&mut complete, 1, None);
    push(&mut complete, 1, None);
    auth.revoked.store(true, Ordering::Release);
    assert!(complete.finish().is_err());
    assert!(!cache.contains(spec.key()).unwrap());
}
#[test]
fn cache_delivery_and_pending_emission_recheck_current_authority() {
    let f = Fixture::new(true, 32 << 20);
    let auth = Authorization::new(&f);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let clip = prepare(&f, &cache, auth.clone(), Some(f.evidence.token(0).unwrap()));
    let mut playback = player(&f, clip, auth.clone(), StopToken::default());
    let pending = lease(&mut playback, 0.).prepare_emission(&[1]).unwrap();
    auth.revoked.store(true, Ordering::Release);
    assert!(
        auth.flushed_proof(&f).is_err(),
        "revocation prevents a new broker ticket"
    );
    drop(pending); // No output occurred, so no flushed proof or history exists.
    assert!(cache
        .begin(
            f.spec(&json!({}), false),
            f.authority(),
            auth,
            StopToken::default(),
            || panic!("revocation precedes all acquisition")
        )
        .is_err());
    assert!(f.history.0.lock().unwrap().is_empty());
}

#[test]
fn frozen_input_capture_retains_ordered_native_values_and_enforces_total_limit() {
    let quota = root(8 << 20);
    let mut first_planes = BTreeMap::new();
    first_planes.insert("b0".into(), vec![1, 2, 3, 4]);
    let first_value = ServiceValue::copy_from_host(
        &json!({
            "provider":"fixture",
            "revision":3,
            "samples":{"$ilium_binary":"b0"}
        }),
        &[ArraySpec {
            name: "b0".into(),
            kind: TypedArrayKind::U8,
            elements: 4,
        }],
        &first_planes,
        &EngineLimits::default(),
        quota.clone(),
    )
    .unwrap();
    let first_snapshot = FrozenInputSnapshot::from_native(
        InputFamily::Series,
        "series:primary",
        3,
        first_value.clone(),
    )
    .unwrap();
    let second_snapshot = FrozenInputSnapshot::from_native(
        InputFamily::Weather,
        "weather:local",
        8,
        ServiceValue::copy_from_host(
            &json!({"provider":"fixture","revision":8}),
            &[],
            &BTreeMap::new(),
            &EngineLimits::default(),
            quota.clone(),
        )
        .unwrap(),
    )
    .unwrap();
    let snapshots = vec![first_snapshot, second_snapshot];
    let measured_bytes = snapshots
        .iter()
        .map(|snapshot| snapshot.wire_bytes().unwrap())
        .sum::<usize>();
    let capture = FrozenInputs::from_capture(
        quota.clone(),
        "recording-native-1",
        snapshots.clone(),
        measured_bytes,
        None,
        "test capture",
        &[],
    )
    .unwrap();

    assert_eq!(capture.recording(), Some("recording-native-1"));
    assert_eq!(capture.snapshots().len(), 2);
    assert_eq!(capture.snapshots()[0].handle_id(), "series:primary");
    assert_eq!(capture.snapshots()[0].revision(), 3);
    assert_eq!(capture.snapshots()[0].value().planes()["b0"], [1, 2, 3, 4]);
    assert_eq!(capture.snapshots()[1].handle_id(), "weather:local");

    let repeated = FrozenInputs::from_capture(
        quota.clone(),
        "recording-native-2",
        snapshots.clone(),
        measured_bytes,
        None,
        "test capture",
        &[],
    )
    .unwrap();
    assert_eq!(capture.digest(), repeated.digest());
    assert!(FrozenInputs::from_capture(
        quota,
        "recording-too-large",
        snapshots,
        measured_bytes - 1,
        None,
        "test capture",
        &[],
    )
    .is_err());
}

#[test]
fn frozen_input_capture_retains_exact_native_image_allocations_and_hashes_pixels() {
    use crate::{
        native_media::{MediaLimits, NativeMedia},
        sources::NativeSourceImage,
    };

    let quota = root(8 << 20);
    let source_image = |rgba| {
        let mut media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
        let handle = media.solid_image(rgba).unwrap();
        NativeSourceImage::from_native(&media, handle).unwrap()
    };
    let first_image = source_image([12, 24, 36, 255]);
    let equal_pixels_different_owner = source_image([12, 24, 36, 255]);
    let second_image = source_image([12, 24, 37, 255]);
    let make_capture = |image: NativeSourceImage| {
        let value = ServiceValue::copy_from_host(
            &json!({"layers":[{"tiles":[{"image":{"native_image_slot":0,"width":1,"height":1}}]}]}),
            &[],
            &BTreeMap::new(),
            &EngineLimits::default(),
            quota.clone(),
        )
        .unwrap();
        let snapshot = FrozenInputSnapshot::from_native_with_images(
            InputFamily::Weather,
            "weather:tiles",
            7,
            value,
            vec![image],
        )
        .unwrap();
        FrozenInputs::from_capture(
            quota.clone(),
            "recording-weather",
            vec![snapshot],
            1 << 20,
            None,
            "native weather fixture",
            &[],
        )
        .unwrap()
    };

    let first = make_capture(first_image.clone());
    let repeated = make_capture(first_image.clone());
    let equal_pixels = make_capture(equal_pixels_different_owner.clone());
    let changed_pixels = make_capture(second_image);
    assert!(first.snapshots()[0].native_images()[0].same_allocation(&first_image));
    assert!(!first_image.same_allocation(&equal_pixels_different_owner));
    assert!(equal_pixels.snapshots()[0].native_images()[0]
        .same_allocation(&equal_pixels_different_owner));
    assert_eq!(first.digest(), repeated.digest());
    assert_eq!(first.digest(), equal_pixels.digest());
    assert_ne!(first.digest(), changed_pixels.digest());
}

#[test]
fn frozen_source_sequence_accounts_native_image_vector_capacity() {
    let sequence_storage = |image_capacity| {
        let quota = root(1 << 20);
        let value = ServiceValue::copy_from_host(
            &json!({"provider":"fixture"}),
            &[],
            &BTreeMap::new(),
            &EngineLimits::default(),
            quota.clone(),
        )
        .unwrap();
        let native_images = Vec::with_capacity(image_capacity);
        let image_slot_capacity = native_images.capacity();
        let snapshot = FrozenInputSnapshot::from_native_with_images(
            InputFamily::Weather,
            "weather:tiles",
            1,
            value,
            native_images,
        )
        .unwrap();
        let baseline = quota.snapshot().worker_bytes;
        let _sequence = FrozenSourceSequence::from_native(
            quota.clone(),
            1_000,
            vec![FrozenSourceFrame::from_native(0, vec![snapshot])],
            1,
            1 << 20,
        )
        .unwrap();
        (
            quota.snapshot().worker_bytes - baseline,
            image_slot_capacity,
        )
    };

    let (one_slot_bytes, one_slot_capacity) = sequence_storage(1);
    let (eight_slots_bytes, eight_slot_capacity) = sequence_storage(8);
    assert_eq!(
        eight_slots_bytes - one_slot_bytes,
        (eight_slot_capacity - one_slot_capacity) * size_of::<NativeSourceImage>()
    );
}

#[test]
fn frozen_source_sequence_binds_timestamps_order_and_distinct_revisions() {
    let quota = root(8 << 20);
    let make_snapshot = |revision, label: &str| {
        FrozenInputSnapshot::from_native(
            InputFamily::Chess,
            "chess:tv",
            revision,
            ServiceValue::copy_from_host(
                &json!({"fen":label,"moves":[]}),
                &[],
                &BTreeMap::new(),
                &EngineLimits::default(),
                quota.clone(),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let first = FrozenSourceFrame::from_native(0, vec![make_snapshot(3, "position-a")]);
    let second = FrozenSourceFrame::from_native(1_000, vec![make_snapshot(4, "position-b")]);
    let sequence =
        FrozenSourceSequence::from_native(quota.clone(), 12_000, vec![first, second], 16, 1 << 20)
            .unwrap();

    assert_eq!(sequence.frame_at(0).unwrap().offset_ms(), 0);
    assert_eq!(sequence.frame_at(999).unwrap().snapshots()[0].revision(), 3);
    assert_eq!(
        sequence.frame_at(1_000).unwrap().snapshots()[0].revision(),
        4
    );
    assert_eq!(
        sequence.frame_at(11_999).unwrap().snapshots()[0].revision(),
        4
    );
    assert_eq!(sequence.duration_ms(), 12_000);
    assert_eq!(
        sequence.digest(),
        FrozenSourceSequence::from_native(
            quota.clone(),
            12_000,
            vec![
                FrozenSourceFrame::from_native(0, vec![make_snapshot(3, "position-a")]),
                FrozenSourceFrame::from_native(1_000, vec![make_snapshot(4, "position-b")]),
            ],
            16,
            1 << 20,
        )
        .unwrap()
        .digest()
    );

    let reordered = FrozenSourceSequence::from_native(
        quota.clone(),
        12_000,
        vec![
            FrozenSourceFrame::from_native(0, vec![make_snapshot(3, "position-a")]),
            FrozenSourceFrame::from_native(1_001, vec![make_snapshot(4, "position-b")]),
        ],
        16,
        1 << 20,
    )
    .unwrap();
    assert_ne!(sequence.digest(), reordered.digest());
}

#[test]
fn frozen_source_sequence_carries_forward_unchanged_feeds_between_sparse_updates() {
    let quota = root(8 << 20);
    let make_snapshot = |handle: &str, revision, fen: &str| {
        FrozenInputSnapshot::from_native(
            InputFamily::Chess,
            handle,
            revision,
            ServiceValue::copy_from_host(
                &json!({"fen":fen}),
                &[],
                &BTreeMap::new(),
                &EngineLimits::default(),
                quota.clone(),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let sequence = FrozenSourceSequence::from_native(
        quota.clone(),
        3_000,
        vec![
            FrozenSourceFrame::from_native(
                0,
                vec![
                    make_snapshot("chess:alpha", 4, "alpha-4"),
                    make_snapshot("chess:beta", 8, "beta-8"),
                ],
            ),
            FrozenSourceFrame::from_native(1_000, vec![make_snapshot("chess:alpha", 5, "alpha-5")]),
            FrozenSourceFrame::from_native(2_000, vec![make_snapshot("chess:beta", 9, "beta-9")]),
        ],
        8,
        1 << 20,
    )
    .unwrap();

    let at_start: Vec<_> = sequence.snapshots_at(0).unwrap().collect();
    assert_eq!(at_start.len(), 2);
    assert_eq!(at_start[0].revision(), 4);
    assert_eq!(at_start[1].revision(), 8);
    let between_updates: Vec<_> = sequence.snapshots_at(1_500).unwrap().collect();
    assert_eq!(between_updates.len(), 2);
    assert_eq!(between_updates[0].revision(), 5);
    assert_eq!(between_updates[1].revision(), 8);
    let after_updates: Vec<_> = sequence.snapshots_at(2_500).unwrap().collect();
    assert_eq!(after_updates.len(), 2);
    assert_eq!(after_updates[0].revision(), 5);
    assert_eq!(after_updates[1].revision(), 9);
}

#[test]
fn frozen_source_sequence_refuses_missing_start_duplicate_revisions_and_limits() {
    let quota = root(8 << 20);
    let make_snapshot = |revision| {
        FrozenInputSnapshot::from_native(
            InputFamily::Chess,
            "chess:tv",
            revision,
            ServiceValue::copy_from_host(
                &json!({"fen":"position"}),
                &[],
                &BTreeMap::new(),
                &EngineLimits::default(),
                quota.clone(),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let make_frame = |offset_ms, revision| {
        FrozenSourceFrame::from_native(offset_ms, vec![make_snapshot(revision)])
    };
    assert!(FrozenSourceSequence::from_native(
        quota.clone(),
        1_000,
        vec![make_frame(1, 1)],
        16,
        1 << 20,
    )
    .is_err());
    assert!(FrozenSourceSequence::from_native(
        quota.clone(),
        1_000,
        vec![make_frame(0, 1), make_frame(500, 1)],
        16,
        1 << 20,
    )
    .is_err());
    assert!(FrozenSourceSequence::from_native(
        quota.clone(),
        1_000,
        vec![make_frame(0, 1), make_frame(1_000, 2)],
        16,
        1 << 20,
    )
    .is_err());
    assert!(FrozenSourceSequence::from_native(
        quota.clone(),
        1_000,
        vec![make_frame(0, 1)],
        0,
        1 << 20,
    )
    .is_err());
}
