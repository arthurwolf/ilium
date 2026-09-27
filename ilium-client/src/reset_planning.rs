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

const CODEX_STATUS_URL: &str = "https://codex-resets.com/api/v1/status";
const CLAUDE_RESETS_URL: &str = "https://claude-resets.com/api/resets";
const POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

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
                state.scheduled = scheduled;
                state.last_error = None;
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
}

/// Own the polling task in the client's event loop. Dropping or aborting the
/// returned handle stops recurring work when this attached client exits.
pub fn spawn_monitor(
    settings_rx: watch::Receiver<ResetPlanningSettings>,
    events_tx: mpsc::Sender<MonitorEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut settings_rx = settings_rx;
        loop {
            let settings = settings_rx.borrow().clone();
            for provider in [ResetProvider::Claude, ResetProvider::Codex] {
                if !provider.enabled(&settings) {
                    continue;
                }
                let result = tokio::task::spawn_blocking(move || fetch_provider(provider)).await;
                let result = match result {
                    Ok(result) => result,
                    Err(error) => Err(format!("reset monitor worker failed: {error}")),
                };
                if events_tx
                    .send(MonitorEvent { provider, result })
                    .await
                    .is_err()
                {
                    return;
                }
            }

            tokio::select! {
                changed = settings_rx.changed() => {
                    if changed.is_err() { return; }
                }
                () = tokio::time::sleep(POLL_INTERVAL) => {}
            }
        }
    })
}

fn fetch_provider(provider: ResetProvider) -> Result<Option<ScheduledReset>, String> {
    let url = match provider {
        ResetProvider::Claude => CLAUDE_RESETS_URL,
        ResetProvider::Codex => CODEX_STATUS_URL,
    };
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build(),
    );
    let mut response = agent.get(url).call().map_err(|error| error.to_string())?;
    let mut body = String::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_string(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("reset feed exceeded the response size limit".to_owned());
    }
    match provider {
        ResetProvider::Claude => parse_claude_feed(&body),
        ResetProvider::Codex => parse_codex_status(&body),
    }
}

fn parse_codex_status(body: &str) -> Result<Option<ScheduledReset>, String> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let Some(data) = json.get("data") else {
        return Err("Codex reset feed has no data object".to_owned());
    };
    let Some(scheduled) = data.get("scheduled_reset") else {
        return Err("Codex reset feed has no scheduled_reset field".to_owned());
    };
    if scheduled.is_null() {
        return Ok(None);
    }
    if scheduled.get("status").and_then(|value| value.as_str()) != Some("scheduled") {
        return Err("Codex scheduled reset has an unexpected status".to_owned());
    }
    let announced_at = parse_timestamp(scheduled, "announced_at")?;
    let scheduled_for = match scheduled.get("scheduled_for") {
        Some(serde_json::Value::Null) => None,
        Some(_) => Some(parse_timestamp(scheduled, "scheduled_for")?),
        None => return Err("Codex scheduled reset has no scheduled_for field".to_owned()),
    };
    let source_url = scheduled
        .pointer("/source/url")
        .and_then(|value| value.as_str())
        .filter(|url| url.starts_with("https://x.com/"))
        .ok_or("Codex scheduled reset has no source post")?
        .to_owned();
    Ok(Some(ScheduledReset {
        announced_at,
        scheduled_for,
        source_url,
        is_banked: scheduled.get("reset_type").and_then(|value| value.as_str()) == Some("banked"),
    }))
}

fn parse_claude_feed(body: &str) -> Result<Option<ScheduledReset>, String> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    if !json
        .pointer("/providers/claude/events")
        .is_some_and(serde_json::Value::is_array)
    {
        return Err("Claude reset feed has no events list".to_owned());
    }
    // This public catalog currently records announcements after publication,
    // with no future `scheduled_for` contract. Do not turn historical gaps or
    // an announcement's prose into an invented personal countdown.
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

    #[test]
    fn codex_scheduled_without_time_is_not_a_countdown() {
        let scheduled = parse_codex_status(r#"{"data":{"scheduled_reset":{"status":"scheduled","announced_at":"2026-09-26T21:41:35Z","scheduled_for":null,"reset_type":"regular","source":{"url":"https://x.com/thsottiaux/status/1"}}}}"#)
            .unwrap().unwrap();
        assert_eq!(scheduled.scheduled_for, None);
        assert_eq!(
            countdown_text(
                ResetProvider::Codex,
                &scheduled,
                ResetTimeStyle::Exact,
                Utc::now()
            ),
            "Codex reset scheduled · time TBD"
        );
    }

    #[test]
    fn exact_countdown_has_seconds_and_due_waits_for_confirmation() {
        let now = DateTime::parse_from_rfc3339("2026-09-27T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let scheduled = ScheduledReset {
            announced_at: now,
            scheduled_for: Some(
                now + chrono::Duration::days(1)
                    + chrono::Duration::hours(4)
                    + chrono::Duration::minutes(7)
                    + chrono::Duration::seconds(32),
            ),
            source_url: "https://x.com/thsottiaux/status/1".to_owned(),
            is_banked: false,
        };
        assert_eq!(
            countdown_text(ResetProvider::Codex, &scheduled, ResetTimeStyle::Exact, now),
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

    #[test]
    fn claude_historical_events_do_not_create_a_future_reset() {
        assert!(parse_claude_feed(r#"{"providers":{"claude":{"events":[{"date":"2026-09-22T16:44:06Z","kind":"reset"}]}}}"#).unwrap().is_none());
    }
}
