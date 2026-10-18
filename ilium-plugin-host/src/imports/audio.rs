//! Host implementation of `ilium:host/audio` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostAudio` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::audio::{AudioDevice, HostAudio};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `audio` interface.
#[derive(Debug)]
pub struct AudioImport;

impl HostAudio for AudioImport {
    fn devices(&mut self) -> Vec<AudioDevice> {
        unimplemented!("host::imports::audio::devices")
    }

    fn open_capture(
        &mut self,
        _device: Option<String>,
        _sample_rate: u32,
        _channels: u8,
    ) -> Result<u64, PluginError> {
        unimplemented!("host::imports::audio::open_capture")
    }

    fn open_loopback(&mut self, _sample_rate: u32, _channels: u8) -> Result<u64, PluginError> {
        unimplemented!("host::imports::audio::open_loopback")
    }

    fn close_capture(&mut self, _capture: u64) {
        unimplemented!("host::imports::audio::close_capture")
    }

    fn play(
        &mut self,
        _samples: Vec<i16>,
        _sample_rate: u32,
        _channels: u8,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::audio::play")
    }
}
