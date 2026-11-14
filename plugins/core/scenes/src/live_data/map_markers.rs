//! Fleet marker geometry: one position list rasterised for one viewport. A scene runs inside a
//! render frame and owns no worker, so preparation is a plain function the fleet layer calls
//! when the data or the viewport changed.
use super::{map, maps::MapKind, model::Position};
use crate::raster::Raster;
use std::sync::Arc;

/// Everything a prepared raster depends on besides the positions themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MarkerKey {
    pub kind: MapKind,
    pub dot_width: usize,
    pub dot_height: usize,
    pub cell_width: usize,
    pub cell_height: usize,
    pub marker_brightness: u8,
    pub show_heading: bool,
}

#[derive(Debug, Clone)]
pub(super) struct MarkerRequest {
    pub key: MarkerKey,
    pub positions: Arc<Vec<Position>>,
}

#[derive(Debug)]
pub(super) struct PreparedMarkers {
    pub key: MarkerKey,
    pub raster: Raster,
    pub occupied_centers: Vec<bool>,
    pub accepted_positions: usize,
    pub unique_center_cells: usize,
}

fn valid_key(key: MarkerKey) -> bool {
    key.marker_brightness <= 100
        && key.kind != MapKind::Earthquakes
        && u16::try_from(key.cell_width).is_ok()
        && u16::try_from(key.cell_height).is_ok()
        && key
            .cell_width
            .checked_mul(key.cell_height)
            .is_some_and(|n| n <= 4_194_304)
        && key
            .dot_width
            .checked_mul(key.dot_height)
            .is_some_and(|n| n <= 16_777_216)
}

pub(super) fn prepare_positions(request: &MarkerRequest) -> Option<PreparedMarkers> {
    if !valid_key(request.key) {
        return None;
    }
    let key = request.key;
    let mut raster = Raster::default();
    raster
        .dots
        .try_reserve_exact(key.dot_width * key.dot_height)
        .ok()?;
    raster.resize(key.dot_width, key.dot_height);
    let mut occupied_centers = Vec::new();
    occupied_centers
        .try_reserve_exact(key.cell_width * key.cell_height)
        .ok()?;
    occupied_centers.resize(key.cell_width * key.cell_height, false);
    let mut prepared = PreparedMarkers {
        key,
        raster,
        occupied_centers,
        accepted_positions: 0,
        unique_center_cells: 0,
    };
    if key.cell_width == 0 || key.cell_height == 0 || key.dot_width == 0 || key.dot_height == 0 {
        return Some(prepared);
    }
    for position in request.positions.iter() {
        let Some((x, y, cell)) = marker_center(
            position,
            key.cell_width as u16,
            key.cell_height as u16,
            key.dot_width,
            key.dot_height,
        ) else {
            continue;
        };
        prepared.accepted_positions += 1;
        if !prepared.occupied_centers[cell] {
            prepared.occupied_centers[cell] = true;
            prepared.unique_center_cells += 1;
        }
        let intensity = f32::from(key.marker_brightness) / 100.0;
        if key.show_heading {
            draw_vehicle(
                &mut prepared.raster,
                (x, y),
                position.heading_degrees,
                intensity,
            );
        } else {
            // Exactly one raster dot per vehicle, never a soft multi-dot blob.
            let column = ((x * key.dot_width as f32) as usize).min(key.dot_width - 1);
            let row = ((y * key.dot_height as f32) as usize).min(key.dot_height - 1);
            prepared.raster.dots[row * key.dot_width + column] = intensity;
        }
    }
    Some(prepared)
}

pub(super) fn marker_center(
    position: &Position,
    width: u16,
    height: u16,
    dot_width: usize,
    dot_height: usize,
) -> Option<(f32, f32, usize)> {
    let (x, y) = map::project(position.longitude, position.latitude)?;
    // Clamp only to visible dot centers: poles and the dateline remain visible,
    // while the geographic projection itself retains exact boundaries.
    let x = x.clamp(0.5 / dot_width as f32, 1.0 - 0.5 / dot_width as f32);
    let y = y.clamp(0.5 / dot_height as f32, 1.0 - 0.5 / dot_height as f32);
    let column = ((x * f32::from(width)) as usize).min(usize::from(width) - 1);
    let row = ((y * f32::from(height)) as usize).min(usize::from(height) - 1);
    Some((x, y, row * usize::from(width) + column))
}

