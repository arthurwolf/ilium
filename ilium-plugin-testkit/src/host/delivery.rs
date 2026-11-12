//! Native submission/request bookkeeping, isolated from callback ownership and setup policy.
//! Every queue item is an occurrence: plugin-chosen correlations are intentionally not unique.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ilium_plugin_api::dedup;
use ilium_plugin_api::generated::types::intents::Submission as ApiSubmission;
use ilium_plugin_api::grants::Grant;
use ilium_plugin_sdk_core::generated::types::requests::RequestResult;
use ilium_plugin_sdk_core::types::{
    ErrorCode, Event, Intent, Outcome, PluginError, RequestRef, Submission,
};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::errors::{codes, HarnessError};

pub(super) struct Context<'a> {
    pub(super) instance: u64,
    pub(super) plugin: &'a str,
    pub(super) granted: &'a BTreeSet<String>,
    pub(super) now: u64,
}

#[derive(Debug)]
struct Pending {
    instance: u64,
    request: RequestRef,
    outcome_index: usize,
    submission: Submission,
    canonical: ApiSubmission,
    accepted_at: u64,
    reserved: bool,
    // Batch refusals and duplicates wait in occurrence order, without being overridable by
    // a test's complete call. Only genuinely unfinished work appears in pending_requests.
    ready: Option<Outcome>,
    automatic: Option<Outcome>,
}

#[derive(Debug)]
struct Reservation {
    at: u64,
    request: RequestRef,
}

#[derive(Debug)]
struct Completion {
    instance: u64,
    result: RequestResult,
}

#[derive(Debug, Default)]
pub(super) struct DeliveryState {
    submitted: Vec<Submission>,
    outcomes: Vec<Option<Outcome>>,
    pending: VecDeque<Pending>,
    completions: VecDeque<Completion>,
    dedup: BTreeMap<String, Reservation>,
    automatic: Option<Outcome>,
    played_sounds: Vec<String>,
}

// SDK and API generated records are distinct Rust types. The common generated serde wire
// representation is the explicit bridge; a schema drift fails closed as an ABI diagnostic.
fn wire_convert<T: Serialize, U: DeserializeOwned>(value: &T) -> Result<U, HarnessError> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|error| {
            HarnessError::new(
                "trap.abi",
                format!("SDK/API wire conversion failed: {error}"),
            )
        })
}

fn held_grants(names: &BTreeSet<String>) -> Result<Vec<Grant>, HarnessError> {
    names
        .iter()
        .map(|name| {
            Grant::parse(name).map_err(|error| {
                HarnessError::new(codes::GRANT_UNKNOWN, format!("{name}: {}", error.message))
            })
        })
        .collect()
}

impl DeliveryState {
    pub(super) fn admit(
        &mut self,
        submissions: Vec<Submission>,
        context: Context<'_>,
    ) -> Result<(), HarnessError> {
        let canonical: Vec<ApiSubmission> = submissions
            .iter()
            .map(wire_convert)
            .collect::<Result<_, _>>()?;
        let held = held_grants(context.granted)?;
        let refusals =
            ilium_plugin_api::submission::batch_refusals(&canonical, context.plugin, &held);
        let refusals: Vec<Option<PluginError>> = refusals
            .iter()
            .map(wire_convert)
            .collect::<Result<_, _>>()?;
        // Conversion and every batch check finish before history, reservations or queues mutate.
        self.submitted.extend(submissions.iter().cloned());
        self.dedup
            .retain(|_, reservation| dedup::is_reserved(reservation.at, context.now));
        for ((submission, canonical), refusal) in
            submissions.into_iter().zip(canonical).zip(refusals)
        {
            let request = RequestRef::Submission(submission.correlation);
            let mut reserved = false;
            let ready = if let Some(error) = refusal {
                Some(Outcome::Refused(error))
            } else if let Some(key) = submission.dedup_key.as_ref() {
                if let Some(original) = self.dedup.get(key) {
                    Some(Outcome::Duplicate(original.request.clone()))
                } else {
                    self.dedup.insert(
                        key.clone(),
                        Reservation {
                            at: context.now,
                            request: request.clone(),
                        },
                    );
                    reserved = true;
                    None
                }
            } else {
                None
            };
            let outcome_index = self.outcomes.len();
            self.outcomes.push(None);
            self.pending.push_back(Pending {
                instance: context.instance,
                request,
                outcome_index,
                submission,
                canonical,
                accepted_at: context.now,
                reserved,
                ready,
                automatic: self.automatic.clone(),
            });
        }
        Ok(())
    }

    fn finish(&mut self, pending: Pending, outcome: Outcome, enqueue: bool) {
        if pending.reserved && matches!(&outcome, Outcome::Refused(_)) {
            if let Some(key) = pending.submission.dedup_key.as_ref() {
                let owns_reservation = self.dedup.get(key).is_some_and(|reservation| {
                    reservation.at == pending.accepted_at && reservation.request == pending.request
                });
                if owns_reservation {
                    self.dedup.remove(key);
                }
            }
        }
        // History is indexed by submission occurrence; events still queue in completion order.
        self.outcomes[pending.outcome_index] = Some(outcome.clone());
        if enqueue {
            self.completions.push_back(Completion {
                instance: pending.instance,
                result: RequestResult {
                    request: pending.request,
                    outcome,
                },
            });
        }
    }

