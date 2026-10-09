//! Scene-owned orchestration of the reviewed marker worker; no duplicate rasterizer.
use super::{
    fleet_cache::FleetBatch,
    map_markers::{
        MarkerError, MarkerKey, MarkerRequest, MarkerWorker, PreparedMarkers, SubmitState,
    },
    model::Position,
}; // Actual released API.
use std::{sync::Arc, time::Duration}; // Raw inputs are additionally protected by fleet-cache custody.
#[derive(Default)] // Construction performs no thread creation or fleet traversal.
pub(super) struct FleetLayer {
    // One desired request and one accepted viewport-sized result.
    worker: Option<MarkerWorker>, // Reused across source, data and settings changes.
    positions: Option<Arc<Vec<Position>>>, // Clone the Arc, never the vector.
    owner: Option<Arc<FleetBatch>>, // Carries storage admission with every retained position Arc.
    generation: u64,              // Monotonic request generation, including A -> B -> A.
    data_generation: u64, // Changes only when the positions Arc changes or the source clears.
    desired: Option<MarkerKey>, // Complete geometry dependencies.
    submitted: Option<u64>, // Busy keeps this unset for a later retry.
    prepared: Option<Arc<PreparedMarkers>>, // May be older SAME-source, compatible geometry.
    received_ms: Option<i64>, // Receipt of the desired data, not a coordinate time.
    rendered_received_ms: Option<i64>, // Receipt corresponding to the actually displayed data.
    retry_at: Duration,   // Spawn retry uses the supplied scene wall clock.
    error: Option<String>, // Failures remain visible rather than silently freezing.
} // End block.
impl FleetLayer {
    // Every presentation method is O(1) plus viewport-sized work in the caller.
    fn invalidate(&mut self) {
        // Never reuse a request identity.
        self.generation = self
            .generation
            .checked_add(1)
            .expect("marker generation exhausted"); // Exhaustion fails the scene rather than accepting an ABA result.
        self.desired = None;
        self.submitted = None; // Latest input must be submitted again.
        if let Some(worker) = &self.worker {
            worker.invalidate(self.generation);
        } // Cancellation does not wait for a lock.
    } // End block.
    pub fn clear_source(&mut self) {
        // A different provider must never inherit old markers or times.
        self.invalidate();
        self.data_generation = self
            .data_generation
            .checked_add(1)
            .expect("data generation exhausted"); // Include source transitions in monotonic identities.
        self.positions = None;
        self.owner = None;
        self.prepared = None;
        self.received_ms = None;
        self.rendered_received_ms = None; // Cache custody owns final raw-vector reclamation.
    } // End block.
    pub fn clear_geometry(&mut self) {
        self.invalidate();
        self.prepared = None;
        self.rendered_received_ms = None;
    } // Brightness changes hide incompatible output immediately.
    pub fn set_data(&mut self, owner: Arc<FleetBatch>, received_ms: Option<i64>) {
        let positions = Arc::clone(&owner.positions);
        // Called only for admitted source data.
        if self
            .positions
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, &positions))
        {
            return;
        } // Error/receipt-only publications cause no geometry work.
        self.invalidate();
        self.data_generation = self
            .data_generation
            .checked_add(1)
            .expect("data generation exhausted"); // Data identity is not a provider timestamp.
        self.positions = Some(positions);
        self.owner = Some(owner);
        self.received_ms = received_ms; // The previous compatible prepared layer stays visible until replacement.
    } // End block.
    pub fn update(&mut self, mut key: MarkerKey, wall: Duration) {
        // Caller supplies dimensions, kind and marker brightness only.
        if self.positions.is_none() {
            return;
        } // No invented empty data before the first response.
        key.data_generation = self.data_generation;
        key.request_generation = self.generation; // Scene-owned generation authority.
        if self.desired != Some(key) {
            // Data, dimensions, kind or brightness changed.
            self.invalidate();
            key.request_generation = self.generation;
            self.desired = Some(key); // Full-key generation remains monotonic.
            if self
                .prepared
                .as_ref()
                .is_some_and(|old| !compatible(old.key, key))
            {
                self.prepared = None;
                self.rendered_received_ms = None;
            } // Never stretch/reuse incompatible geometry.
        } // End block.
        if key.marker_brightness == 0
            || key.cell_width == 0
            || key.cell_height == 0
            || key.dot_width == 0
            || key.dot_height == 0
        {
            // Suppression is immediate.
            self.prepared = None;
            self.rendered_received_ms = None;
            return; // No invisible fleet-sized computation.
        } // End block.
        if self.worker.is_none() {
            // At most one owned preparer per scene.
            if wall < self.retry_at {
                return;
            } // Bound attempts after a spawn/admission failure.
            match MarkerWorker::try_start() {
                // The released implementation, with bounded admission.
                Ok(worker) => {
                    self.worker = Some(worker);
                    self.submitted = None;
                    self.error = None;
                } // Retry current desired input on this worker.
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.retry_at = wall.saturating_add(Duration::from_secs(1));
                    return;
                } // No blocking or thread storm.
            } // End block.
        } // End block.
        let Some(worker) = &self.worker else { return }; // Keep the ownership guard explicit.
        if self.submitted != Some(key.request_generation) {
            // Busy means retry; it never means accepted.
            let request = MarkerRequest {
                key,
                positions: Arc::clone(self.positions.as_ref().expect("checked positions")),
                _owner: self.owner.clone(),
            }; // Constant-time raw input sharing.
            match worker.try_submit_latest(&request) {
                // No request mutex wait on the UI.
                Ok(SubmitState::Accepted) => self.submitted = Some(key.request_generation), // Record only successful admission.
                Ok(SubmitState::Busy) => {
                    self.error = Some("marker admission busy; retrying latest".into());
                    return;
                } // Keep desired input and compatible last-good output.
                Ok(SubmitState::Stale) => {
                    self.error = Some("marker request superseded".into());
                    return;
                } // Never relabel an obsolete generation.
                Err(error) => {
                    self.error = Some(error.to_string());
                    return;
                } // Explicit poison/disconnect/viewport/preparation fault.
            } // End block.
        } // End block.
        match worker.try_latest(key) {
            // The worker compares full key AND desired generation.
            Ok(Some(prepared)) => {
                self.prepared = Some(prepared);
                self.rendered_received_ms = self.received_ms;
                self.error = None;
            } // Only a matching result advances rendered metadata.
            Ok(None) | Err(MarkerError::Busy) => {} // Pending/contended reads retain compatible last-good geometry.
            Err(error) => self.error = Some(error.to_string()), // Expose a failed worker; no automatic replacement loop.
        } // End block.
    } // End block.
    pub fn prepared(&self) -> Option<&PreparedMarkers> {
        self.prepared.as_deref()
    } // Viewport-bound borrowed output.
    pub fn ready(&self) -> bool {
        self.prepared
            .as_ref()
            .is_some_and(|p| Some(p.key) == self.desired)
    } // Also used by bounded integration fixtures.
    pub fn brief(&self) -> String {
        // Put preparation state before any truncatable metadata.
        let rendered = self.prepared.as_ref().map_or(0, |p| p.accepted_positions); // Actually displayed count.
        if self.error.is_some() {
            return format!("preparer error; drawn {rendered}");
        } // Preserve faults even when older geometry is visible.
        if self.ready() {
            return format!("drawn {rendered}");
        } // Only full-key acceptance is ready.
        if self.desired.is_some_and(|key| key.marker_brightness == 0) {
            return "markers hidden".into();
        } // Explicit zero-brightness state.
        format!(
            "preparing; previous {rendered}; received {}",
            self.positions.as_ref().map_or(0, |p| p.len())
        ) // Old/new counts are not a partially drawn new fleet.
    } // End block.
    pub fn status(&self) -> String {
        // Counts describe the displayed and desired generations separately.
        let received = self
            .positions
            .as_ref()
            .map_or(0, |positions| positions.len()); // No raw-vector traversal.
        let (generation, rendered, centers) = self.prepared.as_ref().map_or((0, 0, 0), |p| {
            (
                p.key.data_generation,
                p.accepted_positions,
                p.unique_center_cells,
            )
        }); // Actual admitted raster result.
        let phase = if self.ready() {
            "ready"
        } else if self.desired.is_some_and(|key| key.marker_brightness == 0) {
            "markers hidden"
        } else {
            "preparing / previous compatible data only"
        }; // Never claim new data is already drawn.
        format!("{phase}; rendered generation {generation}: {rendered} positions / {centers} occupied cells (receipt {:?}); received generation {}: {received} positions (receipt {:?}){}", self.rendered_received_ms, self.data_generation, self.received_ms, self.error.as_ref().map_or(String::new(), |error| format!("; {error}")))
        // Times are explicitly receipts, not fixes.
    } // End block.
} // End block.
fn compatible(old: MarkerKey, new: MarkerKey) -> bool {
    // Provider compatibility is enforced separately by clear_source.
    old.kind == new.kind
        && old.dot_width == new.dot_width
        && old.dot_height == new.dot_height
        && old.cell_width == new.cell_width
        && old.cell_height == new.cell_height
        && old.show_heading == new.show_heading
        && old.marker_brightness == new.marker_brightness // Data generations may differ while preparation is pending.
} // End block.
#[cfg(test)] // Pure generation tests plus the real worker tests in maps/map_markers.
mod tests {
    // No provider data or network calls.
    use super::*; // Inspect layer state without introducing a second production renderer.
    use crate::live_data::{map_markers::prepare_positions, maps::MapKind}; // Actual released pure preparation oracle.
    fn input() -> Arc<Vec<Position>> {
        Arc::new(vec![
            Position::new("fixture".into(), 0.0, 0.0, None).unwrap()
        ])
    } // Unknown fix time is valid.
    fn batch() -> Arc<FleetBatch> {
        Arc::new(FleetBatch {
            positions: input(),
            meta: Default::default(),
            _storage: None,
        }) // Synthetic fixtures do not retain provider allocations.
    } // Geometry fixture remains small and immutable.
    fn key() -> MarkerKey {
        MarkerKey {
            request_generation: 1,
            data_generation: 1,
            kind: MapKind::Boats,
            dot_width: 160,
            dot_height: 96,
            cell_width: 80,
            cell_height: 24,
            marker_brightness: 90,
            show_heading: true,
        }
    } // Normal viewport contract.
    #[test] // New receipt/data must not relabel old geometry as already drawn.
    fn same_source_pending_preserves_old_rendered_metadata_and_source_clear_drops_it() {
        // Deterministic handoff before worker polling.
        let positions = input();
        let request = MarkerRequest {
            key: key(),
            positions: Arc::clone(&positions),
            _owner: None,
        }; // First data generation.
        let prepared = Arc::new(prepare_positions(&request, &|| false).unwrap()); // Actual existing geometry implementation.
        let mut layer = FleetLayer {
            positions: Some(positions),
            generation: 1,
            data_generation: 1,
            desired: Some(key()),
            prepared: Some(prepared),
            received_ms: Some(1000),
            rendered_received_ms: Some(1000),
            ..Default::default()
        }; // Seed a previously accepted layer.
        layer.set_data(batch(), Some(2000)); // New Arc, same provider and viewport.
        assert!(!layer.ready());
        assert_eq!(layer.prepared().unwrap().key.data_generation, 1); // Prior compatible frame survives.
        assert!(layer.status().contains("received generation 2"));
        assert!(layer.status().contains("rendered generation 1")); // Distinct received/rendered identities.
        assert_eq!(layer.rendered_received_ms, Some(1000)); // Do not advance the displayed layer's receipt prematurely.
        let before = layer.generation;
        layer.clear_source(); // A -> B clears even when B has no data.
        assert!(layer.prepared().is_none());
        assert!(layer.positions.is_none());
        assert!(layer.generation > before); // No cross-source reuse.
        layer.set_data(batch(), Some(3000));
        assert!(layer.generation > before + 1); // B -> A cannot reuse A's old request generation.
    } // End block.
    #[test] // Geometry identity excludes color/poll settings and includes every raster dependency.
    fn incompatible_dimensions_brightness_and_kind_never_reuse_prepared_output() {
        // Compatibility is the production acceptance predicate.
        let original = key(); // Same source is enforced by clear_source before this predicate.
        assert!(compatible(
            original,
            MarkerKey {
                data_generation: 2,
                request_generation: 2,
                ..original
            }
        )); // Pending newer data may retain old geometry.
        for changed in [
            MarkerKey {
                cell_width: 81,
                ..original
            },
            MarkerKey {
                dot_height: 97,
                ..original
            },
            MarkerKey {
                marker_brightness: 0,
                show_heading: true,
                ..original
            },
            MarkerKey {
                kind: MapKind::Aircraft,
                ..original
            },
        ] {
            assert!(!compatible(original, changed));
        } // Each incompatible dependency blocks reuse.
        let mut layer = FleetLayer::default();
        layer.set_data(batch(), Some(1000)); // Data exists, but darkness needs no worker.
        layer.update(
            MarkerKey {
                marker_brightness: 0,
                show_heading: true,
                ..original
            },
            Duration::ZERO,
        ); // Immediate suppression.
        assert!(layer.prepared().is_none());
        assert!(layer.worker.is_none());
        assert!(layer.status().contains("markers hidden")); // No hidden large raster job.
    } // End block.
} // End block.
