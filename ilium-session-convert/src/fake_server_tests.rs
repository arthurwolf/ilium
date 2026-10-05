//! Drives the real subprocess/JSON-RPC client against a fake `codex`
//! executable (a shell script speaking just enough of the app-server
//! protocol). Unix only: the fake is a `/bin/sh` script.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ilium_core::BuiltinAgentProvider;
use serde_json::json;

use crate::claude_to_codex::{convert, Timeouts};
use crate::error::ConvertError;
use crate::report::Reporter;
use crate::{ConversionEvent, ConversionRequest};

const CLAUDE_SESSION_ID: &str = "11111111-2222-4333-8444-555555555555";
const THREAD_ID: &str = "01a0f39a-7c0e-7722-97d0-06365c66f201";

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    codex_home: PathBuf,
    project: PathBuf,
    transcript: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let home = root.join("home");
        let codex_home = home.join(".codex");
        let project = root.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&codex_home).unwrap();
        let slug: String = project
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let directory = home.join(".claude/projects").join(slug);
        std::fs::create_dir_all(&directory).unwrap();
        let transcript = directory.join(format!("{CLAUDE_SESSION_ID}.jsonl"));
        let cwd = project.to_string_lossy();
        let lines = [
            json!({"type":"user","uuid":"u1","parentUuid":null,"sessionId":CLAUDE_SESSION_ID,"cwd":cwd,"message":{"role":"user","content":"hello"}}),
            json!({"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":CLAUDE_SESSION_ID,"cwd":cwd,"message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}),
            json!({"type":"ai-title","aiTitle":"t","sessionId":CLAUDE_SESSION_ID}),
        ];
        let body: Vec<String> = lines.iter().map(ToString::to_string).collect();
        std::fs::write(&transcript, body.join("\n")).unwrap();
        Self {
            _temporary: temporary,
            root,
            home,
            codex_home,
            project,
            transcript,
        }
    }

    fn request(&self, executable: &Path) -> ConversionRequest {
        ConversionRequest {
            home_dir: self.home.clone(),
            project_cwd: self.project.clone(),
            source: BuiltinAgentProvider::Claude,
            target: BuiltinAgentProvider::Codex,
            source_session_id: CLAUDE_SESSION_ID.to_string(),
            codex_home: Some(self.codex_home.clone()),
            codex_executable: Some(executable.to_path_buf()),
        }
    }

    fn rollout_path(&self) -> PathBuf {
        self.codex_home.join(format!(
            "sessions/2026/09/30/rollout-2026-09-30T10-00-00-{THREAD_ID}.jsonl"
        ))
    }

    fn pid_file(&self) -> PathBuf {
        self.root.join("fake.pid")
    }

    /// Writes an executable fake `codex` whose post-handshake behaviour is
    /// `scenario` (shell text run after the import request was read).
    fn fake_codex(&self, scenario: &str) -> PathBuf {
        let script = format!(
            r#"#!/bin/sh
echo "$$" > '{pid}'
echo "$*" > '{args}'
echo "$CODEX_HOME|$HOME" > '{env}'
echo "fake stderr chatter" >&2
sleep 0.1
read -r init_line
printf '%s\n' '{{"id":1,"result":{{"userAgent":"fake-codex/0","codexHome":"{codex_home}","platformFamily":"unix","platformOs":"linux"}}}}'
printf '%s\n' 'this line is not json'
printf '%s\n' '{{"id":77,"method":"item/tool/requestUserInput","params":{{}}}}'
read -r initialized_line
read -r import_line
{scenario}
cat > /dev/null
"#,
            pid = self.pid_file().display(),
            args = self.root.join("fake.args").display(),
            env = self.root.join("fake.env").display(),
            codex_home = self.codex_home.display(),
        );
        let path = self.root.join("fake-codex.sh");
        std::fs::write(&path, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn success_scenario(&self) -> String {
        format!(
            r#"mkdir -p '{dir}'
printf '%s\n' '{{"type":"session_meta","payload":{{"id":"{thread}","cwd":"{cwd}"}}}}' > '{rollout}'
printf '%s\n' '{{"id":2,"result":{{"importId":"imp-1"}}}}'
printf '%s\n' '{{"method":"remoteControl/status/changed","params":{{}}}}'
printf '%s\n' '{{"method":"externalAgentConfig/import/progress","params":{{"importId":"imp-1","itemTypeResults":[{{"itemType":"SESSIONS","successes":[],"failures":[]}}]}}}}'
printf '%s\n' '{{"method":"externalAgentConfig/import/progress","params":{{"importId":"imp-1","itemTypeResults":[{{"itemType":"SESSIONS","successes":[{{"itemType":"SESSIONS","source":"{source}","target":"{thread}"}}],"failures":[]}}]}}}}'
printf '%s\n' '{{"method":"externalAgentConfig/import/completed","params":{{"importId":"imp-1","itemTypeResults":[{{"itemType":"SESSIONS","successes":[{{"itemType":"SESSIONS","cwd":"{cwd}","source":"{source}","target":"{thread}"}}],"failures":[]}}]}}}}'"#,
            dir = self.rollout_path().parent().unwrap().display(),
            rollout = self.rollout_path().display(),
            thread = THREAD_ID,
            cwd = self.project.display(),
            source = self.transcript.display(),
        )
    }
}

struct Run {
    result: Result<crate::ConversionOutcome, ConvertError>,
    events: Vec<ConversionEvent>,
}

fn run(request: &ConversionRequest, cancel: &AtomicBool, timeouts: Timeouts) -> Run {
    let mut events = Vec::new();
    let result = {
        let mut sink = |event| events.push(event);
        let mut reporter = Reporter::new(&mut sink, cancel, 6);
        convert(request, &mut reporter, &timeouts)
    };
    Run { result, events }
}

fn quick_timeouts() -> Timeouts {
    Timeouts {
        handshake: Duration::from_secs(10),
        import: Duration::from_secs(10),
        rollout_attempts: 3,
    }
}

fn logs(events: &[ConversionEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            ConversionEvent::Log(line) => Some(line.as_str()),
            _ => None,
        })
        .collect()
}

fn assert_child_gone(fixture: &Fixture) {
    let pid: u32 = std::fs::read_to_string(fixture.pid_file())
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // The fake is a direct child that was killed and reaped: its /proc entry
    // must be gone (no zombie, no survivor).
    if Path::new("/proc/self").exists() {
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "fake codex {pid} is still alive"
        );
    }
}

