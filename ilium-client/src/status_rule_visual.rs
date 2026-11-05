//! Terminal-cell validation and frame selection for custom status visuals.

use ilium_core::status_rule_expression::{
    first_match, Observation, PaneFacts, ScreenPattern, StatusRules, StatusVisual, VisualKind,
};
use ilium_core::{representative_progress, PaneProgress, PaneStatus, ShellOutputPhase};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use crate::config::MotionLevel;

/// Braille animation presets, by stable name. Every frame is one cell wide,
/// so a preset can never move a title. The first two names are the ones the
/// shipped default rules use; the rest are offered by the editor's picker.
pub(crate) const BRAILLE_PRESETS: &[(&str, &[&str])] = &[
    (
        "spinner",
        &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
    ),
    ("loop", &["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈"]),
    ("dots", &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"]),
    ("snake", &["⠁", "⠉", "⠙", "⠸", "⢰", "⣠", "⣄", "⡆", "⠇", "⠃"]),
    ("bounce", &["⠁", "⠂", "⠄", "⡀", "⠄", "⠂"]),
    ("pulse", &["⠂", "⠒", "⠶", "⠿", "⠶", "⠒"]),
    ("bar-fill", &["⡀", "⣀", "⣄", "⣤", "⣦", "⣶", "⣷", "⣿"]),
];

/// The frames of a named Braille preset, if the name is known.
pub(crate) fn braille_preset_frames(preset: &str) -> Option<&'static [&'static str]> {
    BRAILLE_PRESETS
        .iter()
        .find(|(name, _)| *name == preset)
        .map(|(_, frames)| *frames)
}

/// Client-only facts in the shape the rule engine expects. Shared by the tree
/// renderer and the live-pane evaluator so a cent is rounded the same way in
/// both.
pub(crate) fn supplementary_facts(
    frozen: bool,
    session_cost_usd: Option<f64>,
) -> ilium_core::status_rule_expression::SupplementaryFacts {
    ilium_core::status_rule_expression::SupplementaryFacts {
        frozen: Some(frozen),
        session_cost_cents: session_cost_usd.map(|usd| (usd * 100.0).round().max(0.0) as u64),
    }
}

/// Time until the next potentially visible frame of any enabled rule or the
/// fallback. The settings previews render every rule, so they use this
/// whole-set clock; the live tree uses `next_frame_delay` over the visuals
/// its panes actually matched.
pub(crate) fn animation_frame_delay(
    rules: &StatusRules,
    motion: MotionLevel,
    elapsed_millis: u128,
) -> Option<u64> {
    next_frame_delay(
        rules
            .rules
            .iter()
            .filter(|rule| rule.enabled)
            .map(|rule| &rule.then)
            .chain(rules.fallback.iter()),
        motion,
        elapsed_millis,
    )
}

/// Time until the next frame boundary of the given visuals, honouring the
/// motion level (Reduced caps every animation at 2 frames per second, Off
/// animates nothing). `None` when no visual animates.
pub(crate) fn next_frame_delay<'a>(
    visuals: impl IntoIterator<Item = &'a StatusVisual>,
    motion: MotionLevel,
    elapsed_millis: u128,
) -> Option<u64> {
    if motion == MotionLevel::Off {
        return None;
    }
    visuals
        .into_iter()
        .filter_map(|visual| match &visual.kind {
            VisualKind::BrailleAnimation {
                frames_per_second, ..
            }
            | VisualKind::FrameAnimation {
                frames_per_second, ..
            } => {
                let speed = if motion == MotionLevel::Reduced {
                    (*frames_per_second).min(2)
                } else {
                    *frames_per_second
                };
                if speed == 0 {
                    return None;
                }
                let speed = u128::from(speed);
                let tick = elapsed_millis.saturating_mul(speed) / 1000;
                let next = tick
                    .saturating_add(1)
                    .saturating_mul(1000)
                    .saturating_add(speed - 1)
                    / speed;
                Some(
                    next.saturating_sub(elapsed_millis)
                        .max(1)
                        .min(u128::from(u64::MAX)) as u64,
                )
            }
            _ => None,
        })
        .min()
}

/// Validate user-authored frames against the fixed tree status slot.
pub fn validate_frames(frames: &[String], maximum_width: usize) -> Result<(), &'static str> {
    if !(2..=13).contains(&frames.len()) {
        return Err("Choose between 2 and 13 frames");
    }
    let width = frames[0].width();
    if width == 0 || width > maximum_width || width > 2 {
        return Err("Every frame must fit in one or two terminal cells");
    }
    if frames
        .iter()
        .any(|frame| frame.width() != width || frame.chars().any(char::is_control))
    {
        return Err("Frames must have equal widths and contain no control characters");
    }
    Ok(())
}

