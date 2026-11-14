//! Host implementation of `ilium:host/cli` (api-cli; capability-cli-contribute).
//!
//! Without a [`CliBook`] the import only validates: it tracks, per invocation id, whether the
//! invocation has finished and how much standard input it has requested. With a book (what the
//! runtime gives every instance) an invocation is a registered session: output goes into the
//! book for the CLI router to carry to the caller, `finish` queues the exit code, standard input
//! comes from the bytes the caller supplied, and the output form and `stdin` mode of the
//! manifest entry are enforced.

use ilium_plugin_api::generated::host::cli::{HostCli, PaneSelection};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{NodeId, RequestId};
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef, RequestResult};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use super::backed::ResultQueue;
use super::env::RequestIds;

/// Largest text one output call accepts, in bytes (api-cli "Output forms").
const OUTPUT_LIMIT_BYTES: usize = 64 * 1024;
/// Total standard input one invocation may read (api-cli "Standard input").
const STDIN_TOTAL_LIMIT_BYTES: u64 = 4 * 1024 * 1024;

/// The output form a manifest entry declares (`output`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputForm {
    /// One JSON object per line; `write-text` is refused.
    Jsonl,
    /// Plain text; `write-line` is refused.
    Text,
    /// Both.
    Mixed,
}

impl OutputForm {
    /// The form named by the manifest value; the default is JSON lines.
    pub fn from_manifest(value: Option<&str>) -> Self {
        match value {
            Some("text") => Self::Text,
            Some("mixed") => Self::Mixed,
            _ => Self::Jsonl,
        }
    }
}

/// How a command may use standard input (`stdin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdinMode {
    /// The command reads none; `read-stdin` is `unsupported`.
    None,
    /// The command may read it.
    Optional,
    /// The command needs it.
    Required,
}

impl StdinMode {
    /// The mode named by the manifest value; the default is none.
    pub fn from_manifest(value: Option<&str>) -> Self {
        match value {
            Some("optional") => Self::Optional,
            Some("required") => Self::Required,
            _ => Self::None,
        }
    }
}

/// Everything the host knows about one invocation when it starts.
#[derive(Debug, Clone)]
pub struct CliSession {
    /// The plugin that owns the command and so the invocation.
    pub plugin: String,
    /// The output form of the command.
    pub output: OutputForm,
    /// The standard input mode of the command.
    pub stdin_mode: StdinMode,
    /// The bytes the caller supplied as standard input.
    pub stdin: Vec<u8>,
    /// Whether the caller's standard output is a terminal.
    pub stdout_tty: bool,
    /// Whether the caller's standard error is a terminal.
    pub stderr_tty: bool,
    /// The pane the command ran in, when the caller named one.
    pub caller_pane: Option<NodeId>,
    /// The panes `--pane` or `--all-panes` selected, resolved by the host.
    pub selection: Option<PaneSelection>,
}

/// One piece of what an invocation produced, in the order it was produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliChunk {
    /// Bytes for the caller's standard output.
    Stdout {
        /// The invocation.
        invocation: u64,
        /// The bytes.
        data: Vec<u8>,
    },
    /// Bytes for the caller's standard error.
    Stderr {
        /// The invocation.
        invocation: u64,
        /// The bytes.
        data: Vec<u8>,
    },
    /// The invocation ended; the caller exits with this code.
    Exit {
        /// The invocation.
        invocation: u64,
        /// The process exit code.
        code: i32,
    },
}

#[derive(Debug)]
struct OpenSession {
    spec: CliSession,
    /// How many bytes of `spec.stdin` earlier reads took.
    stdin_taken: usize,
}

#[derive(Debug, Default)]
struct BookState {
    open: HashMap<u64, OpenSession>,
    output: Vec<CliChunk>,
    next_invocation: u64,
}

