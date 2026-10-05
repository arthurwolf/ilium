//! Native review custody. UI intent is never an authority or durable receipt.
use super::permissions::{
    self, PackageOrigin, PermissionChoice, PermissionRequest, PermissionReview, ReviewIdentity,
};
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ilium_animation_js::{
    manifest::Capability,
    package::Package,
    permissions::{
        Capability as NativeCapability, PackageIdentity, PlanReview, Scope, Selection, UserChoice,
        Verdict,
    },
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ratatui::{
    layout::{Position, Rect},
    style::Style,
    widgets::{Clear, Paragraph},
    Frame,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::Notify;
type Result<T> = std::result::Result<T, String>;
const REVIEW_BYTES: usize = 1024 * 1024;
const MAILBOX_BYTES: usize = 32 * 1024;
const STATUS_BYTES: usize = 2048;
const MAX_PROJECTION_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeReviewKey {
    selection_revision: u64,
    instance_id: u64,
    plan_revision: u64,
    authorization_epoch: u64,
}
impl NativeReviewKey {
    pub(crate) fn from_native(selection_revision: u64, review: &PlanReview) -> Result<Self> {
        let key = Self {
            selection_revision,
            instance_id: review.instance_id(),
            plan_revision: review.plan_revision(),
            authorization_epoch: review.authorization_epoch(),
        };
        if selection_revision == 0
            || key.instance_id == 0
            || key.plan_revision == 0
            || key.authorization_epoch == 0
        {
            return Err("Invalid native review fence".into());
        }
        Ok(key)
    }
    pub(crate) fn matches_authority(self, instance_id: u64, revision: u64, epoch: u64) -> bool {
        self.instance_id == instance_id
            && self.plan_revision == revision
            && self.authorization_epoch == epoch
    }
}
/// No Deserialize, public fields, or JSON constructor. Snapshot custody retains
/// its ORIGINAL root debit even after the worker/slot drops its last reference.
pub(crate) struct ReviewEnvelope {
    key: NativeReviewKey,
    _principal: PackageIdentity,
    projection: PermissionReview,
    verdicts: Vec<Verdict>,
    session_alive: AtomicBool,
    _storage: StorageAdmission,
}
impl ReviewEnvelope {
    fn capture(
        quota: &QuotaGroup,
        selection: u64,
        package: &Package,
        principal: &PackageIdentity,
        review: &PlanReview,
    ) -> Result<Arc<Self>> {
        let storage = quota
            .reserve_external_storage(REVIEW_BYTES)
            .map_err(|error| format!("Permission review admission: {error:?}"))?;
        let digest: String = principal
            .content_hash()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let principal_key = principal.principal_key();
        let expected_unsigned = format!("unsigned:{}:{digest}", package.manifest().id);
        let expected_official = format!("ilium:{}", package.manifest().id);
        if principal_key != expected_unsigned && principal_key != expected_official {
            return Err("Review principal package ID differs from verified archive".into());
        }
        if digest != package.digest() {
            return Err("Review principal does not match verified archive".into());
        }
        let key = NativeReviewKey::from_native(selection, review)?;
        if review.items().len() > 64 {
            return Err("Too many native review rights".into());
        }
        // Bound serialized strings/maps BEFORE the UI projection and its one
        // mutable session copy are constructed. A counting writer allocates no
        // second serialization buffer. This serializes requests, never grants.
        struct Counter(usize);
        impl std::io::Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0 = self
                    .0
                    .checked_add(bytes.len())
                    .ok_or_else(|| std::io::Error::other("Review size overflow"))?;
                if self.0 > MAX_PROJECTION_BYTES {
                    return Err(std::io::Error::other("Review projection too large"));
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        serde_json::to_writer(Counter(0), review.items()).map_err(|error| error.to_string())?;
        let mut requests = Vec::with_capacity(review.items().len());
        for item in review.items() {
            let id = serde_json::to_value(item.request.right.id)
                .map_err(|error| error.to_string())?
                .as_str()
                .ok_or("Native capability name missing")?
                .to_owned();
            requests.push(PermissionRequest {
                request_id: item.request.request_id.clone(),
                capability: Capability {
                    id,
                    scope: serde_json::to_value(&item.request.right.scope)
                        .map_err(|error| error.to_string())?,
                },
                required: item.request.required,
                reason: item.request.reason.clone(),
            });
        }
        let origin = if principal.principal_key().starts_with("ilium:") {
            PackageOrigin::VerifiedIlium
        } else {
            PackageOrigin::Unverified
        };
        let projection = PermissionReview::new(
            ReviewIdentity {
                package_digest: digest,
                instance_generation: key.instance_id,
                plan_revision: key.plan_revision,
                authorization_epoch: key.authorization_epoch,
            },
            &package.manifest().name,
            "Verified archive · native plan",
            origin,
            requests,
        )?;
        Ok(Arc::new(Self {
            key,
            _principal: principal.clone(),
            projection,
            verdicts: review.items().iter().map(|item| item.verdict).collect(),
            session_alive: AtomicBool::new(false),
            _storage: storage,
        }))
    }
}
struct PendingReview {
    envelope: Arc<ReviewEnvelope>,
    token: PlanReview,
}
/// Actual state, supplied by the original worker/controller; these labels never
/// imply storage success, creation or activation without the corresponding event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReviewPhase {
    Inactive,
    Loading,
    Review,
    Resolving,
    DurableWrite,
    Creating,
    Ready,
    Cancelled,
    Failed,
}
struct Mailbox {
    selection: u64,
    pending: Option<PendingReview>,
    intent: Option<Intent>,
    phase: ReviewPhase,
    error: String,
}
enum Intent {
    Submit {
        envelope: Arc<ReviewEnvelope>,
        answers: BTreeMap<String, UserChoice>,
    },
    Cancel {
        envelope: Arc<ReviewEnvelope>,
    },
    Pick {
        envelope: Arc<ReviewEnvelope>,
        request_id: String,
        path: String,
    },
}
/// Returned only to the native owning worker. resolve consumes the ORIGINAL
/// broker-issued PlanReview, never a reconstructed stamp or deserialized grant.
pub(crate) enum ReviewAction {
    Resolve {
        selection_revision: u64,
        review: PlanReview,
        answers: BTreeMap<String, UserChoice>,
    },
    Cancel {
        selection_revision: u64,
    },
    Pick {
        selection_revision: u64,
        request_id: String,
        path: String,
        slot: String,
        disk_selection: Selection,
        writable: bool,
        review_revision: u64,
        authorization_epoch: u64,
    },
    PickAudio {
        selection_revision: u64,
        request_id: String,
        endpoint: String,
        scope_device: String,
        capability: NativeCapability,
        review_revision: u64,
        authorization_epoch: u64,
    },
}
pub(crate) struct ReviewBridge {
    quota: QuotaGroup,
    state: Mutex<Mailbox>,
    ui_ready: Arc<Notify>,
    wake_worker: Box<dyn Fn() + Send + Sync>,
    _storage: StorageAdmission,
}
impl ReviewBridge {
    pub(crate) fn new(
        quota: QuotaGroup,
        ui_ready: Arc<Notify>,
        wake_worker: Box<dyn Fn() + Send + Sync>,
    ) -> Result<Arc<Self>> {
        let storage = quota
            .reserve_external_storage(MAILBOX_BYTES)
            .map_err(|error| format!("Permission mailbox admission: {error:?}"))?;
        Ok(Arc::new(Self {
            quota,
            state: Mutex::new(Mailbox {
                selection: 0,
                pending: None,
                intent: None,
                phase: ReviewPhase::Inactive,
                error: String::new(),
            }),
            ui_ready,
            wake_worker,
            _storage: storage,
        }))
    }
    /// Call when a native configuration revision is ADMITTED, including pause
    /// and native selection. Dropping tokens does not replace worker teardown.
    pub(crate) fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.quota.shares_root(quota)
    }

    pub(crate) fn select(&self, selection: u64, plugin: bool) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Permission mailbox poisoned")?;
        if selection == 0 || selection <= state.selection {
            return Err("Stale native selection".into());
        }
        state.selection = selection;
        state.pending = None;
        state.intent = None;
        state.error.clear();
        state.phase = if plugin {
            ReviewPhase::Loading
        } else {
            ReviewPhase::Inactive
        };
        drop(state);
        self.ui_ready.notify_one();
        (self.wake_worker)();
        Ok(())
    }
    pub(crate) fn publish(
        &self,
        selection: u64,
        package: &Package,
        principal: &PackageIdentity,
        review: PlanReview,
    ) -> Result<()> {
        let envelope =
            ReviewEnvelope::capture(&self.quota, selection, package, principal, &review)?;
        let mut state = self.state.try_lock().map_err(|_| {
            "Permission mailbox busy or poisoned; native activation remains pending"
        })?;
        if state.selection != selection || state.pending.is_some() || state.intent.is_some() {
            return Err("Native review superseded or already pending".into());
        }
        state.pending = Some(PendingReview {
            envelope,
            token: review,
        });
        state.phase = ReviewPhase::Review;
        state.error.clear();
        drop(state);
        self.ui_ready.notify_one();
        Ok(())
    }
    /// Call for every original broker epoch/plan transition BEFORE publishing
    /// new acquisition/frames. Root resolve checks its own broker again.
    pub(crate) fn authority_changed(
        &self,
        selection: u64,
        instance: u64,
        revision: u64,
        epoch: u64,
    ) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Permission mailbox poisoned")?;
        if state.selection != selection {
            return Err("Native selection superseded".into());
        }
        if state.pending.as_ref().is_some_and(|pending| {
            !pending
                .envelope
                .key
                .matches_authority(instance, revision, epoch)
        }) {
            state.pending = None;
            state.intent = None;
            state.phase = ReviewPhase::Loading;
        }
        drop(state);
        self.ui_ready.notify_one();
        Ok(())
    }
    /// Native owner has failed/invalidated its workflow. This discards UI
    /// tokens only; it never replaces original job cancellation or teardown.
    pub(crate) fn deny_pending(&self, selection: u64, error: &str) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Permission mailbox poisoned")?;
        if state.selection != selection {
            return Err("Native failure belongs to an old selection".into());
        }
        state.pending = None;
        state.intent = None;
        state.phase = ReviewPhase::Failed;
        state.error = error
            .chars()
            .filter(|character| !character.is_control())
            .take(240)
            .collect();
        drop(state);
        self.ui_ready.notify_one();
        Ok(())
    }
    pub(crate) fn set_phase(
        &self,
        selection: u64,
        phase: ReviewPhase,
        error: Option<&str>,
    ) -> Result<()> {
        if matches!(
            phase,
            ReviewPhase::Review | ReviewPhase::Inactive | ReviewPhase::Loading
        ) {
            return Err("Use native selection/publication methods for this phase".into());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Permission mailbox poisoned")?;
        if state.selection != selection {
            return Err("Native phase is stale".into());
        }
        if state.pending.is_some() || state.intent.is_some() {
            return Err("Native review is still owned".into());
        }
        state.phase = phase;
        state.error = error
            .unwrap_or_default()
            .chars()
            .filter(|character| !character.is_control())
            .take(240)
            .collect();
        drop(state);
        self.ui_ready.notify_one();
        Ok(())
    }
    pub(crate) fn status(&self) -> Result<ReviewStatus> {
        let storage = self
            .quota
            .reserve_external_storage(STATUS_BYTES)
            .map_err(|error| format!("Permission status admission: {error:?}"))?;
        let state = self
            .state
            .lock()
            .map_err(|_| "Permission mailbox poisoned")?;
        Ok(ReviewStatus {
            phase: state.phase,
            error: state.error.clone(),
            _storage: storage,
        })
    }
    pub(crate) fn session(&self) -> Result<Option<ReviewSession>> {
        let state = self
            .state
            .lock()
            .map_err(|_| "Permission mailbox poisoned")?;
        let Some(pending) = state.pending.as_ref().filter(|_| state.intent.is_none()) else {
            return Ok(None);
        };
        if pending
            .envelope
            .session_alive
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(None);
        }
        Ok(Some(ReviewSession {
            envelope: pending.envelope.clone(),
            view: pending.envelope.projection.clone(),
            choice_cursor: 2,
            error: state.error.clone(),
            selection_input: None,
        }))
    }
    pub(crate) fn picker_error(&self, selection: u64, error: &str) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Permission mailbox poisoned")?;
        if state.selection != selection
            || state.phase != ReviewPhase::Review
            || state.pending.is_none()
            || state.intent.is_some()
        {
            return Err("Native picker error belongs to a stale review".into());
        }
        state.error = error
            .chars()
            .filter(|character| !character.is_control())
            .take(240)
            .collect();
        drop(state);
        self.ui_ready.notify_one();
        Ok(())
    }
    pub(crate) fn is_current(&self, session: &ReviewSession) -> bool {
        self.state.lock().ok().is_some_and(|state| {
            state.intent.is_none()
                && state
                    .pending
                    .as_ref()
                    .is_some_and(|pending| Arc::ptr_eq(&pending.envelope, &session.envelope))
        })
    }
    fn queue(&self, session: &ReviewSession, cancel: bool) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Permission mailbox poisoned")?;
        let pending = state
            .pending
            .as_ref()
            .ok_or("Native review no longer pending")?;
        if state.intent.is_some() || !Arc::ptr_eq(&pending.envelope, &session.envelope) {
            return Err("Permission intent is stale or already queued".into());
        }
        let intent = if cancel {
            Intent::Cancel {
                envelope: session.envelope.clone(),
            }
        } else {
            let decisions = session
                .view
                .decisions(&session.envelope.projection.identity)?;
            let answers = decisions
                .into_iter()
                .map(|decision| (decision.request_id, decision.choice.native_intent()))
                .collect();
            Intent::Submit {
                envelope: session.envelope.clone(),
                answers,
            }
        };
        state.intent = Some(intent);
        state.phase = ReviewPhase::Resolving;
        drop(state);
        (self.wake_worker)();
        self.ui_ready.notify_one();
        Ok(())
    }
    fn queue_pick(&self, session: &ReviewSession, path: String) -> Result<()> {
        if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
            return Err("Selected path is empty or outside native limits".into());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Permission mailbox poisoned")?;
        let pending = state
            .pending
            .as_ref()
            .ok_or("Native review no longer pending")?;
        if state.intent.is_some() || !Arc::ptr_eq(&pending.envelope, &session.envelope) {
            return Err("Native picker intent is stale".into());
        }
        if !pending
            .envelope
            .verdicts
            .get(session.view.cursor)
            .is_some_and(|verdict| *verdict == Verdict::NeedsSelection)
        {
            return Err("Current right does not need a native selection".into());
        }
        let request_id = pending
            .envelope
            .projection
            .requests
            .get(session.view.cursor)
            .ok_or("Current native right missing")?
            .request_id
            .clone();
        state.intent = Some(Intent::Pick {
            envelope: session.envelope.clone(),
            request_id,
            path,
        });
        state.phase = ReviewPhase::Loading;
        drop(state);
        (self.wake_worker)();
        self.ui_ready.notify_one();
        Ok(())
    }
    pub(crate) fn has_intent(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.intent.is_some())
            .unwrap_or(false)
    }
    /// Worker must obtain the arguments from its CURRENT controller/broker,
    /// never from ReviewSession or JSON. An old intent cannot revive old review.
    pub(crate) fn take_action(
        &self,
        selection: u64,
        instance: u64,
        revision: u64,
        epoch: u64,
    ) -> Result<Option<ReviewAction>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Permission mailbox poisoned")?;
        if state.selection != selection {
            return Err("Native action selection is stale".into());
        }
        let Some(pending) = state.pending.as_ref() else {
            return Ok(None);
        };
        if pending.envelope.key.selection_revision != selection
            || !pending
                .envelope
                .key
                .matches_authority(instance, revision, epoch)
        {
            state.pending = None;
            state.intent = None;
            state.phase = ReviewPhase::Loading;
            return Err("Native permission authority changed".into());
        }
        let Some(intent) = state.intent.take() else {
            return Ok(None);
        };
        let pending = state.pending.take().ok_or("Native review disappeared")?;
        let envelope = match &intent {
            Intent::Submit { envelope, .. }
            | Intent::Cancel { envelope }
            | Intent::Pick { envelope, .. } => envelope,
        };
        if !Arc::ptr_eq(envelope, &pending.envelope) {
            return Err("Wrong native review token".into());
        }
        match intent {
            Intent::Submit { answers, .. } => Ok(Some(ReviewAction::Resolve {
                selection_revision: selection,
                review: pending.token,
                answers,
            })),
            Intent::Cancel { .. } => {
                // Cancellation is intent until the owning controller settles
                // original teardown/invalidation; do not claim it completed.
                Ok(Some(ReviewAction::Cancel {
                    selection_revision: selection,
                }))
            }
            Intent::Pick {
                request_id, path, ..
            } => {
                let item = pending
                    .token
                    .items()
                    .iter()
                    .find(|item| item.request.request_id == request_id)
                    .ok_or("Native picker right disappeared")?;
                if item.verdict != Verdict::NeedsSelection {
                    return Err("Native picker verdict changed".into());
                }
                if let Scope::Audio { device, .. } = &item.request.right.scope {
                    if !matches!(
                        item.request.right.id,
                        NativeCapability::AudioLoopback | NativeCapability::AudioMicrophone
                    ) {
                        return Err("Native audio picker capability mismatch".into());
                    }
                    return Ok(Some(ReviewAction::PickAudio {
                        selection_revision: selection,
                        request_id,
                        endpoint: path,
                        scope_device: device.clone(),
                        capability: item.request.right.id,
                        review_revision: pending.token.plan_revision(),
                        authorization_epoch: pending.token.authorization_epoch(),
                    }));
                }
                let Scope::Disk {
                    slot,
                    selection: disk_selection,
                } = &item.request.right.scope
                else {
                    return Err("Native picker right is neither disk nor audio selection".into());
                };
                let writable = item.request.right.id == NativeCapability::DiskWrite;
                if !writable && item.request.right.id != NativeCapability::DiskRead {
                    return Err("Native picker capability mismatch".into());
                }
                Ok(Some(ReviewAction::Pick {
                    selection_revision: selection,
                    request_id,
                    path,
                    slot: slot.clone(),
                    disk_selection: *disk_selection,
                    writable,
                    review_revision: pending.token.plan_revision(),
                    authorization_epoch: pending.token.authorization_epoch(),
                }))
            }
        }
    }
}
/// Only ONE session may be installed by App for a given envelope. Its admission
/// is retained by the immutable envelope; it is not serializable/constructible.
pub(crate) struct ReviewStatus {
    phase: ReviewPhase,
    error: String,
    _storage: StorageAdmission,
}
impl ReviewStatus {
    pub(crate) fn phase(&self) -> ReviewPhase {
        self.phase
    }
    pub(crate) fn message(&self) -> &str {
        if !self.error.is_empty() {
            return &self.error;
        }
        match self.phase {
            ReviewPhase::Inactive => "",
            ReviewPhase::Loading => "Loading native permission ledger",
            ReviewPhase::Review => "Animation permissions need review",
            ReviewPhase::Resolving => "Applying native permission intent",
            ReviewPhase::DurableWrite => "Saving remembered choices",
            ReviewPhase::Creating => "Creating permitted animation",
            ReviewPhase::Ready => "Animation ready",
            ReviewPhase::Cancelled => "Animation activation cancelled",
            ReviewPhase::Failed => "Animation activation failed",
        }
    }
}
pub(crate) struct ReviewSession {
    envelope: Arc<ReviewEnvelope>,
    view: PermissionReview,
    choice_cursor: usize,
    error: String,
    selection_input: Option<String>,
}
impl ReviewSession {
    pub(crate) fn handle_key(&mut self, bridge: &ReviewBridge, key: KeyCode) -> Result<()> {
        if !bridge.is_current(self) {
            return Err("Permission review changed; await native refresh".into());
        }
        if let Some(path) = self.selection_input.as_mut() {
            match key {
                KeyCode::Esc => {
                    self.selection_input = None;
                    return Ok(());
                }
                KeyCode::Backspace => {
                    path.pop();
                    return Ok(());
                }
                KeyCode::Char(character) if !character.is_control() => {
                    if path.len() + character.len_utf8() <= 4096 {
                        path.push(character);
                    }
                    return Ok(());
                }
                KeyCode::Enter => {
                    let path = self
                        .selection_input
                        .take()
                        .ok_or("Native path input vanished")?;
                    return bridge.queue_pick(self, path);
                }
                _ => return Ok(()),
            }
        }
        match key {
            KeyCode::Esc => bridge.queue(self, true),
            KeyCode::Char('p')
                if self
                    .envelope
                    .verdicts
                    .get(self.view.cursor)
                    .is_some_and(|verdict| *verdict == Verdict::NeedsSelection) =>
            {
                self.selection_input = Some(String::new());
                Ok(())
            }
            KeyCode::Up => {
                self.choice_cursor = self.choice_cursor.saturating_sub(1);
                Ok(())
            }
            KeyCode::Down => {
                self.choice_cursor = (self.choice_cursor + 1).min(3);
                Ok(())
            }
            KeyCode::Left | KeyCode::BackTab => {
                self.view.cursor = self.view.cursor.saturating_sub(1);
                Ok(())
            }
            KeyCode::Right | KeyCode::Tab => {
                self.view.cursor =
                    (self.view.cursor + 1).min(self.view.requests.len().saturating_sub(1));
                Ok(())
            }
            KeyCode::PageUp => {
                self.view.detail_scroll = self.view.detail_scroll.saturating_sub(4);
                Ok(())
            }
            KeyCode::PageDown => {
                self.view.detail_scroll = self.view.detail_scroll.saturating_add(4);
                Ok(())
            }
            KeyCode::Char('1'..='4') => {
                let KeyCode::Char(number) = key else {
                    return Ok(());
                };
                self.choose(usize::from(number as u8 - b'1'))
            }
            KeyCode::Char(' ') => self.choose(self.choice_cursor),
            KeyCode::Enter => bridge.queue(self, false),
            _ => Ok(()),
        }
    }
    fn choose(&mut self, index: usize) -> Result<()> {
        let choice = PermissionChoice::ALL
            .get(index)
            .copied()
            .ok_or("Unknown native choice")?;
        if matches!(
            choice,
            PermissionChoice::AllowSession | PermissionChoice::AllowRemembered
        ) && self
            .envelope
            .verdicts
            .get(self.view.cursor)
            .is_some_and(|verdict| matches!(verdict, Verdict::HostDenied | Verdict::NeedsSelection))
        {
            return Err("This right needs a native host resource selection or is unavailable under host policy".into());
        }
        self.view.decide(self.view.cursor, choice)?;
        self.choice_cursor = index;
        if self.view.cursor + 1 < self.view.requests.len() {
            self.view.cursor += 1;
        }
        Ok(())
    }
    pub(crate) fn handle_mouse(
        &mut self,
        bridge: &ReviewBridge,
        area: Rect,
        mouse: MouseEvent,
    ) -> Result<()> {
        if !bridge.is_current(self) {
            return Err("Permission review changed; await native refresh".into());
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(choice) =
                    permissions::permission_choice_at(area, Position::new(mouse.column, mouse.row))
                {
                    let index = PermissionChoice::ALL
                        .iter()
                        .position(|candidate| *candidate == choice)
                        .ok_or("Unknown native choice")?;
                    return self.choose(index);
                }
                if review_submit_rect(area)
                    .is_some_and(|rect| rect.contains(Position::new(mouse.column, mouse.row)))
                {
                    return bridge.queue(self, false);
                }
                Ok(())
            }
            MouseEventKind::ScrollUp => {
                self.view.detail_scroll = self.view.detail_scroll.saturating_sub(2);
                Ok(())
            }
            MouseEventKind::ScrollDown => {
                self.view.detail_scroll = self.view.detail_scroll.saturating_add(2);
                Ok(())
            }
            MouseEventKind::Down(MouseButton::Right) => bridge.queue(self, true),
            _ => Ok(()),
        }
    }
    pub(crate) fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, style: Style) {
        frame.render_widget(Clear, area);
        permissions::draw_permission_review(frame, area, &self.view, style);
        if area.height >= 8 {
            let availability = match self.envelope.verdicts.get(self.view.cursor) {
                Some(Verdict::HostDenied) => "Unavailable under native host policy",
                Some(Verdict::NeedsSelection) => {
                    "Choose a native file/device before allowing this right"
                }
                Some(Verdict::Allowed) => "Previously allowed by native broker; you may revoke it",
                Some(Verdict::UserDenied) => "Previously denied by native broker",
                _ => "No consent recorded for this right",
            };
            frame.render_widget(
                Paragraph::new(availability).style(style),
                Rect::new(area.x, area.y + 2, area.width, 1),
            );
        }
        if let Some(choice) = PermissionChoice::ALL.get(self.choice_cursor).copied() {
            if let Some(rect) = permissions::permission_choice_rect(area, choice) {
                let mut choice_style = style.add_modifier(ratatui::style::Modifier::UNDERLINED);
                if self.view.choice(self.view.cursor) == Some(choice) {
                    choice_style = choice_style.add_modifier(ratatui::style::Modifier::REVERSED);
                }
                frame.render_widget(Paragraph::new(choice.label()).style(choice_style), rect);
            }
        }

        if let Some(rect) = review_submit_rect(area) {
            let footer = if self.error.is_empty() {
                if let Some(path) = &self.selection_input {
                    frame.render_widget(
                        Paragraph::new(format!("Selected path or exact audio source: {path}"))
                            .style(style),
                        rect,
                    );
                    return;
                }
                "1–4/Space chooses · Tab changes right · P selects path/source · Enter submits · Esc cancels"
            } else {
                self.error.as_str()
            };
            frame.render_widget(Paragraph::new(footer).style(style), rect);
        }
    }
    pub(crate) fn set_error(&mut self, error: &str) {
        self.error = error
            .chars()
            .filter(|character| !character.is_control())
            .take(240)
            .collect();
    }
}
pub(crate) fn review_submit_rect(area: Rect) -> Option<Rect> {
    (area.width > 0 && area.height > 0)
        .then(|| Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1))
}

impl Drop for ReviewSession {
    fn drop(&mut self) {
        self.envelope.session_alive.store(false, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "review_bridge_tests.rs"]
mod tests;
