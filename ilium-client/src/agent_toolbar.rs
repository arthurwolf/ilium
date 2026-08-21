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
    /// Index into `models_for(provider)`. Claude and Antigravity only --
    /// Codex's model buttons open `CodexModelTier` instead (see that
    /// variant's doc comment for why a flat one-shot command doesn't work
    /// for Codex).
    Model(u8),
    /// Flips `UiSettings::terminal_text_selection_enabled`. Client-side and
    /// universal, like `Close`/`Stop`/`CopyScreen` -- available regardless
    /// of provider since it governs mouse behavior, not agent commands.
    ToggleTextSelection,
    /// Opens the reasoning-strength submenu for `CODEX_MODEL_TIERS[index]`
    /// (Sol/Terra/Luna). Codex-only: unlike Claude's `/model <name>`, Codex's
    /// `/model` command has no inline argument form -- typing one is read as
    /// a chat prompt, not a command (confirmed against a live `codex`
    /// session) -- so picking a model is an interactive picker requiring
    /// this menu rather than a single sent command.
    CodexModelTier(u8),
    /// Sends the full staged keystroke sequence that selects
    /// `CODEX_MODEL_TIERS[tier_index]` at `codex_reasoning_levels(tier_index)[level_index]`.
    /// Only reachable by clicking an entry inside the `CodexModelTier` submenu.
    CodexReasoningLevel(u8, u8),
}

/// One selectable Codex model tier: Sol, Terra, or Luna. `model_digit` is
/// positional against Codex's `/model` root picker list (1=Sol, 2=Terra,
/// 3=Luna) -- if OpenAI ever reorders that list, the wrong model gets
/// selected silently, so keep this table in the exact order Codex renders.
pub struct CodexModelTier {
    pub glyph: &'static str,
    pub label: &'static str,
    model_digit: u8,
}

/// Sol/Terra/Luna in Codex's own `/model` picker order, each with the sun,
/// earth, and moon glyphs the top-level menu is built around.
pub const CODEX_MODEL_TIERS: [CodexModelTier; 3] = [
    CodexModelTier {
        glyph: "\u{2600}\u{fe0f}",
        label: "Sol",
        model_digit: b'1',
    },
    CodexModelTier {
        glyph: "\u{1f30d}",
        label: "Terra",
        model_digit: b'2',
    },
    CodexModelTier {
        glyph: "\u{1f319}",
        label: "Luna",
        model_digit: b'3',
    },
];

/// One reasoning-effort strength inside a Codex model tier's submenu.
/// `digits` are the keys pressed after the model digit, one PTY write per
/// digit (see `codex_model_keystroke_stages`'s doc comment for why each
/// digit is its own write): Low/Medium/High/Extra high are one digit inside
/// Codex's "Select Reasoning Level" screen; Max/Ultra are two, since Codex
/// nests them behind that screen's "More reasoning..." entry (its own
/// "Advanced Reasoning" screen).
pub struct CodexReasoningLevel {
    pub label: &'static str,
    digits: &'static [u8],
}

const CODEX_REASONING_LEVELS: [CodexReasoningLevel; 6] = [
    CodexReasoningLevel {
        label: "Low",
        digits: b"1",
    },
    CodexReasoningLevel {
        label: "Medium",
        digits: b"2",
    },
    CodexReasoningLevel {
        label: "High",
        digits: b"3",
    },
    CodexReasoningLevel {
        label: "Extra high",
        digits: b"4",
    },
    CodexReasoningLevel {
        label: "Max",
        digits: b"51",
    },
    CodexReasoningLevel {
        label: "Ultra",
        digits: b"52",
    },
];

/// The reasoning levels offered for `CODEX_MODEL_TIERS[tier_index]`. Every
/// tier but Luna offers all six; Luna's "Advanced Reasoning" screen has no
/// Ultra entry (confirmed live -- Codex only lists "1. Max" there for Luna),
/// so its submenu stops at five.
pub fn codex_reasoning_levels(tier_index: usize) -> &'static [CodexReasoningLevel] {
    const LUNA_INDEX: usize = 2;
    if tier_index == LUNA_INDEX {
        &CODEX_REASONING_LEVELS[..5]
    } else {
        &CODEX_REASONING_LEVELS
    }
}

