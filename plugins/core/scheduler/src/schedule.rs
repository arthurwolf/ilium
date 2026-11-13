//! Scheduled terminal input: duration validation, absolute deadlines and the due-work rules
//! of the executor. Pure: a pane is a `u64` id and the clock is a caller-supplied value in
//! milliseconds since the Unix epoch. Ported from ilium-client/src/scheduled_input.rs (dialog
//! validation) and ilium-server/src/scheduled_input.rs (deadline and due selection).

pub const MILLIS_PER_SECOND: u64 = 1000;
pub const SECONDS_PER_MINUTE: u64 = 60;
pub const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;
/// Seconds field a fresh dialog starts with.
pub const DEFAULT_SECONDS: &str = "30";

/// One durable countdown: what to type and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledInput {
    pub execute_at_unix_millis: u64,
    pub text: String,
    pub send_enter: bool,
}

impl ScheduledInput {
    /// At least text or Enter must be present, otherwise the schedule is an invisible no-op.
    pub fn has_input(&self) -> bool {
        !self.text.is_empty() || self.send_enter
    }
}

/// The dialog control that has focus, in Tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogFocus {
    Hours,
    Minutes,
    Seconds,
    Text,
    SendEnter,
    ScheduleButton,
}

impl DialogFocus {
    const ORDER: [Self; 6] = [
        Self::Hours,
        Self::Minutes,
        Self::Seconds,
        Self::Text,
        Self::SendEnter,
        Self::ScheduleButton,
    ];

    fn position(self) -> usize {
        Self::ORDER
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }

    /// Tab moves forward and wraps.
    pub fn next(self) -> Self {
        Self::ORDER[(self.position() + 1) % Self::ORDER.len()]
    }

    /// Shift-Tab moves backward and wraps.
    pub fn previous(self) -> Self {
        Self::ORDER[(self.position() + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
}

/// The three duration fields as typed, plus the payload, before validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub hours: String,
    pub minutes: String,
    pub seconds: String,
    pub text: String,
    pub send_enter: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            hours: String::new(),
            minutes: String::new(),
            seconds: DEFAULT_SECONDS.to_string(),
            text: String::new(),
            send_enter: true,
        }
    }
}

/// A validated request: relative delay in seconds, payload text and the Enter flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub delay_seconds: u64,
    pub text: String,
    pub send_enter: bool,
}

fn parse_duration_field(value: &str, label: &str) -> Result<u64, String> {
    if value.is_empty() {
        return Ok(0);
    }
    value
        .parse::<u64>()
        .map_err(|_| format!("{label} must be a whole number"))
}

impl Draft {
    /// Parses and bounds the three duration fields. Minutes and seconds use clock-style
    /// 0-59 ranges; hours is bounded only by `u64` arithmetic.
    pub fn delay_seconds(&self) -> Result<u64, String> {
        let hours = parse_duration_field(&self.hours, "Hours")?;
        let minutes = parse_duration_field(&self.minutes, "Minutes")?;
        let seconds = parse_duration_field(&self.seconds, "Seconds")?;
        if minutes >= SECONDS_PER_MINUTE {
            return Err("Minutes must be between 0 and 59".to_string());
        }
        if seconds >= SECONDS_PER_MINUTE {
            return Err("Seconds must be between 0 and 59".to_string());
        }
        let total = hours
            .checked_mul(SECONDS_PER_HOUR)
            .and_then(|value| value.checked_add(minutes * SECONDS_PER_MINUTE))
            .and_then(|value| value.checked_add(seconds))
            .ok_or_else(|| "The duration is too large".to_string())?;
        if total == 0 {
            return Err("Choose a delay of at least one second".to_string());
        }
        Ok(total)
    }

    /// Validates the countdown and the three payload forms: text only, text plus Enter, or
    /// Enter only.
    pub fn validated_request(&self) -> Result<Request, String> {
        let delay_seconds = self.delay_seconds()?;
        if self.text.is_empty() && !self.send_enter {
            return Err("Enter text, enable Send Enter, or use both".to_string());
        }
        Ok(Request {
            delay_seconds,
            text: self.text.clone(),
            send_enter: self.send_enter,
        })
    }
}

/// True when `character` may be typed into a numeric duration field (digits only; a paste is
/// kept verbatim elsewhere so that `-1` or `1.5` is rejected rather than silently rewritten).
pub fn accepts_numeric_key(character: char) -> bool {
    character.is_ascii_digit()
}

/// Converts a relative delay into the absolute deadline persisted with the pane. Zero is
/// rejected so accepting the dialog always produces a visible countdown.
pub fn deadline_from_delay(now_unix_millis: u64, delay_seconds: u64) -> Result<u64, String> {
    if delay_seconds == 0 {
        return Err("scheduled input delay must be at least one second".to_string());
    }
    let delay_millis = delay_seconds
        .checked_mul(MILLIS_PER_SECOND)
        .ok_or_else(|| "scheduled input delay is too large".to_string())?;
    now_unix_millis
        .checked_add(delay_millis)
        .ok_or_else(|| "scheduled input deadline is too large".to_string())
}

/// A pane with a pending schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub pane_id: u64,
    pub input: ScheduledInput,
    /// False while the pane's workspace folder is missing: such a countdown is retained but
    /// never fires until the workspace is recovered.
    pub available: bool,
}

/// The earliest deadline among available panes; the executor rescans after every wake, so a
/// replacement or removal never leaves an obsolete sleep active.
pub fn nearest_deadline(pending: &[Pending]) -> Option<u64> {
    pending
        .iter()
        .filter(|entry| entry.available)
        .map(|entry| entry.input.execute_at_unix_millis)
        .min()
}

/// The schedules due at `now`, oldest deadline first (ties keep the input order).
pub fn due_inputs(pending: &[Pending], now_unix_millis: u64) -> Vec<&Pending> {
    let mut due: Vec<&Pending> = pending
        .iter()
        .filter(|entry| entry.available && entry.input.execute_at_unix_millis <= now_unix_millis)
        .collect();
    due.sort_by_key(|entry| entry.input.execute_at_unix_millis);
    due
}

/// How long the executor sleeps: zero when already due.
pub fn sleep_millis(deadline: u64, now_unix_millis: u64) -> u64 {
    deadline.saturating_sub(now_unix_millis)
}

/// Compare-and-clear: a finished executor may clear a pane's schedule only while it is still
/// the one it ran, so a replacement submitted meanwhile survives.
pub fn clear_if_matches(slot: &mut Option<ScheduledInput>, expected: &ScheduledInput) -> bool {
    if slot.as_ref() == Some(expected) {
        *slot = None;
        true
    } else {
        false
    }
}

/// Remaining whole seconds shown in the tree, rounded up so the countdown never reads zero
/// before it fires. Past deadlines read zero.
pub fn remaining_seconds(deadline_unix_millis: u64, now_unix_millis: u64) -> u64 {
    sleep_millis(deadline_unix_millis, now_unix_millis).div_ceil(MILLIS_PER_SECOND)
}

/// The next instant (unix milliseconds) at which the countdown label changes, so a client can
/// redraw exactly then: the moment the displayed rounded-up second drops by one.
pub fn next_redraw_unix_millis(deadline_unix_millis: u64, now_unix_millis: u64) -> Option<u64> {
    if now_unix_millis >= deadline_unix_millis {
        return None;
    }
    let remaining = deadline_unix_millis - now_unix_millis;
    let into_second = remaining % MILLIS_PER_SECOND;
    let wait = if into_second == 0 {
        MILLIS_PER_SECOND
    } else {
        into_second
    };
    Some(now_unix_millis + wait)
}
