//! Drawing of the spend indicators on tree rows.
//!
//! Pure presentation: [`segments_for_row`] decides *what* a row shows from the
//! user's settings and the hover state, and [`draw_segments`] paints those
//! segments into a ratatui buffer, right-aligned, without ever touching the
//! tree or I/O. The tree renderer owns where the strip ends (left of the hover
//! action buttons); this module only needs an exclusive right edge.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Clear;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::cost_model::{
    budget_fill, format_amount, format_quota, format_rate, format_tokens, format_usd,
    format_window, meter_string, percent_rank, sparkline_glyphs, Calibration, CalibrationNote,
    CostLevel, CostMetric,
};
use crate::cost_settings::{CostDisplay, CostSettings};
use crate::cost_tracker::{CostOverlay, PaneCost, RowCost};
use crate::theme;

/// Green to red, one colour per [`CostLevel`]. Fixed RGB: readable on the
/// dark and light schemes and on the accent background of a hovered row.
pub fn level_color(level: CostLevel) -> Color {
    match level.index() {
        0 => Color::Rgb(86, 200, 120),
        1 => Color::Rgb(160, 210, 70),
        2 => Color::Rgb(240, 200, 50),
        3 => Color::Rgb(245, 140, 40),
        _ => Color::Rgb(240, 70, 70),
    }
}

const PENDING_COLOR: Color = Color::Rgb(150, 150, 160);
const SPIKE_COLOR: Color = Color::Rgb(255, 120, 60);

/// One run of text in a single colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub color: Color,
    pub is_bold: bool,
}

impl Segment {
    fn new(text: impl Into<String>, color: Color, is_bold: bool) -> Self {
        Self {
            text: text.into(),
            color,
            is_bold,
        }
    }

    fn width(&self) -> u16 {
        UnicodeWidthStr::width(self.text.as_str()) as u16
    }
}

/// Total cells `segments` occupy, including one space between segments.
pub fn segments_width(segments: &[Segment]) -> u16 {
    let text: u16 = segments.iter().map(Segment::width).sum();
    text + segments.len().saturating_sub(1) as u16
}

/// What a tree row shows for `row`. `is_group` marks project/group rows,
/// which only ever show their rolled-up total, and only when the group-total
/// option is on. Returns nothing when no enabled option is currently visible.
pub fn segments_for_row(
    row: &RowCost,
    settings: &CostSettings,
    is_hovered: bool,
    is_group: bool,
) -> Vec<Segment> {
    let shown = |display: CostDisplay| settings.option(display).is_shown(is_hovered);
    let level_color_of = |row: &RowCost| row.level.map_or(PENDING_COLOR, level_color);
    let amount = |row: &RowCost| {
        if row.is_loading {
            "…".to_owned()
        } else {
            format_amount(settings.metric, row.amount, row.is_lower_bound)
        }
    };
    let glyph = |row: &RowCost| row.level.map_or('·', CostLevel::glyph).to_string();

    let mut segments = Vec::new();
    if row.is_unavailable {
        // The chosen metric has no figure for this agent; draw nothing rather
        // than an empty track that would read as "free".
        return segments;
    }
    if is_group {
        if shown(CostDisplay::GroupTotals) {
            segments.push(Segment::new(glyph(row), level_color_of(row), true));
            segments.push(Segment::new(amount(row), level_color_of(row), false));
        }
        return segments;
    }

    if shown(CostDisplay::Sparkline) && !row.spark.is_empty() {
        segments.push(Segment::new(row.spark.clone(), level_color_of(row), false));
    }
    if shown(CostDisplay::Meter) {
        segments.push(Segment::new(
            meter_string(row.level),
            level_color_of(row),
            false,
        ));
    }
    // The glyph-and-dollars option already contains the glyph, so enabling
    // both would show it twice.
    if shown(CostDisplay::LevelDollars) {
        segments.push(Segment::new(glyph(row), level_color_of(row), true));
        segments.push(Segment::new(amount(row), level_color_of(row), false));
    } else if shown(CostDisplay::LevelGlyph) {
        segments.push(Segment::new(glyph(row), level_color_of(row), true));
    }
    let has_indicator = !segments.is_empty();
    if has_indicator && shown(CostDisplay::BurnMarker) && row.is_spike {
        segments.push(Segment::new("↑", SPIKE_COLOR, true));
    }
    if has_indicator && row.is_over_budget {
        segments.push(Segment::new(
            "!",
            level_color(CostLevel::new(CostLevel::MAX.into())),
            true,
        ));
    }
    segments
}

