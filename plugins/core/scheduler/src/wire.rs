//! JSON shapes of the scheduler: the messages the client side sends to the server side, the
//! state the server keeps in plugin storage, and the state it pushes back to the panels. Pure
//! functions over `serde_json::Value`; a malformed payload yields `None`.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::queue::{Delivery, PromptQueue, QueuedPrompt};
use crate::schedule::ScheduledInput;

/// Client to server: schedule one input. `{"pane", "delay_seconds", "text", "send_enter"}`.
pub const SCHEDULE_CHANNEL: &str = "schedule";
/// Client to server: put a prompt on a pane's queue. `{"pane", "text", "delivery", "runs"}`.
pub const ENQUEUE_CHANNEL: &str = "enqueue";
/// Client to server: empty a pane's queue. `{"pane"}`.
pub const CLEAR_QUEUE_CHANNEL: &str = "clear-queue";
/// Client to server: drop a pane's pending schedule. `{"pane"}`.
pub const CANCEL_SCHEDULE_CHANNEL: &str = "cancel-schedule";
/// Client to server: send the current state to the asking client. Empty payload.
pub const STATE_REQUEST_CHANNEL: &str = "state-request";
/// Server to client: the pending schedules and queue lengths, see [`state_payload`].
pub const STATE_CHANNEL: &str = "state";
/// Server to client: a refusal or a delivery failure to show the user. `{"text"}`.
pub const NOTICE_CHANNEL: &str = "notice";

/// A client's request to schedule one input, before the server turns the delay into a deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleRequest {
    pub pane: u64,
    pub delay_seconds: u64,
    pub text: String,
    pub send_enter: bool,
}

impl ScheduleRequest {
    pub fn to_payload(&self) -> Vec<u8> {
        json!({
            "pane": self.pane,
            "delay_seconds": self.delay_seconds,
            "text": self.text,
            "send_enter": self.send_enter,
        })
        .to_string()
        .into_bytes()
    }

    pub fn from_payload(payload: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(payload).ok()?;
        Some(Self {
            pane: value.get("pane")?.as_u64()?,
            delay_seconds: value.get("delay_seconds")?.as_u64()?,
            text: value.get("text")?.as_str()?.to_owned(),
            send_enter: value.get("send_enter")?.as_bool()?,
        })
    }
}

/// A client's request to queue one prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnqueueRequest {
    pub pane: u64,
    pub text: String,
    pub delivery: Delivery,
}

fn delivery_json(delivery: &Delivery) -> Value {
    match delivery {
        Delivery::Once => json!({ "mode": "once" }),
        Delivery::Times { remaining_runs } => json!({ "mode": "times", "runs": remaining_runs }),
        Delivery::Forever => json!({ "mode": "forever" }),
    }
}

fn delivery_from_json(value: &Value) -> Option<Delivery> {
    match value.get("mode")?.as_str()? {
        "once" => Some(Delivery::Once),
        "forever" => Some(Delivery::Forever),
        "times" => {
            let runs = u32::try_from(value.get("runs")?.as_u64()?).ok()?;
            Some(Delivery::Times {
                remaining_runs: runs,
            })
        }
        _ => None,
    }
}

impl EnqueueRequest {
    pub fn to_payload(&self) -> Vec<u8> {
        json!({
            "pane": self.pane,
            "text": self.text,
            "delivery": delivery_json(&self.delivery),
        })
        .to_string()
        .into_bytes()
    }

    pub fn from_payload(payload: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(payload).ok()?;
        Some(Self {
            pane: value.get("pane")?.as_u64()?,
            text: value.get("text")?.as_str()?.to_owned(),
            delivery: delivery_from_json(value.get("delivery")?)?,
        })
    }
}

/// The payload of a message that names only a pane.
pub fn pane_payload(pane: u64) -> Vec<u8> {
    json!({ "pane": pane }).to_string().into_bytes()
}

