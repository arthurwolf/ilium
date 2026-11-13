//! Host implementation of `ilium:host/media` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostMedia` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::media::{HostMedia, VideoInfo};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::paths::FsPath;

/// Host side of the `media` interface.
#[derive(Debug)]
pub struct MediaImport;

impl HostMedia for MediaImport {
    fn open_video(&mut self, _path: FsPath) -> Result<(u64, VideoInfo), PluginError> {
        unimplemented!("host::imports::media::open_video")
    }

    fn open_video_bytes(
        &mut self,
        _bytes: Vec<u8>,
        _mime: String,
    ) -> Result<(u64, VideoInfo), PluginError> {
        unimplemented!("host::imports::media::open_video_bytes")
    }

    fn open_video_range(
        &mut self,
        _path: FsPath,
        _start_millis: u64,
        _end_millis: Option<u64>,
    ) -> Result<(u64, VideoInfo), PluginError> {
        unimplemented!("host::imports::media::open_video_range")
    }

    fn next_frame(
        &mut self,
        _video: u64,
        _width: u32,
        _height: u32,
    ) -> Result<Option<Vec<u8>>, PluginError> {
        unimplemented!("host::imports::media::next_frame")
    }

    fn seek(&mut self, _video: u64, _millis: u64) -> Result<(), PluginError> {
        unimplemented!("host::imports::media::seek")
    }

    fn close_video(&mut self, _video: u64) {
        unimplemented!("host::imports::media::close_video")
    }
}
