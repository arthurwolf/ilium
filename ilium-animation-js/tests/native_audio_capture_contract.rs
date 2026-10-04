#![cfg(feature = "native-host")]
use ilium_ambient::resources::{AmbientResources, WorkerCost};
use ilium_animation_js::{
    native_audio::{
        AudioDemand, AudioProduct, AudioSourceSelection, AuthenticatedAudioGrant,
        AuthenticatedCaptureFactory,
    },
    native_audio_capture::{CaptureBackend, NativeAudioCaptureFactory, QualifiedCaptureBinding},
    permissions::{
        AudioProduct as PermissionProduct, Capability, Ceiling, Demand, PackageIdentity,
        PermissionBroker, PermissionPlan, PermissionRequest, Right, Scope, UserChoice,
    },
};
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
/// Synthetic task-local helper. It emits PCM using shell builtins only; no
/// device, system capture process, network, descendant process or user state.
#[test]
fn actual_owned_fixture_pcm_revocation_and_join_custody() {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("ilium-audio-capture-{}-{id}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    let program = path.join("parec");
    // Stereo 0.5/0.5 as little-endian f32, repeated; no external shell commands.
    std::fs::write(
        &program,
        b"#!/bin/sh\nwhile :; do printf '\\000\\000\\000\\077\\000\\000\\000\\077'; done\n",
    )
    .unwrap();
    ilium_platform::secure_fs::restrict_executable_file_to_owner(&program).unwrap();
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 2,
        jobs: 2,
        service_jobs: 0,
        input_bytes: 1024,
        result_bytes: 1024,
        worker_threads: 8,
        worker_bytes: 32 * 1024 * 1024,
    });
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
    let selected = QualifiedCaptureBinding::from_host(
        AudioSourceSelection::Loopback,
        "fixture.monitor".into(),
        "loopback".into(),
        Capability::AudioLoopback,
        "fixture_device".into(),
        program,
        CaptureBackend::Pulse,
        WorkerCost {
            threads: 1,
            resident_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    let right = Right {
        id: Capability::AudioLoopback,
        scope: Scope::Audio {
            device: "loopback".into(),
            products: BTreeSet::from([PermissionProduct::Waveform]),
        },
    };
    let ceiling = Ceiling {
        permissions: vec![right.clone()],
    };
    let mut broker = PermissionBroker::new(
        PackageIdentity::unverified("audio_fixture".into(), b"synthetic").unwrap(),
        ceiling.clone(),
        ceiling,
    )
    .unwrap();
    let review = broker
        .prepare(
            1,
            1,
            PermissionPlan {
                permissions: vec![PermissionRequest {
                    request_id: Some("capture".into()),
                    id: right.id,
                    scope: right.scope.clone(),
                    required: true,
                    reason: "Task-local synthetic PCM fixture".into(),
                }],
                demands: vec![Demand {
                    demand_id: "audio".into(),
                    request_ids: BTreeSet::from(["capture".into()]),
                }],
            },
            BTreeMap::from([("capture".into(), selected.binding().clone())]),
        )
        .unwrap();
    let channel = broker
        .resolve(
            review,
            BTreeMap::from([("capture".into(), UserChoice::AllowSession)]),
        )
        .unwrap()
        .activation
        .unwrap()
        .channel;
    let broker = Arc::new(Mutex::new(broker));
    let baseline = quota.snapshot();
    let mut factory = NativeAudioCaptureFactory::new(
        selected,
        broker.clone(),
        channel,
        "audio".into(),
        quota.clone(),
    )
    .unwrap();
    let products = BTreeSet::from([AudioProduct::Waveform]);
    let grant =
        AuthenticatedAudioGrant::from_host(AudioSourceSelection::Loopback, products.clone(), 1)
            .unwrap();
    let demand = AudioDemand {
        products,
        ..AudioDemand::default()
    };
    let mut capture = factory.open(&grant, &demand, &resources).unwrap();
    assert!(quota.snapshot().worker_threads >= baseline.worker_threads + 3);
    assert!(quota.snapshot().worker_bytes >= baseline.worker_bytes + 5 * 1024 * 1024);
    let mut samples = [0.; 32];
    let count = capture.read_mono(&mut samples).unwrap();
    assert!(count > 0);
    assert!(samples[..count].iter().all(|sample| *sample == 0.5));
    assert!(capture.read_mono(&mut vec![0.; 8193]).is_err());
    let invalidation = broker.lock().unwrap().revoke(right).unwrap();
    assert!(!invalidation.operations.is_empty());
    assert!(capture.read_mono(&mut samples).is_err());
    capture.request_stop();
    assert!(capture
        .join_until(Instant::now() + Duration::from_secs(2))
        .unwrap());
    // Logical producer still owns admission after actual join until its retained graph drops.
    assert!(quota.snapshot().worker_bytes > baseline.worker_bytes);
    drop(capture);
    assert_eq!(quota.snapshot().worker_bytes, baseline.worker_bytes);
    assert_eq!(quota.snapshot().worker_threads, baseline.worker_threads);
    drop(factory);
    drop(resources);
    drop(execution);
    std::fs::remove_file(path.join("parec")).unwrap();
    std::fs::remove_dir(&path).unwrap();
}
