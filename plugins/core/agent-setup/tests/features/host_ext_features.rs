//! Agent-setup hooks on `FakeHost` that only the feature test target uses (commands, help files, field keys).
//!
//! Shared hooks live in `tests/host_ext.rs`; the UI ones ask the plugin's service `ilium.core.agent-setup/ui`.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{CommandContext, CommandInvokedEvent, CommandOrigin, Event};
use serde_json::json;

use crate::host_ext::ui_call;

/// Feature-target hooks. Implemented for `FakeHost`.
pub trait AgentSetupFeaturesHostExt {
    /// Runs a registered command by id (agent-setup "Commands").
    fn invoke_command(&mut self, command: &str);
    /// Row ids of the contributed tab, in order.
    fn kernel_tab_row_ids(&mut self, surface: &str) -> Vec<String>;
    /// Sends a key to a text field of the contributed tab.
    fn press_field_key(&mut self, surface: &str, field: &str, key: char);
    /// Provides a help file the plugin ships.
    fn provide_help_file(&mut self, path: &str, text: &str);
}

impl AgentSetupFeaturesHostExt for FakeHost {
    fn invoke_command(&mut self, command: &str) {
        self.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
            command: command.to_string(),
            arguments_json: "{}".to_string(),
            origin: CommandOrigin::Palette,
            context: CommandContext {
                pane: None,
                view: None,
                buffer: None,
                line: None,
                virtual_row: None,
                menu_item: None,
            },
        }));
        self.settle();
    }

    fn kernel_tab_row_ids(&mut self, surface: &str) -> Vec<String> {
        ui_call(self, "row-ids", json!({ "surface": surface }))["rows"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn press_field_key(&mut self, surface: &str, field: &str, key: char) {
        ui_call(
            self,
            "field-key",
            json!({ "surface": surface, "field": field, "key": key.to_string() }),
        );
        self.settle();
    }

    /// The plugin reads shipped help files through the host's help store.
    fn provide_help_file(&mut self, path: &str, text: &str) {
        self.help().add_topic(path, text);
    }
}
