//! Core plugin `ilium.core.notifications` (manual page core-plugin-notifications.md).
//!
//! The server side owns the alert rules: it reacts to agent activity changes and to the task
//! outcomes of the Progress plugin, and submits desktop notices, sounds and attention marks. The
//! pure rules (texts, redundancy, merge window, settings) live in [`rules`] and [`sound`]; this file
//! only reads the host (tree, clock, settings, storage) and turns decisions into submissions.

pub mod rules;
pub mod sound;
pub mod synthesis;

use std::collections::HashSet;

use ilium_plugin_sdk::generated::host::log::Level;
use ilium_plugin_sdk::generated::host::tree::HostTree;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::host::{Bucket, HostClock, HostSettings, HostStorage};
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, AgentActivity, AgentActivityEvent, Area, Intent, MarkAttentionSpec,
    NodeId, NotifySpec, UiAction, UiTree, Urgency, ViewTarget,
};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, Lifecycle, Views};
use rules::{AgentState, AlertSettings, PaneLabel, TaskOutcomeCoalescer, TaskOutcomeKind};
use sound::{event_for_transition, Activity, NotificationEvent, SoundEvent};

/// Topic of the Progress plugin that reports the state of a monitored task.
const PROGRESS_TOPIC: &str = "ilium.core.progress/changed";
/// The kernel tab whose changes reload the alert settings.
const SOUND_TAB: &str = "sound";
/// Key of the mute flag in the plugin's global storage bucket: the byte `1` or `0`.
const MUTED_KEY: &str = "muted";
/// Key of the toggle that mutes sounds (page sample 5).
const MUTE_TOGGLE_KEY: &str = "sound-mute";
/// The most terminal outcomes remembered for the once-only claim before the memory is reset.
const MAX_SETTLED_TASKS: usize = 1024;
/// Longest merge window the host accepts on a notice, in milliseconds.
const MAX_NOTICE_WINDOW_MS: u64 = 60_000;
/// The name shown for a session the plugin API does not name (API_GAPS.md).
const SESSION_LABEL: &str = "default";

/// The server side of `ilium.core.notifications` (core-plugin-notifications.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct NotificationsServer {
    settings: AlertSettings,
    /// The mute flag of the toggle; sounds are skipped while it is set, notices and marks are not.
    muted: bool,
    coalescer: TaskOutcomeCoalescer,
    /// The pane the user looks at.
    focused: Option<NodeId>,
    /// Nodes this plugin has seen marked and not yet unmarked.
    marked: HashSet<u64>,
    /// Task outcomes already alerted, so one terminal report notifies once.
    settled_tasks: HashSet<(u64, String)>,
}

impl Lifecycle for NotificationsServer {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        validate_settings(&context.settings_json)?;
        self.settings = AlertSettings::from_json(&context.settings_json);
        let mut api = host();
        self.muted = HostStorage::get(&mut *api, Bucket::Global, MUTED_KEY.to_string())
            .is_some_and(|bytes| bytes == b"1");
        Ok(())
    }
}

impl Handler for NotificationsServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::KernelSettingsChanged(change) if change.tab == SOUND_TAB => {
                    self.reload_settings();
                }
                Event::SettingsChanged(_) => self.reload_settings(),
                Event::PaneFocused(node) => {
                    self.focused = node;
                    if let Some(node) = node {
                        if self.marked.remove(&node.0) {
                            submissions.push(Submission::new(Intent::AckAttention(node)));
                        }
                    }
                }
                Event::AgentActivityChanged(change) => {
                    self.on_activity(&change, &mut submissions);
                }
                Event::AttentionChanged(change) => {
                    if change.marked {
                        if self.focused == Some(change.node) {
                            submissions.push(Submission::new(Intent::AckAttention(change.node)));
                        } else {
                            self.marked.insert(change.node.0);
                        }
                    } else {
                        self.marked.remove(&change.node.0);
                    }
                }
                Event::TopicPublished(topic) if topic.topic == PROGRESS_TOPIC => {
                    self.on_task_report(&topic.payload_json, &mut submissions);
                }
                Event::UiAction(action) => self.on_ui_action(&action),
                _ => {}
            }
        }
        submissions
    }
}

impl NotificationsServer {
    /// Reads the settings again; a change of any other tab never gets here.
    fn reload_settings(&mut self) {
        let mut api = host();
        self.settings = AlertSettings::from_json(&HostSettings::get_json(&mut *api));
    }

