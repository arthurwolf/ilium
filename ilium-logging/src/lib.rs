//! Process-wide, dynamically switchable file diagnostics for ilium.
//!
//! The detached server owns one timestamped path for its complete lifetime.
//! Attached clients forward bounded event frames to the server, which admits
//! them to the one ordered file-writer queue. A single tracing subscriber per
//! process routes existing and new `tracing` events through this boundary.
//! Disabling logging closes the file at its ordered acknowledgement boundary
//! and turns writes into a sink. File I/O is performed by one bounded
//! process-owned OS thread.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use ilium_execution::{QuotaGroup, RejectReason, WorkerStartError};
use ilium_platform::owned_worker::WorkerTicket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};

mod relay;
mod service;
pub use relay::{LogRelayEndpoint, LogRelayServer};
pub use service::{
    logger_control_bytes, logger_storage_bytes, LoggingHealth, LoggingReceipt, LoggingShutdown,
    LoggingShutdownDeadline, LoggingShutdownReport, LOGGER_STACK_BYTES, MAX_EVENT_BYTES,
    MAX_RETAINED_BYTES,
};

use tracing_subscriber::filter::{FilterExt, LevelFilter, Targets};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

static PROCESS_LOGGER: OnceLock<Arc<LoggerState>> = OnceLock::new();
#[derive(Clone)]
struct RetiringLogger {
    ticket: WorkerTicket,
    quota: QuotaGroup,
    path_bytes: usize,
}
enum InitializationState {
    Idle,
    Initializing,
    Retiring(RetiringLogger),
}
static PROCESS_INITIALIZATION: Mutex<InitializationState> = Mutex::new(InitializationState::Idle);

struct InitializationAttempt {
    failed_worker: Option<RetiringLogger>,
}
impl InitializationAttempt {
    fn claim() -> Result<Self, LoggingError> {
        let previous = {
            let mut state = PROCESS_INITIALIZATION
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match &*state {
                InitializationState::Initializing => return Err(LoggingError::AdmissionBusy),
                InitializationState::Retiring(owner) if owner.ticket.exit().is_none() => {
                    return Err(LoggingError::InitializationRetiring);
                }
                _ => {}
            }
            std::mem::replace(&mut *state, InitializationState::Initializing)
        };
        let attempt = Self {
            failed_worker: None,
        };
        // Retired ticket captures can release storage and invoke a quota wake.
        // The claim remains published, but no initialization mutex is held.
        drop(previous);
        Ok(attempt)
    }
}
impl Drop for InitializationAttempt {
    fn drop(&mut self) {
        let next = match self.failed_worker.take() {
            Some(owner) => {
                owner.ticket.cancel();
                InitializationState::Retiring(owner)
            }
            None => InitializationState::Idle,
        };
        let previous = {
            let mut state = PROCESS_INITIALIZATION
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::mem::replace(&mut *state, next)
        };
        drop(previous);
    }
}

fn clear_retired_initialization(worker_id: u64) {
    let previous = {
        let mut state = PROCESS_INITIALIZATION
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !matches!(&*state, InitializationState::Retiring(owner) if owner.ticket.id() == worker_id)
        {
            return;
        }
        std::mem::replace(&mut *state, InitializationState::Idle)
    };
    // Called only after the original ticket reports actual join. Captures and
    // their quota wake are destroyed outside the initialization gate.
    drop(previous);
}