    pub(super) fn settle_pending(&mut self) {
        // Callback-produced submissions are processed only by the next round.
        let pending: Vec<_> = self.pending.drain(..).collect();
        for item in pending {
            let outcome =
                if let Some(outcome) = item.ready.clone().or_else(|| item.automatic.clone()) {
                    outcome
                } else if let Intent::PlaySound(name) = &item.submission.intent {
                    // The native harness contract's sound fake records acceptance instead of
                    // touching host audio. No other unfinished effect is reported as successful.
                    self.played_sounds.push(name.clone());
                    Outcome::Ok
                } else {
                    Outcome::Failed(PluginError::new(
                        ErrorCode::Unsupported,
                        format!(
                            "native fake effect is not implemented: {}",
                            item.submission.intent.case_name()
                        ),
                    ))
                };
            self.finish(item, outcome, true);
        }
    }

    pub(super) fn complete(
        &mut self,
        request: RequestRef,
        outcome: Outcome,
    ) -> Result<(), HarnessError> {
        let index = self
            .pending
            .iter()
            .position(|pending| pending.request == request && pending.ready.is_none());
        let pending = index
            .and_then(|index| self.pending.remove(index))
            .ok_or_else(|| {
                HarnessError::new(
                    codes::REQUEST_NOT_PENDING,
                    format!("request is unknown or already completed: {request:?}"),
                )
            })?;
        self.finish(pending, outcome, true);
        Ok(())
    }

    pub(super) fn revoke_pending(&mut self, context: Context<'_>) -> Result<usize, HarnessError> {
        let held = held_grants(context.granted)?;
        let pending: Vec<_> = self.pending.drain(..).collect();
        let mut count = 0;
        for item in pending {
            let revoked = item.ready.is_none()
                && ilium_plugin_api::submission::check_grants(
                    &item.canonical,
                    context.plugin,
                    &held,
                )
                .is_err();
            if revoked {
                count += 1;
                self.finish(
                    item,
                    Outcome::Failed(PluginError::new(ErrorCode::Revoked, "grant revoked")),
                    true,
                );
            } else {
                self.pending.push_back(item);
            }
        }
        Ok(count)
    }

    /// Stopping an instance never hands its results to a fresh instance. Accepted work is
    /// cancelled, already-determined refusals/duplicates retain their one result, and dedup
    /// reservations survive for the original fixed window, including failed/cancelled work.
    pub(super) fn retire_instance(&mut self) {
        self.completions.clear();
        let pending: Vec<_> = self.pending.drain(..).collect();
        for item in pending {
            let outcome = item.ready.clone().unwrap_or(Outcome::Cancelled);
            self.finish(item, outcome, false);
        }
    }

    pub(super) fn admitted_count(&self) -> usize {
        self.outcomes.len()
    }

    /// DEV-564 preserves already-accepted work whose grants remain held. Its owning instance
    /// stays the old one: a later completion is observable in history, never by a fresh plugin.
    /// Callback replies produced while flushing revocation results do not inherit that lease.
    pub(super) fn retire_grant_change(&mut self, admitted_before: usize) {
        self.completions.clear();
        let pending: Vec<_> = self.pending.drain(..).collect();
        for item in pending {
            if item.outcome_index < admitted_before && item.ready.is_none() {
                self.pending.push_back(item);
            } else {
                let outcome = item.ready.clone().unwrap_or(Outcome::Cancelled);
                self.finish(item, outcome, false);
            }
        }
    }

    pub(super) fn has_work(&self) -> bool {
        !self.pending.is_empty() || !self.completions.is_empty()
    }

    pub(super) fn completion_count(&self) -> usize {
        self.completions.len()
    }

    pub(super) fn take_events(&mut self, instance: u64, maximum: usize) -> Vec<Event> {
        let mut events = Vec::new();
        while events.len() < maximum {
            let Some(completion) = self.completions.pop_front() else {
                break;
            };
            if completion.instance == instance {
                events.push(Event::RequestCompleted(completion.result));
            }
        }
        events
    }

    pub(super) fn outcomes(&self) -> Vec<Outcome> {
        self.outcomes.iter().filter_map(Clone::clone).collect()
    }

    pub(super) fn submitted(&self) -> Vec<Submission> {
        self.submitted.clone()
    }

    pub(super) fn pending_requests(&self) -> Vec<RequestRef> {
        self.pending
            .iter()
            .filter(|pending| pending.ready.is_none())
            .map(|pending| pending.request.clone())
            .collect()
    }

    pub(super) fn auto_outcome(&mut self, outcome: Outcome) {
        self.automatic = Some(outcome);
    }

    pub(super) fn played_sounds(&self) -> Vec<String> {
        self.played_sounds.clone()
    }
}
