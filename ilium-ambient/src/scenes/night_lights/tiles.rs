//! Shared satellite-imagery plumbing for the night-lights and clouds scenes:
//! UTC time formatting, the NASA GIBS EPSG:4326 tile matrix, WMS URL building,
//! image decoding, lon/lat registered grids, mosaic assembly, a mip chain that
//! keeps small bright features alive, and a bounded, stoppable tile download.
//!
//! Data services used by the callers (all keyless):
//! * NASA GIBS WMTS, EPSG:4326 "best" endpoint. Documentation:
//!   <https://nasa-gibs.github.io/gibs-api-docs/access-basics/> and
//!   <https://nasa-gibs.github.io/gibs-api-docs/available-visualizations/>.
//!   Terms: NASA data are free to use; acknowledge NASA GIBS/ESDIS
//!   (<https://www.earthdata.nasa.gov/engage/open-data-services-software-policies>).
//! * EUMETSAT EUMETView WMS (`https://view.eumetsat.int/geoserver/ows`).
//!   Its capabilities document declares `Fees: none`, `AccessConstraints: none`;
//!   EUMETSAT asks for the credit "Copyright EUMETSAT"
//!   (<https://www.eumetsat.int/eumetsat-data-licensing>).

use crate::source::{self, FetchError};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const GIBS_ROOT: &str = "https://gibs.earthdata.nasa.gov/wmts/epsg4326/best";
pub const EUMETVIEW_ROOT: &str = "https://view.eumetsat.int/geoserver";
pub const TILE_PIXELS: usize = 512;
/// GIBS EPSG:4326 level 0 tiles are 512 px at 0.5625 degrees per pixel.
const LEVEL0_SPAN_DEGREES: f64 = 288.0;
/// Tile URLs that contain a date or a fixed layer never change: cache "forever".
pub const IMMUTABLE_AGE: Duration = Duration::from_secs(10 * 365 * 24 * 3600);
const TILE_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_TILE_BYTES: usize = 12 * 1024 * 1024;

// ---------------------------------------------------------------------------
// UTC time (no calendar dependency)

/// A UTC instant with one-second resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcTime(pub i64);

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

impl UtcTime {
    pub fn from_system_time(time: SystemTime) -> Self {
        match time.duration_since(UNIX_EPOCH) {
            Ok(after) => Self(after.as_secs() as i64),
            Err(before) => Self(-(before.duration().as_secs() as i64)),
        }
    }

    pub fn from_civil(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
    ) -> Self {
        Self(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second)
    }

    /// (year, month, day, hour, minute, second)
    pub fn civil(self) -> (i64, i64, i64, i64, i64, i64) {
        let days = self.0.div_euclid(86_400);
        let seconds = self.0.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        (
            year,
            month,
            day,
            seconds / 3600,
            (seconds % 3600) / 60,
            seconds % 60,
        )
    }

    /// `2026-09-30T12:50:00Z`, the GIBS/WMS time syntax.
    pub fn iso_seconds(self) -> String {
        let (y, mo, d, h, mi, s) = self.civil();
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
    }

    /// `2026-09-30`, the GIBS daily-layer time syntax.
    pub fn date(self) -> String {
        let (y, mo, d, ..) = self.civil();
        format!("{y:04}-{mo:02}-{d:02}")
    }

    /// `2026-09-30 12:50 UTC`, for status lines.
    pub fn label(self) -> String {
        let (y, mo, d, h, mi, _) = self.civil();
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02} UTC")
    }

    pub fn plus_seconds(self, seconds: i64) -> Self {
        Self(self.0 + seconds)
    }

    /// Round down to a multiple of `step_seconds` since the Unix epoch.
    pub fn floor_to(self, step_seconds: i64) -> Self {
        Self(self.0 - self.0.rem_euclid(step_seconds.max(1)))
    }

    pub fn start_of_day(self) -> Self {
        self.floor_to(86_400)
    }

    /// `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM[:SS[.fff]]Z`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (date_part, time_part) = match text.split_once(['T', ' ']) {
            Some((date, time)) => (date, Some(time)),
            None => (text, None),
        };
        let mut date_fields = date_part.split('-');
        let year: i64 = date_fields.next()?.parse().ok()?;
        let month: i64 = date_fields.next()?.parse().ok()?;
        let day: i64 = date_fields.next()?.parse().ok()?;
        if date_fields.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let (mut hour, mut minute, mut second) = (0, 0, 0);
        if let Some(time) = time_part {
            let time = time.trim_end_matches('Z');
            let mut fields = time.split(':');
            hour = fields.next()?.parse().ok()?;
            minute = fields.next().unwrap_or("0").parse().ok()?;
            if let Some(seconds) = fields.next() {
                second = seconds.split('.').next()?.parse().ok()?;
            }
            if hour > 23 || minute > 59 || second > 60 {
                return None;
            }
        }
        Some(Self::from_civil(year, month, day, hour, minute, second))
    }
}

/// ISO-8601 duration ("PT10M", "PT3H", "P1D", "PT1H30M") in seconds.
pub fn parse_period_seconds(text: &str) -> Option<i64> {
    let rest = text.trim().strip_prefix('P')?;
    let (date_part, time_part) = match rest.split_once('T') {
        Some((date, time)) => (date, time),
        None => (rest, ""),
    };
    let mut total = 0i64;
    let mut number = String::new();
    for character in date_part.chars() {
        if character.is_ascii_digit() {
            number.push(character);
        } else {
            let value: i64 = number.parse().ok()?;
            number.clear();
            total += match character {
                'D' => value * 86_400,
                'W' => value * 7 * 86_400,
                _ => return None,
            };
        }
    }
    for character in time_part.chars() {
        if character.is_ascii_digit() {
            number.push(character);
        } else {
            let value: i64 = number.parse().ok()?;
            number.clear();
            total += match character {
                'H' => value * 3600,
                'M' => value * 60,
                'S' => value,
                _ => return None,
            };
        }
    }
    (number.is_empty() && total > 0).then_some(total)
}

/// Expand `start/end/PT10M,start/end/PT10M,single` (the GIBS DescribeDomains
/// and WMTS dimension syntax) into ascending, de-duplicated instants. At most
/// `limit` instants are produced.
pub fn expand_time_ranges(text: &str, limit: usize) -> Vec<UtcTime> {
    let mut times = Vec::new();
    for range in text.split(',') {
        let fields: Vec<&str> = range.trim().split('/').collect();
        match fields.as_slice() {
            [single] => {
                if let Some(time) = UtcTime::parse(single) {
                    times.push(time);
                }
            }
            [start, end, period] => {
                let (Some(start), Some(end), Some(step)) = (
                    UtcTime::parse(start),
                    UtcTime::parse(end),
                    parse_period_seconds(period),
                ) else {
                    continue;
                };
                let mut current = start;
                while current <= end && times.len() < limit {
                    times.push(current);
                    current = current.plus_seconds(step);
                }
            }
            _ => {}
        }
        if times.len() >= limit {
            break;
        }
    }
    times.sort_unstable();
    times.dedup();
    times
}

