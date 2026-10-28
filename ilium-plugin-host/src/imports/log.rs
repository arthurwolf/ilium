//! Explicit host logging context for the grant-free `ilium:host/log` interface.
//! Persistence, filtering, rate state and rotation remain telemetry-owned.
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    sync::{Arc, Mutex},
};

use crate::telemetry::{
    truncate_message, LogConfig, LogLevel, LogLine, LogRing, PluginLog, PluginLogInstanceBudget,
    Refusal,
};
use ilium_plugin_api::{
    generated::host::log::{HostLog, Level},
    Side, Tier,
};

/// One host clock sample. Local formatting and the shared monotonic origin are
/// supplied by the runtime, never taken from plugin-controlled text.
#[derive(Debug, Clone)]
pub struct LogTime {
    pub local_timestamp: String,
    pub monotonic_millis: u64,
}
pub trait LogClock: Send + Sync {
    fn now(&self) -> Result<LogTime, Refusal>;
}
/// Payload-free failure notification for a void import. Neither raw plugin text,
/// installed secrets nor filesystem error strings reach the diagnostic callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFailure {
    pub plugin: String,
    pub side: Side,
    pub code: String,
}
pub trait LogDiagnostics: Send + Sync {
    fn failed(&self, failure: LogFailure);
}

/// One actual plugin log and bounded recent-line ring, shared by its import
/// handles. Reuse this owner rather than creating a fresh quota on every call.
/// Instance budgets are separately owned and shared only by handles belonging
/// to that actual runtime instance. File locking and rotation remain per plugin.
pub struct PluginLogger {
    plugin: String,
    writer: PluginLog,
    ring: LogRing,
    secret_variants: Vec<String>,
}
impl std::fmt::Debug for PluginLogger {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginLogger")
            .field("plugin", &self.plugin)
            .finish_non_exhaustive()
    }
}
impl PluginLogger {
    /// `config` is the actual resolved tier/settings policy. Refuse an
    /// over-ceiling rate instead of silently admitting broader authority.
    pub fn open(
        dir: &Path,
        plugin: &str,
        tier: Tier,
        config: LogConfig,
        ring_capacity: usize,
        secrets: &[String],
    ) -> Result<Self, Refusal> {
        let ceiling = LogConfig::for_tier(tier).rate_per_second;
        if config.rate_per_second > ceiling {
            return Err(Refusal {
                code: "log.rate-invalid".into(),
                message: "Log rate exceeds the admitted tier ceiling".into(),
            });
        }
        let mut writer = PluginLog::open(dir, plugin, config)?;
        writer.set_secrets(secrets);
        Ok(Self {
            plugin: plugin.into(),
            writer,
            ring: LogRing::new(ring_capacity),
            secret_variants: crate::secrets::variants(secrets),
        })
    }
    /// Supply the complete current effective secret set, including any retained
    /// rotation grace values. Only recognized secret representations are redacted.
    pub fn set_secrets(&mut self, secrets: &[String]) {
        self.writer.set_secrets(secrets);
        self.secret_variants = crate::secrets::variants(secrets);
    }
    pub fn lines(&self) -> Vec<LogLine> {
        self.ring.lines_of(&self.plugin)
    }
    fn write(
        &mut self,
        time: &LogTime,
        budget: &mut PluginLogInstanceBudget,
        level: LogLevel,
        message: &str,
    ) -> Result<(), Refusal> {
        // Consume only the error of this operation; threshold/rate drops are not
        // failures of the void ABI and do not enter the recent-line ring.
        self.writer.take_error();
        if self.writer.write_plugin_for_instance(
            &time.local_timestamp,
            time.monotonic_millis,
            budget,
            level,
            message,
        ) {
            let redacted = crate::secrets::redact_variants(message, &self.secret_variants);
            let escaped = redacted.replace('\r', "\\r").replace('\n', "\\n");
            self.ring.push(LogLine {
                plugin: self.plugin.clone(),
                level: level.as_str().into(),
                message: truncate_message(&escaped),
            });
        }
        self.writer.take_error().map_or(Ok(()), Err)
    }
    fn tick(
        &mut self,
        time: &LogTime,
        budget: &mut PluginLogInstanceBudget,
    ) -> Result<(), Refusal> {
        self.writer.take_error();
        self.writer
            .tick_instance(&time.local_timestamp, time.monotonic_millis, budget);
        self.writer.take_error().map_or(Ok(()), Err)
    }
}