    fn sounds_allowed(&self) -> bool {
        !self.muted && !self.settings.source_muted
    }

    /// Pushes the sound of `event` when the user wants it.
    fn push_sound(&self, event: SoundEvent, dedup_key: String, out: &mut Vec<Submission>) {
        if self.sounds_allowed() && self.settings.sound_events.is_enabled(event) {
            out.push(Submission::keyed(
                Intent::PlaySound(rules::sound_id(event).to_string()),
                dedup_key,
            ));
        }
    }

    /// Pushes a desktop notice when its event is switched on.
    fn push_notice(
        &self,
        event: NotificationEvent,
        spec: NotifySpec,
        dedup_key: String,
        out: &mut Vec<Submission>,
    ) {
        if self.settings.notifications.is_enabled(event) {
            out.push(Submission::keyed(Intent::Notify(spec), dedup_key));
        }
    }

    fn on_activity(&mut self, change: &AgentActivityEvent, out: &mut Vec<Submission>) {
        let Some(current) = to_activity(change.current) else {
            return;
        };
        let Some(event) = event_for_transition(to_activity(change.previous), current) else {
            return;
        };
        let node = change.node;
        let suffix = format!("{}:{}", node.0, change.revision);
        let (name, objective) = pane_label(node);
        let label = PaneLabel {
            session: SESSION_LABEL,
            name: &name,
            objective: objective.as_deref(),
        };
        match event {
            SoundEvent::AgentFinished => {
                self.push_notice(
                    NotificationEvent::AgentFinished,
                    notice(
                        rules::finished_title(&label),
                        rules::finished_body(&label),
                        Urgency::Normal,
                    ),
                    format!("notice-finished:{suffix}"),
                    out,
                );
                self.push_sound(event, format!("finished:{suffix}"), out);
            }
            SoundEvent::ApprovalRequired => {
                self.push_notice(
                    NotificationEvent::ApprovalRequired,
                    notice(
                        rules::approval_title(&label),
                        rules::approval_body(&label),
                        Urgency::High,
                    ),
                    format!("notice-ask:{suffix}"),
                    out,
                );
                self.push_sound(event, format!("ask:{suffix}"), out);
                // The mark does not depend on the desktop switches or on the mute flag.
                out.push(Submission::keyed(
                    Intent::MarkAttention(MarkAttentionSpec {
                        node,
                        reason: rules::attention_reason(&name),
                    }),
                    format!("mark:{suffix}"),
                ));
            }
            SoundEvent::AgentStarted | SoundEvent::WaitingBackground => {
                self.push_sound(event, format!("{}:{suffix}", rules::sound_id(event)), out);
            }
            SoundEvent::TaskSucceeded | SoundEvent::TaskFailed => {}
        }
    }

