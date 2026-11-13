//! Kernel entry points that hand a user action to a plugin command (foundation contract
//! section 15.3).
//!
//! A ported feature keeps its key binding, toolbar button and menu entry in the kernel until
//! the plugins that own those surfaces are ported; what they do is run the plugin's command.
//! The kernel passes what only it knows (the selection) as the command's arguments and
//! decides nothing else.

use ilium_plugin_api::generated::types::CommandOrigin;

use crate::app::App;

/// The Layout plugin's "New split view" command.
pub(crate) const NEW_SPLIT_COMMAND: &str = "ilium.core.layout/new-split";

impl App {
    /// Runs a plugin command. Tells the user when no plugin host is running; a command that
    /// no enabled plugin contributes is reported by the host as a toast.
    pub(crate) fn invoke_plugin_command(
        &mut self,
        command: &str,
        arguments_json: String,
        origin: CommandOrigin,
    ) {
        let screen = self.popup_screen();
        let posted = self
            .wasm_plugins
            .as_ref()
            .is_some_and(|plugins| plugins.invoke_command(command, arguments_json, origin, screen));
        if !posted {
            let owner = command.split_once('/').map_or(command, |(owner, _)| owner);
            self.status_message = Some(format!(
                "{owner} is not running, so this action is unavailable"
            ));
        }
    }

    /// "New split view" from the key binding, the tree toolbar or the context menu.
    pub(crate) fn action_new_split_view(&mut self, origin: CommandOrigin) {
        let arguments = self.new_split_arguments();
        self.invoke_plugin_command(NEW_SPLIT_COMMAND, arguments, origin);
    }

    /// The arguments of `new-split`: the group the selection points at. The plugin reads the
    /// tree itself and decides which panes may join.
    fn new_split_arguments(&self) -> String {
        serde_json::json!({ "parent": self.split_parent_group().0 }).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::ROOT_ID;

    #[test]
    fn new_split_arguments_carry_the_selected_group() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.tree_state.select(vec![group]);

        let arguments: serde_json::Value =
            serde_json::from_str(&app.new_split_arguments()).unwrap();

        assert_eq!(arguments, serde_json::json!({ "parent": group.0 }));
    }

    #[test]
    fn a_plugin_command_without_a_plugin_host_tells_the_user() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.action_new_split_view(CommandOrigin::Keybinding);
        assert_eq!(
            app.status_message.as_deref(),
            Some("ilium.core.layout is not running, so this action is unavailable")
        );
    }
}
