//! The kernel events feed (gap G2): turns two readings of the kernel state into the plugin
//! events that describe the change.
//!
//! Both kernel sides keep the previous reading and call these functions whenever a new one
//! arrives: the server on each tree snapshot of its event bus, the client on each mirrored
//! tree, focus change and `config.toml` change. The functions are pure, so every event a
//! plugin sees is decided here and tested here.
//!
//! - [`node_events`]: `pane-created`, `pane-title-changed`, `agent-identified`,
//!   `agent-activity-changed` and `agent-goal-changed` from two maps of node views.
//! - [`focus_event`]: `pane-focused` when the focused pane differs.
//! - [`config_events`]: `kernel-settings-changed` per kernel tab whose table changed, plus
//!   `theme-changed` and `keymap-changed`.
//! - [`attention_event`]: one `attention-changed`.

use std::collections::BTreeMap;

use ilium_plugin_api::generated::types::events::{
    AgentActivityEvent, AgentGoalEvent, AgentIdentifiedEvent, AttentionEvent, Event,
    KernelSettingsEvent, PaneTitleChangedEvent,
};
use ilium_plugin_api::generated::types::tree::{
    AgentActivity, NodeClass, NodeView, PaneInfo, PrimitiveKind, TitleOrigin,
};
use ilium_plugin_api::generated::types::{NodeId, PluginId};

/// The `config.toml` table of the colour theme.
const THEME_TABLE: &str = "theme";
/// The `config.toml` key of the user's key bindings.
const KEYMAP_KEY: &str = "keybindings";

/// The primitive behind a pane content kind.
fn primitive_of(content_kind: Option<&str>) -> PrimitiveKind {
    match content_kind {
        Some("terminal") => PrimitiveKind::Terminal,
        Some("editor") => PrimitiveKind::Editor,
        _ => PrimitiveKind::Basic,
    }
}

/// The events between two readings of the tree. `before` and `after` map a node id to its
/// view; `title_origin` says where a pane's current title came from; `focused` is the pane
/// that has focus in `after`. A node absent from `before` is new: a new pane yields
/// `pane-created`, and its agent (when one is already known) `agent-identified`.
pub fn node_events(
    before: &BTreeMap<u64, NodeView>,
    after: &BTreeMap<u64, NodeView>,
    focused: Option<NodeId>,
    title_origin: impl Fn(NodeId) -> TitleOrigin,
) -> Vec<Event> {
    let mut events = Vec::new();
    for (id, now) in after {
        if now.class != NodeClass::Pane {
            continue;
        }
        let previous = before.get(id);
        match previous {
            None => events.push(Event::PaneCreated(PaneInfo {
                node: now.id,
                content_kind: now.content_kind.clone().unwrap_or_default(),
                primitive: primitive_of(now.content_kind.as_deref()),
                columns: 0,
                rows: 0,
                focused: focused == Some(now.id),
                title: now.title.clone(),
                is_restored: false,
            })),
            Some(was) if was.title != now.title => {
                events.push(Event::PaneTitleChanged(PaneTitleChangedEvent {
                    node: now.id,
                    title: now.title.clone(),
                    source: title_origin(now.id),
                }));
            }
            Some(_) => {}
        }
        let was_agent = previous.and_then(|was| was.agent.as_ref());
        let Some(agent) = now.agent.as_ref() else {
            continue;
        };
        let is_same_agent = was_agent.is_some_and(|was| was.signature == agent.signature);
        if !is_same_agent {
            events.push(Event::AgentIdentified(AgentIdentifiedEvent {
                node: now.id,
                signature: agent.signature.clone(),
            }));
        }
        let previous_activity = match was_agent {
            Some(was) if is_same_agent => was.activity,
            _ => AgentActivity::None,
        };
        if previous_activity != agent.activity {
            events.push(Event::AgentActivityChanged(AgentActivityEvent {
                node: now.id,
                previous: previous_activity,
                current: agent.activity,
                revision: now.revision,
            }));
        }
        let previous_goal = was_agent
            .filter(|_| is_same_agent)
            .and_then(|was| was.goal.as_ref());
        if previous_goal != agent.goal.as_ref() && (is_same_agent || agent.goal.is_some()) {
            events.push(Event::AgentGoalChanged(AgentGoalEvent {
                node: now.id,
                goal: agent.goal.clone(),
            }));
        }
    }
    events
}

/// `pane-focused` when the focused pane changed.
pub fn focus_event(before: Option<NodeId>, after: Option<NodeId>) -> Option<Event> {
    (before != after).then_some(Event::PaneFocused(after))
}

/// The keys of two tables whose values differ, sorted.
fn changed_keys(before: Option<&toml::Value>, after: Option<&toml::Value>) -> Vec<String> {
    let empty = toml::Table::new();
    let table = |value: Option<&toml::Value>| match value {
        Some(toml::Value::Table(table)) => table.clone(),
        _ => empty.clone(),
    };
    let (before, after) = (table(before), table(after));
    let mut keys: Vec<String> = before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(*key) != after.get(*key))
        .cloned()
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// The events between two readings of `config.toml`: one `kernel-settings-changed` per kernel
/// tab whose table changed (tabs sharing a table each get one), `theme-changed` when the
/// theme table changed and `keymap-changed` when the key bindings changed.
pub fn config_events(before: &toml::Table, after: &toml::Table) -> Vec<Event> {
    let mut events = Vec::new();
    for tab in crate::config_settings::kernel_tab_ids() {
        let Some(table) = crate::config_settings::table_of_tab(tab) else {
            continue;
        };
        let keys = changed_keys(before.get(table), after.get(table));
        if !keys.is_empty() {
            events.push(Event::KernelSettingsChanged(KernelSettingsEvent {
                tab: tab.to_owned(),
                keys,
            }));
        }
    }
    if before.get(THEME_TABLE) != after.get(THEME_TABLE) {
        events.push(Event::ThemeChanged);
    }
    if before.get(KEYMAP_KEY) != after.get(KEYMAP_KEY) {
        events.push(Event::KeymapChanged);
    }
    events
}

