//! The server runtime of reset planning: reads the public reset feeds hourly through the `http`
//! import, keeps what they announce, and shows it as one status bar segment.
//!
//! One one-shot timer is armed at a time. It fires when the next hourly read of a monitored
//! provider is due or, while a countdown is on screen, when the label must be redrawn (every
//! second in the exact style, every minute in the human style). The parsing and the text are the
//! pure functions of `reset.rs`; this file only moves their input and output through the host.
//!
//! The settings are the `[reset_planning]` table of `config.toml`, read with `kernel-settings`
//! and re-read when `kernel-settings-changed` names the tab.

use std::cell::RefCell;
use std::time::Duration;

use chrono::{DateTime, Utc};
use ilium_plugin_sdk::host::{
    HostClock, HostHttp, HostKernelSettings, HostLog, HostTimers, HttpRequest, HttpResponse, Level,
};
use ilium_plugin_sdk::types::{
    Event, Header, Intent, Outcome, RequestId, RequestRef, RichSpan, SetStatusSegmentSpec, Span,
    SpanTarget, Style, Submission, TimerId,
};
use ilium_plugin_sdk::HostApi;

use crate::reset::{
    advance, first_url, next_poll_millis, parse_retry_after, status_text, FetchFailure,
    FetchedBody, MonitorEvent, ResetMonitorState, ResetPlanningSettings, ResetProvider, Stage,
    Step, MAX_RESPONSE_BYTES, REQUEST_TIMEOUT_MILLIS,
};

/// The kernel settings tab this runtime reads (`[reset_planning]` of `config.toml`).
pub const SETTINGS_TAB: &str = "reset-planning";
/// The status bar segment that carries the label.
const STATUS_SEGMENT: &str = "reset-planning";
/// The longest wait of the timer, in milliseconds (settings are re-read on change, not on a timer).
const LONGEST_WAIT_MILLIS: i64 = 60 * 60 * 1000;

/// One request in flight for one provider.
#[derive(Debug, Clone)]
struct Inflight {
    request: RequestId,
    url: &'static str,
    stage: Stage,
}

#[derive(Default)]
struct Runtime {
    settings: ResetPlanningSettings,
    monitor: ResetMonitorState,
    /// Epoch milliseconds of the next read per provider; `None` reads at the next drive.
    next_poll: [Option<i64>; 2],
    inflight: [Option<Inflight>; 2],
    timer: Option<TimerId>,
    /// The label on screen, to submit only changes.
    shown: Option<String>,
}

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default());
}

fn with_runtime<T>(body: impl FnOnce(&mut Runtime) -> T) -> T {
    RUNTIME.with(|cell| body(&mut cell.borrow_mut()))
}

fn log(api: &mut dyn HostApi, level: Level, message: String) {
    HostLog::log(api, level, message);
}

fn now_millis(api: &mut dyn HostApi) -> i64 {
    i64::try_from(HostClock::now_unix_millis(api)).unwrap_or(i64::MAX)
}

fn datetime(millis: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_millis(millis).unwrap_or_default()
}

fn load_settings(api: &mut dyn HostApi) -> ResetPlanningSettings {
    match HostKernelSettings::read(api, SETTINGS_TAB.to_string()) {
        Ok(text) => ResetPlanningSettings::from_json(&text),
        Err(error) => {
            log(
                api,
                Level::Warn,
                format!("reset planning settings unreadable: {}", error.message),
            );
            ResetPlanningSettings::default()
        }
    }
}

/// Starts monitoring at activation: reads the settings and begins the first reads.
pub fn start(api: &mut dyn HostApi) {
    let settings = load_settings(api);
    with_runtime(|runtime| *runtime = Runtime::default());
    with_runtime(|runtime| runtime.settings = settings);
    drive(api);
}