/// Failures that prevent this process from providing the requested log.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("file logging is already initialized in this process")]
    AlreadyInitialized,
    #[error("file logging is already bound to another quota root")]
    DifferentQuota,
    #[error("a failed logging initialization is still retiring")]
    InitializationRetiring,
    #[error("logging worker admission failed: {0}")]
    WorkerAdmission(#[source] WorkerStartError),
    #[error("logging storage admission rejected: {0:?}")]
    StorageAdmissionRejected(RejectReason),
    #[error("logging worker panicked before native retirement completed")]
    WorkerPanicked,
    #[error("{0}")]
    ShutdownDeadline(#[source] Box<LoggingShutdownDeadline>),
    #[error("logging service admission is busy; retry the control request")]
    AdmissionBusy,
    #[error("logging service stopped")]
    WorkerStopped,
    #[error("logging acknowledgement deadline elapsed; completion is unknown")]
    Deadline,
    #[error("an earlier log event write failed; no partial event was retried")]
    PriorWriteFailed,
    #[error("logging I/O failed: {0}")]
    Io(#[source] io::Error),
    #[error("file logging has not been initialized in this process")]
    NotInitialized,
    #[error("failed to prepare log file {path}: {source}")]
    PrepareFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to install the process tracing subscriber: {0}")]
    InstallSubscriber(String),
    #[error("log relay failed: {0}")]
    Relay(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LogDestination {
    LocalFile,
    Relay(LogRelayEndpoint),
}

/// Process owner; the local file or relay stream is exclusively owned by the service thread.
struct LoggerState {
    quota: QuotaGroup,
    path: PathBuf,
    destination: LogDestination,
    process_role: &'static str,
    enabled: Arc<AtomicBool>,
    service: service::Service,
}
impl LoggerState {
    fn new_admitted(
        path: &Path,
        destination: LogDestination,
        process_role: &'static str,
        quota: &QuotaGroup,
    ) -> Result<Self, LoggingError> {
        let admission = service::Service::prepare(quota, path)?;
        let path = path.to_owned();
        let enabled = Arc::new(AtomicBool::new(false));
        let service = admission.start(
            path.clone(),
            destination.clone(),
            Arc::clone(&enabled),
            None,
            || {},
        )?;
        Ok(Self {
            quota: quota.clone(),
            path,
            destination,
            process_role,
            enabled,
            service,
        })
    }
    #[cfg(test)]
    fn new(path: PathBuf) -> Result<Self, LoggingError> {
        let quota = service::fixture_quota(&path)?;
        Self::new_admitted(&path, LogDestination::LocalFile, "test", &quota)
    }
    // Startup/off-loop only; the same deadline covers control admission and
    // its acknowledgement. Interactive callers retain and poll receipts.
    fn set_enabled(&self, enabled: bool) -> Result<(), LoggingError> {
        self.set_enabled_until(
            enabled,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
        )
    }
    fn set_enabled_until(
        &self,
        enabled: bool,
        deadline: std::time::Instant,
    ) -> Result<(), LoggingError> {
        loop {
            match self.service.enable(enabled) {
                Ok(receipt) => {
                    return receipt.wait_timeout(
                        deadline.saturating_duration_since(std::time::Instant::now()),
                    );
                }
                Err(LoggingError::AdmissionBusy) => {}
                Err(error) => return Err(error),
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(LoggingError::Deadline);
            }
            std::thread::sleep(remaining.min(std::time::Duration::from_millis(1)));
        }
    }
}

/// One writer per event; rejected or partially written events are never retried.
struct DynamicFileWriter {
    state: Arc<LoggerState>,
    event_buffer: Vec<u8>,
    overflowed: bool,
}
impl DynamicFileWriter {
    fn discard_event(&mut self) {
        self.state.service.release(self.event_buffer.capacity());
        self.event_buffer = Vec::new();
        self.overflowed = true;
        self.state.service.dropped();
    }
}
impl Write for DynamicFileWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // A toggle cannot truncate an event already being formatted. The
        // ordered owner either appends the whole event or counts its rejection.
        if self.overflowed
            || (self.event_buffer.is_empty() && !self.state.enabled.load(Ordering::Acquire))
        {
            return Ok(bytes.len());
        }
        let required = self.event_buffer.len().saturating_add(bytes.len());
        if required > MAX_EVENT_BYTES {
            self.discard_event();
            return Ok(bytes.len());
        }
        if required > self.event_buffer.capacity() {
            let target = required.next_power_of_two().max(64);
            let reserved = target - self.event_buffer.capacity();
            if !self.state.service.reserve(reserved) {
                self.discard_event();
                return Ok(bytes.len());
            }
            let old_capacity = self.event_buffer.capacity();
            if self
                .event_buffer
                .try_reserve_exact(target - self.event_buffer.len())
                .is_err()
            {
                self.state.service.release(reserved);
                self.discard_event();
                return Ok(bytes.len());
            }
            // Account for the capacity actually returned by the allocator, not
            // merely the event length. Keep even partially formatted events in
            // the same process-wide byte budget as queued and writing events.
            let actual_growth = self.event_buffer.capacity() - old_capacity;
            if actual_growth > reserved && !self.state.service.reserve(actual_growth - reserved) {
                self.state.service.release(old_capacity + reserved);
                self.event_buffer = Vec::new();
                self.overflowed = true;
                self.state.service.dropped();
                return Ok(bytes.len());
            }
            if actual_growth < reserved {
                self.state.service.release(reserved - actual_growth);
            }
        }
        self.event_buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if !self.event_buffer.is_empty() {
            self.state
                .service
                .event(std::mem::take(&mut self.event_buffer));
        }
        Ok(())
    }
}
impl Drop for DynamicFileWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// Installs this process's one tracing subscriber and optionally opens `path`.
/// No file is created while `enabled` is false, preserving the normal-user
/// default even though every call site remains instrumented. The caller supplies
/// its existing quota root; real process bootstrap admits the supervisor first.
/// Independent fixture roots do not acquire that permanent process designation.
pub fn initialize(
    path: impl AsRef<Path>,
    enabled: bool,
    process_role: &'static str,
    quota: &QuotaGroup,
) -> Result<(), LoggingError> {
    initialize_with_destination(
        path,
        LogDestination::LocalFile,
        enabled,
        process_role,
        quota,
    )
}

/// Installs this process's tracing subscriber and forwards events to the
/// detached server's single diagnostics-file writer for this session.
pub fn initialize_forwarded(
    path: impl AsRef<Path>,
    enabled: bool,
    process_role: &'static str,
    quota: &QuotaGroup,
) -> Result<(), LoggingError> {
    let path = path.as_ref();
    initialize_with_destination(
        path,
        LogDestination::Relay(LogRelayEndpoint::for_log_path(path)),
        enabled,
        process_role,
        quota,
    )
}

fn initialize_with_destination(
    path: impl AsRef<Path>,
    destination: LogDestination,
    enabled: bool,
    process_role: &'static str,
    quota: &QuotaGroup,
) -> Result<(), LoggingError> {
    let path = path.as_ref();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut attempt = loop {
        match InitializationAttempt::claim() {
            Ok(attempt) => break attempt,
            Err(LoggingError::AdmissionBusy) => {}
            Err(error) => return Err(error),
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(LoggingError::Deadline);
        }
        std::thread::sleep(remaining.min(std::time::Duration::from_millis(1)));
    };
    if let Some(state) = PROCESS_LOGGER.get() {
        if state.path.as_path() != path {
            return Err(LoggingError::AlreadyInitialized);
        }
        if state.destination != destination {
            return Err(LoggingError::AlreadyInitialized);
        }
        if !state.quota.shares_root(quota) {
            return Err(LoggingError::DifferentQuota);
        }
        state.set_enabled_until(enabled, deadline)?;
        drop(attempt);
        tracing::info!(
            process_role,
            process_id = std::process::id(),
            "process logging reused for an in-process role handoff"
        );
        return Ok(());
    }

    let state = Arc::new(LoggerState::new_admitted(
        path,
        destination,
        process_role,
        quota,
    )?);
    attempt.failed_worker = Some(RetiringLogger {
        ticket: state.service.ticket(),
        quota: quota.clone(),
        path_bytes: path.as_os_str().as_encoded_bytes().len(),
    });
    state.set_enabled_until(enabled, deadline)?;
    let writer_state = Arc::clone(&state);
    let filter_state = Arc::clone(&state);
    let configured_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // The Debug-tab contract includes complete HTTP and LLM text payloads,
    // which deliberately live at DEBUG inside the two provider adapters so
    // unrelated agent-history evidence remains outside the process log. An
    // explicit broader `RUST_LOG` can still opt into other debug/trace traffic.
    let provider_payload_filter = Targets::new()
        .with_target("ilium_inference", LevelFilter::DEBUG)
        .with_target("ilium_kilo_gateway", LevelFilter::DEBUG);
    let verbosity_filter = LevelFilter::INFO
        .or(provider_payload_filter)
        .or(configured_filter);
    let enabled_filter = tracing_subscriber::filter::filter_fn(move |_metadata| {
        filter_state.enabled.load(Ordering::Acquire)
    });
    let formatting_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(move || DynamicFileWriter {
            state: Arc::clone(&writer_state),
            event_buffer: Vec::new(),
            overflowed: false,
        })
        .with_filter(verbosity_filter.and(enabled_filter));
    if let Err(error) = tracing_subscriber::registry()
        .with(formatting_layer)
        .try_init()
    {
        return Err(LoggingError::InstallSubscriber(error.to_string()));
    }
    PROCESS_LOGGER
        .set(state)
        .map_err(|_| LoggingError::AlreadyInitialized)?;
    attempt.failed_worker.take();
    drop(attempt);
    tracing::info!(
        process_role,
        process_id = std::process::id(),
        "process logging initialized"
    );
    Ok(())
}

/// Blocking startup/off-loop compatibility adapter. Interactive/coordination
/// loops must use `request_set_enabled` and poll its acknowledgement instead.
pub fn set_enabled(enabled: bool) -> Result<(), LoggingError> {
    PROCESS_LOGGER
        .get()
        .ok_or(LoggingError::NotInitialized)?
        .set_enabled(enabled)
}

/// Nonblocking live toggle. A successful receipt confirms the ordered file transition.
pub fn request_set_enabled(enabled: bool) -> Result<LoggingReceipt, LoggingError> {
    PROCESS_LOGGER
        .get()
        .ok_or(LoggingError::NotInitialized)?
        .service
        .enable(enabled)
}
/// Ordered flush barrier for all previously accepted events.
pub fn request_flush() -> Result<LoggingReceipt, LoggingError> {
    PROCESS_LOGGER
        .get()
        .ok_or(LoggingError::NotInitialized)?
        .service
        .flush()
}
/// Process-exit only: reject later events, drain accepted events, flush and close.
/// Do not call during an in-process CLI/client role handoff. Use `request_flush` before exec.
pub fn request_shutdown() -> Result<LoggingReceipt, LoggingError> {
    PROCESS_LOGGER
        .get()
        .ok_or(LoggingError::NotInitialized)?
        .service
        .shutdown()
}
/// Process-exit drain plus native retirement. Async callers poll the returned
/// owner from their existing timer; blocking callers use one absolute deadline.
/// Failed initialization also retains its original native ticket for this path.
pub fn request_shutdown_joined() -> Result<LoggingShutdown, LoggingError> {
    if let Some(state) = PROCESS_LOGGER.get() {
        return state.service.shutdown_joined();
    }
    let initializing = match PROCESS_INITIALIZATION.try_lock() {
        Ok(initializing) => initializing,
        Err(TryLockError::WouldBlock) => return Err(LoggingError::AdmissionBusy),
        Err(TryLockError::Poisoned(error)) => error.into_inner(),
    };
    let owner = match &*initializing {
        InitializationState::Retiring(owner) => owner.clone(),
        InitializationState::Initializing => return Err(LoggingError::AdmissionBusy),
        InitializationState::Idle => return Err(LoggingError::NotInitialized),
    };
    drop(initializing);
    LoggingShutdown::retiring(
        owner.ticket,
        &owner.quota,
        owner.path_bytes,
        LoggingError::WorkerStopped,
    )
}

/// Counters never log recursively, including admission loss and filesystem errors.
pub fn health() -> Option<LoggingHealth> {
    PROCESS_LOGGER.get().map(|state| state.service.health())
}

/// Reports whether this process currently accepts diagnostic events. Runtime
/// producers use this to avoid collecting expensive structured evidence when
/// neither the file sink nor another opt-in diagnostics surface needs it.
pub fn is_enabled() -> bool {
    PROCESS_LOGGER
        .get()
        .is_some_and(|logger| logger.enabled.load(Ordering::Acquire))
}

/// Returns the exact file selected for this server lifetime.
pub fn log_path() -> Option<&'static Path> {
    PROCESS_LOGGER.get().map(|logger| logger.path.as_path())
}

/// Starts the session's single log-ingress owner. Must run inside the server's
/// already-admitted async runtime before it publishes readiness to clients.
pub async fn start_log_relay(log_path: &Path) -> Result<LogRelayServer, LoggingError> {
    let state = PROCESS_LOGGER
        .get()
        .cloned()
        .ok_or(LoggingError::NotInitialized)?;
    LogRelayServer::start(state, LogRelayEndpoint::for_log_path(log_path)).await
}

/// Removes URL user-info, every query value, and any fragment before an
/// endpoint enters a diagnostic event. Provider and proxy URLs are
/// user-configurable and may embed tokens in all three places even when the
/// normal built-in endpoints do not.
pub fn redacted_url(url: &str) -> String {
    // A fragment follows the query in a well-formed URL, so it has to come off
    // first -- and it has to come off at all, because a redirect URL carrying
    // `#access_token=...` has no `?` for the query branch below to catch.
    let (before_fragment, had_fragment) = match url.split_once('#') {
        Some((base, _fragment)) => (base, true),
        None => (url, false),
    };
    let (without_query, had_query) = match before_fragment.split_once('?') {
        Some((base, _query)) => (base, true),
        None => (before_fragment, false),
    };
    let mut redacted = redacted_user_info(without_query);
    if had_query {
        redacted.push_str("?<redacted>");
    }
    if had_fragment {
        redacted.push_str("#<redacted>");
    }
    redacted
}

/// Replaces a `user:password@` prefix on the authority of a URL whose query
/// and fragment have already been removed.
///
/// The authority is redacted whether or not a scheme is present. A
/// user-configured proxy is the one endpoint that routinely carries
/// credentials there, it is commonly written scheme-less
/// (`user:password@proxy.example:8080`), and a scheme-less proxy string is
/// also the shape most likely to be rejected and end up quoted verbatim in a
/// `GatewayError::InvalidProxy` message.
fn redacted_user_info(url_without_query: &str) -> String {
    let (scheme_prefix, authority_and_path) = match url_without_query.split_once("://") {
        Some((scheme, remainder)) => (format!("{scheme}://"), remainder),
        None => (String::new(), url_without_query),
    };
    // `/` is ASCII, so this byte index is always a character boundary and the
    // split below cannot land inside a multi-byte character.
    let authority_end = authority_and_path
        .find('/')
        .unwrap_or(authority_and_path.len());
    let (authority, path) = authority_and_path.split_at(authority_end);
    match authority.rsplit_once('@') {
        Some((_user_info, host)) => format!("{scheme_prefix}<redacted>@{host}{path}"),
        None => url_without_query.to_owned(),
    }
}

/// Redacts credential-bearing HTTP header values while leaving request IDs,
/// rate-limit state, content metadata, and other debugging headers intact.
pub fn redacted_header_value(name: &str, value: &str) -> String {
    if is_credential_name(name) {
        "<redacted>".to_owned()
    } else {
        value.to_owned()
    }
}

/// Keeps only response headers that help diagnose payload shape, request
/// correlation, throttling, or retry behavior. Browser-security policy
/// headers can be many kilobytes long and add no value to an API-call log.
pub fn is_diagnostic_response_header(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "content-type"
            | "content-length"
            | "retry-after"
            | "request-id"
            | "x-request-id"
            | "openai-request-id"
            | "cf-ray"
    ) || normalized.starts_with("x-ratelimit-")
}

