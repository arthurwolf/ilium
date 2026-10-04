//! Immutable sidebar filesystem preparation, shared by rendering and input.
use ilium_core::NodeId;
use ilium_execution::{Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Receipt, Retained};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SidebarRead {
    pub roots: Vec<(NodeId, PathBuf, Vec<NodeId>)>,
    pub opened: HashSet<Vec<NodeId>>,
}
pub struct SidebarEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_directory: bool,
}
#[derive(Default)]
pub struct SidebarSnapshot {
    pub(crate) roots: HashMap<NodeId, PathBuf>,
    pub(crate) directories: HashMap<(NodeId, PathBuf), Vec<SidebarEntry>>,
    pub(crate) rows: HashMap<NodeId, crate::tree_ui::FolderEntry>,
}
impl SidebarSnapshot {
    fn physical_bytes(&self) -> usize {
        use std::mem::size_of;
        let mut bytes = self.roots.capacity() * (size_of::<(NodeId, PathBuf)>() + 16)
            + self.directories.capacity()
                * (size_of::<((NodeId, PathBuf), Vec<SidebarEntry>)>() + 16)
            + self.rows.capacity() * (size_of::<(NodeId, crate::tree_ui::FolderEntry)>() + 16);
        bytes += self.roots.values().map(PathBuf::capacity).sum::<usize>();
        for ((_, path), entries) in &self.directories {
            bytes += path.capacity() + entries.capacity() * size_of::<SidebarEntry>();
            bytes += entries
                .iter()
                .map(|entry| entry.name.capacity() + entry.path.capacity())
                .sum::<usize>();
        }
        bytes += self
            .rows
            .values()
            .map(|entry| {
                entry.path.capacity() + entry.identifier_path.capacity() * size_of::<NodeId>()
            })
            .sum::<usize>();
        bytes
    }
}
impl SidebarRead {
    const COST: JobCost = JobCost {
        input_bytes: 32 * 1024 * 1024,
        result_bytes: 16 * 1024 * 1024,
    };
    fn checked(&self) -> Result<(), String> {
        if self.roots.capacity() > 64
            || self.opened.len() > 1024
            || self.opened.capacity() > 4096
            || self
                .roots
                .iter()
                .any(|(_, path, ids)| path.capacity() > 64 * 1024 || ids.capacity() > 64)
            || self.opened.iter().any(|ids| ids.capacity() > 64)
        {
            return Err("Sidebar filesystem inventory exceeds bounded preparation limits".into());
        }
        Ok(())
    }
}
impl Job for SidebarRead {
    type Output = SidebarSnapshot;
    type Error = String;
    fn run(self, context: JobContext) -> Result<SidebarSnapshot, String> {
        self.checked()?;
        let mut snapshot = SidebarSnapshot::default();
        let mut fields = 0_usize;
        let mut count = 0_usize;
        for (root, path, ids) in self.roots {
            snapshot.roots.insert(root, path.clone());
            let mut pending = vec![(path, ids)];
            while let Some((directory, ancestor)) = pending.pop() {
                if context.stop_requested() {
                    return Err("Sidebar preparation cancelled".into());
                }
                if !self.opened.contains(&ancestor) {
                    continue;
                }
                if ancestor.len() >= 64 {
                    return Err("Sidebar folder depth exceeds 64".into());
                }
                let entries = std::fs::read_dir(&directory)
                    .map_err(|error| format!("{}: {error}", directory.display()))?;
                let mut children = Vec::new();
                for entry in entries.flatten() {
                    if context.stop_requested() {
                        return Err("Sidebar preparation cancelled".into());
                    }
                    let name = entry.file_name();
                    if name.to_string_lossy().starts_with('.') {
                        continue;
                    }
                    let path = entry.path();
                    let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
                    let mut identifier_path = ancestor.clone();
                    let id = crate::tree_ui::virtual_folder_node_id(root, &path);
                    identifier_path.push(id);
                    let name = name.to_string_lossy().into_owned();
                    count += 1;
                    fields = fields.saturating_add(
                        name.capacity()
                            + path.capacity() * 3
                            + identifier_path.capacity() * std::mem::size_of::<NodeId>() * 2,
                    );
                    if count > 8192 || fields > 8 * 1024 * 1024 || path.capacity() > 64 * 1024 {
                        return Err(
                            "Sidebar listing exceeds retained limit (8192 entries/8 MiB fields)"
                                .into(),
                        );
                    }
                    if is_directory && self.opened.contains(&identifier_path) {
                        pending.push((path.clone(), identifier_path.clone()));
                    }
                    snapshot.rows.insert(
                        id,
                        crate::tree_ui::FolderEntry {
                            root_id: root,
                            path: path.clone(),
                            is_directory,
                            identifier_path,
                        },
                    );
                    children.push(SidebarEntry {
                        name,
                        path,
                        is_directory,
                    });
                }
                // Match existing native filename ordering, with directories first.
                children.sort_by(|a, b| {
                    b.is_directory
                        .cmp(&a.is_directory)
                        .then_with(|| a.path.file_name().cmp(&b.path.file_name()))
                });
                snapshot.directories.insert((root, directory), children);
            }
        }
        if snapshot.physical_bytes() > 16 * 1024 * 1024 {
            return Err("Sidebar snapshot exceeds 16 MiB physical retention limit".into());
        }
        Ok(snapshot)
    }
}
pub(crate) struct SidebarFiles {
    client: Client,
    active: Option<(SidebarRead, Receipt<SidebarRead>)>,
    desired: Option<SidebarRead>,
    last: Option<SidebarRead>,
    refresh_at: Instant,
    hold: Option<Retained<()>>,
    closing: bool,
}
impl SidebarFiles {
    pub fn new(client: Client, ready: Arc<tokio::sync::Notify>) -> Self {
        Self {
            client: client.with_completion_wake(move || ready.notify_one()),
            active: None,
            desired: None,
            last: None,
            refresh_at: Instant::now(),
            hold: None,
            closing: false,
        }
    }
    #[cfg(test)]
    pub(crate) fn pending(&self) -> bool {
        self.active.is_some() || self.desired.is_some()
    }
    pub fn close(&mut self) {
        self.closing = true;
        self.desired = None;
        if let Some((_, receipt)) = &self.active {
            receipt.cancel();
        }
        self.active = None;
    }
    pub fn poll(&mut self, request: SidebarRead) -> Result<Option<SidebarSnapshot>, String> {
        if self.closing {
            return Ok(None);
        }
        request.checked()?;
        if self.last.as_ref() != Some(&request) || Instant::now() >= self.refresh_at {
            self.last = Some(request.clone());
            self.desired = Some(request.clone());
            self.refresh_at = Instant::now() + Duration::from_secs(1);
            if let Some((old, receipt)) = &self.active {
                if old != &request {
                    receipt.cancel();
                }
            }
        }
        let mut result = None;
        if let Some((old, receipt)) = &mut self.active {
            match receipt.try_take() {
                JobPoll::Pending => return Ok(None),
                JobPoll::Ready(outcome) => {
                    let hold = outcome.map(|outcome| {
                        result = Some(match outcome {
                            JobOutcome::Finished(value) => value,
                            _ => Err("Sidebar worker did not finish".into()),
                        });
                    });
                    if old == &request {
                        match result.take() {
                            Some(Ok(snapshot)) => {
                                self.hold = Some(
                                    match self
                                        .client
                                        .try_reserve_external(JobCost {
                                            input_bytes: 16 * 1024 * 1024,
                                            result_bytes: 0,
                                        })
                                        .and_then(|reservation| {
                                            reservation
                                                .retain(())
                                                .map_err(|rejected| rejected.reason)
                                        }) {
                                        Ok(resident) => resident,
                                        Err(_) => hold,
                                    },
                                );
                                result = Some(Ok(snapshot));
                            }
                            Some(Err(error)) => result = Some(Err(error)),
                            None => {}
                        }
                    } else {
                        result = None;
                    }
                }
                JobPoll::Lost | JobPoll::Taken => {
                    result = Some(Err("Sidebar preparation receipt lost".into()))
                }
            }
            self.active = None;
        }
        if let Some(desired) = self.desired.take() {
            match self.client.try_submit(
                ilium_execution::Lane::Io,
                SidebarRead::COST,
                desired.clone(),
            ) {
                Ok(receipt) => self.active = Some((desired, receipt)),
                Err(rejected) => {
                    if matches!(
                        rejected.reason,
                        ilium_execution::RejectReason::Busy
                            | ilium_execution::RejectReason::QueueFull
                            | ilium_execution::RejectReason::JobLimit
                            | ilium_execution::RejectReason::InputBytes
                            | ilium_execution::RejectReason::ResultBytes
                    ) {
                        self.desired = Some(rejected.value);
                    } else {
                        return Err(format!("Sidebar not admitted: {:?}", rejected.reason));
                    }
                }
            }
        }
        result.transpose()
    }
}
impl Drop for SidebarFiles {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod capacity_tests {
    use super::*;

