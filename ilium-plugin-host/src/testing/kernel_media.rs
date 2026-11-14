//! The fake kernel as the clipboard and the video decoder of the harness: `rig.kernel`'s
//! clipboard text and seeded videos are what the real `clipboard` and `media` imports reach, so
//! the harness runs the imports' own rules (grants, limits, quota, ranges) over scripted devices.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ilium_plugin_api::generated::host::media::{HostMedia, VideoInfo};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::paths::FsPath;

use super::fake_kernel::FakeKernel;
use crate::imports::clipboard::ClipboardBackend;
use crate::imports::env::ImportEnv;
use crate::imports::media::{DecodeRequest, FrameSource, MediaBackend, MediaImport, VideoFiles};

impl ClipboardBackend for FakeKernel {
    fn read_text(&self, _plugin: &str) -> Result<Option<String>, PluginError> {
        Ok(self.clipboard())
    }

    fn write_text(&self, _plugin: &str, text: &str) -> Result<(), PluginError> {
        self.set_clipboard(Some(text.to_owned()));
        Ok(())
    }
}

/// The videos seeded with `FakeKernel::set_video`, found as `<root>/<path>`, and the clip of
/// `set_bytes_video` for every spooled file.
#[derive(Debug, Clone)]
pub struct KernelVideos {
    kernel: Arc<FakeKernel>,
    spool: PathBuf,
}

impl KernelVideos {
    /// Videos of `kernel`; `spool` is where the media import writes in-memory clips.
    pub fn new(kernel: Arc<FakeKernel>, spool: PathBuf) -> Self {
        Self { kernel, spool }
    }

    fn seeded(&self, file: &Path) -> Option<VideoInfo> {
        let mut parts = file.iter();
        let root = parts.next()?.to_string_lossy().into_owned();
        let rest = parts.as_path().to_string_lossy().into_owned();
        self.kernel.state().videos.get(&(root, rest)).cloned()
    }
}

impl VideoFiles for KernelVideos {
    fn locate(&self, path: &FsPath) -> Result<PathBuf, PluginError> {
        let key = (path.scope.clone(), path.path.clone());
        if self.kernel.state().videos.contains_key(&key) {
            Ok(Path::new(&path.scope).join(&path.path))
        } else {
            Err(PluginError::new(
                ErrorCode::NotFound,
                format!("video `{}` was not found", path.path),
            ))
        }
    }
}

impl MediaBackend for KernelVideos {
    fn probe(&self, file: &Path) -> Result<VideoInfo, PluginError> {
        if self.kernel.state().ffmpeg_missing {
            return Err(PluginError::new(
                ErrorCode::Unsupported,
                "the program `ffmpeg` is not on PATH",
            ));
        }
        let info = if file.starts_with(&self.spool) {
            self.kernel.state().bytes_video.clone()
        } else {
            self.seeded(file)
        };
        info.ok_or_else(|| PluginError::new(ErrorCode::Unsupported, "no video stream"))
    }

    fn decode(&self, request: &DecodeRequest) -> Result<Box<dyn FrameSource>, PluginError> {
        let info = self.probe(&request.file)?;
        let rate = f64::from(info.frames_per_second).max(1.0);
        let end = request.end_millis.or(info.duration_millis).unwrap_or(0);
        let span = end.saturating_sub(request.start_millis) as f64;
        Ok(Box::new(GreyFrames {
            left: (span * rate / 1_000.0).floor() as u64,
            bytes: request.width as usize * request.height as usize * 3,
        }))
    }
}

/// Mid-grey frames until the clip ends.
struct GreyFrames {
    left: u64,
    bytes: usize,
}

impl FrameSource for GreyFrames {
    fn next_frame(&mut self) -> Result<Option<Vec<u8>>, PluginError> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        Ok(Some(vec![128; self.bytes]))
    }
}

/// The media import of one harness instance and the folder its in-memory clips go to.
struct MediaSlot {
    media: MediaImport,
    _spool: Option<tempfile::TempDir>,
}

/// The one media import of a harness instance, shared by every `media()` accessor call so open
/// videos and the quota carry over between calls.
#[derive(Clone, Default)]
pub struct SharedMedia {
    slot: Arc<Mutex<Option<Arc<Mutex<MediaSlot>>>>>,
}

impl SharedMedia {
    /// The media import of the instance `env` describes over the videos of `kernel`, made on
    /// first use.
    pub fn handle(
        &self,
        kernel: &Arc<FakeKernel>,
        env: impl FnOnce() -> ImportEnv,
    ) -> HarnessMedia {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let media = slot.get_or_insert_with(|| {
            let spool = tempfile::tempdir().ok();
            let spool_path = spool.as_ref().map(|folder| folder.path().to_path_buf());
            let videos = Arc::new(KernelVideos::new(
                Arc::clone(kernel),
                spool_path.clone().unwrap_or_default(),
            ));
            Arc::new(Mutex::new(MediaSlot {
                media: MediaImport::new(
                    env(),
                    Some(Arc::clone(&videos) as Arc<dyn VideoFiles>),
                    Some(videos as Arc<dyn MediaBackend>),
                    spool_path,
                ),
                _spool: spool,
            }))
        });
        HarnessMedia {
            media: Arc::clone(media),
            kernel: Arc::clone(kernel),
        }
    }
}

