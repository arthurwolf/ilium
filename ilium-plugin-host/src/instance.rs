//! One side instance of a plugin: activation, calls and teardown (reference-runtime-model.md).

use ilium_plugin_api::generated::exports::hook_handler::{HookCall, HookReply};
use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::layout::Area;
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
use ilium_plugin_api::PluginError;

use crate::calls::Trap;
use ilium_plugin_api::generated::exports::annotator::LineAnnotation;
use ilium_plugin_api::generated::exports::menu_provider::{MenuContext, MenuEntry};
use ilium_plugin_api::generated::exports::service_provider::{ServiceCall, ServiceReply};
use ilium_plugin_api::generated::exports::token_provider::Token;
use ilium_plugin_api::generated::exports::virtual_rows_provider::VirtualRowChild;
use ilium_plugin_api::generated::host::catalogs::{CatalogPage, CatalogQuery};
use ilium_plugin_api::generated::types::ids::BufferId;

/// A call into a guest export other than events and render (reference-wit.md section 8).
#[derive(Debug, Clone)]
pub enum ExportCall {
    /// `hook-handler.handle`.
    Hook(HookCall),
    /// `job-runner.run`.
    Job {
        /// The job kind.
        kind: String,
        /// The job input.
        input: Vec<u8>,
    },
    /// `migrator.migrate`.
    Migrate {
        /// The data version being migrated from.
        from_version: u32,
        /// The stored bytes.
        data: Vec<u8>,
    },
    /// `annotator.annotate` (reference-wit.md section 8.8).
    Annotate {
        /// The buffer.
        buffer: BufferId,
        /// The buffer revision.
        revision: u64,
        /// The first line given.
        first_line: u32,
        /// The lines given.
        lines: Vec<String>,
    },
    /// `token-provider.tokens` (reference-wit.md section 8.13).
    Tokens {
        /// The buffer.
        buffer: BufferId,
        /// The first line given.
        first_line: u32,
        /// The lines given.
        lines: Vec<String>,
        /// The tokenizer state carried in.
        state_in: Option<Vec<u8>>,
    },
    /// `catalog-provider.query-catalog` (reference-wit.md section 8.10).
    QueryCatalog(CatalogQuery),
    /// `menu-provider.menu-items` (reference-wit.md section 8.11).
    MenuItems(MenuContext),
    /// `virtual-rows-provider.list-virtual-children` (reference-wit.md section 8.12).
    ListVirtualChildren {
        /// The row kind.
        row_kind: String,
        /// The parent row.
        parent: String,
        /// The page cursor.
        cursor: Option<String>,
    },
    /// `service-provider.handle` (reference-wit.md section 8.5).
    ServiceHandle(ServiceCall),
}

/// The reply of an [`ExportCall`].
#[derive(Debug, Clone)]
pub enum ExportReply {
    /// Reply of a hook.
    Hook(HookReply),
    /// Reply of a job.
    Job(Result<Vec<u8>, PluginError>),
    /// Reply of a migration.
    Migrated(Result<Vec<u8>, PluginError>),
    /// Reply of `hook-handler.handle-hook` with its error branch (reference-wit.md section 8.14).
    HookResult(Result<HookReply, PluginError>),
    /// Reply of `annotator.annotate`.
    Annotations(Vec<LineAnnotation>),
    /// Reply of `token-provider.tokens`: tokens and the state carried out.
    Tokens(Vec<Token>, Option<Vec<u8>>),
    /// Reply of `catalog-provider.query-catalog`.
    CatalogPage(Result<CatalogPage, PluginError>),
    /// Reply of `menu-provider.menu-items`.
    MenuEntries(Vec<MenuEntry>),
    /// Reply of `virtual-rows-provider.list-virtual-children`: children and the next cursor.
    VirtualChildren(Result<(Vec<VirtualRowChild>, Option<String>), PluginError>),
    /// Reply of `service-provider.handle`.
    ServiceReply(ServiceReply),
}

