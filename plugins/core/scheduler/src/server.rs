//! The server side of the scheduler: it keeps one countdown and one prompt queue per pane in plugin
//! storage, arms a durable timer for each countdown, and writes into the pane with `send-input`.
//!
//! Countdowns send when their durable timer fires, even after a restart; the schedule is removed
//! and stored before the write, so a crash cannot send the same input twice. The queue sends one
//! prompt each time an agent finishes a turn (`agent-activity-changed` from Working to Done or
//! Idle). Before the write the head is marked as attempted and the state is stored, so a prompt
//! that may have reached the terminal is never replayed after a crash.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use ilium_plugin_sdk::host::{Bucket, HostClock, HostStorage, HostTimers};
use ilium_plugin_sdk::types::{
    AgentActivity, ClientId, DecorationKey, DecorationVisibility, Delivery as InputDelivery,
    EpochMs, Event, Intent, MessageEvent, MessageSpec, MessageTarget, NodeId, Outcome, PluginId,
    PluginSide, RequestRef, SendInputSpec, SetDecorationSpec, Span, Style, Submission, TimerId,
    ToastSpec, Tone,
};
use ilium_plugin_sdk::HostApi;

use crate::queue::{next_send, Activity, QueueError, QueuedPrompt, Turn};
use crate::schedule::{deadline_from_delay, remaining_seconds, ScheduledInput};
use crate::wire::{
    decode_stored, encode_stored, pane_from_payload, state_payload, EnqueueRequest,
    ScheduleRequest, Stored, CANCEL_SCHEDULE_CHANNEL, CLEAR_QUEUE_CHANNEL, ENQUEUE_CHANNEL,
    SCHEDULE_CHANNEL, STATE_CHANNEL, STATE_REQUEST_CHANNEL,
};
use crate::PLUGIN_ID;

/// Storage key of everything the server keeps.
const STORAGE_KEY: &str = "state";
/// Prefix of the durable timer name of one countdown; the pane id follows.
const TIMER_PREFIX: &str = "scheduled-";
/// Decoration slot of a pending countdown.
const SCHEDULE_SLOT: &str = "scheduled";
/// Decoration slot of a non-empty queue.
const QUEUE_SLOT: &str = "queued";
/// The countdown label refreshes this often, in milliseconds.
const TICK_MILLIS: u32 = 1000;
/// The first refresh after activation, in milliseconds.
const KICK_MILLIS: u32 = 200;

/// What a submission in flight carries until the host reports its outcome.
#[derive(Debug, Clone)]
enum Inflight {
    Schedule { pane: u64 },
    Queue { pane: u64, prompt: QueuedPrompt },
}

#[derive(Default)]
struct ServerState {
    stored: Stored,
    epochs: BTreeMap<u64, CompletionEpoch>,
    inflight: HashMap<u64, Inflight>,
    /// Decorations currently shown, so a vanished one is cleared.
    decorated: BTreeSet<(u64, &'static str)>,
    ticker: Option<TimerId>,
    kick: Option<TimerId>,
}

thread_local! {
    static STATE: RefCell<ServerState> = RefCell::new(ServerState::default());
}

fn with_state<T>(body: impl FnOnce(&mut ServerState) -> T) -> T {
    STATE.with(|cell| body(&mut cell.borrow_mut()))
}

fn timer_name(pane: u64) -> String {
    format!("{TIMER_PREFIX}{pane}")
}

fn to_client(channel: &str, payload: Vec<u8>, client: Option<ClientId>) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: PluginSide::Client,
            client,
        },
        channel: channel.to_string(),
        payload,
    }))
}

/// A toast with `text` as its title (the host shows at most 60 characters).
fn notice(text: &str, _client: Option<ClientId>) -> Submission {
    Submission::new(Intent::Toast(ToastSpec {
        id: Some("scheduler-notice".to_string()),
        tone: Tone::Warning,
        title: text.chars().take(60).collect(),
        body: None,
        actions: Vec::new(),
        duration_ms: None,
    }))
}

fn persist(api: &mut dyn HostApi, stored: &Stored) {
    let _ = HostStorage::put(
        api,
        Bucket::Project,
        STORAGE_KEY.to_string(),
        encode_stored(stored),
    );
}

/// Loads the stored state, re-arms every countdown and asks for the first refresh.
pub fn start(api: &mut dyn HostApi) {
    let stored = HostStorage::get(api, Bucket::Project, STORAGE_KEY.to_string())
        .map(|bytes| decode_stored(&bytes))
        .unwrap_or_default();
    for (pane, input) in &stored.schedules {
        let _ = HostTimers::at(api, EpochMs(input.execute_at_unix_millis), timer_name(*pane));
    }
    let kick = HostTimers::set(api, KICK_MILLIS, None).ok();
    with_state(|state| {
        *state = ServerState {
            stored,
            kick,
            ..ServerState::default()
        };
    });
    sync_ticker(api);
}

