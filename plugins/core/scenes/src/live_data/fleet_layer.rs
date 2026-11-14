//! Scene-owned fleet geometry: the positions of the latest batch rasterised for the current
//! viewport. A scene owns no worker, so a stale raster is rebuilt in place, once, when the data
//! or one of its geometry dependencies changed; between changes a frame costs nothing.
use super::{
    fleet::FleetBatch,
    map_markers::{prepare_positions, MarkerKey, MarkerRequest, PreparedMarkers},
    model::Position,
};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct FleetLayer {
    positions: Option<Arc<Vec<Position>>>,
    data_generation: u64,
    desired: Option<MarkerKey>,
    prepared: Option<(u64, PreparedMarkers)>,
    received_ms: Option<i64>,
    rendered_received_ms: Option<i64>,
    error: Option<String>,
}

impl FleetLayer {
    /// A different provider must never inherit old markers or times.
    pub fn clear_source(&mut self) {
        self.data_generation = self.data_generation.wrapping_add(1);
        self.positions = None;
        self.desired = None;
        self.prepared = None;
        self.received_ms = None;
        self.rendered_received_ms = None;
        self.error = None;
    }

    /// Brightness changes hide the incompatible output at once.
    pub fn clear_geometry(&mut self) {
        self.prepared = None;
        self.rendered_received_ms = None;
    }

    /// Accepts the batch of the latest successful refresh; the same positions `Arc` again is
    /// a receipt-only change and causes no geometry work.
    pub fn set_data(&mut self, batch: &FleetBatch, received_ms: Option<i64>) {
        if self
            .positions
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, &batch.positions))
        {
            return;
        }
        self.data_generation = self.data_generation.wrapping_add(1);
        self.positions = Some(Arc::clone(&batch.positions));
        self.received_ms = received_ms;
    }

    /// Rebuilds the raster when the data or `key` differs from what is prepared.
    pub fn update(&mut self, key: MarkerKey) {
        let Some(positions) = &self.positions else {
            return;
        };
        self.desired = Some(key);
        if key.marker_brightness == 0
            || key.cell_width == 0
            || key.cell_height == 0
            || key.dot_width == 0
            || key.dot_height == 0
        {
            self.prepared = None;
            self.rendered_received_ms = None;
            return;
        }
        if self
            .prepared
            .as_ref()
            .is_some_and(|(generation, old)| *generation == self.data_generation && old.key == key)
        {
            return;
        }
        let request = MarkerRequest {
            key,
            positions: Arc::clone(positions),
        };
        match prepare_positions(&request) {
            Some(prepared) => {
                self.prepared = Some((self.data_generation, prepared));
                self.rendered_received_ms = self.received_ms;
                self.error = None;
            }
            None => self.error = Some("marker preparation refused this viewport".into()),
        }
    }

    pub fn prepared(&self) -> Option<&PreparedMarkers> {
        self.prepared.as_ref().map(|(_, prepared)| prepared)
    }

    /// Whether the displayed raster is the one of the current data and viewport.
    pub fn ready(&self) -> bool {
        self.desired.is_some_and(|key| {
            self.prepared.as_ref().is_some_and(|(generation, old)| {
                *generation == self.data_generation && old.key == key
            })
        })
    }

    /// The preparation state first, before any truncatable metadata.
    pub fn brief(&self) -> String {
        let rendered = self.prepared().map_or(0, |p| p.accepted_positions);
        if self.error.is_some() {
            return format!("preparer error; drawn {rendered}");
        }
        if self.ready() {
            return format!("drawn {rendered}");
        }
        if self.desired.is_some_and(|key| key.marker_brightness == 0) {
            return "markers hidden".into();
        }
        format!(
            "preparing; previous {rendered}; received {}",
            self.positions.as_ref().map_or(0, |p| p.len())
        )
    }

    pub fn status(&self) -> String {
        let received = self
            .positions
            .as_ref()
            .map_or(0, |positions| positions.len());
        let (rendered, centers) = self
            .prepared()
            .map_or((0, 0), |p| (p.accepted_positions, p.unique_center_cells));
        format!(
            "{rendered} positions / {centers} occupied cells drawn (receipt {:?}); received generation {}: {received} positions (receipt {:?}){}",
            self.rendered_received_ms,
            self.data_generation,
            self.received_ms,
            self.error
                .as_ref()
                .map_or(String::new(), |error| format!("; {error}"))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::{fleet::FleetMeta, maps::MapKind};
    use super::*;

    fn batch(count: usize) -> FleetBatch {
        FleetBatch {
            positions: Arc::new(
                (0..count)
                    .map(|index| {
                        Position::new(format!("v{index}"), index as f64, 10.0, Some(0)).unwrap()
                    })
                    .collect(),
            ),
            meta: FleetMeta::default(),
        }
    }

    fn key(brightness: u8) -> MarkerKey {
        MarkerKey {
            kind: MapKind::Boats,
            dot_width: 160,
            dot_height: 96,
            cell_width: 80,
            cell_height: 24,
            marker_brightness: brightness,
            show_heading: false,
        }
    }

    #[test]
    fn nothing_is_prepared_before_the_first_batch() {
        let mut layer = FleetLayer::default();
        layer.update(key(90));
        assert!(layer.prepared().is_none());
    }

    #[test]
    fn a_batch_is_prepared_once_per_viewport_and_a_receipt_only_change_costs_nothing() {
        let mut layer = FleetLayer::default();
        let first = batch(5);
        layer.set_data(&first, Some(100));
        layer.update(key(90));
        assert!(layer.ready());
        assert_eq!(layer.prepared().unwrap().accepted_positions, 5);
        let before = layer.data_generation;
        layer.set_data(&first, Some(200));
        assert_eq!(layer.data_generation, before);
        assert_eq!(layer.received_ms, Some(100));
        layer.update(key(90));
        assert!(layer.ready());
    }

    #[test]
    fn new_data_and_a_new_viewport_each_rebuild_and_zero_brightness_hides_markers() {
        let mut layer = FleetLayer::default();
        layer.set_data(&batch(3), Some(1));
        layer.update(key(90));
        layer.set_data(&batch(7), Some(2));
        assert!(!layer.ready());
        layer.update(key(90));
        assert_eq!(layer.prepared().unwrap().accepted_positions, 7);
        assert_eq!(layer.rendered_received_ms, Some(2));
        let mut bigger = key(90);
        bigger.dot_width = 320;
        layer.update(bigger);
        assert!(layer.ready());
        layer.update(key(0));
        assert!(layer.prepared().is_none());
    }

    #[test]
    fn a_source_change_drops_markers_and_times() {
        let mut layer = FleetLayer::default();
        layer.set_data(&batch(3), Some(1));
        layer.update(key(90));
        layer.clear_source();
        assert!(layer.prepared().is_none());
        assert_eq!(layer.received_ms, None);
        layer.update(key(90));
        assert!(layer.prepared().is_none());
    }

    #[test]
    fn an_invalid_viewport_is_reported_not_silently_dropped() {
        let mut layer = FleetLayer::default();
        layer.set_data(&batch(1), Some(1));
        let mut huge = key(90);
        huge.dot_width = usize::MAX;
        layer.update(huge);
        assert!(layer.prepared().is_none());
        assert!(layer.status().contains("refused"));
    }
}
