//! Slider track geometry shared by the Animations rows: which value lies under a track cell
//! and which cell shows a value.

/// The value under track cell `offset` of a `track_width`-cell slider. Both
/// track endpoints map exactly to `minimum` and `maximum`, including ranges
/// that are not a multiple of `step`.
pub fn slider_value_at(
    minimum: i32,
    maximum: i32,
    step: i32,
    offset: u16,
    track_width: u16,
) -> i32 {
    if track_width < 2 || maximum <= minimum {
        return minimum;
    }
    let offset = i64::from(offset.min(track_width - 1));
    let span = i64::from(maximum) - i64::from(minimum);
    let denominator = i64::from(track_width - 1);
    let raw = i64::from(minimum) + (span * offset + denominator / 2) / denominator;
    let step = i64::from(step.max(1));
    let rounded = i64::from(minimum) + ((raw - i64::from(minimum) + step / 2) / step) * step;
    rounded.min(i64::from(maximum)) as i32
}

/// The track cell that shows `value` on a `track_width`-cell slider.
pub fn slider_thumb_offset(minimum: i32, maximum: i32, value: i32, track_width: u16) -> u16 {
    let span = (i64::from(maximum) - i64::from(minimum)).max(1);
    let value = i64::from(value.clamp(minimum, maximum)) - i64::from(minimum);
    ((value * i64::from(track_width.saturating_sub(1)) + span / 2) / span) as u16
}
