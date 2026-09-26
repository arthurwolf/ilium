//! `ilium voice say`: type sentences into the running voice session as if
//! they had been spoken.
//!
//! The voice session lives in an attached interactive client (it owns the
//! microphone, the provider connection, and the tool executor), so this
//! one-shot command cannot talk to it directly. It sends
//! `ClientRequest::SubmitVoiceText` to the session's server, which offers the
//! sentences to the client hosting voice and relays that client's answer back
//! as `ServerEvent::VoiceTextResult` (see `ilium-server`'s `voice_relay`).
//! From there each sentence is one user turn in the live model conversation,
//! interpreted and routed to the agent exactly like a recognised utterance --
//! with the microphone still live if voice is listening.
//!
//! Output is JSONL on stdout, one object per line, each with a `type`:
//! `progress` (what is being attempted), then exactly one `result` (the
//! sentences were handed to the voice session) or `error`. The process exits
//! non-zero after an `error` record. Human-readable diagnostics go to stderr.
//! "Accepted" means the text is in the live session's queue -- the provider
//! sends no per-turn acknowledgement -- not that the model has acted on it.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand};
use ilium::session;
use ilium_ipc::{
    normalize_voice_sentences, ClientRequest, ServerEvent, VoiceTextAccepted, VoiceTextPhase,
    VoiceTextRejection, VoiceTextRejectionCode,
};

use crate::{json_string, next_progress_request_id, pane_identity_from_env, CliError};

/// The token that reads sentences from standard input instead.
const STDIN_TOKEN: &str = "-";
/// The command name printed in every record.
const COMMAND_NAME: &str = "voice say";

#[derive(Subcommand, Debug)]
pub(crate) enum VoiceCommand {
    /// Says sentences to the running voice session as if they had been
    /// spoken. Each sentence becomes one turn in the voice model's
    /// conversation, in order; the model interprets it and acts on it (for
    /// example by typing it into the focused agent) exactly as it would for
    /// speech, and text may be mixed freely with live microphone audio.
    ///
    /// The session must live in an attached interactive Ilium client. When
    /// voice control is off the command fails with code `voice-off` unless
    /// `--start` is given, which switches it on (and saves that setting, as
    /// pressing F8 does).
    ///
    /// Run from inside an Ilium pane it addresses that pane's session;
    /// elsewhere it uses `--cwd` and `--session-name`. It never starts a
    /// server. Output is JSONL: `progress`, then one `result` or `error`.
    Say(SayArgs),
}

#[derive(Args, Debug)]
pub(crate) struct SayArgs {
    /// Sentences to say, in order. A lone `-` reads one sentence per
    /// non-empty line from standard input. Put `--` first when a sentence
    /// begins with a hyphen.
    #[arg(value_name = "SENTENCE", required = true)]
    sentences: Vec<String>,

    /// Switch voice control on when it is off (persisted like F8), or restart
    /// a session that failed to start, instead of failing with `voice-off`.
    #[arg(long)]
    start: bool,

    /// Session to address when not run from inside an Ilium pane.
    #[arg(long, default_value = session::DEFAULT_SESSION_NAME)]
    session_name: String,

    /// How long to wait for the voice session to accept the text. Starting
    /// voice (`--start`) opens audio devices and the provider connection
    /// first, so allow more time than for a session that is already running.
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=600))]
    timeout_s: u64,
}

/// A refusal or failure, before it is rendered as an `error` record.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SayFailure {
    code: &'static str,
    message: String,
    hint: Option<&'static str>,
}

impl SayFailure {
    fn new(code: &'static str, message: impl Into<String>, hint: Option<&'static str>) -> Self {
        Self {
            code,
            message: message.into(),
            hint,
        }
    }
}

/// Where the session socket came from; reported so an agent can tell which
/// session it just spoke to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SayTarget {
    socket_path: PathBuf,
    session_name: String,
    source: &'static str,
}

pub(crate) async fn voice(command: VoiceCommand, cwd: &Path) -> Result<(), CliError> {
    match command {
        VoiceCommand::Say(args) => say(args, cwd).await,
    }
}

async fn say(args: SayArgs, cwd: &Path) -> Result<(), CliError> {
    let request_id = next_progress_request_id();
    match run_say(&args, cwd, request_id).await {
        Ok(()) => Ok(()),
        Err(failure) => {
            println!("{}", error_record(request_id, &failure));
            Err(CliError::ServerReportedError(failure.message))
        }
    }
}

