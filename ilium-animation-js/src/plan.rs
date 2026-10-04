//! Bounded script demand declarations. Parsing never acquires an input.
//! Authorization and frozen-recording identity are separate host checks.
use crate::{
    error::{AnimationError, Result},
    manifest::AnimationMode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub struct PlanBudget {
    pub json_bytes: usize,
    pub max_fps: f64,
    pub max_clip_seconds: f64,
    pub max_samples: usize,
}
impl Default for PlanBudget {
    fn default() -> Self {
        Self {
            json_bytes: 64 * 1024,
            max_fps: 120.0,
            max_clip_seconds: 120.0,
            max_samples: 65536,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputPlan {
    pub mode: String,
    pub format: String,
    pub update: String,
    #[serde(default)]
    pub cell_rgb: bool,
    #[serde(default)]
    pub colour_space: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateDemand {
    pub max_hz: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockDemand {
    pub max_hz: f64,
    pub civil: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OcclusionDemand {
    pub max_hz: f64,
    #[serde(default)]
    pub cells: bool,
    #[serde(default)]
    pub pixels: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioDemand {
    pub max_hz: f64,
    pub products: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub band_count: Option<usize>,
    #[serde(default)]
    pub waveform_samples: Option<usize>,
    #[serde(default)]
    pub window: Option<String>,
    #[serde(default)]
    pub history_frames: Option<usize>,
}
impl AudioDemand {
    pub fn needs_fft(&self) -> bool {
        self.products
            .iter()
            .any(|product| matches!(product.as_str(), "bands" | "history"))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesDemand {
    pub max_hz: f64,
    pub provider: String,
    pub fields: Vec<String>,
    pub samples: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeographicBounds {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeoDemand {
    pub max_hz: f64,
    pub bounds: GeographicBounds,
    pub max_entities: usize,
    pub fields: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChessDemand {
    pub max_hz: f64,
    #[serde(default)]
    pub game_id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AstronomyDemand {
    pub max_hz: f64,
    pub catalogue: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeatherDemand {
    pub max_hz: f64,
    pub bounds: GeographicBounds,
    pub max_entities: usize,
    pub fields: Vec<String>,
    pub layers: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputDemands {
    pub pointer: Option<RateDemand>,
    pub occlusion: Option<OcclusionDemand>,
    pub clock: Option<ClockDemand>,
    pub location: Option<RateDemand>,
    pub audio: Option<AudioDemand>,
    pub series: Option<SeriesDemand>,
    pub earthquakes: Option<GeoDemand>,
    pub aircraft: Option<GeoDemand>,
    pub boats: Option<GeoDemand>,
    pub chess: Option<ChessDemand>,
    pub astronomy: Option<AstronomyDemand>,
    pub weather: Option<WeatherDemand>,
}
impl InputDemands {
    pub fn is_empty(&self) -> bool {
        self.pointer.is_none()
            && self.occlusion.is_none()
            && self.clock.is_none()
            && self.location.is_none()
            && self.audio.is_none()
            && self.series.is_none()
            && self.earthquakes.is_none()
            && self.aircraft.is_none()
            && self.boats.is_none()
            && self.chess.is_none()
            && self.astronomy.is_none()
            && self.weather.is_none()
    }
    fn requires_recording(&self) -> bool {
        self.pointer.is_some()
            || self.location.is_some()
            || self.audio.is_some()
            || self.series.is_some()
            || self.earthquakes.is_some()
            || self.aircraft.is_some()
            || self.boats.is_some()
            || self.chess.is_some()
            || self.weather.is_some()
    }
    fn validate(&self, budget: PlanBudget) -> Result<()> {
        let rates = [
            self.pointer.as_ref().map(|x| x.max_hz),
            self.occlusion.as_ref().map(|x| x.max_hz),
            self.clock.as_ref().map(|x| x.max_hz),
            self.location.as_ref().map(|x| x.max_hz),
            self.audio.as_ref().map(|x| x.max_hz),
            self.series.as_ref().map(|x| x.max_hz),
            self.earthquakes.as_ref().map(|x| x.max_hz),
            self.aircraft.as_ref().map(|x| x.max_hz),
            self.boats.as_ref().map(|x| x.max_hz),
            self.chess.as_ref().map(|x| x.max_hz),
            self.astronomy.as_ref().map(|x| x.max_hz),
            self.weather.as_ref().map(|x| x.max_hz),
        ];
        for rate in rates.into_iter().flatten() {
            positive_bounded(rate, budget.max_fps, "input rate")?;
        }
        if let Some(demand) = &self.occlusion {
            if !demand.cells && !demand.pixels {
                return invalid("occlusion must request a representation");
            }
        }
        if let Some(audio) = &self.audio {
            names(&audio.products, 5)?;
            if audio.products.is_empty()
                || audio.products.iter().any(|x| {
                    !matches!(
                        x.as_str(),
                        "level" | "waveform" | "bands" | "envelope" | "history"
                    )
                })
            {
                return invalid("unknown or empty audio products");
            }
            if audio
                .window
                .as_ref()
                .is_some_and(|x| !matches!(x.as_str(), "hann" | "blackman"))
            {
                return invalid("unsupported FFT window");
            }
            if let Some(source) = &audio.source {
                bounded_name(source)?;
            }
            for size in [
                audio.band_count,
                audio.waveform_samples,
                audio.history_frames,
            ]
            .into_iter()
            .flatten()
            {
                bounded_count(size, budget.max_samples)?;
            }
            let bands = audio.band_count.unwrap_or(64);
            let history = audio.history_frames.unwrap_or(1);
            if bands
                .checked_mul(history)
                .is_none_or(|x| x > budget.max_samples)
            {
                return invalid("audio history budget");
            }
        }
        if let Some(series) = &self.series {
            if !matches!(
                series.provider.as_str(),
                "crypto"
                    | "ecb"
                    | "noaa_solar_wind"
                    | "iss"
                    | "usgs"
                    | "wikipedia"
                    | "drand_quicknet"
            ) {
                return invalid("unknown series provider");
            }
            names(&series.fields, 32)?;
            bounded_count(series.samples, budget.max_samples)?;
        }
        for geo in [&self.earthquakes, &self.aircraft, &self.boats]
            .into_iter()
            .flatten()
        {
            geo.bounds.validate()?;
            bounded_count(geo.max_entities, budget.max_samples)?;
            names(&geo.fields, 32)?;
        }
        if let Some(chess) = &self.chess {
            if let Some(id) = &chess.game_id {
                bounded_name(id)?;
            }
            if chess
                .source
                .as_ref()
                .is_some_and(|source| source != "lichess_tv")
            {
                return invalid("unknown chess source");
            }
            if chess.game_id.is_none() && chess.source.is_none() {
                return invalid("chess requires a game or TV source");
            }
        }
        if let Some(astronomy) = &self.astronomy {
            bounded_name(&astronomy.catalogue)?;
        }
        if let Some(weather) = &self.weather {
            weather.bounds.validate()?;
            bounded_count(weather.max_entities, budget.max_samples)?;
            names(&weather.fields, 32)?;
            names(&weather.layers, 16)?;
        }
        Ok(())
    }
}
impl GeographicBounds {
    fn validate(&self) -> Result<()> {
        if ![self.west, self.south, self.east, self.north]
            .into_iter()
            .all(f64::is_finite)
            || self.west < -180.0
            || self.east > 180.0
            || self.west > self.east
            || self.south < -90.0
            || self.north > 90.0
            || self.south > self.north
        {
            return invalid("geographic bounds");
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequest {
    pub id: String,
    pub scope: Value,
    pub required: bool,
    pub reason: String,
    #[serde(default)]
    pub request_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayPlan {
    pub seed: u32,
    pub duration_seconds: f64,
    pub seamless: bool,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub civil_anchor_ms: Option<i64>,
    #[serde(default)]
    pub input_recording: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Controls {
    pub density: Option<bool>,
    pub dither: Option<bool>,
    pub contrast: Option<bool>,
    pub inversion: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationPlan {
    #[serde(default)]
    pub output: Option<OutputPlan>,
    #[serde(default)]
    pub format: Option<String>,
    pub fps: f64,
    #[serde(default)]
    pub permissions: Vec<PermissionRequest>,
    pub inputs: InputDemands,
    #[serde(default)]
    pub preparation: Option<Value>,
    #[serde(default)]
    pub replay: Option<ReplayPlan>,
    #[serde(default)]
    pub mode_unavailable_reason: Option<String>,
    #[serde(default)]
    pub controls: Controls,
}
impl AnimationPlan {
    pub fn parse(value: &Value, mode: AnimationMode, budget: PlanBudget) -> Result<Self> {
        let bytes = serde_json::to_vec(value)?;
        if bytes.len() > budget.json_bytes {
            return invalid("plan metadata budget");
        }
        let plan: Self = serde_json::from_slice(&bytes)?;
        positive_bounded(plan.fps, budget.max_fps, "frame rate")?;
        if let Some(reason) = &plan.mode_unavailable_reason {
            return Err(AnimationError::Runtime(reason.chars().take(512).collect()));
        }
        let format = match (&plan.output, &plan.format) {
            (Some(output), None) => {
                if !matches!(output.update.as_str(), "replace" | "retain") {
                    return invalid("unknown surface update mode");
                }
                if output.mode == "cells" && output.format != "mask8"
                    || output.mode == "pixels" && output.format == "mask8"
                    || !matches!(output.mode.as_str(), "cells" | "pixels")
                {
                    return invalid("output mode/format mismatch");
                }
                if output
                    .colour_space
                    .as_ref()
                    .is_some_and(|space| !matches!(space.as_str(), "srgb" | "linear"))
                {
                    return invalid("unknown colour space");
                }
                output.format.as_str()
            }
            (None, Some(format)) => format.as_str(),
            _ => return invalid("declare exactly one output plan or format"),
        };
        if !matches!(
            format,
            "mask8" | "mono1" | "mono8" | "gray8" | "gray32" | "rgb8" | "rgba8"
        ) {
            return invalid("unknown output format");
        }
        plan.inputs.validate(budget)?;
        if plan.permissions.len() > 32 {
            return invalid("too many permission requests");
        }
        for permission in &plan.permissions {
            bounded_name(&permission.id)?;
            if permission.reason.trim().is_empty() || permission.reason.len() > 1024 {
                return invalid("permission explanation required");
            }
            if let Some(id) = &permission.request_id {
                bounded_name(id)?;
            }
        }
        if let Some(preparation) = &plan.preparation {
            validate_preparation(preparation)?;
        }
        if let Some(replay) = &plan.replay {
            positive_bounded(
                replay.duration_seconds,
                budget.max_clip_seconds,
                "clip duration",
            )?;
            if let Some(fps) = replay.fps {
                positive_bounded(fps, budget.max_fps, "clip frame rate")?;
            }
            if let Some(id) = &replay.input_recording {
                bounded_name(id)?;
            }
        }
        if mode == AnimationMode::PreRendered {
            let replay = plan.replay.as_ref().ok_or_else(|| {
                AnimationError::Runtime(
                    "pre-rendered mode requires explicit replay metadata".into(),
                )
            })?;
            if plan.inputs.requires_recording() && replay.input_recording.is_none() {
                return invalid("live inputs require authenticated frozen recording");
            }
            if plan.inputs.clock.as_ref().is_some_and(|clock| clock.civil)
                && replay.civil_anchor_ms.is_none()
            {
                return invalid("civil replay requires an explicit frozen anchor");
            }
        }
        Ok(plan)
    }
}
fn validate_preparation(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| AnimationError::Runtime("preparation must be an object".into()))?;
    for (family, value) in object {
        let allowed: &[&str] = match family.as_str() {
            "http" => &["max_requests", "max_bytes"],
            "disk" => &["max_bytes"],
            "media" => &["max_bytes", "max_pixels"],
            "compute" => &["max_jobs", "max_bytes"],
            _ => return invalid("unknown preparation family"),
        };
        let fields = value
            .as_object()
            .ok_or_else(|| AnimationError::Runtime("invalid preparation budget".into()))?;
        for (key, value) in fields {
            if !allowed.contains(&key.as_str())
                || value
                    .as_u64()
                    .is_none_or(|value| value == 0 || value > 256 * 1024 * 1024)
            {
                return invalid("preparation budget");
            }
        }
    }
    Ok(())
}
fn positive_bounded(value: f64, maximum: f64, label: &str) -> Result<()> {
    if !value.is_finite() || value <= 0.0 || value > maximum {
        return invalid(label);
    }
    Ok(())
}
fn bounded_count(value: usize, maximum: usize) -> Result<()> {
    if value == 0 || value > maximum {
        return invalid("input sample budget");
    }
    Ok(())
}
fn bounded_name(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return invalid("input name");
    }
    Ok(())
}
fn names(values: &[String], maximum: usize) -> Result<()> {
    if values.len() > maximum {
        return invalid("input field budget");
    }
    let mut unique = std::collections::BTreeSet::new();
    for value in values {
        bounded_name(value)?;
        if !unique.insert(value) {
            return invalid("duplicate input field");
        }
    }
    Ok(())
}
fn invalid<T>(message: &str) -> Result<T> {
    Err(AnimationError::Runtime(format!(
        "invalid animation plan: {message}"
    )))
}
