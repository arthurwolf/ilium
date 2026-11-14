//! The server side of text triggers: it reads the rule list from the `text-triggers` kernel
//! settings tab, scans a pane's visible screen whenever it changes, and sends a rule's message to
//! the pane after the rule's delay.
//!
//! Matching is the pure [`Tracker`] (instance identity by visible match counts, settle and resize
//! windows); this module owns the effects: the rules, one tracker and one delivery queue per pane,
//! one volatile timer for the earliest due delivery and the `send-input` submissions. Before a
//! delayed send the rule is looked up again: a rule that was removed, switched off, re-worded or
//! re-targeted since the match sends nothing, and so does a pane that is gone.

use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_sdk::host::{HostClock, HostKernelSettings, HostLog, HostTimers, HostTree, Level};
use ilium_plugin_sdk::primitives::{HostTerminal, ScreenPart, TerminalMode};
use ilium_plugin_sdk::types::{
    ClientId, Delivery as InputDelivery, Event, Intent, MessageEvent, MessageSpec, MessageTarget,
    NodeId, PluginId, PluginSide, SendInputSpec, Submission, TimerId,
};
use ilium_plugin_sdk::HostApi;
use serde_json::{json, Value};

use crate::config::{parse_table, validate, Rule};
use crate::matching::{target_matches, Delivery, DeliveryQueue, PaneKind, Tracker};
use crate::wire::{
    id_from_payload, notice_payload, rule_from_payload, state_payload, toggle_from_payload,
    DELETE_RULE_CHANNEL, NOTICE_CHANNEL, SAVE_RULE_CHANNEL, STATE_CHANNEL, STATE_REQUEST_CHANNEL,
    TOGGLE_RULE_CHANNEL,
};
use crate::PLUGIN_ID;

/// The kernel settings tab that holds the `[text_triggers]` table.
pub const SETTINGS_TAB: &str = "text-triggers";
/// Key of the rule list inside that tab.
const RULES_KEY: &str = "triggers";
/// Most visible rows one scan reads.
const MAX_SCAN_ROWS: u32 = 500;
/// The shortest timer the host accepts, in milliseconds.
const MIN_TIMER_MILLIS: u32 = 50;

#[derive(Default)]
struct ServerState {
    rules: Vec<Rule>,
    loaded: bool,
    trackers: HashMap<u64, Tracker>,
    queues: HashMap<u64, DeliveryQueue>,
    timer: Option<TimerId>,
}

thread_local! {
    static STATE: RefCell<ServerState> = RefCell::new(ServerState::default());
}

fn with_state<T>(body: impl FnOnce(&mut ServerState) -> T) -> T {
    STATE.with(|cell| body(&mut cell.borrow_mut()))
}