async fn run_say(args: &SayArgs, cwd: &Path, request_id: u64) -> Result<(), SayFailure> {
    let sentences = collect_sentences(&args.sentences, &mut std::io::stdin().lock())?;
    let target = resolve_target(&args.session_name, cwd)?;
    println!(
        "{}",
        progress_record(request_id, "sending", &target, sentences.len(), args.start)
    );

    let mut connection = ilium_client::connection::Connection::connect(
        &target.socket_path,
        target.session_name.clone(),
    )
    .await
    .map_err(|error| {
        SayFailure::new(
            "connection-failed",
            error.to_string(),
            Some("the session's server is not reachable; check `ilium ls`"),
        )
    })?;
    let request = ClientRequest::SubmitVoiceText {
        request_id,
        sentences: sentences.clone(),
        start_voice: args.start,
    };
    if connection.requests.send(request).await.is_err() {
        return Err(SayFailure::new(
            "request-send-failed",
            "connection closed before the request was sent",
            None,
        ));
    }

    let timeout = Duration::from_secs(args.timeout_s);
    let outcome = tokio::time::timeout(timeout, async {
        while let Some(event) = connection.events.recv().await {
            match event {
                ServerEvent::VoiceTextResult {
                    request_id: response_id,
                    result,
                } if response_id == request_id => return Some(result),
                // Only the correlated answer matters; the attach handshake
                // and unrelated broadcasts are not this command's business.
                _ => {}
            }
        }
        None
    })
    .await;
    let _ = connection.requests.send(ClientRequest::Detach).await;

    match outcome {
        Ok(Some(Ok(accepted))) => {
            println!(
                "{}",
                result_record(request_id, &target, sentences.len(), &accepted)
            );
            Ok(())
        }
        Ok(Some(Err(rejection))) => Err(rejection_failure(&rejection)),
        Ok(None) => Err(SayFailure::new(
            "server-closed-connection",
            "the server closed the connection before answering",
            Some(STALE_SERVER_HINT),
        )),
        Err(_elapsed) => Err(SayFailure::new(
            "timeout",
            format!(
                "no answer from the voice session within {} s",
                args.timeout_s
            ),
            Some(STALE_SERVER_HINT),
        )),
    }
}

/// A server built before this command existed neither understands the
/// request nor answers it.
const STALE_SERVER_HINT: &str =
    "an Ilium server started before `ilium voice say` existed cannot answer it; restart Ilium to load the current server";

/// Positional sentences in order, with each `-` replaced by the non-empty
/// lines of `stdin`. Every sentence is trimmed and bounds-checked with the
/// same rules the server enforces.
fn collect_sentences(
    positional: &[String],
    stdin: &mut impl BufRead,
) -> Result<Vec<String>, SayFailure> {
    let mut sentences = Vec::new();
    for argument in positional {
        if argument != STDIN_TOKEN {
            sentences.push(argument.clone());
            continue;
        }
        for line in stdin.lines() {
            let line = line.map_err(|error| {
                SayFailure::new(
                    "stdin-unreadable",
                    format!("could not read sentences from standard input: {error}"),
                    None,
                )
            })?;
            if !line.trim().is_empty() {
                sentences.push(line);
            }
        }
    }
    normalize_voice_sentences(sentences).map_err(|rejection| rejection_failure(&rejection))
}

/// The pane's own session when run from inside one (its environment names
/// the exact socket), otherwise the `--cwd` project's named session, which
/// must already be running: this command never starts a server, because a
/// fresh server has no client to host voice.
fn resolve_target(session_name: &str, cwd: &Path) -> Result<SayTarget, SayFailure> {
    if let Ok(identity) = pane_identity_from_env() {
        return Ok(SayTarget {
            socket_path: identity.socket_path,
            session_name: identity.session_name,
            source: "pane-env",
        });
    }
    let project_session = session::resolve_project_session(cwd, session_name)
        .map_err(|error| SayFailure::new("invalid-session", error.to_string(), None))?;
    if !session::is_session_live(&project_session.socket_path) {
        return Err(SayFailure::new(
            "session-not-running",
            format!("session {session_name:?} is not running for this project"),
            Some("start Ilium in this project with `ilium` (voice runs inside its interactive client), or pass --cwd/--session-name"),
        ));
    }
    Ok(SayTarget {
        socket_path: project_session.socket_path,
        session_name: session_name.to_owned(),
        source: "cwd",
    })
}

