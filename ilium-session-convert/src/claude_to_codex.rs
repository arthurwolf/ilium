//! Claude Code -> Codex: hands the source transcript to Codex's own importer
//! (`externalAgentConfig/import` with a `SESSIONS` item) over a private
//! `codex app-server` and reports the thread it created.
//!
//! Observed protocol (codex-cli 0.159): after `initialize`/`initialized`, the
//! import request answers `{importId}` immediately; the work then reports via
//! `externalAgentConfig/import/progress` and ends with
//! `externalAgentConfig/import/completed`, whose `itemTypeResults[].successes[]`
//! carry `{source, cwd, target}` where `target` is the new thread id. The
//! importer deduplicates by transcript path plus content hash, so re-importing
//! an unchanged transcript completes with neither successes nor failures; the
//! thread recorded in `external_agent_session_imports.json` is then reused.

use std::ffi::OsString;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_agent_session::TranscriptLocator;
use ilium_core::AgentClass;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app_server::{response_for, AppServer, AppServerLaunch, Flow};
use crate::bounded_lines::{BoundedLine, BoundedLineReader};
use crate::codex_rollout::ScanLimits;
use crate::error::{single_line, ConvertError};
use crate::lenient_json::{LenientObject, LenientSeq, LenientString};
use crate::paths::{canonical_or_original, locate_codex_rollout, resolve_codex_home};
use crate::report::Reporter;
use crate::{ConversionOutcome, ConversionRequest};

pub(crate) const TOTAL_STEPS: usize = 6;
const ROLLOUT_APPEARANCE_INTERVAL: Duration = Duration::from_millis(200);
/// How often the transcript inspection reports progress and polls
/// cancellation, in lines (the same cadence as the Codex rollout scanner).
const PROGRESS_INTERVAL_LINES: usize = 500;
/// Bounds on every wait; tests shorten them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timeouts {
    pub(crate) handshake: Duration,
    pub(crate) import: Duration,
    pub(crate) rollout_attempts: usize,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            handshake: Duration::from_secs(60),
            import: Duration::from_secs(120),
            rollout_attempts: 25,
        }
    }
}

/// Identifies this client in `initialize` and in the importer's history.
const CLIENT_NAME: &str = "ilium-session-convert";

pub(crate) fn convert(
    request: &ConversionRequest,
    reporter: &mut Reporter<'_>,
    timeouts: &Timeouts,
) -> Result<ConversionOutcome, ConvertError> {
    let codex_home = resolve_codex_home(&request.home_dir, request.codex_home.as_deref());
    let project = canonical_or_original(&request.project_cwd);

    // Step 1: locate the Claude transcript.
    reporter.step(1, "Locate the Claude Code transcript", 0.0);
    let locator = TranscriptLocator::new(&request.home_dir, &project);
    let transcript = locator
        .transcript_for_session(&AgentClass::Claude, &request.source_session_id)
        .ok_or_else(|| ConvertError::SourceNotFound {
            agent: "Claude Code",
            session_id: request.source_session_id.clone(),
            project: project.clone(),
        })?;
    reporter.log(format!("Found {}", transcript.path.display()));
    reporter.progress(0.05);
    reporter.check_cancel()?;

    // Step 2: inspect it, for the counts reported at the end.
    reporter.step(2, "Inspect the transcript", 0.05);
    // Progress callbacks may cancel synchronously; settle before opening the file.
    reporter.check_cancel()?;
    let counts = {
        // The inspection owns 5% of the bar (0.05 to 0.10), by bytes read.
        let mut on_progress = |fraction: f32| {
            reporter.progress(0.05 + 0.05 * fraction);
            !reporter.is_cancelled()
        };
        inspect_transcript(&transcript.path, ScanLimits::default(), &mut on_progress)?
    };
    reporter.log(format!(
        "{} user/assistant lines, {} other bookkeeping lines",
        counts.messages, counts.other
    ));
    if counts.oversized > 0 {
        reporter.log(format!(
            "{} lines longer than {} MiB were skipped unread",
            counts.oversized,
            crate::MAX_LINE_BYTES / (1024 * 1024)
        ));
    }
    reporter.progress(0.10);
    reporter.check_cancel()?;

    // Step 3: start the private app-server and complete the handshake.
    reporter.step(3, "Start codex app-server", 0.10);
    if request.codex_home.is_some() {
        // codex refuses a CODEX_HOME that does not exist yet.
        std::fs::create_dir_all(&codex_home).map_err(|error| ConvertError::TargetWrite {
            path: codex_home.clone(),
            error,
        })?;
    }
    let launch = build_launch(request);
    reporter.log(format!(
        "Starting `{} app-server --listen stdio://` with CODEX_HOME {}",
        launch.executable.to_string_lossy(),
        codex_home.display()
    ));
    let mut server = AppServer::spawn(&launch)?;
    let context = ImportContext {
        request,
        project: &project,
        codex_home: &codex_home,
        transcript_path: &transcript.path,
        counts: &counts,
        timeouts,
    };
    let result = import_with_server(&mut server, &context, reporter);
    // Step 6 (shutdown) runs on every path; errors above must not skip it.
    reporter.step(6, "Stop codex app-server", 0.95);
    server.shutdown();
    reporter.log("codex app-server stopped");
    if matches!(result, Err(ConvertError::Cancelled)) {
        reporter.log("Cancelled: codex app-server terminated; a thread Codex had already created stays in Codex's own store");
    }
    let outcome = result?;
    reporter.progress(1.0);
    Ok(outcome)
}

