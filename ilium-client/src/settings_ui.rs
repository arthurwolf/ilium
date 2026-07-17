//! Renders `crate::app::Mode::Settings`: the full-screen settings view.
//!
//! Design brief (the user's own words -- see `crate::app::SettingsState`'s
//! doc comment for the full quote, kept there since `App` owns the state
//! this module only renders): the **entire** screen is replaced, aerated
//! rather than boxy, tab list on the left / settings panel on the right
//! with *no separating border* between them (unlike every bordered panel
//! in `crate::theme::block`), usable at any terminal size via a scrollbar
//! plus mouse-wheel scrolling, and every control is a live, self-applying
//! toggle/stepper -- never a buffered form with a Save button.
//!
//! Adding a fourth Appearance setting? Add a variant to
//! `crate::app::AppearanceRow` and its `ALL` array, then extend
//! [`appearance_row_label`]/[`appearance_row_description`]/
//! [`appearance_row_value`] and `App::settings_adjust_row` -- row count,
//! scroll bounds, and mouse hit-testing all fall out of `AppearanceRow::ALL`
//! automatically, nothing else here needs to change. Adding another *tab*
//! is a bigger change: extend `crate::app::SettingsTab::ALL` and add a
//! render/hit-test branch here for it.
//!
//! Rendering and mouse hit-testing both read the same geometry
//! ([`compute_layout`], [`appearance_content_hit`], [`tab_at`]) so the two
//! can never drift apart -- the same pattern `crate::modal`'s create-group
//! dialog and `crate::tree_ui`'s row controls already use.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

use crate::app::{
    App, AppearanceRow, InferenceRow, InferenceSettingField, InferenceTestState, KanbanBoardRow,
    OllamaModelDiscoveryState, SettingsState, SettingsTab, SoundRow,
};
use crate::config::{
    EditorSettings, KanbanBoardSettings, KeyboardSettings, SessionSettings, TerminalSettings,
    UiSettings,
};
use crate::keymap::{self, ShortcutBase, SHORTCUT_BASE_PRESETS};
use crate::theme::{self, ColorScheme};

/// Fixed header height: a title line (with the close button right-aligned
/// on it) plus a dim "Esc or q to close" hint line before the tab list /
/// content body begins.
const HEADER_HEIGHT: u16 = 2;
/// Width of the left tab list column.
const TAB_LIST_WIDTH: u16 = 26;
/// Blank columns between the tab list and the content panel -- this, not a
/// border character, is what separates them (see the module doc comment).
const GAP_WIDTH: u16 = 3;

/// One blank line above the first tab, then one line per tab plus one blank
/// spacer line after it -- see [`tab_at`], which must reproduce this exact
/// arithmetic to hit-test a click back to a tab.
const TAB_LIST_TOP_PADDING: u16 = 1;
const TAB_ROW_HEIGHT: u16 = 2;

/// One blank line above the first row, then one label/value line + one dim
/// description line + one blank spacer per row -- see
/// [`appearance_content_hit`], which must reproduce this exact arithmetic.
const APPEARANCE_TOP_PADDING: u16 = 1;
const APPEARANCE_ROW_HEIGHT: u16 = 3;

/// Column where a row's value control (`‹ value ›`) begins -- two leading
/// spaces plus the padded label column. Shared by rendering and
/// [`appearance_content_hit`].
const LABEL_COLUMN_WIDTH: u16 = 40;
const ROW_LEFT_INSET: u16 = 2;
/// Width of the `‹ ` hot zone that decrements/cycles-back a value; a click
/// anywhere else in the control increments/cycles-forward.
const DECREMENT_ZONE_WIDTH: u16 = 2;

/// The screen regions [`render`] draws into and mouse hit-testing reads
/// back out -- computed fresh from `area` every frame/event rather than
/// cached, since it's cheap pure arithmetic (mirrors `crate::layout::UiLayout`
/// in spirit, just for this one overlay instead of the whole screen).
pub struct SettingsLayout {
    pub header_area: Rect,
    pub tab_list_area: Rect,
    pub content_area: Rect,
}

/// Splits `area` into the header/tab-list/content regions. No vertical
/// border between the tab list and content -- [`GAP_WIDTH`] blank columns
/// instead, per the module doc comment's "aerated, not boxy" brief.
pub fn compute_layout(area: Rect) -> SettingsLayout {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(HEADER_HEIGHT), Constraint::Min(1)])
        .split(area);
    let header_area = rows[0];
    let body_area = rows[1];

    let tab_list_width = TAB_LIST_WIDTH.min(body_area.width);
    let gap_width = GAP_WIDTH.min(body_area.width.saturating_sub(tab_list_width));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(tab_list_width),
            Constraint::Length(gap_width),
            Constraint::Min(1),
        ])
        .split(body_area);

    SettingsLayout {
        header_area,
        tab_list_area: columns[0],
        content_area: columns[2],
    }
}

/// Label for the clickable close control in the top-right corner of the
/// header -- see [`close_button_hit`] for its matching hit-test rect. Kept
/// short so it reads as a button, not a sentence.
const CLOSE_LABEL: &str = "\u{2715} Close";