/// Text between the first `open` and the next `close` after it.
pub fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let length = text[start..].find(close)?;
    Some(&text[start..start + length])
}

// ---------------------------------------------------------------------------
// Geometry and GIBS tile matrix

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoBox {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl GeoBox {
    pub const WORLD: Self = Self {
        west: -180.0,
        south: -90.0,
        east: 180.0,
        north: 90.0,
    };

    pub fn width_degrees(&self) -> f64 {
        self.east - self.west
    }

    pub fn height_degrees(&self) -> f64 {
        self.north - self.south
    }

    #[cfg(test)]
    pub fn center(&self) -> (f64, f64) {
        (
            (self.west + self.east) / 2.0,
            (self.south + self.north) / 2.0,
        )
    }
}

/// One tile of the GIBS EPSG:4326 matrix (origin at -180, 90, rows count south).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    pub level: u8,
    pub col: u32,
    pub row: u32,
}

/// Edge length of one tile in degrees at `level`.
pub fn tile_span_degrees(level: u8) -> f64 {
    LEVEL0_SPAN_DEGREES / f64::from(1u32 << level)
}

/// Tile columns and rows of the whole-world matrix at `level`.
pub fn matrix_size(level: u8) -> (u32, u32) {
    let span = tile_span_degrees(level);
    ((360.0 / span).ceil() as u32, (180.0 / span).ceil() as u32)
}

/// Source pixels per degree at `level`.
pub fn pixels_per_degree(level: u8) -> f64 {
    TILE_PIXELS as f64 / tile_span_degrees(level)
}

#[cfg(test)]
pub fn tile_bounds(id: TileId) -> GeoBox {
    let span = tile_span_degrees(id.level);
    GeoBox {
        west: -180.0 + f64::from(id.col) * span,
        east: -180.0 + f64::from(id.col + 1) * span,
        north: 90.0 - f64::from(id.row) * span,
        south: 90.0 - f64::from(id.row + 1) * span,
    }
}

/// The tile containing a point (longitude wraps, latitude clamps).
pub fn lon_lat_to_tile(level: u8, lon: f64, lat: f64) -> TileId {
    let span = tile_span_degrees(level);
    let (columns, rows) = matrix_size(level);
    let wrapped = (lon + 180.0).rem_euclid(360.0);
    let col = ((wrapped / span).floor() as u32).min(columns - 1);
    let row = (((90.0 - lat.clamp(-90.0, 90.0)) / span).floor() as u32).min(rows - 1);
    TileId { level, col, row }
}

/// Every tile touched by `bbox` (longitudes may exceed +-180 and then wrap).
pub fn tiles_for_box(level: u8, bbox: &GeoBox) -> Vec<TileId> {
    let span = tile_span_degrees(level);
    let mut tiles = Vec::new();
    let north = bbox.north.min(90.0);
    let south = bbox.south.max(-90.0);
    let mut lat = north;
    loop {
        let mut lon = bbox.west;
        loop {
            let tile = lon_lat_to_tile(level, lon, lat.max(south));
            if !tiles.contains(&tile) {
                tiles.push(tile);
            }
            if lon >= bbox.east {
                break;
            }
            lon = (lon + span * 0.5).min(bbox.east);
        }
        if lat <= south {
            break;
        }
        lat = (lat - span * 0.5).max(south);
    }
    tiles.sort_unstable();
    tiles
}

/// Smallest level whose pixel density reaches `dots_per_degree`, at most
/// `max_level`.
pub fn level_for_density(dots_per_degree: f64, max_level: u8) -> u8 {
    (0..=max_level)
        .find(|level| pixels_per_degree(*level) >= dots_per_degree)
        .unwrap_or(max_level)
}

/// Highest level (<= `level`) whose tile count for `bbox` fits `max_tiles`.
pub fn level_within_budget(level: u8, bbox: &GeoBox, max_tiles: usize) -> u8 {
    let mut level = level;
    while level > 0 && tiles_for_box(level, bbox).len() > max_tiles {
        level -= 1;
    }
    level
}

// ---------------------------------------------------------------------------
// URLs

/// GIBS REST tile URL. `time` is `None` for layers without a time dimension.
pub fn gibs_tile_url(
    layer: &str,
    time: Option<&str>,
    matrix_set: &str,
    id: TileId,
    extension: &str,
) -> String {
    let TileId { level, col, row } = id;
    match time {
        Some(time) => format!(
            "{GIBS_ROOT}/{layer}/default/{time}/{matrix_set}/{level}/{row}/{col}.{extension}"
        ),
        None => format!("{GIBS_ROOT}/{layer}/default/{matrix_set}/{level}/{row}/{col}.{extension}"),
    }
}

/// GIBS DescribeDomains: the times a layer really has inside a window.
pub fn gibs_domains_url(layer: &str, matrix_set: &str, start: UtcTime, end: UtcTime) -> String {
    format!(
        "{GIBS_ROOT}/1.0.0/{layer}/default/{matrix_set}/all/{}--{}.xml",
        start.iso_seconds(),
        end.iso_seconds()
    )
}

/// The content of the `<Domain>` element of a DescribeDomains answer.
pub fn parse_domains(xml: &str) -> Vec<UtcTime> {
    between(xml, "<Domain>", "</Domain>")
        .map(|text| expand_time_ranges(text, 5000))
        .unwrap_or_default()
}

/// WMS GetMap (CRS:84 keeps lon,lat axis order).
pub fn wms_map_url(
    layer: &str,
    bbox: &GeoBox,
    width: usize,
    height: usize,
    time: Option<UtcTime>,
) -> String {
    let mut url = format!(
        "{EUMETVIEW_ROOT}/ows?service=WMS&version=1.3.0&request=GetMap&layers={layer}&styles=&crs=CRS:84&bbox={:.4},{:.4},{:.4},{:.4}&width={width}&height={height}&format=image/png&transparent=true",
        bbox.west, bbox.south, bbox.east, bbox.north
    );
    if let Some(time) = time {
        url.push_str("&time=");
        url.push_str(&time.iso_seconds());
    }
    url
}

pub fn wms_capabilities_url(workspace: &str) -> String {
    format!("{EUMETVIEW_ROOT}/{workspace}/ows?service=WMS&version=1.3.0&request=GetCapabilities")
}

/// The newest time of `layer` (the `default` attribute of its time dimension).
/// The workspace-specific capabilities documents (`.../msg_fes/ows`) list the
/// layer without its workspace prefix (`ir108`), the global one with it
/// (`msg_fes:ir108`); both spellings are accepted.
pub fn wms_default_time(capabilities: &str, layer: &str) -> Option<UtcTime> {
    let short_name = layer.rsplit(':').next().unwrap_or(layer);
    let name_at = capabilities
        .find(&format!("<Name>{layer}</Name>"))
        .or_else(|| capabilities.find(&format!("<Name>{short_name}</Name>")))?;
    let rest = &capabilities[name_at..];
    let layer_end = rest.find("</Layer>").unwrap_or(rest.len());
    let rest = &rest[..layer_end];
    let value = between(rest, "<Dimension name=\"time\"", "</Dimension>")?;
    let default = between(value, "default=\"", "\"")?;
    UtcTime::parse(default)
}