/// Text appended to the tree panel's title when the total-line option is
/// visible: ` · Σ $52.3`. `is_hovered` is whether the pointer is over the
/// tree panel.
pub fn title_suffix(overlay: &CostOverlay, is_hovered: bool) -> Option<String> {
    if !overlay.settings.header_total.is_shown(is_hovered) || overlay.agent_count == 0 {
        return None;
    }
    Some(format!(
        " · Σ {}",
        format_amount(
            overlay.metric,
            overlay.total_amount,
            overlay.total_is_lower_bound
        )
    ))
}

/// Words describing what the level was measured against.
fn rating_basis(overlay: &CostOverlay, cost: &PaneCost) -> String {
    let settings = &overlay.settings;
    let amount = cost.amount(overlay.metric);
    match (settings.calibration, overlay.calibrated.note) {
        (Calibration::FixedBands, _) => "fixed bands".to_owned(),
        (Calibration::PeerRelative, CalibrationNote::NoPeers) => {
            "fixed bands (no peers yet)".to_owned()
        }
        (Calibration::PeerRelative, _) => "the open agents".to_owned(),
        (Calibration::OwnHistory, CalibrationNote::HistoryUnavailable) => {
            "fixed bands (history not ready)".to_owned()
        }
        (Calibration::OwnHistory, _) => format!(
            "your history: p{:.0} of {} sessions",
            percent_rank(&overlay.history_sorted, amount),
            overlay.history_sessions
        ),
        (Calibration::Budget, _) => format!(
            "budget: {:.0}% of {}",
            budget_fill(amount, settings.active_budget()) * 100.0,
            format_amount(overlay.metric, settings.active_budget(), false)
        ),
        (Calibration::BurnRate, _) => "burn rate".to_owned(),
    }
}