/// Draws the full-screen settings view. Callers must skip drawing anything
/// else this frame (see `crate::ui::draw`'s `Mode::Settings` branch) -- this
/// replaces the entire screen, it isn't a popup layered on top. Two ways to
/// leave are always on screen so the view never feels like a trap: the
/// `CLOSE_LABEL` button at the top right (mouse) and the hint line under the
/// title (keyboard) -- both call back to `Mode::Normal`, see
/// `crate::keys::handle_settings_event`/`crate::mouse::handle_settings_mouse`.
pub fn render(frame: &mut Frame, area: Rect, app: &App, state: &SettingsState) {
    frame.render_widget(Clear, area);
    let layout = compute_layout(area);

    render_header(frame, layout.header_area);
    render_tab_list(frame, layout.tab_list_area, state.tab);
    match state.tab {
        SettingsTab::Inference => {
            let lines = inference_lines(
                &app.inference_settings,
                app.inference_test_result.as_ref(),
                &app.inference_test_state,
                &app.ollama_model_discovery,
                &app.ollama_models,
                state.selected_row,
            );
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::Appearance => {
            let lines = appearance_lines(&app.ui_settings, state.selected_row);
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::Terminal => render_scrollable(
            frame,
            layout.content_area,
            terminal_lines(&app.terminal_settings, state.selected_row),
            state.scroll,
        ),
        SettingsTab::Editor => render_scrollable(
            frame,
            layout.content_area,
            editor_lines(&app.editor_settings, state.selected_row),
            state.scroll,
        ),
        SettingsTab::Session => render_scrollable(
            frame,
            layout.content_area,
            session_lines(&app.session_settings, state.selected_row),
            state.scroll,
        ),
        SettingsTab::Keyboard => {
            let lines = keyboard_lines(&app.keyboard_settings);
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::KanbanBoard => {
            let lines = kanban_board_lines(&app.kanban_board_settings, state.selected_row);
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::Sound => {
            let lines = sound_lines(
                &app.sound_settings,
                &app.sound_discovery,
                state.selected_row,
            );
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::About => {
            render_scrollable(frame, layout.content_area, about_lines(), state.scroll);
        }
    }
}

/// Renders the title line (with the [`CLOSE_LABEL`] button right-aligned on
/// it) and a dim keyboard-hint line beneath it -- see [`close_button_hit`]
/// for the button's hit-test rect, which must track this layout.
fn render_header(frame: &mut Frame, area: Rect) {
    let title = Line::from(Span::styled(
        "\u{2699} Settings",
        Style::new().add_modifier(Modifier::BOLD),
    ));
    let close = Line::from(Span::styled(
        CLOSE_LABEL,
        theme::selected_style().add_modifier(Modifier::BOLD),
    ))
    .alignment(Alignment::Right);

    frame.render_widget(Paragraph::new(title), area);
    frame.render_widget(Paragraph::new(close), area);

    if area.height > 1 {
        let hint_area = Rect::new(area.x, area.y + 1, area.width, 1);
        let hint = Line::from(Span::styled(
            "Esc or q to close",
            Style::new().add_modifier(Modifier::DIM),
        ));
        frame.render_widget(Paragraph::new(hint), hint_area);
    }
}

/// The header row's `CLOSE_LABEL` button rect, right-aligned exactly as
/// [`render_header`] draws it -- a click anywhere in this rect closes
/// settings, same as `Esc`/`q`. See `crate::mouse::handle_settings_mouse`.
pub fn close_button_hit(header_area: Rect, position: Position) -> bool {
    let label_width = CLOSE_LABEL.chars().count() as u16;
    if header_area.height == 0 || label_width > header_area.width {
        return false;
    }
    let button_area = Rect::new(
        header_area.right() - label_width,
        header_area.y,
        label_width,
        1,
    );
    button_area.contains(position)
}

/// One blank top-padding line, then each tab's label with one blank spacer
/// line after it -- see [`tab_at`] for the matching hit-test arithmetic.
fn render_tab_list(frame: &mut Frame, area: Rect, active: SettingsTab) {
    let mut lines = Vec::with_capacity(1 + SettingsTab::ALL.len() * usize::from(TAB_ROW_HEIGHT));
    lines.push(Line::from(""));
    for tab in SettingsTab::ALL {
        let style = if tab == active {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        lines.push(Line::from(Span::styled(
            format!("  {}", tab.label()),
            style,
        )));
        lines.push(Line::from(""));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// The tab a click at `position` lands on, if any -- reproduces
/// [`render_tab_list`]'s exact line arithmetic rather than re-deriving it.
pub fn tab_at(tab_list_area: Rect, position: Position) -> Option<SettingsTab> {
    if !tab_list_area.contains(position) {
        return None;
    }
    let row = position.y - tab_list_area.y;
    if row < TAB_LIST_TOP_PADDING {
        return None;
    }
    let row = row - TAB_LIST_TOP_PADDING;
    if !row.is_multiple_of(TAB_ROW_HEIGHT) {
        return None;
    }
    let index = usize::from(row / TAB_ROW_HEIGHT);
    SettingsTab::ALL.get(index).copied()
}

/// Renders `lines` scrolled by `scroll` rows, adding a vertical scrollbar
/// only once the content is actually taller than `area` -- matches
/// `crate::ui`'s own `draw_terminal_scrollbar`/`draw_rendered_scrollbar`.
fn render_scrollable(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>, scroll: u16) {
    let total_lines = lines.len() as u16;
    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(paragraph, area);

    if total_lines > area.height {
        let mut scrollbar_state =
            ScrollbarState::new(usize::from(total_lines)).position(usize::from(scroll));
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(" "))
            .style(theme::border_style(false));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// The largest valid `scroll` value for `tab` at `content_area`'s current
/// height -- callers (`crate::keys`/`crate::mouse`) clamp scroll-wheel and
/// keyboard scrolling to this so the view can never scroll past its own end.
pub fn max_scroll(tab: SettingsTab, app: &App, selected_row: usize, content_area: Rect) -> u16 {
    let total_lines = match tab {
        SettingsTab::Inference => inference_lines(
            &app.inference_settings,
            app.inference_test_result.as_ref(),
            &app.inference_test_state,
            &app.ollama_model_discovery,
            &app.ollama_models,
            selected_row,
        )
        .len() as u16,
        SettingsTab::Appearance => appearance_lines(&app.ui_settings, selected_row).len() as u16,
        SettingsTab::Terminal => terminal_lines(&app.terminal_settings, selected_row).len() as u16,
        SettingsTab::Editor => editor_lines(&app.editor_settings, selected_row).len() as u16,
        SettingsTab::Session => session_lines(&app.session_settings, selected_row).len() as u16,
        SettingsTab::Keyboard => keyboard_lines(&app.keyboard_settings).len() as u16,
        SettingsTab::KanbanBoard => {
            kanban_board_lines(&app.kanban_board_settings, selected_row).len() as u16
        }
        SettingsTab::Sound => {
            sound_lines(&app.sound_settings, &app.sound_discovery, selected_row).len() as u16
        }
        SettingsTab::About => about_lines().len() as u16,
    };
    total_lines.saturating_sub(content_area.height)
}

/// Visible settings depend on the selected provider. This keeps Kilo's
/// no-key configuration honest and prevents irrelevant empty fields.
pub fn inference_rows(settings: &ilium_inference::InferenceSettings) -> Vec<InferenceRow> {
    use ilium_inference::InferenceProviderKind;
    let mut rows = vec![InferenceRow::Provider];
    match settings.selected_provider {
        InferenceProviderKind::KiloGateway => {}
        InferenceProviderKind::Ollama => rows.extend([
            InferenceRow::Field(InferenceSettingField::OllamaUrl),
            InferenceRow::RefreshOllamaModels,
            InferenceRow::Field(InferenceSettingField::OllamaModel),
        ]),
        InferenceProviderKind::OpenAi => rows.extend([
            InferenceRow::Field(InferenceSettingField::OpenAiUrl),
            InferenceRow::Field(InferenceSettingField::OpenAiApiKey),
            InferenceRow::Field(InferenceSettingField::OpenAiModel),
        ]),
        InferenceProviderKind::Anthropic => rows.extend([
            InferenceRow::Field(InferenceSettingField::AnthropicUrl),
            InferenceRow::Field(InferenceSettingField::AnthropicApiKey),
            InferenceRow::Field(InferenceSettingField::AnthropicModel),
        ]),
        InferenceProviderKind::OpenRouter => rows.extend([
            InferenceRow::Field(InferenceSettingField::OpenRouterApiKey),
            InferenceRow::Field(InferenceSettingField::OpenRouterModel),
        ]),
    }
    rows.push(InferenceRow::Test);
    rows
}

fn inference_value(
    row: InferenceRow,
    settings: &ilium_inference::InferenceSettings,
    ollama_model_discovery: &OllamaModelDiscoveryState,
) -> String {
    match row {
        InferenceRow::Provider => format!("‹ {} ›", settings.selected_provider.label()),
        InferenceRow::RefreshOllamaModels => match ollama_model_discovery {
            OllamaModelDiscoveryState::Loading { started_at, .. } => {
                let frame_index = (started_at.elapsed().as_millis()
                    / crate::tree_ui::SPINNER_FRAME_MS) as usize
                    % crate::tree_ui::SPINNER_FRAMES.len();
                format!(
                    "[ {} Discovering models… ]",
                    crate::tree_ui::SPINNER_FRAMES[frame_index]
                )
            }
            OllamaModelDiscoveryState::Failed { .. } => "[ Retry model discovery ]".to_string(),
            OllamaModelDiscoveryState::Idle | OllamaModelDiscoveryState::Loaded { .. } => {
                "[ Load models from Ollama ]".to_string()
            }
        },
        InferenceRow::Test => "[ Test provider ]".to_string(),
        InferenceRow::Field(field) => match field {
            InferenceSettingField::OllamaUrl => settings.ollama.base_url.clone(),
            InferenceSettingField::OllamaModel => settings.ollama.model.clone(),
            InferenceSettingField::OpenAiUrl => settings.openai.base_url.clone(),
            InferenceSettingField::OpenAiApiKey => masked_key(&settings.openai.api_key),
            InferenceSettingField::OpenAiModel => settings.openai.model.clone(),
            InferenceSettingField::AnthropicUrl => settings.anthropic.base_url.clone(),
            InferenceSettingField::AnthropicApiKey => masked_key(&settings.anthropic.api_key),
            InferenceSettingField::AnthropicModel => settings.anthropic.model.clone(),
            InferenceSettingField::OpenRouterApiKey => masked_key(&settings.openrouter.api_key),
            InferenceSettingField::OpenRouterModel => settings.openrouter.model.clone(),
        },
    }
}

fn masked_key(key: &str) -> String {
    if key.is_empty() {
        "Not set (Enter to edit)".to_string()
    } else {
        // API keys are always ASCII; safe to use byte slicing to get last 4 chars
        let start = key.len().saturating_sub(4);
        format!("••••{}", &key[start..])
    }
}
fn inference_label(row: InferenceRow) -> &'static str {
    match row {
        InferenceRow::Provider => "Provider",
        InferenceRow::Field(field) => field.label(),
        InferenceRow::RefreshOllamaModels => "Available models",
        InferenceRow::Test => "Test",
    }
}
fn inference_lines(
    settings: &ilium_inference::InferenceSettings,
    test_result: Option<&crate::inference_test::InferenceTestResult>,
    test_state: &InferenceTestState,
    ollama_model_discovery: &OllamaModelDiscoveryState,
    ollama_models: &[String],
    selected_row: usize,
) -> Vec<Line<'static>> {
    let rows = inference_rows(settings);
    let mut lines = vec![Line::from("")];
    if settings.selected_provider == ilium_inference::InferenceProviderKind::KiloGateway {
        lines.push(Line::from(Span::styled("  Kilo Gateway is useful to try things out. Do not send secrets: requests may be used for LLM training.", Style::new().fg(Color::Yellow))));
        lines.push(Line::from(""));
    }
    for (index, row) in rows.into_iter().enumerate() {
        let label = inference_label(row);
        let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
        let label_style = if index == selected_row {
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new()
        };
        let value_style = if index == selected_row {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::accent_bg())
        };
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
            Span::styled(label, label_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(
                inference_value(row, settings, ollama_model_discovery),
                value_style,
            ),
        ]));
        let description = match row {
            InferenceRow::Provider => {
                "Choose the backend used by background title and organization inference."
            }
            InferenceRow::Field(InferenceSettingField::OllamaModel) => {
                "Use left/right to select a loaded local model, or Enter to type a model name."
            }
            InferenceRow::Field(_) => {
                "Press Enter to edit this value. API keys remain masked in this view."
            }
            InferenceRow::RefreshOllamaModels => {
                "Fetch the selectable model list from this Ollama API URL."
            }
            InferenceRow::Test => {
                "Send a harmless title-and-organize prompt and preview the returned tree structure."
            }
        };
        lines.push(Line::from(Span::styled(
            format!("    {description}"),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }
    if settings.selected_provider == ilium_inference::InferenceProviderKind::Ollama {
        lines.extend(ollama_discovery_lines(
            settings,
            ollama_model_discovery,
            ollama_models,
        ));
    }
    lines.extend(inference_test_log_lines(test_state));
    if let Some(result) = test_result {
        lines.push(Line::from(Span::styled(
            format!(
                "  Test result · processed in {:.2}s",
                result.elapsed.as_secs_f64()
            ),
            theme::selected_style().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  Preview (as it will appear in the left pane)",
            Style::new().add_modifier(Modifier::DIM),
        )));
        for group in &result.groups {
            lines.push(Line::from(Span::styled(
                format!("  ▾ {}", group.name),
                Style::new().add_modifier(Modifier::BOLD),
            )));
            for (index, pane) in group.panes.iter().enumerate() {
                let branch = if index + 1 == group.panes.len() {
                    "└─"
                } else {
                    "├─"
                };
                lines.push(Line::from(format!("     {branch} ▣ {pane}")));
            }
        }
    }
    lines
}

/// Renders a durable boxed operation trace within Settings. The full-screen
/// settings view hides the normal status bar, so explicit remote work must
/// remain visible here from launch through success or failure.
fn boxed_operation_log(
    title: &str,
    headline: String,
    headline_style: Style,
    log_lines: Vec<(String, Style)>,
) -> Vec<Line<'static>> {
    const HORIZONTAL_RULE: &str = "────────────────────────────────────────────────";
    let mut lines = vec![Line::from(Span::styled(
        format!("  ┌─ {title} {HORIZONTAL_RULE}┐"),
        Style::new().fg(Color::DarkGray),
    ))];
    lines.push(Line::from(vec![
        Span::styled("  │ ", Style::new().fg(Color::DarkGray)),
        Span::styled(headline, headline_style),
    ]));
    for (line, style) in log_lines {
        lines.push(Line::from(vec![
            Span::styled("  │ ", Style::new().fg(Color::DarkGray)),
            Span::styled(line, style),
        ]));
    }
    lines.push(Line::from(Span::styled(
        format!("  └{HORIZONTAL_RULE}┘"),
        Style::new().fg(Color::DarkGray),
    )));
    lines
}

/// Renders test execution as an explicit operation log instead of relying on
/// an invisible worker or a transient status-bar message.
fn inference_test_log_lines(test_state: &InferenceTestState) -> Vec<Line<'static>> {
    match test_state {
        InferenceTestState::Idle => Vec::new(),
        InferenceTestState::Running {
            provider,
            started_at,
        } => {
            let frame_index = (started_at.elapsed().as_millis() / crate::tree_ui::SPINNER_FRAME_MS)
                as usize
                % crate::tree_ui::SPINNER_FRAMES.len();
            boxed_operation_log(
                "Provider test log",
                format!(
                    "{} Testing {} · {:.1}s elapsed",
                    crate::tree_ui::SPINNER_FRAMES[frame_index],
                    provider.label(),
                    started_at.elapsed().as_secs_f64()
                ),
                theme::selected_style().add_modifier(Modifier::BOLD),
                vec![
                    (
                        "1. Build a harmless title-and-organize request.".to_string(),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                    (
                        format!("2. Send it to {}.", provider.label()),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                    (
                        "3. Wait for and validate the required JSON response.".to_string(),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                ],
            )
        }
        InferenceTestState::Succeeded { provider, elapsed } => boxed_operation_log(
            "Provider test log",
            format!(
                "✓ {} replied in {:.2}s",
                provider.label(),
                elapsed.as_secs_f64()
            ),
            theme::selected_style().add_modifier(Modifier::BOLD),
            vec![
                (
                    "1. Provider request completed successfully.".to_string(),
                    Style::new(),
                ),
                (
                    "2. JSON response validated; preview is shown below.".to_string(),
                    Style::new(),
                ),
            ],
        ),
        InferenceTestState::Failed {
            provider,
            error,
            elapsed,
        } => boxed_operation_log(
            "Provider test log",
            format!(
                "✗ {} test failed after {:.2}s",
                provider.label(),
                elapsed.as_secs_f64()
            ),
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            vec![
                (
                    format!("Provider error: {error}"),
                    Style::new().fg(Color::Red),
                ),
                (
                    "No settings were changed. Correct the endpoint/model and retry.".to_string(),
                    Style::new().add_modifier(Modifier::DIM),
                ),
            ],
        ),
    }
}

/// Renders the actual model-discovery trace inside Settings itself. The
/// normal status bar is intentionally not visible while Settings replaces the
/// whole screen, so an async operation must report its progress and outcome
/// here rather than only through `App::status_message`.
fn ollama_discovery_lines(
    settings: &ilium_inference::InferenceSettings,
    discovery: &OllamaModelDiscoveryState,
    models: &[String],
) -> Vec<Line<'static>> {
    let endpoint = settings.ollama.base_url.trim_end_matches('/');
    let tags_url = format!("{endpoint}/api/tags");
    let mut lines = vec![Line::from("")];
    match discovery {
        OllamaModelDiscoveryState::Idle => {
            lines.push(Line::from(Span::styled(
                "  Model discovery has not run in this client session.",
                Style::new().add_modifier(Modifier::DIM),
            )));
            lines.push(Line::from(Span::styled(
                format!("  Load models calls GET {tags_url}, then extracts each model name."),
                Style::new().add_modifier(Modifier::DIM),
            )));
        }
        OllamaModelDiscoveryState::Loading {
            endpoint,
            started_at,
        } => {
            let frame_index = (started_at.elapsed().as_millis() / crate::tree_ui::SPINNER_FRAME_MS)
                as usize
                % crate::tree_ui::SPINNER_FRAMES.len();
            let elapsed = started_at.elapsed().as_secs_f64();
            lines.extend(boxed_operation_log(
                "Ollama model discovery log",
                format!(
                    "{} Discovering local models from {endpoint} · {elapsed:.1}s elapsed",
                    crate::tree_ui::SPINNER_FRAMES[frame_index],
                ),
                theme::selected_style().add_modifier(Modifier::BOLD),
                vec![
                    (
                        format!("1. Connect to the configured Ollama API at {endpoint}."),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                    (
                        format!("2. Request GET {endpoint}/api/tags."),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                    (
                        "3. Validate the JSON response and extract model names.".to_string(),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                ],
            ));
        }
        OllamaModelDiscoveryState::Loaded {
            endpoint,
            model_count,
            elapsed,
        } => {
            lines.extend(boxed_operation_log(
                "Ollama model discovery log",
                format!("✓ GET /api/tags completed in {:.2}s", elapsed.as_secs_f64()),
                theme::selected_style().add_modifier(Modifier::BOLD),
                vec![
                    (format!("1. Connected to {endpoint}."), Style::new()),
                    (
                        format!("2. Extracted {model_count} selectable model name(s)."),
                        Style::new(),
                    ),
                    (
                        "3. Updated the selected model when needed.".to_string(),
                        Style::new(),
                    ),
                ],
            ));
        }
        OllamaModelDiscoveryState::Failed {
            endpoint,
            error,
            elapsed,
        } => {
            lines.extend(boxed_operation_log(
                "Ollama model discovery log",
                format!(
                    "✗ Model discovery failed after {:.2}s",
                    elapsed.as_secs_f64()
                ),
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
                vec![
                    (format!("Endpoint: {endpoint}"), Style::new().fg(Color::Red)),
                    (
                        format!("Ollama reported: {error}"),
                        Style::new().fg(Color::Red),
                    ),
                    (
                        format!("Retry calls GET {tags_url} after Ollama is available."),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                ],
            ));
        }
    }
    if !models.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Models returned by Ollama (use left/right on Ollama model to select)",
            Style::new().add_modifier(Modifier::BOLD),
        )));
        for model in models {
            let marker = if model == &settings.ollama.model {
                "[x]"
            } else {
                "[ ]"
            };
            let style = if marker == "[x]" {
                theme::selected_style().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            lines.push(Line::from(Span::styled(
                format!("    {marker} {model}"),
                style,
            )));
        }
    }
    lines
}

pub fn inference_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
    settings: &ilium_inference::InferenceSettings,
) -> Option<(InferenceRow, i32)> {
    if !content_area.contains(position) {
        return None;
    }
    let warning_lines =
        if settings.selected_provider == ilium_inference::InferenceProviderKind::KiloGateway {
            2
        } else {
            0
        };
    let line = usize::from(position.y.saturating_sub(content_area.y)) + usize::from(scroll);
    let offset = line.checked_sub(usize::from(APPEARANCE_TOP_PADDING) + warning_lines)?;
    if offset % usize::from(APPEARANCE_ROW_HEIGHT) != 0 {
        return None;
    }
    let row = *inference_rows(settings).get(offset / usize::from(APPEARANCE_ROW_HEIGHT))?;
    Some((
        row,
        if position.x < content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH + DECREMENT_ZONE_WIDTH
        {
            -1
        } else {
            1
        },
    ))
}

fn kanban_board_row_label(row: KanbanBoardRow) -> &'static str {
    match row {
        KanbanBoardRow::CardPreviewLines => "Card preview lines",
        KanbanBoardRow::MinimumColumnWidth => "Minimum column width",
    }
}

fn kanban_board_row_description(row: KanbanBoardRow) -> &'static str {
    match row {
        KanbanBoardRow::CardPreviewLines => {
            "Number of wrapped text lines shown on each card, from 1 to 10."
        }
        KanbanBoardRow::MinimumColumnWidth => {
            "Minimum terminal columns per board column before horizontal scrolling is used."
        }
    }
}

fn kanban_board_row_value(row: KanbanBoardRow, settings: &KanbanBoardSettings) -> String {
    match row {
        KanbanBoardRow::CardPreviewLines => format!("‹ {} ›", settings.card_preview_lines),
        KanbanBoardRow::MinimumColumnWidth => {
            format!("‹ {} ›", settings.minimum_column_width)
        }
    }
}

/// Kanban Board tab content. Both controls are client-global presentation
/// settings, so board-owned Markdown/folder data remains purely domain data.
fn kanban_board_lines(settings: &KanbanBoardSettings, selected_row: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("")];
    for (index, row) in KanbanBoardRow::ALL.into_iter().enumerate() {
        let label = kanban_board_row_label(row);
        let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
        let label_style = if index == selected_row {
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new()
        };
        let value_style = if index == selected_row {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::accent_bg())
        };
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
            Span::styled(label, label_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(kanban_board_row_value(row, settings), value_style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("    {}", kanban_board_row_description(row)),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "Click a card to open and edit it in the board's right-hand panel.",
        Style::new().add_modifier(Modifier::DIM),
    )));
    lines
}

/// Returns the row plus decrement/increment direction for either Kanban
/// Board stepper using the same three-line row geometry as rendering.
pub fn kanban_board_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
) -> Option<(KanbanBoardRow, i32)> {
    if !content_area.contains(position) {
        return None;
    }
    let line_index =
        usize::try_from(i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll))
            .ok()?;
    let row_offset = line_index.checked_sub(usize::from(APPEARANCE_TOP_PADDING))?;
    if row_offset % usize::from(APPEARANCE_ROW_HEIGHT) != 0 {
        return None;
    }
    let row = *KanbanBoardRow::ALL.get(row_offset / usize::from(APPEARANCE_ROW_HEIGHT))?;
    let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
    if position.x < control_start_x {
        return None;
    }
    let direction = if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
        -1
    } else {
        1
    };
    Some((row, direction))
}