/// The invocations of one runtime and what they produced. Shared between the runtime (which
/// opens sessions and drains output) and the `cli` import of every instance.
#[derive(Debug)]
pub struct CliBook {
    state: Mutex<BookState>,
    results: ResultQueue,
    /// The request ids of `read-stdin` come from the runtime's allocator, so they never meet the
    /// id of another request of the plugin.
    ids: RequestIds,
}

impl CliBook {
    /// An empty book. Completed `read-stdin` requests are queued on `results`.
    pub fn new(results: ResultQueue) -> Arc<Self> {
        Self::with_ids(results, RequestIds::new())
    }

    /// An empty book whose `read-stdin` request ids come from `ids`.
    pub fn with_ids(results: ResultQueue, ids: RequestIds) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(BookState {
                next_invocation: 1,
                ..BookState::default()
            }),
            results,
            ids,
        })
    }

    fn state(&self) -> MutexGuard<'_, BookState> {
        // A poisoned lock only means another thread panicked mid-update; the data stays usable.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Registers an invocation and returns its id, unique for the life of the book.
    pub fn open(&self, spec: CliSession) -> u64 {
        let mut state = self.state();
        let invocation = state.next_invocation;
        state.next_invocation += 1;
        state.open.insert(
            invocation,
            OpenSession {
                spec,
                stdin_taken: 0,
            },
        );
        invocation
    }

    /// Whether the invocation is still running.
    pub fn is_open(&self, invocation: u64) -> bool {
        self.state().open.contains_key(&invocation)
    }

    /// The plugin that owns a running invocation.
    pub fn plugin_of(&self, invocation: u64) -> Option<String> {
        self.state()
            .open
            .get(&invocation)
            .map(|session| session.spec.plugin.clone())
    }

    /// The ids of the invocations a plugin still has running, in start order.
    pub fn open_of(&self, plugin: &str) -> Vec<u64> {
        let state = self.state();
        let mut ids: Vec<u64> = state
            .open
            .iter()
            .filter(|(_, session)| session.spec.plugin == plugin)
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Ends an invocation without the plugin: the caller sees `code` and the invocation takes no
    /// more output. Returns false when it was not running.
    pub fn close(&self, invocation: u64, code: i32) -> bool {
        let mut state = self.state();
        if state.open.remove(&invocation).is_none() {
            return false;
        }
        state.output.push(CliChunk::Exit { invocation, code });
        true
    }

    /// Ends an invocation the caller walked away from. Nothing more is produced for it, and no
    /// exit is reported because nobody is listening. Returns false when it was not running.
    pub fn cancel(&self, invocation: u64) -> bool {
        self.state().open.remove(&invocation).is_some()
    }

    /// Ends every invocation of a plugin whose instance stopped, with `code` and `message` on
    /// standard error. Returns the ids.
    pub fn abandon_plugin(&self, plugin: &str, code: i32, message: &str) -> Vec<u64> {
        let ids = self.open_of(plugin);
        let mut state = self.state();
        for invocation in &ids {
            state.open.remove(invocation);
            state.output.push(CliChunk::Stderr {
                invocation: *invocation,
                data: format!("{message}\n").into_bytes(),
            });
            state.output.push(CliChunk::Exit {
                invocation: *invocation,
                code,
            });
        }
        ids
    }

    /// Takes everything produced since the last call, oldest first.
    pub fn take_output(&self) -> Vec<CliChunk> {
        std::mem::take(&mut self.state().output)
    }

    fn push(&self, chunk: CliChunk) {
        self.state().output.push(chunk);
    }

    fn with_session<T>(
        &self,
        plugin: &str,
        invocation: u64,
        use_session: impl FnOnce(&mut OpenSession) -> T,
    ) -> Result<T, PluginError> {
        let mut state = self.state();
        match state.open.get_mut(&invocation) {
            Some(session) if session.spec.plugin == plugin => Ok(use_session(session)),
            _ => Err(PluginError::new(
                ErrorCode::NotFound,
                "the invocation has already finished",
            )),
        }
    }
}