#[test]
fn a_successful_import_reports_the_thread_and_cleans_up_the_child() {
    let fixture = Fixture::new();
    let executable = fixture.fake_codex(&fixture.success_scenario());
    let cancel = AtomicBool::new(false);
    let run = run(&fixture.request(&executable), &cancel, quick_timeouts());
    let outcome = run.result.expect("import succeeds");
    assert_eq!(outcome.new_session_id, THREAD_ID);
    assert_eq!(outcome.target_transcript_path, fixture.rollout_path());
    assert_eq!(outcome.converted_items, 2);
    assert_eq!(outcome.dropped_items, 1);

    // Launched as a private stdio server, with the intended environment.
    let args = std::fs::read_to_string(fixture.root.join("fake.args")).unwrap();
    assert_eq!(args.trim(), "app-server --listen stdio://");
    let environment = std::fs::read_to_string(fixture.root.join("fake.env")).unwrap();
    assert_eq!(
        environment.trim(),
        format!(
            "{}|{}",
            fixture.codex_home.display(),
            fixture.home.display()
        )
    );
    assert_child_gone(&fixture);

    // Steps 1..6 in order; progress monotonic and complete.
    let steps: Vec<usize> = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversionEvent::Step { index, .. } => Some(*index),
            _ => None,
        })
        .collect();
    assert_eq!(steps, vec![1, 2, 3, 4, 5, 6]);
    let progress: Vec<f32> = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversionEvent::Progress(value) => Some(*value),
            _ => None,
        })
        .collect();
    assert!(
        progress.windows(2).all(|pair| pair[0] <= pair[1]),
        "{progress:?}"
    );
    assert_eq!(progress.last(), Some(&1.0));

    // Stderr chatter, non-JSON output and a server request were all tolerated
    // and surfaced as log lines.
    let lines = logs(&run.events);
    assert!(lines
        .iter()
        .any(|line| line.contains("fake stderr chatter")));
    assert!(lines.iter().any(|line| line.contains("not JSON")));
    assert!(lines
        .iter()
        .any(|line| line.contains("item/tool/requestUserInput")));
    assert!(lines
        .iter()
        .any(|line| line.contains("Import progress: 1 imported")));
    assert!(lines
        .iter()
        .any(|line| line.contains("Rollout cwd matches")));
}