fn sound_row_label(row: SoundRow) -> &'static str {
    match row {
        SoundRow::Source => "Sound source",
        SoundRow::File => "Selected sound file",
        SoundRow::Preview => "Preview",
        SoundRow::AgentFinished => "Agent finished",
        SoundRow::ApprovalRequired => "Agent needs approval",
        SoundRow::AgentStarted => "Agent started working",
        SoundRow::WaitingBackground => "Agent waits for background work",
    }
}

fn sound_row_description(row: SoundRow) -> &'static str {
    match row {
        SoundRow::Source => "Use the operating system beep or a discovered sound file.",
        SoundRow::File => "Left/Right cycles through files found in the folders listed below.",
        SoundRow::Preview => "Play the current choice once through the detached server.",
        SoundRow::AgentFinished => "A busy agent completed its turn and is waiting for you.",
        SoundRow::ApprovalRequired => "An agent entered a confirmation or approval prompt.",
        SoundRow::AgentStarted => "An idle or finished agent began a new turn.",
        SoundRow::WaitingBackground => "An agent is waiting for background workers it started.",
    }
}

fn selected_sound_label(
    settings: &ilium_sound::SoundSettings,
    discovery: &ilium_sound::SoundDiscovery,
) -> String {
    let Some(path) = settings.file.as_ref() else {
        return if discovery.sounds.is_empty() {
            "No sound files found".to_string()
        } else {
            "Not selected".to_string()
        };
    };
    discovery
        .sounds
        .iter()
        .find(|sound| &sound.path == path)
        .map(|sound| format!("{} — {}", sound.collection, sound.display_name))
        .unwrap_or_else(|| format!("{} (unavailable)", path.display()))
}

