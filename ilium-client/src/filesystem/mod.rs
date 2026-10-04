//! Client-owned filesystem preparation and acknowledged durability adapters.
mod app;
pub mod editor;
pub mod editors;
pub mod ordered;
pub(crate) mod plugin_permissions;

pub(crate) mod board_app;
pub(crate) mod boards;
pub mod configuration;
mod configuration_app;
pub mod configurations;

#[cfg(test)]
mod tests;

pub(crate) use configuration_app::ConfigurationAdmission;

pub(crate) mod explorer;

mod integration_app;
pub(crate) mod integrations;

pub(crate) mod sidebar;
pub(crate) mod transcript_baseline;

pub(crate) mod plugin_permission_controller;
