//! Shipped provenance and remote playback URLs; never consult local media.

use super::discover::MediaInput;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(super) struct Entry {
    id: String,
    title: String,
    url: String,
    source_page: String,
    author: String,
    license: String,
    license_url: String,
    source_sha1: String,
    pub expected_download_bytes: usize,
    pub download_sha256: String,
}

fn entries() -> Result<&'static [Entry], String> {
    static CATALOGUE: OnceLock<Result<Vec<Entry>, String>> = OnceLock::new();
    CATALOGUE
        .get_or_init(|| {
            let entries: Vec<Entry> =
                serde_json::from_str(include_str!("../../../assets/germination.json"))
                    .map_err(|error| format!("Invalid Germination catalogue: {error}"))?;
            if entries.is_empty()
                || entries.iter().any(|entry| {
                    entry.id.is_empty()
                        || entry.title.is_empty()
                        || !entry.url.starts_with("https://upload.wikimedia.org/")
                        || !entry
                            .source_page
                            .starts_with("https://commons.wikimedia.org/wiki/File:")
                        || entry.author.is_empty()
                        || entry.license.is_empty()
                        || (entry.license_url.is_empty() && entry.license != "Public domain")
                        || entry.source_sha1.len() != 40
                        || entry.download_sha256.len() != 64
                        || !entry
                            .download_sha256
                            .bytes()
                            .all(|value| value.is_ascii_hexdigit())
                        || entry.expected_download_bytes == 0
                        || entry.expected_download_bytes > 64 * 1024 * 1024
                })
            {
                return Err("Germination catalogue has invalid source or licence records".into());
            }
            Ok(entries)
        })
        .as_deref()
        .map_err(Clone::clone)
}

pub(super) fn inputs() -> Result<Vec<MediaInput>, String> {
    Ok(entries()?
        .iter()
        .map(|entry| MediaInput::Url(entry.url.clone()))
        .collect())
}

pub(super) fn display_name(input: &MediaInput) -> Option<String> {
    let MediaInput::Url(url) = input else {
        return None;
    };
    entries()
        .ok()?
        .iter()
        .find(|entry| &entry.url == url)
        .map(|entry| format!("{} — {} ({})", entry.title, entry.author, entry.license))
}

/// Identify the exact published representation before acquiring remote bytes.
pub(super) fn entry(input: &MediaInput) -> Result<&'static Entry, String> {
    let MediaInput::Url(url) = input else {
        return Err("Germination requires a catalogued remote source".into());
    };
    entries()?
        .iter()
        .find(|entry| &entry.url == url)
        .ok_or_else(|| "Source is not in the Germination catalogue".into())
}
