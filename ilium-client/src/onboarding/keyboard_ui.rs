//! A compact custom-keyboard overlay. Settings and collision-aware capture
//! remain owned by the application; this module only emits typed requests.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Clear, Paragraph, Wrap},
    Frame,
};

use crate::{
    config::KeyboardSettings,
    keymap::{self, Action, BindingKey, KeyBinding, KeymapPreset, ShortcutBase},
};

const PANEL: Color = Color::Rgb(24, 29, 40);
const INK: Color = Color::Rgb(224, 231, 244);
const MUTED: Color = Color::Rgb(144, 157, 179);
const ACCENT: Color = Color::Rgb(242, 188, 105);
const FIRST_BINDING: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardRequest {
    ApplyPreset(KeymapPreset),
    SetGeneralPrefix(ShortcutBase),
    SetNavigationPrefix(ShortcutBase),
    Rebind(Action),
    AssignKey(Action, BindingKey),
    Close,
}

#[derive(Debug, Default)]
pub struct KeyboardUiState {
    pub focus: usize,
    pub hovered: Option<usize>,
    /// Scroll is measured in complete bindings, not terminal rows.
    pub scroll: usize,
    pub pending_rebind: Option<Action>,
    visible_bindings: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardHit {
    Preset(KeymapPreset),
    GeneralPrefix(i32),
    NavigationPrefix(i32),
    Binding(usize),
    Close,
}

impl KeyboardHit {
    fn focus(self) -> usize {
        match self {
            Self::Preset(KeymapPreset::Tmux) => 0,
            Self::Preset(KeymapPreset::Screen) => 1,
            Self::GeneralPrefix(_) => 2,
            Self::NavigationPrefix(_) => 3,
            Self::Binding(index) => FIRST_BINDING + index,
            Self::Close => usize::MAX,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct HitArea {
    area: Rect,
    target: KeyboardHit,
}

#[derive(Debug, Default)]
pub struct KeyboardGeometry {
    hits: Vec<HitArea>,
    visible_bindings: usize,
    footer: Rect,
    row_height: u16,
}

impl KeyboardGeometry {
    pub fn hit(&self, position: Position) -> Option<KeyboardHit> {
        self.hits
            .iter()
            .rev()
            .find(|hit| hit.area.contains(position))
            .map(|hit| hit.target)
    }
}

/// Geometry is independent of rendering and contains UI indices rather than
/// snapshots of settings. Click resolution reads the current bindings/prefixes.
pub fn geometry(area: Rect, state: &KeyboardUiState) -> KeyboardGeometry {
    let row = |offset: u16| {
        Rect::new(
            area.x,
            area.y.saturating_add(offset).min(area.bottom()),
            area.width,
            u16::from(offset < area.height),
        )
    };
    let footer_height = area.height.saturating_sub(4).min(2);
    let list_height = area.height.saturating_sub(4 + footer_height);
    let row_height = if area.width < 70 && list_height >= 2 {
        2
    } else {
        1
    };
    let visible_bindings = (list_height / row_height).max(1) as usize;
    let mut result = KeyboardGeometry {
        hits: Vec::new(),
        visible_bindings,
        row_height,
        footer: Rect::new(
            area.x,
            area.bottom().saturating_sub(footer_height).max(area.y),
            area.width,
            footer_height,
        ),
    };
    result.hits.push(HitArea {
        area: Rect::new(
            area.right().saturating_sub(8).max(area.x),
            area.y,
            area.width.min(8),
            area.height.min(1),
        ),
        target: KeyboardHit::Close,
    });
    let presets = row(1);
    let half = presets.width / 2;
    result.hits.push(HitArea {
        area: Rect::new(presets.x, presets.y, half, presets.height),
        target: KeyboardHit::Preset(KeymapPreset::Tmux),
    });
    result.hits.push(HitArea {
        area: Rect::new(
            presets.x + half,
            presets.y,
            presets.width - half,
            presets.height,
        ),
        target: KeyboardHit::Preset(KeymapPreset::Screen),
    });
    for navigation in [false, true] {
        let prefix_area = row(if navigation { 3 } else { 2 });
        let target = |direction| {
            if navigation {
                KeyboardHit::NavigationPrefix(direction)
            } else {
                KeyboardHit::GeneralPrefix(direction)
            }
        };
        result.hits.push(HitArea {
            area: prefix_area,
            target: target(1),
        });
        result.hits.push(HitArea {
            area: Rect::new(
                prefix_area.x,
                prefix_area.y,
                prefix_area.width.min(2),
                prefix_area.height,
            ),
            target: target(-1),
        });
        result.hits.push(HitArea {
            area: Rect::new(
                prefix_area.right().saturating_sub(2).max(prefix_area.x),
                prefix_area.y,
                prefix_area.width.min(2),
                prefix_area.height,
            ),
            target: target(1),
        });
    }
    for index in 0..visible_bindings {
        let offset = 4 + index as u16 * row_height;
        let y = area.y.saturating_add(offset);
        let rect = Rect::new(
            area.x,
            y,
            area.width,
            row_height.min(
                area.bottom()
                    .saturating_sub(y.saturating_add(footer_height)),
            ),
        );
        if !rect.is_empty() {
            result.hits.push(HitArea {
                area: rect,
                target: KeyboardHit::Binding(state.scroll + index),
            });
        }
    }
    result
}

impl KeyboardUiState {
    /// Parent applies the result and re-renders from its authoritative settings.
    /// Release events never activate controls or begin another capture.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        keyboard: &KeyboardSettings,
        bindings: &[KeyBinding],
        area: Rect,
    ) -> Option<KeyboardRequest> {
        self.resize(area, bindings);
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if let Some(action) = self.pending_rebind {
            if key.code == KeyCode::Esc {
                self.pending_rebind = None;
                return None;
            }
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                return None;
            }
            return BindingKey::from_key_event(key)
                .map(|binding| KeyboardRequest::AssignKey(action, binding));
        }
        let close = FIRST_BINDING + bindings.len();
        self.focus = self.focus.min(close);
        match key.code {
            KeyCode::Esc => return Some(KeyboardRequest::Close),
            KeyCode::Tab | KeyCode::Down => self.focus = (self.focus + 1) % (close + 1),
            KeyCode::BackTab | KeyCode::Up => self.focus = (self.focus + close) % (close + 1),
            KeyCode::Home => self.focus = 0,
            KeyCode::End => self.focus = close.saturating_sub(1).max(FIRST_BINDING).min(close),
            KeyCode::PageDown => {
                self.focus = (self.focus.max(FIRST_BINDING) + self.visible_bindings.max(1))
                    .min(close.saturating_sub(1))
                    .min(close)
            }
            KeyCode::PageUp => {
                self.focus = self
                    .focus
                    .saturating_sub(self.visible_bindings.max(1))
                    .max(FIRST_BINDING)
                    .min(close)
            }
            KeyCode::Left | KeyCode::Right if matches!(self.focus, 2 | 3) => {
                let direction = if key.code == KeyCode::Left { -1 } else { 1 };
                return Some(self.prefix_request(keyboard, direction));
            }
            KeyCode::Char(letter)
                if matches!(self.focus, 2 | 3)
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let base = ShortcutBase::parse(&letter.to_string())?;
                return Some(if self.focus == 2 {
                    KeyboardRequest::SetGeneralPrefix(base)
                } else {
                    KeyboardRequest::SetNavigationPrefix(base)
                });
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                return match self.focus {
                    0 => Some(KeyboardRequest::ApplyPreset(KeymapPreset::Tmux)),
                    1 => Some(KeyboardRequest::ApplyPreset(KeymapPreset::Screen)),
                    2 | 3 => Some(self.prefix_request(keyboard, 1)),
                    focus if focus == close => Some(KeyboardRequest::Close),
                    focus => {
                        self.pending_rebind = bindings
                            .get(focus - FIRST_BINDING)
                            .map(|binding| binding.action);
                        self.pending_rebind.map(KeyboardRequest::Rebind)
                    }
                };
            }
            _ => {}
        }
        self.reveal(bindings.len());
        None
    }

