//! Terminal-cell validation and frame selection for custom status visuals.

use ilium_core::status_rule_expression::{
    first_match, Observation, PaneFacts, ScreenPattern, StatusRules, StatusVisual, VisualKind,
};
use ilium_core::{
    representative_progress, PaneProgress, PaneStatus, ProgressMonitorHealth, ShellOutputPhase,
};
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

/// A ready-made glyph-frame animation the editor's picker offers, so users do
/// not type moon phases or clock hours by hand. Every family's frames share
/// one terminal width (one or two cells), checked by a test.
pub(crate) struct FrameFamily {
    /// Stable name, used in descriptions.
    pub name: &'static str,
    pub frames: &'static [&'static str],
    pub frames_per_second: u8,
    pub ping_pong: bool,
}

/// Glyph-frame animation families, in picker order. The first one is the
/// picker's starting point.
pub(crate) const FRAME_FAMILIES: &[FrameFamily] = &[
    FrameFamily {
        name: "quarters",
        frames: &["◐", "◓", "◑", "◒"],
        frames_per_second: 4,
        ping_pong: false,
    },
    FrameFamily {
        name: "moon phases",
        frames: &["🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"],
        frames_per_second: 4,
        ping_pong: false,
    },
    FrameFamily {
        name: "clock hours",
        frames: &[
            "🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚", "🕛",
        ],
        frames_per_second: 2,
        ping_pong: false,
    },
    FrameFamily {
        name: "arrows",
        frames: &["↑", "↗", "→", "↘", "↓", "↙", "←", "↖"],
        frames_per_second: 8,
        ping_pong: false,
    },
    FrameFamily {
        name: "hearts",
        frames: &["💗", "💓", "💖"],
        frames_per_second: 3,
        ping_pong: true,
    },
    FrameFamily {
        name: "blocks",
        frames: &["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"],
        frames_per_second: 8,
        ping_pong: true,
    },
];

/// The family whose frames are exactly `frames`, if any.
pub(crate) fn frame_family_of(frames: &[String]) -> Option<&'static FrameFamily> {
    FRAME_FAMILIES.iter().find(|family| {
        family.frames.len() == frames.len()
            && family
                .frames
                .iter()
                .zip(frames)
                .all(|(expected, actual)| *expected == actual.as_str())
    })
}

impl FrameFamily {
    /// The visual kind that plays this family at its own speed.
    pub(crate) fn visual_kind(&self) -> VisualKind {
        VisualKind::FrameAnimation {
            frames: self
                .frames
                .iter()
                .map(|frame| (*frame).to_owned())
                .collect(),
            frames_per_second: self.frames_per_second,
            ping_pong: self.ping_pong,
        }
    }
}

/// Health of the representative progress monitor, for tinting progress
/// frames the way the progress bar marks an unsteady monitor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MonitorHealthTone {
    #[default]
    Healthy,
    Degraded,
    Failed,
}

/// What the pane's representative progress monitor contributes to a visual:
/// the percent that picks a progress frame, and the monitor's health.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ProgressCue {
    /// `None` when there is no monitor or its progress is not measurable
    /// (indeterminate, or a placeholder zero with no estimate).
    pub percent: Option<f32>,
    pub health: MonitorHealthTone,
}

impl ProgressCue {
    /// No monitor or no measurable percent, healthy.
    pub(crate) const UNMEASURED: Self = Self {
        percent: None,
        health: MonitorHealthTone::Healthy,
    };

    /// A healthy monitor at `percent`; used by previews.
    pub(crate) const fn measured(percent: f32) -> Self {
        Self {
            percent: Some(percent),
            health: MonitorHealthTone::Healthy,
        }
    }

    /// The cue of the monitor that speaks for the pane (see
    /// `ilium_core::representative_progress`).
    pub(crate) fn from_monitors(monitors: &[PaneProgress]) -> Self {
        let Some(monitor) = representative_progress(monitors) else {
            return Self::UNMEASURED;
        };
        Self {
            percent: monitor
                .has_measurable_progress()
                .then_some(monitor.report.percent),
            health: match monitor.monitor_health {
                ProgressMonitorHealth::Healthy => MonitorHealthTone::Healthy,
                ProgressMonitorHealth::Degraded { .. } => MonitorHealthTone::Degraded,
                ProgressMonitorHealth::Failed { .. } => MonitorHealthTone::Failed,
            },
        }
    }
}

