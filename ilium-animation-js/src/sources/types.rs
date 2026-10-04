//! Bounded source contracts. Provider time and local capture time remain distinct.
use crate::error::{AnimationError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) fn fail<T>(message: &str) -> Result<T> {
    Err(AnimationError::Runtime(format!("sources: {message}")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeriesProvider {
    Crypto,
    Ecb,
    NoaaSolarWind,
    Iss,
    Usgs,
    Wikipedia,
    DrandQuicknet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesOptions {
    pub provider: SeriesProvider,
    #[serde(default)]
    pub source_id: Option<String>,
    pub max_samples: usize,
    pub interval_ms: u64,
    #[serde(default = "default_window")]
    pub window_minutes: i32,
}
fn default_window() -> i32 {
    1440
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeographicBounds {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}
impl GeographicBounds {
    pub fn validate(self) -> Result<()> {
        if ![self.west, self.east, self.south, self.north]
            .into_iter()
            .all(f64::is_finite)
            || !(-180.0..=180.0).contains(&self.west)
            || !(-180.0..=180.0).contains(&self.east)
            || !(-90.0..=90.0).contains(&self.south)
            || !(-90.0..=90.0).contains(&self.north)
            || self.south > self.north
        {
            return fail("invalid geographic bounds");
        }
        Ok(())
    }
    pub fn contains(self, latitude: f64, longitude: f64) -> bool {
        latitude >= self.south
            && latitude <= self.north
            && if self.west <= self.east {
                longitude >= self.west && longitude <= self.east
            } else {
                longitude >= self.west || longitude <= self.east
            }
    }
}

pub const FIELD_NAMES: &[&str] = &[
    "id",
    "latitude",
    "longitude",
    "epoch_ms",
    "label",
    "magnitude",
    "depth_km",
    "depth",
    "altitude_m",
    "altitude",
    "heading_degrees",
    "heading",
    "speed_mps",
    "speed",
    "callsign",
    "vessel_type",
];
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoatProvider {
    Openseafeed,
    Digitraffic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeoOptions {
    pub bounds: GeographicBounds,
    pub max_entities: usize,
    pub max_hz: f64,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub credential: Option<String>,
    #[serde(default)]
    pub provider: Option<BoatProvider>,
}
impl GeoOptions {
    pub fn requests_field(&self, canonical: &str) -> bool {
        self.fields.iter().any(|field| {
            field == canonical
                || matches!(
                    (canonical, field.as_str()),
                    ("depth_km", "depth")
                        | ("altitude_m", "altitude")
                        | ("heading_degrees", "heading")
                        | ("speed_mps", "speed")
                )
        })
    }

    pub fn validate(&self) -> Result<()> {
        self.bounds.validate()?;
        if !(1..=4096).contains(&self.max_entities)
            || !self.max_hz.is_finite()
            || !(0.001..=60.0).contains(&self.max_hz)
            || self.fields.len() > 16
            || self
                .fields
                .iter()
                .any(|field| !FIELD_NAMES.contains(&field.as_str()))
            || self.credential.as_ref().is_some_and(|id| {
                id.is_empty() || id.len() > 128 || id.chars().any(char::is_control)
            })
        {
            return fail("invalid geographic demand");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChessOptions {
    pub game_id: String,
    pub max_hz: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeatherOptions {
    #[serde(flatten)]
    pub geographic: GeoOptions,
    pub layers: Vec<String>,
    #[serde(default = "default_image_size")]
    pub image_width: usize,
    #[serde(default = "default_image_size")]
    pub image_height: usize,
    #[serde(default = "default_frames")]
    pub max_frames: usize,
    #[serde(default = "default_tiles")]
    pub max_tiles: usize,
    #[serde(default)]
    pub history_hours: i64,
    #[serde(default)]
    pub anchor_epoch_ms: Option<i64>,
}
fn default_image_size() -> usize {
    512
}
fn default_frames() -> usize {
    1
}
fn default_tiles() -> usize {
    9
}
impl WeatherOptions {
    pub fn validate(&self) -> Result<()> {
        self.geographic.validate()?;
        if self.geographic.provider.is_some()
            || self.layers.is_empty()
            || self.layers.len() > 4
            || self
                .layers
                .iter()
                .any(|layer| !super::astronomy::weather_layer(layer))
            || self.image_width == 0
            || self.image_height == 0
            || self.image_width > 4096
            || self.image_height > 4096
            || self
                .image_width
                .checked_mul(self.image_height)
                .is_none_or(|pixels| pixels > 1024 * 1024)
            || !(1..=24).contains(&self.max_frames)
            || !(2..=50).contains(&self.max_tiles)
            || !(0..=168).contains(&self.history_hours)
        {
            return fail("invalid weather demand");
        }
        // Keep the complete response, all frames and layers, below a declared
        // decoded pixel ceiling. The broker still charges actual image handles.
        let images = self
            .layers
            .len()
            .checked_mul(self.max_frames)
            .and_then(|count| count.checked_mul(self.max_tiles))
            .ok_or_else(|| AnimationError::Runtime("weather image count overflow".into()))?;
        if images > 216
            || images
                .checked_mul(512 * 512)
                .is_none_or(|pixels| pixels > 64 * 1024 * 1024)
        {
            return fail("weather image budget");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "family", content = "options", rename_all = "snake_case")]
pub enum SourceDemand {
    Series(SeriesOptions),
    Earthquakes(GeoOptions),
    Aircraft(GeoOptions),
    Boats(GeoOptions),
    Chess(ChessOptions),
    Weather(WeatherOptions),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", content = "options", rename_all = "snake_case")]
pub enum SourceRequest {
    GeographyCoastlines {
        body: String,
        bounds: GeographicBounds,
        max_points: usize,
        #[serde(default)]
        seed: Option<u64>,
    },
    GeographyElevation {
        body: String,
        bounds: GeographicBounds,
        width: usize,
        height: usize,
        #[serde(default)]
        seed: Option<u32>,
    },
    GeographyProject {
        latitude: f64,
        longitude: f64,
        projection: String,
    },
    ChessDiscover {
        max_games: usize,
    },
    WikipediaSearch {
        query: String,
        max_results: usize,
    },
    WikipediaArticle {
        title: String,
        max_bytes: usize,
        max_images: usize,
    },
    OsmGeocode {
        query: String,
        max_results: usize,
    },
    OsmTile {
        x: u32,
        y: u32,
        zoom: u8,
        format: String,
    },
    AstronomyCatalogue {
        name: String,
        max_stars: usize,
    },
    AstronomyObserve {
        epoch_ms: i64,
        latitude: f64,
        longitude: f64,
    },
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SnapshotMetadata {
    pub revision: u64,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_ms: Option<u64>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeriesPoint {
    pub epoch_ms: i64,
    pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub low: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct SeriesSnapshot {
    #[serde(flatten)]
    pub metadata: SnapshotMetadata,
    pub provider: SeriesProvider,
    pub source_id: String,
    pub points: Vec<SeriesPoint>,
    pub verification: String,
    pub attribution: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeoEntity {
    pub id: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magnitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_km: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altitude_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading_degrees: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_mps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vessel_type: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct GeoSnapshot {
    #[serde(flatten)]
    pub metadata: SnapshotMetadata,
    pub entities: Vec<GeoEntity>,
    pub attribution: String,
    pub rejected: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ChessSnapshot {
    #[serde(flatten)]
    pub metadata: SnapshotMetadata,
    pub game_id: String,
    pub fen: String,
    pub moves: Vec<String>,
    pub white: String,
    pub black: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub white_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub black_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SourceSnapshot {
    Series(SeriesSnapshot),
    Geographic(GeoSnapshot),
    Chess(ChessSnapshot),
    Weather(WeatherSnapshot),
}
impl SourceSnapshot {
    pub(crate) fn metadata_mut(&mut self) -> Option<&mut SnapshotMetadata> {
        match self {
            Self::Series(snapshot) => Some(&mut snapshot.metadata),
            Self::Geographic(snapshot) => Some(&mut snapshot.metadata),
            Self::Chess(snapshot) => Some(&mut snapshot.metadata),
            Self::Weather(snapshot) => Some(&mut snapshot.metadata),
        }
    }
}

pub fn captured(revision: u64, now_ms: i64, observed_at_ms: Option<i64>) -> SnapshotMetadata {
    SnapshotMetadata {
        revision,
        available: true,
        captured_at_ms: Some(now_ms),
        observed_at_ms,
        age_ms: observed_at_ms.map(|observed| now_ms.saturating_sub(observed).max(0) as u64),
        status: "ready".into(),
        error: None,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WeatherSnapshot {
    #[serde(flatten)]
    pub metadata: SnapshotMetadata,
    pub layers: Vec<Value>,
    pub attribution: String,
}

/// Immutable cache/delivery ownership. Arc cloning shares this SAME debit;
/// deep copying the payload requires a newly reserved snapshot admission.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct AdmittedSourceSnapshot {
    snapshot: SourceSnapshot,
    #[serde(skip)]
    _admission: ilium_execution::StorageAdmission,
    #[serde(skip)]
    images: Vec<super::NativeSourceImage>,
}
impl AdmittedSourceSnapshot {
    pub fn view(&self) -> &SourceSnapshot {
        &self.snapshot
    }
    pub fn native_images(&self) -> &[super::NativeSourceImage] {
        &self.images
    }
    pub(crate) fn new(
        snapshot: SourceSnapshot,
        admission: ilium_execution::StorageAdmission,
        images: Vec<super::NativeSourceImage>,
    ) -> Self {
        Self {
            snapshot,
            _admission: admission,
            images,
        }
    }
}
impl SourceSnapshot {
    /// Compact full-feed vector capacity before publishing projected results.
    /// This runs while the separate fetch peak debit is still held.
    pub(crate) fn compact(&mut self) {
        match self {
            Self::Series(snapshot) => snapshot.points.shrink_to_fit(),
            Self::Geographic(snapshot) => snapshot.entities.shrink_to_fit(),
            Self::Chess(snapshot) => snapshot.moves.shrink_to_fit(),
            Self::Weather(snapshot) => snapshot.layers.shrink_to_fit(),
        }
    }
    /// Declared owned payload bytes; tree-node envelopes are conservative,
    /// not allocator overhead or process RSS. Includes actual Vec/String spare
    /// capacity rather than serialized length. No temporary JSON copy is built.
    pub(crate) fn owned_bytes(&self) -> Result<usize> {
        fn add(total: &mut usize, bytes: usize) -> Result<()> {
            *total = total
                .checked_add(bytes)
                .ok_or_else(|| AnimationError::Budget("source snapshot size overflow".into()))?;
            Ok(())
        }
        fn optional(value: &Option<String>) -> usize {
            value.as_ref().map_or(0, String::capacity)
        }
        fn metadata(total: &mut usize, value: &SnapshotMetadata) -> Result<()> {
            add(total, value.status.capacity())?;
            add(total, optional(&value.error))
        }
        fn json(total: &mut usize, value: &Value, depth: usize) -> Result<()> {
            if depth > 32 {
                return Err(AnimationError::Budget("source result JSON depth".into()));
            }
            match value {
                Value::String(text) => add(total, text.capacity())?,
                Value::Array(values) => {
                    add(
                        total,
                        values
                            .capacity()
                            .checked_mul(std::mem::size_of::<Value>())
                            .ok_or_else(|| {
                                AnimationError::Budget("source JSON array size".into())
                            })?,
                    )?;
                    for value in values {
                        json(total, value, depth + 1)?;
                    }
                }
                Value::Object(values) => {
                    // Includes sparse BTreeMap leaf/internal nodes in the
                    // pinned Rust/serde configuration, plus keys separately.
                    add(
                        total,
                        values
                            .len()
                            .checked_mul(1024)
                            .ok_or_else(|| AnimationError::Budget("source JSON map size".into()))?,
                    )?;
                    for (key, value) in values {
                        add(total, key.capacity())?;
                        json(total, value, depth + 1)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        let mut total = std::mem::size_of::<Self>() + 64;
        match self {
            Self::Series(value) => {
                metadata(&mut total, &value.metadata)?;
                add(
                    &mut total,
                    value
                        .points
                        .capacity()
                        .checked_mul(std::mem::size_of::<SeriesPoint>())
                        .ok_or_else(|| AnimationError::Budget("source series size".into()))?,
                )?;
                for text in [&value.source_id, &value.verification, &value.attribution] {
                    add(&mut total, text.capacity())?;
                }
                add(&mut total, optional(&value.detail))?;
            }
            Self::Geographic(value) => {
                metadata(&mut total, &value.metadata)?;
                add(
                    &mut total,
                    value
                        .entities
                        .capacity()
                        .checked_mul(std::mem::size_of::<GeoEntity>())
                        .ok_or_else(|| AnimationError::Budget("source entity size".into()))?,
                )?;
                for entity in &value.entities {
                    add(&mut total, entity.id.capacity())?;
                    for text in [&entity.label, &entity.callsign, &entity.vessel_type] {
                        add(&mut total, optional(text))?;
                    }
                }
                add(&mut total, value.attribution.capacity())?;
                add(&mut total, optional(&value.coverage))?;
            }
            Self::Chess(value) => {
                metadata(&mut total, &value.metadata)?;
                add(
                    &mut total,
                    value
                        .moves
                        .capacity()
                        .checked_mul(std::mem::size_of::<String>())
                        .ok_or_else(|| AnimationError::Budget("source chess size".into()))?,
                )?;
                for text in &value.moves {
                    add(&mut total, text.capacity())?;
                }
                for text in [
                    &value.game_id,
                    &value.fen,
                    &value.white,
                    &value.black,
                    &value.state,
                ] {
                    add(&mut total, text.capacity())?;
                }
                add(&mut total, optional(&value.result))?;
            }
            Self::Weather(value) => {
                metadata(&mut total, &value.metadata)?;
                add(
                    &mut total,
                    value
                        .layers
                        .capacity()
                        .checked_mul(std::mem::size_of::<Value>())
                        .ok_or_else(|| AnimationError::Budget("source weather size".into()))?,
                )?;
                for value in &value.layers {
                    json(&mut total, value, 0)?;
                }
                add(&mut total, value.attribution.capacity())?;
            }
        }
        Ok(total)
    }
}

/// One-shot results retain the original debit and image allocations through
/// the final escaped reader, exactly like cached snapshots.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct AdmittedSourceValue {
    value: Value,
    #[serde(skip)]
    _admission: ilium_execution::StorageAdmission,
    #[serde(skip)]
    _images: Vec<super::NativeSourceImage>,
}
impl AdmittedSourceValue {
    pub fn view(&self) -> &Value {
        &self.value
    }
    pub fn native_images(&self) -> &[super::NativeSourceImage] {
        &self._images
    }
    pub(crate) fn new(
        value: Value,
        admission: ilium_execution::StorageAdmission,
        images: Vec<super::NativeSourceImage>,
    ) -> Self {
        Self {
            value,
            _admission: admission,
            _images: images,
        }
    }
}
impl std::ops::Index<&str> for AdmittedSourceValue {
    type Output = Value;
    fn index(&self, key: &str) -> &Value {
        &self.value[key]
    }
}
/// Accounts nested JSON without allocating a serialization copy.
pub(crate) fn json_owned_bytes(value: &Value) -> Result<usize> {
    fn scan(value: &Value, depth: usize, nodes: &mut usize) -> Result<usize> {
        *nodes += 1;
        if depth > 32 || *nodes > 4 * 1024 * 1024 {
            return Err(AnimationError::Budget(
                "source result JSON cardinality".into(),
            ));
        }
        let mut total = std::mem::size_of::<Value>();
        let mut add = |bytes: usize| -> Result<()> {
            total = total
                .checked_add(bytes)
                .ok_or_else(|| AnimationError::Budget("source JSON size".into()))?;
            Ok(())
        };
        match value {
            Value::String(text) => add(text.capacity())?,
            Value::Array(values) => {
                add(values
                    .capacity()
                    .checked_mul(std::mem::size_of::<Value>())
                    .ok_or_else(|| AnimationError::Budget("source JSON capacity".into()))?)?;
                for value in values {
                    add(scan(value, depth + 1, nodes)?)?;
                }
            }
            Value::Object(values) => {
                add(values
                    .len()
                    .checked_mul(1024)
                    .ok_or_else(|| AnimationError::Budget("source map capacity".into()))?)?;
                for (key, value) in values {
                    add(key.capacity())?;
                    add(scan(value, depth + 1, nodes)?)?;
                }
            }
            _ => {}
        }
        Ok(total)
    }
    scan(value, 0, &mut 0)
}
