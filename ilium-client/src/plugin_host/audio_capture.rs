//! Recording for the `open-capture` and `open-loopback` calls of `ilium:host/audio`.
//!
//! The client records through the trusted installed `parec` helper (PulseAudio, or the Pulse
//! server of PipeWire), the same helper the animation plugins use for their audio. `parec`
//! converts to the rate and channel count the plugin asked for, so the samples arrive as
//! interleaved little-endian 16-bit PCM and are only cut into 20 ms chunks here. A reader thread
//! owned by the [`HelperCapture`] hands the chunks to the import; dropping the capture stops the
//! helper and joins the reader.
//!
//! The default microphone is `@DEFAULT_SOURCE@` and the loopback is `@DEFAULT_MONITOR@`, the
//! monitor of the default output. A device id from `devices()` is passed as the Pulse source
//! name; a name the audio server does not know makes the helper exit, and the capture then
//! delivers no more frames.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_host::imports::audio::{CaptureSource, CaptureStream, FrameSink};

/// Longest device name passed to the helper.
const DEVICE_NAME_LIMIT: usize = 256;
/// Bytes read from the helper at a time.
const READ_BYTES: usize = 8 * 1024;

/// Samples in one 20 ms chunk: `sample_rate / 50 * channels`.
pub fn chunk_samples(sample_rate: u32, channels: u8) -> usize {
    usize::try_from(sample_rate / 50).unwrap_or(usize::MAX) * usize::from(channels)
}

/// The `parec` arguments that record `source` at `sample_rate` with `channels` channels, or
/// `None` when a device name is not a plain source name.
pub fn helper_arguments(
    source: &CaptureSource,
    sample_rate: u32,
    channels: u8,
) -> Option<Vec<String>> {
    let device = match source {
        CaptureSource::DefaultInput => "@DEFAULT_SOURCE@",
        CaptureSource::Loopback => "@DEFAULT_MONITOR@",
        CaptureSource::Device(name) => {
            let plain = !name.is_empty()
                && name.len() <= DEVICE_NAME_LIMIT
                && !name.starts_with('-')
                && name.bytes().all(|byte| byte.is_ascii_graphic());
            if !plain {
                return None;
            }
            name.as_str()
        }
    };
    Some(vec![
        "-d".to_owned(),
        device.to_owned(),
        "--format=s16le".to_owned(),
        format!("--rate={sample_rate}"),
        format!("--channels={channels}"),
        "--latency-msec=20".to_owned(),
        "--client-name=ilium-plugins".to_owned(),
        "--stream-name=plugin-capture".to_owned(),
    ])
}

/// Cuts a little-endian 16-bit byte stream that arrives in arbitrary pieces into chunks of a
/// fixed number of samples.
#[derive(Debug)]
pub struct ChunkAssembler {
    chunk: usize,
    odd_byte: Option<u8>,
    samples: Vec<i16>,
}

impl ChunkAssembler {
    /// An assembler of chunks of `chunk` samples (at least one).
    pub fn new(chunk: usize) -> Self {
        let chunk = chunk.max(1);
        Self {
            chunk,
            odd_byte: None,
            samples: Vec::with_capacity(chunk),
        }
    }

    /// Adds `bytes` and hands every completed chunk to `emit`.
    pub fn push(&mut self, bytes: &[u8], emit: &mut dyn FnMut(Vec<i16>)) {
        let mut rest = bytes;
        if let Some(low) = self.odd_byte.take() {
            let Some((&high, tail)) = rest.split_first() else {
                self.odd_byte = Some(low);
                return;
            };
            self.add(i16::from_le_bytes([low, high]), emit);
            rest = tail;
        }
        let mut pairs = rest.chunks_exact(2);
        for pair in &mut pairs {
            self.add(i16::from_le_bytes([pair[0], pair[1]]), emit);
        }
        self.odd_byte = pairs.remainder().first().copied();
    }

    fn add(&mut self, sample: i16, emit: &mut dyn FnMut(Vec<i16>)) {
        self.samples.push(sample);
        if self.samples.len() == self.chunk {
            let full = std::mem::replace(&mut self.samples, Vec::with_capacity(self.chunk));
            emit(full);
        }
    }
}

/// A running `parec` recording and the thread that reads it.
pub struct HelperCapture {
    child: Child,
    reader: Option<JoinHandle<()>>,
}