pub fn validate_visual(visual: &StatusVisual, maximum_width: usize) -> Result<(), &'static str> {
    if let Some(color) = visual.style.color.as_deref() {
        let named = matches!(
            color,
            "danger" | "warning" | "success" | "info" | "accent" | "muted"
        );
        let rgb = color.len() == 7
            && color.starts_with('#')
            && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit);
        if !named && !rgb {
            return Err("Choose a theme colour or an RGB colour in #RRGGBB format");
        }
    }
    match &visual.kind {
        VisualKind::Hidden => Ok(()),
        VisualKind::BuiltIn(key) => {
            if crate::icon_settings::IconTarget::ALL
                .iter()
                .any(|target| target.key() == key)
            {
                Ok(())
            } else {
                Err("Unknown built-in icon")
            }
        }
        VisualKind::Glyph(glyph) => {
            if (1..=maximum_width.min(2)).contains(&glyph.width())
                && !glyph.chars().any(char::is_control)
            {
                Ok(())
            } else {
                Err("The glyph must fit in one or two terminal cells")
            }
        }
        VisualKind::BrailleAnimation {
            preset,
            frames_per_second,
        } => {
            if braille_preset_frames(preset).is_none() {
                Err("Unknown Braille animation")
            } else if !(1..=20).contains(frames_per_second) {
                Err("Animation speed must be between 1 and 20 frames per second")
            } else {
                Ok(())
            }
        }
        VisualKind::FrameAnimation {
            frames,
            frames_per_second,
            ..
        } => {
            if !(1..=20).contains(frames_per_second) {
                return Err("Animation speed must be between 1 and 20 frames per second");
            }
            validate_frames(frames, maximum_width)
        }
        VisualKind::ProgressFrames { frames } => validate_frames(frames, maximum_width),
    }
}

fn animation_index(
    elapsed_millis: u64,
    frames_per_second: u8,
    length: usize,
    ping_pong: bool,
    motion: MotionLevel,
) -> usize {
    if motion == MotionLevel::Off || length < 2 || frames_per_second == 0 {
        return 0;
    }
    let speed = if motion == MotionLevel::Reduced {
        frames_per_second.min(2)
    } else {
        frames_per_second
    };
    let tick = elapsed_millis.saturating_mul(u64::from(speed)) / 1_000;
    let period = if ping_pong {
        length.saturating_mul(2).saturating_sub(2)
    } else {
        length
    };
    let position = (tick % period as u64) as usize;
    if position >= length {
        period - position
    } else {
        position
    }
}

/// Return the chosen user-authored frame. Built-in icon keys are deliberately
/// resolved by the existing icon settings rather than by this helper.
pub fn frame(
    kind: &VisualKind,
    elapsed_millis: u64,
    motion: MotionLevel,
    progress_percent: Option<f32>,
) -> Option<&str> {
    match kind {
        VisualKind::Hidden | VisualKind::BuiltIn(_) => None,
        VisualKind::Glyph(glyph) => Some(glyph),
        VisualKind::BrailleAnimation {
            preset,
            frames_per_second,
        } => {
            // An unknown preset (rejected by validation, but possibly loaded
            // from an edited config) falls back to the spinner rather than
            // leaving the slot blank.
            let frames = braille_preset_frames(preset).unwrap_or(BRAILLE_PRESETS[0].1);
            frames
                .get(animation_index(
                    elapsed_millis,
                    *frames_per_second,
                    frames.len(),
                    false,
                    motion,
                ))
                .copied()
        }
        VisualKind::FrameAnimation {
            frames,
            frames_per_second,
            ping_pong,
        } => frames
            .get(animation_index(
                elapsed_millis,
                *frames_per_second,
                frames.len(),
                *ping_pong,
                motion,
            ))
            .map(String::as_str),
        VisualKind::ProgressFrames { frames } => {
            if frames.is_empty() {
                return None;
            }
            let percent = progress_percent
                .filter(|value| value.is_finite())
                .unwrap_or(0.0)
                .clamp(0.0, 100.0);
            let index = ((percent / 100.0) * (frames.len() - 1) as f32).floor() as usize;
            frames.get(index).map(String::as_str)
        }
    }
}

