//! The optional action toolbar shown above a detected agent pane's content:
//! one reserved row of centered icon buttons (compact/clear/model/effort/
//! stop/etc), plus a right-anchored close button. Mirrors `editor_toolbar`'s
//! shape -- a pure `button_rects` shared by rendering and click hit-testing
//! so the two can never drift apart -- but centers its main group instead of
//! left-aligning it, and anchors only the close button to the right edge.
//!
//! Per-provider command text lives entirely in this module as data
//! (`command_for`, `models_for`), not as branches at each call site: adding
//! a new agent provider extends those tables, matching the registry pattern
//! `ilium-detect`'s `AgentSignature` already uses.

use ilium_core::BuiltinAgentProvider;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::icon_settings::{IconSettings, IconTarget};

/// Terminal cell width of `text`. Every configurable icon here is an emoji
/// or symbol that can occupy two cells; `str::chars().count()` undercounts
/// those and leaves `button_rects` advancing `x` by less than what actually
/// got drawn, so later buttons render on top of earlier ones instead of
/// after them.
fn cell_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text) as u16
}

/// One click target in the toolbar. Rendering and dispatch both key off
/// this; `command_for`/`models_for` translate it into the bytes actually
/// sent to the pane's PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentToolbarAction {
    /// Hides the toolbar (mirrors `AppearanceRow::AgentToolbar`'s toggle).
    Close,
    /// Sends a raw Escape byte -- works for every provider, not routed
    /// through `command_for`.
    Stop,
    /// Copies the pane's currently visible screen text to the clipboard.
    /// Entirely client-side -- nothing is sent to the agent.
    CopyScreen,
    CopyLastMessage,
    Compact,
    Clear,
    Config,
    Exit,
    Fast,
    CycleEffort,
    /// Index into `models_for(provider)`.
    Model(u8),
}

/// A cyclable, client-local "requested reasoning effort" indicator. Nothing
/// here observes the agent's actual effort -- this is optimistic
/// presentation plus the literal command sent when cycled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EffortLevel {
    #[default]
    Auto,
    Low,
    Medium,
    High,
    XHigh,
    Max,
    /// Not a real `/effort` argument: staged as literal typed text ("no
    /// Enter") so the user's next prompt carries the `ultracode` keyword,
    /// matching how that keyword is actually consumed (as a word inside a
    /// prompt), not as a slash command.
    Ultracode,
}

impl EffortLevel {
    const ALL: [Self; 7] = [
        Self::Auto,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::XHigh,
        Self::Max,
        Self::Ultracode,
    ];

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|level| *level == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// Short in-toolbar label, kept to a handful of cells so the button row
    /// stays compact next to every other icon.
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Low => "low",
            Self::Medium => "med",
            Self::High => "high",
            Self::XHigh => "xhi",
            Self::Max => "max",
            Self::Ultracode => "ultra",
        }
    }

    /// The exact `/effort <word>` argument. Unused for `Ultracode`, which
    /// never becomes a slash command -- see the variant's doc comment.
    pub const fn command_word(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
            Self::Ultracode => "",
        }
    }
}

/// One selectable model button: display label plus the exact command the
/// provider expects. Best-effort for Codex/Antigravity -- their model
/// argument syntax is not as firmly documented as Claude Code's -- but a
/// wrong string here is a one-line data fix, never a code change.
pub struct ModelButton {
    pub label: &'static str,
    pub command: &'static str,
}

/// The model buttons offered for `provider`, in display order.
pub fn models_for(provider: BuiltinAgentProvider) -> &'static [ModelButton] {
    match provider {
        BuiltinAgentProvider::Claude => &[
            ModelButton {
                label: "Haiku",
                command: "/model haiku",
            },
            ModelButton {
                label: "Sonnet",
                command: "/model sonnet",
            },
            ModelButton {
                label: "Opus",
                command: "/model opus",
            },
            ModelButton {
                label: "Fable",
                command: "/model fable",
            },
        ],
        BuiltinAgentProvider::Codex => &[
            ModelButton {
                label: "Codex",
                command: "/model gpt-5.1-codex",
            },
            ModelButton {
                label: "Codex Max",
                command: "/model gpt-5.1-codex-max",
            },
        ],
        BuiltinAgentProvider::Antigravity => &[
            ModelButton {
                label: "Gemini Pro",
                command: "/model gemini-3-pro",
            },
            ModelButton {
                label: "Gemini Flash",
                command: "/model gemini-3-flash",
            },
        ],
    }
}

