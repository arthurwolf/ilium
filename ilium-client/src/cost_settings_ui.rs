//! The "Agent Cost" settings tab.
//!
//! One scrollable page in three parts: radio cards choosing how "expensive"
//! is decided (each with a live preview of the thresholds it would use right
//! now), the parameter of the chosen calibration, and one block per display
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
    calibrate, format_usd, format_usd_per_hour, format_window, meter_string, sparkline_glyphs,
    Calibration, CalibrationInputs, CalibrationNote, CostLevel, ScaleBasis,
};
use crate::cost_overlay::level_color;
use crate::cost_settings::{
    is_preset, CostDisplay, CostRow, CostSettings, BURN_PRESETS, FIXED_PRESETS,
};
use crate::theme;

/// Left margin shared with the other settings tabs.
const INSET: u16 = 2;
/// Width of the label column of a control line.
const LABEL_WIDTH: u16 = 30;
/// Columns of the `‹` zone that decrements; the rest of the control
/// increments.
const DECREMENT_ZONE: u16 = 2;
/// Indent of descriptions under a card or option.
const BODY_INDENT: u16 = 6;

/// Vertical extent of one selectable row within the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowSpan {
    pub row: CostRow,
    pub first_line: u16,
    pub last_line: u16,
    /// Line carrying the `‹ value ›` control, for parameter rows.
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

