//! The demo project every status display shares: the Agent Monitoring cards,
//! the rules editor's Demo preview and the "Why this icon" explanation all
//! render the same situations, taken from `ilium_core`'s catalogue so the
//! client never carries a second list of pane facts.
//!
//! The core catalogue describes facts; this module adds the two things only
//! the client can do: evaluate screen patterns (regular expressions are a
//! client concern) and turn the chosen visual into a styled span.

use std::sync::OnceLock;

use ilium_core::status_rule_expression::{
    DemoSituation, Observation, ScreenPattern, ScreenRegion,
};
use ratatui::text::Span;

use crate::agent_monitoring::AgentMonitoringMode;
use crate::app::App;

/// Every demo situation, built once per process.
pub(crate) fn situations() -> &'static [DemoSituation] {
    static SITUATIONS: OnceLock<Vec<DemoSituation>> = OnceLock::new();
    SITUATIONS.get_or_init(ilium_core::status_rule_expression::demo_situations)
}

/// Names of the demo situations, in catalogue order.
pub(crate) fn names() -> Vec<&'static str> {
    situations()
        .iter()
        .map(|situation| situation.name)
        .collect()
}

/// Match a screen pattern against one synthetic screen using the same regex
/// options and region semantics as the server. Compiled expressions are
/// cached so animation redraws stay cheap.
pub(crate) fn screen_match(screen: &str, pattern: &ScreenPattern) -> Observation {
    use std::{cell::RefCell, collections::HashMap};
    thread_local! {
        static EXPRESSIONS: RefCell<HashMap<(String, bool), Option<regex::Regex>>> = RefCell::new(HashMap::new());
    }
    let lines: Vec<_> = screen.lines().collect();
    let selected = match pattern.region {
        ScreenRegion::All => screen.to_owned(),
        ScreenRegion::FirstLines(count) => lines[..usize::from(count).min(lines.len())].join("\n"),
        ScreenRegion::LastLines(count) => {
            lines[lines.len().saturating_sub(usize::from(count))..].join("\n")
        }
    };
    EXPRESSIONS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= 128
            && !cache.contains_key(&(pattern.pattern.clone(), pattern.ignore_case))
        {
            cache.clear();
        }
        let expression = cache
            .entry((pattern.pattern.clone(), pattern.ignore_case))
            .or_insert_with(|| {
                if pattern.pattern.chars().count() > 256 {
                    return None;
                }
                regex::RegexBuilder::new(&pattern.pattern)
                    .case_insensitive(pattern.ignore_case)
                    .multi_line(true)
                    .size_limit(1 << 20)
                    .dfa_size_limit(1 << 20)
                    .build()
                    .ok()
            });
        match expression {
            Some(regex) if regex.is_match(&selected) => Observation::True,
            Some(_) => Observation::False,
            None => Observation::Unknown,
        }
    })
}