/// The exact command text sent for a non-model, non-universal action, or
/// `None` when `provider` has no known equivalent (hides the button rather
/// than sending a command that certainly doesn't exist).
pub fn command_for(
    provider: BuiltinAgentProvider,
    action: AgentToolbarAction,
) -> Option<&'static str> {
    use BuiltinAgentProvider::{Antigravity, Claude, Codex};
    match action {
        AgentToolbarAction::Compact => match provider {
            Claude | Codex => Some("/compact"),
            Antigravity => None,
        },
        AgentToolbarAction::Clear => Some("/clear"),
        AgentToolbarAction::Config => Some("/config"),
        AgentToolbarAction::CopyLastMessage => Some("/copy"),
        AgentToolbarAction::Exit => match provider {
            Claude => Some("/exit"),
            Codex | Antigravity => Some("/quit"),
        },
        AgentToolbarAction::Fast => match provider {
            Claude => Some("/fast"),
            Codex | Antigravity => None,
        },
        AgentToolbarAction::Model(index) => models_for(provider)
            .get(usize::from(index))
            .map(|button| button.command),
        AgentToolbarAction::Close
        | AgentToolbarAction::Stop
        | AgentToolbarAction::CopyScreen
        | AgentToolbarAction::CycleEffort => None,
    }
}

/// Short label shown to the right of the icon when the toolbar's "show
/// labels" setting is on. Empty for actions whose button text already
/// carries a readable word (models, effort) -- appending here would
/// duplicate it.
pub const fn action_label(action: AgentToolbarAction) -> &'static str {
    match action {
        AgentToolbarAction::Close => "Close",
        AgentToolbarAction::Stop => "Stop",
        AgentToolbarAction::CopyScreen => "Screen",
        AgentToolbarAction::CopyLastMessage => "Copy",
        AgentToolbarAction::Compact => "Compact",
        AgentToolbarAction::Clear => "Clear",
        AgentToolbarAction::Config => "Config",
        AgentToolbarAction::Exit => "Exit",
        AgentToolbarAction::Fast => "Fast",
        AgentToolbarAction::CycleEffort | AgentToolbarAction::Model(_) => "",
    }
}

/// Appends `action`'s label after `glyph` when `show_labels` is set, mirroring
/// how model/effort buttons already carry their own text.
fn button_text(glyph: &str, action: AgentToolbarAction, show_labels: bool) -> String {
    let label = action_label(action);
    if show_labels && !label.is_empty() {
        format!("{glyph} {label}")
    } else {
        glyph.to_string()
    }
}

/// One-line hover description shown in the tooltip.
pub fn tooltip_for(
    action: AgentToolbarAction,
    provider: Option<BuiltinAgentProvider>,
    effort: EffortLevel,
) -> String {
    match action {
        AgentToolbarAction::Close => {
            "Close toolbar (re-open from the \u{2261} icon or right-click menu)".to_string()
        }
        AgentToolbarAction::Stop => "Send Escape (interrupt the agent)".to_string(),
        AgentToolbarAction::CopyScreen => "Copy the visible screen to the clipboard".to_string(),
        AgentToolbarAction::CopyLastMessage => {
            "Copy the agent's last message (sends /copy)".to_string()
        }
        AgentToolbarAction::Compact => "Compact the conversation (sends /compact)".to_string(),
        AgentToolbarAction::Clear => "Clear the conversation (sends /clear)".to_string(),
        AgentToolbarAction::Config => "Open configuration (sends /config)".to_string(),
        AgentToolbarAction::Exit => "Exit the agent".to_string(),
        AgentToolbarAction::Fast => "Toggle fast mode (sends /fast)".to_string(),
        AgentToolbarAction::CycleEffort => {
            format!(
                "Reasoning effort: {} -- click to cycle",
                effort.short_label()
            )
        }
        AgentToolbarAction::Model(index) => provider
            .and_then(|provider| models_for(provider).get(usize::from(index)))
            .map(|button| format!("Switch model to {}", button.label))
            .unwrap_or_default(),
    }
}

