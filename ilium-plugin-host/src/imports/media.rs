//! Host implementation of `ilium:host/media` (api-media): video decoding for plugin surfaces.
//!
//! The import owns the contract rules: the accepted extensions and mime types, the frame size
//! bounds, the open-video quota, the playback position and the range of `open-video-range`. A
//! path is found through [`VideoFiles`], which in a host is the instance's own `fs` rules
//! ([`FsReadResolver`]), so a plugin opens only a file it could read. Decoding is a [`MediaBackend`] the composition root supplies
//! (`wiring::wave_two::ClientBackendInputs::media`): the client runs `ffmpeg` in its own process,
//! decoding ahead into a bounded buffer. Without a backend every open answers `unsupported`.
//!
//! A video opened with `open-video-range` is the clip between its two times: its `video-info`
//! states the clip's length and `seek` counts from the clip's start (api-media "Video from bytes
//! and ranges"). The position of a video is the time of the next frame it returns: where the
//! decoder last started plus the frames returned since, at the stated frame rate. A new frame
//! size or a seek restarts the decoder at that position, so a plugin may resize its surface at
//! any time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ilium_plugin_api::generated::host::media::{HostMedia, VideoInfo};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::paths::FsPath;

use super::env::ImportEnv;
use super::fs::FsReadResolver;

/// Grant that unlocks the interface.
pub const VIDEO_GRANT: &str = "media.video";

const ACCEPTED_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "m4v", "mpg", "mpeg", "wmv", "flv", "ts",
];
/// Accepted mime types of `open-video-bytes` with the extension the spooled file gets.
const ACCEPTED_MIME_TYPES: &[(&str, &str)] = &[
    ("video/mp4", "mp4"),
    ("video/x-matroska", "mkv"),
    ("video/x-msvideo", "avi"),
    ("video/quicktime", "mov"),
    ("video/webm", "webm"),
    ("video/mpeg", "mpg"),
    ("video/x-ms-wmv", "wmv"),
    ("video/x-flv", "flv"),
    ("video/mp2t", "ts"),
];
/// Scopes the `fs` picker hands out; the chosen file itself has an empty path.
const PICKED_SCOPE_PREFIX: &str = "picked/";
/// Largest in-memory clip, in bytes.
const MAX_CLIP_BYTES: usize = 16 * 1024 * 1024;
const MAX_FRAME_WIDTH: u32 = 3_840;
const MAX_FRAME_HEIGHT: u32 = 2_160;
/// Videos one instance may hold open at once (api-media "Open videos", the default).
const OPEN_VIDEO_LIMIT: usize = 2;
/// Frames decoded ahead per open video, in bytes (`video_buffer_mib`, 64 MiB by default).
pub const DECODE_AHEAD_BYTES: usize = 64 * 1024 * 1024;
/// Frame rate assumed when a container states none.
const FALLBACK_FRAMES_PER_SECOND: f64 = 25.0;

/// One decoder run: decode `file` from `start_millis` up to `end_millis`, scaled to exactly
/// `width` by `height`, as packed `rgb8` frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeRequest {
    pub file: PathBuf,
    pub start_millis: u64,
    pub end_millis: Option<u64>,
    pub width: u32,
    pub height: u32,
    /// How many bytes of frames the decoder may hold ahead of the plugin; always at least one
    /// frame.
    pub buffer_bytes: usize,
}

/// The frames of one decoder run, in order. Dropping it stops the decoder.
pub trait FrameSource: Send {
    /// The next `rgb8` frame of `width * height * 3` bytes, `None` at the end of the run.
    fn next_frame(&mut self) -> Result<Option<Vec<u8>>, PluginError>;
}

/// Finds the real file behind a plugin's `fs-path`.
pub trait VideoFiles: Send + Sync {
    /// The file behind `path`: `not-found` when no file is there, `not-permitted` when the
    /// instance may not read it.
    fn locate(&self, path: &FsPath) -> Result<PathBuf, PluginError>;
}

impl VideoFiles for FsReadResolver {
    fn locate(&self, path: &FsPath) -> Result<PathBuf, PluginError> {
        let file = self.resolve_read(path)?;
        if !file.is_file() {
            return Err(not_found(path));
        }
        Ok(file)
    }
}