/// Pulse phases per second: two at Full motion (a half-second beat), one at
/// Reduced (slower), none at Off, where a pulsing visual stays static.
fn pulse_rate(motion: MotionLevel) -> Option<u8> {
    match motion {
        MotionLevel::Full => Some(2),
        MotionLevel::Reduced => Some(1),
        MotionLevel::Off => None,
    }
}

/// Whether the visual pulses: its style asks for it, or it shows progress
/// frames without a measurable percent (so frame 0 of an unmeasured task
/// reads differently from a measured 0 %). A hidden visual has nothing to
/// pulse.
fn pulses(visual: &StatusVisual, cue: ProgressCue) -> bool {
    match visual.kind {
        VisualKind::Hidden => false,
        VisualKind::ProgressFrames { .. } if cue.percent.is_none() => true,
        _ => visual.style.pulse,
    }
}

/// The soft half of the pulse beat at `elapsed_millis`.
fn pulse_is_soft(elapsed_millis: u64, motion: MotionLevel) -> bool {
    pulse_rate(motion)
        .is_some_and(|rate| (elapsed_millis.saturating_mul(u64::from(rate)) / 1_000) % 2 == 1)
}

/// Milliseconds from `elapsed_millis` to the next tick of a clock running at
/// `ticks_per_second`, at least 1.
fn delay_to_next_tick(ticks_per_second: u8, elapsed_millis: u128) -> Option<u64> {
    if ticks_per_second == 0 {
        return None;
    }
    let speed = u128::from(ticks_per_second);
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

/// Time until the next potentially visible frame of any enabled rule or the
/// fallback. The settings previews render every rule, so they use this
/// whole-set clock, assuming the unmeasured progress cue (the one that can
/// pulse); the live tree uses `next_frame_delay` over the visuals its panes
/// actually matched, with each pane's own cue.
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
            .chain(rules.fallback.iter())
            .map(|visual| (visual, ProgressCue::UNMEASURED)),
        motion,
        elapsed_millis,
    )
}

/// Time until the next frame or pulse boundary of the given visuals,
/// honouring the motion level (Reduced caps every animation at 2 frames per
/// second and slows the pulse, Off animates nothing). `None` when no visual
/// moves, so an idle tree schedules no redraws.
pub(crate) fn next_frame_delay<'a>(
    visuals: impl IntoIterator<Item = (&'a StatusVisual, ProgressCue)>,
    motion: MotionLevel,
    elapsed_millis: u128,
) -> Option<u64> {
    if motion == MotionLevel::Off {
        return None;
    }
    visuals
        .into_iter()
        .flat_map(|(visual, cue)| {
            let frame_rate = match &visual.kind {
                VisualKind::BrailleAnimation {
                    frames_per_second, ..
                }
                | VisualKind::FrameAnimation {
                    frames_per_second, ..
                } => Some(if motion == MotionLevel::Reduced {
                    (*frames_per_second).min(2)
                } else {
                    *frames_per_second
                }),
                _ => None,
            };
            let pulse = pulses(visual, cue).then(|| pulse_rate(motion)).flatten();
            [frame_rate, pulse]
        })
        .flatten()
        .filter_map(|rate| delay_to_next_tick(rate, elapsed_millis))
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
        ProgressCue::from_monitors(progress),
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
        } => {
            let back_and_forth = if *ping_pong { ", back and forth" } else { "" };
            match frame_family_of(frames) {
                Some(family) => format!(
                    "the {} animation at {frames_per_second} frames per second{back_and_forth}",
                    family.name
                ),
                None => format!(
                    "a {}-frame animation at {frames_per_second} frames per second{back_and_forth}",
                    frames.len()
                ),
            }
        }
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
    if visual.style.pulse && !matches!(visual.kind, VisualKind::Hidden) {
        text.push_str(", pulsing");
    }
    text
}

/// The foreground of a named theme slot or `#rrggbb` colour; `None` for an
/// unknown name (validation rejects it, a hand-edited config may hold it).
fn rule_colour(color: &str) -> Option<Color> {
    let tones = crate::theme::status_tones();
    match color {
        "danger" => Some(tones.danger),
        "warning" => Some(tones.warning),
        "success" => Some(tones.success),
        "info" => Some(tones.info),
        "accent" => Some(crate::theme::accent_bg()),
        "muted" => Some(tones.muted),
        value if value.len() == 7 && value.starts_with('#') => u32::from_str_radix(&value[1..], 16)
            .ok()
            .map(|rgb| Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)),
        _ => None,
    }
}

