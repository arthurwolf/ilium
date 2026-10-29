//! Preparation of the exact immutable frame captured by a context-menu click.
use crate::{
    terminal_selection::TerminalSelection,
    terminal_view::{PaintedTerminal, PreparationSnapshot},
};
use ilium_core::NodeId;
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason, Retained,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Notify;
#[derive(Clone)]
pub(crate) struct ContextKey {
    pub pane_id: NodeId,
    pub identity: Arc<()>,
    pub generation: u64,
    pub column: u16,
    pub row: u16,
    pub snapshot_ordinal: u64,
}
/// Agent class, session identity, pane launch directory and the optional
/// server-verified transcript path (a hint re-verified before use).
pub(crate) type HistoryContext = (ilium_core::AgentClass, String, PathBuf, Option<PathBuf>);

pub(crate) struct ContextRequest {
    pub key: ContextKey,
    pub source_row: usize,
    pub column: usize,
    pub selection: Option<TerminalSelection>,
    pub cwd: PathBuf,
    pub home: Option<PathBuf>,
    pub history_context: Option<HistoryContext>,
}
pub(crate) struct ContextText {
    pub source_line_text: String,
    pub visible_contents: String,
    pub full_history: String,
    pub selection_text: Option<String>,
    pub open_target: Option<crate::open_target::OpenTarget>,
    pub history_path: Option<PathBuf>,
    pub warning: Option<String>,
    pub source_hold: Option<Retained<()>>,
}
struct ContextJob {
    snapshot: PreparationSnapshot,
    source_row: usize,
    column: usize,
    selection: Option<TerminalSelection>,
}
impl Job for ContextJob {
    type Output = ContextText;
    type Error = String;
    fn run(self, context: JobContext) -> Result<ContextText, String> {
        if context.stop_requested() {
            return Err("context text preparation cancelled".into());
        }
        let bytes = self.snapshot.history.to_vec();
        let stripped = strip_ansi_escapes::strip(&bytes);
        let full_history = String::from_utf8_lossy(&stripped).into_owned();
        if context.stop_requested() {
            return Err("context text preparation cancelled".into());
        }
        let visible_contents = self.snapshot.visible.contents();
        let source_line_text = visible_contents
            .lines()
            .nth(self.source_row)
            .map(|line| line.trim_end().to_owned())
            .unwrap_or_default();
        let selection_text = self.selection.as_ref().and_then(|selection| {
            crate::terminal_selection::text(&self.snapshot.visible, selection)
        });
        let column =
            crate::terminal_links::cell_column_to_byte_offset(&source_line_text, self.column);
        let osc_target = self
            .snapshot
            .links
            .iter()
            .rev()
            .find_map(|(label, target)| {
                let start = source_line_text.find(label)?;
                (start..start + label.len())
                    .contains(&column)
                    .then(|| target.clone())
            });
        // Only URL recognition here: path metadata is a separate finite I/O job.
        let open_target = osc_target
            .as_deref()
            .and_then(crate::open_target::resolve_url);
        Ok(ContextText {
            source_line_text,
            visible_contents,
            full_history,
            selection_text,
            open_target,
            history_path: None,
            warning: None,
            source_hold: None,
        })
    }
}
struct ResolveJob {
    text: ContextText,
    column: usize,
    cwd: PathBuf,
    home: Option<PathBuf>,
    history_context: Option<HistoryContext>,
}
impl Job for ResolveJob {
    type Output = ContextText;
    type Error = String;
    fn run(mut self, context: JobContext) -> Result<ContextText, String> {
        if context.stop_requested() {
            return Err("context target resolution cancelled".into());
        }
        self.text.open_target = crate::open_target::resolve_at(
            &self.text.source_line_text,
            self.column,
            &self.cwd,
            self.home.as_deref(),
        )
        .or(self.text.open_target);
        if let (Some(home), Some((class, session, path, transcript_hint))) =
            (&self.home, &self.history_context)
        {
            if matches!(
                class,
                ilium_core::AgentClass::Claude | ilium_core::AgentClass::Codex
            ) {
                let locator = ilium_agent_session::TranscriptLocator::new_bounded(
                    home,
                    path,
                    ilium_agent_session::INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS,
                );
                self.text.history_path = crate::agent_history_path::resolve_history_path(
                    &locator,
                    class,
                    session,
                    transcript_hint.as_deref(),
                );
                if locator.read_limit_reached() {
                    // Optional path verification must not discard the already
                    // captured screen/selection or disable prompt recovery.
                    self.text.history_path = None;
                    self.text.warning = Some(
                        "Transcript path unavailable: history scan exceeded its safety limit. Screen and prompt recovery are still available.".into(),
                    );
                }
            }
        }
        Ok(self.text)
    }
}
enum Pending {
    Cpu(Receipt<ContextJob>),
    Between(Retained<JobOutcome<ContextJob>>),
    Io(Receipt<ResolveJob>),
}
struct Slot {
    key: ContextKey,
    column: usize,
    cwd: PathBuf,
    home: Option<PathBuf>,
    pending: Pending,
    history_context: Option<HistoryContext>,
}
pub(crate) struct ContextCompletion {
    pub key: ContextKey,
    pub text: Result<Retained<ContextText>, String>,
}
pub(crate) struct PreparedCapturePayload {
    pub source: PaintedTerminal,
    pub session: crate::smart_copy::SmartCopySession,
    pub prompt: String,
    pub charge: Option<Arc<crate::terminal_parsing::SnapshotCharge>>,
    #[cfg(test)]
    retirement_probe: Option<tests::CaptureDropProbe>,
}
/// The original result debit is installed by the producing job, before any
/// receipt/owner can disappear. Discarding an output only moves an envelope.
/// Successful publication transfers custody into the existing session owner.
pub(crate) struct PreparedCapture {
    payload: Option<PreparedCapturePayload>,
    retirement: Option<ilium_execution::RetirementReservation<PreparedCapturePayload>>,
    session_retirement:
        Option<ilium_execution::RetirementReservation<crate::smart_copy::SmartCopySession>>,
    retention: Option<ilium_execution::Retention>,
}
impl std::ops::Deref for PreparedCapture {
    type Target = PreparedCapturePayload;
    fn deref(&self) -> &Self::Target {
        // Invariant: only consuming into_payload takes this value.
        self.payload.as_ref().expect("live capture payload")
    }
}
impl PreparedCapture {
    pub(crate) fn into_payload(mut self) -> PreparedCapturePayload {
        // Invariant: output creation installs all three owned custody fields;
        // consuming this wrapper is the sole successful publication transition.
        let mut payload = self.payload.take().expect("live capture payload");
        payload.session.capture_retention = self.retention.take();
        payload.session.return_retirement = self.session_retirement.take();
        payload
    }
}
impl Drop for PreparedCapture {
    fn drop(&mut self) {
        let Some(payload) = self.payload.take() else {
            return;
        };
        // Invariant: a payload is constructed only with its preadmitted permit.
        // Its session has no retirement permit until into_payload publishes it,
        // so CPU destruction cannot recursively queue another session retirement.
        let permit = self
            .retirement
            .take()
            .expect("capture retirement admitted before submission");
        let mut retired = permit.attach(payload);
        if let Some(retention) = self.retention.take() {
            retired.set_retention(retention);
        }
        drop(retired);
    }
}
struct CaptureJob {
    pane_id: NodeId,
    generation: u64,
    paths: crate::smart_copy_tokens::PathContext,
    session_bytes: usize,
    retirement: ilium_execution::RetirementReservation<PreparedCapturePayload>,
    session_retirement: ilium_execution::RetirementReservation<crate::smart_copy::SmartCopySession>,
    retention: ilium_execution::Retention,
    identity: Arc<()>,
    ordinal: u64,
    source: PreparationSnapshot,
    charge: Option<Arc<crate::terminal_parsing::SnapshotCharge>>,
}
impl Job for CaptureJob {
    type Output = PreparedCapture;
    type Error = String;
    fn run(self, context: JobContext) -> Result<PreparedCapture, String> {
        if context.stop_requested() {
            return Err("Smart Copy preparation cancelled".into());
        }
        let mut snapshot = crate::smart_copy::SmartCopySnapshot::capture(&self.source.visible);
        if let Some(charge) = &self.charge {
            snapshot.retain_allocation(charge.clone());
        }
        let prompt =
            crate::smart_copy::user_prompt(&snapshot).map_err(|error| error.to_string())?;
        if context.stop_requested() {
            return Err("Smart Copy preparation cancelled".into());
        }
        if snapshot.prepared_session_bytes()? > self.session_bytes {
            return Err(
                "Smart Copy candidate allocations exceed the admitted preparation envelope".into(),
            );
        }
        let session = crate::smart_copy::SmartCopySession::prepare_with_path_context(
            self.generation,
            self.pane_id,
            snapshot,
            &self.paths,
            || !context.stop_requested(),
        )?;
        Ok(PreparedCapture {
            payload: Some(PreparedCapturePayload {
                source: self.source.painted(self.identity, self.ordinal),
                session,
                prompt,
                charge: self.charge,
                #[cfg(test)]
                retirement_probe: None,
            }),
            retirement: Some(self.retirement),
            session_retirement: Some(self.session_retirement),
            retention: Some(self.retention),
        })
    }
}
pub(crate) struct CaptureCompletion {
    pub pane_id: NodeId,
    pub identity: Arc<()>,
    pub generation: u64,
    pub result: Result<Retained<PreparedCapture>, String>,
}
struct CaptureSlot {
    pane_id: NodeId,
    identity: Arc<()>,
    generation: u64,
    receipt: Receipt<CaptureJob>,
    cancelled: bool,
}
pub(crate) struct PreparedLink {
    #[cfg(test)]
    pub worker_thread: std::thread::ThreadId,
    pub link: Option<crate::terminal_links::TerminalLink>,
    pub storage: Option<Arc<ilium_execution::StorageAdmission>>,
}
pub(crate) struct LinkRequest<'a> {
    pub id: u64,
    pub pane_id: NodeId,
    pub row: usize,
    pub column: usize,
    pub cwd: &'a std::path::Path,
    pub home: Option<&'a std::path::Path>,
}
pub(crate) struct LinkCompletion {
    pub id: u64,
    pub pane_id: NodeId,
    pub identity: Arc<()>,
    pub result: Result<Retained<PreparedLink>, String>,
}
struct LinkSlot {
    id: u64,
    pane_id: NodeId,
    identity: Arc<()>,
    receipt: Receipt<LinkJob>,
}
struct LinkJob {
    source: PreparationSnapshot,
    row: usize,
    column: usize,
    cwd: PathBuf,
    home: Option<PathBuf>,
}
impl Job for LinkJob {
    type Output = PreparedLink;
    type Error = String;
    fn run(self, context: JobContext) -> Result<PreparedLink, String> {
        if context.stop_requested() {
            return Err("Terminal link preparation cancelled".into());
        }
        let (_, columns) = self.source.visible.size();
        let line = self
            .source
            .visible
            .rows(0, columns)
            .nth(self.row)
            .unwrap_or_default();
        let link =
            crate::terminal_links::link_at(&line, self.column, &self.cwd, self.home.as_deref())
                .or_else(|| {
                    let column =
                        crate::terminal_links::cell_column_to_byte_offset(&line, self.column);
                    self.source.links.iter().rev().find_map(|(label, target)| {
                        let start = line.find(label)?;
                        (start..start + label.len())
                            .contains(&column)
                            .then(|| crate::terminal_links::TerminalLink::Url(target.clone()))
                    })
                });
        if context.stop_requested() {
            return Err("Terminal link preparation cancelled".into());
        }
        let bytes = std::mem::size_of::<PreparedLink>()
            + match &link {
                Some(crate::terminal_links::TerminalLink::Url(url)) => url.capacity(),
                Some(crate::terminal_links::TerminalLink::File { path, .. }) => path.capacity(),
                None => 0,
            }
            + 128;
        let storage = if link.is_some() {
            Some(Arc::new(
                crate::execution::process_quota()
                    .reserve_external_storage(bytes)
                    .map_err(|reason| {
                        format!("Terminal link result storage admission: {reason:?}")
                    })?,
            ))
        } else {
            None
        };
        Ok(PreparedLink {
            link,
            storage,
            #[cfg(test)]
            worker_thread: std::thread::current().id(),
        })
    }
}
pub(crate) struct TerminalContextPreparation {
    client: Client,
    notification: Arc<Notify>,
    slot: Option<Slot>,
    capture: Option<CaptureSlot>,
    links: Vec<LinkSlot>,
}
impl TerminalContextPreparation {
    pub(crate) fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = notification.clone();
        Self {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            slot: None,
            capture: None,
            links: Vec::new(),
        }
    }
    pub(crate) fn notification(&self) -> Arc<Notify> {
        self.notification.clone()
    }
    pub(crate) fn request_link(
        &mut self,
        source: &PaintedTerminal,
        request: LinkRequest<'_>,
    ) -> Result<usize, String> {
        let LinkRequest {
            id,
            pane_id,
            row,
            column,
            cwd,
            home,
        } = request;
        if self.links.len() >= 8 {
            return Err(
                "Terminal link preparation queue is full; retry after pending clicks".into(),
            );
        }
        let (_, columns) = source.with_screen(|screen| screen.size());
        let paths = cwd
            .as_os_str()
            .len()
            .saturating_add(home.map_or(0, |home| home.as_os_str().len()));
        let result_bytes = paths
            .saturating_mul(2)
            .saturating_add(usize::from(columns).saturating_mul(128))
            .saturating_add(4096)
            .max(128 * 1024);
        let source_bytes = source.allocation_bytes();
        let cost = JobCost {
            input_bytes: source_bytes.saturating_add(paths.saturating_mul(2)),
            result_bytes,
        };
        let reservation = self
            .client
            .try_reserve(Lane::Cpu, cost)
            .map_err(|reason| format!("Terminal link CPU admission: {reason:?}"))?;
        let job = LinkJob {
            source: source.preparation_snapshot()?,
            row,
            column,
            cwd: cwd.to_path_buf(),
            home: home.map(std::path::Path::to_path_buf),
        };
        let receipt = reservation.submit(job).map_err(|rejected| {
            format!(
                "Terminal link preparation submission: {:?}",
                rejected.reason
            )
        })?;
        self.links.push(LinkSlot {
            id,
            pane_id,
            identity: source.identity.clone(),
            receipt,
        });
        Ok(result_bytes)
    }
    pub(crate) fn cancel_link(&mut self, id: u64) {
        if let Some(index) = self.links.iter().position(|slot| slot.id == id) {
            self.links.remove(index).receipt.cancel();
        }
    }
    pub(crate) fn collect_link(&mut self) -> Option<LinkCompletion> {
        for index in 0..self.links.len() {
            let result = match self.links[index].receipt.try_take() {
                JobPoll::Pending => continue,
                JobPoll::Ready(outcome) => {
                    let error = match outcome.view() {
                        JobOutcome::Finished(Ok(_)) => None,
                        JobOutcome::Finished(Err(error)) => Some(error.clone()),
                        JobOutcome::NotStarted { .. } => {
                            Some("Terminal link preparation cancelled before execution".into())
                        }
                        JobOutcome::Panicked => Some("Terminal link preparation panicked".into()),
                    };
                    match error {
                        Some(error) => Err(error),
                        None => Ok(outcome.map(|outcome| match outcome {
                            JobOutcome::Finished(Ok(value)) => value,
                            _ => unreachable!("owned successful link discriminant changed"),
                        })),
                    }
                }
                JobPoll::Lost | JobPoll::Taken => {
                    Err("Terminal link preparation completion lost".into())
                }
            };
            let slot = self.links.remove(index);
            return Some(LinkCompletion {
                id: slot.id,
                pane_id: slot.pane_id,
                identity: slot.identity,
                result,
            });
        }
        None
    }
    pub(crate) fn request_capture(
        &mut self,
        pane_id: NodeId,
        generation: u64,
        source: &PaintedTerminal,
        paths: crate::smart_copy_tokens::PathContext,
    ) -> Result<(), String> {
        // A cancelled metadata syscall can still be running. Keep its receipt
        // until physical completion; replacing it would multiply blocked jobs.
        if self.capture.is_some() {
            return Err("Smart Copy preparation is still settling; retry shortly".into());
        }
        let capture_cost = source.capture_cost()?;
        let (_, columns) = source.with_screen(|screen| screen.size());
        let path_bytes = paths.preparation_bytes(columns)?;
        let session_bytes = capture_cost
            .result_bytes
            .checked_mul(2)
            .ok_or_else(|| "Smart Copy session charge overflow".to_string())?;
        let cost = JobCost {
            input_bytes: capture_cost
                .input_bytes
                .checked_add(path_bytes)
                .ok_or_else(|| "Smart Copy input charge overflow".to_string())?,
            result_bytes: capture_cost
                .result_bytes
                .checked_add(session_bytes)
                .and_then(|bytes| bytes.checked_add(path_bytes))
                .ok_or_else(|| "Smart Copy result charge overflow".to_string())?,
        };
        let reservation = self
            .client
            // Capture is finite CPU work, followed by filesystem verification.
            // One I/O job avoids a retained CPU job waiting for another lane.
            .try_reserve(Lane::Io, cost)
            .map_err(|reason| format!("Smart Copy preparation admission: {reason:?}"))?;
        // Empty envelopes reserve only metadata/slots. No CPU job is awaited,
        // and heap custody remains on the same original capture reservation.
        let retirement = self
            .client
            .retirement()
            .try_reserve::<PreparedCapturePayload>(std::mem::size_of::<PreparedCapturePayload>())
            .map_err(|reason| format!("Smart Copy capture retirement admission: {reason:?}"))?;
        let session_retirement = self
            .client
            .retirement()
            .try_reserve::<crate::smart_copy::SmartCopySession>(std::mem::size_of::<
                crate::smart_copy::SmartCopySession,
            >())
            .map_err(|reason| format!("Smart Copy session retirement admission: {reason:?}"))?;
        let retention = reservation.retention();
        let job = CaptureJob {
            pane_id,
            generation,
            paths,
            session_bytes,
            retirement,
            session_retirement,
            retention,
            identity: source.identity.clone(),
            ordinal: source.ordinal,
            source: source.preparation_snapshot()?,
            charge: source.capture_charge(capture_cost.result_bytes)?,
        };
        let receipt = reservation.submit(job).map_err(|rejected| {
            format!("Smart Copy preparation submission: {:?}", rejected.reason)
        })?;
        self.capture = Some(CaptureSlot {
            pane_id,
            identity: source.identity.clone(),
            generation,
            receipt,
            cancelled: false,
        });
        Ok(())
    }
    pub(crate) fn cancel_capture(&mut self) {
        if let Some(slot) = self.capture.as_mut() {
            slot.cancelled = true;
            slot.receipt.cancel();
        }
    }
    pub(crate) fn collect_capture(&mut self) -> Option<CaptureCompletion> {
        let mut slot = self.capture.take()?;
        let result = match slot.receipt.try_take() {
            JobPoll::Pending => {
                self.capture = Some(slot);
                return None;
            }
            JobPoll::Ready(outcome) => {
                let error = match outcome.view() {
                    JobOutcome::Finished(Ok(_)) => None,
                    JobOutcome::Finished(Err(error)) => Some(error.clone()),
                    JobOutcome::NotStarted { .. } => {
                        Some("Smart Copy preparation cancelled before execution".into())
                    }
                    JobOutcome::Panicked => Some("Smart Copy preparation panicked".into()),
                };
                match error {
                    Some(error) => Err(error),
                    None => Ok(outcome.map(|outcome| match outcome {
                        JobOutcome::Finished(Ok(value)) => value,
                        _ => unreachable!("owned successful capture discriminant changed"),
                    })),
                }
            }
            JobPoll::Lost | JobPoll::Taken => Err("Smart Copy preparation completion lost".into()),
        };
        let result = if slot.cancelled {
            // Even a completion racing cancellation must not publish a session.
            drop(result);
            Err("Smart Copy preparation cancelled".into())
        } else {
            result
        };
        Some(CaptureCompletion {
            pane_id: slot.pane_id,
            identity: slot.identity,
            generation: slot.generation,
            result,
        })
    }
    pub(crate) fn request(
        &mut self,
        view: &PaintedTerminal,
        request: ContextRequest,
    ) -> Result<(), String> {
        let ContextRequest {
            key,
            source_row,
            column,
            selection,
            cwd,
            home,
            history_context,
        } = request;
        self.cancel();
        let cost = view.preparation_cost()?;
        let reservation = self
            .client
            .try_reserve(Lane::Cpu, cost)
            .map_err(|reason| format!("Context text preparation admission: {reason:?}"))?;
        let job = ContextJob {
            snapshot: view.preparation_snapshot()?,
            source_row,
            column,
            selection,
        };
        let receipt = reservation
            .submit(job)
            .map_err(|rejected| format!("Context text preparation: {:?}", rejected.reason))?;
        self.slot = Some(Slot {
            key,
            column,
            cwd,
            home,
            history_context,
            pending: Pending::Cpu(receipt),
        });
        Ok(())
    }
    pub(crate) fn collect(&mut self) -> Option<ContextCompletion> {
        let mut slot = self.slot.take()?;
        match &mut slot.pending {
            Pending::Cpu(receipt) => match receipt.try_take() {
                JobPoll::Pending => {
                    self.slot = Some(slot);
                    return None;
                }
                JobPoll::Ready(outcome) => slot.pending = Pending::Between(outcome),
                JobPoll::Lost | JobPoll::Taken => {
                    return Some(ContextCompletion {
                        key: slot.key,
                        text: Err("Context preparation completion was lost".into()),
                    });
                }
            },
            Pending::Io(receipt) => match receipt.try_take() {
                JobPoll::Pending => {
                    self.slot = Some(slot);
                    return None;
                }
                JobPoll::Ready(outcome) => return Some(completed(slot.key, outcome)),
                JobPoll::Lost | JobPoll::Taken => {
                    return Some(ContextCompletion {
                        key: slot.key,
                        text: Err("Context resolution completion was lost".into()),
                    });
                }
            },
            Pending::Between(_) => {}
        }
        let Pending::Between(outcome) = slot.pending else {
            self.slot = Some(slot);
            return None;
        };
        if !matches!(outcome.view(), JobOutcome::Finished(Ok(_))) {
            return Some(completed(slot.key, outcome));
        }
        // The CPU reservation remains inside the next output until the menu
        // releases its captured text. Only resolver metadata needs a new debit.
        let cost = JobCost {
            input_bytes: 8 * 1024 * 1024,
            result_bytes: 128 * 1024,
        };
        let reservation = match self.client.try_reserve(Lane::Io, cost) {
            Ok(value) => value,
            Err(
                RejectReason::Closed | RejectReason::InvalidCost | RejectReason::AccountingPoisoned,
            ) => {
                return Some(ContextCompletion {
                    key: slot.key,
                    text: Err("Context resolver unavailable".into()),
                });
            }
            Err(_) => {
                slot.pending = Pending::Between(outcome);
                self.slot = Some(slot);
                return None;
            }
        };
        let mut text = None;
        let source_hold = outcome.map(|outcome| {
            if let JobOutcome::Finished(Ok(value)) = outcome {
                text = Some(value);
            }
        });
        let Some(mut text) = text else {
            return Some(ContextCompletion {
                key: slot.key,
                text: Err("Context source unavailable".into()),
            });
        };
        text.source_hold = Some(source_hold);
        let job = ResolveJob {
            text,
            column: slot.column,
            cwd: slot.cwd.clone(),
            home: slot.home.clone(),
            history_context: slot.history_context.clone(),
        };
        match reservation.submit(job) {
            Ok(receipt) => {
                slot.pending = Pending::Io(receipt);
                self.slot = Some(slot);
                None
            }
            Err(rejected) => Some(ContextCompletion {
                key: slot.key,
                text: Err(format!(
                    "Context resolver submission rejected: {:?}",
                    rejected.reason
                )),
            }),
        }
    }
    pub(crate) fn cancel(&mut self) {
        if let Some(slot) = self.slot.take() {
            match slot.pending {
                Pending::Cpu(receipt) => receipt.cancel(),
                Pending::Io(receipt) => receipt.cancel(),
                Pending::Between(_) => {}
            }
        }
    }
}
impl Drop for TerminalContextPreparation {
    fn drop(&mut self) {
        for slot in self.links.drain(..) {
            slot.receipt.cancel();
        }
        self.cancel_capture();
        self.cancel();
    }
}

