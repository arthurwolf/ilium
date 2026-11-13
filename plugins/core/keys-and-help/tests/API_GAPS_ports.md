# API gaps relied on by the keys-and-help port twins

- ilium-plugin-sdk: `FakeHost::keymap() -> FakeKeymap` with `set_bindings(Vec<Binding>)` and `set_label(&str, &str)`; REQ PORT:*; seeds the keymap the key reference renders (shared with tests/features).
- ilium-plugin-sdk: `FakeHost::kernel_settings()` with `set_tab(&str, serde_json::Value)`, `tab(&str) -> serde_json::Value`, `writes() -> Vec<(String, String, String)>`; REQ PORT:*; observe the Keyboard tab writes.
- ilium-plugin-sdk: `FakeHost::invalidated()`, `failures()`, `inject_failure(&str, PluginError)`; REQ PORT:*; failure and refresh channels.
- ilium-plugin-sdk: `Screen::find(&self, &str) -> Option<(u16, u16)>`, `Screen::row(&self, u16) -> String`; REQ PORT:*; render inspection.
