//! Smart Copy light: the model-free variant. Only regions detected locally are offered; clicking
//! adds them to a selection, releasing the modifier copies it and shows a short-lived preview.
//!
//! Pure rules moved from `ilium-client/src/smart_copy_light.rs`. Time is whole milliseconds since
//! an arbitrary epoch (`u64`), so the module has no clock and stays wasm-safe.

/// How long the preview stays up; its progress bar counts this down.
pub const PREVIEW_DURATION_MS: u64 = 1000;
/// Idle time after the last modifier-bearing event, once something is selected, after which a
/// terminal that cannot report key release is treated as having released the key.
pub const RELEASE_IDLE_GRACE_MS: u64 = 1500;
/// A frame cadence smooth enough for a one-second progress bar.
pub const PREVIEW_FRAME_INTERVAL_MS: u64 = 33;
pub const PREVIEW_MAXIMUM_LINES: usize = 6;
pub const PREVIEW_MAXIMUM_WIDTH: usize = 72;

/// A light interaction in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightState {
    pub key: String,
    pub last_key_activity_ms: u64,
}

impl LightState {
    pub fn new(key: &str, now_ms: u64) -> Self {
        Self {
            key: key.to_string(),
            last_key_activity_ms: now_ms,
        }
    }

    /// Records a modifier-bearing event.
    pub fn touch(&mut self, now_ms: u64) {
        self.last_key_activity_ms = now_ms;
    }

    /// Whether a terminal that cannot report key release should be treated as released: something
    /// is selected and the idle grace passed since the last modifier-bearing event.
    pub fn is_idle_released(&self, has_selection: bool, now_ms: u64) -> bool {
        has_selection && now_ms.saturating_sub(self.last_key_activity_ms) >= RELEASE_IDLE_GRACE_MS
    }
}

/// What the last light selection put on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub text: String,
    pub region_count: usize,
    /// False when the clipboard write failed; the dialog says so.
    pub copied: bool,
    pub started_at_ms: u64,
    characters: usize,
    lines: Vec<String>,
}

impl Preview {
    pub fn new(text: String, region_count: usize, copied: bool, now_ms: u64) -> Self {
        let characters = text.chars().count();
        let lines = preview_lines(&text, PREVIEW_MAXIMUM_LINES, PREVIEW_MAXIMUM_WIDTH);
        Self {
            text,
            region_count,
            copied,
            started_at_ms: now_ms,
            characters,
            lines,
        }
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.started_at_ms) >= PREVIEW_DURATION_MS
    }

    /// Fraction of the countdown still remaining, from 1.0 down to 0.0.
    pub fn remaining_fraction(&self, now_ms: u64) -> f64 {
        let elapsed = now_ms.saturating_sub(self.started_at_ms) as f64;
        1.0 - (elapsed / PREVIEW_DURATION_MS as f64).clamp(0.0, 1.0)
    }

    /// The text as shown: at most a few lines, each clipped to a display width.
    pub fn display_lines(&self) -> &[String] {
        &self.lines
    }

    pub fn summary(&self) -> String {
        let regions = if self.region_count == 1 {
            "1 selection".to_string()
        } else {
            format!("{} selections", self.region_count)
        };
        if self.copied {
            format!("{regions} · {} characters copied", self.characters)
        } else {
            format!(
                "{regions} · {} characters · clipboard unavailable",
                self.characters
            )
        }
    }
}

/// Clips `text` for display: at most `maximum_lines` lines of at most `maximum_width` characters,
/// replacing control characters, with a trailing "… N more lines" marker when lines were dropped.
/// Blank lines between regions are kept so separate selections stay visibly separate.
pub fn preview_lines(text: &str, maximum_lines: usize, maximum_width: usize) -> Vec<String> {
    let mut count = 0usize;
    let mut lines = Vec::with_capacity(maximum_lines.max(1));
    for line in text.lines() {
        if lines.len() < maximum_lines.max(1) {
            lines.push(clip_line(line, maximum_width));
        }
        count += 1;
    }
    let shown = if count > maximum_lines {
        maximum_lines.saturating_sub(1).max(1)
    } else {
        count
    };
    lines.truncate(shown);
    if count > shown {
        let hidden = count - shown;
        lines.push(format!(
            "… {hidden} more line{}",
            if hidden == 1 { "" } else { "s" }
        ));
    }
    lines
}

fn clip_line(line: &str, maximum_width: usize) -> String {
    let mut prefix: String = line
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(maximum_width.saturating_add(1))
        .collect();
    if prefix.chars().count() <= maximum_width {
        return prefix;
    }
    prefix = prefix
        .chars()
        .take(maximum_width.saturating_sub(1))
        .collect();
    prefix.push('…');
    prefix
}