/// Runtime adapter for one guest instance. Implementations own the actual guest
/// store and enforce engine interruption and import authorization. This trait
/// does not itself instantiate WebAssembly.
pub trait GuestExports: Send {
    /// Distinguishes a guest error result from an engine trap.
    fn activate(&mut self, activation: Activation) -> Result<Result<(), PluginError>, Trap>;
    /// A trap drops saved state without counting a new lifecycle failure.
    fn deactivate(&mut self, reason: DeactivationReason) -> Result<Option<Vec<u8>>, Trap>;
    /// Returns submissions only after the export has returned successfully.
    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap>;
    /// Calls the view export under its runtime deadline and pure-import guard.
    fn render(&mut self, view: ViewTarget, area: Area) -> Result<UiTree, Trap>;
    /// Calls an optional export under its runtime-specific guard.
    fn call_export(&mut self, call: ExportCall) -> Result<ExportReply, Trap>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstanceKind {
    Main,
    Job,
    Migration,
}

/// Owns one guest. Mutable access serializes calls (ABI-049); a trap or
/// deactivation consumes the guest, so its memory cannot survive teardown.
pub struct PluginInstance {
    state: InstanceState,
    guest: Option<Box<dyn GuestExports>>,
    kind: InstanceKind,
    last_trap: Option<Trap>,
}

impl std::fmt::Debug for PluginInstance {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PluginInstance")
            .field("state", &self.state)
            .field("kind", &self.kind)
            .field("has_guest", &self.guest.is_some())
            .field("last_trap", &self.last_trap)
            .finish()
    }
}

impl PluginInstance {
    /// Takes a runtime-created guest adapter; does not compile or link Wasm.
    pub fn from_guest(guest: Box<dyn GuestExports>) -> Self {
        Self::with_kind(guest, InstanceKind::Main)
    }

    /// A job has its own unactivated instance and no event handler.
    pub fn for_job(guest: Box<dyn GuestExports>) -> Self {
        Self::with_kind(guest, InstanceKind::Job)
    }

    /// A migration runs before activation in its own instance.
    pub fn for_migration(guest: Box<dyn GuestExports>) -> Self {
        Self::with_kind(guest, InstanceKind::Migration)
    }

    fn with_kind(guest: Box<dyn GuestExports>, kind: InstanceKind) -> Self {
        Self { state: InstanceState::Inactive, guest: Some(guest), kind, last_trap: None }
    }

    /// Calls activate once. Restore data over the ABI cap is never delivered.
    pub fn activate(&mut self, mut activation: Activation) -> Result<(), PluginError> {
        if self.kind != InstanceKind::Main || self.state != InstanceState::Inactive || self.guest.is_none() {
            return Err(PluginError::new(ilium_plugin_api::ErrorCode::Conflict, "instance cannot activate in its current state"));
        }
        if activation.restored.as_ref().is_some_and(|bytes| bytes.len() > ilium_plugin_api::abi::RESTORE_BYTES_MAX) {
            activation.restored = None;
        }
        self.state = InstanceState::Activating;
        let result = self.invoke("lifecycle", "activate", |guest| guest.activate(activation));
        match result {
            Ok(Ok(())) => { self.state = InstanceState::Active; Ok(()) }
            Ok(Err(error)) => {
                self.end_failed();
                Err(error)
            }
            Err(trap) => Err(PluginError::with_prefix(ilium_plugin_api::ErrorCode::Internal, &trap.code, "activation trapped")),
        }
    }