// ---------------------------------------------------------------------------
// Fetching

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileError {
    /// The server answered "not found": that tile/time simply does not exist.
    Missing,
    /// Transport failure or server error: retrying later may work.
    Network(String),
    /// The owning scene is being dropped.
    Stopped,
}

impl From<FetchError> for TileError {
    fn from(error: FetchError) -> Self {
        let text = error.to_string();
        if text.contains("404") {
            Self::Missing
        } else {
            Self::Network(text)
        }
    }
}

/// Where scene workers get bytes. Production uses [`HttpFetcher`]; tests inject
/// synthetic tiles, so no test ever touches the network.
pub trait TileFetcher: Send + Sync {
    fn fetch(
        &self,
        cache_dir: &Path,
        url: &str,
        max_age: Duration,
        stop: &AtomicBool,
    ) -> Result<Vec<u8>, TileError>;
}

/// `source::fetch_cached` with throttling, User-Agent and disk cache.
pub struct HttpFetcher;

impl TileFetcher for HttpFetcher {
    fn fetch(
        &self,
        cache_dir: &Path,
        url: &str,
        max_age: Duration,
        stop: &AtomicBool,
    ) -> Result<Vec<u8>, TileError> {
        if stop.load(Ordering::Relaxed) {
            return Err(TileError::Stopped);
        }
        let extension = url
            .split('?')
            .next()
            .and_then(|path| path.rsplit('.').next())
            .filter(|extension| extension.len() <= 4 && !extension.contains('/'))
            .unwrap_or("bin");
        // WMS URLs carry no extension: they are all PNG maps or XML documents.
        let extension = if url.contains("format=image/png") {
            "png"
        } else if url.contains("GetCapabilities") {
            "xml"
        } else {
            extension
        };
        let bytes = source::fetch_cached(
            cache_dir,
            url,
            extension,
            max_age,
            MAX_TILE_BYTES,
            TILE_TIMEOUT,
        )
        .map_err(TileError::from)?;
        // A WMS service exception is an XML body behind a success status. It
        // must not stay in the (immutable) cache under an image name.
        if matches!(extension, "png" | "jpeg" | "jpg") && !looks_like_image(&bytes) {
            let _ = std::fs::remove_file(cache_dir.join(source::cache_file_name(url, extension)));
            return Err(TileError::Missing);
        }
        Ok(bytes)
    }
}

/// True for PNG and JPEG data (by magic number).
pub fn looks_like_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G']) || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// Clock injected into workers so tests never read the real one.
pub type NowFn = std::sync::Arc<dyn Fn() -> SystemTime + Send + Sync>;

pub fn system_clock() -> NowFn {
    std::sync::Arc::new(SystemTime::now)
}

/// Delete the oldest regular files in `directory` until it holds at most
/// `max_bytes`. Only direct children are touched. Returns the bytes removed.
pub fn prune_cache(directory: &Path, max_bytes: u64) -> u64 {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    let mut files: Vec<(SystemTime, u64, std::path::PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| {
                (
                    metadata.modified().unwrap_or(UNIX_EPOCH),
                    metadata.len(),
                    entry.path(),
                )
            })
        })
        .collect();
    let mut total: u64 = files.iter().map(|file| file.1).sum();
    files.sort();
    let mut removed = 0;
    for (_, length, path) in files {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= length;
            removed += length;
        }
    }
    removed
}

// ---------------------------------------------------------------------------
// Decoding

/// A decoded tile or map: 8-bit luminance (transparent pixels are black) and
/// optionally the colour.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedImage {
    pub width: usize,
    pub height: usize,
    pub luma: Vec<u8>,
    pub rgb: Option<Vec<[u8; 3]>>,
}

pub fn decode_image(bytes: &[u8], keep_rgb: bool) -> Result<DecodedImage, String> {
    let rgba = image::load_from_memory(bytes)
        .map_err(|error| format!("image decode failed: {error}"))?
        .to_rgba8();
    let (width, height) = (rgba.width() as usize, rgba.height() as usize);
    let mut luma = Vec::with_capacity(width * height);
    let mut rgb = keep_rgb.then(|| Vec::with_capacity(width * height));
    for pixel in rgba.pixels() {
        let [red, green, blue, alpha] = pixel.0;
        let alpha = u32::from(alpha);
        let value = (299 * u32::from(red) + 587 * u32::from(green) + 114 * u32::from(blue)) / 1000;
        luma.push(((value * alpha + 127) / 255) as u8);
        if let Some(rgb) = rgb.as_mut() {
            let scale = |channel: u8| ((u32::from(channel) * alpha + 127) / 255) as u8;
            rgb.push([scale(red), scale(green), scale(blue)]);
        }
    }
    Ok(DecodedImage {
        width,
        height,
        luma,
        rgb,
    })
}

// ---------------------------------------------------------------------------
// Lon/lat registered grids

/// An equirectangular image registered to a lon/lat box, one byte of luminance
/// per texel and optionally colour.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoGrid {
    pub bbox: GeoBox,
    pub width: usize,
    pub height: usize,
    pub luma: Vec<u8>,
    pub rgb: Option<Vec<[u8; 3]>>,
}

