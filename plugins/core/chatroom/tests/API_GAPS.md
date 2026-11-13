# API gaps found while writing the chatroom feature tests

The tests under `tests/features/` use the items below. None exists in `ilium-plugin-testkit` or the SDK today,
or the page leaves the value open. Each needs an owner decision before the implementation phase.

Testkit additions the tests call:

- `FakeHost::published_topics() -> Vec<(String, String)>`: topic publications of the plugin as (topic, payload JSON).
- `FakeHost::service_calls() -> Vec<(String, String, String)>`: service calls the plugin made as (service, method, request JSON).
- `FakeHost::plugin::<P>() -> &mut P`: borrow the activated plugin to call an export (here `list_virtual_children`).
- `FakeHost::contribute_hook(point, closure)` and `contribute_hook_trap(point)`: register a hook handler that replies or traps, so the `before-post` transform can be tested.
- `FakeCli::set_env(name, value)` and `FakeCli::set_cwd(path)`: set the environment (`AGENT_NAME`, `ILIUM_CHATROOM_AUTHOR`, `ILIUM_PANE_ID`) and the working directory of a command line run.
- `FakeFs::mkdir`, `FakeFs::symlink(link, target)`, `FakeFs::replace_atomically(path, bytes)`, `FakeFs::write_quiet(path, bytes)` (write without delivering `fs-changed`).
- `FakeFs::lock_log() -> Vec<{path, exclusive}>`, `FakeFs::held_locks()`, `FakeFs::hold_lock(path, exclusive)`, `FakeFs::write_log() -> Vec<{path, append}>` and `FakeFs::tail_log() -> Vec<{max_bytes, returned_cursor}>`: observe the lock, write and tail calls (chatroom-B17, the 64 KiB read bound).
- `ilium_plugin_sdk::testing::TreeNode` re-exported next to `FakeHost`.
- `ActionKind::Changed` for the composer draft; the page names only `Submitted` and `Scrolled`.

Values the page leaves open (assumptions the tests make):

- The panel id of the message view is `chatroom.panel` (`ViewTarget::Panel`); the widget keys are `composer` and `scroll`.
- The client view is fed by `topic-published` events on `chatroom` whose payload is `{"text": "<record without the leading '- '>"}`; the list of agents is derived from the authors of the messages shown, in order of first appearance, joined by ` · `.
- The figure of the page captioned "Rendered at 48 columns" is 50 cells wide in every row; the golden `view_48.txt` is the figure as written and the test renders at 50 columns. Either the figure or the caption needs correcting.
- Forwarding a record that names a pane (`@name`, baseline `chatroom_router.rs` and `smoke.rs`) needs a `SendInput` intent and its grant; the manifest has no input grant and the page does not describe the feature. The tests expect `Intent::SendInput` to the named pane.
- The page does not name the failure behaviour of a held lock; the test expects exit 1 within the bounded lock wait.
- Reader key `user` for the unread count of the tree row is taken from chatroom-B16; the cursor file for `user` is not written by the row.
- Record limit of 20 KiB per record, 64 records and 64 KiB per read, 1 s reconcile and 15 s repair intervals come from the baseline source (`chatroom_router.rs`, `app.rs`), not from the page.