    #[test]
    fn collapsed_inventory_cannot_admit_unbounded_spare_capacity() {
        let opened = HashSet::with_capacity(8192);
        assert!(opened.is_empty());
        let request = SidebarRead {
            roots: Vec::new(),
            opened,
        };
        assert!(request.checked().is_err());
    }
}

impl crate::app::App {
    pub(crate) fn collect_sidebar_files(&mut self) -> bool {
        let mut roots = Vec::new();
        if self.tree_state.opened().len() > 1024
            || self.tree_state.opened().capacity() > 4096
            || self
                .tree_state
                .opened()
                .iter()
                .any(|path| path.capacity() > 64)
        {
            self.status_message =
                Some("Sidebar expanded-path capacity reached; previous listing retained".into());
            return false;
        }
        for id in self.tree.all_ids() {
            let Some(node) = self.tree.get(id) else {
                continue;
            };
            if let ilium_core::NodeKind::Folder { path, .. } = &node.kind {
                if roots.len() >= 64 || path.capacity() > 64 * 1024 {
                    self.status_message =
                        Some("Sidebar folder capacity reached; previous listing retained".into());
                    return false;
                }
                roots.push((id, path.clone(), crate::tree_ui::tree_path(&self.tree, id)));
            }
        }
        let request = SidebarRead {
            roots: roots.into_boxed_slice().into_vec(),
            opened: self.tree_state.opened().clone(),
        };
        let Some(files) = &mut self.sidebar_files else {
            return false;
        };
        match files.poll(request) {
            Ok(Some(snapshot)) => {
                self.sidebar_snapshot = snapshot;
                self.bump_tree_version();
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.status_message = Some(format!(
                    "Sidebar preparation failed; previous listing retained: {error}"
                ));
                true
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn prepared_for_test(
    tree: &ilium_core::Tree,
    opened: &HashSet<Vec<NodeId>>,
) -> Retained<SidebarSnapshot> {
    let roots = tree
        .all_ids()
        .filter_map(|id| match &tree.get(id)?.kind {
            ilium_core::NodeKind::Folder { path, .. } => {
                Some((id, path.clone(), crate::tree_ui::tree_path(tree, id)))
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
        .into_vec();
    let mut request = SidebarRead {
        roots,
        opened: opened.clone(),
    };
    let client = crate::execution::test_client();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut receipt = loop {
        match client.try_submit(ilium_execution::Lane::Io, SidebarRead::COST, request) {
            Ok(receipt) => break receipt,
            Err(rejected) => {
                request = rejected.value;
                assert!(
                    Instant::now() < deadline,
                    "sidebar fixture admission failed: {:?}",
                    rejected.reason
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    };
    loop {
        match receipt.try_take() {
            JobPoll::Ready(outcome) => {
                return outcome.map(|outcome| match outcome {
                    JobOutcome::Finished(Ok(snapshot)) => snapshot,
                    _ => panic!("sidebar fixture preparation failed"),
                })
            }
            JobPoll::Pending => {
                assert!(Instant::now() < deadline, "sidebar fixture never completed");
                std::thread::sleep(Duration::from_millis(1));
            }
            _ => panic!("sidebar fixture receipt lost"),
        }
    }
}