fn format_cuts(cuts: &[f64; 4], per_hour: bool) -> String {
    cuts.iter()
        .map(|cut| {
            if per_hour {
                format_usd_per_hour(*cut)
            } else {
                format_usd(*cut, false)
            }
            .replace(".00", "")
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn calibration_blurb(calibration: Calibration, settings: &CostSettings) -> String {
    match calibration {
        Calibration::FixedBands => {
            "Fixed dollar thresholds: the same for every agent, every day. Predictable and easy \
             to explain, but arbitrary: a heavy user sees everything red, a light one nothing."
                .to_owned()
        }
        Calibration::PeerRelative => {
            "Each agent is rated against the median of the agents open right now. Good at \
             spotting the outlier among your agents; says nothing about whether the spend is \
             large in absolute terms, and the scale moves as agents open and close."
                .to_owned()
        }
        Calibration::OwnHistory => format!(
            "Each agent is rated against the 25th, 50th, 80th and 95th percentile of your own \
             sessions over the last {} days. Needs no configuration and fits how you actually \
             work. Until enough history is scanned the fixed bands stand in.",
            settings.history_days
        ),
        Calibration::Budget => {
            "Indicators fill toward a dollar budget per agent, and a red ! appears once an \
             agent passes it. The most meaningful choice when you have a limit to respect; \
             nothing is rated until you set one."
                .to_owned()
        }
        Calibration::BurnRate => {
            "Rates how fast an agent is spending right now in dollars per hour, not its running \
             total. Catches a runaway loop early; an old expensive session that is now idle \
             reads as cheap."
                .to_owned()
        }
    }
}

/// The threshold ladder `calibration` would use with the data available now.
fn ladder_line(app: &App, calibration: Calibration) -> Line<'static> {
    let overlay = app.cost_tracker.overlay();
    let settings = &app.cost_settings;
    let inputs = CalibrationInputs {
        fixed_cuts: settings.fixed_cuts,
        burn_cuts: settings.burn_cuts,
        budget_usd: settings.budget_usd,
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
                    format_usd_per_hour(first)
                } else {
                    format_usd(first, false)
                }
            )
        } else {
            let cut = calibrated.scale.cuts[index - 1];
            format!(
                "≥{}",
                if per_hour {
                    format_usd_per_hour(cut)
                } else {
                    format_usd(cut, false)
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
    let text = match display {
        CostDisplay::LevelGlyph => level.glyph().to_string(),
        CostDisplay::LevelDollars => format!("{} $6.20", level.glyph()),
        CostDisplay::Meter => meter_string(Some(level)),
        CostDisplay::Sparkline => {
            let pattern = [0.0, 1.0, 2.0, 4.0, 7.0, 5.0, 3.0, 1.0];
            let cells = usize::from(settings.sparkline_cells);
            let values: Vec<f64> = (0..cells).map(|i| pattern[i % pattern.len()]).collect();
            sparkline_glyphs(&values)
        }
        CostDisplay::GroupTotals => format!("{} $12.4", CostLevel::new(3).glyph()),
        CostDisplay::DetailCard => "Card beside the row".to_owned(),
        CostDisplay::HeaderTotal => "Σ $52.3".to_owned(),
        CostDisplay::BurnMarker => format!("{} ↑", meter_string(Some(level))),
    };
    (text, level)
}

fn param_label(row: CostRow) -> &'static str {
    match row {
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
        CostRow::FixedPreset => custom(
            is_preset(&FIXED_PRESETS, settings.fixed_cuts),
            format_cuts(&settings.fixed_cuts, false),
        ),
        CostRow::BurnPreset => custom(
            is_preset(&BURN_PRESETS, settings.burn_cuts),
            format_cuts(&settings.burn_cuts, true),
        ),
        CostRow::HistoryDays => format!("{} days", settings.history_days),
        CostRow::Budget => {
            // Whole dollars stay whole; cents only when the budget has them.
            if settings.budget_usd.fract() == 0.0 {
                format!("${:.0}", settings.budget_usd)
            } else {
                format!("${:.2}", settings.budget_usd)
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
    match row {
        CostRow::FixedPreset => {
            "Dollar totals at which the level steps up. Any four ascending values can be \
             set as fixed_cuts under [cost] in config.toml."
                .to_owned()
        }
        CostRow::BurnPreset => {
            "Dollars per hour, measured over the last 15 minutes, at which the level steps up. \
             Custom values: burn_cuts under [cost] in config.toml."
                .to_owned()
        }
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
        CostRow::Budget => {
            "Dollars one agent may spend before it counts as over budget. Custom values: \
             budget_usd under [cost] in config.toml."
                .to_owned()
        }
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
    let accent = Style::new()
        .fg(theme::accent_bg())
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans_out: Vec<RowSpan> = Vec::new();
    let control_x = INSET + LABEL_WIDTH;

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Agent Cost",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    for text in wrap(
        "See how much each agent has spent. Figures are estimated from the agent's own \
         transcript at API list prices (a leading ~ means some model had no known price). \
         Every change is saved immediately.",
        usize::from(width.saturating_sub(INSET + 2)),
    ) {
        lines.push(Line::from(Span::styled(format!("  {text}"), dim())));
    }
    lines.push(Line::from(""));

    // ---- how "expensive" is decided
    lines.push(Line::from(Span::styled(
        "  HOW IS \"EXPENSIVE\" DECIDED?",
        accent,
    )));
    lines.push(Line::from(""));
    for calibration in Calibration::ALL {
        let row = CostRow::Calibration(calibration);
        let first_line = lines.len() as u16;
        let active = settings.calibration == calibration;
        let marker = if active { "◉" } else { "○" };
        let default_tag = if calibration == Calibration::OwnHistory {
            "  (default)"
        } else {
            ""
        };
        let title = format!("  {marker} {}{default_tag}", calibration.label());
        let title_style = if is_selected(row) {
            selected_style
        } else if active {
            accent
        } else {
            Style::new()
        };
        lines.push(Line::from(Span::styled(title, title_style)));
        let body_style = if active { Style::new() } else { dim() };
        for text in wrap(&calibration_blurb(calibration, settings), body_width) {
            lines.push(Line::from(Span::styled(
                format!("{}{text}", " ".repeat(usize::from(BODY_INDENT))),
                body_style,
            )));
        }
        lines.push(ladder_line(app, calibration));
        let last_line = lines.len() as u16 - 1;
        spans_out.push(RowSpan {
            row,
            first_line,
            last_line,
            control_line: first_line,
            control_x: 0,
        });
        lines.push(Line::from(""));

        // The parameter of the active calibration follows its card.
        if active {
            for parameter in [
                CostRow::FixedPreset,
                CostRow::HistoryDays,
                CostRow::Budget,
                CostRow::BurnPreset,
            ] {
                if !all_rows.contains(&parameter) {
                    continue;
                }
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
    }

    // ---- what to show
    lines.push(Line::from(Span::styled(
        "  WHAT TO SHOW ON EACH AGENT",
        accent,
    )));
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
            Span::raw(" ".repeat(padding)),
            Span::styled(format!("‹ {} ›", option.visibility.label()), control_style),
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
    lines.push(Line::from(Span::styled("  SPARKLINE AND ORDER", accent)));
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
    lines.push(Line::from(Span::styled(
        "  Up/Down select · Left/Right or Enter change · a ? next to a row explains it",
        dim(),
    )));
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
    lines.push(Line::from(vec![
        Span::styled(label, label_style),
        Span::raw(" ".repeat(padding)),
        Span::styled(format!("‹ {} ›", param_value(row, app)), control_style),
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

pub fn render(frame: &mut Frame, area: Rect, app: &App, selected_row: usize, scroll: u16) {
    let view = view(app, selected_row, area.width);
    crate::settings_ui::render_scrollable(frame, area, view.lines, scroll);
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
    let is_stepper = matches!(
        span.row,
        CostRow::FixedPreset
            | CostRow::HistoryDays
            | CostRow::Budget
            | CostRow::BurnPreset
            | CostRow::SparklineWindow
            | CostRow::SparklineCells
    );
    let direction = if is_stepper {
        // Only the `‹ value ›` control steps; the label and description lines
        // of a stepper are inert so a stray click never changes a value.
        if virtual_line != span.control_line || virtual_x < span.control_x {
            return None;
        }
        if virtual_x < span.control_x + DECREMENT_ZONE {
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
    fn page_explains_every_calibration_and_shows_live_ladders() {
        let app = app();
        let view = view(&app, 0, 110);
        let page = text(&view);
        for calibration in Calibration::ALL {
            assert!(
                page.contains(calibration.label()),
                "{}",
                calibration.label()
            );
        }
        assert!(page.contains("◉ Relative to your history  (default)"));
        assert!(page.contains("○ Fixed bands"));
        assert!(
            page.contains("<$1"),
            "fixed ladder starts below the first cut: {page}"
        );
        assert!(page.contains("≥$50"));
        assert!(page.contains("$/h") || page.contains("/h"));
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
        assert!(page.contains("‹ Only when hovering the entry ›"));
        assert!(page.contains("Sparkline window"));
        assert!(
            page.contains("‹ 6 h ›"),
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
        let click = hit(area, 0, Position::new(3, visibility.first_line), &app).unwrap();
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

        let card = span_of(CostRow::Calibration(Calibration::Budget));
        let click = hit(area, 0, Position::new(20, card.first_line + 1), &app).unwrap();
        assert_eq!(click.row, CostRow::Calibration(Calibration::Budget));
    }

    #[test]
    fn selecting_a_calibration_reveals_its_parameter_row() {
        let mut app = app();
        assert!(!text(&view(&app, 0, 110)).contains("Budget per agent"));
        app.cost_settings.calibration = Calibration::Budget;
        let page = text(&view(&app, 0, 110));
        assert!(page.contains("Budget per agent"));
        assert!(page.contains("‹ $10 ›"));
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
}
