//! Host implementation of `ilium:host/media` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostMedia` trait; hand-owned afterwards.

use std::convert::Infallible;

use ilium_plugin_api::generated::host::media::{HostMedia, VideoInfo};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::paths::FsPath;

/// Host side of the `media` interface.
#[derive(Debug)]
pub struct MediaImport;

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
/// Largest in-memory clip, in bytes.
const MAX_CLIP_BYTES: usize = 16 * 1024 * 1024;
const MAX_FRAME_WIDTH: u32 = 3_840;
const MAX_FRAME_HEIGHT: u32 = 2_160;

fn check_path(path: &FsPath) -> Result<Infallible, PluginError> {
    if path.path.is_empty() {
        return Err(PluginError::new(ErrorCode::InvalidArgument, "empty video path"));
    }
    let extension = path
        .path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    if !ACCEPTED_EXTENSIONS.contains(&extension.as_str()) {
        return Err(PluginError::new(
            ErrorCode::Unsupported,
            format!("video extension `{extension}` is not accepted"),
        ));
    }
    // No file scope is attached to this import, so no file can be found.
    Err(PluginError::new(
        ErrorCode::NotFound,
        format!("video `{}` was not found", path.path),
    ))
}

fn unknown_video(video: u64) -> PluginError {
    PluginError::new(ErrorCode::NotFound, format!("video {video} is unknown or closed"))
}

impl HostMedia for MediaImport {
    fn open_video(&mut self, path: FsPath) -> Result<(u64, VideoInfo), PluginError> {
        check_path(&path).map(|never| match never {})
    }

    fn open_video_bytes(
        &mut self,
        bytes: Vec<u8>,
        mime: String,
    ) -> Result<(u64, VideoInfo), PluginError> {
        if bytes.is_empty() || bytes.len() > MAX_CLIP_BYTES {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "video bytes must be between 1 byte and 16 MiB",
            ));
        }
        if !ACCEPTED_MIME_TYPES.contains(&mime.as_str()) {
            return Err(PluginError::new(
                ErrorCode::Unsupported,
                format!("mime type `{mime}` is not accepted"),
            ));
        }
        Err(PluginError::new(
            ErrorCode::Unsupported,
            "no video decoder is attached to this host",
        ))
    }

    fn open_video_range(
        &mut self,
        path: FsPath,
        _start_millis: u64,
        _end_millis: Option<u64>,
    ) -> Result<(u64, VideoInfo), PluginError> {
        check_path(&path).map(|never| match never {})
    }

    fn next_frame(
        &mut self,
        video: u64,
        width: u32,
        height: u32,
    ) -> Result<Option<Vec<u8>>, PluginError> {
        if width == 0 || height == 0 || width > MAX_FRAME_WIDTH || height > MAX_FRAME_HEIGHT {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "frame size must be 1..=3840 by 1..=2160",
            ));
        }
        Err(unknown_video(video))
    }

    fn seek(&mut self, video: u64, _millis: u64) -> Result<(), PluginError> {
        Err(unknown_video(video))
    }

    fn close_video(&mut self, _video: u64) {
        // No video is ever open on this import, so closing any id is a silent no-op.
    }
}

#[cfg(test)]
mod tests {
    use super::MediaImport;
    use ilium_plugin_api::generated::host::media::HostMedia;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::paths::FsPath;

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

    /// REQ: WIT:ilium:host/media.open-video
    #[test]
    fn open_video_missing_file_is_not_found() {
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.open_video(clip("nope.mp4"))),
            ErrorCode::NotFound
        );
    }

    /// REQ: WIT:ilium:host/media.open-video
    #[test]
    fn open_video_extension_outside_the_accepted_list_is_unsupported() {
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.open_video(clip("notes.txt"))),
            ErrorCode::Unsupported
        );
    }

    /// REQ: WIT:ilium:host/media.open-video
    #[test]
    fn open_video_every_accepted_extension_is_not_refused_as_unsupported() {
        let mut media = MediaImport;
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
        let mut media = MediaImport;
        assert!(media.open_video(clip("")).is_err());
    }

    /// REQ: WIT:ilium:host/media.open-video-bytes
    #[test]
    fn open_video_bytes_empty_input_is_invalid_argument() {
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.open_video_bytes(Vec::new(), "video/mp4".to_owned())),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.open-video-bytes
    #[test]
    fn open_video_bytes_at_sixteen_mib_is_not_refused_for_size() {
        let mut media = MediaImport;
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
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.open_video_bytes(vec![0u8; 16 * MIB + 1], "video/mp4".to_owned())),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.open-video-bytes
    #[test]
    fn open_video_bytes_unknown_mime_type_is_unsupported() {
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.open_video_bytes(vec![0u8; 8], "text/plain".to_owned())),
            ErrorCode::Unsupported
        );
    }

    /// REQ: WIT:ilium:host/media.open-video-bytes
    #[test]
    fn open_video_bytes_every_accepted_mime_type_is_not_refused_for_size() {
        let mut media = MediaImport;
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
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.open_video_range(clip("nope.mp4"), 0, None)),
            ErrorCode::NotFound
        );
    }

    /// REQ: WIT:ilium:host/media.open-video-range
    #[test]
    fn open_video_range_extension_outside_the_accepted_list_is_unsupported() {
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.open_video_range(clip("notes.txt"), 1_000, Some(3_000))),
            ErrorCode::Unsupported
        );
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_zero_width_is_invalid_argument() {
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.next_frame(UNKNOWN_VIDEO_ID, 0, 10)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_zero_height_is_invalid_argument() {
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.next_frame(UNKNOWN_VIDEO_ID, 10, 0)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_at_the_4k_maximum_is_not_refused_for_size() {
        let mut media = MediaImport;
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
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.next_frame(UNKNOWN_VIDEO_ID, 3_841, 2_160)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_height_over_the_4k_maximum_is_invalid_argument() {
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.next_frame(UNKNOWN_VIDEO_ID, 3_840, 2_161)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: WIT:ilium:host/media.next-frame
    #[test]
    fn next_frame_unknown_or_closed_video_is_not_found() {
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.next_frame(UNKNOWN_VIDEO_ID, 8, 8)),
            ErrorCode::NotFound
        );
    }

    /// REQ: WIT:ilium:host/media.seek
    #[test]
    fn seek_unknown_video_is_not_found() {
        let mut media = MediaImport;
        assert_eq!(
            error_code_of(media.seek(UNKNOWN_VIDEO_ID, 0)),
            ErrorCode::NotFound
        );
    }

    /// REQ: WIT:ilium:host/media.close-video
    #[test]
    fn close_video_unknown_id_is_a_silent_no_op() {
        let mut media = MediaImport;
        media.close_video(UNKNOWN_VIDEO_ID);
        media.close_video(u64::MAX);
    }
}
