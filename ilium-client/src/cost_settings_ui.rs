//! The "Agent Cost" settings tab.
//!
//! One scrollable page in three parts: shared selectors choosing what to
//! measure and how "expensive" is decided (with a live threshold preview),
//! the parameter of the chosen calibration, and one block per display
//! option with an enable checkbox and a visibility selector. Geometry is
//! produced once by [`view`] and shared by rendering, mouse hit testing,
//! help anchors and keyboard scrolling, so they cannot drift apart.

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::cost_model::{
    calibrate, format_amount, format_rate, format_window, meter_string, sparkline_glyphs,
    Calibration, CalibrationInputs, CalibrationNote, CostLevel, CostMetric, ScaleBasis,
};
use crate::cost_overlay::level_color;
use crate::cost_settings::{
    is_preset, CostDisplay, CostRow, CostSettings, BURN_PRESETS, FIXED_PRESETS, QUOTA_BURN_PRESETS,
    QUOTA_FIXED_PRESETS,
};
use crate::theme;
use crate::value_control::{leader_span, NUMBER_DECREMENT_GLYPH, NUMBER_INCREMENT_GLYPH};

/// Left margin shared with the other settings tabs.
const INSET: u16 = 2;
/// Width of the label column of a control line.
const LABEL_WIDTH: u16 = 30;
/// Indent of descriptions under a card or option.
const BODY_INDENT: u16 = 6;

/// Vertical extent of one selectable row within the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowSpan {
    pub row: CostRow,
    pub first_line: u16,
    pub last_line: u16,
    /// Line carrying the selector/number control, for parameter rows.
    pub control_line: u16,
    /// Column (relative to the content area) where the control starts.
    pub control_x: u16,
}

pub struct CostView {
    pub lines: Vec<Line<'static>>,
    pub rows: Vec<RowSpan>,
}

/// A click, resolved to the row it landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostHit {
    pub index: usize,
    pub row: CostRow,
    /// `-1`/`1` for the decrement/increment half of a stepper, `0` for a
    /// toggle or radio activation.
    pub direction: i32,
}