    fn prefix_request(&self, keyboard: &KeyboardSettings, direction: i32) -> KeyboardRequest {
        if self.focus == 2 {
            KeyboardRequest::SetGeneralPrefix(keyboard.shortcut_base.stepped(direction))
        } else {
            KeyboardRequest::SetNavigationPrefix(
                keyboard.navigation_shortcut_base.stepped(direction),
            )
        }
    }

    fn reveal(&mut self, count: usize) {
        let visible = self.visible_bindings.max(1);
        self.scroll = self.scroll.min(count.saturating_sub(visible));
        if self.focus < FIRST_BINDING || self.focus >= FIRST_BINDING + count {
            return;
        }
        let index = self.focus - FIRST_BINDING;
        if index < self.scroll {
            self.scroll = index;
        }
        if index >= self.scroll + visible {
            self.scroll = index + 1 - visible;
        }
    }

    pub fn click(
        &mut self,
        geometry: &KeyboardGeometry,
        position: Position,
        keyboard: &KeyboardSettings,
        bindings: &[KeyBinding],
    ) -> Option<KeyboardRequest> {
        let target = geometry.hit(position)?;
        let request = match target {
            KeyboardHit::Preset(preset) => KeyboardRequest::ApplyPreset(preset),
            KeyboardHit::GeneralPrefix(direction) => {
                KeyboardRequest::SetGeneralPrefix(keyboard.shortcut_base.stepped(direction))
            }
            KeyboardHit::NavigationPrefix(direction) => KeyboardRequest::SetNavigationPrefix(
                keyboard.navigation_shortcut_base.stepped(direction),
            ),
            KeyboardHit::Binding(index) => KeyboardRequest::Rebind(bindings.get(index)?.action),
            KeyboardHit::Close => KeyboardRequest::Close,
        };
        self.focus = if target == KeyboardHit::Close {
            FIRST_BINDING + bindings.len()
        } else {
            target.focus()
        };
        self.visible_bindings = geometry.visible_bindings;
        self.reveal(bindings.len());
        self.pending_rebind = if let KeyboardRequest::Rebind(action) = request {
            Some(action)
        } else {
            None
        };
        Some(request)
    }