#[test]
fn importer_failures_become_a_one_line_error() {
    let fixture = Fixture::new();
    let executable = fixture.fake_codex(
        r#"printf '%s\n' '{"id":2,"result":{"importId":"imp-1"}}'
printf '%s\n' '{"method":"externalAgentConfig/import/completed","params":{"importId":"imp-1","itemTypeResults":[{"itemType":"SESSIONS","successes":[],"failures":[{"itemType":"SESSIONS","failureStage":"parse","message":"bad\nline"}]}]}}'"#,
    );
    let cancel = AtomicBool::new(false);
    let error = run(&fixture.request(&executable), &cancel, quick_timeouts())
        .result
        .unwrap_err();
    assert!(matches!(error, ConvertError::ImportFailed(_)), "{error}");
    assert_eq!(
        error.to_string(),
        "the Codex importer failed: parse: bad line"
    );
    assert_child_gone(&fixture);
}

#[test]
fn a_rejected_import_request_is_an_import_failure() {
    let fixture = Fixture::new();
    let executable = fixture.fake_codex(
        r#"printf '%s\n' '{"id":2,"error":{"code":-32600,"message":"Invalid request: missing field"}}'"#,
    );
    let cancel = AtomicBool::new(false);
    let error = run(&fixture.request(&executable), &cancel, quick_timeouts())
        .result
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "the Codex importer failed: Invalid request: missing field"
    );
    assert_child_gone(&fixture);
}

#[test]
fn an_unchanged_transcript_reuses_the_earlier_import() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.rollout_path().parent().unwrap()).unwrap();
    std::fs::write(
        fixture.rollout_path(),
        json!({"type":"session_meta","payload":{"id":THREAD_ID,"cwd":fixture.project}}).to_string(),
    )
    .unwrap();
    std::fs::write(
        fixture
            .codex_home
            .join("external_agent_session_imports.json"),
        json!({"records":[{"source_path": fixture.transcript, "imported_thread_id": THREAD_ID}]})
            .to_string(),
    )
    .unwrap();
    let executable = fixture.fake_codex(
        r#"printf '%s\n' '{"id":2,"result":{"importId":"imp-2"}}'
printf '%s\n' '{"method":"externalAgentConfig/import/completed","params":{"importId":"imp-2","itemTypeResults":[{"itemType":"SESSIONS","successes":[],"failures":[]}]}}'"#,
    );
    let cancel = AtomicBool::new(false);
    let run = run(&fixture.request(&executable), &cancel, quick_timeouts());
    assert_eq!(run.result.unwrap().new_session_id, THREAD_ID);
    assert!(logs(&run.events)
        .iter()
        .any(|line| line.contains("already imported unchanged")));
}

#[test]
fn a_skipped_import_without_a_ledger_record_is_an_error() {
    let fixture = Fixture::new();
    let executable = fixture.fake_codex(
        r#"printf '%s\n' '{"id":2,"result":{"importId":"imp-2"}}'
printf '%s\n' '{"method":"externalAgentConfig/import/completed","params":{"importId":"imp-2","itemTypeResults":[{"itemType":"SESSIONS","successes":[],"failures":[]}]}}'"#,
    );
    let cancel = AtomicBool::new(false);
    let error = run(&fixture.request(&executable), &cancel, quick_timeouts())
        .result
        .unwrap_err();
    assert!(matches!(error, ConvertError::ImportSkipped(_)), "{error}");
}

#[test]
fn a_missing_rollout_after_success_is_reported() {
    let fixture = Fixture::new();
    let executable = fixture.fake_codex(
        r#"printf '%s\n' '{"id":2,"result":{"importId":"imp-1"}}'
printf '%s\n' '{"method":"externalAgentConfig/import/completed","params":{"importId":"imp-1","itemTypeResults":[{"itemType":"SESSIONS","successes":[{"itemType":"SESSIONS","source":"x","target":"01a0f39a-7c0e-7722-97d0-06365c66f201"}],"failures":[]}]}}'"#,
    );
    let cancel = AtomicBool::new(false);
    let timeouts = Timeouts {
        rollout_attempts: 2,
        ..quick_timeouts()
    };
    let error = run(&fixture.request(&executable), &cancel, timeouts)
        .result
        .unwrap_err();
    assert!(
        error.to_string().contains("no rollout file for thread"),
        "{error}"
    );
}

