#![cfg(feature = "native-host")]
use ilium_animation_js::{
    helper::HelperAuthority,
    native_audio::{AudioDemand, AudioProcessor, AudioProduct},
};
use ilium_execution::{QuotaGroup, QuotaLimits};
use std::collections::BTreeSet;
fn quota() -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 2,
        jobs: 2,
        service_jobs: 0,
        input_bytes: 1024,
        result_bytes: 1024,
        worker_threads: 2,
        worker_bytes: 8 * 1024 * 1024,
    })
}
fn demand(products: &[AudioProduct]) -> AudioDemand {
    AudioDemand {
        products: products.iter().copied().collect::<BTreeSet<_>>(),
        ..AudioDemand::default()
    }
}
fn authority(epoch: u64) -> HelperAuthority {
    HelperAuthority {
        package_digest: "a".repeat(64),
        instance_id: 1,
        plan_generation: 1,
        authorization_epoch: epoch,
    }
}
#[test]
fn waveform_level_and_envelope_never_allocate_or_execute_fft() {
    let mut processor = AudioProcessor::new(
        demand(&[
            AudioProduct::Level,
            AudioProduct::Waveform,
            AudioProduct::Envelope,
        ]),
        quota(),
    )
    .unwrap();
    processor.push(&vec![0.5; 1024]).unwrap();
    let snapshot = processor.snapshot(100, 1_780_000_000_100).unwrap();
    assert_eq!(processor.fft_count(), 0);
    assert!(!processor.has_fft());
    assert_eq!(snapshot.view().rms, Some(0.5));
    assert!(snapshot.view().bands.is_none());
    assert_eq!(snapshot.view().waveform.as_ref().unwrap().len(), 256);
    assert_eq!(snapshot.view().envelope.as_ref().unwrap().len(), 64);
}
#[test]
fn spectra_and_history_are_selective_and_bounded() {
    let mut processor = AudioProcessor::new(demand(&[AudioProduct::History]), quota()).unwrap();
    processor.push(&vec![0.5; 1024]).unwrap();
    let snapshot = processor.snapshot(100, 1_780_000_000_100).unwrap();
    assert_eq!(processor.fft_count(), 1);
    assert!(snapshot.view().bands.is_none());
    assert_eq!(snapshot.view().history_frames, Some(1));
    assert_eq!(snapshot.view().history.as_ref().unwrap().len(), 32);
    assert!(processor.snapshot(101, 1_780_000_000_101).is_err());
}
#[test]
fn retention_remains_charged_after_processor_is_dropped() {
    let quota = quota();
    let baseline = quota.snapshot().worker_bytes;
    let mut processor =
        AudioProcessor::new(demand(&[AudioProduct::Waveform]), quota.clone()).unwrap();
    let snapshot = processor.snapshot(100, 1_780_000_000_100).unwrap();
    drop(processor);
    assert!(quota.snapshot().worker_bytes > baseline);
    assert_eq!(snapshot.view().waveform.as_ref().unwrap().len(), 256);
    drop(snapshot);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}
#[test]
fn malformed_sizes_and_samples_are_bounded() {
    let quota = quota();
    let mut invalid = demand(&[AudioProduct::Bands]);
    invalid.fft_samples = 1000;
    assert!(AudioProcessor::new(invalid, quota.clone()).is_err());
    let mut processor = AudioProcessor::new(demand(&[AudioProduct::Level]), quota).unwrap();
    assert!(processor.push(&vec![0.; 8193]).is_err());
    processor.push(&[f32::NAN, f32::INFINITY, 0.]).unwrap();
    assert_eq!(
        processor
            .snapshot(100, 1_780_000_000_100)
            .unwrap()
            .view()
            .rms,
        Some(0.)
    );
    processor.cancel();
    assert!(processor.push(&[1.]).is_err());
    assert!(processor.snapshot(200, 1_780_000_000_200).is_err());
}

#[test]
fn fractional_and_sub_hertz_plan_cadence_never_rounds_up() {
    use ilium_animation_js::plan::AudioDemand as PlannedAudioDemand;
    let planned = PlannedAudioDemand {
        max_hz: 0.5,
        products: vec!["level".into()],
        source: Some("loopback".into()),
        band_count: None,
        waveform_samples: None,
        window: None,
        history_frames: None,
    };
    let mut processor =
        AudioProcessor::new(AudioDemand::from_accepted_plan(&planned).unwrap(), quota()).unwrap();
    processor.push(&[0.25, -0.25]).unwrap();
    assert!(processor.snapshot(0, 1_780_000_000_000).is_ok());
    assert!(processor.snapshot(1_999, 1_780_000_001_999).is_err());
    assert!(processor.snapshot(2_000, 1_780_000_002_000).is_ok());
    assert_eq!(processor.fft_count(), 0);
}

