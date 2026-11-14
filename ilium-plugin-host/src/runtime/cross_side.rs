//! Topics and service calls between the two runtimes of one plugin set (gap G4).
//!
//! The server and the client each run their own `HostRuntime`. A plugin has one component per
//! side, and a topic or service of a plugin can be wanted by a plugin on either side. This
//! module is the pure state of that crossing; the runtime feeds it and the embedding process
//! moves the [`RemoteMessage`] values over the plugin envelope channel (the server and the
//! client crates convert them to and from `ilium-ipc` records; this crate does not know the
//! wire).
//!
//! Rules this module fixes:
//! - Every topic a plugin publishes is offered to the other side, except topics of the kernel
//!   namespace `ilium.kernel/`, which each side handles itself. The other side delivers it to
//!   the subscribers it has and drops it when there are none. A topic that arrived from the
//!   other side is delivered locally only; it is never exported again, so no echo loop exists.
//! - A service call whose provider has no component on this side is sent to the other side.
//!   The provider there sees the caller as `remote:<plugin id>`. A call that itself came from
//!   the other side is never sent back.
//! - A call sent away waits at most [`REMOTE_CALL_TIMEOUT`] for its reply and then ends with
//!   `timed-out`.
//! - Both queues are bounded by [`QUEUE_LIMIT`]; a full outbound queue refuses a service call
//!   with `busy` and drops the oldest topic message.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

use crate::imports::services::{FinishedServiceCall, PendingServiceCall, ServiceBrokerState};
use crate::imports::topics::Published;

/// A call sent to the other side waits this long for its reply.
pub const REMOTE_CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Most messages each queue holds.
pub const QUEUE_LIMIT: usize = 4096;
/// The caller of a service call that came from the other side starts with this.
pub const REMOTE_CALLER_PREFIX: &str = "remote:";
/// Topics under this prefix are handled by each side itself and never cross.
pub const KERNEL_TOPIC_PREFIX: &str = "ilium.kernel/";

/// What crosses between the two runtimes.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteMessage {
    /// A published topic message.
    Topic {
        /// `<plugin>/<name>`.
        topic: String,
        /// The publishing plugin.
        from: String,
        /// Valid JSON.
        payload_json: String,
    },
    /// A service call for a provider on the receiving side.
    ServiceCall {
        /// The id the sender gave the call; the reply carries it back.
        call: u64,
        /// The calling plugin on the sending side.
        caller: String,
        /// `<provider plugin>/<name>`.
        service: String,
        /// The method.
        method: String,
        /// Valid JSON.
        payload_json: String,
    },
    /// The end of a service call the receiver sent earlier.
    ServiceReply {
        /// The id the receiver gave the call.
        call: u64,
        /// The reply JSON, or why the call failed.
        outcome: Result<String, PluginError>,
    },
}

/// The `plugin` field of the envelopes that carry [`RemoteWire`] values: they belong to the
/// kernel, not to one plugin.
pub const REMOTE_ENVELOPE_PLUGIN: &str = "ilium.kernel";
/// The name a service call carries as a client-request call on the wire.
pub const SERVICE_CALL_NAME: &str = "ilium.service-call";

/// A [`RemoteMessage`] shaped like the plugin envelope bodies that carry it: a sourced event
/// for a topic, a client-request call for a service call and a client reply for its end. The
/// server and client crates map these one to one to the `ilium-ipc` records (this crate does
/// not know the wire).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteWire {
    /// A topic message: the topic and a JSON payload.
    Event {
        /// The topic.
        topic: String,
        /// UTF-8 JSON naming the publisher and holding the payload.
        payload: Vec<u8>,
    },
    /// A service call: the sender's call id, the name [`SERVICE_CALL_NAME`] and a JSON body.
    Request {
        /// The sender's call id.
        call_id: u64,
        /// Always [`SERVICE_CALL_NAME`].
        name: String,
        /// UTF-8 JSON naming caller, service, method and payload.
        payload: Vec<u8>,
    },
    /// The end of a service call.
    Reply {
        /// The call id of the request.
        call_id: u64,
        /// UTF-8 JSON, the reply or the failure.
        payload: Vec<u8>,
    },
}

impl RemoteMessage {
    /// The wire shape of this message.
    pub fn to_wire(&self) -> RemoteWire {
        match self {
            RemoteMessage::Topic {
                topic,
                from,
                payload_json,
            } => RemoteWire::Event {
                topic: topic.clone(),
                payload: serde_json::json!({ "from": from, "payload_json": payload_json })
                    .to_string()
                    .into_bytes(),
            },
            RemoteMessage::ServiceCall {
                call,
                caller,
                service,
                method,
                payload_json,
            } => RemoteWire::Request {
                call_id: *call,
                name: SERVICE_CALL_NAME.to_owned(),
                payload: serde_json::json!({
                    "caller": caller,
                    "service": service,
                    "method": method,
                    "payload_json": payload_json,
                })
                .to_string()
                .into_bytes(),
            },
            RemoteMessage::ServiceReply { call, outcome } => RemoteWire::Reply {
                call_id: *call,
                payload: match outcome {
                    Ok(reply) => serde_json::json!({ "ok": reply }),
                    Err(error) => serde_json::json!({ "error": error }),
                }
                .to_string()
                .into_bytes(),
            },
        }
    }

