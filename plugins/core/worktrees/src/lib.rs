//! Core plugin `ilium.core.worktrees` (manual page core-plugin-worktrees.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::{Handler, Lifecycle, Views, ilium_plugin};
use ilium_plugin_sdk::types::{Area, UiTree, ViewTarget};
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;

/// The server side of `ilium.core.worktrees` (core-plugin-worktrees.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct WorktreesServer;

impl Lifecycle for WorktreesServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.worktrees: server lifecycle.activate")
    }
}

impl Handler for WorktreesServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.worktrees: server handler.on_events")
    }
}

/// The client side of `ilium.core.worktrees` (core-plugin-worktrees.md "runs on both sides", world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct WorktreesClient;

impl Lifecycle for WorktreesClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.worktrees: client lifecycle.activate")
    }
}

impl Handler for WorktreesClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.worktrees: client handler.on_events")
    }
}

impl Views for WorktreesClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.worktrees: client views.render")
    }
}
