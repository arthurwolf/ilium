//! Process-wide, dynamically switchable file diagnostics for ilium.
//!
//! The detached server owns one timestamped path for its complete lifetime;
//! every attached client receives that same path from the CLI and appends to
//! it. A single tracing subscriber per process routes existing and new
//! `tracing` events through this boundary. Disabling logging closes the file
//! immediately and turns writes into a sink without changing call sites.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tracing_subscriber::filter::{FilterExt, LevelFilter};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

static PROCESS_LOGGER: OnceLock<Arc<LoggerState>> = OnceLock::new();

/// Failures that prevent this process from providing the requested log.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("file logging is already initialized in this process")]
    AlreadyInitialized,
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
}

/// Mutable output state shared by the tracing writer and the Debug setting.
struct LoggerState {
    path: PathBuf,
    enabled: AtomicBool,
    file: Mutex<Option<File>>,
}

impl LoggerState {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            enabled: AtomicBool::new(false),
            file: Mutex::new(None),
        }
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), LoggingError> {
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if enabled && file.is_none() {
            let parent = self
                .path
                .parent()
                .ok_or_else(|| LoggingError::PrepareFile {
                    path: self.path.clone(),
                    source: io::Error::new(io::ErrorKind::InvalidInput, "log path has no parent"),
                })?;
            std::fs::create_dir_all(parent).map_err(|source| LoggingError::PrepareFile {
                path: self.path.clone(),
                source,
            })?;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).map_err(
                |source| LoggingError::PrepareFile {
                    path: self.path.clone(),
                    source,
                },
            )?;
            let opened_file = OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
                .open(&self.path)
                .map_err(|source| LoggingError::PrepareFile {
                    path: self.path.clone(),
                    source,
                })?;
            opened_file
                .set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|source| LoggingError::PrepareFile {
                    path: self.path.clone(),
                    source,
                })?;
            *file = Some(opened_file);
        } else if !enabled {
            *file = None;
        }
        self.enabled.store(enabled, Ordering::Release);
        // `filter_fn` suppresses expensive diagnostic field construction, but
        // caches each call site's interest. Rebuild after every live toggle
        // so a call site first seen while disabled becomes observable on
        // opt-in and stops evaluating fields again after opt-out.
        drop(file);
        tracing::callsite::rebuild_interest_cache();
        Ok(())
    }
}

/// A cloning writer lets tracing format one complete event while the setting
/// may be changed by another runtime task. Each physical write is serialized
/// inside this process; `O_APPEND` keeps server/client appends from overwriting
/// each other when both processes target the same file.
#[derive(Clone)]
struct DynamicFileWriter {
    state: Arc<LoggerState>,
    event_buffer: Vec<u8>,
}

impl Write for DynamicFileWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.state.enabled.load(Ordering::Acquire) {
            return Ok(bytes.len());
        }
        // `tracing-subscriber` formats one event through several `write`
        // calls. Buffer them so two processes sharing the file can append one
        // complete event rather than interleaving formatting fragments.
        self.event_buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.event_buffer.is_empty() {
            return Ok(());
        }
        if !self.state.enabled.load(Ordering::Acquire) {
            self.event_buffer.clear();
            return Ok(());
        }
        let mut file = self
            .state
            .file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(file) = file.as_mut() {
            file.write_all(&self.event_buffer)?;
            file.flush()?;
        }
        self.event_buffer.clear();
        Ok(())
    }
}

impl Drop for DynamicFileWriter {
    fn drop(&mut self) {
        // The formatting layer owns one writer per event and does not expose
        // write failures to the application. Best-effort drop flushing keeps
        // that contract while preserving each event as one append operation.
        let _ = self.flush();
    }
}

/// Installs this process's one tracing subscriber and optionally opens `path`.
/// No file is created while `enabled` is false, preserving the normal-user
/// default even though every call site remains instrumented.
pub fn initialize(
    path: impl Into<PathBuf>,
    enabled: bool,
    process_role: &'static str,
) -> Result<(), LoggingError> {
    let path = path.into();
    if let Some(state) = PROCESS_LOGGER.get() {
        if state.path != path {
            return Err(LoggingError::AlreadyInitialized);
        }
        state.set_enabled(enabled)?;
        tracing::info!(
            process_role,
            process_id = std::process::id(),
            "process logging reused for an in-process role handoff"
        );
        return Ok(());
    }

    let state = Arc::new(LoggerState::new(path));
    state.set_enabled(enabled)?;
    let writer_state = Arc::clone(&state);
    let filter_state = Arc::clone(&state);
    let configured_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // Debug logging promises the complete info-level diagnostic contract even
    // when the parent shell has a narrower `RUST_LOG`; an explicit broader
    // filter can still opt into debug/trace traffic for deeper investigation.
    let verbosity_filter = LevelFilter::INFO.or(configured_filter);
    let enabled_filter = tracing_subscriber::filter::filter_fn(move |_metadata| {
        filter_state.enabled.load(Ordering::Acquire)
    });
    let formatting_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(move || DynamicFileWriter {
            state: Arc::clone(&writer_state),
            event_buffer: Vec::new(),
        })
        .with_filter(verbosity_filter.and(enabled_filter));
    tracing_subscriber::registry()
        .with(formatting_layer)
        .try_init()
        .map_err(|error| LoggingError::InstallSubscriber(error.to_string()))?;
    PROCESS_LOGGER
        .set(state)
        .map_err(|_| LoggingError::AlreadyInitialized)?;
    tracing::info!(
        process_role,
        process_id = std::process::id(),
        "process logging initialized"
    );
    Ok(())
}