/// The video decoder of a process.
pub trait MediaBackend: Send + Sync {
    /// Describes the video in `file`; a file without a video stream is `unsupported`.
    fn probe(&self, file: &Path) -> Result<VideoInfo, PluginError>;

    /// Starts one decoder run.
    fn decode(&self, request: &DecodeRequest) -> Result<Box<dyn FrameSource>, PluginError>;
}

impl std::fmt::Debug for dyn MediaBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MediaBackend")
    }
}

/// The running decoder of an open video and how far it got.
struct RunningDecoder {
    width: u32,
    height: u32,
    /// Position the run started at.
    started_at_millis: u64,
    frames_returned: u64,
    source: Box<dyn FrameSource>,
}

/// One open video.
struct OpenVideo {
    file: PathBuf,
    /// The file is a spooled copy of `open-video-bytes` and is removed on close.
    spooled: bool,
    info: VideoInfo,
    /// Where the clip starts in the file.
    start_millis: u64,
    /// Where the clip ends in the file, if it states an end.
    end_millis: Option<u64>,
    /// Length of the clip, when known; `seek` may not pass it.
    clip_millis: Option<u64>,
    /// Position of the next frame in the file while no decoder runs.
    position_millis: u64,
    /// The decoder reached the end of the range; `next-frame` answers `none` until a seek.
    ended: bool,
    decoder: Option<RunningDecoder>,
}

impl OpenVideo {
    fn frame_millis(&self) -> f64 {
        let stated = f64::from(self.info.frames_per_second);
        let rate = if stated.is_finite() && stated > 0.0 {
            stated
        } else {
            FALLBACK_FRAMES_PER_SECOND
        };
        1_000.0 / rate
    }

    /// Time of the next frame.
    fn position(&self) -> u64 {
        match &self.decoder {
            None => self.position_millis,
            Some(decoder) => {
                let elapsed = decoder.frames_returned as f64 * self.frame_millis();
                decoder
                    .started_at_millis
                    .saturating_add(elapsed.round() as u64)
            }
        }
    }

    fn stop_decoder(&mut self) {
        self.position_millis = self.position();
        self.decoder = None;
    }
}

impl Drop for OpenVideo {
    fn drop(&mut self) {
        self.decoder = None;
        if self.spooled {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

/// Host side of the `media` interface of one plugin instance.
pub struct MediaImport {
    env: ImportEnv,
    files: Option<Arc<dyn VideoFiles>>,
    backend: Option<Arc<dyn MediaBackend>>,
    /// Host-private folder for the clips of `open-video-bytes`.
    spool_dir: Option<PathBuf>,
    videos: HashMap<u64, OpenVideo>,
    next_video: u64,
}

impl std::fmt::Debug for MediaImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaImport")
            .field("plugin", &self.env.plugin())
            .field("backend", &self.backend.is_some())
            .field("open_videos", &self.videos.len())
            .finish()
    }
}

impl Default for MediaImport {
    /// Every grant held, no file scope and no decoder: opening answers `not-found` or
    /// `unsupported`.
    fn default() -> Self {
        Self::new(ImportEnv::unrestricted(), None, None, None)
    }
}

impl MediaImport {
    /// The import of the instance `env` describes. `files` finds the plugin's files, `backend`
    /// decodes them and `spool_dir` receives the clips of `open-video-bytes`.
    pub fn new(
        env: ImportEnv,
        files: Option<Arc<dyn VideoFiles>>,
        backend: Option<Arc<dyn MediaBackend>>,
        spool_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            env,
            files,
            backend,
            spool_dir,
            videos: HashMap::new(),
            next_video: 0,
        }
    }

    /// How many videos are open.
    pub fn open_count(&self) -> usize {
        self.videos.len()
    }

    /// Where an open video is, in milliseconds from the start of its clip.
    pub fn position_millis(&self, video: u64) -> Option<u64> {
        self.videos
            .get(&video)
            .map(|open| open.position().saturating_sub(open.start_millis))
    }

    fn require_grant(&self) -> Result<(), PluginError> {
        if self.env.holds(VIDEO_GRANT) {
            Ok(())
        } else {
            Err(PluginError::new(
                ErrorCode::NotPermitted,
                "the media.video grant is not held",
            ))
        }
    }

