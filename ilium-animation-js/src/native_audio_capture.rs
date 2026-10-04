//! Fixed-endpoint native PCM capture. Only trusted host code constructs bindings.
//! No PATH lookup, default-device switching, CPAL fallback, or spectral work.
use crate::{
    error::{AnimationError, Result},
    native_audio::{
        AudioDemand, AudioProduct, AudioSourceSelection, AuthenticatedAudioGrant,
        AuthenticatedCaptureFactory, OwnedAudioCapture,
    },
    permissions::{
        AudioProduct as PermissionProduct, BindingKind, CallPhase, Capability, Channel,
        HostBinding, OperationNeed, PermissionBroker, Right, Scope,
    },
};
use ilium_ambient::{
    native_pipewire_audio_command, native_pulse_audio_command,
    resources::{AmbientResources, WorkerCost},
    NativeAudioPcmDecoder, NativeAudioTarget,
};
use ilium_execution::QuotaGroup;
use ilium_platform::owned_worker::{spawn_owned, OwnedWorker, StopToken, WorkerExit, WorkerKind};
use std::{
    collections::BTreeSet,
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
const RING_SAMPLES: usize = 8192;
const OWN_RESIDENT: usize = 4 * 1024 * 1024;
fn invalid(message: impl Into<String>) -> AnimationError {
    AnimationError::Runtime(message.into())
}
fn auth(error: crate::permissions::PermissionError) -> AnimationError {
    AnimationError::PermissionDenied(error.to_string())
}
#[derive(Clone, Copy)]
pub enum CaptureBackend {
    PipeWire,
    Pulse,
}
/// Qualification is supplied by the native host, never inferred from the helper
/// name. Accounting bounds are reservations, not a physical RSS containment claim.
pub struct QualifiedCaptureBinding {
    selector: AudioSourceSelection,
    endpoint: String,
    scope_device: String,
    capability: Capability,
    binding: HostBinding,
    program: PathBuf,
    backend: CaptureBackend,
    helper_cost: WorkerCost,
}
impl QualifiedCaptureBinding {
    #[allow(clippy::too_many_arguments)] // One host-selected resource's complete qualification.
    pub fn from_host(
        selector: AudioSourceSelection,
        endpoint: String,
        scope_device: String,
        capability: Capability,
        opaque_device_id: String,
        program: PathBuf,
        backend: CaptureBackend,
        helper_cost: WorkerCost,
    ) -> Result<Self> {
        let expected = match backend {
            CaptureBackend::PipeWire => "pw-record",
            CaptureBackend::Pulse => "parec",
        };
        if !program.is_absolute()
            || program.file_name().and_then(|value| value.to_str()) != Some(expected)
            || [&endpoint, &scope_device].iter().any(|value| {
                value.is_empty() || value.len() > 256 || value.chars().any(char::is_control)
            })
            || endpoint.starts_with('-')
            || opaque_device_id.is_empty()
            || helper_cost.threads == 0
            || helper_cost.threads > 32
            || helper_cost.resident_bytes == 0
            || helper_cost.resident_bytes > 256 * 1024 * 1024
            || !matches!(
                capability,
                Capability::AudioLoopback | Capability::AudioMicrophone
            )
            || matches!(selector, AudioSourceSelection::Loopback)
                && capability != Capability::AudioLoopback
            || matches!(selector, AudioSourceSelection::Microphone)
                && capability != Capability::AudioMicrophone
        {
            return Err(invalid("invalid qualified native audio binding"));
        }
        Ok(Self {
            selector,
            endpoint,
            scope_device,
            capability,
            binding: HostBinding::new(BindingKind::AudioDevice, opaque_device_id).map_err(auth)?,
            program,
            backend,
            helper_cost,
        })
    }
    pub fn binding(&self) -> &HostBinding {
        &self.binding
    }
}
struct Ring {
    samples: Vec<f32>,
    write: usize,
    filled: usize,
    ready: bool,
    ended: bool,
}
impl Ring {
    fn new() -> Self {
        Self {
            samples: vec![0.; RING_SAMPLES],
            write: 0,
            filled: 0,
            ready: false,
            ended: false,
        }
    }
    fn push(&mut self, samples: &[f32]) {
        for &sample in samples {
            self.samples[self.write] = sample.clamp(-1., 1.);
            self.write = (self.write + 1) % RING_SAMPLES;
            self.filled = (self.filled + 1).min(RING_SAMPLES);
        }
        self.ready |= !samples.is_empty();
    }
    fn drain(&mut self, out: &mut [f32]) -> usize {
        let count = out.len().min(self.filled);
        let start = (self.write + RING_SAMPLES - self.filled) % RING_SAMPLES;
        for (index, target) in out[..count].iter_mut().enumerate() {
            *target = self.samples[(start + index) % RING_SAMPLES];
        }
        self.filled -= count;
        count
    }
}
struct Control {
    child: Option<Child>,
    stopping: bool,
}
struct Data {
    ring: Mutex<Ring>,
    changed: Condvar,
}
/// Must run on a dedicated admitted host preparation thread, never an Execution
/// bank callback or UI thread: open waits at most two seconds
/// for actual PCM, outside the broker lock. The caller's final snapshot-to-JS
/// delivery still requires its own current authorization fence.
pub struct NativeAudioCaptureFactory {
    selected: QualifiedCaptureBinding,
    broker: Arc<Mutex<PermissionBroker>>,
    channel: Channel,
    demand_id: String,
    quota: QuotaGroup,
}
impl NativeAudioCaptureFactory {
    pub fn new(
        selected: QualifiedCaptureBinding,
        broker: Arc<Mutex<PermissionBroker>>,
        channel: Channel,
        demand_id: String,
        quota: QuotaGroup,
    ) -> Result<Self> {
        if demand_id.is_empty() || demand_id.len() > 256 {
            return Err(invalid("invalid native audio demand id"));
        }
        Ok(Self {
            selected,
            broker,
            channel,
            demand_id,
            quota,
        })
    }
    fn need(&self, products: &BTreeSet<AudioProduct>) -> Result<OperationNeed> {
        let products = products
            .iter()
            .map(|product| match product {
                AudioProduct::Level => PermissionProduct::Level,
                AudioProduct::Waveform => PermissionProduct::Waveform,
                AudioProduct::Envelope => PermissionProduct::Envelope,
                AudioProduct::Bands => PermissionProduct::Bands,
                AudioProduct::History => PermissionProduct::History,
            })
            .collect();
        OperationNeed::new(
            Right {
                id: self.selected.capability,
                scope: Scope::Audio {
                    device: self.selected.scope_device.clone(),
                    products,
                },
            },
            Some(self.selected.binding.clone()),
        )
        .map_err(auth)
    }
}
impl AuthenticatedCaptureFactory for NativeAudioCaptureFactory {
    fn open(
        &mut self,
        grant: &AuthenticatedAudioGrant,
        demand: &AudioDemand,
        resources: &AmbientResources,
    ) -> Result<Box<dyn OwnedAudioCapture>> {
        demand.validate()?;
        if demand.is_empty()
            || demand.sample_rate != 44100
            || grant.source() != &self.selected.selector
            || !self.quota.shares_root(&resources.finite().quota_group())
        {
            return Err(invalid("capture demand/source/quota mismatch"));
        }
        let need = self.need(&demand.products)?;
        let reservation = resources
            .reserve_worker(WorkerCost {
                threads: self.selected.helper_cost.threads + 2,
                resident_bytes: self.selected.helper_cost.resident_bytes + OWN_RESIDENT,
            })
            .map_err(|error| {
                AnimationError::Budget(format!("audio capture admission: {error:?}"))
            })?;
        let mut command = match self.selected.backend {
            CaptureBackend::PipeWire => native_pipewire_audio_command(&NativeAudioTarget::Named(
                self.selected.endpoint.clone(),
            )),
            CaptureBackend::Pulse => native_pulse_audio_command(&NativeAudioTarget::Named(
                self.selected.endpoint.clone(),
            )),
        };
        // Builders are native originals; only the qualified absolute executable overrides their static program name.
        if command.channels != 2 || command.sample_rate != 44100 {
            return Err(invalid("unsupported qualified PCM contract"));
        }
        let program = self.selected.program.clone();
        let broker = Arc::clone(&self.broker);
        let channel = self.channel.clone();
        let demand_id = self.demand_id.clone();
        let data = Arc::new(Data {
            ring: Mutex::new(Ring::new()),
            changed: Condvar::new(),
        });
        let control = Arc::new(Mutex::new(Control {
            child: None,
            stopping: false,
        }));
        let wake_control = Arc::clone(&control);
        let body_control = Arc::clone(&control);
        let body_data = Arc::clone(&data);
        let body_need = need.clone();
        let worker = spawn_owned(
            "animation-audio-pcm",
            WorkerKind::SynchronousIo,
            StopToken::default(),
            move || {
                let _keep_admission = &reservation;
                let mut control = wake_control
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                control.stopping = true;
                if let Some(child) = control.child.as_mut() {
                    let _ = child.kill();
                }
            },
            move |stop| {
                let ticket = {
                    broker
                        .lock()
                        .map_err(|_| invalid("native audio authority poisoned"))
                        .and_then(|mut broker| {
                            broker
                                .dispatch(&channel, CallPhase::Create, &demand_id, vec![body_need])
                                .map_err(auth)
                        })
                };
                if let Ok(ticket) = ticket {
                    if !stop.is_stopped() {
                        let spawned = broker
                            .lock()
                            .map_err(|_| invalid("native audio authority poisoned"))
                            .and_then(|mut broker| {
                                broker
                                    .commit(&ticket, || {
                                        Command::new(program)
                                            .args(std::mem::take(&mut command.args))
                                            .stdin(Stdio::null())
                                            .stdout(Stdio::piped())
                                            .stderr(Stdio::null())
                                            .spawn()
                                    })
                                    .map_err(auth)
                            });
                        if let Ok(Ok(mut child)) = spawned {
                            let stdout = child.stdout.take();
                            {
                                let mut control = body_control
                                    .lock()
                                    .unwrap_or_else(|error| error.into_inner());
                                if control.stopping || stop.is_stopped() {
                                    let _ = child.kill();
                                }
                                control.child = Some(child);
                            }
                            if let Some(mut stdout) = stdout {
                                let mut decoder =
                                    NativeAudioPcmDecoder::new(command.format, command.channels);
                                let mut bytes = [0u8; 8192];
                                let mut mono = Vec::with_capacity(1024);
                                while !stop.is_stopped() {
                                    match stdout.read(&mut bytes) {
                                        Ok(0) => break,
                                        Ok(count) => {
                                            mono.clear();
                                            decoder.push(&bytes[..count], &mut mono);
                                            body_data
                                                .ring
                                                .lock()
                                                .unwrap_or_else(|error| error.into_inner())
                                                .push(&mono);
                                            body_data.changed.notify_all();
                                        }
                                        Err(error)
                                            if error.kind() == std::io::ErrorKind::Interrupted =>
                                        {
                                            continue;
                                        }
                                        Err(_) => break,
                                    }
                                }
                            }
                            // The wake may kill but never waits. Actual exit remains in this worker's custody.
                            let child = body_control
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .child
                                .take();
                            if let Some(mut child) = child {
                                let _ = child.kill();
                                while child.wait().is_err() {
                                    std::thread::yield_now();
                                }
                            }
                        }
                    }
                    if let Ok(mut broker) = broker.lock() {
                        let _ = broker.settle_without_delivery(&ticket);
                    } // Poison means no continued authority; actual worker still retires its child.
                }
                body_data
                    .ring
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .ended = true;
                body_data.changed.notify_all();
            },
        )
        .map_err(|error| invalid(format!("owned audio worker: {error}")))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut ring = data.ring.lock().unwrap_or_else(|error| error.into_inner());
        while !ring.ready && !ring.ended {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            ring = data
                .changed
                .wait_timeout(ring, remaining)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        if !ring.ready {
            drop(ring);
            worker.ticket().cancel();
            return Err(invalid(
                "native audio helper produced no PCM before startup deadline",
            ));
        }
        drop(ring);
        Ok(Box::new(ProcessCapture {
            worker,
            data,
            broker: Arc::clone(&self.broker),
            channel: self.channel.clone(),
            demand_id: self.demand_id.clone(),
            need,
        }))
    }
}
struct ProcessCapture {
    data: Arc<Data>,
    broker: Arc<Mutex<PermissionBroker>>,
    channel: Channel,
    demand_id: String,
    need: OperationNeed,
    worker: OwnedWorker,
}
impl OwnedAudioCapture for ProcessCapture {
    fn sample_rate(&self) -> u32 {
        44100
    }
    fn read_mono(&mut self, out: &mut [f32]) -> Result<usize> {
        if out.len() > RING_SAMPLES {
            return Err(invalid("native PCM read exceeds bound"));
        }
        let mut broker = self
            .broker
            .lock()
            .map_err(|_| invalid("native audio authority poisoned"))?;
        let ticket = broker
            .dispatch(
                &self.channel,
                CallPhase::Async,
                &self.demand_id,
                vec![self.need.clone()],
            )
            .map_err(auth)?;
        let result = broker.commit(&ticket, || ()).and_then(|()| {
            broker.deliver(&ticket, || {
                let mut ring = self
                    .data
                    .ring
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if ring.ended && ring.filled == 0 {
                    Err(invalid("native PCM source ended"))
                } else {
                    Ok(ring.drain(out))
                }
            })
        });
        match result {
            Ok(value) => value,
            Err(error) => {
                let _ = broker.settle_without_delivery(&ticket);
                Err(auth(error))
            }
        }
    }
    fn request_stop(&mut self) {
        self.worker.ticket().cancel();
    }
    fn join_until(&mut self, deadline: Instant) -> Result<bool> {
        match self.worker.ticket().join_until(deadline) {
            Ok(WorkerExit::Joined) => Ok(true),
            Ok(WorkerExit::Panicked) => Err(invalid("audio worker panicked")),
            Err(_) => Ok(false),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_is_bounded_and_keeps_latest_order() {
        let mut ring = Ring::new();
        for index in 0..RING_SAMPLES + 3 {
            ring.push(&[index as f32 / 10000.]);
        }
        let mut out = [0.; 4];
        assert_eq!(ring.drain(&mut out), 4);
        assert_eq!(out, [0.0003, 0.0004, 0.0005, 0.0006]);
        assert_eq!(ring.samples.len(), RING_SAMPLES);
        assert_eq!(ring.filled, RING_SAMPLES - 4);
    }
    #[test]
    fn native_pcm_decoder_handles_partial_frames_without_spectral_work() {
        let command =
            native_pulse_audio_command(&NativeAudioTarget::Named("fixture.monitor".into()));
        let mut decoder = NativeAudioPcmDecoder::new(command.format, command.channels);
        let bytes = [0.5f32.to_le_bytes(), (-0.25f32).to_le_bytes()].concat();
        let mut mono = Vec::new();
        decoder.push(&bytes[..3], &mut mono);
        assert!(mono.is_empty());
        decoder.push(&bytes[3..], &mut mono);
        assert_eq!(mono, vec![0.125]);
    }
}
