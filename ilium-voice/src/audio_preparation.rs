//! One admitted persistent duplex DSP owner. Native callbacks only transfer
//! bounded preallocated samples; all resampling/PCM allocation happens here.
use crate::audio::{
    f32_to_pcm16_le, AudioCustody, CapturedAudio, NativeAudioAdmission, Pcm16SampleAligner,
    PlaybackRing, RawCapture, StreamingLinearResampler, MAX_CAPTURE_DEVICE_SAMPLES,
    REALTIME_SAMPLE_RATE,
};
use crate::VoiceError;
use ilium_execution::QuotaGroup;
use ilium_platform::owned_worker::{spawn_owned, OwnedWorker, StopToken, WorkerKind};
use std::sync::{atomic::Ordering, Arc};
use tokio::sync::mpsc;
const COMMAND_CAPACITY: usize = 8;
const PCM_CHUNK_BYTES: usize = 8 * 1024;
struct PcmChunk {
    generation: u64,
    bytes: Vec<u8>,
}
enum PreparationCommand {
    Pcm(PcmChunk),
    #[cfg(test)]
    Barrier(tokio::sync::oneshot::Sender<std::thread::ThreadId>),
    #[cfg(test)]
    Gate(
        std::sync::mpsc::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ),
}
pub(super) struct CapturePreparation {
    pub(super) raw: Arc<RawCapture>,
    pub(super) sender: mpsc::Sender<CapturedAudio>,
    pub(super) sample_rate: u32,
}
pub(crate) struct OutputPreparation {
    sender: mpsc::Sender<PreparationCommand>,
    raw: Arc<RawCapture>,
    _owner: OwnedWorker,
}
impl OutputPreparation {
    pub(crate) fn start(
        sample_rate: u32,
        volume: f32,
        samples: Arc<PlaybackRing>,
        capture: CapturePreparation,
        quota: &QuotaGroup,
        custody: &AudioCustody,
        native: Option<Arc<NativeAudioAdmission>>,
    ) -> Result<Self, VoiceError> {
        let CapturePreparation {
            raw,
            sender: capture_sender,
            sample_rate: input_sample_rate,
        } = capture;
        if sample_rate == 0 || input_sample_rate == 0 {
            return Err(VoiceError::InvalidConfiguration(
                "audio sample rates must be nonzero".into(),
            ));
        }
        let admission = quota
            .reserve_external_worker(1, 4 * 1024 * 1024)
            .map_err(|reason| {
                VoiceError::AudioPreparation(format!("audio DSP admission: {reason:?}"))
            })?;
        let (sender, mut receiver) = mpsc::channel(COMMAND_CAPACITY);
        let wake_thread = raw.wake.clone();
        let thread = raw.wake.clone();
        let worker_raw = raw.clone();
        let callback_native = native.clone();
        let startup_allocation = custody.startup_allocation();
        let owner = spawn_owned(
            "ilium-audio-duplex-dsp",
            WorkerKind::Cooperative,
            StopToken::default(),
            move || {
                let _lease = &admission;
                let _native = &native;
                let _startup = &startup_allocation;
                if let Some(thread) = wake_thread.get() {
                    thread.unpark();
                }
            },
            move |stop| {
                let _ = thread.set(std::thread::current());
                let mut pipeline = OutputPipeline::new(sample_rate, volume);
                let mut capture =
                    StreamingLinearResampler::new(input_sample_rate, REALTIME_SAMPLE_RATE);
                let mut raw_samples = Vec::with_capacity(MAX_CAPTURE_DEVICE_SAMPLES);
                let mut pending: Option<(u64, Vec<f32>, usize)> = None;
                while !stop.is_stopped() {
                    let mut progressed = false;
                    // Capture stays serviced even when a stalled output device
                    // has exhausted playback capacity. Admission BEFORE pop.
                    if let Ok(permit) = capture_sender.try_reserve() {
                        if let Some(sequence) = worker_raw.pop_into(&mut raw_samples) {
                            let converted = capture.process(&raw_samples);
                            if !converted.is_empty() {
                                permit.send(CapturedAudio {
                                    pcm16_le: f32_to_pcm16_le(&converted),
                                    _allocation: callback_native.clone(),
                                });
                            }
                            worker_raw.processed.store(sequence, Ordering::Release);
                            worker_raw.changed.notify_one();
                            progressed = true;
                        }
                    }
                    if let Some((generation, values, offset)) = &mut pending {
                        if *generation != samples.generation.load(Ordering::Acquire) {
                            pending = None;
                            progressed = true;
                        } else {
                            let copied = samples.push(*generation, &values[*offset..]);
                            *offset += copied;
                            progressed |= copied != 0;
                            if *offset == values.len() {
                                pending = None;
                            }
                        }
                    }
                    if pending.is_none() {
                        match receiver.try_recv() {
                            Ok(PreparationCommand::Pcm(chunk)) => {
                                if chunk.generation == samples.generation.load(Ordering::Acquire) {
                                    let output = pipeline.prepare(chunk.generation, &chunk.bytes);
                                    if !output.is_empty() {
                                        pending = Some((chunk.generation, output, 0));
                                    }
                                }
                                progressed = true;
                            }
                            #[cfg(test)]
                            Ok(PreparationCommand::Barrier(acknowledge)) => {
                                let _ = acknowledge.send(std::thread::current().id());
                                progressed = true;
                            }
                            #[cfg(test)]
                            Ok(PreparationCommand::Gate(release, entered)) => {
                                let _ = entered.send(());
                                let _ = release.recv();
                                progressed = true;
                            }
                            Err(mpsc::error::TryRecvError::Disconnected) => break,
                            Err(mpsc::error::TryRecvError::Empty) => {}
                        }
                    }
                    if !progressed {
                        std::thread::park();
                    }
                }
            },
        )
        .map_err(|error| VoiceError::AudioPreparation(error.to_string()))?;
        custody.register(owner.ticket())?;
        Ok(Self {
            sender,
            raw,
            _owner: owner,
        })
    }
    #[cfg(test)]
    pub(super) fn mailbox_capacity_probe(&self) -> impl Fn() -> usize + 'static {
        let sender = self.sender.clone();
        move || sender.capacity()
    }

    pub(crate) async fn enqueue(&self, generation: u64, bytes: &[u8]) -> Result<(), VoiceError> {
        for bytes in bytes.chunks(PCM_CHUNK_BYTES) {
            let permit = self
                .sender
                .reserve()
                .await
                .map_err(|_| VoiceError::AudioPreparation("worker stopped".into()))?;
            permit.send(PreparationCommand::Pcm(PcmChunk {
                generation,
                bytes: bytes.to_vec(),
            }));
            if let Some(thread) = self.raw.wake.get() {
                thread.unpark();
            }
        }
        Ok(())
    }
}
struct OutputPipeline {
    generation: Option<u64>,
    sample_rate: u32,
    volume: f32,
    aligner: Pcm16SampleAligner,
    resampler: StreamingLinearResampler,
}

