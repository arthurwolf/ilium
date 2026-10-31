//! Presentation helpers for session-bound goal evidence.
//!
//! Goal content is local user data; these helpers do not log or persist it.

use ilium_agent_session::goal_evidence::{Freshness, GoalObservation, GoalPhase};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Wrap only as far as the viewport, retaining at most its visible rows.
/// A scroll beyond the end is clamped to the final full viewport.
pub(crate) fn wrap_goal_detail(
    header: &str,
    body: Option<&str>,
    width: u16,
    scroll: usize,
    visible: usize,
) -> (usize, Vec<String>) {
    use std::borrow::Cow;
    use std::collections::VecDeque;
    fn add(
        rows: &mut VecDeque<String>,
        count: &mut usize,
        visible: usize,
        target: usize,
        row: String,
    ) -> bool {
        *count = count.saturating_add(1);
        if rows.len() == visible {
            rows.pop_front();
        }
        rows.push_back(row);
        *count >= target
    }
    if visible == 0 {
        return (0, Vec::new());
    }
    let width = usize::from(width.max(1));
    let target = scroll.saturating_add(visible);
    let mut rows = VecDeque::with_capacity(visible);
    let mut count = 0usize;
    let mut row = String::new();
    let mut row_width = 0;
    for (index, segment) in [Some(header), body].into_iter().enumerate() {
        let Some(segment) = segment else {
            continue;
        };
        if index == 1 {
            if add(
                &mut rows,
                &mut count,
                visible,
                target,
                std::mem::take(&mut row),
            ) {
                return (scroll, rows.into());
            }
            row_width = 0;
        }
        for grapheme in segment.graphemes(true) {
            if matches!(grapheme, "\n" | "\r" | "\r\n") {
                if add(
                    &mut rows,
                    &mut count,
                    visible,
                    target,
                    std::mem::take(&mut row),
                ) {
                    return (scroll, rows.into());
                }
                row_width = 0;
                continue;
            }
            let safe = if grapheme.chars().any(char::is_control) {
                Cow::Owned(sanitized_objective(grapheme))
            } else {
                Cow::Borrowed(grapheme)
            };
            for piece in safe.graphemes(true) {
                let piece_width = UnicodeWidthStr::width(piece);
                if row_width + piece_width > width && !row.is_empty() {
                    if add(
                        &mut rows,
                        &mut count,
                        visible,
                        target,
                        std::mem::take(&mut row),
                    ) {
                        return (scroll, rows.into());
                    }
                    row_width = 0;
                }
                row.push_str(piece);
                row_width += piece_width;
            }
        }
    }
    add(&mut rows, &mut count, visible, target, row);
    (count.saturating_sub(visible), rows.into())
}

/// Prevent terminal controls from changing the surrounding interface while
/// preserving line breaks and printable Unicode in the displayed objective.
pub(crate) fn sanitized_objective(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\n' => result.push('\n'),
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                result.push('\n');
            }
            '\t' => result.push_str("    "),
            value if value.is_control() => result.push('\u{fffd}'),
            value => result.push(value),
        }
    }
    result
}

pub(crate) fn phase_label(phase: GoalPhase) -> &'static str {
    match phase {
        GoalPhase::Active => "Active",
        GoalPhase::Paused => "Paused",
        GoalPhase::Blocked => "Blocked",
        GoalPhase::UsageLimited => "Usage limit reached",
        GoalPhase::BudgetLimited => "Budget limit reached",
        GoalPhase::Reached => "Complete",
    }
}

/// Borrow the objective so rendering does not copy it on every redraw.
pub(crate) fn goal_detail_parts(observation: &GoalObservation) -> (String, Option<&str>) {
    if observation.absent {
        return ("No current goal".to_owned(), None);
    }
    let phase = observation.phase.map_or("Status unavailable", phase_label);
    let objective = observation
        .objective
        .as_deref()
        .filter(|value| !value.is_empty());
    let freshness = if observation.freshness == Freshness::Stale && objective.is_some() {
        " (last known text; provider unavailable)"
    } else {
        ""
    };
    (
        format!("{phase}{freshness}"),
        Some(objective.unwrap_or("Goal text unavailable")),
    )
}

#[cfg(test)]
fn goal_detail(observation: &GoalObservation) -> String {
    let (header, body) = goal_detail_parts(observation);
    body.map_or(header.clone(), |body| {
        format!("{header}\n{}", sanitized_objective(body))
    })
}