    /// Tears down once; traps and oversized saved data return no restore bytes.
    /// The lifecycle owner logs a dropped restore and decides whether to restart.
    pub fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        if self.state != InstanceState::Active || self.kind != InstanceKind::Main { return None; }
        self.state = InstanceState::Deactivating;
        let saved = self.invoke("lifecycle", "deactivate", |guest| guest.deactivate(reason)).ok().flatten();
        self.guest = None;
        self.state = InstanceState::Stopped;
        saved.filter(|bytes| bytes.len() <= ilium_plugin_api::abi::RESTORE_BYTES_MAX)
    }

    /// A caller must split its queue into batches of at most 256 events. A bad
    /// host batch is rejected without calling or destroying the guest.
    pub fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        self.require_active("handler", "on-events")?;
        if batch.len() > ilium_plugin_api::abi::MAX_EVENTS_PER_BATCH {
            return Err(boundary_trap("trap.abi", "handler", "on-events"));
        }
        let submissions = self.invoke("handler", "on-events", |guest| guest.on_events(batch))?;
        if submissions.len() > ilium_plugin_api::abi::SUBMISSION_BATCH_LIMIT {
            let trap = boundary_trap("trap.submission-batch", "handler", "on-events");
            self.record_trap(&trap);
            return Err(trap);
        }
        Ok(submissions)
    }

    /// The adapter guards pure imports and engine deadlines. Validate structural
    /// ABI invariants before handing a returned tree to any renderer.
    pub fn render(&mut self, view: ViewTarget, area: Area) -> Result<UiTree, Trap> {
        self.require_active("views", "render")?;
        let tree = self.invoke("views", "render", |guest| guest.render(view, area))?;
        if !valid_ui_tree(&tree) {
            let trap = boundary_trap("trap.invalid-ui-tree", "views", "render");
            self.record_trap(&trap);
            return Err(trap);
        }
        Ok(tree)
    }

    /// Jobs and migrations use fresh dedicated instances; every other optional
    /// export is available only while the main instance is active.
    pub fn call_export(&mut self, call: ExportCall) -> Result<ExportReply, Trap> {
        let allowed = match (&call, self.kind) {
            (ExportCall::Job { .. }, InstanceKind::Job)
            | (ExportCall::Migrate { .. }, InstanceKind::Migration) => self.state == InstanceState::Inactive && self.guest.is_some(),
            (ExportCall::Job { .. } | ExportCall::Migrate { .. }, _) => false,
            (_, InstanceKind::Main) => self.state == InstanceState::Active,
            _ => false,
        };
        if !allowed { return Err(boundary_trap("trap.abi", "exports", "call-export")); }
        let expected = reply_kind(&call);
        let result = self.invoke("exports", "call-export", |guest| guest.call_export(call));
        let result = result.and_then(|reply| {
            if reply_matches(expected, &reply) { Ok(reply) } else {
                let trap = boundary_trap("trap.abi", "exports", "call-export");
                self.record_trap(&trap);
                Err(trap)
            }
        });
        if self.kind != InstanceKind::Main {
            self.guest = None;
            self.state = InstanceState::Stopped;
        }
        result
    }

    /// Last trapped call, including activate traps mapped to PluginError.
    pub fn last_trap(&self) -> Option<&Trap> { self.last_trap.as_ref() }

    /// The current lifecycle state. Restart scheduling belongs to the owner.
    pub fn state(&self) -> InstanceState { self.state }

    fn require_active(&self, interface: &str, function: &str) -> Result<(), Trap> {
        if self.kind == InstanceKind::Main && self.state == InstanceState::Active && self.guest.is_some() { Ok(()) }
        else { Err(boundary_trap("trap.abi", interface, function)) }
    }

    fn invoke<T>(&mut self, interface: &str, function: &str, call: impl FnOnce(&mut dyn GuestExports) -> Result<T, Trap>) -> Result<T, Trap> {
        let Some(guest) = self.guest.as_mut() else { return Err(boundary_trap("trap.abi", interface, function)); };
        // A native adapter must not unwind through the host. Wasm adapters
        // return engine traps through Result; both paths discard their result.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(guest.as_mut())))
            .unwrap_or_else(|_| Err(boundary_trap("trap.panic", interface, function)));
        if let Err(trap) = &result { self.record_trap(trap); }
        result
    }

    fn record_trap(&mut self, trap: &Trap) {
        self.last_trap = Some(trap.clone());
        self.end_failed();
    }

    fn end_failed(&mut self) {
        self.guest = None;
        self.state = InstanceState::RestartScheduled;
    }
}

