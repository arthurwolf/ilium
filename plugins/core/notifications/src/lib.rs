//! Core plugin `ilium.core.notifications` (manual page core-plugin-notifications.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{Area, UiTree, ViewTarget};
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, Views};

/// The server side of `ilium.core.notifications` (core-plugin-notifications.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct NotificationsServer;

impl Lifecycle for NotificationsServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.notifications: server lifecycle.activate")
    }
}

impl Handler for NotificationsServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.notifications: server handler.on_events")
    }
}

/// The client side of `ilium.core.notifications` (core-plugin-notifications.md "runs on both sides", world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct NotificationsClient;

impl Lifecycle for NotificationsClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.notifications: client lifecycle.activate")
    }
}

impl Handler for NotificationsClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.notifications: client handler.on_events")
    }
}

impl Views for NotificationsClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.notifications: client views.render")
    }
}
