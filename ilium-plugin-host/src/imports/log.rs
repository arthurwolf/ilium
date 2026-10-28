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
