use super::editor::{EditorRead, EditorWrite, EditorWriteSlot};
use super::editor_snapshot::{
    capture_cost, writer_cost, CaptureEditorSave, MeasureEditorSave, RetireEditorPane,
};
use super::ordered::{OrderedWriter, WriteCompletion, WriteId};
use ilium_core::NodeId;
use ilium_execution::{Client, JobOutcome, JobPoll, Lane, Receipt, Retained};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

const MAX_LOADS: usize = 4;
const MAX_EDITOR_RETIREMENTS: usize = 16;
const MAX_FROZEN_SCREEN_LOADS: usize = 2;
const MAX_FROZEN_SCREEN_SAVES: usize = 2;
const MAX_FROZEN_SCREEN_PATH_BYTES: usize = 64 * 1024;
const FROZEN_SCREEN_BYTES: usize = crate::terminal_parsing::MAX_STATE_BYTES;
const FROZEN_SCREEN_WRITE_RETAINED_BYTES: usize = FROZEN_SCREEN_BYTES + 1024 * 1024;
const FROZEN_SCREEN_IO_COST: ilium_execution::JobCost = ilium_execution::JobCost {
    input_bytes: MAX_FROZEN_SCREEN_PATH_BYTES + 4096,
    result_bytes: FROZEN_SCREEN_BYTES,
};
const FROZEN_SCREEN_PARSE_COST: ilium_execution::JobCost = ilium_execution::JobCost {
    input_bytes: FROZEN_SCREEN_BYTES,
    result_bytes: FROZEN_SCREEN_BYTES,
};
const FROZEN_SCREEN_SERIALIZE_COST: ilium_execution::JobCost = ilium_execution::JobCost {
    input_bytes: FROZEN_SCREEN_BYTES * 2,
    result_bytes: FROZEN_SCREEN_BYTES,
};
const FROZEN_SCREEN_WRITE_MAX_COST: ilium_execution::JobCost = ilium_execution::JobCost {
    input_bytes: FROZEN_SCREEN_BYTES + MAX_FROZEN_SCREEN_PATH_BYTES + 4096,
    result_bytes: MAX_FROZEN_SCREEN_PATH_BYTES + 256 * 1024,
};

#[derive(Clone)]
pub struct FrozenScreenTarget {
    pub pane_id: NodeId,
    pub identity: Arc<()>,
}

struct PendingFrozenScreenRead {
    target: FrozenScreenTarget,
    receipt: Receipt<ReadFrozenScreen>,
}

struct PendingFrozenScreenParse {
    target: FrozenScreenTarget,
    receipt: Receipt<ParseFrozenScreen>,
}

struct PendingFrozenScreenSave {
    receipt: Receipt<SerializeFrozenScreen>,
    write_slot: EditorWriteSlot,
}

struct FrozenScreenSerialization {
    bytes: Vec<u8>,
    thread: std::thread::ThreadId,
}

struct SerializeFrozenScreen {
    screen: crate::terminal_view::PaintedTerminal,
}

impl ilium_execution::Job for SerializeFrozenScreen {
    type Output = FrozenScreenSerialization;
    type Error = String;

    fn run(self, context: ilium_execution::JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Err("frozen screen serialization cancelled".into());
        }
        let bytes = self.screen.frozen_bytes();
        if bytes.capacity() > FROZEN_SCREEN_BYTES {
            return Err("frozen screen exceeds its retained byte limit".into());
        }
        Ok(FrozenScreenSerialization {
            bytes,
            thread: std::thread::current().id(),
        })
    }
}

struct ReadFrozenScreen {
    path: PathBuf,
}
impl ilium_execution::Job for ReadFrozenScreen {
    type Output = Vec<u8>;
    type Error = String;

