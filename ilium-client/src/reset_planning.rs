//! Public reset-announcement monitoring for the client status bar.
//!
//! These unauthenticated feeds describe discretionary, provider-wide resets.
//! They do not expose a user's own rolling quota window. In particular the
//! Claude feed currently has no scheduled-reset contract, and a Codex
//! announcement may be scheduled without an exact `scheduled_for` time.

use std::io::Read;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

const CODEX_STATUS_URL: &str = "https://codex-resets.com/api/v1/status";
const CODEX_LEGACY_URL: &str = "https://codex-resets.com/api/resets";
const CLAUDE_RESETS_URL: &str = "https://claude-resets.com/api/resets";
const POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_SOURCE_AGE: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResetTimeStyle {
    #[default]
    Exact,
    Human,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResetPlanningSettings {
    pub monitor_claude: bool,
    pub monitor_codex: bool,
    pub time_style: ResetTimeStyle,
}

impl Default for ResetPlanningSettings {
    fn default() -> Self {
        Self {
            monitor_claude: true,
            monitor_codex: true,
            time_style: ResetTimeStyle::Exact,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetProvider {
    Claude,
    Codex,
}

impl ResetProvider {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    pub const fn enabled(self, settings: &ResetPlanningSettings) -> bool {
        match self {
            Self::Claude => settings.monitor_claude,
            Self::Codex => settings.monitor_codex,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledReset {
    pub announced_at: DateTime<Utc>,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub source_url: String,
    pub is_banked: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderStatus {
    pub last_checked: Option<SystemTime>,
    pub last_error: Option<String>,
    pub scheduled: Option<ScheduledReset>,
}

#[derive(Debug, Default, Clone)]
pub struct ResetMonitorState {
    pub claude: ProviderStatus,
    pub codex: ProviderStatus,
}

impl ResetMonitorState {
    pub fn provider(&self, provider: ResetProvider) -> &ProviderStatus {
        match provider {
            ResetProvider::Claude => &self.claude,
            ResetProvider::Codex => &self.codex,
        }
    }

    pub fn apply(&mut self, event: MonitorEvent, settings: &ResetPlanningSettings) {
        if !event.provider.enabled(settings) {
            return;
        }
        let state = match event.provider {
            ResetProvider::Claude => &mut self.claude,
            ResetProvider::Codex => &mut self.codex,
        };
        state.last_checked = Some(SystemTime::now());
        match event.result {
            Ok(scheduled) => {
                if let Some(warning) = event.source_warning {
                    // Legacy has no upstream-freshness contract. It may add a
                    // newer announcement, but cannot retract or roll back one
                    // already obtained from the versioned status endpoint.
                    if let Some(candidate) = scheduled {
                        let is_newer = state
                            .scheduled
                            .as_ref()
                            .is_none_or(|retained| candidate.announced_at > retained.announced_at);
                        if is_newer {
                            state.scheduled = Some(candidate);
                        }
                    }
                    state.last_error = Some(warning);
                } else {
                    state.scheduled = scheduled;
                    state.last_error = None;
                }
            }
            Err(error) => {
                // Keep a previously confirmed future announcement through a
                // transient outage; the UI also exposes the fetch failure.
                state.last_error = Some(error);
            }
        }
    }

    pub fn display_tick_interval(&self, settings: &ResetPlanningSettings) -> Option<Duration> {
        let has_timed_reset = [ResetProvider::Claude, ResetProvider::Codex]
            .into_iter()
            .any(|provider| {
                provider.enabled(settings)
                    && self
                        .provider(provider)
                        .scheduled
                        .as_ref()
                        .is_some_and(|reset| reset.scheduled_for.is_some())
            });
        has_timed_reset.then_some(match settings.time_style {
            ResetTimeStyle::Exact => Duration::from_secs(1),
            ResetTimeStyle::Human => Duration::from_secs(60),
        })
    }
}

#[derive(Debug)]
pub struct MonitorEvent {
    pub provider: ResetProvider,
    pub result: Result<Option<ScheduledReset>, String>,
    /// A usable legacy observation arrived after the primary source failed.
    pub source_warning: Option<String>,
}

#[derive(Debug)]
struct FetchOutcome {
    result: Result<Option<ScheduledReset>, String>,
    source_warning: Option<String>,
    retry_after: Option<Duration>,
}

#[derive(Debug)]
struct FetchedBody {
    body: String,
    age: Duration,
}

#[derive(Debug)]
struct FetchFailure {
    message: String,
    may_use_legacy: bool,
    retry_after: Option<Duration>,
}

impl FetchFailure {
    fn malformed(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            may_use_legacy: true,
            retry_after: None,
        }
    }

    fn transport(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            may_use_legacy: false,
            retry_after: None,
        }
    }

    fn http(url: &str, status: u16, retry_after: Option<Duration>) -> Self {
        Self {
            message: format!("{url}: HTTP {status}"),
            may_use_legacy: matches!(status, 404 | 405 | 410 | 500..=599) && retry_after.is_none(),
            retry_after,
        }
    }
}

fn failed_outcome(failure: FetchFailure) -> FetchOutcome {
    FetchOutcome {
        result: Err(failure.message),
        source_warning: None,
        retry_after: failure.retry_after,
    }
}

/// Own the polling task in the client's event loop. Dropping or aborting the
/// returned handle stops recurring work when this attached client exits.
pub fn spawn_monitor(
    settings_rx: watch::Receiver<ResetPlanningSettings>,
    events_tx: mpsc::Sender<MonitorEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut settings_rx = settings_rx;
        let mut next_checks = [Some(Instant::now()); 2];
        loop {
            for (index, provider) in [ResetProvider::Claude, ResetProvider::Codex]
                .into_iter()
                .enumerate()
            {
                let settings = settings_rx.borrow().clone();
                if !provider.enabled(&settings)
                    || next_checks[index].is_none_or(|deadline| deadline > Instant::now())
                {
                    continue;
                }
                let outcome =
                    match tokio::task::spawn_blocking(move || fetch_provider(provider)).await {
                        Ok(outcome) => outcome,
                        Err(error) => failed_outcome(FetchFailure::transport(format!(
                            "reset monitor worker failed: {error}"
                        ))),
                    };
                next_checks[index] = next_poll_deadline(Instant::now(), outcome.retry_after);
                if events_tx
                    .send(MonitorEvent {
                        provider,
                        result: outcome.result,
                        source_warning: outcome.source_warning,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
            }

            let settings = settings_rx.borrow().clone();
            let next_deadline = [ResetProvider::Claude, ResetProvider::Codex]
                .into_iter()
                .enumerate()
                .filter(|(_, provider)| provider.enabled(&settings))
                .filter_map(|(index, _)| next_checks[index])
                .min();
            match next_deadline {
                Some(deadline) => tokio::select! {
                    changed = settings_rx.changed() => {
                        if changed.is_err() { return; }
                    }
                    () = tokio::time::sleep_until(deadline) => {}
                },
                None => {
                    if settings_rx.changed().await.is_err() {
                        return;
                    }
                }
            }
        }
    })
}

fn next_poll_deadline(completed_at: Instant, retry_after: Option<Duration>) -> Option<Instant> {
    // A server delay may be longer than the normal cadence. Checked addition
    // avoids wrapping an enormous Retry-After into an immediate busy loop.
    completed_at.checked_add(POLL_INTERVAL.max(retry_after.unwrap_or_default()))
}

fn fetch_provider(provider: ResetProvider) -> FetchOutcome {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .max_redirects(0)
            .build(),
    );
    fetch_provider_with(provider, Utc::now(), |url| fetch_body(&agent, url))
}

fn fetch_body(agent: &ureq::Agent, url: &str) -> Result<FetchedBody, FetchFailure> {
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| FetchFailure::transport(format!("{url}: {error}")))?;
    let status = response.status().as_u16();
    if status != 200 {
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|header| header.to_str().ok())
            .and_then(|header| parse_retry_after(header, Utc::now()));
        return Err(FetchFailure::http(url, status, retry_after));
    }
    let age = match response.headers().get("age") {
        None => Duration::ZERO,
        Some(header) => {
            let seconds = header
                .to_str()
                .ok()
                .and_then(|text| text.parse::<u64>().ok())
                .ok_or_else(|| FetchFailure::malformed(format!("{url}: invalid HTTP Age")))?;
            Duration::from_secs(seconds)
        }
    };
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| FetchFailure::transport(format!("{url}: body read failed: {error}")))?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(FetchFailure::malformed(format!(
            "{url}: reset feed exceeded the response size limit"
        )));
    }
    let body = String::from_utf8(bytes)
        .map_err(|_| FetchFailure::malformed(format!("{url}: invalid UTF-8")))?;
    Ok(FetchedBody { body, age })
}

fn parse_retry_after(value: &str, now: DateTime<Utc>) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let deadline = DateTime::parse_from_rfc2822(value).ok()?;
    let delay = deadline.signed_duration_since(now);
    if delay <= chrono::Duration::zero() {
        return Some(Duration::ZERO);
    }
    delay.to_std().ok()
}

