//! Bounded incremental reads from an already verified transcript.
//!
//! This reader checks file identity and continuity only. The owning server
//! must separately verify process ownership and session generation before and
//! after every read, and reject the entire result if that fence changes.
use sha2::{Digest, Sha256};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use ilium_core::polling_detection::ActivityEvent;
use ilium_core::AgentClass;
use ilium_platform::{file_identity, secure_fs};

use crate::polling_activity::{
    is_visible_codex_message, ActivityExtractor, ActivityFramer, FramedRecord,
};

const CHUNK_BYTES: usize = 64 * 1024;
const RECORD_BYTES: usize = 256 * 1024;
const ANCHOR_BYTES: usize = 32;
const MAX_EVENTS_PER_BATCH: usize = 512;

/// Metadata-only recovery state; never contains transcript bytes or paths.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TranscriptCheckpoint {
    pub version: u8,
    pub device: u64,
    pub inode: u64,
    pub offset: u64,
    pub anchor_digest: [u8; 32],
    pub recent_record_digests: Vec<[u8; 32]>,
}

#[derive(Clone)]
pub struct IncrementalReader {
    identity: file_identity::FileIdentity,
    offset: u64,
    anchor: Vec<u8>,
    skip_old_fragment: bool,
    framer: ActivityFramer,
    extractor: ActivityExtractor,
}

#[derive(Clone)]
pub struct IncrementalBatch {
    pub events: Vec<ActivityEvent>,
    /// An oversized or malformed record means absence of events is unknown.
    pub contains_gap: bool,
}

