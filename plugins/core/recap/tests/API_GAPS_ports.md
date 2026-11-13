# API gaps found while writing the port twins

The gaps already listed in `API_GAPS.md` (set_kernel_settings, set_onboarding_finished, screen_reads,
transcript_reads, transcript_queries, running_job_limit, inference_*, run_job, set_screen,
set_screen_unavailable, set_agent_session, FakeCli refresh/cancel/set_env) apply to the twins too and are
not repeated. New gaps:

| crate | exact signature | id | reason |
|---|---|---|---|
| ilium-plugin-sdk (testing::fakes::tree) | `FakeTree::set_agent_goal(&mut self, id: u64, objective: &str, status: &str)` | F-112 | goal lines need recorded goal state (active, paused, blocked, done, unknown); the host, not the model, supplies it |
| ilium-plugin-sdk (testing::fakes::tree) | `FakeTree::set_pane_directory(&mut self, id: u64, directory: &str)` | F-112 | the protected-directory twin needs a pane whose project directory is `.ssh` |
| ilium-plugin-sdk (testing::fakes::tree) | `FakeTree::set_agent_session(&mut self, id: u64, session: &str, turns: Vec<String>)` must allow replacing the session id mid-run | F-112 | the fence twins re-attach a pane to a new session while a job runs |
| ilium-plugin-sdk (testing::fakes::cli) | `Output::exit_code(&self) -> Option<u8>` | F-112 | failed-provider twin asserts exit 0 |
| ilium-plugin-sdk (testing::fakes) | `FakeHost::transcript_queries(&mut self) -> Vec<TranscriptQuery>` with `max_bytes` | F-112 | window-reader twin checks the 512 KiB query bound |
| ilium-plugin-sdk (testing::fakes::tree) | `agent_pane(name, signature)` must accept "claude", "codex", "antigravity", "custom" | F-112 | agent display-name twin maps each to a transcript class |

Window-reader and transcript-parsing behaviour (partial edge lines, oversize lines) is only partly observable:
the host parses the session file into turns, so the twins assert the resulting evidence, not the byte window.
