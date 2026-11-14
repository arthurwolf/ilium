//! Server routes for the plugin intents that a user action already has a kernel handler for
//! (gap G3). Each route turns the intent into the `ClientRequest` the client would send, so a
//! plugin gets the same validation, snapshot and broadcast path as the user. Intents that only
//! a client can carry out (attention, toasts, external opens, kernel commands such as
//! `kernel.focus-node`) are forwarded to the attached clients as `ForwardedIntent`s.
//!
//! This is a child module of `facade` so it can use the facade's private request helpers.

use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ilium_core::{AgentProvider, NodeKind, RestructureNode, RestructurePlan, Tree};
use ilium_ipc::{ClientRequest, PromptSubmissionSource};
use ilium_plugin_api::generated::types::intents::{
    CreateFolderSpec, CreateGroupSpec, CreateProjectSpec, Delivery, InputStage, MoveNodeSpec,
    NotifySpec, ReplacePaneCommandSpec, RestructureOp, RestructureSpec, SendInputSpec,
    SetInferredIconSpec,
};
use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::generated::types::{ErrorCode, Intent, NodeId, Outcome, Placement};
use ilium_plugin_api::kernel::Origin;

use super::{
    failed, refused, shell_word, to_core, to_plugin, unsupported, RequestEnd, ServerFacade,
};

/// The id of the one sound file Ilium ships (capability-sound-play.md, "Sound ids and files").
pub(crate) const BUNDLED_SOUND_ID: &str = "chirping";

/// The catalogue id of each alert sound event. The ids are the ones the Notifications plugin
/// plays; the player resolves the sound file from the user's sound settings for the event.
fn sound_event_for_id(sound: &str) -> Option<ilium_sound::SoundEvent> {
    match sound {
        "finished" => Some(ilium_sound::SoundEvent::AgentFinished),
        "ask" => Some(ilium_sound::SoundEvent::ApprovalRequired),
        "started" => Some(ilium_sound::SoundEvent::AgentStarted),
        "waiting" => Some(ilium_sound::SoundEvent::WaitingBackground),
        "task-done" => Some(ilium_sound::SoundEvent::TaskSucceeded),
        "task-failed" => Some(ilium_sound::SoundEvent::TaskFailed),
        _ => None,
    }
}

/// Whether `sound` is in the sound catalogue.
fn is_catalogue_sound(sound: &str) -> bool {
    sound == BUNDLED_SOUND_ID || sound_event_for_id(sound).is_some()
}
/// Most stages one `send-input` carries (reference-wit.md section 4.12).
const MAXIMUM_INPUT_STAGES: usize = 16;
/// Longest total delay of the stages of one `send-input`.
const MAXIMUM_STAGED_DELAY: Duration = Duration::from_secs(10);
/// Largest raw byte payload of one `send-input`.
const MAXIMUM_RAW_INPUT_BYTES: usize = 64 * 1024;
/// Most operations one `restructure` carries (api-intent-tree-structure.md).
const MAXIMUM_RESTRUCTURE_OPERATIONS: usize = 256;
/// Longest coalescing window a `notify` may ask for.
const MAXIMUM_COALESCE_WINDOW_MS: u32 = 60_000;
/// Most coalescing keys remembered at once; the oldest is forgotten first.
const MAXIMUM_COALESCE_KEYS: usize = 1024;
/// Most desktop notices one plugin shows per [`NOTIFY_RATE_WINDOW`]
/// (capability-notify-desktop.md, "More than 6 notices per minute").
const MAXIMUM_NOTICES_PER_WINDOW: usize = 6;
/// The sliding window of the per-plugin notice rate.
const NOTIFY_RATE_WINDOW: Duration = Duration::from_secs(60);

/// Notify coalescing memory: when each `(plugin, key)` last reached the desktop, and when each
/// plugin's recent notices did, for the per-plugin rate.
#[derive(Debug, Default)]
pub(crate) struct NotifyCoalescer {
    shown: HashMap<(String, String), Instant>,
    recent: HashMap<String, VecDeque<Instant>>,
}

impl NotifyCoalescer {
    /// Whether a plugin may show one more notice now, under the rate of 6 per minute. Only
    /// [`Self::record_notice`] uses up a place, so a refused or failed notice does not.
    pub(crate) fn within_rate(&mut self, plugin: &str, now: Instant) -> bool {
        let Some(shown) = self.recent.get_mut(plugin) else {
            return true;
        };
        while shown
            .front()
            .is_some_and(|first| now.saturating_duration_since(*first) >= NOTIFY_RATE_WINDOW)
        {
            shown.pop_front();
        }
        shown.len() < MAXIMUM_NOTICES_PER_WINDOW
    }

    /// Records that a plugin's notice reached the notifier.
    pub(crate) fn record_notice(&mut self, plugin: &str, now: Instant) {
        self.recent
            .entry(plugin.to_owned())
            .or_default()
            .push_back(now);
    }

    /// Whether a notice with this key may show now; records it when it may.
    pub(crate) fn admit(
        &mut self,
        plugin: &str,
        key: &str,
        window: Duration,
        now: Instant,
    ) -> bool {
        let entry = (plugin.to_owned(), key.to_owned());
        if let Some(last) = self.shown.get(&entry) {
            if now.saturating_duration_since(*last) < window {
                return false;
            }
        }
        if self.shown.len() >= MAXIMUM_COALESCE_KEYS && !self.shown.contains_key(&entry) {
            if let Some(oldest) = self
                .shown
                .iter()
                .min_by_key(|(_, shown)| **shown)
                .map(|(key, _)| key.clone())
            {
                self.shown.remove(&oldest);
            }
        }
        self.shown.insert(entry, now);
        true
    }
}

/// A plugin path resolved to a directory on disk. Parent segments and absolute paths are
/// `invalid-argument` (reference-wit.md section 2); the scopes a server plugin may name for a
/// tree entry are `project` (the session's project directory) and `home`.
pub(crate) fn resolve_fs_path(
    path: &FsPath,
    project_dir: &Path,
    home_dir: Option<&Path>,
) -> Result<PathBuf, Outcome> {
    let relative = Path::new(&path.path);
    let escapes = relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if escapes {
        return Err(refused(
            ErrorCode::InvalidArgument,
            "a path must be relative to its scope and must not leave it",
        ));
    }
    let base = match path.scope.as_str() {
        "project" => project_dir.to_path_buf(),
        "home" => match home_dir {
            Some(home) => home.to_path_buf(),
            None => {
                return Err(failed(
                    ErrorCode::NotFound,
                    "this user has no home directory",
                ))
            }
        },
        other => {
            return Err(refused(
                ErrorCode::Unsupported,
                format!("path scope {other:?} cannot hold a tree entry"),
            ));
        }
    };
    Ok(base.join(relative))
}