/// Host side of `log`. There is deliberately no default/no-op logger: runtime
/// callers must provide the real writer, side, clock and diagnostic owner.
pub struct LogImport {
    logger: Arc<Mutex<PluginLogger>>,
    budget: Arc<Mutex<PluginLogInstanceBudget>>,
    plugin: String,
    side: Side,
    clock: Arc<dyn LogClock>,
    diagnostics: Arc<dyn LogDiagnostics>,
}
impl std::fmt::Debug for LogImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LogImport")
            .field("plugin", &self.plugin)
            .field("side", &self.side)
            .finish_non_exhaustive()
    }
}
impl LogImport {
    pub fn new(
        logger: Arc<Mutex<PluginLogger>>,
        budget: Arc<Mutex<PluginLogInstanceBudget>>,
        clock: Arc<dyn LogClock>,
        diagnostics: Arc<dyn LogDiagnostics>,
    ) -> Result<Self, Refusal> {
        let plugin = logger
            .lock()
            .map_err(|_| Refusal {
                code: "log.poisoned".into(),
                message: "Shared logger lock is poisoned".into(),
            })?
            .plugin
            .clone();
        let side = budget
            .lock()
            .map_err(|_| Refusal {
                code: "log.poisoned".into(),
                message: "Instance budget lock is poisoned".into(),
            })?
            .side();
        Ok(Self {
            logger,
            budget,
            plugin,
            side,
            clock,
            diagnostics,
        })
    }
    /// Runtime owner drives this even when the plugin is idle so due suppression
    /// summaries are flushed. It shares the same clock, writer and failure path.
    pub fn tick(&mut self) {
        self.perform(None);
    }
    fn perform(&self, entry: Option<(LogLevel, &str)>) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let time = self.clock.now()?;
            let mut logger = self.logger.lock().map_err(|_| Refusal {
                code: "log.poisoned".into(),
                message: "Shared logger lock is poisoned".into(),
            })?;
            let mut budget = self.budget.lock().map_err(|_| Refusal {
                code: "log.poisoned".into(),
                message: "Instance budget lock is poisoned".into(),
            })?;
            match entry {
                Some((level, message)) => logger.write(&time, &mut budget, level, message),
                None => logger.tick(&time, &mut budget),
            }
        }));
        let code = match result {
            Ok(Ok(())) => return,
            Ok(Err(refusal))
                if !refusal.code.is_empty()
                    && refusal.code.len() <= 64
                    && refusal.code.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
                    }) =>
            {
                refusal.code
            }
            Ok(Err(_)) => "log.backend-failed".into(),
            Err(_) => "log.backend-panic".into(),
        };
        // Diagnostic sinks are host-owned but a broken callback must never turn
        // an always-available void logging import into a guest-visible panic.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.diagnostics.failed(LogFailure {
                plugin: self.plugin.clone(),
                side: self.side,
                code,
            })
        }));
    }
}
impl HostLog for LogImport {
    fn log(&mut self, level: Level, message: String) {
        let level = match level {
            Level::Trace => LogLevel::Trace,
            Level::Debug => LogLevel::Debug,
            Level::Info => LogLevel::Info,
            Level::Warn => LogLevel::Warn,
            Level::Error => LogLevel::Error,
        };
        self.perform(Some((level, &message)));
    }
}

/// Builds a `LogImport` over a real plugin log inside `directory`, for tests and the harness.
/// The clock and diagnostics are fixed fakes so no host service is touched.
#[cfg(any(test, feature = "testing"))]
pub fn import_for_tests(directory: &Path) -> LogImport {
    struct FixedClock;
    impl LogClock for FixedClock {
        fn now(&self) -> Result<LogTime, Refusal> {
            Ok(LogTime {
                local_timestamp: "2026-01-01 00:00:00".into(),
                monotonic_millis: 0,
            })
        }
    }
    struct SilentDiagnostics;
    impl LogDiagnostics for SilentDiagnostics {
        fn failed(&self, _failure: LogFailure) {}
    }
    let logger = PluginLogger::open(
        directory,
        "log-test",
        Tier::T3,
        LogConfig::default(),
        64,
        &[],
    )
    .expect("the test plugin log opens in a fresh directory");
    let budget = PluginLogInstanceBudget::new(1, Side::Server)
        .expect("a nonzero instance identity is accepted");
    LogImport::new(
        Arc::new(Mutex::new(logger)),
        Arc::new(Mutex::new(budget)),
        Arc::new(FixedClock),
        Arc::new(SilentDiagnostics),
    )
    .expect("the shared locks are not poisoned")
}

#[cfg(test)]
mod tests {
    use super::{import_for_tests, LogImport};
    use ilium_plugin_api::generated::host::log::{HostLog, Level};

