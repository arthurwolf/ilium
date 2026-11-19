//! Tone-only procedural scenes; the host applies the shared look and palette.

use ilium_plugin_sdk::SceneField;
use std::f64::consts::TAU;

/// A twelve-second seamless field of moving waves.
pub(crate) fn waves(field: &mut SceneField, time_millis: u64) {
    let width = field.width();
    let height = field.height();
    let phase = (time_millis % 12_000) as f64 * TAU / 12_000.0;
    field.glyphs.clear();
    field.text_glyphs.clear();
    for y in 0..height {
        for x in 0..width {
            let primary = (x as f64 * 0.20 + y as f64 * 0.10 - phase).sin();
            let cross = (x as f64 * 0.07 - y as f64 * 0.14 + phase).sin();
            field.pixels[y * width + x] = ((primary * 0.7 + cross * 0.3 + 1.0) * 127.5) as u8;
        }
    }
}

/// A UTC analogue clock: the face and hands are pixels, the hour numbers are glyphs.
pub(crate) fn clock(field: &mut SceneField, now_unix_millis: u64) {
    field.clear();
    let (width, height) = (field.width(), field.height());
    if width == 0 || height == 0 {
        return;
    }
    let center_x = (width - 1) as f64 / 2.0;
    let center_y = (height - 1) as f64 / 2.0;
    let radius = (width.min(height) as f64 * 0.43).max(1.0);
    for y in 0..height {
        for x in 0..width {
            let distance = ((x as f64 - center_x).powi(2) + (y as f64 - center_y).powi(2)).sqrt();
            if (distance - radius).abs() <= 0.75 {
                field.pixels[y * width + x] = 140;
            }
        }
    }
    let seconds = (now_unix_millis % 60_000) as f64 / 1_000.0;
    let minutes = (now_unix_millis / 60_000 % 60) as f64 + seconds / 60.0;
    let hours = (now_unix_millis / 3_600_000 % 12) as f64 + minutes / 60.0;
    hand(field, center_x, center_y, radius * 0.52, hours * TAU / 12.0, 230);
    hand(field, center_x, center_y, radius * 0.76, minutes * TAU / 60.0, 200);
    hand(field, center_x, center_y, radius * 0.82, seconds * TAU / 60.0, 120);
    for hour in 1..=12 {
        let angle = hour as f64 * TAU / 12.0;
        let x = center_x + angle.sin() * radius * 0.82;
        let y = center_y - angle.cos() * radius * 0.82;
        let text = hour.to_string();
        let column = (x / 2.0).round() as i32 - (text.len() as i32 - 1) / 2;
        let row = (y / 4.0).round() as i32;
        for (offset, character) in text.chars().enumerate() {
            let column = column + offset as i32;
            if column >= 0 && row >= 0 && column < i32::from(field.columns) && row < i32::from(field.rows) {
                field.glyphs.push((column as u16, row as u16, character));
            }
        }
    }
}

fn hand(field: &mut SceneField, x: f64, y: f64, length: f64, angle: f64, tone: u8) {
    let width = field.width();
    let height = field.height();
    let steps = length.ceil() as usize;
    for step in 0..=steps {
        let distance = step as f64;
        let px = (x + angle.sin() * distance).round() as usize;
        let py = (y - angle.cos() * distance).round() as usize;
        if px < width && py < height {
            let pixel = &mut field.pixels[py * width + px];
            *pixel = (*pixel).max(tone);
        }
    }
}