/// Runs the repeating one-second timer exactly while a countdown is pending.
fn sync_ticker(api: &mut dyn HostApi) {
    let (needs, has) = with_state(|state| (!state.stored.schedules.is_empty(), state.ticker));
    match (needs, has) {
        (true, None) => {
            let timer = HostTimers::set(api, TICK_MILLIS, Some(TICK_MILLIS)).ok();
            with_state(|state| state.ticker = timer);
        }
        (false, Some(timer)) => {
            HostTimers::cancel(api, timer);
            with_state(|state| state.ticker = None);
        }
        _ => {}
    }
}

/// `m:ss`, or `h:mm:ss` from one hour.
pub fn format_remaining(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let rest = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes}:{rest:02}")
    }
}

fn decoration(pane: u64, slot: &str, text: String, tone: Tone) -> Submission {
    Submission::new(Intent::SetDecoration(SetDecorationSpec {
        node: NodeId(pane),
        slot: slot.to_string(),
        content: vec![Span {
            text,
            style: Style::default(),
        }],
        icon: None,
        sort_key: None,
        persist: false,
        spans: None,
        tooltip: None,
        frames: Vec::new(),
        frame_ms: 90,
        trailing: true,
        visibility: DecorationVisibility::Always,
        tone: Some(tone),
    }))
}

