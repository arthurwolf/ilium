//! Where the Video scene gets its clips: the list of entries (a picked file, the videos of
//! picked folders or the Germination catalogue), the folder discovery that builds it, and the
//! [`VideoIo`] seam over the host imports (`fs` on the read-only `picked/<n>` scopes, `media`,
//! `http`, `codec`) so everything above it is plain code a test can drive with a fake.
//!
//! Nothing here waits. Discovery lists a few folders per call and the scene calls it once per
//! frame until it reports `Done`.

use ilium_plugin_sdk::host::{
    EntryKind, HostCodec, HostFs, HostHttp, HostMedia, HttpRequest, VideoInfo,
};
use ilium_plugin_sdk::types::errors::ErrorCode;
use ilium_plugin_sdk::types::net::Header;
use ilium_plugin_sdk::types::paths::FsPath;
use ilium_plugin_sdk::types::RequestId;
use serde::Deserialize;
use std::collections::VecDeque;

/// File extensions the scene plays (matched case-insensitively): the ones the host's `media`
/// import opens.
pub const VIDEO_EXTENSIONS: [&str; 11] = [
    "mp4", "mkv", "avi", "mov", "webm", "m4v", "mpg", "mpeg", "wmv", "flv", "ts",
];
/// Upper bound on the entries one discovery keeps.
pub const MAX_FILES: usize = 20_000;
/// Deepest folder level descended into (the picked folder is level 0).
const MAX_DEPTH: usize = 12;
/// Folders listed at most by one discovery.
const MAX_DIRECTORIES: usize = 2_000;
/// Folders listed by one `Discovery::step`.
const DIRECTORIES_PER_STEP: usize = 8;
/// The host lists at most this many entries of one folder (`fs.list-dir` cap).
pub const LIST_DIR_ENTRIES: u32 = 4_096;
/// Longest note kept, in characters.
const MAX_NOTES: usize = 3;
/// Timeout asked of one download request.
pub const FETCH_TIMEOUT_MILLIS: u32 = 30_000;

/// True when `name` ends with a supported video extension.
pub fn is_video_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty()
            && VIDEO_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
    })
}

/// One clip of the Germination catalogue: a pinned Wikimedia Commons file whose bytes are
/// verified against `download_sha256` before they are played.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CatalogueClip {
    pub id: String,
    pub title: String,
    pub url: String,
    pub author: String,
    pub license: String,
    pub expected_download_bytes: usize,
    pub download_sha256: String,
    /// The mime type `open-video-bytes` takes for this clip.
    pub mime: String,
}

impl CatalogueClip {
    /// The line shown while the clip plays.
    pub fn display_name(&self) -> String {
        format!("{} - {} ({})", self.title, self.author, self.license)
    }
}

/// The shipped catalogue: clips whose container the host opens and that fit the host's
/// 16 MiB in-memory clip limit. An unreadable catalogue is empty.
pub fn catalogue() -> Vec<CatalogueClip> {
    let clips: Vec<CatalogueClip> =
        serde_json::from_str(include_str!("../../assets/germination.json")).unwrap_or_default();
    clips
        .into_iter()
        .filter(|clip| {
            clip.url.starts_with("https://upload.wikimedia.org/")
                && clip.download_sha256.len() == 64
                && clip.expected_download_bytes > 0
                && clip.expected_download_bytes <= MAX_CLIP_BYTES
        })
        .collect()
}

/// The largest clip `open-video-bytes` takes.
pub const MAX_CLIP_BYTES: usize = 16 * 1024 * 1024;

/// Where one entry's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntrySource {
    /// A file of the lent scope `picked/<scope>`; `path` is relative to the lent root and empty
    /// for a lent file.
    File { scope: usize, path: String },
    /// The clip with this index of [`catalogue`].
    Catalogue(usize),
}

/// One clip of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Shown in the status line.
    pub name: String,
    pub source: EntrySource,
}