pub fn rows(app: &App) -> Vec<CostRow> {
    CostRow::rows(&app.cost_settings)
}

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && UnicodeWidthStr::width(candidate.as_str()) > width {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn section_bar(title: &str, compact_title: &str, width: u16) -> Line<'static> {
    let width = usize::from(width);
    let title = if width >= UnicodeWidthStr::width(title) + 5 {
        title
    } else {
        compact_title
    };
    let title_width = UnicodeWidthStr::width(title);
    let available_title_width = width.saturating_sub(5);
    if width < 6 {
        return Line::from(Span::styled(
            title.chars().take(width).collect::<String>(),
            Style::new()
                .fg(theme::accent_bg())
                .add_modifier(Modifier::BOLD),
        ));
    }
    let title = if title_width > available_title_width {
        title
            .chars()
            .take(available_title_width)
            .collect::<String>()
    } else {
        title.to_owned()
    };
    let title_width = UnicodeWidthStr::width(title.as_str());
    let rule_width = width.saturating_sub(5 + title_width);
    let border = theme::border_style(false);
    Line::from(vec![
        Span::styled("╭─ ", border),
        Span::styled(
            title,
            Style::new()
                .fg(theme::accent_bg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", border),
        Span::styled("─".repeat(rule_width), border),
        Span::styled("╮", border),
    ])
}

fn format_cuts(metric: CostMetric, cuts: &[f64; 4], per_hour: bool) -> String {
    cuts.iter()
        .map(|cut| {
            if per_hour {
                format_rate(metric, *cut)
            } else {
                format_amount(metric, *cut, false)
            }
            .replace(".00", "")
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn calibration_blurb(calibration: Calibration, settings: &CostSettings) -> String {
    let is_quota = settings.metric == CostMetric::Quota;
    match calibration {
        Calibration::FixedBands => if is_quota {
            "Fixed thresholds in percent of the plan window an agent used up: the same for \
             every agent, every day. Predictable and easy to explain, but arbitrary: a heavy \
             user sees everything red, a light one nothing."
        } else {
            "Fixed dollar thresholds: the same for every agent, every day. Predictable and easy \
             to explain, but arbitrary: a heavy user sees everything red, a light one nothing."
        }
        .to_owned(),
        Calibration::PeerRelative => {
            "Each agent is rated against the median of the agents open right now. Good at \
             spotting the outlier among your agents; says nothing about whether the use is \
             large in absolute terms, and the scale moves as agents open and close."
                .to_owned()
        }
        Calibration::OwnHistory => format!(
            "Each agent is rated against the 25th, 50th, 80th and 95th percentile of your own \
             sessions over the last {} days. Needs no configuration and fits how you actually \
             work. Until enough history is scanned the fixed bands stand in.",
            settings.history_days
        ),
        Calibration::Budget => if is_quota {
            "Indicators fill toward a per-agent share of the plan window, and a red ! appears \
             once an agent passes it. Useful when one window is all you have; nothing is rated \
             until you set a share."
        } else {
            "Indicators fill toward a dollar budget per agent, and a red ! appears once an \
             agent passes it. The most meaningful choice when you have a limit to respect; \
             nothing is rated until you set one."
        }
        .to_owned(),
        Calibration::BurnRate => if is_quota {
            "Rates how fast an agent is draining the plan window right now, in percentage \
             points per hour, not its running total. Catches a runaway loop early; an old \
             session that is now idle reads as cheap."
        } else {
            "Rates how fast an agent is spending right now in dollars per hour, not its running \
             total. Catches a runaway loop early; an old expensive session that is now idle \
             reads as cheap."
        }
        .to_owned(),
    }
}

fn metric_blurb(metric: CostMetric) -> &'static str {
    match metric {
        CostMetric::Dollars => {
            "Estimated dollars at API list prices, from the agent's own transcript. Works for \
             Claude Code and Codex alike. Subscription plans are not billed this amount, so \
             read it as relative weight."
        }
        CostMetric::Quota => {
            "Percentage points of the plan's rate-limit window used up while the agent ran, as \
             Codex records them. Codex reports the whole account's use, so agents running at \
             the same time are counted together. Claude Code transcripts carry no quota, so \
             those agents show nothing under this metric."
        }
    }
}

/// The threshold ladder `calibration` would use with the data available now.
fn ladder_line(app: &App, calibration: Calibration) -> Line<'static> {
    let overlay = app.cost_tracker.overlay();
    let settings = &app.cost_settings;
    let metric = settings.metric;
    let inputs = CalibrationInputs {
        fixed_cuts: settings.active_fixed_cuts(),
        burn_cuts: settings.active_burn_cuts(),
        budget: settings.active_budget(),
        peer_totals: &overlay.peer_totals,
        history_sorted: (overlay.history_sessions > 0 || !overlay.is_history_scanning)
            .then_some(overlay.history_sorted.as_slice()),
    };
    let calibrated = calibrate(calibration, &inputs);
    let per_hour = calibrated.basis == ScaleBasis::BurnUsdPerHour;
    let mut spans = vec![Span::raw(" ".repeat(usize::from(BODY_INDENT)))];
    for index in 0..5 {
        let level = CostLevel::new(index);
        let bound = if index == 0 {
            let first = calibrated.scale.cuts[0];
            format!(
                "<{}",
                if per_hour {
                    format_rate(metric, first)
                } else {
                    format_amount(metric, first, false)
                }
            )
        } else {
            let cut = calibrated.scale.cuts[index - 1];
            format!(
                "≥{}",
                if per_hour {
                    format_rate(metric, cut)
                } else {
                    format_amount(metric, cut, false)
                }
            )
        };
        spans.push(Span::styled(
            format!("{} ", level.glyph()),
            Style::new()
                .fg(level_color(level))
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(format!("{bound}   "), dim()));
    }
    match calibrated.note {
        CalibrationNote::Ready => {}
        CalibrationNote::HistoryUnavailable => {
            let note = if overlay.is_history_scanning {
                "scanning your sessions… fixed bands for now".to_owned()
            } else {
                format!(
                    "only {} past sessions found; fixed bands for now",
                    overlay.history_sessions
                )
            };
            spans.push(Span::styled(note, Style::new().fg(theme::accent_bg())));
        }
        CalibrationNote::NoPeers => spans.push(Span::styled(
            "no agent has spent anything yet; fixed bands for now",
            Style::new().fg(theme::accent_bg()),
        )),
    }
    Line::from(spans)
}

fn sample_for(display: CostDisplay, settings: &CostSettings) -> (String, CostLevel) {
    let level = CostLevel::new(2);
    let is_quota = settings.metric == CostMetric::Quota;
    let text = match display {
        CostDisplay::LevelGlyph => level.glyph().to_string(),
        CostDisplay::LevelDollars => format!(
            "{} {}",
            level.glyph(),
            if is_quota { "6.2%" } else { "$6.20" }
        ),
        CostDisplay::Meter => meter_string(Some(level)),
        CostDisplay::Sparkline => {
            let pattern = [0.0, 1.0, 2.0, 4.0, 7.0, 5.0, 3.0, 1.0];
            let cells = usize::from(settings.sparkline_cells);
            let values: Vec<f64> = (0..cells).map(|i| pattern[i % pattern.len()]).collect();
            sparkline_glyphs(&values)
        }
        CostDisplay::GroupTotals => format!(
            "{} {}",
            CostLevel::new(3).glyph(),
            if is_quota { "12%" } else { "$12.4" }
        ),
        CostDisplay::DetailCard => "Card beside the row".to_owned(),
        CostDisplay::HeaderTotal => format!("Σ {}", if is_quota { "52%" } else { "$52.3" }),
        CostDisplay::BurnMarker => format!("{} ↑", meter_string(Some(level))),
    };
    (text, level)
}

fn param_label(row: CostRow) -> &'static str {
    match row {
        CostRow::Metric(_) => "Measure",
        CostRow::Calibration(_) => "Rating",
        CostRow::QuotaWindow => "Quota window",
        CostRow::FixedPreset => "Fixed bands",
        CostRow::HistoryDays => "History window",
        CostRow::Budget => "Budget per agent",
        CostRow::BurnPreset => "Burn-rate bands",
        CostRow::SparklineWindow => "Sparkline window",
        CostRow::SparklineCells => "Sparkline width",
        CostRow::SortByCost => "Sort tree by cost",
        _ => "",
    }
}

fn param_value(row: CostRow, app: &App) -> String {
    let settings = &app.cost_settings;
    let custom = |is_known: bool, text: String| {
        if is_known {
            text
        } else {
            format!("{text} (custom)")
        }
    };
    match row {
        CostRow::Metric(_) => settings.metric.label().to_owned(),
        CostRow::Calibration(_) => settings.calibration.label().to_owned(),
        CostRow::QuotaWindow => settings.quota_window.label().to_owned(),
        CostRow::FixedPreset => {
            let (presets, cuts): (&[[f64; 4]], _) = match settings.metric {
                CostMetric::Dollars => (&FIXED_PRESETS, settings.fixed_cuts),
                CostMetric::Quota => (&QUOTA_FIXED_PRESETS, settings.quota_fixed_cuts),
            };
            custom(
                is_preset(presets, cuts),
                format_cuts(settings.metric, &cuts, false),
            )
        }
        CostRow::BurnPreset => {
            let (presets, cuts): (&[[f64; 4]], _) = match settings.metric {
                CostMetric::Dollars => (&BURN_PRESETS, settings.burn_cuts),
                CostMetric::Quota => (&QUOTA_BURN_PRESETS, settings.quota_burn_cuts),
            };
            custom(
                is_preset(presets, cuts),
                format_cuts(settings.metric, &cuts, true),
            )
        }
        CostRow::HistoryDays => format!("{} days", settings.history_days),
        CostRow::Budget => {
            let budget = settings.active_budget();
            // Whole numbers stay whole; decimals only when the budget has them.
            match (settings.metric, budget.fract() == 0.0) {
                (CostMetric::Dollars, true) => format!("${budget:.0}"),
                (CostMetric::Dollars, false) => format!("${budget:.2}"),
                (CostMetric::Quota, true) => format!("{budget:.0}%"),
                (CostMetric::Quota, false) => format!("{budget:.1}%"),
            }
        }
        CostRow::SparklineWindow => format_window(settings.sparkline_window_minutes),
        CostRow::SparklineCells => format!("{} cells", settings.sparkline_cells),
        CostRow::SortByCost => {
            if settings.sort_by_cost {
                "On".to_owned()
            } else {
                "Off".to_owned()
            }
        }
        _ => String::new(),
    }
}

fn param_description(row: CostRow, app: &App) -> String {
    let overlay = app.cost_tracker.overlay();
    let is_quota = app.cost_settings.metric == CostMetric::Quota;
    match row {
        CostRow::Metric(_) => format!(
            "{} Choose whether Agent Cost measures estimated API dollars or Codex plan quota. The full list is available from +.",
            metric_blurb(app.cost_settings.metric)
        ),
        CostRow::Calibration(_) => format!(
            "{} The full list of rating methods is available from +.",
            calibration_blurb(app.cost_settings.calibration, &app.cost_settings)
        ),
        CostRow::QuotaWindow => {
            "Which Codex rate-limit window the quota figures follow: the short rolling window \
             or the longer weekly one. Press Enter or click to switch."
                .to_owned()
        }
        CostRow::FixedPreset => if is_quota {
            "Percent of the plan window at which the level steps up. Any four ascending values \
             can be set as quota_fixed_cuts under [cost] in config.toml."
        } else {
            "Dollar totals at which the level steps up. Any four ascending values can be \
             set as fixed_cuts under [cost] in config.toml."
        }
        .to_owned(),
        CostRow::BurnPreset => if is_quota {
            "Percentage points of the window per hour, measured over the last 15 minutes, at \
             which the level steps up. Custom values: quota_burn_cuts under [cost]."
        } else {
            "Dollars per hour, measured over the last 15 minutes, at which the level steps up. \
             Custom values: burn_cuts under [cost] in config.toml."
        }
        .to_owned(),
        CostRow::HistoryDays => {
            let state = if overlay.is_history_scanning {
                "scanning…".to_owned()
            } else {
                format!("{} sessions found", overlay.history_sessions)
            };
            format!(
                "How far back past sessions are read to learn your normal spend ({state}). \
                 Only transcript tails are read and results are cached, so rescans are cheap."
            )
        }
        CostRow::Budget => if is_quota {
            "Percent of the plan window one agent may use before it counts as over budget. \
             Custom values: quota_budget_percent under [cost] in config.toml."
        } else {
            "Dollars one agent may spend before it counts as over budget. Custom values: \
             budget_usd under [cost] in config.toml."
        }
        .to_owned(),
        CostRow::SparklineWindow => {
            "Time covered by the burn sparkline (six hours by default). Left/Right step through \
             common values from five minutes to thirty days; Enter types any exact window such \
             as 90, 45m, 10h or 3d, up to a year."
                .to_owned()
        }
        CostRow::SparklineCells => {
            "Number of time slices in the burn sparkline; each cell covers window ÷ width."
                .to_owned()
        }
        CostRow::SortByCost => {
            "Order every group and project by descending spend instead of the order chosen in \
             the User Interface tab. Manual order is untouched and returns when this is off."
                .to_owned()
        }
        _ => String::new(),
    }
}

/// Builds the whole page for `width` columns.
pub fn view(app: &App, selected_row: usize, width: u16) -> CostView {
    let settings = &app.cost_settings;
    let all_rows = rows(app);
    let is_selected = |row: CostRow| all_rows.get(selected_row) == Some(&row);
    let body_width = usize::from(width.saturating_sub(BODY_INDENT + 3)).max(20);
    let selected_style = theme::selected_style().add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans_out: Vec<RowSpan> = Vec::new();
    let control_x = INSET + LABEL_WIDTH;

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Agent Cost",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    for text in wrap(
        "See how much each agent has used. Dollar figures are estimated from the agent's own \
         transcript at API list prices (a leading ~ means some model had no known price); \
         quota figures are percentage points of the plan window, where Codex records them. \
         Every change is saved immediately.",
        usize::from(width.saturating_sub(INSET + 2)),
    ) {
        lines.push(Line::from(Span::styled(format!("  {text}"), dim())));
    }
    lines.push(Line::from(""));

    // ---- what is measured
    lines.push(section_bar("WHAT IS MEASURED?", "MEASURED", width));
    lines.push(Line::from(""));
    let metric_row = CostRow::Metric(settings.metric);
    push_control(
        &mut lines,
        &mut spans_out,
        app,
        metric_row,
        is_selected(metric_row),
        control_x,
        width,
    );
    if all_rows.contains(&CostRow::QuotaWindow) {
        push_control(
            &mut lines,
            &mut spans_out,
            app,
            CostRow::QuotaWindow,
            is_selected(CostRow::QuotaWindow),
            control_x,
            width,
        );
    }

    // ---- how "expensive" is decided
    lines.push(section_bar(
        "HOW IS \"EXPENSIVE\" DECIDED?",
        "RATING",
        width,
    ));
    lines.push(Line::from(""));
    let calibration_row = CostRow::Calibration(settings.calibration);
    push_control(
        &mut lines,
        &mut spans_out,
        app,
        calibration_row,
        is_selected(calibration_row),
        control_x,
        width,
    );
    lines.push(ladder_line(app, settings.calibration));
    lines.push(Line::from(""));

    // The active calibration's parameter follows its selector.
    for parameter in [
        CostRow::FixedPreset,
        CostRow::HistoryDays,
        CostRow::Budget,
        CostRow::BurnPreset,
    ] {
        if all_rows.contains(&parameter) {
            push_control(
                &mut lines,
                &mut spans_out,
                app,
                parameter,
                is_selected(parameter),
                control_x,
                width,
            );
        }
    }

    // ---- what to show
    lines.push(section_bar(
        "WHAT TO SHOW ON EACH AGENT",
        "INDICATORS",
        width,
    ));
    for text in wrap(
        "Every indicator can be switched on independently, and each one is visible either always \
         or only while the pointer is over that entry. Indicators sit just left of the row's \
         action buttons.",
        usize::from(width.saturating_sub(INSET + 2)),
    ) {
        lines.push(Line::from(Span::styled(format!("  {text}"), dim())));
    }
    lines.push(Line::from(""));
    for display in CostDisplay::ALL {
        let option = settings.option(display);
        let toggle_row = CostRow::Display(display);
        let visibility_row = CostRow::Visibility(display);
        let first_line = lines.len() as u16;
        let checkbox = if option.enabled { "[x]" } else { "[ ]" };
        let label_style = if is_selected(toggle_row) {
            selected_style
        } else if option.enabled {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            dim()
        };
        let (sample, level) = sample_for(display, settings);
        let label = format!("  {checkbox} {}", display.label());
        let padding = usize::from(control_x).saturating_sub(UnicodeWidthStr::width(label.as_str()));
        lines.push(Line::from(vec![
            Span::styled(label, label_style),
            Span::raw(" ".repeat(padding.max(2))),
            Span::styled(
                sample,
                if option.enabled {
                    Style::new().fg(level_color(level))
                } else {
                    dim()
                },
            ),
        ]));
        spans_out.push(RowSpan {
            row: toggle_row,
            first_line,
            last_line: first_line,
            control_line: first_line,
            control_x: 0,
        });

        let visibility_line = lines.len() as u16;
        let visibility_label = "      Visible";
        let padding = usize::from(control_x)
            .saturating_sub(UnicodeWidthStr::width(visibility_label))
            .max(2);
        let control_style = if is_selected(visibility_row) {
            selected_style
        } else if option.enabled {
            Style::new().fg(theme::accent_bg())
        } else {
            dim()
        };
        lines.push(Line::from(vec![
            Span::styled(
                visibility_label,
                if option.enabled { Style::new() } else { dim() },
            ),
            leader_span(padding),
            Span::styled(
                format!("← {} + →", option.visibility.label()),
                control_style,
            ),
        ]));
        spans_out.push(RowSpan {
            row: visibility_row,
            first_line: visibility_line,
            last_line: visibility_line,
            control_line: visibility_line,
            control_x,
        });
        for text in wrap(display.description(), body_width) {
            lines.push(Line::from(Span::styled(
                format!("{}{text}", " ".repeat(usize::from(BODY_INDENT))),
                dim(),
            )));
        }
        lines.push(Line::from(""));
    }

    // ---- sparkline and ordering parameters
    lines.push(section_bar("SPARKLINE AND ORDER", "SPARKLINE", width));
    lines.push(Line::from(""));
    for row in [
        CostRow::SparklineWindow,
        CostRow::SparklineCells,
        CostRow::SortByCost,
    ] {
        push_control(
            &mut lines,
            &mut spans_out,
            app,
            row,
            is_selected(row),
            control_x,
            width,
        );
    }
    for text in wrap(
        "Up/Down select · Left/Right step · + opens lists/increments · - decreases · * edits numbers · Enter opens/edits · ? help",
        usize::from(width.saturating_sub(BODY_INDENT + 3)).max(20),
    ) {
        lines.push(Line::from(Span::styled(
            format!("{}{text}", " ".repeat(usize::from(BODY_INDENT))),
            dim(),
        )));
    }
    CostView {
        lines,
        rows: spans_out,
    }
}

fn push_control(
    lines: &mut Vec<Line<'static>>,
    spans_out: &mut Vec<RowSpan>,
    app: &App,
    row: CostRow,
    selected: bool,
    control_x: u16,
    width: u16,
) {
    let line = lines.len() as u16;
    let label = format!("  {}", param_label(row));
    let padding = usize::from(control_x)
        .saturating_sub(UnicodeWidthStr::width(label.as_str()))
        .max(2);
    let control_style = if selected {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme::accent_bg())
    };
    let label_style = if selected {
        Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::new()
    };
    let is_number = matches!(
        row,
        CostRow::HistoryDays | CostRow::Budget | CostRow::SparklineWindow | CostRow::SparklineCells
    );
    let shown = if is_number {
        format!(
            "{NUMBER_DECREMENT_GLYPH} {} {NUMBER_INCREMENT_GLYPH} *",
            param_value(row, app)
        )
    } else {
        format!("← {} + →", param_value(row, app))
    };
    lines.push(Line::from(vec![
        Span::styled(label, label_style),
        leader_span(padding),
        Span::styled(shown, control_style),
    ]));
    for text in wrap(
        &param_description(row, app),
        usize::from(width.saturating_sub(BODY_INDENT + 3)).max(20),
    ) {
        lines.push(Line::from(Span::styled(
            format!("{}{text}", " ".repeat(usize::from(BODY_INDENT))),
            dim(),
        )));
    }
    if row == CostRow::SparklineWindow {
        if let Some(input) = &app.cost_window_input {
            lines.push(Line::from(vec![
                Span::raw(" ".repeat(usize::from(BODY_INDENT))),
                Span::styled(
                    format!(" {input}▏"),
                    theme::selected_style().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  type minutes, or 90m, 6h, 2d · Enter saves · Esc cancels",
                    dim(),
                ),
            ]));
        }
    }
    let last_line = lines.len() as u16 - 1;
    lines.push(Line::from(""));
    spans_out.push(RowSpan {
        row,
        first_line: line,
        last_line,
        control_line: line,
        control_x,
    });
}

pub fn value_control(
    area: Rect,
    scroll: u16,
    span: &RowSpan,
    app: &App,
) -> Option<crate::value_control::ValueControl> {
    use crate::value_control::{ControlKind, ControlSpec, ValueControl};
    let number = app.cost_settings.number_text(span.row);
    if number.is_none() && !crate::value_cost::is_choice(span.row) {
        return None;
    }
    let offset = span.control_line.checked_sub(scroll)?;
    if offset >= area.height || span.control_x >= area.width {
        return None;
    }
    let rect = Rect::new(
        area.x + span.control_x,
        area.y + offset,
        area.width - span.control_x,
        1,
    );
    let value = match span.row {
        CostRow::Visibility(display) => app
            .cost_settings
            .option(display)
            .visibility
            .label()
            .to_owned(),
        _ => param_value(span.row, app),
    };
    let (previous_enabled, next_enabled) = number.as_ref().map_or((true, true), |current| {
        (
            app.cost_settings.stepped_number_text(span.row, -1).as_ref() != Some(current),
            app.cost_settings.stepped_number_text(span.row, 1).as_ref() != Some(current),
        )
    });
    Some(ValueControl::new(
        rect,
        ControlSpec {
            kind: if number.is_some() {
                ControlKind::Number
            } else {
                ControlKind::Choice
            },
            label: "",
            value: &value,
            label_width: 0,
            previous_enabled,
            next_enabled,
            open_enabled: true,
        },
    ))
}

pub fn value_hit(
    area: Rect,
    scroll: u16,
    position: Position,
    button: crate::value_control::PointerButton,
    app: &App,
) -> Option<(usize, CostRow, crate::value_control::ControlAction)> {
    let rows = rows(app);
    view(app, 0, area.width).rows.iter().find_map(|span| {
        let action = value_control(area, scroll, span, app)?.hit(position, button)?;
        Some((
            rows.iter().position(|row| *row == span.row)?,
            span.row,
            action,
        ))
    })
}

pub fn render(frame: &mut Frame, area: Rect, app: &App, selected_row: usize, scroll: u16) {
    let view = view(app, selected_row, area.width);
    crate::settings_ui::render_scrollable(frame, area, view.lines, scroll);
    let rows = rows(app);
    for span in &view.rows {
        let Some(control) = value_control(area, scroll, span, app) else {
            continue;
        };
        let selected = rows.get(selected_row) == Some(&span.row);
        let style = if selected {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::accent_bg())
        };
        frame.render_widget(
            ratatui::widgets::Paragraph::new(" ".repeat(usize::from(control.geometry().row.width)))
                .style(style),
            control.geometry().row,
        );
        control.render(
            frame,
            crate::value_control::ControlStyles {
                background: style,
                label: style,
                value: style,
                button: style,
                disabled: style.add_modifier(Modifier::DIM),
            },
        );
    }
}