/// Forgets everything (activation).
pub fn reset() {
    with_state(|state| *state = ServerState::default());
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

fn pending_count() -> usize {
    with_state(|state| state.queues.values().map(DeliveryQueue::len).sum())
}

fn state_message(client: Option<ClientId>) -> Submission {
    let rules = with_state(|state| state.rules.clone());
    to_client(
        STATE_CHANNEL,
        state_payload(&rules, pending_count()),
        client,
    )
}

fn log(api: &mut dyn HostApi, level: Level, message: String) {
    HostLog::log(api, level, message);
}

/// Reads the rule list from the kernel settings tab. A list that fails validation keeps the last
/// valid one; a tab without a list means no rules.
fn load_rules(api: &mut dyn HostApi) {
    let text = match HostKernelSettings::read(api, SETTINGS_TAB.to_string()) {
        Ok(text) => text,
        Err(error) => {
            log(
                api,
                Level::Warn,
                format!("text triggers: settings unreadable: {error:?}"),
            );
            return;
        }
    };
    let Ok(table) = serde_json::from_str::<Value>(&text) else {
        log(
            api,
            Level::Warn,
            "text triggers: settings are not JSON".to_string(),
        );
        return;
    };
    let document = json!({ "text_triggers": table });
    let parsed = if table.get(RULES_KEY).is_some() {
        parse_table(&document)
    } else {
        Ok(Some(Vec::new()))
    };
    let rules = match parsed {
        Ok(Some(rules)) => rules,
        Ok(None) => Vec::new(),
        Err(message) => {
            log(api, Level::Warn, format!("text triggers: {message}"));
            return;
        }
    };
    if let Some(reason) = validate(&rules) {
        log(
            api,
            Level::Warn,
            format!("text triggers: list rejected: {reason}"),
        );
        return;
    }
    with_state(|state| {
        state.rules = rules;
        state.loaded = true;
    });
}

fn ensure_loaded(api: &mut dyn HostApi) {
    if !with_state(|state| state.loaded) {
        load_rules(api);
    }
}

/// Loads the rules at activation.
pub fn start(api: &mut dyn HostApi) {
    reset();
    load_rules(api);
}

fn pane_kind(api: &mut dyn HostApi, pane: u64) -> Option<PaneKind> {
    let view = HostTree::get(api, NodeId(pane))?;
    if view.content_kind.as_deref() != Some("ilium.terminal") {
        return Some(PaneKind::Other);
    }
    Some(if view.agent.is_some() {
        PaneKind::Agent
    } else {
        PaneKind::PlainShell
    })
}

fn now_ms(api: &mut dyn HostApi) -> u64 {
    HostClock::now_unix_millis(api)
}

fn input_for(pane: u64, message: &str) -> SendInputSpec {
    SendInputSpec {
        pane: NodeId(pane),
        text: message.to_owned(),
        delivery: InputDelivery::Submitted,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    }
}

/// Scans one pane's visible screen and queues the replies of every new instance.
fn on_screen(api: &mut dyn HostApi, pane: u64) -> Vec<Submission> {
    ensure_loaded(api);
    if with_state(|state| state.rules.iter().all(|rule| !rule.enabled)) {
        return Vec::new();
    }
    let Some(kind) = pane_kind(api, pane) else {
        return Vec::new();
    };
    let Some(screen) =
        HostTerminal::read_screen(api, NodeId(pane), ScreenPart::Visible, MAX_SCAN_ROWS)
    else {
        return Vec::new();
    };
    let is_alternate = HostTerminal::modes(api, NodeId(pane))
        .is_some_and(|modes| modes.contains(TerminalMode::ALTERNATE_SCREEN));
    let lines: Vec<String> = screen
        .rows
        .iter()
        .map(|row| row.trim_end().to_owned())
        .collect();
    let now = now_ms(api);
    let overflowed = with_state(|state| {
        let rules = state.rules.clone();
        let fired = state.trackers.entry(pane).or_default().evaluate(
            &rules,
            &lines,
            is_alternate,
            kind,
            now,
        );
        let mut overflowed = 0;
        for rule_id in fired {
            let Some(rule) = rules.iter().find(|rule| rule.id == rule_id) else {
                continue;
            };
            let delivery = Delivery {
                rule_id: rule.id.clone(),
                message: rule.message.clone(),
                detected_at_ms: now,
                delay_ms: u64::from(rule.delay_seconds) * 1000,
            };
            if state
                .queues
                .entry(pane)
                .or_default()
                .push(delivery)
                .is_err()
            {
                overflowed += 1;
            }
        }
        overflowed
    });
    if overflowed > 0 {
        log(
            api,
            Level::Warn,
            format!("text triggers: pane {pane} dropped {overflowed} replies (queue full)"),
        );
    }
    drain(api, now)
}

/// Whether a delivery that is due may still be sent: its rule must exist unchanged and enabled,
/// and the pane must still be one the rule targets.
fn still_valid(api: &mut dyn HostApi, pane: u64, delivery: &Delivery) -> bool {
    let Some(kind) = pane_kind(api, pane) else {
        return false;
    };
    with_state(|state| {
        state.rules.iter().any(|rule| {
            rule.id == delivery.rule_id
                && rule.enabled
                && rule.message == delivery.message
                && target_matches(rule.target, kind)
        })
    })
}

/// Sends every delivery that is due and re-arms the timer for the next one.
fn drain(api: &mut dyn HostApi, now: u64) -> Vec<Submission> {
    let due: Vec<(u64, Delivery)> = with_state(|state| {
        let mut due = Vec::new();
        for (pane, queue) in &mut state.queues {
            due.extend(queue.pop_due(now).into_iter().map(|item| (*pane, item)));
        }
        state.queues.retain(|_, queue| !queue.is_empty());
        due
    });
    let mut out = Vec::new();
    for (pane, delivery) in due {
        if !still_valid(api, pane, &delivery) {
            continue;
        }
        out.push(Submission::new(Intent::SendInput(input_for(
            pane,
            &delivery.message,
        ))));
    }
    arm_timer(api, now);
    out
}

fn arm_timer(api: &mut dyn HostApi, now: u64) {
    if let Some(timer) = with_state(|state| state.timer.take()) {
        HostTimers::cancel(api, timer);
    }
    let next = with_state(|state| state.queues.values().filter_map(|q| q.next_due_ms()).min());
    let Some(due) = next else {
        return;
    };
    let wait = due.saturating_sub(now).max(u64::from(MIN_TIMER_MILLIS));
    let wait = u32::try_from(wait).unwrap_or(u32::MAX);
    if let Ok(timer) = HostTimers::set(api, wait, None) {
        with_state(|state| state.timer = Some(timer));
    }
}

fn on_timer(api: &mut dyn HostApi, timer: TimerId) -> Vec<Submission> {
    if with_state(|state| state.timer) != Some(timer) {
        return Vec::new();
    }
    with_state(|state| state.timer = None);
    let now = now_ms(api);
    let mut out = drain(api, now);
    out.push(state_message(None));
    out
}

/// A pane left the tree: its memory and pending replies go with it.
fn on_node_removed(api: &mut dyn HostApi, node: NodeId) -> Vec<Submission> {
    let had = with_state(|state| {
        let tracker = state.trackers.remove(&node.0).is_some();
        let queue = state.queues.remove(&node.0).is_some();
        tracker || queue
    });
    if had {
        let now = now_ms(api);
        arm_timer(api, now);
    }
    Vec::new()
}

/// Writes the rule list back to the kernel settings tab.
fn store_rules(api: &mut dyn HostApi, rules: &[Rule]) -> Result<(), String> {
    if let Some(reason) = validate(rules) {
        return Err(reason);
    }
    let value = Value::Array(rules.iter().map(Rule::to_json).collect());
    HostKernelSettings::write(
        api,
        SETTINGS_TAB.to_string(),
        RULES_KEY.to_string(),
        value.to_string(),
    )
    .map_err(|error| format!("The rules could not be saved: {error:?}"))?;
    with_state(|state| state.rules = rules.to_vec());
    Ok(())
}

fn change_rules(
    api: &mut dyn HostApi,
    client: Option<ClientId>,
    change: impl FnOnce(&mut Vec<Rule>) -> Result<(), String>,
) -> Vec<Submission> {
    ensure_loaded(api);
    let mut rules = with_state(|state| state.rules.clone());
    let result = change(&mut rules).and_then(|()| store_rules(api, &rules));
    match result {
        Ok(()) => vec![state_message(None)],
        Err(message) => vec![to_client(NOTICE_CHANNEL, notice_payload(&message), client)],
    }
}

/// Answers one message of the client side.
fn on_message(api: &mut dyn HostApi, message: &MessageEvent) -> Vec<Submission> {
    let client = message.from_client;
    match message.channel.as_str() {
        STATE_REQUEST_CHANNEL => {
            ensure_loaded(api);
            vec![state_message(client)]
        }
        SAVE_RULE_CHANNEL => {
            let Some(rule) = rule_from_payload(&message.payload) else {
                return Vec::new();
            };
            change_rules(api, client, |rules| {
                match rules.iter_mut().find(|existing| existing.id == rule.id) {
                    Some(existing) => *existing = rule,
                    None => rules.push(rule),
                }
                Ok(())
            })
        }
        DELETE_RULE_CHANNEL => {
            let Some(id) = id_from_payload(&message.payload) else {
                return Vec::new();
            };
            change_rules(api, client, |rules| {
                rules.retain(|rule| rule.id != id);
                Ok(())
            })
        }
        TOGGLE_RULE_CHANNEL => {
            let Some((id, enabled)) = toggle_from_payload(&message.payload) else {
                return Vec::new();
            };
            change_rules(api, client, |rules| {
                rules
                    .iter_mut()
                    .find(|rule| rule.id == id)
                    .map(|rule| rule.enabled = enabled)
                    .ok_or_else(|| "That rule no longer exists".to_string())
            })
        }
        _ => Vec::new(),
    }
}

/// The rule list was edited elsewhere (the settings file or another tool).
fn on_settings_changed(api: &mut dyn HostApi) -> Vec<Submission> {
    load_rules(api);
    let now = now_ms(api);
    arm_timer(api, now);
    vec![state_message(None)]
}

/// Routes one event of the batch.
pub fn on_event(api: &mut dyn HostApi, event: Event) -> Vec<Submission> {
    match event {
        Event::ScreenChanged(change) => on_screen(api, change.pane.0),
        Event::PaneResized(resized) => {
            let now = now_ms(api);
            with_state(|state| {
                if let Some(tracker) = state.trackers.get_mut(&resized.node.0) {
                    tracker.resized(now);
                }
            });
            Vec::new()
        }
        Event::TimerFired(timer) => on_timer(api, timer),
        Event::NodeRemoved(node) => on_node_removed(api, node),
        Event::Message(message) => on_message(api, &message),
        Event::KernelSettingsChanged(change) if change.tab == SETTINGS_TAB => {
            on_settings_changed(api)
        }
        Event::EventsDropped(dropped) => {
            let total: u64 = dropped.iter().map(|item| u64::from(item.count)).sum();
            log(
                api,
                Level::Warn,
                format!("text triggers: {total} events were dropped; a match may have been missed"),
            );
            Vec::new()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delivery_sends_as_a_submitted_line() {
        let spec = input_for(7, "yes");
        assert_eq!(spec.pane, NodeId(7));
        assert_eq!(spec.text, "yes");
        assert_eq!(spec.delivery, InputDelivery::Submitted);
        assert_eq!(spec.raw, None);
    }
}
