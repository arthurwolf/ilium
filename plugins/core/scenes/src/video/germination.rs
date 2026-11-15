//! Downloading one Germination clip: pinned Wikimedia Commons bytes fetched in ranged chunks
//! through the host `http` import (each answer is capped by the host), checked against the
//! catalogue's SHA-256, then handed to the host decoder from memory (`open-video-bytes`).
//! Nothing is written to disk.

use super::source::{CatalogueClip, Fetched, VideoIo};

/// Bytes asked for by one range request: well under the host's 8 MiB answer cap.
pub const CHUNK_BYTES: u64 = 4 * 1024 * 1024;

/// How one [`Download::step`] ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Working,
    /// The verified bytes of the whole clip.
    Done(Vec<u8>),
    Failed(String),
}

/// One clip being downloaded.
pub struct Download {
    clip: CatalogueClip,
    buffer: Vec<u8>,
    request: Option<u64>,
}

impl Download {
    pub fn new(clip: CatalogueClip) -> Self {
        Self {
            buffer: Vec::with_capacity(clip.expected_download_bytes),
            clip,
            request: None,
        }
    }

    pub fn clip(&self) -> &CatalogueClip {
        &self.clip
    }

    /// Whole percent downloaded.
    pub fn percent(&self) -> u64 {
        (self.buffer.len() as u64 * 100) / (self.clip.expected_download_bytes.max(1) as u64)
    }

    /// Starts the next range request or collects the running one. Never waits.
    pub fn step(&mut self, io: &mut dyn VideoIo) -> Step {
        let total = self.clip.expected_download_bytes as u64;
        let Some(request) = self.request else {
            let start = self.buffer.len() as u64;
            let end = (start + CHUNK_BYTES).min(total).saturating_sub(1);
            return match io.fetch_range(&self.clip.url, start, end) {
                Ok(id) => {
                    self.request = Some(id);
                    Step::Working
                }
                Err(message) => Step::Failed(message),
            };
        };
        let Some(answer) = io.take_fetched(request) else {
            return Step::Working;
        };
        self.request = None;
        let (status, bytes) = match answer {
            Fetched::Status(status) => return Step::Failed(format!("HTTP status {status}")),
            Fetched::Body { status, bytes } => (status, bytes),
        };
        if bytes.is_empty() {
            return Step::Failed("the server sent an empty answer".to_owned());
        }
        if status == 200 {
            // A server that ignores the Range header sends the whole file at once.
            if !self.buffer.is_empty() || bytes.len() as u64 != total {
                return Step::Failed("the server ignored the range request".to_owned());
            }
            self.buffer = bytes;
        } else {
            self.buffer.extend_from_slice(&bytes);
        }
        let length = self.buffer.len() as u64;
        if length > total {
            return Step::Failed("the clip is longer than the catalogue says".to_owned());
        }
        if length < total {
            return Step::Working;
        }
        let digest = io.sha256_hex(&self.buffer);
        if !digest.eq_ignore_ascii_case(&self.clip.download_sha256) {
            return Step::Failed("the clip does not match its pinned digest".to_owned());
        }
        Step::Done(std::mem::take(&mut self.buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::super::source::fake::FakeIo;
    use super::*;

    fn clip(total: usize, digest: &str) -> CatalogueClip {
        CatalogueClip {
            id: "test".into(),
            title: "Test".into(),
            url: "https://upload.wikimedia.org/test.webm".into(),
            author: "Someone".into(),
            license: "CC0".into(),
            expected_download_bytes: total,
            download_sha256: digest.into(),
            mime: "video/webm".into(),
        }
    }

    fn run(download: &mut Download, io: &mut FakeIo) -> Step {
        for _ in 0..100 {
            match download.step(io) {
                Step::Working => {}
                done => return done,
            }
        }
        panic!("the download never finished");
    }

    #[test]
    fn a_clip_arrives_in_chunks_and_is_verified() {
        let total = CHUNK_BYTES as usize * 2 + 10;
        let mut io = FakeIo::serving((0..total).map(|index| (index % 251) as u8).collect(), 206);
        let mut download = Download::new(clip(total, &format!("fake-sha256-{total}")));
        let Step::Done(bytes) = run(&mut download, &mut io) else {
            panic!("expected the clip");
        };
        assert_eq!(bytes, io.body);
        assert_eq!(
            io.ranges,
            vec![
                (0, CHUNK_BYTES - 1),
                (CHUNK_BYTES, 2 * CHUNK_BYTES - 1),
                (2 * CHUNK_BYTES, total as u64 - 1)
            ]
        );
    }

    #[test]
    fn a_wrong_digest_fails() {
        let mut io = FakeIo::serving(vec![1; 100], 206);
        let mut download = Download::new(clip(100, "something-else"));
        assert!(
            matches!(run(&mut download, &mut io), Step::Failed(text) if text.contains("digest"))
        );
    }

    #[test]
    fn an_http_error_fails() {
        let mut io = FakeIo::serving(Vec::new(), 404);
        let mut download = Download::new(clip(100, "x"));
        assert!(matches!(run(&mut download, &mut io), Step::Failed(text) if text.contains("404")));
    }

    #[test]
    fn a_server_without_range_support_may_send_the_whole_clip_once() {
        let mut io = FakeIo::serving(vec![7; 64], 200);
        let mut download = Download::new(clip(64, "fake-sha256-64"));
        assert_eq!(download.percent(), 0);
        assert_eq!(download.step(&mut io), Step::Working);
        assert!(matches!(run(&mut download, &mut io), Step::Done(bytes) if bytes.len() == 64));
    }
}
