//! Presentation state and full-screen rendering for the agent usage view.
use crate::weekly_usage::{allocate_bar_cells, ModelWorkload, UsageRange};
use crate::weekly_usage_heat::{heat_sort_key, shaded_rgb, strength_tier};
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ilium_core::{AgentClass, NodeId};
use ratatui::{
    layout::{Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs,
    },
    Frame,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageRowId {
    pub pane_id: NodeId,
    pub provider: crate::weekly_usage::UsageProvider,
    /// Stable hash of the custom agent profile id; `None` for built-in agents.
    pub profile: Option<u64>,
}

/// Custom agent profile that ran a row's sessions. Such agents use their own
/// backend, so they never share a provider's account totals or quota.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProfileLabel {
    pub id: String,
    pub name: String,
    pub icon: String,
}

fn profile_key(profile_id: &str) -> u64 {
    // FNV-1a: stable across runs and cheap; ids are short and few.
    profile_id
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}
impl UsageRowId {
    pub(crate) fn new(pane_id: NodeId, class: &AgentClass, profile: Option<&str>) -> Self {
        Self {
            pane_id,
            profile: profile.map(profile_key),
            provider: if *class == AgentClass::Claude {
                crate::weekly_usage::UsageProvider::Claude
            } else {
                crate::weekly_usage::UsageProvider::Codex
            },
        }
    }
    pub(crate) fn matches_class(self, class: &AgentClass) -> bool {
        matches!(
            (self.provider, class),
            (
                crate::weekly_usage::UsageProvider::Claude,
                AgentClass::Claude
            ) | (crate::weekly_usage::UsageProvider::Codex, AgentClass::Codex)
        )
    }
}

pub(crate) struct UsageRow {
    pub pane_id: NodeId,
    pub title: String,
    pub class: AgentClass,
    pub profile: Option<ProfileLabel>,
    pub models: Vec<ModelWorkload>,
    pub status: String,
    pub binding_since_ms: Option<i64>,
    pub source_time_ms: Option<i64>,
    pub codex_quota: Option<crate::weekly_usage_quota::CodexQuota>,
    pub historical_provider: bool,
    /// Set for a session found on disk with no pane in Ilium.
    pub session_id: Option<String>,
}
impl UsageRow {
    pub(crate) fn id(&self) -> UsageRowId {
        UsageRowId::new(
            self.pane_id,
            &self.class,
            self.profile.as_ref().map(|profile| profile.id.as_str()),
        )
    }
}

#[derive(Default)]
pub struct WeeklyUsageState {
    pub(crate) rows: Vec<UsageRow>,
    pub(crate) acquisition: crate::weekly_usage_store::UsageAcquisition,
    pub(crate) history_capacity_limited: bool,
    pub selected: Option<UsageRowId>,
    /// Rows whose breakdown is open. Selection only moves; clicking or Space
    /// toggles, so an open row can be closed again.
    pub expanded: Vec<UsageRowId>,
    pub range: UsageRange,
    /// Range the current row totals were derived for.
    pub(crate) derived_range: Option<UsageRange>,
    pub scroll: std::cell::Cell<usize>,
    pub reveal_selection: std::cell::Cell<bool>,
    pub hit_rows: std::cell::RefCell<Vec<(u16, UsageRowId)>>,
    /// Absolute rectangles of the controls drawn by the last render.
    pub range_hits: std::cell::RefCell<Vec<(Rect, UsageRange)>>,
    pub close_hit: std::cell::Cell<Rect>,
    pub last_cycle: Option<std::time::Instant>,
    pub(crate) active: Option<crate::weekly_usage_store::ActiveDerivation>,
    pub(crate) quota_active:
        Option<ilium_execution::Receipt<crate::weekly_usage_store::ReadClaudeQuota>>,
    pub quota_started: bool,
    pub quota_started_at: Option<std::time::Instant>,
    pub(crate) claude_quota: Option<crate::weekly_usage_quota::QuotaDisplay>,
    pub quota_status: Option<String>,
    pub quota_retention: Option<ilium_execution::Retention>,
    /// Sessions found on disk that no pane owns, for the derived range.
    pub(crate) discovered: Vec<crate::weekly_usage_discovery::DiscoveredSession>,
    pub(crate) discovery: crate::weekly_usage_store::DiscoveryState,
}

impl Drop for WeeklyUsageState {
    fn drop(&mut self) {
        self.acquisition.cancel();
        if let Some(active) = &self.active {
            active.receipt.cancel();
        }
        if let Some(active) = &self.quota_active {
            active.cancel();
        }
        if let Some(active) = &self.discovery.active {
            active.cancel();
        }
    }
}