    fn require_capacity(&self) -> Result<(), PluginError> {
        if self.videos.len() >= OPEN_VIDEO_LIMIT {
            return Err(PluginError::new(
                ErrorCode::QuotaExceeded,
                format!("at most {OPEN_VIDEO_LIMIT} videos may be open at once"),
            ));
        }
        Ok(())
    }

    fn backend(&self) -> Result<Arc<dyn MediaBackend>, PluginError> {
        self.backend.clone().ok_or_else(|| {
            PluginError::new(
                ErrorCode::Unsupported,
                "no video decoder is attached to this host",
            )
        })
    }

    /// The real file behind `path`, after the extension and scope checks. A picked file
    /// (`picked/<n>` with an empty path) is checked by the name of the file the user chose.
    fn readable_file(&self, path: &FsPath) -> Result<PathBuf, PluginError> {
        let picked_file = path.path.is_empty() && path.scope.starts_with(PICKED_SCOPE_PREFIX);
        if !picked_file {
            check_extension(&path.path)?;
        }
        let Some(files) = &self.files else {
            return Err(not_found(path));
        };
        let file = files.locate(path)?;
        if picked_file {
            check_extension(&file.to_string_lossy())?;
        }
        Ok(file)
    }

    /// Probes `file` and records it as a new open video.
    fn open_file(
        &mut self,
        file: PathBuf,
        spooled: bool,
        start_millis: u64,
        end_millis: Option<u64>,
    ) -> Result<(u64, VideoInfo), PluginError> {
        let probed = self.backend().and_then(|backend| backend.probe(&file));
        let clipped = probed.and_then(|info| clip_info(info, start_millis, end_millis));
        let info = match clipped {
            Ok(info) => info,
            Err(error) => {
                if spooled {
                    let _ = std::fs::remove_file(&file);
                }
                return Err(error);
            }
        };
        self.next_video += 1;
        let video = self.next_video;
        self.videos.insert(
            video,
            OpenVideo {
                file,
                spooled,
                info: info.clone(),
                start_millis,
                end_millis,
                clip_millis: info.duration_millis,
                position_millis: start_millis,
                ended: false,
                decoder: None,
            },
        );
        Ok((video, info))
    }

    fn video_mut(&mut self, video: u64) -> Result<&mut OpenVideo, PluginError> {
        self.videos
            .get_mut(&video)
            .ok_or_else(|| unknown_video(video))
    }
}

/// Refuses an empty path and an extension outside the accepted list.
fn check_extension(path: &str) -> Result<(), PluginError> {
    if path.is_empty() {
        return Err(PluginError::new(
            ErrorCode::InvalidArgument,
            "empty video path",
        ));
    }
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    if !ACCEPTED_EXTENSIONS.contains(&extension.as_str()) {
        return Err(PluginError::new(
            ErrorCode::Unsupported,
            format!("video extension `{extension}` is not accepted"),
        ));
    }
    Ok(())
}

/// The description of the clip from `start_millis` to `end_millis` of a video described by
/// `info`: its length is what remains of the file between the two times. A clip that starts at
/// or after the end of the file is refused.
fn clip_info(
    mut info: VideoInfo,
    start_millis: u64,
    end_millis: Option<u64>,
) -> Result<VideoInfo, PluginError> {
    if let Some(duration) = info.duration_millis {
        if start_millis > 0 && start_millis >= duration {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                format!("the range starts at {start_millis} ms, after the video's {duration} ms"),
            ));
        }
    }
    let clip_end = match (end_millis, info.duration_millis) {
        (Some(end), Some(duration)) => Some(end.min(duration)),
        (Some(end), None) => Some(end),
        (None, duration) => duration,
    };
    info.duration_millis = clip_end.map(|end| end.saturating_sub(start_millis));
    Ok(info)
}

fn check_frame_size(width: u32, height: u32) -> Result<(), PluginError> {
    if width == 0 || height == 0 || width > MAX_FRAME_WIDTH || height > MAX_FRAME_HEIGHT {
        return Err(PluginError::new(
            ErrorCode::InvalidArgument,
            "frame size must be 1..=3840 by 1..=2160",
        ));
    }
    Ok(())
}

fn not_found(path: &FsPath) -> PluginError {
    PluginError::new(
        ErrorCode::NotFound,
        format!("video `{}` was not found", path.path),
    )
}

fn unknown_video(video: u64) -> PluginError {
    PluginError::new(
        ErrorCode::NotFound,
        format!("video {video} is unknown or closed"),
    )
}