const fn rejection_code_name(code: VoiceTextRejectionCode) -> &'static str {
    match code {
        VoiceTextRejectionCode::InvalidRequest => "invalid-request",
        VoiceTextRejectionCode::NoVoiceClient => "no-voice-client",
        VoiceTextRejectionCode::VoiceOff => "voice-off",
        VoiceTextRejectionCode::VoiceUnavailable => "voice-unavailable",
        VoiceTextRejectionCode::ClientUnresponsive => "client-unresponsive",
    }
}

fn rejection_failure(rejection: &VoiceTextRejection) -> SayFailure {
    let hint = match rejection.code {
        VoiceTextRejectionCode::VoiceOff => {
            Some("pass --start to switch voice control on, or press F8 in the Ilium client")
        }
        VoiceTextRejectionCode::NoVoiceClient => Some(
            "attach an interactive Ilium client to this session with `ilium`; the voice session runs inside it",
        ),
        VoiceTextRejectionCode::VoiceUnavailable => Some(
            "fix the voice settings in the Ilium client (Settings -> Voice control: API key, audio devices) and try again",
        ),
        VoiceTextRejectionCode::ClientUnresponsive => {
            Some("the attached client did not answer; is it frozen or suspended?")
        }
        VoiceTextRejectionCode::InvalidRequest => None,
    };
    SayFailure::new(
        rejection_code_name(rejection.code),
        rejection.message.clone(),
        hint,
    )
}

const fn phase_name(phase: VoiceTextPhase) -> &'static str {
    match phase {
        VoiceTextPhase::Connecting => "connecting",
        VoiceTextPhase::Listening => "listening",
        VoiceTextPhase::Recording => "recording",
        VoiceTextPhase::Thinking => "thinking",
        VoiceTextPhase::Speaking => "speaking",
    }
}

fn progress_record(
    request_id: u64,
    stage: &str,
    target: &SayTarget,
    sentence_count: usize,
    start: bool,
) -> String {
    format!(
        "{{\"type\":\"progress\",\"command\":{},\"stage\":{},\"request_id\":{request_id},\"session\":{},\"socket\":{},\"session_source\":{},\"sentence_count\":{sentence_count},\"start\":{start}}}",
        json_string(COMMAND_NAME),
        json_string(stage),
        json_string(&target.session_name),
        json_string(&target.socket_path.to_string_lossy()),
        json_string(target.source),
    )
}

fn result_record(
    request_id: u64,
    target: &SayTarget,
    sentence_count: usize,
    accepted: &VoiceTextAccepted,
) -> String {
    format!(
        "{{\"type\":\"result\",\"command\":{},\"ok\":true,\"request_id\":{request_id},\"session\":{},\"sentence_count\":{sentence_count},\"accepted_sentences\":{},\"voice_phase\":{},\"started_voice\":{},\"delivery\":\"queued-to-voice-session\"}}",
        json_string(COMMAND_NAME),
        json_string(&target.session_name),
        accepted.sentence_count,
        json_string(phase_name(accepted.phase)),
        accepted.started_voice,
    )
}

