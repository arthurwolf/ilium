//! Advisory visibility checks; terminal default colors are estimates.

use crate::background_animation::AnimationSurface;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct VisibilityCheck {
    checked_at: Option<Instant>,
    warning: bool,
}

impl VisibilityCheck {
    pub(crate) fn warning(&self) -> bool {
        self.warning
    }

    pub(crate) fn update(
        &mut self,
        surface: &AnimationSurface,
        buffer: &Buffer,
        foreground: (u8, u8, u8),
        light: bool,
        black_backdrop: bool,
    ) {
        let now = Instant::now();
        if self
            .checked_at
            .is_some_and(|last| now.duration_since(last) < Duration::from_secs(1))
        {
            return;
        }
        if surface.width() == 0 || surface.height() == 0 || surface.is_wikipedia() {
            self.warning = false;
            return;
        }
        self.checked_at = Some(now);
        // Spread a bounded sample over the field rather than scanning every frame.
        let total = usize::from(surface.width()) * usize::from(surface.height());
        let stride = total.div_ceil(2048).max(1);
        let mut sampled = 0usize;
        let mut ink = 0usize;
        let mut faint = 0usize;
        for index in (0..total).step_by(stride) {
            let x = (index % usize::from(surface.width())) as u16;
            let y = (index / usize::from(surface.width())) as u16;
            sampled += 1;
            let glyph = surface
                .native_glyph(x, y)
                .unwrap_or_else(|| surface.glyph(x, y));
            if glyph.is_whitespace() || glyph == '\u{2800}' {
                continue;
            }
            ink += 1;
            let color = surface.cell_color(x, y).unwrap_or(foreground);
            let background = if black_backdrop {
                (0, 0, 0)
            } else {
                buffer
                    .cell((buffer.area.x + x, buffer.area.y + y))
                    .and_then(|cell| rgb(cell.bg))
                    .unwrap_or(if light { (245, 245, 245) } else { (26, 26, 26) })
            };
            let a = luminance(color);
            let b = luminance(background);
            if (a.max(b) + 0.05) / (a.min(b) + 0.05) < 1.2 {
                faint += 1;
            }
        }
        // Some scenes deliberately have sparse/dark phases: this is only a hint.
        self.warning = sampled >= 128 && (ink * 200 < sampled || (ink > 0 && faint * 5 >= ink * 4));
    }
}

fn luminance((r, g, b): (u8, u8, u8)) -> f32 {
    let linear = |value: u8| {
        let v = f32::from(value) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Black | Color::Indexed(0 | 16) => Some((0, 0, 0)),
        Color::White | Color::Indexed(15 | 231) => Some((255, 255, 255)),
        Color::Indexed(n @ 232..=255) => {
            Some((8 + 10 * (n - 232), 8 + 10 * (n - 232), 8 + 10 * (n - 232)))
        }
        Color::Indexed(n @ 16..=231) => {
            let n = n - 16;
            let levels = [0, 95, 135, 175, 215, 255];
            Some((
                levels[usize::from(n / 36)],
                levels[usize::from((n / 6) % 6)],
                levels[usize::from(n % 6)],
            ))
        }
        _ => None,
    }
}