impl HostMedia for MediaImport {
    fn open_video(&mut self, path: FsPath) -> Result<(u64, VideoInfo), PluginError> {
        self.require_grant()?;
        let file = self.readable_file(&path)?;
        self.require_capacity()?;
        self.open_file(file, false, 0, None)
    }

    fn open_video_bytes(
        &mut self,
        bytes: Vec<u8>,
        mime: String,
    ) -> Result<(u64, VideoInfo), PluginError> {
        self.require_grant()?;
        if bytes.is_empty() || bytes.len() > MAX_CLIP_BYTES {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "video bytes must be between 1 byte and 16 MiB",
            ));
        }
        let Some((_, extension)) = ACCEPTED_MIME_TYPES
            .iter()
            .find(|(accepted, _)| *accepted == mime.as_str())
        else {
            return Err(PluginError::new(
                ErrorCode::Unsupported,
                format!("mime type `{mime}` is not accepted"),
            ));
        };
        self.backend()?;
        let Some(spool_dir) = self.spool_dir.clone() else {
            return Err(PluginError::new(
                ErrorCode::Unsupported,
                "this host keeps no folder for in-memory clips",
            ));
        };
        self.require_capacity()?;
        std::fs::create_dir_all(&spool_dir)
            .map_err(|error| PluginError::new(ErrorCode::Io, error.to_string()))?;
        let file = spool_dir.join(format!(
            "{}-{}-{}.{extension}",
            self.env.instance(),
            std::process::id(),
            self.env.ids().next_call()
        ));
        std::fs::write(&file, &bytes)
            .map_err(|error| PluginError::new(ErrorCode::Io, error.to_string()))?;
        self.open_file(file, true, 0, None)
    }

    fn open_video_range(
        &mut self,
        path: FsPath,
        start_millis: u64,
        end_millis: Option<u64>,
    ) -> Result<(u64, VideoInfo), PluginError> {
        self.require_grant()?;
        if end_millis.is_some_and(|end| end <= start_millis) {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "the range must end after it starts",
            ));
        }
        let file = self.readable_file(&path)?;
        self.require_capacity()?;
        self.open_file(file, false, start_millis, end_millis)
    }

    fn next_frame(
        &mut self,
        video: u64,
        width: u32,
        height: u32,
    ) -> Result<Option<Vec<u8>>, PluginError> {
        check_frame_size(width, height)?;
        let backend = self.backend.clone();
        let open = self.video_mut(video)?;
        if open.ended {
            return Ok(None);
        }
        let resized = open
            .decoder
            .as_ref()
            .is_some_and(|decoder| (decoder.width, decoder.height) != (width, height));
        if resized {
            open.stop_decoder();
        }
        if open.decoder.is_none() {
            let Some(backend) = backend else {
                return Err(PluginError::new(
                    ErrorCode::Unsupported,
                    "no video decoder is attached to this host",
                ));
            };
            let request = DecodeRequest {
                file: open.file.clone(),
                start_millis: open.position_millis,
                end_millis: open.end_millis,
                width,
                height,
                buffer_bytes: DECODE_AHEAD_BYTES,
            };
            let source = backend.decode(&request)?;
            open.decoder = Some(RunningDecoder {
                width,
                height,
                started_at_millis: request.start_millis,
                frames_returned: 0,
                source,
            });
        }
        let Some(decoder) = open.decoder.as_mut() else {
            return Ok(None);
        };
        match decoder.source.next_frame() {
            Ok(Some(frame)) => {
                decoder.frames_returned += 1;
                Ok(Some(frame))
            }
            Ok(None) => {
                open.stop_decoder();
                open.ended = true;
                Ok(None)
            }
            Err(error) => {
                // A failed decoder fails this call only; the next call starts a new one.
                open.stop_decoder();
                Err(error)
            }
        }
    }

    fn seek(&mut self, video: u64, millis: u64) -> Result<(), PluginError> {
        let open = self.video_mut(video)?;
        if let Some(clip) = open.clip_millis {
            if millis > clip {
                return Err(PluginError::new(
                    ErrorCode::InvalidArgument,
                    format!("cannot seek to {millis} ms in a {clip} ms video"),
                ));
            }
        }
        open.decoder = None;
        open.position_millis = open.start_millis.saturating_add(millis);
        open.ended = false;
        Ok(())
    }

    fn close_video(&mut self, video: u64) {
        // Dropping the entry stops its decoder and removes a spooled clip.
        self.videos.remove(&video);
    }
}