impl WeeklyUsageState {
    pub fn key(&mut self, key: KeyCode) {
        let index = self
            .selected
            .and_then(|id| self.rows.iter().position(|row| row.id() == id))
            .unwrap_or(0);
        match key {
            KeyCode::Up => {
                self.selected = self.rows.get(index.saturating_sub(1)).map(UsageRow::id);
                self.reveal_selection.set(true);
            }
            KeyCode::Down => {
                self.selected = self
                    .rows
                    .get(
                        index
                            .saturating_add(1)
                            .min(self.rows.len().saturating_sub(1)),
                    )
                    .map(UsageRow::id);
                self.reveal_selection.set(true);
            }
            KeyCode::Left => self.range = self.range.previous(),
            KeyCode::Right => self.range = self.range.next(),
            KeyCode::Char(' ') => {
                if let Some(id) = self
                    .selected
                    .or_else(|| self.rows.first().map(UsageRow::id))
                {
                    self.selected = Some(id);
                    self.toggle(id);
                }
            }
            KeyCode::PageUp => self.scroll.set(self.scroll.get().saturating_sub(8)),
            KeyCode::PageDown => self.scroll.set(self.scroll.get().saturating_add(8)),
            _ => {}
        }
    }
    fn toggle(&mut self, id: UsageRowId) {
        if let Some(position) = self.expanded.iter().position(|open| *open == id) {
            self.expanded.remove(position);
        } else {
            self.expanded.push(id);
        }
    }
    pub fn mouse(&mut self, mouse: MouseEvent) -> bool {
        let point = ratatui::layout::Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll.set(self.scroll.get().saturating_sub(3)),
            MouseEventKind::ScrollDown => self.scroll.set(self.scroll.get().saturating_add(3)),
            MouseEventKind::Down(MouseButton::Left) => {
                if self.close_hit.get().contains(point) {
                    return true;
                }
                let tab = self
                    .range_hits
                    .borrow()
                    .iter()
                    .find(|(rect, _)| rect.contains(point))
                    .map(|(_, range)| *range);
                if let Some(range) = tab {
                    self.range = range;
                } else {
                    let row = self
                        .hit_rows
                        .borrow()
                        .iter()
                        .find(|(y, _)| *y == mouse.row)
                        .map(|(_, id)| *id);
                    if let Some(id) = row {
                        self.selected = Some(id);
                        self.toggle(id);
                    }
                }
            }
            _ => {}
        }
        false
    }
}

fn push_wrapped(lines: &mut Vec<Line<'static>>, text: &str, style: Style, width: u16) {
    let width = usize::from(width).max(1);
    // Break preferably between " · " facts, then between words, and only inside
    // a single word that is wider than the whole line.
    let mut current = String::new();
    let mut used = 0usize;
    let mut flush = |current: &mut String, used: &mut usize, lines: &mut Vec<Line<'static>>| {
        if !current.is_empty() {
            lines.push(Line::from(Span::styled(std::mem::take(current), style)));
        }
        *used = 0;
    };
    for (fact_index, fact) in text.split(" · ").enumerate() {
        let separator = if fact_index == 0 { "" } else { " · " };
        let fact_cells = fact.width() + separator.width();
        if used > 0 && used + fact_cells > width {
            flush(&mut current, &mut used, lines);
        }
        if used == 0 && fact.width() <= width {
            // A wrapped fact starts clean: the separator belongs to the line it ended.
            current.push_str(fact);
            used = fact.width();
            continue;
        }
        if used > 0 {
            current.push_str(separator);
            used += separator.width();
            current.push_str(fact);
            used += fact.width();
            continue;
        }
        // The fact alone is wider than the line: wrap it by words.
        for word in fact.split(' ') {
            let cells = word.width();
            let gap = usize::from(used > 0);
            if used > 0 && used + gap + cells > width {
                flush(&mut current, &mut used, lines);
            }
            if used > 0 {
                current.push(' ');
                used += 1;
            }
            if cells <= width {
                current.push_str(word);
                used += cells;
                continue;
            }
            for grapheme in word.graphemes(true) {
                let grapheme_cells = grapheme.width();
                if used > 0 && used + grapheme_cells > width {
                    flush(&mut current, &mut used, lines);
                }
                current.push_str(grapheme);
                used += grapheme_cells;
            }
        }
    }
    flush(&mut current, &mut used, lines);
}

/// One short line for the row; the full text lives in the selected details.
fn short_status(status: &str) -> String {
    let head = status.split(" · ").next().unwrap_or(status);
    let mut short = match head {
        "partial" => "loaded".to_owned(),
        "unavailable" => "unavailable".to_owned(),
        "duplicate session ownership" => "unassigned (ambiguous owner)".to_owned(),
        other => other.chars().take(40).collect(),
    };
    if status.contains("unknown model weight") {
        short.push_str(" · some weights unsupported");
    }
    if status.contains("refreshing") {
        short.push_str(" · refreshing");
    }
    if status.contains("stale") || status.contains("model limit") {
        short.push_str(" · partial");
    }
    short
}

/// One colour per model, assigned by strength tier. Models of the same tier
/// get lighter shades of the tier colour so heat stays readable.
fn model_palette<'a>(
    models: impl Iterator<Item = &'a str>,
) -> std::collections::BTreeMap<&'a str, Color> {
    let names: std::collections::BTreeSet<_> = models.collect();
    let mut by_tier: std::collections::BTreeMap<Option<_>, Vec<&str>> =
        std::collections::BTreeMap::new();
    for name in names {
        by_tier.entry(strength_tier(name)).or_default().push(name);
    }
    let mut palette = std::collections::BTreeMap::new();
    for (tier, members) in by_tier {
        let count = members.len();
        for (index, name) in members.into_iter().enumerate() {
            let (red, green, blue) = shaded_rgb(tier, index, count);
            palette.insert(name, Color::Rgb(red, green, blue));
        }
    }
    palette
}

/// Models weakest first, historical estimates after verified usage.
fn heat_ordered(models: &[ModelWorkload]) -> Vec<&ModelWorkload> {
    let mut ordered: Vec<_> = models.iter().collect();
    ordered.sort_by(|left, right| {
        heat_sort_key(&left.model)
            .cmp(&heat_sort_key(&right.model))
            .then(left.historical.cmp(&right.historical))
    });
    ordered
}

/// Truncate to `max` terminal cells, marking the cut with an ellipsis.
fn fit(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let cells = grapheme.width();
        if used + cells + 1 > max {
            break;
        }
        out.push_str(grapheme);
        used += cells;
    }
    out.push('…');
    out
}

/// Estimated API list-price cost in dollars. Never rounds a real cost to zero.
fn format_cost(value: f64) -> String {
    if value <= 0.0 {
        "$0.00".to_owned()
    } else if value < 0.01 {
        "<$0.01".to_owned()
    } else if value < 10.0 {
        format!("${value:.3}")
    } else {
        format!("${value:.2}")
    }
}

