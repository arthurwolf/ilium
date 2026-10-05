//! Original-instance audio producer. Only the animation actor consumes results;
//! opening and physical retirement run as separately admitted finite IO jobs.
use ilium_ambient::resources::AmbientResources;
use ilium_animation_js::{
    helper::HelperAuthority,
    native_audio::{
        AudioDemand, AuthenticatedAudioGrant, NativeAudioService, RetainedAudioSnapshot,
    },
    native_audio_capture::{NativeAudioCaptureFactory, QualifiedCaptureBinding},
    runtime::PackageInstance,
};
use ilium_execution::{
    Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, Receipt, Reservation,
    Retention,
};
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const AUDIO_JOB_BYTES: usize = 64 * 1024;
static AUDIO_CLOCK_ORIGIN: OnceLock<Instant> = OnceLock::new();

fn audio_clock() -> Result<(u64, u64), String> {
    let now = Instant::now();
    let origin = AUDIO_CLOCK_ORIGIN.get_or_init(|| now);
    let monotonic = u64::try_from(now.saturating_duration_since(*origin).as_millis())
        .map_err(|_| "Audio monotonic clock range".to_owned())?;
    let epoch = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Audio civil clock precedes Unix epoch".to_owned())?
            .as_millis(),
    )
    .map_err(|_| "Audio civil clock range".to_owned())?;
    Ok((monotonic, epoch))
}

struct AudioOpenJob {
    demand: AudioDemand,
    grant: AuthenticatedAudioGrant,
    authority: HelperAuthority,
    factory: NativeAudioCaptureFactory,
    resources: AmbientResources,
    quota: QuotaGroup,
}
impl Job for AudioOpenJob {
    type Output = NativeAudioService;
    type Error = String;

    fn run(mut self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Err("Audio preparation cancelled before capture open".into());
        }
        let mut service = NativeAudioService::open(
            self.demand,
            self.grant,
            self.authority,
            &self.resources,
            self.quota,
            &mut self.factory,
        )
        .map_err(|error| error.to_string())?;
        if context.stop_requested() {
            service.cancel();
        }
        Ok(service) // Even a late cancellation returns original join custody.
    }
}

struct AudioCloseJob {
    service: NativeAudioService,
}
struct AudioCloseFailure {
    message: String,
    service: NativeAudioService,
}
impl Job for AudioCloseJob {
    type Output = ();
    type Error = AudioCloseFailure;

    fn run(mut self, _context: JobContext) -> Result<Self::Output, Self::Error> {
        self.service.cancel();
        loop {
            let Some(deadline) = Instant::now().checked_add(Duration::from_secs(30)) else {
                return Err(AudioCloseFailure {
                    message: "Audio retirement deadline range".into(),
                    service: self.service,
                });
            };
            match self.service.retire(deadline) {
                Ok(true) => return Ok(()), // Physical child and reader join proved.
                Ok(false) => continue,     // The original worker and all charges remain owned.
                Err(error) => {
                    return Err(AudioCloseFailure {
                        message: error.to_string(),
                        service: self.service,
                    });
                }
            }
        }
    }
}