fn sound_row_value(
    row: SoundRow,
    settings: &ilium_sound::SoundSettings,
    discovery: &ilium_sound::SoundDiscovery,
) -> String {
    match row {
        SoundRow::Source => format!("‹ {} ›", settings.source.label()),
        SoundRow::File => format!("‹ {} ›", selected_sound_label(settings, discovery)),
        SoundRow::Preview => "[ Play ]".to_string(),
        event_row => event_row
            .event()
            .map(|event| {
                if settings.events.is_enabled(event) {
                    "[x] Enabled".to_string()
                } else {
                    "[ ] Disabled".to_string()
                }
            })
            .unwrap_or_default(),
    }
}

/// Sound-tab content: fixed controls first, then the exact existing folders
/// and files discovered for this operating system. The catalog both proves
/// where options came from and lets mouse users select a file directly.
fn sound_lines(
    settings: &ilium_sound::SoundSettings,
    discovery: &ilium_sound::SoundDiscovery,
    selected_row: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("")];
    for (index, row) in SoundRow::ALL.into_iter().enumerate() {
        let label = sound_row_label(row);
        let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
        let label_style = if index == selected_row {
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new()
        };
        let value_style = if index == selected_row {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::accent_bg())
        };
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
            Span::styled(label, label_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(sound_row_value(row, settings, discovery), value_style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("    {}", sound_row_description(row)),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "Discovered system sound folders",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    let truncation = if discovery.was_truncated {
        " (scan limit reached)"
    } else {
        ""
    };
    lines.push(Line::from(format!(
        "  {}: {} playable files across {} existing folders{truncation}",
        discovery.platform,
        discovery.sounds.len(),
        discovery.directories.len()
    )));
    for directory in &discovery.directories {
        lines.push(Line::from(format!(
            "  • {} — {}",
            directory.origin,
            directory.path.display()
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Available system sounds (click one, or use the selector above)",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    if discovery.sounds.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No supported sound files were found in the common folders on this system.",
            Style::new().add_modifier(Modifier::DIM),
        )));
    } else {
        for sound in &discovery.sounds {
            let is_selected = settings.file.as_ref() == Some(&sound.path);
            let marker = if is_selected { "[x]" } else { "[ ]" };
            let style = if is_selected {
                theme::selected_style().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            lines.push(Line::from(Span::styled(
                format!("  {marker} {} — {}", sound.collection, sound.display_name),
                style,
            )));
        }
    }
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundContentHit {
    Row { row: SoundRow, direction: i32 },
    File(usize),
}

/// Hit-tests fixed controls and dynamic discovered-file rows against the same
/// line arithmetic used by [`sound_lines`].
pub fn sound_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
    discovery: &ilium_sound::SoundDiscovery,
) -> Option<SoundContentHit> {
    if !content_area.contains(position) {
        return None;
    }
    let line_index =
        usize::try_from(i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll))
            .ok()?;
    if line_index >= usize::from(APPEARANCE_TOP_PADDING) {
        let row_offset = line_index - usize::from(APPEARANCE_TOP_PADDING);
        if row_offset % usize::from(APPEARANCE_ROW_HEIGHT) == 0 {
            let row_index = row_offset / usize::from(APPEARANCE_ROW_HEIGHT);
            if let Some(row) = SoundRow::ALL.get(row_index).copied() {
                let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
                if position.x >= control_start_x {
                    let direction = if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
                        -1
                    } else {
                        1
                    };
                    return Some(SoundContentHit::Row { row, direction });
                }
            }
        }
    }

    let first_sound_line = usize::from(APPEARANCE_TOP_PADDING)
        + SoundRow::ALL.len() * usize::from(APPEARANCE_ROW_HEIGHT)
        + 4
        + discovery.directories.len();
    let sound_index = line_index.checked_sub(first_sound_line)?;
    (sound_index < discovery.sounds.len()).then_some(SoundContentHit::File(sound_index))
}