/// Content of the per-agent cost card, one `Line` per row.
pub fn detail_card_lines(
    cost: &PaneCost,
    row: &RowCost,
    overlay: &CostOverlay,
) -> Vec<Line<'static>> {
    let label = |text: &str| {
        Span::styled(
            format!("{text:<7}"),
            Style::new().add_modifier(Modifier::DIM),
        )
    };
    let level_style = Style::new()
        .fg(row.level.map_or(PENDING_COLOR, level_color))
        .add_modifier(Modifier::BOLD);
    let mut lines = Vec::new();

    let metric = overlay.metric;
    let dim = Style::new().add_modifier(Modifier::DIM);
    if metric == CostMetric::Quota && !cost.is_available(metric) {
        // Claude Code transcripts never record plan quota.
        lines.push(Line::from(vec![
            label("Quota"),
            Span::styled("not recorded by this agent's CLI", dim),
        ]));
        lines.push(Line::from(vec![
            label("Cost"),
            Span::styled(
                format!(
                    "{} (API-equivalent estimate)",
                    format_usd(cost.usd, cost.is_lower_bound)
                ),
                dim,
            ),
        ]));
        return lines;
    }
    let mut cost_line = vec![
        label(if metric == CostMetric::Quota {
            "Quota"
        } else {
            "Cost"
        }),
        Span::styled(
            format_amount(metric, cost.amount(metric), cost.is_lower_bound),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "{} {}",
                row.level.map_or('·', CostLevel::glyph),
                meter_string(row.level)
            ),
            level_style,
        ),
    ];
    if row.is_over_budget {
        cost_line.push(Span::styled(" over budget", level_style));
    }
    lines.push(Line::from(cost_line));
    lines.push(Line::from(vec![
        label(""),
        Span::styled(
            format!("vs {}", rating_basis(overlay, cost)),
            Style::new().add_modifier(Modifier::DIM),
        ),
    ]));

    if metric == CostMetric::Quota {
        lines.push(Line::from(vec![
            label("Cost"),
            Span::styled(
                format!(
                    "{} (API-equivalent estimate)",
                    format_usd(cost.usd, cost.is_lower_bound)
                ),
                dim,
            ),
        ]));
    }
    let mut burn = vec![
        label("Burn"),
        Span::raw(format_rate(metric, cost.burn(metric))),
        Span::styled(" last 15 min", Style::new().add_modifier(Modifier::DIM)),
    ];
    if cost.is_spike {
        burn.push(Span::styled(
            " ↑ spiking",
            Style::new().fg(SPIKE_COLOR).add_modifier(Modifier::BOLD),
        ));
    }
    lines.push(Line::from(burn));

    let tokens = &cost.tokens;
    lines.push(Line::from(vec![
        label("Tokens"),
        Span::raw(format!(
            "fresh {} · cache-read {}",
            format_tokens(tokens.input),
            format_tokens(tokens.cache_read)
        )),
    ]));
    lines.push(Line::from(vec![
        label(""),
        Span::raw(format!(
            "cache-write {} · output {}",
            format_tokens(tokens.cache_write),
            format_tokens(tokens.output)
        )),
    ]));

    let mut models: Vec<_> = cost.model_costs.iter().collect();
    models.sort_by(|a, b| b.usd.unwrap_or(0.0).total_cmp(&a.usd.unwrap_or(0.0)));
    for (index, model) in models.iter().take(3).enumerate() {
        lines.push(Line::from(vec![
            label(if index == 0 { "Models" } else { "" }),
            Span::raw(format!(
                "{} {}",
                model.model,
                model
                    .usd
                    .map_or("no price".to_owned(), |usd| format_usd(usd, false))
            )),
        ]));
    }
    if let Some(reported) = cost.reported_usd {
        lines.push(Line::from(vec![
            label("CLI"),
            Span::raw(format!("reported {}", format_usd(reported, false))),
        ]));
    }
    for (name, percent) in &cost.quota {
        lines.push(Line::from(vec![
            label(if metric == CostMetric::Quota {
                ""
            } else {
                "Quota"
            }),
            Span::raw(format!(
                "{name} window {} used account-wide",
                format_quota(*percent)
            )),
        ]));
    }
    if !cost.spark_cells.is_empty() {
        lines.push(Line::from(vec![
            label("Trend"),
            Span::styled(sparkline_glyphs(&cost.spark_cells), level_style),
            Span::styled(
                format!(
                    "  last {}",
                    format_window(overlay.settings.sparkline_window_minutes)
                ),
                Style::new().add_modifier(Modifier::DIM),
            ),
        ]));
    }
    lines
}