/// Checks the shape of a `send-input` the kernel can carry out and returns why it cannot.
fn send_input_refusal(spec: &SendInputSpec) -> Option<Outcome> {
    if spec.mouse.is_some() {
        return Some(unsupported("send-input with mouse events"));
    }
    if spec.if_current.is_some() {
        return Some(unsupported("send-input with an if-current precondition"));
    }
    if spec.lock_user_input {
        return Some(unsupported("send-input with lock-user-input"));
    }
    if spec.journal {
        return Some(unsupported("send-input with a journal entry"));
    }
    if spec.prompt_epoch.is_some() {
        return Some(unsupported("send-input with a prompt epoch"));
    }
    if spec
        .raw
        .as_ref()
        .is_some_and(|raw| raw.len() > MAXIMUM_RAW_INPUT_BYTES)
    {
        return Some(refused(
            ErrorCode::InvalidArgument,
            "raw input is at most 64 KiB",
        ));
    }
    if spec.raw.is_some() && !spec.stages.is_empty() {
        return Some(refused(
            ErrorCode::InvalidArgument,
            "raw input and stages cannot be combined",
        ));
    }
    if spec.stages.len() > MAXIMUM_INPUT_STAGES {
        return Some(refused(
            ErrorCode::InvalidArgument,
            "send-input carries at most 16 stages",
        ));
    }
    let total: u64 = spec
        .stages
        .iter()
        .map(|stage| u64::from(stage.delay_ms))
        .sum();
    if Duration::from_millis(total) > MAXIMUM_STAGED_DELAY {
        return Some(refused(
            ErrorCode::InvalidArgument,
            "the stages of one send-input wait at most 10 s in total",
        ));
    }
    None
}

/// The request that writes one piece of text into a pane with the given delivery.
pub(crate) fn input_request(
    pane_id: ilium_core::NodeId,
    text: String,
    delivery: Delivery,
) -> ClientRequest {
    match delivery {
        Delivery::Typed => ClientRequest::KeyInput {
            pane_id,
            bytes: text.into_bytes(),
            submission: None,
        },
        Delivery::Submitted => ClientRequest::SubmitTerminalText {
            pane_id,
            text,
            source: PromptSubmissionSource::PluginIntent,
        },
        Delivery::Pasted => ClientRequest::PasteTerminalText { pane_id, text },
    }
}

// ---------------------------------------------------------------------------------------------
// Restructure: the plugin's operations applied to a mirror of one project, then sent as the
// complete project plan the kernel's own restructure handler validates and applies.
// ---------------------------------------------------------------------------------------------

/// One entry of the mirror: an existing node, or a group a `create-group` operation made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EntryKey {
    Node(ilium_core::NodeId),
    New(usize),
}

#[derive(Debug, Clone, PartialEq)]
enum EntryShape {
    Project,
    Pane {
        title: String,
        short_title: Option<String>,
        icon: Option<String>,
    },
    Folder {
        title: String,
        short_title: Option<String>,
        icon: Option<String>,
    },
    /// An existing group. A group the user named keeps its identity; any other group is
    /// rebuilt from its title by the kernel's restructure.
    Group {
        is_name_fixed: bool,
        title: String,
        short_title: Option<String>,
        icon: Option<String>,
    },
    NewGroup {
        title: String,
    },
    /// A split view keeps its exact panes; the restructure may only move it as a whole.
    SplitView,
}

#[derive(Debug, Clone)]
struct MirrorEntry {
    shape: EntryShape,
    parent: Option<EntryKey>,
    children: Vec<EntryKey>,
}

/// A project as a plan the operations edit.
#[derive(Debug, Clone)]
pub(crate) struct ProjectMirror {
    project: ilium_core::NodeId,
    entries: HashMap<EntryKey, MirrorEntry>,
    next_new: usize,
}

impl ProjectMirror {
    /// Mirrors the current shape of one project. Worktree containers are transparent: their
    /// panes are listed in place, and the kernel rebuilds the worktree membership itself.
    pub(crate) fn of(tree: &Tree, project: ilium_core::NodeId) -> Result<Self, String> {
        if !tree.get(project).is_some_and(ilium_core::Node::is_project) {
            return Err(format!("node {} is not a project", project.0));
        }
        let mut mirror = Self {
            project,
            entries: HashMap::new(),
            next_new: 0,
        };
        mirror.entries.insert(
            EntryKey::Node(project),
            MirrorEntry {
                shape: EntryShape::Project,
                parent: None,
                children: Vec::new(),
            },
        );
        mirror.add_children(tree, project, EntryKey::Node(project))?;
        Ok(mirror)
    }

    fn add_children(
        &mut self,
        tree: &Tree,
        container: ilium_core::NodeId,
        parent: EntryKey,
    ) -> Result<(), String> {
        let children = tree
            .children_of(container)
            .map_err(|error| error.to_string())?
            .to_vec();
        for child in children {
            let node = tree
                .get(child)
                .ok_or_else(|| format!("node {} vanished", child.0))?;
            if node.is_worktree() {
                self.add_children(tree, child, parent)?;
                continue;
            }
            let shape = match &node.kind {
                NodeKind::Pane { .. } => EntryShape::Pane {
                    title: node.name.clone(),
                    short_title: node.short_name.clone(),
                    icon: node.inferred_icon.clone(),
                },
                NodeKind::Folder { .. } => EntryShape::Folder {
                    title: node.name.clone(),
                    short_title: node.short_name.clone(),
                    icon: node.inferred_icon.clone(),
                },
                NodeKind::Container(_) if node.is_split_view() => EntryShape::SplitView,
                NodeKind::Container(_) if node.is_group() => EntryShape::Group {
                    is_name_fixed: node.is_name_fixed,
                    title: node.name.clone(),
                    short_title: node.short_name.clone(),
                    icon: node.inferred_icon.clone(),
                },
                NodeKind::Container(_) => {
                    return Err(format!("node {} cannot sit inside a project", child.0));
                }
            };
            let key = EntryKey::Node(child);
            self.entries.insert(
                key,
                MirrorEntry {
                    shape: shape.clone(),
                    parent: Some(parent),
                    children: Vec::new(),
                },
            );
            self.push_child(parent, key, None);
            if matches!(shape, EntryShape::Group { .. } | EntryShape::SplitView) {
                self.add_children(tree, child, key)?;
            }
        }
        Ok(())
    }