impl IncrementalReader {
    /// Conservative retained storage, independent of the temporary JSON parse
    /// envelope. Includes spare backing capacity and the inline reader.
    pub fn retained_storage_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.anchor.capacity())
            .saturating_add(self.framer.retained_heap_bytes())
            .saturating_add(self.extractor.retained_heap_bytes())
    }

    /// Opaque consumed byte position, including an unfinished record.
    /// Only `checkpoint` establishes a durable record-boundary cursor.
    pub fn position(&self) -> u64 {
        self.offset
    }

    /// A partial or oversized record must finish before checkpoint publication.
    pub fn checkpoint(&self) -> Option<TranscriptCheckpoint> {
        (!self.skip_old_fragment && self.framer.at_record_boundary()).then(|| {
            TranscriptCheckpoint {
                version: 1,
                device: self.identity.device,
                inode: self.identity.inode,
                offset: self.offset,
                anchor_digest: Sha256::digest(&self.anchor).into(),
                recent_record_digests: self.extractor.checkpoint_ids(),
            }
        })
    }

    /// The caller must additionally revalidate process/session ownership.
    pub fn resume(path: &Path, checkpoint: &TranscriptCheckpoint) -> io::Result<Self> {
        if checkpoint.version != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported transcript checkpoint",
            ));
        }
        let extractor = ActivityExtractor::from_checkpoint_ids(&checkpoint.recent_record_digests)
            .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid transcript checkpoint identities",
            )
        })?;
        let identity = file_identity::FileIdentity {
            device: checkpoint.device,
            inode: checkpoint.inode,
        };
        let mut file = secure_fs::open_regular_file(path)?;
        if file_identity::identity(&file)? != identity || file.metadata()?.len() < checkpoint.offset
        {
            return Err(io::Error::other("transcript checkpoint identity changed"));
        }
        let anchor = read_anchor(&mut file, checkpoint.offset)?;
        let digest: [u8; 32] = Sha256::digest(&anchor).into();
        if digest != checkpoint.anchor_digest
            || (checkpoint.offset > 0 && anchor.last() != Some(&b'\n'))
        {
            return Err(io::Error::other("transcript checkpoint continuity changed"));
        }
        let reopened = secure_fs::open_regular_file(path)?;
        if file_identity::identity(&reopened)? != identity
            || file.metadata()?.len() < checkpoint.offset
            || read_anchor(&mut file, checkpoint.offset)? != anchor
        {
            return Err(io::Error::other(
                "transcript changed during checkpoint recovery",
            ));
        }
        Ok(Self {
            identity,
            offset: checkpoint.offset,
            anchor,
            skip_old_fragment: false,
            framer: ActivityFramer::new(RECORD_BYTES),
            extractor,
        })
    }

    /// Identity of the file actually opened by this cursor, for the caller's
    /// process-to-file ownership fence.
    pub fn identity(&self) -> &file_identity::FileIdentity {
        &self.identity
    }

    /// Starts at the current end: enabling never scans or sends old history.
    pub fn baseline(path: &Path) -> io::Result<Self> {
        let mut file = secure_fs::open_regular_file(path)?;
        let identity = file_identity::identity(&file)?;
        let offset = file.metadata()?.len();
        let anchor = read_anchor(&mut file, offset)?;
        let skip_old_fragment = offset > 0 && anchor.last() != Some(&b'\n');
        let reopened = secure_fs::open_regular_file(path)?;
        if file_identity::identity(&reopened)? != identity || file.metadata()?.len() < offset {
            return Err(io::Error::other("transcript changed during baseline"));
        }
        Ok(Self {
            identity,
            offset,
            anchor,
            skip_old_fragment,
            framer: ActivityFramer::new(RECORD_BYTES),
            extractor: ActivityExtractor::default(),
        })
    }

    /// Returns a tentative cursor and at most one bounded chunk. The caller
    /// must commit neither until the server's process/session fence passes.
    pub fn read(
        &self,
        path: &Path,
        provider: &AgentClass,
        expected_session: &str,
    ) -> io::Result<(Self, IncrementalBatch)> {
        if expected_session.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing verified session identity",
            ));
        }
        let mut file = secure_fs::open_regular_file(path)?;
        if file_identity::identity(&file)? != self.identity
            || file.metadata()?.len() < self.offset
            || read_anchor(&mut file, self.offset)? != self.anchor
        {
            return Err(io::Error::other(
                "transcript identity or continuity changed",
            ));
        }
        let mut next = self.clone();
        file.seek(SeekFrom::Start(next.offset))?;
        let mut bytes = Vec::with_capacity(CHUNK_BYTES);
        file.by_ref()
            .take(CHUNK_BYTES as u64)
            .read_to_end(&mut bytes)?;
        next.offset = next
            .offset
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| io::Error::other("transcript offset overflow"))?;
        let mut input = bytes.as_slice();
        if next.skip_old_fragment {
            if let Some(end) = input.iter().position(|byte| *byte == b'\n') {
                next.skip_old_fragment = false;
                input = &input[end + 1..];
            } else {
                input = &[];
            }
        }
        let mut batch = IncrementalBatch {
            events: Vec::new(),
            contains_gap: false,
        };
        for record in next.framer.push(input) {
            match record {
                FramedRecord::Oversized => batch.contains_gap = true,
                FramedRecord::Record(bytes) if bytes.is_empty() => {}
                FramedRecord::Record(bytes) => {
                    match serde_json::from_slice::<serde_json::Value>(&bytes) {
                        Ok(value)
                            if *provider == AgentClass::Claude
                                && value["sessionId"]
                                    .as_str()
                                    .is_some_and(|id| id != expected_session) =>
                        {
                            batch.contains_gap = true
                        }
                        Ok(value)
                            if *provider == AgentClass::Codex
                                && value["type"].as_str() == Some("response_item")
                                && value["payload"]["type"].as_str() == Some("message")
                                && value["payload"]["role"].as_str() == Some("assistant")
                                && value["payload"]["channel"].as_str() != Some("analysis")
                                && value["payload"]["phase"].as_str() != Some("analysis")
                                && !is_visible_codex_message(&value["payload"]) =>
                        {
                            batch.contains_gap = true
                        }
                        Ok(value) => {
                            let events = next.extractor.observe(provider, &value);
                            let remaining = MAX_EVENTS_PER_BATCH.saturating_sub(batch.events.len());
                            if events.len() > remaining {
                                batch.contains_gap = true;
                            }
                            batch.events.extend(events.into_iter().take(remaining));
                        }
                        Err(_) => batch.contains_gap = true,
                    }
                }
            }
        }
        let reopened = secure_fs::open_regular_file(path)?;
        if file_identity::identity(&reopened)? != self.identity
            || file.metadata()?.len() < next.offset
            || read_anchor(&mut file, self.offset)? != self.anchor
        {
            return Err(io::Error::other("transcript changed during read"));
        }
        next.anchor = read_anchor(&mut file, next.offset)?;
        Ok((next, batch))
    }
}

fn read_anchor(file: &mut std::fs::File, offset: u64) -> io::Result<Vec<u8>> {
    let start = offset.saturating_sub(ANCHOR_BYTES as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; (offset - start) as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}
