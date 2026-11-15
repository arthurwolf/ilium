//! The `fetch-to-cache` job: downloads one remote file into the scene's private `cache` scope.
//!
//! A scene must never wait inside `render`, and the host `http` body cap (8 MiB) is smaller than
//! many media files, so a large download runs as a job (`jobs.run`, `contributes.jobs`) in its
//! own instance. The job asks for the file in ranged chunks, appends each chunk to
//! `<name>.part` in the `cache` scope (`fs.write:cache`), checks the length and SHA-256 when the
//! caller pins them, and renames the finished file to `<name>`. A job has a deadline, so one run
//! stops after [`RUN_BUDGET_MILLIS`] and reports how far it got; the scene starts the job again
//! and the next run resumes from the length of the `.part` file. Before a new download starts,
//! the oldest cached files are removed until the new one fits in [`CACHE_BUDGET_BYTES`].
//!
//! The scene side starts the job with `jobs.start(FETCH_JOB, request JSON)`, polls it with
//! `jobs.wait-request(id, 0)` and reads the [`FetchReport`] JSON with `jobs.take-output`. A
//! finished file is read back through the `cache` scope (`fs.read:cache`), for example by
//! `media.open-video`.

use ilium_plugin_sdk::host::{
    EntryKind, HostClock, HostFs, HostHttp, HostJobs, HttpRequest, WriteMode,
};
use ilium_plugin_sdk::types::errors::ErrorCode;
use ilium_plugin_sdk::types::net::Header;
use ilium_plugin_sdk::types::paths::FsPath;
use ilium_plugin_sdk::types::{PluginError, RequestId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The job kind, as declared in `contributes.jobs` of the manifest.
pub const FETCH_JOB: &str = "fetch-to-cache";
/// The scope the job writes to and the scene reads from.
pub const CACHE_SCOPE: &str = "cache";
/// Bytes asked for in one ranged request (below the 8 MiB `http` body cap).
pub const CHUNK_BYTES: u64 = 4 * 1024 * 1024;
/// Largest file the job downloads.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
/// Total size the cached files may reach before the oldest are removed.
pub const CACHE_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;
/// One run stops starting new chunks after this long: with one chunk still answering (at most
/// 25 s) it stays inside the 60 s job deadline.
pub const RUN_BUDGET_MILLIS: u64 = 30_000;
/// Timeout asked of one ranged request.
const REQUEST_TIMEOUT_MILLIS: u32 = 20_000;
/// A chunk that has not answered after this long fails the run.
const ANSWER_PATIENCE_MILLIS: u64 = 25_000;
/// `fs.read` returns at most this much per call.
const READ_SLICE_BYTES: u32 = 1024 * 1024;
/// Longest cache file name.
const MAX_NAME_CHARS: usize = 128;
/// Suffix of a download in progress.
const PART_SUFFIX: &str = ".part";

/// What the scene asks the job to download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRequest {
    /// An http(s) address on a host the scene's grants allow.
    pub url: String,
    /// File name in the `cache` scope: letters, digits, `.`, `-` and `_`.
    pub name: String,
    /// Lower-case hex SHA-256 the finished file must have, when pinned.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Exact length the finished file must have, when pinned.
    #[serde(default)]
    pub expected_bytes: Option<u64>,
}

/// How far one run got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchReport {
    /// True once `<name>` is in the cache, checked.
    pub done: bool,
    /// Bytes on disk so far.
    pub bytes: u64,
    /// Length of the whole file, when known.
    pub total: Option<u64>,
}

/// One answer to a ranged request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeAnswer {
    pub status: u16,
    pub body: Vec<u8>,
    /// First byte and total length of a `Content-Range` header, when present.
    pub range_start: Option<u64>,
    pub total: Option<u64>,
}

/// One file of the cache scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedFile {
    pub name: String,
    pub length: u64,
    pub modified_millis: u64,
}

/// The host calls the job makes. [`HostFetchIo`] in the component, a fake in tests.
pub trait FetchIo {
    /// Length of cache file `name`, `None` when it does not exist.
    fn length(&mut self, name: &str) -> Option<u64>;
    fn append(&mut self, name: &str, data: &[u8]) -> Result<(), String>;
    fn read(&mut self, name: &str, offset: u64, max_bytes: u32) -> Result<Vec<u8>, String>;
    fn rename(&mut self, from: &str, to: &str) -> Result<(), String>;
    fn remove(&mut self, name: &str) -> Result<(), String>;
    fn list(&mut self) -> Result<Vec<CachedFile>, String>;
    /// GET `url` with `Range: bytes=<first>-<last>`, waiting for the answer.
    fn get_range(&mut self, url: &str, first: u64, last: u64) -> Result<RangeAnswer, String>;
    fn cancelled(&mut self) -> bool;
    fn progress(&mut self, percent: f32);
    fn now_millis(&mut self) -> u64;
}

