//! Synthetic fixtures exercise real native Surface sealing and original-root
//! replay ownership. No fixture claims V8/process or saved-world integration.
use ilium_animation_js::{
    error::{AnimationError, Result},
    manifest::AnimationMode,
    package::{Package, PackageLimits},
    plan::{AnimationPlan, PlanBudget},
    replay::*,
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
fn certify(seamless: bool) -> ReplayCertification {
    ReplayCertification {
        reset_rules_digest: [1; 32],
        clock_random_binding_digest: [2; 32],
        prepared_assets_digest: [3; 32],
        async_order_digest: [4; 32],
        full_unoccluded: true,
        loop_state_continuity_verified: seamless,
    }
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
        self.spec_with(
            &plan(seamless),
            settings,
            shape(),
            self.frozen.clone(),
            certify(seamless),
        )
        .unwrap()
    }
    fn spec_with(
        &self,
        plan: &AnimationPlan,
        settings: &Value,
        shape: Shape,
        frozen: Arc<FrozenInputs>,
        certification: ReplayCertification,
    ) -> Result<Arc<ClipSpec>> {
        ClipSpec::from_accepted(
            &self.quota,
            ClipSpecification {
                package: &self.package,
                verifier: &self.verifier,
                plan,
                settings,
                shape,
                backend: "native-test",
                api_version: 1,
                appearance_digest: [6; 32],
                certification,
                frozen,
                evidence: self.evidence.clone(),
            },
        )
    }
    fn authority(&self) -> ReplayAuthority {
        ReplayAuthority {
            package_digest: self.package.digest().into(),
            instance_id: 1,
            revision: 1,
            authorization_epoch: 7,
        }
    }
}
struct Authorization {
    epoch: AtomicU64,
    revoked: AtomicBool,
}
impl Authorization {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            epoch: AtomicU64::new(7),
            revoked: AtomicBool::new(false),
        })
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
        {
            Err(AnimationError::PermissionDenied("fixture revoked".into()))
        } else {
            Ok(())
        }
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
        ilium_animation_js::replay::PlaybackSettings {
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
    let auth = Authorization::new();
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
    let mut limits = ReplayLimits::default();
    limits.max_frames = 1;
    let cache = ReplayCache::new(f.quota.clone(), limits).unwrap();
    assert!(cache
        .begin(
            spec.clone(),
            f.authority(),
            Authorization::new(),
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
            Authorization::new(),
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
    let auth = Authorization::new();
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
        Authorization::new(),
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
        .spec_with(
            &plan(false),
            &json!({}),
            altered,
            f.frozen.clone(),
            certify(false)
        )
        .is_err());
    let mut cert = certify(false);
    cert.clock_random_binding_digest = [0; 32];
    assert!(f
        .spec_with(&plan(false), &json!({}), shape(), f.frozen.clone(), cert)
        .is_err());
    let mut live = plan(false);
    live.inputs.pointer = Some(ilium_animation_js::plan::RateDemand { max_hz: 2. });
    assert!(f
        .spec_with(&live, &json!({}), shape(), f.frozen.clone(), certify(false))
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
    assert!(f
        .spec_with(&live, &json!({}), shape(), frozen, certify(false))
        .is_ok());
}
#[test]
fn history_only_actual_surviving_emission_and_proof_outlives_player_cache() {
    let f = Fixture::new(true, 32 << 20);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let auth = Authorization::new();
    let token = f.evidence.token(0).unwrap();
    let clip = prepare(&f, &cache, auth.clone(), Some(token));
    assert!(f.history.0.lock().unwrap().is_empty());
    let key = clip.key();
    let mut player = player(&f, clip, auth.clone(), StopToken::default());
    let first = lease(&mut player, 0.75);
    let sequence = first.receipt().lease_sequence;
    assert!(first
        .prepare_emission(&[0])
        .unwrap()
        .after_host_emission()
        .unwrap()
        .settle()
        .is_ok());
    assert!(f.history.0.lock().unwrap().is_empty());
    let second = lease(&mut player, 0.75);
    assert_ne!(sequence, second.receipt().lease_sequence);
    let pending = second.prepare_emission(&[1 | 128]).unwrap();
    assert!(f.history.0.lock().unwrap().is_empty());
    let proof = pending.after_host_emission().unwrap();
    cache.evict(key).unwrap();
    drop(player);
    drop(cache);
    auth.revoked.store(true, Ordering::Release);
    proof.settle().unwrap();
    let events = f.history.0.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, vec![0, 7]);
}
#[test]
fn revocation_epoch_stale_lease_and_false_emission_fail_closed() {
    let f = Fixture::new(true, 32 << 20);
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let auth = Authorization::new();
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
    let auth = Authorization::new();
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
        Authorization::new(),
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
    let mut cert = certify(true);
    cert.loop_state_continuity_verified = false;
    assert!(f
        .spec_with(&plan(true), &json!({}), shape(), f.frozen.clone(), cert)
        .is_err());
    let other = f.spec(&json!({"other":true}), true);
    let mut prep = begin(
        &cache,
        &f,
        other.clone(),
        Authorization::new(),
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
    let auth = Authorization::new();
    let clip = prepare(&f, &cache, auth.clone(), None);
    let key = clip.key();
    let mut player = ReplayPlayer::new(
        clip,
        f.authority(),
        auth,
        ilium_animation_js::replay::PlaybackSettings {
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
    let auth = Authorization::new();
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
    let auth = Authorization::new();
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
    let auth = Authorization::new();
    let cache = ReplayCache::new(f.quota.clone(), ReplayLimits::default()).unwrap();
    let clip = prepare(&f, &cache, auth.clone(), Some(f.evidence.token(0).unwrap()));
    let mut playback = player(&f, clip, auth.clone(), StopToken::default());
    let pending = lease(&mut playback, 0.).prepare_emission(&[1]).unwrap();
    auth.revoked.store(true, Ordering::Release);
    assert!(pending.after_host_emission().is_err());
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