fn fetch_provider_with<F>(provider: ResetProvider, now: DateTime<Utc>, mut fetch: F) -> FetchOutcome
where
    F: FnMut(&str) -> Result<FetchedBody, FetchFailure>,
{
    match provider {
        ResetProvider::Claude => {
            let result = fetch(CLAUDE_RESETS_URL).and_then(|document| {
                parse_claude_feed(&document.body, now, document.age)
                    .map_err(FetchFailure::malformed)
            });
            match result {
                Ok(scheduled) => FetchOutcome {
                    result: Ok(scheduled),
                    source_warning: None,
                    retry_after: None,
                },
                Err(failure) => failed_outcome(failure),
            }
        }
        ResetProvider::Codex => {
            let primary = fetch(CODEX_STATUS_URL).and_then(|document| {
                parse_codex_status(&document.body, now, document.age)
                    .map_err(FetchFailure::malformed)
            });
            match primary {
                Ok(scheduled) => FetchOutcome {
                    result: Ok(scheduled),
                    source_warning: None,
                    retry_after: None,
                },
                Err(primary_failure) if primary_failure.may_use_legacy => {
                    let legacy = fetch(CODEX_LEGACY_URL).and_then(|document| {
                        validate_http_age(document.age)
                            .and_then(|()| parse_codex_legacy(&document.body))
                            .map_err(FetchFailure::malformed)
                    });
                    match legacy {
                        Ok(scheduled) => FetchOutcome {
                            result: Ok(scheduled),
                            source_warning: Some(format!(
                                "Codex v1 failed ({}); using legacy feed with unknown upstream freshness",
                                primary_failure.message
                            )),
                            retry_after: None,
                        },
                        Err(legacy_failure) => failed_outcome(FetchFailure {
                            message: format!(
                                "Codex v1 failed ({}); legacy failed ({})",
                                primary_failure.message, legacy_failure.message
                            ),
                            may_use_legacy: false,
                            retry_after: legacy_failure.retry_after,
                        }),
                    }
                }
                Err(failure) => failed_outcome(failure),
            }
        }
    }
}