fn completed<J: Job<Output = ContextText, Error = String>>(
    key: ContextKey,
    outcome: Retained<JobOutcome<J>>,
) -> ContextCompletion {
    let error = match outcome.view() {
        JobOutcome::Finished(Ok(_)) => None,
        JobOutcome::Finished(Err(error)) => Some(error.clone()),
        JobOutcome::NotStarted { .. } => {
            Some("Context preparation cancelled before starting".into())
        }
        JobOutcome::Panicked => Some("Context preparation panicked".into()),
    };
    let text = if let Some(error) = error {
        Err(error)
    } else {
        Ok(outcome.map(|outcome| match outcome {
            JobOutcome::Finished(Ok(text)) => text,
            // The immutable discriminant was inspected immediately above.
            _ => unreachable!("successful context outcome changed during owned move"),
        }))
    };
    ContextCompletion { key, text }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_view::TerminalView;
    use std::time::{Duration, Instant};
    pub(super) struct CaptureDropProbe {
        client: Client,
        observed: std::sync::mpsc::Sender<(std::thread::ThreadId, usize, usize)>,
    }
    impl Drop for CaptureDropProbe {
        fn drop(&mut self) {
            let usage = self.client.usage();
            let _ = self.observed.send((
                std::thread::current().id(),
                usage.input_bytes,
                usage.result_bytes,
            ));
        }
    }
    fn ready_capture(client: Client) -> Retained<PreparedCapture> {
        let mut owner = TerminalContextPreparation::new(client);
        let mut view = TerminalView::new(4, 40);
        view.feed(b"original emitted retirement words");
        let source = view.painted_source();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match owner.request_capture(
                NodeId(91),
                7,
                &source,
                crate::smart_copy_tokens::PathContext::default(),
            ) {
                Ok(()) => break,
                Err(error) if error.ends_with("Busy") && Instant::now() < deadline => {
                    std::thread::yield_now()
                }
                Err(error) => panic!("capture fixture admission: {error}"),
            }
        }
        loop {
            if let Some(completion) = owner.collect_capture() {
                return completion.result.expect("prepared capture");
            }
            assert!(Instant::now() < deadline, "capture completion deadline");
            std::thread::yield_now();
        }
    }
    #[test]
    fn discarded_capture_retires_on_cpu_with_full_original_debit() {
        let client = crate::execution::test_document_client();
        let baseline = client.usage();
        let output = ready_capture(client.clone());
        let (mut prepared, outer_hold) = output.into_parts();
        let (sent, received) = std::sync::mpsc::channel();
        prepared.payload.as_mut().expect("payload").retirement_probe = Some(CaptureDropProbe {
            client: client.clone(),
            observed: sent,
        });
        assert!(
            prepared.session.return_retirement.is_none(),
            "discard must not recursively retire a session"
        );
        // Model a stale/cancelled completion after every receipt/outer owner has
        // gone. The producing job already installed the original debit inside.
        drop(outer_hold);
        drop(prepared);
        let (thread, input_bytes, result_bytes) = received
            .recv_timeout(Duration::from_secs(5))
            .expect("physical payload retirement");
        assert_ne!(thread, std::thread::current().id());
        assert!(
            input_bytes > baseline.input_bytes,
            "original input debit outlives payload"
        );
        assert!(
            result_bytes > baseline.result_bytes,
            "original result debit outlives payload"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while client.usage().input_bytes != baseline.input_bytes
            || client.usage().result_bytes != baseline.result_bytes
        {
            assert!(
                Instant::now() < deadline,
                "retirement releases original debit after payload"
            );
            std::thread::yield_now();
        }
    }
    #[test]
    fn published_capture_moves_same_debit_and_preadmitted_session_custody() {
        let client = crate::execution::test_document_client();
        let output = ready_capture(client.clone());
        let (prepared, outer_hold) = output.into_parts();
        let before = client.usage();
        let payload = prepared.into_payload();
        drop(outer_hold);
        assert!(payload.session.return_retirement.is_some());
        assert!(payload.session.capture_retention.is_some());
        assert!(payload.session.output_retention.is_none());
        assert_eq!(payload.session.generation, 7);
        assert_eq!(payload.session.pane_id, NodeId(91));
        assert!(payload.prompt.contains("original emitted retirement words"));
        assert!(payload
            .session
            .snapshot
            .screen
            .contents()
            .contains("original emitted retirement words"));
        let after = client.usage();
        assert_eq!(after.input_bytes, before.input_bytes);
        assert_eq!(after.result_bytes, before.result_bytes);
        // The existing session Drop now carries candidates, snapshot, geometry
        // and the original debit onto the preadmitted physical retirement path.
        drop(payload);
    }
    // Fixtures share one real execution bank. Busy is its nonblocking quota-lock
    // contention, not a failed snapshot. Admit before mutating the captured view;
    // structural/limit errors remain fatal and the deadline bounds setup.
    fn request_when_admitted(
        preparation: &mut TerminalContextPreparation,
        view: &TerminalView,
        request: impl Fn() -> ContextRequest,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match preparation.request(&view.painted_source(), request()) {
                Ok(()) => return,
                Err(error)
                    if error == "Context text preparation admission: Busy"
                        && Instant::now() < deadline =>
                {
                    std::thread::yield_now();
                }
                Err(error) => panic!("context fixture admission failed: {error}"),
            }
        }
    }
    fn collect(preparation: &mut TerminalContextPreparation) -> ContextCompletion {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = preparation.collect() {
                return result;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn ctrl_link_cpu_job_preserves_original_source_path_offsets_and_no_link() {
        let mut owner = TerminalContextPreparation::new(crate::execution::test_client());
        let mut view = TerminalView::new(4, 80);
        view.feed(b"./original.rs:12:7 plain words");
        assert_eq!(b"./original.rs:12:7 plain words"[24], b' ');
        let source = view.painted_source();
        view.feed(b"\r\nhttps://newer.test");
        for (id, column) in [(81, 4), (82, 24)] {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match owner.request_link(
                    &source,
                    LinkRequest {
                        id,
                        pane_id: NodeId(71),
                        row: 0,
                        column,
                        cwd: std::path::Path::new("/captured"),
                        home: None,
                    },
                ) {
                    Ok(_) => break,
                    Err(error)
                        if error == "Terminal link CPU admission: Busy"
                            && Instant::now() < deadline =>
                    {
                        std::thread::yield_now()
                    }
                    Err(error) => panic!("link fixture admission: {error}"),
                }
            }
            let completion = loop {
                if let Some(completion) = owner.collect_link() {
                    break completion;
                }
                assert!(Instant::now() < deadline, "link completion deadline");
                std::thread::yield_now();
            };
            assert_eq!(completion.id, id);
            assert!(Arc::ptr_eq(&completion.identity, &view.identity));
            let output = completion.result.unwrap();
            let output = output.view();
            assert_ne!(output.worker_thread, std::thread::current().id());
            if id == 81 {
                assert_eq!(
                    output.link,
                    Some(crate::terminal_links::TerminalLink::File {
                        path: PathBuf::from("/captured/./original.rs"),
                        line: Some(12),
                        column: Some(7)
                    })
                );
                assert!(output.storage.is_some());
            } else {
                assert!(output.link.is_none());
            }
        }
    }

    #[test]
    fn smart_copy_preparation_uses_original_painted_source_after_new_output() {
        let client = crate::execution::test_client();
        let mut owner = TerminalContextPreparation::new(client);
        let mut view = TerminalView::new(4, 40);
        view.feed(b"original emitted words");
        let painted = view.painted_source();
        view.feed(b"\r\nnewer invisible words");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match owner.request_capture(
                NodeId(71),
                9,
                &painted,
                crate::smart_copy_tokens::PathContext::default(),
            ) {
                Ok(()) => break,
                Err(error)
                    if error == "Smart Copy preparation admission: Busy"
                        && Instant::now() < deadline =>
                {
                    std::thread::yield_now()
                }
                Err(error) => panic!("capture admission: {error}"),
            }
        }
        let completion = loop {
            if let Some(completion) = owner.collect_capture() {
                break completion;
            }
            assert!(Instant::now() < deadline, "capture completion deadline");
            std::thread::yield_now();
        };
        assert_eq!(completion.pane_id, NodeId(71));
        assert_eq!(completion.generation, 9);
        assert!(Arc::ptr_eq(&completion.identity, &view.identity));
        let result = completion.result.unwrap();
        let result = result.view();
        assert!(result
            .session
            .snapshot
            .screen
            .contents()
            .contains("original emitted words"));
        assert!(!result
            .session
            .snapshot
            .screen
            .contents()
            .contains("newer invisible words"));
        assert!(result.prompt.contains("original emitted words"));
        assert!(!result.prompt.contains("newer invisible words"));
    }

    #[test]
    fn click_snapshot_preserves_history_selection_and_osc_link_after_later_output() {
        let mut view = TerminalView::new(4, 60);
        view.feed(b"\x1b[32mold\x1b[0m\r\n\x1b]8;;https://example.test\x1b\\link\x1b]8;;\x1b\\");
        let mut preparation = TerminalContextPreparation::new(crate::execution::test_client());
        let identity = view.identity.clone();
        request_when_admitted(&mut preparation, &view, || ContextRequest {
            key: ContextKey {
                pane_id: NodeId(1),
                identity: identity.clone(),
                generation: 7,
                column: 0,
                row: 0,
                snapshot_ordinal: 0,
            },
            source_row: 1,
            column: 1,
            selection: None,
            cwd: PathBuf::from("/"),
            home: None,
            history_context: None,
        });
        view.feed(b"\r\nnewer output");
        let completion = collect(&mut preparation);
        assert_eq!(completion.key.generation, 7);
        let text = completion.text.unwrap();
        assert!(text.view().full_history.contains("old"));
        assert!(!text.view().full_history.contains("newer output"));
        assert!(!text.view().full_history.contains('\x1b'));
        assert_eq!(text.view().source_line_text, "link");
        assert!(text.view().open_target.is_some());
        assert!(text.view().source_hold.is_some());
    }
    #[test]
    fn bounded_transcript_lookup_keeps_captured_recovery_text_available() {
        for class in [
            ilium_core::AgentClass::Codex,
            ilium_core::AgentClass::Claude,
        ] {
            let home = tempfile::tempdir().unwrap();
            let session = "11111111-1111-4111-8111-111111111111";
            let directory = match class {
                ilium_core::AgentClass::Codex => home.path().join(".codex/sessions/2026/10/03"),
                _ => home.path().join(".claude/projects/-"),
            };
            std::fs::create_dir_all(&directory).unwrap();
            let filename = match class {
                ilium_core::AgentClass::Codex => {
                    format!("rollout-2026-10-03T12-00-00-{session}.jsonl")
                }
                _ => format!("{session}.jsonl"),
            };
            let path = directory.join(filename);
            let metadata = match class {
                ilium_core::AgentClass::Codex => serde_json::json!({
                    "type": "session_meta", "payload": {"id": session, "cwd": "/"},
                    "synthetic_fixture": "x".repeat(65 * 1024)
                }),
                _ => serde_json::json!({
                    "type": "user", "sessionId": session, "cwd": "/",
                    "message": {"content": "x".repeat(65 * 1024)},
                    "synthetic_fixture": true
                }),
            };
            std::fs::write(&path, format!("{metadata}\n")).unwrap();
            let mut view = TerminalView::new(4, 60);
            view.feed("FATAL café 日本語 🦀".as_bytes());
            let mut preparation = TerminalContextPreparation::new(crate::execution::test_client());
            request_when_admitted(&mut preparation, &view, || ContextRequest {
                key: ContextKey {
                    pane_id: NodeId(1),
                    identity: view.identity.clone(),
                    generation: 1,
                    column: 0,
                    row: 0,
                    snapshot_ordinal: 0,
                },
                source_row: 0,
                column: 0,
                selection: None,
                cwd: PathBuf::from("/"),
                home: Some(home.path().to_owned()),
                history_context: Some((class.clone(), session.into(), PathBuf::from("/"), None)),
            });
            let completion = collect(&mut preparation);
            let text = completion.text.unwrap_or_else(|error| {
                    panic!("bounded transcript lookup must preserve captured recovery text for {class:?}: {error}")
                });
            assert!(
                text.view().warning.as_deref().is_some_and(
                    |warning| warning.contains("history scan exceeded its safety limit")
                ),
                "both provider fixtures must actually reach the bounded scan failure"
            );
            assert_eq!(text.view().source_line_text, "FATAL café 日本語 🦀");
            assert!(text
                .view()
                .visible_contents
                .contains("FATAL café 日本語 🦀"));
            assert!(text.view().full_history.contains("FATAL café 日本語 🦀"));
            assert!(
                text.view().history_path.is_none(),
                "partial scan must never authorize a transcript path"
            );
            assert!(
                text.view().source_hold.is_some(),
                "captured text keeps its execution credit"
            );
            assert_eq!(
                std::fs::read_to_string(path).unwrap(),
                format!("{metadata}\n")
            );
        }
    }

    #[test]
    fn cancellation_releases_captured_generation_without_publication() {
        let view = TerminalView::new(3, 20);
        let mut preparation = TerminalContextPreparation::new(crate::execution::test_client());
        request_when_admitted(&mut preparation, &view, || ContextRequest {
            key: ContextKey {
                pane_id: NodeId(1),
                identity: view.identity.clone(),
                generation: 1,
                column: 0,
                row: 0,
                snapshot_ordinal: 0,
            },
            source_row: 0,
            column: 0,
            selection: None,
            cwd: PathBuf::from("/"),
            home: None,
            history_context: None,
        });
        preparation.cancel();
        assert!(preparation.collect().is_none());
        assert!(preparation.slot.is_none());
    }
}