/// The `.part` name of a download in progress.
pub fn part_name(name: &str) -> String {
    format!("{name}{PART_SUFFIX}")
}

/// Checks a cache file name: a plain name, never a path or a hidden file.
pub fn check_name(name: &str) -> Result<(), String> {
    let plain = !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.starts_with('.')
        && !name.ends_with(PART_SUFFIX)
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character));
    if plain {
        Ok(())
    } else {
        Err(format!("{name:?} is not a plain cache file name"))
    }
}

fn check_url(url: &str) -> Result<(), String> {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        Ok(())
    } else {
        Err("only http(s) addresses can be downloaded".to_owned())
    }
}

/// Parses `bytes <first>-<last>/<total>` (or `bytes */<total>`) into the first byte and the
/// total length; `*` totals are `None`.
pub fn parse_content_range(value: &str) -> (Option<u64>, Option<u64>) {
    let Some(rest) = value.trim().strip_prefix("bytes") else {
        return (None, None);
    };
    let Some((range, total)) = rest.trim().split_once('/') else {
        return (None, None);
    };
    let first = range
        .split_once('-')
        .and_then(|(first, _)| first.trim().parse().ok());
    (first, total.trim().parse().ok())
}

/// Lower-case hex of a digest.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// SHA-256 of the first `length` bytes of cache file `name`, read in slices.
fn hash_file(io: &mut dyn FetchIo, name: &str, length: u64) -> Result<String, String> {
    let mut hasher = Sha256::new();
    let mut offset = 0;
    while offset < length {
        let slice = io.read(name, offset, READ_SLICE_BYTES)?;
        if slice.is_empty() {
            return Err("the downloaded file is shorter than expected".to_owned());
        }
        offset += slice.len() as u64;
        hasher.update(&slice);
    }
    Ok(hex(&hasher.finalize()))
}

/// Removes the oldest cached files until `incoming` more bytes fit in the budget. The file being
/// downloaded (and its `.part`) is never removed.
fn make_room(io: &mut dyn FetchIo, name: &str, incoming: u64) -> Result<(), String> {
    let part = part_name(name);
    let mut files: Vec<CachedFile> = io
        .list()?
        .into_iter()
        .filter(|file| file.name != name && file.name != part)
        .collect();
    let mut used: u64 = files.iter().map(|file| file.length).sum();
    files.sort_by_key(|file| file.modified_millis);
    for file in files {
        if used.saturating_add(incoming) <= CACHE_BUDGET_BYTES {
            break;
        }
        io.remove(&file.name)?;
        used = used.saturating_sub(file.length);
    }
    Ok(())
}

/// Drops the `.part` file and returns `message` as the failure.
fn discard(io: &mut dyn FetchIo, part: &str, message: String) -> Result<FetchReport, String> {
    // The failure is the message; a `.part` that cannot be removed is overwritten by the next
    // run's checks anyway (its length no longer matches).
    let _ = io.remove(part);
    Err(message)
}