/// Builds the full staged keystroke sequence that switches Codex to `tier`
/// at `level`. Each returned element is one PTY write; the caller must send
/// them as separate writes with a real gap between at least the first two,
/// never concatenated into one buffer.
///
/// Confirmed against a live `codex` session: typing `/model` opens its
/// slash-command autocomplete, and an Enter that arrives before that popup
/// has settled is consumed as "accept completion" instead of "submit" --
/// the whole `/model gpt-5.6-terra` line lands in the chat composer as text
/// (and gets sent to the agent as a prompt) rather than opening the picker.
/// Once the picker is actually open, digit-to-digit navigation between its
/// nested screens (model -> reasoning level -> advanced reasoning) is
/// synchronous and needs no gap.
pub fn codex_model_keystroke_stages(
    tier: &CodexModelTier,
    level: &CodexReasoningLevel,
) -> Vec<Vec<u8>> {
    let mut stages = vec![b"/model".to_vec(), b"\r".to_vec(), vec![tier.model_digit]];
    stages.extend(level.digits.iter().map(|&digit| vec![digit]));
    stages
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

/// The model buttons offered for `provider`, in display order. Empty for
/// Codex -- its model selection is the `CodexModelTier`/`CodexReasoningLevel`
/// submenu built from `CODEX_MODEL_TIERS` instead, since Codex's `/model`
/// command has no inline argument form a flat `ModelButton` could send.
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
        BuiltinAgentProvider::Codex => &[],
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
        | AgentToolbarAction::CycleEffort
        | AgentToolbarAction::ToggleTextSelection
        | AgentToolbarAction::CodexModelTier(_)
        | AgentToolbarAction::CodexReasoningLevel(_, _) => None,
    }
}