/// One accepted scene owns its opening receipt, live capture, close reservation
/// and physical close receipt. A lost/panicked receipt is unresolved custody.
pub(super) struct AudioOwner {
    opening: Option<Receipt<AudioOpenJob>>,
    active: Option<NativeAudioService>,
    closing: Option<Receipt<AudioCloseJob>>,
    close_reservation: Option<Reservation>,
    open_retention: Option<Retention>,
    authority: Option<HelperAuthority>,
    stopped: bool,
    unresolved: bool,
    fault: Option<String>,
}
impl AudioOwner {
    pub(super) fn new(
        instance: &PackageInstance,
        selected: Option<QualifiedCaptureBinding>,
        resources: &AmbientResources,
        quota: &QuotaGroup,
        live_mode: bool,
    ) -> Result<Self, String> {
        let mut owner = Self {
            opening: None,
            active: None,
            closing: None,
            close_reservation: None,
            open_retention: None,
            authority: None,
            stopped: false,
            unresolved: false,
            fault: None,
        };
        if instance.plan().inputs.audio.is_none() {
            return Ok(owner); // No demand means no capture, reservation, or helper.
        }
        if !live_mode {
            return Err("Pre-rendered audio requires an authenticated frozen recording".into());
        }
        let selected = selected.ok_or("Accepted audio plan has no qualified host device")?;
        let (demand, grant, factory) = instance
            .selected_audio_capture(selected)
            .map_err(|error| error.to_string())?
            .ok_or("Accepted audio plan was pruned before acquisition")?;
        let authority = instance
            .frame_authority()
            .ok_or("Original accepted audio activation unavailable")?;
        if !resources.finite().quota_group().shares_root(quota) || !instance.shares_root(quota) {
            return Err("Foreign original audio quota root".into());
        }
        // Reserve the eventual close slot BEFORE opening. Cancellation cannot
        // lose physical join merely because every ordinary IO slot is full.
        let close_reservation = resources
            .finite()
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes: AUDIO_JOB_BYTES,
                    result_bytes: AUDIO_JOB_BYTES,
                },
            )
            .map_err(|error| format!("Audio close admission: {error:?}"))?;
        let open_reservation = resources
            .finite()
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes: AUDIO_JOB_BYTES,
                    result_bytes: AUDIO_JOB_BYTES,
                },
            )
            .map_err(|error| format!("Audio open admission: {error:?}"))?;
        let job = AudioOpenJob {
            demand,
            grant,
            authority: authority.clone(),
            factory,
            resources: resources.clone(),
            quota: quota.clone(),
        };
        owner.opening = Some(
            open_reservation
                .submit(job)
                .map_err(|rejected| format!("Audio open publication: {:?}", rejected.reason))?,
        );
        owner.close_reservation = Some(close_reservation);
        owner.authority = Some(authority);
        Ok(owner)
    }

    fn begin_close(&mut self, mut service: NativeAudioService) {
        service.cancel();
        let Some(reservation) = self.close_reservation.take() else {
            self.active = Some(service);
            self.unresolved = true;
            self.fault = Some("Original audio close reservation missing".into());
            return;
        };
        match reservation.submit(AudioCloseJob { service }) {
            Ok(receipt) => self.closing = Some(receipt),
            Err(rejected) => {
                self.active = Some(rejected.value.service);
                self.unresolved = true;
                self.fault = Some(format!("Audio close publication: {:?}", rejected.reason));
            }
        }
    }

    pub(super) fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        if let Some(receipt) = &self.opening {
            receipt.cancel();
        }
        if let Some(service) = self.active.take() {
            self.begin_close(service);
        }
    }

    pub(super) fn collect_on_wake(&mut self) -> Result<(), String> {
        if let Some(receipt) = self.opening.as_mut() {
            let outcome = match receipt.try_take() {
                JobPoll::Pending => None,
                JobPoll::Ready(outcome) => Some(outcome),
                JobPoll::Lost | JobPoll::Taken => {
                    self.unresolved = true;
                    return Err("Original audio open receipt lost".into());
                }
            };
            if let Some(outcome) = outcome {
                self.opening = None;
                let (outcome, retention) = outcome.into_parts();
                self.open_retention = Some(retention);
                match outcome {
                    JobOutcome::Finished(Ok(service)) if self.stopped => self.begin_close(service),
                    JobOutcome::Finished(Ok(service)) => self.active = Some(service),
                    JobOutcome::Finished(Err(error)) => {
                        self.fault = Some(error.clone());
                        return Err(error);
                    }
                    JobOutcome::NotStarted { .. } if self.stopped => {}
                    JobOutcome::NotStarted { .. } => {
                        return Err("Audio open job did not start".into());
                    }
                    JobOutcome::Panicked => {
                        self.unresolved = true;
                        return Err("Audio open job panicked; capture custody unknown".into());
                    }
                }
            }
        }
        if let Some(receipt) = self.closing.as_mut() {
            let outcome = match receipt.try_take() {
                JobPoll::Pending => None,
                JobPoll::Ready(outcome) => Some(outcome),
                JobPoll::Lost | JobPoll::Taken => {
                    self.unresolved = true;
                    return Err("Original audio close receipt lost".into());
                }
            };
            if let Some(outcome) = outcome {
                self.closing = None;
                let (outcome, _retention) = outcome.into_parts();
                match outcome {
                    JobOutcome::Finished(Ok(())) => self.open_retention = None,
                    JobOutcome::Finished(Err(failure)) => {
                        self.active = Some(failure.service);
                        self.unresolved = true;
                        self.fault = Some(failure.message.clone());
                        return Err(failure.message);
                    }
                    JobOutcome::NotStarted { job, .. } => {
                        self.active = Some(job.service);
                        self.unresolved = true;
                        return Err("Audio close job did not start".into());
                    }
                    JobOutcome::Panicked => {
                        self.unresolved = true;
                        return Err("Audio close job panicked; physical join unknown".into());
                    }
                }
            }
        }
        if let Some(error) = &self.fault {
            return Err(error.clone());
        }
        Ok(())
    }

    pub(super) fn snapshot(
        &mut self,
    ) -> ilium_animation_js::error::Result<Option<Arc<RetainedAudioSnapshot>>> {
        if self.stopped {
            return Ok(None);
        }
        let Some(service) = self.active.as_mut() else {
            return Ok(None); // Preparation has not produced a live capture.
        };
        let authority = self.authority.as_ref().ok_or_else(|| {
            ilium_animation_js::error::AnimationError::PermissionDenied(
                "Original audio activation missing".into(),
            )
        })?;
        let (monotonic_ms, captured_at_epoch_ms) = audio_clock()
            .map_err(|error| ilium_animation_js::error::AnimationError::Runtime(error))?;
        match service.poll(
            monotonic_ms,
            captured_at_epoch_ms,
            authority.authorization_epoch,
        ) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                self.stop();
                Err(error)
            }
        }
    }

    pub(super) fn is_drained(&self) -> bool {
        self.opening.is_none()
            && self.active.is_none()
            && self.closing.is_none()
            && !self.unresolved
    }
}