    /// The message a wire shape carries, or `None` when it is malformed or is a request the
    /// crossing does not know.
    pub fn from_wire(wire: &RemoteWire) -> Option<RemoteMessage> {
        fn text(value: &serde_json::Value, key: &str) -> Option<String> {
            value.get(key)?.as_str().map(str::to_owned)
        }
        match wire {
            RemoteWire::Event { topic, payload } => {
                let body: serde_json::Value = serde_json::from_slice(payload).ok()?;
                Some(RemoteMessage::Topic {
                    topic: topic.clone(),
                    from: text(&body, "from")?,
                    payload_json: text(&body, "payload_json")?,
                })
            }
            RemoteWire::Request {
                call_id,
                name,
                payload,
            } => {
                if name != SERVICE_CALL_NAME {
                    return None;
                }
                let body: serde_json::Value = serde_json::from_slice(payload).ok()?;
                Some(RemoteMessage::ServiceCall {
                    call: *call_id,
                    caller: text(&body, "caller")?,
                    service: text(&body, "service")?,
                    method: text(&body, "method")?,
                    payload_json: text(&body, "payload_json")?,
                })
            }
            RemoteWire::Reply { call_id, payload } => {
                let body: serde_json::Value = serde_json::from_slice(payload).ok()?;
                let outcome = match (body.get("ok"), body.get("error")) {
                    (Some(reply), None) => Ok(reply.as_str()?.to_owned()),
                    (None, Some(error)) => Err(serde_json::from_value(error.clone()).ok()?),
                    _ => return None,
                };
                Some(RemoteMessage::ServiceReply {
                    call: *call_id,
                    outcome,
                })
            }
        }
    }
}

/// A call sent to the other side that still waits.
#[derive(Debug, Clone)]
struct Awaiting {
    call: PendingServiceCall,
    deadline: Instant,
}

/// The crossing state of one runtime.
#[derive(Debug, Default)]
pub struct CrossSideState {
    outbound: VecDeque<RemoteMessage>,
    inbound: VecDeque<RemoteMessage>,
    /// Calls sent away, by the id they carry.
    awaiting: BTreeMap<u64, Awaiting>,
    /// Calls received, by (local caller name, local request id), with the sender's call id.
    serving: BTreeMap<(String, u64), u64>,
    next_call: u64,
}

impl CrossSideState {
    /// Whether a caller name belongs to the other side.
    pub fn is_remote_caller(caller: &str) -> bool {
        caller.starts_with(REMOTE_CALLER_PREFIX)
    }

    /// Whether anything waits for the pump.
    pub fn has_work(&self) -> bool {
        !self.inbound.is_empty() || !self.awaiting.is_empty()
    }