    fn run(self, context: ilium_execution::JobContext) -> Result<Self::Output, Self::Error> {
        use std::io::Read;

        if context.stop_requested() {
            return Err("frozen screen read cancelled".into());
        }
        let metadata = std::fs::symlink_metadata(&self.path)
            .map_err(|error| format!("frozen screen metadata: {error}"))?;
        if !metadata.file_type().is_file() {
            return Err("frozen screen is not a regular file".into());
        }
        if metadata.len() > FROZEN_SCREEN_BYTES as u64 {
            return Err("frozen screen exceeds its retained byte limit".into());
        }
        let file = ilium_platform::secure_fs::open_regular_file(&self.path)
            .map_err(|error| format!("frozen screen open: {error}"))?;
        let opened_metadata = file
            .metadata()
            .map_err(|error| format!("frozen screen handle metadata: {error}"))?;
        if !opened_metadata.is_file() || opened_metadata.len() > FROZEN_SCREEN_BYTES as u64 {
            return Err("frozen screen changed to an unsupported or oversized file".into());
        }
        let mut file = file;
        let mut bytes = Vec::with_capacity(opened_metadata.len() as usize);
        let mut chunk = [0_u8; 16 * 1024];
        loop {
            if context.stop_requested() {
                return Err("frozen screen read cancelled".into());
            }
            let remaining = FROZEN_SCREEN_BYTES.saturating_sub(bytes.len());
            let read_limit = chunk.len().min(remaining.saturating_add(1));
            let count = file
                .read(&mut chunk[..read_limit])
                .map_err(|error| format!("frozen screen read: {error}"))?;
            if count == 0 {
                break;
            }
            if count > remaining {
                return Err("frozen screen grew beyond its retained byte limit".into());
            }
            bytes
                .try_reserve_exact(count)
                .map_err(|error| format!("frozen screen allocation: {error}"))?;
            bytes.extend_from_slice(&chunk[..count]);
        }
        Ok(bytes)
    }
}

struct ParseFrozenScreen {
    bytes: Vec<u8>,
    _read_retention: ilium_execution::Retention,
}
impl ilium_execution::Job for ParseFrozenScreen {
    type Output = crate::terminal_view::PaintedTerminal;
    type Error = String;

    fn run(self, context: ilium_execution::JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Err("frozen screen parse cancelled".into());
        }
        crate::terminal_view::PaintedTerminal::from_frozen_bytes(&self.bytes)
    }
}

#[derive(Clone)]
pub struct LoadTarget {
    pub pane_id: NodeId,
    pub path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
    // Retain preflight admission when the finite bank is temporarily full.
    pub source_hold: Option<Arc<ilium_execution::StorageAdmission>>,
}
struct PendingLoad {
    target: LoadTarget,
    receipt: Receipt<EditorRead>,
    current: bool,
}
#[derive(Clone, Copy)]
pub enum SavePurpose {
    Explicit,
    Autosave,
    SaveAs,
    OpenAsBoard,
}
#[derive(Clone)]
pub struct SaveTarget {
    pub pane_id: NodeId,
    pub identity: Arc<()>,
    pub revision: u64,
    pub old_path: Option<PathBuf>,
    pub purpose: SavePurpose,
    pub operation: Arc<()>,
    pub prompt_input: Option<String>,
}
pub enum EditorCompletion {
    FrozenScreenSaved {
        target: FrozenScreenTarget,
        completion: WriteCompletion<EditorWrite>,
    },
    FrozenScreenLoaded {
        target: FrozenScreenTarget,
        screen: Retained<crate::terminal_view::PaintedTerminal>,
    },
    FrozenScreenLoadFailed {
        target: FrozenScreenTarget,
        message: String,
    },
    Loaded {
        target: LoadTarget,
        outcome: Retained<JobOutcome<EditorRead>>,
        current: bool,
    },
    LoadLost {
        target: LoadTarget,
        current: bool,
    },
    Saved {
        target: SaveTarget,
        completion: WriteCompletion<EditorWrite>,
    },
    SaveModelReturned {
        target: SaveTarget,
        pane: Box<crate::editor_pane::EditorPane>,
        result: Result<(), String>,
        cpu_thread: Option<std::thread::ThreadId>,
    },
    SaveModelLost {
        target: SaveTarget,
        message: String,
    },
    UnmatchedWrite(WriteCompletion<EditorWrite>),
}

pub struct SaveAdmissionFailure {
    pub pane: Box<crate::editor_pane::EditorPane>,
    pub message: String,
}