/// Routes one event of the batch.
pub fn on_event(api: &mut dyn HostApi, event: &Event) -> Vec<Submission> {
    match event {
        Event::TimerFired(timer) => {
            let ours = with_runtime(|runtime| {
                let ours = runtime.timer == Some(*timer);
                if ours {
                    runtime.timer = None;
                }
                ours
            });
            if ours {
                drive(api)
            } else {
                Vec::new()
            }
        }
        Event::RequestCompleted(result) => match &result.request {
            RequestRef::Call(request) => on_completed(api, *request, &result.outcome),
            RequestRef::Submission(_) => Vec::new(),
        },
        Event::KernelSettingsChanged(change) if change.tab == SETTINGS_TAB => {
            on_settings_changed(api)
        }
        _ => Vec::new(),
    }
}

/// The settings were edited: a provider switched on or off starts from nothing.
fn on_settings_changed(api: &mut dyn HostApi) -> Vec<Submission> {
    let settings = load_settings(api);
    with_runtime(|runtime| {
        for provider in ResetProvider::ALL {
            if provider.enabled(&runtime.settings) != provider.enabled(&settings) {
                runtime.monitor.clear(provider);
                runtime.next_poll[provider.index()] = None;
                // A late answer of the old request finds no request and is dropped.
                runtime.inflight[provider.index()] = None;
            }
        }
        runtime.settings = settings;
    });
    drive(api)
}

/// Starts every read that is due, redraws the label and arms the timer.
fn drive(api: &mut dyn HostApi) -> Vec<Submission> {
    let now = now_millis(api);
    for provider in ResetProvider::ALL {
        let due = with_runtime(|runtime| {
            provider.enabled(&runtime.settings)
                && runtime.inflight[provider.index()].is_none()
                && runtime.next_poll[provider.index()].is_none_or(|deadline| deadline <= now)
        });
        if due {
            begin(api, provider, first_url(provider), Stage::Primary, now);
        }
    }
    let mut submissions = Vec::new();
    submissions.extend(publish(now));
    rearm(api, now);
    submissions
}

/// Sends the request of one stage of a provider's read.
fn begin(
    api: &mut dyn HostApi,
    provider: ResetProvider,
    url: &'static str,
    stage: Stage,
    now: i64,
) {
    let request = HttpRequest {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: vec![Header {
            name: "Accept".to_string(),
            value: "application/json".to_string(),
        }],
        body: None,
        timeout_millis: REQUEST_TIMEOUT_MILLIS,
    };
    match HostHttp::fetch(api, request) {
        Ok(request) => with_runtime(|runtime| {
            runtime.inflight[provider.index()] = Some(Inflight {
                request,
                url,
                stage,
            });
        }),
        Err(error) => {
            let failure = FetchFailure::transport(format!("{url}: {}", error.message));
            step(api, provider, stage, Err(failure), now);
        }
    }
}

fn on_completed(api: &mut dyn HostApi, request: RequestId, outcome: &Outcome) -> Vec<Submission> {
    let found = with_runtime(|runtime| {
        ResetProvider::ALL.into_iter().find_map(|provider| {
            runtime.inflight[provider.index()]
                .as_ref()
                .filter(|inflight| inflight.request == request)
                .cloned()
                .map(|inflight| (provider, inflight))
        })
    });
    let Some((provider, inflight)) = found else {
        return Vec::new();
    };
    with_runtime(|runtime| runtime.inflight[provider.index()] = None);
    let now = now_millis(api);
    let result = match outcome {
        Outcome::Ok => match HostHttp::take_response(api, request) {
            Some(response) => body_of(inflight.url, &response, now),
            None => Err(FetchFailure::transport(format!(
                "{}: the response was not available",
                inflight.url
            ))),
        },
        Outcome::Failed(error) | Outcome::Refused(error) => Err(FetchFailure::transport(format!(
            "{}: {}",
            inflight.url, error.message
        ))),
        _ => Err(FetchFailure::transport(format!(
            "{}: the request was cancelled",
            inflight.url
        ))),
    };
    step(api, provider, inflight.stage, result, now);
    drive(api)
}

