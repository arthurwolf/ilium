//! UI-side immutable animation view. No scene, rasterizer or cache lives here.
use super::worker::{
    AdmissionError, AnimationService, EmissionReceipt, FrameSnapshot, PresentationLease,
    RenderRequest,
};
use super::{AnimationCacheStatus, AnimationSettings};
use ilium_animation_js::replay::{PendingEmission, ReplayFlushedProof};
use ilium_animation_js::runtime::CommittedFrameEmission;
use ratatui::layout::Rect;
use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_PENDING_CONFIGURATIONS: usize = 16;

enum Configuration {
    Render(Box<RenderRequest>),
    Pause(u64),
}

pub struct ComposedPresentation {
    lease: PresentationLease,
    surviving: Option<Vec<u8>>,
    pending_replay: Option<PendingEmission>,
    composed_at: Instant,
}
impl ComposedPresentation {
    pub fn snapshot(&self) -> &FrameSnapshot {
        self.lease.snapshot()
    }
    pub fn composed_at(&self) -> Instant {
        self.composed_at
    }
    /// The Presenter computes this from the very same previous/current buffer
    /// diff it submits to the backend. A surviving but unchanged glyph has no
    /// new terminal write and cannot credit a newly attributed source dot.
    pub(crate) fn restrict_to_emitted_cells(&mut self, emitted: &[u8]) -> Result<(), String> {
        let Some(surviving) = self.surviving.as_mut() else {
            return Ok(());
        };
        if surviving.len() != emitted.len() || emitted.iter().any(|flag| *flag > 1) {
            return Err("Terminal diff provenance does not match composed frame".into());
        }
        for (bits, &flag) in surviving.iter_mut().zip(emitted) {
            if flag == 0 {
                *bits = 0;
            }
        }
        Ok(())
    }
    pub(crate) fn begin_output(&mut self) -> Result<Option<CommittedFrameEmission>, String> {
        if self.lease.has_replay() {
            let surviving = self
                .surviving
                .as_deref()
                .ok_or("Final compositor provenance unavailable for replay output")?;
            self.pending_replay = self.lease.prepare_replay(surviving)?;
        }
        self.lease.begin_output()
    }
}

enum UnsettledReplay {
    Uncertain { _pending: PendingEmission },
    Validation { _pending: PendingEmission },
    History { _emitted: PendingEmission },
}

struct UnsettledOutput {
    _presentation: ComposedPresentation,
    _proof: Option<ReplayFlushedProof>,
    _world_emissions: Vec<ilium_animation_js::native_worlds::WorldEmission>,
}

pub struct AnimationSurface {
    resources: Option<ilium_ambient::resources::AmbientResources>,
    service: Option<AnimationService>,
    ready: Arc<tokio::sync::Notify>,
    revision: u64,
    desired: Option<(AnimationSettings, u16, u16)>,
    configurations: VecDeque<Configuration>,
    receipts: VecDeque<EmissionReceipt>,
    blocked_replay: Option<UnsettledReplay>,
    blocked_output: Option<UnsettledOutput>,
    display: Option<Arc<FrameSnapshot>>,
    last_request: Option<(u64, Duration, Option<[f32; 2]>, u64)>,
    occupancy: Option<Arc<ilium_ambient::OccupancyMask>>,
    occupancy_revision: u64,
    composition: Option<PresentationLease>,
    composed_bits: Vec<u8>,
    error: Option<String>,
    visibility: crate::animation_visibility::VisibilityCheck,
    #[cfg(test)]
    initial_frame: Option<super::AnimationFrame>,
    #[cfg(test)]
    pub(crate) geometry_render_count: usize,
}
impl Default for AnimationSurface {
    fn default() -> Self {
        Self {
            resources: {
                #[cfg(test)]
                {
                    Some(ilium_ambient::resources::AmbientResources::new(
                        crate::execution::test_client(),
                    ))
                }
                #[cfg(not(test))]
                {
                    None
                }
            },
            service: None,
            ready: Arc::new(tokio::sync::Notify::new()),
            revision: 0,
            desired: None,
            configurations: VecDeque::new(),
            receipts: VecDeque::new(),
            blocked_replay: None,
            blocked_output: None,
            display: None,
            last_request: None,
            occupancy: None,
            occupancy_revision: 0,
            composition: None,
            composed_bits: Vec::new(),
            error: None,
            visibility: Default::default(),
            #[cfg(test)]
            initial_frame: None,
            #[cfg(test)]
            geometry_render_count: 0,
        }
    }
}
impl AnimationSurface {
    /// Identity belongs to the loaded immutable frame and the current desired
    /// selection. A catalogue descriptor or superseded frame cannot supply it.
    pub fn plugin_package_digest(&self, package_id: &str) -> Option<&str> {
        let (settings, _, _) = self.desired.as_ref()?;
        if settings.source != crate::animation_plugins::AnimationSourceTab::Plugin
            || settings.plugin.selected.as_ref()?.package_id != package_id
        {
            return None;
        }
        let frame = self.display.as_ref()?;
        if frame.revision != self.revision {
            return None;
        }
        let identity = frame.plugin_identity()?;
        if identity.package_id != package_id {
            return None;
        }
        frame.plugin_package_digest()
    }