fn validate_http_age(age: Duration) -> Result<(), String> {
    if age > MAX_SOURCE_AGE {
        Err("reset feed HTTP Age exceeds six hours".to_owned())
    } else {
        Ok(())
    }
}

fn validate_recent_timestamp(
    value: &serde_json::Value,
    field: &str,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let timestamp = parse_timestamp(value, field)?;
    if timestamp.signed_duration_since(now) > chrono::Duration::minutes(5) {
        return Err(format!("reset feed {field} is in the future"));
    }
    if now.signed_duration_since(timestamp) > chrono::Duration::hours(6) {
        return Err(format!("reset feed {field} is older than six hours"));
    }
    Ok(())
}

fn parse_is_banked(value: &serde_json::Value) -> Result<bool, String> {
    match value.get("reset_type").and_then(|field| field.as_str()) {
        Some("regular") => Ok(false),
        Some("banked") => Ok(true),
        _ => Err("reset announcement has an invalid reset_type".to_owned()),
    }
}

fn parse_optional_timestamp(
    value: &serde_json::Value,
    field: &str,
) -> Result<Option<DateTime<Utc>>, String> {
    match value.get(field) {
        Some(serde_json::Value::Null) => Ok(None),
        Some(_) => parse_timestamp(value, field).map(Some),
        None => Err(format!("reset announcement has no {field} field")),
    }
}