#[cfg(test)]
mod tests {
    use super::{DecodeRequest, FrameSource, MediaBackend, MediaImport, VideoFiles};
    use crate::imports::env::ImportEnv;
    use crate::imports::fs::FsImport;
    use ilium_plugin_api::generated::host::media::{HostMedia, VideoInfo};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::paths::FsPath;
    use ilium_plugin_api::grants::Grant;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    const MIB: usize = 1024 * 1024;
    /// An id that no open call in these tests can return, so it is always unknown or closed.
    const UNKNOWN_VIDEO_ID: u64 = 12_345;
    const ACCEPTED_EXTENSIONS: &[&str] = &[
        "mp4", "mkv", "avi", "mov", "webm", "m4v", "mpg", "mpeg", "wmv", "flv", "ts",
    ];
    const ACCEPTED_MIME_TYPES: &[&str] = &[
        "video/mp4",
        "video/x-matroska",
        "video/x-msvideo",
        "video/quicktime",
        "video/webm",
        "video/mpeg",
        "video/x-ms-wmv",
        "video/x-flv",
        "video/mp2t",
    ];

    fn clip(path: &str) -> FsPath {
        FsPath {
            scope: "project".to_owned(),
            path: path.to_owned(),
        }
    }

    /// Returns the error code of a failed call, and fails the test when the call succeeded.
    fn error_code_of<T: std::fmt::Debug>(result: Result<T, PluginError>) -> ErrorCode {
        match result {
            Ok(value) => panic!("expected a plugin error, got Ok({value:?})"),
            Err(error) => error.code,
        }
    }

    /// A decoder that returns `frames` frames of one repeated byte per run, numbering the runs.
    #[derive(Default)]
    struct FakeDecoder {
        frames: u64,
        runs: Mutex<Vec<DecodeRequest>>,
    }

    struct FakeRun {
        left: u64,
        bytes: usize,
        fill: u8,
    }

    impl FrameSource for FakeRun {
        fn next_frame(&mut self) -> Result<Option<Vec<u8>>, PluginError> {
            if self.left == 0 {
                return Ok(None);
            }
            self.left -= 1;
            Ok(Some(vec![self.fill; self.bytes]))
        }
    }

    impl MediaBackend for FakeDecoder {
        fn probe(&self, file: &Path) -> Result<VideoInfo, PluginError> {
            if std::fs::read(file)
                .map_err(|error| PluginError::new(ErrorCode::Io, error.to_string()))?
                == b"not a video"
            {
                return Err(PluginError::new(ErrorCode::Unsupported, "no video stream"));
            }
            Ok(VideoInfo {
                width: 640,
                height: 360,
                frames_per_second: 25.0,
                duration_millis: Some(4_000),
            })
        }

        fn decode(&self, request: &DecodeRequest) -> Result<Box<dyn FrameSource>, PluginError> {
            let mut runs = self.runs.lock().expect("runs");
            runs.push(request.clone());
            Ok(Box::new(FakeRun {
                left: self.frames,
                bytes: (request.width * request.height * 3) as usize,
                fill: u8::try_from(runs.len()).unwrap_or(u8::MAX),
            }))
        }
    }

    /// A media import over a project folder holding `files`, granted `grants`.
    struct Rig {
        _scratch: tempfile::TempDir,
        project: PathBuf,
        spool: PathBuf,
        decoder: Arc<FakeDecoder>,
        media: MediaImport,
    }