    pub fn configure_resources(&mut self, resources: ilium_ambient::resources::AmbientResources) {
        self.resources = Some(resources);
    }

    pub fn admission_notification(&self) -> Arc<tokio::sync::Notify> {
        super::worker::admission_notification()
    }
    pub fn notification(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.ready)
    }

    fn start(&mut self) -> bool {
        if self.service.is_some() {
            return true;
        }
        let Some(resources) = self.resources.clone() else {
            self.error = Some(format!(
                "Animation worker unavailable: {}",
                ilium_ambient::resources::MissingResources
            ));
            return false;
        };
        let admission = match AnimationService::reserve() {
            Ok(admission) => admission,
            Err(error) => {
                self.error = Some(format!("Animation worker unavailable: {error}"));
                return false;
            }
        };
        #[cfg(test)]
        let frame = self.initial_frame.take();
        #[cfg(not(test))]
        let frame = None;
        match AnimationService::start_admitted(admission, frame, Arc::clone(&self.ready), resources)
        {
            Ok(service) => {
                self.service = Some(service);
                if self
                    .error
                    .as_deref()
                    .is_some_and(|error| error.starts_with("Animation worker unavailable:"))
                {
                    self.error = None;
                }
                true
            }
            Err(error) => {
                self.error = Some(format!("Animation worker unavailable: {error}"));
                false
            }
        }
    }

    /// Stores the screen occupancy the next requests carry. A mask that equals
    /// the previous one keeps its revision, so a still screen costs nothing.
    pub fn set_occupancy(&mut self, mask: Option<ilium_ambient::OccupancyMask>) {
        let unchanged = match (&self.occupancy, &mask) {
            (Some(previous), Some(next)) => **previous == *next,
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        self.occupancy = mask.map(Arc::new);
        self.occupancy_revision = self.occupancy_revision.wrapping_add(1);
    }

    pub fn has_requested(&self) -> bool {
        self.desired.is_some()
    }
    pub fn can_queue_configuration(&self) -> bool {
        // `release_hosts` is an ordered lifecycle transition that must remain
        // enqueueable after the latest accepted render configuration.
        self.configurations.len() < MAX_PENDING_CONFIGURATIONS - 1
    }

    /// Configuration changes are ordered; only time updates are replaceable.
    /// Rejection leaves the authored setting with the caller, who must not
    /// report an accepted semantic action without retaining its execution.
    pub fn request(
        &mut self,
        settings: &AnimationSettings,
        width: u16,
        height: u16,
        elapsed: Duration,
        pointer: Option<[f32; 2]>,
    ) -> Result<(), AdmissionError> {
        self.collect();
        if !super::worker::settings_fit(settings) || !super::worker::dimensions_fit(width, height) {
            self.error = Some("Animation request exceeds retained settings or frame limit".into());
            return Err(AdmissionError::Invalid);
        }
        let changed = self
            .desired
            .as_ref()
            .is_none_or(|(previous, columns, rows)| {
                previous != settings || *columns != width || *rows != height
            });
        if changed {
            self.visibility = Default::default();
            if !self.can_queue_configuration() {
                self.error = Some("Animation configuration queue full; retry this change".into());
                return Err(AdmissionError::Full);
            }
            self.revision = self
                .revision
                .checked_add(1)
                .ok_or(AdmissionError::Invalid)?;
            self.desired = Some((settings.clone(), width, height));
            self.configurations
                .push_back(Configuration::Render(Box::new(RenderRequest {
                    revision: self.revision,
                    settings: settings.clone(),
                    width,
                    height,
                    elapsed,
                    requested_at: Instant::now(),
                    pointer,
                    occupancy: self.occupancy.clone(),
                    occupancy_revision: self.occupancy_revision,
                })));
            self.last_request = None;
        }
        if !self.start() {
            return Err(AdmissionError::Full);
        }
        self.flush();
        if self.configurations.is_empty()
            && self.last_request != Some((self.revision, elapsed, pointer, self.occupancy_revision))
        {
            let request = RenderRequest {
                revision: self.revision,
                settings: settings.clone(),
                width,
                height,
                elapsed,
                requested_at: Instant::now(),
                pointer,
                occupancy: self.occupancy.clone(),
                occupancy_revision: self.occupancy_revision,
            };
            match self
                .service
                .as_ref()
                .ok_or(AdmissionError::Stopped)?
                .try_request(request)
            {
                Ok(()) => {
                    self.last_request =
                        Some((self.revision, elapsed, pointer, self.occupancy_revision))
                }
                Err(rejected)
                    if matches!(
                        rejected.reason,
                        AdmissionError::Busy
                            | AdmissionError::Full
                            | AdmissionError::RevisionNeedsBarrier
                    ) =>
                {
                    self.ready.notify_one();
                }
                Err(rejected) => {
                    self.error = Some(format!("Animation request rejected: {:?}", rejected.reason));
                    return Err(rejected.reason);
                }
            }
        }
        Ok(())
    }

    fn flush(&mut self) {
        let Some(service) = &self.service else {
            return;
        };
        while let Some(receipt) = self.receipts.pop_front() {
            match service.try_receipt(receipt) {
                Ok(()) => {}
                Err(rejected) => {
                    let busy = rejected.reason == AdmissionError::Busy;
                    self.receipts.push_front(rejected.value);
                    if busy {
                        self.ready.notify_one();
                    }
                    break;
                }
            }
        }
        while let Some(configuration) = self.configurations.pop_front() {
            match configuration {
                Configuration::Render(request) => {
                    let identity = (
                        request.revision,
                        request.elapsed,
                        request.pointer,
                        request.occupancy_revision,
                    );
                    match service.try_request(*request) {
                        Ok(()) => self.last_request = Some(identity),
                        Err(rejected) => {
                            self.error = (!matches!(
                                rejected.reason,
                                AdmissionError::Busy
                                    | AdmissionError::Full
                                    | AdmissionError::RevisionNeedsBarrier
                            ))
                            .then(|| {
                                format!("Animation configuration rejected: {:?}", rejected.reason)
                            });
                            let busy = rejected.reason == AdmissionError::Busy;
                            self.configurations
                                .push_front(Configuration::Render(Box::new(rejected.value)));
                            if busy {
                                self.ready.notify_one();
                            }
                            break;
                        }
                    }
                }
                Configuration::Pause(revision) => match service.try_pause(revision) {
                    Ok(()) => {}
                    Err(reason) => {
                        self.configurations
                            .push_front(Configuration::Pause(revision));
                        if reason == AdmissionError::Busy {
                            self.ready.notify_one();
                        }
                        break;
                    }
                },
            }
        }
    }

    /// Bounded completion wakeups collect only the latest complete snapshot.
    pub fn collect(&mut self) -> bool {
        if self.service.is_none() && !self.configurations.is_empty() {
            self.start();
        }
        self.flush();
        let Some(service) = &self.service else {
            return false;
        };
        if let Some(status) = service.try_status() {
            if status.error.is_some() {
                self.error = status.error;
            } else if !status.is_accepting {
                self.error =
                    Some("Animation worker stopped; pending execution was not accepted".into());
            }
        }
        let Some(frame) = service.try_snapshot() else {
            return false;
        };
        if self.desired.is_none() || frame.revision != self.revision {
            return false;
        }
        #[cfg(test)]
        {
            self.geometry_render_count = frame.geometry_render_count;
        }
        self.display = Some(frame);
        true
    }

    pub fn begin_composition(&mut self) -> bool {
        // A discarded, never-emitted composition is safely retired without credit.
        self.composition = None;
        self.composed_bits.clear();
        if self.blocked_replay.is_some()
            || self.blocked_output.is_some()
            || !self.configurations.is_empty()
            || !self.receipts.is_empty()
        {
            return false;
        }
        let Some(frame) = &self.display else {
            return false;
        };
        if self.desired.is_none() || frame.revision != self.revision {
            return false;
        }
        match frame.begin_presentation() {
            Ok(lease) => {
                self.composition = Some(lease);
                true
            }
            Err(AdmissionError::Busy) => {
                self.ready.notify_one();
                false
            }
            Err(_) => false,
        }
    }

    pub fn capture(&mut self, surviving: Option<Vec<u8>>) -> Option<ComposedPresentation> {
        self.composed_bits.clear();
        self.composition.take().map(|lease| ComposedPresentation {
            lease,
            surviving,
            pending_replay: None,
            composed_at: Instant::now(),
        })
    }
    pub fn acknowledge(
        &mut self,
        mut presentation: ComposedPresentation,
        mut proof: Option<ReplayFlushedProof>,
    ) {
        let mut world_emissions = Vec::new();
        if let Some(mut pending) = presentation.pending_replay.take() {
            let Some(proof) = proof.take() else {
                self.blocked_replay = Some(UnsettledReplay::Uncertain { _pending: pending });
                self.reject_output("replay flush proof missing after output");
                return;
            };
            if let Err(error) = pending.after_host_emission(proof) {
                self.blocked_replay = Some(UnsettledReplay::Validation { _pending: pending });
                self.reject_output(&format!("original replay flush validation failed: {error}"));
                return;
            }
            if let Err(error) = pending.settle() {
                self.blocked_replay = Some(UnsettledReplay::History { _emitted: pending });
                self.reject_output(&format!("original replay history uncertain: {error}"));
                return;
            }
        } else {
            if presentation.lease.snapshot().plugin_identity().is_some() != proof.is_some() {
                self.retain_unsettled_output(
                    presentation,
                    proof,
                    Vec::new(),
                    "terminal proof did not match its original frame",
                );
                return;
            }
            if let Some(error) = proof
                .as_ref()
                .and_then(|proof| presentation.lease.validate_output_proof(proof).err())
            {
                self.retain_unsettled_output(
                    presentation,
                    proof,
                    Vec::new(),
                    &format!("terminal proof validation failed: {error}"),
                );
                return;
            }
            let emission_result = match (presentation.surviving.as_deref(), proof.as_ref()) {
                (Some(bits), Some(proof)) => Some(presentation.lease.world_emissions(bits, proof)),
                _ => None,
            };
            match emission_result {
                Some(Ok(emissions)) => world_emissions = emissions,
                Some(Err(error)) => {
                    self.retain_unsettled_output(
                        presentation,
                        proof,
                        Vec::new(),
                        &format!("world emission validation failed: {error}"),
                    );
                    return;
                }
                None => {}
            }
            // Live plugin frames have no replay source projection. Their broker
            // proof remains owned until receipt construction succeeds.
        }
        if let Some(bits) = presentation.surviving {
            let composed_at = presentation.composed_at;
            match presentation
                .lease
                .receipt_with_world_emissions(bits, world_emissions)
            {
                Ok(receipt) => {
                    drop(proof);
                    self.receipts.push_back(receipt);
                }
                Err(rejected) => {
                    let (lease, surviving, world_emissions) = rejected.value;
                    self.retain_unsettled_output(
                        ComposedPresentation {
                            lease,
                            surviving: Some(surviving),
                            pending_replay: None,
                            composed_at,
                        },
                        proof,
                        world_emissions,
                        "invalid animation emission receipt after terminal output",
                    );
                }
            }
        } else {
            drop(proof);
        }
        self.flush();
    }
    pub fn uncertain_output(&mut self, mut presentation: ComposedPresentation, reason: &str) {
        if let Some(pending) = presentation.pending_replay.take() {
            self.blocked_replay = Some(UnsettledReplay::Uncertain { _pending: pending });
        } else {
            self.blocked_output = Some(UnsettledOutput {
                _presentation: presentation,
                _proof: None,
                _world_emissions: Vec::new(),
            });
        }
        self.reject_output(reason);
    }

    pub(crate) fn reject_output(&mut self, reason: &str) {
        self.composition = None;
        self.display = None;
        self.composed_bits.clear();
        self.error = Some(format!("Animation output could not be confirmed: {reason}"));
        self.ready.notify_one();
    }

    fn retain_unsettled_output(
        &mut self,
        presentation: ComposedPresentation,
        proof: Option<ReplayFlushedProof>,
        world_emissions: Vec<ilium_animation_js::native_worlds::WorldEmission>,
        reason: &str,
    ) {
        self.blocked_output = Some(UnsettledOutput {
            _presentation: presentation,
            _proof: proof,
            _world_emissions: world_emissions,
        });
        self.reject_output(reason);
    }

    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.composition = None;
        if self.blocked_output.is_some() {
            return Err(io::Error::other(
                "Original animation output remains unsettled",
            ));
        }
        if self.service.is_none() && !self.configurations.is_empty() && !self.start() {
            return Err(io::Error::other(
                "Animation admission unavailable while accepted configurations remain",
            ));
        }
        loop {
            self.flush();
            let Some(service) = &self.service else {
                return if self.blocked_replay.is_some() || self.blocked_output.is_some() {
                    Err(io::Error::other(
                        "Original animation output remains unsettled",
                    ))
                } else {
                    Ok(())
                };
            };
            if !service.is_accepting()
                && (!self.receipts.is_empty()
                    || !self.configurations.is_empty()
                    || service.configuration_count() != 0
                    || service.presentation_count() != 0)
            {
                return Err(io::Error::other(
                    "Animation worker stopped before emitted receipts drained",
                ));
            }
            if self.receipts.is_empty()
                && self.configurations.is_empty()
                && service.configuration_count() == 0
                && service.presentation_count() == 0
            {
                break;
            }
            self.ready.notified().await;
        }
        self.service = None;
        if self.blocked_replay.is_some() || self.blocked_output.is_some() {
            return Err(io::Error::other(
                "Original animation output remains unsettled",
            ));
        }
        Ok(())
    }

    pub fn release_hosts(&mut self) {
        self.composition = None;
        self.composed_bits.clear();
        self.display = None;
        if self.desired.take().is_none() {
            return;
        }
        if let Some(next) = self.revision.checked_add(1) {
            self.revision = next;
        }
        self.configurations
            .push_back(Configuration::Pause(self.revision));
        self.last_request = None;
        self.flush();
    }
    pub(crate) fn check_visibility(
        &mut self,
        buffer: &ratatui::buffer::Buffer,
        foreground: (u8, u8, u8),
        light: bool,
        black_backdrop: bool,
    ) {
        let mut check = std::mem::take(&mut self.visibility);
        check.update(self, buffer, foreground, light, black_backdrop);
        self.visibility = check;
    }

    pub(crate) fn visibility_warning(&self) -> bool {
        self.visibility.warning()
    }

    pub fn width(&self) -> u16 {
        self.display.as_ref().map_or(0, |frame| frame.width)
    }
    pub fn height(&self) -> u16 {
        self.display.as_ref().map_or(0, |frame| frame.height)
    }
    pub fn glyph(&self, x: u16, y: u16) -> char {
        self.display
            .as_ref()
            .and_then(|frame| frame.cell(x, y))
            .map_or(' ', |cell| cell.glyph)
    }
    pub fn native_glyph(&self, x: u16, y: u16) -> Option<char> {
        self.display.as_ref()?.cell(x, y)?.native_glyph
    }
    pub fn article_symbol(&self, x: u16, y: u16) -> Option<&str> {
        self.display.as_ref()?.cell(x, y)?.article_symbol.as_deref()
    }
    pub fn article_is_continuation(&self, x: u16, y: u16) -> bool {
        self.display
            .as_ref()
            .and_then(|frame| frame.cell(x, y))
            .is_some_and(|cell| cell.article_is_continuation)
    }
    pub fn article_style(&self, x: u16, y: u16) -> (bool, bool) {
        self.display
            .as_ref()
            .and_then(|frame| frame.cell(x, y))
            .map_or((false, false), |cell| cell.article_style)
    }
    pub fn article_background(&self, x: u16, y: u16) -> Option<(u8, u8, u8)> {
        self.display.as_ref()?.cell(x, y)?.article_background
    }
    pub fn article_underline(&self, x: u16, y: u16) -> bool {
        self.display
            .as_ref()
            .and_then(|frame| frame.cell(x, y))
            .is_some_and(|cell| cell.article_underline)
    }
    pub fn cell_color(&self, x: u16, y: u16) -> Option<(u8, u8, u8)> {
        self.display.as_ref()?.cell(x, y)?.color
    }
    pub fn has_cell_colors(&self) -> bool {
        self.display
            .as_ref()
            .is_some_and(|frame| frame.has_cell_colors)
    }
    pub fn is_wikipedia(&self) -> bool {
        self.display
            .as_ref()
            .is_some_and(|frame| frame.is_wikipedia)
    }
    pub fn frames_per_second(&self) -> Option<u32> {
        self.display.as_ref()?.frames_per_second
    }
    pub(crate) fn permission_bridge(
        &self,
    ) -> Option<Arc<crate::animation_plugins::review_bridge::ReviewBridge>> {
        self.service.as_ref()?.permission_bridge()
    }

    pub fn status(&self) -> Option<String> {
        self.error
            .clone()
            .or_else(|| self.display.as_ref().and_then(|frame| frame.status.clone()))
    }
    pub fn cache_status(&self) -> AnimationCacheStatus {
        self.display
            .as_ref()
            .map_or(Default::default(), |frame| frame.cache)
    }
    pub fn packed_bit(&self, x: u16, y: u16) -> u8 {
        self.display
            .as_ref()
            .and_then(|frame| frame.cell(x, y))
            .map_or(0, |cell| cell.packed_bits)
    }
    pub fn composed(&mut self, bits: Vec<u8>) {
        if self.composition.is_some() {
            self.composed_bits = bits;
        }
    }
    pub fn composed_bits(&self) -> &[u8] {
        &self.composed_bits
    }
    pub fn discard_composed_receipt(&mut self) {
        self.composed_bits.clear();
    }
    pub fn occlude_composed(&mut self, screen: Rect, region: Rect) {
        if self.composed_bits.len() != usize::from(screen.width) * usize::from(screen.height) {
            return;
        }
        let clipped = screen.intersection(region);
        for row in clipped.top()..clipped.bottom() {
            let start = usize::from(row - screen.y) * usize::from(screen.width)
                + usize::from(clipped.x - screen.x);
            self.composed_bits[start..start + usize::from(clipped.width)].fill(0);
        }
    }
}