fn parse_codex_source_url(source: &serde_json::Value) -> Result<String, String> {
    match source.get("type").and_then(|value| value.as_str()) {
        Some("x_post") => {
            if source.get("author").and_then(|value| value.as_str()) != Some("thsottiaux") {
                return Err("Codex scheduled reset has an unknown source author".to_owned());
            }
            source
                .get("url")
                .and_then(|value| value.as_str())
                .filter(|url| url.starts_with("https://x.com/"))
                .map(str::to_owned)
                .ok_or_else(|| "Codex scheduled reset has no valid source post".to_owned())
        }
        Some("observed") => match source.get("url") {
            None => Ok(CODEX_STATUS_URL.to_owned()),
            Some(value) => value
                .as_str()
                .filter(|url| url.starts_with("https://"))
                .map(str::to_owned)
                .ok_or_else(|| "Codex observed reset has an invalid source URL".to_owned()),
        },
        _ => Err("Codex scheduled reset has an unknown source type".to_owned()),
    }
}

fn parse_codex_status(
    body: &str,
    now: DateTime<Utc>,
    age: Duration,
) -> Result<Option<ScheduledReset>, String> {
    validate_http_age(age)?;
    let json: serde_json::Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let meta = json
        .get("meta")
        .ok_or("Codex reset feed has no meta object")?;
    if meta.get("api_version").and_then(|value| value.as_str()) != Some("v1") {
        return Err("Codex reset feed has an unsupported API version".to_owned());
    }
    validate_recent_timestamp(meta, "generated_at", now)?;
    let data = json
        .get("data")
        .ok_or("Codex reset feed has no data object")?;
    let scheduled = data
        .get("scheduled_reset")
        .ok_or("Codex reset feed has no scheduled_reset field")?;
    if scheduled.is_null() {
        return Ok(None);
    }
    if scheduled.get("status").and_then(|value| value.as_str()) != Some("scheduled") {
        return Err("Codex scheduled reset has an unexpected status".to_owned());
    }
    if scheduled
        .get("id")
        .and_then(|value| value.as_str())
        .is_none_or(str::is_empty)
    {
        return Err("Codex scheduled reset has no id".to_owned());
    }
    let announced_at = parse_timestamp(scheduled, "announced_at")?;
    let scheduled_for = parse_optional_timestamp(scheduled, "scheduled_for")?;
    let source = scheduled
        .get("source")
        .ok_or("Codex scheduled reset has no source")?;
    let source_url = parse_codex_source_url(source)?;
    Ok(Some(ScheduledReset {
        announced_at,
        scheduled_for,
        source_url,
        is_banked: parse_is_banked(scheduled)?,
    }))
}

fn parse_codex_legacy(body: &str) -> Result<Option<ScheduledReset>, String> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let scheduled = json
        .get("scheduled")
        .ok_or("legacy Codex reset feed has no scheduled field")?;
    if scheduled.is_null() {
        return Ok(None);
    }
    if scheduled
        .get("tweet_id")
        .and_then(|value| value.as_str())
        .is_none_or(str::is_empty)
    {
        return Err("legacy Codex scheduled reset has no tweet_id".to_owned());
    }
    let source_url = scheduled
        .get("tweet_url")
        .and_then(|value| value.as_str())
        .filter(|url| url.starts_with("https://x.com/"))
        .ok_or("legacy Codex scheduled reset has no valid tweet_url")?
        .to_owned();
    Ok(Some(ScheduledReset {
        announced_at: parse_timestamp(scheduled, "announced_at")?,
        scheduled_for: parse_optional_timestamp(scheduled, "scheduled_for")?,
        source_url,
        is_banked: parse_is_banked(scheduled)?,
    }))
}