impl GeoGrid {
    pub fn blank(bbox: GeoBox, width: usize, height: usize, want_rgb: bool) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            bbox,
            width,
            height,
            luma: vec![0; width * height],
            rgb: want_rgb.then(|| vec![[0; 3]; width * height]),
        }
    }

    pub fn from_image(bbox: GeoBox, image: DecodedImage) -> Self {
        Self {
            bbox,
            width: image.width.max(1),
            height: image.height.max(1),
            luma: image.luma,
            rgb: image.rgb,
        }
    }

    /// True when the grid covers all longitudes, so sampling wraps.
    pub fn wraps_longitude(&self) -> bool {
        self.bbox.width_degrees() >= 359.0
    }

    /// Texels per degree of longitude.
    pub fn texels_per_degree(&self) -> f64 {
        self.width as f64 / self.bbox.width_degrees()
    }

    /// Continuous texel coordinates (texel centres at +0.5) of a point.
    fn texel_position(&self, lon: f64, lat: f64) -> Option<(f64, f64)> {
        if lat > self.bbox.north || lat < self.bbox.south {
            return None;
        }
        let width_degrees = self.bbox.width_degrees();
        let relative = if self.wraps_longitude() {
            (lon - self.bbox.west).rem_euclid(width_degrees)
        } else if lon < self.bbox.west || lon > self.bbox.east {
            return None;
        } else {
            lon - self.bbox.west
        };
        Some((
            relative / width_degrees * self.width as f64,
            (self.bbox.north - lat) / self.bbox.height_degrees() * self.height as f64,
        ))
    }

    fn bilinear(&self, lon: f64, lat: f64, texel: impl Fn(usize) -> f32) -> Option<f32> {
        let (u, v) = self.texel_position(lon, lat)?;
        let fx = u - 0.5;
        let fy = v - 0.5;
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
        let wrap = self.wraps_longitude();
        let column = |x: i64| -> usize {
            if wrap {
                x.rem_euclid(self.width as i64) as usize
            } else {
                x.clamp(0, self.width as i64 - 1) as usize
            }
        };
        let row = |y: i64| -> usize { y.clamp(0, self.height as i64 - 1) as usize };
        let (x0, y0) = (x0 as i64, y0 as i64);
        let at = |x: i64, y: i64| texel(row(y) * self.width + column(x));
        let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1, y0) * tx;
        let bottom = at(x0, y0 + 1) * (1.0 - tx) + at(x0 + 1, y0 + 1) * tx;
        Some(top * (1.0 - ty) + bottom * ty)
    }

    /// Bilinear luminance 0..=1, `None` outside the grid.
    pub fn sample_luma(&self, lon: f64, lat: f64) -> Option<f32> {
        self.bilinear(lon, lat, |index| f32::from(self.luma[index]) / 255.0)
    }

    /// Bilinear colour 0..=1 per channel, `None` outside or without colour.
    pub fn sample_rgb(&self, lon: f64, lat: f64) -> Option<[f32; 3]> {
        let rgb = self.rgb.as_ref()?;
        let channel = |c: usize| self.bilinear(lon, lat, |index| f32::from(rgb[index][c]) / 255.0);
        Some([channel(0)?, channel(1)?, channel(2)?])
    }

    /// Subtract a black level (0..=255) and stretch the rest back to full range.
    pub fn apply_black_level(&mut self, black: u8) {
        let black = u32::from(black.min(250));
        for value in &mut self.luma {
            let value_u32 = u32::from(*value);
            *value = if value_u32 <= black {
                0
            } else {
                ((value_u32 - black) * 255 / (255 - black)) as u8
            };
        }
    }
}

/// A luminance pyramid whose reduction keeps small bright features (city
/// lights) visible when the map is shown smaller than its source resolution:
/// each parent texel blends the mean and the maximum of its 2x2 children.
#[derive(Debug, Clone)]
pub struct MipGrid {
    levels: Vec<GeoGrid>,
}

impl MipGrid {
    pub fn build(base: GeoGrid) -> Self {
        let mut levels = vec![base];
        while let Some(last) = levels.last() {
            if last.width < 64 || last.height < 32 {
                break;
            }
            let (width, height) = (last.width.div_ceil(2), last.height.div_ceil(2));
            let mut next = GeoGrid::blank(last.bbox, width, height, false);
            for y in 0..height {
                for x in 0..width {
                    let mut sum = 0u32;
                    let mut peak = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(last.width - 1);
                        let sy = (y * 2 + dy).min(last.height - 1);
                        let value = u32::from(last.luma[sy * last.width + sx]);
                        sum += value;
                        peak = peak.max(value);
                    }
                    let mean = sum as f32 / 4.0;
                    next.luma[y * width + x] = (0.4 * mean + 0.6 * peak as f32).round() as u8;
                }
            }
            levels.push(next);
        }
        Self { levels }
    }

    pub fn base(&self) -> &GeoGrid {
        &self.levels[0]
    }

    #[cfg(test)]
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    /// Luminance 0..=1 at a point when one screen dot spans `texels_per_dot`
    /// base texels (trilinear between pyramid levels).
    pub fn sample(&self, lon: f64, lat: f64, texels_per_dot: f32) -> Option<f32> {
        let detail = texels_per_dot.max(1.0).log2();
        let low = (detail.floor() as usize).min(self.levels.len() - 1);
        let high = (low + 1).min(self.levels.len() - 1);
        let blend = if high == low {
            0.0
        } else {
            detail - low as f32
        };
        let near = self.levels[low].sample_luma(lon, lat)?;
        if blend <= 0.01 {
            return Some(near);
        }
        let far = self.levels[high].sample_luma(lon, lat)?;
        Some(near * (1.0 - blend.clamp(0.0, 1.0)) + far * blend.clamp(0.0, 1.0))
    }
}

// ---------------------------------------------------------------------------
// Mosaic assembly and download

/// Tiles of one GIBS level, indexed for fast lookup while assembling.
struct TileCanvas<'a> {
    level: u8,
    columns: u32,
    rows: u32,
    tiles: Vec<Option<&'a DecodedImage>>,
}

impl<'a> TileCanvas<'a> {
    fn new(level: u8, tiles: &'a HashMap<TileId, DecodedImage>) -> Self {
        let (columns, rows) = matrix_size(level);
        let mut indexed = vec![None; (columns * rows) as usize];
        for (id, image) in tiles {
            if id.level == level && id.col < columns && id.row < rows {
                indexed[(id.row * columns + id.col) as usize] = Some(image);
            }
        }
        Self {
            level,
            columns,
            rows,
            tiles: indexed,
        }
    }

    /// Texel at global pixel (x, y) of the level; missing tiles are black.
    fn texel(&self, x: i64, y: i64) -> Option<(&'a DecodedImage, usize)> {
        let max_x = i64::from(self.columns) * TILE_PIXELS as i64 - 1;
        let max_y = i64::from(self.rows) * TILE_PIXELS as i64 - 1;
        let x = x.clamp(0, max_x) as usize;
        let y = y.clamp(0, max_y) as usize;
        let tile = self.tiles[(y / TILE_PIXELS) * self.columns as usize + x / TILE_PIXELS]?;
        let local_x = (x % TILE_PIXELS).min(tile.width.saturating_sub(1));
        let local_y = (y % TILE_PIXELS).min(tile.height.saturating_sub(1));
        Some((tile, local_y * tile.width + local_x))
    }

    fn level(&self) -> u8 {
        self.level
    }
}