/// What the scene knows about one folder item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirItem {
    pub name: String,
    pub kind: ItemKind,
}

/// The kinds of folder item discovery cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    File,
    Directory,
    /// A link or special file: never followed.
    Other,
}

/// What the host says about an opened clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipInfo {
    pub width: u32,
    pub height: u32,
    pub frames_per_second: f32,
    pub duration_millis: Option<u64>,
}

impl From<VideoInfo> for ClipInfo {
    fn from(info: VideoInfo) -> Self {
        Self {
            width: info.width,
            height: info.height,
            frames_per_second: info.frames_per_second,
            duration_millis: info.duration_millis,
        }
    }
}

/// An open clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opened {
    pub video: u64,
    pub info: ClipInfo,
}

/// The answer of one non-waiting frame request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramePull {
    /// A packed `rgb8` frame of the requested size.
    Frame(Vec<u8>),
    /// The decoder has not produced the next frame yet.
    Pending,
    /// The clip is over.
    End,
}

/// One finished download request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    Body {
        status: u16,
        bytes: Vec<u8>,
    },
    /// A non-success HTTP status.
    Status(u16),
}

/// The host calls the scene makes. Implemented by [`HostIo`] in the component and by fakes in
/// tests.
pub trait VideoIo: Send {
    /// The items of folder `path` (relative, empty for the lent root) of scope `picked/<scope>`.
    fn list_dir(&mut self, scope: usize, path: &str) -> Result<Vec<DirItem>, String>;
    /// Opens the video `path` of scope `picked/<scope>` (empty path for a picked file).
    fn open_file(&mut self, scope: usize, path: &str) -> Result<Opened, String>;
    /// Opens a clip held in memory (`mime` is the type of its container).
    fn open_bytes(&mut self, bytes: Vec<u8>, mime: &str) -> Result<Opened, String>;
    /// The next frame of `video` at `width` by `height` pixels, without waiting.
    fn next_frame(&mut self, video: u64, width: u32, height: u32) -> Result<FramePull, String>;
    /// Moves `video` to `millis` from its start.
    fn seek(&mut self, video: u64, millis: u64) -> Result<(), String>;
    fn close(&mut self, video: u64);
    /// Starts a download of bytes `start..=end` of `url`; the id is polled with
    /// [`VideoIo::take_fetched`].
    fn fetch_range(&mut self, url: &str, start: u64, end: u64) -> Result<u64, String>;
    /// The finished answer of download `id`, or `None` while it runs.
    fn take_fetched(&mut self, id: u64) -> Option<Fetched>;
    /// The SHA-256 of `bytes` as lower-case hex.
    fn sha256_hex(&mut self, bytes: &[u8]) -> String;
}

/// The host's `fs`, `media`, `http` and `codec` imports.
#[derive(Debug, Default, Clone, Copy)]
pub struct HostIo;

fn picked_path(scope: usize, path: &str) -> FsPath {
    FsPath {
        scope: format!("picked/{scope}"),
        path: path.to_owned(),
    }
}

impl VideoIo for HostIo {
    fn list_dir(&mut self, scope: usize, path: &str) -> Result<Vec<DirItem>, String> {
        let entries = HostFs::list_dir(
            &mut *ilium_plugin_sdk::host(),
            picked_path(scope, path),
            LIST_DIR_ENTRIES,
        )
        .map_err(|error| error.message)?;
        Ok(entries
            .into_iter()
            .map(|entry| DirItem {
                name: entry.name,
                kind: match entry.metadata.kind {
                    EntryKind::File => ItemKind::File,
                    EntryKind::Directory => ItemKind::Directory,
                    EntryKind::Symlink | EntryKind::Other => ItemKind::Other,
                },
            })
            .collect())
    }