    /// One report of the Progress plugin: `{ monitor, pane, job_id, status, percent }`.
    fn on_task_report(&mut self, payload_json: &str, out: &mut Vec<Submission>) {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(payload_json) else {
            return;
        };
        let Some(kind) = payload
            .get("status")
            .and_then(serde_json::Value::as_str)
            .and_then(TaskOutcomeKind::from_status)
        else {
            return;
        };
        let Some(pane) = payload.get("pane").and_then(serde_json::Value::as_u64) else {
            return;
        };
        let job_id = payload
            .get("job_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("task")
            .to_string();
        if self.settled_tasks.len() >= MAX_SETTLED_TASKS {
            self.settled_tasks.clear();
        }
        if !self.settled_tasks.insert((pane, job_id.clone())) {
            return;
        }

        let notice_on = self
            .settings
            .notifications
            .is_enabled(kind.notification_event());
        let sound_on =
            self.sounds_allowed() && self.settings.sound_events.is_enabled(kind.sound_event());
        if !notice_on && !sound_on {
            return;
        }
        let (name, objective, agent) = {
            let node = NodeId(pane);
            let (name, objective) = pane_label(node);
            (name, objective, agent_state(node))
        };
        if rules::is_task_outcome_redundant(&self.settings.notifications, agent) {
            return;
        }
        let now = {
            let mut api = host();
            HostClock::monotonic_millis(&mut *api)
        };
        if !self.coalescer.admit(
            pane,
            kind,
            now,
            self.settings.notifications.task_coalesce_seconds,
        ) {
            return;
        }

        let mid_turn = rules::is_agent_mid_turn(agent);
        let label = PaneLabel {
            session: SESSION_LABEL,
            name: &name,
            objective: objective.as_deref(),
        };
        let key = sanitized_key(&format!("{pane}:{job_id}"));
        let window_ms = (u64::from(self.settings.notifications.task_coalesce_seconds) * 1000)
            .min(MAX_NOTICE_WINDOW_MS);
        let mut spec = notice(
            rules::task_title(&label, kind, mid_turn),
            rules::task_body(&label, kind, &job_id, mid_turn),
            if kind == TaskOutcomeKind::Done {
                Urgency::Low
            } else {
                Urgency::Normal
            },
        );
        spec.coalesce_key = Some(sanitized_key(&format!(
            "task:{pane}:{}",
            if kind == TaskOutcomeKind::Done {
                "done"
            } else {
                "failed"
            }
        )));
        spec.coalesce_window_ms = u32::try_from(window_ms).unwrap_or(u32::MAX);
        self.push_notice(
            kind.notification_event(),
            spec,
            format!("task-notice:{key}"),
            out,
        );
        self.push_sound(kind.sound_event(), format!("task-sound:{key}"), out);
    }

    /// The mute toggle: `ui-action` `toggled` with a flag on [`MUTE_TOGGLE_KEY`].
    fn on_ui_action(&mut self, action: &UiAction) {
        if action.kind != ActionKind::Toggled || action.key != MUTE_TOGGLE_KEY {
            return;
        }
        let Some(ActionValue::Flag(muted)) = action.value else {
            return;
        };
        self.muted = muted;
        let mut api = host();
        let byte: &[u8] = if muted { b"1" } else { b"0" };
        if let Err(error) = HostStorage::put(
            &mut *api,
            Bucket::Global,
            MUTED_KEY.to_string(),
            byte.to_vec(),
        ) {
            ilium_plugin_sdk::log::log(
                Level::Warn,
                &format!("the mute flag could not be stored: {}", error.message),
            );
        }
    }
}

/// A desktop notice without sound (the sound is its own submission) and without merging.
fn notice(title: String, body: String, urgency: Urgency) -> NotifySpec {
    NotifySpec {
        title,
        body: Some(body),
        urgency,
        sound: None,
        coalesce_key: None,
        coalesce_window_ms: 0,
    }
}

/// The agent activity as the sound rules know it; `None` and menus are no alerting state.
fn to_activity(activity: AgentActivity) -> Option<Activity> {
    match activity {
        AgentActivity::Working => Some(Activity::Working),
        AgentActivity::Idle => Some(Activity::Idle),
        AgentActivity::Done => Some(Activity::Done),
        AgentActivity::AwaitingApproval => Some(Activity::WaitingApproval),
        AgentActivity::None | AgentActivity::Menu => None,
    }
}

/// The title of a node and the objective of its agent, read from the tree.
fn pane_label(node: NodeId) -> (String, Option<String>) {
    let mut api = host();
    match HostTree::get(&mut *api, node) {
        Some(view) => (view.title, view.agent.and_then(|agent| agent.objective)),
        None => (format!("pane {}", node.0), None),
    }
}

/// How the agent of `node` stands now, read when the outcome arrives.
fn agent_state(node: NodeId) -> AgentState {
    let mut api = host();
    match HostTree::get(&mut *api, node).and_then(|view| view.agent) {
        None => AgentState::NoAgent,
        Some(agent) => match agent.activity {
            AgentActivity::Idle | AgentActivity::Done => AgentState::Resting,
            AgentActivity::Working | AgentActivity::AwaitingApproval => AgentState::MidTurn,
            AgentActivity::None | AgentActivity::Menu => AgentState::Other,
        },
    }
}

/// Makes `text` a valid dedup-key part: characters outside the allowed set become `_`, at most 80.
fn sanitized_key(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect()
}

/// The client side of `ilium.core.notifications` (core-plugin-notifications.md "runs on both sides", world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct NotificationsClient;

impl Lifecycle for NotificationsClient {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        validate_settings(&context.settings_json)
    }
}

impl Handler for NotificationsClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl Views for NotificationsClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        empty_tree()
    }
}

