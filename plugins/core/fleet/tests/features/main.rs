//! Feature tests of the core plugin `ilium.core.fleet` (manual page core-plugin-fleet.md).
//!
//! Features F-023, F-036, F-058, F-107, F-109, F-110 and F-113: `ilium panes`, `ilium broadcast`,
//! `ilium new-pane`, `POST /create_agent`, Ask for update, the API settings tab, the extension
//! points, the services, the topic, the manifest and the page itself. The page defines no
//! behaviour contract ids (`fleet-Bnn`), so none are tagged.

mod ask_for_update;
mod broadcast_cli;
mod common;
mod doc_page;
mod extension_points;
mod http_create_agent;
mod manifest;
mod new_pane_cli;
mod panes_cli;
mod services_topics;
mod settings_api;
