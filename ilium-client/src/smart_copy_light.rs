//! Smart Copy light: the model-free variant of Smart Copy.
//!
//! Holding the configured modifier key over a terminal pane freezes that
//! pane, exactly like Smart Copy, but never calls a model: only the regions
//! the client detects itself are offered. Clicking regions adds them to a
//! persistent selection; releasing the key copies the selection and shows a
//! short-lived "Preview" of what went to the clipboard.
//!
//! Terminals only report which modifiers are held on mouse events, not the
//! key release itself (unless they implement the Kitty key-release protocol),
//! so "released" is detected by the first of: a key-release event for the
//! modifier, a mouse event without the modifier, focus loss, or a short idle
//! grace after the last click.

use std::time::{Duration, Instant};

use crate::config::SmartCopyLightKey;

/// How long the Preview dialog stays up; its progress bar counts this down.
pub const PREVIEW_DURATION: Duration = Duration::from_secs(1);

/// Idle time after the last modifier-bearing event, once something is
/// selected, after which a terminal that cannot report key release is
/// treated as having released the key.
pub const RELEASE_IDLE_GRACE: Duration = Duration::from_millis(1500);

/// A frame cadence smooth enough for a one-second progress bar.
pub const PREVIEW_FRAME_INTERVAL: Duration = Duration::from_millis(33);

pub(crate) const PREVIEW_MAXIMUM_LINES: usize = 6;
pub(crate) const PREVIEW_MAXIMUM_WIDTH: usize = 72;

/// A Smart Copy light interaction in progress.
#[derive(Debug, Clone, Copy)]
pub struct SmartCopyLightState {
    pub key: SmartCopyLightKey,
    pub last_key_activity: Instant,
}

impl SmartCopyLightState {
    pub fn new(key: SmartCopyLightKey, now: Instant) -> Self {
        Self {
            key,
            last_key_activity: now,
        }
    }
}

/// What the last light selection put on the clipboard.
#[derive(Debug, Clone)]
pub struct SmartCopyPreview {
    pub text: ilium_execution::RetiringArc<String>,
    characters: usize,
    lines: Vec<String>,
    _facts_retention: Option<ilium_execution::Retention>,
    pub region_count: usize,
    /// False when the clipboard write failed; the dialog says so.
    pub copied: bool,
    pub started_at: Instant,
}

impl SmartCopyPreview {
    pub(crate) fn from_prepared(
        text: ilium_execution::RetiringArc<String>,
        region_count: usize,
        characters: usize,
        lines: Vec<String>,
        copied: bool,
        now: Instant,
        retention: Option<ilium_execution::Retention>,
    ) -> Self {
        Self {
            text,
            characters,
            lines,
            region_count,
            copied,
            started_at: now,
            _facts_retention: retention,
        }
    }

    /// Compatibility for explicitly synthetic synchronous UI/unit fixtures.
    /// Production has no whole-String constructor or preparation bypass.
    #[cfg(test)]
    pub fn new(text: String, region_count: usize, copied: bool, now: Instant) -> Self {
        let characters = text.chars().count();
        let lines = preview_lines(&text, PREVIEW_MAXIMUM_LINES, PREVIEW_MAXIMUM_WIDTH);
        let text = crate::execution::test_client()
            .retirement()
            .try_reserve::<String>(text.capacity() + 4096)
            .unwrap()
            .attach_shared(text);
        Self {
            text,
            characters,
            lines,
            region_count,
            copied,
            started_at: now,
            _facts_retention: None,
        }
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started_at) >= PREVIEW_DURATION
    }

    /// Fraction of the countdown still remaining, from 1.0 down to 0.0.
    pub fn remaining_fraction(&self, now: Instant) -> f64 {
        let elapsed = now.saturating_duration_since(self.started_at);
        1.0 - (elapsed.as_secs_f64() / PREVIEW_DURATION.as_secs_f64()).clamp(0.0, 1.0)
    }

    /// The text as shown in the dialog: at most a few lines, each clipped to
    /// a display width, with a final line saying how much more was copied.
    pub fn display_lines(&self) -> Vec<String> {
        self.lines.clone() // At most six cached bounded lines; never scans original.
    }

    pub fn summary(&self) -> String {
        let characters = self.characters;
        let regions = if self.region_count == 1 {
            "1 selection".to_string()
        } else {
            format!("{} selections", self.region_count)
        };
        if self.copied {
            format!("{regions} · {characters} characters copied")
        } else {
            format!("{regions} · {characters} characters · clipboard unavailable")
        }
    }
}

/// Clips `text` for display: at most `maximum_lines` lines of at most
/// `maximum_width` characters, replacing control characters, with a trailing
/// "… N more lines" marker when lines were dropped. Blank lines between
/// regions are kept so separate selections stay visibly separate.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_shown_whole() {
        assert_eq!(preview_lines("one\ntwo", 6, 20), vec!["one", "two"]);
    }

    #[test]
    fn long_lines_are_clipped_with_an_ellipsis() {
        let lines = preview_lines("abcdefghij", 6, 5);
        assert_eq!(lines, vec!["abcd…"]);
    }

    #[test]
    fn extra_lines_collapse_into_a_count() {
        let text = (1..=10)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let lines = preview_lines(&text, 4, 20);
        assert_eq!(lines, vec!["1", "2", "3", "… 7 more lines"]);
        let lines = preview_lines("a\nb\nc", 2, 20);
        assert_eq!(lines, vec!["a", "… 2 more lines"]);
        let lines = preview_lines("a\nb\nc", 3, 20);
        assert_eq!(lines, vec!["a", "b", "c"]);
    }

    #[test]
    fn control_characters_never_reach_the_dialog() {
        assert_eq!(preview_lines("a\u{1b}[31mb\tc", 6, 40), vec!["a [31mb c"]);
    }

    #[test]
    fn preview_counts_down_over_exactly_one_second() {
        let start = Instant::now();
        let preview = SmartCopyPreview::new("x".into(), 1, true, start);
        assert!((preview.remaining_fraction(start) - 1.0).abs() < 1e-9);
        assert!(
            (preview.remaining_fraction(start + Duration::from_millis(500)) - 0.5).abs() < 1e-9
        );
        assert!(!preview.is_expired(start + Duration::from_millis(999)));
        assert!(preview.is_expired(start + Duration::from_secs(1)));
        assert_eq!(
            preview.remaining_fraction(start + Duration::from_secs(5)),
            0.0
        );
    }

    #[test]
    fn summary_reports_counts_and_clipboard_failure() {
        let now = Instant::now();
        assert_eq!(
            SmartCopyPreview::new("abc".into(), 1, true, now).summary(),
            "1 selection · 3 characters copied"
        );
        assert_eq!(
            SmartCopyPreview::new("abc".into(), 2, false, now).summary(),
            "2 selections · 3 characters · clipboard unavailable"
        );
    }
}