pub struct StatusSpanContext<'a> {
    pub rules: &'a StatusRules,
    pub status: &'a PaneStatus,
    pub progress: &'a [PaneProgress],
    pub scheduled_input: bool,
    pub shell_output: Option<ShellOutputPhase>,
    pub activity_stale: Option<bool>,
    /// Client-only facts (frozen screen, session spend); `Default` when the
    /// caller has not observed them.
    pub supplementary: ilium_core::status_rule_expression::SupplementaryFacts,
    pub screen_matches: &'a dyn Fn(&ScreenPattern) -> Observation,
    pub icons: &'a crate::icon_settings::IconSettings,
    pub use_stable_glyphs: bool,
    pub elapsed_millis: u64,
    pub motion: MotionLevel,
}

/// Project one pane through the user's ordered rules into one status span.
pub fn status_span(context: StatusSpanContext<'_>) -> Span<'static> {
    let StatusSpanContext {
        rules,
        status,
        progress,
        scheduled_input,
        shell_output,
        activity_stale,
        supplementary,
        screen_matches,
        icons,
        use_stable_glyphs,
        elapsed_millis,
        motion,
    } = context;
    let facts = PaneFacts {
        status,
        progress,
        scheduled_input,
        shell_output,
        activity_stale,
        supplementary,
        screen_matches,
    };
    let visual = first_match(&rules.rules, |condition| condition.observe(&facts))
        .map(|rule| &rule.then)
        .or(rules.fallback.as_ref());
    let Some(visual) = visual else {
        return Span::raw("");
    };
    visual_span(
        visual,
        icons,
        use_stable_glyphs,
        elapsed_millis,
        motion,
        representative_progress(progress)
            .filter(|monitor| monitor.has_measurable_progress())
            .map(|monitor| monitor.report.percent),
    )
}

/// One sentence fragment describing a visual, for the hover tooltip:
/// `the Braille spinner animation at 8 frames per second, in accent, bold`.
pub(crate) fn visual_description(visual: &StatusVisual) -> String {
    let mut text = match &visual.kind {
        VisualKind::Hidden => "no icon".to_owned(),
        VisualKind::BuiltIn(key) => {
            format!("the built-in {} icon", key.replace(['-', '_'], " "))
        }
        VisualKind::Glyph(glyph) => format!("the glyph {glyph}"),
        VisualKind::BrailleAnimation {
            preset,
            frames_per_second,
        } => format!("the Braille {preset} animation at {frames_per_second} frames per second"),
        VisualKind::FrameAnimation {
            frames,
            frames_per_second,
            ping_pong,
        } => format!(
            "a {}-frame animation at {frames_per_second} frames per second{}",
            frames.len(),
            if *ping_pong { ", back and forth" } else { "" }
        ),
        VisualKind::ProgressFrames { frames } => format!(
            "one of {} progress frames chosen by the monitor's percent",
            frames.len()
        ),
    };
    if let Some(color) = visual.style.color.as_deref() {
        text.push_str(", in ");
        text.push_str(color);
    }
    if visual.style.bold {
        text.push_str(", bold");
    }
    if visual.style.dim {
        text.push_str(", dim");
    }
    text
}

/// Render the same visual in the rules editor and in the live tree.
pub(crate) fn visual_span(
    visual: &StatusVisual,
    icons: &crate::icon_settings::IconSettings,
    use_stable_glyphs: bool,
    elapsed_millis: u64,
    motion: MotionLevel,
    progress_percent: Option<f32>,
) -> Span<'static> {
    let glyph = match &visual.kind {
        VisualKind::BuiltIn(key) => crate::icon_settings::IconTarget::ALL
            .into_iter()
            .find(|target| target.key() == key)
            .map(|target| {
                icons
                    .glyph_for_display(target, use_stable_glyphs)
                    .to_owned()
            }),
        kind => frame(kind, elapsed_millis, motion, progress_percent).map(str::to_owned),
    };
    let mut style = Style::new();
    style = match visual.style.color.as_deref() {
        Some("danger") => style.fg(Color::Red),
        Some("warning") => style.fg(Color::Yellow),
        Some("success") => style.fg(Color::Green),
        Some("info") => style.fg(Color::Cyan),
        Some("accent") => style.fg(crate::theme::accent_bg()),
        Some("muted") => style.fg(Color::DarkGray),
        Some(value) if value.len() == 7 && value.starts_with('#') => {
            match u32::from_str_radix(&value[1..], 16) {
                Ok(rgb) => style.fg(Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)),
                Err(_) => style,
            }
        }
        _ => style,
    };
    if visual.style.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if visual.style.dim {
        style = style.add_modifier(Modifier::DIM);
    }
    Span::styled(glyph.unwrap_or_default(), style)
}