impl OutputPipeline {
    fn new(sample_rate: u32, volume: f32) -> Self {
        Self {
            generation: None,
            sample_rate,
            volume,
            aligner: Pcm16SampleAligner::default(),
            resampler: StreamingLinearResampler::new(REALTIME_SAMPLE_RATE, sample_rate),
        }
    }

    fn prepare(&mut self, generation: u64, bytes: &[u8]) -> Vec<f32> {
        if self.generation != Some(generation) {
            self.generation = Some(generation);
            self.aligner.reset();
            self.resampler = StreamingLinearResampler::new(REALTIME_SAMPLE_RATE, self.sample_rate);
        }
        let input = self.aligner.process(bytes);
        let mut output = self.resampler.process(&input);
        for sample in &mut output {
            *sample *= self.volume;
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{f32_to_pcm16_le, pcm16_le_to_f32, render_output};
    use ilium_platform::owned_worker::WorkerExit;
    use std::sync::atomic::AtomicU64;
    use std::time::{Duration, Instant};
    struct Fixture {
        preparation: OutputPreparation,
        playback: Arc<PlaybackRing>,
        raw: Arc<RawCapture>,
        capture: mpsc::Receiver<CapturedAudio>,
        custody: AudioCustody,
        _storage: ilium_execution::StorageAdmission,
    }
    fn fixture(quota: &QuotaGroup, sample_rate: u32) -> Fixture {
        let storage = quota.reserve_external_storage(64 * 1024 * 1024).unwrap();
        let custody = AudioCustody::new(quota).unwrap();
        let raw = Arc::new(RawCapture::new());
        let generation = Arc::new(AtomicU64::new(1));
        let playback = Arc::new(PlaybackRing::new(sample_rate, generation, raw.wake.clone()));
        let (sender, capture) = mpsc::channel(32);
        let preparation = OutputPreparation::start(
            sample_rate,
            1.,
            playback.clone(),
            CapturePreparation {
                raw: raw.clone(),
                sender,
                sample_rate: 24000,
            },
            quota,
            &custody,
            None,
        )
        .unwrap();
        Fixture {
            preparation,
            playback,
            raw,
            capture,
            custody,
            _storage: storage,
        }
    }
    async fn flush(preparation: &OutputPreparation) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        preparation
            .sender
            .send(PreparationCommand::Barrier(sender))
            .await
            .unwrap();
        if let Some(thread) = preparation.raw.wake.get() {
            thread.unpark();
        }
        assert_ne!(receiver.await.unwrap(), std::thread::current().id());
    }
    #[test]
    fn preparation_preserves_pcm_parity_across_odd_byte_chunks_and_rates() {
        let samples = (0..10000)
            .map(|index| (index as f32 / 17.).sin())
            .collect::<Vec<_>>();
        let bytes = f32_to_pcm16_le(&samples);
        for sample_rate in [8000, 24000, 48000, 192000] {
            let decoded = pcm16_le_to_f32(&bytes);
            let expected = StreamingLinearResampler::new(REALTIME_SAMPLE_RATE, sample_rate)
                .process(&decoded)
                .into_iter()
                .map(|sample| sample * 0.4)
                .collect::<Vec<_>>();
            for chunk_size in [1, 3, 17, PCM_CHUNK_BYTES] {
                let mut pipeline = OutputPipeline::new(sample_rate, 0.4);
                let actual = bytes
                    .chunks(chunk_size)
                    .flat_map(|chunk| pipeline.prepare(7, chunk))
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "rate={sample_rate},chunk={chunk_size}");
            }
        }
    }
    #[test]
    fn replacement_discards_old_half_sample_and_resampler_tail() {
        let mut pipeline = OutputPipeline::new(48000, 1.);
        let _ = pipeline.prepare(1, &[0x12]);
        assert_eq!(
            pipeline.prepare(2, &[0, 64, 0, 32]),
            OutputPipeline::new(48000, 1.).prepare(2, &[0, 64, 0, 32])
        );
    }
    #[test]
    fn muted_audio_keeps_queued_frames_instead_of_becoming_underrun() {
        assert_eq!(
            OutputPipeline::new(24000, 0.).prepare(1, &[0, 64, 0, 32, 0, 16]),
            vec![0., 0.]
        );
    }
    #[tokio::test]
    async fn blocked_preparation_has_bounded_lossless_admission() {
        let quota = crate::test_quota();
        let fixture = fixture(&quota, 24000);
        let (release, blocked) = std::sync::mpsc::channel();
        let (entered, acknowledge) = tokio::sync::oneshot::channel();
        fixture
            .preparation
            .sender
            .send(PreparationCommand::Gate(blocked, entered))
            .await
            .unwrap();
        if let Some(thread) = fixture.raw.wake.get() {
            thread.unpark();
        }
        acknowledge.await.unwrap();
        let bytes = vec![0; PCM_CHUNK_BYTES];
        for _ in 0..COMMAND_CAPACITY {
            fixture.preparation.enqueue(1, &bytes).await.unwrap();
        }
        assert_eq!(fixture.preparation.sender.capacity(), 0);
        let mut pending = Box::pin(fixture.preparation.enqueue(1, &bytes));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::future::Future::poll(pending.as_mut(), &mut context).is_pending());
        release.send(()).unwrap();
        pending.await.unwrap();
        flush(&fixture.preparation).await;
        let frames = (COMMAND_CAPACITY + 1) * PCM_CHUNK_BYTES / 2 - 1;
        let mut output = vec![1f32; frames];
        render_output(&mut output, 1, &fixture.playback);
        assert!(output.iter().all(|sample| *sample == 0.));
        assert_eq!(
            fixture.playback.played.load(Ordering::Acquire),
            frames as u64
        );
    }
    #[tokio::test]
    async fn owned_thread_preserves_order_fences_stale_jobs_and_stops() {
        let quota = crate::test_quota();
        let fixture = fixture(&quota, 24000);
        fixture.preparation.enqueue(0, &[0x12]).await.unwrap();
        fixture.preparation.enqueue(1, &[0, 64, 0]).await.unwrap();
        fixture.preparation.enqueue(1, &[32, 0, 16]).await.unwrap();
        flush(&fixture.preparation).await;
        let expected = OutputPipeline::new(24000, 1.).prepare(1, &[0, 64, 0, 32, 0, 16]);
        let mut actual = vec![0f32; expected.len()];
        render_output(&mut actual, 1, &fixture.playback);
        assert_eq!(actual, expected);
        fixture.playback.invalidate().await.unwrap();
        fixture
            .preparation
            .enqueue(1, &[0, 64, 0, 64])
            .await
            .unwrap();
        fixture
            .preparation
            .enqueue(2, &[0, 16, 0, 32])
            .await
            .unwrap();
        flush(&fixture.preparation).await;
        let expected = OutputPipeline::new(24000, 1.).prepare(2, &[0, 16, 0, 32]);
        let mut actual = vec![0f32; expected.len()];
        render_output(&mut actual, 1, &fixture.playback);
        assert_eq!(actual, expected);
        let ticket = fixture.preparation._owner.ticket();
        drop(fixture.preparation);
        assert_eq!(
            ticket
                .join_until(Instant::now() + Duration::from_secs(2))
                .unwrap(),
            WorkerExit::Joined
        );
    }
    #[tokio::test]
    async fn full_playback_keeps_every_admitted_sample_and_capture_still_progresses() {
        let quota = crate::test_quota();
        let mut fixture = fixture(&quota, 1);
        // Fill before producer admission; one real DSP output then remains
        // pending while the same owner continues capture work.
        while fixture.playback.push(1, &[0.25; 512]) != 0 {}
        assert_eq!(fixture.playback.push(1, &[0.5]), 0);
        fixture
            .preparation
            .enqueue(1, &[0, 64, 0, 32])
            .await
            .unwrap();
        assert!(fixture.raw.push(&[0.5f32, 0.25, 0.125], 1));
        let capture = tokio::time::timeout(Duration::from_secs(2), fixture.capture.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(capture.pcm16_le, f32_to_pcm16_le(&[0.5, 0.25]));
        let mut output = vec![0f32; 30];
        render_output(&mut output, 1, &fixture.playback);
        assert!(output.iter().all(|sample| *sample == 0.25));
        tokio::time::timeout(Duration::from_secs(2), flush(&fixture.preparation))
            .await
            .unwrap();
        let mut next = [0f32];
        render_output(&mut next, 1, &fixture.playback);
        assert_eq!(next[0], crate::audio::pcm16_le_to_f32(&[0, 64])[0]);
        drop(fixture.preparation);
        fixture
            .custody
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }
    #[tokio::test]
    async fn shutdown_unparks_dsp_with_full_playback_without_needing_device_progress() {
        let quota = crate::test_quota();
        let fixture = fixture(&quota, 1);
        while fixture.playback.push(1, &[0.25; 512]) != 0 {}
        fixture
            .preparation
            .enqueue(1, &[0, 64, 0, 32])
            .await
            .unwrap();
        // No device ever consumes the full ring. Logical Drop must wake the
        // original physical owner rather than waiting for output capacity.
        drop(fixture.preparation);
        fixture
            .custody
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(fixture.custody.pending_owners(), 0);
        assert_eq!(quota.snapshot().worker_threads, 0);
    }
    #[tokio::test]
    async fn blocked_dsp_retains_admission_after_logical_drop() {
        let quota = crate::test_quota();
        let fixture = fixture(&quota, 24000);
        let (release, blocked) = std::sync::mpsc::channel();
        let (entered, acknowledge) = tokio::sync::oneshot::channel();
        fixture
            .preparation
            .sender
            .send(PreparationCommand::Gate(blocked, entered))
            .await
            .unwrap();
        if let Some(thread) = fixture.raw.wake.get() {
            thread.unpark();
        }
        acknowledge.await.unwrap();
        drop(fixture.preparation);
        assert_eq!(quota.snapshot().worker_threads, 1);
        assert_eq!(quota.snapshot().worker_bytes, 68 * 1024 * 1024 + 128 * 1024);
        assert!(fixture.custody.join_until(Instant::now()).is_err());
        release.send(()).unwrap();
        fixture
            .custody
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(quota.snapshot().worker_threads, 0);
    }
    #[test]
    fn dsp_quota_refusal_precedes_os_spawn() {
        let mut limits = crate::test_quota().snapshot().limits;
        limits.worker_threads = 0;
        let quota = QuotaGroup::new(limits);
        let custody = AudioCustody::new(&quota).unwrap();
        let _storage = quota.reserve_external_storage(64 * 1024 * 1024).unwrap();
        let raw = Arc::new(RawCapture::new());
        let samples = Arc::new(PlaybackRing::new(
            1,
            Arc::new(AtomicU64::new(1)),
            raw.wake.clone(),
        ));
        let (sender, _receiver) = mpsc::channel(1);
        assert!(OutputPreparation::start(
            1,
            1.,
            samples,
            CapturePreparation {
                raw,
                sender,
                sample_rate: 24000
            },
            &quota,
            &custody,
            None
        )
        .is_err());
        assert_eq!(custody.pending_owners(), 0);
    }
}