    /// Call from the resize/input event path, never from paint. Domain choices,
    /// capture and focused action are preserved while its row is revealed.
    pub fn resize(&mut self, area: Rect, bindings: &[KeyBinding]) {
        self.visible_bindings = geometry(area, self).visible_bindings;
        self.focus = self.focus.min(FIRST_BINDING + bindings.len());
        self.reveal(bindings.len());
    }

    /// Call only after the parent accepts/persists the assignment. On a
    /// collision leave capture pending, display its feedback and allow retry.
    pub fn finish_rebind(&mut self) {
        self.pending_rebind = None;
    }

    pub fn hover(&mut self, geometry: &KeyboardGeometry, position: Position) -> bool {
        let hovered = geometry.hit(position).map(KeyboardHit::focus);
        let changed = self.hovered != hovered;
        self.hovered = hovered;
        changed
    }

    /// Pointer-wheel scrolling preserves a visible, reachable binding focus.
    pub fn scroll_by(&mut self, delta: i32, bindings: &[KeyBinding]) {
        if bindings.is_empty() {
            return;
        }
        let current = self
            .focus
            .saturating_sub(FIRST_BINDING)
            .min(bindings.len() - 1);
        let next = (current as i64 + i64::from(delta))
            .clamp(0, bindings.len().saturating_sub(1) as i64) as usize;
        self.focus = FIRST_BINDING + next;
        self.reveal(bindings.len());
    }
}

/// `feedback` is the parent's actual capture/save/collision result. The editor
/// never claims that a requested preset, prefix or binding has been persisted.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &KeyboardUiState,
    keyboard: &KeyboardSettings,
    bindings: &[KeyBinding],
    feedback: Option<&str>,
) -> KeyboardGeometry {
    let geometry = geometry(area, state);
    if area.is_empty() {
        return geometry;
    }
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new("").style(Style::default().bg(PANEL)), area);
    frame.render_widget(
        Paragraph::new("Custom keyboard").style(
            Style::default()
                .fg(ACCENT)
                .bg(PANEL)
                .add_modifier(Modifier::BOLD),
        ),
        Rect::new(area.x, area.y, area.width, area.height.min(1)),
    );
    for hit in &geometry.hits {
        let focus = if hit.target == KeyboardHit::Close {
            FIRST_BINDING + bindings.len()
        } else {
            hit.target.focus()
        };
        let highlighted = state.focus == focus || state.hovered == Some(hit.target.focus());
        let text = match hit.target {
            KeyboardHit::Close => "[Close]".into(),
            KeyboardHit::Preset(preset) => format!("[{} preset]", preset.label()),
            KeyboardHit::GeneralPrefix(direction) | KeyboardHit::NavigationPrefix(direction) => {
                if hit.area.width <= 2 {
                    if direction < 0 {
                        "‹".into()
                    } else {
                        "›".into()
                    }
                } else {
                    let navigation = matches!(hit.target, KeyboardHit::NavigationPrefix(_));
                    format!(
                        "  {}: {}",
                        if navigation {
                            "Tree prefix"
                        } else {
                            "General prefix"
                        },
                        if navigation {
                            keyboard.navigation_shortcut_base
                        } else {
                            keyboard.shortcut_base
                        }
                        .label()
                    )
                }
            }
            KeyboardHit::Binding(index) => {
                let Some(binding) = bindings.get(index) else {
                    continue;
                };
                let prefix = keymap::action_prefix_label(
                    binding.action,
                    keyboard.shortcut_base,
                    keyboard.navigation_shortcut_base,
                );
                let shortcut = format!("{prefix} {}", keymap::key_label(binding.key));
                let label = keymap::action_label(binding.action);
                if geometry.row_height == 2 {
                    format!("{label}\n  {shortcut}  [rebind]")
                } else {
                    format!("{shortcut:18} {label}")
                }
            }
        };
        control(frame, hit.area, text, highlighted);
    }
    let detail = feedback.map(str::to_owned).unwrap_or_else(|| {
        if let Some(action) = state.pending_rebind {
            format!(
                "Rebind {}: press a key; Esc cancels",
                keymap::action_label(action)
            )
        } else if matches!(state.focus, 2 | 3) {
            let base = if state.focus == 2 {
                keyboard.shortcut_base
            } else {
                keyboard.navigation_shortcut_base
            };
            keymap::shortcut_base_advice(base).explanation.to_owned()
        } else {
            format!("{} bindings | Enter rebind | PgUp/PgDn", bindings.len())
        }
    });
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Tab/↑/↓ focus | ←/→ or A–Z prefix"),
            Line::from(detail),
        ])
        .style(Style::default().fg(MUTED).bg(PANEL)),
        geometry.footer,
    );
    geometry
}

