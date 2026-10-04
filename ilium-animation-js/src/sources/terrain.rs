//! Sampling and zero-level contours over broker-admitted native heightfields.
//! The native generator identifies fictional bodies; measured bodies never
//! receive generated substitute data.
use super::*;
use ilium_ambient::animation_services::topography::{Heightfield, WorldId};
use serde_json::json;
fn world(body: &str) -> Result<WorldId> {
    serde_json::from_value(json!(body)).map_err(AnimationError::from)
}
fn admitted<C: BrokerSourceClient>(
    client: &mut C,
    body: &str,
    seed: u32,
    stop: &AtomicBool,
) -> Result<(WorldId, Arc<Heightfield>)> {
    let identity = world(body)?;
    cancelled(stop)?;
    let field = client.terrain(body, seed, stop)?;
    if field.width == 0
        || field.height == 0
        || field.width.checked_mul(field.height) != Some(field.meters.len())
        || field.meters.len() > 2048 * 1024
        || field.meters.iter().any(|value| !value.is_finite())
    {
        return types::fail("invalid native heightfield");
    }
    cancelled(stop)?;
    Ok((identity, field))
}
fn provenance(identity: WorldId) -> Result<Value> {
    if identity.is_fictional() {
        return Ok(
            json!({"fictional":true,"credit":"Native seeded fictional-world generator","datum":"generated zero level"}),
        );
    }
    let manifest: Value = serde_json::from_str(
        ilium_ambient::animation_services::topography::MEASURED_BODY_MANIFEST,
    )?;
    let id = serde_json::to_value(identity)?;
    manifest["bodies"]
        .as_array()
        .and_then(|bodies| bodies.iter().find(|body| body["id"] == id))
        .cloned()
        .ok_or_else(|| AnimationError::Runtime("native terrain provenance missing".into()))
}
pub fn elevation<C: BrokerSourceClient>(
    client: &mut C,
    body: &str,
    bounds: GeographicBounds,
    width: usize,
    height: usize,
    seed: u32,
    stop: &AtomicBool,
) -> Result<Value> {
    bounds.validate()?;
    validate_pixels(width, height)?;
    let (identity, field) = admitted(client, body, seed, stop)?;
    let longitude_span = if bounds.east >= bounds.west {
        bounds.east - bounds.west
    } else {
        360.0 - bounds.west + bounds.east
    };
    let mut elevations = Vec::with_capacity(width * height);
    for row in 0..height {
        cancelled(stop)?;
        let latitude =
            bounds.north - (row as f64 + 0.5) / height as f64 * (bounds.north - bounds.south);
        for column in 0..width {
            let longitude = bounds.west + (column as f64 + 0.5) / width as f64 * longitude_span;
            elevations.push(field.sample(longitude, latitude));
        }
    }
    Ok(
        json!({"body":body,"name":field.name,"seed":if identity.is_fictional(){Some(seed)}else{None},"fictional":identity.is_fictional(),"bounds":bounds,"width":width,"height":height,"units":"metres_relative_to_native_zero_level","row_order":"north_to_south","elevations":elevations,"provenance":provenance(identity)?}),
    )
}
pub fn coastlines<C: BrokerSourceClient>(
    client: &mut C,
    body: &str,
    bounds: GeographicBounds,
    max_points: usize,
    seed: Option<u64>,
    stop: &AtomicBool,
) -> Result<Value> {
    bounds.validate()?;
    if !(2..=100_000).contains(&max_points) {
        return types::fail("coastline point budget");
    }
    let seed = u32::try_from(seed.unwrap_or(0))
        .map_err(|_| AnimationError::Runtime("native world seed exceeds u32".into()))?;
    let (identity, field) = admitted(client, body, seed, stop)?;
    // Bound the contour extraction independently of source dataset size.
    // Native zero-level contours are shores only on Earth/fictional oceans.
    let width = field.width.min(720);
    let height = field.height.min(360);
    let span = if bounds.east >= bounds.west {
        bounds.east - bounds.west
    } else {
        360.0 - bounds.west + bounds.east
    };
    let mut segments = Vec::new();
    let mut point_count = 0;
    let mut truncated = false;
    'rows: for row in 0..height {
        cancelled(stop)?;
        let north = bounds.north - row as f64 / height as f64 * (bounds.north - bounds.south);
        let south = bounds.north - (row + 1) as f64 / height as f64 * (bounds.north - bounds.south);
        for col in 0..width {
            let west = bounds.west + col as f64 / width as f64 * span;
            let east = bounds.west + (col + 1) as f64 / width as f64 * span;
            let corners = [[west, north], [east, north], [east, south], [west, south]];
            let values = corners.map(|point| field.sample(point[0], point[1]));
            let mut edges = Vec::with_capacity(4);
            for edge in 0..4 {
                let next = (edge + 1) % 4;
                if (values[edge] < 0.) == (values[next] < 0.) {
                    continue;
                }
                let t = f64::from(values[edge]) / f64::from(values[edge] - values[next]);
                edges.push([
                    corners[edge][0] + t * (corners[next][0] - corners[edge][0]),
                    corners[edge][1] + t * (corners[next][1] - corners[edge][1]),
                ]);
            }
            // Resolve saddle pairing using the actual bilinear cell centre.
            if edges.len() == 4
                && (field.sample((west + east) / 2., (north + south) / 2.) < 0.) != (values[0] < 0.)
            {
                edges.rotate_left(1);
            }
            for pair in edges.chunks_exact(2) {
                if point_count + 2 > max_points {
                    truncated = true;
                    break 'rows;
                }
                let point = |position: [f64; 2]| json!({"x":(position[0]+180.).rem_euclid(360.)-180.,"y":position[1]});
                segments.push(json!([point(pair[0]), point(pair[1])]));
                point_count += 2;
            }
        }
    }
    Ok(
        json!({"body":body,"fictional":identity.is_fictional(),"seed":if identity.is_fictional(){Some(seed)}else{None},"semantics":"zero_elevation_contour_not_implied_water_boundary","space":"longitude_latitude_degrees","paths":segments,"truncated":truncated,"sample_width":width,"sample_height":height,"provenance":provenance(identity)?}),
    )
}
