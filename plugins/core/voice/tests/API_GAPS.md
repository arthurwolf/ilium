# API gaps for the voice feature tests

Tests are in tests/features/. Items the skeleton or harness does not offer yet (testkit = ilium-plugin-testkit,
reached through `ilium_plugin_sdk::testing`). Each test is written against the item as listed.

- testkit: `FakeHost::keys(&mut self) -> &mut FakeKeys` with `FakeKeys::grabs(&self) -> Vec<KeyGrabRecord>` and
  `#[derive(Debug, Clone, PartialEq)] pub struct KeyGrabRecord { pub id: u64, pub chords: Vec<String>, pub pane: Option<u64>, pub until_release: bool, pub released: bool }`; F-050 (keys.grab/release must be observable).
- testkit: `FakeHost::audio(&mut self) -> &mut FakeAudio` with `FakeAudio::captures(&self) -> Vec<CaptureRecord>`, `#[derive(Debug, Clone, PartialEq)] pub struct CaptureRecord { pub id: u64, pub device: Option<String>, pub sample_rate_hz: u32, pub channels: u16, pub open: bool }`; F-050, F-054 (audio.open_capture/close_capture).
- testkit: `FakeAudio::set_input_devices(&mut self, names: Vec<String>)`, `set_output_devices(&mut self, names: Vec<String>)`, `unplug(&mut self, name: &str)`, `plug(&mut self, name: &str)`; F-054 (device discovery and reconnect).
- testkit: `FakeHost::client_requests(&mut self) -> &mut FakeClientRequests` with `sent(&self) -> Vec<SentClientRequest>` (`pub struct SentClientRequest { pub request: RequestRef, pub client: u64, pub kind: String, pub payload_json: String, pub timeout_ms: u32 }`), `answer(&mut self, request: RequestRef, result: Result<String, PluginError>)` (server side: the client's answer) and `responses(&self) -> Vec<ClientResponseRecord>` (`pub struct ClientResponseRecord { pub call: u64, pub result: Result<String, PluginError> }`, client side: what `client_requests.respond` was given); F-052.
- testkit: `FakeHost::kernel_settings(&mut self, tab: &str) -> &mut FakeSettings` (the `[voice]` keys written through `settings.write-kernel:voice-control`; `get`/`set` as FakeSettings); F-050, F-054.
- testkit: `FakeHost::popups(&self) -> Vec<String>` (keys of the popups the plugin has open); F-051.
- testkit: `FakeHost::contribute_extension(&mut self, point: &str, id: &str, priority: i32, entry_json: &str)` (feeds a collect point of the plugin, here `ilium.core.voice/voice-intents`); F-051.
- testkit: `FakeHost::set_hook_reply(&mut self, point: &str, reply: HookReply)` and `FakeHost::hook_calls(&self, point: &str) -> Vec<String>` (payload JSON of every call the plugin made to a hook point it owns, here `ilium.core.voice/transcript`); F-050, F-051.
- testkit: `FakeHost::topics(&self) -> Vec<(String, String)>` (topic name, payload JSON of every `topics.publish`); F-050 (`ilium.core.voice/heard`).
- testkit: `FakeSession::install_core("voice")` must know the voice stub (core_stubs) and answer the `ilium.core.voice/say` service.
- Not on the page (taken from the sample and from src/docs/voice.md; confirm or adjust):
  - the settings page id `voice-control` and its action keys are the `[voice]` key names (`enabled`, `api_key`, `model`, `voice`, `reasoning_effort`, `input_mode`, `vad_eagerness`, `input_device_name`, `output_device_name`, `output_volume_percent`, `confirm_terminal_submissions`, `pause_media_while_active`, `custom_prompt`) plus `reconnect` for the Reconnect now button;
  - the strip is the panel `voice`; the confirmation is the popup `confirm-voice` (page sample 3); the onboarding step id is `voice`;
  - the `voice-say` payload fields (`sentences`, `start`) and the answer being the JSONL result line (page sample 4 shows `{ "text": ... }` and a timeout of 5000 ms for illustration);
  - the argument check of `ilium voice say` runs on the server side before the client is asked (kernel-and-boot: "only for its argument check") and reports `invalid-request`, including `--timeout-s` outside 1..=600;
  - the phrase table of built-in commands: "close the pane called <name>" maps to `kernel.close_pane` (page figure; command id from core-plugin-keys-and-help.md);
  - the `transcript` hook payload `{ text, confidence, language }` and reply `{ text }`; the `heard` topic payload `{ text, command }` with a null command when nothing matched.
- Not tested for lack of an observable channel: the onboarding lightbulb demo tool (`set_lightbulb`, 1024 calls, 1024 argument bytes) and the in-session behaviours of the Realtime conversation (tools, queue, media pause); they are not on the page and have no harness import.
