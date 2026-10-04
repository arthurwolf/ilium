//! Explicit Overpass transport on an already-admitted OSM worker.
//! Resolver/rate ownership adapted from the recorded bounded advisor proposal.
use super::bundle::MAX_JSON_BYTES;
use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use ureq::unversioned::{
    resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
    transport::{time::Duration as ResolverDuration, DefaultConnector, NextTimeout},
};
const SPACING: Duration = Duration::from_secs(60);
#[derive(Default)]
struct GateState {
    busy: bool,
    next: Option<Instant>,
    disabled: bool,
}
#[derive(Default)]
struct Gate(Mutex<GateState>);
struct Permit<'a>(&'a Gate);
impl Gate {
    fn reserve(&self, now: Instant) -> Result<Permit<'_>, String> {
        let mut state = self.0.lock().map_err(|_| "OSM network gate unavailable")?;
        if state.disabled {
            return Err(
                "OSM provider cooldown cannot be determined; no further requests in this process"
                    .into(),
            );
        }
        if state.busy {
            return Err("An OSM request is already running".into());
        }
        if let Some(next) = state.next.filter(|next| *next > now) {
            return Err(format!(
                "OSM service cooldown: {} seconds remaining",
                next.duration_since(now).as_secs() + 1
            ));
        }
        state.busy = true;
        state.next = now.checked_add(SPACING);
        Ok(Permit(self))
    }
    fn defer(&self, now: Instant, seconds: u64) {
        if let Ok(mut state) = self.0.lock() {
            match now.checked_add(Duration::from_secs(seconds.max(SPACING.as_secs()))) {
                Some(until) => {
                    state.next = Some(state.next.map_or(until, |previous| previous.max(until)))
                }
                None => state.disabled = true,
            }
        }
    }
}
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0 .0.lock() {
            state.busy = false;
        }
    }
}
#[derive(Debug)]
pub(super) struct OwnedResolver;
fn resolve_owned(
    resolver: &impl Resolver,
    uri: &ureq::http::Uri,
    config: &ureq::config::Config,
    timeout: NextTimeout,
) -> Result<ResolvedSocketAddrs, ureq::Error> {
    let began = Instant::now();
    if matches!(timeout.after,ResolverDuration::Exact(duration) if duration.is_zero()) {
        return Err(ureq::Error::Timeout(timeout.reason));
    }
    // The OS lookup remains on this admitted worker. It cannot be forcibly
    // cancelled; a stuck lookup keeps its slot rather than leaking helpers.
    let result = resolver.resolve(
        uri,
        config,
        NextTimeout {
            after: ResolverDuration::NotHappening,
            reason: timeout.reason,
        },
    );
    if matches!(timeout.after,ResolverDuration::Exact(duration) if began.elapsed()>=duration) {
        return Err(ureq::Error::Timeout(timeout.reason));
    }
    result
}
impl Resolver for OwnedResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        resolve_owned(&DefaultResolver::default(), uri, config, timeout)
    }
}
pub fn fetch(url: &str, stop: &AtomicBool) -> Result<Vec<u8>, String> {
    if stop.load(Ordering::Relaxed) {
        return Err("OSM load cancelled".into());
    }
    let uri: ureq::http::Uri = url
        .parse()
        .map_err(|error| format!("Invalid Overpass URI: {error}"))?;
    if uri.scheme_str() != Some("https")
        || uri.host().is_none_or(str::is_empty)
        || uri
            .authority()
            .is_some_and(|authority| authority.as_str().contains('@'))
        || url.len() > 4096
    {
        return Err("Use a valid bounded HTTPS Overpass URI without credentials".into());
    }
    static GATE: OnceLock<Gate> = OnceLock::new();
    let gate = GATE.get_or_init(Gate::default);
    let _permit = gate.reserve(Instant::now())?;
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(20)))
        .timeout_connect(Some(Duration::from_secs(5)))
        .max_response_header_size(16 * 1024)
        .user_agent(crate::source::USER_AGENT)
        .build();
    let agent = ureq::Agent::with_parts(config, DefaultConnector::default(), OwnedResolver);
    let mut response = agent
        .get(url)
        .header("Accept", "application/json")
        .header("Accept-Encoding", "identity")
        .call()
        .map_err(|error| format!("OSM transport: {error}"))?;
    if stop.load(Ordering::Relaxed) {
        return Err("OSM load cancelled".into());
    }
    let status = response.status().as_u16();
    if status == 429 || status == 503 {
        let seconds = response.headers().get("retry-after").map_or(3600, |value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.trim().parse::<u64>().ok())
                .unwrap_or(u64::MAX)
        });
        gate.defer(Instant::now(), seconds);
    }
    if !(200..300).contains(&status) {
        return Err(format!("Overpass HTTP {status}; no automatic retry"));
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    if !content_type.eq_ignore_ascii_case("application/json") {
        return Err("Overpass response must be application/json".into());
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_JSON_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("OSM response read: {error}"))?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err("OSM response exceeds 16 MiB limit".into());
    }
    if stop.load(Ordering::Relaxed) {
        return Err("OSM load cancelled".into());
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gate_holds_one_request_and_enforces_start_spacing_after_drop() {
        let gate = Gate::default();
        let now = Instant::now();
        let permit = gate.reserve(now).unwrap();
        assert!(gate.reserve(now + SPACING).is_err());
        drop(permit);
        assert!(gate.reserve(now + Duration::from_secs(59)).is_err());
        assert!(gate.reserve(now + SPACING).is_ok());
    }
    #[test]
    fn retry_after_extends_cooldown_and_overflow_fails_closed() {
        let gate = Gate::default();
        let now = Instant::now();
        gate.defer(now, 120);
        assert!(gate.reserve(now + SPACING).is_err());
        assert!(gate.reserve(now + Duration::from_secs(120)).is_ok());
        gate.defer(now, u64::MAX);
        assert!(gate.reserve(now + Duration::from_secs(1000)).is_err());
    }
    #[test]
    fn invalid_or_cancelled_requests_do_not_reach_network() {
        let stop = AtomicBool::new(false);
        for url in [
            "http://example.org",
            "https://user:password@example.org/api",
            "https:///api",
            "not a uri",
        ] {
            assert!(fetch(url, &stop).is_err());
        }
        assert!(fetch("https://example.org/api", &AtomicBool::new(true)).is_err());
    }
}