#[cfg(test)]
impl AnimationSurface {
    /// Injection constructs only the test adapter; rendering still runs on the
    /// real animation service thread. Call before its first request.
    pub(crate) fn host_mut(&mut self) -> &mut super::AmbientHost {
        assert!(
            self.service.is_none(),
            "Inject a host before starting the animation service"
        );
        self.initial_frame
            .get_or_insert_with(Default::default)
            .host_mut()
    }
    pub(crate) fn inject_wikipedia_document_for_test(
        &mut self,
        document: Arc<ilium_wikipedia::Document>,
        settings: &super::WikipediaSettings,
        columns: u16,
    ) {
        self.initial_frame
            .get_or_insert_with(Default::default)
            .inject_wikipedia_document_for_test(document, settings, columns);
    }
    pub(crate) fn render(
        &mut self,
        settings: &AnimationSettings,
        width: u16,
        height: u16,
        elapsed: Duration,
    ) {
        self.composition = None;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match self.request(settings, width, height, elapsed, None) {
                Ok(()) => break,
                Err(AdmissionError::Full | AdmissionError::Busy) => {
                    assert!(
                        Instant::now() < deadline,
                        "Animation test admission did not settle"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("Animation test request rejected: {error:?}"),
            }
        }
        self.wait_for_frame_for_test(elapsed);
    }
    pub(crate) fn wait_for_frame_for_test(&mut self, elapsed: Duration) {
        self.composition = None;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.collect();
            if self
                .display
                .as_ref()
                .is_some_and(|frame| frame.revision == self.revision && frame.elapsed == elapsed)
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Animation worker did not produce requested frame: {:?}",
                self.status()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    pub(crate) fn settle_for_test(&mut self) {
        self.composition = None;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if self.service.is_none() && !self.configurations.is_empty() {
                self.start();
            }
            self.collect();
            let complete = self.configurations.is_empty()
                && self.receipts.is_empty()
                && self.service.as_ref().is_none_or(|service| {
                    service.configuration_count() == 0 && service.presentation_count() == 0
                })
                && (self.desired.is_none()
                    || self.display.as_ref().is_some_and(|frame| {
                        frame.revision == self.revision
                            && self
                                .last_request
                                .is_none_or(|(_, elapsed, _, _)| frame.elapsed == elapsed)
                    }));
            if complete {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Animation transition did not settle: {:?}",
                self.status()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    pub(crate) fn packed_cells(&self) -> Vec<u8> {
        (0..self.height())
            .flat_map(|y| (0..self.width()).map(move |x| self.packed_bit(x, y)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_ambient::raster::PaintedOwner;
    use ilium_ambient::{Frame, Scene};
    use std::sync::{mpsc, Mutex};

    struct GatedScene {
        entered: mpsc::SyncSender<std::thread::ThreadId>,
        release: Option<mpsc::Receiver<()>>,
        painted: mpsc::SyncSender<Vec<PaintedOwner>>,
    }
    impl Scene for GatedScene {
        fn render(&mut self, frame: &mut Frame<'_>) {
            if let Some(release) = self.release.take() {
                self.entered.send(std::thread::current().id()).unwrap();
                release.recv().unwrap();
            }
            frame.raster.dots.fill(1.0);
            frame.raster.owner_ids.fill(7);
        }
        fn frames_per_second(&self) -> u32 {
            1
        }
        fn presented(&mut self, owners: &[PaintedOwner]) {
            self.painted.send(owners.to_vec()).unwrap();
        }
    }
    type Harness = (
        AnimationSurface,
        mpsc::Receiver<std::thread::ThreadId>,
        mpsc::SyncSender<()>,
        mpsc::Receiver<Vec<PaintedOwner>>,
    );
    fn fixture() -> Harness {
        let (entered, observed) = mpsc::sync_channel(1);
        let (release, advance) = mpsc::sync_channel(1);
        let (painted, receipts) = mpsc::sync_channel(4);
        let scene = Mutex::new(Some(GatedScene {
            entered,
            release: Some(advance),
            painted,
        }));
        let frame = super::super::AnimationFrame {
            host: super::super::AmbientHost::with_factory(Box::new(move |_, _, _| {
                Box::new(scene.lock().unwrap().take().unwrap())
            })),
            ..Default::default()
        };
        let mut surface = AnimationSurface {
            initial_frame: Some(frame),
            ..Default::default()
        };
        let settings = AnimationSettings {
            kind: super::super::AnimationKind::Stars,
            ..Default::default()
        };
        // Full admission retains the exact initial configuration for retry.
        let result = surface.request(&settings, 2, 1, Duration::ZERO, None);
        assert!(result.is_ok() || result == Err(AdmissionError::Full));
        (surface, observed, release, receipts)
    }

    #[test]
    fn render_admission_reserves_capacity_for_the_ordered_pause_transition() {
        let mut surface = AnimationSurface {
            desired: Some((AnimationSettings::default(), 1, 1)),
            ..Default::default()
        };
        surface
            .configurations
            .extend((0..MAX_PENDING_CONFIGURATIONS - 1).map(Configuration::Pause));

        assert!(!surface.can_queue_configuration());
        surface.release_hosts();

        assert_eq!(surface.configurations.len(), MAX_PENDING_CONFIGURATIONS);
        assert!(surface.desired.is_none());
        assert!(matches!(
            surface.configurations.back(),
            Some(Configuration::Pause(1))
        ));
    }

    fn entered(
        surface: &mut AnimationSurface,
        observed: &mpsc::Receiver<std::thread::ThreadId>,
    ) -> std::thread::ThreadId {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            surface.collect();
            if let Ok(thread) = observed.try_recv() {
                return thread;
            }
            assert!(
                Instant::now() < deadline,
                "Animation admission did not become available"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[tokio::test]
    async fn blocked_render_returns_to_ui_and_completion_wakes_one_fps_surface() {
        let (mut surface, observed, release, receipts) = fixture();
        assert_ne!(
            entered(&mut surface, &observed),
            std::thread::current().id()
        );
        assert!(surface.display.is_none());
        assert!(!surface.begin_composition());
        let notification = surface.notification();
        // Drain a recoverable admission wake, then register while rendering
        // is still blocked. Only completed frame publication can release it.
        let _ = tokio::time::timeout(Duration::from_millis(1), notification.notified()).await;
        let completion = notification.notified();
        tokio::pin!(completion);
        completion.as_mut().enable();
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), completion)
            .await
            .unwrap();
        surface.wait_for_frame_for_test(Duration::ZERO);
        assert_eq!(surface.frames_per_second(), Some(1));
        assert!(surface.begin_composition());
        let surviving = surface.packed_bit(0, 0);
        assert_ne!(surviving, 0);
        let discarded = surface.capture(Some(vec![surviving, 0])).unwrap();
        drop(discarded);
        assert!(
            receipts.try_recv().is_err(),
            "Discarding a prepared frame must not credit presentation"
        );
        surface.shutdown().await.unwrap();
    }

    struct GatedOutput {
        first: bool,
        entered: mpsc::SyncSender<std::thread::ThreadId>,
        release: mpsc::Receiver<()>,
    }
    impl std::io::Write for GatedOutput {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.first {
                self.first = false;
                self.entered.send(std::thread::current().id()).unwrap();
                self.release.recv().unwrap();
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn scene_credit_waits_for_real_terminal_owner_flush_acknowledgement() {
        let (mut surface, observed, release, receipts) = fixture();
        entered(&mut surface, &observed);
        release.send(()).unwrap();
        surface.wait_for_frame_for_test(Duration::ZERO);
        assert!(surface.begin_composition());
        let surviving = surface.packed_bit(0, 0);
        assert_ne!(surviving, 0);
        let glyph = surface.glyph(0, 0);
        let presentation = surface.capture(Some(vec![surviving, 0])).unwrap();
        let (output_entered, output_observed) = mpsc::sync_channel(1);
        let (output_release, output_advance) = mpsc::sync_channel(1);
        let backend = ratatui::backend::CrosstermBackend::new(
            crate::presentation::TerminalOutput::new(GatedOutput {
                first: true,
                entered: output_entered,
                release: output_advance,
            }),
        );
        let mut presenter =
            crate::presentation::Presenter::start(backend, &crate::presentation::test_quota())
                .unwrap();
        let mut buffer = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 2, 1));
        buffer[(0, 0)].set_symbol(&glyph.to_string());
        let mut frame = crate::presentation::PreparedFrame::new(
            presenter.try_reserve().unwrap(),
            buffer,
            None,
            42,
            1,
        )
        .unwrap();
        frame.attach_animation(Some(presentation));
        presenter.submit(frame).unwrap();
        assert_ne!(
            output_observed
                .recv_timeout(Duration::from_secs(3))
                .unwrap(),
            std::thread::current().id()
        );
        assert!(presenter.acknowledgements.try_recv().is_err());
        assert!(receipts.try_recv().is_err());
        output_release.send(()).unwrap();
        let emitted =
            tokio::time::timeout(Duration::from_secs(3), presenter.acknowledgements.recv())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        assert_eq!(emitted.frame.frame_id, 42);
        let mut completed_frame = emitted.frame;
        let animation = completed_frame.take_animation().unwrap();
        surface.acknowledge(animation, emitted.flush_proof);
        surface.settle_for_test();
        assert_eq!(
            receipts.recv_timeout(Duration::from_secs(3)).unwrap(),
            vec![PaintedOwner {
                id: 7,
                dots: surviving.count_ones()
            }]
        );
        // The exact same scene glyph remains in the terminal diff base. This
        // second successful flush must not credit it again as a fresh view.
        assert!(surface.begin_composition());
        let second = surface.capture(Some(vec![surviving, 0])).unwrap();
        let mut same = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 2, 1));
        same[(0, 0)].set_symbol(&glyph.to_string());
        let mut frame = crate::presentation::PreparedFrame::new(
            presenter.try_reserve().unwrap(),
            same,
            None,
            43,
            1,
        )
        .unwrap();
        frame.attach_animation(Some(second));
        presenter.submit(frame).unwrap();
        let emitted =
            tokio::time::timeout(Duration::from_secs(3), presenter.acknowledgements.recv())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        assert!(emitted.rejection.is_none() && emitted.uncertainty.is_none());
        let mut completed_frame = emitted.frame;
        surface.acknowledge(
            completed_frame.take_animation().unwrap(),
            emitted.flush_proof,
        );
        surface.settle_for_test();
        if let Ok(owners) = receipts.try_recv() {
            assert!(
                owners.is_empty(),
                "identical retained glyph has no new terminal dots"
            );
        }
        presenter.shutdown().await.unwrap();
        surface.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn uncertain_live_output_retains_lease_and_blocks_next_composition() {
        let (mut surface, observed, release, _receipts) = fixture();
        entered(&mut surface, &observed);
        release.send(()).unwrap();
        surface.wait_for_frame_for_test(Duration::ZERO);
        assert!(surface.begin_composition());
        let surviving = surface.packed_bit(0, 0);
        let presentation = surface.capture(Some(vec![surviving, 0])).unwrap();

        surface.uncertain_output(presentation, "test-only uncertain flush");

        assert!(surface.blocked_output.is_some());
        assert!(!surface.begin_composition());
        assert!(surface
            .error
            .as_deref()
            .unwrap()
            .contains("could not be confirmed"));

        // The real owner remains held through the assertions above. Release it
        // here so the fixture can shut its worker down cleanly.
        surface.blocked_output.take();
        surface.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn settings_transitions_remain_ordered_while_first_render_is_blocked() {
        struct OrderedScene {
            entered: mpsc::SyncSender<std::thread::ThreadId>,
            release: Option<mpsc::Receiver<()>>,
            applied: Arc<Mutex<Vec<String>>>,
        }
        impl Scene for OrderedScene {
            fn render(&mut self, frame: &mut Frame<'_>) {
                if let Some(release) = self.release.take() {
                    self.entered.send(std::thread::current().id()).unwrap();
                    release.recv().unwrap();
                }
                frame.raster.dots.fill(1.0);
            }
            fn reconfigure(&mut self, settings: &ilium_ambient::AmbientSettings) -> bool {
                self.applied
                    .lock()
                    .unwrap()
                    .push(settings.stars.start_datetime.clone());
                true
            }
        }
        let applied = Arc::new(Mutex::new(Vec::new()));
        let (observed_entered, observed) = mpsc::sync_channel(1);
        let (release, advance) = mpsc::sync_channel(1);
        let scene = Mutex::new(Some(OrderedScene {
            entered: observed_entered,
            release: Some(advance),
            applied: Arc::clone(&applied),
        }));
        let frame = super::super::AnimationFrame {
            host: super::super::AmbientHost::with_factory(Box::new(move |_, settings, _| {
                let scene = scene.lock().unwrap().take().unwrap();
                scene
                    .applied
                    .lock()
                    .unwrap()
                    .push(settings.stars.start_datetime.clone());
                Box::new(scene)
            })),
            ..Default::default()
        };
        let mut surface = AnimationSurface {
            initial_frame: Some(frame),
            ..Default::default()
        };
        let mut settings = AnimationSettings {
            kind: super::super::AnimationKind::Stars,
            ..Default::default()
        };
        let result = surface.request(&settings, 2, 1, Duration::ZERO, None);
        assert!(result.is_ok() || result == Err(AdmissionError::Full));
        entered(&mut surface, &observed);
        for date in [
            "2026-10-01 00:00:00",
            "2026-10-02 00:00:00",
            "2026-10-03 00:00:00",
        ] {
            settings.ambient.stars.start_datetime = date.into();
            surface
                .request(&settings, 2, 1, Duration::ZERO, None)
                .unwrap();
        }
        release.send(()).unwrap();
        surface.settle_for_test();
        assert_eq!(
            *applied.lock().unwrap(),
            [
                "",
                "2026-10-01 00:00:00",
                "2026-10-02 00:00:00",
                "2026-10-03 00:00:00"
            ]
        );
        surface.shutdown().await.unwrap();
    }
}

#[cfg(test)]
mod resource_injection_tests {
    use super::*;
    #[test]
    fn unconfigured_surface_refuses_before_spawning_and_preserves_error() {
        let mut surface = AnimationSurface {
            resources: None,
            ..Default::default()
        };
        assert!(!surface.start());
        assert!(surface.service.is_none());
        assert!(surface
            .error
            .as_deref()
            .unwrap()
            .contains("resources are not configured"));
    }
}
