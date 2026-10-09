//! Each board has one ordered durability owner; finite reads and writes share
//! the client's I/O bank. UI handles contain no file descriptors.
use super::ordered::{OrderedWriter, WriteCompletion};
use crate::board::{BoardColumn, BoardRollback, BoardSource, BoardWriteFailure, StorageAction};
use ilium_core::{BoardStorage, NodeId};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const COST: JobCost = JobCost {
    input_bytes: 16 * 1024 * 1024,
    result_bytes: 2 * 1024 * 1024,
};
struct DiskState {
    revision: Option<String>,
    failed: bool,
}
pub(crate) struct BoardWrite {
    storage: BoardStorage,
    columns: Vec<BoardColumn>,
    action: Option<StorageAction>,
    revision: u64,
    state: Arc<Mutex<DiskState>>,
}
impl Job for BoardWrite {
    type Output = u64;
    type Error = BoardWriteFailure;
    fn run(self, _context: JobContext) -> Result<u64, BoardWriteFailure> {
        // Only this ordered writer accesses the predecessor state. The UI
        // never takes this mutex, and no registry lock covers filesystem I/O.
        let mut state = self
            .state
            .lock()
            .map_err(|_| BoardWriteFailure::uncertain("Board writer failed".to_owned()))?;
        if state.failed {
            return Err(BoardWriteFailure::uncertain(
                "Previous board write failed; authored edits remain unsaved; reload before saving"
                    .into(),
            ));
        }
        match crate::board::persist_source(
            &self.storage,
            &self.columns,
            state.revision.as_deref(),
            self.action.as_ref(),
        ) {
            Ok(revision) => {
                state.revision = revision;
                Ok(self.revision)
            }
            Err(error) => {
                state.failed = true;
                Err(error)
            }
        }
    }
}
struct BoardWriterInner {
    writer: OrderedWriter<BoardWrite>,
    state: Arc<Mutex<DiskState>>,
    identity: Arc<()>,
    revisions: Vec<(super::ordered::WriteId, u64, Option<BoardRollback>)>,
    // Last field: resident markdown revision is released before its storage debit.
    _source_hold: Option<Arc<ilium_execution::StorageAdmission>>,
}
#[derive(Clone)]
pub struct BoardWriter(Rc<RefCell<BoardWriterInner>>);
impl std::fmt::Debug for BoardWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoardWriter").finish_non_exhaustive()
    }
}
impl BoardWriter {
    fn new(client: Client, ready: Arc<tokio::sync::Notify>, source: &BoardSource) -> Self {
        Self(Rc::new(RefCell::new(BoardWriterInner {
            _source_hold: source.retention.clone(),
            writer: OrderedWriter::new(client, ready),
            state: Arc::new(Mutex::new(DiskState {
                revision: source.markdown_revision.clone(),
                failed: false,
            })),
            identity: Arc::new(()),
            revisions: Vec::new(),
        })))
    }
    pub fn identity(&self) -> Arc<()> {
        Arc::clone(&self.0.borrow().identity)
    }
    pub fn enqueue(
        &self,
        storage: &BoardStorage,
        columns: &[BoardColumn],
        revision: u64,
        action: Option<StorageAction>,
    ) -> Result<(), String> {
        crate::board::validate_retained_columns(columns)?;
        if storage_path_capacity(storage) > 64 * 1024 {
            return Err("Board path exceeds byte limit".into());
        }
        let mut inner = self.0.borrow_mut();
        let state = Arc::clone(&inner.state);
        let id = inner
            .writer
            .enqueue_with_detailed(COST, || BoardWrite {
                storage: storage.clone(),
                columns: columns.to_vec(),
                revision,
                action,
                state,
            })
            .map_err(|reason| {
                format!("Board write refused: {reason:?}; authored edit is unsaved")
            })?;
        inner.revisions.push((id, revision, None));
        Ok(())
    }
    pub(crate) fn attach_rollback(&self, rollback: BoardRollback) {
        if let Some((_, _, previous)) = self.0.borrow_mut().revisions.last_mut() {
            *previous = Some(rollback);
        }
    }
    pub fn pending(&self) -> usize {
        self.0.borrow().writer.pending()
    }
    fn poll(&self) -> Option<BoardAck> {
        let mut inner = self.0.borrow_mut();
        let completion = inner.writer.poll()?;
        let id = match &completion {
            WriteCompletion::Outcome { id, .. }
            | WriteCompletion::Rejected { id, .. }
            | WriteCompletion::Lost { id } => *id,
        };
        let metadata = inner
            .revisions
            .iter()
            .position(|(pending, _, _)| *pending == id)
            .map(|index| inner.revisions.remove(index));
        let (revision, rollback) = metadata.map_or((None, None), |(_, revision, rollback)| {
            (Some(revision), rollback)
        });
        let mut result = Err(BoardWriteFailure::uncertain(
            "Board publication is unconfirmed".into(),
        ));
        match completion {
            WriteCompletion::Outcome { outcome, .. } => {
                let _retained = outcome.map(|outcome| {
                    result = match outcome {
                        JobOutcome::Finished(result) => result,
                        _ => Err(BoardWriteFailure::uncertain(
                            "Board worker did not complete publication".into(),
                        )),
                    }
                });
            }
            WriteCompletion::Rejected { rejection, .. } => {
                result = Err(BoardWriteFailure {
                    message: format!("Board command did not run: {:?}", rejection.reason),
                    unchanged: true,
                })
            }
            WriteCompletion::Lost { .. } => {}
        }
        Some(BoardAck {
            rollback,
            revision,
            identity: Arc::clone(&inner.identity),
            result,
        })
    }
    fn close_admission(&self) {
        self.0.borrow_mut().writer.close_admission();
    }
}
pub struct BoardAck {
    pub rollback: Option<BoardRollback>,
    pub revision: Option<u64>,
    pub identity: Arc<()>,
    pub result: Result<u64, BoardWriteFailure>,
}
pub(crate) struct BoardRead {
    pub storage: BoardStorage,
    pub create_missing: bool,
    source_hold: Arc<ilium_execution::StorageAdmission>,
    capture_hold: Option<Arc<ilium_execution::StorageAdmission>>,
}
impl Job for BoardRead {
    type Output = BoardSource;
    type Error = String;
    fn run(self, context: JobContext) -> Result<BoardSource, String> {
        if context.stop_requested() {
            return Err("Board read cancelled".into());
        }
        crate::board::read_source(self.storage, self.create_missing).map(|mut source| {
            source.capture_hold = self.capture_hold;
            source.retention = Some(self.source_hold);
            source
        })
    }
}
#[derive(Clone)]
pub(crate) enum BoardLoadTarget {
    Pane(NodeId, BoardStorage),
    Create(crate::app::CreateBoardState, BoardStorage),
    Open {
        parent_group: NodeId,
        storage: BoardStorage,
    },
}
pub(crate) struct BoardLoaded {
    pub target: BoardLoadTarget,
    pub result: Result<BoardSource, String>,
}
pub(crate) struct BoardDefaultRead {
    directory: std::path::PathBuf,
    occupied: Vec<std::path::PathBuf>,
}
impl Job for BoardDefaultRead {
    type Output = std::path::PathBuf;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, String> {
        for suffix in 1..=4096 {
            if context.stop_requested() {
                return Err("Board suggestion cancelled".into());
            }
            let candidate = self.directory.join(if suffix == 1 {
                "board.md".into()
            } else {
                format!("board-{suffix}.md")
            });
            if !candidate.exists() && !self.occupied.contains(&candidate) {
                return Ok(candidate);
            }
        }
        Err("Board name suggestion limit reached; enter an explicit path".into())
    }
}
pub struct BoardFiles {
    storage_quota: ilium_execution::QuotaGroup,
    client: Client,
    ready: Arc<tokio::sync::Notify>,
    reads: Vec<(BoardLoadTarget, Receipt<BoardRead>)>,
    desired_reads: std::collections::VecDeque<(BoardLoadTarget, BoardRead)>,
    // A completed semantic open/create keeps its original source/capture
    // reservations until outbound admission accepts its NewBoard request.
    pending_handoff: Option<BoardLoaded>,
    writers: Vec<BoardWriter>,
    closing: bool,
    suggestion: Option<(NodeId, std::path::PathBuf, Receipt<BoardDefaultRead>)>,
}
impl BoardFiles {
    pub fn new(client: Client, ready: Arc<tokio::sync::Notify>) -> Self {
        let wake = Arc::clone(&ready);
        Self {
            storage_quota: crate::execution::process_quota(),
            client: client.with_completion_wake(move || wake.notify_one()),
            ready,
            reads: Vec::new(),
            desired_reads: std::collections::VecDeque::new(),
            pending_handoff: None,
            writers: Vec::new(),
            closing: false,
            suggestion: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn set_storage_quota(&mut self, quota: ilium_execution::QuotaGroup) {
        self.storage_quota = quota;
    }
    pub fn pending(&self) -> usize {
        self.reads.len()
            + self.desired_reads.len()
            + usize::from(self.pending_handoff.is_some())
            + usize::from(self.suggestion.is_some())
            + self.writers.iter().map(BoardWriter::pending).sum::<usize>()
    }
    pub(crate) fn notification(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.ready)
    }
    pub(crate) fn is_closing(&self) -> bool {
        self.closing
    }
    pub fn suggest(
        &mut self,
        parent: NodeId,
        initial: std::path::PathBuf,
        directory: std::path::PathBuf,
        occupied: Vec<std::path::PathBuf>,
    ) -> Result<(), String> {
        if self.suggestion.is_some() {
            return Err("Board name suggestion busy; enter an explicit path".into());
        }
        if directory.capacity() > 64 * 1024
            || occupied.len() > 32
            || occupied.iter().map(|path| path.capacity()).sum::<usize>() > 256 * 1024
        {
            return Err("Board suggestion exceeds byte limit".into());
        }
        let receipt = self
            .client
            .try_submit(
                Lane::Io,
                JobCost {
                    input_bytes: 1024 * 1024,
                    result_bytes: 128 * 1024,
                },
                BoardDefaultRead {
                    directory,
                    occupied,
                },
            )
            .map_err(|rejected| format!("Board suggestion admission: {:?}", rejected.reason))?;
        self.suggestion = Some((parent, initial, receipt));
        Ok(())
    }
    pub fn poll_suggestion(
        &mut self,
    ) -> Option<(
        NodeId,
        std::path::PathBuf,
        Result<std::path::PathBuf, String>,
    )> {
        let (parent, initial, receipt) = self.suggestion.as_mut()?;
        let mut result = None;
        match receipt.try_take() {
            JobPoll::Pending => return None,
            JobPoll::Ready(outcome) => {
                let _held = outcome.map(|outcome| {
                    result = Some(match outcome {
                        JobOutcome::Finished(result) => result,
                        _ => Err("Board suggestion did not complete".into()),
                    })
                });
            }
            JobPoll::Lost | JobPoll::Taken => {
                result = Some(Err("Board suggestion receipt lost".into()))
            }
        }
        let parent = *parent;
        let initial = initial.clone();
        self.suggestion = None;
        result.map(|result| (parent, initial, result))
    }
    pub fn attach(&mut self, source: BoardSource) -> Result<crate::board::BoardPane, String> {
        self.writers
            .retain(|writer| Rc::strong_count(&writer.0) > 1 || writer.pending() != 0);
        if self.writers.len() >= 32 {
            return Err("Board writer limit reached".into());
        }
        let writer = BoardWriter::new(self.client.clone(), Arc::clone(&self.ready), &source);
        self.writers.push(writer.clone());
        Ok(crate::board::BoardPane::from_source(source, writer))
    }
    /// Snapshot only bounded storage paths after independent storage admission.
    pub fn request_pane(&mut self, pane_id: NodeId, storage: &BoardStorage) -> Result<(), String> {
        if storage_path_capacity(storage) > 64 * 1024 {
            return Err("Board path exceeds retained limit".into());
        }
        if self.reads.iter().any(|(target,_)|matches!(target,BoardLoadTarget::Pane(id,current) if *id==pane_id && current==storage)) || self.desired_reads.iter().any(|(target,_)|matches!(target,BoardLoadTarget::Pane(id,current) if *id==pane_id && current==storage)) { return Ok(()); }
        let hold = self
            .storage_quota
            .reserve_external_storage(COST.result_bytes + 192 * 1024)
            .map_err(|reason| format!("Board capture pending: {reason:?}"))?;
        self.request_with_capture(
            BoardLoadTarget::Pane(pane_id, storage.clone()),
            storage.clone(),
            true,
            Some(Arc::new(hold)),
        )
    }
    pub fn request(
        &mut self,
        target: BoardLoadTarget,
        storage: BoardStorage,
        create_missing: bool,
    ) -> Result<(), String> {
        self.request_with_capture(target, storage, create_missing, None)
    }
    fn request_with_capture(
        &mut self,
        target: BoardLoadTarget,
        storage: BoardStorage,
        create_missing: bool,
        capture: Option<Arc<ilium_execution::StorageAdmission>>,
    ) -> Result<(), String> {
        if self.closing {
            return Err("Board file admission closed".into());
        }
        if storage_path_capacity(&storage) > 64 * 1024 || !target_is_bounded(&target) {
            return Err("Board path/dialog exceeds retained limit".into());
        }
        if self
            .pending_handoff
            .as_ref()
            .is_some_and(|loaded| same_read_target(&loaded.target, &target))
            || self
                .reads
                .iter()
                .any(|(current, _)| same_read_target(current, &target))
            || self
                .desired_reads
                .iter()
                .any(|(current, _)| same_read_target(current, &target))
        {
            return Ok(());
        }
        let replaces_existing = matches!(&target, BoardLoadTarget::Pane(pane, _) if self.desired_reads.iter().any(|(current,_)| matches!(current,BoardLoadTarget::Pane(id,_) if id==pane)));
        if self.desired_reads.len() >= 16 && !replaces_existing {
            return Err("Board read intent limit reached; retry opening the board".into());
        }
        let target_bytes = match &target {
            BoardLoadTarget::Pane(_, storage) | BoardLoadTarget::Open { storage, .. } => {
                storage_path_capacity(storage)
            }
            BoardLoadTarget::Create(state, storage) => storage_path_capacity(storage)
                .saturating_add(state.path.buf.capacity())
                .saturating_add(state.name.buf.capacity()),
        };
        let capture_bytes = target_bytes
            .saturating_add(storage_path_capacity(&storage))
            .saturating_add(4096);
        // One atomic resident admission covers source and capture together.
        // Failed source admission must not release a separate capture debit
        // and repeatedly wake the same original intent into another retry.
        let source_hold = match capture {
            Some(hold) => hold,
            None => Arc::new(
                self.storage_quota
                    .reserve_external_storage(COST.result_bytes.saturating_add(capture_bytes))
                    .map_err(|reason| format!("Board source/capture pending: {reason:?}"))?,
            ),
        };
        let capture_hold = Some(Arc::clone(&source_hold));
        // All admissions succeeded before replacing a preparation intent.
        // Semantic create/open intents remain ordered.
        if let BoardLoadTarget::Pane(pane, _) = &target {
            self.desired_reads
                .retain(|(current, _)| !matches!(current,BoardLoadTarget::Pane(id,_) if id==pane));
        }
        self.desired_reads.push_back((
            target,
            BoardRead {
                storage,
                create_missing,
                source_hold,
                capture_hold,
            },
        ));
        self.ready.notify_one();
        Ok(())
    }
    pub fn poll_read(&mut self) -> Option<BoardLoaded> {
        if let Some(loaded) = self.pending_handoff.take() {
            return Some(loaded);
        }
        while self.reads.len() < 4 {
            let Some((target, job)) = self.desired_reads.pop_front() else {
                break;
            };
            match self.client.try_submit(Lane::Io, COST, job) {
                Ok(receipt) => self.reads.push((target, receipt)),
                Err(rejected)
                    if matches!(
                        rejected.reason,
                        RejectReason::Busy
                            | RejectReason::QueueFull
                            | RejectReason::JobLimit
                            | RejectReason::InputBytes
                            | RejectReason::ResultBytes
                    ) =>
                {
                    self.desired_reads.push_front((target, rejected.value));
                    break;
                }
                Err(rejected) => {
                    return Some(BoardLoaded {
                        target,
                        result: Err(format!("Board read not admitted: {:?}", rejected.reason)),
                    });
                }
            }
        }
        for index in 0..self.reads.len() {
            let mut result = None;
            match self.reads[index].1.try_take() {
                JobPoll::Pending => continue,
                JobPoll::Ready(outcome) => {
                    let (outcome, retention) = outcome.into_parts();
                    let _retained = retention.retain(outcome).map(|outcome| {
                        result = Some(match outcome {
                            JobOutcome::Finished(result) => result,
                            _ => Err("Board read did not complete".into()),
                        })
                    });
                }
                JobPoll::Lost | JobPoll::Taken => {
                    result = Some(Err("Board read receipt lost".into()))
                }
            }
            let (target, _) = self.reads.remove(index);
            return result.map(|result| BoardLoaded { target, result });
        }
        None
    }
    pub(crate) fn retain_handoff(&mut self, loaded: BoardLoaded) {
        // The collector stops reading immediately after retaining this head.
        // Existing receipt and intent bounds therefore also bound this bridge.
        debug_assert!(self.pending_handoff.is_none());
        self.pending_handoff = Some(loaded);
    }
    pub fn poll_write(&self) -> Option<BoardAck> {
        self.writers.iter().find_map(BoardWriter::poll)
    }
    pub fn close_admission(&mut self) {
        self.closing = true;
        if let Some((_, _, receipt)) = &self.suggestion {
            receipt.cancel();
        }
        for writer in &self.writers {
            writer.close_admission();
        }
    }
}

pub(super) fn storage_path_capacity(storage: &BoardStorage) -> usize {
    match storage {
        BoardStorage::Folder { path } | BoardStorage::MarkdownFile { path } => path.capacity(),
    }
}

fn same_read_target(first: &BoardLoadTarget, second: &BoardLoadTarget) -> bool {
    match (first, second) {
        (BoardLoadTarget::Pane(a, storage_a), BoardLoadTarget::Pane(b, storage_b)) => {
            a == b && storage_a == storage_b
        }
        (BoardLoadTarget::Create(a, storage_a), BoardLoadTarget::Create(b, storage_b)) => {
            a.parent_group == b.parent_group
                && a.name.buf == b.name.buf
                && a.path.buf == b.path.buf
                && a.storage_kind == b.storage_kind
                && storage_a == storage_b
        }
        (
            BoardLoadTarget::Open {
                parent_group: a,
                storage: storage_a,
            },
            BoardLoadTarget::Open {
                parent_group: b,
                storage: storage_b,
            },
        ) => a == b && storage_a == storage_b,
        _ => false,
    }
}
fn target_is_bounded(target: &BoardLoadTarget) -> bool {
    match target {
        BoardLoadTarget::Pane(_, storage) | BoardLoadTarget::Open { storage, .. } => {
            storage_path_capacity(storage) <= 64 * 1024
        }
        BoardLoadTarget::Create(state, storage) => {
            storage_path_capacity(storage) <= 64 * 1024
                && state.path.buf.capacity() <= 64 * 1024
                && state.name.buf.capacity() <= 8192
        }
    }
}
