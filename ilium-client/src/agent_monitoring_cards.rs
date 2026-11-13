//! The three display-mode cards at the top of Settings › Agent Monitoring
//! (Normal, Attention, Custom), built on the shared [`crate::selectable_cards`]
//! chrome. Each card shows its title, a one-sentence description and, when
//! there is room, the shared demo project rendered in that mode so the user
//! sees the difference before choosing.
//!
//! Geometry is virtual (line 0 is the first content line of the page) so the
//! same rectangles serve rendering, hover and click hit tests.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::agent_monitoring::AgentMonitoringMode;
use crate::app::App;
use crate::selectable_cards::{self, CardChrome, CardDensity};
use crate::settings_ui::truncate_icon_text;

/// Cells between two cards.
pub(crate) const CARD_GAP: u16 = 1;
/// Most lines a description may take inside a side-by-side card.
const MAX_DESCRIPTION_ROWS: usize = 4;

/// One line of the shared demo project: the project, a folder, or an agent
/// showing a demo situation (named by its catalogue name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DemoLine {
    Project,
    Folder {
        branch: &'static str,
        name: &'static str,
    },
    Agent {
        branch: &'static str,
        situation: &'static str,
    },
}

/// The demo project every card previews: one project, three agents, a
/// `workers/` folder holding two agents. Seven lines, tree-shaped, identical
/// for all three modes.
pub(crate) const DEMO_TREE: [DemoLine; 7] = [
    DemoLine::Project,
    DemoLine::Agent {
        branch: " ├ ",
        situation: "Working",
    },
    DemoLine::Agent {
        branch: " ├ ",
        situation: "Needs approval",
    },
    DemoLine::Agent {
        branch: " ├ ",
        situation: "Task done, unread",
    },
    DemoLine::Folder {
        branch: " └ ",
        name: "workers/",
    },
    DemoLine::Agent {
        branch: "   ├ ",
        situation: "Goal paused",
    },
    DemoLine::Agent {
        branch: "   └ ",
        situation: "Idle",
    },
];

/// The one-sentence promise of each mode, from the feature plan.
pub(crate) const fn description(mode: AgentMonitoringMode) -> &'static str {
    match mode {
        AgentMonitoringMode::Normal => {
            "Two icons per agent: what it is working toward and what it is doing now."
        }
        AgentMonitoringMode::Attention => {
            "One icon per agent: the single most important thing that needs you."
        }
        AgentMonitoringMode::Custom => {
            "One icon per agent, chosen by rules you write. Order decides."
        }
    }
}

/// The rendered cards and their hit rectangles in virtual page coordinates.
pub(crate) struct CardsBlock {
    pub lines: Vec<Line<'static>>,
    pub cards: Vec<(AgentMonitoringMode, Rect)>,
}

/// Which density the page uses for `content_width`.
pub(crate) fn density(content_width: u16) -> CardDensity {
    selectable_cards::card_density(content_width, AgentMonitoringMode::ALL.len(), CARD_GAP)
}

/// Lines of the demo tree shown at `density`; zero when the card has no room.
fn demo_rows(density: CardDensity) -> usize {
    match density {
        CardDensity::Full | CardDensity::ShortDemo => DEMO_TREE.len(),
        CardDensity::DescriptionOnly | CardDensity::Stacked => 0,
    }
}

/// Catalogue index of the demo situation called `name`, when it exists.
fn situation_index(name: &str) -> Option<usize> {
    crate::status_demo::situations()
        .iter()
        .position(|situation| situation.name == name)
}

fn chrome(
    app: &App,
    mode: AgentMonitoringMode,
    focused: bool,
    hovered: Option<AgentMonitoringMode>,
) -> CardChrome {
    let selected = app.ui_settings.agent_monitoring_mode == mode;
    CardChrome {
        selected,
        hovered: hovered == Some(mode),
        focused,
    }
}

