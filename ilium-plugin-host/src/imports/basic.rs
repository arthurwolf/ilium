//! Checked host implementation of `ilium:primitives/basic`.
use std::sync::{Arc, Mutex, MutexGuard};
use ilium_plugin_api::generated::primitives::basic::HostBasic;
use ilium_plugin_api::generated::types::{errors::PluginError, ids::NodeId, text::StyledText, tree::AgentActivity};
use ilium_plugin_api::{ErrorCode, grants::Grant};

/// A backend must resolve the content kind through its authoritative kind registry,
/// require its exact owner and Basic primitive, and mutate under the same transaction.
/// The caller holds the shared grant gate through the complete backend operation.
/// Runtime kernels must supply an adapter; implementing this trait does not run a guest.
pub trait BasicBackend: Send + Sync {
    fn apply_basic(&self, plugin: &str, pane: NodeId, change: BasicChange) -> Result<(), PluginError>;
    fn basic_size(&self, plugin: &str, pane: NodeId) -> Option<(u16, u16)>;
}
pub enum BasicChange { Title(String), Status(StyledText), Focus, Activity(AgentActivity), Dirty(bool) }

pub struct BasicImport<'a> {
    backend: Arc<dyn BasicBackend>,
    plugin: String,
    // Every grant mutation must use this same gate. Lock order is authorization,
    // then backend transaction; a backend must never acquire this gate itself.
    grants: Arc<Mutex<Vec<Grant>>>,
    refusal: Box<dyn Fn(&str) + 'a>,
}
impl<'a> BasicImport<'a> {
    pub fn new(backend: Arc<dyn BasicBackend>, plugin: String, grants: Arc<Mutex<Vec<Grant>>>, refusal: impl Fn(&str) + 'a) -> Self {
        Self { backend, plugin, grants, refusal: Box::new(refusal) }
    }
    fn authorized(&self, function: &str) -> Result<MutexGuard<'_, Vec<Grant>>, PluginError> {
        let grants = self.grants.lock().map_err(|_| PluginError::new(ErrorCode::Internal, "authorization gate poisoned"))?;
        for grant in ["panes.provide-kind", "tree.metadata"] {
            if !Grant::parse(grant).is_ok_and(|needed| grants.iter().any(|current| current.covers(&needed))) {
                (self.refusal)(&format!("refused {grant} by basic.{function} (not-permitted)"));
                return Err(PluginError::new(ErrorCode::NotPermitted, "missing basic primitive grant"));
            }
        }
        Ok(grants)
    }
    fn apply(&self, pane: NodeId, function: &str, change: BasicChange) -> Result<(), PluginError> {
        let _authorization = self.authorized(function)?;
        self.apply_backend(pane, function, change)
    }
    fn apply_backend(&self, pane: NodeId, function: &str, change: BasicChange) -> Result<(), PluginError> {
        let result = self.backend.apply_basic(&self.plugin, pane, change);
        if result.as_ref().is_err_and(|error| error.code == ErrorCode::NotPermitted) {
            (self.refusal)(&format!("refused basic.{function}: not-permitted"));
        }
        result
    }
}
impl HostBasic for BasicImport<'_> {
    fn set_title(&mut self, pane: NodeId, title: String) -> Result<(), PluginError> {
        let _authorization = self.authorized("set-title")?;
        if title.chars().count() > 256 { return Err(PluginError::new(ErrorCode::InvalidArgument, "title exceeds 256 characters")); }
        self.apply_backend(pane, "set-title", BasicChange::Title(title))
    }
    fn set_status(&mut self, pane: NodeId, status: StyledText) -> Result<(), PluginError> {
        let _authorization = self.authorized("set-status")?;
        if status.iter().map(|span| span.text.chars().count()).sum::<usize>() > 256 || status.iter().any(|span| span.text.chars().any(|c| matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'))) {
            return Err(PluginError::new(ErrorCode::InvalidArgument, "status must be one line of at most 256 characters"));
        }
        self.apply_backend(pane, "set-status", BasicChange::Status(status))
    }
    fn request_focus(&mut self, pane: NodeId) -> Result<(), PluginError> { self.apply(pane, "request-focus", BasicChange::Focus) }
    fn size(&mut self, pane: NodeId) -> Option<(u16,u16)> { let _authorization = self.authorized("size").ok()?; self.backend.basic_size(&self.plugin,pane) }
    fn report_activity(&mut self,pane: NodeId,activity: AgentActivity)->Result<(),PluginError>{ self.apply(pane,"report-activity",BasicChange::Activity(activity)) }
    fn set_dirty(&mut self,pane: NodeId,dirty:bool)->Result<(),PluginError>{self.apply(pane,"set-dirty",BasicChange::Dirty(dirty))}
}