#[test]
fn only_requested_products_are_serialized_and_fft_is_lazy() {
    let mut level = AudioProcessor::new(demand(&[AudioProduct::Level]), quota()).unwrap();
    level.push(&[0.5; 128]).unwrap();
    let snapshot = level.snapshot(100, 1_780_000_000_100).unwrap();
    let value = serde_json::to_value(snapshot.view()).unwrap();
    assert_eq!(value["captured_at_ms"], 1_780_000_000_100_u64);
    assert!(value.get("rms").is_some());
    for absent in ["waveform", "envelope", "bands", "history"] {
        assert!(value.get(absent).is_none(), "unexpected product {absent}");
    }
    assert!(!level.has_fft());
    assert_eq!(level.fft_count(), 0);
    let mut bands = AudioProcessor::new(demand(&[AudioProduct::Bands]), quota()).unwrap();
    assert!(bands.has_fft());
    bands.push(&[0.5; 1024]).unwrap();
    let spectrum = bands.snapshot(100, 1_780_000_000_100).unwrap();
    let spectrum = serde_json::to_value(spectrum.view()).unwrap();
    assert!(spectrum.get("bands").is_some());
    assert!(spectrum.get("waveform").is_none());
    assert_eq!(bands.fft_count(), 1);
}

#[test]
fn empty_demand_never_opens_capture_and_epoch_revocation_stops_owned_source() {
    use ilium_ambient::resources::AmbientResources;
    use ilium_animation_js::{
        error::Result,
        native_audio::{
            AudioSourceSelection, AuthenticatedAudioGrant, AuthenticatedCaptureFactory,
            NativeAudioService, OwnedAudioCapture,
        },
    };
    use ilium_execution::{ClientLimits, Execution, ExecutionConfig, LaneConfig};
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Instant,
    };
    struct Capture(Arc<AtomicUsize>);
    impl OwnedAudioCapture for Capture {
        fn sample_rate(&self) -> u32 {
            44100
        }
        fn read_mono(&mut self, out: &mut [f32]) -> Result<usize> {
            out[..4].fill(0.5);
            Ok(4)
        }
        fn request_stop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn join_until(&mut self, _: Instant) -> Result<bool> {
            Ok(true)
        }
    }
    struct Factory {
        opens: usize,
        stops: Arc<AtomicUsize>,
    }
    impl AuthenticatedCaptureFactory for Factory {
        fn open(
            &mut self,
            _: &AuthenticatedAudioGrant,
            _: &AudioDemand,
            _: &AmbientResources,
        ) -> Result<Box<dyn OwnedAudioCapture>> {
            self.opens += 1;
            Ok(Box::new(Capture(self.stops.clone())))
        }
    }
    let quota = quota();
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
    let stops = Arc::new(AtomicUsize::new(0));
    let mut factory = Factory {
        opens: 0,
        stops: stops.clone(),
    };
    let grant =
        AuthenticatedAudioGrant::from_host(AudioSourceSelection::Loopback, BTreeSet::new(), 1)
            .unwrap();
    let mut empty = NativeAudioService::open(
        demand(&[]),
        grant,
        authority(1),
        &resources,
        quota.clone(),
        &mut factory,
    )
    .unwrap();
    assert_eq!(factory.opens, 0);
    assert!(empty.poll(100, 1_780_000_000_100, 1).unwrap().is_none());
    drop(empty);
    let granted = [AudioProduct::Level].into_iter().collect();
    let grant =
        AuthenticatedAudioGrant::from_host(AudioSourceSelection::Microphone, granted, 2).unwrap();
    let mut live = NativeAudioService::open(
        demand(&[AudioProduct::Level]),
        grant,
        authority(2),
        &resources,
        quota,
        &mut factory,
    )
    .unwrap();
    assert_eq!(factory.opens, 1);
    let snapshot = live.poll(100, 1_780_000_000_100, 2).unwrap().unwrap();
    assert_eq!(snapshot.view().captured_at_ms, 1_780_000_000_100);
    assert_eq!(snapshot.authority(), Some(&authority(2)));
    assert!(live.poll(200, 1_780_000_000_200, 3).is_err());
    assert!(stops.load(Ordering::SeqCst) > 0);
    assert!(live.retire(Instant::now()).unwrap());
}

#[test]
fn retained_audio_authenticates_original_root_after_producer_drop() {
    let original = quota();
    let foreign = quota();
    let mut producer =
        AudioProcessor::new(demand(&[AudioProduct::Waveform]), original.clone()).unwrap();
    producer.push(&[0.25, -0.25]).unwrap();
    let snapshot = producer.snapshot(100, 1_780_000_000_100).unwrap();
    let alias = std::sync::Arc::clone(&snapshot);
    drop(producer);
    assert!(snapshot.shares_root(&original));
    assert!(!snapshot.shares_root(&foreign));
    let retained = original.snapshot().worker_bytes;
    assert!(retained > 0);
    assert_eq!(foreign.snapshot().worker_bytes, 0);
    drop(snapshot);
    assert_eq!(original.snapshot().worker_bytes, retained);
    drop(alias);
    assert_eq!(original.snapshot().worker_bytes, 0);
}