    fn push_child(&mut self, parent: EntryKey, child: EntryKey, index: Option<usize>) {
        if let Some(entry) = self.entries.get_mut(&parent) {
            let index = index
                .unwrap_or(entry.children.len())
                .min(entry.children.len());
            entry.children.insert(index, child);
        }
        if let Some(entry) = self.entries.get_mut(&child) {
            entry.parent = Some(parent);
        }
    }

    fn detach(&mut self, child: EntryKey) {
        let parent = self.entries.get(&child).and_then(|entry| entry.parent);
        if let Some(parent) = parent.and_then(|parent| self.entries.get_mut(&parent)) {
            parent.children.retain(|known| *known != child);
        }
    }

    fn shape(&self, key: EntryKey) -> Option<&EntryShape> {
        self.entries.get(&key).map(|entry| &entry.shape)
    }

    fn accepts_children(&self, key: EntryKey) -> bool {
        matches!(
            self.shape(key),
            Some(EntryShape::Project | EntryShape::Group { .. } | EntryShape::NewGroup { .. })
        )
    }

    fn is_in_split_view(&self, key: EntryKey) -> bool {
        self.entries
            .get(&key)
            .and_then(|entry| entry.parent)
            .is_some_and(|parent| matches!(self.shape(parent), Some(EntryShape::SplitView)))
    }

    /// Whether `ancestor` is `key` or holds it.
    fn holds(&self, ancestor: EntryKey, key: EntryKey) -> bool {
        let mut current = Some(key);
        while let Some(at) = current {
            if at == ancestor {
                return true;
            }
            current = self.entries.get(&at).and_then(|entry| entry.parent);
        }
        false
    }

    fn movable(&self, node: EntryKey) -> Result<(), String> {
        match self.shape(node) {
            None => Err("the node is not part of this project".to_owned()),
            Some(EntryShape::Project) => Err("a project cannot be moved".to_owned()),
            Some(_) if self.is_in_split_view(node) => {
                Err("a pane of a split view stays in its split view".to_owned())
            }
            Some(_) => Ok(()),
        }
    }

    /// Moves `node` next to or into `target`.
    pub(crate) fn move_node(&mut self, spec: &MoveNodeSpec) -> Result<(), String> {
        let node = EntryKey::Node(to_core(spec.node));
        let target = EntryKey::Node(to_core(spec.target));
        self.movable(node)?;
        if self.shape(target).is_none() {
            return Err("the target is not part of this project".to_owned());
        }
        let (parent, index) = match spec.placement {
            Placement::InsideFirst | Placement::InsideLast => {
                if !self.accepts_children(target) {
                    return Err("the target cannot hold entries".to_owned());
                }
                let index = matches!(spec.placement, Placement::InsideFirst).then_some(0);
                (target, index)
            }
            Placement::Before | Placement::After => {
                if node == target {
                    return Ok(());
                }
                let parent = self
                    .entries
                    .get(&target)
                    .and_then(|entry| entry.parent)
                    .ok_or_else(|| "the target has no parent".to_owned())?;
                if !self.accepts_children(parent) {
                    return Err("a split view keeps its own panes".to_owned());
                }
                (parent, None)
            }
        };
        if self.holds(node, parent) {
            return Err("a node cannot move inside itself".to_owned());
        }
        self.detach(node);
        let index = match spec.placement {
            Placement::Before | Placement::After => {
                let position = self
                    .entries
                    .get(&parent)
                    .and_then(|entry| entry.children.iter().position(|known| *known == target))
                    .ok_or_else(|| "the target left its parent".to_owned())?;
                Some(if matches!(spec.placement, Placement::After) {
                    position + 1
                } else {
                    position
                })
            }
            Placement::InsideFirst | Placement::InsideLast => index,
        };
        self.push_child(parent, node, index);
        Ok(())
    }

    /// Adds a new group under `parent` (the project when absent) and moves `children` into it.
    pub(crate) fn create_group(&mut self, spec: &CreateGroupSpec) -> Result<EntryKey, String> {
        let parent = spec.parent.map_or(EntryKey::Node(self.project), |parent| {
            EntryKey::Node(to_core(parent))
        });
        if !self.accepts_children(parent) {
            return Err("the parent cannot hold a group".to_owned());
        }
        for child in &spec.children {
            let child = EntryKey::Node(to_core(*child));
            self.movable(child)?;
            if self.holds(child, parent) {
                return Err("a group cannot be created inside one of its own children".to_owned());
            }
        }
        let index = match spec.placement {
            Placement::InsideFirst => Some(0),
            Placement::InsideLast => None,
            // Before or after the first child when it already sits under the parent.
            Placement::Before | Placement::After => spec.children.first().and_then(|first| {
                let first = EntryKey::Node(to_core(*first));
                let position = self
                    .entries
                    .get(&parent)?
                    .children
                    .iter()
                    .position(|known| *known == first)?;
                Some(if matches!(spec.placement, Placement::After) {
                    position + 1
                } else {
                    position
                })
            }),
        };
        let key = EntryKey::New(self.next_new);
        self.next_new += 1;
        self.entries.insert(
            key,
            MirrorEntry {
                shape: EntryShape::NewGroup {
                    title: spec.title.clone(),
                },
                parent: Some(parent),
                children: Vec::new(),
            },
        );
        self.push_child(parent, key, index);
        for child in &spec.children {
            let child = EntryKey::Node(to_core(*child));
            self.detach(child);
            self.push_child(key, child, None);
        }
        Ok(key)
    }

    /// Replaces a group with its children, in place.
    pub(crate) fn ungroup(&mut self, group: NodeId) -> Result<(), String> {
        let key = EntryKey::Node(to_core(group));
        match self.shape(key) {
            Some(EntryShape::Group {
                is_name_fixed: false,
                ..
            }) => {}
            Some(EntryShape::Group {
                is_name_fixed: true,
                ..
            }) => return Err("a group the user named cannot be ungrouped".to_owned()),
            _ => return Err("the node is not a group of this project".to_owned()),
        }
        let parent = self
            .entries
            .get(&key)
            .and_then(|entry| entry.parent)
            .ok_or_else(|| "the group has no parent".to_owned())?;
        let index = self
            .entries
            .get(&parent)
            .and_then(|entry| entry.children.iter().position(|known| *known == key))
            .unwrap_or(0);
        let children = self
            .entries
            .get(&key)
            .map(|entry| entry.children.clone())
            .unwrap_or_default();
        self.detach(key);
        self.entries.remove(&key);
        for (offset, child) in children.into_iter().enumerate() {
            self.push_child(parent, child, Some(index + offset));
        }
        Ok(())
    }