    fn rig(grants: &[&str], files: &[(&str, &[u8])], frames: u64) -> Rig {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let project = scratch.path().join("project");
        std::fs::create_dir_all(&project).expect("project folder");
        for (name, bytes) in files {
            std::fs::write(project.join(name), bytes).expect("clip written");
        }
        let granted: Vec<String> = grants.iter().map(|grant| (*grant).to_owned()).collect();
        let env = ImportEnv::unrestricted()
            .with_identity("test.media", 1)
            .with_grants(granted.clone());
        let mut fs = FsImport::new(env.clone());
        let root = scratch.path().join("fs");
        fs.configure_scope_roots(root.join("package"), root.join("data"), root.join("cache"))
            .expect("scopes");
        let parsed: Vec<Grant> = granted
            .iter()
            .filter_map(|grant| Grant::parse(grant).ok())
            .collect();
        fs.configure_project_scope(project.clone(), Arc::new(Mutex::new(parsed)))
            .expect("project scope");
        let resolver = fs.read_resolver().expect("resolver");
        let decoder = Arc::new(FakeDecoder {
            frames,
            ..FakeDecoder::default()
        });
        let spool = scratch.path().join("spool");
        let media = MediaImport::new(
            env,
            Some(Arc::new(resolver) as Arc<dyn VideoFiles>),
            Some(Arc::clone(&decoder) as Arc<dyn MediaBackend>),
            Some(spool.clone()),
        );
        Rig {
            _scratch: scratch,
            project,
            spool,
            decoder,
            media,
        }
    }

    const PLAYING: &[&str] = &["media.video", "fs.read:project"];