fn parse_claude_feed(
    body: &str,
    now: DateTime<Utc>,
    age: Duration,
) -> Result<Option<ScheduledReset>, String> {
    validate_http_age(age)?;
    let json: serde_json::Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let claude = json
        .pointer("/providers/claude")
        .ok_or("Claude reset feed has no provider object")?;
    if json.get("scheduled_reset").is_some()
        || claude.get("scheduled_reset").is_some()
        || claude.get("scheduled").is_some()
    {
        return Err("Claude reset feed has an unrecognized schedule contract".to_owned());
    }
    let events = claude
        .get("events")
        .and_then(|value| value.as_array())
        .ok_or("Claude reset feed has no events list")?;
    for event in events {
        match event.get("kind").and_then(|value| value.as_str()) {
            Some("reset" | "policy") => {}
            _ => return Err("Claude reset feed has an unknown event kind".to_owned()),
        }
        if let Some(reset_type) = event.get("resetType") {
            match reset_type.as_str() {
                Some("regular" | "banked") => {}
                _ => return Err("Claude reset feed has an unknown resetType".to_owned()),
            }
        }
    }
    let meta = json
        .get("meta")
        .ok_or("Claude reset feed has no meta object")?;
    validate_recent_timestamp(meta, "asOf", now)?;
    let detector = meta
        .get("detector")
        .ok_or("Claude reset feed has no detector metadata")?;
    if detector.get("status").and_then(|value| value.as_str()) != Some("fresh") {
        return Err("Claude reset detector is not fresh".to_owned());
    }
    validate_recent_timestamp(detector, "asOf", now)?;
    validate_recent_timestamp(detector, "lastSuccessfulCheckAt", now)?;
    // This catalog has historical resets/policy changes and detector health,
    // but no structured future execution time or pending-reset lifecycle.
    Ok(None)
}

fn parse_timestamp(value: &serde_json::Value, field: &str) -> Result<DateTime<Utc>, String> {
    let text = value
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("reset announcement has no {field}"))?;
    DateTime::parse_from_rfc3339(text)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| format!("invalid {field}: {error}"))
}