fn error_record(request_id: u64, failure: &SayFailure) -> String {
    let hint = failure.hint.map_or_else(|| "null".to_owned(), json_string);
    format!(
        "{{\"type\":\"error\",\"command\":{},\"ok\":false,\"request_id\":{request_id},\"code\":{},\"message\":{},\"hint\":{hint}}}",
        json_string(COMMAND_NAME),
        json_string(failure.code),
        json_string(&failure.message),
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn target() -> SayTarget {
        SayTarget {
            socket_path: PathBuf::from("/run/user/1000/ilium/x.sock"),
            session_name: "default".to_owned(),
            source: "cwd",
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn positional_sentences_keep_order_and_are_trimmed() {
        let sentences = collect_sentences(
            &strings(&["  open the settings ", "close it"]),
            &mut Cursor::new(""),
        )
        .expect("valid");
        assert_eq!(sentences, ["open the settings", "close it"]);
    }

    #[test]
    fn a_lone_hyphen_expands_to_the_non_empty_lines_of_stdin_in_place() {
        let mut stdin = Cursor::new("second\n\n   \nthird  \n");
        let sentences =
            collect_sentences(&strings(&["first", "-", "last"]), &mut stdin).expect("valid");
        assert_eq!(sentences, ["first", "second", "third", "last"]);
    }

    #[test]
    fn nothing_to_say_and_oversized_input_are_invalid_requests() {
        let empty =
            collect_sentences(&strings(&["-"]), &mut Cursor::new("\n\n")).expect_err("empty stdin");
        assert_eq!(empty.code, "invalid-request");
        let blank =
            collect_sentences(&strings(&["  "]), &mut Cursor::new("")).expect_err("blank sentence");
        assert_eq!(blank.code, "invalid-request");
        let many = "line\n".repeat(ilium_ipc::MAX_VOICE_TEXT_SENTENCES + 1);
        let too_many = collect_sentences(&strings(&["-"]), &mut Cursor::new(many))
            .expect_err("too many sentences");
        assert_eq!(too_many.code, "invalid-request");
    }

    #[test]
    fn records_are_single_line_json_objects_with_a_type() {
        let progress = progress_record(9, "sending", &target(), 2, true);
        assert!(progress.starts_with("{\"type\":\"progress\","));
        assert!(progress.contains("\"session_source\":\"cwd\""));
        assert!(progress.contains("\"start\":true"));

        let accepted = VoiceTextAccepted {
            sentence_count: 2,
            phase: VoiceTextPhase::Listening,
            started_voice: true,
        };
        let result = result_record(9, &target(), 2, &accepted);
        assert!(result.starts_with("{\"type\":\"result\","));
        assert!(result.contains("\"ok\":true"));
        assert!(result.contains("\"voice_phase\":\"listening\""));
        assert!(result.contains("\"started_voice\":true"));

        let failure = rejection_failure(&VoiceTextRejection::new(
            VoiceTextRejectionCode::VoiceOff,
            "voice control is \"off\"\nsecond line",
        ));
        let error = error_record(9, &failure);
        assert!(error.starts_with("{\"type\":\"error\","));
        assert!(error.contains("\"code\":\"voice-off\""));
        assert!(error.contains("--start"), "the hint names the fix: {error}");
        assert!(error.contains("\\\"off\\\"") && error.contains("\\n"));

        for record in [&progress, &result, &error] {
            assert!(!record.contains('\n'), "one record per line: {record}");
        }
    }

    #[test]
    fn say_parses_sentences_the_stdin_token_and_flags() {
        use clap::Parser;

        let cli = crate::Cli::try_parse_from([
            "ilium",
            "--cwd",
            "/work/project",
            "voice",
            "say",
            "--start",
            "--timeout-s",
            "5",
            "open settings",
            "-",
        ])
        .expect("valid invocation");
        let Some(crate::Command::Voice {
            command: VoiceCommand::Say(args),
        }) = cli.command
        else {
            panic!("expected `voice say`");
        };
        assert_eq!(args.sentences, ["open settings", "-"]);
        assert!(args.start);
        assert_eq!(args.timeout_s, 5);
        assert_eq!(args.session_name, session::DEFAULT_SESSION_NAME);

        // Defaults, and a sentence that starts with a hyphen after `--`.
        let cli = crate::Cli::try_parse_from(["ilium", "voice", "say", "--", "-5 degrees"])
            .expect("valid invocation");
        let Some(crate::Command::Voice {
            command: VoiceCommand::Say(args),
        }) = cli.command
        else {
            panic!("expected `voice say`");
        };
        assert_eq!(args.sentences, ["-5 degrees"]);
        assert!(!args.start);
        assert_eq!(args.timeout_s, 30);
    }

    #[test]
    fn say_needs_at_least_one_sentence_and_a_sane_timeout() {
        use clap::Parser;

        assert!(crate::Cli::try_parse_from(["ilium", "voice", "say"]).is_err());
        assert!(
            crate::Cli::try_parse_from(["ilium", "voice", "say", "--timeout-s", "0", "hi"])
                .is_err()
        );
        assert!(crate::Cli::try_parse_from(["ilium", "voice"]).is_err());
    }

    #[test]
    fn every_rejection_code_has_a_distinct_stable_name() {
        let names = [
            VoiceTextRejectionCode::InvalidRequest,
            VoiceTextRejectionCode::NoVoiceClient,
            VoiceTextRejectionCode::VoiceOff,
            VoiceTextRejectionCode::VoiceUnavailable,
            VoiceTextRejectionCode::ClientUnresponsive,
        ]
        .map(rejection_code_name);
        assert_eq!(
            names,
            [
                "invalid-request",
                "no-voice-client",
                "voice-off",
                "voice-unavailable",
                "client-unresponsive"
            ]
        );
    }
}
