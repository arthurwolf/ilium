//! Colours of every drawn thing and the user's colour adjustments.
//!
//! The base colours are authored in the Neon scheme. The other schemes remap
//! their hues; brightness, contrast, hue rotation and saturation are applied
//! last, so the sliders work the same for every scheme.

use super::model::{MonsterKind, TowerKind};
use super::settings::{Palette as PaletteMode, Scheme, VectorTdSettings};
use super::sim::Tint;
use crate::style::ScenePalette;

/// What a stroke is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Grid,
    PathEdge,
    PathFill,
    Flow,
    Text,
    Tint(Tint),
}

/// Authored colours of the Neon scheme.
fn neon(role: Role) -> [u8; 3] {
    match role {
        Role::Grid => [34, 62, 96],
        Role::PathEdge => [58, 128, 214],
        Role::PathFill => [16, 44, 86],
        Role::Flow => [120, 190, 255],
        Role::Text => [214, 238, 255],
        Role::Tint(Tint::Money) => [255, 224, 74],
        Role::Tint(Tint::Danger) => [255, 52, 52],
        Role::Tint(Tint::Frost) => [140, 224, 255],
        Role::Tint(Tint::Tower(kind)) => match kind {
            TowerKind::Pulse => [93, 255, 138],
            TowerKind::Needle => [255, 232, 80],
            TowerKind::Chill => [127, 216, 255],
            TowerKind::Nova => [255, 150, 60],
            TowerKind::Arc => [176, 124, 255],
            TowerKind::Lancer => [255, 79, 122],
            TowerKind::Hive => [255, 95, 216],
            TowerKind::Beacon => [255, 242, 176],
        },
        Role::Tint(Tint::Monster(kind)) => match kind {
            MonsterKind::Drone => [255, 77, 94],
            MonsterKind::Dart => [255, 122, 47],
            MonsterKind::Shell => [214, 58, 122],
            MonsterKind::Splitter => [255, 176, 46],
            MonsterKind::Swarm => [255, 111, 145],
            MonsterKind::Wisp => [255, 214, 255],
            MonsterKind::Boss => [255, 46, 46],
            MonsterKind::Shard => [255, 193, 90],
        },
    }
}

const ROLES: usize = 5 + 3 + 8 + 8;

fn role_index(role: Role) -> usize {
    match role {
        Role::Grid => 0,
        Role::PathEdge => 1,
        Role::PathFill => 2,
        Role::Flow => 3,
        Role::Text => 4,
        Role::Tint(Tint::Money) => 5,
        Role::Tint(Tint::Danger) => 6,
        Role::Tint(Tint::Frost) => 7,
        Role::Tint(Tint::Tower(kind)) => 8 + kind.index(),
        Role::Tint(Tint::Monster(kind)) => {
            16 + match kind {
                MonsterKind::Drone => 0,
                MonsterKind::Dart => 1,
                MonsterKind::Shell => 2,
                MonsterKind::Splitter => 3,
                MonsterKind::Swarm => 4,
                MonsterKind::Wisp => 5,
                MonsterKind::Boss => 6,
                MonsterKind::Shard => 7,
            }
        }
    }
}

fn all_roles() -> Vec<Role> {
    let mut roles = vec![
        Role::Grid,
        Role::PathEdge,
        Role::PathFill,
        Role::Flow,
        Role::Text,
        Role::Tint(Tint::Money),
        Role::Tint(Tint::Danger),
        Role::Tint(Tint::Frost),
    ];
    roles.extend(
        TowerKind::ALL
            .iter()
            .map(|kind| Role::Tint(Tint::Tower(*kind))),
    );
    roles.extend(
        [
            MonsterKind::Drone,
            MonsterKind::Dart,
            MonsterKind::Shell,
            MonsterKind::Splitter,
            MonsterKind::Swarm,
            MonsterKind::Wisp,
            MonsterKind::Boss,
            MonsterKind::Shard,
        ]
        .map(|kind| Role::Tint(Tint::Monster(kind))),
    );
    roles
}

fn to_hsv([r, g, b]: [u8; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta < 1e-6 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let saturation = if max < 1e-6 { 0.0 } else { delta / max };
    (hue, saturation, max)
}

fn from_hsv(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let hue = hue.rem_euclid(360.0);
    let chroma = value * saturation;
    let sector = hue / 60.0;
    let x = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let offset = value - chroma;
    [
        ((r + offset).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((g + offset).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((b + offset).clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

/// Remap one authored colour into `scheme`.
fn scheme_color(scheme: Scheme, role: Role, color: [u8; 3]) -> [u8; 3] {
    let (hue, saturation, value) = to_hsv(color);
    let monster = matches!(
        role,
        Role::Tint(Tint::Monster(_)) | Role::Tint(Tint::Danger)
    );
    match scheme {
        Scheme::Neon => color,
        // Everything cold, monsters pushed to violet and magenta.
        Scheme::Cool => {
            let (base, span) = if monster {
                (270.0, 70.0)
            } else {
                (150.0, 100.0)
            };
            from_hsv(base + hue / 360.0 * span, saturation, value)
        }
        // Everything hot, towers from yellow to orange, monsters deep red.
        Scheme::Warm => {
            let (base, span) = if monster { (340.0, 30.0) } else { (10.0, 60.0) };
            from_hsv(base + hue / 360.0 * span, saturation, value)
        }
        Scheme::Phosphor => from_hsv(125.0, saturation * 0.55 + 0.1, value),
    }
}

/// Colours of every role after the scheme and adjustments.
#[derive(Debug, Clone)]
pub struct Colors {
    colors: Vec<[u8; 3]>,
    /// Multiplier on tones: brightness and contrast in black and white.
    pub brightness: f32,
    pub contrast: f32,
    pub is_color: bool,
}

impl Colors {
    /// `palette`, when provided, maps every role colour onto it by lightness
    /// after the scheme and adjustments.
    pub fn new(settings: &VectorTdSettings, palette: &ScenePalette) -> Self {
        let brightness = settings.brightness as f32 / 100.0;
        let contrast = settings.contrast as f32 / 100.0;
        let saturation = settings.saturation as f32 / 100.0;
        let hue = settings.hue as f32;
        let mut colors = vec![[255, 255, 255]; ROLES];
        for role in all_roles() {
            let base = scheme_color(settings.scheme, role, neon(role));
            let (h, s, v) = to_hsv(base);
            let adjusted = from_hsv(
                h + hue,
                (s * saturation).clamp(0.0, 1.0),
                (v * brightness).clamp(0.0, 1.0),
            );
            let contrasted = adjusted.map(|channel| {
                (((f32::from(channel) / 255.0 - 0.5) * contrast + 0.5).clamp(0.0, 1.0) * 255.0)
                    .round() as u8
            });
            colors[role_index(role)] = palette.recolor(contrasted);
        }
        Self {
            colors,
            brightness,
            contrast,
            is_color: settings.palette == PaletteMode::Colour,
        }
    }

    pub fn color(&self, role: Role) -> [u8; 3] {
        self.colors[role_index(role)]
    }

    /// Tone for the dither: contrast and brightness applied to `tone`.
    pub fn tone(&self, tone: f32) -> f32 {
        (((tone - 0.5) * self.contrast + 0.5) * self.brightness.min(1.0)).clamp(0.0, 1.0)
    }
}