/// Host side of the `cli` interface.
#[derive(Debug, Default)]
pub struct CliImport {
    plugin: String,
    book: Option<Arc<CliBook>>,
    finished: HashSet<u64>,
    stdin_requested: HashMap<u64, u64>,
    stdin_requests: HashMap<u64, Vec<u8>>,
    next_request: u64,
}

impl CliImport {
    /// The import of one plugin instance over the runtime's book.
    pub fn with_book(plugin: &str, book: Arc<CliBook>) -> Self {
        Self {
            plugin: plugin.to_owned(),
            book: Some(book),
            ..Self::default()
        }
    }

    fn ensure_open(&self, invocation: u64) -> Result<(), PluginError> {
        if self.finished.contains(&invocation) {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                "the invocation has already finished",
            ));
        }
        match &self.book {
            Some(book) => book.with_session(&self.plugin, invocation, |_| ()),
            None => Ok(()),
        }
    }

    fn check_text(&self, invocation: u64, text: &str) -> Result<(), PluginError> {
        self.ensure_open(invocation)?;
        if text.len() > OUTPUT_LIMIT_BYTES {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "output is larger than 64 KiB",
            ));
        }
        Ok(())
    }

    /// Refuses an output call the command's declared form does not allow, then queues the bytes.
    fn emit(
        &self,
        invocation: u64,
        allowed: &[OutputForm],
        refusal: &str,
        chunk: impl FnOnce(u64) -> CliChunk,
    ) -> Result<(), PluginError> {
        let Some(book) = &self.book else {
            return Ok(());
        };
        let form = book.with_session(&self.plugin, invocation, |session| session.spec.output)?;
        if !allowed.contains(&form) {
            return Err(invalid(refusal));
        }
        book.push(chunk(invocation));
        Ok(())
    }

    fn stderr_chunk(invocation: u64, mut text: String, newline: bool) -> CliChunk {
        if newline {
            text.push('\n');
        }
        CliChunk::Stderr {
            invocation,
            data: text.into_bytes(),
        }
    }
}

fn invalid(message: &str) -> PluginError {
    PluginError::new(ErrorCode::InvalidArgument, message)
}

impl HostCli for CliImport {
    fn write_line(&mut self, invocation: u64, jsonl: String) -> Result<(), PluginError> {
        self.check_text(invocation, &jsonl)?;
        if jsonl.contains('\n') || jsonl.contains('\r') {
            return Err(invalid("a JSONL line must not contain a raw newline"));
        }
        let value: serde_json::Value = serde_json::from_str(&jsonl)
            .map_err(|error| invalid(&format!("line is not valid JSON: {error}")))?;
        let has_string_type = value
            .as_object()
            .and_then(|object| object.get("type"))
            .is_some_and(serde_json::Value::is_string);
        if !has_string_type {
            return Err(invalid(
                "a JSONL line must be an object with a string \"type\"",
            ));
        }
        self.emit(
            invocation,
            &[OutputForm::Jsonl, OutputForm::Mixed],
            "write-line is only for commands with output \"jsonl\" or \"mixed\"",
            |invocation| {
                let mut data = jsonl.into_bytes();
                data.push(b'\n');
                CliChunk::Stdout { invocation, data }
            },
        )
    }

    fn write_error(&mut self, invocation: u64, text: String) -> Result<(), PluginError> {
        self.check_text(invocation, &text)?;
        self.emit(
            invocation,
            &[OutputForm::Jsonl, OutputForm::Text, OutputForm::Mixed],
            "",
            |invocation| Self::stderr_chunk(invocation, text, true),
        )
    }

    fn write_text(&mut self, invocation: u64, text: String) -> Result<(), PluginError> {
        self.check_text(invocation, &text)?;
        self.emit(
            invocation,
            &[OutputForm::Text, OutputForm::Mixed],
            "write-text is only for commands with output \"text\" or \"mixed\"",
            |invocation| CliChunk::Stdout {
                invocation,
                data: text.into_bytes(),
            },
        )
    }