    /// The plan the kernel applies for this mirror.
    pub(crate) fn plan(&self) -> RestructurePlan {
        RestructurePlan {
            children: self.plan_children(EntryKey::Node(self.project)),
        }
    }

    fn plan_children(&self, key: EntryKey) -> Vec<RestructureNode> {
        let Some(entry) = self.entries.get(&key) else {
            return Vec::new();
        };
        entry
            .children
            .iter()
            .filter_map(|child| self.plan_node(*child))
            .collect()
    }

    fn plan_node(&self, key: EntryKey) -> Option<RestructureNode> {
        let entry = self.entries.get(&key)?;
        let children = || self.plan_children(key);
        Some(match (&entry.shape, key) {
            (EntryShape::Project, _) => return None,
            (
                EntryShape::Pane {
                    title,
                    short_title,
                    icon,
                },
                EntryKey::Node(id),
            ) => RestructureNode::Pane {
                id,
                title: title.clone(),
                short_title: short_title.clone(),
                icon: icon.clone(),
            },
            (
                EntryShape::Folder {
                    title,
                    short_title,
                    icon,
                },
                EntryKey::Node(id),
            ) => RestructureNode::Folder {
                id,
                title: title.clone(),
                short_title: short_title.clone(),
                icon: icon.clone(),
            },
            (
                EntryShape::Group {
                    is_name_fixed: true,
                    ..
                },
                EntryKey::Node(id),
            ) => RestructureNode::ExistingGroup {
                id,
                children: children(),
            },
            (
                EntryShape::Group {
                    is_name_fixed: false,
                    title,
                    short_title,
                    icon,
                },
                _,
            ) => RestructureNode::Group {
                title: title.clone(),
                short_title: short_title.clone(),
                icon: icon.clone(),
                children: children(),
            },
            (EntryShape::NewGroup { title }, _) => RestructureNode::Group {
                title: title.clone(),
                short_title: None,
                icon: None,
                children: children(),
            },
            (EntryShape::SplitView, EntryKey::Node(id)) => RestructureNode::ExistingSplitView {
                id,
                children: children(),
            },
            // Panes, folders, fixed groups and split views are always existing nodes.
            (_, EntryKey::New(_)) => return None,
        })
    }
}

/// The one project every node a restructure names belongs to.
fn restructure_project(tree: &Tree, spec: &RestructureSpec) -> Result<ilium_core::NodeId, String> {
    let mut named = Vec::new();
    for operation in &spec.operations {
        match operation {
            RestructureOp::Move(spec) => named.extend([spec.node, spec.target]),
            RestructureOp::CreateGroup(spec) => {
                named.extend(spec.parent);
                named.extend(spec.children.iter().copied());
            }
            RestructureOp::Ungroup(group) => named.push(*group),
            RestructureOp::Rename(spec) => named.push(spec.node),
        }
    }
    let mut project = None;
    for node in named {
        let node = to_core(node);
        let found = if tree.get(node).is_some_and(ilium_core::Node::is_project) {
            Some(node)
        } else {
            tree.project_ancestor(node)
        };
        let Some(found) = found else {
            return Err(format!("node {} is not inside a project", node.0));
        };
        match project {
            None => project = Some(found),
            Some(known) if known != found => {
                return Err("one restructure changes one project".to_owned());
            }
            Some(_) => {}
        }
    }
    project.ok_or_else(|| "the restructure names no node".to_owned())
}

/// Applies the structural operations of a restructure to the mirror of its project. A failing
/// operation is reported with its index (api-intent-tree-structure.md).
pub(crate) fn mirror_restructure(
    tree: &Tree,
    spec: &RestructureSpec,
) -> Result<(ilium_core::NodeId, RestructurePlan), String> {
    let project = restructure_project(tree, spec)?;
    let mut mirror = ProjectMirror::of(tree, project)?;
    for (index, operation) in spec.operations.iter().enumerate() {
        let result = match operation {
            RestructureOp::Move(spec) => mirror.move_node(spec),
            RestructureOp::CreateGroup(spec) => mirror.create_group(spec).map(drop),
            RestructureOp::Ungroup(group) => mirror.ungroup(*group),
            RestructureOp::Rename(spec) => {
                if tree.get(to_core(spec.node)).is_none() {
                    Err("no such node".to_owned())
                } else {
                    Ok(())
                }
            }
        };
        result.map_err(|reason| format!("operation {index}: {reason}"))?;
    }
    Ok((project, mirror.plan()))
}

impl ServerFacade {
    /// Routes the intents `apply_intent` does not handle inline.
    pub(super) fn route_intent(&self, origin: &Origin, intent: Intent) -> Outcome {
        match intent {
            Intent::SendInput(spec) => self.send_input(spec),
            Intent::CreateFolder(spec) => self.create_folder(spec),
            Intent::CreateProject(spec) => self.create_project(spec),
            Intent::Restructure(spec) => self.restructure(spec),
            Intent::RevertRestructure(token) => {
                self.simple(ClientRequest::RevertProjectRestructure {
                    project_id: ilium_core::NodeId(token),
                })
            }
            Intent::FreezePane(spec) => self.freeze_pane(spec.pane),
            Intent::UnfreezePane(spec) => self.simple(ClientRequest::UnfreezePane {
                pane_id: to_core(spec.pane),
            }),
            Intent::ReplacePaneCommand(spec) => self.replace_pane_command(spec),
            // The kernel stops the process tree through its captured termination handle; the
            // grace period of the intent is the handle's own.
            Intent::TerminatePaneProcess(spec) => {
                self.simple(ClientRequest::TerminatePaneProcess {
                    pane_id: to_core(spec.pane),
                })
            }
            Intent::SetInferredIcon(spec) => self.set_inferred_icon(spec),
            Intent::PlaySound(sound) => self.play_sound(&sound),
            Intent::Notify(spec) => self.notify(origin, spec),
            Intent::MarkAttention(_)
            | Intent::AckAttention(_)
            | Intent::Toast(_)
            | Intent::DismissToast(_)
            | Intent::OpenExternal(_) => self.forward_to_clients(origin, &intent),
            Intent::InvokeCommand(ref spec) if spec.command.starts_with("kernel.") => {
                self.forward_to_clients(origin, &intent)
            }
            Intent::InvokeCommand(_) => unsupported("invoke-command for a plugin command"),
            // The remaining intents belong to features that are not ported yet; each port adds
            // its arm together with its handler (foundation contract section 12.4).
            _ => unsupported("this intent"),
        }
    }