impl Drop for HelperCapture {
    fn drop(&mut self) {
        // Killing the helper closes its output, which ends the reader's loop.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn refuse(code: ErrorCode, message: impl Into<String>) -> PluginError {
    PluginError::new(code, message)
}

/// Starts recording `source` and hands its 20 ms chunks to `frames`.
pub fn start(
    source: &CaptureSource,
    sample_rate: u32,
    channels: u8,
    mut frames: FrameSink,
) -> Result<CaptureStream, PluginError> {
    let arguments = helper_arguments(source, sample_rate, channels)
        .ok_or_else(|| refuse(ErrorCode::NotFound, "the device id is not a source name"))?;
    // Without the helper there is no monitor source either, which the manual reports as
    // `unsupported` for `open-loopback`; a microphone capture says the same.
    let program =
        ilium_platform::audio_backend::trusted_pulse_capture_helper().map_err(|error| {
            refuse(
                ErrorCode::Unsupported,
                format!("no audio capture helper: {error}"),
            )
        })?;
    let mut child = Command::new(&program)
        .args(&arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| refuse(ErrorCode::Io, format!("cannot start parec: {error}")))?;
    let Some(mut output) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(refuse(ErrorCode::Internal, "parec gave no output pipe"));
    };
    let mut assembler = ChunkAssembler::new(chunk_samples(sample_rate, channels));
    let reader = std::thread::Builder::new()
        .name("plugin-audio-capture".to_owned())
        .spawn(move || {
            let mut buffer = vec![0u8; READ_BYTES];
            loop {
                match output.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => assembler.push(&buffer[..count], &mut *frames),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        tracing::debug!(%error, "plugin audio capture: helper output failed");
                        break;
                    }
                }
            }
        });
    match reader {
        Ok(reader) => Ok(Box::new(HelperCapture {
            child,
            reader: Some(reader),
        })),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(refuse(
                ErrorCode::Internal,
                format!("the capture reader did not start: {error}"),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{chunk_samples, helper_arguments, ChunkAssembler};
    use ilium_plugin_host::imports::audio::CaptureSource;

    #[test]
    fn a_chunk_holds_twenty_milliseconds_of_every_channel() {
        assert_eq!(chunk_samples(16_000, 1), 320);
        assert_eq!(chunk_samples(48_000, 2), 1_920);
    }

    #[test]
    fn the_assembler_joins_split_samples_and_emits_full_chunks_only() {
        let mut assembler = ChunkAssembler::new(3);
        let mut chunks = Vec::new();
        let bytes: Vec<u8> = [1i16, -2, 300, 4, 5]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        // Odd-sized pieces split a sample across two reads.
        assembler.push(&bytes[..3], &mut |chunk| chunks.push(chunk));
        assembler.push(&bytes[3..7], &mut |chunk| chunks.push(chunk));
        assembler.push(&bytes[7..], &mut |chunk| chunks.push(chunk));
        assert_eq!(chunks, vec![vec![1, -2, 300]]);
        assembler.push(&6i16.to_le_bytes(), &mut |chunk| chunks.push(chunk));
        assert_eq!(chunks, vec![vec![1, -2, 300], vec![4, 5, 6]]);
    }

    #[test]
    fn the_helper_records_the_requested_source_format() {
        let loopback = helper_arguments(&CaptureSource::Loopback, 48_000, 2).expect("arguments");
        assert_eq!(loopback[..2], ["-d", "@DEFAULT_MONITOR@"]);
        assert!(loopback.contains(&"--format=s16le".to_owned()));
        assert!(loopback.contains(&"--rate=48000".to_owned()));
        assert!(loopback.contains(&"--channels=2".to_owned()));
        let microphone =
            helper_arguments(&CaptureSource::DefaultInput, 16_000, 1).expect("arguments");
        assert_eq!(microphone[1], "@DEFAULT_SOURCE@");
        let named = helper_arguments(
            &CaptureSource::Device("alsa_input.usb".to_owned()),
            16_000,
            1,
        )
        .expect("arguments");
        assert_eq!(named[1], "alsa_input.usb");
    }

    #[test]
    fn a_device_name_that_could_be_an_option_or_holds_spaces_is_refused() {
        for name in ["", "-x", "two words", "tab\tname"] {
            assert!(
                helper_arguments(&CaptureSource::Device(name.to_owned()), 16_000, 1).is_none(),
                "{name:?}"
            );
        }
    }
}