/// One run of the job. `Ok` with `done: false` means "start the job again to continue".
pub fn run(io: &mut dyn FetchIo, request: &FetchRequest) -> Result<FetchReport, String> {
    check_name(&request.name)?;
    check_url(&request.url)?;
    if request
        .expected_bytes
        .is_some_and(|bytes| bytes > MAX_FILE_BYTES)
    {
        return Err(format!(
            "files over {MAX_FILE_BYTES} bytes are not downloaded"
        ));
    }
    if let Some(length) = io.length(&request.name) {
        return Ok(FetchReport {
            done: true,
            bytes: length,
            total: Some(length),
        });
    }
    let part = part_name(&request.name);
    let mut offset = io.length(&part).unwrap_or(0);
    if offset == 0 {
        make_room(
            io,
            &request.name,
            request.expected_bytes.unwrap_or(CHUNK_BYTES),
        )?;
    }
    let started = io.now_millis();
    let mut total = request.expected_bytes;
    loop {
        if total.is_some_and(|total| offset >= total) {
            break;
        }
        if io.cancelled() || io.now_millis().saturating_sub(started) > RUN_BUDGET_MILLIS {
            return Ok(FetchReport {
                done: false,
                bytes: offset,
                total,
            });
        }
        let mut last = offset + CHUNK_BYTES - 1;
        if let Some(total) = total {
            last = last.min(total - 1);
        }
        let asked = last - offset + 1;
        let answer = io.get_range(&request.url, offset, last)?;
        match answer.status {
            206 => {
                if answer.body.is_empty() {
                    return discard(io, &part, "the server answered an empty chunk".to_owned());
                }
                if answer.range_start.is_some_and(|first| first != offset) {
                    return discard(io, &part, "the server answered the wrong range".to_owned());
                }
                match (total, answer.total) {
                    (Some(expected), Some(reported)) if expected != reported => {
                        return discard(
                            io,
                            &part,
                            format!("the file is {reported} bytes, expected {expected}"),
                        );
                    }
                    (None, Some(reported)) => total = Some(reported),
                    _ => {}
                }
                io.append(&part, &answer.body)?;
                offset += answer.body.len() as u64;
                if total.is_none() && (answer.body.len() as u64) < asked {
                    total = Some(offset);
                }
            }
            200 if offset == 0 => {
                // No range support, but the whole file fit in one answer.
                io.append(&part, &answer.body)?;
                offset = answer.body.len() as u64;
                total = Some(offset);
            }
            200 => {
                return discard(
                    io,
                    &part,
                    "the server does not support partial downloads".to_owned(),
                );
            }
            416 if total.is_none() && answer.total == Some(offset) => total = Some(offset),
            status => return Err(format!("the server answered HTTP {status}")),
        }
        if offset > MAX_FILE_BYTES {
            return discard(
                io,
                &part,
                format!("files over {MAX_FILE_BYTES} bytes are not downloaded"),
            );
        }
        if let Some(total) = total.filter(|total| *total > 0) {
            io.progress((offset as f64 / total as f64 * 100.0).min(100.0) as f32);
        }
    }
    if let Some(expected) = request.expected_bytes {
        if offset != expected {
            return discard(
                io,
                &part,
                format!("downloaded {offset} bytes, expected {expected}"),
            );
        }
    }
    if let Some(pinned) = &request.sha256 {
        let digest = hash_file(io, &part, offset)?;
        if !digest.eq_ignore_ascii_case(pinned) {
            return discard(
                io,
                &part,
                "the downloaded file failed its checksum".to_owned(),
            );
        }
    }
    io.rename(&part, &request.name)?;
    Ok(FetchReport {
        done: true,
        bytes: offset,
        total: Some(offset),
    })
}

/// The job entry point: `input` is a [`FetchRequest`] as JSON, the output a [`FetchReport`].
pub fn run_job(input: &[u8]) -> Result<Vec<u8>, PluginError> {
    let request: FetchRequest = serde_json::from_slice(input).map_err(|error| {
        PluginError::new(
            ErrorCode::InvalidArgument,
            format!("fetch-to-cache input: {error}"),
        )
    })?;
    let report = run(&mut HostFetchIo, &request)
        .map_err(|message| PluginError::new(ErrorCode::Internal, message))?;
    serde_json::to_vec(&report)
        .map_err(|error| PluginError::new(ErrorCode::Internal, error.to_string()))
}

/// The host's `fs` (cache scope), `http`, `jobs` and `clock` imports.
#[derive(Debug, Default, Clone, Copy)]
pub struct HostFetchIo;

fn cache_path(name: &str) -> FsPath {
    FsPath {
        scope: CACHE_SCOPE.to_owned(),
        path: name.to_owned(),
    }
}

impl FetchIo for HostFetchIo {
    fn length(&mut self, name: &str) -> Option<u64> {
        HostFs::metadata(&mut *ilium_plugin_sdk::host(), cache_path(name))
            .ok()
            .filter(|metadata| metadata.kind == EntryKind::File)
            .map(|metadata| metadata.length)
    }

    fn append(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        HostFs::write(
            &mut *ilium_plugin_sdk::host(),
            cache_path(name),
            data.to_vec(),
            WriteMode::Append,
        )
        .map(|_| ())
        .map_err(|error| error.message)
    }