    /// Sends a client-only intent of a server plugin to every attached client.
    pub(super) fn forward_to_clients(&self, origin: &Origin, intent: &Intent) -> Outcome {
        let Origin::Plugin { plugin, .. } = origin else {
            return refused(
                ErrorCode::NotPermitted,
                "only a plugin intent is forwarded to clients",
            );
        };
        let intent_json = match serde_json::to_vec(intent) {
            Ok(bytes) => bytes,
            Err(error) => {
                return failed(
                    ErrorCode::Internal,
                    format!("the intent does not serialize: {error}"),
                );
            }
        };
        let Some(state) = self.state.upgrade() else {
            return failed(ErrorCode::Internal, "the server is shutting down");
        };
        state.broadcast(ilium_ipc::ServerEvent::Plugin(
            ilium_ipc::plugin::PluginEnvelope {
                plugin: plugin.0.clone(),
                seq: self.next_envelope.fetch_add(1, Ordering::Relaxed),
                body: ilium_ipc::plugin::EnvelopeBody::ForwardedIntent(
                    ilium_ipc::plugin::ForwardedIntent {
                        plugin: plugin.0.clone(),
                        target: ilium_ipc::plugin::ForwardTarget::AllClients,
                        intent_json,
                    },
                ),
            },
        ));
        Outcome::Ok
    }

    fn send_input(&self, spec: SendInputSpec) -> Outcome {
        if let Some(refusal) = send_input_refusal(&spec) {
            return refusal;
        }
        let pane_id = to_core(spec.pane);
        if let Some(raw) = spec.raw {
            return self.simple(ClientRequest::KeyInput {
                pane_id,
                bytes: raw,
                submission: None,
            });
        }
        if spec.stages.is_empty() {
            return self.simple(input_request(pane_id, spec.text, spec.delivery));
        }
        self.send_stages(pane_id, spec.stages)
    }

    /// Writes the stages in order on the server's runtime, each after its delay. The intent
    /// completes once the stages are scheduled; a stage that fails stops the rest.
    fn send_stages(&self, pane_id: ilium_core::NodeId, stages: Vec<InputStage>) -> Outcome {
        let Some(state) = self.state.upgrade() else {
            return failed(ErrorCode::Internal, "the server is shutting down");
        };
        let work = async move {
            for stage in stages {
                tokio::time::sleep(Duration::from_millis(u64::from(stage.delay_ms))).await;
                let request = input_request(pane_id, stage.text, stage.delivery);
                match super::drive_request(&state, request, None).await {
                    RequestEnd::Done | RequestEnd::Created(_) => {}
                    RequestEnd::Failed(message) => {
                        tracing::warn!("a staged plugin input stopped: {message}");
                        return;
                    }
                    RequestEnd::TimedOut => return,
                }
            }
        };
        match self.input_stages.lock() {
            Ok(mut tasks) => {
                while tasks.try_join_next().is_some() {}
                tasks.spawn_on(work, &self.tokio);
                Outcome::Ok
            }
            Err(_) => failed(ErrorCode::Internal, "the staged input set is poisoned"),
        }
    }

    /// The node ids of the tree, to find what one request created.
    fn node_ids(&self) -> std::collections::BTreeSet<ilium_core::NodeId> {
        self.with_tree(|tree| tree.all_ids().collect())
            .unwrap_or_default()
    }

    /// The one node a request created that matches `wanted`.
    fn created_node(
        &self,
        before: &std::collections::BTreeSet<ilium_core::NodeId>,
        wanted: impl Fn(&ilium_core::Node) -> bool,
    ) -> Option<ilium_core::NodeId> {
        self.with_tree(|tree| {
            tree.all_ids()
                .filter(|id| !before.contains(id))
                .find(|id| tree.get(*id).is_some_and(&wanted))
        })
        .flatten()
    }