    fn write_stderr(&mut self, invocation: u64, text: String) -> Result<(), PluginError> {
        self.check_text(invocation, &text)?;
        self.emit(
            invocation,
            &[OutputForm::Jsonl, OutputForm::Text, OutputForm::Mixed],
            "",
            |invocation| Self::stderr_chunk(invocation, text, false),
        )
    }

    fn finish(&mut self, invocation: u64, exit_code: u8) {
        match &self.book {
            Some(book) => {
                // An invocation of another plugin, a finished one or an unknown one is ignored.
                if book.with_session(&self.plugin, invocation, |_| ()).is_ok() {
                    book.close(invocation, i32::from(exit_code));
                    self.finished.insert(invocation);
                }
            }
            None => {
                self.finished.insert(invocation);
            }
        }
    }

    fn read_stdin(&mut self, invocation: u64, max_bytes: u32) -> Result<u64, PluginError> {
        self.ensure_open(invocation)?;
        if let Some(book) = &self.book {
            let mode =
                book.with_session(&self.plugin, invocation, |session| session.spec.stdin_mode)?;
            if mode == StdinMode::None {
                return Err(PluginError::new(
                    ErrorCode::Unsupported,
                    "the command declares stdin \"none\"",
                ));
            }
        }
        if max_bytes == 0 {
            return Err(invalid("max-bytes must be at least 1"));
        }
        let requested = self.stdin_requested.entry(invocation).or_insert(0);
        if *requested >= STDIN_TOTAL_LIMIT_BYTES {
            return Err(PluginError::new(
                ErrorCode::QuotaExceeded,
                "the invocation has used its 4 MiB of standard input",
            ));
        }
        *requested = (*requested + u64::from(max_bytes)).min(STDIN_TOTAL_LIMIT_BYTES);
        let chunk = match &self.book {
            Some(book) => book.with_session(&self.plugin, invocation, |session| {
                let available = session.spec.stdin.len() - session.stdin_taken;
                let wanted = usize::try_from(max_bytes).unwrap_or(usize::MAX);
                let size = available.min(wanted);
                let start = session.stdin_taken;
                session.stdin_taken += size;
                session.spec.stdin[start..start + size].to_vec()
            })?,
            // No stdin source is attached to a bare import: the read ends at once with end of
            // input.
            None => Vec::new(),
        };
        let request = match &self.book {
            Some(book) => book.ids.next_request().0,
            None => {
                self.next_request += 1;
                self.next_request
            }
        };
        self.stdin_requests.insert(request, chunk);
        if let Some(book) = &self.book {
            let mut results = book
                .results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            results.push_back((
                self.plugin.clone(),
                RequestResult {
                    request: RequestRef::Call(RequestId(request)),
                    outcome: Outcome::Ok,
                },
            ));
        }
        Ok(request)
    }

    fn take_stdin(&mut self, request: u64) -> Option<Vec<u8>> {
        self.stdin_requests.remove(&request)
    }

    fn is_tty(&mut self, invocation: u64) -> (bool, bool) {
        self.book
            .as_ref()
            .and_then(|book| {
                book.with_session(&self.plugin, invocation, |session| {
                    (session.spec.stdout_tty, session.spec.stderr_tty)
                })
                .ok()
            })
            .unwrap_or((false, false))
    }

    fn caller_pane(&mut self, invocation: u64) -> Option<NodeId> {
        self.book.as_ref().and_then(|book| {
            book.with_session(&self.plugin, invocation, |session| session.spec.caller_pane)
                .ok()
                .flatten()
        })
    }

