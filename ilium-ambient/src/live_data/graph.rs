//! Live provider-time graphs. Rendering never waits for a network request.
use super::{
    catalog::{self, GraphSource, Provider},
    chart::{self, ChartMode},
    events::WikiFeed,
    fetch,
    model::{Candle, History, Observation},
    poll::{Poller, Snapshot},
    series::DataSeries,
};
use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc, time::UNIX_EPOCH};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphMode {
    #[default]
    Line,
    Bars,
    Candles,
}

const WINDOW_MINUTES: [i32; 11] = [1, 5, 30, 60, 120, 360, 1440, 10080, 43200, 129600, 525600];
const WINDOW_LABELS: [&str; 11] = [
    "1 minute",
    "5 minutes",
    "30 minutes",
    "1 hour",
    "2 hours",
    "6 hours",
    "1 day",
    "1 week",
    "30 days",
    "90 days",
    "1 year",
];

/// Stable source IDs are persisted; catalogue ordering is only a UI detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphSettings {
    pub source_id: String,
    pub mode: GraphMode,
    pub window_minutes: i32,
    pub poll_seconds: i32,
    pub brightness_percent: i32,
    pub hue: i32,
    pub rising_hue: i32,
    pub falling_hue: i32,
}

impl Default for GraphSettings {
    fn default() -> Self {
        Self {
            source_id: "btc_usd".into(),
            mode: GraphMode::Line,
            window_minutes: 120,
            poll_seconds: 60,
            brightness_percent: 65,
            hue: 190,
            rising_hue: 120,
            falling_hue: 0,
        }
    }
}

impl GraphSettings {
    pub fn source(&self) -> &'static GraphSource {
        // The catalogue is a nonempty static array, not external input.
        catalog::find(&self.source_id).unwrap_or(&catalog::SOURCES[0])
    }

    pub fn effective_poll_seconds(&self) -> u64 {
        (self.poll_seconds.clamp(5, 3600) as u64).max(self.source().minimum_poll_seconds)
    }
}

impl SceneSettings for GraphSettings {
    fn normalized(&self) -> Self {
        let mut settings = self.clone();
        settings.source_id = self.source().id.into();
        if settings.mode == GraphMode::Candles && !settings.source().has_ohlc {
            settings.mode = GraphMode::Line;
        }
        settings.window_minutes = settings
            .window_minutes
            .clamp(fetch::MIN_WINDOW_MINUTES, fetch::MAX_WINDOW_MINUTES);
        settings.poll_seconds = settings.poll_seconds.clamp(5, 3600);
        settings.brightness_percent = settings.brightness_percent.clamp(0, 100);
        settings.hue = settings.hue.clamp(0, 360);
        settings.rising_hue = settings.rising_hue.clamp(0, 360);
        settings.falling_hue = settings.falling_hue.clamp(0, 360);
        settings
    }

