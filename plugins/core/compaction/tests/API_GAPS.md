# API gaps for the compaction feature tests

Format: crate, signature, requirement id, why.

- ilium-plugin-testkit: `FakeHost::run_job(&mut self, kind: &str, input: Vec<u8>) -> Result<Vec<u8>, PluginError>` (F-080): run the job-runner export natively.
- ilium-plugin-testkit: `FakeHost::run_job_for_pane(&mut self, pane: u64, kind: &str, input: Vec<u8>) -> Result<Vec<u8>, PluginError>` (F-080): technique selection per agent.
- ilium-plugin-testkit: `FakeHost::script_inference(&mut self, replies: Vec<Result<String, PluginError>>)` (F-080): scripted model replies.
- ilium-plugin-testkit: `FakeHost::inference_requests(&self) -> Vec<(String, CompletionRequest)>` (F-080): inspect prompts sent.
- ilium-plugin-testkit: `FakeHost::job_progress_reports(&self) -> Vec<f64>` and `cancel_job_after_chunks(&mut self, n: usize)` (F-080): progress and cancellation.
- ilium-plugin-testkit: `FakeHost::agents()` with `set_transcript`, `transcript`, `set_transcript_verified`, `transcript_backups` (F-080): agent.transcript fake.
- ilium-plugin-testkit: `FakeJobs::list()` entries with `kind`, `input`, `request`, and `finish(request, Result<Vec<u8>, PluginError>)` (F-080).
- ilium-plugin-testkit: `FakeHost::script_hook_reply`, `script_hook_trap`, `hook_calls`, `published_topics`, `set_collected` (F-080): extension points and topic.
- ilium-plugin-testkit: `FakeSession::call_service`, `install_core`, `server`, `client`, `pump` as used in extension_points.rs and trigger_reader.rs (F-080, F-083).
- ilium-plugin-testkit: `FakeHost::fs()` with `write`/`read` on `agent-homes:` paths (F-083).
- ilium-plugin-testkit: `FakeHost::open_view`, `send_key`, `view_closed`, `scans_started`, `set_recommendation`, `recommended_tokens`, `set_scan_result`, `agent_files`, `agent_file_writes`, `set_agent_env`, `select_agent_target`, `convert_session`, `convert_session_failing` (F-082, F-115): optimiser panel driving.
- ilium-plugin-testkit: `FakeHost::settings().set/get` on `remote_compaction.*` keys and `FakeHost::add_agent` via `tree().add(TreeNode)` with `agent: Some(AgentView)` (F-080).

## Assumptions the manual leaves open

- The channel carrying trigger data from the server (fs.read:agent-homes) to the client tab is unspecified; tests read it through the rendered tab.
- Assumed windows: Claude Code 1 000 000 and Codex 258 400 tokens, from the existing implementation.
- Exact optimiser confirmation text follows the existing client implementation.