fn build_launch(request: &ConversionRequest) -> AppServerLaunch {
    let executable = request
        .codex_executable
        .as_ref()
        .map(|path| path.as_os_str().to_os_string())
        .unwrap_or_else(|| OsString::from("codex"));
    // The importer finds Claude transcripts under $HOME/.claude, so the child
    // sees the same home the transcript was located in. In production that is
    // the user's real home and this is a no-op.
    let mut environment = vec![
        (
            OsString::from("HOME"),
            request.home_dir.clone().into_os_string(),
        ),
        (
            OsString::from("USERPROFILE"),
            request.home_dir.clone().into_os_string(),
        ),
    ];
    if let Some(codex_home) = &request.codex_home {
        environment.push((
            OsString::from("CODEX_HOME"),
            codex_home.clone().into_os_string(),
        ));
    }
    AppServerLaunch {
        executable,
        environment,
    }
}

/// What the inspection step learned about the Claude transcript; it only
/// feeds the log and the final report, since Codex's importer re-reads the
/// file itself.
#[derive(Debug, Default, PartialEq, Eq)]
struct TranscriptCounts {
    /// Lines whose `type` is `user` or `assistant`.
    messages: usize,
    /// Every other non-blank line: bookkeeping records, unparseable lines
    /// and the oversized ones.
    other: usize,
    /// Lines over the single-line cap, skipped unread; included in `other`.
    oversized: usize,
}

/// The one field the inspection reads from a transcript line. Serde skips
/// every other field without allocating it, so a line costs its own bytes
/// and nothing more; a non-string `type` reads as absent, as before.
#[derive(Deserialize)]
struct TranscriptLineHeader {
    #[serde(rename = "type", default)]
    kind: LenientString,
}

enum TranscriptLine {
    Blank,
    Message,
    Other,
    Oversized,
}

fn classify_transcript_line(bytes: &[u8]) -> TranscriptLine {
    // Lossy decoding keeps a line with a stray invalid byte countable
    // instead of failing the whole inspection; the importer decides later
    // what it accepts.
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return TranscriptLine::Blank;
    }
    match serde_json::from_str::<TranscriptLineHeader>(trimmed) {
        Ok(header) if matches!(header.kind.as_deref(), Some("user" | "assistant")) => {
            TranscriptLine::Message
        }
        _ => TranscriptLine::Other,
    }
}