struct Button {
    action: AgentToolbarAction,
    text: String,
}

/// Bundles the toolbar's per-pane presentation inputs. `center_buttons`,
/// `button_rects`, `action_at`, and `render` all need the same four values
/// together; grouping them keeps each function's own argument count small
/// and stops a future addition from tipping any of them over clippy's
/// too-many-arguments lint.
#[derive(Clone, Copy)]
pub struct ToolbarContext<'a> {
    pub provider: Option<BuiltinAgentProvider>,
    pub icons: &'a IconSettings,
    pub effort: EffortLevel,
    pub show_labels: bool,
}

/// The centered button group: universal actions available for every pane
/// (Stop/CopyScreen), then provider-specific actions and models, only for
/// panes whose provider is known. `provider` is `None` both for an
/// undetected/custom (`AgentClass::Other`) agent and for a pane whose
/// detected agent has since exited -- in both cases only the
/// provider-independent buttons make sense to show.
fn center_buttons(ctx: ToolbarContext) -> Vec<Button> {
    let ToolbarContext {
        provider,
        icons,
        effort,
        show_labels,
    } = ctx;
    let mut buttons = vec![
        Button {
            action: AgentToolbarAction::Stop,
            text: button_text(
                icons.glyph(IconTarget::AgentToolbarStop),
                AgentToolbarAction::Stop,
                show_labels,
            ),
        },
        Button {
            action: AgentToolbarAction::CopyScreen,
            text: button_text(
                icons.glyph(IconTarget::AgentToolbarCopyScreen),
                AgentToolbarAction::CopyScreen,
                show_labels,
            ),
        },
    ];
    let Some(provider) = provider else {
        return buttons;
    };
    fn push_if_supported(
        buttons: &mut Vec<Button>,
        provider: BuiltinAgentProvider,
        action: AgentToolbarAction,
        glyph: &str,
        show_labels: bool,
    ) {
        if command_for(provider, action).is_some() {
            buttons.push(Button {
                action,
                text: button_text(glyph, action, show_labels),
            });
        }
    }
    push_if_supported(
        &mut buttons,
        provider,
        AgentToolbarAction::CopyLastMessage,
        icons.glyph(IconTarget::AgentToolbarCopyLastMessage),
        show_labels,
    );
    push_if_supported(
        &mut buttons,
        provider,
        AgentToolbarAction::Compact,
        icons.glyph(IconTarget::AgentToolbarCompact),
        show_labels,
    );
    push_if_supported(
        &mut buttons,
        provider,
        AgentToolbarAction::Clear,
        icons.glyph(IconTarget::AgentToolbarClear),
        show_labels,
    );
    push_if_supported(
        &mut buttons,
        provider,
        AgentToolbarAction::Config,
        icons.glyph(IconTarget::AgentToolbarConfig),
        show_labels,
    );
    // Claude's four models get a distinct size/power-progression glyph each
    // (small dot -> hollow -> filled -> large filled) rather than sharing one
    // generic model icon, since Haiku/Sonnet/Opus/Fable is itself a
    // progression and the shared puzzle-piece glyph didn't communicate that.
    // Other providers keep the single configurable `AgentToolbarModel` glyph.
    const CLAUDE_MODEL_ICONS: [&str; 4] = ["\u{b7}", "\u{25cb}", "\u{25cf}", "\u{2b24}"];
    let model_glyph = icons.glyph(IconTarget::AgentToolbarModel);
    for (index, model) in models_for(provider).iter().enumerate() {
        let glyph = if provider == BuiltinAgentProvider::Claude {
            CLAUDE_MODEL_ICONS
                .get(index)
                .copied()
                .unwrap_or(model_glyph)
        } else {
            model_glyph
        };
        buttons.push(Button {
            action: AgentToolbarAction::Model(index as u8),
            text: format!("{glyph}{}", model.label),
        });
    }
    if command_for(provider, AgentToolbarAction::Fast).is_some() {
        buttons.push(Button {
            action: AgentToolbarAction::Fast,
            text: button_text(
                icons.glyph(IconTarget::AgentToolbarFast),
                AgentToolbarAction::Fast,
                show_labels,
            ),
        });
    }
    // Effort cycling has no known equivalent outside Claude Code; gated the
    // same way as every other provider-specific button rather than a
    // special case.
    if provider == BuiltinAgentProvider::Claude {
        buttons.push(Button {
            action: AgentToolbarAction::CycleEffort,
            text: format!(
                "{}{}",
                icons.glyph(IconTarget::AgentToolbarEffort),
                effort.short_label()
            ),
        });
    }
    push_if_supported(
        &mut buttons,
        provider,
        AgentToolbarAction::Exit,
        icons.glyph(IconTarget::AgentToolbarExit),
        show_labels,
    );
    buttons
}