pub fn max_scroll(app: &App, selected_row: usize, content_area: Rect) -> u16 {
    (view(app, selected_row, content_area.width).lines.len() as u16)
        .saturating_sub(content_area.height)
}

/// Scroll position that keeps the selected row fully inside the viewport.
pub fn scroll_for_selection(
    app: &App,
    content_area: Rect,
    selected_row: usize,
    scroll: u16,
) -> u16 {
    let view = view(app, selected_row, content_area.width);
    let Some(span) = rows(app)
        .get(selected_row)
        .and_then(|row| view.rows.iter().find(|span| span.row == *row))
    else {
        return scroll;
    };
    let height = content_area.height.max(1);
    // Keep the section heading in view when the first row of a section is
    // selected by scrolling to the top margin of the page.
    if span.first_line < scroll {
        span.first_line.saturating_sub(1)
    } else if span.last_line >= scroll + height {
        (span.last_line + 1).saturating_sub(height)
    } else {
        scroll
    }
}

/// Resolves a click at `position` to the row it landed on.
pub fn hit(content_area: Rect, scroll: u16, position: Position, app: &App) -> Option<CostHit> {
    hit_with_button(
        content_area,
        scroll,
        position,
        crate::value_control::PointerButton::Left,
        app,
    )
}

pub fn hit_with_button(
    content_area: Rect,
    scroll: u16,
    position: Position,
    button: crate::value_control::PointerButton,
    app: &App,
) -> Option<CostHit> {
    if !content_area.contains(position) {
        return None;
    }
    let view = view(app, 0, content_area.width);
    let virtual_line = position.y - content_area.y + scroll;
    let virtual_x = position.x - content_area.x;
    let all_rows = rows(app);
    let span = view
        .rows
        .iter()
        .find(|span| (span.first_line..=span.last_line).contains(&virtual_line))?;
    let index = all_rows.iter().position(|row| *row == span.row)?;
    let is_number = matches!(
        span.row,
        CostRow::HistoryDays | CostRow::Budget | CostRow::SparklineWindow | CostRow::SparklineCells
    );
    let is_choice = matches!(
        span.row,
        CostRow::Metric(_)
            | CostRow::Calibration(_)
            | CostRow::FixedPreset
            | CostRow::BurnPreset
            | CostRow::Visibility(_)
    );
    let direction = if is_number || is_choice {
        // Only the painted control steps; the label and description lines
        // of a stepper are inert so a stray click never changes a value.
        if virtual_line != span.control_line || virtual_x < span.control_x {
            return None;
        }
        if virtual_x == span.control_x {
            -1
        } else if is_choice
            && button == crate::value_control::PointerButton::Right
            && virtual_x > span.control_x
        {
            -1
        } else {
            1
        }
    } else {
        0
    };
    Some(CostHit {
        index,
        row: span.row,
        direction,
    })
}