/// Counts the conversation lines of the transcript one bounded line at a
/// time: memory is proportional to the longest line, never to the file.
/// `on_progress` receives the fraction of bytes read every few hundred lines
/// and returns `false` to cancel.
fn inspect_transcript(
    path: &Path,
    limits: ScanLimits,
    on_progress: &mut dyn FnMut(f32) -> bool,
) -> Result<TranscriptCounts, ConvertError> {
    let unreadable = |error| ConvertError::SourceUnreadable {
        path: path.to_path_buf(),
        error,
    };
    let file = ilium_platform::secure_fs::open_regular_file(path).map_err(unreadable)?;
    let length = file.metadata().map_err(unreadable)?.len();
    if length > limits.source_bytes {
        return Err(ConvertError::FileTooLarge {
            path: path.to_path_buf(),
            bytes: length,
            maximum: limits.source_bytes,
        });
    }
    let mut lines = BoundedLineReader::new(BufReader::new(file), limits.line_bytes);
    let mut counts = TranscriptCounts::default();
    let mut line_count = 0usize;
    loop {
        let classified = match lines.next_line().map_err(unreadable)? {
            None => break,
            Some(BoundedLine::Oversized { .. }) => TranscriptLine::Oversized,
            Some(BoundedLine::Line { bytes, .. }) => classify_transcript_line(bytes),
        };
        line_count += 1;
        let bytes_read = lines.offset();
        if bytes_read > limits.source_bytes {
            // The file grew past the cap while being read.
            return Err(ConvertError::FileTooLarge {
                path: path.to_path_buf(),
                bytes: bytes_read,
                maximum: limits.source_bytes,
            });
        }
        if line_count.is_multiple_of(PROGRESS_INTERVAL_LINES) {
            let fraction = if length == 0 {
                0.0
            } else {
                (bytes_read as f64 / length as f64) as f32
            };
            if !on_progress(fraction) {
                return Err(ConvertError::Cancelled);
            }
        }
        match classified {
            TranscriptLine::Blank => {}
            TranscriptLine::Message => counts.messages += 1,
            TranscriptLine::Other => counts.other += 1,
            TranscriptLine::Oversized => {
                counts.other += 1;
                counts.oversized += 1;
            }
        }
    }
    if counts.messages == 0 {
        return Err(ConvertError::EmptyConversation {
            path: path.to_path_buf(),
        });
    }
    Ok(counts)
}

/// Everything the import steps share, so each function takes one handle.
struct ImportContext<'a> {
    request: &'a ConversionRequest,
    project: &'a Path,
    codex_home: &'a Path,
    transcript_path: &'a Path,
    counts: &'a TranscriptCounts,
    timeouts: &'a Timeouts,
}

fn import_with_server(
    server: &mut AppServer,
    context: &ImportContext<'_>,
    reporter: &mut Reporter<'_>,
) -> Result<ConversionOutcome, ConvertError> {
    let ImportContext {
        request,
        project,
        codex_home,
        transcript_path,
        counts,
        timeouts,
    } = *context;
    handshake(server, codex_home, timeouts, reporter)?;

    // Step 4: run the importer.
    reporter.step(4, "Import the session with Codex's importer", 0.30);
    let imported = run_import(server, project, transcript_path, timeouts, reporter)?;
    reporter.progress(0.85);

    let thread_id = match imported {
        ImportResult::Created { thread_id } => {
            reporter.log(format!("Codex created thread {thread_id}"));
            thread_id
        }
        ImportResult::Skipped => reuse_previous_import(codex_home, transcript_path, reporter)?,
    };

    // Step 5: verify the rollout Codex wrote.
    reporter.step(5, "Verify the Codex rollout", 0.85);
    let rollout = wait_for_rollout(request, project, codex_home, &thread_id, timeouts, reporter)?;
    reporter.log(format!("Rollout: {}", rollout.display()));
    reporter.progress(0.95);
    Ok(ConversionOutcome {
        new_session_id: thread_id,
        target_transcript_path: rollout,
        converted_items: counts.messages,
        dropped_items: counts.other,
    })
}

