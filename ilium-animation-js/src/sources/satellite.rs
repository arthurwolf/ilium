//! Native cloud/NightLights plans over broker-controlled transport and images.
use super::*;
use ilium_ambient::animation_services::satellite as native;
use serde_json::json;
const DAILY: &str = "VIIRS_NOAA20_GapFilled_BRDF_Corrected_DayNightBand_Radiance";
const BLACK_MARBLE: &str = "VIIRS_Black_Marble";
pub fn weather_layer(layer: &str) -> bool {
    matches!(
        layer,
        "auto"
            | "goes_east"
            | "goes_west"
            | "meteosat"
            | "indian_ocean"
            | "world_ir"
            | "polar_daily"
            | "night_lights_daily"
            | "black_marble"
    )
}
fn optional_bytes<C: BrokerSourceClient>(
    client: &mut C,
    url: String,
    max_bytes: usize,
    response_type: &str,
    stop: &AtomicBool,
) -> Result<Option<Vec<u8>>> {
    cancelled(stop)?;
    let options = http_options(url, max_bytes, response_type);
    options.validate()?;
    let response = client.request(&options, stop)?;
    cancelled(stop)?;
    if response.status == 404 {
        return Ok(None);
    }
    if !(200..=299).contains(&response.status) {
        return types::fail(&format!("satellite HTTP {}", response.status));
    }
    let data = match response.body {
        Value::String(text) => text.into_bytes(),
        Value::Array(values) => values
            .into_iter()
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|value| u8::try_from(value).ok())
                    .ok_or_else(|| AnimationError::Runtime("satellite byte response type".into()))
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return types::fail("satellite response type"),
    };
    if data.len() > max_bytes {
        return types::fail("satellite byte budget");
    }
    Ok(Some(data))
}
/// One fixed native tile frame plan, including its optional already-retrieved
/// probe. Transport/cancellation stay separate from provider frame identity.
struct TileLayerRequest<'a> {
    layer: &'a str,
    matrix: &'a str,
    level: u8,
    bbox: &'a native::GeoBox,
    time: native::UtcTime,
    daily: bool,
    probe: Option<(native::TileId, Vec<u8>)>,
}
fn tile_layer<C: BrokerSourceClient>(
    client: &mut C,
    request: TileLayerRequest<'_>,
    stop: &AtomicBool,
) -> Result<Option<Vec<Value>>> {
    let TileLayerRequest {
        layer,
        matrix,
        level,
        bbox,
        time,
        daily,
        probe,
    } = request;
    let ids = native::tiles_for_box(level, bbox);
    let mut encoded = Vec::new();
    let mut probe = probe;
    let mut total_bytes = 0usize;
    let time_text = if daily {
        time.date()
    } else {
        time.iso_seconds()
    };
    for id in &ids {
        cancelled(stop)?;
        let url = native::gibs_tile_url(
            layer,
            Some(&time_text),
            matrix,
            *id,
            if daily && layer != "VIIRS_Black_Marble" && layer != DAILY {
                "jpeg"
            } else {
                "png"
            },
        );
        let data = if probe.as_ref().is_some_and(|(probe_id, _)| probe_id == id) {
            probe.take().map(|(_, data)| data)
        } else {
            optional_bytes(client, url, 8 * 1024 * 1024, "bytes", stop)?
        };
        if let Some(data) = data {
            total_bytes = total_bytes
                .checked_add(data.len())
                .ok_or_else(|| AnimationError::Runtime("satellite frame bytes overflow".into()))?;
            if total_bytes > 16 * 1024 * 1024 {
                return types::fail("satellite encoded frame budget");
            }
            encoded.push((*id, data));
        }
    }
    // Native clouds demand >=half of planned tiles; preserve missing coverage
    // explicitly, with no substituted imagery for absent source regions.
    if encoded.is_empty() || encoded.len() * 2 < ids.len() {
        return Ok(None);
    }
    let missing = ids.len() - encoded.len();
    let mut output = Vec::with_capacity(encoded.len());
    for (id, data) in encoded {
        cancelled(stop)?;
        let image = client.decode_image(&data, 512 * 512, stop)?;
        let span = native::tile_span_degrees(level);
        output.push(json!({"image":image,"source_extent":{"west":-180.+f64::from(id.col)*span,"east":-180.+f64::from(id.col+1)*span,"north":90.-f64::from(id.row)*span,"south":90.-f64::from(id.row+1)*span},"level":level,"column":id.col,"row":id.row,"epoch_ms":time.0.checked_mul(1000),"missing_tiles":missing}));
    }
    Ok(Some(output))
}
fn night<C: BrokerSourceClient>(
    client: &mut C,
    requested: &str,
    options: &WeatherOptions,
    bbox: &native::GeoBox,
    now: native::UtcTime,
    stop: &AtomicBool,
) -> Result<Value> {
    let density = options.image_width as f64 / bbox.width_degrees().max(0.001);
    let level = native::level_within_budget(
        native::level_for_density(density, 3),
        bbox,
        options.max_tiles,
    );
    // Daily availability is confirmed by actual image retrieval, just as in
    // the native worker; no guessed date becomes an available snapshot.
    if requested == "night_lights_daily" {
        for back in 0..=4 {
            let time = now.start_of_day().plus_seconds(-back * 86400);
            let ids = native::tiles_for_box(level, bbox);
            let first = *ids
                .first()
                .ok_or_else(|| AnimationError::Runtime("empty night tile plan".into()))?;
            let probe = optional_bytes(
                client,
                native::gibs_tile_url(DAILY, Some(&time.date()), "500m", first, "png"),
                8 * 1024 * 1024,
                "bytes",
                stop,
            )?;
            let Some(data) = probe else {
                continue;
            };
            if let Some(tiles) = tile_layer(
                client,
                TileLayerRequest {
                    layer: DAILY,
                    matrix: "500m",
                    level,
                    bbox,
                    time,
                    daily: true,
                    probe: Some((first, data)),
                },
                stop,
            )? {
                return Ok(
                    json!({"requested_layer":requested,"layer":"night_lights_daily","epoch_ms":time.0.checked_mul(1000),"tiles":tiles,"clip":options.geographic.bounds,"fallback":false,"attribution":"NASA GIBS / VIIRS NOAA-20 daily radiance"}),
                );
            }
        }
    }
    let time = native::UtcTime::from_civil(2016, 1, 1, 0, 0, 0);
    let tiles = tile_layer(
        client,
        TileLayerRequest {
            layer: BLACK_MARBLE,
            matrix: "500m",
            level,
            bbox,
            time,
            daily: true,
            probe: None,
        },
        stop,
    )?
    .ok_or_else(|| AnimationError::Runtime("Black Marble source has no usable tiles".into()))?;
    Ok(
        json!({"requested_layer":requested,"layer":"black_marble","epoch_ms":time.0.checked_mul(1000),"tiles":tiles,"clip":options.geographic.bounds,"fallback":requested!="black_marble","attribution":"NASA GIBS / VIIRS Black Marble 2016 composite"}),
    )
}
fn latest<C: BrokerSourceClient>(
    client: &mut C,
    provider: &native::Provider,
    now: native::UtcTime,
    stop: &AtomicBool,
) -> Result<Option<native::UtcTime>> {
    match provider.endpoint {
        native::Endpoint::Wms { layer, workspace } => {
            let Some(data) = optional_bytes(
                client,
                native::wms_capabilities_url(workspace),
                2 * 1024 * 1024,
                "text",
                stop,
            )?
            else {
                return Ok(None);
            };
            Ok(
                native::wms_default_time(&String::from_utf8_lossy(&data), layer)
                    .filter(|time| time.0 <= now.0),
            )
        }
        native::Endpoint::GibsDaily { .. } => Ok(Some(now.start_of_day())),
        native::Endpoint::GibsTimed {
            layer, matrix_set, ..
        } => {
            let Some(data) = optional_bytes(
                client,
                native::gibs_domains_url(layer, matrix_set, now.plus_seconds(-7 * 86400), now),
                2 * 1024 * 1024,
                "text",
                stop,
            )?
            else {
                return Ok(None);
            };
            Ok(native::parse_domains(&String::from_utf8_lossy(&data))
                .into_iter()
                .filter(|time| time.0 <= now.0)
                .max_by_key(|time| time.0))
        }
    }
}
fn frame<C: BrokerSourceClient>(
    client: &mut C,
    provider: &native::Provider,
    time: native::UtcTime,
    options: &WeatherOptions,
    bbox: &native::GeoBox,
    stop: &AtomicBool,
) -> Result<Option<Value>> {
    match provider.endpoint {
        native::Endpoint::Wms { layer, .. } => {
            let Some(data) = optional_bytes(
                client,
                native::wms_map_url(
                    layer,
                    bbox,
                    options.image_width,
                    options.image_height,
                    Some(time),
                ),
                8 * 1024 * 1024,
                "bytes",
                stop,
            )?
            else {
                return Ok(None);
            };
            let image =
                client.decode_image(&data, options.image_width * options.image_height, stop)?;
            Ok(Some(
                json!({"epoch_ms":time.0.checked_mul(1000),"image":image,"bounds":options.geographic.bounds}),
            ))
        }
        native::Endpoint::GibsTimed {
            layer,
            matrix_set,
            max_level,
        }
        | native::Endpoint::GibsDaily {
            layer,
            matrix_set,
            max_level,
        } => {
            let daily = matches!(provider.endpoint, native::Endpoint::GibsDaily { .. });
            let level = native::level_within_budget(
                native::level_for_density(
                    options.image_width as f64 / bbox.width_degrees().max(0.001),
                    max_level,
                ),
                bbox,
                options.max_tiles.min(9),
            );
            Ok(tile_layer(client, TileLayerRequest { layer, matrix: matrix_set, level, bbox, time, daily, probe: None }, stop)?.map(|tiles|json!({"epoch_ms":time.0.checked_mul(1000),"tiles":tiles,"clip":options.geographic.bounds})))
        }
    }
}
fn clouds<C: BrokerSourceClient>(
    client: &mut C,
    requested: &str,
    options: &WeatherOptions,
    bbox: &native::GeoBox,
    now: native::UtcTime,
    stop: &AtomicBool,
) -> Result<Value> {
    let requested_source: native::CloudSource = serde_json::from_value(json!(requested))?;
    let mut source = native::choose_source(
        requested_source,
        bbox.width_degrees() >= 300.0 && bbox.height_degrees() >= 150.0,
        (bbox.south + bbox.north) / 2.,
        (bbox.west + bbox.east) / 2.,
    );
    // Native Auto currently falls back to world infrared in provider(). Keep
    // actual resolved identity in the result, never label it as GOES.
    for _ in 0..3 {
        let provider = native::provider(source);
        if let Some(latest) = latest(client, provider, now, stop)? {
            let times = if options.history_hours == 0 {
                (0..4)
                    .map(|back| latest.plus_seconds(-back * provider.cadence_minutes * 60))
                    .collect()
            } else {
                native::plan_frame_times(
                    latest,
                    options.history_hours,
                    provider.cadence_minutes,
                    options.max_frames,
                )
            };
            let mut frames = Vec::new();
            for time in times {
                cancelled(stop)?;
                if let Some(frame) = frame(client, provider, time, options, bbox, stop)? {
                    frames.push(frame);
                    if options.history_hours == 0 {
                        break;
                    }
                }
            }
            if !frames.is_empty() {
                frames.reverse();
                return Ok(
                    json!({"requested_layer":requested,"layer":provider.source,"provider":provider.name,"frames":frames,"fallback":source!=requested_source && requested_source!=native::CloudSource::Auto,"attribution":if matches!(provider.endpoint,native::Endpoint::Wms{..}){"Copyright EUMETSAT; EUMETSAT data licensing"}else{"NASA GIBS / source satellite provider"}}),
                );
            }
        }
        let Some(next) = native::fallback(source) else {
            break;
        };
        source = next;
    }
    types::fail("native cloud source and advertised fallbacks have no usable frames")
}
pub fn weather<C: BrokerSourceClient>(
    client: &mut C,
    options: &WeatherOptions,
    now_ms: i64,
    revision: u64,
    stop: &AtomicBool,
) -> Result<WeatherSnapshot> {
    options.validate()?;
    let bounds = options.geographic.bounds;
    if bounds.west >= bounds.east || bounds.south >= bounds.north {
        return types::fail("weather crop needs nonempty nonwrapping bounds; split dateline crops");
    }
    let bbox = native::GeoBox {
        west: bounds.west,
        east: bounds.east,
        south: bounds.south,
        north: bounds.north,
    };
    let epoch = options.anchor_epoch_ms.unwrap_or(now_ms);
    if !(0..=253_402_300_799_000).contains(&epoch) {
        return types::fail("invalid satellite time anchor");
    }
    let now = native::UtcTime(epoch / 1000);
    let mut layers = Vec::new();
    let mut observed = None;
    for layer in &options.layers {
        cancelled(stop)?;
        let result = if matches!(layer.as_str(), "night_lights_daily" | "black_marble") {
            night(client, layer, options, &bbox, now, stop)?
        } else {
            clouds(client, layer, options, &bbox, now, stop)?
        };
        let time = result["epoch_ms"].as_i64().or_else(|| {
            result["frames"]
                .as_array()
                .and_then(|frames| frames.last())
                .and_then(|frame| frame["epoch_ms"].as_i64())
        });
        if let Some(time) = time {
            observed = Some(observed.map_or(time, |previous: i64| previous.min(time)));
        }
        layers.push(result);
    }
    Ok(WeatherSnapshot {
        metadata: captured(revision, now_ms, observed),
        layers,
        attribution: "Per-layer source credit retained; per-frame capture/time/extent retained"
            .into(),
    })
}
