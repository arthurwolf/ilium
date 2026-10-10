//! Finite CPU preparation and one ordered environment-I/O phase on the shared bank.
use crate::{config::VoiceSettings, control::VoiceTargetContext};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason, SkipReason,
    StorageAdmission,
};
use ilium_voice::{OwnedVoiceContext, OwnedVoiceStartup, VoiceRuntimeConfig, VoiceTextAllocation};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Notify;

#[derive(Clone, Debug)]
pub(crate) enum PreparationKind {
    NormalStartup {
        target: VoiceTargetContext,
    },
    DemoStartup,
    Context {
        target: VoiceTargetContext,
        actor: Arc<()>,
    },
}
impl PreparationKind {
    pub fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::NormalStartup { target: left }, Self::NormalStartup { target: right }) => {
                left == right
            }
            (Self::DemoStartup, Self::DemoStartup) => true,
            (
                Self::Context {
                    target: left,
                    actor: a,
                },
                Self::Context {
                    target: right,
                    actor: b,
                },
            ) => left == right && Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}
#[derive(Debug)]
pub(crate) enum PrepareRefusal {
    Admission(RejectReason),
    Limit,
    Closed,
    Unavailable,
}
impl PrepareRefusal {
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Admission(reason) => !matches!(
                reason,
                RejectReason::Closed | RejectReason::InvalidCost | RejectReason::AccountingPoisoned
            ),
            _ => false,
        }
    }
}
struct Capture {
    generation: u64,
    settings: VoiceSettings,
    kind: PreparationKind,
    bytes: usize,
    allocation: Arc<StorageAdmission>,
}
impl std::fmt::Debug for Capture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoiceCapture")
            .field("kind", &self.kind)
            .field("declared_bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}
impl VoiceTextAllocation for Capture {}
struct Request {
    generation: u64,
    environment_key: Option<String>,
    source: Arc<Capture>,
}
pub(crate) enum PreparedValue {
    Startup(OwnedVoiceStartup),
    Context(OwnedVoiceContext),
}
pub(crate) struct PreparedVoice {
    pub value: Result<PreparedValue, String>,
    #[cfg(test)]
    prepared_on: std::thread::ThreadId,
    source: Arc<Capture>,
}
impl PreparedVoice {
    pub fn settings(&self) -> &VoiceSettings {
        &self.source.settings
    }
    pub fn kind(&self) -> &PreparationKind {
        &self.source.kind
    }
    pub fn allocation(&self) -> Arc<StorageAdmission> {
        self.source.allocation.clone()
    }
}
enum Phase {
    Environment(Request),
    Ready(PreparedVoice),
}
struct Prepare {
    request: Request,
    environment: bool,
}
impl Job for Prepare {
    type Output = Phase;
    type Error = std::convert::Infallible;
    fn run(mut self, context: JobContext) -> Result<Phase, Self::Error> {
        let failed = |source: Arc<Capture>, error: String| {
            Phase::Ready(PreparedVoice {
                source,
                value: Err(error),
                #[cfg(test)]
                prepared_on: std::thread::current().id(),
            })
        };
        if context.stop_requested() {
            return Ok(failed(
                self.request.source,
                "Voice preparation cancelled before construction".into(),
            ));
        }
        if self.environment {
            // No safe capped-copy environment reader exists in platform yet.
            // This finite IO phase preserves current behavior, caps accepted
            // originals before further copies and never repeats a completed read.
            let key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
            if key.len() > 64 * 1024 {
                return Ok(failed(self.request.source,"Voice environment original exceeds64KiB; bounded-copy platform reader remains required".into()));
            }
            self.request.environment_key = Some(key);
            return Ok(Phase::Environment(self.request));
        }
        let source = self.request.source;
        let settings = &source.settings;
        let (instructions, tools) = match &source.kind {
            PreparationKind::NormalStartup { target } | PreparationKind::Context { target, .. } => {
                (
                    crate::control::system_instructions(&settings.custom_prompt, *target),
                    crate::control::ControlPlane::default().tool_definitions(),
                )
            }
            PreparationKind::DemoStartup => (
                ilium_prompts::render_value("voice/onboarding-lightbulb", &serde_json::json!({})),
                crate::onboarding::voice_runtime::tool_definitions(),
            ),
        };
        let hold: Arc<dyn VoiceTextAllocation> = source.clone();
        let value = if matches!(source.kind, PreparationKind::Context { .. }) {
            OwnedVoiceContext::charged(instructions, tools, source.bytes, hold)
                .map(PreparedValue::Context)
                .map_err(|error| error.to_string())
        } else {
            let key = if settings.api_key.trim().is_empty() {
                self.request.environment_key.take().unwrap_or_default()
            } else {
                settings.api_key.clone()
            };
            let config = VoiceRuntimeConfig {
                api_key: key.into(),
                model: settings.model,
                voice: settings.voice,
                reasoning_effort: settings.reasoning_effort,
                input_mode: settings.input_mode,
                vad_eagerness: settings.vad_eagerness,
                input_device_name: settings.input_device_name.clone(),
                output_device_name: settings.output_device_name.clone(),
                output_volume_percent: settings.output_volume_percent,
                instructions,
            };
            OwnedVoiceStartup::charged(config, tools, source.bytes, hold)
                .map(PreparedValue::Startup)
                .map_err(|error| error.to_string())
        };
        Ok(Phase::Ready(PreparedVoice {
            source,
            value,
            #[cfg(test)]
            prepared_on: std::thread::current().id(),
        }))
    }
}
/// One active/retiring callback and one latest desired capture. Completed IO
/// keys, settings and storage declarations retain the original request identity.
pub(crate) struct VoicePreparation {
    client: Option<Client>,
    notification: Arc<Notify>,
    pending: Option<Request>,
    active: Option<(u64, Receipt<Prepare>, Arc<Capture>)>,
    generation: u64,
    retry_at: Option<Instant>,
    closed: bool,
    terminal: Option<PreparedVoice>,
    deferred: Option<PreparedVoice>,
}
impl Default for VoicePreparation {
    fn default() -> Self {
        let mut value = Self {
            client: {
                #[cfg(test)]
                {
                    Some(crate::execution::test_client())
                }
                #[cfg(not(test))]
                {
                    None
                }
            },
            notification: Arc::new(Notify::new()),
            pending: None,
            active: None,
            generation: 0,
            retry_at: None,
            closed: false,
            terminal: None,
            deferred: None,
        };
        if let Some(client) = value.client.take() {
            value.configure(client);
        }
        value
    }
}
impl VoicePreparation {
    #[cfg(test)]
    pub fn new(client: Client) -> Self {
        let mut owner = Self::default();
        owner.configure(client);
        owner
    }
    pub fn configure(&mut self, client: Client) {
        let wake = self.notification.clone();
        self.client = Some(client.with_completion_wake(move || wake.notify_one()));
    }
    pub fn new_with_notification(client: Client, notification: Arc<Notify>) -> Self {
        let mut owner = Self {
            notification,
            ..Self::default()
        };
        owner.configure(client);
        owner
    }
    pub fn retry_delay(&self, now: Instant) -> Option<Duration> {
        if self.closed
            || (self.pending.is_none() && self.deferred.is_none())
            || self.active.is_some()
        {
            return None;
        }
        self.retry_at
            .map(|deadline| deadline.saturating_duration_since(now))
    }
    pub fn notification(&self) -> Arc<Notify> {
        self.notification.clone()
    }
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
            || self.active.is_some()
            || self.terminal.is_some()
            || self.deferred.is_some()
    }
    pub fn request(
        &mut self,
        settings: &VoiceSettings,
        kind: PreparationKind,
    ) -> Result<(), PrepareRefusal> {
        if self.closed {
            return Err(PrepareRefusal::Closed);
        }
        if self.client.is_none() {
            return Err(PrepareRefusal::Unavailable);
        }
        let same = |source: &Capture| source.settings == *settings && source.kind.matches(&kind);
        if self
            .deferred
            .as_ref()
            .is_some_and(|ready| same(&ready.source))
            || self
                .pending
                .as_ref()
                .is_some_and(|request| same(&request.source))
            || self.pending.is_none()
                && self.active.as_ref().is_some_and(|(generation, _, source)| {
                    *generation == self.generation && same(source)
                })
        {
            return Ok(());
        }
        let (template, schema) = if matches!(kind, PreparationKind::DemoStartup) {
            (
                ilium_prompts::voice::ONBOARDING_LIGHTBULB.len(),
                include_str!("onboarding/voice_demo.rs").len(),
            )
        } else {
            (
                ilium_prompts::voice::VOICE_MOD_SYSTEM_INSTRUCTIONS.len(),
                include_str!("control/tools.rs").len(),
            )
        };
        let bytes = crate::normal_voice::capture_bytes(settings, template, schema)
            .and_then(|bytes| bytes.checked_add(4096))
            .ok_or(PrepareRefusal::Limit)?;
        let allocation = Arc::new(
            self.client
                .as_ref()
                .ok_or(PrepareRefusal::Unavailable)?
                .quota_group()
                .reserve_external_storage(bytes)
                .map_err(PrepareRefusal::Admission)?,
        );
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(PrepareRefusal::Limit)?;
        self.generation = generation;
        self.terminal = None;
        self.deferred = None;
        self.pending = Some(Request {
            source: Arc::new(Capture {
                generation,
                settings: settings.clone(),
                kind,
                bytes,
                allocation,
            }),
            generation,
            environment_key: None,
        });
        if let Some((_, receipt, _)) = &self.active {
            receipt.cancel();
        }
        self.retry_at = None;
        self.pump();
        Ok(())
    }
    fn pump(&mut self) {
        if self.closed
            || self.active.is_some()
            || self
                .retry_at
                .is_some_and(|deadline| Instant::now() < deadline)
        {
            return;
        }
        let Some(request) = self.pending.take() else {
            return;
        };
        let Some(client) = &self.client else {
            self.pending = Some(request);
            return;
        };
        let environment = !matches!(request.source.kind, PreparationKind::Context { .. })
            && request.source.settings.api_key.trim().is_empty()
            && request.environment_key.is_none();
        let lane = if environment { Lane::Io } else { Lane::Cpu };
        let generation = request.generation;
        let source = request.source.clone();
        match client.try_submit(
            lane,
            JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            },
            Prepare {
                request,
                environment,
            },
        ) {
            Ok(receipt) => {
                self.retry_at = None;
                self.active = Some((generation, receipt, source));
            }
            Err(original) => {
                if matches!(
                    original.reason,
                    RejectReason::Closed
                        | RejectReason::InvalidCost
                        | RejectReason::AccountingPoisoned
                ) {
                    self.retry_at = None;
                    self.terminal = Some(PreparedVoice {
                        source: original.value.request.source,
                        value: Err(format!(
                            "Voice preparation owner rejected original: {:?}",
                            original.reason
                        )),
                        #[cfg(test)]
                        prepared_on: std::thread::current().id(),
                    });
                    self.notification.notify_one();
                } else {
                    self.pending = Some(original.value.request);
                    self.retry_at = Some(Instant::now() + Duration::from_millis(20));
                }
            }
        }
    }
    /// Retain the exact ready source while actor metadata is unavailable.
    /// A stale/closed capture is explicitly cancelled, never retried or rebuilt.
    pub fn defer_ready(&mut self, prepared: PreparedVoice, now: Instant) -> bool {
        if self.closed || prepared.source.generation != self.generation || self.deferred.is_some() {
            return false;
        }
        self.deferred = Some(prepared);
        self.retry_at = Some(now + Duration::from_millis(20));
        true
    }
    pub fn collect(&mut self) -> Option<PreparedVoice> {
        if self.deferred.is_some() {
            if self
                .retry_at
                .is_some_and(|deadline| Instant::now() < deadline)
            {
                return None;
            }
            self.retry_at = None;
            return self.deferred.take();
        }
        let mut result = self.terminal.take();
        if let Some((_, receipt, _)) = &mut self.active {
            let polled = match receipt.try_take() {
                JobPoll::Pending => None,
                JobPoll::Ready(outcome) => Some(Some(outcome.into_parts().0)),
                _ => Some(None),
            };
            if let Some(polled) = polled {
                let (generation, _, source) = self.active.take()?;
                if generation == self.generation && !self.closed {
                    match polled {
                        Some(JobOutcome::Finished(Ok(Phase::Environment(request))))=>self.pending=Some(request),
                        Some(JobOutcome::Finished(Ok(Phase::Ready(prepared))))=>result=Some(prepared),
                        Some(JobOutcome::NotStarted{job,reason})=>{
                            if reason==SkipReason::Shutdown||self.client.as_ref().is_none_or(|client|!client.is_open()) {
                                result=Some(PreparedVoice {source:job.request.source,value:Err("Voice original not started because shared execution is shutting down".into()), #[cfg(test)] prepared_on:std::thread::current().id()});
                            } else {self.pending=Some(job.request);self.retry_at=Some(Instant::now()+Duration::from_millis(20));}
                        }
                        Some(JobOutcome::Finished(Err(never)))=>match never{},
                        _=>result=Some(PreparedVoice{source,value:Err("Voice preparation callback lost or panicked; original source retained, no actor started".into()), #[cfg(test)] prepared_on:std::thread::current().id()}),
                    }
                }
            }
        }
        if result.is_some() {
            self.retry_at = None;
        }
        self.pump();
        result.or_else(|| self.terminal.take())
    }
    pub fn cancel(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.pending = None;
        self.terminal = None;
        self.deferred = None;
        self.retry_at = None;
        if let Some((_, receipt, _)) = &self.active {
            receipt.cancel();
        }
    }
    pub fn close(&mut self) {
        self.cancel();
        self.closed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settle(owner: &mut VoicePreparation) -> PreparedVoice {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(prepared) = owner.collect() {
                return prepared;
            }
            assert!(
                Instant::now() < deadline,
                "real CPU preparation did not settle"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn actual_cpu_prepares_original_context_and_source_survives_controller_drop() {
        let client = crate::execution::test_client();
        let mut owner = VoicePreparation::new(client);
        let settings = VoiceSettings {
            api_key: "isolated-no-network".into(),
            custom_prompt: "original policy".into(),
            ..Default::default()
        };
        let actor = Arc::new(());
        let ui_thread = std::thread::current().id();
        owner
            .request(
                &settings,
                PreparationKind::Context {
                    target: VoiceTargetContext::NoDetectedAgent,
                    actor: actor.clone(),
                },
            )
            .unwrap();
        let result = settle(&mut owner);
        assert_ne!(
            result.prepared_on, ui_thread,
            "actual CPU body must execute off caller thread"
        );
        assert_eq!(result.settings(), &settings);
        assert!(
            matches!(result.kind(),PreparationKind::Context{actor:original,..} if Arc::ptr_eq(original,&actor))
        );
        let source = Arc::downgrade(&result.allocation());
        let PreparedVoice {
            value,
            source: original,
            ..
        } = result;
        let PreparedValue::Context(context) = value.unwrap() else {
            panic!("context")
        };
        drop(original);
        drop(owner);
        assert!(context.instructions().contains("original policy"));
        assert!(source.upgrade().is_some());
        drop(context);
        assert!(source.upgrade().is_none());
    }
    #[test]
    fn newer_startup_wins_and_audited_payload_is_ready_without_starting_provider() {
        let client = crate::execution::test_client();
        let mut owner = VoicePreparation::new(client);
        let mut settings = VoiceSettings {
            api_key: "isolated-no-network".into(),
            custom_prompt: "old policy".into(),
            ..Default::default()
        };
        owner
            .request(
                &settings,
                PreparationKind::NormalStartup {
                    target: VoiceTargetContext::DetectedAgent,
                },
            )
            .unwrap();
        settings.custom_prompt = "new policy".into();
        owner
            .request(
                &settings,
                PreparationKind::NormalStartup {
                    target: VoiceTargetContext::NoDetectedAgent,
                },
            )
            .unwrap();
        let prepared = settle(&mut owner);
        assert_eq!(prepared.settings(), &settings);
        let PreparedValue::Startup(startup) = prepared.value.unwrap() else {
            panic!("startup")
        };
        assert!(startup.config().instructions.contains("new policy"));
        assert!(!startup.config().instructions.contains("old policy"));
        // No VoiceService/provider/native device is created by preparation.
    }
    #[test]
    fn cancel_disposes_stale_preparation_without_publishing_or_actor_start() {
        let mut owner = VoicePreparation::new(crate::execution::test_client());
        let settings = VoiceSettings {
            api_key: "isolated-no-network".into(),
            ..Default::default()
        };
        owner
            .request(
                &settings,
                PreparationKind::NormalStartup {
                    target: VoiceTargetContext::NoDetectedAgent,
                },
            )
            .unwrap();
        owner.cancel();
        let deadline = Instant::now() + Duration::from_secs(5);
        while owner.is_pending() {
            assert!(owner.collect().is_none());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn busy_bank_retries_exact_capture_without_recloning_settings_or_recharging_source() {
        let shared = crate::execution::test_client();
        let client = shared
            .child(ilium_execution::ClientLimits {
                jobs: 1,
                service_jobs: 0,
                input_bytes: 8192,
                result_bytes: 8192,
            })
            .unwrap();
        let blocked = client
            .try_reserve_external(JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            })
            .unwrap()
            .retain(())
            .unwrap();
        let mut owner = VoicePreparation::new(client);
        let settings = VoiceSettings {
            api_key: "isolated-no-network".into(),
            custom_prompt: "owned original".into(),
            ..Default::default()
        };
        owner
            .request(
                &settings,
                PreparationKind::Context {
                    target: VoiceTargetContext::NoDetectedAgent,
                    actor: Arc::new(()),
                },
            )
            .unwrap();
        let original = owner.pending.as_ref().unwrap().source.clone();
        let pointer = original.settings.custom_prompt.as_ptr();
        for _ in 0..16 {
            owner.retry_at = Some(Instant::now());
            assert!(owner.collect().is_none());
            assert!(Arc::ptr_eq(
                &original,
                &owner.pending.as_ref().unwrap().source
            ));
            assert_eq!(
                owner
                    .pending
                    .as_ref()
                    .unwrap()
                    .source
                    .settings
                    .custom_prompt
                    .as_ptr(),
                pointer
            );
        }
        drop(blocked);
        owner.retry_at = Some(Instant::now());
        owner.pump();
        assert!(owner.active.is_some(), "original accepted on real bank");
        assert!(owner.pending.is_none());
        assert!(owner.retry_at.is_none());
        assert!(owner.retry_delay(Instant::now()).is_none());
        let prepared = settle(&mut owner);
        assert_eq!(prepared.settings().custom_prompt.as_ptr(), pointer);
        assert!(prepared.value.is_ok());
        assert!(!owner.is_pending());
        assert!(owner.retry_delay(Instant::now()).is_none());

        // A second real admission refusal is stopped before a job is accepted.
        // Neither its old deadline nor its original capture may survive Stop.
        let blocked = owner
            .client
            .as_ref()
            .unwrap()
            .try_reserve_external(JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            })
            .unwrap()
            .retain(())
            .unwrap();
        owner
            .request(
                &settings,
                PreparationKind::Context {
                    target: VoiceTargetContext::NoDetectedAgent,
                    actor: Arc::new(()),
                },
            )
            .unwrap();
        let stopped_source = Arc::downgrade(&owner.pending.as_ref().unwrap().source);
        assert!(owner.retry_delay(Instant::now()).is_some());
        owner.cancel();
        assert!(!owner.is_pending());
        assert!(stopped_source.upgrade().is_none());
        assert!(owner.retry_at.is_none());
        assert!(owner.retry_delay(Instant::now()).is_none());
        assert!(owner.collect().is_none());
        drop(blocked);
    }
    #[test]
    fn deferred_actual_cpu_result_retries_same_original_then_stop_releases_source() {
        let mut owner = VoicePreparation::new(crate::execution::test_client());
        let settings = VoiceSettings {
            api_key: "isolated-no-network".into(),
            custom_prompt: "deferred original".into(),
            ..Default::default()
        };
        let kind = PreparationKind::Context {
            target: VoiceTargetContext::NoDetectedAgent,
            actor: Arc::new(()),
        };
        owner.request(&settings, kind.clone()).unwrap();
        let prepared = settle(&mut owner);
        assert!(prepared.value.is_ok());
        let source = Arc::downgrade(&prepared.source);
        let pointer = prepared.settings().custom_prompt.as_ptr();
        let now = Instant::now();
        assert!(owner.defer_ready(prepared, now + Duration::from_secs(1)));
        assert!(owner.is_pending());
        assert!(owner.retry_delay(now).is_some());
        assert!(owner.collect().is_none());
        let generation = owner.generation;
        owner.request(&settings, kind).unwrap();
        assert_eq!(owner.generation, generation);
        assert!(owner.active.is_none());
        assert!(owner.pending.is_none());
        owner.retry_at = Some(Instant::now());
        let prepared = owner.collect().unwrap();
        assert_eq!(prepared.settings().custom_prompt.as_ptr(), pointer);
        assert!(Arc::ptr_eq(&source.upgrade().unwrap(), &prepared.source));
        assert!(owner.retry_delay(Instant::now()).is_none());
        assert!(owner.defer_ready(prepared, Instant::now()));
        owner.cancel();
        assert!(!owner.is_pending());
        assert!(owner.retry_delay(Instant::now()).is_none());
        assert!(source.upgrade().is_none());
        assert!(owner.collect().is_none());
    }
    #[test]
    fn newer_capture_fences_deferred_actual_cpu_original() {
        let mut owner = VoicePreparation::new(crate::execution::test_client());
        let mut settings = VoiceSettings {
            api_key: "isolated-no-network".into(),
            ..Default::default()
        };
        let kind = PreparationKind::Context {
            target: VoiceTargetContext::NoDetectedAgent,
            actor: Arc::new(()),
        };
        owner.request(&settings, kind.clone()).unwrap();
        let stale = settle(&mut owner);
        let source = Arc::downgrade(&stale.source);
        settings.custom_prompt = "new desired source".into();
        owner.request(&settings, kind).unwrap();
        assert!(!owner.defer_ready(stale, Instant::now()));
        assert!(source.upgrade().is_none());
        assert!(owner.deferred.is_none());
        assert!(settle(&mut owner).value.is_ok());
    }
}
