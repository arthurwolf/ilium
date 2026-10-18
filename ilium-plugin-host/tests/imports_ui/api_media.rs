//! `ilium:host/media` (api-media.md): video decoding.

use ilium_plugin_api::generated::host::media::{HostMedia, VideoInfo};
use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &["media.video", "fs.read"];

fn clip(path: &str) -> FsPath {
    FsPath {
        scope: "project".to_owned(),
        path: path.to_owned(),
    }
}

fn info() -> VideoInfo {
    VideoInfo {
        width: 640,
        height: 360,
        frames_per_second: 25.0,
        duration_millis: Some(4_000),
    }
}

fn seeded() -> Rig {
    let rig = rig(GRANTS);
    rig.kernel.set_video("project", "a.mp4", info());
    rig.kernel.set_video("project", "b.mp4", info());
    rig.kernel.set_video("project", "c.mp4", info());
    rig
}

/// REQ: WIT:ilium:host/media.open-video
#[test]
fn wit_media_open_video_returns_an_id_and_the_description() {
    let rig = seeded();
    let (id, described) = rig.host.media().open_video(clip("a.mp4")).expect("opens");
    assert_eq!((described.width, described.height), (640, 360));
    assert_eq!(described.duration_millis, Some(4_000));
    let _ = id;
}

