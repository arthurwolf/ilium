//! Nonprivileged task scheduling on the already admitted scene actor.
//! No timer thread, service-bank sleeper, or reconstructed permission ticket.
use crate::{
    engine::{CompletionState, EngineLimits, HostRequest, ServiceAuthority, ServiceValue},
    error::{AnimationError, Result},
    runtime::PackageInstance,
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const MAX_POLLS: usize = 32;
// The trusted facade admits at most 64 wrapper identities. Closed snapshots
// awaiting the next genuine frame seed are retained separately from live polls.
const MAX_TRACKED_WRAPPERS: usize = 64;
const MIN_INTERVAL_MS: u64 = 16;
const MAX_INTERVAL_MS: u64 = 5_000;

fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("native tasks: {message}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PollOptions {
    interval_ms: u64,
    deadline_ms: u64,
}
impl PollOptions {
    fn parse(value: &Value, limits: &EngineLimits) -> Result<Self> {
        let fields = value
            .as_object()
            .ok_or_else(|| invalid("poll options object"))?;
        if fields.len() != 2 {
            return Err(invalid("poll options fields"));
        }
        let interval_ms = fields
            .get("interval_ms")
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid("interval_ms integer"))?;
        let deadline_ms = fields
            .get("deadline_ms")
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid("deadline_ms integer"))?;
        // Every native next fits within the original helper service deadline.
        let maximum = limits.preparation_ms.saturating_sub(1).min(MAX_INTERVAL_MS);
        if !(MIN_INTERVAL_MS..=maximum).contains(&interval_ms)
            || deadline_ms <= interval_ms
            || deadline_ms > limits.preparation_ms
        {
            return Err(invalid("poll cadence/deadline bounds"));
        }
        Ok(Self {
            interval_ms,
            deadline_ms,
        })
    }
}

struct Poll {
    authority: ServiceAuthority,
    interval: Duration,
    deadline: Instant,
    next_due: Instant,
    pending_next: Option<HostRequest>,
    closing: Option<HostRequest>,
    closed: bool,
    revision: u64,
}
impl Poll {
    fn descriptor(&self, id: &str) -> Value {
        json!({"id":id,"kind":"tasks.poll","revision":self.revision,
            "status":{"state":if self.closed {"closed"} else {"ready"}}})
    }
}
struct Yield {
    request: HostRequest,
    due: Instant,
}

pub struct NativeTaskHost {
    polls: BTreeMap<String, Poll>,
    terminal_snapshots: BTreeMap<String, Value>,
    yields: Vec<Yield>,
    // IDs below next_id were published by a Delivered open ACK. Failed opens
    // reuse their ID, so a range check never treats an unpublished gap as real.
    next_id: u64,
    closed: bool,
    quota: QuotaGroup,
    limits: EngineLimits,
    // This covers the bounded map nodes, IDs, due inventory, and yield vector.
    // Each retained HostRequest and ServiceValue carries its own original charge.
    _metadata: StorageAdmission,
}
impl NativeTaskHost {
    pub fn new(quota: QuotaGroup, limits: EngineLimits) -> Result<Self> {
        let metadata = quota
            .reserve_external_storage(128 * 1024)
            .map_err(|error| AnimationError::Budget(format!("task registry: {error:?}")))?;
        Ok(Self {
            polls: BTreeMap::new(),
            terminal_snapshots: BTreeMap::new(),
            yields: Vec::new(),
            next_id: 1,
            closed: false,
            quota,
            limits,
            _metadata: metadata,
        })
    }

    fn published_id(&self, id: &str, authority: ServiceAuthority) -> bool {
        let prefix = format!(
            "task-{}-{}-{}-",
            authority.instance_id, authority.plan_generation, authority.authorization_epoch
        );
        let Some(sequence) = id
            .strip_prefix(&prefix)
            .and_then(|suffix| suffix.parse::<u64>().ok())
        else {
            return false;
        };
        sequence > 0 && sequence < self.next_id && id == format!("{prefix}{sequence}")
    }
    fn closed_descriptor(id: &str) -> Value {
        json!({"id":id,"kind":"tasks.poll","revision":2,"status":{"state":"closed"}})
    }

