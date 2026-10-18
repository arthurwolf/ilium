# API gaps for agent-setup feature tests

- ilium-plugin-sdk (testing): `FakeFs::symlink(&mut self, link: FsPath, target: FsPath)` and `FakeFs::is_symlink(&self, path: FsPath) -> bool`; F-063; the symbolic-link-kept rule cannot be set up or asserted without them.
- ilium-plugin-sdk (testing): `FakeFs::all_paths(&self) -> Vec<FsPath>`; F-063; finds the hook file written by Chatroom setup, whose path the page does not name.
- ilium-plugin-sdk (testing): `FakeHost::render_kernel_tab(&mut self, surface: &str, width: u16, height: u16) -> String`; F-037, COR-030; golden renders of the Setup tab.
- ilium-plugin-sdk (testing): `FakeHost::kernel_tab_row_ids(&mut self, surface: &str) -> Vec<String>`; F-037, COR-033; check-list row ids `block|target`.
- ilium-plugin-sdk (testing): `FakeHost::press_row(&mut self, surface: &str, row_id: &str)`, `render_popup(&mut self, width: u16) -> String`, `press_popup_button(&mut self, label: &str)`; F-063; the confirm dialog.
- ilium-plugin-sdk (testing): `FakeHost::type_into_field(&mut self, surface: &str, field: &str, text: &str)` and `press_field_key(&mut self, surface: &str, field: &str, key: char)`; F-037; the 600 ms save and R restore.
- ilium-plugin-sdk (testing): `FakeHost::invoke_command(&mut self, id: &str)`; F-063; the refresh command (command id `agent-setup.refresh` is assumed, check the manifest).
- ilium-plugin-sdk (testing): `FakeHost::disable_plugin(&mut self)` and `enable_plugin(&mut self)`; F-037; the disabled notice and kept values.
- ilium-plugin-sdk (testing): `FakeHost::hook_calls(&self, point: &str) -> Vec<String>`, `set_hook_reply(&mut self, point: &str, reply: HookReply)`, `set_hook_reply_when(&mut self, point: &str, payload_match: Value, reply: HookReply)`; F-063; the before-install veto.
- ilium-plugin-sdk (testing): `FakeHost::contribute_extension(&mut self, point: &str, id: &str, order: i32, value: Value)`, `withdraw_extension(&mut self, point: &str, id: &str)`, `provide_help_file(&mut self, path: &str, text: &str)`; F-063; contributed blocks and targets.
- ilium-plugin-sdk (testing): `FakeHost::open_project(&mut self, name: &str)`, `submitted(&self) -> Vec<Intent>`, `inject_failure(&mut self, op: &str, error: PluginError)`, `revoke(&mut self, grant: &str)`; F-063; project open, notices, failures, grants.
- ilium-plugin-sdk (types): `Event::DurableTimerFired(DurableTimerEvent { timer, name, late_by_millis })`, `Event::TopicPublished(TopicPublishedEvent)`, `CommandInvokedEvent { command, origin, args_json }`; F-063; event payload shapes assumed.
- Unspecified by the page: target ids (`claude-global`, `codex-global`, `claude-project`, `codex-project` assumed in common.rs), the status row JSON keys (block, target, scope, path, status, installed, current), and the badge word for newer.
- The Chatroom page shows an install request shape `{feature, project}` that conflicts with the manifest schema `{block, target}`; tests follow the manifest.
