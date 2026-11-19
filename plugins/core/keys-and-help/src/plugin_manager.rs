//! Plugin manager presentation and interaction state.

use ilium_plugin_sdk::host::{plugins::PluginInfo, HostPlugins};
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, Constraint, DialogKind, DialogSpec, NodeFlags, Outcome, PluginId,
    RequestId, RequestRef, RequestResult, ScrollViewSpec, Span, Style, ToggleSpec, ToggleStyle,
    UiAction, UiNode, UiTree, Widget,
};
use ilium_plugin_sdk::host;

#[derive(Default)]
pub struct PluginManager {
    plugins: Vec<PluginInfo>,
    error: Option<String>,
    pending: Option<RequestId>,
}

impl PluginManager {
    pub fn refresh(&mut self) {
        self.plugins = HostPlugins::list(&mut *host());
        self.error = None;
    }

    pub fn act(&mut self, action: &UiAction) {
        if self.pending.is_some() {
            return;
        }
        let (ActionKind::Toggled, Some(ActionValue::Flag(enabled))) = (action.kind, &action.value) else {
            return;
        };
        let Some(plugin) = self.plugins.iter().find(|plugin| plugin.id.0 == action.key) else {
            return;
        };
        let result = HostPlugins::set_enabled(
            &mut *host(),
            PluginId(plugin.id.0.clone()),
            *enabled,
            plugin.scope.clone(),
        );
        match result {
            Ok(request) => {
                self.pending = Some(request);
                self.error = None;
            }
            Err(error) => self.error = Some(error.message),
        }
    }

    pub fn completed(&mut self, result: &RequestResult) -> bool {
        if !matches!((&self.pending, &result.request), (Some(expected), RequestRef::Call(actual)) if expected == actual) {
            return false;
        }
        self.pending = None;
        match &result.outcome {
            Outcome::Ok => self.refresh(),
            Outcome::Failed(error) | Outcome::Refused(error) => self.error = Some(error.message.clone()),
            Outcome::Cancelled => self.error = Some("Plugin change was cancelled".to_owned()),
            _ => self.error = Some("Unexpected plugin change result".to_owned()),
        }
        true
    }

    pub fn render(&self) -> UiTree {
        let fields: Vec<UiNode> = self.plugins.iter().map(|plugin| {
            let label = if plugin.failed { format!("{} (failed)", plugin.name) } else { plugin.name.clone() };
            UiNode {
                key: Some(plugin.id.0.clone()),
                widget: Widget::Toggle(ToggleSpec {
                    label,
                    value: plugin.enabled,
                    style: ToggleStyle::Switch,
                    tri_state: false,
                    icon_on: None,
                    icon_off: None,
                }),
                children: Vec::new(),
                flags: NodeFlags::default(),
                tooltip: None,
                tab_index: None,
                disabled_reason: self.pending.as_ref().map(|_| "Changing plugin state".to_owned())
                    .or_else(|| plugin.disabled_reason.clone()),
            }
        }).collect();
        let title = Span { text: "Plugins".to_owned(), style: Style::default() };
        let message = self.error.as_ref().map(String::as_str)
            .or(self.pending.as_ref().map(|_| "Changing plugin state…"))
            .map(|text| vec![vec![Span { text: text.to_owned(), style: Style::default() }]]);
        let root = UiNode {
            key: Some("plugins".to_owned()),
            widget: Widget::Dialog(DialogSpec {
                title: vec![title], kind: DialogKind::Plain, buttons: Vec::new(),
                width: Constraint::Length(60), height: Constraint::Min(0), message,
            }),
            children: vec![1],
            flags: NodeFlags::default(), tooltip: None, tab_index: None, disabled_reason: None,
        };
        let scroll = UiNode {
            key: Some("plugin-list".to_owned()),
            widget: Widget::ScrollView(ScrollViewSpec {
                content_rows: u32::try_from(fields.len()).unwrap_or(u32::MAX),
                show_scrollbar: true,
                follow_tail: false,
            }),
            children: (2..2 + fields.len() as u32).collect(),
            flags: NodeFlags::default(), tooltip: None, tab_index: None, disabled_reason: None,
        };
        let mut nodes = vec![root, scroll];
        nodes.extend(fields);
        UiTree { version: 1, nodes, root: 0 }
    }
}