/// Reads the pane out of [`pane_payload`].
pub fn pane_from_payload(payload: &[u8]) -> Option<u64> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    value.get("pane")?.as_u64()
}

/// Reads `"pane"` out of a command's arguments JSON.
pub fn pane_from_arguments(arguments_json: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(arguments_json).ok()?;
    value.get("pane")?.as_u64()
}

/// Everything the server keeps: one schedule and one queue per pane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stored {
    pub schedules: BTreeMap<u64, ScheduledInput>,
    pub queues: BTreeMap<u64, PromptQueue>,
}

/// The storage text of `stored`.
pub fn encode_stored(stored: &Stored) -> Vec<u8> {
    let schedules: Vec<Value> = stored
        .schedules
        .iter()
        .map(|(pane, input)| {
            json!({
                "pane": pane,
                "at": input.execute_at_unix_millis,
                "text": input.text,
                "send_enter": input.send_enter,
            })
        })
        .collect();
    let queues: Vec<Value> = stored
        .queues
        .iter()
        .filter(|(_, queue)| !queue.is_empty())
        .map(|(pane, queue)| {
            let prompts: Vec<Value> = queue
                .prompts()
                .iter()
                .map(|prompt| {
                    json!({
                        "text": prompt.text,
                        "delivery": delivery_json(&prompt.delivery),
                        "attempted": prompt.attempted_delivery,
                    })
                })
                .collect();
            json!({ "pane": pane, "prompts": prompts })
        })
        .collect();
    json!({ "schedules": schedules, "queues": queues })
        .to_string()
        .into_bytes()
}

/// Reads storage text; entries that do not parse are skipped, not fatal.
pub fn decode_stored(bytes: &[u8]) -> Stored {
    let mut stored = Stored::default();
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return stored;
    };
    for entry in value
        .get("schedules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let parsed = (|| {
            Some((
                entry.get("pane")?.as_u64()?,
                ScheduledInput {
                    execute_at_unix_millis: entry.get("at")?.as_u64()?,
                    text: entry.get("text")?.as_str()?.to_owned(),
                    send_enter: entry.get("send_enter")?.as_bool()?,
                },
            ))
        })();
        if let Some((pane, input)) = parsed {
            if input.has_input() {
                stored.schedules.insert(pane, input);
            }
        }
    }
    for entry in value
        .get("queues")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(pane) = entry.get("pane").and_then(Value::as_u64) else {
            continue;
        };
        let mut queue = PromptQueue::new();
        for prompt in entry
            .get("prompts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let parsed = (|| {
                Some(QueuedPrompt {
                    text: prompt.get("text")?.as_str()?.to_owned(),
                    delivery: delivery_from_json(prompt.get("delivery")?)?,
                    attempted_delivery: prompt
                        .get("attempted")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            })();
            if let Some(prompt) = parsed {
                // A refused entry (blank text, zero runs) is dropped like the kernel did.
                let _ = queue.enqueue(prompt);
            }
        }
        if !queue.is_empty() {
            stored.queues.insert(pane, queue);
        }
    }
    stored
}

/// The state pushed to the panels: `{"schedules": [{pane, at, text, send_enter}],
/// "queues": [{pane, length}]}`.
pub fn state_payload(stored: &Stored) -> Vec<u8> {
    let schedules: Vec<Value> = stored
        .schedules
        .iter()
        .map(|(pane, input)| {
            json!({
                "pane": pane,
                "at": input.execute_at_unix_millis,
                "text": input.text,
                "send_enter": input.send_enter,
            })
        })
        .collect();
    let queues: Vec<Value> = stored
        .queues
        .iter()
        .filter(|(_, queue)| !queue.is_empty())
        .map(|(pane, queue)| json!({ "pane": pane, "length": queue.len() }))
        .collect();
    let mut object = Map::new();
    object.insert("schedules".to_owned(), Value::Array(schedules));
    object.insert("queues".to_owned(), Value::Array(queues));
    Value::Object(object).to_string().into_bytes()
}

