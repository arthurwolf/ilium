//! Host implementation of `ilium:host/settings` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostSettings` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::settings::{
    ChoiceOption, HostSettings, ItemState, SettingsRef,
};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::PluginId;
use ilium_plugin_api::generated::types::text::RichText;
use ilium_plugin_api::generated::types::ui::{CatalogItem, MeterSegment, RowState};

/// Host side of the `settings` interface.
#[derive(Debug)]
pub struct SettingsImport;

impl HostSettings for SettingsImport {
    fn get_json(&mut self) -> String {
        unimplemented!("host::imports::settings::get_json")
    }

    fn set_json(&mut self, _patch: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_json")
    }

    fn set_info(&mut self, _key: String, _text: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_info")
    }

    fn set_choices(
        &mut self,
        _key: String,
        _options: Vec<ChoiceOption>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_choices")
    }

    fn set_row_note(&mut self, _key: String, _note: Option<String>) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_row_note")
    }

    fn set_row_state(&mut self, _key: String, _state: RowState) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_row_state")
    }

    fn set_item_states(&mut self, _key: String, _items: Vec<ItemState>) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_item_states")
    }

    fn set_choice_items(
        &mut self,
        _key: String,
        _items: Vec<CatalogItem>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_choice_items")
    }

    fn set_info_rich(&mut self, _key: String, _text: RichText) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_info_rich")
    }

    fn set_meter(&mut self, _id: String, _segments: Vec<MeterSegment>) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_meter")
    }

    fn open(&mut self, _target: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::open")
    }

    fn list_settings(&mut self) -> Vec<SettingsRef> {
        unimplemented!("host::imports::settings::list_settings")
    }

    fn get_plugin_json(&mut self, _plugin: PluginId) -> Result<String, PluginError> {
        unimplemented!("host::imports::settings::get_plugin_json")
    }

    fn set_plugin_json(&mut self, _plugin: PluginId, _patch: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_plugin_json")
    }
}