    fn controls(&self) -> Vec<Control> {
        let settings = self.normalized();
        let source = settings.source();
        let selection = catalog::SOURCES
            .iter()
            .position(|entry| entry.id == source.id)
            .unwrap_or(0);
        let labels: Vec<_> = catalog::SOURCES.iter().map(|entry| entry.label).collect();
        let mut mode = Control::choice("mode", "Chart", settings.mode as usize,
            &["Line", "Bars", "OHLC candles"],
            "Candles use the provider's actual open, high, low and close, never invented price samples.");
        if !source.has_ohlc {
            mode =
                mode.with_disabled_option(2, "This source does not provide genuine OHLC candles.");
        }
        let window_index = WINDOW_MINUTES
            .iter()
            .position(|minutes| *minutes == settings.window_minutes);
        let mut window_labels = WINDOW_LABELS.to_vec();
        if window_index.is_none() {
            window_labels.push("Custom saved span");
        }
        let mut window=Control::choice("window","Time window",window_index.unwrap_or(WINDOW_LABELS.len()),&window_labels,
            "Shows provider timestamps over the selected rolling window. Missing history remains empty; daily reference rates need longer windows.")
            .with_help_detail(format!("Selected span: {} min. History retains at most 20,000 measurements; changing Coinbase candle interval clears the previous interval's history.",settings.window_minutes));
        if window_index.is_none() {
            window = window.with_disabled_option(
                WINDOW_LABELS.len(),
                "Saved custom span is retained; choose a preset to change it.",
            );
        }
        let mut controls = vec![
            Control::choice("source", "Source", selection, &labels,
                "Public observations; successful requests do not guarantee a new measurement. Selecting ISS, Wikipedia or drand from a nonfast source uses one minute only from the default two-hour window; authored windows are retained.")
                .with_help_detail(format!("{}; {}. {}", source.attribution, source.units, source.documentation)),
            mode,
            window,
            Control::slider("poll", "Requested refresh", settings.poll_seconds, (5,3600,5), " s",
                "Provider request floors override faster requests. Errors retry with bounded backoff.")
                .with_help_detail(format!("Effective request interval: {} s; provider floor: {} s.", settings.effective_poll_seconds(),source.minimum_poll_seconds)),
            Control::slider("brightness", "Brightness", settings.brightness_percent,(0,100,5),"%",
                "Intensity of chart ink, including the dim axes."),
            Control::slider("hue", "Chart hue", settings.hue,(0,360,10),"°",
                "Color for lines, bars and axes."),
        ];
        if settings.mode == GraphMode::Candles {
            controls.push(Control::slider(
                "rising_hue",
                "Rising candle hue",
                settings.rising_hue,
                (0, 360, 10),
                "°",
                "Close at or above open uses this color; real candle wicks remain visible.",
            ));
            controls.push(Control::slider(
                "falling_hue",
                "Falling candle hue",
                settings.falling_hue,
                (0, 360, 10),
                "°",
                "Close below open uses this color.",
            ));
        }
        controls
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let mut next = self.clone();
        match id {
            "source" => {
                let selected = control::index(&value)
                    .and_then(|index| catalog::SOURCES.get(index))
                    .ok_or_else(|| "Choose a listed graph source.".to_owned())?;
                next.source_id = selected.id.into();
                let is_fast = |provider: Provider| {
                    matches!(
                        provider,
                        Provider::Iss(_) | Provider::Wikipedia(_) | Provider::DrandRandom
                    )
                };
                if is_fast(selected.provider)
                    && !is_fast(self.source().provider)
                    && self.window_minutes == GraphSettings::default().window_minutes
                {
                    next.window_minutes = 1;
                }

                // A two-hour spot-price window cannot usefully display daily
                // calendar reference rates, especially over a weekend.
                if matches!(selected.provider, Provider::EcbReference(_))
                    && !matches!(self.source().provider, Provider::EcbReference(_))
                    && next.window_minutes < 10080
                {
                    next.window_minutes = 10080;
                }
            }
            "mode" => {
                next.mode = match control::index(&value) {
                    Some(0) => GraphMode::Line,
                    Some(1) => GraphMode::Bars,
                    Some(2) if self.source().has_ohlc => GraphMode::Candles,
                    Some(2) => {
                        return Err("This source does not provide genuine OHLC candles.".into())
                    }
                    _ => return Err("Choose line, bars or OHLC candles.".into()),
                };
            }
            "window" => {
                next.window_minutes = match &value {
                    ControlValue::Index(index) => *WINDOW_MINUTES
                        .get(*index)
                        .ok_or_else(|| "Choose a listed time window.".to_owned())?,
                    ControlValue::Number(minutes) => *minutes,
                    _ => return Err("Choose a time window.".into()),
                };
            }
            "poll" | "brightness" | "hue" | "rising_hue" | "falling_hue" => {
                let number = control::number(&value)
                    .ok_or_else(|| "This setting requires a number.".to_owned())?;
                match id {
                    "poll" => next.poll_seconds = number,
                    "brightness" => next.brightness_percent = number,
                    "hue" => next.hue = number,
                    "rising_hue" => next.rising_hue = number,
                    "falling_hue" => next.falling_hue = number,
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        }
        let next = next.normalized();
        let changed = next != *self;
        *self = next;
        Ok(changed)
    }
}

pub struct GraphScene {
    settings: GraphSettings,
    poller: Option<Poller<DataSeries>>,
    stream: Option<WikiFeed>,
    snapshot: Arc<Snapshot<DataSeries>>,
    history: History,
    candles: BTreeMap<i64, Candle>,
    startup_error: Option<String>,
    last_status: String,
    last_bounds: Option<chart::ChartBounds>,
}

impl GraphScene {
    pub fn new(settings: &GraphSettings, _env: &SceneEnv) -> Self {
        let mut scene = Self::without_worker(settings.normalized());
        scene.start_worker();
        scene
    }

    fn without_worker(settings: GraphSettings) -> Self {
        Self {
            settings,
            poller: None,
            stream: None,
            snapshot: Arc::new(Snapshot::default()),
            history: History::new(20_000),
            candles: BTreeMap::new(),
            startup_error: None,
            last_status: "Waiting for public observations".into(),
            last_bounds: None,
        }
    }

    fn start_worker(&mut self) {
        self.poller = None; // Drop hands bounded joining to the ambient reaper.
        self.stream = None;
        self.startup_error = None;
        if let Provider::Wikipedia(metric) = self.settings.source().provider {
            match WikiFeed::start(metric, self.settings.effective_poll_seconds()) {
                Ok(stream) => self.stream = Some(stream),
                Err(error) => self.startup_error = Some(error),
            }
            return;
        }
        match fetch::graph(
            self.settings.source(),
            self.settings.effective_poll_seconds(),
            self.settings.window_minutes,
        ) {
            Ok(poller) => self.poller = Some(poller),
            Err(error) => self.startup_error = Some(error),
        }
    }

    /// Hosts can preserve history while changing visual settings. Source changes
    /// clear history so units or unrelated measurements can never be mixed.
    pub fn apply_settings(&mut self, settings: &GraphSettings) -> bool {
        let next = settings.normalized();
        if next == self.settings {
            return false;
        }
        let restart = self.update_settings(next);
        if restart {
            self.start_worker();
        }
        true
    }

    /// Apply state transitions separately from worker creation so interval
    /// boundaries have a deterministic fixture without making HTTP requests.
    fn update_settings(&mut self, next: GraphSettings) -> bool {
        let source_changed = next.source_id != self.settings.source_id;
        let bucket = |settings: &GraphSettings| {
            matches!(settings.source().provider, Provider::Coinbase(_))
                .then(|| fetch::candle_granularity(settings.window_minutes))
        };
        let bucket_changed = bucket(&next) != bucket(&self.settings);
        let window_request_changed = matches!(next.source().provider, Provider::Coinbase(_))
            && next.window_minutes != self.settings.window_minutes;
        let restart = source_changed
            || bucket_changed
            || window_request_changed
            || next.effective_poll_seconds() != self.settings.effective_poll_seconds();
        self.settings = next;
        if source_changed || bucket_changed {
            self.history = History::new(20_000);
            self.candles.clear();
            self.snapshot = Arc::new(Snapshot::default());
            self.last_bounds = None;
        }
        restart
    }

    fn prune_history(&mut self, now_ms: i64) {
        let retention = now_ms.saturating_sub(i64::from(fetch::MAX_WINDOW_MINUTES) * 60000);
        self.history.retain_since(retention);
        self.candles = self.candles.split_off(&retention);
    }

    fn accept_snapshot(&mut self, mut snapshot: Arc<Snapshot<DataSeries>>) {
        if snapshot.data.is_none() && self.snapshot.data.is_some() {
            if snapshot.state.error.is_none() && snapshot.state.received_ms.is_none() {
                return;
            }
            let mut retained = (*snapshot).clone();
            retained.data = self.snapshot.data.clone();
            retained.state.received_ms = self.snapshot.state.received_ms;
            retained.state.observed_ms = self.snapshot.state.observed_ms;
            snapshot = Arc::new(retained);
        }
        let same_data = match (&self.snapshot.data, &snapshot.data) {
            (Some(before), Some(after)) => Arc::ptr_eq(before, after),
            (None, None) => true,
            _ => false,
        };
        if !same_data {
            if let Some(data) = &snapshot.data {
                for sample in &data.samples {
                    self.history.ingest(*sample);
                }
                for candle in &data.candles {
                    if Candle::new(
                        candle.observed_ms,
                        candle.open,
                        candle.high,
                        candle.low,
                        candle.close,
                        candle.volume,
                    )
                    .is_some()
                    {
                        self.candles.insert(candle.observed_ms, *candle);
                    }
                    if self.candles.len() > 4096 {
                        self.candles.pop_first();
                    }
                }
            }
        }
        self.snapshot = snapshot;
    }

    fn status_at(&self, now_ms: i64) -> String {
        let source = self.settings.source();
        let state = &self.snapshot.state;
        let stale = state.is_stale(
            now_ms,
            (self.settings.effective_poll_seconds() as i64).saturating_mul(3000),
        );
        let condition = if state.error.is_some() || self.startup_error.is_some() {
            "last good / request error"
        } else if state.received_ms.is_none() {
            "waiting for observations"
        } else if stale {
            "last good / stale receipt"
        } else {
            "public observations"
        };
        let mut status = format!(
            "{} — {}; {}; observed {}; received {}; refresh {}s (floor {}s); window {}min{}",
            source.label,
            source.attribution,
            condition,
            timestamp_age(state.observed_ms, now_ms),
            timestamp_age(state.received_ms, now_ms),
            self.settings.effective_poll_seconds(),
            source.minimum_poll_seconds,
            self.settings.window_minutes,
            if stale { "; stale transport" } else { "" }
        );
        if let Some(data) = &self.snapshot.data {
            if let Some(detail) = &data.detail {
                status.push_str(&format!("; {}", detail));
            }
            if data.rejected > 0 {
                status.push_str(&format!("; {} invalid rows omitted", data.rejected));
            }
        }
        if let Some(bounds) = self.last_bounds {
            status.push_str(&format!(
                "; range {:.4}..{:.4} {}",
                bounds.low, bounds.high, source.units
            ));
        } else {
            status.push_str("; no observations in window");
        }
        if let Some(error) = self.startup_error.as_ref().or(state.error.as_ref()) {
            let safe: String = error
                .chars()
                .filter(|c| !c.is_control())
                .take(240)
                .collect();
            status.push_str(&format!("; {}", safe));
        }
        status
    }
}

impl Scene for GraphScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        if let Some(snapshot) = self.stream.as_ref().and_then(WikiFeed::try_snapshot) {
            if !Arc::ptr_eq(&snapshot, &self.snapshot) {
                self.accept_snapshot(snapshot);
            }
        }
        if let Some(snapshot) = self.poller.as_ref().and_then(Poller::try_snapshot) {
            if !Arc::ptr_eq(&snapshot, &self.snapshot) {
                self.accept_snapshot(snapshot);
            }
        }
        let now_ms = frame
            .now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_millis().min(i64::MAX as u128) as i64);
        // Retain up to one year in provider time, still capped at 20,000
        // measurements / 4,096 candles. Missing history remains absent.
        self.prune_history(now_ms);
        let window = (
            now_ms.saturating_sub(i64::from(self.settings.window_minutes) * 60_000),
            now_ms,
        );
        let candles: Vec<_> = if self.settings.mode == GraphMode::Candles {
            self.candles.values().copied().collect()
        } else {
            Vec::new()
        };
        self.last_bounds = render_graph(
            &self.settings,
            self.history.samples(),
            &candles,
            window,
            frame,
        );
        self.last_status = self.status_at(now_ms);
    }

    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn reconfigure(&mut self, settings: &crate::AmbientSettings) -> bool {
        self.apply_settings(&settings.graph);
        true
    }
    fn frames_per_second(&self) -> u32 {
        1
    }
    fn status(&self) -> Option<String> {
        Some(self.last_status.clone())
    }
}