/// Activation succeeds when the settings JSON is an object (an empty string means no settings).
fn validate_settings(settings_json: &str) -> Result<(), PluginError> {
    if settings_json.trim().is_empty() {
        return Ok(());
    }
    match serde_json::from_str::<serde_json::Value>(settings_json) {
        Ok(value) if value.is_object() => Ok(()),
        _ => Err(PluginError {
            code: ilium_plugin_sdk::types::ErrorCode::InvalidArgument,
            message: "settings must be one JSON object".to_string(),
            retry_after_ms: None,
        }),
    }
}

/// An empty, valid view tree: one root node that draws nothing.
fn empty_tree() -> UiTree {
    use ilium_plugin_sdk::types::{NodeFlags, UiNode, Widget};
    UiTree {
        version: 1,
        nodes: vec![UiNode {
            key: None,
            widget: Widget::Clear,
            children: Vec::new(),
            flags: NodeFlags::default(),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }],
        root: 0,
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::{json, Value};

    const MANIFEST_JSON: &str = include_str!("../manifest.json");

    fn load_manifest() -> Value {
        serde_json::from_str(MANIFEST_JSON).expect("manifest.json must parse as JSON")
    }

    /// Returns the grant strings a manifest lists for one side ("server" or "client").
    fn grants_of(manifest: &Value, side: &str) -> Vec<String> {
        manifest["capabilities"][side]
            .as_array()
            .map_or_else(Vec::new, |grants| {
                grants
                    .iter()
                    .filter_map(|grant| grant.as_str().map(str::to_owned))
                    .collect()
            })
    }

    /// Returns the manifest's extension point declaration with the given id, if any.
    fn extension_point<'manifest>(
        manifest: &'manifest Value,
        point_id: &str,
    ) -> Option<&'manifest Value> {
        manifest["contributes"]["extension_points"]
            .as_array()?
            .iter()
            .find(|point| point["id"] == point_id)
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_the_notifications_plugin() {
        let manifest = load_manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.notifications");
        assert_eq!(manifest["publisher"], "Ilium");
        assert_eq!(manifest["limits_tier"], "first-party");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_activates_at_startup_with_the_documented_capabilities() {
        let manifest = load_manifest();
        assert_eq!(manifest["activation"], json!(["startup"]));

        let server_grants = grants_of(&manifest, "server");
        for documented_grant in [
            "sound.play",
            "notify.desktop",
            "agent.read",
            "tree.read",
            "tree.attention",
        ] {
            assert!(
                server_grants.iter().any(|held| held == documented_grant),
                "server side must hold {documented_grant}"
            );
        }
        let client_grants = grants_of(&manifest, "client");
        assert!(
            client_grants.iter().any(|held| held == "notify.toast"),
            "client side must hold notify.toast"
        );
    }

    /// REQ: COR-030, COR-033
    #[test]
    fn server_side_holds_the_single_sound_tab_write_grant() {
        let manifest = load_manifest();
        let sound_tab_grant = "settings.write-kernel:sound";
        let holding_sides: Vec<&str> = ["server", "client"]
            .into_iter()
            .filter(|side| {
                grants_of(&manifest, side)
                    .iter()
                    .any(|held| held == sound_tab_grant)
            })
            .collect();
        assert_eq!(
            holding_sides,
            vec!["server"],
            "exactly one holder, on the server side"
        );
    }

    /// REQ: EXT-052, SEC-293
    #[test]
    fn before_notify_is_a_standard_risk_veto_hook_with_a_20_ms_deadline() {
        let manifest = load_manifest();
        let before_notify =
            extension_point(&manifest, "before-notify").expect("before-notify must be declared");
        assert_eq!(before_notify["kind"], "hook");
        assert_eq!(before_notify["mode"], "veto");
        assert_eq!(before_notify["deadline_ms"], 20);
        assert_eq!(before_notify["max_handlers"], 8);
        assert_eq!(before_notify["version"], "0.1.0");
        assert_eq!(before_notify["risk"], "standard");
    }

    /// REQ: EXT-052, SEC-293
    #[test]
    fn channels_is_a_high_risk_collect_point_capped_at_64_entries() {
        let manifest = load_manifest();
        let channels = extension_point(&manifest, "channels").expect("channels must be declared");
        assert_eq!(channels["kind"], "collect");
        assert_eq!(channels["risk"], "high");
        assert_eq!(channels["max_entries"], 64);
        assert_eq!(channels["dynamic"], false);
    }
}