#[cfg(test)]
mod tests {
    use super::{
        animation_frame_delay, frame, next_frame_delay, status_span, supplementary_facts,
        validate_frames, validate_visual, visual_description, StatusSpanContext, BRAILLE_PRESETS,
    };
    use crate::config::MotionLevel;
    use ilium_core::status_rule_expression::{
        default_rules, Observation, StatusVisual, VisualKind, VisualStyle,
    };
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn every_braille_preset_is_one_cell_wide_and_validates() {
        for (name, frames) in BRAILLE_PRESETS {
            assert!(frames.len() >= 2, "{name}");
            assert!(
                frames.iter().all(|frame| frame.width() == 1),
                "{name} must stay within one cell"
            );
            let visual = StatusVisual {
                kind: VisualKind::BrailleAnimation {
                    preset: (*name).to_owned(),
                    frames_per_second: 8,
                },
                style: VisualStyle::default(),
            };
            assert_eq!(validate_visual(&visual, 2), Ok(()), "{name}");
            // Every frame index is reachable and every frame non-empty.
            for tick in 0..frames.len() {
                let shown = frame(&visual.kind, (tick as u64) * 125, MotionLevel::Full, None);
                assert_eq!(shown, Some(frames[tick]), "{name} frame {tick}");
            }
        }
        let unknown = StatusVisual {
            kind: VisualKind::BrailleAnimation {
                preset: "no-such-preset".to_owned(),
                frames_per_second: 8,
            },
            style: VisualStyle::default(),
        };
        assert!(validate_visual(&unknown, 2).is_err());
        // Loaded anyway (hand-edited config): shows the spinner, never blank.
        assert_eq!(
            frame(&unknown.kind, 0, MotionLevel::Off, None),
            Some(BRAILLE_PRESETS[0].1[0])
        );
    }

    #[test]
    fn matched_visuals_alone_drive_the_frame_clock() {
        let spinner = StatusVisual {
            kind: VisualKind::BrailleAnimation {
                preset: "spinner".to_owned(),
                frames_per_second: 8,
            },
            style: VisualStyle::default(),
        };
        let glyph = StatusVisual {
            kind: VisualKind::Glyph("!".to_owned()),
            style: VisualStyle::default(),
        };
        let progress = StatusVisual {
            kind: VisualKind::ProgressFrames {
                frames: vec!["a".into(), "b".into()],
            },
            style: VisualStyle::default(),
        };
        assert_eq!(next_frame_delay([], MotionLevel::Full, 0), None);
        assert_eq!(
            next_frame_delay([&glyph, &progress], MotionLevel::Full, 0),
            None
        );
        assert_eq!(
            next_frame_delay([&glyph, &spinner], MotionLevel::Full, 0),
            Some(125)
        );
        assert_eq!(
            next_frame_delay([&spinner], MotionLevel::Reduced, 0),
            Some(500)
        );
        assert_eq!(next_frame_delay([&spinner], MotionLevel::Off, 0), None);
    }

    #[test]
    fn visual_descriptions_read_as_prose() {
        let mut visual = StatusVisual {
            kind: VisualKind::BuiltIn("waiting_approval".to_owned()),
            style: VisualStyle {
                color: Some("danger".to_owned()),
                bold: true,
                dim: false,
            },
        };
        assert_eq!(
            visual_description(&visual),
            "the built-in waiting approval icon, in danger, bold"
        );
        visual.kind = VisualKind::FrameAnimation {
            frames: vec!["a".into(), "b".into(), "c".into()],
            frames_per_second: 4,
            ping_pong: true,
        };
        visual.style = VisualStyle::default();
        assert_eq!(
            visual_description(&visual),
            "a 3-frame animation at 4 frames per second, back and forth"
        );
        visual.kind = VisualKind::Hidden;
        visual.style.dim = true;
        assert_eq!(visual_description(&visual), "no icon, dim");
    }

    #[test]
    fn supplementary_facts_round_spend_to_whole_cents() {
        let facts = supplementary_facts(true, Some(1.2349));
        assert_eq!(facts.frozen, Some(true));
        assert_eq!(facts.session_cost_cents, Some(123));
        let facts = supplementary_facts(false, Some(-0.4));
        assert_eq!(facts.session_cost_cents, Some(0));
        assert_eq!(supplementary_facts(false, None).session_cost_cents, None);
    }

