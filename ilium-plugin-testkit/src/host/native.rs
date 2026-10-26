//! The owned native lifecycle boundary. NativeEntry itself remains Sized and unchanged.

use std::any::Any;
use std::fmt;

use ilium_plugin_sdk_core::generated::host::InstanceState;
use ilium_plugin_sdk_core::types::{
    Activation, DeactivationReason, Event, PluginError, Submission,
};
use ilium_plugin_sdk_core::NativeEntry;

use super::{FailureRecord, PluginHandle};
use crate::errors::{codes, HarnessError};

pub(super) trait ErasedEntry {
    fn activate(&mut self, activation: Activation) -> Result<(), PluginError>;
    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>>;
    fn on_events(&mut self, events: Vec<Event>) -> Vec<Submission>;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

struct Entry<P>(P);

impl<P: NativeEntry + 'static> ErasedEntry for Entry<P> {
    fn activate(&mut self, activation: Activation) -> Result<(), PluginError> {
        self.0.native_activate(activation)
    }

    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        self.0.native_deactivate(reason)
    }

    fn on_events(&mut self, events: Vec<Event>) -> Vec<Submission> {
        self.0.native_on_events(events)
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.0
    }
}

pub(super) type Factory = fn() -> Box<dyn ErasedEntry>;

pub(super) fn factory<P: NativeEntry + Default + 'static>() -> Box<dyn ErasedEntry> {
    Box::new(Entry(P::default()))
}

#[derive(Default)]
pub(super) struct NativeState {
    entry: Option<Box<dyn ErasedEntry>>,
    factory: Option<Factory>,
    pub(super) instance: u64,
    restored: Option<Vec<u8>>,
    failures: Vec<FailureRecord>,
}

impl fmt::Debug for NativeState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeState")
            .field("instance", &self.instance)
            .field("state", &self.state())
            .field("restored", &self.restored)
            .field("failures", &self.failures)
            .finish_non_exhaustive()
    }
}

impl NativeState {
    pub(super) fn is_active(&self) -> bool {
        self.entry.is_some()
    }

    pub(super) fn state(&self) -> InstanceState {
        if self.is_active() {
            InstanceState::Active
        } else {
            InstanceState::Inactive
        }
    }

    pub(super) fn next_instance(&self) -> u64 {
        self.instance.checked_add(1).unwrap_or_else(|| {
            panic!(
                "{}",
                HarnessError::new(
                    codes::INSTANCE_EXHAUSTED,
                    "activation instance counter reached u64::MAX"
                )
            )
        })
    }

    pub(super) fn saved_factory(&self) -> Factory {
        self.factory.unwrap_or_else(|| {
            panic!(
                "{}",
                HarnessError::new(codes::NOT_ACTIVE, "no plugin type has been activated")
            )
        })
    }

    pub(super) fn activate(
        &mut self,
        factory: Factory,
        activation: Activation,
        now: u64,
    ) -> Result<PluginHandle, PluginError> {
        let handle = PluginHandle {
            side: activation.side.clone(),
            instance: activation.instance,
        };
        self.instance = activation.instance;
        self.restored = activation.restored.clone();
        self.factory = Some(factory);
        // Keep the candidate local until activate succeeds. Errors and unwinds drop it; neither
        // can leave a synthetic Active state or retain the failed plugin value.
        let mut entry = factory();
        match entry.activate(activation) {
            Ok(()) => {
                self.entry = Some(entry);
                Ok(handle)
            }
            Err(error) => {
                self.record_failure(error.code.as_str(), now);
                Err(error)
            }
        }
    }

    pub(super) fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        let mut entry = self.entry.take().unwrap_or_else(|| {
            panic!(
                "{}",
                HarnessError::new(codes::NOT_ACTIVE, "no active native plugin")
            )
        });
        // Taking first makes the host inactive even if the real callback unwinds.
        entry.deactivate(reason)
    }

    fn record_failure(&mut self, code: &str, now: u64) {
        self.failures
            .retain(|failure| now.saturating_sub(failure.at_ms) < 600_000);
        self.failures.push(FailureRecord {
            at_ms: now,
            code: code.to_owned(),
        });
    }

    pub(super) fn trap(&mut self, code: &str, now: u64) {
        // Trapped instances are dropped, never sent a deactivation callback.
        self.entry = None;
        self.record_failure(code, now);
    }

    pub(super) fn on_events(
        &mut self,
        events: Vec<Event>,
        now: u64,
    ) -> Result<Vec<Submission>, HarnessError> {
        let mut entry = self
            .entry
            .take()
            .ok_or_else(|| HarnessError::new(codes::NOT_ACTIVE, "no active native plugin"))?;
        // Taking the owned entry across the call prevents an unwind from leaving a damaged
        // plugin Active. AssertUnwindSafe does not impose Send on the plugin or its fields.
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| entry.on_events(events)));
        let submissions = match result {
            Ok(submissions) => submissions,
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| {
                        payload
                            .downcast_ref::<&str>()
                            .map(|message| (*message).to_owned())
                    })
                    .unwrap_or_else(|| "non-string panic payload".to_owned());
                self.record_failure("trap.panic", now);
                return Err(HarnessError::new("trap.panic", message));
            }
        };
        if let Err(code) = ilium_plugin_api::abi::check_submission_count(submissions.len()) {
            self.record_failure(code.as_str(), now);
            return Err(HarnessError::new(
                code.as_str(),
                "on-events returned more than 256 submissions",
            ));
        }
        self.entry = Some(entry);
        Ok(submissions)
    }

    pub(super) fn restored(&self) -> Option<Vec<u8>> {
        self.restored.clone()
    }

    pub(super) fn failures(&self, now: u64) -> Vec<FailureRecord> {
        self.failures
            .iter()
            .filter(|failure| now.saturating_sub(failure.at_ms) < 600_000)
            .cloned()
            .collect()
    }

    pub(super) fn instance_mut<T: NativeEntry + 'static>(&mut self) -> &mut T {
        let entry = self.entry.as_mut().unwrap_or_else(|| {
            panic!(
                "{}",
                HarnessError::new(codes::NOT_ACTIVE, "no active native plugin")
            )
        });
        entry.as_any_mut().downcast_mut::<T>().unwrap_or_else(|| {
            panic!(
                "{}",
                HarnessError::new(
                    codes::INSTANCE_TYPE,
                    format!("active plugin is not {}", std::any::type_name::<T>())
                )
            )
        })
    }
}