    fn open_file(&mut self, scope: usize, path: &str) -> Result<Opened, String> {
        HostMedia::open_video(&mut *ilium_plugin_sdk::host(), picked_path(scope, path))
            .map(|(video, info)| Opened {
                video,
                info: info.into(),
            })
            .map_err(|error| error.message)
    }

    fn open_bytes(&mut self, bytes: Vec<u8>, mime: &str) -> Result<Opened, String> {
        HostMedia::open_video_bytes(&mut *ilium_plugin_sdk::host(), bytes, mime.to_owned())
            .map(|(video, info)| Opened {
                video,
                info: info.into(),
            })
            .map_err(|error| error.message)
    }

    fn next_frame(&mut self, video: u64, width: u32, height: u32) -> Result<FramePull, String> {
        match HostMedia::next_frame(&mut *ilium_plugin_sdk::host(), video, width, height) {
            Ok(Some(frame)) => Ok(FramePull::Frame(frame)),
            Ok(None) => Ok(FramePull::End),
            Err(error) if error.code == ErrorCode::TimedOut => Ok(FramePull::Pending),
            Err(error) => Err(error.message),
        }
    }

    fn seek(&mut self, video: u64, millis: u64) -> Result<(), String> {
        HostMedia::seek(&mut *ilium_plugin_sdk::host(), video, millis)
            .map_err(|error| error.message)
    }

    fn close(&mut self, video: u64) {
        HostMedia::close_video(&mut *ilium_plugin_sdk::host(), video);
    }

    fn fetch_range(&mut self, url: &str, start: u64, end: u64) -> Result<u64, String> {
        HostHttp::fetch(
            &mut *ilium_plugin_sdk::host(),
            HttpRequest {
                method: "GET".into(),
                url: url.into(),
                headers: vec![Header {
                    name: "Range".into(),
                    value: format!("bytes={start}-{end}"),
                }],
                body: None,
                timeout_millis: FETCH_TIMEOUT_MILLIS,
            },
        )
        .map(|RequestId(id)| id)
        .map_err(|error| error.message)
    }

    fn take_fetched(&mut self, id: u64) -> Option<Fetched> {
        let response = HostHttp::take_response(&mut *ilium_plugin_sdk::host(), RequestId(id))?;
        Some(if (200..300).contains(&response.status) {
            Fetched::Body {
                status: response.status,
                bytes: response.body,
            }
        } else {
            Fetched::Status(response.status)
        })
    }

    fn sha256_hex(&mut self, bytes: &[u8]) -> String {
        let digest = HostCodec::sha256(&mut *ilium_plugin_sdk::host(), bytes.to_vec());
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// How one `Discovery::step` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Working,
    Done,
}

/// The folder walk, advanced a few folders per call.
pub struct Discovery {
    queue: VecDeque<(usize, String, usize)>,
    recursive: bool,
    entries: Vec<Entry>,
    notes: Vec<String>,
    listed: usize,
    failed_roots: usize,
    roots: usize,
}

/// The finished list.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Discovered {
    pub entries: Vec<Entry>,
    /// Short remarks for the status line (a folder that could not be read, a cap that was hit).
    pub notes: Vec<String>,
    /// Set when no folder could be read at all or nothing was found.
    pub error: Option<String>,
}

impl Discovery {
    /// Walks each `scope` folder; `recursive` descends into sub-folders.
    pub fn new(scopes: &[usize], recursive: bool) -> Self {
        Self {
            queue: scopes
                .iter()
                .map(|scope| (*scope, String::new(), 0))
                .collect(),
            recursive,
            entries: Vec::new(),
            notes: Vec::new(),
            listed: 0,
            failed_roots: 0,
            roots: scopes.len(),
        }
    }

    fn note(&mut self, text: String) {
        if self.notes.len() < MAX_NOTES && !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }

    /// Lists up to a few folders. `Done` once the queue is empty or a cap was hit.
    pub fn step(&mut self, io: &mut dyn VideoIo) -> Progress {
        for _ in 0..DIRECTORIES_PER_STEP {
            let Some((scope, path, depth)) = self.queue.pop_front() else {
                return Progress::Done;
            };
            if self.listed >= MAX_DIRECTORIES {
                self.note("folder limit reached".to_owned());
                self.queue.clear();
                return Progress::Done;
            }
            self.listed += 1;
            let items = match io.list_dir(scope, &path) {
                Ok(items) => items,
                Err(message) => {
                    if depth == 0 {
                        self.failed_roots += 1;
                    }
                    self.note(format!("unreadable folder: {message}"));
                    continue;
                }
            };
            if items.len() >= LIST_DIR_ENTRIES as usize {
                self.note("a folder was cut at 4096 items".to_owned());
            }
            for item in items {
                let relative = if path.is_empty() {
                    item.name.clone()
                } else {
                    format!("{path}/{}", item.name)
                };
                match item.kind {
                    ItemKind::Directory => {
                        if self.recursive && depth < MAX_DEPTH {
                            self.queue.push_back((scope, relative, depth + 1));
                        }
                    }
                    ItemKind::File if is_video_name(&item.name) => {
                        if self.entries.len() >= MAX_FILES {
                            self.note("more than 20000 videos: the rest are left out".to_owned());
                            self.queue.clear();
                            return Progress::Done;
                        }
                        self.entries.push(Entry {
                            name: relative.clone(),
                            source: EntrySource::File {
                                scope,
                                path: relative,
                            },
                        });
                    }
                    ItemKind::File | ItemKind::Other => {}
                }
            }
        }
        if self.queue.is_empty() {
            Progress::Done
        } else {
            Progress::Working
        }
    }

    /// The finished list, sorted by name.
    pub fn finish(mut self) -> Discovered {
        self.entries.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.name.cmp(&right.name))
        });
        let error = if self.roots == 0 {
            Some("Choose a folder of videos".to_owned())
        } else if self.failed_roots == self.roots {
            Some(
                self.notes
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "The folder could not be read".to_owned()),
            )
        } else if self.entries.is_empty() {
            Some("No videos found in the chosen folders".to_owned())
        } else {
            None
        };
        Discovered {
            entries: self.entries,
            notes: self.notes,
            error,
        }
    }
}

#[cfg(test)]
pub(super) mod fake {
    use super::*;
    use std::collections::HashMap;

    /// What a scripted clip does when it is played.
    #[derive(Debug, Clone)]
    pub struct ClipScript {
        pub info: ClipInfo,
        /// Frames the clip has; each is `[level; width * height * 3]` of the requested size.
        pub frames: u64,
        /// `Pending` answers before every frame.
        pub pending_between: u32,
        pub level: u8,
    }

    impl ClipScript {
        pub fn new(width: u32, height: u32, fps: f32, frames: u64) -> Self {
            Self {
                info: ClipInfo {
                    width,
                    height,
                    frames_per_second: fps,
                    duration_millis: Some((frames as f64 * 1000.0 / f64::from(fps)) as u64),
                },
                frames,
                pending_between: 0,
                level: 200,
            }
        }
    }

    struct Playing {
        script: ClipScript,
        position: u64,
        waits: u32,
    }

    /// A scripted [`VideoIo`]: folders keyed by scope and path, clips keyed by scope and path,
    /// downloads answered at once or after a number of polls.
    #[derive(Default)]
    pub struct FakeIo {
        pub folders: HashMap<(usize, String), Vec<DirItem>>,
        /// `(scope, file path)` to the clip it opens as.
        pub clips: HashMap<(usize, String), ClipScript>,
        /// The clip any `open_bytes` opens as.
        pub bytes_clip: Option<ClipScript>,
        /// `(start, end)` of each range request to the answer it gets.
        pub body: Vec<u8>,
        pub status: u16,
        pub open_calls: Vec<(usize, String)>,
        pub opened_bytes: Vec<usize>,
        pub seeks: Vec<(u64, u64)>,
        pub closed: Vec<u64>,
        pub requested_sizes: Vec<(u32, u32)>,
        pub ranges: Vec<(u64, u64)>,
        playing: HashMap<u64, Playing>,
        running: HashMap<u64, (u64, u64)>,
        next_video: u64,
        next_request: u64,
    }