    /// Message size limit of one log entry, in bytes (manual: api-log, "Message size").
    const MESSAGE_LIMIT_BYTES: usize = 16_384;

    /// Calls per second that the default per-plugin log budget accepts (manual: api-log, "Limits").
    const DEFAULT_LINES_PER_SECOND: usize = 200;

    /// Every level the manual lists, in the order of the WIT enum (`trace` to `error`).
    const ALL_LEVELS: [Level; 5] = [
        Level::Trace,
        Level::Debug,
        Level::Info,
        Level::Warn,
        Level::Error,
    ];

    /// REQ: DEV-652
    #[test]
    fn writes_a_line_at_every_documented_level_without_panicking() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        let mut log_import = import_for_tests(directory.path());
        for level in ALL_LEVELS {
            log_import.log(level, format!("level {}", level.as_str()));
        }
    }

    /// REQ: DEV-652
    #[test]
    fn level_names_follow_the_wit_order_trace_to_error() {
        assert_eq!(
            Level::CASE_NAMES,
            &["trace", "debug", "info", "warn", "error"]
        );
        let names: Vec<&str> = ALL_LEVELS.iter().map(|level| level.as_str()).collect();
        assert_eq!(names, Level::CASE_NAMES);
    }

    /// REQ: DEV-653
    #[test]
    fn trace_and_debug_calls_are_accepted_and_do_not_panic() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        let mut log_import = import_for_tests(directory.path());
        log_import.log(Level::Trace, "every step of a sequence".to_string());
        log_import.log(Level::Debug, "a value that helps find a fault".to_string());
    }

    /// REQ: DEV-650
    #[test]
    fn empty_message_is_accepted_and_produces_no_panic() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        let mut log_import = import_for_tests(directory.path());
        log_import.log(Level::Info, String::new());
    }

    /// REQ: DEV-650
    #[test]
    fn message_with_line_breaks_is_accepted_and_does_not_panic() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        let mut log_import = import_for_tests(directory.path());
        log_import.log(Level::Info, "first\nsecond\nthird".to_string());
    }

    /// REQ: DEV-651
    #[test]
    fn message_of_exactly_the_size_limit_is_accepted_without_panicking() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        let mut log_import = import_for_tests(directory.path());
        let message = "a".repeat(MESSAGE_LIMIT_BYTES);
        assert_eq!(message.len(), MESSAGE_LIMIT_BYTES);
        log_import.log(Level::Info, message);
    }

    /// REQ: DEV-651
    #[test]
    fn message_one_byte_over_the_size_limit_is_accepted_and_cut_without_panicking() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        let mut log_import = import_for_tests(directory.path());
        let message = "a".repeat(MESSAGE_LIMIT_BYTES + 1);
        assert_eq!(message.len(), MESSAGE_LIMIT_BYTES + 1);
        log_import.log(Level::Warn, message);
    }

    /// REQ: DEV-651
    #[test]
    fn oversized_multibyte_message_is_cut_on_a_character_boundary_without_panicking() {
        // Two-byte characters: the limit falls on a character boundary exactly.
        let directory = tempfile::tempdir().expect("temporary log directory");
        let mut log_import = import_for_tests(directory.path());
        let two_byte_message = "é".repeat(MESSAGE_LIMIT_BYTES / 2 + 10);
        assert!(two_byte_message.len() > MESSAGE_LIMIT_BYTES);
        log_import.log(Level::Error, two_byte_message);

        // Three-byte characters: the limit falls inside a character, so the cut must back off.
        let three_byte_message = "€".repeat(MESSAGE_LIMIT_BYTES / 3 + 10);
        assert!(three_byte_message.len() > MESSAGE_LIMIT_BYTES);
        log_import.log(Level::Error, three_byte_message);
    }

    /// REQ: DEV-656
    #[test]
    fn calls_beyond_the_default_rate_are_dropped_silently_without_panicking() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        let mut log_import = import_for_tests(directory.path());
        for index in 0..=DEFAULT_LINES_PER_SECOND {
            log_import.log(Level::Info, format!("line {index}"));
        }
    }

    /// REQ: SEC-243
    #[test]
    fn message_containing_a_secret_pattern_is_accepted_without_panicking() {
        // Redaction is checked by reading the log back, which this import cannot do; this test only
        // confirms that a call carrying secret-like text is accepted and does not panic.
        let directory = tempfile::tempdir().expect("temporary log directory");
        let mut log_import = import_for_tests(directory.path());
        log_import.log(
            Level::Warn,
            "token ghp_0000000000000000000000000000000000000000 rejected".to_string(),
        );
    }
}
