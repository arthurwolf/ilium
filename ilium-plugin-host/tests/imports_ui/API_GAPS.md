# API gaps of the imports-ui tests

The skeleton gives a test no way to call a host import or to script host state. Every item below
is used by the tests exactly as written and is missing from the skeleton. Convention follows the
primitives lane (`rig.host.terminal()`).

## Import accessors on `HarnessHandle` (crate ilium-plugin-host, `testing::harness`)
Each returns a handle that implements the generated trait and runs the call through the real
import struct with the harness plugin's grants, side and quotas. Needed by every WIT id of the lane.
- `pub fn audio(&mut self) -> &mut impl HostAudio` (WIT:ilium:host/audio.*)
- `pub fn catalogs(&mut self) -> &mut impl HostCatalogs` (catalogs.*)
- `pub fn cli(&mut self) -> &mut impl HostCli` (cli.*)
- `pub fn client_requests(&mut self) -> &mut impl HostClientRequests` (client-requests.*)
- `pub fn clipboard(&mut self) -> &mut impl HostClipboard` (clipboard.*)
- `pub fn geocode(&mut self) -> &mut impl HostGeocode` (geocode.*)
- `pub fn gpu(&mut self) -> &mut impl HostGpu` (gpu.*)
- `pub fn help(&mut self) -> &mut impl HostHelp` (help.topic)
- `pub fn http_api(&mut self) -> &mut impl HostHttpApi` (http-api.respond)
- `pub fn inference(&mut self) -> &mut impl HostInference` (inference.*)
- `pub fn kernel_settings(&mut self) -> &mut impl HostKernelSettings` (kernel-settings.read)
- `pub fn keymap(&mut self) -> &mut impl HostKeymap` (keymap.*)
- `pub fn keys(&mut self) -> &mut impl HostKeys` (keys.*)
- `pub fn location(&mut self) -> &mut impl HostLocation` (location.current)
- `pub fn media(&mut self) -> &mut impl HostMedia` (media.*)
- `pub fn plugins(&mut self) -> &mut impl HostPlugins` (plugins.*)
- `pub fn sessions(&mut self) -> &mut impl HostSessions` (sessions.*)
- `pub fn sounds(&mut self) -> &mut impl HostSounds` (sounds.catalog)
- `pub fn ui_host(&mut self) -> &mut impl HostUiHost` (ui-host.*)
- `pub fn vector(&mut self) -> &mut impl HostVector` (vector.*)
- `pub fn workspace(&mut self) -> &mut impl HostWorkspace` (workspace.*)

## Harness builder and handle (crate ilium-plugin-host, `testing::harness`)
- `HostHarness::declined(self, requested: &[&str]) -> HostHarness`: the plugin requested these grants and the user declined them (returns `not-permitted`, differs from a never-requested trap). Needed by every `*_declined_grant_*` test.
- `HarnessHandle::request_completed(&self, request: RequestId) -> bool` (audio.play, catalogs.query-async).
- `HarnessHandle::request_error(&self, request: RequestId) -> Option<ErrorCode>` (timeouts of inference, geocode, catalogs, client-requests).
- `HarnessHandle::provider_calls(&self) -> Vec<ilium_plugin_api::generated::exports::inference_provider::ProviderCall>`: calls the harness plugin's own provider export received (inference.complete round trip).
- `HarnessHandle::set_provider_models(&mut self, models: Vec<String>)`: what the plugin's provider export returns from `models`.
- `HarnessHandle::call_provider_complete(&mut self, call: ProviderCall) -> Result<(), PluginError>` and `call_provider_models(&mut self) -> Result<Vec<String>, PluginError>`: direct calls of the `inference-provider` export (WIT:ilium:exports/inference-provider.*). `ExportCall` has only Hook, Job and Migrate.

