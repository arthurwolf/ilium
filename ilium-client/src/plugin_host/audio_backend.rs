//! The client's [`AudioBackend`] for `ilium:host/audio`: devices come from the voice crate's
//! device catalogue, and `play` goes through the same system player as notification sounds.

use ilium_plugin_api::generated::host::audio::AudioDevice;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_host::imports::audio::AudioBackend;
use ilium_plugin_host::imports::env::CancelFlag;

/// Sound devices and the system player of this computer.
#[derive(Debug, Default, Clone, Copy)]
pub struct ClientAudio;

/// The plugin-facing records of a list of device names. Names are the stable ids the voice
/// settings use too; the catalogue does not say which device is the default.
pub fn device_records(names: Vec<String>, is_input: bool) -> Vec<AudioDevice> {
    names
        .into_iter()
        .map(|name| AudioDevice {
            id: name.clone(),
            name,
            is_input,
            is_default: false,
        })
        .collect()
}

impl AudioBackend for ClientAudio {
    fn devices(&self) -> Vec<AudioDevice> {
        let mut devices = Vec::new();
        match ilium_voice::available_input_devices() {
            Ok(names) => devices.extend(device_records(names, true)),
            Err(error) => tracing::debug!(%error, "plugin audio: no input devices listed"),
        }
        match ilium_voice::available_output_devices() {
            Ok(names) => devices.extend(device_records(names, false)),
            Err(error) => tracing::debug!(%error, "plugin audio: no output devices listed"),
        }
        devices
    }

    fn play(&self, wav: &[u8], cancel: &CancelFlag) -> Result<(), PluginError> {
        if cancel.is_set() {
            return Err(PluginError::new(ErrorCode::Cancelled, "the instance ended"));
        }
        ilium_sound::play_prepared_wav(wav)
            .map_err(|error| PluginError::new(ErrorCode::Io, error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::device_records;

    #[test]
    fn device_names_become_records_with_their_direction() {
        let records = device_records(vec!["Built-in Microphone".to_owned()], true);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "Built-in Microphone");
        assert_eq!(records[0].name, "Built-in Microphone");
        assert!(records[0].is_input);
        assert!(!records[0].is_default);
    }
}