/// Render the same visual in the rules editor and in the live tree.
///
/// Beyond the rule's own colour and weight: a pulsing visual (style pulse,
/// or progress frames with no measurable percent) alternates its authored
/// weight with a dim phase on the shared clock, slower at Reduced motion and
/// static at Off, where unmeasured progress frames stay dim instead; and
/// progress frames take the warning tone while their monitor is degraded and
/// the danger tone once it has failed.
pub(crate) fn visual_span(
    visual: &StatusVisual,
    icons: &crate::icon_settings::IconSettings,
    use_stable_glyphs: bool,
    elapsed_millis: u64,
    motion: MotionLevel,
    progress: ProgressCue,
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
        kind => frame(kind, elapsed_millis, motion, progress.percent).map(str::to_owned),
    };
    let mut style = Style::new();
    if let Some(colour) = visual.style.color.as_deref().and_then(rule_colour) {
        style = style.fg(colour);
    }
    if matches!(visual.kind, VisualKind::ProgressFrames { .. }) {
        let tones = crate::theme::status_tones();
        match progress.health {
            MonitorHealthTone::Healthy => {}
            MonitorHealthTone::Degraded => style = style.fg(tones.warning),
            MonitorHealthTone::Failed => style = style.fg(tones.danger),
        }
    }
    if visual.style.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if visual.style.dim {
        style = style.add_modifier(Modifier::DIM);
    }
    if pulses(visual, progress) {
        let unmeasured_progress =
            matches!(visual.kind, VisualKind::ProgressFrames { .. }) && progress.percent.is_none();
        let soft = if motion == MotionLevel::Off {
            unmeasured_progress
        } else {
            pulse_is_soft(elapsed_millis, motion)
        };
        if soft {
            style = style
                .remove_modifier(Modifier::BOLD)
                .add_modifier(Modifier::DIM);
        }
    }
    Span::styled(glyph.unwrap_or_default(), style)
}

