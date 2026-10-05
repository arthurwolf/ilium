use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use ilium_ambient::source::{cache_file_name, USER_AGENT};

pub(crate) const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_SOURCE_PIXELS: u64 = 40_000_000;
pub(crate) const MAX_TOTAL_PIXELS: u64 = 32_000_000;

pub(crate) fn valid_wikimedia_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some("en.wikipedia.org" | "upload.wikimedia.org" | "thumb.wikimedia.org")
        )
}

/// Redirects are inspected before another request, including downgrade,
/// credentials, ports and host changes. A relative Wikimedia redirect is valid.
fn redirect_url(current: &str, location: &str) -> Result<String, String> {
    let url = url::Url::parse(current)
        .map_err(|error| error.to_string())?
        .join(location)
        .map_err(|error| error.to_string())?;
    if !valid_wikimedia_url(url.as_str()) {
        return Err("Wikipedia redirect left approved HTTPS Wikimedia hosts".into());
    }
    Ok(url.into())
}

fn wait_for_slot(host: &str) {
    static HOST_SLOTS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    let slots = HOST_SLOTS.get_or_init(Default::default);
    let wait = {
        let mut slots = slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        let ready = slots
            .get(host)
            .map_or(now, |last| (*last + Duration::from_millis(500)).max(now));
        slots.insert(host.to_owned(), ready);
        ready.saturating_duration_since(now)
    };
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
}

static COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

fn retry_after_seconds(value: Option<&str>) -> u64 {
    value
        .and_then(|value| {
            value.parse::<u64>().ok().or_else(|| {
                chrono::DateTime::parse_from_rfc2822(value)
                    .ok()
                    .map(|date| {
                        (date.with_timezone(&chrono::Utc) - chrono::Utc::now())
                            .num_seconds()
                            .max(1) as u64
                    })
            })
        })
        .unwrap_or(60)
        .clamp(1, 86_400)
}

fn guarded_http_get(initial: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut url = initial.to_owned();
    let started = Instant::now();
    for hop in 0..=5 {
        if !valid_wikimedia_url(&url) {
            return Err("Wikipedia request left approved HTTPS Wikimedia hosts".into());
        }
        let parsed = url::Url::parse(&url).map_err(|error| error.to_string())?;
        let host = parsed.host_str().ok_or("Wikipedia URL has no host")?;
        let cooldowns = COOLDOWNS.get_or_init(Default::default);
        if let Some(remaining) = cooldowns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(host)
            .and_then(|deadline| deadline.checked_duration_since(Instant::now()))
        {
            return Err(format!(
                "Wikipedia requests held for {} seconds after rate/service limit",
                remaining.as_secs() + 1
            ));
        }
        wait_for_slot(host);
        let remaining = TIMEOUT
            .checked_sub(started.elapsed())
            .filter(|duration| !duration.is_zero())
            .ok_or("Wikipedia request deadline exceeded")?;
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(remaining))
            .user_agent(USER_AGENT)
            .https_only(true)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .build();
        let agent = ilium_http::agent(config);
        let mut response = agent.get(&url).call().map_err(|error| error.to_string())?;
        if response.status().is_redirection() {
            if hop == 5 {
                return Err("Wikipedia redirect limit exceeded".into());
            }
            let location = response
                .headers()
                .get("location")
                .ok_or("Wikipedia redirect omitted Location")?
                .to_str()
                .map_err(|error| error.to_string())?;
            url = redirect_url(&url, location)?;
            continue;
        }
        if matches!(response.status().as_u16(), 429 | 503) {
            let seconds = retry_after_seconds(
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok()),
            );
            cooldowns
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    host.to_owned(),
                    Instant::now() + Duration::from_secs(seconds),
                );
        }
        if !response.status().is_success() {
            return Err(format!("Wikipedia HTTP {}", response.status()));
        }
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > max_bytes {
            return Err(format!("Wikipedia response exceeds {max_bytes} bytes"));
        }
        return Ok(bytes);
    }
    Err("Wikipedia redirect limit exceeded".into())
}

pub(crate) fn html_url(title: &str) -> Result<String, String> {
    let mut url = url::Url::parse("https://en.wikipedia.org/w/rest.php/v1/page/")
        .map_err(|error| error.to_string())?;
    url.path_segments_mut()
        .map_err(|()| "invalid Wikipedia endpoint")?
        .pop_if_empty()
        .push(&title.replace(' ', "_"))
        .push("html");
    Ok(url.into())
}

pub(crate) fn public_url(title: &str) -> Result<String, String> {
    let mut url =
        url::Url::parse("https://en.wikipedia.org/wiki/").map_err(|error| error.to_string())?;
    url.path_segments_mut()
        .map_err(|()| "invalid Wikipedia endpoint")?
        .pop_if_empty()
        .push(&title.replace(' ', "_"));
    Ok(url.into())
}