## Scripted host state on `FakeKernel` (crate ilium-plugin-host, `testing::fake_kernel`)
Setters (all `&self`): `set_audio_devices(Vec<AudioDevice>)`, `set_loopback_available(bool)`, `set_clipboard(Option<String>)`, `set_location(Option<(f64, f64)>)`, `set_gpu_available(bool)`, `set_plugin_sounds(&[&str])`, `set_other_plugin_sounds(&str, &[&str])`, `set_kernel_tab(&str, &str)`, `set_kernel_tab_secret(&str, &str, &str)`, `set_keymap(Vec<Binding>)`, `set_help_topics(&[(&str, &str)])`, `set_installed_plugins(&[&str])`, `set_live_sessions(Vec<LiveSession>)`, `set_session_reply(&str, &str)`, `set_catalog(&str, Vec<CatalogItem>)`, `set_catalog_provider_delay(&str, Duration)`, `set_geocode_places(Vec<GeoPlace>)`, `set_geocode_silent(bool)`, `set_embedder(Option<u16>)`, `set_vector_entry_limit(u32)`, `select_inference_provider(&str)`, `set_video(&str, &str, VideoInfo)`, `set_bytes_video(VideoInfo)`, `set_ffmpeg_available(bool)`, `set_git_repository(&str, &str)`, `set_workspace_in_use(u64, bool)`, `hold_chord_for(&str, &str)`, `register_client(ClientId, &[&str])`, `declare_panel(&str)`, `commit_view_tree(&str, &[(&str, &str)], &[&str])` (view, (widget kind, key) pairs, popup keys; kind `surface:rgb8:4x4` declares a surface), `set_widget_state(&str, &str, WidgetState)`, `set_theme(u32, bool)`, `set_icon_mode(IconMode)`, `set_density(Density)`, `set_reduced_motion(bool)`.
- Observers: `open_captures() -> Vec<u64>`, `played_audio() -> Vec<Vec<i16>>`, `clipboard() -> Option<String>`, `plugin_enabled(&str) -> bool`, `session_commands(&str) -> Vec<String>`, `active_grabs() -> Vec<u64>`, `video_position(u64) -> Option<u64>`, `open_panels() -> Vec<String>`, `open_popups() -> Vec<String>`, `popup_options(&str) -> Option<PopupOptions>`, `render_count(&str) -> u32`, `surface_rows(&str, &str, u32, u32) -> Vec<u8>`, `client_inbox(ClientId) -> Vec<(String, String)>`, `client_request_answer(u64) -> Option<Result<String, PluginError>>`, `client_answers(ClientId, RequestId, Result<String, PluginError>)` (a scripted client answer), `http_response(u64) -> Option<HttpAnswer>` with `status: u16`, `headers`, `body: Vec<u8>`, `cli_stdout(u64) -> Vec<String>`, `cli_stdout_text(u64) -> String`, `cli_stderr_text(u64) -> String`, `cli_errors(u64) -> Vec<String>`, `cli_exit_code(u64) -> Option<u8>`.
- Hooks that open a host-side call with a known id (a hand-built event does not make the host treat the call as open): `start_cli_invocation(output: &str, kind: &str) -> u64` (`output` is the declared `jsonl` or `text`) and the variants `start_cli_invocation_with_stdin(.., &[u8])`, `start_cli_invocation_without_stdin(..)`, `start_cli_invocation_with_tty(.., (bool, bool))`, `start_cli_invocation_from_pane(.., NodeId)`, `start_cli_invocation_with_selection(.., Vec<NodeId>, bool)`; `start_http_call(method: &str, path: &str) -> u64`; `start_client_request_call(kind: &str, payload_json: &str) -> u64`.

## Spec conflicts and assumptions (no API change needed)
- Audio sample-rate range: `wit/host/audio.wit` and reference-wit.md say 8,000 to 48,000, `api-audio.md` says 8,000 to 192,000. The tests use 7,999 (invalid), 8,000 and 48,000 (valid) and 192,001 (invalid) and skip 48,001 to 192,000 until the manual is reconciled.
- Grant names not stated verbatim on the pages were taken from the grant lists: `inference.complete`, `inference.provide`, `catalogs` use `services.use`, `ui.panels`. A different spelling only changes the rig helpers' `GRANTS` constants.
- Tests for a never-requested import (trap `host.capability-not-requested`) are not in this lane: no accessor exposes a trap-or-value result for an unlinked function.