/// The help topic anchors for every row, as (id, line, row).
pub fn help_anchors(app: &App, width: u16) -> Vec<(&'static str, u16, CostRow)> {
    view(app, 0, width)
        .rows
        .into_iter()
        .map(|span| (span.row.help_id(), span.first_line, span.row))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost_settings::CostDisplay;

    fn app() -> App {
        App::new("cost-settings-test".to_owned(), std::env::temp_dir())
    }

    fn text(view: &CostView) -> String {
        view.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn cost_settings_sections_use_rounded_title_bars_without_widening_the_page() {
        let app = app();
        for width in [110, 28] {
            let view = view(&app, 0, width);
            let bars = view
                .lines
                .iter()
                .filter(|line| line.to_string().starts_with("╭─"))
                .collect::<Vec<_>>();
            assert_eq!(bars.len(), 4);
            assert!(bars.iter().all(|line| {
                line.width() <= usize::from(width) && line.to_string().ends_with('╮')
            }));
            if width >= 110 {
                for title in [
                    "WHAT IS MEASURED?",
                    "HOW IS \"EXPENSIVE\" DECIDED?",
                    "WHAT TO SHOW ON EACH AGENT",
                    "SPARKLINE AND ORDER",
                ] {
                    assert!(bars.iter().any(|line| line.to_string().contains(title)));
                }
            }
        }

        use ratatui::{backend::TestBackend, Terminal};
        let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app, 0, 0))
            .unwrap();
        crate::ui_capture::save("cost-settings-section-bars-110x40", &terminal);
    }

    #[test]
    fn shared_cost_chrome_renders_and_hits_every_numeric_and_choice_variant() {
        use crate::value_control::{
            ControlAction, PointerButton, NUMBER_DECREMENT_GLYPH, NUMBER_INCREMENT_GLYPH,
        };
        use ratatui::{backend::TestBackend, Terminal};
        let mut app = app();
        for metric in CostMetric::ALL {
            for calibration in Calibration::ALL {
                app.cost_settings.metric = metric;
                app.cost_settings.calibration = calibration;
                let view = view(&app, 0, 110);
                let height = view.lines.len() as u16 + 1;
                let area = Rect::new(0, 0, 110, height);
                let mut terminal = Terminal::new(TestBackend::new(110, height)).unwrap();
                terminal
                    .draw(|frame| render(frame, area, &app, 0, 0))
                    .unwrap();
                for span in &view.rows {
                    if app.cost_settings.number_spec(span.row).is_none()
                        && !crate::value_cost::is_choice(span.row)
                    {
                        continue;
                    }
                    let control = value_control(area, 0, span, &app).unwrap();
                    let geometry = control.geometry();
                    let is_number = app.cost_settings.number_spec(span.row).is_some();
                    let (previous, next, open, action) = if is_number {
                        (
                            NUMBER_DECREMENT_GLYPH,
                            NUMBER_INCREMENT_GLYPH,
                            "*",
                            ControlAction::EditNumber,
                        )
                    } else {
                        ("←", "→", "+", ControlAction::OpenChoices)
                    };
                    for (rect, symbol) in [
                        (geometry.previous, previous),
                        (geometry.next, next),
                        (geometry.open, open),
                    ] {
                        assert_eq!(
                            terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                            symbol,
                            "{:?}",
                            span.row
                        );
                    }
                    let index = rows(&app).iter().position(|row| *row == span.row).unwrap();
                    assert_eq!(
                        value_hit(
                            area,
                            0,
                            Position::new(geometry.open.x, geometry.open.y),
                            PointerButton::Left,
                            &app
                        ),
                        Some((index, span.row, action))
                    );
                    if is_number {
                        let current = app.cost_settings.number_text(span.row).unwrap();
                        assert_eq!(
                            value_hit(
                                area,
                                0,
                                Position::new(geometry.previous.x, geometry.previous.y),
                                PointerButton::Left,
                                &app
                            ),
                            app.cost_settings
                                .stepped_number_text(span.row, -1)
                                .as_deref()
                                .filter(|stepped| *stepped != current)
                                .map(|_| (index, span.row, ControlAction::Decrement))
                        );
                        assert_eq!(
                            value_hit(
                                area,
                                0,
                                Position::new(geometry.next.x, geometry.next.y),
                                PointerButton::Left,
                                &app
                            ),
                            app.cost_settings
                                .stepped_number_text(span.row, 1)
                                .as_deref()
                                .filter(|stepped| *stepped != current)
                                .map(|_| (index, span.row, ControlAction::Increment))
                        );
                        assert_eq!(
                            geometry.value.x - geometry.value_slot.x,
                            (geometry.value_slot.width - geometry.value.width) / 2
                        );
                    } else {
                        let value = Position::new(geometry.value.x, geometry.value.y);
                        assert_eq!(
                            value_hit(area, 0, value, PointerButton::Left, &app),
                            Some((index, span.row, ControlAction::NextChoice))
                        );
                        assert_eq!(
                            value_hit(area, 0, value, PointerButton::Right, &app),
                            Some((index, span.row, ControlAction::PreviousChoice))
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn cost_choice_right_click_reverses_and_plus_opens_the_complete_dialog() {
        use crate::app::{Mode, SettingsState, SettingsTab};
        use crate::cost_model::CostMetric;
        use crate::value_dialog::ValueDialogState;
        use crate::value_dialog_host::ValueTarget;
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "cost-choice-dialog-test".to_owned(),
            directory.path().to_owned(),
        );
        app.config_dir = Some(directory.path().to_owned());
        app.cost_settings.metric = CostMetric::Dollars;
        app.set_screen_area(Rect::new(0, 0, 130, 260));
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Cost,
            ..SettingsState::default()
        });
        let Mode::Settings(state) = &app.mode else {
            panic!("Cost settings fixture");
        };
        let content_area =
            crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, &app, state)
                .content_area;
        let current_metric = app.cost_settings.metric;
        let current_index = CostMetric::ALL
            .iter()
            .position(|metric| *metric == current_metric)
            .expect("current metric is in its catalog");
        let previous_metric =
            CostMetric::ALL[(current_index + CostMetric::ALL.len() - 1) % CostMetric::ALL.len()];
        let current_row = CostRow::Metric(current_metric);
        let current_span = view(&app, 0, content_area.width)
            .rows
            .into_iter()
            .find(|span| span.row == current_row)
            .expect("metric selector is visible");
        let current_control = value_control(content_area, 0, &current_span, &app)
            .expect("metric selector uses shared control")
            .geometry();

        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: current_control.value.x,
                row: current_control.value.y,
                modifiers: KeyModifiers::empty(),
            },
        );
        assert_eq!(app.cost_settings.metric, previous_metric);

        let row = CostRow::Metric(app.cost_settings.metric);
        let span = view(&app, 0, content_area.width)
            .rows
            .into_iter()
            .find(|span| span.row == row)
            .expect("metric selector remains visible after stepping");
        let open = value_control(content_area, 0, &span, &app)
            .expect("metric selector uses shared control")
            .geometry()
            .open;
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: open.x,
                row: open.y,
                modifiers: KeyModifiers::empty(),
            },
        );

        let Mode::ValueDialog(host) = &app.mode else {
            panic!("plus opens the shared value dialog");
        };
        assert!(matches!(
            &host.target,
            ValueTarget::Cost { target, .. } if target.row == row
        ));
        let ValueDialogState::Choice(choice) = &host.dialog else {
            panic!("cost selector opens a choice list");
        };
        assert_eq!(choice.options().len(), CostMetric::ALL.len());

        let document = crate::value_dialog::dialog_layout(app.layout.screen_area).document;
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: document.x.saturating_add(2),
                row: document.y,
                modifiers: KeyModifiers::empty(),
            },
        );
        assert_eq!(app.cost_settings.metric, CostMetric::Dollars);
        app.settle_filesystem_for_test();
        assert_eq!(
            crate::config::load(directory.path()).unwrap().cost.metric,
            CostMetric::Dollars
        );
    }

    #[test]
    fn clicking_cost_number_star_opens_keyboard_entry_dialog() {
        use crate::app::{Mode, SettingsState, SettingsTab};
        use crate::cost_model::Calibration;
        use crate::value_dialog::ValueDialogState;
        use crate::value_dialog_host::ValueTarget;
        use crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        };

        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "cost-number-dialog-test".to_owned(),
            directory.path().to_owned(),
        );
        app.config_dir = Some(directory.path().to_owned());
        app.cost_settings.calibration = Calibration::Budget;
        app.set_screen_area(Rect::new(0, 0, 130, 260));
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Cost,
            ..SettingsState::default()
        });
        let Mode::Settings(state) = &app.mode else {
            panic!("Cost settings fixture");
        };
        let content_area =
            crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, &app, state)
                .content_area;
        let row = CostRow::Budget;
        let span = view(&app, 0, content_area.width)
            .rows
            .into_iter()
            .find(|span| span.row == row)
            .expect("budget number is visible in Budget calibration");
        let open = value_control(content_area, 0, &span, &app)
            .expect("budget uses shared numeric control")
            .geometry()
            .open;

        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: open.x,
                row: open.y,
                modifiers: KeyModifiers::empty(),
            },
        );

        let Mode::ValueDialog(host) = &app.mode else {
            panic!("star opens the shared number dialog");
        };
        assert!(matches!(
            &host.target,
            ValueTarget::Cost { target, .. } if target.row == row
        ));
        assert!(matches!(&host.dialog, ValueDialogState::Number(_)));

        let draft_length = match &app.mode {
            Mode::ValueDialog(host) => match &host.dialog {
                ValueDialogState::Number(number) => number.draft.buf.chars().count(),
                ValueDialogState::Choice(_) => unreachable!("number dialog was asserted above"),
            },
            _ => unreachable!("number dialog was asserted above"),
        };
        let press = |app: &mut App, code| {
            crate::keys::handle_event(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        };
        for _ in 0..draft_length {
            press(&mut app, KeyCode::Backspace);
        }
        for character in "17.125".chars() {
            press(&mut app, KeyCode::Char(character));
        }
        press(&mut app, KeyCode::Enter);
        app.settle_filesystem_for_test();
        assert_eq!(app.cost_settings.budget_usd, 17.125);
        assert_eq!(
            crate::config::load(directory.path()).unwrap().cost,
            app.cost_settings
        );
        assert!(matches!(app.mode, Mode::Settings(_)));
    }

    #[test]
    fn page_explains_every_calibration_and_shows_live_ladders() {
        let app = app();
        let view = view(&app, 0, 110);
        let page = text(&view);
        assert!(page.contains("← Relative to your history + →"));
        assert!(
            page.contains("<$1"),
            "fixed ladder starts below the first cut: {page}"
        );
        assert!(page.contains("≥$50"));
        assert!(page.contains("History window"));
    }

    #[test]
    fn every_display_option_has_a_checkbox_visibility_and_sample() {
        let app = app();
        let page = text(&view(&app, 0, 110));
        for display in CostDisplay::ALL {
            assert!(page.contains(display.label()), "{}", display.label());
        }
        assert!(page.contains("[x] Five-cell meter"));
        assert!(page.contains("[ ] Level glyph"));
        assert!(page.contains("← Only when hovering the entry + →"));
        assert!(page.contains("Sparkline window"));
        assert!(
            page.contains("- 6 h + *"),
            "default window is six hours: {page}"
        );
    }

    #[test]
    fn row_spans_cover_every_row_once_in_order() {
        let app = app();
        let view = view(&app, 0, 110);
        let expected = rows(&app);
        let found: Vec<CostRow> = view.rows.iter().map(|span| span.row).collect();
        let mut sorted_expected = expected.clone();
        let mut sorted_found = found.clone();
        let key = |row: &CostRow| format!("{row:?}");
        sorted_expected.sort_by_key(key);
        sorted_found.sort_by_key(key);
        assert_eq!(sorted_found, sorted_expected);
        let lines: Vec<u16> = view.rows.iter().map(|span| span.first_line).collect();
        assert!(lines.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn clicks_toggle_options_and_step_parameters() {
        let app = app();
        let area = Rect::new(0, 0, 110, 200);
        let view = view(&app, 0, area.width);
        let span_of = |row: CostRow| *view.rows.iter().find(|span| span.row == row).unwrap();

        let meter = span_of(CostRow::Display(CostDisplay::Meter));
        let click = hit(area, 0, Position::new(40, meter.first_line), &app).unwrap();
        assert_eq!(
            (click.row, click.direction),
            (CostRow::Display(CostDisplay::Meter), 0)
        );

        let visibility = span_of(CostRow::Visibility(CostDisplay::Meter));
        assert!(hit(area, 0, Position::new(3, visibility.first_line), &app).is_none());
        let click = hit(
            area,
            0,
            Position::new(visibility.control_x, visibility.control_line),
            &app,
        )
        .unwrap();
        assert_eq!(click.row, CostRow::Visibility(CostDisplay::Meter));

        let window = span_of(CostRow::SparklineWindow);
        let left = hit(
            area,
            0,
            Position::new(window.control_x, window.control_line),
            &app,
        )
        .unwrap();
        let right = hit(
            area,
            0,
            Position::new(window.control_x + 6, window.control_line),
            &app,
        )
        .unwrap();
        assert_eq!((left.direction, right.direction), (-1, 1));
        assert!(
            hit(
                area,
                0,
                Position::new(window.control_x, window.control_line + 1),
                &app
            )
            .is_none(),
            "description lines do not step the value"
        );

        let selector = span_of(CostRow::Calibration(Calibration::OwnHistory));
        let click = hit(
            area,
            0,
            Position::new(selector.control_x + 1, selector.control_line),
            &app,
        )
        .unwrap();
        assert_eq!(click.row, CostRow::Calibration(Calibration::OwnHistory));
        assert_eq!(click.direction, 1);
    }

    #[test]
    fn selecting_a_calibration_reveals_its_parameter_row() {
        let mut app = app();
        assert!(!text(&view(&app, 0, 110)).contains("Budget per agent"));
        app.cost_settings.calibration = Calibration::Budget;
        let page = text(&view(&app, 0, 110));
        assert!(page.contains("Budget per agent"));
        assert!(page.contains("- $10 + *"));
        assert!(!page.contains("History window"));
    }

    #[test]
    fn scroll_follows_the_selection_in_both_directions() {
        let app = app();
        let area = Rect::new(0, 0, 100, 12);
        let all = rows(&app);
        let last = all.len() - 1;
        let down = scroll_for_selection(&app, area, last, 0);
        assert!(down > 0);
        let up = scroll_for_selection(&app, area, 0, down);
        assert!(up < down);
        assert!(max_scroll(&app, 0, area) > 0);
    }

    #[test]
    fn metric_selector_switches_every_unit_and_quota_window() {
        let mut app = app();
        let page = text(&view(&app, 0, 110));
        assert!(page.contains("WHAT IS MEASURED?"));
        assert!(page.contains("← API dollars + →"));
        assert!(!page.contains("Quota window"));
        assert!(page.contains("<$1.00"), "dollar ladders by default");

        app.cost_settings.metric = CostMetric::Quota;
        let view = view(&app, 0, 110);
        let page = text(&view);
        assert!(page.contains("← Plan quota + →"));
        assert!(page.contains("← Short window + →"));
        assert!(page.contains("<1.0%"), "fixed-band ladder is in percent");
        assert!(!page.contains("<$1.00"));
        assert!(page.contains("Claude Code transcripts carry no quota"));
        let metric_rows = view
            .rows
            .iter()
            .filter(|span| matches!(span.row, CostRow::Metric(_) | CostRow::QuotaWindow))
            .count();
        assert_eq!(metric_rows, 2);

        app.cost_settings.calibration = Calibration::Budget;
        let page = text(&super::view(&app, 0, 110));
        assert!(
            page.contains("- 10% + *"),
            "budget is a share of the window"
        );
        assert!(page.contains("quota_budget_percent"));
    }

    #[test]
    fn clicking_a_metric_control_steps_it_and_the_window_row_toggles() {
        let app = app();
        let area = Rect::new(0, 0, 110, 60);
        let view = view(&app, 0, 110);
        let metric = view
            .rows
            .iter()
            .find(|span| matches!(span.row, CostRow::Metric(_)))
            .unwrap();
        let control = value_control(area, 0, metric, &app).unwrap();
        let geometry = control.geometry();
        assert_eq!(
            value_hit(
                area,
                0,
                Position::new(geometry.value.x, geometry.value.y),
                crate::value_control::PointerButton::Left,
                &app,
            )
            .unwrap()
            .2,
            crate::value_control::ControlAction::NextChoice
        );

        let mut quota_app = app;
        quota_app.cost_settings.metric = CostMetric::Quota;
        let view = super::view(&quota_app, 0, 110);
        let window = view
            .rows
            .iter()
            .find(|span| span.row == CostRow::QuotaWindow)
            .unwrap();
        let click = hit(
            area,
            0,
            Position::new(window.control_x + 4, window.control_line),
            &quota_app,
        )
        .unwrap();
        assert_eq!(click.row, CostRow::QuotaWindow);
        assert_eq!(click.direction, 0, "the window is a toggle, not a stepper");
    }
}