    fn empty_request(request: &HostRequest) -> Result<()> {
        if request.payload.metadata() != &json!({})
            || !request.payload.arrays().is_empty()
            || !request.payload.planes().is_empty()
        {
            return Err(invalid("yield payload must be exactly empty"));
        }
        Ok(())
    }
    fn id(request: &HostRequest) -> Result<&str> {
        let fields = request
            .payload
            .metadata()
            .as_object()
            .ok_or_else(|| invalid("handle payload object"))?;
        if fields.len() != 2
            || fields.get("kind").and_then(Value::as_str) != Some("tasks.poll")
            || !request.payload.arrays().is_empty()
            || !request.payload.planes().is_empty()
        {
            return Err(invalid("handle payload schema"));
        }
        fields
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| {
                id.len() <= 128
                    && id.starts_with("task-")
                    && id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
            .ok_or_else(|| invalid("handle ID"))
    }
    fn complete(
        &self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        result: Value,
    ) -> Result<CompletionState> {
        // Check the original channel before allocating the producer copy and
        // again inside the helper's inert publication/ACK transaction.
        instance.check_native_task_request(request)?;
        let value = ServiceValue::copy_from_host(
            &result,
            &[],
            &BTreeMap::new(),
            &self.limits,
            self.quota.clone(),
        )?;
        instance.complete_native_task(request, value)
    }
    pub fn dispatch(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
    ) -> Result<Option<HostRequest>> {
        if !matches!(
            request.method.as_str(),
            "tasks.yield" | "tasks.poll.open" | "tasks.poll.next" | "tasks.poll.close"
        ) {
            return Ok(Some(request));
        }
        if self.closed {
            return Err(invalid("registry is retiring"));
        }
        instance.check_native_task_request(&request)?;
        match request.method.as_str() {
            "tasks.yield" => {
                Self::empty_request(&request)?;
                if self.yields.len() >= self.limits.pending_requests {
                    return Err(AnimationError::Budget("pending task yields".into()));
                }
                let due = Instant::now()
                    .checked_add(Duration::from_millis(1))
                    .ok_or_else(|| invalid("yield deadline overflow"))?;
                let retained = request.clone();
                instance.with_native_task_registry(&request, |_| {
                    self.yields.push(Yield {
                        request: retained,
                        due,
                    });
                    Ok(())
                })?;
            }
            "tasks.poll.open" => {
                if !request.payload.arrays().is_empty() || !request.payload.planes().is_empty() {
                    return Err(invalid("poll options must be JSON only"));
                }
                let options = PollOptions::parse(request.payload.metadata(), &self.limits)?;
                if self.polls.len() >= MAX_POLLS
                    || self.polls.len() + self.terminal_snapshots.len() >= MAX_TRACKED_WRAPPERS
                    || self.next_id == u64::MAX
                {
                    return Err(AnimationError::Budget("task handle limit".into()));
                }
                let authority = instance.check_native_task_request(&request)?;
                let now = Instant::now();
                let id = format!(
                    "task-{}-{}-{}-{}",
                    authority.instance_id,
                    authority.plan_generation,
                    authority.authorization_epoch,
                    self.next_id
                );
                let poll = Poll {
                    authority,
                    interval: Duration::from_millis(options.interval_ms),
                    deadline: now
                        .checked_add(Duration::from_millis(options.deadline_ms))
                        .ok_or_else(|| invalid("task deadline overflow"))?,
                    next_due: now
                        .checked_add(Duration::from_millis(options.interval_ms))
                        .ok_or_else(|| invalid("task cadence overflow"))?,
                    pending_next: None,
                    closing: None,
                    closed: false,
                    revision: 1,
                };
                let descriptor = poll.descriptor(&id);
                instance.with_native_task_registry(&request, |_| {
                    self.polls.insert(id.clone(), poll);
                    Ok(())
                })?;
                match self.complete(instance, &request, json!({"ok":true,"value":descriptor})) {
                    Ok(CompletionState::Delivered) => self.next_id += 1,
                    Ok(_) => {
                        self.polls.remove(&id);
                    } // No wrapper was published.
                    Err(error) => {
                        // Publication outcome is unknown to this owner. Fail
                        // closed and retain the poll until workflow retirement.
                        self.closed = true;
                        return Err(error);
                    }
                }
            }
            "tasks.poll.next" => {
                let id = Self::id(&request)?.to_owned();
                let authority = instance.check_native_task_request(&request)?;
                if !self.polls.contains_key(&id) {
                    if !self.published_id(&id, authority) {
                        return Err(invalid("unknown task handle"));
                    }
                    let state =
                        self.complete(instance, &request, json!({"ok":true,"value":null}))?;
                    if state == CompletionState::Delivered {
                        self.terminal_snapshots.remove(&id);
                    }
                    return Ok(None);
                }
                let retained = request.clone();
                instance.with_native_task_registry(&request, |authority| {
                    let poll = self
                        .polls
                        .get_mut(&id)
                        .ok_or_else(|| invalid("unknown task handle"))?;
                    if poll.authority != authority || poll.pending_next.is_some() {
                        return Err(invalid("stale or overlapping task next"));
                    }
                    poll.pending_next = Some(retained);
                    Ok(())
                })?;
            }
            "tasks.poll.close" => {
                let id = Self::id(&request)?.to_owned();
                let authority = instance.check_native_task_request(&request)?;
                if !self.polls.contains_key(&id) {
                    if !self.published_id(&id, authority) {
                        return Err(invalid("unknown task handle"));
                    }
                    let state = self.complete(
                        instance,
                        &request,
                        json!({"ok":true,"value":Self::closed_descriptor(&id)}),
                    )?;
                    if state == CompletionState::Delivered {
                        self.terminal_snapshots.remove(&id);
                    }
                    return Ok(None);
                }
                let retained = request.clone();
                instance.with_native_task_registry(&request, |authority| {
                    let poll = self
                        .polls
                        .get_mut(&id)
                        .ok_or_else(|| invalid("unknown task handle"))?;
                    if poll.authority != authority || poll.closing.is_some() {
                        return Err(invalid("stale or overlapping task close"));
                    }
                    poll.closed = true;
                    poll.revision = poll
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| invalid("task revision exhausted"))?;
                    poll.closing = Some(retained);
                    Ok(())
                })?;
            }
            _ => return Err(invalid("method inventory")),
        }
        Ok(None)
    }