#[cfg(test)]
mod tests {
    use super::{InstanceKind, PluginInstance};
    use ilium_plugin_api::generated::host::plugins::InstanceState;
    use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};

    /// Builds an instance in the given state. The stub holds only its state, so no guest is involved.
    fn instance_in_state(state: InstanceState) -> PluginInstance {
        PluginInstance { state, guest: None, kind: InstanceKind::Main, last_trap: None }
    }

    /// REQ: LIF-005
    #[test]
    fn state_reports_each_of_the_nine_instance_states() {
        assert_eq!(InstanceState::CASE_NAMES.len(), 9);
        let every_state = [
            InstanceState::Inactive,
            InstanceState::Activating,
            InstanceState::WaitingDependency,
            InstanceState::Active,
            InstanceState::Deactivating,
            InstanceState::Failed,
            InstanceState::RestartScheduled,
            InstanceState::StoppedByFailure,
            InstanceState::Stopped,
        ];
        for state in every_state {
            assert_eq!(instance_in_state(state).state(), state);
        }
    }

    /// REQ: LIF-005
    #[test]
    fn activate_moves_an_activating_instance_to_active() {
        let mut instance = instance_in_state(InstanceState::Activating);
        let activation = Activation {
            api: "ilium:plugin@0.1.0".to_string(),
            instance: 1,
            side: "server".to_string(),
            ..Default::default()
        };
        let result = instance.activate(activation);
        assert!(result.is_ok(), "activate of a valid activation must return ok");
        assert_eq!(instance.state(), InstanceState::Active);
    }

    /// REQ: LIF-006
    #[test]
    fn on_events_on_a_stopped_instance_delivers_no_submissions() {
        let mut instance = instance_in_state(InstanceState::Stopped);
        let submissions = instance
            .on_events(Vec::new())
            .ok()
            .expect("an event to a stopped instance is dropped, not reported as a trap");
        assert!(submissions.is_empty());
    }

    /// REQ: ABI-045
    #[test]
    fn on_events_with_an_empty_batch_returns_no_submissions() {
        let mut instance = instance_in_state(InstanceState::Active);
        let submissions = instance
            .on_events(Vec::new())
            .ok()
            .expect("an empty batch is within the 256-event limit");
        assert!(submissions.is_empty());
    }

    /// REQ: LIF-014
    #[test]
    fn deactivate_of_an_active_instance_leaves_the_active_state() {
        let mut instance = instance_in_state(InstanceState::Active);
        let _saved_bytes: Option<Vec<u8>> = instance.deactivate(DeactivationReason::Disabled);
        assert_ne!(instance.state(), InstanceState::Active);
    }
}

fn boundary_trap(code: &'static str, interface: &str, function: &str) -> Trap {
    Trap { code: ilium_plugin_api::Code::from_static(code), interface: interface.to_owned(), function: function.to_owned() }
}

#[derive(Clone, Copy)]
enum ReplyKind { Hook, Job, Migration, Annotations, Tokens, Catalog, Menu, VirtualChildren, Service }

fn reply_kind(call: &ExportCall) -> ReplyKind {
    match call {
        ExportCall::Hook(_) => ReplyKind::Hook,
        ExportCall::Job { .. } => ReplyKind::Job,
        ExportCall::Migrate { .. } => ReplyKind::Migration,
        ExportCall::Annotate { .. } => ReplyKind::Annotations,
        ExportCall::Tokens { .. } => ReplyKind::Tokens,
        ExportCall::QueryCatalog(_) => ReplyKind::Catalog,
        ExportCall::MenuItems(_) => ReplyKind::Menu,
        ExportCall::ListVirtualChildren { .. } => ReplyKind::VirtualChildren,
        ExportCall::ServiceHandle(_) => ReplyKind::Service,
    }
}

fn reply_matches(kind: ReplyKind, reply: &ExportReply) -> bool {
    matches!((kind, reply),
        (ReplyKind::Hook, ExportReply::Hook(_) | ExportReply::HookResult(_)) |
        (ReplyKind::Job, ExportReply::Job(_)) | (ReplyKind::Migration, ExportReply::Migrated(_)) |
        (ReplyKind::Annotations, ExportReply::Annotations(_)) | (ReplyKind::Tokens, ExportReply::Tokens(_, _)) |
        (ReplyKind::Catalog, ExportReply::CatalogPage(_)) | (ReplyKind::Menu, ExportReply::MenuEntries(_)) |
        (ReplyKind::VirtualChildren, ExportReply::VirtualChildren(_)) | (ReplyKind::Service, ExportReply::ServiceReply(_)))
}

fn valid_ui_tree(tree: &UiTree) -> bool {
    use ilium_plugin_api::generated::types::ui::Widget;
    if tree.root != 0 || tree.nodes.is_empty() || tree.nodes.len() > 4096 { return false; }
    let mut parents = vec![0usize; tree.nodes.len()];
    let mut depths = vec![1usize; tree.nodes.len()];
    let mut keys = std::collections::HashSet::new();
    for (index, node) in tree.nodes.iter().enumerate() {
        if let Some(key) = &node.key {
            if key.is_empty() || key.chars().count() > 64 || !keys.insert(key) { return false; }
        }
        if let Widget::Layout(layout) = &node.widget {
            if layout.constraints.len() != node.children.len() { return false; }
        }
        for &child in &node.children {
            let child = child as usize;
            if child <= index || child >= tree.nodes.len() { return false; }
            parents[child] += 1;
            depths[child] = depths[index] + 1;
            if parents[child] > 1 || depths[child] > 32 { return false; }
        }
    }
    parents.iter().skip(1).all(|count| *count == 1)
}