/// The decorations of the tree rows, with a clear for every one that is no longer wanted.
fn decorations(api: &mut dyn HostApi) -> Vec<Submission> {
    let now = HostClock::now_unix_millis(api);
    with_state(|state| {
        let mut wanted: BTreeSet<(u64, &'static str)> = BTreeSet::new();
        let mut out = Vec::new();
        for (pane, input) in &state.stored.schedules {
            wanted.insert((*pane, SCHEDULE_SLOT));
            let label = format!(
                "in {}",
                format_remaining(remaining_seconds(input.execute_at_unix_millis, now))
            );
            out.push(decoration(*pane, SCHEDULE_SLOT, label, Tone::Info));
        }
        for (pane, queue) in &state.stored.queues {
            if queue.is_empty() {
                continue;
            }
            wanted.insert((*pane, QUEUE_SLOT));
            out.push(decoration(
                *pane,
                QUEUE_SLOT,
                format!("queue {}", queue.len()),
                Tone::Muted,
            ));
        }
        for (pane, slot) in state.decorated.difference(&wanted) {
            out.push(Submission::new(Intent::ClearDecoration(DecorationKey {
                node: NodeId(*pane),
                slot: (*slot).to_string(),
            })));
        }
        state.decorated = wanted;
        out
    })
}

fn state_message(client: Option<ClientId>) -> Submission {
    let payload = with_state(|state| state_payload(&state.stored));
    to_client(STATE_CHANNEL, payload, client)
}

/// Everything that follows a change of the stored state.
fn after_change(api: &mut dyn HostApi) -> Vec<Submission> {
    with_state(|state| persist(api, &state.stored.clone()));
    sync_ticker(api);
    let mut out = decorations(api);
    out.push(state_message(None));
    out
}

/// Answers one message of the client side.
pub fn on_message(api: &mut dyn HostApi, message: &MessageEvent) -> Vec<Submission> {
    let client = message.from_client;
    match message.channel.as_str() {
        SCHEDULE_CHANNEL => {
            let Some(request) = ScheduleRequest::from_payload(&message.payload) else {
                return Vec::new();
            };
            schedule(api, request, client)
        }
        ENQUEUE_CHANNEL => {
            let Some(request) = EnqueueRequest::from_payload(&message.payload) else {
                return Vec::new();
            };
            enqueue(api, request, client)
        }
        CLEAR_QUEUE_CHANNEL => {
            let Some(pane) = pane_from_payload(&message.payload) else {
                return Vec::new();
            };
            with_state(|state| {
                state.stored.queues.remove(&pane);
                state.epochs.remove(&pane);
            });
            after_change(api)
        }
        CANCEL_SCHEDULE_CHANNEL => {
            let Some(pane) = pane_from_payload(&message.payload) else {
                return Vec::new();
            };
            with_state(|state| state.stored.schedules.remove(&pane));
            HostTimers::cancel_durable(api, timer_name(pane));
            after_change(api)
        }
        STATE_REQUEST_CHANNEL => {
            let mut out = decorations(api);
            out.push(state_message(client));
            out
        }
        _ => Vec::new(),
    }
}

fn schedule(
    api: &mut dyn HostApi,
    request: ScheduleRequest,
    client: Option<ClientId>,
) -> Vec<Submission> {
    let input = ScheduledInput {
        execute_at_unix_millis: 0,
        text: request.text,
        send_enter: request.send_enter,
    };
    if !input.has_input() {
        return vec![notice(
            "Enter text, enable Send Enter, or use both",
            client,
        )];
    }
    let now = HostClock::now_unix_millis(api);
    let deadline = match deadline_from_delay(now, request.delay_seconds) {
        Ok(deadline) => deadline,
        Err(message) => return vec![notice(&message, client)],
    };
    if HostTimers::at(api, EpochMs(deadline), timer_name(request.pane)).is_err() {
        return vec![notice("The countdown could not be armed", client)];
    }
    with_state(|state| {
        state.stored.schedules.insert(
            request.pane,
            ScheduledInput {
                execute_at_unix_millis: deadline,
                ..input
            },
        );
    });
    after_change(api)
}

fn enqueue(
    api: &mut dyn HostApi,
    request: EnqueueRequest,
    client: Option<ClientId>,
) -> Vec<Submission> {
    let prompt = QueuedPrompt {
        text: request.text,
        delivery: request.delivery,
        attempted_delivery: false,
    };
    let refused = with_state(|state| {
        state
            .stored
            .queues
            .entry(request.pane)
            .or_default()
            .enqueue(prompt)
            .err()
    });
    match refused {
        Some(QueueError::EmptyPrompt) => {
            vec![notice("Write a prompt before enqueueing it", client)]
        }
        Some(QueueError::Full(limit)) => {
            vec![notice(
                &format!("The prompt queue holds at most {limit} prompts"),
                client,
            )]
        }
        None => after_change(api),
    }
}

/// The write of one countdown: text and Enter as one submission, text only, or Enter only.
fn input_for(input: &ScheduledInput, pane: u64) -> SendInputSpec {
    let base = SendInputSpec {
        pane: NodeId(pane),
        text: input.text.clone(),
        delivery: InputDelivery::Typed,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    };
    match (input.text.is_empty(), input.send_enter) {
        (false, true) => SendInputSpec {
            delivery: InputDelivery::Submitted,
            ..base
        },
        (true, true) => SendInputSpec {
            raw: Some(b"\r".to_vec()),
            ..base
        },
        _ => base,
    }
}

/// A durable timer fired: sends the countdown of that pane when it is due, or arms it again when
/// the clock says it is not yet (a changed system clock).
pub fn on_durable_timer(api: &mut dyn HostApi, name: &str) -> Vec<Submission> {
    let Some(pane) = name
        .strip_prefix(TIMER_PREFIX)
        .and_then(|rest| rest.parse::<u64>().ok())
    else {
        return Vec::new();
    };
    let Some(input) = with_state(|state| state.stored.schedules.get(&pane).cloned()) else {
        return Vec::new();
    };
    let now = HostClock::now_unix_millis(api);
    if input.execute_at_unix_millis > now {
        let _ = HostTimers::at(api, EpochMs(input.execute_at_unix_millis), timer_name(pane));
        return Vec::new();
    }
    // The schedule is gone and stored before the write, so a crash cannot send it twice.
    with_state(|state| state.stored.schedules.remove(&pane));
    let submission = Submission::new(Intent::SendInput(input_for(&input, pane)));
    with_state(|state| {
        state
            .inflight
            .insert(submission.correlation, Inflight::Schedule { pane });
    });
    let mut out = vec![submission];
    out.extend(after_change(api));
    out
}

/// The first and every later one-second tick: the countdown labels move.
pub fn on_timer(api: &mut dyn HostApi, timer: TimerId) -> Vec<Submission> {
    let is_ours = with_state(|state| {
        let is_kick = state.kick == Some(timer);
        if is_kick {
            state.kick = None;
        }
        is_kick || state.ticker == Some(timer)
    });
    if !is_ours {
        return Vec::new();
    }
    sync_ticker(api);
    decorations(api)
}

/// An agent changed its activity. A new turn invalidates the completion authorisation; a finished
/// turn authorises at most one queued prompt.
pub fn on_activity(
    api: &mut dyn HostApi,
    node: NodeId,
    previous: AgentActivity,
    current: AgentActivity,
) -> Vec<Submission> {
    let pane = node.0;
    if current == AgentActivity::Working {
        with_state(|state| state.epochs.entry(pane).or_default().invalidate());
        return Vec::new();
    }
    let finished = previous == AgentActivity::Working
        && matches!(current, AgentActivity::Done | AgentActivity::Idle);
    if !finished {
        return Vec::new();
    }
    let claimed = with_state(|state| {
        let snapshot = state.epochs.entry(pane).or_default().clone();
        let expected = snapshot.current();
        let head = state
            .stored
            .queues
            .get(&pane)
            .and_then(|queue| {
                next_send(queue, Some(Activity { turn: Turn::Idle }), &snapshot, expected)
            })
            .cloned()?;
        if !state.epochs.entry(pane).or_default().claim(expected) {
            return None;
        }
        let queue = state.stored.queues.get_mut(&pane)?;
        if !queue.mark_attempted(&head) {
            return None;
        }
        Some(QueuedPrompt {
            attempted_delivery: true,
            ..head
        })
    });
    let Some(marked) = claimed else {
        return Vec::new();
    };
    // The fence: the attempt is stored before the first byte is written.
    with_state(|state| persist(api, &state.stored.clone()));
    let submission = Submission::new(Intent::SendInput(SendInputSpec {
        pane: node,
        text: marked.text.clone(),
        delivery: InputDelivery::Submitted,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    }));
    with_state(|state| {
        state.inflight.insert(
            submission.correlation,
            Inflight::Queue {
                pane,
                prompt: marked,
            },
        );
    });
    vec![submission]
}

/// The host reported the outcome of a write this plugin submitted.
pub fn on_request_completed(
    api: &mut dyn HostApi,
    request: RequestRef,
    outcome: Outcome,
) -> Vec<Submission> {
    let RequestRef::Submission(correlation) = request else {
        return Vec::new();
    };
    let Some(inflight) = with_state(|state| state.inflight.remove(&correlation)) else {
        return Vec::new();
    };
    let delivered = matches!(
        outcome,
        Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_)
    );
    match inflight {
        Inflight::Schedule { pane } => {
            if delivered {
                Vec::new()
            } else {
                vec![notice(
                    &format!("Scheduled input for pane {pane} could not be delivered"),
                    None,
                )]
            }
        }
        Inflight::Queue { pane, prompt } => {
            if delivered {
                with_state(|state| {
                    if let Some(queue) = state.stored.queues.get_mut(&pane) {
                        queue.acknowledge(&prompt);
                    }
                    state.epochs.entry(pane).or_default().retire_submission();
                });
                return after_change(api);
            }
            let never_written = matches!(outcome, Outcome::Refused(_) | Outcome::Cancelled);
            if never_written {
                with_state(|state| {
                    if let Some(queue) = state.stored.queues.get_mut(&pane) {
                        queue.unmark_attempted(&prompt);
                    }
                });
            }
            let mut out = after_change(api);
            out.push(notice(
                &format!(
                    "Queued prompt for pane {pane} {}",
                    if never_written {
                        "was refused; retrying next turn"
                    } else {
                        "unconfirmed; clear the queue"
                    }
                ),
                None,
            ));
            out
        }
    }
}

