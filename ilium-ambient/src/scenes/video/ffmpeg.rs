//! `FrameSource` backed by the external ffmpeg and ffprobe programs.
//!
//! Every child is registered in the scene's `ChildSlot` so the scene's `Drop`
//! can kill it while the worker is blocked reading its pipe, and every child
//! is killed and reaped when its stream is dropped, so nothing lingers as a
//! zombie however a clip ends.

use super::command::{
    ffmpeg_spec, ffprobe_spec, parse_duration, ChildControl, ChildSlot, CommandRunner, PlayRequest,
    StreamOptions, FFMPEG_PROGRAM,
};
use super::discover::MediaInput;
use super::player::{FrameSource, FrameStream, OpenError};
use std::io::{self, Read};
use std::sync::Arc;
use std::time::Duration;

/// A killed child is normally gone within milliseconds.
const REAP_TIMEOUT: Duration = Duration::from_secs(2);
/// Duration probes are cut off after this many bytes of output.
const PROBE_OUTPUT_LIMIT: u64 = 4096;

pub struct FfmpegSource {
    runner: Arc<dyn CommandRunner>,
    slot: ChildSlot,
    options: StreamOptions,
}

impl FfmpegSource {
    pub fn new(runner: Arc<dyn CommandRunner>, slot: ChildSlot, options: StreamOptions) -> Self {
        Self {
            runner,
            slot,
            options,
        }
    }
}

impl FrameSource for FfmpegSource {
    fn probe_duration(&mut self, input: &MediaInput) -> Option<f64> {
        let mut child = self.runner.spawn(&ffprobe_spec(input)).ok()?;
        if !self.slot.register(Arc::clone(&child.control)) {
            child.control.kill();
            child.control.reap(REAP_TIMEOUT);
            return None;
        }
        let mut output = String::new();
        let read = (&mut child.stdout)
            .take(PROBE_OUTPUT_LIMIT)
            .read_to_string(&mut output);
        child.control.kill();
        child.control.reap(REAP_TIMEOUT);
        self.slot.clear();
        read.ok().and_then(|_| parse_duration(&output))
    }

    fn open(&mut self, request: &PlayRequest) -> Result<Box<dyn FrameStream>, OpenError> {
        let spec = ffmpeg_spec(request, &self.options);
        let child = self.runner.spawn(&spec).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                OpenError::ToolMissing(FFMPEG_PROGRAM)
            } else {
                OpenError::Failed(format!("cannot start {FFMPEG_PROGRAM}: {error}"))
            }
        })?;
        if !self.slot.register(Arc::clone(&child.control)) {
            child.control.kill();
            child.control.reap(REAP_TIMEOUT);
            return Err(OpenError::Failed("stopping".to_owned()));
        }
        Ok(Box::new(FfmpegStream {
            stdout: child.stdout,
            control: child.control,
            slot: self.slot.clone(),
            frame_bytes: request.frame_bytes(self.options.pixel_format),
        }))
    }
}

pub struct FfmpegStream {
    stdout: Box<dyn Read + Send>,
    control: Arc<dyn ChildControl>,
    slot: ChildSlot,
    frame_bytes: usize,
}

impl FrameStream for FfmpegStream {
    fn read_frame(&mut self, buffer: &mut Vec<u8>) -> io::Result<bool> {
        buffer.resize(self.frame_bytes, 0);
        match self.stdout.read_exact(buffer) {
            Ok(()) => Ok(true),
            // Clean end of clip, or the child was killed mid-frame.
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
            Err(error) => Err(error),
        }
    }
}

impl Drop for FfmpegStream {
    fn drop(&mut self) {
        self.control.kill();
        self.control.reap(REAP_TIMEOUT);
        self.slot.clear();
    }
}