    #[test]
    fn redraw_clock_respects_motion_and_disabled_rules() {
        let mut rules = default_rules();
        for rule in &mut rules.rules {
            rule.enabled = false;
        }
        rules.fallback = None;
        assert_eq!(animation_frame_delay(&rules, MotionLevel::Full, 0), None);
        rules.rules[0].enabled = true;
        rules.rules[0].then.kind = VisualKind::BrailleAnimation {
            preset: "spinner".into(),
            frames_per_second: 8,
        };
        assert_eq!(
            animation_frame_delay(&rules, MotionLevel::Full, 0),
            Some(125)
        );
        assert_eq!(
            animation_frame_delay(&rules, MotionLevel::Reduced, 0),
            Some(500)
        );
        assert_eq!(animation_frame_delay(&rules, MotionLevel::Off, 0), None);
        rules.rules[0].then.kind = VisualKind::BrailleAnimation {
            preset: "spinner".into(),
            frames_per_second: 3,
        };
        assert_eq!(
            animation_frame_delay(&rules, MotionLevel::Full, 333),
            Some(1)
        );
        assert_eq!(
            animation_frame_delay(&rules, MotionLevel::Full, 334),
            Some(333)
        );
        rules.rules[1].enabled = true;
        rules.rules[1].then.kind = VisualKind::BrailleAnimation {
            preset: "loop".into(),
            frames_per_second: 8,
        };
        // At 333 ms, the slower animation changes before the faster one.
        assert_eq!(
            animation_frame_delay(&rules, MotionLevel::Full, 333),
            Some(1)
        );
        // At 334 ms, the faster animation has the next boundary at 375 ms.
        assert_eq!(
            animation_frame_delay(&rules, MotionLevel::Full, 334),
            Some(41)
        );
    }

    #[test]
    fn frame_widths_are_consistent_and_control_characters_are_rejected() {
        assert!(validate_frames(&["a".into(), "b".into()], 2).is_ok());
        assert!(validate_frames(&["a".into(), "🌕".into()], 2).is_err());
        assert!(validate_frames(&["a".into(), "\n".into()], 2).is_err());
    }

    #[test]
    fn animation_obeys_motion_off_and_progress_tracks_the_percent() {
        let animation = VisualKind::FrameAnimation {
            frames: vec!["a".into(), "b".into()],
            frames_per_second: 2,
            ping_pong: false,
        };
        assert_eq!(frame(&animation, 500, MotionLevel::Full, None), Some("b"));
        assert_eq!(frame(&animation, 500, MotionLevel::Off, None), Some("a"));
        let fast = VisualKind::FrameAnimation {
            frames: vec!["a".into(), "b".into(), "c".into()],
            frames_per_second: 8,
            ping_pong: false,
        };
        assert_eq!(frame(&fast, 500, MotionLevel::Full, None), Some("b"));
        assert_eq!(frame(&fast, 500, MotionLevel::Reduced, None), Some("b"));
        assert_eq!(frame(&fast, 250, MotionLevel::Full, None), Some("c"));
        assert_eq!(frame(&fast, 250, MotionLevel::Reduced, None), Some("a"));
        let progress = VisualKind::ProgressFrames {
            frames: vec!["a".into(), "b".into(), "c".into()],
        };
        assert_eq!(
            frame(&progress, 0, MotionLevel::Off, Some(100.0)),
            Some("c")
        );
        assert_eq!(frame(&progress, 0, MotionLevel::Full, None), Some("a"));
    }

    #[test]
    fn default_rules_show_approval_in_one_span() {
        let status = ilium_core::PaneStatus::Agent(ilium_core::AgentState {
            class: ilium_core::AgentClass::Codex,
            turn: ilium_core::AgentTurn::WaitingApproval,
            goal: None,
            completion_unread: false,
        });
        let icons = crate::icon_settings::IconSettings::default();
        let rules = default_rules();
        let span = status_span(StatusSpanContext {
            rules: &rules,
            status: &status,
            progress: &[],
            scheduled_input: false,
            shell_output: None,
            activity_stale: None,
            supplementary: Default::default(),
            screen_matches: &|_| Observation::Unknown,
            icons: &icons,
            use_stable_glyphs: false,
            elapsed_millis: 0,
            motion: MotionLevel::Off,
        });
        assert_eq!(
            span.content,
            icons.glyph(crate::icon_settings::IconTarget::WaitingApproval)
        );
    }
}