    impl FakeIo {
        /// A fake whose downloads answer from `body` with HTTP status `status`.
        pub fn serving(body: Vec<u8>, status: u16) -> Self {
            Self {
                body,
                status,
                ..Self::default()
            }
        }

        pub fn folder(&mut self, scope: usize, path: &str, items: &[(&str, ItemKind)]) {
            self.folders.insert(
                (scope, path.to_owned()),
                items
                    .iter()
                    .map(|(name, kind)| DirItem {
                        name: (*name).to_owned(),
                        kind: *kind,
                    })
                    .collect(),
            );
        }

        pub fn clip(&mut self, scope: usize, path: &str, script: ClipScript) {
            self.clips.insert((scope, path.to_owned()), script);
        }

        fn start(&mut self, script: ClipScript) -> Opened {
            self.next_video += 1;
            let video = self.next_video;
            let info = script.info;
            self.playing.insert(
                video,
                Playing {
                    script,
                    position: 0,
                    waits: 0,
                },
            );
            Opened { video, info }
        }
    }

    impl VideoIo for FakeIo {
        fn list_dir(&mut self, scope: usize, path: &str) -> Result<Vec<DirItem>, String> {
            self.folders
                .get(&(scope, path.to_owned()))
                .cloned()
                .ok_or_else(|| "not found".to_owned())
        }

        fn open_file(&mut self, scope: usize, path: &str) -> Result<Opened, String> {
            self.open_calls.push((scope, path.to_owned()));
            let script = self
                .clips
                .get(&(scope, path.to_owned()))
                .cloned()
                .ok_or_else(|| "not a video".to_owned())?;
            Ok(self.start(script))
        }

        fn open_bytes(&mut self, bytes: Vec<u8>, _mime: &str) -> Result<Opened, String> {
            self.opened_bytes.push(bytes.len());
            let script = self
                .bytes_clip
                .clone()
                .ok_or_else(|| "not a video".to_owned())?;
            Ok(self.start(script))
        }

        fn next_frame(&mut self, video: u64, width: u32, height: u32) -> Result<FramePull, String> {
            self.requested_sizes.push((width, height));
            let playing = self.playing.get_mut(&video).ok_or("unknown video")?;
            if playing.position >= playing.script.frames {
                return Ok(FramePull::End);
            }
            if playing.waits < playing.script.pending_between {
                playing.waits += 1;
                return Ok(FramePull::Pending);
            }
            playing.waits = 0;
            playing.position += 1;
            let size = (width * height * 3) as usize;
            Ok(FramePull::Frame(vec![playing.script.level; size]))
        }

        fn seek(&mut self, video: u64, millis: u64) -> Result<(), String> {
            self.seeks.push((video, millis));
            let playing = self.playing.get_mut(&video).ok_or("unknown video")?;
            let frame_millis = 1000.0 / f64::from(playing.script.info.frames_per_second);
            playing.position = (millis as f64 / frame_millis) as u64;
            Ok(())
        }

        fn close(&mut self, video: u64) {
            self.closed.push(video);
            self.playing.remove(&video);
        }

        fn fetch_range(&mut self, _url: &str, start: u64, end: u64) -> Result<u64, String> {
            self.ranges.push((start, end));
            self.next_request += 1;
            self.running.insert(self.next_request, (start, end));
            Ok(self.next_request)
        }