    /// REQ: WIT:ilium:host/media.open-video
    #[test]
    fn open_video_missing_file_is_not_found() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.open_video(clip("nope.mp4"))),
            ErrorCode::NotFound
        );
    }

    /// REQ: WIT:ilium:host/media.open-video
    #[test]
    fn open_video_extension_outside_the_accepted_list_is_unsupported() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.open_video(clip("notes.txt"))),
            ErrorCode::Unsupported
        );
    }

    /// REQ: WIT:ilium:host/media.open-video
    #[test]
    fn open_video_every_accepted_extension_is_not_refused_as_unsupported() {
        let mut media = MediaImport::default();
        for extension in ACCEPTED_EXTENSIONS {
            let outcome = media.open_video(clip(&format!("clip.{extension}")));
            if let Err(error) = &outcome {
                assert_ne!(
                    error.code,
                    ErrorCode::Unsupported,
                    "extension {extension} is documented as accepted"
                );
            }
        }
    }

    /// REQ: WIT:ilium:host/media.open-video
    #[test]
    fn open_video_empty_path_is_an_error() {
        let mut media = MediaImport::default();
        assert!(media.open_video(clip("")).is_err());
    }

    /// REQ: WIT:ilium:host/media.open-video-bytes
    #[test]
    fn open_video_bytes_empty_input_is_invalid_argument() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.open_video_bytes(Vec::new(), "video/mp4".to_owned())),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.open-video-bytes
    #[test]
    fn open_video_bytes_at_sixteen_mib_is_not_refused_for_size() {
        let mut media = MediaImport::default();
        let outcome = media.open_video_bytes(vec![0u8; 16 * MIB], "video/mp4".to_owned());
        if let Err(error) = &outcome {
            assert_ne!(
                error.code,
                ErrorCode::InvalidArgument,
                "16 MiB is the documented maximum and must be accepted"
            );
        }
    }

    /// REQ: WIT:ilium:host/media.open-video-bytes
    #[test]
    fn open_video_bytes_over_sixteen_mib_is_invalid_argument() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.open_video_bytes(vec![0u8; 16 * MIB + 1], "video/mp4".to_owned())),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.open-video-bytes
    #[test]
    fn open_video_bytes_unknown_mime_type_is_unsupported() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.open_video_bytes(vec![0u8; 8], "text/plain".to_owned())),
            ErrorCode::Unsupported
        );
    }

    /// REQ: WIT:ilium:host/media.open-video-bytes
    #[test]
    fn open_video_bytes_every_accepted_mime_type_is_not_refused_for_size() {
        let mut media = MediaImport::default();
        for mime_type in ACCEPTED_MIME_TYPES {
            let outcome = media.open_video_bytes(vec![0u8; 8], (*mime_type).to_owned());
            if let Err(error) = &outcome {
                assert_ne!(
                    error.code,
                    ErrorCode::InvalidArgument,
                    "mime type {mime_type} is documented as accepted"
                );
            }
        }
    }

    /// REQ: WIT:ilium:host/media.open-video-range
    #[test]
    fn open_video_range_missing_file_is_not_found() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.open_video_range(clip("nope.mp4"), 0, None)),
            ErrorCode::NotFound
        );
    }

    /// REQ: WIT:ilium:host/media.open-video-range
    #[test]
    fn open_video_range_extension_outside_the_accepted_list_is_unsupported() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.open_video_range(clip("notes.txt"), 1_000, Some(3_000))),
            ErrorCode::Unsupported
        );
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_zero_width_is_invalid_argument() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.next_frame(UNKNOWN_VIDEO_ID, 0, 10)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_zero_height_is_invalid_argument() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.next_frame(UNKNOWN_VIDEO_ID, 10, 0)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_at_the_4k_maximum_is_not_refused_for_size() {
        let mut media = MediaImport::default();
        let outcome = media.next_frame(UNKNOWN_VIDEO_ID, 3_840, 2_160);
        if let Err(error) = &outcome {
            assert_ne!(
                error.code,
                ErrorCode::InvalidArgument,
                "3,840 by 2,160 is the documented maximum and must be accepted"
            );
        }
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_width_over_the_4k_maximum_is_invalid_argument() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.next_frame(UNKNOWN_VIDEO_ID, 3_841, 2_160)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_height_over_the_4k_maximum_is_invalid_argument() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.next_frame(UNKNOWN_VIDEO_ID, 3_840, 2_161)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_unknown_or_closed_video_is_not_found() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.next_frame(UNKNOWN_VIDEO_ID, 8, 8)),
            ErrorCode::NotFound
        );
    }

    /// REQ: WIT:ilium:host/media.seek
    #[test]
    fn seek_unknown_video_is_not_found() {
        let mut media = MediaImport::default();
        assert_eq!(
            error_code_of(media.seek(UNKNOWN_VIDEO_ID, 0)),
            ErrorCode::NotFound
        );
    }

    /// REQ: WIT:ilium:host/media.close-video
    #[test]
    fn close_video_unknown_id_is_a_silent_no_op() {
        let mut media = MediaImport::default();
        media.close_video(UNKNOWN_VIDEO_ID);
        media.close_video(u64::MAX);
    }

    #[test]
    fn an_opened_project_video_plays_its_frames_then_ends() {
        let mut rig = rig(PLAYING, &[("a.mp4", b"video")], 2);
        let (video, info) = rig.media.open_video(clip("a.mp4")).expect("opens");
        assert_eq!((info.width, info.height), (640, 360));
        let frame = rig
            .media
            .next_frame(video, 4, 2)
            .expect("decodes")
            .expect("a frame");
        assert_eq!(frame.len(), 4 * 2 * 3);
        assert!(rig
            .media
            .next_frame(video, 4, 2)
            .expect("decodes")
            .is_some());
        assert_eq!(rig.media.next_frame(video, 4, 2).expect("end"), None);
        // The end stays the end until a seek.
        assert_eq!(rig.media.next_frame(video, 4, 2).expect("end"), None);
        let runs = rig.decoder.runs.lock().expect("runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0].file,
            rig.project.join("a.mp4").canonicalize().expect("real path")
        );
    }

    #[test]
    fn a_new_frame_size_restarts_the_decoder_at_the_current_position() {
        let mut rig = rig(PLAYING, &[("a.mp4", b"video")], 10);
        let (video, _) = rig.media.open_video(clip("a.mp4")).expect("opens");
        for _ in 0..5 {
            rig.media.next_frame(video, 4, 2).expect("decodes");
        }
        let frame = rig
            .media
            .next_frame(video, 8, 4)
            .expect("decodes")
            .expect("a frame");
        assert_eq!(frame.len(), 8 * 4 * 3);
        let runs = rig.decoder.runs.lock().expect("runs");
        assert_eq!(runs.len(), 2);
        // Five frames at 25 frames per second are 200 ms.
        assert_eq!(runs[1].start_millis, 200);
        assert_eq!((runs[1].width, runs[1].height), (8, 4));
    }

    #[test]
    fn a_range_is_a_clip_that_seeks_from_its_own_start_and_reopens_once_ended() {
        let mut rig = rig(PLAYING, &[("a.mp4", b"video")], 1);
        let (video, info) = rig
            .media
            .open_video_range(clip("a.mp4"), 1_000, Some(3_000))
            .expect("opens");
        assert_eq!(info.duration_millis, Some(2_000));
        assert!(rig
            .media
            .next_frame(video, 2, 2)
            .expect("decodes")
            .is_some());
        assert_eq!(rig.media.next_frame(video, 2, 2).expect("end"), None);
        rig.media.seek(video, 1_500).expect("seeks");
        assert_eq!(rig.media.position_millis(video), Some(1_500));
        assert!(rig
            .media
            .next_frame(video, 2, 2)
            .expect("decodes")
            .is_some());
        rig.media.seek(video, 0).expect("seeks");
        assert!(rig
            .media
            .next_frame(video, 2, 2)
            .expect("decodes")
            .is_some());
        assert_eq!(
            error_code_of(rig.media.seek(video, 2_001)),
            ErrorCode::InvalidArgument
        );
        let runs = rig.decoder.runs.lock().expect("runs");
        assert_eq!(runs[0].start_millis, 1_000);
        assert_eq!(runs[0].end_millis, Some(3_000));
        assert_eq!(runs[1].start_millis, 2_500);
        assert_eq!(runs[2].start_millis, 1_000);
    }

    #[test]
    fn an_open_ended_range_runs_to_the_end_and_cannot_start_past_it() {
        let mut rig = rig(PLAYING, &[("a.mp4", b"video")], 1);
        let (_, info) = rig
            .media
            .open_video_range(clip("a.mp4"), 1_000, None)
            .expect("opens");
        assert_eq!(info.duration_millis, Some(3_000));
        assert_eq!(
            error_code_of(rig.media.open_video_range(clip("a.mp4"), 4_000, None)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(rig.media.open_count(), 1);
    }

    #[test]
    fn an_empty_or_reversed_range_is_invalid() {
        let mut rig = rig(PLAYING, &[("a.mp4", b"video")], 1);
        assert_eq!(
            error_code_of(
                rig.media
                    .open_video_range(clip("a.mp4"), 2_000, Some(2_000))
            ),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_third_open_video_exceeds_the_quota_until_one_is_closed() {
        let mut rig = rig(PLAYING, &[("a.mp4", b"video")], 1);
        let (first, _) = rig.media.open_video(clip("a.mp4")).expect("first");
        rig.media.open_video(clip("a.mp4")).expect("second");
        assert_eq!(
            error_code_of(rig.media.open_video(clip("a.mp4"))),
            ErrorCode::QuotaExceeded
        );
        rig.media.close_video(first);
        assert_eq!(rig.media.open_count(), 1);
        rig.media
            .open_video(clip("a.mp4"))
            .expect("third after a close");
    }

    #[test]
    fn a_scope_the_plugin_cannot_read_is_refused() {
        let mut rig = rig(&["media.video"], &[("a.mp4", b"video")], 1);
        assert_eq!(
            error_code_of(rig.media.open_video(clip("a.mp4"))),
            ErrorCode::NotPermitted
        );
        let escape = FsPath {
            scope: "project".to_owned(),
            path: "../outside.mp4".to_owned(),
        };
        let mut granted = rig_with_read();
        assert_eq!(
            error_code_of(granted.media.open_video(escape)),
            ErrorCode::NotPermitted
        );
    }

    fn rig_with_read() -> Rig {
        rig(PLAYING, &[("a.mp4", b"video")], 1)
    }

    #[test]
    fn without_the_video_grant_nothing_opens() {
        let mut rig = rig(&["fs.read:project"], &[("a.mp4", b"video")], 1);
        assert_eq!(
            error_code_of(rig.media.open_video(clip("a.mp4"))),
            ErrorCode::NotPermitted
        );
    }

    #[test]
    fn a_file_without_a_video_stream_does_not_stay_open() {
        let mut rig = rig(PLAYING, &[("broken.mp4", b"not a video")], 1);
        assert_eq!(
            error_code_of(rig.media.open_video(clip("broken.mp4"))),
            ErrorCode::Unsupported
        );
        assert_eq!(rig.media.open_count(), 0);
    }

    #[test]
    fn an_in_memory_clip_is_spooled_and_removed_on_close() {
        let mut rig = rig(PLAYING, &[], 1);
        let (video, _) = rig
            .media
            .open_video_bytes(b"video".to_vec(), "video/webm".to_owned())
            .expect("opens");
        let spooled: Vec<PathBuf> = std::fs::read_dir(&rig.spool)
            .expect("spool folder")
            .map(|entry| entry.expect("entry").path())
            .collect();
        assert_eq!(spooled.len(), 1);
        assert_eq!(
            spooled[0]
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("webm")
        );
        rig.media.close_video(video);
        assert!(!spooled[0].exists());
    }
}