/// One `attention-changed`: `marked` with the reason, or cleared.
pub fn attention_event(node: NodeId, reason: Option<String>, by: Option<&str>) -> Event {
    Event::AttentionChanged(AttentionEvent {
        node,
        marked: reason.is_some(),
        reason,
        by: by.map(|plugin| PluginId(plugin.to_owned())),
    })
}

/// Reads `config.toml` for [`config_events`]; a missing or unreadable file reads as empty,
/// so a broken file never produces a storm of events, only one change when it is fixed.
pub fn read_config(path: &std::path::Path) -> toml::Table {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<toml::Table>(&text).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_api::generated::types::tree::AgentView;

    fn pane(id: u64, title: &str, agent: Option<(&str, AgentActivity)>) -> NodeView {
        NodeView {
            id: NodeId(id),
            parent: None,
            class: NodeClass::Pane,
            content_kind: Some("terminal".to_owned()),
            title: title.to_owned(),
            cwd: None,
            agent: agent.map(|(signature, activity)| AgentView {
                signature: signature.to_owned(),
                activity,
                objective: None,
                goal: None,
            }),
            locked: false,
            revision: 1,
            container: None,
        }
    }

    fn map(views: Vec<NodeView>) -> BTreeMap<u64, NodeView> {
        views.into_iter().map(|view| (view.id.0, view)).collect()
    }

    fn manual(_: NodeId) -> TitleOrigin {
        TitleOrigin::Manual
    }

    #[test]
    fn a_new_pane_with_an_agent_is_created_then_identified() {
        let after = map(vec![pane(
            3,
            "shell",
            Some(("codex", AgentActivity::Working)),
        )]);
        let events = node_events(&BTreeMap::new(), &after, Some(NodeId(3)), manual);
        assert!(matches!(&events[0], Event::PaneCreated(info)
            if info.node == NodeId(3) && info.focused && info.primitive == PrimitiveKind::Terminal));
        assert!(matches!(&events[1], Event::AgentIdentified(found) if found.signature == "codex"));
        assert!(matches!(&events[2], Event::AgentActivityChanged(change)
            if change.previous == AgentActivity::None && change.current == AgentActivity::Working));
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn a_rename_and_an_activity_change_are_reported_once() {
        let before = map(vec![pane(1, "a", Some(("claude", AgentActivity::Working)))]);
        let after = map(vec![pane(1, "b", Some(("claude", AgentActivity::Idle)))]);
        let events = node_events(&before, &after, None, manual);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], Event::PaneTitleChanged(change)
            if change.title == "b" && change.source == TitleOrigin::Manual));
        assert!(matches!(&events[1], Event::AgentActivityChanged(change)
            if change.previous == AgentActivity::Working && change.current == AgentActivity::Idle));
        assert!(node_events(&after, &after, None, manual).is_empty());
    }

    #[test]
    fn a_goal_change_of_the_same_agent_is_reported() {
        let before = map(vec![pane(1, "a", Some(("codex", AgentActivity::Working)))]);
        let mut after = before.clone();
        if let Some(agent) = after.get_mut(&1).and_then(|view| view.agent.as_mut()) {
            agent.goal = Some("ship it".to_owned());
        }
        let events = node_events(&before, &after, None, manual);
        assert!(matches!(&events[..], [Event::AgentGoalChanged(change)]
            if change.goal.as_deref() == Some("ship it")));
    }

    #[test]
    fn focus_moves_are_reported() {
        assert!(focus_event(Some(NodeId(1)), Some(NodeId(1))).is_none());
        assert!(matches!(
            focus_event(Some(NodeId(1)), Some(NodeId(2))),
            Some(Event::PaneFocused(Some(NodeId(2))))
        ));
    }

    #[test]
    fn config_changes_name_their_tab_and_keys() {
        let before: toml::Table =
            toml::from_str("[ui]\nshow_icons = true\n[theme]\nfocus = \"red\"").unwrap();
        let after: toml::Table =
            toml::from_str("[ui]\nshow_icons = false\n[theme]\nfocus = \"blue\"\nkeybindings = []")
                .unwrap();
        let events = config_events(&before, &after);
        let tabs: Vec<&str> = events
            .iter()
            .filter_map(|event| match event {
                Event::KernelSettingsChanged(change) => Some(change.tab.as_str()),
                _ => None,
            })
            .collect();
        // `user-interface` and `icons` share the `ui` table.
        assert_eq!(tabs, vec!["user-interface", "icons"]);
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::ThemeChanged)));
        assert!(!events
            .iter()
            .any(|event| matches!(event, Event::KeymapChanged)));
        let with_keys: toml::Table = toml::from_str("keybindings = []").unwrap();
        assert!(config_events(&toml::Table::new(), &with_keys)
            .iter()
            .any(|event| matches!(event, Event::KeymapChanged)));
        assert!(config_events(&after, &after).is_empty());
    }

    #[test]
    fn attention_events_carry_the_marker() {
        let event = attention_event(NodeId(4), Some("look".to_owned()), Some("acme"));
        assert!(matches!(event, Event::AttentionChanged(change)
            if change.marked && change.by == Some(PluginId("acme".to_owned()))));
    }
}
