//! Polling of one public source from inside a scene.
//!
//! A scene runs in a render frame and owns no thread, so a [`Poller`] is a small state machine
//! the scene advances once per frame: it issues the source's requests over the host http import,
//! collects the finished bodies without waiting, and publishes the decoded result. A failed or
//! malformed answer keeps the last good data and only records the error.
use super::model::FeedState;
use ilium_plugin_sdk::host::HttpRequest;
use ilium_plugin_sdk::types::RequestId;
use std::sync::Arc;
use std::time::Duration;

/// Longest a request may run before the host reports a timeout.
const REQUEST_TIMEOUT_MILLIS: u32 = 15_000;

/// One finished answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Body(Vec<u8>),
    /// A non-success HTTP status.
    Status(u16),
    Failed(String),
}

/// The transport under a [`Poller`].
pub trait Transport: Send {
    fn start(&mut self, url: &str) -> Result<u64, String>;
    /// The finished answer of request `id`, or `None` while it is still running.
    fn take(&mut self, id: u64) -> Option<Answer>;
}

/// The host's http import.
#[derive(Default)]
pub struct HostTransport;

impl Transport for HostTransport {
    fn start(&mut self, url: &str) -> Result<u64, String> {
        ilium_plugin_sdk::host()
            .fetch(HttpRequest {
                method: "GET".into(),
                url: url.into(),
                headers: vec![ilium_plugin_sdk::types::net::Header {
                    name: "Accept".into(),
                    value: "application/json".into(),
                }],
                body: None,
                timeout_millis: REQUEST_TIMEOUT_MILLIS,
            })
            .map(|RequestId(id)| id)
            .map_err(|error| error.message)
    }

    fn take(&mut self, id: u64) -> Option<Answer> {
        let response = ilium_plugin_sdk::host().take_response(RequestId(id))?;
        Some(if (200..300).contains(&response.status) {
            Answer::Body(response.body)
        } else {
            Answer::Status(response.status)
        })
    }
}

/// One request of a plan and the longest body it may return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub limit: usize,
}

#[derive(Debug)]
pub struct Snapshot<T> {
    pub data: Option<Arc<T>>,
    pub state: FeedState,
}

impl<T> Default for Snapshot<T> {
    fn default() -> Self {
        Self {
            data: None,
            state: FeedState::default(),
        }
    }
}

impl<T> Clone for Snapshot<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            state: self.state.clone(),
        }
    }
}

impl<T> Snapshot<T> {
    fn apply(&mut self, received_ms: i64, result: Result<(T, Option<i64>), String>) {
        match result {
            Ok((data, observed_ms)) => {
                self.data = Some(Arc::new(data));
                self.state.received(received_ms, observed_ms);
            }
            Err(error) => self.state.failed(error),
        }
    }
}

fn retry_delay(interval: Duration, failures: u32) -> Duration {
    interval
        .saturating_mul(1_u32 << failures.min(8))
        .min(Duration::from_secs(900).max(interval))
}

/// Builds the requests of one refresh at the given wall clock (ms since the epoch), or says
/// why none may be made now (a shared request floor, an invalid identity).
pub type Plan = Box<dyn FnMut(i64) -> Result<Vec<Request>, String> + Send>;
/// Decodes the bodies of one refresh, in request order, into the data and the observation time.
pub type Decode<T> = Box<dyn FnMut(&[Vec<u8>], i64) -> Result<(T, Option<i64>), String> + Send>;

enum Stage {
    Idle,
    Waiting {
        pending: Vec<(u64, usize)>,
        bodies: Vec<Option<Vec<u8>>>,
    },
}

/// One owned source request loop. Closures must bound network and decode work;
/// dropping the poller requests stop and transfers the bounded join to the
/// existing ambient reaper so the presentation thread cannot stall on HTTP.
pub struct Poller<T> {
    transport: Box<dyn Transport>,
    plan: Plan,
    decode: Decode<T>,
    interval: Duration,
    stage: Stage,
    limits: Vec<usize>,
    next_at_ms: i64,
    failures: u32,
    snapshot: Arc<Snapshot<T>>,
}