fn handshake(
    server: &mut AppServer,
    codex_home: &Path,
    timeouts: &Timeouts,
    reporter: &mut Reporter<'_>,
) -> Result<(), ConvertError> {
    let id = server.send_request(
        "initialize",
        json!({
            "clientInfo": {"name": CLIENT_NAME, "version": env!("CARGO_PKG_VERSION")},
            "capabilities": {"experimentalApi": true},
        }),
    )?;
    let mut reported_home: Option<String> = None;
    let mut failure: Option<String> = None;
    server.wait_for(
        "initialize",
        timeouts.handshake,
        reporter,
        &mut |message, reporter| {
            let Some(response) = response_for(message, id) else {
                return Ok(Flow::Continue);
            };
            match response {
                Ok(result) => {
                    reporter.log(format!(
                        "Connected to {}",
                        result
                            .get("userAgent")
                            .and_then(Value::as_str)
                            .unwrap_or("codex app-server")
                    ));
                    reported_home = result
                        .get("codexHome")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                Err(text) => failure = Some(text),
            }
            Ok(Flow::Done)
        },
    )?;
    if let Some(text) = failure {
        return Err(ConvertError::CodexProtocol(format!(
            "initialize failed: {text}"
        )));
    }
    if let Some(reported) = reported_home {
        if canonical_or_original(Path::new(&reported)) != canonical_or_original(codex_home) {
            reporter.log(format!(
                "Warning: app-server uses CODEX_HOME {reported}, expected {}",
                codex_home.display()
            ));
        }
    }
    server.notify("initialized")?;
    reporter.progress(0.30);
    Ok(())
}

enum ImportResult {
    Created {
        thread_id: String,
    },
    /// Completed with no success and no failure: the importer already holds
    /// this transcript at its current content.
    Skipped,
}

fn run_import(
    server: &mut AppServer,
    project: &Path,
    transcript_path: &Path,
    timeouts: &Timeouts,
    reporter: &mut Reporter<'_>,
) -> Result<ImportResult, ConvertError> {
    let session = json!({
        "cwd": project.to_string_lossy(),
        "path": transcript_path.to_string_lossy(),
        "title": Value::Null,
    });
    let id = server.send_request(
        "externalAgentConfig/import",
        json!({
            "migrationItems": [{
                "itemType": "SESSIONS",
                "description": "Convert one Claude Code session for Codex",
                "cwd": Value::Null,
                "details": {"sessions": [session]},
            }],
            "source": CLIENT_NAME,
        }),
    )?;
    reporter.log("Import requested");

    let mut completed: Option<Value> = None;
    let mut rejection: Option<String> = None;
    server.wait_for(
        "import",
        timeouts.import,
        reporter,
        &mut |message, reporter| {
            if let Some(response) = response_for(message, id) {
                match response {
                    Ok(result) => {
                        let import_id = result.get("importId").and_then(Value::as_str);
                        reporter.log(format!("Import accepted (id {})", import_id.unwrap_or("?")));
                        reporter.progress(0.40);
                    }
                    Err(text) => {
                        rejection = Some(text);
                        return Ok(Flow::Done);
                    }
                }
                return Ok(Flow::Continue);
            }
            match message.get("method").and_then(Value::as_str) {
                Some("externalAgentConfig/import/progress") => {
                    let (successes, failures) = session_counts(message.get("params"));
                    reporter.log(format!(
                        "Import progress: {successes} imported, {failures} failed"
                    ));
                    reporter.progress(0.40 + 0.35 * (successes + failures).min(1) as f32);
                    Ok(Flow::Continue)
                }
                Some("externalAgentConfig/import/completed") => {
                    completed = message.get("params").cloned();
                    Ok(Flow::Done)
                }
                Some(other) => {
                    reporter.log(format!("codex notification: {other}"));
                    Ok(Flow::Continue)
                }
                None => Ok(Flow::Continue),
            }
        },
    )?;
    if let Some(text) = rejection {
        return Err(ConvertError::ImportFailed(single_line(&text, 300)));
    }
    let Some(completed) = completed else {
        return Err(ConvertError::CodexProtocol(
            "the import finished without a completion notification".to_string(),
        ));
    };
    interpret_completion(&completed, transcript_path)
}

/// `(successes, failures)` of the SESSIONS results in a notification's params.
fn session_counts(params: Option<&Value>) -> (usize, usize) {
    let Some(results) = params
        .and_then(|params| params.get("itemTypeResults"))
        .and_then(Value::as_array)
    else {
        return (0, 0);
    };
    results
        .iter()
        .filter(|result| result.get("itemType").and_then(Value::as_str) == Some("SESSIONS"))
        .fold((0, 0), |(successes, failures), result| {
            let count = |key: &str| {
                result
                    .get(key)
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
            };
            (successes + count("successes"), failures + count("failures"))
        })
}

fn interpret_completion(
    completed: &Value,
    transcript_path: &Path,
) -> Result<ImportResult, ConvertError> {
    let results = completed
        .get("itemTypeResults")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let sessions: Vec<&Value> = results
        .iter()
        .filter(|result| result.get("itemType").and_then(Value::as_str) == Some("SESSIONS"))
        .collect();
    let failures: Vec<String> = sessions
        .iter()
        .flat_map(|result| {
            result
                .get("failures")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .map(|failure| {
            let stage = failure
                .get("failureStage")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let message = failure
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("no message");
            format!("{stage}: {message}")
        })
        .collect();
    if !failures.is_empty() {
        return Err(ConvertError::ImportFailed(single_line(
            &failures.join("; "),
            400,
        )));
    }
    let successes: Vec<&Value> = sessions
        .iter()
        .flat_map(|result| {
            result
                .get("successes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .collect();
    let expected = canonical_or_original(transcript_path);
    let matching = successes.iter().find(|success| {
        success
            .get("source")
            .and_then(Value::as_str)
            .is_some_and(|source| canonical_or_original(Path::new(source)) == expected)
    });
    // A connection this client owns imports one session; accept a lone
    // success even if the importer spelled the source path differently.
    let chosen = matching.or(if successes.len() == 1 {
        successes.first()
    } else {
        None
    });
    match chosen.and_then(|success| success.get("target").and_then(Value::as_str)) {
        Some(thread_id) if !thread_id.is_empty() => Ok(ImportResult::Created {
            thread_id: thread_id.to_string(),
        }),
        _ if successes.is_empty() => Ok(ImportResult::Skipped),
        _ => Err(ConvertError::ImportFailed(
            "the importer reported success without a thread id".to_string(),
        )),
    }
}

/// The importer skipped an unchanged transcript; its ledger names the thread
/// it created earlier.
fn reuse_previous_import(
    codex_home: &Path,
    transcript_path: &Path,
    reporter: &mut Reporter<'_>,
) -> Result<String, ConvertError> {
    reporter.log(
        "The importer imported nothing new; looking for the earlier import of this transcript",
    );
    let ledger_path = codex_home.join("external_agent_session_imports.json");
    let thread_id = recorded_thread_for(&ledger_path, transcript_path)?.ok_or_else(|| {
        ConvertError::ImportSkipped(format!(
            "it was already handled but {} has no record of it",
            ledger_path.display()
        ))
    })?;
    reporter.log(format!(
        "This transcript was already imported unchanged as thread {thread_id}; reusing it"
    ));
    Ok(thread_id)
}

/// The two fields of each `external_agent_session_imports.json` record the
/// lookup needs. Any field of the wrong shape reads as absent and a record
/// that is not an object reads as empty, matching the earlier `Value`
/// lookups, while the rest of the document is skipped unallocated.
#[derive(Deserialize)]
struct ImportLedger {
    #[serde(default)]
    records: LenientSeq<LenientObject<ImportLedgerRecord>>,
}

#[derive(Deserialize, Default)]
struct ImportLedgerRecord {
    #[serde(default)]
    source_path: LenientString,
    #[serde(default)]
    imported_thread_id: LenientString,
}

/// Thread id Codex recorded for `transcript_path` in its import ledger.
pub(crate) fn recorded_thread_for(
    ledger_path: &Path,
    transcript_path: &Path,
) -> Result<Option<String>, ConvertError> {
    let file = match ilium_platform::secure_fs::open_regular_file(ledger_path) {
        Ok(file) => file,
        Err(_) => return Ok(None),
    };
    let size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    if size > crate::MAX_IMPORT_LEDGER_BYTES {
        return Err(ConvertError::FileTooLarge {
            path: ledger_path.to_path_buf(),
            bytes: size,
            maximum: crate::MAX_IMPORT_LEDGER_BYTES,
        });
    }
    // The ledger is one JSON document, so it is parsed straight from the
    // file into the typed records above; the `take` keeps a file that grows
    // past the cap while being read from being read any further.
    let mut reader = BufReader::new(file.take(crate::MAX_IMPORT_LEDGER_BYTES + 1));
    let parsed = serde_json::from_reader::<_, ImportLedger>(&mut reader);
    if reader.get_ref().limit() == 0 {
        return Err(ConvertError::FileTooLarge {
            path: ledger_path.to_path_buf(),
            bytes: crate::MAX_IMPORT_LEDGER_BYTES + 1,
            maximum: crate::MAX_IMPORT_LEDGER_BYTES,
        });
    }
    let Ok(ledger) = parsed else {
        // An unreadable or malformed ledger means "no earlier import found".
        return Ok(None);
    };
    let expected = canonical_or_original(transcript_path);
    // Later records win: a changed transcript is re-imported and re-recorded.
    let thread_id = ledger
        .records
        .0
        .iter()
        .rev()
        .find(|record| {
            record
                .0
                .source_path
                .as_deref()
                .is_some_and(|source| canonical_or_original(Path::new(source)) == expected)
        })
        .and_then(|record| record.0.imported_thread_id.as_deref())
        .map(str::to_string);
    Ok(thread_id)
}

fn wait_for_rollout(
    request: &ConversionRequest,
    project: &Path,
    codex_home: &Path,
    thread_id: &str,
    timeouts: &Timeouts,
    reporter: &mut Reporter<'_>,
) -> Result<PathBuf, ConvertError> {
    for attempt in 0..timeouts.rollout_attempts {
        reporter.check_cancel()?;
        if let Some(rollout) =
            locate_codex_rollout(&request.home_dir, project, codex_home, thread_id)
        {
            match (&rollout.recorded_cwd, rollout.cwd_matches) {
                (Some(recorded), false) => reporter.log(format!(
                    "Warning: the importer recorded cwd {recorded}, not the project {}",
                    project.display()
                )),
                (None, _) => reporter.log("Warning: the rollout has no readable cwd"),
                _ => reporter.log("Rollout cwd matches the project"),
            }
            return Ok(rollout.path);
        }
        if attempt + 1 < timeouts.rollout_attempts {
            std::thread::sleep(ROLLOUT_APPEARANCE_INTERVAL);
        }
    }
    Err(ConvertError::ImportFailed(format!(
        "no rollout file for thread {thread_id} appeared under {}",
        codex_home.join("sessions").display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_with_a_matching_success_yields_its_thread() {
        let completed = json!({"itemTypeResults": [{"itemType": "SESSIONS",
            "successes": [{"itemType": "SESSIONS", "source": "/a/b.jsonl", "target": "thread-1"}],
            "failures": []}]});
        let result = interpret_completion(&completed, Path::new("/a/b.jsonl")).unwrap();
        assert!(matches!(result, ImportResult::Created { thread_id } if thread_id == "thread-1"));
    }

    #[test]
    fn completion_without_successes_or_failures_is_a_skip() {
        let completed = json!({"itemTypeResults": [{"itemType": "SESSIONS",
            "successes": [], "failures": []}]});
        assert!(matches!(
            interpret_completion(&completed, Path::new("/a/b.jsonl")).unwrap(),
            ImportResult::Skipped
        ));
    }

    #[test]
    fn completion_failures_become_an_import_error() {
        let completed = json!({"itemTypeResults": [{"itemType": "SESSIONS",
            "successes": [],
            "failures": [{"itemType": "SESSIONS", "failureStage": "parse", "message": "bad line"}]}]});
        let error = interpret_completion(&completed, Path::new("/a/b.jsonl"))
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "the Codex importer failed: parse: bad line"
        );
    }

    #[test]
    fn progress_counts_only_session_results() {
        let params = json!({"itemTypeResults": [
            {"itemType": "CONFIG", "successes": [{}], "failures": []},
            {"itemType": "SESSIONS", "successes": [{}, {}], "failures": [{}]}]});
        assert_eq!(session_counts(Some(&params)), (2, 1));
    }

    #[test]
    fn ledger_lookup_prefers_the_latest_record() {
        let directory = tempfile::tempdir().unwrap();
        let ledger = directory.path().join("ledger.json");
        std::fs::write(
            &ledger,
            json!({"records": [
                {"source_path": "/a/b.jsonl", "imported_thread_id": "old"},
                {"source_path": "/a/c.jsonl", "imported_thread_id": "other"},
                {"source_path": "/a/b.jsonl", "imported_thread_id": "new"}]})
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            recorded_thread_for(&ledger, Path::new("/a/b.jsonl"))
                .unwrap()
                .as_deref(),
            Some("new")
        );
        assert_eq!(
            recorded_thread_for(&ledger, Path::new("/a/none.jsonl")).unwrap(),
            None
        );
    }

    #[test]
    fn ledger_lookup_tolerates_records_of_the_wrong_shape() {
        let directory = tempfile::tempdir().unwrap();
        let ledger = directory.path().join("ledger.json");
        std::fs::write(
            &ledger,
            json!({"records": [
                7,
                {"source_path": 5, "imported_thread_id": "not-a-match"},
                {"source_path": "/a/b.jsonl", "imported_thread_id": "kept"},
                {"source_path": "/a/b.jsonl"}]})
            .to_string(),
        )
        .unwrap();
        // The latest matching record wins even when it lacks a thread id.
        assert_eq!(
            recorded_thread_for(&ledger, Path::new("/a/b.jsonl")).unwrap(),
            None
        );
        std::fs::write(
            &ledger,
            json!({"records": [7, {"source_path": "/a/b.jsonl", "imported_thread_id": "kept"}]})
                .to_string(),
        )
        .unwrap();
        assert_eq!(
            recorded_thread_for(&ledger, Path::new("/a/b.jsonl"))
                .unwrap()
                .as_deref(),
            Some("kept")
        );
        for malformed in ["not json at all", r#"{"records": "no"}"#, "[]"] {
            std::fs::write(&ledger, malformed).unwrap();
            assert_eq!(
                recorded_thread_for(&ledger, Path::new("/a/b.jsonl")).unwrap(),
                None,
                "{malformed}"
            );
        }
    }

    /// Lines of a Claude transcript as the inspection sees them.
    fn transcript_body(extra_lines: &[&str]) -> String {
        let mut body = format!(
            "{}\n{}\n{}\n",
            json!({"type": "user", "uuid": "u1", "message": {"role": "user", "content": "hello"}}),
            json!({"type": "assistant", "uuid": "a1", "message": {"role": "assistant", "content": []}}),
            json!({"type": "ai-title", "aiTitle": "t"}),
        );
        for line in extra_lines {
            body.push_str(line);
            body.push('\n');
        }
        body
    }

    #[test]
    fn inspection_counts_messages_and_everything_else_without_a_full_parse() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("transcript.jsonl");
        let mut body = transcript_body(&[
            "",
            "   ",
            "not json",
            r#"{"type": 7}"#,
            r#"[{"type": "user"}]"#,
            r#"{"message": "no type"}"#,
        ])
        .into_bytes();
        // A user line with one invalid UTF-8 byte is still counted as a
        // message instead of failing the whole inspection.
        body.extend_from_slice(b"{\"type\":\"user\",\"message\":\"stray \xff byte\"}\n");
        std::fs::write(&path, body).unwrap();
        let counts = inspect_transcript(&path, ScanLimits::default(), &mut |_| true).unwrap();
        assert_eq!(
            counts,
            TranscriptCounts {
                messages: 3,
                other: 5,
                oversized: 0
            }
        );
    }

    #[test]
    fn inspection_of_a_transcript_without_messages_is_an_empty_conversation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("transcript.jsonl");
        std::fs::write(&path, "{\"type\":\"ai-title\"}\n").unwrap();
        let error = inspect_transcript(&path, ScanLimits::default(), &mut |_| true).unwrap_err();
        assert!(matches!(error, ConvertError::EmptyConversation { .. }));
    }

    #[test]
    fn oversized_claude_transcript_is_rejected_from_metadata_before_reading() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.jsonl");
        let limits = ScanLimits {
            source_bytes: 4096,
            line_bytes: 1024,
        };
        std::fs::File::create(&path)
            .unwrap()
            .set_len(limits.source_bytes + 1)
            .unwrap();
        let error = inspect_transcript(&path, limits, &mut |_| true).unwrap_err();
        assert!(matches!(
            error,
            ConvertError::FileTooLarge { bytes, maximum, .. } if bytes == 4097 && maximum == 4096
        ));
        // Production uses the crate's 64 GiB source sanity bound.
        assert_eq!(
            ScanLimits::default().source_bytes,
            crate::MAX_SOURCE_TRANSCRIPT_BYTES
        );
    }

    #[test]
    fn a_large_sparse_transcript_is_inspected_without_being_read_whole() {
        // 2 GiB + 1 bytes: far larger than any whole-file read could hold. The
        // zero tail is one line over the (injected) line cap, so it is drained
        // chunk by chunk and counted, never allocated.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sparse.jsonl");
        std::fs::write(&path, transcript_body(&[])).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(2 * 1024 * 1024 * 1024 + 1)
            .unwrap();
        let limits = ScanLimits {
            source_bytes: crate::MAX_SOURCE_TRANSCRIPT_BYTES,
            line_bytes: 1024 * 1024,
        };
        let counts = inspect_transcript(&path, limits, &mut |_| true).unwrap();
        assert_eq!(
            counts,
            TranscriptCounts {
                messages: 2,
                other: 2,
                oversized: 1
            }
        );
    }

    #[test]
    fn inspection_reports_progress_and_honours_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("transcript.jsonl");
        let filler: Vec<&str> = std::iter::repeat_n(r#"{"type":"progress"}"#, 1200).collect();
        std::fs::write(&path, transcript_body(&filler)).unwrap();
        let mut fractions = Vec::new();
        let counts = inspect_transcript(&path, ScanLimits::default(), &mut |fraction| {
            fractions.push(fraction);
            true
        })
        .unwrap();
        assert_eq!(counts.messages, 2);
        assert_eq!(counts.other, 1201);
        // One report per 500 lines, each a growing fraction of the bytes read.
        assert_eq!(fractions.len(), 2);
        assert!(fractions[0] > 0.0 && fractions[0] < fractions[1] && fractions[1] <= 1.0);
        let mut calls = 0;
        let error = inspect_transcript(&path, ScanLimits::default(), &mut |_| {
            calls += 1;
            false
        })
        .unwrap_err();
        assert!(matches!(error, ConvertError::Cancelled));
        assert_eq!(calls, 1);
    }

    #[test]
    fn oversized_import_ledger_is_rejected_before_parsing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.json");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(crate::MAX_IMPORT_LEDGER_BYTES + 1)
            .unwrap();
        let error = recorded_thread_for(&path, Path::new("/a/b.jsonl")).unwrap_err();
        assert!(matches!(error, ConvertError::FileTooLarge { .. }));
    }
}