#[cfg(test)]
mod tests {
    use super::{
        animation_frame_delay, frame, frame_family_of, next_frame_delay, status_span,
        validate_frames, validate_visual, visual_description, visual_span, MonitorHealthTone,
        ProgressCue, StatusSpanContext, BRAILLE_PRESETS, FRAME_FAMILIES,
    };
    use crate::config::MotionLevel;
    use ilium_core::status_rule_expression::{
        default_rules, Observation, StatusVisual, VisualKind, VisualStyle,
    };
    use ratatui::style::Modifier;
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
        let measured = ProgressCue::measured(40.0);
        assert_eq!(next_frame_delay([], MotionLevel::Full, 0), None);
        assert_eq!(
            next_frame_delay(
                [(&glyph, measured), (&progress, measured)],
                MotionLevel::Full,
                0
            ),
            None
        );
        assert_eq!(
            next_frame_delay(
                [(&glyph, measured), (&spinner, measured)],
                MotionLevel::Full,
                0
            ),
            Some(125)
        );
        assert_eq!(
            next_frame_delay([(&spinner, measured)], MotionLevel::Reduced, 0),
            Some(500)
        );
        assert_eq!(
            next_frame_delay([(&spinner, measured)], MotionLevel::Off, 0),
            None
        );
    }

    #[test]
    fn pulse_beats_on_the_shared_clock_slower_when_reduced_and_never_when_off() {
        let icons = crate::icon_settings::IconSettings::default();
        let pulsing = StatusVisual {
            kind: VisualKind::Glyph("!".to_owned()),
            style: VisualStyle {
                color: Some("danger".to_owned()),
                bold: true,
                dim: false,
                pulse: true,
            },
        };
        let cue = ProgressCue::UNMEASURED;
        let style_at = |elapsed: u64, motion: MotionLevel| {
            visual_span(&pulsing, &icons, false, elapsed, motion, cue).style
        };
        // Full: half-second beat, bright then soft.
        let bright = style_at(0, MotionLevel::Full);
        assert!(bright.add_modifier.contains(Modifier::BOLD));
        assert!(!bright.add_modifier.contains(Modifier::DIM));
        let soft = style_at(500, MotionLevel::Full);
        assert!(soft.add_modifier.contains(Modifier::DIM));
        assert!(!soft.add_modifier.contains(Modifier::BOLD));
        assert_eq!(soft.fg, bright.fg, "the pulse keeps the rule colour");
        assert_eq!(style_at(1_000, MotionLevel::Full), bright);
        // Reduced: one-second beat.
        assert_eq!(style_at(500, MotionLevel::Reduced), bright);
        assert_eq!(style_at(1_000, MotionLevel::Reduced), soft);
        // Off: the authored style, at any time.
        for elapsed in [0, 500, 1_000, 123_456] {
            assert_eq!(style_at(elapsed, MotionLevel::Off), bright);
        }
        // The pulse alone wakes the clock, at the pulse rate.
        assert_eq!(
            next_frame_delay([(&pulsing, cue)], MotionLevel::Full, 0),
            Some(500)
        );
        assert_eq!(
            next_frame_delay([(&pulsing, cue)], MotionLevel::Full, 499),
            Some(1)
        );
        assert_eq!(
            next_frame_delay([(&pulsing, cue)], MotionLevel::Reduced, 0),
            Some(1_000)
        );
        assert_eq!(
            next_frame_delay([(&pulsing, cue)], MotionLevel::Off, 0),
            None
        );
        // A hidden visual has nothing to pulse.
        let hidden = StatusVisual {
            kind: VisualKind::Hidden,
            style: pulsing.style.clone(),
        };
        assert_eq!(
            next_frame_delay([(&hidden, cue)], MotionLevel::Full, 0),
            None
        );
        assert_eq!(visual_description(&hidden), "no icon, in danger, bold");
        assert_eq!(
            visual_description(&pulsing),
            "the glyph !, in danger, bold, pulsing"
        );
    }

    #[test]
    fn unmeasured_progress_frames_pulse_and_unsteady_monitors_tint() {
        let icons = crate::icon_settings::IconSettings::default();
        let tones = crate::theme::status_tones();
        let progress = StatusVisual {
            kind: VisualKind::ProgressFrames {
                frames: vec!["a".into(), "b".into(), "c".into()],
            },
            style: VisualStyle {
                color: Some("info".to_owned()),
                ..VisualStyle::default()
            },
        };
        let span = |elapsed: u64, motion: MotionLevel, cue: ProgressCue| {
            visual_span(&progress, &icons, false, elapsed, motion, cue)
        };
        // Measured: the percent picks the frame; no pulse, no clock.
        let measured = span(500, MotionLevel::Full, ProgressCue::measured(0.0));
        assert_eq!(measured.content, "a");
        assert_eq!(measured.style.fg, Some(tones.info));
        assert!(!measured.style.add_modifier.contains(Modifier::DIM));
        assert_eq!(
            next_frame_delay(
                [(&progress, ProgressCue::measured(0.0))],
                MotionLevel::Full,
                0
            ),
            None
        );
        // Unmeasured: frame 0 with the pulse, so it differs from 0 %.
        let unmeasured = ProgressCue::UNMEASURED;
        assert_eq!(span(0, MotionLevel::Full, unmeasured).content, "a");
        assert!(!span(0, MotionLevel::Full, unmeasured)
            .style
            .add_modifier
            .contains(Modifier::DIM));
        assert!(span(500, MotionLevel::Full, unmeasured)
            .style
            .add_modifier
            .contains(Modifier::DIM));
        assert_eq!(
            next_frame_delay([(&progress, unmeasured)], MotionLevel::Full, 0),
            Some(500)
        );
        // Motion Off: static, but dim, still distinct from a measured 0 %.
        for elapsed in [0, 500] {
            let frozen = span(elapsed, MotionLevel::Off, unmeasured);
            assert_eq!(frozen.content, "a");
            assert!(frozen.style.add_modifier.contains(Modifier::DIM));
        }
        // Motion Off still shows the measured progress frame.
        assert_eq!(
            span(0, MotionLevel::Off, ProgressCue::measured(100.0)).content,
            "c"
        );
        // Degraded and failed monitors override the rule colour.
        let degraded = ProgressCue {
            percent: Some(50.0),
            health: MonitorHealthTone::Degraded,
        };
        assert_eq!(
            span(0, MotionLevel::Full, degraded).style.fg,
            Some(tones.warning)
        );
        assert_eq!(span(0, MotionLevel::Full, degraded).content, "b");
        let failed = ProgressCue {
            percent: None,
            health: MonitorHealthTone::Failed,
        };
        assert_eq!(
            span(0, MotionLevel::Full, failed).style.fg,
            Some(tones.danger)
        );
        // The tint applies to progress frames only.
        let glyph = StatusVisual {
            kind: VisualKind::Glyph("!".to_owned()),
            style: progress.style.clone(),
        };
        assert_eq!(
            visual_span(&glyph, &icons, false, 0, MotionLevel::Full, degraded)
                .style
                .fg,
            Some(tones.info)
        );
    }

    #[test]
    fn progress_cue_follows_the_representative_monitor() {
        use ilium_core::{
            PaneProgress, ProgressMonitorHealth, ProgressTaskReport, ProgressTaskStatus,
        };
        assert_eq!(ProgressCue::from_monitors(&[]), ProgressCue::UNMEASURED);
        let report = |percent: f32, indeterminate: bool| {
            let mut report = ProgressTaskReport::new(
                "job".to_owned(),
                ProgressTaskStatus::Running,
                percent,
                "Building".to_owned(),
                String::new(),
                None,
            )
            .unwrap();
            report.indeterminate = indeterminate;
            report
        };
        let monitor = PaneProgress::new(1, report(42.0, false), 1).unwrap();
        assert_eq!(
            ProgressCue::from_monitors(std::slice::from_ref(&monitor)),
            ProgressCue::measured(42.0)
        );
        let placeholder = PaneProgress::new(2, report(0.0, false), 1).unwrap();
        assert_eq!(
            ProgressCue::from_monitors(&[placeholder]),
            ProgressCue::UNMEASURED
        );
        let indeterminate = PaneProgress::new(3, report(30.0, true), 1).unwrap();
        assert_eq!(ProgressCue::from_monitors(&[indeterminate]).percent, None);
        let mut degraded = monitor;
        degraded.monitor_health = ProgressMonitorHealth::Degraded {
            consecutive_failures: 2,
            last_error: "timeout".to_owned(),
        };
        assert_eq!(
            ProgressCue::from_monitors(&[degraded]).health,
            MonitorHealthTone::Degraded
        );
    }

    #[test]
    fn every_frame_family_validates_and_is_recognised() {
        assert!(FRAME_FAMILIES.len() >= 5);
        let mut names = std::collections::HashSet::new();
        for family in FRAME_FAMILIES {
            assert!(names.insert(family.name), "{} is unique", family.name);
            let visual = StatusVisual {
                kind: family.visual_kind(),
                style: VisualStyle::default(),
            };
            // Equal widths of one or two cells, 2 to 13 frames, a valid speed.
            assert_eq!(validate_visual(&visual, 2), Ok(()), "{}", family.name);
            let width = family.frames[0].width();
            assert!((1..=2).contains(&width), "{}", family.name);
            assert!(
                family.frames.iter().all(|frame| frame.width() == width),
                "{}",
                family.name
            );
            let VisualKind::FrameAnimation { frames, .. } = &visual.kind else {
                unreachable!("visual_kind builds a frame animation");
            };
            assert_eq!(
                frame_family_of(frames).map(|found| found.name),
                Some(family.name)
            );
            assert!(visual_description(&visual).starts_with(&format!("the {} ", family.name)));
        }
        assert!(frame_family_of(&["a".to_owned(), "b".to_owned()]).is_none());
        assert!(frame_family_of(&[]).is_none());
    }

    #[test]
    fn visual_descriptions_read_as_prose() {
        let mut visual = StatusVisual {
            kind: VisualKind::BuiltIn("waiting_approval".to_owned()),
            style: VisualStyle {
                color: Some("danger".to_owned()),
                bold: true,
                dim: false,
                pulse: false,
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
    fn redraw_clock_respects_motion_and_disabled_rules() {
        let mut rules = default_rules();
        for rule in &mut rules.rules {
            rule.enabled = false;
        }
        rules.fallback = None;
        assert_eq!(animation_frame_delay(&rules, MotionLevel::Full, 0), None);
        // The frame maths below ignore the pulse the first default carries.
        rules.rules[0].then.style.pulse = false;
        rules.rules[1].then.style.pulse = false;
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
