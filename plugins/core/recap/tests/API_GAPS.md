# API gaps of the recap feature tests

Every item is used by tests in `features/`; the manual's reference-test-harness.md names the harness areas but the skeleton lacks these methods. All are on `ilium-plugin-testkit` (`ilium_plugin_sdk::testing`).

- `FakeHost::set_kernel_settings(&mut self, tab: &str, value: serde_json::Value)`: the read-only kernel settings tab the plugin reads with `settings.read-kernel:inference` (consent gate: `automatic_ai`). Used by `open_gate`/`close_gate` and the gate tests (F-112).
- `FakeHost::set_kernel_settings_unreadable(&mut self, tab: &str)`: makes `kernel_settings::read(tab)` fail; page rule "unreadable means closed" (F-112).
- `FakeHost::set_onboarding_finished(&mut self, finished: bool)`: the state the client side answers to the `onboarding-state` client request (F-112).
- `FakeHost::client_requests(&self) -> Vec<ClientRequestRecord>` with `pub struct ClientRequestRecord { pub kind: String, pub payload_json: String, pub timeout_ms: u32 }`: requests the server side sent with `client_requests::request` (F-112, page "The consent gate": kind `onboarding-state`, payload `{}`, 2000 ms).
- `FakeHost::leave_client_requests_unanswered(&mut self)`: the fake client never answers, so the 2000 ms timeout path can be tested (F-112).
- `FakeHost::answer_client_request(&mut self, kind: &str, payload_json: &str) -> Option<String>`: on a client-side host, delivers `Event::ClientRequest` and returns the `client_requests::respond` payload, `None` for an unanswered kind (F-112).
- `FakeTree::set_screen(&mut self, id: u64, rows: Vec<String>)`: the visible screen rows `terminal::read_screen` returns for a pane (F-112, shell evidence).
- `FakeTree::set_screen_unavailable(&mut self, id: u64)`: `read_screen` returns `None` for the pane (F-112).
- `FakeTree::set_agent_session(&mut self, id: u64, session: &str, transcript: Vec<String>)`: gives the pane a verified session and a transcript (F-112, agent evidence).
- `FakeHost::screen_reads(&self) -> usize` and `FakeHost::transcript_reads(&self) -> usize`: how many `read_screen` / `agent::transcript` calls the plugin made (F-112: a closed gate and an unverified agent read nothing).
- `FakeHost::screen_requests(&self) -> Vec<ScreenRequest>` with `pub struct ScreenRequest { pub rows: u16, pub visible_only: bool }`: the arguments of each `read_screen` (F-112: visible part, 40 rows).
- `FakeHost::transcript_queries(&self) -> Vec<TranscriptQuery>`: reuse the SDK `TranscriptQuery { pane, last_turns: u32, max_bytes: u32 }` of each `agent::transcript` call (F-112: 40 turns, 512 KiB).
- `FakeCli::refresh(&mut self, invocation: &Invocation) -> Invocation`: re-reads stdout, stderr and exit code of an invocation that was still open when `invoke` returned (jobs finish later) (F-112).
- `FakeCli::cancel(&mut self, invocation: &Invocation)`: delivers `cli-cancelled` for the invocation (F-112, Ctrl-C ends the run with `cancelled: true`).
- `FakeCli::set_env(&mut self, name: &str, value: &str)`: sets the environment the invocation sees, here `ILIUM_PANE_ID` for the pane running the command (F-112, "the pane that runs the command is skipped").
- `FakeHost::run_job(&mut self, kind: &str, input: &[u8]) -> Result<Vec<u8>, PluginError>`: calls the `job-runner.run` export directly (F-112, job `recap-pane`).
- `FakeHost::set_inference_answer(&mut self, text: String)`, `FakeHost::inference_requests(&self) -> Vec<InferenceRecord>` with `pub struct InferenceRecord { pub purpose: String, pub messages: Vec<ChatMessageRecord>, pub json_schema: Option<String>, pub max_output_tokens: Option<u32> }` and `pub struct ChatMessageRecord { pub role: String, pub content: String }`, `FakeHost::inference_wait_ms(&self) -> Vec<u32>`: fake of `inference::complete_request`/`take_completion` and `jobs::wait_request` (F-112).
- `FakeHost::reported_progress(&self) -> Vec<f32>` and `FakeHost::cancel_current_job(&mut self)`: observe `jobs::report_progress` and make `jobs::is_cancelled` true (F-112).
- `FakeHost::set_extension_entries(&mut self, point: &str, entries: Vec<String>)` and `FakeHost::extension_list_calls(&self, point: &str) -> usize`: fake `extensions::list` for the owner side of `fact-sources` (F-112).
- `FakeHost::set_service_answer(&mut self, service: &str, reply: String)` and `FakeHost::fail_service(&mut self, service: &str)`: fake `services::call` to other plugins (F-112, fact sources).
- `FakeHost::set_hook_reply(&mut self, point: &str, reply_json: String)`, `FakeHost::fail_hook(&mut self, point: &str)` and `FakeHost::hook_runs(&self, point: &str) -> Vec<String>` (payload JSON of each `extensions::run_hook`): fake the owner side of the `before-recap` transform hook (F-112).
- `FakeHost::running_job_limit(&self) -> usize`: the `concurrent_jobs` limit the plugin set for the run, from `--max-concurrency` (F-112).