    fn read(&mut self, name: &str, offset: u64, max_bytes: u32) -> Result<Vec<u8>, String> {
        HostFs::read(
            &mut *ilium_plugin_sdk::host(),
            cache_path(name),
            offset,
            max_bytes,
        )
        .map_err(|error| error.message)
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        HostFs::rename(
            &mut *ilium_plugin_sdk::host(),
            cache_path(from),
            cache_path(to),
        )
        .map(|_| ())
        .map_err(|error| error.message)
    }

    fn remove(&mut self, name: &str) -> Result<(), String> {
        HostFs::remove(&mut *ilium_plugin_sdk::host(), cache_path(name))
            .map(|_| ())
            .map_err(|error| error.message)
    }

    fn list(&mut self) -> Result<Vec<CachedFile>, String> {
        let entries = HostFs::list_dir(&mut *ilium_plugin_sdk::host(), cache_path(""), 4096)
            .map_err(|error| error.message)?;
        Ok(entries
            .into_iter()
            .filter(|entry| entry.metadata.kind == EntryKind::File)
            .map(|entry| CachedFile {
                name: entry.name,
                length: entry.metadata.length,
                modified_millis: entry.metadata.modified_unix_millis.unwrap_or(0),
            })
            .collect())
    }

    fn get_range(&mut self, url: &str, first: u64, last: u64) -> Result<RangeAnswer, String> {
        let RequestId(id) = HostHttp::fetch(
            &mut *ilium_plugin_sdk::host(),
            HttpRequest {
                method: "GET".into(),
                url: url.into(),
                headers: vec![Header {
                    name: "Range".into(),
                    value: format!("bytes={first}-{last}"),
                }],
                body: None,
                timeout_millis: REQUEST_TIMEOUT_MILLIS,
            },
        )
        .map_err(|error| error.message)?;
        let started = self.now_millis();
        loop {
            let asked = self.now_millis();
            // In a job instance `take-response` waits up to two seconds for a request in flight;
            // an immediate `None` means the request is gone (it failed).
            if let Some(response) =
                HostHttp::take_response(&mut *ilium_plugin_sdk::host(), RequestId(id))
            {
                let (range_start, total) = response
                    .headers
                    .iter()
                    .find(|header| header.name.eq_ignore_ascii_case("content-range"))
                    .map_or((None, None), |header| parse_content_range(&header.value));
                return Ok(RangeAnswer {
                    status: response.status,
                    body: response.body,
                    range_start,
                    total,
                });
            }
            let now = self.now_millis();
            if now.saturating_sub(asked) < 200 {
                return Err("the download failed".to_owned());
            }
            if now.saturating_sub(started) > ANSWER_PATIENCE_MILLIS {
                return Err("the download timed out".to_owned());
            }
        }
    }

    fn cancelled(&mut self) -> bool {
        HostJobs::is_cancelled(&mut *ilium_plugin_sdk::host())
    }

    fn progress(&mut self, percent: f32) {
        HostJobs::report_progress(&mut *ilium_plugin_sdk::host(), percent);
    }