#[cfg(test)]
mod queued_capture_custody_tests {
    use super::*;
    use crate::terminal_view::TerminalView;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    #[derive(Debug)]
    struct Gate {
        started: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Job for Gate {
        type Output = ();
        type Error = String;
        fn run(self, _: JobContext) -> Result<(), String> {
            self.started.send(()).map_err(|error| error.to_string())?;
            self.release.recv().map_err(|error| error.to_string())
        }
    }
    struct Release(Option<mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(sent) = self.0.take() {
                let _ = sent.send(());
            }
        }
    }
    #[test]
    fn cancelled_queued_capture_keeps_physical_slot_until_worker_observes_it() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 4,
            jobs: 16,
            service_jobs: 0,
            input_bytes: 128 * 1024 * 1024,
            result_bytes: 128 * 1024 * 1024,
            worker_threads: 2,
            worker_bytes: 128 * 1024 * 1024,
        });
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 8,
            priority: None,
            resident_bytes_per_thread: 64 * 1024,
        };
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let mut bank = Execution::start(
            quota,
            ExecutionConfig {
                cpu: lane,
                io: lane,
                service: disabled,
            },
        )
        .expect("isolated bank");
        let client = bank
            .client(ClientLimits {
                jobs: 16,
                service_jobs: 0,
                input_bytes: 128 * 1024 * 1024,
                result_bytes: 128 * 1024 * 1024,
            })
            .expect("tenant");
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let release = Release(Some(release));
        let mut gate = client
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes: 1024,
                    result_bytes: 1024,
                },
            )
            .expect("gate admission")
            .submit(Gate {
                started,
                release: wait,
            })
            .expect("gate submission");
        ready
            .recv_timeout(Duration::from_secs(5))
            .expect("sole I/O worker occupied");
        let mut owner = TerminalContextPreparation::new(client.clone());
        let mut view = TerminalView::new(4, 40);
        view.feed(b"original immutable words");
        let painted = view.painted_source();
        owner
            .request_capture(
                NodeId(77),
                12,
                &painted,
                crate::smart_copy_tokens::PathContext::default(),
            )
            .expect("queued capture");
        owner.cancel_capture();
        let held = client.usage();
        for generation in 13..33 {
            assert!(
                owner.collect_capture().is_none(),
                "queued job has not physically completed"
            );
            assert!(owner
                .request_capture(
                    NodeId(77),
                    generation,
                    &painted,
                    crate::smart_copy_tokens::PathContext::default()
                )
                .expect_err("no replacement before physical completion")
                .contains("still settling"));
            let usage = client.usage();
            assert_eq!(
                (usage.jobs, usage.input_bytes, usage.result_bytes),
                (held.jobs, held.input_bytes, held.result_bytes)
            );
        }
        drop(release);
        let deadline = Instant::now() + Duration::from_secs(5);
        let completion = loop {
            if let Some(done) = owner.collect_capture() {
                break done;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert!(
            completion.result.is_err(),
            "cancelled generation never publishes"
        );
        loop {
            match gate.try_take() {
                JobPoll::Ready(done) => {
                    drop(done);
                    break;
                }
                JobPoll::Pending => {}
                _ => panic!("gate result lost"),
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        // A taken receipt still owns its original debit until it is dropped.
        // Release the fixture's gate before asserting that capture credit settled.
        drop(gate);
        while client.usage().jobs != 0 {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        owner
            .request_capture(
                NodeId(77),
                33,
                &painted,
                crate::smart_copy_tokens::PathContext::default(),
            )
            .expect("retry after physical completion");
        let completion = loop {
            if let Some(done) = owner.collect_capture() {
                break done;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(completion.generation, 33);
        assert!(completion.result.is_ok());
        drop(completion);
        drop(owner);
        drop(painted);
        drop(view);
        drop(client);
        bank.request_shutdown(ShutdownMode::Drain);
        bank.join_until_background(Instant::now() + Duration::from_secs(5))
            .expect("owned bank retires");
    }
}