/// The body and age of an answered request, or why it is not usable.
fn body_of(url: &str, response: &HttpResponse, now: i64) -> Result<FetchedBody, FetchFailure> {
    let header = |name: &str| {
        response
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
    };
    if response.status != 200 {
        let retry_after =
            header("retry-after").and_then(|value| parse_retry_after(value, datetime(now)));
        return Err(FetchFailure::http(url, response.status, retry_after));
    }
    let age = match header("age") {
        None => Duration::ZERO,
        Some(value) => {
            let seconds = value
                .trim()
                .parse::<u64>()
                .map_err(|_| FetchFailure::malformed(format!("{url}: invalid HTTP Age")))?;
            Duration::from_secs(seconds)
        }
    };
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(FetchFailure::malformed(format!(
            "{url}: reset feed exceeded the response size limit"
        )));
    }
    let body = String::from_utf8(response.body.clone())
        .map_err(|_| FetchFailure::malformed(format!("{url}: invalid UTF-8")))?;
    Ok(FetchedBody { body, age })
}

/// Feeds one response into the provider's read: next request, or the finished outcome.
fn step(
    api: &mut dyn HostApi,
    provider: ResetProvider,
    stage: Stage,
    result: Result<FetchedBody, FetchFailure>,
    now: i64,
) {
    match advance(provider, stage, datetime(now), result) {
        Step::Fetch(url, next_stage) => begin(api, provider, url, next_stage, now),
        Step::Done(outcome) => {
            if let Err(message) = &outcome.result {
                log(
                    api,
                    Level::Warn,
                    format!("{} reset feed check failed: {message}", provider.label()),
                );
            }
            if let Some(warning) = &outcome.source_warning {
                log(api, Level::Warn, warning.clone());
            }
            with_runtime(|runtime| {
                runtime.next_poll[provider.index()] =
                    Some(next_poll_millis(now, outcome.retry_after));
                let settings = runtime.settings.clone();
                runtime.monitor.apply(
                    MonitorEvent {
                        provider,
                        result: outcome.result,
                        source_warning: outcome.source_warning,
                    },
                    &settings,
                    now,
                );
            });
        }
    }
}

/// The segment submission when the label changed since the last one.
fn publish(now: i64) -> Option<Submission> {
    with_runtime(|runtime| {
        let text = status_text(&runtime.monitor, &runtime.settings, datetime(now));
        if text == runtime.shown {
            return None;
        }
        runtime.shown = text.clone();
        Some(match text {
            Some(text) => Submission::new(Intent::SetStatusSegment(SetStatusSegmentSpec {
                segment: STATUS_SEGMENT.to_string(),
                content: vec![RichSpan {
                    text,
                    style: Style::default(),
                    target: SpanTarget::None,
                    tooltip: None,
                }],
                tooltip: None,
                tone: None,
            })),
            None => Submission::new(Intent::ClearStatusSegment(STATUS_SEGMENT.to_string())),
        })
    })
}

/// Arms the one-shot timer for the earliest of the next read and the next label redraw.
fn rearm(api: &mut dyn HostApi, now: i64) {
    let (previous, wake) = with_runtime(|runtime| {
        let mut wake: Option<i64> = None;
        let mut consider = |at: i64| wake = Some(wake.map_or(at, |current| current.min(at)));
        for provider in ResetProvider::ALL {
            if !provider.enabled(&runtime.settings) || runtime.inflight[provider.index()].is_some()
            {
                continue;
            }
            consider(runtime.next_poll[provider.index()].unwrap_or(now));
        }
        if let Some(interval) = runtime
            .monitor
            .display_tick_interval(&runtime.settings, datetime(now))
        {
            consider(now.saturating_add(i64::try_from(interval.as_millis()).unwrap_or(i64::MAX)));
        }
        (runtime.timer.take(), wake)
    });
    if let Some(previous) = previous {
        HostTimers::cancel(api, previous);
    }
    let Some(wake) = wake else {
        return;
    };
    let delay = (wake - now).clamp(1, LONGEST_WAIT_MILLIS);
    let delay = u32::try_from(delay).unwrap_or(u32::MAX);
    match HostTimers::set(api, delay, None) {
        Ok(timer) => with_runtime(|runtime| runtime.timer = Some(timer)),
        Err(error) => log(
            api,
            Level::Warn,
            format!("reset planning timer unavailable: {}", error.message),
        ),
    }
}