    /// The earliest moment a waiting call times out.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.awaiting.values().map(|waiting| waiting.deadline).min()
    }

    fn push_outbound(&mut self, message: RemoteMessage) -> Result<(), PluginError> {
        if self.outbound.len() >= QUEUE_LIMIT {
            if matches!(message, RemoteMessage::Topic { .. }) {
                if let Some(index) = self
                    .outbound
                    .iter()
                    .position(|queued| matches!(queued, RemoteMessage::Topic { .. }))
                {
                    self.outbound.remove(index);
                } else {
                    return Err(PluginError::new(
                        ErrorCode::Busy,
                        "too many messages wait for the other side",
                    ));
                }
            } else {
                return Err(PluginError::new(
                    ErrorCode::Busy,
                    "too many messages wait for the other side",
                ));
            }
        }
        self.outbound.push_back(message);
        Ok(())
    }

    /// Offers a topic message to the other side. Kernel topics stay home.
    pub fn export_topic(&mut self, published: &Published) {
        if published.topic.starts_with(KERNEL_TOPIC_PREFIX) {
            return;
        }
        // A full queue of calls refuses the topic; topics are best effort, so the failure is
        // only that the message is dropped.
        let _dropped = self.push_outbound(RemoteMessage::Topic {
            topic: published.topic.clone(),
            from: published.from.clone(),
            payload_json: published.payload_json.clone(),
        });
    }

    /// Sends a service call to the other side. A call from the other side is refused so that
    /// a service nobody provides cannot bounce between the runtimes.
    pub fn send_call(
        &mut self,
        call: &PendingServiceCall,
        now: Instant,
    ) -> Result<(), PluginError> {
        if Self::is_remote_caller(&call.caller) {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                "no plugin provides that service",
            ));
        }
        self.next_call += 1;
        let id = self.next_call;
        self.push_outbound(RemoteMessage::ServiceCall {
            call: id,
            caller: call.caller.clone(),
            service: call.service.clone(),
            method: call.method.clone(),
            payload_json: call.payload_json.clone(),
        })?;
        self.awaiting.insert(
            id,
            Awaiting {
                call: call.clone(),
                deadline: now + REMOTE_CALL_TIMEOUT,
            },
        );
        Ok(())
    }

    /// Takes what is ready to go to the other side.
    pub fn take_outbound(&mut self) -> Vec<RemoteMessage> {
        self.outbound.drain(..).collect()
    }

    /// Queues what arrived from the other side. A call that does not fit is answered with
    /// `busy` at once; a topic that does not fit is dropped.
    pub fn receive(&mut self, messages: Vec<RemoteMessage>) {
        for message in messages {
            if self.inbound.len() < QUEUE_LIMIT {
                self.inbound.push_back(message);
                continue;
            }
            if let RemoteMessage::ServiceCall { call, .. } = message {
                // Best effort: when the outbound queue is full too, the sender's own timeout
                // ends the call.
                let _refused = self.push_outbound(RemoteMessage::ServiceReply {
                    call,
                    outcome: Err(PluginError::new(
                        ErrorCode::Busy,
                        "too many messages wait for this side",
                    )),
                });
            }
        }
    }

    /// Handles what arrived and what timed out. Calls from the other side go to `broker` as
    /// pending calls of a `remote:` caller; replies finish the calls that waited for them.
    /// Returns the topic messages to deliver to the local subscribers.
    pub fn process_inbound(
        &mut self,
        broker: &mut ServiceBrokerState,
        now: Instant,
    ) -> Vec<Published> {
        let expired: Vec<u64> = self
            .awaiting
            .iter()
            .filter(|(_, waiting)| waiting.deadline <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some(waiting) = self.awaiting.remove(&id) {
                broker.fail(
                    &waiting.call,
                    PluginError::new(ErrorCode::TimedOut, "the other side did not answer"),
                );
            }
        }
        let mut topics = Vec::new();
        for message in std::mem::take(&mut self.inbound) {
            match message {
                RemoteMessage::Topic {
                    topic,
                    from,
                    payload_json,
                } => topics.push(Published {
                    from,
                    topic,
                    payload_json,
                }),
                RemoteMessage::ServiceReply { call, outcome } => {
                    // A reply that arrives after the timeout finds nothing and is dropped.
                    if let Some(waiting) = self.awaiting.remove(&call) {
                        match outcome {
                            Ok(reply) => broker.complete(&waiting.call, reply),
                            Err(error) => broker.fail(&waiting.call, error),
                        }
                    }
                }
                RemoteMessage::ServiceCall {
                    call,
                    caller,
                    service,
                    method,
                    payload_json,
                } => {
                    let (request, local_call) = broker.fresh_ids();
                    let local_caller = format!("{REMOTE_CALLER_PREFIX}{caller}");
                    let pending = PendingServiceCall {
                        call: local_call,
                        request,
                        caller: local_caller.clone(),
                        service,
                        method,
                        payload_json,
                    };
                    match broker.push_remote(pending) {
                        Ok(()) => {
                            self.serving.insert((local_caller, request.0), call);
                        }
                        Err(error) => {
                            let _refused = self.push_outbound(RemoteMessage::ServiceReply {
                                call,
                                outcome: Err(error),
                            });
                        }
                    }
                }
            }
        }
        topics
    }

    /// Turns the end of a call that came from the other side into a reply message. Returns
    /// false for any other call, which the runtime completes itself.
    pub fn finish_served(
        &mut self,
        broker: &mut ServiceBrokerState,
        end: &FinishedServiceCall,
    ) -> bool {
        if !Self::is_remote_caller(&end.caller) {
            return false;
        }
        let reply = broker.take_reply_of(&end.caller, end.request);
        let Some(call) = self.serving.remove(&(end.caller.clone(), end.request.0)) else {
            return true;
        };
        let outcome = match &end.error {
            Some(error) => Err(error.clone()),
            None => Ok(reply.unwrap_or_else(|| "null".to_owned())),
        };
        // A full queue loses the reply; the caller's timeout ends the call.
        let _refused = self.push_outbound(RemoteMessage::ServiceReply { call, outcome });
        true
    }

    /// Forgets the calls a stopped plugin waited for; nobody reads their end.
    pub fn forget_plugin(&mut self, plugin: &str) {
        self.awaiting
            .retain(|_, waiting| waiting.call.caller != plugin);
    }
}