fn timestamp_age(time: Option<i64>, now_ms: i64) -> String {
    let Some(time) = time else {
        return "unknown".into();
    };
    let stamp = chrono::DateTime::from_timestamp_millis(time).map_or_else(
        || format!("epoch-ms {time}"),
        |date| date.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    );
    if time > now_ms {
        format!(
            "{stamp} ({}s ahead of clock)",
            time.saturating_sub(now_ms) / 1000
        )
    } else {
        format!("{stamp} ({}s ago)", now_ms.saturating_sub(time) / 1000)
    }
}

fn hue_rgb(hue: i32) -> [u8; 3] {
    let sector = hue.rem_euclid(360) as f32 / 60.0;
    let fraction = sector - sector.floor();
    let low = 0.15;
    let rising = low + (1.0 - low) * fraction;
    let falling = 1.0 - (1.0 - low) * fraction;
    let channels = match sector as u32 {
        0 => [1.0, rising, low],
        1 => [falling, 1.0, low],
        2 => [low, 1.0, rising],
        3 => [low, falling, 1.0],
        4 => [rising, low, 1.0],
        _ => [1.0, low, falling],
    };
    channels.map(|channel| (channel * 255.0) as u8)
}

/// Pure fixture-friendly projection. Candle colour is selected by nearest
/// genuine candle column; each wick/body shares its provider's direction.
fn render_graph(
    settings: &GraphSettings,
    samples: &[Observation],
    candles: &[Candle],
    window: (i64, i64),
    frame: &mut Frame<'_>,
) -> Option<chart::ChartBounds> {
    let cells = usize::from(frame.width) * usize::from(frame.height);
    frame.cell_colors.resize(cells, hue_rgb(settings.hue));
    frame.cell_colors.fill(hue_rgb(settings.hue));
    let intensity = settings.brightness_percent.clamp(0, 100) as f32 / 100.0;
    if settings.mode != GraphMode::Candles || !settings.source().has_ohlc {
        let mode = if settings.mode == GraphMode::Bars {
            ChartMode::Bars
        } else {
            ChartMode::Line
        };
        return chart::render_series(frame.raster, samples, window, mode, intensity);
    }
    let bounds = chart::render_candles(frame.raster, candles, window, intensity)?;
    let visible: Vec<_> = candles
        .iter()
        .filter(|c| {
            c.observed_ms >= window.0
                && c.observed_ms <= window.1
                && Candle::new(c.observed_ms, c.open, c.high, c.low, c.close, c.volume).is_some()
        })
        .collect();
    // Convert exact timestamp differences, preserving precision near i64::MAX.
    let span = (i128::from(window.1) - i128::from(window.0)) as f64;
    for x in 0..frame.width {
        let fraction = (f64::from(x) + 0.5) / f64::from(frame.width);
        let nearest = visible.iter().min_by(|left, right| {
            let column = |c: &Candle| {
                0.08 + ((i128::from(c.observed_ms) - i128::from(window.0)) as f64 / span) * 0.86
            };
            (column(left) - fraction)
                .abs()
                .total_cmp(&(column(right) - fraction).abs())
        });
        let Some(candle) = nearest else { continue };
        let color = hue_rgb(if candle.close >= candle.open {
            settings.rising_hue
        } else {
            settings.falling_hue
        });
        for y in 0..frame.height {
            // Axis ink is at most 30% of chart intensity. Only cells with
            // candle ink receive its direction color; empty cells and axes
            // keep the user's chart hue. Terminal cells have one color even
            // where several sub-cell candle fragments overlap.
            let has_candle_ink = (0..4).any(|dot_y| {
                (0..2).any(|dot_x| {
                    let column = usize::from(x) * 2 + dot_x;
                    let row = usize::from(y) * 4 + dot_y;
                    column < frame.raster.width
                        && row < frame.raster.height
                        && frame.raster.dots[row * frame.raster.width + column] > intensity * 0.31
                })
            });
            if has_candle_ink {
                if let Some(cell) = frame.cell_color_mut(x, y) {
                    *cell = color;
                }
            }
        }
    }
    Some(bounds)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn fixture_scene(settings: GraphSettings) -> GraphScene {
        GraphScene::without_worker(settings)
    }

    #[test]
    fn one_and_five_minute_presets_normalize_without_changing_the_persisted_field() {
        let mut settings = GraphSettings {
            window_minutes: 1,
            ..Default::default()
        };
        assert_eq!(settings.normalized().window_minutes, 1);
        let row = settings
            .controls()
            .into_iter()
            .find(|row| row.id == "window")
            .unwrap();
        assert_eq!(row.display_value(), "1 minute");
        settings
            .set_control("window", ControlValue::Index(1))
            .unwrap();
        assert_eq!(settings.window_minutes, 5);
        settings
            .set_control("window", ControlValue::Number(0))
            .unwrap();
        assert_eq!(settings.window_minutes, 1);
        let persisted = serde_json::to_value(settings).unwrap();
        assert_eq!(persisted["window_minutes"], 1);
    }
    #[test]
    fn default_nonfast_source_selection_uses_one_minute_for_each_fast_provider() {
        for id in [
            "iss_altitude",
            "iss_velocity",
            "iss_latitude",
            "iss_longitude",
            "wikipedia_edit_rate",
            "wikipedia_bot_share",
            "drand_randomness",
        ] {
            let mut settings = GraphSettings::default();
            let selected = catalog::SOURCES
                .iter()
                .position(|source| source.id == id)
                .unwrap();
            settings
                .set_control("source", ControlValue::Index(selected))
                .unwrap();
            assert_eq!(settings.window_minutes, 1, "{id}");
            assert!(settings.controls()[0].help.contains("default two-hour"));
        }
    }
    #[test]
    fn nondefault_windows_and_fast_to_fast_selections_preserve_authored_spans() {
        for (from, minutes, to) in [
            ("btc_usd", 5, "iss_altitude"),
            ("btc_usd", 121, "iss_altitude"),
            ("eur_usd", 10080, "drand_randomness"),
            ("iss_altitude", 120, "wikipedia_edit_rate"),
            ("wikipedia_edit_rate", 5, "drand_randomness"),
        ] {
            let mut settings = GraphSettings {
                source_id: from.into(),
                window_minutes: minutes,
                ..Default::default()
            };
            let selected = catalog::SOURCES
                .iter()
                .position(|source| source.id == to)
                .unwrap();
            settings
                .set_control("source", ControlValue::Index(selected))
                .unwrap();
            assert_eq!(settings.window_minutes, minutes, "{from} to {to}");
        }
    }
    #[test]
    fn selecting_daily_reference_starts_with_week_span_and_saved_custom_window_is_honest() {
        let mut settings = GraphSettings::default();
        let ecb = catalog::SOURCES
            .iter()
            .position(|source| source.id == "eur_usd")
            .unwrap();
        settings
            .set_control("source", ControlValue::Index(ecb))
            .unwrap();
        assert_eq!(settings.window_minutes, 10080);
        settings
            .set_control("window", ControlValue::Number(180))
            .unwrap();
        let row = settings
            .controls()
            .into_iter()
            .find(|row| row.id == "window")
            .unwrap();
        assert_eq!(row.display_value(), "Custom saved span");
        assert!(row.help_detail.unwrap().contains("180 min"));
        assert_eq!(settings.normalized().window_minutes, 180);
    }
    #[test]
    fn changing_coinbase_bucket_clears_history_but_visual_and_same_bucket_windows_keep_it() {
        let mut scene = fixture_scene(GraphSettings::default());
        let seed = |scene: &mut GraphScene| {
            let data = DataSeries {
                samples: vec![Observation::new(60000, 2.0).unwrap()],
                candles: vec![Candle::new(60000, 1.0, 3.0, 0.0, 2.0, 1.0).unwrap()],
                ..Default::default()
            };
            scene.accept_snapshot(Arc::new(Snapshot {
                data: Some(Arc::new(data)),
                state: Default::default(),
            }));
        };
        seed(&mut scene);
        let mut same = scene.settings.clone();
        same.window_minutes = 60;
        assert!(scene.update_settings(same));
        assert_eq!(scene.history.samples().len(), 1);
        assert_eq!(scene.candles.len(), 1);
        let mut longer = scene.settings.clone();
        longer.window_minutes = 1440;
        assert!(scene.update_settings(longer));
        assert!(scene.history.samples().is_empty());
        assert!(scene.candles.is_empty());
        assert!(scene.snapshot.data.is_none());
        seed(&mut scene);
        let mut hue = scene.settings.clone();
        hue.hue = 250;
        assert!(!scene.update_settings(hue));
        assert_eq!(scene.history.samples().len(), 1);
    }
    #[test]
    fn year_retention_keeps_daily_reference_history_and_prunes_only_older_data() {
        let mut scene = fixture_scene(GraphSettings {
            source_id: "eur_usd".into(),
            window_minutes: 525600,
            ..Default::default()
        });
        let year_ms = i64::from(fetch::MAX_WINDOW_MINUTES) * 60000;
        let now = year_ms + 100000;
        for time in [99999, 100000, now - 86400000, now] {
            scene.history.ingest(Observation::new(time, 1.1).unwrap());
        }
        scene.prune_history(now);
        assert_eq!(scene.history.samples().len(), 3);
        assert_eq!(scene.history.samples()[0].observed_ms, 100000);
        assert_eq!(scene.settings.effective_poll_seconds(), 3600);
        let mut settings = scene.settings.clone();
        settings
            .set_control("window", ControlValue::Index(WINDOW_MINUTES.len() - 1))
            .unwrap();
        assert_eq!(settings.window_minutes, 525600);
        assert_eq!(settings.normalized().window_minutes, 525600);
        assert!(scene.status_at(now).contains("daily EUR reference"));
    }
    #[test]
    fn stable_source_selection_and_ohlc_availability_follow_catalogue() {
        let mut settings = GraphSettings::default();
        assert_eq!(
            settings.controls()[0].kind,
            crate::control::ControlKind::Choice {
                options: catalog::SOURCES.iter().map(|source| source.label).collect()
            }
        );
        let index = catalog::SOURCES
            .iter()
            .position(|source| source.id == "solar_wind_speed")
            .unwrap();
        settings
            .set_control("source", ControlValue::Index(index))
            .unwrap();
        assert_eq!(settings.source_id, "solar_wind_speed");
        assert_eq!(
            settings.controls()[1].disabled_reason(2),
            Some("This source does not provide genuine OHLC candles.")
        );
        assert!(settings
            .set_control("mode", ControlValue::Index(2))
            .is_err());
        settings.mode = GraphMode::Candles;
        assert_eq!(settings.normalized().mode, GraphMode::Line);
        settings.poll_seconds = 5;
        assert_eq!(settings.effective_poll_seconds(), 60);
    }

    #[test]
    fn repeated_snapshots_do_not_create_history_and_revisions_replace() {
        let mut scene = fixture_scene(GraphSettings::default());
        let mut data = DataSeries::default();
        data.samples.push(Observation::new(1000, 3.0).unwrap());
        let mut snapshot = Snapshot {
            data: Some(Arc::new(data)),
            state: Default::default(),
        };
        snapshot.state.received(2000, Some(1000));
        scene.accept_snapshot(Arc::new(snapshot.clone()));
        snapshot.state.received(3000, Some(1000));
        scene.accept_snapshot(Arc::new(snapshot));
        assert_eq!(scene.history.samples().len(), 1);
        let revised = DataSeries {
            samples: vec![Observation::new(1000, 4.0).unwrap()],
            ..Default::default()
        };
        scene.accept_snapshot(Arc::new(Snapshot {
            data: Some(Arc::new(revised)),
            state: Default::default(),
        }));
        assert_eq!(scene.history.samples()[0].value, 4.0);
    }

    #[test]
    fn freshness_status_distinguishes_provider_time_and_receipt_and_error() {
        let mut scene = fixture_scene(GraphSettings::default());
        let mut snapshot = Snapshot::<DataSeries>::default();
        snapshot.state.received(120000, Some(60000));
        snapshot.state.failed("offline".into());
        scene.accept_snapshot(Arc::new(snapshot));
        let status = scene.status_at(180000);
        assert!(status.contains("Coinbase Exchange"));
        assert!(status.contains("observed 1970-01-01 00:01:00 UTC (120s ago)"));
        assert!(status.contains("received 1970-01-01 00:02:00 UTC (60s ago)"));
        assert!(!status.contains("stale transport"));
        assert!(scene.status_at(400000).contains("stale transport"));
        assert!(status.contains("offline"));
        assert!(status.contains("last good"));
    }

    #[test]
    fn line_bars_and_genuine_candles_paint_and_overwrite_all_colors() {
        let samples = [
            Observation::new(60000, 5.0).unwrap(),
            Observation::new(120000, 8.0).unwrap(),
        ];
        let candles = [
            Candle::new(60000, 4.0, 9.0, 2.0, 7.0, 1.0).unwrap(),
            Candle::new(120000, 7.0, 10.0, 1.0, 3.0, 1.0).unwrap(),
        ];
        for mode in [GraphMode::Line, GraphMode::Bars, GraphMode::Candles] {
            let settings = GraphSettings {
                mode,
                rising_hue: 120,
                falling_hue: 0,
                ..Default::default()
            };
            let mut raster = crate::raster::Raster::default();
            raster.resize(120, 80);
            let mut colors = vec![[1, 2, 3]; 3000];
            let mut frame = Frame {
                raster: &mut raster,
                cell_colors: &mut colors,
                width: 60,
                height: 20,
                time: Duration::ZERO,
                wall: Duration::ZERO,
                now: UNIX_EPOCH + Duration::from_secs(130),
            };
            let bounds = render_graph(&settings, &samples, &candles, (0, 180000), &mut frame);
            assert!(bounds.is_some());
            assert_eq!(frame.cell_colors.len(), 1200);
            assert!(frame.cell_colors.iter().all(|color| *color != [1, 2, 3]));
            assert!(frame.raster.dots.iter().any(|dot| *dot > 0.3));
            if mode == GraphMode::Candles {
                assert_eq!(
                    bounds.unwrap(),
                    chart::ChartBounds {
                        low: 1.0,
                        high: 10.0
                    }
                );
                assert!(frame.cell_colors.contains(&hue_rgb(120)));
                assert!(frame.cell_colors.contains(&hue_rgb(0)));
                assert!(frame.cell_colors.contains(&hue_rgb(settings.hue)));
            }
        }
    }

    #[test]
    fn empty_data_and_expired_data_draw_no_fabricated_series() {
        let mut scene = fixture_scene(GraphSettings::default());
        let data = DataSeries {
            samples: vec![Observation::new(1000, 2.0).unwrap()],
            ..Default::default()
        };
        scene.accept_snapshot(Arc::new(Snapshot {
            data: Some(Arc::new(data)),
            state: Default::default(),
        }));
        let mut raster = crate::raster::Raster::default();
        raster.resize(80, 48);
        let mut colors = Vec::new();
        let mut frame = Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 40,
            height: 12,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: UNIX_EPOCH + Duration::from_secs(86401),
        };
        scene.render(&mut frame);
        assert!(frame.raster.dots.iter().all(|dot| *dot == 0.0));
        assert!(scene
            .status()
            .unwrap()
            .contains("no observations in window"));
    }

    #[test]
    fn replacement_worker_preserves_last_good_receipt_and_data_until_new_success() {
        let mut scene = fixture_scene(GraphSettings::default());
        let data = Arc::new(DataSeries {
            samples: vec![Observation::new(900, 2.0).unwrap()],
            ..Default::default()
        });
        let state = super::super::model::FeedState {
            received_ms: Some(1000),
            observed_ms: Some(900),
            error: None,
        };
        scene.accept_snapshot(Arc::new(Snapshot {
            data: Some(Arc::clone(&data)),
            state,
        }));
        scene.accept_snapshot(Arc::new(Snapshot::default()));
        assert_eq!(scene.snapshot.state.received_ms, Some(1000));
        let failed = Snapshot {
            data: None,
            state: super::super::model::FeedState {
                error: Some("offline".into()),
                ..Default::default()
            },
        };
        scene.accept_snapshot(Arc::new(failed));
        assert!(Arc::ptr_eq(scene.snapshot.data.as_ref().unwrap(), &data));
        assert_eq!(scene.snapshot.state.observed_ms, Some(900));
        assert_eq!(scene.snapshot.state.error.as_deref(), Some("offline"));
    }

    #[test]
    fn visual_edits_preserve_history_and_invalid_controls_are_rejected() {
        let mut scene = fixture_scene(GraphSettings::default());
        scene.history.ingest(Observation::new(1, 2.0).unwrap());
        let mut settings = scene.settings.clone();
        settings.hue = 240;
        assert!(scene.apply_settings(&settings));
        assert_eq!(scene.history.samples().len(), 1);
        assert!(settings
            .set_control("window", ControlValue::Text("oops".into()))
            .is_err());
        assert!(!settings
            .set_control("unknown", ControlValue::Number(2))
            .unwrap());
        assert_eq!(
            serde_json::from_str::<GraphSettings>("{}").unwrap(),
            GraphSettings::default()
        );
    }
}
