//! Bounded, generation-bound polling activity windows.
use std::{sync::Arc, time::Duration};

use ilium_core::polling_detection::{ActivityEvent, AgentBinding, PollingSettings};

use crate::polling_activity::ActivityWindow;

const MAX_WINDOWS: usize = 1024;
const MAX_IDENTITY_BYTES: usize = 256;

struct BoundWindow {
    binding: AgentBinding,
    activity: ActivityWindow,
    source_position: Option<u64>,
    identity: Arc<()>,
    admission_revision: Option<u64>,
}

/// Applying transport observations never establishes their provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchApplication {
    Baseline,
    Applied { ready: bool },
    Duplicate,
    Gap,
    Rejected,
}

/// A local interval marker, never a provenance or spending authorization.
pub struct WindowCheckMark {
    // Holding the allocation prevents identity reuse after unbind/reset.
    identity: Arc<()>,
    revision: u64,
    eligible_events: usize,
    captured_at: Duration,
}

#[derive(Default)]
pub struct PollingWindows {
    configuration: Option<(usize, usize, Duration)>,
    windows: Vec<BoundWindow>,
    dropped_capacity: u64,
}

impl PollingWindows {
    /// Apply only a batch whose serving incarnation and private peer are verified.
    pub fn apply_batch(
        &mut self,
        batch: ilium_core::polling_detection::PollingActivityBatch,
        now: Duration,
    ) -> BatchApplication {
        let Some((_, _, spacing)) = self.configuration else {
            return BatchApplication::Rejected;
        };
        if batch.validate().is_err() {
            return BatchApplication::Rejected;
        }
        let baseline = batch.source_start == batch.source_end
            && batch.events.is_empty()
            && !batch.contains_gap;
        if !self
            .windows
            .iter()
            .any(|window| window.binding == batch.binding)
            && (!baseline || !self.bind(batch.binding.clone()))
        {
            return BatchApplication::Rejected;
        }
        let Some(window) = self
            .windows
            .iter_mut()
            .find(|window| window.binding == batch.binding)
        else {
            return BatchApplication::Rejected;
        };
        let Some(previous) = window.source_position else {
            if !baseline {
                return BatchApplication::Rejected;
            }
            window.activity.discard_evidence();
            window.admission_revision = window
                .admission_revision
                .and_then(|value| value.checked_add(1));
            window.source_position = Some(batch.source_end);
            return BatchApplication::Baseline;
        };
        if batch.source_end < previous || (batch.source_end == previous && !batch.contains_gap) {
            return BatchApplication::Duplicate;
        }
        window.source_position = Some(batch.source_end);
        if batch.contains_gap || batch.source_start != previous {
            // Never classify across a missing range or a partially duplicated
            // range. Forget evidence and interval ticks, retaining spacing.
            window.activity.discard_evidence();
            window.admission_revision = window
                .admission_revision
                .and_then(|value| value.checked_add(1));
            return BatchApplication::Gap;
        }
        let has_events = !batch.events.is_empty();
        for event in batch.events {
            window.activity.push(event);
        }
        BatchApplication::Applied {
            ready: has_events && window.activity.ready_at(now, spacing),
        }
    }

    pub fn configure(&mut self, settings: &PollingSettings) {
        let next = (settings.enabled && settings.validate().is_ok()).then_some((
            usize::from(settings.event_interval),
            usize::from(settings.window_events),
            Duration::from_secs(u64::from(settings.minimum_spacing_seconds)),
        ));
        if self.configuration != next {
            self.windows.clear();
        }
        self.configuration = next;
    }

