//! Public observed geography: cached coastlines and truthful live markers.
use super::{
    fetch,
    fleet_cache::{FleetBatch, FleetFeed, FleetMeta, FleetSource, FleetView}, // Shared source cache and custody.
    fleet_layer::FleetLayer, // Scene-owned preparation orchestration.
    map,
    map_markers::{marker_center, MarkerKey}, // Reuse reviewed pure geometry.
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
            Self::Boats => 60, // Default broader source; explicit regional source has its own floor.
        }
    }
    fn attribution(self) -> &'static str {
        match self {
            Self::Earthquakes=>"USGS — worldwide reported earthquakes, all reported magnitudes",
            Self::Aircraft=>"OpenSky — worldwide received airborne positions; coverage is incomplete; anonymous budget limits refresh to 15 min",
            Self::Boats=>"OpenSeaFeed — incomplete reported AIS reception; position age unavailable", // Default, never a complete physical inventory.
        }
    } // End block.
} // End block.

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] // Persist identity, never a catalogue index.
pub enum BoatSource {
    // Explicit alternatives; failures never switch providers.
    #[default] // Missing old fields migrate to the broader source.
    #[serde(rename = "openseafeed")] // Stable saved identifier.
    OpenSeaFeed, // Incomplete cross-region AIS aggregation.
    #[serde(rename = "digitraffic")] // Stable saved identifier.
    Digitraffic, // Explicit Finnish-water reception.
} // End block.
impl BoatSource {
    // Metadata is usable before any network request succeeds.
    fn fleet_source(self) -> FleetSource {
        match self {
            Self::OpenSeaFeed => FleetSource::OpenSeaFeed,
            Self::Digitraffic => FleetSource::Digitraffic,
        }
    } // Closed cache identity.
    fn summary(self) -> &'static str {
        match self {
            Self::OpenSeaFeed => {
                "OpenSeaFeed — incomplete reported AIS reception; position age unavailable"
            }
            Self::Digitraffic => "Digitraffic AIS — Finnish waters, not global ship coverage",
        }
    } // Lead with limitations.
    fn info(self) -> [(&'static str, &'static str, &'static str); 6] {
        // Short individual read-only fields, not a truncated status tail.
        let (credit, provider, coverage, changes, times) = match self { // Provider-specific terms and semantics.
            Self::OpenSeaFeed => (super::openseafeed::ATTRIBUTION, super::openseafeed::PROVIDER_URL, super::openseafeed::COVERAGE, super::openseafeed::CHANGES, "Position age unavailable. Snapshot build, latest ANY AIS update and network receipt are separate times."), // No invented coordinate freshness.
            Self::Digitraffic => ("Fintraffic / Digitraffic · CC BY 4.0", "https://www.digitraffic.fi/en/marine-traffic/", "Finnish waters only; incomplete received AIS coverage; reported identities are not authenticated.", "Invalid rows omitted; coordinates projected; unavailable headings remain unknown.", "timestampExternal is the reported position timestamp; local receipt is separate."), // Preserve regional source semantics.
        }; // End block.
        [
            ("boat_credit", "Read credit", credit),
            ("boat_provider", "Read provider URL", provider),
            (
                "boat_license",
                "Read license URL",
                "https://creativecommons.org/licenses/by/4.0/",
            ),
            ("boat_coverage", "Read coverage", coverage),
            ("boat_changes", "Read changes", changes),
            ("boat_time", "Read time semantics", times),
        ] // Each value opens in the existing text prompt.
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSettings {
    pub boat_source: BoatSource, // Persisted separately for the boat scene; ignored by quakes and aircraft.
    pub map_hue: i32,
    pub marker_hue: i32,
    pub map_brightness_percent: i32,
    pub marker_brightness_percent: i32,
    pub poll_seconds: i32,
    pub magnitude_labels: bool,
    pub show_heading: bool, // Aircraft/boat markers: one dot per vehicle unless this adds a heading arrow.
}
impl Default for MapSettings {
    fn default() -> Self {
        Self {
            boat_source: BoatSource::default(), // Missing legacy field selects OpenSeaFeed.
            map_hue: 210,
            marker_hue: 35,
            map_brightness_percent: 45, // Markers default to twice this.
            marker_brightness_percent: 90,
            poll_seconds: 60,
            magnitude_labels: true,
            show_heading: false,
        }
    }
}
impl MapSettings {
    fn minimum_poll_seconds(&self, kind: MapKind) -> u64 {
        if kind == MapKind::Boats {
            self.boat_source.fleet_source().floor()
        } else {
            kind.minimum_poll_seconds()
        }
    } // Source-aware floor.
    fn attribution(&self, kind: MapKind) -> &'static str {
        if kind == MapKind::Boats {
            self.boat_source.summary()
        } else {
            kind.attribution()
        }
    } // No regional data under a broader-source label.
    pub fn effective_poll_seconds(&self, kind: MapKind) -> u64 {
        // Faster requests cannot override source policy.
        (self.poll_seconds.clamp(5, 3600) as u64).max(self.minimum_poll_seconds(kind))
        // Local cache may serve earlier receipts without a new request.
    }
    pub fn controls_for(&self, kind: MapKind) -> Vec<Control> {
        let mut controls = self.controls();
        if kind != MapKind::Earthquakes {
            controls.retain(|row| row.id != "labels");
        } else {
            controls.retain(|row| row.id != "heading");
        }
        for row in &mut controls {
            if row.id == "poll" {
                row.help_detail = Some(format!(
                    "Effective interval: {} s; minimum client interval: {} s. {}", // Preserve the typed operation.
                    self.effective_poll_seconds(kind),
                    self.minimum_poll_seconds(kind), // Selected source floor.
                    self.attribution(kind)           // Selected source scope.
                ));
            }
        } // End block.
        if kind == MapKind::Boats {
            // Source and complete notes are boat-specific controls.
            controls.insert(0, Control::choice("boat_source", "Boat source", usize::from(self.boat_source == BoatSource::Digitraffic), &["OpenSeaFeed (broader)", "Digitraffic (Finnish)"], "Selecting a source clears the previous source. Errors retain only that source's last good data.")); // Stable choice maps to a persisted enum.
            controls.extend(self.boat_source.info().into_iter().map(|(id, label, value)| Control::text(id, label, value, "Read-only source information", "Open to read the complete value. This is a source notice, not an editable setting.")));
            // The existing text prompt exposes full values, not the two-line help tail.
        }
        controls
    }
}
impl SceneSettings for MapSettings {
    fn normalized(&self) -> Self {
        Self {
            boat_source: self.boat_source, // Normalization cannot silently change provider identity.
            map_hue: self.map_hue.clamp(0, 360),
            marker_hue: self.marker_hue.clamp(0, 360),
            map_brightness_percent: self.map_brightness_percent.clamp(0, 100),
            marker_brightness_percent: self.marker_brightness_percent.clamp(0, 100),
            poll_seconds: self.poll_seconds.clamp(5, 3600),
            magnitude_labels: self.magnitude_labels,
            show_heading: self.show_heading,
        }
    }
    fn controls(&self) -> Vec<Control> {
        let settings = self.normalized();
        vec![
            Control::slider("poll","Requested refresh",settings.poll_seconds,(5,3600,5)," s",
                "Provider request floors override faster requests; failed requests use bounded backoff.")
                .with_help_detail("Earthquakes: at least 60 s. Aircraft: at least 900 s. OpenSeaFeed: at least 60 s. Digitraffic Finnish-water ships: at least 30 s."), // Preserve the typed operation.
            Control::slider("map_hue","Map hue",settings.map_hue,(0,360,10),"°","Color of the embedded Natural Earth coastlines."),
            Control::slider("map_brightness","Map brightness",settings.map_brightness_percent,(0,100,5),"%","Coastline brightness, independent of reported-object markers."),
            Control::slider("marker_hue","Marker hue",settings.marker_hue,(0,360,10),"°","Color of markers and earthquake magnitude labels."),
            Control::slider("marker_brightness","Marker brightness",settings.marker_brightness_percent,(0,100,5),"%","Brightness of reported-object markers."),
            Control::toggle("heading","Heading arrows",settings.show_heading,"Aircraft and boats: draw a heading arrow around each dot. Off draws exactly one dot per reported vehicle."),
            Control::toggle("labels","Magnitude labels",settings.magnitude_labels,"Show reported magnitudes, including zero and negative values; '?' means unknown. Colliding labels are omitted, never event markers."),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let mut next = self.clone();
        match id {
            "boat_source" => {
                next.boat_source = match control::index(&value) {
                    Some(0) => BoatSource::OpenSeaFeed,
                    Some(1) => BoatSource::Digitraffic,
                    _ => return Err("Choose a listed boat source.".into()),
                };
            } // Never coerce invalid input into a provider.
            "boat_credit" | "boat_provider" | "boat_license" | "boat_coverage" | "boat_changes"
            | "boat_time" => {
                // Readable notices are not persisted user strings.
                if self
                    .boat_source
                    .info()
                    .into_iter()
                    .any(|(field, _, expected)| {
                        field == id && control::text(&value) == Some(expected)
                    })
                {
                    return Ok(false);
                } // Confirming unchanged text simply closes the prompt.
                return Err("Source information is read-only; cancel to return.".into());
                // A user cannot rewrite provenance by editing the prompt.
            } // End block.
            "heading" => {
                next.show_heading = control::boolean(&value)
                    .ok_or_else(|| "This setting requires on or off.".to_owned())?
            }
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
    Positions(FleetFeed), // Shared cache subscription, not a per-scene HTTP worker.
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
    markers: Raster,
    labels: Vec<Option<char>>,
    marker_cells: Vec<usize>,
    label_width: u16,
    label_height: u16,
    labels_omitted: usize,
    status_line: String,
    fleet: FleetLayer, // One owned preparer; raw vectors remain guarded by the source custodian.
    fleet_meta: FleetMeta, // Counts and source times of the latest RECEIVED data.
    network_enabled: bool, // Offline tests never start network subscriptions on settings edits.
    feed_retry_at: std::time::Duration, // Retry exhausted subscription admission without a frame-rate loop.
    resources: Option<crate::resources::AmbientResources>, // Reuse host admission on subscription retries.
}

impl LiveMapScene {
    // PALETTE (native Scene contract): `env.palette` is the shared look's current
    // palette. A custom native Scene receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. Monochrome scenes may ignore it. Today `PaletteScene` (scene.rs),
    // which `create_scene` wraps around every scene, shifts this scene's cell
    // colours onto the palette by brightness.
    pub fn new(kind: MapKind, settings: &MapSettings, env: &SceneEnv) -> Self {
        let mut scene = Self::offline(kind, settings.normalized());
        scene.resources = Some(env.resources.clone());
        scene.network_enabled = true; // Only the live constructor enables network subscriptions.
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
            markers: Raster::default(),
            labels: Vec::new(),
            marker_cells: Vec::new(),
            label_width: 0,
            label_height: 0,
            labels_omitted: 0,
            status_line: "Waiting for reported positions".into(),
            fleet: FleetLayer::default(), // No thread until positioned data needs preparation.
            fleet_meta: FleetMeta::default(), // No invented source metadata.
            network_enabled: false,       // Real constructor opts in after initialization.
            resources: None,
            feed_retry_at: std::time::Duration::from_secs(1), // Initial failed admission can retry after one scene second.
        }
    }
    fn start_worker(&mut self) {
        self.worker = None;
        self.startup_error = None;
        let seconds = self.settings.effective_poll_seconds(self.kind);
        let result = match self.kind {
            MapKind::Earthquakes => fetch::earthquakes(seconds).map(MapWorker::Quakes),
            MapKind::Aircraft => self
                .resources
                .clone()
                .ok_or_else(|| "Live fleet admission is unavailable".to_owned())
                .and_then(|resources| FleetFeed::start(resources, FleetSource::Aircraft, seconds))
                .map(MapWorker::Positions), // Share airborne source data and custody.
            MapKind::Boats => self
                .resources
                .clone()
                .ok_or_else(|| "Live fleet admission is unavailable".to_owned())
                .and_then(|resources| {
                    FleetFeed::start(resources, self.settings.boat_source.fleet_source(), seconds)
                })
                .map(MapWorker::Positions), // Default really selects the broader adapter.
        };
        match result {
            Ok(worker) => self.worker = Some(worker),
            Err(error) => self.startup_error = Some(error),
        }
    }
    /// Source changes isolate data; color and poll edits preserve fleet geometry.
    pub fn apply_settings(&mut self, settings: &MapSettings) -> bool {
        // No provider inference from cached contents.
        let next = settings.normalized(); // Validate before changing runtime state.
        if next == self.settings {
            return false;
        } // Preserve all generations for unchanged settings.
        let source_changed =
            self.kind == MapKind::Boats && next.boat_source != self.settings.boat_source; // Aircraft never adopts the saved boat choice.
        let interval_changed = next.effective_poll_seconds(self.kind)
            != self.settings.effective_poll_seconds(self.kind); // No new HTTP owner for a poll edit.
        let brightness_changed =
            next.marker_brightness_percent != self.settings.marker_brightness_percent; // Raster intensity is a geometry dependency.
        self.settings = next; // Source label and runtime identity transition together.
        if source_changed {
            // Clear even while the new source is unavailable.
            self.worker = None;
            self.data = MapData::Empty;
            self.state = FeedState::default();
            self.fleet_meta = FleetMeta::default(); // Cache custody prevents UI-side final raw disposal.
            self.fleet.clear_source();
            self.startup_error = None;
            self.status_line = format!(
                "{}; waiting for this source",
                self.settings.attribution(self.kind)
            );
            self.markers.dots.fill(0.0);
            self.marker_cells.clear();
            self.labels.fill(None); // No incompatible old-provider output.
        } else if brightness_changed {
            self.fleet.clear_geometry();
        } // Zero suppresses markers before another preparation completes.
        if let Some(MapWorker::Positions(feed)) = &self.worker {
            feed.set_interval(self.settings.effective_poll_seconds(self.kind));
        } // No restart, no geometry invalidation.
        if self.network_enabled
            && (source_changed || (interval_changed && self.kind == MapKind::Earthquakes))
        {
            self.start_worker();
        } // Only quakes retain their existing poller model.
        true // Settings changed; geometry changes only for actual dependencies.
    } // End block.
    fn accept_fleet(&mut self, snapshot: Arc<FleetView>) {
        // This path is fed only by the selected source subscription.
        self.state = snapshot.state.clone();
        if let Some(data) = &snapshot.data {
            self.fleet_meta = data.meta;
            let positions = &data.positions;
            let same = matches!(
                &self.data,
                MapData::Positions(before) if Arc::ptr_eq(before, positions)
            );
            if !same {
                self.data = MapData::Positions(Arc::clone(positions));
                self.fleet
                    .set_data(Arc::clone(data), snapshot.state.received_ms);
            }
        } // Batch owner carries the retained-storage charge into marker preparation.
    }
    #[cfg(test)]
    fn accept_positions(&mut self, snapshot: Arc<Snapshot<Vec<Position>>>) {
        // Keep snapshot-shaped fixtures readable while exercising the current fleet transition.
        if snapshot.data.is_none() && snapshot.state.received_ms.is_none() {
            if let Some(error) = &snapshot.state.error {
                self.state.failed(error.clone());
            }
            return;
        }
        let data = snapshot.data.as_ref().map(|positions| {
            Arc::new(FleetBatch {
                positions: Arc::clone(positions),
                meta: FleetMeta::default(),
                _storage: None,
            })
        });
        self.accept_fleet(Arc::new(FleetView {
            data,
            state: snapshot.state.clone(),
            generation: 0,
        }));
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
            MapData::Positions(_) => {} // Fleet geometry is prepared only by the reviewed owned worker.
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
                    let offset = quake
                        .position
                        .observed_ms // Offset is visual, never an invented observation.
                        .map_or(0.0, |time| (time % 3000) as f32 / 3000.0); // Unknown time uses a neutral animation phase.
                    let phase = (time * 0.5 + offset).rem_euclid(1.0); // Preserve the existing pulse for known event times.
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
        let heading = if self.kind == MapKind::Earthquakes {
            self.kind.attribution().to_owned()
        } else {
            // Pending/error state must precede long provenance text.
            let provider = match self.kind {
                MapKind::Aircraft => "OpenSky",
                _ if self.settings.boat_source == BoatSource::Digitraffic => "Digitraffic",
                _ => "OpenSeaFeed",
            }; // Compact but exact source identity.
            let health = if self.state.error.is_some() || self.startup_error.is_some() {
                "error; "
            } else if stale {
                "stale; "
            } else {
                ""
            }; // Keep transport failures visible under truncation.
            format!("{provider}: {health}{}", self.fleet.brief()) // The full source limitations remain independently openable in Settings.
        }; // End block.
        let mut status = format!(
            "{}; {}; {} retained positions; latest known fix {}; received {}; refresh {}s (floor {}s){}", // Preserve the typed operation.
            heading, // Preparation and transport condition lead the visible status.
            condition,
            count,
            timestamp_age(self.state.observed_ms, now),
            timestamp_age(self.state.received_ms, now),
            interval,
            self.settings.minimum_poll_seconds(self.kind), // Source-specific floor.
            if stale { "; stale transport" } else { "" }
        );
        if self.kind != MapKind::Earthquakes {
            // Rendered and received data are distinct during preparation.
            status.push_str("; ");
            status.push_str(self.settings.attribution(self.kind)); // Preserve full source scope after the compact leading state.
            let meta = self.fleet_meta; // No raw-position iteration on the UI.
            status.push_str(&format!("; {}; raw rows {}; malformed {}; unpositioned {}; filtered {}; excluded SAR {} / aids {} / distress {} / coast-group {}; unusual retained {}; known fixes {}; snapshot build {}; latest ANY AIS update {}", self.fleet.status(), meta.counts.records, meta.counts.malformed, meta.counts.unpositioned, meta.filtered, meta.counts.sar_aircraft, meta.counts.navigation_aids, meta.counts.distress_devices, meta.counts.coast_or_group, meta.counts.unusual_retained, meta.known_fixes, timestamp_age(meta.generated_ms, now), timestamp_age(meta.latest_ais_ms, now)));
            // None of these times is promoted to a coordinate fix.
        } // End block.
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
        if self.network_enabled
            && self.kind != MapKind::Earthquakes
            && self.worker.is_none()
            && frame.wall >= self.feed_retry_at
        {
            // Subscription admission may have recovered.
            self.feed_retry_at = frame.wall.saturating_add(std::time::Duration::from_secs(1)); // No retry storm while slots remain occupied.
            self.start_worker(); // This attaches to the existing service, never starts a per-scene HTTP thread.
        } // End block.
        match &self.worker {
            Some(MapWorker::Quakes(worker)) => {
                if let Some(snapshot) = worker.try_snapshot() {
                    self.accept_quakes(snapshot);
                }
            }
            Some(MapWorker::Positions(worker)) => {
                match worker.try_snapshot() {
                    // Shared-cache reads never wait on the source owner.
                    Ok(Some(snapshot)) => self.accept_fleet(snapshot), // Original network receipt is preserved.
                    Ok(None) => {} // Busy: retain the current frame.
                    Err(error) => self.state.failed(error), // Expose owner/publication failures.
                }
            }
            None => {}
        }
        let dimensions_changed = self.coastline.width != frame.raster.width
            || self.coastline.height != frame.raster.height;
        if dimensions_changed {
            self.coastline
                .resize(frame.raster.width, frame.raster.height);
            if self.coastline.width > 0 && self.coastline.height > 0 {
                // Full tone: brightness is applied to the cell colour so a
                // dimmer map is not thinned out by the shared dither.
                map::coastline(&mut self.coastline, 1.0);
            }
        }
        if self.kind == MapKind::Earthquakes {
            // Pulses remain scene-time dependent and retain every event marker.
            self.update_markers(
                frame.width,
                frame.height,
                frame.raster.width,
                frame.raster.height,
                frame.time.as_secs_f32(),
            ); // Existing all-magnitude path.
        } else {
            // Fleet work is only submission, polling and viewport-sized blending.
            self.fleet.update(
                MarkerKey {
                    request_generation: 0,
                    data_generation: 0,
                    kind: self.kind,
                    dot_width: frame.raster.width,
                    dot_height: frame.raster.height,
                    cell_width: usize::from(frame.width),
                    cell_height: usize::from(frame.height),
                    marker_brightness: self.settings.marker_brightness_percent as u8,
                    show_heading: self.settings.show_heading,
                },
                frame.wall,
            ); // The layer assigns both monotonic generations.
            self.label_width = frame.width;
            self.label_height = frame.height; // Preserve native-glyph bounds after resize.
            self.labels
                .resize(usize::from(frame.width) * usize::from(frame.height), None);
            self.labels.fill(None); // Fleet labels are not invented.
            self.marker_cells.clear(); // Occupancy is deduplicated bookkeeping, not source sampling.
            if let Some(prepared) = self.fleet.prepared() {
                // Markers bypass the shared dither: each lit dot becomes a
                // native Braille glyph, so a lone aircraft over open ocean
                // is as visible as one inside a dense cluster.
                let lit = 0.3 * self.settings.marker_brightness_percent as f32 / 100.0;
                let raster = &prepared.raster;
                let columns = usize::from(frame.width);
                if raster.width == columns * 2 && raster.height == usize::from(frame.height) * 4 {
                    const BITS: [[u32; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
                    for (index, label) in self.labels.iter_mut().enumerate() {
                        let (row, column) = (index / columns, index % columns);
                        let mut bits = 0u32;
                        for (dy, bit_row) in BITS.iter().enumerate() {
                            for (dx, bit) in bit_row.iter().enumerate() {
                                let dot = (row * 4 + dy) * raster.width + column * 2 + dx;
                                if raster.dots[dot] > 0.0 && raster.dots[dot] >= lit {
                                    bits |= bit;
                                }
                            }
                        }
                        if bits != 0 {
                            *label = char::from_u32(0x2800 + bits);
                        }
                    }
                }
                self.marker_cells.extend(
                    prepared
                        .occupied_centers
                        .iter()
                        .enumerate()
                        .filter_map(|(index, occupied)| occupied.then_some(index)),
                );
            } // At most one index per viewport cell.
        }
        let markers = &self.markers; // Fleet markers are native glyphs above; only quake pulses use the dithered raster.
        frame.raster.dots.copy_from_slice(&self.coastline.dots);
        for (dot, marker) in frame.raster.dots.iter_mut().zip(&markers.dots) {
            // Preserve the typed operation.
            *dot = dot.max(*marker);
        }
        let map_color = hue_rgb(self.settings.map_hue).map(|channel| {
            (f32::from(channel) * self.settings.map_brightness_percent as f32 / 100.0) as u8
        }); // Brightness lives in the colour, not in thinned-out dots.
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
                        column < markers.width // Preserve the typed operation.
                            && row < markers.height // Preserve the typed operation.
                            && markers.dots[row * markers.width + column] > 0.0 // Preserve the typed operation.
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
    #[test] // Provider selection, defaults and source notices use real settings controls.
    fn boat_source_and_readable_notices_have_stable_persistence() {
        // No runtime or network required.
        let mut settings: MapSettings = serde_json::from_str("{}").unwrap(); // Legacy configuration lacks the new field.
        assert_eq!(settings.boat_source, BoatSource::OpenSeaFeed); // Broader source is the actual default.
        for (index, source, saved, floor) in [
            (0, BoatSource::OpenSeaFeed, "openseafeed", 60),
            (1, BoatSource::Digitraffic, "digitraffic", 30),
        ] {
            // Both explicit sources.
            settings
                .set_control("boat_source", ControlValue::Index(index))
                .unwrap();
            settings.poll_seconds = 1; // Exercise source-aware clamping.
            assert_eq!(settings.boat_source, source);
            assert_eq!(settings.effective_poll_seconds(MapKind::Boats), floor); // Cadence follows identity.
            assert_eq!(
                serde_json::to_value(&settings).unwrap()["boat_source"],
                saved
            ); // Persist identity, not option order.
            let controls = settings.controls_for(MapKind::Boats); // Actual caller-facing control inventory.
            for (id, _, value) in source.info() {
                // Every complete credit/URL/limitation is independently accessible.
                let row = controls.iter().find(|row| row.id == id).unwrap(); // No status-only hidden strings.
                assert!(matches!(row.kind, crate::control::ControlKind::Text { .. }));
                assert_eq!(row.value, ControlValue::Text(value.into())); // Openable full-value prompt contract.
                assert!(!settings
                    .set_control(id, ControlValue::Text(value.into()))
                    .unwrap()); // Read-only confirmation preserves settings.
                assert!(settings
                    .set_control(id, ControlValue::Text("changed credit".into()))
                    .is_err()); // Provenance cannot be rewritten.
            } // End block.
        } // End block.
        assert!(settings
            .set_control("boat_source", ControlValue::Index(2))
            .is_err()); // Unknown providers are not coerced.
        assert!(serde_json::from_str::<MapSettings>(r#"{"boat_source":"invented"}"#).is_err()); // Invalid saved identities fail explicitly.
        assert!(!settings
            .controls_for(MapKind::Aircraft)
            .iter()
            .any(|row| row.id.starts_with("boat_"))); // No unrelated aircraft controls.
    } // End block.
    #[test] // Exercise settings transitions through the actual asynchronous scene renderer.
    fn fleet_geometry_survives_color_and_poll_edits_but_not_source_or_brightness() {
        // Synthetic source data only.
        let mut scene = LiveMapScene::offline(MapKind::Boats, MapSettings::default()); // No subscriptions or HTTP in this fixture.
        let data = Arc::new(vec![
            Position::new("fixture".into(), 0.0, 0.0, None).unwrap()
        ]); // Usable unknown-age position.
        let state = FeedState {
            received_ms: Some(1000),
            ..Default::default()
        }; // Separate transport metadata.
        scene.accept_positions(Arc::new(Snapshot {
            data: Some(Arc::clone(&data)),
            state: state.clone(),
        })); // Actual input transition.
        paint(&mut scene, 80, 24, 0);
        let old_key = scene.fleet.prepared().unwrap().key;
        let dots = scene.fleet.prepared().unwrap().raster.dots.clone(); // Actual worker result.
        let next = MapSettings {
            map_hue: 90,
            marker_hue: 180,
            map_brightness_percent: 50,
            poll_seconds: 120,
            ..scene.settings.clone()
        }; // None changes marker geometry.
        scene.apply_settings(&next);
        paint(&mut scene, 80, 24, 0); // Reconfigure and render the production path.
        assert_eq!(scene.fleet.prepared().unwrap().key, old_key);
        assert_eq!(scene.fleet.prepared().unwrap().raster.dots, dots); // No redundant geometry job.
        let mut failed = state;
        failed.failed("offline".into());
        scene.accept_positions(Arc::new(Snapshot {
            data: Some(data),
            state: failed,
        })); // Same raw Arc, new error metadata.
        paint(&mut scene, 80, 24, 0);
        assert_eq!(scene.fleet.prepared().unwrap().key, old_key); // Failure does not rebuild or rejuvenate data.
        scene.apply_settings(&MapSettings {
            marker_brightness_percent: 0,
            ..scene.settings.clone()
        }); // Suppress incompatible intensity immediately.
        assert!(scene.fleet.prepared().is_none()); // No need to wait for the next frame or worker completion.
        scene.apply_settings(&MapSettings {
            boat_source: BoatSource::Digitraffic,
            ..scene.settings.clone()
        }); // Explicit source transition.
        assert!(matches!(scene.data, MapData::Empty));
        assert_eq!(scene.state, FeedState::default());
        assert!(scene.fleet.prepared().is_none()); // No cross-source data, receipt or geometry.
        assert!(scene.status().unwrap().contains("Finnish waters")); // Source status updates before a new receipt arrives.
    } // End block.
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
            position: Position::new(id.into(), longitude, 0.0, Some(1000)).unwrap(), // Existing fixture supplies a known coordinate time.
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
        let started = std::time::Instant::now(); // Test-only bounded wait; production render never sleeps.
        loop {
            // Exercise the actual integrated worker path, not a synchronous substitute.
            frame.wall = Duration::from_secs(time).saturating_add(started.elapsed()); // Permit bounded startup-admission retries in parallel tests.
            scene.render(&mut frame); // Same render path as the client.
            if scene.kind == MapKind::Earthquakes
                || !matches!(&scene.data, MapData::Positions(_))
                || scene.settings.marker_brightness_percent == 0
                || width == 0
                || height == 0
                || scene.fleet.ready()
            {
                break;
            } // Explicit ready/empty cases.
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "{}",
                scene.fleet.status()
            ); // No indefinitely hung fixture.
            std::thread::sleep(Duration::from_millis(1)); // Worker gets execution time without UI-path blocking.
        } // End block.
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
                Position::new("a".into(), 0.0, 0.0, Some(1000)).unwrap(), // Existing fixture supplies a known coordinate time.
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
                Position::new("a".into(), 0.0, 0.0, Some(1000)).unwrap(), // Existing fixture supplies a known coordinate time.
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
                Position::new("a".into(), 0.0, 0.0, Some(1000)).unwrap(), // Existing fixture supplies a known coordinate time.
            ])),
            state: FeedState::default(),
        }));
        let (raster, colors) = paint(&mut scene, 80, 24, 0);
        assert!(colors.contains(&hue_rgb(240).map(|c| (f32::from(c) * 0.4) as u8)));
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
        let settings = MapSettings {
            show_heading: true,
            ..Default::default()
        };
        let mut scene = LiveMapScene::offline(MapKind::Aircraft, settings);
        let mut position = Position::new("a".into(), 15.0, 20.0, Some(1000)).unwrap(); // Existing fixture supplies a known coordinate time.
        position.heading_degrees = Some(0.0);
        let snapshot = Arc::new(Snapshot {
            data: Some(Arc::new(vec![position.clone()])),
            state: FeedState::default(),
        });
        scene.accept_positions(snapshot);
        paint(&mut scene, 80, 24, 0);
        let north = scene.labels.clone();
        paint(&mut scene, 80, 24, 1000);
        assert_eq!(north, scene.labels);
        position.heading_degrees = Some(90.0);
        scene.accept_positions(Arc::new(Snapshot {
            data: Some(Arc::new(vec![position])),
            state: FeedState::default(),
        }));
        paint(&mut scene, 80, 24, 0);
        assert_ne!(north, scene.labels);
    }
    #[test]
    fn default_aircraft_are_single_undithered_dots_twice_as_bright_as_the_map() {
        let mut scene = LiveMapScene::offline(MapKind::Aircraft, MapSettings::default());
        let mut position = Position::new("ocean".into(), -35.0, 40.0, Some(1000)).unwrap(); // Mid-Atlantic, far from any land.
        position.heading_degrees = Some(45.0);
        scene.accept_positions(Arc::new(Snapshot {
            data: Some(Arc::new(vec![position])),
            state: FeedState::default(),
        }));
        let (_, colors) = paint(&mut scene, 80, 24, 0);
        let glyphs = scene.labels.iter().flatten().collect::<Vec<_>>();
        assert_eq!(glyphs.len(), 1);
        assert!(
            (u32::from(*glyphs[0]) - 0x2800).count_ones() == 1,
            "one dot per plane"
        );
        let index = scene.labels.iter().position(Option::is_some).unwrap();
        let map = hue_rgb(scene.settings.map_hue);
        let plane = colors[index];
        let map_color = colors.iter().find(|c| **c != plane).unwrap();
        let luma = |c: [u8; 3]| f32::from(*c.iter().max().unwrap());
        assert!(luma(plane) / luma(*map_color).max(1.0) >= 1.9);
        assert_ne!(*map_color, map); // Map colour is scaled by its brightness.
    }
    #[test]
    fn failed_and_malformed_fetch_keeps_last_good_and_freshness_separate() {
        let mut scene = LiveMapScene::offline(
            MapKind::Boats,
            MapSettings {
                boat_source: BoatSource::Digitraffic,
                ..Default::default()
            },
        ); // Preserve the typed operation.
        let data = Arc::new(vec![
            Position::new("a".into(), 0.0, 0.0, Some(1000)).unwrap()
        ]); // Existing fixture supplies a known coordinate time.
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
        assert!(status.contains("latest known fix 1970-01-01 00:00:01 UTC (99s ago)")); // Preserve the typed operation.
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
            (MapKind::Boats, 60), // OpenSeaFeed is the missing-field default.
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