fn control(frame: &mut Frame, area: Rect, text: String, highlighted: bool) {
    if area.is_empty() {
        return;
    }
    let style = if highlighted {
        Style::default().fg(PANEL).bg(ACCENT)
    } else {
        Style::default().fg(INK).bg(PANEL)
    };
    frame.render_widget(
        Paragraph::new(text).style(style).wrap(Wrap { trim: false }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn narrow_overlay_reveals_and_can_rebind_every_actual_binding() {
        let keyboard = KeyboardSettings::default();
        let bindings = keymap::preset_bindings(KeymapPreset::Tmux);
        for (width, height) in [(38, 9), (40, 16), (80, 24), (120, 40), (200, 60)] {
            let mut state = KeyboardUiState {
                focus: FIRST_BINDING,
                ..Default::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for binding in &bindings {
                state.resize(Rect::new(0, 0, width, height), &bindings);
                terminal
                    .draw(|frame| {
                        render(frame, frame.area(), &state, &keyboard, &bindings, None);
                    })
                    .unwrap();
                assert!(
                    text(&terminal).contains(keymap::action_label(binding.action)),
                    "{} at {width}x{height}",
                    keymap::action_label(binding.action)
                );
                assert_eq!(
                    state.handle_key(
                        &key(KeyCode::Enter),
                        &keyboard,
                        &bindings,
                        Rect::new(0, 0, 38, 9)
                    ),
                    Some(KeyboardRequest::Rebind(binding.action))
                );
                assert_eq!(
                    state.handle_key(
                        &key(KeyCode::Char('q')),
                        &keyboard,
                        &bindings,
                        Rect::new(0, 0, 38, 9)
                    ),
                    Some(KeyboardRequest::AssignKey(
                        binding.action,
                        BindingKey::Character('q')
                    ))
                );
                state.finish_rebind();
                state.handle_key(
                    &key(KeyCode::Down),
                    &keyboard,
                    &bindings,
                    Rect::new(0, 0, 38, 9),
                );
            }
        }
    }

    #[test]
    fn requests_read_authoritative_prefix_and_presets_do_not_fake_saved_values() {
        let mut keyboard = KeyboardSettings::default();
        let bindings = keymap::preset_bindings(KeymapPreset::Screen);
        let mut state = KeyboardUiState {
            focus: 2,
            ..Default::default()
        };
        let next = keyboard.shortcut_base.stepped(1);
        assert_eq!(
            state.handle_key(
                &key(KeyCode::Right),
                &keyboard,
                &bindings,
                Rect::new(0, 0, 38, 9)
            ),
            Some(KeyboardRequest::SetGeneralPrefix(next))
        );
        assert_eq!(keyboard.shortcut_base, ShortcutBase::B);
        keyboard.shortcut_base = next;
        assert_eq!(
            state.handle_key(
                &key(KeyCode::Right),
                &keyboard,
                &bindings,
                Rect::new(0, 0, 38, 9)
            ),
            Some(KeyboardRequest::SetGeneralPrefix(next.stepped(1)))
        );
        state.focus = 3;
        assert_eq!(
            state.handle_key(
                &key(KeyCode::Char('z')),
                &keyboard,
                &bindings,
                Rect::new(0, 0, 38, 9)
            ),
            Some(KeyboardRequest::SetNavigationPrefix(
                ShortcutBase::parse("z").unwrap()
            ))
        );
        state.focus = 1;
        assert_eq!(
            state.handle_key(
                &key(KeyCode::Enter),
                &keyboard,
                &bindings,
                Rect::new(0, 0, 38, 9)
            ),
            Some(KeyboardRequest::ApplyPreset(KeymapPreset::Screen))
        );
    }

    #[test]
    fn hit_testing_matches_live_rows_and_release_does_not_capture() {
        let keyboard = KeyboardSettings::default();
        let bindings = keymap::preset_bindings(KeymapPreset::Tmux);
        let mut state = KeyboardUiState {
            focus: FIRST_BINDING + bindings.len() - 1,
            ..Default::default()
        };
        state.resize(Rect::new(0, 0, 40, 16), &bindings);
        let mut terminal = Terminal::new(TestBackend::new(40, 16)).unwrap();
        let mut geometry = KeyboardGeometry::default();
        terminal
            .draw(|frame| {
                geometry = render(
                    frame,
                    frame.area(),
                    &state,
                    &keyboard,
                    &bindings,
                    Some("Key already assigned; choose another"),
                );
            })
            .unwrap();
        let last = geometry
            .hits
            .iter()
            .find(|hit| hit.target == KeyboardHit::Binding(bindings.len() - 1))
            .unwrap();
        let position = Position::new(last.area.x + 1, last.area.y);
        assert_eq!(
            state.click(&geometry, position, &keyboard, &bindings),
            Some(KeyboardRequest::Rebind(bindings.last().unwrap().action))
        );
        assert_eq!(
            state.handle_key(
                &KeyEvent {
                    kind: KeyEventKind::Release,
                    ..key(KeyCode::Enter)
                },
                &keyboard,
                &bindings,
                Rect::new(0, 0, 38, 9)
            ),
            None
        );
        assert!(text(&terminal).contains("Key already assigned"));
        assert!(text(&terminal).contains("General prefix: Ctrl+B"));
        assert!(text(&terminal).contains("Tree prefix: Ctrl+B"));
        assert_eq!(
            geometry.hit(Position::new(0, 2)),
            Some(KeyboardHit::GeneralPrefix(-1))
        );
        assert_eq!(
            geometry.hit(Position::new(39, 2)),
            Some(KeyboardHit::GeneralPrefix(1))
        );
    }

    #[test]
    fn empty_bindings_and_tiny_geometry_do_not_underflow() {
        let keyboard = KeyboardSettings::default();
        let mut state = KeyboardUiState::default();
        for (width, height) in [(1, 1), (10, 4), (38, 9)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    render(frame, frame.area(), &state, &keyboard, &[], None);
                })
                .unwrap();
            state.handle_key(
                &key(KeyCode::PageDown),
                &keyboard,
                &[],
                Rect::new(0, 0, 38, 9),
            );
            state.handle_key(&key(KeyCode::End), &keyboard, &[], Rect::new(0, 0, 38, 9));
            assert_eq!(
                state.handle_key(&key(KeyCode::Enter), &keyboard, &[], Rect::new(0, 0, 38, 9)),
                Some(KeyboardRequest::Close)
            );
        }
    }

    #[test]
    fn capture_keeps_collision_retry_pending_and_escape_cancels_without_closing() {
        let keyboard = KeyboardSettings::default();
        let mut bindings = keymap::preset_bindings(KeymapPreset::Tmux);
        let mut state = KeyboardUiState {
            focus: FIRST_BINDING,
            ..Default::default()
        };
        let action = bindings[0].action;
        state.handle_key(
            &key(KeyCode::Enter),
            &keyboard,
            &bindings,
            Rect::new(0, 0, 38, 9),
        );
        let owned_key = bindings
            .iter()
            .find(|binding| binding.action != action)
            .unwrap()
            .key;
        let BindingKey::Character(character) = owned_key else {
            panic!("fixture is a printable preset key");
        };
        assert_eq!(
            state.handle_key(
                &key(KeyCode::Char(character)),
                &keyboard,
                &bindings,
                Rect::new(0, 0, 38, 9)
            ),
            Some(KeyboardRequest::AssignKey(action, owned_key))
        );
        assert!(keymap::assign_key(&mut bindings, action, owned_key).is_err());
        assert_eq!(state.pending_rebind, Some(action));
        assert_eq!(
            state.handle_key(
                &key(KeyCode::Enter),
                &keyboard,
                &bindings,
                Rect::new(0, 0, 38, 9)
            ),
            None
        );
        let free = BindingKey::Character(keymap::available_keys(&bindings)[0]);
        assert_eq!(
            state.handle_key(
                &key(KeyCode::PageDown),
                &keyboard,
                &bindings,
                Rect::new(0, 0, 38, 9)
            ),
            Some(KeyboardRequest::AssignKey(action, BindingKey::PageDown))
        );
        keymap::assign_key(&mut bindings, action, free).unwrap();
        state.finish_rebind();
        assert_eq!(state.pending_rebind, None);
        state.handle_key(
            &key(KeyCode::Enter),
            &keyboard,
            &bindings,
            Rect::new(0, 0, 38, 9),
        );
        assert_eq!(
            state.handle_key(
                &key(KeyCode::Esc),
                &keyboard,
                &bindings,
                Rect::new(0, 0, 38, 9)
            ),
            None
        );
        assert_eq!(state.pending_rebind, None);
        assert_eq!(
            state.handle_key(
                &key(KeyCode::Esc),
                &keyboard,
                &bindings,
                Rect::new(0, 0, 38, 9)
            ),
            Some(KeyboardRequest::Close)
        );
    }

    #[test]
    fn rendering_is_read_only_and_geometry_is_available_before_paint() {
        let keyboard = KeyboardSettings::default();
        let bindings = keymap::preset_bindings(KeymapPreset::Tmux);
        let mut state = KeyboardUiState::default();
        for (width, height) in [(200, 60), (38, 9), (120, 40)] {
            let area = Rect::new(0, 0, width, height);
            state.handle_key(&key(KeyCode::End), &keyboard, &bindings, area);
            let before = (
                state.focus,
                state.scroll,
                state.pending_rebind,
                state.hovered,
            );
            let hit_geometry = geometry(area, &state);
            let last = hit_geometry
                .hits
                .iter()
                .find(|hit| hit.target == KeyboardHit::Binding(bindings.len() - 1))
                .unwrap();
            let position = Position::new(last.area.x, last.area.y);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    let painted = render(frame, area, &state, &keyboard, &bindings, None);
                    assert_eq!(painted.hit(position), hit_geometry.hit(position));
                })
                .unwrap();
            assert_eq!(
                (
                    state.focus,
                    state.scroll,
                    state.pending_rebind,
                    state.hovered
                ),
                before
            );
            assert!(text(&terminal).contains(keymap::action_label(bindings.last().unwrap().action)));
        }
    }
}