impl<T: Send + Sync + 'static> Poller<T> {
    /// A poller over the host's http import. The first refresh happens on the first `poll`;
    /// later ones every `requested` (never below `minimum`, and backed off after failures).
    pub fn new(requested: Duration, minimum: Duration, plan: Plan, decode: Decode<T>) -> Self {
        Self::with_transport(Box::new(HostTransport), requested, minimum, plan, decode)
    }

    pub fn with_transport(
        transport: Box<dyn Transport>,
        requested: Duration,
        minimum: Duration,
        plan: Plan,
        decode: Decode<T>,
    ) -> Self {
        let interval = requested
            .min(Duration::from_secs(86400))
            .max(minimum)
            .max(Duration::from_secs(1));
        Self {
            transport,
            plan,
            decode,
            interval,
            stage: Stage::Idle,
            limits: Vec::new(),
            next_at_ms: 0,
            failures: 0,
            snapshot: Arc::new(Snapshot::default()),
        }
    }

    /// Changes the refresh interval from the next refresh on; nothing is restarted.
    pub fn set_interval(&mut self, requested: Duration, minimum: Duration) {
        self.interval = requested
            .min(Duration::from_secs(86400))
            .max(minimum)
            .max(Duration::from_secs(1));
    }

    /// The latest published snapshot; the same `Arc` until something new is published.
    pub fn snapshot(&self) -> Arc<Snapshot<T>> {
        Arc::clone(&self.snapshot)
    }

    fn fail(&mut self, now_ms: i64, error: String) {
        let mut next = (*self.snapshot).clone();
        next.state.failed(error);
        self.snapshot = Arc::new(next);
        self.stage = Stage::Idle;
        self.failures = self.failures.saturating_add(1);
        self.schedule(now_ms);
    }

    fn schedule(&mut self, now_ms: i64) {
        let delay = retry_delay(self.interval, self.failures);
        self.next_at_ms = now_ms.saturating_add(delay.as_millis().min(i64::MAX as u128) as i64);
    }

    /// Advances the refresh to the wall clock `now_ms`; never waits.
    pub fn poll(&mut self, now_ms: i64) {
        if matches!(self.stage, Stage::Idle) {
            if now_ms < self.next_at_ms {
                return;
            }
            let requests = match (self.plan)(now_ms) {
                Ok(requests) if !requests.is_empty() => requests,
                Ok(_) => return self.fail(now_ms, "source plan has no request".into()),
                Err(error) => return self.fail(now_ms, error),
            };
            let mut pending = Vec::with_capacity(requests.len());
            for (index, request) in requests.iter().enumerate() {
                match self.transport.start(&request.url) {
                    Ok(id) => pending.push((id, index)),
                    Err(error) => return self.fail(now_ms, error),
                }
            }
            self.limits = requests.iter().map(|request| request.limit).collect();
            self.stage = Stage::Waiting {
                bodies: vec![None; pending.len()],
                pending,
            };
        }
        let Stage::Waiting { pending, bodies } = &mut self.stage else {
            return;
        };
        let mut failure = None;
        pending.retain(|(id, index)| match self.transport.take(*id) {
            None => true,
            Some(Answer::Body(body)) => {
                if body.len() > self.limits[*index] {
                    failure = Some(format!(
                        "source answer exceeds {} bytes",
                        self.limits[*index]
                    ));
                } else {
                    bodies[*index] = Some(body);
                }
                false
            }
            Some(Answer::Status(status)) => {
                failure = Some(format!("source answered HTTP {status}"));
                false
            }
            Some(Answer::Failed(error)) => {
                failure = Some(error);
                false
            }
        });
        if let Some(error) = failure {
            return self.fail(now_ms, error);
        }
        if !pending.is_empty() {
            return;
        }
        let bodies: Vec<Vec<u8>> = bodies
            .iter_mut()
            .map(|body| body.take().unwrap_or_default())
            .collect();
        self.stage = Stage::Idle;
        match (self.decode)(&bodies, now_ms) {
            Ok(result) => {
                let mut next = (*self.snapshot).clone();
                next.apply(now_ms, Ok(result));
                self.snapshot = Arc::new(next);
                self.failures = 0;
                self.schedule(now_ms);
            }
            Err(error) => self.fail(now_ms, error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Script {
        started: Vec<String>,
        answers: HashMap<u64, Answer>,
    }

    /// A transport whose answers the test supplies; an id has no answer until it is given.
    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Script>>);

    impl Fake {
        fn answer(&self, id: u64, answer: Answer) {
            self.0.lock().unwrap().answers.insert(id, answer);
        }
        fn started(&self) -> Vec<String> {
            self.0.lock().unwrap().started.clone()
        }
    }

    impl Transport for Fake {
        fn start(&mut self, url: &str) -> Result<u64, String> {
            let mut script = self.0.lock().unwrap();
            script.started.push(url.to_owned());
            Ok(script.started.len() as u64)
        }
        fn take(&mut self, id: u64) -> Option<Answer> {
            self.0.lock().unwrap().answers.remove(&id)
        }
    }

    fn counting_poller(fake: &Fake, urls: &[&str]) -> Poller<usize> {
        let urls: Vec<String> = urls.iter().map(|url| (*url).to_owned()).collect();
        Poller::with_transport(
            Box::new(fake.clone()),
            Duration::from_secs(60),
            Duration::from_secs(1),
            Box::new(move |_| {
                Ok(urls
                    .iter()
                    .map(|url| Request {
                        url: url.clone(),
                        limit: 16,
                    })
                    .collect())
            }),
            Box::new(|bodies, _| {
                let total: usize = bodies.iter().map(Vec::len).sum();
                if total == 0 {
                    Err("empty".into())
                } else {
                    Ok((total, Some(7)))
                }
            }),
        )
    }

    #[test]
    fn a_plan_of_two_requests_publishes_once_both_bodies_arrived() {
        let fake = Fake::default();
        let mut poller = counting_poller(&fake, &["https://a.test/", "https://b.test/"]);
        poller.poll(1_000);
        assert_eq!(fake.started(), ["https://a.test/", "https://b.test/"]);
        fake.answer(1, Answer::Body(b"abc".to_vec()));
        poller.poll(1_100);
        assert!(poller.snapshot().data.is_none());
        fake.answer(2, Answer::Body(b"de".to_vec()));
        poller.poll(1_200);
        let snapshot = poller.snapshot();
        assert_eq!(snapshot.data.as_deref(), Some(&5));
        assert_eq!(snapshot.state.received_ms, Some(1_200));
        assert_eq!(snapshot.state.observed_ms, Some(7));
    }

    #[test]
    fn the_next_refresh_waits_for_the_interval() {
        let fake = Fake::default();
        let mut poller = counting_poller(&fake, &["https://a.test/"]);
        poller.poll(1_000);
        fake.answer(1, Answer::Body(b"x".to_vec()));
        poller.poll(1_001);
        poller.poll(30_000);
        assert_eq!(fake.started().len(), 1);
        poller.poll(61_001);
        assert_eq!(fake.started().len(), 2);
    }

    #[test]
    fn status_oversize_and_decode_failures_keep_the_last_good_data_and_back_off() {
        let fake = Fake::default();
        let mut poller = counting_poller(&fake, &["https://a.test/"]);
        poller.poll(0);
        fake.answer(1, Answer::Body(b"xy".to_vec()));
        poller.poll(1);
        let good = poller.snapshot();
        assert_eq!(good.data.as_deref(), Some(&2));

        poller.poll(60_001);
        fake.answer(2, Answer::Status(503));
        poller.poll(60_002);
        let failed = poller.snapshot();
        assert_eq!(failed.data.as_deref(), Some(&2));
        assert_eq!(failed.state.received_ms, Some(1));
        assert!(failed.state.error.as_deref().unwrap().contains("503"));
        // One failure doubles the delay: nothing is requested after the plain interval.
        poller.poll(120_002);
        assert_eq!(fake.started().len(), 2);
        poller.poll(180_003);
        assert_eq!(fake.started().len(), 3);
        fake.answer(3, Answer::Body(vec![b'x'; 17]));
        poller.poll(180_004);
        assert!(poller
            .snapshot()
            .state
            .error
            .as_deref()
            .unwrap()
            .contains("exceeds"));
        assert_eq!(poller.snapshot().data.as_deref(), Some(&2));
    }

    #[test]
    fn a_refused_plan_is_an_error_and_makes_no_request() {
        let fake = Fake::default();
        let mut poller: Poller<usize> = Poller::with_transport(
            Box::new(fake.clone()),
            Duration::from_secs(5),
            Duration::from_secs(5),
            Box::new(|_| Err("shared source request limit: wait 12 seconds".into())),
            Box::new(|_, _| Ok((0, None))),
        );
        poller.poll(0);
        assert!(fake.started().is_empty());
        assert!(poller
            .snapshot()
            .state
            .error
            .as_deref()
            .unwrap()
            .contains("shared source request limit"));
    }

    #[test]
    fn failed_request_keeps_last_good_data_and_observation_timestamp() {
        let mut snapshot = Snapshot::<u32>::default();
        snapshot.apply(1000, Ok((42, Some(500))));
        snapshot.apply(2000, Err("offline".into()));
        assert_eq!(snapshot.data.as_deref(), Some(&42));
        assert_eq!(snapshot.state.observed_ms, Some(500));
        assert_eq!(snapshot.state.received_ms, Some(1000));
        assert_eq!(snapshot.state.error.as_deref(), Some("offline"));
        snapshot.apply(3000, Ok((42, Some(500))));
        assert_eq!(snapshot.state.observed_ms, Some(500));
        assert!(snapshot.state.error.is_none());
    }

    #[test]
    fn malformed_provider_refresh_preserves_last_good_but_valid_empty_replaces_it() {
        use super::super::{model::Earthquake, parse};

        let first = br#"{"type":"FeatureCollection","features":[{"id":"fixture","properties":{"time":500,"mag":null},"geometry":{"type":"Point","coordinates":[0,0]}}]}"#;
        let mut snapshot = Snapshot::<Vec<Earthquake>>::default();
        snapshot.apply(
            1000,
            parse::usgs(first).map(|decoded| (decoded.items, Some(500))),
        );
        let previous = Arc::clone(snapshot.data.as_ref().unwrap());

        let malformed = br#"{"type":"FeatureCollection","features":[null,{}]}"#;
        snapshot.apply(
            2000,
            parse::usgs(malformed).map(|decoded| (decoded.items, None)),
        );
        assert!(Arc::ptr_eq(snapshot.data.as_ref().unwrap(), &previous));
        assert_eq!(snapshot.state.received_ms, Some(1000));
        assert_eq!(snapshot.state.observed_ms, Some(500));
        assert!(snapshot.state.error.is_some());

        let empty = br#"{"type":"FeatureCollection","features":[]}"#;
        snapshot.apply(
            3000,
            parse::usgs(empty).map(|decoded| (decoded.items, None)),
        );
        assert!(snapshot.data.as_ref().unwrap().is_empty());
        assert_eq!(snapshot.state.received_ms, Some(3000));
        assert_eq!(snapshot.state.observed_ms, None);
        assert!(snapshot.state.error.is_none());
    }
    #[test]
    fn retry_schedule_respects_source_floor_and_caps_exponential_backoff() {
        assert_eq!(
            retry_delay(Duration::from_secs(60), 0),
            Duration::from_secs(60)
        );
        assert_eq!(
            retry_delay(Duration::from_secs(60), 2),
            Duration::from_secs(240)
        );
        assert_eq!(
            retry_delay(Duration::from_secs(60), 32),
            Duration::from_secs(900)
        );
        assert_eq!(
            retry_delay(Duration::from_secs(1200), 32),
            Duration::from_secs(1200)
        );
    }
}