/// One schedule as the panel shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownSchedule {
    pub pane: u64,
    pub at: u64,
    pub text: String,
    pub send_enter: bool,
}

/// What the panel shows: pending schedules and the length of each non-empty queue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shown {
    pub schedules: Vec<ShownSchedule>,
    pub queues: Vec<(u64, u64)>,
}

/// Reads [`state_payload`].
pub fn decode_state(payload: &[u8]) -> Option<Shown> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    let schedules = value
        .get("schedules")?
        .as_array()?
        .iter()
        .filter_map(|entry| {
            Some(ShownSchedule {
                pane: entry.get("pane")?.as_u64()?,
                at: entry.get("at")?.as_u64()?,
                text: entry.get("text")?.as_str()?.to_owned(),
                send_enter: entry.get("send_enter")?.as_bool()?,
            })
        })
        .collect();
    let queues = value
        .get("queues")?
        .as_array()?
        .iter()
        .filter_map(|entry| Some((entry.get("pane")?.as_u64()?, entry.get("length")?.as_u64()?)))
        .collect();
    Some(Shown { schedules, queues })
}

/// A notice for the user.
pub fn notice_payload(text: &str) -> Vec<u8> {
    json!({ "text": text }).to_string().into_bytes()
}

/// Reads [`notice_payload`].
pub fn notice_from_payload(payload: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    Some(value.get("text")?.as_str()?.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_request_round_trips() {
        let request = ScheduleRequest {
            pane: 7,
            delay_seconds: 90,
            text: "continue".to_owned(),
            send_enter: true,
        };
        assert_eq!(
            ScheduleRequest::from_payload(&request.to_payload()),
            Some(request)
        );
        assert_eq!(ScheduleRequest::from_payload(b"nonsense"), None);
    }

    #[test]
    fn enqueue_request_round_trips_every_delivery() {
        for delivery in [
            Delivery::Once,
            Delivery::Forever,
            Delivery::Times { remaining_runs: 4 },
        ] {
            let request = EnqueueRequest {
                pane: 3,
                text: "next step".to_owned(),
                delivery,
            };
            assert_eq!(
                EnqueueRequest::from_payload(&request.to_payload()),
                Some(request)
            );
        }
    }

    #[test]
    fn stored_state_round_trips_and_drops_empty_queues() {
        let mut stored = Stored::default();
        stored.schedules.insert(
            2,
            ScheduledInput {
                execute_at_unix_millis: 5000,
                text: "go".to_owned(),
                send_enter: false,
            },
        );
        let mut queue = PromptQueue::new();
        queue
            .enqueue(QueuedPrompt {
                text: "one".to_owned(),
                delivery: Delivery::Times { remaining_runs: 2 },
                attempted_delivery: true,
            })
            .unwrap();
        stored.queues.insert(9, queue);
        stored.queues.insert(10, PromptQueue::new());

        let decoded = decode_stored(&encode_stored(&stored));
        assert_eq!(decoded.schedules, stored.schedules);
        assert_eq!(decoded.queues.len(), 1);
        assert_eq!(decoded.queues[&9], stored.queues[&9]);
    }

    #[test]
    fn corrupt_storage_reads_as_empty() {
        assert_eq!(decode_stored(b"{"), Stored::default());
    }

    #[test]
    fn state_payload_reaches_the_panel() {
        let mut stored = Stored::default();
        stored.schedules.insert(
            1,
            ScheduledInput {
                execute_at_unix_millis: 10,
                text: String::new(),
                send_enter: true,
            },
        );
        let shown = decode_state(&state_payload(&stored)).unwrap();
        assert_eq!(shown.schedules.len(), 1);
        assert!(shown.queues.is_empty());
    }

    #[test]
    fn pane_arguments_are_read() {
        assert_eq!(pane_from_arguments(r#"{"pane": 12}"#), Some(12));
        assert_eq!(pane_from_arguments("{}"), None);
        assert_eq!(pane_from_payload(&pane_payload(4)), Some(4));
    }
}