#[test]
fn a_silent_importer_times_out_and_the_child_is_killed() {
    let fixture = Fixture::new();
    let executable = fixture.fake_codex("sleep 30");
    let cancel = AtomicBool::new(false);
    let timeouts = Timeouts {
        import: Duration::from_millis(600),
        ..quick_timeouts()
    };
    let started = Instant::now();
    let error = run(&fixture.request(&executable), &cancel, timeouts)
        .result
        .unwrap_err();
    assert!(
        matches!(
            error,
            ConvertError::CodexTimeout {
                stage: "import",
                ..
            }
        ),
        "{error}"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_child_gone(&fixture);
}

#[test]
fn cancelling_while_waiting_kills_the_child_promptly() {
    let fixture = Fixture::new();
    let executable = fixture.fake_codex("sleep 30");
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let mut events = Vec::new();
    let error = {
        let request = fixture.request(&executable);
        let timeouts = quick_timeouts();
        let mut sink = |event: ConversionEvent| {
            // Cancel as soon as the import request has been sent.
            if matches!(&event, ConversionEvent::Log(line) if line == "Import requested") {
                cancel.store(true, Ordering::Relaxed);
            }
            events.push(event);
        };
        let mut reporter = Reporter::new(&mut sink, &cancel, 6);
        convert(&request, &mut reporter, &timeouts).unwrap_err()
    };
    assert!(matches!(error, ConvertError::Cancelled), "{error}");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_child_gone(&fixture);
    assert!(!fixture.rollout_path().exists());
}

#[test]
fn a_server_that_exits_early_is_reported_with_its_stderr() {
    let fixture = Fixture::new();
    let executable = fixture.fake_codex("echo 'fatal: database locked' >&2\nexit 3");
    let cancel = AtomicBool::new(false);
    let error = run(&fixture.request(&executable), &cancel, quick_timeouts())
        .result
        .unwrap_err();
    assert!(matches!(error, ConvertError::CodexExited(_)), "{error}");
    assert!(error.to_string().contains("database locked"), "{error}");
    assert!(!error.to_string().contains('\n'));
}

#[test]
fn a_missing_executable_is_reported_clearly() {
    let fixture = Fixture::new();
    let cancel = AtomicBool::new(false);
    let error = run(
        &fixture.request(&fixture.root.join("no-such-codex")),
        &cancel,
        quick_timeouts(),
    )
    .result
    .unwrap_err();
    assert!(
        matches!(error, ConvertError::CodexUnavailable { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("no-such-codex"));
}

#[test]
fn a_transcript_of_another_project_is_not_imported() {
    let fixture = Fixture::new();
    let executable = fixture.fake_codex("exit 0");
    let mut request = fixture.request(&executable);
    request.project_cwd = fixture.root.join("elsewhere");
    std::fs::create_dir_all(&request.project_cwd).unwrap();
    let cancel = AtomicBool::new(false);
    let error = run(&request, &cancel, quick_timeouts()).result.unwrap_err();
    assert!(
        matches!(error, ConvertError::SourceNotFound { .. }),
        "{error}"
    );
    assert!(
        !fixture.pid_file().exists(),
        "codex must not start for a missing source"
    );
}

#[test]
fn cancellation_at_inspection_step_precedes_transcript_reads() {
    // Synthetic owned transcript only. Its path is verified at step one.
    // Invalidating the file at step two distinguishes a read from a cancel
    // checkpoint without depending on elapsed time or allocator behaviour.
    let fixture = Fixture::new();
    let request = fixture.request(&fixture.root.join("must-not-spawn-codex"));
    let cancel = AtomicBool::new(false);
    let mut reached_inspection = false;
    let error = {
        let mut sink = |event: ConversionEvent| {
            if matches!(event, ConversionEvent::Step { index: 2, .. }) {
                reached_inspection = true;
                std::fs::write(&fixture.transcript, [0xff]).unwrap();
                cancel.store(true, Ordering::Relaxed);
            }
        };
        let mut reporter = Reporter::new(&mut sink, &cancel, 6);
        convert(&request, &mut reporter, &quick_timeouts()).unwrap_err()
    };
    assert!(
        reached_inspection,
        "fixture must reach the actual inspection step"
    );
    assert!(
        matches!(error, ConvertError::Cancelled),
        "cancellation must settle before inspection reads, got: {error}"
    );
    assert!(!fixture.pid_file().exists(), "no importer was spawned");
    assert!(
        !fixture.rollout_path().exists(),
        "no target transcript was created"
    );
}