/// A view of the shared media import that reports each video's position to the fake kernel
/// (`FakeKernel::video_position`).
pub struct HarnessMedia {
    media: Arc<Mutex<MediaSlot>>,
    kernel: Arc<FakeKernel>,
}

impl HarnessMedia {
    fn with<T>(&mut self, video: Option<u64>, call: impl FnOnce(&mut MediaImport) -> T) -> T {
        let mut slot = self.media.lock().unwrap_or_else(PoisonError::into_inner);
        let answer = call(&mut slot.media);
        if let Some(video) = video {
            let mut state = self.kernel.state();
            match slot.media.position_millis(video) {
                Some(position) => state.video_positions.insert(video, position),
                None => state.video_positions.remove(&video),
            };
        }
        answer
    }
}

impl HostMedia for HarnessMedia {
    fn open_video(&mut self, path: FsPath) -> Result<(u64, VideoInfo), PluginError> {
        let opened = self.with(None, |media| media.open_video(path))?;
        self.with(Some(opened.0), |_| ());
        Ok(opened)
    }

    fn open_video_bytes(
        &mut self,
        bytes: Vec<u8>,
        mime: String,
    ) -> Result<(u64, VideoInfo), PluginError> {
        let opened = self.with(None, |media| media.open_video_bytes(bytes, mime))?;
        self.with(Some(opened.0), |_| ());
        Ok(opened)
    }

    fn open_video_range(
        &mut self,
        path: FsPath,
        start_millis: u64,
        end_millis: Option<u64>,
    ) -> Result<(u64, VideoInfo), PluginError> {
        let opened = self.with(None, |media| {
            media.open_video_range(path, start_millis, end_millis)
        })?;
        self.with(Some(opened.0), |_| ());
        Ok(opened)
    }

    fn next_frame(
        &mut self,
        video: u64,
        width: u32,
        height: u32,
    ) -> Result<Option<Vec<u8>>, PluginError> {
        self.with(Some(video), |media| media.next_frame(video, width, height))
    }

    fn seek(&mut self, video: u64, millis: u64) -> Result<(), PluginError> {
        self.with(Some(video), |media| media.seek(video, millis))
    }

    fn close_video(&mut self, video: u64) {
        self.with(Some(video), |media| media.close_video(video));
    }
}

#[cfg(test)]
mod tests {
    use super::{KernelVideos, SharedMedia};
    use crate::imports::env::ImportEnv;
    use crate::testing::fake_kernel::FakeKernel;
    use ilium_plugin_api::generated::host::media::{HostMedia, VideoInfo};
    use ilium_plugin_api::generated::types::errors::ErrorCode;
    use ilium_plugin_api::generated::types::paths::FsPath;
    use ilium_plugin_api::ProcessKind;
    use std::sync::Arc;

    fn seeded() -> Arc<FakeKernel> {
        let kernel = Arc::new(FakeKernel::new(ProcessKind::Client));
        kernel.set_video(
            "project",
            "a.mp4",
            VideoInfo {
                width: 64,
                height: 36,
                frames_per_second: 10.0,
                duration_millis: Some(300),
            },
        );
        kernel
    }

    fn clip() -> FsPath {
        FsPath {
            scope: "project".to_owned(),
            path: "a.mp4".to_owned(),
        }
    }

    #[test]
    fn a_seeded_video_plays_its_frames_and_reports_its_position() {
        let kernel = seeded();
        let shared = SharedMedia::default();
        let mut media = shared.handle(&kernel, || {
            ImportEnv::unrestricted().with_grants(vec!["media.video".to_owned()])
        });
        let (video, info) = media.open_video(clip()).expect("opens");
        assert_eq!(info.width, 64);
        for _ in 0..3 {
            assert!(media.next_frame(video, 4, 4).expect("frame").is_some());
        }
        assert_eq!(media.next_frame(video, 4, 4).expect("end"), None);
        assert_eq!(kernel.video_position(video), Some(300));
        // A second view shares the open video.
        let mut again = shared.handle(&kernel, ImportEnv::unrestricted);
        again.seek(video, 100).expect("seeks");
        assert_eq!(kernel.video_position(video), Some(100));
        again.close_video(video);
        assert_eq!(kernel.video_position(video), None);
    }

    #[test]
    fn without_ffmpeg_a_seeded_video_is_unsupported() {
        let kernel = seeded();
        kernel.set_ffmpeg_available(false);
        let mut media = SharedMedia::default().handle(&kernel, ImportEnv::unrestricted);
        let refused = media.open_video(clip()).expect_err("no ffmpeg");
        assert_eq!(refused.code, ErrorCode::Unsupported);
    }

    #[test]
    fn an_unseeded_path_is_not_found() {
        use crate::imports::media::VideoFiles;
        let videos = KernelVideos::new(seeded(), std::path::PathBuf::from("/spool"));
        let missing = FsPath {
            scope: "project".to_owned(),
            path: "b.mp4".to_owned(),
        };
        assert_eq!(
            videos.locate(&missing).expect_err("missing").code,
            ErrorCode::NotFound
        );
    }
}