/// REQ: WIT:ilium:host/media.open-video
#[test]
fn wit_media_open_video_missing_file_not_found() {
    let rig = seeded();
    assert_eq!(
        code_of(rig.host.media().open_video(clip("nope.mp4"))),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/media.open-video
#[test]
fn wit_media_open_video_third_open_video_quota_exceeded() {
    let rig = seeded();
    rig.host.media().open_video(clip("a.mp4")).expect("first");
    rig.host.media().open_video(clip("b.mp4")).expect("second");
    assert_eq!(
        code_of(rig.host.media().open_video(clip("c.mp4"))),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/media.open-video
#[test]
fn wit_media_open_video_unsupported_extension_unsupported() {
    let rig = seeded();
    rig.kernel.set_video("project", "notes.txt", info());
    assert_eq!(
        code_of(rig.host.media().open_video(clip("notes.txt"))),
        ErrorCode::Unsupported
    );
}

/// REQ: WIT:ilium:host/media.open-video
#[test]
fn wit_media_open_video_without_ffmpeg_unsupported() {
    let rig = seeded();
    rig.kernel.set_ffmpeg_available(false);
    assert_eq!(
        code_of(rig.host.media().open_video(clip("a.mp4"))),
        ErrorCode::Unsupported
    );
}

/// REQ: WIT:ilium:host/media.open-video
#[test]
fn wit_media_open_video_declined_grant_not_permitted() {
    let rig = declined_rig(&["media.video"]);
    assert_eq!(
        code_of(rig.host.media().open_video(clip("a.mp4"))),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/media.open-video-bytes
#[test]
fn wit_media_open_video_bytes_opens_from_memory() {
    let rig = seeded();
    rig.kernel.set_bytes_video(info());
    let (_, described) = rig
        .host
        .media()
        .open_video_bytes(vec![0; 64], "video/mp4".to_owned())
        .expect("opens");
    assert_eq!(described.width, 640);
}

/// REQ: WIT:ilium:host/media.open-video-bytes
#[test]
fn wit_media_open_video_bytes_sixteen_mib_boundary_quota_exceeded() {
    let rig = seeded();
    rig.kernel.set_bytes_video(info());
    let at_limit = rig
        .host
        .media()
        .open_video_bytes(vec![0; 16 * MIB], "video/mp4".to_owned());
    assert!(at_limit.is_ok());
    let (id, _) = at_limit.expect("opened");
    rig.host.media().close_video(id);
    assert_eq!(
        code_of(
            rig.host
                .media()
                .open_video_bytes(vec![0; 16 * MIB + 1], "video/mp4".to_owned())
        ),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/media.open-video-bytes
#[test]
fn wit_media_open_video_bytes_unknown_mime_invalid_argument() {
    let rig = seeded();
    assert_eq!(
        code_of(
            rig.host
                .media()
                .open_video_bytes(vec![0; 8], "text/plain".to_owned())
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/media.open-video-range
#[test]
fn wit_media_open_video_range_opens_a_clipped_range() {
    let rig = seeded();
    let (_, described) = rig
        .host
        .media()
        .open_video_range(clip("a.mp4"), 1_000, Some(3_000))
        .expect("opens");
    assert_eq!(described.duration_millis, Some(2_000));
}

/// REQ: WIT:ilium:host/media.open-video-range
#[test]
fn wit_media_open_video_range_open_ended_range_runs_to_the_end() {
    let rig = seeded();
    let (_, described) = rig
        .host
        .media()
        .open_video_range(clip("a.mp4"), 1_000, None)
        .expect("opens");
    assert_eq!(described.duration_millis, Some(3_000));
}

/// REQ: WIT:ilium:host/media.open-video-range
#[test]
fn wit_media_open_video_range_declined_grant_not_permitted() {
    let rig = declined_rig(&["media.video"]);
    assert_eq!(
        code_of(rig.host.media().open_video_range(clip("a.mp4"), 0, None)),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/media.next-frame
#[test]
fn wit_media_next_frame_returns_rgb8_of_the_requested_size() {
    let rig = seeded();
    let (id, _) = rig.host.media().open_video(clip("a.mp4")).expect("opens");
    let frame = rig
        .host
        .media()
        .next_frame(id, 32, 18)
        .expect("frame")
        .expect("some frame");
    assert_eq!(frame.len(), 32 * 18 * 3);
}

/// REQ: WIT:ilium:host/media.next-frame
#[test]
fn wit_media_next_frame_end_of_video_is_none() {
    let rig = seeded();
    let (id, _) = rig.host.media().open_video(clip("a.mp4")).expect("opens");
    rig.host.media().seek(id, 4_000).expect("seeks to the end");
    assert_eq!(rig.host.media().next_frame(id, 8, 8), Ok(None));
}

/// REQ: WIT:ilium:host/media.next-frame
#[test]
fn wit_media_next_frame_size_boundaries_invalid_argument() {
    let rig = seeded();
    let (id, _) = rig.host.media().open_video(clip("a.mp4")).expect("opens");
    assert_eq!(
        code_of(rig.host.media().next_frame(id, 0, 10)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        code_of(rig.host.media().next_frame(id, 10, 0)),
        ErrorCode::InvalidArgument
    );
    assert!(rig.host.media().next_frame(id, 3_840, 2_160).is_ok());
    assert_eq!(
        code_of(rig.host.media().next_frame(id, 3_841, 2_160)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        code_of(rig.host.media().next_frame(id, 3_840, 2_161)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/media.next-frame
#[test]
fn wit_media_next_frame_closed_video_not_found() {
    let rig = seeded();
    let (id, _) = rig.host.media().open_video(clip("a.mp4")).expect("opens");
    rig.host.media().close_video(id);
    assert_eq!(
        code_of(rig.host.media().next_frame(id, 8, 8)),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/media.seek
#[test]
fn wit_media_seek_moves_the_playhead() {
    let rig = seeded();
    let (id, _) = rig.host.media().open_video(clip("a.mp4")).expect("opens");
    rig.host.media().seek(id, 2_000).expect("seeks");
    assert_eq!(rig.kernel.video_position(id), Some(2_000));
}

/// REQ: WIT:ilium:host/media.seek
#[test]
fn wit_media_seek_past_the_end_invalid_argument() {
    let rig = seeded();
    let (id, _) = rig.host.media().open_video(clip("a.mp4")).expect("opens");
    assert!(rig.host.media().seek(id, 4_000).is_ok());
    assert_eq!(
        code_of(rig.host.media().seek(id, 4_001)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/media.seek
#[test]
fn wit_media_seek_unknown_video_not_found() {
    let rig = seeded();
    assert_eq!(
        code_of(rig.host.media().seek(12_345, 0)),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/media.close-video
#[test]
fn wit_media_close_video_frees_the_slot() {
    let rig = seeded();
    let (first, _) = rig.host.media().open_video(clip("a.mp4")).expect("first");
    rig.host.media().open_video(clip("b.mp4")).expect("second");
    rig.host.media().close_video(first);
    assert!(rig.host.media().open_video(clip("c.mp4")).is_ok());
}

/// REQ: WIT:ilium:host/media.close-video
#[test]
fn wit_media_close_video_unknown_id_is_a_no_op() {
    let rig = seeded();
    rig.host.media().close_video(12_345);
    assert!(rig.host.failures().is_empty());
}

/// REQ: WIT:ilium:host/media.close-video
#[test]
fn wit_media_close_video_twice_is_a_no_op() {
    let rig = seeded();
    let (id, _) = rig.host.media().open_video(clip("a.mp4")).expect("opens");
    rig.host.media().close_video(id);
    rig.host.media().close_video(id);
    assert!(rig.host.failures().is_empty());
}