    fn selected_panes(&mut self, invocation: u64) -> Option<PaneSelection> {
        self.book.as_ref().and_then(|book| {
            book.with_session(&self.plugin, invocation, |session| {
                session.spec.selection.clone()
            })
            .ok()
            .flatten()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::CliImport;
    use ilium_plugin_api::generated::host::cli::HostCli;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

    /// Invocation id used by every test. The host assigns ids; this module only receives them.
    const INVOCATION: u64 = 1;

    /// Largest text one `write-line`, `write-text` or `write-error` call accepts, in bytes
    /// (api-cli "Output forms"; capability-cli-contribute "Functions": over 64 KiB is invalid-argument).
    const OUTPUT_LIMIT_BYTES: usize = 64 * 1024;

    /// Total standard input one invocation may read (api-cli "Standard input": at most 4 MiB).
    const STDIN_TOTAL_LIMIT_BYTES: u32 = 4 * 1024 * 1024;

    /// Fixed head and tail of the padded JSON object built by `jsonl_object_of_len`.
    const OBJECT_HEAD: &str = r#"{"type":"result","pad":""#;
    const OBJECT_TAIL: &str = r#""}"#;

    /// Builds one JSON object with a string `type` field whose UTF-8 length is exactly `total_bytes`.
    fn jsonl_object_of_len(total_bytes: usize) -> String {
        let padding = total_bytes - OBJECT_HEAD.len() - OBJECT_TAIL.len();
        format!("{OBJECT_HEAD}{}{OBJECT_TAIL}", "a".repeat(padding))
    }

    /// Returns the value of an `Ok` result and panics with the error code and message otherwise.
    fn expect_ok<T>(result: Result<T, PluginError>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected Ok, got {:?}: {}", error.code, error.message),
        }
    }

    /// Asserts that the result is an error with the given code.
    fn expect_code<T: std::fmt::Debug>(result: Result<T, PluginError>, expected: ErrorCode) {
        match result {
            Err(error) => assert_eq!(error.code, expected, "message: {}", error.message),
            Ok(value) => panic!("expected {expected:?}, got Ok({value:?})"),
        }
    }

    /// REQ: API-005
    #[test]
    fn write_line_accepts_one_object_with_string_type() {
        let mut cli = CliImport::default();
        expect_ok(cli.write_line(INVOCATION, r#"{"type":"result","ok":true}"#.to_string()));
    }

    /// REQ: API-062
    #[test]
    fn write_line_accepts_object_of_exactly_64_kib() {
        let mut cli = CliImport::default();
        expect_ok(cli.write_line(INVOCATION, jsonl_object_of_len(OUTPUT_LIMIT_BYTES)));
    }

    /// REQ: API-062
    #[test]
    fn write_line_rejects_object_one_byte_over_64_kib() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, jsonl_object_of_len(OUTPUT_LIMIT_BYTES + 1)),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_empty_input() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, String::new()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_object_without_type() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, r#"{"name":"notes"}"#.to_string()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_non_string_type() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, r#"{"type":1}"#.to_string()),
            ErrorCode::InvalidArgument,
        );
        expect_code(
            cli.write_line(INVOCATION, r#"{"type":null}"#.to_string()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_json_that_is_not_an_object() {
        let mut cli = CliImport::default();
        for text in ["[]", r#""text""#, "42", "null", "true"] {
            expect_code(
                cli.write_line(INVOCATION, text.to_string()),
                ErrorCode::InvalidArgument,
            );
        }
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_raw_newline_inside_or_after_the_object() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, "{\"type\":\"result\"}\n".to_string()),
            ErrorCode::InvalidArgument,
        );
        expect_code(
            cli.write_line(INVOCATION, "{\"type\":\n\"result\"}".to_string()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_malformed_json() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, r#"{"type":"#.to_string()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_after_finish_returns_not_found() {
        let mut cli = CliImport::default();
        cli.finish(INVOCATION, 0);
        expect_code(
            cli.write_line(INVOCATION, r#"{"type":"result"}"#.to_string()),
            ErrorCode::NotFound,
        );
    }

    /// REQ: API-062
    #[test]
    fn write_text_rejects_text_one_byte_over_64_kib() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_text(INVOCATION, "t".repeat(OUTPUT_LIMIT_BYTES + 1)),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_text_after_finish_returns_not_found() {
        let mut cli = CliImport::default();
        cli.finish(INVOCATION, 0);
        expect_code(
            cli.write_text(INVOCATION, "hello".to_string()),
            ErrorCode::NotFound,
        );
    }

    /// REQ: API-062
    #[test]
    fn write_error_accepts_text_of_exactly_64_kib() {
        let mut cli = CliImport::default();
        expect_ok(cli.write_error(INVOCATION, "e".repeat(OUTPUT_LIMIT_BYTES)));
    }

    /// REQ: API-062
    #[test]
    fn write_error_rejects_text_one_byte_over_64_kib() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_error(INVOCATION, "e".repeat(OUTPUT_LIMIT_BYTES + 1)),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_error_accepts_plain_text_message() {
        let mut cli = CliImport::default();
        expect_ok(cli.write_error(INVOCATION, "hello: pass --pane or --all-panes".to_string()));
    }

    /// REQ: API-005
    #[test]
    fn write_error_after_finish_returns_not_found() {
        let mut cli = CliImport::default();
        cli.finish(INVOCATION, 1);
        expect_code(
            cli.write_error(INVOCATION, "late".to_string()),
            ErrorCode::NotFound,
        );
    }

    /// REQ: API-005
    #[test]
    fn finish_unknown_or_repeated_invocation_is_ignored() {
        let mut cli = CliImport::default();
        cli.finish(INVOCATION, 0);
        cli.finish(INVOCATION, 0);
        cli.finish(9_999, 0);
    }

    /// REQ: API-005
    #[test]
    fn finish_accepts_largest_exit_code() {
        let mut cli = CliImport::default();
        cli.finish(INVOCATION, u8::MAX);
    }

    /// REQ: API-005
    #[test]
    fn read_stdin_accepts_smallest_request_of_one_byte() {
        let mut cli = CliImport::default();
        expect_ok(cli.read_stdin(INVOCATION, 1));
    }

    /// REQ: API-061
    #[test]
    fn read_stdin_accepts_full_four_mib_total_in_one_call() {
        let mut cli = CliImport::default();
        expect_ok(cli.read_stdin(INVOCATION, STDIN_TOTAL_LIMIT_BYTES));
    }

    /// REQ: API-061
    #[test]
    fn read_stdin_beyond_four_mib_total_returns_quota_exceeded() {
        let mut cli = CliImport::default();
        expect_ok(cli.read_stdin(INVOCATION, STDIN_TOTAL_LIMIT_BYTES));
        expect_code(cli.read_stdin(INVOCATION, 1), ErrorCode::QuotaExceeded);
    }
}

#[cfg(test)]
mod book_tests {
    use super::{
        CliBook, CliChunk, CliImport, CliSession, OutputForm, ResultQueue, StdinMode,
        STDIN_TOTAL_LIMIT_BYTES,
    };
    use ilium_plugin_api::generated::host::cli::{HostCli, PaneSelection};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::{NodeId, RequestId};
    use ilium_plugin_api::generated::types::requests::RequestRef;
    use std::sync::Arc;

    fn session(plugin: &str, output: OutputForm, stdin_mode: StdinMode) -> CliSession {
        CliSession {
            plugin: plugin.to_owned(),
            output,
            stdin_mode,
            stdin: Vec::new(),
            stdout_tty: false,
            stderr_tty: false,
            caller_pane: None,
            selection: None,
        }
    }

    fn book() -> (Arc<CliBook>, ResultQueue) {
        let results = ResultQueue::default();
        (CliBook::new(Arc::clone(&results)), results)
    }

    fn code_of<T: std::fmt::Debug>(result: Result<T, PluginError>) -> ErrorCode {
        match result {
            Err(error) => error.code,
            Ok(value) => panic!("expected an error, got Ok({value:?})"),
        }
    }

    /// REQ: API-005
    #[test]
    fn output_reaches_the_book_in_order_with_the_exit_code_last() {
        let (book, _results) = book();
        let invocation = book.open(session("a.b", OutputForm::Jsonl, StdinMode::None));
        let mut import = CliImport::with_book("a.b", Arc::clone(&book));
        assert!(import
            .write_line(invocation, r#"{"type":"row"}"#.to_owned())
            .is_ok());
        assert!(import.write_error(invocation, "oops".to_owned()).is_ok());
        assert!(import.write_stderr(invocation, "raw".to_owned()).is_ok());
        import.finish(invocation, 3);
        assert_eq!(
            book.take_output(),
            vec![
                CliChunk::Stdout {
                    invocation,
                    data: b"{\"type\":\"row\"}\n".to_vec()
                },
                CliChunk::Stderr {
                    invocation,
                    data: b"oops\n".to_vec()
                },
                CliChunk::Stderr {
                    invocation,
                    data: b"raw".to_vec()
                },
                CliChunk::Exit {
                    invocation,
                    code: 3
                },
            ]
        );
        assert!(!book.is_open(invocation));
    }

    /// REQ: API-005
    #[test]
    fn the_declared_output_form_decides_which_write_is_allowed() {
        let (book, _results) = book();
        let jsonl = book.open(session("a.b", OutputForm::Jsonl, StdinMode::None));
        let text = book.open(session("a.b", OutputForm::Text, StdinMode::None));
        let mixed = book.open(session("a.b", OutputForm::Mixed, StdinMode::None));
        let mut import = CliImport::with_book("a.b", Arc::clone(&book));
        assert_eq!(
            code_of(import.write_text(jsonl, "x".to_owned())),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code_of(import.write_line(text, r#"{"type":"row"}"#.to_owned())),
            ErrorCode::InvalidArgument
        );
        assert!(import.write_text(mixed, "x".to_owned()).is_ok());
        assert!(import
            .write_line(mixed, r#"{"type":"row"}"#.to_owned())
            .is_ok());
        // A refused write is not sent.
        let sent = book.take_output();
        assert_eq!(sent.len(), 2);
    }

    /// REQ: API-005
    #[test]
    fn an_invocation_of_another_plugin_is_not_found() {
        let (book, _results) = book();
        let invocation = book.open(session("a.b", OutputForm::Text, StdinMode::None));
        let mut other = CliImport::with_book("c.d", Arc::clone(&book));
        assert_eq!(
            code_of(other.write_text(invocation, "x".to_owned())),
            ErrorCode::NotFound
        );
        other.finish(invocation, 1);
        assert!(book.is_open(invocation), "a foreign finish is ignored");
    }

    /// REQ: API-005
    #[test]
    fn writes_after_a_cancel_are_not_found_and_nothing_is_reported() {
        let (book, _results) = book();
        let invocation = book.open(session("a.b", OutputForm::Text, StdinMode::None));
        let mut import = CliImport::with_book("a.b", Arc::clone(&book));
        assert!(book.cancel(invocation));
        assert_eq!(
            code_of(import.write_text(invocation, "x".to_owned())),
            ErrorCode::NotFound
        );
        import.finish(invocation, 0);
        assert!(book.take_output().is_empty());
        assert!(!book.cancel(invocation));
    }

    /// REQ: API-061
    #[test]
    fn standard_input_arrives_in_chunks_then_ends_empty() {
        let (book, results) = book();
        let mut spec = session("a.b", OutputForm::Text, StdinMode::Optional);
        spec.stdin = b"hello".to_vec();
        let invocation = book.open(spec);
        let mut import = CliImport::with_book("a.b", Arc::clone(&book));
        let first = import.read_stdin(invocation, 3);
        assert!(first.is_ok());
        let first = first.unwrap_or_default();
        assert_eq!(import.take_stdin(first), Some(b"hel".to_vec()));
        let second = import.read_stdin(invocation, 64).unwrap_or_default();
        assert_eq!(import.take_stdin(second), Some(b"lo".to_vec()));
        let third = import.read_stdin(invocation, 64).unwrap_or_default();
        assert_eq!(import.take_stdin(third), Some(Vec::new()));
        let queued = results.lock().map(|queue| queue.len()).unwrap_or_default();
        assert_eq!(
            queued, 3,
            "each read completes as a request-completed event"
        );
        let front = results.lock().ok().and_then(|queue| queue.front().cloned());
        let Some((plugin, result)) = front else {
            panic!("a completed request is queued");
        };
        assert_eq!(plugin, "a.b");
        assert_eq!(result.request, RequestRef::Call(RequestId(first)));
    }

    /// REQ: API-061
    #[test]
    fn standard_input_is_unsupported_for_stdin_none() {
        let (book, _results) = book();
        let invocation = book.open(session("a.b", OutputForm::Text, StdinMode::None));
        let mut import = CliImport::with_book("a.b", Arc::clone(&book));
        assert_eq!(
            code_of(import.read_stdin(invocation, 8)),
            ErrorCode::Unsupported
        );
    }

    /// REQ: API-061
    #[test]
    fn the_four_mib_total_applies_to_a_booked_invocation_too() {
        let (book, _results) = book();
        let invocation = book.open(session("a.b", OutputForm::Text, StdinMode::Optional));
        let mut import = CliImport::with_book("a.b", Arc::clone(&book));
        let all = u32::try_from(STDIN_TOTAL_LIMIT_BYTES).unwrap_or(u32::MAX);
        assert!(import.read_stdin(invocation, all).is_ok());
        assert_eq!(
            code_of(import.read_stdin(invocation, 1)),
            ErrorCode::QuotaExceeded
        );
    }

    /// REQ: API-005
    #[test]
    fn tty_caller_pane_and_selection_come_from_the_session() {
        let (book, _results) = book();
        let mut spec = session("a.b", OutputForm::Jsonl, StdinMode::None);
        spec.stdout_tty = true;
        spec.caller_pane = Some(NodeId(7));
        spec.selection = Some(PaneSelection {
            panes: vec![NodeId(7), NodeId(9)],
            all: false,
        });
        let invocation = book.open(spec);
        let plain = book.open(session("a.b", OutputForm::Jsonl, StdinMode::None));
        let mut import = CliImport::with_book("a.b", Arc::clone(&book));
        assert_eq!(import.is_tty(invocation), (true, false));
        assert_eq!(import.caller_pane(invocation), Some(NodeId(7)));
        assert_eq!(
            import.selected_panes(invocation),
            Some(PaneSelection {
                panes: vec![NodeId(7), NodeId(9)],
                all: false
            })
        );
        assert_eq!(import.caller_pane(plain), None);
        assert_eq!(import.selected_panes(plain), None);
    }

    /// REQ: API-005
    #[test]
    fn invocation_ids_are_unique_and_a_stopped_plugin_ends_its_invocations() {
        let (book, _results) = book();
        let first = book.open(session("a.b", OutputForm::Text, StdinMode::None));
        let second = book.open(session("a.b", OutputForm::Text, StdinMode::None));
        let other = book.open(session("c.d", OutputForm::Text, StdinMode::None));
        assert!(first < second && second < other);
        let ended = book.abandon_plugin("a.b", 70, "the plugin stopped");
        assert_eq!(ended, vec![first, second]);
        assert!(book.is_open(other));
        let output = book.take_output();
        assert_eq!(output.len(), 4);
        assert_eq!(
            output.last(),
            Some(&CliChunk::Exit {
                invocation: second,
                code: 70
            })
        );
    }
}