/// At least two blank columns between adjacent buttons, per the toolbar's
/// explicit design brief.
const BUTTON_GAP: u16 = 2;

/// Every button's exact screen rect, clipped to `area` -- shared by
/// `render` and `action_at` so a click always maps to what's actually
/// drawn. The center group is centered as a whole (not individually), and
/// the close button is anchored to the right edge; on a terminal too
/// narrow for both, center buttons are dropped from the right first rather
/// than overlapping Close, which must stay reachable.
fn button_rects(area: Rect, ctx: ToolbarContext) -> Vec<(AgentToolbarAction, Rect, String)> {
    let close_text = button_text(
        ctx.icons.glyph(IconTarget::AgentToolbarClose),
        AgentToolbarAction::Close,
        ctx.show_labels,
    );
    let close_width = cell_width(&close_text);
    let close_right_edge = area.width.saturating_sub(close_width);

    let mut center = center_buttons(ctx);
    // Drop from the end until the group fits beside Close, rather than
    // silently overlapping it.
    let available_for_center = close_right_edge.saturating_sub(1);
    loop {
        let total_width = group_width(&center);
        if total_width <= available_for_center || center.is_empty() {
            break;
        }
        center.pop();
    }

    let total_width = group_width(&center);
    let start_x = area.x + (area.width.saturating_sub(total_width)) / 2;
    let mut rects = Vec::with_capacity(center.len() + 1);
    let mut x = start_x;
    for button in center {
        let width = cell_width(&button.text);
        rects.push((button.action, Rect::new(x, area.y, width, 1), button.text));
        x += width + BUTTON_GAP;
    }
    rects.push((
        AgentToolbarAction::Close,
        Rect::new(area.x + close_right_edge, area.y, close_width, 1),
        close_text,
    ));
    rects
}

fn group_width(buttons: &[Button]) -> u16 {
    if buttons.is_empty() {
        return 0;
    }
    let icons_width: u16 = buttons.iter().map(|button| cell_width(&button.text)).sum();
    icons_width + BUTTON_GAP * (buttons.len() as u16 - 1)
}

/// Returns the toolbar action at a terminal coordinate, if any.
pub fn action_at(
    area: Rect,
    ctx: ToolbarContext,
    position: Position,
) -> Option<AgentToolbarAction> {
    if !area.contains(position) {
        return None;
    }
    button_rects(area, ctx)
        .into_iter()
        .find(|(_, rect, _)| rect.contains(position))
        .map(|(action, ..)| action)
}

/// Draws the toolbar's icon row into `area` (one row, reserved above the
/// agent pane's content -- see `PaneViewport::with_agent_toolbar_reserved`).
/// When `hovered` names a button in this toolbar, also draws its tooltip:
/// inline after the last button when there's room in `area` itself, or as a
/// transient overlay on `below_row` (the live terminal content's first row)
/// otherwise -- painted after normal content, exactly like
/// `draw_screen_transfer_controls`, so it never reserves permanent space or
/// resizes the PTY just because the pointer moved.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    below_row: Rect,
    ctx: ToolbarContext,
    hovered: Option<AgentToolbarAction>,
) {
    let rects = button_rects(area, ctx);
    // Close is always anchored flush against `area`'s right edge (see
    // `button_rects`), so including it here would always push `rightmost` to
    // `area.right()`, leaving zero room and permanently disabling the
    // "beside the buttons" tooltip placement below in favor of the
    // `below_row` overlay. Measure the center group only, and separately
    // track where Close starts so the inline tooltip never overlaps it.
    let rightmost = rects
        .iter()
        .filter(|(action, ..)| *action != AgentToolbarAction::Close)
        .map(|(_, rect, _)| rect.right())
        .max()
        .unwrap_or(area.x);
    let close_left = rects
        .iter()
        .find(|(action, ..)| *action == AgentToolbarAction::Close)
        .map(|(_, rect, _)| rect.x)
        .unwrap_or(area.right());
    for (action, rect, text) in &rects {
        let style = if hovered == Some(*action) {
            Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::new()
        };
        frame.render_widget(Paragraph::new(Span::styled(text.as_str(), style)), *rect);
    }

    let Some(hovered_action) = hovered else {
        return;
    };
    let tooltip = tooltip_for(hovered_action, ctx.provider, ctx.effort);
    if tooltip.is_empty() {
        return;
    }
    let inline_start = rightmost + 2;
    let inline_available = close_left.saturating_sub(1).saturating_sub(inline_start);
    if inline_available >= cell_width(&tooltip) {
        let rect = Rect::new(inline_start, area.y, inline_available, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(
                tooltip,
                Style::new().add_modifier(Modifier::DIM),
            )),
            rect,
        );
        return;
    }
    if below_row.height == 0 {
        return;
    }
    let width = cell_width(&tooltip).min(below_row.width);
    let rect = Rect::new(below_row.x, below_row.y, width, 1);
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(Span::styled(
            tooltip,
            Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )),
        rect,
    );
}