    /// Bind only a currently verified server incarnation and agent generation.
    /// The caller must authenticate incarnation changes before invoking this.
    pub fn bind(&mut self, binding: AgentBinding) -> bool {
        let Some((interval, capacity, _)) = self.configuration else {
            return false;
        };
        if [&binding.session, &binding.incarnation]
            .iter()
            .any(|value| value.is_empty() || value.len() > MAX_IDENTITY_BYTES)
        {
            return false;
        }
        if let Some(window) = self.windows.iter_mut().find(|window| {
            window.binding.incarnation == binding.incarnation && window.binding.pane == binding.pane
        }) {
            if window.binding == binding {
                return true;
            }
            if binding.generation < window.binding.generation {
                return false;
            }
            *window = BoundWindow {
                binding,
                activity: ActivityWindow::new(interval, capacity),
                source_position: None,
                identity: Arc::new(()),
                admission_revision: Some(0),
            };
            return true;
        }
        if self.windows.len() == MAX_WINDOWS {
            self.dropped_capacity = self.dropped_capacity.saturating_add(1);
            return false;
        }
        self.windows.push(BoundWindow {
            binding,
            activity: ActivityWindow::new(interval, capacity),
            source_position: None,
            identity: Arc::new(()),
            admission_revision: Some(0),
        });
        true
    }

    pub fn observe(&mut self, binding: &AgentBinding, event: ActivityEvent, now: Duration) -> bool {
        let Some((_, _, spacing)) = self.configuration else {
            return false;
        };
        let Some(window) = self
            .windows
            .iter_mut()
            .find(|window| &window.binding == binding)
        else {
            return false;
        };
        window.activity.push(event);
        window.activity.ready_at(now, spacing)
    }

    /// A check resets the interval only when fresh activity is ready.
    pub fn mark_checked(&mut self, binding: &AgentBinding, now: Duration) -> bool {
        let Some((_, _, spacing)) = self.configuration else {
            return false;
        };
        let Some(window) = self
            .windows
            .iter_mut()
            .find(|window| &window.binding == binding)
        else {
            return false;
        };
        if !window.activity.ready_at(now, spacing) {
            return false;
        }
        window.admission_revision = window
            .admission_revision
            .and_then(|value| value.checked_add(1));
        if window.admission_revision.is_none() {
            return false;
        }
        window.activity.mark_checked_at(now);
        true
    }

    /// Capture the currently ready prefix before deferring admission. Events
    /// arriving later remain eligible for the next interval after admission.
    pub fn capture_check_mark(
        &self,
        binding: &AgentBinding,
        now: Duration,
    ) -> Option<WindowCheckMark> {
        let (_, _, spacing) = self.configuration?;
        let window = self
            .windows
            .iter()
            .find(|window| &window.binding == binding)?;
        if !window.activity.ready_at(now, spacing) || window.activity.pending_count() == usize::MAX
        {
            return None;
        }
        Some(WindowCheckMark {
            identity: Arc::clone(&window.identity),
            revision: window.admission_revision?,
            eligible_events: window.activity.pending_count(),
            captured_at: now,
        })
    }

    /// Acknowledge one captured prefix, once, on its original continuous window.
    /// This carries no native provenance or budget authorization.
    pub fn mark_checked_through(&mut self, mark: &WindowCheckMark, now: Duration) -> bool {
        let Some((_, _, spacing)) = self.configuration else {
            return false;
        };
        let Some(window) = self
            .windows
            .iter_mut()
            .find(|window| Arc::ptr_eq(&window.identity, &mark.identity))
        else {
            return false;
        };
        if window.admission_revision != Some(mark.revision)
            || now < mark.captured_at
            || !window.activity.ready_at(now, spacing)
            || mark.eligible_events > window.activity.pending_count()
        {
            return false;
        }
        window.admission_revision = mark.revision.checked_add(1);
        if window.admission_revision.is_none() {
            return false;
        }
        window
            .activity
            .mark_checked_through(mark.eligible_events, now);
        true
    }

    pub fn dropped_capacity(&self) -> u64 {
        self.dropped_capacity
    }

    /// Release only the exact generation whose closure has been verified.
    pub fn unbind(&mut self, binding: &AgentBinding) -> bool {
        let Some(index) = self
            .windows
            .iter()
            .position(|window| &window.binding == binding)
        else {
            return false;
        };
        self.windows.swap_remove(index);
        true
    }

    pub fn events(&self, binding: &AgentBinding) -> Option<impl Iterator<Item = &ActivityEvent>> {
        self.windows
            .iter()
            .find(|window| &window.binding == binding)
            .map(|window| window.activity.events())
    }
}
