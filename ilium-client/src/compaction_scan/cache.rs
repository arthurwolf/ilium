//! Per-agent trace cache: `(path, size, mtime) -> SessionTrace`.
//!
//! Transcripts only ever append, so a changed file is simply parsed again
//! from the start; an unchanged file costs nothing on the next scan. One cache
//! file per agent lives in the Ilium cache directory (the same convention as
//! the cost-history cache). It is versioned by the local [`CACHE_VERSION`]
//! (this file's layout) *and* the analysis crate's `TRACE_FORMAT_VERSION`
//! (the trace layout and parsing rules), so changing either one discards it.
//!
//! Writes are atomic (private temp file, durable rename, sidecar lock) and a
//! failed write only costs the next scan its speed-up, never the report.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use ilium_compaction_analysis::trace::TurnSample;
use ilium_compaction_analysis::{AgentKind, SessionTrace, TRACE_FORMAT_VERSION};
use serde::{Deserialize, Serialize};

/// Layout version of the cache file itself.
pub(super) const CACHE_VERSION: u32 = 1;
/// Largest cache file read or written. Real traces serialize to about
/// 60-70 bytes per request, so this holds far more than the retained-result
/// cap of 64 MiB of in-memory traces.
const MAX_CACHE_FILE_BYTES: u64 = 192 * 1024 * 1024;

/// A cached trace with the file identity it was parsed from.
#[derive(Debug)]
pub(super) struct CachedTrace {
    pub size: u64,
    pub mtime_ms: i64,
    pub trace: SessionTrace,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    path: String,
    size: u64,
    mtime_ms: i64,
    trace: SessionTrace,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    cache_version: u32,
    trace_format_version: u32,
    agent: AgentKind,
    entries: Vec<CacheEntry>,
}

#[derive(Serialize)]
struct CacheEntryRef<'a> {
    path: &'a str,
    size: u64,
    mtime_ms: i64,
    trace: &'a SessionTrace,
}

#[derive(Serialize)]
struct CacheFileRef<'a> {
    cache_version: u32,
    trace_format_version: u32,
    agent: AgentKind,
    entries: Vec<CacheEntryRef<'a>>,
}

/// Ilium's per-user cache directory.
pub fn default_cache_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "ilium").map(|dirs| dirs.cache_dir().to_path_buf())
}

/// File name of one agent's trace cache inside the cache directory.
pub fn cache_file_name(agent: AgentKind) -> String {
    format!(
        "compaction-traces-{}-v{CACHE_VERSION}.json",
        agent.profile().name
    )
}

/// Default cache file of `agent`.
pub fn default_cache_path(agent: AgentKind) -> Option<PathBuf> {
    default_cache_dir().map(|directory| directory.join(cache_file_name(agent)))
}

/// Rough in-memory size of a trace, for the retained-result cap.
pub(super) fn retained_bytes(trace: &SessionTrace) -> usize {
    std::mem::size_of::<SessionTrace>()
        + trace.turns.capacity() * std::mem::size_of::<TurnSample>()
        + trace.turn_id_hashes.capacity() * std::mem::size_of::<u32>()
        + trace.compactions.capacity()
            * std::mem::size_of::<ilium_compaction_analysis::trace::CompactionEvent>()
        + trace
            .models
            .iter()
            .map(|model| model.capacity() + std::mem::size_of::<String>())
            .sum::<usize>()
}

/// Loads the cache of `agent`; anything unreadable, oversized, written by
/// another layout or for another agent is an empty cache.
pub(super) fn load(path: &Path, agent: AgentKind) -> HashMap<String, CachedTrace> {
    let Ok(file) = File::open(path) else {
        return HashMap::new();
    };
    if file
        .metadata()
        .map_or(true, |metadata| metadata.len() > MAX_CACHE_FILE_BYTES)
    {
        return HashMap::new();
    }
    let reader = BufReader::new(file.take(MAX_CACHE_FILE_BYTES));
    let Ok(cache) = serde_json::from_reader::<_, CacheFile>(reader) else {
        return HashMap::new();
    };
    if cache.cache_version != CACHE_VERSION
        || cache.trace_format_version != TRACE_FORMAT_VERSION
        || cache.agent != agent
    {
        return HashMap::new();
    }
    cache
        .entries
        .into_iter()
        .filter(|entry| {
            entry.trace.format_version == TRACE_FORMAT_VERSION && entry.trace.agent == agent
        })
        .map(|entry| {
            (
                entry.path,
                CachedTrace {
                    size: entry.size,
                    mtime_ms: entry.mtime_ms,
                    trace: entry.trace,
                },
            )
        })
        .collect()
}

/// One entry to persist.
pub(super) struct CacheItem<'a> {
    pub path: &'a str,
    pub size: u64,
    pub mtime_ms: i64,
    pub trace: &'a SessionTrace,
}

/// Atomically replaces the cache file of `agent`.
pub(super) fn save(
    path: &Path,
    agent: AgentKind,
    items: &[CacheItem<'_>],
    should_stop: &dyn Fn() -> bool,
) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Compaction trace cache has no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let lock_path = path.with_extension("json.lock");
    let _lock = ilium_platform::file_lock::ExclusiveFileLock::try_acquire(&lock_path)?.ok_or_else(
        || {
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "Compaction trace cache writer busy",
            )
        },
    )?;
    let document = CacheFileRef {
        cache_version: CACHE_VERSION,
        trace_format_version: TRACE_FORMAT_VERSION,
        agent,
        entries: items
            .iter()
            .map(|item| CacheEntryRef {
                path: item.path,
                size: item.size,
                mtime_ms: item.mtime_ms,
                trace: item.trace,
            })
            .collect(),
    };
    let temporary = parent.join(format!(
        ".ilium-compaction-traces-{}.tmp",
        uuid::Uuid::new_v4()
    ));
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        ilium_platform::secure_fs::restrict_open_file_to_owner(&output)?;
        {
            let mut writer = BufWriter::new(&mut output);
            serde_json::to_writer(&mut writer, &document).map_err(std::io::Error::other)?;
            writer.flush()?;
        }
        if output.metadata()?.len() > MAX_CACHE_FILE_BYTES {
            return Err(std::io::Error::other(
                "Compaction trace cache byte bound exceeded",
            ));
        }
        output.sync_all()?;
        if should_stop() {
            return Err(std::io::Error::other(
                "Compaction scan cancelled before cache publication",
            ));
        }
        ilium_platform::secure_fs::replace_file_durably(&temporary, path)
    })();
    drop(output);
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