struct PendingMeasurement {
    target: SaveTarget,
    path: PathBuf,
    receipt: Receipt<MeasureEditorSave>,
}

struct PendingCapture {
    target: SaveTarget,
    receipt: Receipt<CaptureEditorSave>,
    write_slot: EditorWriteSlot,
}

pub struct EditorFiles {
    storage_quota: ilium_execution::QuotaGroup,
    client: Client,
    ready: Arc<tokio::sync::Notify>,
    loads: Vec<PendingLoad>,
    frozen_screen_reads: Vec<PendingFrozenScreenRead>,
    frozen_screen_parses: Vec<PendingFrozenScreenParse>,
    frozen_screen_saves: Vec<PendingFrozenScreenSave>,
    frozen_screen_write_ids: Vec<(WriteId, FrozenScreenTarget)>,
    writes: OrderedWriter<EditorWrite>,
    saves: Vec<(WriteId, SaveTarget)>,
    measurements: Vec<PendingMeasurement>,
    captures: Vec<PendingCapture>,
    retirements: VecDeque<Box<crate::editor_pane::EditorPane>>,
    retirement_receipts: Vec<Receipt<RetireEditorPane>>,
    closing: bool,
}
impl EditorFiles {
    pub fn new(client: Client) -> Self {
        let ready = Arc::new(tokio::sync::Notify::new());
        let wake = Arc::clone(&ready);
        let client = client.with_completion_wake(move || wake.notify_one());
        Self {
            storage_quota: crate::execution::process_quota(),
            writes: OrderedWriter::new_with_readiness_and_limits(
                client.clone(),
                Arc::clone(&ready),
                EditorWrite::is_ready,
                32,
                FROZEN_SCREEN_WRITE_RETAINED_BYTES,
            ),
            client,
            ready,
            loads: Vec::new(),
            frozen_screen_reads: Vec::new(),
            frozen_screen_parses: Vec::new(),
            frozen_screen_saves: Vec::new(),
            frozen_screen_write_ids: Vec::new(),
            saves: Vec::new(),
            measurements: Vec::new(),
            captures: Vec::new(),
            retirements: VecDeque::new(),
            retirement_receipts: Vec::new(),
            closing: false,
        }
    }
    #[cfg(test)]
    pub(super) fn set_storage_quota(&mut self, quota: ilium_execution::QuotaGroup) {
        self.storage_quota = quota;
    }
    pub fn notification(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.ready)
    }
    pub fn pending(&self) -> usize {
        self.loads.len()
            + self.frozen_screen_reads.len()
            + self.frozen_screen_parses.len()
            + self.measurements.len()
            + self.captures.len()
            + self.retirements.len()
            + self.retirement_receipts.len()
            + self.writes.pending()
    }

    pub fn load_frozen_screen(
        &mut self,
        target: FrozenScreenTarget,
        path: PathBuf,
    ) -> Result<(), String> {
        if self.frozen_screen_reads.len() + self.frozen_screen_parses.len()
            >= MAX_FROZEN_SCREEN_LOADS
        {
            return Err("Frozen screen load admission is full".into());
        }
        if path.capacity() > MAX_FROZEN_SCREEN_PATH_BYTES {
            return Err("Frozen screen path exceeds its retained byte limit".into());
        }
        let mut cost = FROZEN_SCREEN_IO_COST;
        cost.input_bytes = path
            .capacity()
            .checked_add(4096)
            .ok_or_else(|| "Frozen screen path cost overflow".to_owned())?;
        let reservation = self
            .client
            .try_reserve(Lane::Io, cost)
            .map_err(|reason| format!("Frozen screen read not admitted: {reason:?}"))?;
        let receipt = reservation
            .submit(ReadFrozenScreen { path })
            .map_err(|rejection| {
                format!("Frozen screen read not admitted: {:?}", rejection.reason)
            })?;
        self.frozen_screen_reads
            .push(PendingFrozenScreenRead { target, receipt });
        Ok(())
    }

    pub fn save_frozen_screen(
        &mut self,
        target: FrozenScreenTarget,
        path: PathBuf,
        screen: crate::terminal_view::PaintedTerminal,
    ) -> Result<(), String> {
        if self.closing {
            return Err("Frozen screen save admission is closed".into());
        }
        if self.frozen_screen_saves.len() >= MAX_FROZEN_SCREEN_SAVES {
            return Err("Frozen screen save admission is full".into());
        }
        if path.capacity() > MAX_FROZEN_SCREEN_PATH_BYTES {
            return Err("Frozen screen path exceeds its retained byte limit".into());
        }
        let reservation = self
            .client
            .try_reserve(Lane::Cpu, FROZEN_SCREEN_SERIALIZE_COST)
            .map_err(|reason| format!("Frozen screen serialization not admitted: {reason:?}"))?;
        let (write, write_slot) = EditorWrite::waiting(path, 0);
        let write_id = self
            .writes
            .enqueue_detailed(FROZEN_SCREEN_WRITE_MAX_COST, write)
            .map_err(|rejection| {
                format!("Frozen screen write not admitted: {:?}", rejection.failure)
            })?;
        self.frozen_screen_write_ids
            .push((write_id, target.clone()));
        match reservation.submit(SerializeFrozenScreen { screen }) {
            Ok(receipt) => self.frozen_screen_saves.push(PendingFrozenScreenSave {
                receipt,
                write_slot,
            }),
            Err(rejection) => {
                write_slot.fail(format!(
                    "Frozen screen serialization not admitted: {:?}",
                    rejection.reason
                ));
            }
        }
        Ok(())
    }
    pub fn request_load(&mut self, target: &mut LoadTarget) -> Result<(), String> {
        if self.loads.iter().any(|pending| {
            pending.current
                && pending.target.pane_id == target.pane_id
                && pending.target.path == target.path
        }) {
            return Ok(());
        }
        if self.loads.len() >= MAX_LOADS {
            return Err("Editor load admission full; retry opening this file".into());
        }
        if target.path.capacity() > 64 * 1024 {
            return Err("Editor path exceeds retained byte limit".into());
        }
        if target.source_hold.is_none() {
            target.source_hold = Some(Arc::new(
                self.storage_quota
                    .reserve_external_storage(super::editor::MAX_EDITOR_RETAINED_BYTES)
                    .map_err(|reason| format!("Editor source pending: {reason:?}"))?,
            ));
        }
        let Some(source_hold) = target.source_hold.clone() else {
            return Err("Editor source admission unavailable".into());
        };
        let cost = ilium_execution::JobCost {
            input_bytes: 128 * 1024 * 1024 + target.path.capacity(),
            result_bytes: 64 * 1024 * 1024,
        };
        let reservation = self
            .client
            .try_reserve(Lane::Io, cost)
            .map_err(|reason| format!("Editor load pending: {reason:?}"))?;
        let job = EditorRead {
            path: target.path.clone(),
            source_hold,
        };
        let receipt = reservation
            .submit(job)
            .map_err(|rejection| format!("Editor load pending: {:?}", rejection.reason))?;
        for pending in &mut self.loads {
            if pending.target.pane_id == target.pane_id {
                pending.current = false;
                pending.receipt.cancel();
            }
        }
        self.loads.push(PendingLoad {
            target: target.clone(),
            receipt,
            current: true,
        });
        Ok(())
    }
    pub fn save(
        &mut self,
        target: SaveTarget,
        path: PathBuf,
        pane: Box<crate::editor_pane::EditorPane>,
    ) -> Result<(), SaveAdmissionFailure> {
        if self.closing {
            return Err(SaveAdmissionFailure {
                pane,
                message: "Editor save admission is closed; buffer remains unsaved".into(),
            });
        }
        if path.capacity() > 64 * 1024 {
            return Err(SaveAdmissionFailure {
                pane,
                message: "Editor destination exceeds retained path limit".into(),
            });
        }
        let cost = ilium_execution::JobCost {
            input_bytes: super::editor::MAX_EDITOR_RETAINED_BYTES + path.capacity(),
            result_bytes: super::editor::MAX_EDITOR_RETAINED_BYTES,
        };
        let reservation = match self.client.try_reserve(Lane::Cpu, cost) {
            Ok(reservation) => reservation,
            Err(reason) => {
                return Err(SaveAdmissionFailure {
                    pane,
                    message: format!("Editor save preparation not admitted: {reason:?}"),
                });
            }
        };
        let receipt = match reservation.submit(MeasureEditorSave { pane }) {
            Ok(receipt) => receipt,
            Err(rejection) => {
                return Err(SaveAdmissionFailure {
                    pane: rejection.value.pane,
                    message: format!(
                        "Editor save preparation not admitted: {:?}",
                        rejection.reason
                    ),
                });
            }
        };
        self.measurements.push(PendingMeasurement {
            target,
            path,
            receipt,
        });
        Ok(())
    }
    pub fn poll(&mut self) -> Option<EditorCompletion> {
        self.poll_retirements();
        self.schedule_retirements();
        self.prepare_frozen_screen_saves();
        if let Some(completion) = self.poll_frozen_screen_reads() {
            return Some(completion);
        }
        if let Some(completion) = self.poll_frozen_screen_parses() {
            return Some(completion);
        }
        if let Some(completion) = self.poll_measurements() {
            return Some(completion);
        }
        if let Some(completion) = self.poll_captures() {
            return Some(completion);
        }
        self.close_writer_after_preparation();
        for index in 0..self.loads.len() {
            match self.loads[index].receipt.try_take() {
                JobPoll::Pending => {}
                JobPoll::Ready(outcome) => {
                    let pending = self.loads.remove(index);
                    return Some(EditorCompletion::Loaded {
                        target: pending.target,
                        outcome,
                        current: pending.current,
                    });
                }
                JobPoll::Lost | JobPoll::Taken => {
                    let pending = self.loads.remove(index);
                    return Some(EditorCompletion::LoadLost {
                        target: pending.target,
                        current: pending.current,
                    });
                }
            }
        }
        let completion = self.writes.poll()?;
        let id = match &completion {
            WriteCompletion::Outcome { id, .. }
            | WriteCompletion::Rejected { id, .. }
            | WriteCompletion::Lost { id } => *id,
        };
        if let Some(index) = self
            .frozen_screen_write_ids
            .iter()
            .position(|(saved, _)| *saved == id)
        {
            let (_, target) = self.frozen_screen_write_ids.remove(index);
            return Some(EditorCompletion::FrozenScreenSaved { target, completion });
        }
        let Some(index) = self.saves.iter().position(|(saved, _)| *saved == id) else {
            return Some(EditorCompletion::UnmatchedWrite(completion));
        };
        let (_, target) = self.saves.remove(index);
        Some(EditorCompletion::Saved { target, completion })
    }

    fn prepare_frozen_screen_saves(&mut self) {
        let mut index = 0;
        while index < self.frozen_screen_saves.len() {
            let outcome = match self.frozen_screen_saves[index].receipt.try_take() {
                JobPoll::Pending => {
                    index += 1;
                    continue;
                }
                JobPoll::Ready(outcome) => outcome,
                JobPoll::Lost | JobPoll::Taken => {
                    let pending = self.frozen_screen_saves.remove(index);
                    pending
                        .write_slot
                        .fail("Frozen screen serialization result was lost".into());
                    continue;
                }
            };
            let pending = self.frozen_screen_saves.remove(index);
            let (outcome, retention) = outcome.into_parts();
            match outcome {
                JobOutcome::Finished(Ok(serialized)) => {
                    let storage = self
                        .storage_quota
                        .reserve_external_storage(serialized.bytes.capacity());
                    match storage {
                        Ok(storage) => {
                            drop(retention);
                            if let Err(error) = pending.write_slot.prepare_frozen_screen(
                                serialized.bytes,
                                serialized.thread,
                                Arc::new(storage),
                            ) {
                                pending.write_slot.fail(error);
                            }
                        }
                        Err(reason) => pending
                            .write_slot
                            .fail(format!("Frozen screen storage not admitted: {reason:?}")),
                    }
                }
                JobOutcome::Finished(Err(error)) => pending.write_slot.fail(error),
                JobOutcome::NotStarted { .. } => pending
                    .write_slot
                    .fail("Frozen screen serialization did not start".into()),
                JobOutcome::Panicked => pending
                    .write_slot
                    .fail("Frozen screen serialization worker panicked".into()),
            }
        }
    }

    fn poll_frozen_screen_reads(&mut self) -> Option<EditorCompletion> {
        for index in 0..self.frozen_screen_reads.len() {
            let retained = match self.frozen_screen_reads[index].receipt.try_take() {
                JobPoll::Pending => continue,
                JobPoll::Ready(retained) => retained,
                JobPoll::Lost | JobPoll::Taken => {
                    let pending = self.frozen_screen_reads.remove(index);
                    return Some(EditorCompletion::FrozenScreenLoadFailed {
                        target: pending.target,
                        message: "Frozen screen read worker lost its result".into(),
                    });
                }
            };
            let pending = self.frozen_screen_reads.remove(index);
            let (outcome, read_retention) = retained.into_parts();
            let bytes = match outcome {
                JobOutcome::Finished(Ok(bytes)) => bytes,
                JobOutcome::Finished(Err(message)) => {
                    return Some(EditorCompletion::FrozenScreenLoadFailed {
                        target: pending.target,
                        message,
                    });
                }
                JobOutcome::NotStarted { .. } => {
                    return Some(EditorCompletion::FrozenScreenLoadFailed {
                        target: pending.target,
                        message: "Frozen screen read did not start".into(),
                    });
                }
                JobOutcome::Panicked => {
                    return Some(EditorCompletion::FrozenScreenLoadFailed {
                        target: pending.target,
                        message: "Frozen screen read worker failed".into(),
                    });
                }
            };
            let reservation = match self.client.try_reserve(Lane::Cpu, FROZEN_SCREEN_PARSE_COST) {
                Ok(reservation) => reservation,
                Err(reason) => {
                    return Some(EditorCompletion::FrozenScreenLoadFailed {
                        target: pending.target,
                        message: format!("Frozen screen parse not admitted: {reason:?}"),
                    });
                }
            };
            let receipt = match reservation.submit(ParseFrozenScreen {
                bytes,
                _read_retention: read_retention,
            }) {
                Ok(receipt) => receipt,
                Err(rejection) => {
                    return Some(EditorCompletion::FrozenScreenLoadFailed {
                        target: pending.target,
                        message: format!(
                            "Frozen screen parse not admitted: {:?}",
                            rejection.reason
                        ),
                    });
                }
            };
            self.frozen_screen_parses.push(PendingFrozenScreenParse {
                target: pending.target,
                receipt,
            });
            return None;
        }
        None
    }

    fn poll_frozen_screen_parses(&mut self) -> Option<EditorCompletion> {
        for index in 0..self.frozen_screen_parses.len() {
            match self.frozen_screen_parses[index].receipt.try_take() {
                JobPoll::Pending => continue,
                JobPoll::Ready(outcome) => {
                    let pending = self.frozen_screen_parses.remove(index);
                    let (outcome, retention) = outcome.into_parts();
                    return Some(match outcome {
                        JobOutcome::Finished(Ok(screen)) => EditorCompletion::FrozenScreenLoaded {
                            target: pending.target,
                            screen: retention.retain(screen),
                        },
                        JobOutcome::Finished(Err(message)) => {
                            EditorCompletion::FrozenScreenLoadFailed {
                                target: pending.target,
                                message,
                            }
                        }
                        JobOutcome::NotStarted { .. } => EditorCompletion::FrozenScreenLoadFailed {
                            target: pending.target,
                            message: "Frozen screen parsing was cancelled".into(),
                        },
                        JobOutcome::Panicked => EditorCompletion::FrozenScreenLoadFailed {
                            target: pending.target,
                            message: "Frozen screen parser failed".into(),
                        },
                    });
                }
                JobPoll::Lost | JobPoll::Taken => {
                    let pending = self.frozen_screen_parses.remove(index);
                    return Some(EditorCompletion::FrozenScreenLoadFailed {
                        target: pending.target,
                        message: "Frozen screen parser lost its result".into(),
                    });
                }
            }
        }
        None
    }

    pub fn retire_editor_model(
        &mut self,
        pane: Box<crate::editor_pane::EditorPane>,
    ) -> Result<(), Box<crate::editor_pane::EditorPane>> {
        if self.retirements.len() >= MAX_EDITOR_RETIREMENTS {
            return Err(pane);
        }
        self.retirements.push_back(pane);
        self.schedule_retirements();
        Ok(())
    }

    fn schedule_retirements(&mut self) {
        while !self.retirements.is_empty() {
            let cost = ilium_execution::JobCost {
                input_bytes: super::editor::MAX_EDITOR_RETAINED_BYTES,
                result_bytes: 1024,
            };
            let Ok(reservation) = self.client.try_reserve(Lane::Cpu, cost) else {
                return;
            };
            let pane = self
                .retirements
                .pop_front()
                .expect("queue checked nonempty");
            let job = RetireEditorPane { pane: Some(pane) };
            match reservation.submit(job) {
                Ok(receipt) => self.retirement_receipts.push(receipt),
                Err(rejection) => {
                    if let Some(pane) = rejection.value.pane {
                        self.retirements.push_front(pane);
                    }
                    return;
                }
            }
        }
    }

    fn poll_retirements(&mut self) {
        let mut index = 0;
        while index < self.retirement_receipts.len() {
            match self.retirement_receipts[index].try_take() {
                JobPoll::Pending => index += 1,
                JobPoll::Ready(outcome) => {
                    let (outcome, _retention) = outcome.into_parts();
                    if let JobOutcome::NotStarted { job, .. } = outcome {
                        if let Some(pane) = job.pane {
                            self.retirements.push_front(pane);
                        }
                    }
                    drop(self.retirement_receipts.remove(index));
                }
                JobPoll::Lost | JobPoll::Taken => {
                    drop(self.retirement_receipts.remove(index));
                }
            }
        }
    }

    fn poll_measurements(&mut self) -> Option<EditorCompletion> {
        for index in 0..self.measurements.len() {
            let outcome = match self.measurements[index].receipt.try_take() {
                JobPoll::Pending => continue,
                JobPoll::Ready(outcome) => outcome,
                JobPoll::Lost | JobPoll::Taken => {
                    let pending = self.measurements.remove(index);
                    return Some(EditorCompletion::SaveModelLost {
                        target: pending.target,
                        message: "Editor measurement worker lost its model; save state is unknown"
                            .into(),
                    });
                }
            };
            let pending = self.measurements.remove(index);
            let (outcome, _retention) = outcome.into_parts();
            let measured = match outcome {
                JobOutcome::Finished(Ok(measured)) => measured,
                JobOutcome::Finished(Err(error)) => {
                    return Some(EditorCompletion::SaveModelReturned {
                        target: pending.target,
                        pane: error.pane,
                        result: Err(error.message),
                        cpu_thread: None,
                    });
                }
                JobOutcome::NotStarted { job, .. } => {
                    return Some(EditorCompletion::SaveModelReturned {
                        target: pending.target,
                        pane: job.pane,
                        result: Err("Editor save preparation did not start".into()),
                        cpu_thread: None,
                    });
                }
                JobOutcome::Panicked => {
                    return Some(EditorCompletion::SaveModelLost {
                        target: pending.target,
                        message: "Editor measurement worker panicked; save state is unknown".into(),
                    });
                }
            };
            let capture_admission = self.client.try_reserve(
                Lane::Cpu,
                capture_cost(measured.measure, pending.path.capacity()),
            );
            let reservation = match capture_admission {
                Ok(reservation) => reservation,
                Err(reason) => {
                    return Some(EditorCompletion::SaveModelReturned {
                        target: pending.target,
                        pane: measured.pane,
                        result: Err(format!("Editor snapshot capture not admitted: {reason:?}")),
                        cpu_thread: None,
                    });
                }
            };
            if let Err(reason) = reservation.validate_job_type::<CaptureEditorSave>() {
                return Some(EditorCompletion::SaveModelReturned {
                    target: pending.target,
                    pane: measured.pane,
                    result: Err(format!("Editor snapshot capture cost invalid: {reason:?}")),
                    cpu_thread: None,
                });
            }
            let cost = writer_cost(measured.measure, pending.path.capacity());
            let mut slot = None;
            let id = match self.writes.enqueue_with_detailed(cost, || {
                let (write, prepared) =
                    EditorWrite::waiting(pending.path.clone(), pending.target.revision);
                slot = Some(prepared);
                write
            }) {
                Ok(id) => id,
                Err(failure) => {
                    return Some(EditorCompletion::SaveModelReturned {
                        target: pending.target,
                        pane: measured.pane,
                        result: Err(format!("Editor save not admitted: {:?}", failure.reason())),
                        cpu_thread: None,
                    });
                }
            };
            self.saves.push((id, pending.target.clone()));
            let write_slot = slot.expect("accepted writer slot is constructed exactly once");
            let job = CaptureEditorSave {
                pane: measured.pane,
                measure: measured.measure,
            };
            match reservation.submit(job) {
                Ok(receipt) => self.captures.push(PendingCapture {
                    target: pending.target,
                    receipt,
                    write_slot,
                }),
                Err(rejection) => {
                    let message = format!(
                        "Accepted save capture could not start: {:?}",
                        rejection.reason
                    );
                    write_slot.fail(message.clone());
                    return Some(EditorCompletion::SaveModelReturned {
                        target: pending.target,
                        pane: rejection.value.pane,
                        result: Err(message),
                        cpu_thread: None,
                    });
                }
            }
            return None;
        }
        None
    }

    fn poll_captures(&mut self) -> Option<EditorCompletion> {
        for index in 0..self.captures.len() {
            let outcome = match self.captures[index].receipt.try_take() {
                JobPoll::Pending => continue,
                JobPoll::Ready(outcome) => outcome,
                JobPoll::Lost | JobPoll::Taken => {
                    let pending = self.captures.remove(index);
                    pending.write_slot.fail(
                        "Editor capture worker lost its model; write was not prepared".into(),
                    );
                    return Some(EditorCompletion::SaveModelLost {
                        target: pending.target,
                        message: "Editor capture worker lost its model; save state is unknown"
                            .into(),
                    });
                }
            };
            let pending = self.captures.remove(index);
            let (outcome, _retention) = outcome.into_parts();
            return match outcome {
                JobOutcome::Finished(Ok(captured)) => {
                    let result = pending.write_slot.prepare(captured.lines);
                    if let Err(error) = &result {
                        pending.write_slot.fail(error.clone());
                    }
                    Some(EditorCompletion::SaveModelReturned {
                        target: pending.target,
                        pane: captured.pane,
                        result,
                        cpu_thread: Some(captured.worker_thread),
                    })
                }
                JobOutcome::Finished(Err(error)) => {
                    pending.write_slot.fail(error.message.clone());
                    Some(EditorCompletion::SaveModelReturned {
                        target: pending.target,
                        pane: error.pane,
                        result: Err(error.message),
                        cpu_thread: None,
                    })
                }
                JobOutcome::NotStarted { job, .. } => {
                    pending
                        .write_slot
                        .fail("Editor save capture did not start".into());
                    Some(EditorCompletion::SaveModelReturned {
                        target: pending.target,
                        pane: job.pane,
                        result: Err("Editor save capture did not start".into()),
                        cpu_thread: None,
                    })
                }
                JobOutcome::Panicked => {
                    pending
                        .write_slot
                        .fail("Editor capture worker panicked; write was not prepared".into());
                    Some(EditorCompletion::SaveModelLost {
                        target: pending.target,
                        message: "Editor capture worker panicked; save state is unknown".into(),
                    })
                }
            };
        }
        None
    }
    pub fn close_admission(&mut self) {
        self.closing = true;
        self.close_writer_after_preparation();
        for pending in &self.loads {
            pending.receipt.cancel();
        }
    }

    fn close_writer_after_preparation(&mut self) {
        if self.closing
            && self.measurements.is_empty()
            && self.captures.is_empty()
            && self.frozen_screen_saves.is_empty()
        {
            self.writes.close_admission();
        }
    }
}