/// Build the cards starting at virtual line `first_line`. `focused` is true
/// while the keyboard selection sits on the Display mode row, `hovered` is
/// the card under the pointer.
pub(crate) fn build(
    app: &App,
    content_width: u16,
    first_line: u16,
    focused: bool,
    hovered: Option<AgentMonitoringMode>,
) -> CardsBlock {
    let density = density(content_width);
    let modes = AgentMonitoringMode::ALL;
    if density == CardDensity::Stacked {
        return build_stacked(app, content_width, first_line, focused, hovered);
    }
    let probe = selectable_cards::layout(
        Rect::new(0, first_line, content_width, 1),
        modes.len(),
        CARD_GAP,
    );
    let card_width = probe.first().map_or(0, |card| card.width);
    let inner = usize::from(card_width.saturating_sub(2));
    let text_width = inner.saturating_sub(2).max(1);
    let descriptions: Vec<Vec<String>> = modes
        .iter()
        .map(|mode| {
            let mut rows = crate::padding_ui::wrap(description(*mode), text_width);
            rows.truncate(MAX_DESCRIPTION_ROWS);
            rows
        })
        .collect();
    let description_rows = descriptions.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let demo_rows = demo_rows(density);
    // top, title, description, (blank, demo tree), bottom
    let demo_block = if demo_rows > 0 { demo_rows + 1 } else { 0 };
    let height = 3 + description_rows + demo_block;
    let height_u16 = u16::try_from(height).unwrap_or(u16::MAX);
    let cards = selectable_cards::layout(
        Rect::new(0, first_line, content_width, height_u16),
        modes.len(),
        CARD_GAP,
    );
    let chromes: Vec<CardChrome> = modes
        .iter()
        .map(|mode| chrome(app, *mode, focused, hovered))
        .collect();
    let mut lines = Vec::with_capacity(height);
    for line_index in 0..height {
        let mut spans = Vec::new();
        for (card_index, mode) in modes.iter().enumerate() {
            if card_index > 0 {
                spans.push(Span::raw(" ".repeat(usize::from(CARD_GAP))));
            }
            let chrome = chromes[card_index];
            let body = chrome.body_style();
            let card_spans = if line_index == 0 {
                chrome.top_line(card_width)
            } else if line_index == height - 1 {
                chrome.bottom_line(card_width)
            } else if line_index == 1 {
                chrome.title_line(mode.label(), card_width)
            } else if line_index < 2 + description_rows {
                let text = descriptions[card_index]
                    .get(line_index - 2)
                    .map(|row| format!(" {row}"))
                    .unwrap_or_default();
                chrome.framed_line(
                    vec![Span::styled(truncate_icon_text(&text, inner), body)],
                    card_width,
                )
            } else if line_index == 2 + description_rows {
                chrome.framed_line(Vec::new(), card_width)
            } else {
                let tree_index = line_index - (3 + description_rows);
                match DEMO_TREE.get(tree_index).copied() {
                    Some(DemoLine::Project) => chrome.framed_line(
                        vec![Span::styled(
                            truncate_icon_text(" Demo project", inner),
                            Style::new().add_modifier(Modifier::DIM),
                        )],
                        card_width,
                    ),
                    Some(DemoLine::Folder { branch, name }) => chrome.framed_line(
                        vec![Span::styled(
                            truncate_icon_text(&format!("{branch}{name}"), inner),
                            body,
                        )],
                        card_width,
                    ),
                    Some(DemoLine::Agent { branch, situation }) => match situation_index(situation)
                    {
                        Some(index) => chrome.framed_line(
                            demo_row(app, *mode, index, situation, branch, inner, body),
                            card_width,
                        ),
                        // A renamed catalogue entry shows its name without an
                        // icon; the test below fails until DEMO_TREE follows it.
                        None => chrome.framed_line(
                            vec![Span::styled(
                                truncate_icon_text(&format!("{branch}{situation}"), inner),
                                body,
                            )],
                            card_width,
                        ),
                    },
                    None => chrome.framed_line(Vec::new(), card_width),
                }
            };
            spans.extend(card_spans);
        }
        lines.push(Line::from(spans));
    }
    CardsBlock {
        lines,
        cards: modes.into_iter().zip(cards).collect(),
    }
}

/// One demo row: the tree branch, the situation's icon(s) in `mode` and its
/// name, truncated to `inner` cells.
fn demo_row(
    app: &App,
    mode: AgentMonitoringMode,
    index: usize,
    name: &str,
    branch: &str,
    inner: usize,
    body: Style,
) -> Vec<Span<'static>> {
    let prefix = truncate_icon_text(branch, inner);
    let mut remaining = inner.saturating_sub(UnicodeWidthStr::width(prefix.as_str()));
    let mut spans = vec![Span::styled(prefix, body)];
    for icon in crate::status_demo::preview_spans(app, mode, index) {
        let glyph = truncate_icon_text(icon.content.as_ref(), remaining);
        remaining = remaining.saturating_sub(UnicodeWidthStr::width(glyph.as_str()));
        spans.push(Span::styled(glyph, icon.style));
    }
    let label = truncate_icon_text(&format!(" {name}"), remaining);
    spans.push(Span::styled(label, body));
    spans
}

fn build_stacked(
    app: &App,
    content_width: u16,
    first_line: u16,
    focused: bool,
    hovered: Option<AgentMonitoringMode>,
) -> CardsBlock {
    let modes = AgentMonitoringMode::ALL;
    let inner = usize::from(content_width.saturating_sub(2));
    let text_width = inner.saturating_sub(2).max(1);
    let mut lines = Vec::new();
    let mut cards = Vec::new();
    let mut y = first_line;
    for mode in modes {
        let chrome = chrome(app, mode, focused, hovered);
        let rows = crate::padding_ui::wrap(description(mode), text_width);
        let height = u16::try_from(3 + rows.len().max(1)).unwrap_or(u16::MAX);
        cards.push((mode, Rect::new(0, y, content_width, height)));
        lines.push(Line::from(chrome.top_line(content_width)));
        lines.push(Line::from(chrome.title_line(mode.label(), content_width)));
        if rows.is_empty() {
            lines.push(Line::from(chrome.framed_line(Vec::new(), content_width)));
        }
        for row in rows {
            lines.push(Line::from(chrome.framed_line(
                vec![Span::styled(
                    truncate_icon_text(&format!(" {row}"), inner),
                    chrome.body_style(),
                )],
                content_width,
            )));
        }
        lines.push(Line::from(chrome.bottom_line(content_width)));
        y = y.saturating_add(height);
    }
    CardsBlock { lines, cards }
}