/// Keyboard-tab content: one live selector, explicit A/B presets, and the
/// specific recommendation or warning for the currently selected letter.
fn keyboard_lines(keyboard: &KeyboardSettings) -> Vec<Line<'static>> {
    let shortcut_base = keyboard.shortcut_base;
    let advice = keymap::shortcut_base_advice(shortcut_base);
    let advice_heading = if advice.is_recommended {
        "Recommended"
    } else {
        "Warning"
    };
    let advice_color = if advice.is_recommended {
        Color::Green
    } else {
        Color::Yellow
    };
    let label = "Shortcut base";
    let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
    let control = format!("\u{2039} {} \u{203a}", shortcut_base.label());

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
            Span::styled(
                label,
                Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
            Span::raw(" ".repeat(padding)),
            Span::styled(
                control,
                theme::selected_style().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            "    Use Left/Right to browse A-Z, or type Shift+A-Z for an exact custom letter.",
            Style::new().add_modifier(Modifier::DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Recommended presets",
            Style::new().add_modifier(Modifier::BOLD),
        )),
    ];
    for (index, preset) in SHORTCUT_BASE_PRESETS.into_iter().enumerate() {
        let standard = if preset == ShortcutBase::A {
            "GNU Screen standard"
        } else {
            "tmux standard"
        };
        lines.push(Line::from(format!(
            "  [{}] {:<7}  {standard}",
            index + 1,
            preset.label()
        )));
    }
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            format!("{advice_heading}: {}", shortcut_base.label()),
            Style::new().fg(advice_color).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("  {}", advice.explanation)),
        Line::from(""),
        Line::from(Span::styled(
            "The new base applies immediately to input and every shortcut label, including Help.",
            Style::new().add_modifier(Modifier::DIM),
        )),
    ]);
    lines
}

/// Returns the decrement/increment direction for the Keyboard tab's one
/// selector. Geometry mirrors the selector line in [`keyboard_lines`].
pub fn keyboard_content_hit(content_area: Rect, scroll: u16, position: Position) -> Option<i32> {
    if !content_area.contains(position) {
        return None;
    }
    // Map screen position back to a content line index (the same
    // scroll-aware direction `appearance_content_hit` uses) rather than
    // projecting the selector's fixed content line forward onto the screen
    // -- the forward direction underflows via `saturating_sub` once `scroll`
    // exceeds `content_area.y + APPEARANCE_TOP_PADDING`, clamping to 0 and
    // producing a false-positive hit on whatever content has scrolled into
    // the first on-screen row.
    let line_index = i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll);
    if line_index != i32::from(APPEARANCE_TOP_PADDING) {
        return None;
    }
    let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
    if position.x < control_start_x {
        return None;
    }
    Some(if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
        -1
    } else {
        1
    })
}

/// Returns the explicit A/B preset whose rendered row was clicked.
pub fn keyboard_preset_at(
    content_area: Rect,
    scroll: u16,
    position: Position,
) -> Option<ShortcutBase> {
    if !content_area.contains(position) {
        return None;
    }
    let line_index = position
        .y
        .saturating_sub(content_area.y)
        .saturating_add(scroll);
    // `keyboard_lines`: blank, selector, description, blank, heading, then
    // one row per preset.
    let preset_index = usize::from(line_index.checked_sub(5)?);
    SHORTCUT_BASE_PRESETS.get(preset_index).copied()
}

fn appearance_row_label(row: AppearanceRow) -> &'static str {
    match row {
        AppearanceRow::AutoResizeTree => "Auto-resize tree panel on focus",
        AppearanceRow::TreeWidth => "Tree panel width",
        AppearanceRow::TreeOrder => "Tree order",
        AppearanceRow::AgentIdentifierMode => "Agent identifier",
        AppearanceRow::ClaudeAgentIcon => "Claude icon",
        AppearanceRow::CodexAgentIcon => "Codex icon",
        AppearanceRow::AntigravityAgentIcon => "Antigravity icon",
        AppearanceRow::ColorScheme => "Color theme",
        AppearanceRow::MotionLevel => "Motion level",
        AppearanceRow::SidebarDensity => "Sidebar density",
    }
}