    fn home_dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
    }

    fn create_folder(&self, spec: CreateFolderSpec) -> Outcome {
        let Some(path) = spec.path.as_ref() else {
            return refused(
                ErrorCode::InvalidArgument,
                "a folder entry needs the directory it shows",
            );
        };
        let path = match resolve_fs_path(path, &self.project_dir, Self::home_dir().as_deref()) {
            Ok(path) => path,
            Err(outcome) => return outcome,
        };
        if !path.is_dir() {
            return refused(ErrorCode::NotFound, "the folder's directory does not exist");
        }
        let before = self.node_ids();
        let outcome = self.simple(ClientRequest::NewFolder {
            parent_group: spec.parent.map_or(ilium_core::ROOT_ID, to_core),
            path,
        });
        if !matches!(outcome, Outcome::Ok) {
            return outcome;
        }
        let Some(folder) = self.created_node(&before, ilium_core::Node::is_folder) else {
            return failed(ErrorCode::Internal, "the server did not report the folder");
        };
        if !spec.title.is_empty() {
            // A failed title is not a failed folder: the folder exists and is reported.
            let _ = self.simple(ClientRequest::RenameNode {
                node_id: folder,
                title: spec.title,
                short_title: None,
                inferred_icon: None,
            });
        }
        Outcome::OkNode(to_plugin(folder))
    }

    fn create_project(&self, spec: CreateProjectSpec) -> Outcome {
        let path = match resolve_fs_path(&spec.root, &self.project_dir, Self::home_dir().as_deref())
        {
            Ok(path) => path,
            Err(outcome) => return outcome,
        };
        let before = self.node_ids();
        let outcome = self.simple(ClientRequest::NewProject { path });
        if !matches!(outcome, Outcome::Ok) {
            return outcome;
        }
        let Some(project) = self.created_node(&before, ilium_core::Node::is_project) else {
            return failed(ErrorCode::Internal, "the server did not report the project");
        };
        if !spec.title.is_empty() {
            let _ = self.simple(ClientRequest::RenameNode {
                node_id: project,
                title: spec.title,
                short_title: None,
                inferred_icon: None,
            });
        }
        Outcome::OkNode(to_plugin(project))
    }

    /// Applies a restructure as one project plan through the kernel's restructure handler, then
    /// the renames it carries. The undo token is the project id: the kernel keeps one undo point
    /// per project, so `revert-restructure` restores the project as it was before the latest
    /// restructure of it. Renames are user renames and are not part of that undo point.
    fn restructure(&self, spec: RestructureSpec) -> Outcome {
        if spec.operations.len() > MAXIMUM_RESTRUCTURE_OPERATIONS {
            return refused(
                ErrorCode::InvalidArgument,
                "a restructure carries at most 256 operations",
            );
        }
        let structural = spec
            .operations
            .iter()
            .any(|operation| !matches!(operation, RestructureOp::Rename(_)));
        let mirrored = self.with_tree(|tree| mirror_restructure(tree, &spec));
        let (project, plan) = match mirrored {
            None => return failed(ErrorCode::Internal, "the server is shutting down"),
            Some(Err(reason)) => return refused(ErrorCode::Conflict, reason),
            Some(Ok(mirrored)) => mirrored,
        };
        if structural {
            match self.request(
                ClientRequest::ApplyProjectRestructurePlan {
                    project_id: project,
                    plan,
                    inference_activity_revisions: Vec::new(),
                    title_observations: Vec::new(),
                },
                None,
            ) {
                RequestEnd::Done | RequestEnd::Created(_) => {}
                RequestEnd::Failed(message) => return refused(ErrorCode::Conflict, message),
                RequestEnd::TimedOut => {
                    return failed(ErrorCode::TimedOut, "the restructure timed out");
                }
            }
        }
        for (index, operation) in spec.operations.into_iter().enumerate() {
            let RestructureOp::Rename(rename) = operation else {
                continue;
            };
            let outcome = self.simple(ClientRequest::RenameNode {
                node_id: to_core(rename.node),
                title: rename.title,
                short_title: None,
                inferred_icon: None,
            });
            if let Outcome::Failed(error) | Outcome::Refused(error) = outcome {
                return refused(
                    ErrorCode::Conflict,
                    format!("operation {index}: {}", error.message),
                );
            }
        }
        Outcome::OkNode(to_plugin(project))
    }

    /// Freezes an agent pane with the resume command of its live agent session, the way the
    /// client's freeze action does.
    fn freeze_pane(&self, pane: NodeId) -> Outcome {
        let pane_id = to_core(pane);
        let Some(state) = self.state.upgrade() else {
            return failed(ErrorCode::Internal, "the server is shutting down");
        };
        let resume_command = {
            let tree = state.tree.blocking_read();
            let panes = state.panes.blocking_read();
            match (tree.get(pane_id), panes.get(&pane_id)) {
                (Some(node), Some(crate::pane::PaneResource::Terminal(runtime))) => {
                    let snapshot = crate::title_eligibility::TitleRuntimeSnapshot::capture(
                        node, runtime, false,
                    );
                    snapshot
                        .observation
                        .agent_class
                        .as_ref()
                        .and_then(|class| class.provider())
                        .zip(snapshot.observation.session_id.as_deref())
                        .map(|(provider, session_id)| {
                            provider.resume_command(&shell_word(session_id))
                        })
                }
                (None, _) => return refused(ErrorCode::NotFound, "no such pane"),
                _ => None,
            }
        };
        match resume_command {
            Some(resume_command) => self.simple(ClientRequest::FreezePane {
                pane_id,
                resume_command,
            }),
            None => refused(
                ErrorCode::Conflict,
                "only an agent pane with a known session can be frozen",
            ),
        }
    }

    fn replace_pane_command(&self, spec: ReplacePaneCommandSpec) -> Outcome {
        if spec.cwd.is_some() {
            return unsupported("replace-pane-command with a new working directory");
        }
        if spec.command.is_empty() {
            return refused(ErrorCode::InvalidArgument, "the command is empty");
        }
        let command_line = spec
            .command
            .iter()
            .map(|word| shell_word(word))
            .collect::<Vec<_>>()
            .join(" ");
        self.simple(ClientRequest::ReplacePaneWithCommand {
            pane_id: to_core(spec.pane),
            command_line,
        })
    }

    /// Sets the inferred icon of a pane as an automatic title update, which keeps the pane's
    /// current titles and leaves a user-named pane untouched.
    fn set_inferred_icon(&self, spec: SetInferredIconSpec) -> Outcome {
        let pane_id = to_core(spec.node);
        let titles = self
            .with_tree(|tree| {
                tree.get(pane_id)
                    .filter(|node| node.is_pane())
                    .map(|node| (node.name.clone(), node.short_name.clone()))
            })
            .flatten();
        let Some((title, short_title)) = titles else {
            return refused(ErrorCode::NotFound, "set-inferred-icon applies to a pane");
        };
        self.simple(ClientRequest::SetAutomaticPaneTitle {
            pane_id,
            title,
            short_title,
            inferred_icon: spec.icon,
        })
    }

    /// Plays a catalogue sound through the server's sound player. A user who muted sounds
    /// hears nothing and the intent still completes.
    fn play_sound(&self, sound: &str) -> Outcome {
        if !is_catalogue_sound(sound) {
            return refused(
                ErrorCode::NotFound,
                format!("{sound:?} is not in the sound catalogue"),
            );
        }
        let Some(state) = self.state.upgrade() else {
            return failed(ErrorCode::Internal, "the server is shutting down");
        };
        let settings = state.sound_settings.blocking_read().clone();
        if settings.source == ilium_sound::SoundSourceKind::Muted {
            return Outcome::Ok;
        }
        if let Some(event) = sound_event_for_id(sound) {
            // The plugin already applied the user's per-event toggle; the player still
            // resolves the file for the event.
            return match state.sound_requests.prepare(&settings, Some(event), None) {
                Ok(request) => {
                    crate::sounds::enqueue_prepared(&state, request);
                    Outcome::Ok
                }
                Err(reason) => {
                    tracing::warn!(?reason, "plugin sound refused before allocation");
                    failed(ErrorCode::Busy, "the sound player did not admit the sound")
                }
            };
        }
        self.simple(ClientRequest::PreviewSound {
            source: ilium_sound::SoundSourceKind::BundledChirping,
            file: None,
        })
    }

    /// Shows a desktop notice through the server's notifier, after the user's master switch and
    /// the plugin's coalescing key.
    fn notify(&self, origin: &Origin, spec: NotifySpec) -> Outcome {
        let Origin::Plugin { plugin, .. } = origin else {
            return refused(ErrorCode::NotPermitted, "only a plugin sends a notice");
        };
        if let Some(sound) = spec.sound.as_deref() {
            if !is_catalogue_sound(sound) {
                return refused(
                    ErrorCode::NotFound,
                    format!("{sound:?} is not in the sound catalogue"),
                );
            }
        }
        if spec.coalesce_window_ms > MAXIMUM_COALESCE_WINDOW_MS {
            return refused(
                ErrorCode::InvalidArgument,
                "the coalescing window is at most 60,000 ms",
            );
        }
        let Some(state) = self.state.upgrade() else {
            return failed(ErrorCode::Internal, "the server is shutting down");
        };
        if !state.notifications_config.blocking_read().enabled {
            return refused(
                ErrorCode::NotPermitted,
                "the user turned desktop notifications off",
            );
        }
        // A core plugin is the user's own alert rules, which carry their own merge window, so the
        // third-party rate does not apply to it.
        let is_core = ilium_plugin_host::lifecycle::is_core_plugin(&plugin.0);
        // Checked before the coalescing key, so a refused notice does not start its window.
        let within_rate = is_core
            || self
                .notify_coalescer
                .lock()
                .map(|mut coalescer| coalescer.within_rate(&plugin.0, Instant::now()))
                .unwrap_or(true);
        if !within_rate {
            return refused(
                ErrorCode::RateLimited,
                "a plugin shows at most 6 desktop notices per minute",
            );
        }
        if let Some(key) = spec.coalesce_key.as_deref() {
            if spec.coalesce_window_ms > 0 {
                let window = Duration::from_millis(u64::from(spec.coalesce_window_ms));
                let admitted = self
                    .notify_coalescer
                    .lock()
                    .map(|mut coalescer| coalescer.admit(&plugin.0, key, window, Instant::now()))
                    .unwrap_or(true);
                if !admitted {
                    return Outcome::Ok;
                }
            }
        }
        let Some(execution) = state.execution.get() else {
            return failed(
                ErrorCode::Internal,
                "the server execution bank is not started",
            );
        };
        let pending = crate::notifications::PendingNotification::for_plugin(
            self.session_name.clone(),
            plugin.0.clone(),
            spec.title,
            spec.body,
            is_core,
        );
        if !crate::notifications::send_admitted(&execution.client, pending) {
            return failed(ErrorCode::Busy, "the notifier did not admit the notice");
        }
        if !is_core {
            if let Ok(mut coalescer) = self.notify_coalescer.lock() {
                coalescer.record_notice(&plugin.0, Instant::now());
            }
        }
        if let Some(sound) = spec.sound.as_deref() {
            // The notice is shown; a sound the player cannot queue does not undo it.
            let _ = self.play_sound(sound);
        }
        Outcome::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::PaneContentKind;
    use ilium_plugin_api::generated::types::intents::RenameNodeSpec;
    use ilium_plugin_api::generated::types::TitleSource;

    fn plugin_node(id: ilium_core::NodeId) -> NodeId {
        NodeId(id.0)
    }

    /// project / group "a" (pane 1, pane 2) / pane 3
    fn sample() -> (Tree, ilium_core::NodeId, [ilium_core::NodeId; 4]) {
        let mut tree = Tree::new();
        let project = tree
            .add_project(PathBuf::from("/tmp/ilium-intent-routing"))
            .expect("project");
        let group = tree.add_group(project, "a").expect("group");
        let first = tree
            .add_pane(group, "one", PaneContentKind::Terminal)
            .expect("pane one");
        let second = tree
            .add_pane(group, "two", PaneContentKind::Terminal)
            .expect("pane two");
        let third = tree
            .add_pane(project, "three", PaneContentKind::Terminal)
            .expect("pane three");
        (tree, project, [group, first, second, third])
    }

    fn apply(tree: &mut Tree, spec: &RestructureSpec) -> Result<ilium_core::NodeId, String> {
        let (project, plan) = mirror_restructure(tree, spec)?;
        tree.apply_project_restructure_with_title_grants(project, plan, &[], &[])
            .map_err(|error| error.to_string())?;
        Ok(project)
    }

    fn spec(operations: Vec<RestructureOp>) -> RestructureSpec {
        RestructureSpec {
            operations,
            label: "test".to_owned(),
        }
    }

    #[test]
    fn an_unchanged_mirror_is_a_plan_the_kernel_accepts() {
        let (mut tree, project, [_, first, second, third]) = sample();
        let mirror = ProjectMirror::of(&tree, project).expect("mirror");
        tree.apply_project_restructure_with_title_grants(project, mirror.plan(), &[], &[])
            .expect("the identity plan applies");
        let panes: Vec<_> = tree.panes().map(|node| node.id).collect();
        for pane in [first, second, third] {
            assert!(panes.contains(&pane));
        }
    }

    #[test]
    fn a_move_into_a_group_reaches_the_tree() {
        let (mut tree, _, [group, _, _, third]) = sample();
        let restructure = spec(vec![RestructureOp::Move(MoveNodeSpec {
            node: plugin_node(third),
            target: plugin_node(group),
            placement: Placement::InsideFirst,
        })]);
        apply(&mut tree, &restructure).expect("applied");
        let parent = tree.parent_of(third).expect("parent");
        let parent_node = tree.get(parent).expect("group node");
        assert!(parent_node.is_group());
        assert_eq!(parent_node.name, "a");
        assert_eq!(tree.children_of(parent).expect("children")[0], third);
    }

    #[test]
    fn create_group_and_ungroup_reshape_the_project() {
        let (mut tree, project, [group, first, second, third]) = sample();
        let restructure = spec(vec![
            RestructureOp::Ungroup(plugin_node(group)),
            RestructureOp::CreateGroup(CreateGroupSpec {
                parent: None,
                title: "pair".to_owned(),
                placement: Placement::InsideLast,
                children: vec![plugin_node(second), plugin_node(third)],
            }),
        ]);
        apply(&mut tree, &restructure).expect("applied");
        let top = tree.children_of(project).expect("children").to_vec();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0], first);
        let pair = tree.get(top[1]).expect("new group");
        assert_eq!(pair.name, "pair");
        assert_eq!(tree.children_of(top[1]).expect("pair"), &[second, third]);
    }

    #[test]
    fn a_user_named_group_keeps_its_identity_and_cannot_be_ungrouped() {
        let (mut tree, _, [group, _, _, third]) = sample();
        tree.rename_node(group, "Mine", None, None).expect("rename");
        let restructure = spec(vec![RestructureOp::Move(MoveNodeSpec {
            node: plugin_node(third),
            target: plugin_node(group),
            placement: Placement::InsideLast,
        })]);
        apply(&mut tree, &restructure).expect("applied");
        assert_eq!(tree.parent_of(third), Some(group));
        let refused = mirror_restructure(
            &tree,
            &spec(vec![RestructureOp::Ungroup(plugin_node(group))]),
        );
        assert!(refused.expect_err("refused").starts_with("operation 0:"));
    }

    #[test]
    fn a_move_inside_itself_is_refused_with_its_index() {
        let (tree, _, [group, first, _, _]) = sample();
        let restructure = spec(vec![
            RestructureOp::Rename(RenameNodeSpec {
                node: plugin_node(first),
                title: "x".to_owned(),
                source: TitleSource::Plugin,
                expected_revision: None,
            }),
            RestructureOp::Move(MoveNodeSpec {
                node: plugin_node(group),
                target: plugin_node(first),
                placement: Placement::After,
            }),
        ]);
        let error = mirror_restructure(&tree, &restructure).expect_err("refused");
        assert!(error.starts_with("operation 1:"), "{error}");
    }

    #[test]
    fn nodes_of_two_projects_are_refused() {
        let (mut tree, _, [_, first, _, _]) = sample();
        let other = tree
            .add_project(PathBuf::from("/tmp/ilium-intent-routing-other"))
            .expect("other project");
        let foreign = tree
            .add_pane(other, "far", PaneContentKind::Terminal)
            .expect("pane");
        let restructure = spec(vec![RestructureOp::Move(MoveNodeSpec {
            node: plugin_node(first),
            target: plugin_node(foreign),
            placement: Placement::After,
        })]);
        assert!(mirror_restructure(&tree, &restructure).is_err());
    }

    #[test]
    fn paths_stay_inside_their_scope() {
        let project = Path::new("/work/project");
        let home = Path::new("/home/someone");
        let inside = FsPath {
            scope: "project".to_owned(),
            path: "src/bin".to_owned(),
        };
        assert_eq!(
            resolve_fs_path(&inside, project, Some(home)).expect("resolved"),
            PathBuf::from("/work/project/src/bin")
        );
        let home_path = FsPath {
            scope: "home".to_owned(),
            path: "dev".to_owned(),
        };
        assert_eq!(
            resolve_fs_path(&home_path, project, Some(home)).expect("resolved"),
            PathBuf::from("/home/someone/dev")
        );
        for path in ["../etc", "/etc", "a/../../b"] {
            let escaping = FsPath {
                scope: "project".to_owned(),
                path: path.to_owned(),
            };
            assert!(
                resolve_fs_path(&escaping, project, Some(home)).is_err(),
                "{path}"
            );
        }
        let unknown = FsPath {
            scope: "cache".to_owned(),
            path: "x".to_owned(),
        };
        assert!(resolve_fs_path(&unknown, project, Some(home)).is_err());
    }

    #[test]
    fn deliveries_map_to_the_kernel_input_requests() {
        let pane = ilium_core::NodeId(4);
        assert!(matches!(
            input_request(pane, "ls".to_owned(), Delivery::Typed),
            ClientRequest::KeyInput { bytes, submission: None, .. } if bytes == b"ls"
        ));
        assert!(matches!(
            input_request(pane, "go".to_owned(), Delivery::Submitted),
            ClientRequest::SubmitTerminalText {
                source: PromptSubmissionSource::PluginIntent,
                ..
            }
        ));
        assert!(matches!(
            input_request(pane, "x".to_owned(), Delivery::Pasted),
            ClientRequest::PasteTerminalText { .. }
        ));
    }

    #[test]
    fn send_input_refuses_what_the_kernel_cannot_honour() {
        let base = SendInputSpec {
            pane: NodeId(1),
            text: "x".to_owned(),
            delivery: Delivery::Typed,
            prompt_epoch: None,
            stages: Vec::new(),
            raw: None,
            mouse: None,
            if_current: None,
            lock_user_input: false,
            journal: false,
        };
        assert!(send_input_refusal(&base).is_none());
        let staged = SendInputSpec {
            stages: (0..17)
                .map(|_| InputStage {
                    text: "a".to_owned(),
                    delay_ms: 1,
                    delivery: Delivery::Typed,
                })
                .collect(),
            ..base.clone()
        };
        assert!(send_input_refusal(&staged).is_some());
        let slow = SendInputSpec {
            stages: vec![
                InputStage {
                    text: "a".to_owned(),
                    delay_ms: 6_000,
                    delivery: Delivery::Typed,
                };
                2
            ],
            ..base.clone()
        };
        assert!(send_input_refusal(&slow).is_some());
        let locked = SendInputSpec {
            lock_user_input: true,
            ..base
        };
        assert!(send_input_refusal(&locked).is_some());
    }

    #[test]
    fn a_coalescing_key_suppresses_repeats_inside_its_window() {
        let mut coalescer = NotifyCoalescer::default();
        let start = Instant::now();
        let window = Duration::from_secs(5);
        assert!(coalescer.admit("p", "build", window, start));
        assert!(!coalescer.admit("p", "build", window, start + Duration::from_secs(1)));
        assert!(coalescer.admit("q", "build", window, start + Duration::from_secs(1)));
        assert!(coalescer.admit("p", "build", window, start + Duration::from_secs(6)));
    }

    #[test]
    fn a_plugin_shows_at_most_six_notices_per_minute() {
        let mut coalescer = NotifyCoalescer::default();
        let start = Instant::now();
        for second in 0..6 {
            let now = start + Duration::from_secs(second);
            assert!(coalescer.within_rate("p", now));
            coalescer.record_notice("p", now);
        }
        assert!(!coalescer.within_rate("p", start + Duration::from_secs(30)));
        // A refusal uses up nothing: asking again changes nothing.
        assert!(!coalescer.within_rate("p", start + Duration::from_secs(30)));
        // Another plugin has its own allowance.
        assert!(coalescer.within_rate("q", start + Duration::from_secs(30)));
        // The first notice leaves the window after a minute, which frees one place.
        let later = start + Duration::from_secs(60);
        assert!(coalescer.within_rate("p", later));
        coalescer.record_notice("p", later);
        assert!(!coalescer.within_rate("p", later));
    }

    #[test]
    fn the_alert_sound_ids_map_to_their_events() {
        use ilium_sound::SoundEvent;
        for (id, event) in [
            ("finished", SoundEvent::AgentFinished),
            ("ask", SoundEvent::ApprovalRequired),
            ("started", SoundEvent::AgentStarted),
            ("waiting", SoundEvent::WaitingBackground),
            ("task-done", SoundEvent::TaskSucceeded),
            ("task-failed", SoundEvent::TaskFailed),
        ] {
            assert_eq!(sound_event_for_id(id), Some(event));
            assert!(is_catalogue_sound(id));
        }
        assert!(is_catalogue_sound(BUNDLED_SOUND_ID));
        assert!(!is_catalogue_sound("nonexistent"));
    }
}