    fn now_millis(&mut self) -> u64 {
        HostClock::monotonic_millis(&mut *ilium_plugin_sdk::host())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An in-memory cache scope and one remote file served with or without range support.
    struct FakeIo {
        files: HashMap<String, (Vec<u8>, u64)>,
        remote: Vec<u8>,
        ranges: bool,
        status: Option<u16>,
        clock: u64,
        millis_per_request: u64,
        requests: usize,
        cancelled: bool,
        removed: Vec<String>,
        /// Large cached files known only by their listing (no bytes held).
        listed_only: Vec<CachedFile>,
    }

    impl FakeIo {
        fn serving(remote: Vec<u8>) -> Self {
            Self {
                files: HashMap::new(),
                remote,
                ranges: true,
                status: None,
                clock: 0,
                millis_per_request: 10,
                requests: 0,
                cancelled: false,
                removed: Vec::new(),
                listed_only: Vec::new(),
            }
        }

        fn file(&self, name: &str) -> Option<&Vec<u8>> {
            self.files.get(name).map(|(bytes, _)| bytes)
        }
    }

    impl FetchIo for FakeIo {
        fn length(&mut self, name: &str) -> Option<u64> {
            self.files.get(name).map(|(bytes, _)| bytes.len() as u64)
        }

        fn append(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
            let clock = self.clock;
            let entry = self
                .files
                .entry(name.to_owned())
                .or_insert_with(|| (Vec::new(), clock));
            entry.0.extend_from_slice(data);
            entry.1 = clock;
            Ok(())
        }

        fn read(&mut self, name: &str, offset: u64, max_bytes: u32) -> Result<Vec<u8>, String> {
            let bytes = self.file(name).ok_or("not found")?;
            let start = (offset as usize).min(bytes.len());
            let end = (start + max_bytes as usize).min(bytes.len());
            Ok(bytes[start..end].to_vec())
        }

        fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
            let file = self.files.remove(from).ok_or("not found")?;
            self.files.insert(to.to_owned(), file);
            Ok(())
        }

        fn remove(&mut self, name: &str) -> Result<(), String> {
            self.removed.push(name.to_owned());
            let listed = self.listed_only.len();
            self.listed_only.retain(|file| file.name != name);
            if self.files.remove(name).is_some() || self.listed_only.len() < listed {
                Ok(())
            } else {
                Err("not found".to_owned())
            }
        }

        fn list(&mut self) -> Result<Vec<CachedFile>, String> {
            Ok(self
                .files
                .iter()
                .map(|(name, (bytes, modified))| CachedFile {
                    name: name.clone(),
                    length: bytes.len() as u64,
                    modified_millis: *modified,
                })
                .chain(self.listed_only.iter().cloned())
                .collect())
        }

        fn get_range(&mut self, _url: &str, first: u64, last: u64) -> Result<RangeAnswer, String> {
            self.requests += 1;
            self.clock += self.millis_per_request;
            if let Some(status) = self.status {
                return Ok(RangeAnswer {
                    status,
                    body: Vec::new(),
                    range_start: None,
                    total: None,
                });
            }
            let total = self.remote.len() as u64;
            if !self.ranges {
                return Ok(RangeAnswer {
                    status: 200,
                    body: self.remote.clone(),
                    range_start: None,
                    total: None,
                });
            }
            if first >= total {
                return Ok(RangeAnswer {
                    status: 416,
                    body: Vec::new(),
                    range_start: None,
                    total: Some(total),
                });
            }
            let end = (last + 1).min(total);
            Ok(RangeAnswer {
                status: 206,
                body: self.remote[first as usize..end as usize].to_vec(),
                range_start: Some(first),
                total: Some(total),
            })
        }

        fn cancelled(&mut self) -> bool {
            self.cancelled
        }

        fn progress(&mut self, _percent: f32) {}

        fn now_millis(&mut self) -> u64 {
            self.clock
        }
    }

    fn remote(length: usize) -> Vec<u8> {
        (0..length).map(|index| (index % 251) as u8).collect()
    }