/// A pane left the tree: its countdown and queue go with it.
pub fn on_node_removed(api: &mut dyn HostApi, node: NodeId) -> Vec<Submission> {
    let had = with_state(|state| {
        state.epochs.remove(&node.0);
        let schedule = state.stored.schedules.remove(&node.0).is_some();
        let queue = state.stored.queues.remove(&node.0).is_some();
        schedule || queue
    });
    if !had {
        return Vec::new();
    }
    HostTimers::cancel_durable(api, timer_name(node.0));
    after_change(api)
}

/// Routes one event of the batch.
pub fn on_event(api: &mut dyn HostApi, event: Event) -> Vec<Submission> {
    match event {
        Event::Message(message) => on_message(api, &message),
        Event::DurableTimerFired(timer) => on_durable_timer(api, &timer.name),
        Event::TimerFired(timer) => on_timer(api, timer),
        Event::AgentActivityChanged(change) => {
            on_activity(api, change.node, change.previous, change.current)
        }
        Event::RequestCompleted(result) => on_request_completed(api, result.request, result.outcome),
        Event::NodeRemoved(node) => on_node_removed(api, node),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_time_reads_as_clock_time() {
        assert_eq!(format_remaining(5), "0:05");
        assert_eq!(format_remaining(65), "1:05");
        assert_eq!(format_remaining(3725), "1:02:05");
    }

    #[test]
    fn input_forms_follow_the_dialog_payloads() {
        let text_and_enter = ScheduledInput {
            execute_at_unix_millis: 1,
            text: "go".to_string(),
            send_enter: true,
        };
        let spec = input_for(&text_and_enter, 4);
        assert_eq!(spec.delivery, InputDelivery::Submitted);
        assert_eq!(spec.raw, None);

        let enter_only = ScheduledInput {
            text: String::new(),
            ..text_and_enter.clone()
        };
        assert_eq!(input_for(&enter_only, 4).raw, Some(b"\r".to_vec()));

        let text_only = ScheduledInput {
            send_enter: false,
            ..text_and_enter
        };
        let spec = input_for(&text_only, 4);
        assert_eq!(spec.delivery, InputDelivery::Typed);
        assert_eq!(spec.text, "go");
    }
}