pub(crate) struct CachedBytes {
    pub bytes: Vec<u8>,
    pub stale: bool,
    pub cached_at: Option<SystemTime>,
}

fn read_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > max_bytes {
        return Err(format!(
            "cached Wikipedia content exceeds {max_bytes} bytes"
        ));
    }
    Ok(bytes)
}

/// Shares ambient's User-Agent; adds validated redirects and a process-wide
/// Wikimedia request limiter (500 ms per host).
/// Stale fallback is explicit, and cached bytes receive the same size checks.
pub(crate) fn cached_get(
    directory: &Path,
    url: &str,
    extension: &str,
    max_age: Duration,
    max_bytes: usize,
) -> Result<CachedBytes, String> {
    cached_get_using(
        directory,
        url,
        extension,
        max_age,
        max_bytes,
        guarded_http_get,
    )
}

fn cached_get_using(
    directory: &Path,
    url: &str,
    extension: &str,
    max_age: Duration,
    max_bytes: usize,
    fetch: impl FnOnce(&str, usize) -> Result<Vec<u8>, String>,
) -> Result<CachedBytes, String> {
    if !valid_wikimedia_url(url) {
        return Err("only HTTPS English Wikipedia/Wikimedia upload URLs are allowed".into());
    }
    let path = directory.join(cache_file_name(url, extension));
    let cached_at = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let age = cached_at.and_then(|time| time.elapsed().ok());
    if age.is_some_and(|age| age < max_age) {
        if let Ok(bytes) = read_bounded(&path, max_bytes) {
            return Ok(CachedBytes {
                bytes,
                stale: false,
                cached_at,
            });
        }
    }
    match fetch(url, max_bytes) {
        Ok(bytes) => {
            std::fs::create_dir_all(directory)
                .map_err(|error| format!("Wikipedia cache: {error}"))?;
            // Unique temporary names avoid competing clients overwriting an
            // in-progress cache write. The completed immutable bytes win atomically.
            let temporary =
                path.with_extension(format!("{extension}.{:016x}.tmp", rand::random::<u64>()));
            let result = (|| {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)?;
                std::io::Write::write_all(&mut file, &bytes)?;
                std::fs::rename(&temporary, &path)
            })();
            if let Err(error) = result {
                let _ = std::fs::remove_file(&temporary);
                return Err(format!("Wikipedia cache write: {error}"));
            }
            Ok(CachedBytes {
                bytes,
                stale: false,
                cached_at: Some(SystemTime::now()),
            })
        }
        Err(error) => read_bounded(&path, max_bytes)
            .map(|bytes| CachedBytes {
                bytes,
                stale: true,
                cached_at,
            })
            .map_err(|_| error.to_string()),
    }
}