/// Reads only the simple `[debug] file_logging_enabled = <bool>` contract
/// before either process parses the complete configuration. This lets an
/// enabled diagnostic file capture errors in unrelated malformed sections.
pub fn file_logging_enabled_hint(config_path: &Path) -> io::Result<Option<bool>> {
    let contents = match std::fs::read_to_string(config_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut is_debug_section = false;
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') {
            is_debug_section = line == "[debug]";
            continue;
        }
        if !is_debug_section || line.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "file_logging_enabled" {
            continue;
        }
        let value = raw_value.split('#').next().unwrap_or_default().trim();
        return Ok(match value {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        });
    }
    Ok(None)
}

/// Clones a diagnostic JSON payload while removing credential values from
/// ordinary fields, path-addressed settings writes, and stringified tool
/// arguments/results. Prompts and non-secret tool data remain complete.
pub fn redacted_json_credentials(value: &serde_json::Value) -> serde_json::Value {
    let mut redacted = value.clone();
    redact_json_credentials_in_place(&mut redacted);
    redacted
}

/// Redacts one structured diagnostic field only when its label is itself a
/// credential name. Agent lifecycle fields use a label/value representation,
/// so passing the enclosing JSON through [`redacted_json_credentials`] cannot
/// infer that a generic `value` belongs to an `api_key` label.
pub fn redacted_diagnostic_field_value(label: &str, value: &str) -> String {
    if is_credential_name(label) {
        "<redacted>".to_string()
    } else {
        value.to_string()
    }
}