        fn take_fetched(&mut self, id: u64) -> Option<Fetched> {
            let (start, end) = self.running.remove(&id)?;
            if !(200..300).contains(&self.status) {
                return Some(Fetched::Status(self.status));
            }
            let from = (start as usize).min(self.body.len());
            let to = ((end + 1) as usize).min(self.body.len());
            Some(Fetched::Body {
                status: self.status,
                bytes: self.body[from..to].to_vec(),
            })
        }

        fn sha256_hex(&mut self, bytes: &[u8]) -> String {
            // A stand-in digest: the tests give the catalogue entry this very text.
            format!("fake-sha256-{}", bytes.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeIo;
    use super::*;

    #[test]
    fn video_names_are_matched_by_extension_ignoring_case() {
        assert!(is_video_name("a.mp4"));
        assert!(is_video_name("Holiday.MKV"));
        assert!(is_video_name("deep/clip.webm"));
        assert!(!is_video_name("a.txt"));
        assert!(!is_video_name(".mp4"));
        assert!(!is_video_name("mp4"));
    }

    #[test]
    fn the_catalogue_keeps_only_clips_the_host_can_open() {
        let clips = catalogue();
        assert!(clips.len() >= 40, "{} clips", clips.len());
        for clip in &clips {
            assert!(clip.url.starts_with("https://upload.wikimedia.org/"));
            assert!(clip.expected_download_bytes <= MAX_CLIP_BYTES);
            assert!(clip.mime.starts_with("video/"));
            assert_eq!(clip.download_sha256.len(), 64);
        }
        let mut ids: Vec<&str> = clips.iter().map(|clip| clip.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), clips.len());
        assert!(clips[0].display_name().contains(&clips[0].author));
    }

    #[test]
    fn discovery_finds_videos_in_sub_folders_sorted_by_name() {
        let mut io = FakeIo::default();
        io.folder(
            1,
            "",
            &[
                ("b.mp4", ItemKind::File),
                ("notes.txt", ItemKind::File),
                ("sub", ItemKind::Directory),
                ("link", ItemKind::Other),
            ],
        );
        io.folder(
            1,
            "sub",
            &[("A.webm", ItemKind::File), ("z.MOV", ItemKind::File)],
        );
        let mut discovery = Discovery::new(&[1], true);
        let mut steps = 0;
        while discovery.step(&mut io) == Progress::Working {
            steps += 1;
            assert!(steps < 10);
        }
        let found = discovery.finish();
        let names: Vec<&str> = found
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, vec!["b.mp4", "sub/A.webm", "sub/z.MOV"]);
        assert_eq!(found.error, None);
        assert_eq!(
            found.entries[1].source,
            EntrySource::File {
                scope: 1,
                path: "sub/A.webm".into()
            }
        );
    }

    #[test]
    fn discovery_without_recursion_stays_in_the_folder() {
        let mut io = FakeIo::default();
        io.folder(
            1,
            "",
            &[("a.mp4", ItemKind::File), ("sub", ItemKind::Directory)],
        );
        io.folder(1, "sub", &[("deep.mp4", ItemKind::File)]);
        let mut discovery = Discovery::new(&[1], false);
        while discovery.step(&mut io) == Progress::Working {}
        assert_eq!(discovery.finish().entries.len(), 1);
    }

    #[test]
    fn discovery_reports_empty_and_unreadable_folders() {
        let mut io = FakeIo::default();
        io.folder(1, "", &[("a.txt", ItemKind::File)]);
        let mut empty = Discovery::new(&[1], true);
        while empty.step(&mut io) == Progress::Working {}
        assert_eq!(
            empty.finish().error.as_deref(),
            Some("No videos found in the chosen folders")
        );
        let mut missing = Discovery::new(&[2], true);
        while missing.step(&mut io) == Progress::Working {}
        let found = missing.finish();
        assert!(found
            .error
            .as_deref()
            .is_some_and(|text| text.contains("unreadable")));
        let none = Discovery::new(&[], true).finish();
        assert_eq!(none.error.as_deref(), Some("Choose a folder of videos"));
    }
}
