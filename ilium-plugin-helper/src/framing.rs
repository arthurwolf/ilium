//! Length-prefixed JSON frames: a 4-byte little-endian length, then that many bytes of JSON.

use std::io::{Read, Write};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::HelperError;

/// The largest accepted message body: 4 MiB (HST-155).
pub const MAX_MESSAGE_BYTES: u32 = 4 * 1024 * 1024;

/// Writes one frame and flushes.
pub fn write_frame(output: &mut impl Write, message: &impl Serialize) -> Result<(), HelperError> {
    let body = serde_json::to_vec(message)
        .map_err(|error| HelperError::Protocol(format!("cannot encode frame: {error}")))?;
    let length = u32::try_from(body.len())
        .ok()
        .filter(|length| *length <= MAX_MESSAGE_BYTES)
        .ok_or_else(|| HelperError::Protocol("outgoing frame exceeds the message cap".to_owned()))?;
    output
        .write_all(&length.to_le_bytes())
        .and_then(|()| output.write_all(&body))
        .and_then(|()| output.flush())
        .map_err(|error| HelperError::Channel(error.to_string()))
}

/// Reads one frame. `Ok(None)` means the host closed the channel cleanly between frames; a
/// partial prefix or body, an oversized length or invalid JSON is an error.
pub fn read_frame<T: DeserializeOwned>(input: &mut impl Read) -> Result<Option<T>, HelperError> {
    let mut prefix = [0u8; 4];
    let mut filled = 0;
    while filled < prefix.len() {
        let count = input
            .read(&mut prefix[filled..])
            .map_err(|error| HelperError::Channel(error.to_string()))?;
        if count == 0 {
            return if filled == 0 {
                Ok(None)
            } else {
                Err(HelperError::Protocol("truncated frame length prefix".to_owned()))
            };
        }
        filled += count;
    }
    let length = u32::from_le_bytes(prefix);
    if length > MAX_MESSAGE_BYTES {
        return Err(HelperError::Protocol(format!(
            "frame length {length} exceeds the {MAX_MESSAGE_BYTES} byte message cap"
        )));
    }
    let mut body = vec![0u8; length as usize];
    input
        .read_exact(&mut body)
        .map_err(|error| HelperError::Protocol(format!("truncated frame body: {error}")))?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| HelperError::Protocol(format!("invalid frame body: {error}")))
}