/// Resample downloaded GIBS tiles of one `level` into a grid registered to
/// `bbox` (longitudes beyond +-180 wrap). Missing tiles stay black.
pub fn grid_from_tiles(
    bbox: GeoBox,
    width: usize,
    height: usize,
    level: u8,
    tiles: &HashMap<TileId, DecodedImage>,
    want_rgb: bool,
) -> GeoGrid {
    let canvas = TileCanvas::new(level, tiles);
    let span = tile_span_degrees(canvas.level());
    let pixels_per_degree = TILE_PIXELS as f64 / span;
    let mut grid = GeoGrid::blank(bbox, width, height, want_rgb);
    for y in 0..grid.height {
        let lat = bbox.north - (y as f64 + 0.5) / grid.height as f64 * bbox.height_degrees();
        let source_y = (90.0 - lat) * pixels_per_degree - 0.5;
        let (y0, ty) = (source_y.floor(), (source_y - source_y.floor()) as f32);
        for x in 0..grid.width {
            let lon = bbox.west + (x as f64 + 0.5) / grid.width as f64 * bbox.width_degrees();
            let source_x = (lon + 180.0).rem_euclid(360.0) * pixels_per_degree - 0.5;
            let (x0, tx) = (source_x.floor(), (source_x - source_x.floor()) as f32);
            let taps = [
                (x0 as i64, y0 as i64, (1.0 - tx) * (1.0 - ty)),
                (x0 as i64 + 1, y0 as i64, tx * (1.0 - ty)),
                (x0 as i64, y0 as i64 + 1, (1.0 - tx) * ty),
                (x0 as i64 + 1, y0 as i64 + 1, tx * ty),
            ];
            let mut luma = 0.0f32;
            let mut color = [0.0f32; 3];
            for (tap_x, tap_y, weight) in taps {
                let Some((tile, index)) = canvas.texel(tap_x, tap_y) else {
                    continue;
                };
                luma += weight * f32::from(tile.luma[index]);
                if let Some(rgb) = tile.rgb.as_ref() {
                    for (channel, value) in color.iter_mut().zip(rgb[index]) {
                        *channel += weight * f32::from(value);
                    }
                }
            }
            let index = y * grid.width + x;
            grid.luma[index] = luma.round().clamp(0.0, 255.0) as u8;
            if let Some(rgb) = grid.rgb.as_mut() {
                rgb[index] = color.map(|value| value.round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    grid
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    Stopped,
    Network(String),
}

pub struct TileDownload {
    pub tiles: HashMap<TileId, DecodedImage>,
    pub missing: usize,
}

/// Download and decode `ids` one after another (the host throttles requests).
/// Three consecutive transport failures, or a list in which every tile failed
/// on the network, abort with `Network`; 404s and
/// undecodable tiles are counted as missing. `on_progress(done, total)` runs
/// after every tile.
pub fn download_tiles(
    fetcher: &dyn TileFetcher,
    cache_dir: &Path,
    ids: &[TileId],
    url_of: &dyn Fn(TileId) -> String,
    keep_rgb: bool,
    stop: &AtomicBool,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<TileDownload, DownloadError> {
    let mut tiles = HashMap::new();
    let mut missing = 0;
    let mut consecutive_failures = 0;
    let mut network_failures = 0;
    let mut last_network_message = String::new();
    for (done, id) in ids.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return Err(DownloadError::Stopped);
        }
        match fetcher.fetch(cache_dir, &url_of(*id), IMMUTABLE_AGE, stop) {
            Ok(bytes) => {
                consecutive_failures = 0;
                match decode_image(&bytes, keep_rgb) {
                    Ok(image) => {
                        tiles.insert(*id, image);
                    }
                    Err(_) => missing += 1,
                }
            }
            Err(TileError::Missing) => {
                consecutive_failures = 0;
                missing += 1;
            }
            Err(TileError::Stopped) => return Err(DownloadError::Stopped),
            Err(TileError::Network(message)) => {
                consecutive_failures += 1;
                network_failures += 1;
                missing += 1;
                if consecutive_failures >= 3 {
                    return Err(DownloadError::Network(message));
                }
                last_network_message = message;
            }
        }
        on_progress(done + 1, ids.len());
    }
    // Every tile failed on the network (a short list never reaches three in a row).
    if tiles.is_empty() && network_failures > 0 && network_failures == ids.len() {
        return Err(DownloadError::Network(last_network_message));
    }
    Ok(TileDownload { tiles, missing })
}

/// Encode a grayscale PNG (used to persist assembled mosaics).
pub fn encode_gray_png(grid: &GeoGrid) -> Result<Vec<u8>, String> {
    let image =
        image::GrayImage::from_raw(grid.width as u32, grid.height as u32, grid.luma.clone())
            .ok_or_else(|| "grid size does not match its data".to_owned())?;
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

/// Write `bytes` to `path` atomically (temporary file + rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::sync::Mutex;

    /// PNG bytes of a solid RGBA image.
    pub fn solid_png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    /// PNG of a horizontal ramp with an optional bright spot, for map tests.
    pub fn gray_png(width: u32, height: u32, value: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let image = image::GrayImage::from_fn(width, height, |x, y| image::Luma([value(x, y)]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    /// Answers one request: gets the URL and the stop flag.
    pub type Responder = Box<dyn Fn(&str, &AtomicBool) -> Result<Vec<u8>, TileError> + Send + Sync>;

    /// A fetcher backed by a closure; records every requested URL.
    pub struct FakeFetcher {
        pub requests: Mutex<Vec<String>>,
        pub respond: Responder,
    }

    impl FakeFetcher {
        pub fn new(
            respond: impl Fn(&str, &AtomicBool) -> Result<Vec<u8>, TileError> + Send + Sync + 'static,
        ) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                respond: Box::new(respond),
            }
        }

        pub fn urls(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl TileFetcher for FakeFetcher {
        fn fetch(
            &self,
            _cache_dir: &Path,
            url: &str,
            _max_age: Duration,
            stop: &AtomicBool,
        ) -> Result<Vec<u8>, TileError> {
            self.requests.lock().unwrap().push(url.to_owned());
            (self.respond)(url, stop)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn civil_time_round_trips_and_formats() {
        let time = UtcTime::from_civil(2026, 9, 30, 13, 40, 24);
        assert_eq!(time.0, 1_790_775_624);
        assert_eq!(time.iso_seconds(), "2026-09-30T13:40:24Z");
        assert_eq!(time.date(), "2026-09-30");
        assert_eq!(time.label(), "2026-09-30 13:40 UTC");
        assert_eq!(UtcTime(0).iso_seconds(), "1970-01-01T00:00:00Z");
        // Leap day and a pre-epoch instant.
        let leap = UtcTime::from_civil(2024, 2, 29, 23, 59, 59);
        assert_eq!(leap.civil(), (2024, 2, 29, 23, 59, 59));
        assert_eq!(UtcTime(-1).civil(), (1969, 12, 31, 23, 59, 59));
    }

    #[test]
    fn time_parsing_accepts_gibs_and_wms_spellings() {
        let expected = UtcTime::from_civil(2026, 9, 30, 12, 50, 0);
        assert_eq!(UtcTime::parse("2026-09-30T12:50:00Z"), Some(expected));
        assert_eq!(UtcTime::parse("2026-09-30T12:50:00.000Z"), Some(expected));
        assert_eq!(UtcTime::parse("2026-09-30T12:50Z"), Some(expected));
        assert_eq!(
            UtcTime::parse("2026-09-30"),
            Some(UtcTime::from_civil(2026, 9, 30, 0, 0, 0))
        );
        assert_eq!(UtcTime::parse("2026-13-30"), None);
        assert_eq!(UtcTime::parse("garbage"), None);
        assert_eq!(UtcTime::parse("2026-09-30T25:00:00Z"), None);
    }

    #[test]
    fn periods_and_ranges_expand() {
        assert_eq!(parse_period_seconds("PT10M"), Some(600));
        assert_eq!(parse_period_seconds("PT3H"), Some(10_800));
        assert_eq!(parse_period_seconds("P1D"), Some(86_400));
        assert_eq!(parse_period_seconds("PT1H30M"), Some(5400));
        assert_eq!(parse_period_seconds("10M"), None);
        let times = expand_time_ranges(
            "2026-09-30T09:00:00Z/2026-09-30T09:30:00Z/PT10M,2026-09-30T10:00:00Z",
            100,
        );
        let labels: Vec<String> = times.iter().map(|time| time.label()).collect();
        assert_eq!(
            labels,
            [
                "2026-09-30 09:00 UTC",
                "2026-09-30 09:10 UTC",
                "2026-09-30 09:20 UTC",
                "2026-09-30 09:30 UTC",
                "2026-09-30 10:00 UTC"
            ]
        );
        assert_eq!(
            expand_time_ranges("2026-01-01/2027-01-01/PT1S", 10).len(),
            10
        );
    }

    #[test]
    fn floor_to_aligns_to_epoch_multiples() {
        let time = UtcTime::from_civil(2026, 9, 30, 12, 47, 12);
        assert_eq!(time.floor_to(1200).label(), "2026-09-30 12:40 UTC");
        assert_eq!(time.start_of_day().iso_seconds(), "2026-09-30T00:00:00Z");
    }

    #[test]
    fn gibs_matrix_matches_the_published_capabilities() {
        // From GIBS WMTSCapabilities.xml (epsg4326, TileMatrixSet 500m/250m).
        let expected = [
            (0, (2, 1)),
            (1, (3, 2)),
            (2, (5, 3)),
            (3, (10, 5)),
            (4, (20, 10)),
            (5, (40, 20)),
            (6, (80, 40)),
            (7, (160, 80)),
            (8, (320, 160)),
        ];
        for (level, size) in expected {
            assert_eq!(matrix_size(level), size, "level {level}");
        }
        assert!((tile_span_degrees(0) - 288.0).abs() < 1e-9);
        assert!((pixels_per_degree(2) - 512.0 / 72.0).abs() < 1e-9);
    }

    #[test]
    fn lon_lat_and_tiles_are_consistent() {
        let id = lon_lat_to_tile(3, -10.0, 5.0);
        assert_eq!(
            id,
            TileId {
                level: 3,
                col: 4,
                row: 2
            }
        );
        let bounds = tile_bounds(id);
        assert_eq!(
            (bounds.west, bounds.east, bounds.north, bounds.south),
            (-36.0, 0.0, 18.0, -18.0)
        );
        assert!(bounds.west <= -10.0 && -10.0 < bounds.east);
        // Longitude wraps, latitude clamps.
        assert_eq!(
            lon_lat_to_tile(3, 350.0, 0.0).col,
            lon_lat_to_tile(3, -10.0, 0.0).col
        );
        assert_eq!(lon_lat_to_tile(3, 0.0, 95.0).row, 0);
        assert_eq!(lon_lat_to_tile(3, 0.0, -95.0).row, 4);
        // Corners of the world.
        assert_eq!(
            lon_lat_to_tile(2, -180.0, 90.0),
            TileId {
                level: 2,
                col: 0,
                row: 0
            }
        );
        assert_eq!(
            lon_lat_to_tile(2, 179.99, -89.99),
            TileId {
                level: 2,
                col: 4,
                row: 2
            }
        );
    }

    #[test]
    fn tile_cover_of_a_box_is_complete_and_bounded() {
        let world = tiles_for_box(2, &GeoBox::WORLD);
        assert_eq!(world.len(), 15);
        let local = GeoBox {
            west: -20.0,
            south: 30.0,
            east: 20.0,
            north: 60.0,
        };
        let tiles = tiles_for_box(3, &local);
        // 36 degree tiles: columns 4,5 and rows 1,2.
        assert_eq!(tiles.len(), 4);
        for corner in [(-20.0, 60.0), (20.0, 30.0), (-20.0, 30.0), (20.0, 60.0)] {
            assert!(tiles.contains(&lon_lat_to_tile(3, corner.0, corner.1)));
        }
        // Across the antimeridian the covering wraps to both edge columns.
        let dateline = GeoBox {
            west: 170.0,
            south: -10.0,
            east: 190.0,
            north: 10.0,
        };
        let columns: Vec<u32> = tiles_for_box(3, &dateline)
            .iter()
            .map(|tile| tile.col)
            .collect();
        assert!(columns.contains(&9) && columns.contains(&0));
        assert_eq!(level_within_budget(4, &GeoBox::WORLD, 50), 3);
        assert_eq!(level_within_budget(3, &GeoBox::WORLD, 1000), 3);
        assert_eq!(level_for_density(3.0, 8), 1);
        assert_eq!(level_for_density(100.0, 4), 4);
    }

    #[test]
    fn urls_follow_the_documented_templates() {
        let id = TileId {
            level: 3,
            col: 4,
            row: 2,
        };
        assert_eq!(
            gibs_tile_url("GOES-East_ABI_GeoColor", Some("2026-09-30T12:50:00Z"), "1km", id, "png"),
            "https://gibs.earthdata.nasa.gov/wmts/epsg4326/best/GOES-East_ABI_GeoColor/default/2026-09-30T12:50:00Z/1km/3/2/4.png"
        );
        assert_eq!(
            gibs_tile_url("BlueMarble_NextGeneration", None, "500m", id, "jpeg"),
            "https://gibs.earthdata.nasa.gov/wmts/epsg4326/best/BlueMarble_NextGeneration/default/500m/3/2/4.jpeg"
        );
        let start = UtcTime::from_civil(2026, 9, 30, 9, 0, 0);
        let end = UtcTime::from_civil(2026, 9, 30, 14, 0, 0);
        assert_eq!(
            gibs_domains_url("GOES-East_ABI_GeoColor", "1km", start, end),
            "https://gibs.earthdata.nasa.gov/wmts/epsg4326/best/1.0.0/GOES-East_ABI_GeoColor/default/1km/all/2026-09-30T09:00:00Z--2026-09-30T14:00:00Z.xml"
        );
        let url = wms_map_url(
            "msg_fes:ir108",
            &GeoBox {
                west: -30.0,
                south: 25.0,
                east: 30.0,
                north: 65.0,
            },
            720,
            480,
            Some(UtcTime::from_civil(2026, 9, 30, 13, 15, 0)),
        );
        assert_eq!(
            url,
            "https://view.eumetsat.int/geoserver/ows?service=WMS&version=1.3.0&request=GetMap&layers=msg_fes:ir108&styles=&crs=CRS:84&bbox=-30.0000,25.0000,30.0000,65.0000&width=720&height=480&format=image/png&transparent=true&time=2026-09-30T13:15:00Z"
        );
        assert_eq!(
            wms_capabilities_url("msg_fes"),
            "https://view.eumetsat.int/geoserver/msg_fes/ows?service=WMS&version=1.3.0&request=GetCapabilities"
        );
    }

    #[test]
    fn domains_and_capabilities_are_parsed() {
        let domains = "<Domains><DimensionDomain><ows:Identifier>time</ows:Identifier><Domain>2026-09-30T09:00:00Z/2026-09-30T09:20:00Z/PT10M</Domain><Size>1</Size></DimensionDomain></Domains>";
        let times = parse_domains(domains);
        assert_eq!(times.len(), 3);
        assert_eq!(times[2].label(), "2026-09-30 09:20 UTC");
        assert!(parse_domains("<html>nope</html>").is_empty());
        let capabilities = r#"<Layer><Name>msg_fes:ir108</Name><Title>x</Title><Dimension name="time" default="2026-09-30T13:15:00Z" units="ISO8601" nearestValue="1">2020-09-01T00:00:00.000Z/2026-09-30T13:15:00.000Z/PT15M</Dimension><Style><Name>raster</Name></Style></Layer><Layer><Name>msg_fes:vis006</Name><Dimension name="time" default="2026-09-30T13:00:00Z">x</Dimension></Layer>"#;
        assert_eq!(
            wms_default_time(capabilities, "msg_fes:ir108")
                .unwrap()
                .label(),
            "2026-09-30 13:15 UTC"
        );
        assert_eq!(
            wms_default_time(capabilities, "msg_fes:vis006")
                .unwrap()
                .label(),
            "2026-09-30 13:00 UTC"
        );
        assert!(wms_default_time(capabilities, "msg_fes:none").is_none());
        // Workspace-specific capabilities (what the service really returns for
        // .../msg_fes/ows) use unprefixed layer names.
        let workspace = r#"<Layer><Name>clm</Name><Title>c</Title></Layer><Layer queryable="1"><Name>ir108</Name><Title>x</Title><Dimension name="time" default="2026-09-30T13:15:00Z" units="ISO8601" nearestValue="1">a/b/PT15M</Dimension></Layer>"#;
        assert_eq!(
            wms_default_time(workspace, "msg_fes:ir108")
                .unwrap()
                .label(),
            "2026-09-30 13:15 UTC"
        );
        assert!(wms_default_time(workspace, "msg_fes:vis006").is_none());
    }

    #[test]
    fn decoding_maps_transparency_to_black_and_keeps_colour_on_request() {
        let opaque = decode_image(&solid_png(4, 2, [200, 100, 50, 255]), true).unwrap();
        assert_eq!((opaque.width, opaque.height), (4, 2));
        assert_eq!(
            opaque.luma[0],
            ((299 * 200 + 587 * 100 + 114 * 50) / 1000) as u8
        );
        assert_eq!(opaque.rgb.as_ref().unwrap()[0], [200, 100, 50]);
        let clear = decode_image(&solid_png(2, 2, [255, 255, 255, 0]), false).unwrap();
        assert!(clear.luma.iter().all(|value| *value == 0));
        assert!(clear.rgb.is_none());
        assert!(decode_image(b"not an image", false).is_err());
    }

    fn synthetic_level1_tiles(value_of: impl Fn(u32, u32) -> u8) -> HashMap<TileId, DecodedImage> {
        let mut tiles = HashMap::new();
        for row in 0..2 {
            for col in 0..3 {
                let value = value_of(col, row);
                let png = solid_png(512, 512, [value, value, value, 255]);
                tiles.insert(
                    TileId { level: 1, col, row },
                    decode_image(&png, false).unwrap(),
                );
            }
        }
        tiles
    }

    #[test]
    fn mosaic_assembly_places_every_tile_at_its_geographic_position() {
        // Tile value = 20 + 30 * (row * 3 + col).
        let tiles = synthetic_level1_tiles(|col, row| (20 + 30 * (row * 3 + col)) as u8);
        let grid = grid_from_tiles(GeoBox::WORLD, 360, 180, 1, &tiles, false);
        assert_eq!((grid.width, grid.height), (360, 180));
        // Level 1: 144 degree tiles. lon -100 -> col 0; lon 0 -> col 1; lon 150 -> col 2.
        let sample =
            |lon: f64, lat: f64| (grid.sample_luma(lon, lat).unwrap() * 255.0).round() as u32;
        assert_eq!(sample(-100.0, 60.0), 20); // row 0 (north of 90-144=-54), col 0
        assert_eq!(sample(0.0, 60.0), 50);
        assert_eq!(sample(150.0, 60.0), 80);
        assert_eq!(sample(-100.0, -70.0), 110); // row 1
        assert_eq!(sample(150.0, -70.0), 170);
    }

    #[test]
    fn mosaic_assembly_reproduces_pixels_and_leaves_missing_tiles_black() {
        let mut tiles = HashMap::new();
        // One tile at level 1 (col 1,row 0): left half dark, right half bright.
        let png = gray_png(512, 512, |x, _| if x < 256 { 10 } else { 250 });
        tiles.insert(
            TileId {
                level: 1,
                col: 1,
                row: 0,
            },
            decode_image(&png, false).unwrap(),
        );
        // Tile (1,0) spans lon -36..108, lat 90..-54.
        let grid = grid_from_tiles(GeoBox::WORLD, 1280, 640, 1, &tiles, false);
        let left = grid.sample_luma(-20.0, 30.0).unwrap();
        let right = grid.sample_luma(90.0, 30.0).unwrap();
        assert!(left < 0.06 && right > 0.9, "left {left} right {right}");
        assert_eq!(
            grid.sample_luma(-100.0, 30.0).unwrap(),
            0.0,
            "missing tile is black"
        );
        assert_eq!(
            grid.sample_luma(0.0, -80.0).unwrap(),
            0.0,
            "missing row is black"
        );
    }

    #[test]
    fn grid_assembly_wraps_across_the_antimeridian_and_keeps_colour() {
        let mut tiles = HashMap::new();
        for col in 0..10 {
            for row in 0..5 {
                let colour = if col == 9 {
                    [255, 0, 0, 255]
                } else if col == 0 {
                    [0, 0, 255, 255]
                } else {
                    [40, 40, 40, 255]
                };
                tiles.insert(
                    TileId { level: 3, col, row },
                    decode_image(&solid_png(512, 512, colour), true).unwrap(),
                );
            }
        }
        let bbox = GeoBox {
            west: 160.0,
            south: -10.0,
            east: 200.0,
            north: 10.0,
        };
        let grid = grid_from_tiles(bbox, 80, 20, 3, &tiles, true);
        let west_half = grid.sample_rgb(165.0, 0.0).unwrap();
        let east_half = grid.sample_rgb(195.0, 0.0).unwrap();
        assert!(
            west_half[0] > 0.9 && west_half[2] < 0.1,
            "col 9 is red {west_half:?}"
        );
        assert!(
            east_half[2] > 0.9 && east_half[0] < 0.1,
            "col 0 is blue {east_half:?}"
        );
        assert!(grid.sample_luma(100.0, 0.0).is_none(), "outside the box");
    }

    #[test]
    fn grid_sampling_is_bilinear_and_wraps_only_for_world_grids() {
        let mut grid = GeoGrid::blank(GeoBox::WORLD, 4, 2, false);
        grid.luma = vec![0, 255, 255, 0, 0, 255, 255, 0];
        let middle = grid.sample_luma(-90.0, 45.0).unwrap();
        assert!((middle - 0.5).abs() < 0.01, "{middle}");
        // Wrap: longitude 270 equals -90.
        assert_eq!(grid.sample_luma(270.0, 45.0), grid.sample_luma(-90.0, 45.0));
        let local = GeoGrid::blank(
            GeoBox {
                west: 0.0,
                south: 0.0,
                east: 10.0,
                north: 10.0,
            },
            4,
            4,
            false,
        );
        assert!(local.sample_luma(20.0, 5.0).is_none());
        assert!(local.sample_luma(5.0, 20.0).is_none());
        assert!(local.sample_luma(5.0, 5.0).is_some());
    }

    #[test]
    fn black_level_is_subtracted_and_stretched() {
        let mut grid = GeoGrid::blank(GeoBox::WORLD, 4, 1, false);
        grid.luma = vec![5, 30, 130, 255];
        grid.apply_black_level(30);
        assert_eq!(grid.luma, vec![0, 0, (100u32 * 255 / 225) as u8, 255]);
    }

    #[test]
    fn mip_reduction_keeps_single_bright_texels_visible() {
        let mut base = GeoGrid::blank(GeoBox::WORLD, 256, 128, false);
        base.luma[64 * 256 + 128] = 255;
        let plain_mean = 255.0 / 4.0;
        let mip = MipGrid::build(base);
        assert!(mip.level_count() >= 3);
        let lit = mip.sample(0.703_125, -0.703_125, 1.0).unwrap();
        assert!(lit > 0.9);
        let coarse = mip.sample(0.703_125, -0.703_125, 4.0).unwrap();
        // A plain box filter would give 255 / 16; the soft maximum keeps far more.
        assert!(coarse * 255.0 > plain_mean * 0.6, "coarse {coarse}");
    }

    #[test]
    fn prune_removes_oldest_files_until_under_budget() {
        let directory = tempfile::tempdir().unwrap();
        for (index, name) in ["a.png", "b.png", "c.png"].iter().enumerate() {
            let path = directory.path().join(name);
            std::fs::write(&path, vec![0u8; 100]).unwrap();
            let time = SystemTime::now() - Duration::from_secs(1000 - 100 * index as u64);
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(time)
                .unwrap();
        }
        std::fs::create_dir(directory.path().join("keep_dir")).unwrap();
        let removed = prune_cache(directory.path(), 150);
        assert_eq!(removed, 200);
        assert!(!directory.path().join("a.png").exists());
        assert!(!directory.path().join("b.png").exists());
        assert!(directory.path().join("c.png").exists());
        assert!(directory.path().join("keep_dir").exists());
        assert_eq!(prune_cache(&directory.path().join("missing"), 0), 0);
    }

    #[test]
    fn download_reports_progress_counts_missing_and_aborts_on_repeated_failure() {
        let ids: Vec<TileId> = (0..5)
            .map(|col| TileId {
                level: 1,
                col,
                row: 0,
            })
            .collect();
        let fetcher = FakeFetcher::new(|url, _| {
            if url.ends_with("/1.png") {
                Err(TileError::Missing)
            } else if url.ends_with("/2.png") {
                Ok(b"garbage".to_vec())
            } else {
                Ok(solid_png(2, 2, [9, 9, 9, 255]))
            }
        });
        let stop = AtomicBool::new(false);
        let mut progress = Vec::new();
        let result = download_tiles(
            &fetcher,
            Path::new("/nonexistent"),
            &ids,
            &|id| format!("https://example.test/{}.png", id.col),
            false,
            &stop,
            &mut |done, total| progress.push((done, total)),
        )
        .unwrap();
        assert_eq!(result.tiles.len(), 3);
        assert_eq!(result.missing, 2);
        assert_eq!(progress.last(), Some(&(5, 5)));

        let failing = FakeFetcher::new(|_, _| Err(TileError::Network("offline".into())));
        let error = download_tiles(
            &failing,
            Path::new("/x"),
            &ids,
            &|_| "https://example.test/x".into(),
            false,
            &stop,
            &mut |_, _| {},
        )
        .err()
        .unwrap();
        assert_eq!(error, DownloadError::Network("offline".into()));
        assert_eq!(
            failing.urls().len(),
            3,
            "stops after three consecutive failures"
        );

        stop.store(true, Ordering::Relaxed);
        let stopped = download_tiles(
            &fetcher,
            Path::new("/x"),
            &ids,
            &|_| "https://example.test/x".into(),
            false,
            &stop,
            &mut |_, _| {},
        );
        assert_eq!(stopped.err(), Some(DownloadError::Stopped));
    }

    #[test]
    fn gray_png_round_trip_and_atomic_write() {
        let mut grid = GeoGrid::blank(GeoBox::WORLD, 8, 4, false);
        for (index, value) in grid.luma.iter_mut().enumerate() {
            *value = (index * 8) as u8;
        }
        let bytes = encode_gray_png(&grid).unwrap();
        let decoded = decode_image(&bytes, false).unwrap();
        assert_eq!(decoded.luma, grid.luma);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/dir/mosaic.png");
        write_atomic(&path, &bytes).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn service_exceptions_are_not_mistaken_for_images() {
        assert!(looks_like_image(&solid_png(2, 2, [1, 2, 3, 255])));
        assert!(looks_like_image(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10]));
        assert!(!looks_like_image(
            b"<?xml version=\"1.0\"?><ServiceExceptionReport/>"
        ));
        assert!(!looks_like_image(b""));
    }

    #[test]
    fn a_cached_service_exception_is_dropped_and_reported_missing() {
        // The fetcher validates what fetch_cached returns; a fresh cache entry
        // holding XML under an image name (a poisoned WMS answer) is removed.
        let directory = tempfile::tempdir().unwrap();
        let url = "https://example.invalid/wms?format=image/png&time=2026-09-30T13:15:00Z";
        let path = directory.path().join(source::cache_file_name(url, "png"));
        std::fs::write(&path, b"<ServiceExceptionReport/>").unwrap();
        let stop = AtomicBool::new(false);
        let result = HttpFetcher.fetch(directory.path(), url, IMMUTABLE_AGE, &stop);
        assert_eq!(result, Err(TileError::Missing));
        assert!(
            !path.exists(),
            "poisoned entry removed so the next try can succeed"
        );
        // A genuine image in the cache is served untouched.
        let image = solid_png(2, 2, [9, 9, 9, 255]);
        std::fs::write(&path, &image).unwrap();
        assert_eq!(
            HttpFetcher.fetch(directory.path(), url, IMMUTABLE_AGE, &stop),
            Ok(image)
        );
    }

    #[test]
    fn http_errors_are_classified() {
        assert_eq!(
            TileError::from(FetchError::Request("http status: 404".into())),
            TileError::Missing
        );
        assert!(matches!(
            TileError::from(FetchError::Request("connection refused".into())),
            TileError::Network(_)
        ));
    }
}