    /// The scene actor uses this monotonic deadline in its existing condvar
    /// wait. It is only a timer hint; it never polls finite bank receipts.
    pub fn next_due(&self) -> Option<Instant> {
        if self.closed {
            return None;
        }
        self.yields
            .iter()
            .map(|item| item.due)
            .chain(self.polls.values().filter_map(|poll| {
                if poll.closing.is_some() || (poll.closed && poll.pending_next.is_some()) {
                    Some(Instant::now())
                } else if poll.closed {
                    None
                } else if poll.pending_next.is_some() {
                    Some(poll.next_due.min(poll.deadline))
                } else {
                    Some(poll.deadline)
                }
            }))
            .min()
    }

    /// One bounded pass over due entries. A callback reaction runs only in a
    /// subsequent PackageInstance::pump after this method releases its owners.
    pub fn on_due(&mut self, instance: &mut PackageInstance, now: Instant) -> Result<bool> {
        if self.closed {
            return Ok(false);
        }
        instance.native_task_authority()?;
        let mut progressed = false;
        let mut index = 0;
        while index < self.yields.len() {
            if self.yields[index].due > now {
                index += 1;
                continue;
            }
            progressed = true;
            let request = self.yields[index].request.clone();
            if !request.is_cancelled() {
                let state = self.complete(instance, &request, json!({"ok":true,"value":null}))?;
                progressed |= state == CompletionState::Delivered;
            }
            self.yields.remove(index);
        }
        // IDs and intermediate vector capacity are included in the fixed
        // admitted registry envelope; MAX_POLLS makes this sweep finite.
        let ids: Vec<String> = self.polls.keys().cloned().collect();
        for id in ids {
            let (next, close, tick) = {
                let poll = self
                    .polls
                    .get_mut(&id)
                    .ok_or_else(|| invalid("task disappeared"))?;
                if !poll.closed && now >= poll.deadline {
                    poll.closed = true;
                    poll.revision = poll
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| invalid("task revision exhausted"))?;
                    progressed = true;
                }
                let tick = !poll.closed && now >= poll.next_due;
                let next = if poll.pending_next.is_some() && (poll.closed || tick) {
                    poll.pending_next.clone()
                } else {
                    None
                };
                (next, poll.closing.clone(), tick)
            };
            let terminal_next = next.is_some() && !tick;
            let mut terminal_ack = false;
            if let Some(request) = next {
                progressed = true;
                if !request.is_cancelled() {
                    let value = if tick {
                        json!({"ok":true,"value":{"tick":true}})
                    } else {
                        json!({"ok":true,"value":null})
                    };
                    let state = self.complete(instance, &request, value)?;
                    progressed |= state == CompletionState::Delivered;
                    terminal_ack = terminal_next && state == CompletionState::Delivered;
                }
                let poll = self
                    .polls
                    .get_mut(&id)
                    .ok_or_else(|| invalid("task disappeared"))?;
                poll.pending_next = None;
                if tick {
                    poll.next_due = now
                        .checked_add(poll.interval)
                        .ok_or_else(|| invalid("task cadence overflow"))?;
                }
            }
            if let Some(request) = close {
                progressed = true;
                if !request.is_cancelled() {
                    let descriptor = self
                        .polls
                        .get(&id)
                        .ok_or_else(|| invalid("task disappeared"))?
                        .descriptor(&id);
                    let state =
                        self.complete(instance, &request, json!({"ok":true,"value":descriptor}))?;
                    progressed |= state == CompletionState::Delivered;
                    if state == CompletionState::Delivered {
                        self.polls.remove(&id);
                    }
                }
                if let Some(poll) = self.polls.get_mut(&id) {
                    poll.closing = None;
                }
            } else if terminal_next && terminal_ack {
                // A delivered null is the native terminal ACK consumed by the
                // trusted callback loop; no explicit close request is needed.
                self.polls.remove(&id);
            }
            // A callback may still await arbitrary asynchronous work when
            // the total lifetime expires. A cancelled close ACK likewise
            // cannot release its wrapper. Free the live slot but retain an
            // authenticated terminal snapshot until seed or a genuine late
            // request ACK. This also covers the cancelled-close path above.
            let terminal_unacknowledged = self.polls.get(&id).is_some_and(|poll| {
                poll.closed && poll.pending_next.is_none() && poll.closing.is_none()
            });
            if terminal_unacknowledged {
                self.terminal_snapshots
                    .insert(id.clone(), Self::closed_descriptor(&id));
                self.polls.remove(&id);
            }
        }
        Ok(progressed)
    }

    pub fn snapshots(&self, instance: &mut PackageInstance) -> Result<ServiceValue> {
        instance.native_task_authority()?;
        let values: Vec<Value> = self
            .polls
            .iter()
            .map(|(id, poll)| poll.descriptor(id))
            .chain(self.terminal_snapshots.values().cloned())
            .collect();
        // Recheck before the distinct retained copy. Frame seed publication
        // has its own original-channel gate after this observation.
        instance.native_task_authority()?;
        ServiceValue::copy_from_host(
            &Value::Array(values),
            &[],
            &BTreeMap::new(),
            &self.limits,
            self.quota.clone(),
        )
    }
    /// Call only after the original helper successfully applied the complete
    /// frame seed. A failed seed leaves every terminal observation retained.
    pub fn acknowledge_seeded_snapshots(&mut self) {
        self.terminal_snapshots.clear();
    }
    pub fn revoke(&mut self) {
        self.closed = true;
        for item in &self.yields {
            item.request.stop_token().stop();
        }
        self.yields.clear();
        for poll in self.polls.values() {
            if let Some(request) = &poll.pending_next {
                request.stop_token().stop();
            }
            if let Some(request) = &poll.closing {
                request.stop_token().stop();
            }
        }
        self.polls.clear();
        self.terminal_snapshots.clear();
        // No task-owned worker exists. The original helper/controller still
        // prove their independent physical exit before workflow release.
    }
    pub fn is_drained(&self) -> bool {
        self.closed
            && self.yields.is_empty()
            && self.polls.is_empty()
            && self.terminal_snapshots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::QuotaLimits;
    #[test]
    fn exact_poll_bounds_preserve_original_helper_deadline() {
        let limits = EngineLimits::default();
        assert!(PollOptions::parse(&json!({"interval_ms":16,"deadline_ms":100}), &limits).is_ok());
        for value in [
            json!({"interval_ms":15,"deadline_ms":100}),
            json!({"interval_ms":16,"deadline_ms":10_001}),
            json!({"interval_ms":50,"deadline_ms":49}),
            json!({"interval_ms":50,"deadline_ms":50}),
            json!({"interval_ms":16,"deadline_ms":100,"extra":true}),
            json!({"interval_ms":16.5,"deadline_ms":100}),
        ] {
            assert!(PollOptions::parse(&value, &limits).is_err(), "{value}");
        }
    }
    #[test]
    fn empty_task_registry_uses_no_worker_or_finite_job() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: 256 * 1024,
        });
        let mut tasks = NativeTaskHost::new(quota.clone(), EngineLimits::default()).unwrap();
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().jobs, 0);
        assert!(tasks.next_due().is_none());
        tasks.revoke();
        assert!(tasks.is_drained());
        drop(tasks);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
