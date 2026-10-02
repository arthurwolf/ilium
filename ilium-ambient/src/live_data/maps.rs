//! Public observed geography: cached coastlines and truthful live markers.
use super::{
    fetch, map,
    model::{Earthquake, FeedState, Position},
    poll::{Poller, Snapshot},
};
use crate::{
    control::{self, Control, ControlValue, SceneSettings},
    raster::Raster,
    scene::{Frame, Scene, SceneEnv},
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapKind {
    Earthquakes,
    Aircraft,
    Boats,
}
impl MapKind {
    pub fn minimum_poll_seconds(self) -> u64 {
        match self {
            Self::Earthquakes => 60,
            Self::Aircraft => 900,
            Self::Boats => 30,
        }
    }
    fn attribution(self) -> &'static str {
        match self {
            Self::Earthquakes=>"USGS — worldwide reported earthquakes, all reported magnitudes",
            Self::Aircraft=>"OpenSky — worldwide received airborne positions; coverage is incomplete; anonymous budget limits refresh to 15 min",
            Self::Boats=>"Digitraffic AIS — Finnish waters, not global ship coverage",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSettings {
    pub map_hue: i32,
    pub marker_hue: i32,
    pub map_brightness_percent: i32,
    pub marker_brightness_percent: i32,
    pub poll_seconds: i32,
    pub magnitude_labels: bool,
}
impl Default for MapSettings {
    fn default() -> Self {
        Self {
            map_hue: 210,
            marker_hue: 35,
            map_brightness_percent: 25,
            marker_brightness_percent: 90,
            poll_seconds: 60,
            magnitude_labels: true,
        }
    }
}
impl MapSettings {
    pub fn effective_poll_seconds(&self, kind: MapKind) -> u64 {
        (self.poll_seconds.clamp(5, 3600) as u64).max(kind.minimum_poll_seconds())
    }
    pub fn controls_for(&self, kind: MapKind) -> Vec<Control> {
        let mut controls = self.controls();
        if kind != MapKind::Earthquakes {
            controls.retain(|row| row.id != "labels");
        }
        for row in &mut controls {
            if row.id == "poll" {
                row.help_detail = Some(format!(
                    "Effective interval: {} s; provider floor: {} s. {}",
                    self.effective_poll_seconds(kind),
                    kind.minimum_poll_seconds(),
                    kind.attribution()
                ));
            }
        }
        controls
    }
}
impl SceneSettings for MapSettings {
    fn normalized(&self) -> Self {
        Self {
            map_hue: self.map_hue.clamp(0, 360),
            marker_hue: self.marker_hue.clamp(0, 360),
            map_brightness_percent: self.map_brightness_percent.clamp(0, 100),
            marker_brightness_percent: self.marker_brightness_percent.clamp(0, 100),
            poll_seconds: self.poll_seconds.clamp(5, 3600),
            magnitude_labels: self.magnitude_labels,
        }
    }
    fn controls(&self) -> Vec<Control> {
        let settings = self.normalized();
        vec![
            Control::slider("poll","Requested refresh",settings.poll_seconds,(5,3600,5)," s",
                "Provider request floors override faster requests; failed requests use bounded backoff.")
                .with_help_detail("Earthquakes: at least 60 s. Aircraft: at least 900 s. Finnish-water ships: at least 30 s."),
            Control::slider("map_hue","Map hue",settings.map_hue,(0,360,10),"°","Color of the embedded Natural Earth coastlines."),
            Control::slider("map_brightness","Map brightness",settings.map_brightness_percent,(0,100,5),"%","Coastline brightness, independent of reported-object markers."),
            Control::slider("marker_hue","Marker hue",settings.marker_hue,(0,360,10),"°","Color of markers and earthquake magnitude labels."),
            Control::slider("marker_brightness","Marker brightness",settings.marker_brightness_percent,(0,100,5),"%","Brightness of reported-object markers."),
            Control::toggle("labels","Magnitude labels",settings.magnitude_labels,"Show reported magnitudes, including zero and negative values; '?' means unknown. Colliding labels are omitted, never event markers."),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let mut next = self.clone();
        match id {
            "labels" => {
                next.magnitude_labels = control::boolean(&value)
                    .ok_or_else(|| "This setting requires on or off.".to_owned())?
            }
            "poll" | "map_hue" | "map_brightness" | "marker_hue" | "marker_brightness" => {
                let number = control::number(&value)
                    .ok_or_else(|| "This setting requires a number.".to_owned())?;
                match id {
                    "poll" => next.poll_seconds = number,
                    "map_hue" => next.map_hue = number,
                    "map_brightness" => next.map_brightness_percent = number,
                    "marker_hue" => next.marker_hue = number,
                    "marker_brightness" => next.marker_brightness_percent = number,
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

enum MapWorker {
    Quakes(Poller<Vec<Earthquake>>),
    Positions(Poller<Vec<Position>>),
}
enum MapData {
    Empty,
    Quakes(Arc<Vec<Earthquake>>),
    Positions(Arc<Vec<Position>>),
}

pub struct LiveMapScene {
    kind: MapKind,
    settings: MapSettings,
    worker: Option<MapWorker>,
    data: MapData,
    state: FeedState,
    startup_error: Option<String>,
    coastline: Raster,
    coast_brightness: i32,
    markers: Raster,
    marker_dirty: bool,
    labels: Vec<Option<char>>,
    marker_cells: Vec<usize>,
    label_width: u16,
    label_height: u16,
    labels_omitted: usize,
    status_line: String,
}

impl LiveMapScene {
    pub fn new(kind: MapKind, settings: &MapSettings, _env: &SceneEnv) -> Self {
        let mut scene = Self::offline(kind, settings.normalized());
        scene.start_worker();
        scene
    }
    fn offline(kind: MapKind, settings: MapSettings) -> Self {
        Self {
            kind,
            settings,
            worker: None,
            data: MapData::Empty,
            state: FeedState::default(),
            startup_error: None,
            coastline: Raster::default(),
            coast_brightness: -1,
            markers: Raster::default(),
            marker_dirty: true,
            labels: Vec::new(),
            marker_cells: Vec::new(),
            label_width: 0,
            label_height: 0,
            labels_omitted: 0,
            status_line: "Waiting for reported positions".into(),
        }
    }
    fn start_worker(&mut self) {
        self.worker = None;
        self.startup_error = None;
        let seconds = self.settings.effective_poll_seconds(self.kind);
        let result = match self.kind {
            MapKind::Earthquakes => fetch::earthquakes(seconds).map(MapWorker::Quakes),
            MapKind::Aircraft => fetch::aircraft(seconds).map(MapWorker::Positions),
            MapKind::Boats => fetch::boats(seconds).map(MapWorker::Positions),
        };
        match result {
            Ok(worker) => self.worker = Some(worker),
            Err(error) => self.startup_error = Some(error),
        }
    }
    /// Visual changes retain the last genuine positions; interval changes
    /// replace only this scene's owned poller, preserving last-good data.
    pub fn apply_settings(&mut self, settings: &MapSettings) -> bool {
        let next = settings.normalized();
        if next == self.settings {
            return false;
        }
        let restart = next.effective_poll_seconds(self.kind)
            != self.settings.effective_poll_seconds(self.kind);
        self.settings = next;
        self.marker_dirty = true;
        if restart {
            self.start_worker();
        }
        true
    }
    fn accept_quakes(&mut self, snapshot: Arc<Snapshot<Vec<Earthquake>>>) {
        // A newly started poller has not received anything yet. Preserve the
        // old source timestamps while its first replacement request runs.
        if snapshot.data.is_none() && snapshot.state.received_ms.is_none() {
            if let Some(error) = &snapshot.state.error {
                self.state.failed(error.clone());
            }
            return;
        }

        self.state = snapshot.state.clone();
        if let Some(data) = &snapshot.data {
            let same = matches!(&self.data,MapData::Quakes(before) if Arc::ptr_eq(before,data));
            if !same {
                self.data = MapData::Quakes(Arc::clone(data));
                self.marker_dirty = true;
            }
        }
    }
    fn accept_positions(&mut self, snapshot: Arc<Snapshot<Vec<Position>>>) {
        // A newly started poller has not received anything yet. Preserve the
        // old source timestamps while its first replacement request runs.
        if snapshot.data.is_none() && snapshot.state.received_ms.is_none() {
            if let Some(error) = &snapshot.state.error {
                self.state.failed(error.clone());
            }
            return;
        }

        self.state = snapshot.state.clone();
        if let Some(data) = &snapshot.data {
            let same = matches!(&self.data,MapData::Positions(before) if Arc::ptr_eq(before,data));
            if !same {
                self.data = MapData::Positions(Arc::clone(data));
                self.marker_dirty = true;
            }
        }
    }
    fn update_markers(
        &mut self,
        width: u16,
        height: u16,
        dot_width: usize,
        dot_height: usize,
        time: f32,
    ) {
        self.markers.resize(dot_width, dot_height);
        self.labels
            .resize(usize::from(width) * usize::from(height), None);
        self.labels.fill(None);
        self.marker_cells.clear();
        self.labels_omitted = 0;
        self.label_width = width;
        self.label_height = height;
        if width == 0 || height == 0 || dot_width == 0 || dot_height == 0 {
            return;
        }
        let intensity = self.settings.marker_brightness_percent as f32 / 100.0;
        let mut label_candidates = Vec::new();
        match &self.data {
            MapData::Empty => {}
            MapData::Positions(positions) => {
                for position in positions.iter() {
                    let Some(center) =
                        marker_center(position, width, height, dot_width, dot_height)
                    else {
                        continue;
                    };
                    self.marker_cells.push(center.2);
                    draw_vehicle(
                        &mut self.markers,
                        (center.0, center.1),
                        position.heading_degrees,
                        intensity,
                    );
                }
            }
            MapData::Quakes(quakes) => {
                for quake in quakes.iter() {
                    let Some(center) =
                        marker_center(&quake.position, width, height, dot_width, dot_height)
                    else {
                        continue;
                    };
                    self.marker_cells.push(center.2);
                    let magnitude = quake.magnitude.filter(|value| value.is_finite());
                    let radius = 1.0 + magnitude.unwrap_or(0.0).clamp(0.0, 9.0) as f32 * 0.22;
                    let phase = (time * 0.5 + (quake.position.observed_ms % 3000) as f32 / 3000.0)
                        .rem_euclid(1.0);
                    draw_pulse(
                        &mut self.markers,
                        (center.0, center.1),
                        radius,
                        phase,
                        intensity,
                    );
                    if self.settings.magnitude_labels {
                        let text = magnitude.map_or_else(|| "?".into(), magnitude_label);
                        // A malformed direct fixture must not grow an unbounded
                        // native label. Parsers already bound real magnitudes.
                        let text = if text.len() <= 12 { text } else { "?".into() };
                        label_candidates.push((center.2, text));
                    }
                }
            }
        }
        // Reserve every marker before placing any label, including markers of
        // events that will not fit a label. Four deterministic placements avoid
        // clipping or covering neighboring reported positions.
        let mut occupied = vec![false; self.labels.len()];
        for index in &self.marker_cells {
            occupied[*index] = true;
        }
        for (center, text) in label_candidates {
            if !place_label(
                &mut self.labels,
                &mut occupied,
                width,
                height,
                center,
                &text,
            ) {
                self.labels_omitted += 1;
            }
        }
        self.marker_dirty = false;
    }
    fn status_at(&self, now: i64) -> String {
        let interval = self.settings.effective_poll_seconds(self.kind);
        let count = match &self.data {
            MapData::Empty => 0,
            MapData::Positions(data) => data.len(),
            MapData::Quakes(data) => data.len(),
        };
        let stale = self
            .state
            .is_stale(now, (interval as i64).saturating_mul(3000));
        let condition = if self.state.error.is_some() || self.startup_error.is_some() {
            "last good / request error"
        } else if self.state.received_ms.is_none() {
            "waiting for observations"
        } else if stale {
            "last good / stale receipt"
        } else {
            "reported observations"
        };
        let mut status = format!(
            "{}; {}; {} records; observed {}; received {}; refresh {}s (floor {}s){}",
            self.kind.attribution(),
            condition,
            count,
            timestamp_age(self.state.observed_ms, now),
            timestamp_age(self.state.received_ms, now),
            interval,
            self.kind.minimum_poll_seconds(),
            if stale { "; stale transport" } else { "" }
        );
        if self.kind == MapKind::Earthquakes && self.settings.magnitude_labels {
            status.push_str(&format!(
                "; {} colliding or clipped labels omitted; markers retained",
                self.labels_omitted
            ));
        }
        if let Some(error) = self.startup_error.as_ref().or(self.state.error.as_ref()) {
            status.push_str("; ");
            status.extend(error.chars().filter(|c| !c.is_control()).take(240));
        }
        status
    }
}

impl Scene for LiveMapScene {
    fn reconfigure(&mut self, settings: &crate::AmbientSettings) -> bool {
        let next = match self.kind {
            MapKind::Earthquakes => &settings.earthquakes,
            MapKind::Aircraft => &settings.aircraft,
            MapKind::Boats => &settings.boats,
        };
        self.apply_settings(next);
        true
    }
    fn render(&mut self, frame: &mut Frame<'_>) {
        match &self.worker {
            Some(MapWorker::Quakes(worker)) => {
                if let Some(snapshot) = worker.try_snapshot() {
                    self.accept_quakes(snapshot);
                }
            }
            Some(MapWorker::Positions(worker)) => {
                if let Some(snapshot) = worker.try_snapshot() {
                    self.accept_positions(snapshot);
                }
            }
            None => {}
        }
        let dimensions_changed = self.coastline.width != frame.raster.width
            || self.coastline.height != frame.raster.height;
        if dimensions_changed || self.coast_brightness != self.settings.map_brightness_percent {
            self.coastline
                .resize(frame.raster.width, frame.raster.height);
            if self.coastline.width > 0 && self.coastline.height > 0 {
                map::coastline(
                    &mut self.coastline,
                    self.settings.map_brightness_percent as f32 / 100.0,
                );
            }
            self.coast_brightness = self.settings.map_brightness_percent;
        }
        if dimensions_changed
            || self.marker_dirty
            || self.kind == MapKind::Earthquakes
            || self.label_width != frame.width
            || self.label_height != frame.height
        {
            self.update_markers(
                frame.width,
                frame.height,
                frame.raster.width,
                frame.raster.height,
                frame.time.as_secs_f32(),
            );
        }
        frame.raster.dots.copy_from_slice(&self.coastline.dots);
        for (dot, marker) in frame.raster.dots.iter_mut().zip(&self.markers.dots) {
            *dot = dot.max(*marker);
        }
        let map_color = hue_rgb(self.settings.map_hue);
        let marker_color = hue_rgb(self.settings.marker_hue);
        frame.cell_colors.resize(
            usize::from(frame.width) * usize::from(frame.height),
            map_color,
        );
        frame.cell_colors.fill(map_color);
        for y in 0..frame.height {
            for x in 0..frame.width {
                let index = usize::from(y) * usize::from(frame.width) + usize::from(x);
                let ink = (0..4).any(|dy| {
                    (0..2).any(|dx| {
                        let column = usize::from(x) * 2 + dx;
                        let row = usize::from(y) * 4 + dy;
                        column < self.markers.width
                            && row < self.markers.height
                            && self.markers.dots[row * self.markers.width + column] > 0.0
                    })
                });
                if self.labels.get(index).is_some_and(Option::is_some) {
                    // Native glyphs do not pass through Braille intensity, so
                    // apply the marker brightness to their foreground color.
                    let brightness = self.settings.marker_brightness_percent as f32 / 100.0;
                    frame.cell_colors[index] =
                        marker_color.map(|channel| (f32::from(channel) * brightness) as u8);
                } else if ink {
                    frame.cell_colors[index] = marker_color;
                }
            }
        }
        let now = frame
            .now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_millis().min(i64::MAX as u128) as i64);
        self.status_line = self.status_at(now);
    }
    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn frames_per_second(&self) -> u32 {
        if self.kind == MapKind::Earthquakes {
            8
        } else {
            1
        }
    }
    fn native_glyph(&self, x: u16, y: u16) -> Option<char> {
        if self.settings.marker_brightness_percent == 0
            || x >= self.label_width
            || y >= self.label_height
        {
            return None;
        }
        self.labels
            .get(usize::from(y) * usize::from(self.label_width) + usize::from(x))
            .copied()
            .flatten()
    }
    fn status(&self) -> Option<String> {
        Some(self.status_line.clone())
    }
}

fn marker_center(
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

fn draw_vehicle(raster: &mut Raster, center: (f32, f32), heading: Option<f64>, intensity: f32) {
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
fn draw_pulse(raster: &mut Raster, center: (f32, f32), radius: f32, phase: f32, intensity: f32) {
    raster.line(center, center, radius * 0.7, intensity);
    let ring = radius + phase * 4.0;
    let width = raster.width as f32;
    let height = raster.height as f32;
    raster.curve(16, 0.35, intensity * (1.0 - phase) * 0.65, |fraction| {
        let angle = fraction * std::f32::consts::TAU;
        (
            center.0 + angle.cos() * ring / width,
            center.1 + angle.sin() * ring / height,
        )
    });
}
fn place_label(
    labels: &mut [Option<char>],
    occupied: &mut [bool],
    width: u16,
    height: u16,
    center: usize,
    text: &str,
) -> bool {
    let x = (center % usize::from(width)) as i32;
    let y = (center / usize::from(width)) as i32;
    let length = text.len() as i32;
    for (left, row) in [(x + 1, y), (x - length, y), (x, y + 1), (x, y - 1)] {
        if left < 0 || row < 0 || row >= i32::from(height) || left + length > i32::from(width) {
            continue;
        }
        let first = row as usize * usize::from(width) + left as usize;
        let last = first + text.len();
        if occupied[first..last].iter().any(|cell| *cell) {
            continue;
        }
        for (offset, character) in text.chars().enumerate() {
            labels[first + offset] = Some(character);
            occupied[first + offset] = true;
        }
        return true;
    }
    false
}
fn magnitude_label(number: f64) -> String {
    if number.fract() == 0.0 {
        return format!("{number:.1}");
    }
    let text = number.to_string();
    // Keep tiny reported magnitudes distinguishable from zero without
    // allowing a long decimal expansion to consume the whole viewport.
    if text.len() > 12 {
        format!("{number:.2e}")
    } else {
        text
    }
}

fn timestamp_age(time: Option<i64>, now: i64) -> String {
    let Some(time) = time else {
        return "unknown".into();
    };
    let stamp = chrono::DateTime::from_timestamp_millis(time).map_or_else(
        || format!("epoch-ms {time}"),
        |date| date.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    );
    if time > now {
        format!(
            "{stamp} ({}s ahead of clock)",
            time.saturating_sub(now) / 1000
        )
    } else {
        format!("{stamp} ({}s ago)", now.saturating_sub(time) / 1000)
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
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};
    #[test]
    fn reported_magnitude_labels_keep_tiny_nonzero_values() {
        for (value, expected) in [
            (0.0, "0.0"),
            (-0.7, "-0.7"),
            (0.01, "0.01"),
            (-0.001, "-0.001"),
            (1.25, "1.25"),
            (0.00000000000001, "1.00e-14"),
        ] {
            assert_eq!(magnitude_label(value), expected);
        }
    }

    fn quake(id: &str, longitude: f64, magnitude: Option<f64>) -> Earthquake {
        Earthquake {
            position: Position::new(id.into(), longitude, 0.0, 1000).unwrap(),
            magnitude,
            depth_km: None,
        }
    }
    fn paint(
        scene: &mut LiveMapScene,
        width: u16,
        height: u16,
        time: u64,
    ) -> (Raster, Vec<[u8; 3]>) {
        let mut raster = Raster::default();
        raster.resize(usize::from(width) * 2, usize::from(height) * 4);
        let mut colors = vec![[1, 2, 3]; 999];
        let mut frame = Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width,
            height,
            time: Duration::from_secs(time),
            wall: Duration::from_secs(time),
            now: UNIX_EPOCH + Duration::from_secs(100),
        };
        scene.render(&mut frame);
        (raster, colors)
    }
    #[test]
    fn replacement_worker_first_failure_preserves_last_good_metadata_for_both_feed_types() {
        let mut state = FeedState::default();
        state.received(90000, Some(1000));
        let mut error_state = FeedState::default();
        error_state.failed("first replacement request failed".into());
        let mut boats = LiveMapScene::offline(MapKind::Boats, MapSettings::default());
        boats.accept_positions(Arc::new(Snapshot {
            data: Some(Arc::new(vec![
                Position::new("a".into(), 0.0, 0.0, 1000).unwrap()
            ])),
            state: state.clone(),
        }));
        boats.accept_positions(Arc::new(Snapshot {
            data: None,
            state: error_state.clone(),
        }));
        assert_eq!(boats.state.received_ms, state.received_ms);
        assert_eq!(boats.state.observed_ms, state.observed_ms);
        assert_eq!(boats.state.error, error_state.error);
        let mut earthquakes = LiveMapScene::offline(MapKind::Earthquakes, MapSettings::default());
        earthquakes.accept_quakes(Arc::new(Snapshot {
            data: Some(Arc::new(vec![quake("a", 0.0, None)])),
            state: state.clone(),
        }));
        earthquakes.accept_quakes(Arc::new(Snapshot {
            data: None,
            state: error_state.clone(),
        }));
        assert_eq!(earthquakes.state.received_ms, state.received_ms);
        assert_eq!(earthquakes.state.observed_ms, state.observed_ms);
        assert_eq!(earthquakes.state.error, error_state.error);
        paint(&mut boats, 40, 12, 0);
        paint(&mut earthquakes, 40, 12, 0);
        assert_eq!(boats.marker_cells.len(), 1);
        assert_eq!(earthquakes.marker_cells.len(), 1);
    }
    #[test]
    fn initial_replacement_worker_snapshot_cannot_hide_last_good_timestamps() {
        let mut scene = LiveMapScene::offline(MapKind::Boats, MapSettings::default());
        let mut state = FeedState::default();
        state.received(90000, Some(1000));
        scene.accept_positions(Arc::new(Snapshot {
            data: Some(Arc::new(vec![
                Position::new("a".into(), 0.0, 0.0, 1000).unwrap()
            ])),
            state: state.clone(),
        }));
        scene.accept_positions(Arc::new(Snapshot::default()));
        assert_eq!(scene.state, state);
        paint(&mut scene, 40, 12, 0);
        assert_eq!(scene.marker_cells.len(), 1);
    }
    #[test]
    fn native_label_brightness_follows_marker_brightness_and_zero_hides_labels() {
        let mut scene = LiveMapScene::offline(
            MapKind::Earthquakes,
            MapSettings {
                marker_brightness_percent: 50,
                ..Default::default()
            },
        );
        scene.accept_quakes(Arc::new(Snapshot {
            data: Some(Arc::new(vec![quake("a", 0.0, Some(-0.7))])),
            state: FeedState::default(),
        }));
        let (_, colors) = paint(&mut scene, 40, 12, 0);
        let label_index = scene.labels.iter().position(Option::is_some).unwrap();
        assert_eq!(
            colors[label_index],
            hue_rgb(scene.settings.marker_hue).map(|channel| (f32::from(channel) * 0.5) as u8)
        );
        let settings = MapSettings {
            marker_brightness_percent: 0,
            ..scene.settings.clone()
        };
        scene.apply_settings(&settings);
        paint(&mut scene, 40, 12, 0);
        assert!((0..12).all(|y| (0..40).all(|x| scene.native_glyph(x, y).is_none())));
    }
    #[test]
    fn all_magnitudes_have_markers_and_zero_negative_unknown_labels() {
        let mut scene = LiveMapScene::offline(MapKind::Earthquakes, MapSettings::default());
        scene.accept_quakes(Arc::new(Snapshot {
            data: Some(Arc::new(vec![
                quake("a", -120.0, Some(-0.7)),
                quake("b", 0.0, Some(0.0)),
                quake("c", 120.0, None),
                quake("d", 60.0, Some(0.01)),
            ])),
            state: FeedState::default(),
        }));
        paint(&mut scene, 80, 24, 0);
        assert_eq!(scene.marker_cells.len(), 4);
        let text: String = scene.labels.iter().flatten().collect();
        assert!(text.contains("-0.7"));
        assert!(text.contains("0.0"));
        assert!(text.contains("0.01"));
        assert!(text.contains('?'));
        assert!(scene
            .marker_cells
            .iter()
            .all(|index| scene.labels[*index].is_none()));
        assert!(scene.labels.iter().flatten().all(char::is_ascii));
    }
    #[test]
    fn colors_brightness_and_coastline_cache_are_independent() {
        let settings = MapSettings {
            map_hue: 240,
            marker_hue: 0,
            map_brightness_percent: 40,
            marker_brightness_percent: 100,
            ..Default::default()
        };
        let mut scene = LiveMapScene::offline(MapKind::Boats, settings);
        scene.accept_positions(Arc::new(Snapshot {
            data: Some(Arc::new(vec![
                Position::new("a".into(), 0.0, 0.0, 1000).unwrap()
            ])),
            state: FeedState::default(),
        }));
        let (raster, colors) = paint(&mut scene, 80, 24, 0);
        assert!(colors.contains(&hue_rgb(240)));
        assert!(colors.contains(&hue_rgb(0)));
        assert_eq!(colors.len(), 1920);
        assert!(colors.iter().all(|c| *c != [1, 2, 3]));
        assert!(raster.dots.iter().any(|dot| *dot > 0.5));
        let coastline = scene.coastline.dots.clone();
        paint(&mut scene, 80, 24, 5);
        assert_eq!(scene.coastline.dots, coastline);
        let mut updated = scene.settings.clone();
        updated.marker_hue = 120;
        scene.apply_settings(&updated);
        let (_, colors) = paint(&mut scene, 80, 24, 5);
        assert_eq!(scene.coastline.dots, coastline);
        assert!(colors.contains(&hue_rgb(120)));
    }
    #[test]
    fn dateline_poles_tiny_and_empty_viewports_stay_bounded() {
        let mut scene = LiveMapScene::offline(MapKind::Earthquakes, MapSettings::default());
        scene.accept_quakes(Arc::new(Snapshot {
            data: Some(Arc::new(vec![
                quake("a", 180.0, Some(0.0)),
                quake("b", -180.0, None),
            ])),
            state: FeedState::default(),
        }));
        for (width, height) in [(1, 1), (0, 0), (2, 1), (80, 24)] {
            let (raster, colors) = paint(&mut scene, width, height, 0);
            assert!(raster.dots.iter().all(|dot| dot.is_finite()));
            assert_eq!(colors.len(), usize::from(width) * usize::from(height));
            assert!(scene.native_glyph(width, height).is_none());
            assert!(scene
                .marker_cells
                .iter()
                .all(|index| *index < scene.labels.len()));
        }
    }
    #[test]
    fn heading_changes_orientation_but_time_never_moves_observed_positions() {
        let mut scene = LiveMapScene::offline(MapKind::Aircraft, MapSettings::default());
        let mut position = Position::new("a".into(), 15.0, 20.0, 1000).unwrap();
        position.heading_degrees = Some(0.0);
        let snapshot = Arc::new(Snapshot {
            data: Some(Arc::new(vec![position.clone()])),
            state: FeedState::default(),
        });
        scene.accept_positions(snapshot);
        let (north, _) = paint(&mut scene, 80, 24, 0);
        let (later, _) = paint(&mut scene, 80, 24, 1000);
        assert_eq!(north.dots, later.dots);
        position.heading_degrees = Some(90.0);
        scene.accept_positions(Arc::new(Snapshot {
            data: Some(Arc::new(vec![position])),
            state: FeedState::default(),
        }));
        let (east, _) = paint(&mut scene, 80, 24, 0);
        assert_ne!(north.dots, east.dots);
    }
    #[test]
    fn failed_and_malformed_fetch_keeps_last_good_and_freshness_separate() {
        let mut scene = LiveMapScene::offline(MapKind::Boats, MapSettings::default());
        let data = Arc::new(vec![Position::new("a".into(), 0.0, 0.0, 1000).unwrap()]);
        let mut state = FeedState::default();
        state.received(90000, Some(1000));
        scene.accept_positions(Arc::new(Snapshot {
            data: Some(Arc::clone(&data)),
            state: state.clone(),
        }));
        let (before, _) = paint(&mut scene, 80, 24, 0);
        let error = super::super::parse::digitraffic(br#"{"broken":true}"#).unwrap_err();
        state.failed(error);
        scene.accept_positions(Arc::new(Snapshot {
            data: Some(data),
            state,
        }));
        let (after, _) = paint(&mut scene, 80, 24, 0);
        assert_eq!(before.dots, after.dots);
        let status = scene.status().unwrap();
        assert!(status.contains("Finnish waters"));
        assert!(status.contains("last good"));
        assert!(status.contains("observed 1970-01-01 00:00:01 UTC (99s ago)"));
        assert!(status.contains("received 1970-01-01 00:01:30 UTC (10s ago)"));
    }
    #[test]
    fn controls_announce_request_floors_and_defaults_round_trip() {
        let mut settings = MapSettings {
            poll_seconds: 1,
            ..MapSettings::default()
        };
        for (kind, floor) in [
            (MapKind::Earthquakes, 60),
            (MapKind::Aircraft, 900),
            (MapKind::Boats, 30),
        ] {
            assert_eq!(settings.effective_poll_seconds(kind), floor);
            assert!(settings
                .controls_for(kind)
                .iter()
                .find(|c| c.id == "poll")
                .unwrap()
                .help_detail
                .as_ref()
                .unwrap()
                .contains(&format!("{floor} s")));
        }
        assert_eq!(
            serde_json::from_str::<MapSettings>("{}").unwrap(),
            MapSettings::default()
        );
        assert!(settings
            .set_control("map_hue", ControlValue::Text("no".into()))
            .is_err());
    }
    #[test]
    fn earthquakes_pulse_without_changing_location_and_colliding_labels_omit_only_text() {
        let mut scene = LiveMapScene::offline(MapKind::Earthquakes, MapSettings::default());
        scene.accept_quakes(Arc::new(Snapshot {
            data: Some(Arc::new(vec![
                quake("a", 0.0, Some(5.0)),
                quake("b", 0.0, None),
            ])),
            state: FeedState::default(),
        }));
        let (first, _) = paint(&mut scene, 80, 24, 0);
        let centers = scene.marker_cells.clone();
        let (later, _) = paint(&mut scene, 80, 24, 1);
        assert_eq!(scene.marker_cells, centers);
        assert_ne!(first.dots, later.dots);
        assert_eq!(scene.marker_cells.len(), 2);
        assert!(scene
            .marker_cells
            .iter()
            .all(|i| scene.labels[*i].is_none()));
    }
}