pub(crate) fn fetch_image(directory: &Path, url: &str) -> Result<(image::RgbaImage, bool), String> {
    let parsed = url::Url::parse(url).map_err(|error| error.to_string())?;
    if !matches!(
        parsed.host_str(),
        Some("upload.wikimedia.org" | "thumb.wikimedia.org")
    ) || !parsed.path().starts_with("/wikipedia/")
    {
        return Err("article image is not a Wikimedia upload".into());
    }
    let cached = cached_get(
        directory,
        url,
        "image",
        Duration::from_secs(30 * 86400),
        MAX_IMAGE_BYTES,
    )?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(&cached.bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(MAX_SOURCE_PIXELS * 4);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| format!("Wikimedia image decode: {error}"))?;
    if u64::from(image.width()) * u64::from(image.height()) > MAX_SOURCE_PIXELS {
        return Err("Wikimedia image exceeds pixel safety limit".into());
    }
    // Source thumbnails remain intact when already smaller. Larger photographs
    // are sampled for terminal display; encoded source stays in the cache.
    let image = if image.width() > 640 || image.height() > 640 {
        image.thumbnail(640, 640)
    } else {
        image
    };
    Ok((image.to_rgba8(), cached.stale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_scheme_host_credentials_and_port_without_network() {
        for url in [
            "http://upload.wikimedia.org/x",
            "https://en.wikipedia.org.evil.test/x",
            "https://evil.test/@en.wikipedia.org",
            "https://user@en.wikipedia.org/x",
            "https://en.wikipedia.org:80/x",
            "file:///tmp/a",
        ] {
            assert!(!valid_wikimedia_url(url), "{url}");
        }
        assert!(valid_wikimedia_url(
            "https://upload.wikimedia.org/wikipedia/commons/a/a0/a.png"
        ));
        assert!(valid_wikimedia_url(
            "https://thumb.wikimedia.org/wikipedia/commons/thumb/a/a0/a.png/320px-a.png"
        ));
        assert!(!valid_wikimedia_url(
            "https://thumb.wikimedia.org.evil.test/a"
        ));
        assert!(valid_wikimedia_url(
            "https://en.wikipedia.org/w/rest.php/v1/page/Main_Page/html"
        ));
    }

    #[test]
    fn server_rate_limit_has_bounded_nonzero_cooldown() {
        assert_eq!(retry_after_seconds(Some("120")), 120);
        assert_eq!(retry_after_seconds(None), 60);
        assert_eq!(retry_after_seconds(Some("0")), 1);
        assert_eq!(retry_after_seconds(Some("999999999999")), 86_400);
    }

    #[test]
    fn every_redirect_is_checked_before_network() {
        let origin = "https://en.wikipedia.org/w/rest.php/v1/page/A/html";
        assert!(redirect_url(origin, "http://en.wikipedia.org/wiki/A").is_err());
        assert!(redirect_url(origin, "https://evil.test/a").is_err());
        assert!(redirect_url(origin, "https://user@en.wikipedia.org/a").is_err());
        assert!(redirect_url(origin, "https://upload.wikimedia.org:80/a").is_err());
        assert_eq!(
            redirect_url(origin, "/wiki/A").unwrap(),
            "https://en.wikipedia.org/wiki/A"
        );
    }

    #[test]
    fn network_failure_reports_stale_cache_without_network() {
        let directory = tempfile::tempdir().unwrap();
        let url = "https://en.wikipedia.org/w/rest.php/v1/page/Example/html";
        let path = directory.path().join(cache_file_name(url, "html"));
        std::fs::write(&path, b"authored cached source").unwrap();
        let cached = cached_get_using(
            directory.path(),
            url,
            "html",
            Duration::ZERO,
            100,
            |_, _| Err("offline".into()),
        )
        .unwrap();
        assert!(cached.stale);
        assert!(cached.cached_at.is_some());
        assert_eq!(cached.bytes, b"authored cached source");
        assert!(cached_get_using(
            directory.path(),
            url,
            "html",
            Duration::ZERO,
            2,
            |_, _| Err("offline".into())
        )
        .is_err());
    }

    #[test]
    fn title_is_one_encoded_path_segment() {
        let url = html_url("A/B ?#é").unwrap();
        let url = url::Url::parse(&url).unwrap();
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
        assert!(url.path().contains("A%2FB_"));
    }

    #[test]
    fn bounded_cache_refuses_oversize_and_serves_fresh_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let url = "https://en.wikipedia.org/w/rest.php/v1/page/Example/html";
        let path = directory.path().join(cache_file_name(url, "html"));
        std::fs::write(&path, b"cached").unwrap();
        assert_eq!(
            cached_get(directory.path(), url, "html", Duration::from_secs(60), 10)
                .unwrap()
                .bytes,
            b"cached"
        );
        assert!(read_bounded(&path, 2).is_err());
    }

    #[test]
    fn cached_small_thumbnail_preserves_original_dimensions_and_pixels() {
        let directory = tempfile::tempdir().unwrap();
        let url = "https://upload.wikimedia.org/wikipedia/commons/a/a0/small.png";
        let mut original = image::RgbaImage::from_pixel(2, 3, image::Rgba([17, 34, 51, 255]));
        original.put_pixel(1, 2, image::Rgba([99, 88, 77, 128]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(original.clone())
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        std::fs::write(
            directory.path().join(cache_file_name(url, "image")),
            bytes.into_inner(),
        )
        .unwrap();
        let (decoded, stale) = fetch_image(directory.path(), url).unwrap();
        assert!(!stale);
        assert_eq!(decoded.dimensions(), original.dimensions());
        assert_eq!(decoded.as_raw(), original.as_raw());
    }

    #[test]
    fn cached_large_image_is_downsampled_within_display_limit() {
        let directory = tempfile::tempdir().unwrap();
        let url = "https://upload.wikimedia.org/wikipedia/commons/a/a0/large.png";
        let original = image::RgbaImage::from_pixel(1280, 320, image::Rgba([17, 34, 51, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(original)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        std::fs::write(
            directory.path().join(cache_file_name(url, "image")),
            bytes.into_inner(),
        )
        .unwrap();
        let (decoded, stale) = fetch_image(directory.path(), url).unwrap();
        assert!(!stale);
        assert_eq!(decoded.dimensions(), (640, 160));
        assert_eq!(*decoded.get_pixel(320, 80), image::Rgba([17, 34, 51, 255]));
    }
}
