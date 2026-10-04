use super::editor::{EditorRead, EditorWrite};
use super::ordered::{OrderedWriter, WriteCompletion, WriteId};
use ilium_core::NodeId;
use ilium_execution::{Client, JobOutcome, JobPoll, Lane, Receipt, Retained};
use std::path::PathBuf;
use std::sync::Arc;

const MAX_LOADS: usize = 4;

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
pub enum SavePurpose {
    Explicit,
    Autosave,
    SaveAs,
    OpenAsBoard,
}
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
    UnmatchedWrite(WriteCompletion<EditorWrite>),
}

pub struct EditorFiles {
    storage_quota: ilium_execution::QuotaGroup,
    client: Client,
    ready: Arc<tokio::sync::Notify>,
    loads: Vec<PendingLoad>,
    writes: OrderedWriter<EditorWrite>,
    saves: Vec<(WriteId, SaveTarget)>,
}
impl EditorFiles {
    pub fn new(client: Client) -> Self {
        let ready = Arc::new(tokio::sync::Notify::new());
        let wake = Arc::clone(&ready);
        let client = client.with_completion_wake(move || wake.notify_one());
        Self {
            storage_quota: crate::execution::process_quota(),
            writes: OrderedWriter::new(client.clone(), Arc::clone(&ready)),
            client,
            ready,
            loads: Vec::new(),
            saves: Vec::new(),
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
        self.loads.len() + self.writes.pending()
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
        lines: &[String],
    ) -> Result<(), String> {
        // Borrowed temporary computes the physical capacity bound before the
        // actual immutable source clone. The clone is built after reservation.
        let mut bytes = std::mem::size_of::<EditorWrite>() + path.capacity();
        let mut source = 0_usize;
        if lines.len() > 262_144 {
            return Err("Editor save exceeds line limit".into());
        }
        for line in lines {
            bytes = bytes
                .checked_add(line.len())
                .and_then(|bytes| bytes.checked_add(std::mem::size_of::<String>()))
                .ok_or_else(|| "Editor retained bytes overflow".to_string())?;
            source = source
                .checked_add(line.len() + 1)
                .ok_or_else(|| "Editor source bytes overflow".to_string())?;
        }
        if source > super::editor::MAX_EDITOR_SOURCE_BYTES {
            return Err("Editor save exceeds 32 MiB source limit".into());
        }
        let cost = ilium_execution::JobCost {
            input_bytes: bytes + 256 * 1024,
            result_bytes: path.capacity() + 256 * 1024,
        };
        let revision = target.revision;
        let id = self
            .writes
            .enqueue_with_detailed(cost, || EditorWrite {
                path,
                source_revision: revision,
                lines: lines.to_vec().into(),
            })
            .map_err(|reason| format!("Editor save not admitted: {reason:?}"))?;
        self.saves.push((id, target));
        Ok(())
    }
    pub fn poll(&mut self) -> Option<EditorCompletion> {
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
        let Some(index) = self.saves.iter().position(|(saved, _)| *saved == id) else {
            return Some(EditorCompletion::UnmatchedWrite(completion));
        };
        let (_, target) = self.saves.remove(index);
        Some(EditorCompletion::Saved { target, completion })
    }
    pub fn close_admission(&mut self) {
        self.writes.close_admission();
        for pending in &self.loads {
            pending.receipt.cancel();
        }
    }
}