/// Draws the per-agent cost card beside the tree row at `anchor_y`, inside
/// `bounds`, over whatever is underneath.
pub fn draw_detail_card(
    frame: &mut Frame,
    bounds: Rect,
    anchor_x: u16,
    anchor_y: u16,
    title: &str,
    lines: &[Line<'static>],
) {
    const CARD_WIDTH: u16 = 46;
    let width = CARD_WIDTH.min(bounds.width);
    let height = (lines.len() as u16 + 2).min(bounds.height);
    if width < 12 || height < 3 {
        return;
    }
    let x = anchor_x
        .min(bounds.right().saturating_sub(width))
        .max(bounds.x);
    let y = anchor_y
        .saturating_sub(1)
        .min(bounds.bottom().saturating_sub(height))
        .max(bounds.y);
    let area = Rect::new(x, y, width, height);
    frame.render_widget(Clear, area);
    let block = theme::block(true).title(theme::chrome_title(title));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    for (offset, line) in lines.iter().take(usize::from(inner.height)).enumerate() {
        frame.render_widget(
            line,
            Rect::new(inner.x, inner.y + offset as u16, inner.width, 1),
        );
    }
}

/// Paints `segments` right-aligned so the strip ends just left of
/// `right_edge` (exclusive), never reaching `left_limit`. Leading segments
/// are dropped until the rest fits. Existing cell backgrounds are kept, so a
/// selected or hovered row keeps its highlight under the indicators.
/// Returns the first column the strip occupies, or `None` when nothing fit.
pub fn draw_segments(
    buffer: &mut Buffer,
    row: u16,
    left_limit: u16,
    right_edge: u16,
    segments: &[Segment],
) -> Option<u16> {
    let available = right_edge.saturating_sub(left_limit);
    let first =
        (0..segments.len()).find(|start| segments_width(&segments[*start..]) <= available)?;
    let visible = &segments[first..];
    let width = segments_width(visible);
    let start_x = right_edge - width;

    // Take the background from the cell the strip ends on, then reset every
    // cell it will cover so old title glyphs cannot survive underneath.
    let background = buffer[(right_edge - 1, row)].bg;
    if start_x > 0 && UnicodeWidthStr::width(buffer[(start_x - 1, row)].symbol()) > 1 {
        // A wide glyph straddling the boundary would lose its second cell.
        buffer[(start_x - 1, row)].reset();
        buffer[(start_x - 1, row)]
            .set_symbol(" ")
            .set_bg(background);
    }
    for column in start_x..right_edge {
        let cell = &mut buffer[(column, row)];
        cell.reset();
        cell.set_symbol(" ").set_bg(background);
    }
    let mut x = start_x;
    for (index, segment) in visible.iter().enumerate() {
        if index > 0 {
            x += 1;
        }
        let mut style = Style::new().fg(segment.color).bg(background);
        if segment.is_bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        // One character per cell keeps every glyph independently diffable.
        for character in segment.text.chars() {
            let width = UnicodeWidthStr::width(character.to_string().as_str()) as u16;
            if width == 0 {
                continue;
            }
            buffer[(x, row)]
                .set_symbol(&character.to_string())
                .set_style(style);
            x += width;
        }
    }
    Some(start_x)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::*;
    use crate::cost_settings::{CostRow, CostVisibility};

    fn row(level: usize, usd: f64) -> RowCost {
        RowCost {
            level: Some(CostLevel::new(level)),
            amount: usd,
            is_lower_bound: false,
            burn_per_hour: 0.0,
            spark: "▁▃█".to_owned(),
            is_spike: false,
            is_over_budget: false,
            is_loading: false,
            is_unavailable: false,
        }
    }

    fn text(segments: &[Segment]) -> String {
        segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn default_settings_show_the_meter_only_while_hovered() {
        let settings = CostSettings::default();
        assert!(segments_for_row(&row(2, 6.0), &settings, false, false).is_empty());
        let hovered = segments_for_row(&row(2, 6.0), &settings, true, false);
        assert_eq!(text(&hovered), "▰▰▰▱▱");
    }

    #[test]
    fn options_compose_in_a_fixed_order_and_dollars_supersede_the_bare_glyph() {
        let mut settings = CostSettings::default();
        for display in [
            CostDisplay::Sparkline,
            CostDisplay::LevelGlyph,
            CostDisplay::LevelDollars,
            CostDisplay::BurnMarker,
        ] {
            settings.adjust(CostRow::Display(display), 0);
        }
        let mut spiking = row(3, 12.0);
        spiking.is_spike = true;
        let segments = segments_for_row(&spiking, &settings, true, false);
        assert_eq!(text(&segments), "▁▃█ ▰▰▰▰▱ ▆ $12.0 ↑");

        settings.level_dollars.enabled = false;
        let segments = segments_for_row(&spiking, &settings, true, false);
        assert_eq!(text(&segments), "▁▃█ ▰▰▰▰▱ ▆ ↑");
    }

    #[test]
    fn visibility_is_decided_per_option() {
        let mut settings = CostSettings::default();
        settings.adjust(CostRow::Display(CostDisplay::LevelDollars), 0);
        settings.adjust(CostRow::Visibility(CostDisplay::LevelDollars), 0);
        assert_eq!(settings.level_dollars.visibility, CostVisibility::Always);
        let idle = segments_for_row(&row(1, 2.0), &settings, false, false);
        assert_eq!(
            text(&idle),
            "▂ $2.00",
            "always-visible option without hover"
        );
        let hovered = segments_for_row(&row(1, 2.0), &settings, true, false);
        assert_eq!(text(&hovered), "▰▰▱▱▱ ▂ $2.00");
    }

    #[test]
    fn loading_rows_show_an_empty_track_and_an_ellipsis() {
        let mut settings = CostSettings::default();
        settings.adjust(CostRow::Display(CostDisplay::LevelDollars), 0);
        let loading = RowCost {
            level: None,
            amount: 0.0,
            is_lower_bound: false,
            burn_per_hour: 0.0,
            spark: String::new(),
            is_spike: false,
            is_over_budget: false,
            is_loading: true,
            is_unavailable: false,
        };
        assert_eq!(
            text(&segments_for_row(&loading, &settings, true, false)),
            "▱▱▱▱▱ · …"
        );
    }

    #[test]
    fn group_rows_show_only_their_total_and_only_when_enabled() {
        let mut settings = CostSettings::default();
        assert!(segments_for_row(&row(2, 6.0), &settings, true, true).is_empty());
        settings.adjust(CostRow::Display(CostDisplay::GroupTotals), 0);
        let segments = segments_for_row(&row(2, 6.0), &settings, true, true);
        assert_eq!(text(&segments), "▄ $6.00");
    }

    #[test]
    fn lower_bounds_and_budget_overruns_are_marked() {
        let mut settings = CostSettings::default();
        settings.adjust(CostRow::Display(CostDisplay::LevelDollars), 0);
        let mut over = row(4, 25.0);
        over.is_lower_bound = true;
        over.is_over_budget = true;
        assert_eq!(
            text(&segments_for_row(&over, &settings, true, false)),
            "▰▰▰▰▰ █ ~$25.0 !"
        );
    }

    fn buffer_row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol().to_owned())
            .collect()
    }

    #[test]
    fn drawing_right_aligns_before_the_edge_and_overwrites_title_text() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 30, 1));
        buffer.set_string(0, 0, "a-long-agent-title-that-runs-on", Style::new());
        let segments = vec![Segment::new("▰▰▱▱▱", Color::Green, false)];
        let start = draw_segments(&mut buffer, 0, 4, 26, &segments);
        assert_eq!(start, Some(21));
        let rendered = buffer_row(&buffer, 0);
        assert_eq!(&rendered[..21], "a-long-agent-title-th");
        assert!(rendered.contains("▰▰▱▱▱"));
        assert_eq!(buffer[(21, 0)].fg, Color::Green);
        assert_eq!(
            buffer[(26, 0)].symbol(),
            "n",
            "cells right of the edge are untouched"
        );
    }

    #[test]
    fn drawing_keeps_the_row_background_and_drops_leading_segments_to_fit() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 1));
        buffer.set_style(Rect::new(0, 0, 12, 1), Style::new().bg(Color::Magenta));
        let segments = vec![
            Segment::new("▁▃█▅▂", Color::Red, false),
            Segment::new("▰▰▱▱▱", Color::Red, false),
            Segment::new("$9.99", Color::Red, false),
        ];
        // Room for 12 - 2 = 10 cells: sparkline + meter + dollars need 17 and
        // meter + dollars need 11, so only the dollars remain.
        let start = draw_segments(&mut buffer, 0, 2, 12, &segments);
        assert_eq!(start, Some(7));
        let rendered = buffer_row(&buffer, 0);
        assert!(
            !rendered.contains('▁') && !rendered.contains('▰'),
            "{rendered}"
        );
        assert!(rendered.contains("$9.99"));
        assert_eq!(buffer[(11, 0)].bg, Color::Magenta);
        assert_eq!(buffer[(start.unwrap(), 0)].bg, Color::Magenta);
    }

    #[test]
    fn title_suffix_follows_its_visibility_and_needs_agents() {
        let mut overlay = CostOverlay {
            agent_count: 2,
            total_amount: 52.34,
            ..CostOverlay::default()
        };
        assert_eq!(
            title_suffix(&overlay, true),
            None,
            "header total is off by default"
        );
        overlay.settings.header_total.enabled = true;
        assert_eq!(title_suffix(&overlay, false), None, "hover-only by default");
        assert_eq!(title_suffix(&overlay, true).as_deref(), Some(" · Σ $52.3"));
        overlay.settings.header_total.visibility = CostVisibility::Always;
        assert_eq!(title_suffix(&overlay, false).as_deref(), Some(" · Σ $52.3"));
        overlay.total_is_lower_bound = true;
        assert_eq!(
            title_suffix(&overlay, false).as_deref(),
            Some(" · Σ ~$52.3")
        );
        overlay.agent_count = 0;
        assert_eq!(title_suffix(&overlay, false), None);
    }

    #[test]
    fn nothing_is_drawn_when_not_even_one_segment_fits() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
        let segments = vec![Segment::new("▰▰▱▱▱", Color::Red, false)];
        assert_eq!(draw_segments(&mut buffer, 0, 3, 6, &segments), None);
        assert_eq!(buffer_row(&buffer, 0).trim(), "");
    }

    #[test]
    fn quota_rows_draw_percent_and_unavailable_rows_draw_nothing() {
        let mut settings = CostSettings {
            metric: CostMetric::Quota,
            ..CostSettings::default()
        };
        settings.adjust(CostRow::Display(CostDisplay::LevelDollars), 0);
        settings.adjust(CostRow::Visibility(CostDisplay::LevelDollars), 0);
        let texts: Vec<String> = segments_for_row(&row(2, 6.2), &settings, false, false)
            .into_iter()
            .map(|segment| segment.text)
            .collect();
        assert!(texts.contains(&"6.2%".to_owned()), "{texts:?}");
        assert!(!texts.iter().any(|text| text.contains('$')));

        let unavailable = RowCost {
            level: None,
            is_unavailable: true,
            ..row(0, 0.0)
        };
        assert!(segments_for_row(&unavailable, &settings, true, false).is_empty());
    }

    #[test]
    fn detail_card_for_quota_shows_percent_and_explains_missing_data() {
        let settings = CostSettings {
            metric: CostMetric::Quota,
            calibration: Calibration::FixedBands,
            ..CostSettings::default()
        };
        let overlay = CostOverlay {
            metric: CostMetric::Quota,
            settings,
            ..CostOverlay::default()
        };
        let text = |lines: Vec<Line<'static>>| {
            lines
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut cost = PaneCost {
            usd: 4.0,
            is_lower_bound: false,
            burn_usd_per_hour: 0.0,
            is_spike: false,
            spark_cells: Vec::new(),
            quota_points: 7.5,
            quota_burn_per_hour: 3.0,
            has_quota: true,
            tokens: Default::default(),
            model_costs: Vec::new(),
            reported_usd: None,
            quota: vec![("primary".to_owned(), 41.0)],
            spend_points: Vec::new(),
            storage: Default::default(),
        };
        let page = text(detail_card_lines(&cost, &row(3, 7.5), &overlay));
        assert!(page.contains("Quota  7.5%"), "{page}");
        assert!(page.contains("3.0%/h"), "{page}");
        assert!(page.contains("$4.00 (API-equivalent estimate)"), "{page}");
        assert!(
            page.contains("primary window 41% used account-wide"),
            "{page}"
        );

        cost.has_quota = false;
        let page = text(detail_card_lines(&cost, &row(0, 0.0), &overlay));
        assert!(page.contains("not recorded by this agent's CLI"), "{page}");
    }
}