/// The full write sequence for one toolbar action: a single write for every
/// action `command_for` already covers (its command text plus a trailing
/// Enter, matching how a hand-typed submission reaches the PTY), or the
/// multi-write `codex_model_keystroke_stages` sequence for
/// `CodexReasoningLevel`, which `command_for` cannot express as one string.
/// `None` for an action with no PTY effect (`Close`, `Stop`,
/// `ToggleTextSelection`, `CycleEffort`, `CopyScreen`, `CodexModelTier`,
/// which only opens its submenu) or one `provider` doesn't support.
pub fn keystroke_stages_for(
    provider: BuiltinAgentProvider,
    action: AgentToolbarAction,
) -> Option<Vec<Vec<u8>>> {
    if let AgentToolbarAction::CodexReasoningLevel(tier_index, level_index) = action {
        if provider != BuiltinAgentProvider::Codex {
            return None;
        }
        let tier = CODEX_MODEL_TIERS.get(usize::from(tier_index))?;
        let level =
            codex_reasoning_levels(usize::from(tier_index)).get(usize::from(level_index))?;
        return Some(codex_model_keystroke_stages(tier, level));
    }
    command_for(provider, action).map(|command| {
        let mut bytes = command.as_bytes().to_vec();
        bytes.push(b'\r');
        vec![bytes]
    })
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
        AgentToolbarAction::CycleEffort
        | AgentToolbarAction::Model(_)
        | AgentToolbarAction::ToggleTextSelection
        | AgentToolbarAction::CodexModelTier(_)
        | AgentToolbarAction::CodexReasoningLevel(_, _) => "",
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
    selection_enabled: bool,
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
        AgentToolbarAction::ToggleTextSelection => format!(
            "Terminal text selection: {} -- click to toggle",
            if selection_enabled { "on" } else { "off" }
        ),
        AgentToolbarAction::CodexModelTier(index) => CODEX_MODEL_TIERS
            .get(usize::from(index))
            .map(|tier| format!("Choose a reasoning strength for {}", tier.label))
            .unwrap_or_default(),
        AgentToolbarAction::CodexReasoningLevel(tier_index, level_index) => CODEX_MODEL_TIERS
            .get(usize::from(tier_index))
            .zip(codex_reasoning_levels(usize::from(tier_index)).get(usize::from(level_index)))
            .map(|(tier, level)| format!("Switch model to {} ({})", tier.label, level.label))
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
    /// Mirrors `UiSettings::terminal_text_selection_enabled` -- drives both
    /// the `ToggleTextSelection` button's on/off state text and its tooltip.
    pub selection_enabled: bool,
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
        selection_enabled,
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
        Button {
            action: AgentToolbarAction::ToggleTextSelection,
            text: format!(
                "{}{}",
                icons.glyph(IconTarget::AgentToolbarSelection),
                if selection_enabled { "on" } else { "off" }
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
    if provider == BuiltinAgentProvider::Codex {
        // Sol/Terra/Luna each open their own reasoning-strength submenu
        // rather than sending a command directly -- see `CodexModelTier`'s
        // doc comment for why Codex's model buttons can't be flat like
        // Claude's or Antigravity's.
        for (index, tier) in CODEX_MODEL_TIERS.iter().enumerate() {
            buttons.push(Button {
                action: AgentToolbarAction::CodexModelTier(index as u8),
                text: format!("{}{}", tier.glyph, tier.label),
            });
        }
    } else {
        // Every displayed Claude-model glyph gets its own persisted role so
        // the strength progression never bypasses the user-configurable icon
        // registry. Other providers retain the shared model role.
        for (index, model) in models_for(provider).iter().enumerate() {
            let glyph = if provider == BuiltinAgentProvider::Claude {
                match index {
                    0 => icons.glyph(IconTarget::AgentToolbarClaudeHaiku),
                    1 => icons.glyph(IconTarget::AgentToolbarClaudeSonnet),
                    2 => icons.glyph(IconTarget::AgentToolbarClaudeOpus),
                    _ => icons.glyph(IconTarget::AgentToolbarClaudeFable),
                }
            } else {
                icons.glyph(IconTarget::AgentToolbarModel)
            };
            buttons.push(Button {
                action: AgentToolbarAction::Model(index as u8),
                text: format!("{glyph}{}", model.label),
            });
        }
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
    // silently overlapping it. The group is centered, so its right edge sits
    // at `(area.width + total_width) / 2`, not at `total_width` -- comparing
    // the raw width against the space left of Close would let a wide group's
    // centered tail land on top of Close. Test the actual centered placement,
    // and keep the same minimum gap before Close as between buttons.
    loop {
        if center.is_empty() {
            break;
        }
        let total_width = group_width(&center);
        let centered_start = area.width.saturating_sub(total_width) / 2;
        let centered_right_edge = centered_start.saturating_add(total_width);
        if centered_right_edge.saturating_add(BUTTON_GAP) <= close_right_edge {
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
        // Clamp to the toolbar row so a Close label wider than the whole
        // area cannot produce a rect that spills past `area`'s right edge.
        Rect::new(
            area.x + close_right_edge,
            area.y,
            close_width.min(area.width),
            1,
        ),
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

/// The exact screen rect drawn for `action`, if it's currently visible.
/// Shared by the Codex model submenu's opener with `action_at`'s reverse
/// lookup (both key off `button_rects`), so the submenu always anchors flush
/// against the button that opened it, even after buttons get dropped for a
/// narrow pane.
pub fn button_rect_for(
    area: Rect,
    ctx: ToolbarContext,
    action: AgentToolbarAction,
) -> Option<Rect> {
    button_rects(area, ctx)
        .into_iter()
        .find(|(button_action, ..)| *button_action == action)
        .map(|(_, rect, _)| rect)
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
    let tooltip = tooltip_for(
        hovered_action,
        ctx.provider,
        ctx.effort,
        ctx.selection_enabled,
    );
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
            selection_enabled: true,
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
            selection_enabled: true,
        }
    }

    #[test]
    fn universal_buttons_present_without_a_known_provider() {
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 80, 1);
        let texts = rendered_texts(area, None, &icons, EffortLevel::Auto, false);
        assert!(texts.len() >= 4); // Stop, CopyScreen, ToggleTextSelection, Close
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
    fn claude_models_use_their_individually_configurable_icons() {
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
        assert_eq!(model_texts, vec!["🐁Haiku", "🐈Sonnet", "🦁Opus", "⬤Fable"]);

        let mut configured_icons = icons.clone();
        configured_icons.set(IconTarget::AgentToolbarClaudeHaiku, "•".to_string());
        configured_icons.set(IconTarget::AgentToolbarClaudeSonnet, "◉".to_string());
        configured_icons.set(IconTarget::AgentToolbarClaudeOpus, "◆".to_string());
        configured_icons.set(IconTarget::AgentToolbarClaudeFable, "✦".to_string());
        let configured_texts: Vec<String> = button_rects(
            area,
            ctx(Some(provider), &configured_icons, EffortLevel::Auto, false),
        )
        .into_iter()
        .filter(|(action, ..)| matches!(action, AgentToolbarAction::Model(_)))
        .map(|(_, _, text)| text)
        .collect();
        assert_eq!(
            configured_texts,
            vec!["•Haiku", "◉Sonnet", "◆Opus", "✦Fable"]
        );
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
    fn codex_toolbar_exposes_sol_terra_luna_tiers_not_flat_models() {
        let provider = BuiltinAgentProvider::Codex;
        assert!(models_for(provider).is_empty());
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 200, 1);
        let rects = button_rects(area, ctx(Some(provider), &icons, EffortLevel::Auto, false));
        let tier_labels: Vec<&str> = rects
            .iter()
            .filter_map(|(action, _, text)| {
                matches!(action, AgentToolbarAction::CodexModelTier(_)).then_some(text.as_str())
            })
            .collect();
        assert_eq!(
            tier_labels,
            vec!["\u{2600}\u{fe0f}Sol", "\u{1f30d}Terra", "\u{1f319}Luna"]
        );
        assert!(!rects
            .iter()
            .any(|(action, ..)| matches!(action, AgentToolbarAction::Model(_))));
    }

    #[test]
    fn luna_reasoning_levels_omit_ultra() {
        let sol_levels: Vec<&str> = codex_reasoning_levels(0).iter().map(|l| l.label).collect();
        let terra_levels: Vec<&str> = codex_reasoning_levels(1).iter().map(|l| l.label).collect();
        let luna_levels: Vec<&str> = codex_reasoning_levels(2).iter().map(|l| l.label).collect();
        assert_eq!(
            sol_levels,
            vec!["Low", "Medium", "High", "Extra high", "Max", "Ultra"]
        );
        assert_eq!(sol_levels, terra_levels);
        assert_eq!(
            luna_levels,
            vec!["Low", "Medium", "High", "Extra high", "Max"]
        );
    }

    #[test]
    fn codex_keystroke_stages_open_picker_then_pick_model_then_level() {
        let terra = &CODEX_MODEL_TIERS[1];
        let high = &codex_reasoning_levels(1)[2];
        let stages = codex_model_keystroke_stages(terra, high);
        assert_eq!(
            stages,
            vec![b"/model".to_vec(), b"\r".to_vec(), vec![b'2'], vec![b'3']]
        );
    }

    #[test]
    fn codex_max_and_ultra_add_an_advanced_reasoning_stage() {
        let sol = &CODEX_MODEL_TIERS[0];
        let levels = codex_reasoning_levels(0);
        let max = levels.iter().find(|l| l.label == "Max").unwrap();
        let ultra = levels.iter().find(|l| l.label == "Ultra").unwrap();
        assert_eq!(
            codex_model_keystroke_stages(sol, max),
            vec![
                b"/model".to_vec(),
                b"\r".to_vec(),
                vec![b'1'],
                vec![b'5'],
                vec![b'1']
            ]
        );
        assert_eq!(
            codex_model_keystroke_stages(sol, ultra),
            vec![
                b"/model".to_vec(),
                b"\r".to_vec(),
                vec![b'1'],
                vec![b'5'],
                vec![b'2']
            ]
        );
    }

    #[test]
    fn keystroke_stages_for_rejects_codex_reasoning_level_on_other_providers() {
        assert!(keystroke_stages_for(
            BuiltinAgentProvider::Claude,
            AgentToolbarAction::CodexReasoningLevel(1, 2)
        )
        .is_none());
    }

    #[test]
    fn keystroke_stages_for_single_command_actions_matches_command_for_plus_enter() {
        let provider = BuiltinAgentProvider::Claude;
        let stages = keystroke_stages_for(provider, AgentToolbarAction::Clear).unwrap();
        assert_eq!(stages, vec![b"/clear\r".to_vec()]);
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
    fn selection_toggle_button_reflects_current_on_off_state() {
        let icons = IconSettings::default();
        let area = Rect::new(0, 0, 80, 1);
        let mut on_ctx = ctx(None, &icons, EffortLevel::Auto, false);
        on_ctx.selection_enabled = true;
        let mut off_ctx = ctx(None, &icons, EffortLevel::Auto, false);
        off_ctx.selection_enabled = false;

        let on_rects = button_rects(area, on_ctx);
        let off_rects = button_rects(area, off_ctx);
        let on_text = on_rects
            .iter()
            .find(|(action, ..)| *action == AgentToolbarAction::ToggleTextSelection)
            .map(|(_, _, text)| text.as_str());
        let off_text = off_rects
            .iter()
            .find(|(action, ..)| *action == AgentToolbarAction::ToggleTextSelection)
            .map(|(_, _, text)| text.as_str());
        let glyph = icons.glyph(IconTarget::AgentToolbarSelection);

        assert_eq!(on_text, Some(format!("{glyph}on")).as_deref());
        assert_eq!(off_text, Some(format!("{glyph}off")).as_deref());
        assert_ne!(on_text, off_text);
    }

    #[test]
    fn selection_toggle_tooltip_names_current_state() {
        let on = tooltip_for(
            AgentToolbarAction::ToggleTextSelection,
            None,
            EffortLevel::Auto,
            true,
        );
        let off = tooltip_for(
            AgentToolbarAction::ToggleTextSelection,
            None,
            EffortLevel::Auto,
            false,
        );
        assert!(on.contains("on"));
        assert!(off.contains("off"));
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
        let tooltip = tooltip_for(AgentToolbarAction::Stop, None, EffortLevel::Auto, true);
        assert!(
            !below_row_text.contains(tooltip.as_str()),
            "tooltip should fit beside the buttons, not fall through to below_row"
        );
    }
}