pub(super) fn draw_vehicle(
    raster: &mut Raster,
    center: (f32, f32),
    heading: Option<f64>,
    intensity: f32,
) {
    raster.line(center, center, 0.8, intensity);
    let Some(heading) = heading.filter(|value| value.is_finite() && (0.0..360.0).contains(value))
    else {
        return;
    };
    let angle = (heading as f32).to_radians();
    let forward = (angle.sin(), -angle.cos());
    let side = (angle.cos(), angle.sin());
    let point = |along: f32, across: f32| {
        (
            center.0 + (forward.0 * along + side.0 * across) / raster.width as f32,
            center.1 + (forward.1 * along + side.1 * across) / raster.height as f32,
        )
    };
    let tip = point(2.5, 0.0);
    let left = point(-1.0, -1.4);
    let right = point(-1.0, 1.4);
    raster.line(tip, left, 0.45, intensity);
    raster.line(left, right, 0.45, intensity);
    raster.line(right, tip, 0.45, intensity);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> MarkerRequest {
        MarkerRequest {
            key: MarkerKey {
                kind: MapKind::Boats,
                dot_width: 160,
                dot_height: 96,
                cell_width: 80,
                cell_height: 24,
                marker_brightness: 90,
                show_heading: true,
            },
            positions: Arc::new(vec![
                Position::new("fixture".into(), 0.0, 0.0, Some(0)).unwrap()
            ]),
        }
    }

    #[test]
    fn coincident_cells_keep_both_heading_strokes() {
        let mut north = request();
        Arc::make_mut(&mut north.positions)[0].heading_degrees = Some(0.0);
        let mut east = north.positions[0].clone();
        east.heading_degrees = Some(90.0);
        let single = prepare_positions(&north).unwrap();
        Arc::make_mut(&mut north.positions).push(east);
        let both = prepare_positions(&north).unwrap();
        assert_eq!(both.accepted_positions, 2);
        assert_eq!(both.unique_center_cells, 1);
        assert!(both
            .raster
            .dots
            .iter()
            .zip(single.raster.dots)
            .any(|(a, b)| *a > b));
    }

    #[test]
    fn empty_zero_tiny_and_invalid_viewports_are_explicit() {
        let mut input = request();
        input.positions = Arc::new(Vec::new());
        let empty = prepare_positions(&input).unwrap();
        assert_eq!(empty.accepted_positions, 0);
        assert_eq!(empty.occupied_centers.len(), 1920);
        input.key.cell_width = 0;
        input.key.dot_width = 0;
        assert!(prepare_positions(&input).unwrap().raster.dots.is_empty());
        input = request();
        input.key.cell_width = 1;
        input.key.cell_height = 1;
        input.key.dot_width = 2;
        input.key.dot_height = 4;
        input.positions = Arc::new(vec![
            Position::new("pole".into(), 180.0, 90.0, Some(0)).unwrap(), // Preserve the typed operation.
            Position::new("south".into(), -180.0, -90.0, Some(0)).unwrap(), // Preserve the typed operation.
        ]);
        let tiny = prepare_positions(&input).unwrap();
        assert_eq!(tiny.accepted_positions, 2);
        assert_eq!(tiny.occupied_centers, [true]);
        input.key.dot_width = usize::MAX;
        assert!(prepare_positions(&input).is_none());
    }

    #[test]
    fn unknown_invalid_heading_and_zero_brightness_do_not_invent_strokes() {
        let mut input = request();
        let unknown = prepare_positions(&input).unwrap();
        for heading in [f64::NAN, -1.0, 360.0, 511.0] {
            Arc::make_mut(&mut input.positions)[0].heading_degrees = Some(heading);
            assert_eq!(
                prepare_positions(&input).unwrap().raster.dots,
                unknown.raster.dots
            );
        }
        input.key.marker_brightness = 0;
        let dark = prepare_positions(&input).unwrap();
        assert_eq!(dark.accepted_positions, 1);
        assert!(dark.raster.dots.iter().all(|value| *value == 0.0));
    }
}