fn appearance_row_description(row: AppearanceRow) -> &'static str {
    match row {
        AppearanceRow::AutoResizeTree => {
            "Widen the tree panel while it has mouse or keyboard focus, then ease back."
        }
        AppearanceRow::TreeWidth => "Collapsed width of the tree panel, in terminal columns.",
        AppearanceRow::TreeOrder => {
            "Order entries independently inside each group; split-view placement stays fixed."
        }
        AppearanceRow::AgentIdentifierMode => {
            "Show each agent type as its full name, one letter, a chosen icon, or nothing."
        }
        AppearanceRow::ClaudeAgentIcon => {
            "Icon used for Claude panes when Agent identifier is Selected icon."
        }
        AppearanceRow::CodexAgentIcon => {
            "Icon used for Codex panes when Agent identifier is Selected icon."
        }
        AppearanceRow::AntigravityAgentIcon => {
            "Icon used for Antigravity panes when Agent identifier is Selected icon."
        }
        AppearanceRow::ColorScheme => "ilium's built-in color presets.",
        AppearanceRow::MotionLevel => {
            "Full keeps spatial transitions; reduced and off minimise motion."
        }
        AppearanceRow::SidebarDensity => "Controls the vertical spacing used by the tree sidebar.",
    }
}

fn appearance_row_value(row: AppearanceRow, ui: &UiSettings) -> String {
    match row {
        AppearanceRow::AutoResizeTree => {
            if ui.auto_resize_tree_on_focus {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        }
        AppearanceRow::TreeWidth => ui.tree_width.to_string(),
        AppearanceRow::TreeOrder => ui.tree_order.label().to_string(),
        AppearanceRow::AgentIdentifierMode => ui.agent_identifiers.mode.label().to_string(),
        AppearanceRow::ClaudeAgentIcon => ui.agent_identifiers.claude_icon.label().to_string(),
        AppearanceRow::CodexAgentIcon => ui.agent_identifiers.codex_icon.label().to_string(),
        AppearanceRow::AntigravityAgentIcon => {
            ui.agent_identifiers.antigravity_icon.label().to_string()
        }
        AppearanceRow::ColorScheme => match ui.color_scheme {
            ColorScheme::Dark => "Dark".to_string(),
            ColorScheme::Light => "Light".to_string(),
        },
        AppearanceRow::MotionLevel => ui.motion_level.label().to_string(),
        AppearanceRow::SidebarDensity => ui.sidebar_density.label().to_string(),
    }
}

fn setting_lines(rows: &[(&str, String, &str)], selected_row: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("")];
    for (index, (label, value, description)) in rows.iter().enumerate() {
        let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
        let label_style = if index == selected_row {
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new()
        };
        let control_style = if index == selected_row {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::accent_bg())
        };
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
            Span::styled((*label).to_string(), label_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(format!("‹ {value} ›"), control_style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("    {description}"),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }
    lines
}
fn terminal_lines(settings: &TerminalSettings, selected: usize) -> Vec<Line<'static>> {
    setting_lines(
        &[
            (
                "Scrollback budget",
                format!("{} MiB", settings.scrollback_budget_mib),
                "Maximum retained terminal output per pane; takes effect for new terminal views.",
            ),
            (
                "New pane directory",
                settings.new_pane_directory.label().to_string(),
                "Choose the initial directory for a terminal pane.",
            ),
        ],
        selected,
    )
}
fn editor_lines(settings: &EditorSettings, selected: usize) -> Vec<Line<'static>> {
    setting_lines(
        &[
            (
                "Line numbers",
                on_off(settings.show_line_numbers),
                "Default visibility for local editor buffers.",
            ),
            (
                "Minimap",
                on_off(settings.show_minimap),
                "Default minimap visibility for local editor buffers.",
            ),
            (
                "Autosave",
                on_off(settings.autosave_enabled),
                "Save modified local files automatically.",
            ),
            (
                "Autosave delay",
                format!("{} ms", settings.autosave_delay_ms),
                "Delay after the latest edit before autosaving.",
            ),
            (
                "Markdown default",
                if settings.markdown_rendered_by_default {
                    "Rendered".into()
                } else {
                    "Source".into()
                },
                "Initial view for Markdown files opened in the editor.",
            ),
        ],
        selected,
    )
}
fn session_lines(settings: &SessionSettings, selected: usize) -> Vec<Line<'static>> {
    setting_lines(
        &[(
            "Recovery policy",
            settings.recovery_policy.label().to_string(),
            "Used by the detached server when it next starts this project session.",
        )],
        selected,
    )
}
fn on_off(value: bool) -> String {
    if value {
        "On".into()
    } else {
        "Off".into()
    }
}

/// One label/value line per `AppearanceRow`, each followed by a dim
/// description line and a blank spacer -- see [`appearance_content_hit`]
/// for the matching hit-test arithmetic, and the module doc comment for why
/// both read `AppearanceRow::ALL` instead of hardcoding a row count.
fn appearance_lines(ui: &UiSettings, selected_row: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(
        usize::from(APPEARANCE_TOP_PADDING)
            + AppearanceRow::ALL.len() * usize::from(APPEARANCE_ROW_HEIGHT),
    );
    lines.push(Line::from(""));
    for (index, row) in AppearanceRow::ALL.into_iter().enumerate() {
        let selected = index == selected_row;
        lines.push(appearance_row_line(row, ui, selected));
        lines.push(Line::from(Span::styled(
            format!("    {}", appearance_row_description(row)),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }
    lines
}

fn appearance_row_line(row: AppearanceRow, ui: &UiSettings, selected: bool) -> Line<'static> {
    let label = appearance_row_label(row);
    let value = appearance_row_value(row, ui);
    let control = format!("\u{2039} {value} \u{203a}");

    let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
    let label_style = if selected {
        Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::new()
    };
    let control_style = if selected {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme::accent_bg())
    };

    Line::from(vec![
        Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
        Span::styled(label, label_style),
        Span::raw(" ".repeat(padding)),
        Span::styled(control, control_style),
    ])
}

/// The `(row, direction)` a click at `position` should adjust, if any.
/// `direction` is `-1` for the `‹` hot zone, `+1` for anywhere else in the
/// control -- see `App::settings_adjust_row`. Reproduces
/// [`appearance_lines`]'s exact line arithmetic (top padding, three lines
/// per row) and [`appearance_row_line`]'s exact column arithmetic (left
/// inset, padded label column) rather than re-deriving either.
pub fn appearance_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
) -> Option<(AppearanceRow, i32)> {
    if !content_area.contains(position) {
        return None;
    }

    let line_index = i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll);
    let line_index = line_index - i32::from(APPEARANCE_TOP_PADDING);
    if line_index < 0 || line_index % i32::from(APPEARANCE_ROW_HEIGHT) != 0 {
        return None;
    }
    let row_index = usize::try_from(line_index / i32::from(APPEARANCE_ROW_HEIGHT)).ok()?;
    let row = AppearanceRow::ALL.get(row_index).copied()?;

    let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
    if position.x < control_start_x {
        return None;
    }
    let direction = if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
        -1
    } else {
        1
    };
    Some((row, direction))
}