/// Compact banner text keeps the lifecycle and freshness ahead of the
/// objective, so even a single visible row does not imply stale text is live.
pub(crate) fn goal_banner_text(observation: &GoalObservation) -> String {
    if observation.absent {
        return "No current goal".to_owned();
    }
    let phase = observation.phase.map_or("Status unavailable", phase_label);
    let objective = observation
        .objective
        .as_deref()
        .filter(|value| !value.is_empty());
    let freshness = if observation.freshness == Freshness::Stale && objective.is_some() {
        " (stale)"
    } else {
        ""
    };
    // The banner is only a preview: limit work on each redraw independently
    // of the full objective retained for the scrollable detail view.
    let text = objective.map_or_else(
        || "Goal text unavailable".to_owned(),
        |value| {
            const PREVIEW_CHARACTERS: usize = 4096;
            let end = value
                .char_indices()
                .nth(PREVIEW_CHARACTERS)
                .map(|(index, _)| index);
            let mut preview = sanitized_objective(end.map_or(value, |index| &value[..index]));
            if end.is_some() {
                preview.push('…');
            }
            preview
        },
    );
    format!("{phase}{freshness}: {text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_wrapping_preserves_spaces_lines_and_unicode() {
        let (_, rows) = wrap_goal_detail("a  b\n界e\u{301}", None, 3, 0, 10);
        assert_eq!(rows, ["a  ", "b", "界e\u{301}"]);
        assert_eq!(
            wrap_goal_detail("a\n\nb\n", None, 5, 0, 10).1,
            ["a", "", "b", ""]
        );
        assert_eq!(
            wrap_goal_detail("a\nb\nc\nd", None, 5, 1, 2),
            (1, vec!["b".into(), "c".into()])
        );
        assert_eq!(
            wrap_goal_detail("a\nb\nc\nd", None, 5, usize::MAX, 2),
            (2, vec!["c".into(), "d".into()])
        );
        assert_eq!(
            wrap_goal_detail("Active", Some("a\t\u{1b}\r\nb"), 20, 0, 10).1,
            ["Active", "a    �", "b"]
        );
    }

    #[test]
    fn detail_scrolling_clamps_after_resize_and_reaches_every_row() {
        let expected = ["Act", "ive", "a  ", "b", "界e\u{301}", "", "end"];
        for offset in 0..expected.len() {
            let (actual_offset, rows) =
                wrap_goal_detail("Active", Some("a  b\n界e\u{301}\n\nend"), 3, offset, 2);
            let expected_offset = offset.min(expected.len() - 2);
            assert_eq!(actual_offset, expected_offset);
            assert_eq!(rows, expected[expected_offset..expected_offset + 2]);
        }
        assert_eq!(
            wrap_goal_detail("Active", Some("a"), 3, usize::MAX, 10).0,
            0
        );
        assert!(wrap_goal_detail("Active", Some("a"), 3, 0, 0).1.is_empty());
    }

    #[test]
    fn goal_detail_labels_freshness_and_never_emits_terminal_controls() {
        let observation = GoalObservation {
            phase: Some(GoalPhase::BudgetLimited),
            objective: Some("first\n世界\x1b[2J".to_owned()),
            freshness: Freshness::Stale,
            absent: false,
        };
        let result = goal_detail(&observation);
        assert!(result.contains("Budget limit reached"));
        assert!(result.contains("last known text"));
        assert!(result.contains("first\n世界"));
        assert!(!result.contains('\x1b'));
        assert!(result.contains('\u{fffd}'));
        assert!(goal_banner_text(&observation).starts_with("Budget limit reached (stale): first"));
        assert_eq!(sanitized_objective("a\r\nb\rc"), "a\nb\nc");
    }

    #[test]
    fn unknown_and_absent_goals_are_distinct() {
        let mut observation = GoalObservation {
            phase: None,
            objective: None,
            freshness: Freshness::Current,
            absent: false,
        };
        assert!(goal_detail(&observation).contains("Goal text unavailable"));
        observation.absent = true;
        assert_eq!(goal_detail(&observation), "No current goal");
    }

    #[test]
    fn banner_preview_bounds_work_without_cutting_unicode_or_detail() {
        let objective = "界".repeat(5000);
        let observation = GoalObservation {
            phase: Some(GoalPhase::Active),
            objective: Some(objective.clone()),
            freshness: Freshness::Current,
            absent: false,
        };
        let preview = goal_banner_text(&observation);
        assert!(preview.ends_with('…'));
        assert!(preview.chars().count() < 4200);
        assert!(goal_detail(&observation).ends_with(objective.as_str()));
    }
}