/// Applies [`redacted_json_credentials`] to a stringified JSON value. Invalid
/// JSON has no reliable field boundary for selective redaction, so diagnostics
/// retain its size without risking disclosure of an unstructured credential.
pub fn redacted_json_string_credentials(value: &str) -> String {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) else {
        return format!("<invalid JSON omitted: {} bytes>", value.len());
    };
    redacted_json_credentials(&parsed).to_string()
}

fn redact_json_credentials_in_place(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            // Settings tools address fields through a separate `path`, so the
            // generic `value` key becomes secret only in this object shape.
            let targets_credential_path = object
                .get("path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(is_credential_path);
            if targets_credential_path && object.contains_key("value") {
                object.insert(
                    "value".to_owned(),
                    serde_json::Value::String("<redacted>".to_owned()),
                );
            }

            for (field, nested_value) in object {
                if is_credential_name(field) {
                    *nested_value = serde_json::Value::String("<redacted>".to_owned());
                    continue;
                }
                // Realtime tool arguments and outputs are JSON encoded inside
                // protocol strings, so recurse through that second envelope.
                if matches!(field.as_str(), "arguments" | "output" | "body" | "result") {
                    if let Some(serialized) = nested_value.as_str() {
                        *nested_value =
                            serde_json::Value::String(redacted_json_string_credentials(serialized));
                        continue;
                    }
                }
                redact_json_credentials_in_place(nested_value);
            }
        }
        serde_json::Value::Array(values) => {
            for nested_value in values {
                redact_json_credentials_in_place(nested_value);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

fn is_credential_path(path: &str) -> bool {
    path.rsplit(['.', '/'])
        .next()
        .is_some_and(is_credential_name)
}

/// Every name whose *value* is a credential, written in the normalized form
/// [`is_credential_name`] produces: lowercase, with every separator removed.
///
/// One registry serves all four surfaces that ask the question -- JSON fields,
/// settings-path segments, HTTP header names, and human-readable diagnostic
/// labels -- so that a name is never redacted on one surface and logged in
/// full on another. Two separate lists had already drifted apart that way:
/// `x-auth-token` was redacted as a header while an `auth_token` JSON field
/// was not, and `password` was redacted as a field while a `password` header
/// was not.
const CREDENTIAL_NAMES: &[&str] = &[
    "apikey",
    "authorization",
    "proxyauthorization",
    "password",
    "secret",
    "clientsecret",
    "accesstoken",
    "refreshtoken",
    "idtoken",
    "authtoken",
    "token",
    "cookie",
    "setcookie",
    "xapikey",
    "xgoogapikey",
    "xauthtoken",
    "xaccesstoken",
];

/// True when `name` itself names a credential, in any spelling the same secret
/// is written in across a JSON field, a settings path, an HTTP header, and a
/// diagnostic label: `api_key`, `api-key`, `apiKey`, `API key`, `APIKEY`.
///
/// Dropping separators entirely, rather than folding them onto `_`, is what
/// makes the camelCase spelling match -- and camelCase is the dominant one in
/// JSON bodies, so `accessToken`, `refreshToken`, and `clientSecret` were
/// previously written to the log in full because only their snake_case
/// spellings were listed. Separator-free comparison keeps the registry to one
/// entry per secret instead of one entry per spelling.
fn is_credential_name(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect();
    CREDENTIAL_NAMES.contains(&normalized.as_str())
}

/// Records panics in detached/server and alternate-screen/client processes,
/// where the default stderr hook is not a durable diagnostic surface.
pub fn install_panic_logging() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        tracing::error!(
            panic = %panic_info,
            backtrace = %std::backtrace::Backtrace::force_capture(),
            "process panicked"
        );
        previous_hook(panic_info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer(state: &Arc<LoggerState>) -> DynamicFileWriter {
        DynamicFileWriter {
            state: Arc::clone(state),
            event_buffer: Vec::new(),
            overflowed: false,
        }
    }

    #[test]
    fn oversized_event_is_dropped_whole_and_later_events_can_be_saved() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("bounded.log");
        let state = Arc::new(LoggerState::new(path.clone()).expect("worker"));
        state.set_enabled(true).expect("enable");
        let mut oversized = writer(&state);
        oversized
            .write_all(b"prefix must disappear")
            .expect("prefix");
        oversized
            .write_all(&vec![b'x'; MAX_EVENT_BYTES])
            .expect("oversized event");
        drop(oversized);
        writeln!(writer(&state), "later event").expect("event");
        state
            .service
            .flush()
            .expect("admission")
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect("flush");
        assert_eq!(
            std::fs::read_to_string(path).expect("readback"),
            "later event\n"
        );
        assert_eq!(state.service.health().dropped_events, 1);
        assert_eq!(state.service.health().retained_bytes, 0);
    }

    #[test]
    fn retained_byte_exhaustion_discards_a_multichunk_event_without_writing_its_prefix() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("whole-event.log");
        let state = Arc::new(LoggerState::new(path.clone()).expect("worker"));
        state.set_enabled(true).expect("enable");
        assert!(state.service.reserve(MAX_RETAINED_BYTES));
        let mut refused = writer(&state);
        refused
            .write_all(b"first chunk must disappear")
            .expect("formatter accepts bytes");
        refused
            .write_all(b"second chunk must disappear")
            .expect("whole event refused");
        drop(refused);
        assert_eq!(state.service.health().dropped_events, 1);
        state.service.release(MAX_RETAINED_BYTES);
        let mut accepted = writer(&state);
        accepted.write_all(b"retained ").expect("first chunk");
        accepted
            .write_all(b"complete event\n")
            .expect("second chunk");
        drop(accepted);
        state
            .service
            .shutdown_joined()
            .expect("ordered close")
            .wait_until(std::time::Instant::now() + std::time::Duration::from_secs(5))
            .expect("native join")
            .into_result()
            .expect("flush");
        assert_eq!(
            std::fs::read_to_string(path).expect("readback"),
            "retained complete event\n"
        );
        assert_eq!(state.service.health().retained_bytes, 0);
    }

    #[test]
    fn repeated_formatter_flush_and_drop_never_duplicate_an_event() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("once.log");
        let state = Arc::new(LoggerState::new(path.clone()).expect("worker"));
        state.set_enabled(true).expect("enable");
        let mut event = writer(&state);
        event.write_all(b"one event\n").expect("event");
        event.flush().expect("first enqueue");
        event.flush().expect("empty enqueue");
        drop(event);
        state
            .service
            .shutdown()
            .expect("admission")
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect("shutdown");
        assert_eq!(
            std::fs::read_to_string(path).expect("readback"),
            "one event\n"
        );
        assert!(matches!(
            state.service.flush(),
            Err(LoggingError::WorkerStopped)
        ));
    }

    #[test]
    fn disable_acknowledges_prior_event_before_closing_and_reenable_appends() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("toggles.log");
        let state = Arc::new(LoggerState::new(path.clone()).expect("worker"));
        state.set_enabled(true).expect("enable");
        writer(&state)
            .write_all(b"disable boundary\n")
            .expect("event");
        state
            .service
            .enable(false)
            .expect("admission")
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect("disable");
        assert_eq!(
            std::fs::read_to_string(&path).expect("readback"),
            "disable boundary\n"
        );
        writer(&state).write_all(b"disabled event\n").expect("sink");
        state.set_enabled(true).expect("reenable");
        writer(&state).write_all(b"enabled again\n").expect("event");
        state
            .service
            .shutdown()
            .expect("admission")
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect("shutdown");
        assert_eq!(
            std::fs::read_to_string(path).expect("readback"),
            "disable boundary\nenabled again\n"
        );
    }

    #[test]
    #[ignore = "manual performance benchmark"]
    fn benchmark_event_file_writes() {
        const ITERATIONS: usize = 10_000;
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("benchmark.log");
        let state = Arc::new(LoggerState::new(path).expect("logger worker"));
        state.set_enabled(true).expect("enable");

        let started_at = std::time::Instant::now();
        for event_number in 0..ITERATIONS {
            let mut writer = DynamicFileWriter {
                state: Arc::clone(&state),
                event_buffer: Vec::new(),
                overflowed: false,
            };
            writeln!(writer, "event {event_number}: diagnostic payload").expect("event");
        }
        state.set_enabled(false).expect("disable and flush");
        let elapsed = started_at.elapsed();
        println!(
            "PERF logging.event_write average_ns={}",
            elapsed.as_nanos() / ITERATIONS as u128,
        );
    }

    #[test]
    fn disabled_state_does_not_create_a_file_until_enabled() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("debug.txt");
        let state = LoggerState::new(path.clone()).expect("logger worker");

        assert!(!path.exists());
        state.set_enabled(true).expect("enable");
        assert!(path.exists());
        state.set_enabled(false).expect("disable");
        assert!(!state.enabled.load(Ordering::Acquire));
    }

    #[test]
    fn dynamic_writer_appends_only_while_enabled() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("debug.txt");
        let state = Arc::new(LoggerState::new(path.clone()).expect("logger worker"));
        let mut writer = DynamicFileWriter {
            state: Arc::clone(&state),
            event_buffer: Vec::new(),
            overflowed: false,
        };

        writer.write_all(b"ignored\n").expect("disabled write");
        state.set_enabled(true).expect("enable");
        writer.write_all(b"kept\n").expect("enabled write");
        writer.flush().expect("enqueue");
        state
            .service
            .flush()
            .expect("flush admission")
            .wait_timeout(std::time::Duration::from_secs(5))
            .expect("flush acknowledgement");

        assert_eq!(std::fs::read_to_string(path).expect("log"), "kept\n");
    }

    /// Refusing to follow a symlink is an `O_NOFOLLOW` guarantee, which only
    /// Unix provides; on Windows the equivalent protection comes from the log
    /// directory's ACL rather than from the open itself, so there is nothing
    /// for this test to assert there. See `ilium_platform::secure_fs`.
    #[cfg(unix)]
    #[test]
    fn enabling_refuses_a_symlink_log_target() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("target.txt");
        let path = directory.path().join("diagnostic.txt");
        std::fs::write(&target, "private target").expect("target");
        std::os::unix::fs::symlink(&target, &path).expect("symlink");
        let state = LoggerState::new(path).expect("logger worker");

        assert!(matches!(
            state.set_enabled(true),
            Err(LoggingError::PrepareFile { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(target).expect("unchanged target"),
            "private target"
        );
    }

    #[test]
    fn independent_process_style_writers_append_without_overwriting_each_other() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("shared.txt");
        let first_state = Arc::new(LoggerState::new(path.clone()).expect("logger worker"));
        let second_state = Arc::new(LoggerState::new(path.clone()).expect("logger worker"));
        first_state.set_enabled(true).expect("first writer");
        second_state.set_enabled(true).expect("second writer");

        let first_handle = std::thread::spawn(move || {
            for line in 0..100 {
                let mut writer = DynamicFileWriter {
                    state: Arc::clone(&first_state),
                    event_buffer: Vec::new(),
                    overflowed: false,
                };
                writeln!(writer, "client-{line}").expect("client append");
            }
            first_state
                .service
                .flush()
                .expect("admission")
                .wait_timeout(std::time::Duration::from_secs(5))
                .expect("flush");
        });
        let second_handle = std::thread::spawn(move || {
            for line in 0..100 {
                let mut writer = DynamicFileWriter {
                    state: Arc::clone(&second_state),
                    event_buffer: Vec::new(),
                    overflowed: false,
                };
                writeln!(writer, "server-{line}").expect("server append");
            }
            second_state
                .service
                .flush()
                .expect("admission")
                .wait_timeout(std::time::Duration::from_secs(5))
                .expect("flush");
        });
        first_handle.join().expect("client writer thread");
        second_handle.join().expect("server writer thread");

        let contents = std::fs::read_to_string(path).expect("shared log");
        // Collecting into a `HashSet` before counting would silently absorb a
        // duplicated append (two writers flushing the same buffered event) --
        // exactly the failure mode this test exists to catch per the
        // `DynamicFileWriter` doc comment -- so the length check must run
        // against the raw line count first.
        let raw_lines: Vec<&str> = contents.lines().collect();
        assert_eq!(raw_lines.len(), 200);
        let lines = raw_lines
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        assert!(lines.contains("client-0"));
        assert!(lines.contains("client-99"));
        assert!(lines.contains("server-0"));
        assert!(lines.contains("server-99"));
    }

    #[test]
    fn diagnostic_urls_remove_user_info_and_query_values() {
        assert_eq!(
            redacted_url("https://user:secret@example.test/v1/chat?api_key=hidden&model=x"),
            "https://<redacted>@example.test/v1/chat?<redacted>"
        );
        assert_eq!(
            redacted_url("http://127.0.0.1:11434/api/tags"),
            "http://127.0.0.1:11434/api/tags"
        );
        // A rejected proxy string is quoted into `GatewayError::InvalidProxy`,
        // and the shape most likely to be rejected is the scheme-less one --
        // which is also where a proxy password lives.
        assert_eq!(
            redacted_url("user:secret@proxy.example:8080"),
            "<redacted>@proxy.example:8080"
        );
        // A fragment carries no diagnostic value in an API endpoint and can
        // carry an implicit-flow token, so it is never echoed.
        assert_eq!(
            redacted_url("https://example.test/callback#access_token=hidden"),
            "https://example.test/callback#<redacted>"
        );
    }

    #[test]
    fn credential_headers_are_redacted_without_hiding_request_metadata() {
        assert_eq!(
            redacted_header_value("Authorization", "Bearer secret"),
            "<redacted>"
        );
        assert_eq!(
            redacted_header_value("Set-Cookie", "session=secret"),
            "<redacted>"
        );
        assert_eq!(
            redacted_header_value("X-Goog-Api-Key", "secret"),
            "<redacted>"
        );
        assert_eq!(
            redacted_header_value("x-request-id", "request-42"),
            "request-42"
        );
        // Names the JSON-field list carried but the header list did not, back
        // when the two lists were maintained separately.
        assert_eq!(redacted_header_value("Password", "secret"), "<redacted>");
        assert_eq!(
            redacted_header_value("x-ratelimit-remaining-tokens", "17"),
            "17"
        );
    }

    #[test]
    fn diagnostic_response_headers_keep_correlation_and_rate_limit_data_only() {
        assert!(is_diagnostic_response_header("Content-Type"));
        assert!(is_diagnostic_response_header("X-Request-ID"));
        assert!(is_diagnostic_response_header(
            "x-ratelimit-remaining-requests"
        ));
        assert!(!is_diagnostic_response_header("Content-Security-Policy"));
        assert!(!is_diagnostic_response_header("Permissions-Policy"));
    }

    #[test]
    fn debug_setting_hint_survives_an_unrelated_malformed_section() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config.toml");
        std::fs::write(
            &path,
            "[debug]\nfile_logging_enabled = true # keep diagnostics\n\n[broken\n",
        )
        .expect("config fixture");

        assert_eq!(file_logging_enabled_hint(&path).expect("hint"), Some(true));
        assert_eq!(
            file_logging_enabled_hint(&directory.path().join("missing.toml")).expect("missing"),
            None
        );
    }

    #[test]
    fn diagnostic_json_redacts_direct_path_addressed_and_stringified_credentials() {
        let payload = serde_json::json!({
            "api_key": "direct-secret",
            "tool": {
                "arguments": "{\"path\":\"voice.api_key\",\"value\":\"path-secret\",\"action\":\"kept\"}"
            },
            "nested": [{"access-token": "nested-secret", "message": "complete text"}],
            // camelCase is the dominant spelling in JSON bodies, and every one
            // of these used to be logged verbatim.
            "accessToken": "camel-secret",
            "clientSecret": "camel-client-secret",
            "max_tokens": 512,
        });

        let redacted = redacted_json_credentials(&payload);
        let diagnostic = redacted.to_string();

        assert!(!diagnostic.contains("direct-secret"));
        assert!(!diagnostic.contains("path-secret"));
        assert!(!diagnostic.contains("nested-secret"));
        assert!(!diagnostic.contains("camel-secret"));
        assert!(!diagnostic.contains("camel-client-secret"));
        assert!(diagnostic.contains("<redacted>"));
        assert!(diagnostic.contains("kept"));
        assert!(diagnostic.contains("complete text"));
        // A name that merely contains a credential word is not one: token
        // budgets must stay readable in the log.
        assert!(diagnostic.contains("512"));
    }

    #[test]
    fn labelled_diagnostic_fields_redact_credentials_but_keep_lifecycle_evidence() {
        assert_eq!(
            redacted_diagnostic_field_value("API key", "field-secret"),
            "<redacted>"
        );
        assert_eq!(
            redacted_diagnostic_field_value("submitted input", "/clear"),
            "/clear"
        );
        assert_eq!(
            redacted_diagnostic_field_value("session ID before invalidation", "session-old"),
            "session-old"
        );
    }

    #[test]
    fn malformed_json_is_omitted_instead_of_returned_verbatim() {
        let input = "{\"api_key\":\"unclosed-secret";
        let diagnostic = redacted_json_string_credentials(input);

        assert_eq!(
            diagnostic,
            format!("<invalid JSON omitted: {} bytes>", input.len())
        );
        assert!(!diagnostic.contains("unclosed-secret"));
    }
}