    fn sha(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    fn request(name: &str) -> FetchRequest {
        FetchRequest {
            url: "https://upload.example.org/clip.webm".to_owned(),
            name: name.to_owned(),
            sha256: None,
            expected_bytes: None,
        }
    }

    #[test]
    fn a_file_is_fetched_in_chunks_checked_and_renamed() {
        let body = remote((CHUNK_BYTES * 2 + 17) as usize);
        let mut io = FakeIo::serving(body.clone());
        let pinned = FetchRequest {
            sha256: Some(sha(&body)),
            expected_bytes: Some(body.len() as u64),
            ..request("clip.webm")
        };
        let report = run(&mut io, &pinned).expect("the download succeeds");
        assert_eq!(
            report,
            FetchReport {
                done: true,
                bytes: body.len() as u64,
                total: Some(body.len() as u64)
            }
        );
        assert_eq!(io.requests, 3);
        assert_eq!(io.file("clip.webm"), Some(&body));
        assert!(io.file("clip.webm.part").is_none());
        // A second run finds the finished file and downloads nothing.
        assert!(run(&mut io, &pinned).expect("cached").done);
        assert_eq!(io.requests, 3);
    }

    #[test]
    fn an_unpinned_length_is_learned_from_the_content_range() {
        let body = remote((CHUNK_BYTES + 5) as usize);
        let mut io = FakeIo::serving(body.clone());
        let report = run(&mut io, &request("clip.mp4")).expect("ok");
        assert!(report.done);
        assert_eq!(io.file("clip.mp4"), Some(&body));
    }

    #[test]
    fn a_run_stops_at_its_budget_and_the_next_run_resumes() {
        let body = remote((CHUNK_BYTES * 3) as usize);
        let mut io = FakeIo::serving(body.clone());
        io.millis_per_request = RUN_BUDGET_MILLIS / 2 + 1;
        let first = run(&mut io, &request("clip.mp4")).expect("ok");
        assert!(!first.done);
        assert_eq!(first.bytes, CHUNK_BYTES * 2);
        assert_eq!(first.total, Some(CHUNK_BYTES * 3));
        let second = run(&mut io, &request("clip.mp4")).expect("ok");
        assert!(second.done);
        assert_eq!(io.file("clip.mp4"), Some(&body));
    }

    #[test]
    fn a_cancelled_job_stops_before_the_next_chunk() {
        let mut io = FakeIo::serving(remote(100));
        io.cancelled = true;
        let report = run(&mut io, &request("clip.mp4")).expect("ok");
        assert!(!report.done);
        assert_eq!(io.requests, 0);
    }

    #[test]
    fn a_checksum_or_length_mismatch_discards_the_download() {
        let body = remote(1000);
        let mut io = FakeIo::serving(body.clone());
        let wrong_sum = FetchRequest {
            sha256: Some(sha(b"something else")),
            ..request("clip.webm")
        };
        let error = run(&mut io, &wrong_sum).expect_err("checksum fails");
        assert!(error.contains("checksum"), "{error}");
        assert!(io.files.is_empty());

        let wrong_length = FetchRequest {
            expected_bytes: Some(999),
            ..request("clip.webm")
        };
        let error = run(&mut io, &wrong_length).expect_err("length fails");
        assert!(error.contains("1000"), "{error}");
        assert!(io.files.is_empty());
    }

    #[test]
    fn a_server_without_ranges_is_accepted_only_in_one_answer() {
        let body = remote(300);
        let mut io = FakeIo::serving(body.clone());
        io.ranges = false;
        assert!(run(&mut io, &request("small.mp4")).expect("ok").done);
        assert_eq!(io.file("small.mp4"), Some(&body));

        let mut resumed = FakeIo::serving(body);
        resumed.ranges = false;
        resumed.append("big.mp4.part", b"partial").expect("append");
        let error = run(&mut resumed, &request("big.mp4")).expect_err("no ranges");
        assert!(error.contains("partial downloads"), "{error}");
        assert!(resumed.file("big.mp4.part").is_none());
    }

    #[test]
    fn an_http_error_is_reported() {
        let mut io = FakeIo::serving(remote(10));
        io.status = Some(404);
        let error = run(&mut io, &request("gone.mp4")).expect_err("404");
        assert!(error.contains("404"), "{error}");
    }

    #[test]
    fn names_and_addresses_are_checked() {
        for bad in [
            "",
            "../x.mp4",
            "a/b.mp4",
            ".hidden",
            "x.mp4.part",
            "sp ace.mp4",
        ] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
        assert!(check_name("germination-new-02.webm").is_ok());
        let mut io = FakeIo::serving(remote(10));
        let local = FetchRequest {
            url: "file:///etc/passwd".to_owned(),
            ..request("x.mp4")
        };
        assert!(run(&mut io, &local).is_err());
        assert_eq!(io.requests, 0);
    }

    #[test]
    fn the_oldest_files_make_room_for_a_new_download() {
        let mut io = FakeIo::serving(remote(10));
        for (name, modified_millis) in [("newer.mp4", 2), ("old.mp4", 1)] {
            io.listed_only.push(CachedFile {
                name: name.to_owned(),
                length: CACHE_BUDGET_BYTES / 2,
                modified_millis,
            });
        }
        let incoming = FetchRequest {
            expected_bytes: Some(10),
            ..request("next.mp4")
        };
        assert!(run(&mut io, &incoming).expect("ok").done);
        assert_eq!(io.removed, ["old.mp4"]);
        assert_eq!(io.listed_only.len(), 1);
        assert_eq!(io.listed_only[0].name, "newer.mp4");
    }

    #[test]
    fn content_ranges_parse() {
        assert_eq!(
            parse_content_range("bytes 0-99/1000"),
            (Some(0), Some(1000))
        );
        assert_eq!(parse_content_range("bytes */1000"), (None, Some(1000)));
        assert_eq!(parse_content_range("bytes 5-9/*"), (Some(5), None));
        assert_eq!(parse_content_range("items 1-2/3"), (None, None));
    }

    #[test]
    fn the_job_input_and_output_are_json() {
        let error = run_job(b"not json").expect_err("bad input");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        let report: FetchReport =
            serde_json::from_str(r#"{"done":true,"bytes":3,"total":3}"#).expect("report");
        assert!(report.done);
    }
}