/// Shared hit geometry for the small fixed-row settings tabs.
pub fn simple_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
    row_count: usize,
) -> Option<(usize, i32)> {
    if !content_area.contains(position) {
        return None;
    }
    let line_index = i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll)
        - i32::from(APPEARANCE_TOP_PADDING);
    if line_index < 0 || line_index % i32::from(APPEARANCE_ROW_HEIGHT) != 0 {
        return None;
    }
    let row = usize::try_from(line_index / i32::from(APPEARANCE_ROW_HEIGHT)).ok()?;
    if row >= row_count {
        return None;
    }
    let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
    (position.x >= control_start_x).then_some((
        row,
        if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
            -1
        } else {
            1
        },
    ))
}

/// Static "About" content: version plus a short description. No settings
/// live here, per the design brief -- see the module doc comment.
fn about_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            "ilium",
            Style::new().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Version {}", env!("CARGO_PKG_VERSION"))),
        Line::from(""),
        Line::from("A terminal multiplexer with a tree-structured pane list and AI"),
        Line::from(
            "coding agent awareness (Claude Code, Codex CLI, Antigravity CLI, and similar tools).",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    #[test]
    fn compute_layout_leaves_no_border_gap_between_tabs_and_content() {
        let layout = compute_layout(Rect::new(0, 0, 100, 40));
        assert!(layout.content_area.x > layout.tab_list_area.right());
        assert_eq!(
            layout.content_area.x - layout.tab_list_area.right(),
            GAP_WIDTH
        );
    }

    #[test]
    fn tab_at_maps_each_row_to_its_tab() {
        let area = Rect::new(0, 0, 30, 20);
        assert_eq!(
            tab_at(area, Position::new(2, 1)),
            Some(SettingsTab::Appearance)
        );
        assert_eq!(
            tab_at(area, Position::new(2, 3)),
            Some(SettingsTab::Keyboard)
        );
        assert_eq!(
            tab_at(area, Position::new(2, 5)),
            Some(SettingsTab::Terminal)
        );
        assert_eq!(tab_at(area, Position::new(2, 7)), Some(SettingsTab::Editor));
        assert_eq!(
            tab_at(area, Position::new(2, 9)),
            Some(SettingsTab::Session)
        );
        assert_eq!(
            tab_at(area, Position::new(2, 11)),
            Some(SettingsTab::KanbanBoard)
        );
        assert_eq!(tab_at(area, Position::new(2, 13)), Some(SettingsTab::Sound));
        assert_eq!(
            tab_at(area, Position::new(2, 15)),
            Some(SettingsTab::Inference)
        );
        assert_eq!(tab_at(area, Position::new(2, 17)), Some(SettingsTab::About));
        // Row 0 is the top-padding blank line -- no tab there.
        assert_eq!(tab_at(area, Position::new(2, 0)), None);
        // Row 2 is the blank spacer after the first tab.
        assert_eq!(tab_at(area, Position::new(2, 2)), None);
    }

    #[test]
    fn inference_rows_hide_impossible_fields_and_warn_for_kilo() {
        let settings = ilium_inference::InferenceSettings::default();
        assert_eq!(
            inference_rows(&settings),
            vec![InferenceRow::Provider, InferenceRow::Test]
        );
        let text = inference_lines(
            &settings,
            None,
            &InferenceTestState::Idle,
            &OllamaModelDiscoveryState::Idle,
            &[],
            0,
        )
        .into_iter()
        .map(|line| line.to_string())
        .collect::<String>();
        assert!(text.contains("Do not send secrets"));
        assert!(text.contains("LLM training"));

        let mut ollama = settings;
        ollama.selected_provider = ilium_inference::InferenceProviderKind::Ollama;
        assert!(inference_rows(&ollama).contains(&InferenceRow::RefreshOllamaModels));

        let discovered_models = vec!["qwen3.6:latest".to_string(), "gemma4:12b".to_string()];
        let loading_text = inference_lines(
            &ollama,
            None,
            &InferenceTestState::Idle,
            &OllamaModelDiscoveryState::Loading {
                endpoint: "http://127.0.0.1:11434".to_string(),
                started_at: std::time::Instant::now(),
            },
            &discovered_models,
            0,
        )
        .into_iter()
        .map(|line| line.to_string())
        .collect::<String>();
        assert!(loading_text.contains("Discovering local models"));
        assert!(loading_text.contains("Ollama model discovery log"));
        assert!(loading_text.contains("GET http://127.0.0.1:11434/api/tags"));
        assert!(loading_text.contains("qwen3.6:latest"));
    }

    #[test]
    fn inference_test_lifecycle_renders_a_boxed_running_log_and_failure() {
        let settings = ilium_inference::InferenceSettings::default();
        let running = inference_lines(
            &settings,
            None,
            &InferenceTestState::Running {
                provider: ilium_inference::InferenceProviderKind::Ollama,
                started_at: std::time::Instant::now(),
            },
            &OllamaModelDiscoveryState::Idle,
            &[],
            0,
        )
        .into_iter()
        .map(|line| line.to_string())
        .collect::<String>();
        assert!(running.contains("Provider test log"));
        assert!(running.contains("Testing Ollama (local)"));
        assert!(running.contains("Wait for and validate the required JSON response"));

        let failed = inference_test_log_lines(&InferenceTestState::Failed {
            provider: ilium_inference::InferenceProviderKind::Ollama,
            error: "connection refused".to_string(),
            elapsed: std::time::Duration::from_millis(250),
        })
        .into_iter()
        .map(|line| line.to_string())
        .collect::<String>();
        assert!(failed.contains("test failed after 0.25s"));
        assert!(failed.contains("Provider error: connection refused"));
        assert!(failed.contains("No settings were changed"));
    }

    #[test]
    fn running_inference_test_is_visibly_boxed_in_the_rendered_settings_screen() {
        let mut app = App::new("test-session".to_string(), std::path::PathBuf::from("/tmp"));
        app.settings_select_inference_provider(ilium_inference::InferenceProviderKind::Ollama);
        app.request_inference_test();
        let state = SettingsState {
            tab: SettingsTab::Inference,
            selected_row: 0,
            scroll: 0,
        };
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &state))
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Provider test log"));
        assert!(rendered.contains("Testing Ollama (local)"));
        assert!(rendered.contains("Wait for and validate"));
        assert!(rendered.contains("┌"));
        assert!(rendered.contains("│"));
    }

    #[test]
    fn running_ollama_discovery_is_visibly_boxed_in_the_rendered_settings_screen() {
        let mut app = App::new("test-session".to_string(), std::path::PathBuf::from("/tmp"));
        app.settings_select_inference_provider(ilium_inference::InferenceProviderKind::Ollama);
        app.request_ollama_model_refresh();
        let state = SettingsState {
            tab: SettingsTab::Inference,
            selected_row: 1,
            scroll: 0,
        };
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &state))
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Ollama model discovery log"));
        assert!(rendered.contains("Discovering local models"));
        assert!(rendered.contains("Request GET"));
        assert!(rendered.contains("┌"));
        assert!(rendered.contains("│"));
    }

    #[test]
    fn appearance_content_hit_finds_each_row_and_the_correct_zone() {
        let area = Rect::new(0, 0, 60, 20);
        let control_start_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;

        assert_eq!(
            appearance_content_hit(area, 0, Position::new(control_start_x, 1)),
            Some((AppearanceRow::AutoResizeTree, -1))
        );
        assert_eq!(
            appearance_content_hit(
                area,
                0,
                Position::new(control_start_x + DECREMENT_ZONE_WIDTH, 1)
            ),
            Some((AppearanceRow::AutoResizeTree, 1))
        );
        assert_eq!(
            appearance_content_hit(area, 0, Position::new(control_start_x, 4)),
            Some((AppearanceRow::TreeWidth, -1))
        );
        // A click on the label (left of the control) is a no-op.
        assert_eq!(appearance_content_hit(area, 0, Position::new(2, 1)), None);
        // A click on the description line between rows is a no-op.
        assert_eq!(
            appearance_content_hit(area, 0, Position::new(control_start_x, 2)),
            None
        );
    }

    #[test]
    fn appearance_content_hit_accounts_for_scroll_offset() {
        let area = Rect::new(0, 0, 60, 20);
        let control_start_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
        // Scrolled down by 3 lines, row 1 on screen is TreeWidth's line
        // (originally at line 4).
        assert_eq!(
            appearance_content_hit(area, 3, Position::new(control_start_x, 1)),
            Some((AppearanceRow::TreeWidth, -1))
        );
    }

    #[test]
    fn appearance_lines_expose_agent_mode_and_both_icon_selectors() {
        let ui = UiSettings {
            tree_order: crate::config::TreeOrder::AgeAscending,
            agent_identifiers: crate::config::AgentIdentifierSettings {
                mode: crate::config::AgentIdentifierMode::Icon,
                claude_icon: crate::config::ClaudeAgentIcon::MagicWand,
                codex_icon: crate::config::CodexAgentIcon::Tools,
                antigravity_icon: crate::config::AntigravityAgentIcon::Orbit,
            },
            ..UiSettings::default()
        };

        let rendered = appearance_lines(&ui, 3)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Tree order"));
        assert!(rendered.contains("Age up (newest first)"));
        assert!(rendered.contains("Agent identifier"));
        assert!(rendered.contains("Selected icon"));
        assert!(rendered.contains("Claude icon"));
        assert!(rendered.contains("🪄 Magic wand"));
        assert!(rendered.contains("Codex icon"));
        assert!(rendered.contains("🛠️ Tools"));
    }

    #[test]
    fn keyboard_content_hit_finds_both_stepper_directions() {
        let area = Rect::new(0, 0, 60, 20);
        let control_start_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
        assert_eq!(
            keyboard_content_hit(area, 0, Position::new(control_start_x, 1)),
            Some(-1)
        );
        assert_eq!(
            keyboard_content_hit(
                area,
                0,
                Position::new(control_start_x + DECREMENT_ZONE_WIDTH, 1)
            ),
            Some(1)
        );
        assert_eq!(keyboard_content_hit(area, 0, Position::new(2, 1)), None);
    }

    #[test]
    fn kanban_board_tab_renders_and_hits_both_layout_steppers() {
        let settings = KanbanBoardSettings {
            card_preview_lines: 3,
            minimum_column_width: 20,
        };
        let rendered = kanban_board_lines(&settings, 0)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let area = Rect::new(0, 0, 70, 20);
        let control_start_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;

        assert!(rendered.contains("Card preview lines"));
        assert!(rendered.contains("‹ 3 ›"));
        assert!(rendered.contains("Minimum column width"));
        assert!(rendered.contains("‹ 20 ›"));
        assert_eq!(
            kanban_board_content_hit(area, 0, Position::new(control_start_x, 1)),
            Some((KanbanBoardRow::CardPreviewLines, -1))
        );
        assert_eq!(
            kanban_board_content_hit(area, 0, Position::new(control_start_x + 3, 1)),
            Some((KanbanBoardRow::CardPreviewLines, 1))
        );
        assert_eq!(
            kanban_board_content_hit(area, 0, Position::new(control_start_x + 3, 4)),
            Some((KanbanBoardRow::MinimumColumnWidth, 1))
        );
    }

    #[test]
    fn keyboard_preset_hit_testing_maps_the_two_rendered_rows() {
        let area = Rect::new(0, 0, 60, 20);
        assert_eq!(
            keyboard_preset_at(area, 0, Position::new(2, 5)),
            Some(ShortcutBase::A)
        );
        assert_eq!(
            keyboard_preset_at(area, 0, Position::new(2, 6)),
            Some(ShortcutBase::B)
        );
        assert_eq!(keyboard_preset_at(area, 0, Position::new(2, 4)), None);
    }

    #[test]
    fn keyboard_lines_show_presets_and_specific_current_advice() {
        let mut keyboard = KeyboardSettings::default();
        let recommended = keyboard_lines(&keyboard)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(recommended.contains("[1] Ctrl+A"));
        assert!(recommended.contains("[2] Ctrl+B"));
        assert!(recommended.contains("Recommended: Ctrl+A"));

        keyboard.shortcut_base = ShortcutBase::parse("c").unwrap();
        let warned = keyboard_lines(&keyboard)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(warned.contains("Warning: Ctrl+C"));
        assert!(warned.contains("interrupt/SIGINT"));
    }

    #[test]
    fn max_scroll_is_zero_when_content_already_fits() {
        let app = App::new(
            "settings-test".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        let area = Rect::new(0, 0, 60, 40);
        assert_eq!(max_scroll(SettingsTab::Appearance, &app, 0, area), 0);
    }

    #[test]
    fn max_scroll_is_positive_when_content_overflows_a_short_terminal() {
        let app = App::new(
            "settings-test".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        let area = Rect::new(0, 0, 60, 3);
        assert!(max_scroll(SettingsTab::Appearance, &app, 0, area) > 0);
    }

    fn sound_discovery_fixture() -> ilium_sound::SoundDiscovery {
        ilium_sound::SoundDiscovery {
            platform: "Linux",
            directories: vec![ilium_sound::SoundDirectory {
                path: std::path::PathBuf::from("/usr/share/sounds"),
                origin: "XDG sound themes".to_string(),
            }],
            sounds: vec![ilium_sound::SystemSound {
                path: std::path::PathBuf::from("/usr/share/sounds/complete.oga"),
                display_name: "freedesktop / stereo / complete".to_string(),
                collection: "XDG sound themes".to_string(),
            }],
            was_truncated: false,
        }
    }

    #[test]
    fn sound_lines_show_source_events_and_real_discovery_evidence() {
        let settings = ilium_sound::SoundSettings::default();
        let rendered = sound_lines(&settings, &sound_discovery_fixture(), 0)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("System beep"));
        assert!(rendered.contains("[x] Enabled"));
        assert!(rendered.contains("Agent needs approval"));
        assert!(rendered.contains("/usr/share/sounds"));
        assert!(rendered.contains("freedesktop / stereo / complete"));
    }

    #[test]
    fn sound_hit_testing_handles_controls_catalog_and_scroll() {
        let discovery = sound_discovery_fixture();
        let area = Rect::new(0, 0, 100, 40);
        let control_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
        assert_eq!(
            sound_content_hit(area, 0, Position::new(control_x, 1), &discovery),
            Some(SoundContentHit::Row {
                row: SoundRow::Source,
                direction: -1
            })
        );
        let first_sound_line = 1
            + SoundRow::ALL.len() as u16 * APPEARANCE_ROW_HEIGHT
            + 4
            + discovery.directories.len() as u16;
        assert_eq!(
            sound_content_hit(area, 0, Position::new(2, first_sound_line), &discovery),
            Some(SoundContentHit::File(0))
        );
        assert_eq!(
            sound_content_hit(area, 3, Position::new(control_x, 1), &discovery),
            Some(SoundContentHit::Row {
                row: SoundRow::File,
                direction: -1
            })
        );
    }

    #[test]
    fn close_button_hit_covers_the_right_aligned_label_only() {
        let header_area = Rect::new(0, 0, 40, 2);
        let label_width = CLOSE_LABEL.chars().count() as u16;

        // Inside the label, top row.
        assert!(close_button_hit(
            header_area,
            Position::new(header_area.right() - 1, 0)
        ));
        // Just left of the label -- still the title, not a hit.
        assert!(!close_button_hit(
            header_area,
            Position::new(header_area.right() - label_width - 1, 0)
        ));
        // Same column, but the hint line below the label -- not a hit.
        assert!(!close_button_hit(
            header_area,
            Position::new(header_area.right() - 1, 1)
        ));
    }

    #[test]
    fn close_button_hit_is_false_when_header_too_narrow_for_the_label() {
        let header_area = Rect::new(0, 0, 3, 2);
        assert!(!close_button_hit(header_area, Position::new(1, 0)));
    }
}