/// Builds the toolbar's own line for rendering contexts that need the
/// finished `Line` rather than a direct frame write (kept for parity with
/// `theme::chrome_title`'s style; currently unused outside tests).
#[cfg(test)]
fn rendered_texts(
    area: Rect,
    provider: Option<BuiltinAgentProvider>,
    icons: &IconSettings,
    effort: EffortLevel,
    show_labels: bool,
) -> Vec<String> {
    button_rects(
        area,
        ToolbarContext {
            provider,
            icons,
            effort,
            show_labels,
        },
    )
    .into_iter()
    .map(|(_, _, text)| text)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Position;

    fn ctx(
        provider: Option<BuiltinAgentProvider>,
        icons: &IconSettings,
        effort: EffortLevel,
        show_labels: bool,
    ) -> ToolbarContext<'_> {
        ToolbarContext {
            provider,
            icons,
            effort,
            show_labels,
        }
    }

    #[test]
    fn universal_buttons_present_without_a_known_provider() {
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 80, 1);
        let texts = rendered_texts(area, None, &icons, EffortLevel::Auto, false);
        assert!(texts.len() >= 3); // Stop, CopyScreen, Close
        assert_eq!(
            action_at(
                area,
                ctx(None, &icons, EffortLevel::Auto, false),
                Position::new(area.right() - 1, 0)
            ),
            Some(AgentToolbarAction::Close)
        );
    }

    #[test]
    fn claude_exposes_four_models_and_effort_and_fast() {
        let provider = BuiltinAgentProvider::Claude;
        assert_eq!(models_for(provider).len(), 4);
        assert!(command_for(provider, AgentToolbarAction::Fast).is_some());
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 200, 1);
        let rects = button_rects(area, ctx(Some(provider), &icons, EffortLevel::Auto, false));
        assert!(rects
            .iter()
            .any(|(action, ..)| matches!(action, AgentToolbarAction::Model(_))));
        assert!(rects
            .iter()
            .any(|(action, ..)| *action == AgentToolbarAction::CycleEffort));
    }

    #[test]
    fn claude_models_get_distinct_progression_icons() {
        let provider = BuiltinAgentProvider::Claude;
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 200, 1);
        let rects = button_rects(area, ctx(Some(provider), &icons, EffortLevel::Auto, false));
        let model_texts: Vec<&str> = rects
            .iter()
            .filter(|(action, ..)| matches!(action, AgentToolbarAction::Model(_)))
            .map(|(_, _, text)| text.as_str())
            .collect();
        assert_eq!(model_texts.len(), 4);
        let unique: std::collections::HashSet<&str> = model_texts.iter().copied().collect();
        assert_eq!(unique.len(), 4, "each model button must render distinctly");
        // Doesn't fall back to the shared generic model glyph.
        assert!(model_texts
            .iter()
            .all(|text| !text.starts_with(icons.glyph(IconTarget::AgentToolbarModel))));
    }

    #[test]
    fn antigravity_hides_compact_and_fast() {
        let provider = BuiltinAgentProvider::Antigravity;
        assert!(command_for(provider, AgentToolbarAction::Compact).is_none());
        assert!(command_for(provider, AgentToolbarAction::Fast).is_none());
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 200, 1);
        let rects = button_rects(area, ctx(Some(provider), &icons, EffortLevel::Auto, false));
        assert!(!rects
            .iter()
            .any(|(action, ..)| *action == AgentToolbarAction::Compact));
        assert!(!rects
            .iter()
            .any(|(action, ..)| *action == AgentToolbarAction::Fast));
    }

    #[test]
    fn center_group_is_actually_centered() {
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 100, 1);
        let rects = button_rects(area, ctx(None, &icons, EffortLevel::Auto, false));
        let center: Vec<_> = rects
            .iter()
            .filter(|(action, ..)| *action != AgentToolbarAction::Close)
            .collect();
        let left_margin = center.first().unwrap().1.x - area.x;
        let right_margin = area.right() - center.last().unwrap().1.right();
        assert!(left_margin.abs_diff(right_margin) <= 1);
    }

    #[test]
    fn adjacent_buttons_keep_at_least_a_two_column_gap() {
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 100, 1);
        let rects = button_rects(
            area,
            ctx(
                Some(BuiltinAgentProvider::Claude),
                &icons,
                EffortLevel::Auto,
                false,
            ),
        );
        for window in rects.windows(2) {
            let (_, first, _) = &window[0];
            let (_, second, _) = &window[1];
            if second.x > first.right() {
                assert!(second.x - first.right() >= BUTTON_GAP);
            }
        }
    }

    #[test]
    fn narrow_toolbar_drops_center_buttons_before_hiding_close() {
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 6, 1);
        let rects = button_rects(
            area,
            ctx(
                Some(BuiltinAgentProvider::Claude),
                &icons,
                EffortLevel::Auto,
                false,
            ),
        );
        assert!(rects
            .iter()
            .any(|(action, ..)| *action == AgentToolbarAction::Close));
    }

    #[test]
    fn effort_cycles_through_every_level_and_wraps() {
        let mut level = EffortLevel::Auto;
        for _ in 0..EffortLevel::ALL.len() {
            level = level.next();
        }
        assert_eq!(level, EffortLevel::Auto);
    }

    #[test]
    fn click_outside_toolbar_row_misses() {
        let icons = IconSettings::default();
        let area = Rect::new(0, 5, 80, 1);
        assert_eq!(
            action_at(
                area,
                ctx(None, &icons, EffortLevel::Auto, false),
                Position::new(2, 0)
            ),
            None
        );
    }

    #[test]
    fn show_labels_appends_readable_text_after_icon_only_buttons() {
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 80, 1);
        let without_labels = rendered_texts(area, None, &icons, EffortLevel::Auto, false);
        let with_labels = rendered_texts(area, None, &icons, EffortLevel::Auto, true);
        assert!(with_labels.iter().any(|text| text.contains("Stop")));
        assert!(with_labels.iter().any(|text| text.contains("Close")));
        assert!(!without_labels.iter().any(|text| text.contains("Stop")));
        assert!(!without_labels.iter().any(|text| text.contains("Close")));
    }

    #[test]
    fn tooltip_fits_beside_the_buttons_when_area_has_room() {
        // Regression test: `rightmost` used to be measured across every
        // button including the right-anchored Close button, which always
        // sits at `area.right()` and made the inline placement branch
        // unreachable. With a wide area and a short tooltip, the tooltip
        // must land in the gap between the center group and Close, not on
        // `below_row`.
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 120, 1);
        let below_row = Rect::new(0, 1, 120, 1);
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 2))
            .expect("test backend");
        terminal
            .draw(|frame| {
                render(
                    frame,
                    area,
                    below_row,
                    ctx(None, &icons, EffortLevel::Auto, false),
                    Some(AgentToolbarAction::Stop),
                );
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let below_row_text: String = (0..buffer.area.width)
            .map(|x| buffer[(x, 1)].symbol())
            .collect();
        let tooltip = tooltip_for(AgentToolbarAction::Stop, None, EffortLevel::Auto);
        assert!(
            !below_row_text.contains(tooltip.as_str()),
            "tooltip should fit beside the buttons, not fall through to below_row"
        );
    }
}