fn timestamp(milliseconds: Option<i64>) -> String {
    milliseconds
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|time| time.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn total_cost(models: &[ModelWorkload]) -> f64 {
    models
        .iter()
        .filter_map(|model| model.relative_units)
        .fold(0.0_f64, |total, value| total + value)
}

/// What the row's right-hand cell says about its cost. Unknown is never 0.
fn amount_text(row: &UsageRow) -> String {
    if !row.status.starts_with("partial") {
        return String::new();
    }
    let value = total_cost(&row.models);
    if row
        .models
        .iter()
        .any(|model| model.relative_units.is_none())
    {
        if value > 0.0 {
            format!("{} + unpriced tokens", format_cost(value))
        } else {
            "weight unsupported".to_owned()
        }
    } else {
        format_cost(value)
    }
}

fn account_text(state: &WeeklyUsageState, class: &AgentClass, custom: bool) -> String {
    if custom {
        return "own backend · no shared account total or quota".into();
    }
    if *class == AgentClass::Codex {
        return "account total unavailable · reset unknown".into();
    }
    match &state.claude_quota {
        Some(quota) => {
            let reset = chrono::DateTime::from_timestamp(quota.reset_at, 0)
                .map(|time| time.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "unknown".into());
            format!(
                "local account {:.1}% · reset {} · {} · recorded {} · pane binding unknown",
                quota.used_percent,
                reset,
                quota.freshness,
                timestamp(Some(quota.recorded_at_ms))
            )
        }
        None => state
            .quota_status
            .clone()
            .unwrap_or_else(|| "account reading loading".into()),
    }
}

/// Sections in display order: Claude, Codex, then one per custom agent profile
/// that has rows (by name). Built-in sections always show, even when empty.
fn sections(state: &WeeklyUsageState) -> Vec<(AgentClass, Option<String>, String)> {
    let mut sections = vec![
        (AgentClass::Claude, None, "CLAUDE".to_owned()),
        (AgentClass::Codex, None, "CODEX".to_owned()),
    ];
    let mut custom: Vec<(AgentClass, Option<String>, String)> = Vec::new();
    for row in &state.rows {
        let Some(label) = &row.profile else { continue };
        let section = (row.class.clone(), Some(label.id.clone()));
        if !custom
            .iter()
            .any(|(class, id, _)| (class.clone(), id.clone()) == section)
        {
            let provider = if row.class == AgentClass::Claude {
                "Claude"
            } else {
                "Codex"
            };
            let heading = if label.id == crate::weekly_usage_discovery::PROXY_PROFILE_ID {
                format!(
                    "{} {} · own backend · started outside Ilium",
                    label.icon, label.name
                )
            } else {
                format!(
                    "{} {} · custom, based on {provider}",
                    label.icon, label.name
                )
            };
            custom.push((section.0, section.1, heading));
        }
    }
    custom.sort_by(|left, right| left.2.cmp(&right.2).then(left.1.cmp(&right.1)));
    sections.extend(custom);
    sections
}

const FRACTIONS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

pub(crate) fn render(frame: &mut Frame, area: Rect, state: &WeeklyUsageState) {
    frame.render_widget(Clear, area);
    state.hit_rows.borrow_mut().clear();
    state.range_hits.borrow_mut().clear();
    state.close_hit.set(Rect::default());
    if area.height == 0 || area.width == 0 {
        return;
    }
    let accent = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(Color::Rgb(0x8a, 0x8a, 0x8a));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(crate::theme::border_style(true))
        .title(Span::styled(" Agent usage ", accent));
    let frame_area = area;
    let inner = block.inner(frame_area);
    frame.render_widget(block, frame_area);
    if inner.height < 6 || inner.width < 24 {
        frame.render_widget(Paragraph::new("Enlarge the window").style(muted), inner);
        return;
    }
    // Close control on the top border, right-aligned.
    let close_label = " ✕ Esc ";
    let close_width = close_label.width() as u16;
    if frame_area.width > close_width + 2 {
        let close = Rect::new(
            frame_area.right() - 1 - close_width,
            frame_area.y,
            close_width,
            1,
        );
        frame.render_widget(Paragraph::new(close_label).style(accent), close);
        state.close_hit.set(close);
    }
    let content = inner.inner(Margin::new(1, 0));
    // One cell for the scrollbar and one empty gutter cell before it.
    let width = usize::from(content.width.saturating_sub(2)).max(1);

    // Range selector: a real Tabs widget, with geometry mirrored for clicks.
    let labels: Vec<&str> = UsageRange::ALL.iter().map(|range| range.label()).collect();
    let prefix = "Range  ";
    let tabs_x = content.x + prefix.width() as u16;
    frame.render_widget(
        Paragraph::new(prefix).style(muted),
        Rect::new(content.x, content.y, prefix.width() as u16, 1),
    );
    let selected_index = UsageRange::ALL
        .iter()
        .position(|range| *range == state.range)
        .unwrap_or(0);
    frame.render_widget(
        Tabs::new(labels.clone())
            .select(selected_index)
            .padding(" ", " ")
            .divider(" ")
            .style(muted)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        Rect::new(
            tabs_x,
            content.y,
            content.width.saturating_sub(tabs_x - content.x),
            1,
        ),
    );
    let mut x = tabs_x;
    for (label, range) in labels.iter().zip(UsageRange::ALL) {
        let tab_width = label.width() as u16 + 2;
        if x + tab_width <= content.right() {
            state
                .range_hits
                .borrow_mut()
                .push((Rect::new(x, content.y, tab_width, 1), range));
        }
        x += tab_width + 1;
    }

    let scale = state
        .rows
        .iter()
        .map(|row| total_cost(&row.models))
        .fold(0.0_f64, f64::max);
    let summary = format!(
        "Estimated API-price cost of the last {} · bar length = tokens × list price",
        state.range.label()
    );
    frame.render_widget(
        Paragraph::new(fit(&summary, width)).style(muted),
        Rect::new(content.x, content.y + 1, content.width, 1),
    );
    let loaded = state
        .rows
        .iter()
        .filter(|row| row.status.starts_with("partial"))
        .count();
    let scale_line = format!(
        "Common scale 0–{} · {loaded} of {} rows loaded · not quota or billing",
        format_cost(scale),
        state.rows.len()
    );
    frame.render_widget(
        Paragraph::new(fit(&scale_line, width)).style(muted),
        Rect::new(content.x, content.y + 2, content.width, 1),
    );

    let footer_y = inner.bottom() - 1;
    frame.render_widget(
        Paragraph::new(fit(
            "←/→ range · ↑/↓ select · Space or click open/close · Enter open pane stats · Esc close",
            width,
        ))
        .style(muted),
        Rect::new(content.x, footer_y, content.width, 1),
    );
    let body = Rect::new(
        content.x,
        content.y + 4,
        content.width,
        footer_y.saturating_sub(content.y + 4),
    );
    if body.height == 0 {
        return;
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut row_lines: Vec<(usize, UsageRowId)> = Vec::new();
    if state.history_capacity_limited {
        push_wrapped(
            &mut lines,
            "History capacity reached · earlier ownership intervals excluded",
            Style::default().fg(Color::Yellow),
            width as u16,
        );
    }
    for (class, profile, title) in sections(state) {
        let rows: Vec<_> = state
            .rows
            .iter()
            .filter(|row| {
                row.class == class
                    && row.profile.as_ref().map(|label| label.id.as_str()) == profile.as_deref()
            })
            .collect();
        let palette = model_palette(
            rows.iter()
                .flat_map(|row| row.models.iter().map(|model| model.model.as_str())),
        );
        lines.push(Line::from(Span::styled(title, accent)));
        push_wrapped(
            &mut lines,
            &account_text(state, &class, profile.is_some()),
            muted,
            width as u16,
        );
        let unassigned = state
            .acquisition
            .unassigned_sessions(&class, profile.as_deref());
        if unassigned > 0 {
            push_wrapped(
                &mut lines,
                &format!("Unassigned: {unassigned} session(s) claimed by several panes · counted once, not attributed to any agent"),
                Style::default().fg(Color::Yellow),
                width as u16,
            );
        }
        if rows.is_empty() {
            lines.push(Line::from(Span::styled("No supported agents", muted)));
        }
        let mut legend: Vec<&str> = Vec::new();
        for row in &rows {
            let id = row.id();
            let selected = state.selected == Some(id);
            let open = state.expanded.contains(&id);
            let duplicate = state
                .rows
                .iter()
                .filter(|other| {
                    other.class == row.class
                        && other.profile == row.profile
                        && other.title == row.title
                })
                .count()
                > 1;
            let name = if duplicate {
                match &row.session_id {
                    Some(id) => format!(
                        "{} #{}",
                        row.title,
                        crate::weekly_usage_discovery::short_session_id(id)
                    ),
                    None => format!("{} #{}", row.title, row.pane_id.0),
                }
            } else {
                row.title.clone()
            };
            let marker = if open { "▾" } else { "▸" };
            let right = {
                let amount = amount_text(row);
                let status = short_status(&row.status);
                if amount.is_empty() {
                    status
                } else {
                    format!("{amount} · {status}")
                }
            };
            let right = fit(&right, width.saturating_sub(8));
            let left_budget = width.saturating_sub(right.width() + 4);
            let left = fit(&format!("{marker} {name}"), left_budget.max(3));
            let gap = width.saturating_sub(left.width() + right.width());
            let title_style = if selected { accent } else { Style::default() };
            let row_start = lines.len();
            lines.push(Line::from(vec![
                Span::styled(left, title_style),
                crate::value_control::leader_span(gap),
                Span::styled(right, if selected { accent } else { muted }),
            ]));
            let ordered = heat_ordered(&row.models);
            let values: Vec<f64> = ordered
                .iter()
                .map(|model| model.relative_units.unwrap_or(0.0))
                .collect();
            let counts = allocate_bar_cells(&values, scale, width);
            let mut spans = Vec::new();
            for (model, count) in ordered.iter().zip(counts) {
                if !legend.contains(&model.model.as_str()) {
                    legend.push(model.model.as_str());
                }
                spans.push(Span::styled(
                    if model.historical { "▒" } else { "█" }.repeat(count),
                    Style::default().fg(palette[model.model.as_str()]),
                ));
            }
            let total: f64 = values.iter().sum();
            if scale > 0.0 && total > 0.0 && total < scale {
                let exact = total / scale * width as f64;
                let eighths = ((exact.fract() * 8.0).floor() as usize).min(7);
                if eighths > 0 {
                    if let Some(model) = ordered
                        .iter()
                        .rev()
                        .find(|model| model.relative_units.is_some_and(|value| value > 0.0))
                    {
                        spans.push(Span::styled(
                            FRACTIONS[eighths],
                            Style::default().fg(palette[model.model.as_str()]),
                        ));
                    }
                }
            }
            lines.push(Line::from(spans));
            row_lines.extend((row_start..lines.len()).map(|line| (line, id)));
            if open {
                push_details(&mut lines, row, &ordered, &palette, width as u16, muted);
            }
            lines.push(Line::default());
        }
        if !legend.is_empty() {
            legend.sort_by_key(|model| heat_sort_key(model));
            let mut units: Vec<Vec<Span<'static>>> =
                vec![vec![Span::styled("Models, cool → hot  ", muted)]];
            for model in legend {
                let tier = strength_tier(model).map_or("unranked", |tier| tier.label());
                // Swatch and name wrap together, never apart.
                units.push(vec![
                    Span::styled("■ ", Style::default().fg(palette[model])),
                    Span::raw(format!("{model} ({tier})  ")),
                ]);
            }
            let mut legend_lines = Vec::new();
            wrap_units(&mut legend_lines, units, width);
            lines.extend(legend_lines);
        }
        lines.push(Line::default());
    }

    let height = usize::from(body.height);
    let maximum = lines.len().saturating_sub(height);
    let mut scroll = state.scroll.get().min(maximum);
    if state.reveal_selection.replace(false) {
        if let Some((line, _)) = row_lines.iter().find(|(_, id)| Some(*id) == state.selected) {
            if *line < scroll {
                scroll = *line;
            }
            if *line >= scroll + height {
                scroll = (*line + 1).saturating_sub(height);
            }
        }
    }
    state.scroll.set(scroll);
    *state.hit_rows.borrow_mut() = row_lines
        .into_iter()
        .filter(|(line, _)| *line >= scroll && *line < scroll + height)
        .map(|(line, id)| (body.y + (line - scroll) as u16, id))
        .collect();
    let text_area = Rect::new(body.x, body.y, body.width.saturating_sub(2), body.height);
    frame.render_widget(
        Paragraph::new(lines).scroll((scroll.min(usize::from(u16::MAX)) as u16, 0)),
        text_area,
    );
    if maximum > 0 {
        let mut scrollbar_state = ScrollbarState::new(maximum + 1)
            .position(scroll)
            .viewport_content_length(height);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█")
                .style(crate::theme::border_style(false)),
            Rect::new(body.right() - 1, body.y, 1, body.height),
            &mut scrollbar_state,
        );
    }
}

/// Greedy wrap of styled units; the spans of one unit always stay on one line.
fn wrap_units(lines: &mut Vec<Line<'static>>, units: Vec<Vec<Span<'static>>>, width: usize) {
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for unit in units {
        let cells: usize = unit.iter().map(|span| span.content.width()).sum();
        if used > 0 && used + cells > width {
            lines.push(Line::from(std::mem::take(&mut current)));
            used = 0;
        }
        used += cells;
        current.extend(unit);
    }
    if !current.is_empty() {
        lines.push(Line::from(current));
    }
}

fn push_details(
    lines: &mut Vec<Line<'static>>,
    row: &UsageRow,
    ordered: &[&ModelWorkload],
    palette: &std::collections::BTreeMap<&str, Color>,
    width: u16,
    muted: Style,
) {
    let indent = "  ";
    let wrap_width = width.saturating_sub(2);
    let note = |lines: &mut Vec<Line<'static>>, text: String| {
        let mut wrapped = Vec::new();
        push_wrapped(&mut wrapped, &text, muted, wrap_width);
        for line in wrapped {
            let mut spans = vec![Span::raw(indent)];
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        }
    };
    // The row title says "loaded" for a partial reading; say the same here.
    let status = row.status.replacen("partial", "loaded with caveats", 1);
    note(lines, format!("Status: {status}"));
    if row.models.iter().any(|model| model.historical) {
        note(lines, "▒ historical estimate: usage linked to the same session before this pane's ownership was observed, not verified pane usage".into());
    }
    if row.historical_provider {
        note(
            lines,
            "Closed verified ownership · no current account binding for this provider".into(),
        );
    }
    if let Some(session_id) = &row.session_id {
        note(
            lines,
            format!(
                "Session {session_id} · found in the transcript store, no Ilium pane · latest event {}",
                timestamp(row.source_time_ms)
            ),
        );
    } else {
        note(
            lines,
            format!(
                "Latest binding observed {} · transcript latest event {}",
                timestamp(row.binding_since_ms),
                timestamp(row.source_time_ms)
            ),
        );
    }
    if let Some(quota) = &row.codex_quota {
        let reset = chrono::DateTime::from_timestamp(quota.reset_at, 0)
            .map(|time| time.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "unknown".into());
        note(lines, format!("Account reading {:.1}% · reset {reset} · observed {} · freshness unknown · account identity unknown", quota.used_percent, timestamp(Some(quota.observed_at_ms))));
    }
    if ordered.is_empty() {
        note(lines, "No model usage recorded in this range".into());
    }
    for model in ordered {
        let cost = model
            .relative_units
            .map(format_cost)
            .unwrap_or_else(|| "weight unknown".into());
        let text = format!(
            "{}{} · {cost} · in {} · cache read {} · cache write {} · out {}{}",
            if model.historical {
                "historical · "
            } else {
                ""
            },
            model.model,
            model.tokens.input,
            model.tokens.cache_read,
            model.tokens.cache_write,
            model.tokens.output,
            if model.boundary_partial {
                " · boundary minute excluded"
            } else {
                ""
            }
        );
        let mut wrapped = Vec::new();
        push_wrapped(
            &mut wrapped,
            &text,
            Style::default(),
            wrap_width.saturating_sub(2),
        );
        for (index, line) in wrapped.into_iter().enumerate() {
            let mut spans = vec![Span::raw(indent)];
            spans.push(if index == 0 {
                Span::styled("■ ", Style::default().fg(palette[model.model.as_str()]))
            } else {
                Span::raw("  ")
            });
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_stats::TokenTotals;
    use ratatui::{backend::TestBackend, Terminal};

    fn workload(model: &str, units: Option<f64>) -> ModelWorkload {
        ModelWorkload {
            model: model.into(),
            tokens: TokenTotals {
                input: 3,
                output: 4,
                ..TokenTotals::default()
            },
            relative_units: units,
            boundary_partial: false,
            historical: false,
        }
    }

    fn row(pane_id: u64, title: &str, class: AgentClass) -> UsageRow {
        UsageRow {
            pane_id: NodeId(pane_id),
            title: title.into(),
            class,
            profile: None,
            models: vec![
                workload("claude-opus-4-8", Some(2.0)),
                workload("claude-haiku-4-5", Some(1.0)),
            ],
            status: "partial · rolling window".into(),
            binding_since_ms: None,
            source_time_ms: None,
            codex_quota: None,
            historical_provider: false,
            session_id: None,
        }
    }

    fn draw(state: &WeeklyUsageState, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), state))
            .unwrap();
        terminal
    }

    fn dump(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let width = usize::from(buffer.area.width);
        buffer
            .content()
            .chunks(width)
            .map(|cells| {
                let line: String = cells.iter().map(|cell| cell.symbol()).collect();
                line.trim_end().to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn bar_segments_run_cool_to_hot_and_use_tier_colours() {
        let mut state = WeeklyUsageState::default();
        state.rows.push(row(1, "agent", AgentClass::Claude));
        let terminal = draw(&state, 100, 30);
        let mut colours = Vec::new();
        for cell in terminal.backend().buffer().content() {
            if cell.symbol() == "█" && !colours.contains(&cell.fg) {
                colours.push(cell.fg);
            }
        }
        let rgb = |tier: crate::weekly_usage_heat::StrengthTier| {
            let (r, g, b) = tier.base_rgb();
            Color::Rgb(r, g, b)
        };
        use crate::weekly_usage_heat::StrengthTier::{Light, Strong};
        // The cheap, light model comes first even though it sorts after by name.
        assert_eq!(colours, vec![rgb(Light), rgb(Strong)]);
    }

    #[test]
    fn bar_cells_are_proportional_to_cost_not_token_count() {
        let mut agent = row(1, "agent", AgentClass::Claude);
        // Same tokens, three times the price: three times the width.
        agent.models = vec![
            workload("claude-haiku-4-5", Some(1.0)),
            workload("claude-opus-4-8", Some(3.0)),
        ];
        let mut state = WeeklyUsageState::default();
        state.rows.push(agent);
        let terminal = draw(&state, 84, 30);
        let mut cheap = 0;
        let mut dear = 0;
        let (cheap_rgb, dear_rgb) = (
            crate::weekly_usage_heat::StrengthTier::Light.base_rgb(),
            crate::weekly_usage_heat::StrengthTier::Strong.base_rgb(),
        );
        for cell in terminal.backend().buffer().content() {
            if cell.symbol() == "█" {
                if cell.fg == Color::Rgb(cheap_rgb.0, cheap_rgb.1, cheap_rgb.2) {
                    cheap += 1;
                } else if cell.fg == Color::Rgb(dear_rgb.0, dear_rgb.1, dear_rgb.2) {
                    dear += 1;
                }
            }
        }
        assert!(cheap > 0 && dear > 0);
        assert!(
            (dear as i64 - 3 * cheap as i64).abs() <= 3,
            "cheap {cheap} dear {dear}"
        );
    }

    #[test]
    fn same_tier_models_keep_distinct_shades_in_bars_and_legend() {
        let mut agent = row(1, "agent", AgentClass::Claude);
        agent.models = vec![
            workload("claude-opus-4-7", Some(1.0)),
            workload("claude-opus-4-8", Some(1.0)),
        ];
        let mut state = WeeklyUsageState::default();
        state.rows.push(agent);
        let terminal = draw(&state, 100, 30);
        let mut colours = Vec::new();
        for cell in terminal.backend().buffer().content() {
            if cell.symbol() == "█" && !colours.contains(&cell.fg) {
                colours.push(cell.fg);
            }
        }
        assert_eq!(colours.len(), 2);
        let legend: Vec<_> = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.symbol() == "■")
            .map(|cell| cell.fg)
            .collect();
        assert_eq!(legend.len(), 2);
        assert!(legend.iter().all(|colour| colours.contains(colour)));
    }

    #[test]
    fn a_measured_zero_is_shown_and_unknown_weight_is_never_zero() {
        let mut empty = row(1, "idle agent", AgentClass::Codex);
        empty.models.clear();
        let mut unsupported = row(2, "mystery agent", AgentClass::Codex);
        unsupported.models = vec![workload("mystery-model", None)];
        let mut state = WeeklyUsageState::default();
        state.rows = vec![empty, unsupported];
        let text = dump(&draw(&state, 110, 30));
        assert!(text.contains("$0.00"), "{text}");
        assert!(text.contains("weight unsupported"), "{text}");
        assert!(!text.contains("-0"));
    }

    #[test]
    fn tiny_costs_do_not_round_to_zero() {
        assert_eq!(format_cost(0.0), "$0.00");
        assert_eq!(format_cost(0.0000001), "<$0.01");
        assert_eq!(format_cost(1.23456), "$1.235");
        assert_eq!(format_cost(12.345), "$12.35");
    }

    #[test]
    fn clicking_a_row_opens_it_and_clicking_again_closes_it() {
        let mut state = WeeklyUsageState::default();
        state.rows = vec![row(1, "agent", AgentClass::Claude)];
        let id = state.rows[0].id();
        let click = |state: &mut WeeklyUsageState| {
            let y = state.hit_rows.borrow().first().unwrap().0;
            state.mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            });
        };
        draw(&state, 100, 30);
        click(&mut state);
        assert_eq!(state.expanded, vec![id]);
        let open = dump(&draw(&state, 100, 40));
        assert!(open.contains("Status:"), "{open}");
        assert!(open.contains("▾"));
        click(&mut state);
        assert!(state.expanded.is_empty());
        let closed = dump(&draw(&state, 100, 40));
        assert!(!closed.contains("Status:"));
    }

    #[test]
    fn custom_agents_get_their_own_section_without_the_provider_account_line() {
        let mut custom = row(2, "tuned", AgentClass::Claude);
        custom.profile = Some(ProfileLabel {
            id: "tuned-id".into(),
            name: "Tuned Claude".into(),
            icon: "✦".into(),
        });
        let mut state = WeeklyUsageState::default();
        state.rows = vec![row(1, "plain", AgentClass::Claude), custom];
        // Same pane, same provider: the custom profile is a different row.
        assert_ne!(state.rows[0].id(), state.rows[1].id());
        let screen = dump(&draw(&state, 120, 50));
        assert!(screen.contains("CLAUDE"), "{screen}");
        assert!(
            screen.contains("✦ Tuned Claude · custom, based on Claude"),
            "{screen}"
        );
        assert!(
            screen.contains("own backend · no shared account total or quota"),
            "{screen}"
        );
        let plain = screen.find("plain").unwrap();
        let heading = screen.find("Tuned Claude").unwrap();
        let tuned = screen.rfind("tuned").unwrap();
        assert!(plain < heading && heading < tuned, "{screen}");
    }

    #[test]
    fn proxy_sessions_found_on_disk_get_their_own_section_after_claude() {
        let mut proxy = row(2, "Kilo Proxy Brainiac · kilo-proxy", AgentClass::Claude);
        proxy.profile = Some(ProfileLabel {
            id: crate::weekly_usage_discovery::PROXY_PROFILE_ID.into(),
            name: "Kilo Proxy".into(),
            icon: "🧠".into(),
        });
        let mut state = WeeklyUsageState::default();
        state.rows = vec![row(1, "plain", AgentClass::Claude), proxy];
        let screen = dump(&draw(&state, 120, 50));
        assert!(
            screen.contains("Kilo Proxy · own backend · started outside Ilium"),
            "{screen}"
        );
        let plain = screen.find("plain").unwrap();
        let heading = screen.find("started outside Ilium").unwrap();
        let proxied = screen.rfind("Brainiac").unwrap();
        assert!(plain < heading && heading < proxied, "{screen}");
    }

    #[test]
    fn space_toggles_the_selected_row_and_arrows_only_move() {
        let mut state = WeeklyUsageState::default();
        state.rows = vec![
            row(1, "first", AgentClass::Claude),
            row(2, "second", AgentClass::Claude),
        ];
        state.selected = Some(state.rows[0].id());
        state.key(KeyCode::Down);
        assert!(state.expanded.is_empty());
        assert_eq!(state.selected, Some(state.rows[1].id()));
        state.key(KeyCode::Char(' '));
        state.key(KeyCode::Char(' '));
        assert!(state.expanded.is_empty());
        state.key(KeyCode::Char(' '));
        assert_eq!(state.expanded, vec![state.rows[1].id()]);
    }

    #[test]
    fn range_tabs_are_clickable_and_keys_change_the_range() {
        let mut state = WeeklyUsageState::default();
        state.rows = vec![row(1, "agent", AgentClass::Claude)];
        let terminal = draw(&state, 100, 30);
        assert!(dump(&terminal).contains("1m"));
        let hits = state.range_hits.borrow().clone();
        assert_eq!(hits.len(), UsageRange::ALL.len());
        let (rect, range) = hits[0];
        assert_eq!(range, UsageRange::Hours6);
        state.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 1,
            row: rect.y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
        assert_eq!(state.range, UsageRange::Hours6);
        state.key(KeyCode::Right);
        assert_eq!(state.range, UsageRange::Hours12);
        state.key(KeyCode::Left);
        state.key(KeyCode::Left);
        assert_eq!(state.range, UsageRange::Hours6);
        let text = dump(&draw(&state, 100, 30));
        assert!(text.contains("last 6h"), "{text}");
        assert!(!text.to_lowercase().contains("weekly"));
    }

    #[test]
    fn the_close_control_uses_its_rendered_rectangle() {
        let mut state = WeeklyUsageState::default();
        draw(&state, 100, 30);
        let close = state.close_hit.get();
        assert!(close.width > 0);
        let closed = state.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: close.x,
            row: close.y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
        assert!(closed);
        assert!(!state.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }));
    }

    #[test]
    fn provider_rows_for_the_same_pane_select_independently() {
        let mut state = WeeklyUsageState::default();
        state.rows = vec![
            row(1, "same pane", AgentClass::Claude),
            row(1, "same pane", AgentClass::Codex),
        ];
        state.rows[0].historical_provider = true;
        let claude = state.rows[0].id();
        let codex = state.rows[1].id();
        state.selected = Some(claude);
        state.key(KeyCode::Down);
        assert_eq!(state.selected, Some(codex));
        state.key(KeyCode::Up);
        assert_eq!(state.selected, Some(claude));
        draw(&state, 100, 45);
        let y = state
            .hit_rows
            .borrow()
            .iter()
            .find(|(_, id)| *id == codex)
            .unwrap()
            .0;
        state.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
        assert_eq!(state.selected, Some(codex));
        let claude_y = state
            .hit_rows
            .borrow()
            .iter()
            .find(|(_, id)| *id == claude)
            .unwrap()
            .0;
        state.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: claude_y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
        let text = dump(&draw(&state, 100, 60));
        assert!(text.contains("no current account binding"), "{text}");
    }

    #[test]
    fn rows_show_a_short_status_and_keep_raw_errors_in_open_details() {
        let raw = "unavailable · Statistics transcript line size limit reached: used 1 bytes, next chunk requires 2, limit 3";
        assert_eq!(short_status(raw), "unavailable");
        assert_eq!(
            short_status("partial · verified ownership intervals"),
            "loaded"
        );
        let mut state = WeeklyUsageState::default();
        let mut failing = row(1, "failing agent", AgentClass::Claude);
        failing.models.clear();
        failing.status = raw.into();
        let failing_id = failing.id();
        state.rows = vec![failing, row(2, "other agent", AgentClass::Claude)];
        let text = dump(&draw(&state, 100, 40));
        assert!(text.contains("unavailable"));
        assert!(!text.contains("line size limit"));
        state.expanded.push(failing_id);
        let text = dump(&draw(&state, 100, 40));
        assert!(text.contains("line size limit"));
    }

    #[test]
    fn usage_screen_renders_and_degrades_on_small_terminals() {
        let mut state = WeeklyUsageState::default();
        state.rows = vec![
            row(1, "long agent title", AgentClass::Claude),
            row(2, "second", AgentClass::Codex),
        ];
        for (width, height) in [(100, 30), (80, 24), (30, 10), (12, 4), (1, 1)] {
            let terminal = draw(&state, width, height);
            if width == 100 {
                let text = dump(&terminal);
                assert!(text.contains("Agent usage"));
                assert!(text.contains("long agent title"));
                assert!(text.contains("CLAUDE"));
                assert!(text.contains("CODEX"));
                assert!(text.contains("claude-haiku-4-5 (light)"));
                assert!(text.contains("claude-opus-4-8 (strong)"));
            }
        }
    }

    /// Prints populated screens for human review:
    /// `cargo test -p ilium-client --lib print_usage_screens -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_usage_screens() {
        let mut state = WeeklyUsageState::default();
        let mut first = row(1, "Fix usage view", AgentClass::Claude);
        first.models = vec![
            workload("claude-fable-5-2", Some(14.0)),
            workload("claude-opus-4-8", Some(9.0)),
            workload("claude-sonnet-5-5", Some(3.0)),
            workload("claude-haiku-4-5", Some(0.6)),
        ];
        let mut second = row(2, "Ship build", AgentClass::Claude);
        second.models = vec![
            workload("claude-opus-4-8", Some(2.0)),
            workload("claude-haiku-4-5", Some(0.05)),
        ];
        let mut codex = row(3, "Codex review", AgentClass::Codex);
        codex.models = vec![
            workload("gpt-6.1-sol", Some(6.0)),
            workload("gpt-6.1-luna", Some(0.4)),
            workload("mystery-model", None),
        ];
        let mut failing = row(4, "Broken", AgentClass::Codex);
        failing.models.clear();
        failing.status = "unavailable · Statistics transcript line size limit reached".into();
        state.expanded.push(first.id());
        state.selected = Some(second.id());
        state.rows = vec![first, second, codex, failing];
        for (width, height) in [(120, 44), (80, 24)] {
            println!(
                "---- {width}x{height}\n{}",
                dump(&draw(&state, width, height))
            );
        }
    }

    #[test]
    fn narrow_legend_wraps_long_model_names_without_losing_text() {
        let mut agent = row(1, "agent", AgentClass::Claude);
        let model_name = "claude-model-with-a-long-version";
        agent.models[0].model = model_name.into();
        let mut state = WeeklyUsageState::default();
        state.rows.push(agent);
        let text = dump(&draw(&state, 44, 60));
        assert!(text.contains(model_name), "{text}");
    }

    #[test]
    fn wrapped_titles_preserve_unicode_graphemes() {
        let mut lines = Vec::new();
        push_wrapped(&mut lines, " agent 界界 e\u{301}", Style::default(), 5);
        let text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(
            text.split_whitespace().collect::<String>(),
            " agent 界界 e\u{301}"
                .split_whitespace()
                .collect::<String>()
        );
        assert!(lines.iter().all(|line| line.width() <= 5));
    }

    #[test]
    fn details_wrap_between_facts_and_words_never_inside_a_word() {
        let mut lines = Vec::new();
        push_wrapped(
            &mut lines,
            "local account 22.0% · reset 2026-11-25 08:00:00 UTC · pane binding unknown",
            Style::default(),
            30,
        );
        let rendered: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();
        assert!(
            rendered.iter().all(|line| line.width() <= 30),
            "{rendered:?}"
        );
        for word in ["unknown", "binding", "account", "2026-11-25"] {
            assert!(
                rendered.iter().any(|line| line.contains(word)),
                "{word}: {rendered:?}"
            );
        }
        assert!(
            !rendered.iter().any(|line| line.ends_with("unkn")),
            "{rendered:?}"
        );
    }

    #[test]
    fn mouse_selection_uses_rendered_row_geometry_after_scroll() {
        let mut state = WeeklyUsageState::default();
        state.rows = (1..8)
            .map(|id| row(id, "agent", AgentClass::Codex))
            .collect();
        state.scroll.set(4);
        draw(&state, 80, 14);
        let (y, id) = state.hit_rows.borrow().first().copied().unwrap();
        state.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
        assert_eq!(state.selected, Some(id));
    }

    #[test]
    fn usage_full_composition_reserves_the_voice_footer() {
        let mut state = WeeklyUsageState::default();
        let mut agent = row(1, "compact agent", AgentClass::Claude);
        agent.status = "partial ownership coverage ".repeat(80);
        state.rows.push(agent);
        let mut app = crate::app::App::new("usage footer".into(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 60, 16));
        app.mode = crate::app::Mode::WeeklyUsage(Box::new(state));
        let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        for x in 0..app.layout.voice_control_area.x {
            assert_eq!(
                buffer[(x, 15)].symbol(),
                " ",
                "usage must not occupy the footer"
            );
        }
        let footer: String = (0..60).map(|x| buffer[(x, 15)].symbol()).collect();
        assert!(footer.contains("VOICE"));
        let crate::app::Mode::WeeklyUsage(state) = &app.mode else {
            panic!("usage mode")
        };
        assert!(state.hit_rows.borrow().iter().all(|(y, _)| *y < 15));
    }
}