pub fn countdown_text(
    provider: ResetProvider,
    scheduled: &ScheduledReset,
    style: ResetTimeStyle,
    now: DateTime<Utc>,
) -> String {
    let name = if scheduled.is_banked {
        format!("{} banked reset", provider.label())
    } else {
        format!("{} reset", provider.label())
    };
    let Some(target) = scheduled.scheduled_for else {
        return format!("{name} scheduled · time TBD");
    };
    let remaining = (target - now).num_seconds();
    if remaining <= 0 {
        return format!("{name} due · awaiting confirmation");
    }
    let duration = match style {
        ResetTimeStyle::Exact => {
            let days = remaining / 86_400;
            let hours = (remaining % 86_400) / 3_600;
            let minutes = (remaining % 3_600) / 60;
            let seconds = remaining % 60;
            if days > 0 {
                format!("{days}d {hours}h {minutes}m {seconds}s")
            } else if hours > 0 {
                format!("{hours}h {minutes}m {seconds}s")
            } else {
                format!("{minutes}m {seconds}s")
            }
        }
        ResetTimeStyle::Human => {
            let local_target = target.with_timezone(&Local);
            let local_now = now.with_timezone(&Local);
            if local_target.date_naive()
                == local_now
                    .date_naive()
                    .succ_opt()
                    .unwrap_or(local_now.date_naive())
            {
                "tomorrow".to_owned()
            } else if remaining >= 86_400 {
                let days = (remaining + 43_200) / 86_400;
                format!("{days} {}", if days == 1 { "day" } else { "days" })
            } else if remaining >= 3_600 {
                let hours = (remaining + 1_800) / 3_600;
                format!("{hours} {}", if hours == 1 { "hour" } else { "hours" })
            } else {
                let minutes = ((remaining + 30) / 60).max(1);
                format!(
                    "{minutes} {}",
                    if minutes == 1 { "minute" } else { "minutes" }
                )
            }
        }
    };
    if duration == "tomorrow" {
        format!("{name} tomorrow")
    } else {
        format!("{name} in {duration}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-27T04:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn v1(scheduled: Value) -> String {
        json!({"data":{"scheduled_reset":scheduled},"meta":{
            "api_version":"v1","generated_at":"2026-09-27T03:43:37.281Z"
        }})
        .to_string()
    }

    fn pending() -> Value {
        json!({
            "id":"2103963215885701493","status":"scheduled","reset_type":"regular",
            "announced_at":"2026-09-26T21:41:35.000Z","scheduled_for":null,
            "text":"More resets coming next week",
            "source":{"type":"x_post","author":"thsottiaux",
                "url":"https://x.com/thsottiaux/status/2103963215885701493"}
        })
    }

    fn fetched(body: String) -> FetchedBody {
        FetchedBody {
            body,
            age: Duration::ZERO,
        }
    }

    fn claude() -> Value {
        json!({
            "providers":{"claude":{"events":[
                {"kind":"reset","resetType":"banked","date":"2026-09-22T16:44:06Z",
                    "verification":"curated","note":"apply any time until October 22"},
                {"kind":"policy","date":"2026-09-14T00:00:00Z","verification":"curated"},
                {"kind":"reset","date":"2026-09-01T00:00:00Z","verification":"provisional"}
            ]}},
            "meta":{"asOf":"2026-09-27T03:44:29Z","detector":{
                "status":"fresh","asOf":"2026-09-27T03:44:29Z",
                "lastSuccessfulCheckAt":"2026-09-27T03:43:05Z"
            }}
        })
    }

    #[test]
    fn codex_scheduled_without_time_is_not_a_countdown() {
        let scheduled = parse_codex_status(&v1(pending()), now(), Duration::ZERO)
            .unwrap()
            .unwrap();
        assert_eq!(scheduled.scheduled_for, None);
        assert_eq!(
            countdown_text(
                ResetProvider::Codex,
                &scheduled,
                ResetTimeStyle::Exact,
                now()
            ),
            "Codex reset scheduled · time TBD"
        );
    }

    #[test]
    fn codex_explicit_target_offset_and_due_are_not_completion() {
        let mut event = pending();
        event["scheduled_for"] = json!("2026-09-28T02:00:00+02:00");
        let scheduled = parse_codex_status(&v1(event), now(), Duration::ZERO)
            .unwrap()
            .unwrap();
        let target = DateTime::parse_from_rfc3339("2026-09-28T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(scheduled.scheduled_for, Some(target));
        assert_eq!(
            countdown_text(
                ResetProvider::Codex,
                &scheduled,
                ResetTimeStyle::Exact,
                target
            ),
            "Codex reset due · awaiting confirmation"
        );
    }

    #[test]
    fn codex_v1_rejects_unknown_type_missing_target_and_stale_metadata() {
        let mut event = pending();
        event["reset_type"] = json!("unknown");
        assert!(parse_codex_status(&v1(event), now(), Duration::ZERO).is_err());
        let mut event = pending();
        event.as_object_mut().unwrap().remove("scheduled_for");
        assert!(parse_codex_status(&v1(event), now(), Duration::ZERO).is_err());
        assert!(parse_codex_status(
            &v1(Value::Null),
            now() + chrono::Duration::days(1),
            Duration::ZERO
        )
        .is_err());
        assert!(
            parse_codex_status(&v1(Value::Null), now(), Duration::from_secs(7 * 3600)).is_err()
        );
    }

    #[test]
    fn codex_observed_source_uses_status_page_without_url() {
        let mut event = pending();
        event["source"] = json!({"type":"observed"});
        let scheduled = parse_codex_status(&v1(event), now(), Duration::ZERO)
            .unwrap()
            .unwrap();
        assert_eq!(scheduled.source_url, CODEX_STATUS_URL);
    }

    #[test]
    fn legacy_watch_does_not_schedule_and_banked_pending_has_unknown_time() {
        assert!(
            parse_codex_legacy(r#"{"scheduled":null,"watch":{"reset_chance_percent":60}}"#)
                .unwrap()
                .is_none()
        );
        assert!(parse_codex_legacy(r#"{"events":[],"watch":null}"#).is_err());
        let scheduled = parse_codex_legacy(
            r#"{"scheduled":{
            "tweet_id":"2101352781219258527","text":"Tuesday",
            "announced_at":"2026-09-19T16:48:38Z","scheduled_for":null,
            "reset_type":"banked","tweet_url":"https://x.com/thsottiaux/status/2101352781219258527"
        }}"#,
        )
        .unwrap()
        .unwrap();
        assert!(scheduled.is_banked);
        assert_eq!(scheduled.scheduled_for, None);
        assert_eq!(
            countdown_text(
                ResetProvider::Codex,
                &scheduled,
                ResetTimeStyle::Exact,
                now()
            ),
            "Codex banked reset scheduled · time TBD"
        );
    }

    #[test]
    fn primary_success_avoids_fallback_and_malformed_primary_uses_one_legacy_request() {
        let mut urls = Vec::new();
        let healthy = fetch_provider_with(ResetProvider::Codex, now(), |url| {
            urls.push(url.to_owned());
            Ok(fetched(v1(Value::Null)))
        });
        assert!(healthy.result.unwrap().is_none());
        assert_eq!(urls, vec![CODEX_STATUS_URL]);
        urls.clear();
        let degraded = fetch_provider_with(ResetProvider::Codex, now(), |url| {
            urls.push(url.to_owned());
            if url == CODEX_STATUS_URL {
                Ok(fetched("{".to_owned()))
            } else {
                Ok(fetched(
                    r#"{"scheduled":null,"watch":{"reset_chance_percent":60}}"#.to_owned(),
                ))
            }
        });
        assert!(degraded.result.unwrap().is_none());
        assert!(degraded
            .source_warning
            .unwrap()
            .contains("unknown upstream freshness"));
        assert_eq!(urls, vec![CODEX_STATUS_URL, CODEX_LEGACY_URL]);
    }

    #[test]
    fn rate_limit_and_transport_failure_do_not_cascade() {
        let mut requests = 0;
        let limited = fetch_provider_with(ResetProvider::Codex, now(), |_| {
            requests += 1;
            Err(FetchFailure::http(
                CODEX_STATUS_URL,
                429,
                Some(Duration::from_secs(7200)),
            ))
        });
        assert!(limited.result.is_err());
        assert_eq!(limited.retry_after, Some(Duration::from_secs(7200)));
        assert_eq!(requests, 1);
        requests = 0;
        let unavailable = fetch_provider_with(ResetProvider::Codex, now(), |_| {
            requests += 1;
            Err(FetchFailure::transport("network unavailable"))
        });
        assert!(unavailable.result.is_err());
        assert_eq!(requests, 1);
        assert!(!FetchFailure::http(CODEX_STATUS_URL, 503, Some(Duration::ZERO)).may_use_legacy);
        assert!(FetchFailure::http(CODEX_STATUS_URL, 503, None).may_use_legacy);
    }

    #[test]
    fn retry_after_and_poll_deadline_preserve_hourly_floor() {
        let then = DateTime::parse_from_rfc3339("1994-11-06T08:48:37Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(parse_retry_after("90", then), Some(Duration::from_secs(90)));
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT", then),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:47:37 GMT", then),
            Some(Duration::ZERO)
        );
        assert_eq!(parse_retry_after("invalid", then), None);
        let start = Instant::now();
        assert_eq!(
            next_poll_deadline(start, None),
            start.checked_add(POLL_INTERVAL)
        );
        assert_eq!(
            next_poll_deadline(start, Some(Duration::from_secs(7200))),
            start.checked_add(Duration::from_secs(7200))
        );
    }

    #[test]
    fn claude_history_and_banked_grant_have_no_schedule() {
        let payload = claude();
        assert!(
            parse_claude_feed(&payload.to_string(), now(), Duration::ZERO)
                .unwrap()
                .is_none()
        );
        let mut changed = payload;
        changed["providers"]["claude"]["events"][0]["kind"] = json!("scheduled");
        assert!(parse_claude_feed(&changed.to_string(), now(), Duration::ZERO).is_err());
    }

    #[test]
    fn claude_stale_detector_or_new_schedule_contract_is_not_healthy_none() {
        let mut changed = claude();
        changed["meta"]["detector"]["lastSuccessfulCheckAt"] = json!("2026-09-26T00:00:00Z");
        assert!(parse_claude_feed(&changed.to_string(), now(), Duration::ZERO).is_err());
        let mut changed = claude();
        changed["providers"]["claude"]["scheduled_reset"] = Value::Null;
        assert!(parse_claude_feed(&changed.to_string(), now(), Duration::ZERO).is_err());
    }

    #[test]
    fn degraded_legacy_cannot_retract_or_roll_back_retained_announcement() {
        let retained = parse_codex_status(&v1(pending()), now(), Duration::ZERO)
            .unwrap()
            .unwrap();
        let mut state = ResetMonitorState::default();
        state.codex.scheduled = Some(retained.clone());
        let settings = ResetPlanningSettings::default();
        state.apply(
            MonitorEvent {
                provider: ResetProvider::Codex,
                result: Ok(None),
                source_warning: Some("primary failed; legacy used".to_owned()),
            },
            &settings,
        );
        assert_eq!(state.codex.scheduled, Some(retained.clone()));
        let older = ScheduledReset {
            announced_at: retained.announced_at - chrono::Duration::days(1),
            scheduled_for: None,
            source_url: CODEX_LEGACY_URL.to_owned(),
            is_banked: true,
        };
        state.apply(
            MonitorEvent {
                provider: ResetProvider::Codex,
                result: Ok(Some(older)),
                source_warning: Some("legacy used".to_owned()),
            },
            &settings,
        );
        assert_eq!(state.codex.scheduled, Some(retained));
        assert!(state.codex.last_error.is_some());
        state.apply(
            MonitorEvent {
                provider: ResetProvider::Codex,
                result: Ok(None),
                source_warning: None,
            },
            &settings,
        );
        assert!(state.codex.scheduled.is_none());
        assert!(state.codex.last_error.is_none());
    }

    #[test]
    fn disabled_provider_ignores_late_result() {
        let mut state = ResetMonitorState::default();
        let settings = ResetPlanningSettings {
            monitor_codex: false,
            ..ResetPlanningSettings::default()
        };
        state.apply(
            MonitorEvent {
                provider: ResetProvider::Codex,
                result: Err("late failure".to_owned()),
                source_warning: None,
            },
            &settings,
        );
        assert!(state.codex.last_checked.is_none());
        assert!(state.codex.last_error.is_none());
    }

    #[test]
    fn exact_countdown_has_seconds_and_due_waits_for_confirmation() {
        let start = DateTime::parse_from_rfc3339("2026-09-27T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let scheduled = ScheduledReset {
            announced_at: start,
            scheduled_for: Some(
                start
                    + chrono::Duration::days(1)
                    + chrono::Duration::hours(4)
                    + chrono::Duration::minutes(7)
                    + chrono::Duration::seconds(32),
            ),
            source_url: "https://x.com/thsottiaux/status/1".to_owned(),
            is_banked: false,
        };
        assert_eq!(
            countdown_text(
                ResetProvider::Codex,
                &scheduled,
                ResetTimeStyle::Exact,
                start
            ),
            "Codex reset in 1d 4h 7m 32s"
        );
        assert_eq!(
            countdown_text(
                ResetProvider::Codex,
                &scheduled,
                ResetTimeStyle::Exact,
                scheduled.scheduled_for.unwrap()
            ),
            "Codex reset due · awaiting confirmation"
        );
    }
}
