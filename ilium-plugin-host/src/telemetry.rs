//! Counters, traces and the log ring buffer behind `ilium plugin trace/profile/logs`
//! (reference-cli.md).

/// One captured log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// The plugin.
    pub plugin: String,
    /// Level name.
    pub level: String,
    /// The message.
    pub message: String,
}

impl LogLine {
    /// Whether the message contains `needle` (reference-cli.md, `ilium plugin logs` filtering).
    pub fn contains(&self, needle: &str) -> bool {
        self.message.contains(needle)
    }
}

/// The ring buffer of recent log lines.
#[derive(Debug)]
pub struct LogRing {
    capacity: usize,
}

impl LogRing {
    /// A ring that keeps at most `capacity` lines.
    pub fn new(capacity: usize) -> LogRing {
        LogRing { capacity }
    }

    /// Adds a line, dropping the oldest at capacity.
    pub fn push(&mut self, line: LogLine) {
        unimplemented!("host::telemetry::LogRing::push")
    }

    /// The lines of a plugin, oldest first.
    pub fn lines_of(&self, plugin: &str) -> Vec<LogLine> {
        unimplemented!("host::telemetry::LogRing::lines_of")
    }
}
