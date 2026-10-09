//! Ordered, bounded state responses prepared on the shared OS CPU bank.
use super::{snapshot, tools};
use crate::app::App;
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, Retained,
};
use ilium_voice::{VoiceToolInvocation, VoiceToolOutput};
use serde_json::Value;
use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
};
use tokio::sync::Notify;
const MIB: usize = 1024 * 1024;
const MAX_CALLS: usize = 16;
const MAX_ARGUMENT_BYTES: usize = MIB;
const COST: JobCost = JobCost {
    input_bytes: 256 * MIB,
    result_bytes: 128 * MIB,
};
struct SnapshotJob {
    snapshot: snapshot::ControlSnapshot,
}
impl Job for SnapshotJob {
    type Output = Value;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Value, String> {
        if context.stop_requested() {
            return Err("Voice state request cancelled".into());
        }
        let data = self.snapshot.prepare()?;
        let mut value = serde_json::Map::new();
        value.insert("status".into(), Value::String("ok".into()));
        value.insert(
            "message".into(),
            Value::String(ilium_prompts::voice::VOICE_EXECUTOR_CURRENT_ILIUM_STATE.into()),
        );
        value.insert("data".into(), data);
        let value = Value::Object(value);
        if json_bytes(&value) > context.cost().result_bytes {
            return Err("Control state response exceeds its 128 MiB admission".into());
        }
        if context.stop_requested() {
            return Err("Voice state request cancelled".into());
        }
        Ok(value)
    }
}
enum Stage {
    Invocation(Retained<VoiceToolInvocation>),
    Running {
        call_id: String,
        receipt: Receipt<SnapshotJob>,
    },
    Prepared {
        call_id: String,
        value: Retained<Result<Value, String>>,
    },
}
pub(super) struct Preparation {
    client: Client,
    notification: Arc<Notify>,
    queue: VecDeque<Stage>,
    pending: HashSet<String>,
    identity: Option<Arc<()>>,
}
impl Preparation {
    pub(super) fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = Arc::clone(&notification);
        Self {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            queue: VecDeque::new(),
            pending: HashSet::new(),
            identity: None,
        }
    }
    pub(super) fn notification(&self) -> Arc<Notify> {
        Arc::clone(&self.notification)
    }
    pub(super) fn synchronize(&mut self, identity: Option<Arc<()>>) -> bool {
        if match (&self.identity, &identity) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        } {
            return false;
        }
        self.cancel();
        self.identity = identity;
        true
    }
    pub(super) fn has_pending(&self, call_id: &str) -> bool {
        self.pending.contains(call_id)
    }
    pub(super) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
    pub(super) fn submit(
        &mut self,
        invocation: VoiceToolInvocation,
    ) -> Result<(), (VoiceToolInvocation, String)> {
        if self.identity.is_none() {
            return Err((invocation, "Voice actor is no longer available".into()));
        }
        if self.queue.len() >= MAX_CALLS
            || invocation.arguments_json.capacity() > MAX_ARGUMENT_BYTES
            || invocation.call_id.capacity() > 1024
        {
            return Err((
                invocation,
                "Voice control queue/argument admission is full; retry the original command".into(),
            ));
        }
        let cost = JobCost {
            input_bytes: invocation
                .arguments_json
                .capacity()
                .saturating_add(invocation.call_id.capacity())
                .saturating_add(invocation.name.capacity())
                .saturating_add(4096),
            result_bytes: 4096,
        };
        let admission = match self.client.try_reserve_external(cost) {
            Ok(admission) => admission,
            Err(error) => return Err((invocation, format!("Voice control admission: {error:?}"))),
        };
        let invocation = match admission.retain(invocation) {
            Ok(invocation) => invocation,
            Err(rejected) => {
                return Err((
                    rejected.value,
                    format!("Voice control payload admission: {:?}", rejected.reason),
                ));
            }
        };
        self.pending.insert(invocation.view().call_id.clone());
        self.queue.push_back(Stage::Invocation(invocation));
        self.notification.notify_one();
        Ok(())
    }
    /// Returns either a completed output or a non-state command ready to execute.
    pub(super) fn collect(
        &mut self,
        app: &App,
    ) -> Option<Result<VoiceToolOutput, Retained<VoiceToolInvocation>>> {
        let stage = self.queue.pop_front()?;
        match stage {
            Stage::Invocation(invocation) => {
                if invocation.view().name != tools::GET_STATE_TOOL_NAME {
                    self.pending.remove(&invocation.view().call_id);
                    return Some(Err(invocation));
                }
                let reservation = match self.client.try_reserve(Lane::Cpu, COST) {
                    Ok(reservation) => reservation,
                    Err(
                        ilium_execution::RejectReason::Busy
                        | ilium_execution::RejectReason::QueueFull
                        | ilium_execution::RejectReason::InputBytes
                        | ilium_execution::RejectReason::ResultBytes
                        | ilium_execution::RejectReason::JobLimit,
                    ) => {
                        self.queue.push_front(Stage::Invocation(invocation));
                        return None;
                    }
                    Err(error) => {
                        return Some(Ok(self.failed(
                            &invocation.view().call_id,
                            format!("Voice state admission: {error:?}"),
                        )));
                    }
                };
                let command = match super::decode_command(invocation.view()) {
                    Ok(super::command::ControlCommand::State(command)) => command,
                    Ok(_) => {
                        return Some(Ok(self.failed(
                            &invocation.view().call_id,
                            "State tool decoded to a different command".into(),
                        )));
                    }
                    Err(error) => return Some(Ok(self.failed(&invocation.view().call_id, error))),
                };
                if let Err(error) = snapshot::preflight(app) {
                    return Some(Ok(self.failed(&invocation.view().call_id, error)));
                }
                let source = match snapshot::capture(app, command.detail, &command.target) {
                    Ok(source) => source,
                    Err(error) => return Some(Ok(self.failed(&invocation.view().call_id, error))),
                };
                let call_id = invocation.view().call_id.clone();
                match reservation.submit(SnapshotJob { snapshot: source }) {
                    Ok(receipt) => self.queue.push_front(Stage::Running { call_id, receipt }),
                    Err(error) => {
                        return Some(Ok(self.failed(
                            &call_id,
                            format!("Voice state publication: {:?}", error.reason),
                        )));
                    }
                }
                None
            }
            Stage::Running {
                call_id,
                mut receipt,
            } => match receipt.try_take() {
                JobPoll::Pending => {
                    self.queue.push_front(Stage::Running { call_id, receipt });
                    None
                }
                JobPoll::Ready(outcome) => {
                    let value = outcome.map(|outcome| match outcome {
                        JobOutcome::Finished(value) => value,
                        JobOutcome::NotStarted { .. } => {
                            Err("Voice state preparation cancelled before execution".into())
                        }
                        JobOutcome::Panicked => Err("Voice state preparation panicked".into()),
                    });
                    self.queue.push_front(Stage::Prepared { call_id, value });
                    self.notification.notify_one();
                    None
                }
                JobPoll::Lost | JobPoll::Taken => Some(Ok(
                    self.failed(&call_id, "Voice state preparation completion lost".into())
                )),
            },
            Stage::Prepared { call_id, value } => {
                let bytes = match value.view() {
                    Ok(value) => json_bytes(value),
                    Err(error) => return Some(Ok(self.failed(&call_id, error.clone()))),
                };
                let storage = match crate::execution::process_quota()
                    .reserve_external_storage(bytes)
                {
                    Ok(storage) => Arc::new(storage),
                    Err(
                        ilium_execution::RejectReason::Busy
                        | ilium_execution::RejectReason::WorkerBytes,
                    ) => {
                        self.queue.push_front(Stage::Prepared { call_id, value });
                        return None;
                    }
                    Err(error) => {
                        return Some(Ok(
                            self.failed(&call_id, format!("Voice state result storage: {error:?}"))
                        ));
                    }
                };
                let (value, hold) = value.into_parts();
                let Ok(value) = value else {
                    unreachable!("exclusive result inspected above")
                };
                let output = VoiceToolOutput {
                    call_id: call_id.clone(),
                    result: Arc::new(value),
                    request_follow_up: true,
                    terminate_session_after_delivery: false,
                    allocation_hold: Some(storage),
                    retained_bytes: bytes,
                };
                drop(hold);
                self.pending.remove(&call_id);
                self.notification.notify_one();
                Some(Ok(output))
            }
        }
    }
    fn failed(&mut self, call_id: &str, message: String) -> VoiceToolOutput {
        self.pending.remove(call_id);
        self.notification.notify_one();
        super::tool_error(call_id, message)
    }
    pub(super) fn cancel(&mut self) {
        for stage in &self.queue {
            if let Stage::Running { receipt, .. } = stage {
                receipt.cancel();
            }
        }
        self.queue.clear();
        self.pending.clear();
    }
}
impl Drop for Preparation {
    fn drop(&mut self) {
        self.cancel();
    }
}
/// Conservatively charges map-node/allocator metadata; no allocator-hard RSS claim.
fn json_bytes(value: &Value) -> usize {
    match value {
        Value::String(value) => value.capacity().saturating_add(64),
        Value::Array(values) => values.iter().fold(
            values
                .capacity()
                .saturating_mul(std::mem::size_of::<Value>()),
            |bytes, value| bytes.saturating_add(json_bytes(value)),
        ),
        Value::Object(values) => values.iter().fold(256, |bytes, (key, value)| {
            bytes
                .saturating_add(key.capacity())
                .saturating_add(256)
                .saturating_add(json_bytes(value))
        }),
        _ => 64,
    }
}