#[cfg(test)]
mod tests {
    use super::{build, density, DemoLine, CARD_GAP, DEMO_TREE};
    use crate::agent_monitoring::AgentMonitoringMode;
    use crate::app::App;
    use crate::selectable_cards::CardDensity;
    use unicode_width::UnicodeWidthStr;

    fn text(line: &ratatui::text::Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.to_string())
            .collect()
    }

    #[test]
    fn density_tiers_match_the_settings_content_width_at_common_terminal_sizes() {
        // Terminal 80 columns leaves about 50 content cells; 120 about 92;
        // 200 about 170. Every tier keeps the cards legible.
        assert_eq!(density(170), CardDensity::Full);
        assert_eq!(density(92), CardDensity::Full);
        assert_eq!(density(50), CardDensity::DescriptionOnly);
        assert_eq!(density(60), CardDensity::ShortDemo);
        assert_eq!(density(40), CardDensity::Stacked);
    }

    #[test]
    fn side_by_side_cards_are_equal_width_rows_are_exactly_as_wide_as_the_cards() {
        let app = App::new("test".to_string(), std::env::temp_dir());
        for (width, expected_density, demo_rows) in [
            (170u16, CardDensity::Full, DEMO_TREE.len()),
            (92, CardDensity::Full, DEMO_TREE.len()),
            (60, CardDensity::ShortDemo, DEMO_TREE.len()),
            (50, CardDensity::DescriptionOnly, 0),
        ] {
            let block = build(&app, width, 4, false, None);
            assert_eq!(density(width), expected_density, "width {width}");
            assert_eq!(block.cards.len(), 3);
            let card_width = block.cards[0].1.width;
            assert!(block.cards.iter().all(|(_, card)| card.width == card_width));
            assert!(block.cards.iter().all(|(_, card)| card.y == 4));
            let expected_rows = usize::from(block.cards[0].1.height);
            assert_eq!(block.lines.len(), expected_rows, "width {width}");
            let total = usize::from(card_width) * 3 + usize::from(CARD_GAP) * 2;
            for line in &block.lines {
                assert_eq!(
                    UnicodeWidthStr::width(text(line).as_str()),
                    total,
                    "width {width}: {}",
                    text(line)
                );
            }
            let rendered: String = block.lines.iter().map(text).collect::<Vec<_>>().join("\n");
            assert_eq!(
                rendered.matches("Demo project").count(),
                if demo_rows > 0 { 3 } else { 0 },
                "width {width}"
            );
            assert_eq!(
                rendered.matches("Working").count(),
                if demo_rows > 0 { 3 } else { 0 }
            );
            assert!(rendered.contains("(●) Normal"));
            assert!(rendered.contains("( ) Custom"));
            assert!(rendered.contains("Two icons"), "width {width}: {rendered}");
        }
    }

    #[test]
    fn demo_tree_is_seven_lines_with_a_folder_and_known_situations() {
        // Project, three agents, folder, two agents in the folder.
        assert_eq!(DEMO_TREE.len(), 7);
        assert_eq!(
            DEMO_TREE
                .iter()
                .filter(|line| matches!(line, DemoLine::Folder { .. }))
                .count(),
            1
        );
        let names = crate::status_demo::names();
        for line in DEMO_TREE {
            if let DemoLine::Agent { situation, .. } = line {
                assert!(
                    names.contains(&situation),
                    "{situation:?} is not a demo situation: {names:?}"
                );
            }
        }
        let app = App::new("test".to_string(), std::env::temp_dir());
        let block = build(&app, 92, 0, false, None);
        let rendered: String = block.lines.iter().map(text).collect::<Vec<_>>().join("\n");
        assert!(rendered.contains("workers/"), "{rendered}");
        assert!(rendered.contains("Goal paused"), "{rendered}");
    }

    #[test]
    fn narrow_content_stacks_the_cards_with_their_descriptions() {
        let app = App::new("test".to_string(), std::env::temp_dir());
        let block = build(&app, 40, 2, true, Some(AgentMonitoringMode::Custom));
        assert_eq!(density(40), CardDensity::Stacked);
        assert_eq!(block.cards.len(), 3);
        assert!(block
            .cards
            .iter()
            .all(|(_, card)| card.width == 40 && card.x == 0));
        assert_eq!(block.cards[0].1.y, 2);
        assert_eq!(block.cards[1].1.y, block.cards[0].1.bottom());
        let total: u16 = block.cards.iter().map(|(_, card)| card.height).sum();
        assert_eq!(block.lines.len(), usize::from(total));
        let rendered: String = block.lines.iter().map(text).collect::<Vec<_>>().join("\n");
        assert!(rendered.contains("Order decides."));
        assert!(
            rendered.starts_with('┏'),
            "focused selected card uses the heavy frame"
        );
        for line in &block.lines {
            assert_eq!(UnicodeWidthStr::width(text(line).as_str()), 40);
        }
    }
}