/// The status icon(s) the demo situation at `index` shows in `mode`: two
/// spans in Normal mode, one otherwise. An index past the catalogue renders
/// nothing.
pub(crate) fn preview_spans(
    app: &App,
    mode: AgentMonitoringMode,
    index: usize,
) -> Vec<Span<'static>> {
    let Some(situation) = situations().get(index) else {
        return Vec::new();
    };
    let icons = &app.ui_settings.icons;
    let elapsed_ms = app.started_at.elapsed().as_millis();
    if mode == AgentMonitoringMode::Custom {
        return vec![crate::status_rule_visual::status_span(
            crate::status_rule_visual::StatusSpanContext {
                rules: &app.custom_status_rules,
                status: &situation.status,
                progress: &situation.progress,
                scheduled_input: situation.scheduled_input,
                shell_output: situation.shell_output,
                activity_stale: situation.activity_stale,
                supplementary: situation.supplementary,
                screen_matches: &|pattern| screen_match(situation.screen, pattern),
                icons,
                use_stable_glyphs: app.ui_settings.use_stable_glyphs,
                elapsed_millis: u64::try_from(elapsed_ms).unwrap_or(u64::MAX),
                motion: app.ui_settings.motion_level,
            },
        )];
    }
    let signals = crate::agent_monitoring::displayed_pane_signals(
        mode,
        app.ui_settings.attention_progress_reports,
        app.ui_settings.attention_running_indicator,
        &situation.status,
        &situation.progress,
        situation.scheduled_input,
        situation.shell_output,
    );
    let objective = crate::status_icons::objective_span(
        signals.objective,
        icons,
        app.ui_settings.use_stable_glyphs,
    );
    let now_elapsed = if app.ui_settings.motion_level == crate::config::MotionLevel::Off {
        0
    } else {
        elapsed_ms
    };
    let now = if mode == AgentMonitoringMode::Attention
        && signals.now == ilium_core::NowSignal::Working
    {
        crate::tree_ui::attention_running_span(
            app.ui_settings.attention_running_indicator,
            icons,
            now_elapsed,
            app.ui_settings.use_stable_glyphs,
        )
    } else {
        crate::status_icons::now_span(
            signals.now,
            icons,
            now_elapsed,
            app.ui_settings.use_stable_glyphs,
        )
    };
    if mode == AgentMonitoringMode::Normal {
        vec![objective, Span::raw(" "), now]
    } else if objective.content.is_empty() {
        vec![now]
    } else {
        vec![objective]
    }
}

/// [`preview_spans`] folded into one span (styling survives only when the
/// mode produces a single span).
pub(crate) fn preview_span(app: &App, mode: AgentMonitoringMode, index: usize) -> Span<'static> {
    let mut spans = preview_spans(app, mode, index);
    if spans.len() == 1 {
        spans.remove(0)
    } else {
        Span::raw(
            spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{names, preview_spans, screen_match, situations};
    use crate::agent_monitoring::AgentMonitoringMode;
    use crate::app::App;
    use ilium_core::status_rule_expression::{Observation, ScreenPattern, ScreenRegion};

    #[test]
    fn the_demo_list_comes_from_the_core_catalogue_in_order() {
        let expected: Vec<&str> = ilium_core::status_rule_expression::demo_situations()
            .iter()
            .map(|situation| situation.name)
            .collect();
        assert_eq!(names(), expected);
        assert!(situations().len() >= 10);
        assert_eq!(situations()[0].name, "Working");
    }

    #[test]
    fn screen_patterns_respect_regions_case_and_invalid_expressions() {
        let screen = "Working on project\nTask progress: 50%";
        let pattern = |text: &str, ignore_case: bool, region: ScreenRegion| ScreenPattern {
            pattern: text.to_owned(),
            ignore_case,
            region,
        };
        assert_eq!(
            screen_match(screen, &pattern("progress", false, ScreenRegion::All)),
            Observation::True
        );
        assert_eq!(
            screen_match(screen, &pattern("PROGRESS", false, ScreenRegion::All)),
            Observation::False
        );
        assert_eq!(
            screen_match(screen, &pattern("PROGRESS", true, ScreenRegion::All)),
            Observation::True
        );
        assert_eq!(
            screen_match(
                screen,
                &pattern("progress", true, ScreenRegion::FirstLines(1))
            ),
            Observation::False
        );
        assert_eq!(
            screen_match(
                screen,
                &pattern("Working", true, ScreenRegion::LastLines(1))
            ),
            Observation::False
        );
        assert_eq!(
            screen_match(screen, &pattern("(", false, ScreenRegion::All)),
            Observation::Unknown
        );
    }

    #[test]
    fn every_situation_renders_in_every_mode_and_past_the_end_renders_nothing() {
        let app = App::new("test".to_string(), std::env::temp_dir());
        for index in 0..situations().len() {
            assert_eq!(
                preview_spans(&app, AgentMonitoringMode::Normal, index).len(),
                3
            );
            assert_eq!(
                preview_spans(&app, AgentMonitoringMode::Attention, index).len(),
                1
            );
            assert_eq!(
                preview_spans(&app, AgentMonitoringMode::Custom, index).len(),
                1
            );
        }
        assert!(preview_spans(&app, AgentMonitoringMode::Custom, situations().len()).is_empty());
    }
}