/// Applies the live Debug setting to the current process.
pub fn set_enabled(enabled: bool) -> Result<(), LoggingError> {
    PROCESS_LOGGER
        .get()
        .ok_or(LoggingError::NotInitialized)?
        .set_enabled(enabled)
}

/// Returns the exact file selected for this server lifetime.
pub fn log_path() -> Option<&'static Path> {
    PROCESS_LOGGER.get().map(|logger| logger.path.as_path())
}

/// Removes URL user-info and every query value before an endpoint enters a
/// diagnostic event. Provider URLs are user-configurable and may embed tokens
/// even when the normal built-in endpoints do not.
pub fn redacted_url(url: &str) -> String {
    let (without_query, had_query) = match url.split_once('?') {
        Some((base, _query)) => (base, true),
        None => (url, false),
    };
    let without_user_info = match without_query.split_once("://") {
        Some((scheme, remainder)) => {
            let authority_end = remainder.find('/').unwrap_or(remainder.len());
            let (authority, path) = remainder.split_at(authority_end);
            match authority.rsplit_once('@') {
                Some((_user_info, host)) => format!("{scheme}://<redacted>@{host}{path}"),
                None => without_query.to_owned(),
            }
        }
        None => without_query.to_owned(),
    };
    if had_query {
        format!("{without_user_info}?<redacted>")
    } else {
        without_user_info
    }
}

/// Redacts credential-bearing HTTP header values while leaving request IDs,
/// rate-limit state, content metadata, and other debugging headers intact.
pub fn redacted_header_value(name: &str, value: &str) -> String {
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "api-key"
            | "x-api-key"
            | "x-goog-api-key"
            | "x-auth-token"
            | "x-access-token"
    ) {
        "<redacted>".to_owned()
    } else {
        value.to_owned()
    }
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
                if is_credential_field(field) {
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
        .is_some_and(is_credential_field)
}

fn is_credential_field(field: &str) -> bool {
    let normalized = field.to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "api_key"
            | "apikey"
            | "authorization"
            | "proxy_authorization"
            | "password"
            | "secret"
            | "client_secret"
            | "access_token"
            | "refresh_token"
            | "id_token"
            | "token"
            | "cookie"
            | "set_cookie"
            | "x_api_key"
    )
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

    #[test]
    fn disabled_state_does_not_create_a_file_until_enabled() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("debug.txt");
        let state = LoggerState::new(path.clone());

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
        let state = Arc::new(LoggerState::new(path.clone()));
        let mut writer = DynamicFileWriter {
            state: Arc::clone(&state),
            event_buffer: Vec::new(),
        };

        writer.write_all(b"ignored\n").expect("disabled write");
        state.set_enabled(true).expect("enable");
        writer.write_all(b"kept\n").expect("enabled write");
        writer.flush().expect("flush");

        assert_eq!(std::fs::read_to_string(path).expect("log"), "kept\n");
    }

    #[test]
    fn enabling_refuses_a_symlink_log_target() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("target.txt");
        let path = directory.path().join("diagnostic.txt");
        std::fs::write(&target, "private target").expect("target");
        std::os::unix::fs::symlink(&target, &path).expect("symlink");
        let state = LoggerState::new(path);

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
        let first_state = Arc::new(LoggerState::new(path.clone()));
        let second_state = Arc::new(LoggerState::new(path.clone()));
        first_state.set_enabled(true).expect("first writer");
        second_state.set_enabled(true).expect("second writer");

        let first_handle = std::thread::spawn(move || {
            for line in 0..100 {
                let mut writer = DynamicFileWriter {
                    state: Arc::clone(&first_state),
                    event_buffer: Vec::new(),
                };
                writeln!(writer, "client-{line}").expect("client append");
            }
        });
        let second_handle = std::thread::spawn(move || {
            for line in 0..100 {
                let mut writer = DynamicFileWriter {
                    state: Arc::clone(&second_state),
                    event_buffer: Vec::new(),
                };
                writeln!(writer, "server-{line}").expect("server append");
            }
        });
        first_handle.join().expect("client writer thread");
        second_handle.join().expect("server writer thread");

        let lines = std::fs::read_to_string(path)
            .expect("shared log")
            .lines()
            .map(str::to_owned)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(lines.len(), 200);
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
        });

        let redacted = redacted_json_credentials(&payload);
        let diagnostic = redacted.to_string();

        assert!(!diagnostic.contains("direct-secret"));
        assert!(!diagnostic.contains("path-secret"));
        assert!(!diagnostic.contains("nested-secret"));
        assert!(diagnostic.contains("<redacted>"));
        assert!(diagnostic.contains("kept"));
        assert!(diagnostic.contains("complete text"));
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
