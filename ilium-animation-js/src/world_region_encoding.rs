//! Bounded native region response: JSON metadata plus little-endian u16 plane.
//! Encoded buffers have their own caller admission before allocation. They cannot
//! be moved out of the owner while its lease is released.
use crate::world_region::{BlockStateView, BorrowedRegion, RegionError};
use serde::{ser::SerializeSeq, Serialize, Serializer};
use std::{
    collections::BTreeMap,
    io::{self, Write},
};

pub struct EncodedRegion<H> {
    metadata: Vec<u8>,
    blocks: Vec<u8>,
    _admission: H,
}
#[cfg(feature = "v8-runtime")]
impl EncodedRegion<ilium_execution::StorageAdmission> {
    pub(crate) fn shares_root(&self, quota: &ilium_execution::QuotaGroup) -> bool {
        self._admission.shares_root(quota)
    }
}
impl<H> EncodedRegion<H> {
    pub fn metadata(&self) -> &[u8] {
        &self.metadata
    }
    pub fn blocks(&self) -> &[u8] {
        &self.blocks
    }
}
#[derive(Serialize)]
struct BinaryReference {
    #[serde(rename = "$ilium_binary")]
    plane: &'static str,
}
#[derive(Serialize)]
struct State<'a> {
    name: &'a str,
    properties: &'a BTreeMap<String, String>,
}
struct Palette<'a, 'b>(&'a [BlockStateView<'b>]);
impl Serialize for Palette<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&State {
                name: value.name,
                properties: value.properties,
            })?;
        }
        sequence.end()
    }
}
#[derive(Serialize)]
struct RegionMetadata<'a, 'b> {
    origin: [i32; 3],
    size: [u32; 3],
    blocks: BinaryReference,
    palette: Palette<'a, 'b>,
    identity: &'a str,
}
#[derive(Serialize)]
struct Envelope<'a, 'b> {
    ok: bool,
    value: RegionMetadata<'a, 'b>,
}
struct BoundedWriter<'a, C> {
    bytes: Vec<u8>,
    limit: usize,
    work: &'a mut usize,
    cancelled: &'a mut C,
    failure: Option<RegionError>,
}
impl<C: FnMut() -> bool> Write for BoundedWriter<'_, C> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let failure = if (self.cancelled)() {
            Some(RegionError::Cancelled)
        } else if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|length| length > self.limit)
        {
            Some(RegionError::Bytes)
        } else if let Some(remaining) = self.work.checked_sub(bytes.len()) {
            *self.work = remaining;
            None
        } else {
            Some(RegionError::Work)
        };
        if let Some(failure) = failure {
            self.failure = Some(failure);
            return Err(io::Error::other("bounded native region encoding refused"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub fn encode_region<G, H>(
    source: BorrowedRegion<'_, G>,
    identity: &str,
    max_bytes: usize,
    mut max_work: usize,
    mut cancelled: impl FnMut() -> bool,
    admit: impl FnOnce(usize) -> Result<H, RegionError>,
) -> Result<EncodedRegion<H>, RegionError> {
    if cancelled() {
        return Err(RegionError::Cancelled);
    }
    if identity.len() != 64
        || !identity
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(RegionError::Unavailable);
    }
    let cells = source
        .size
        .iter()
        .try_fold(1_usize, |n, &extent| n.checked_mul(extent as usize))
        .ok_or(RegionError::Extent)?;
    if cells == 0
        || cells != source.blocks.len()
        || source.palette.is_empty()
        || source.palette.len() > 65536
    {
        return Err(RegionError::Extent);
    }
    for (origin, extent) in source.origin.into_iter().zip(source.size) {
        if i64::from(origin) + i64::from(extent) - 1 > i64::from(i32::MAX) {
            return Err(RegionError::Extent);
        }
    }
    let binary_bytes = cells.checked_mul(2).ok_or(RegionError::Bytes)?;
    if source.response_bytes > max_bytes {
        return Err(RegionError::Bytes);
    }
    let metadata_limit = source
        .response_bytes
        .checked_sub(binary_bytes)
        .ok_or(RegionError::Bytes)?;
    let charge = source
        .response_bytes
        .checked_add(std::mem::size_of::<EncodedRegion<H>>())
        .and_then(|n| n.checked_add(1024))
        .ok_or(RegionError::Bytes)?;
    // This is the original caller's account, never an independent quota root.
    let admission = admit(charge)?;
    let mut blocks = Vec::new();
    blocks
        .try_reserve_exact(binary_bytes)
        .map_err(|_| RegionError::Allocation)?;
    for &index in &source.blocks {
        if cancelled() {
            return Err(RegionError::Cancelled);
        }
        if usize::from(index) >= source.palette.len() {
            return Err(RegionError::Palette);
        }
        max_work = max_work.checked_sub(2).ok_or(RegionError::Work)?;
        blocks.extend_from_slice(&index.to_le_bytes());
    }
    let mut metadata = Vec::new();
    metadata
        .try_reserve_exact(metadata_limit)
        .map_err(|_| RegionError::Allocation)?;
    let mut writer = BoundedWriter {
        bytes: metadata,
        limit: metadata_limit,
        work: &mut max_work,
        cancelled: &mut cancelled,
        failure: None,
    };
    let payload = Envelope {
        ok: true,
        value: RegionMetadata {
            origin: source.origin,
            size: source.size,
            blocks: BinaryReference { plane: "blocks" },
            palette: Palette(&source.palette),
            identity,
        },
    };
    if serde_json::to_writer(&mut writer, &payload).is_err() {
        return Err(writer.failure.take().unwrap_or(RegionError::Unavailable));
    }
    let metadata = writer.bytes;
    if cancelled() {
        return Err(RegionError::Cancelled);
    }
    // The caller's source guard stays live through every serialization read.
    // Only the distinct admitted encoded copies survive this return.
    Ok(EncodedRegion {
        metadata,
        blocks,
        _admission: admission,
    })
}

#[cfg(test)]
#[path = "world_region_encoding_tests.rs"]
mod tests;
