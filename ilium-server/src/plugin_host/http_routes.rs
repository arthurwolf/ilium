//! Plugin HTTP routes on the loopback HTTP API (G17, REQ API-005).
//!
//! A server plugin that holds the grant `http-api.route:<path>` and lists the same path in
//! `contributes.http_routes` serves that path at `/plugins/<plugin-id><path>`. The route table
//! is built once from the catalogue. A request that matches a route becomes one
//! `Event::HttpCalled` for the owner, and the owner answers with one `http-api` `respond`; the
//! `HttpCalls` table below completes the waiting HTTP request. Kernel routes never reach this
//! module: the HTTP API asks it only when no kernel route matched.
//!
//! The outcomes are: the plugin's own response; 404 for an unknown path; 405 with an `Allow`
//! header when the path exists for other methods only; 503 when the owner is disabled, has no
//! running code or failed while handling the event; 504 when it did not answer within
//! `RESPONSE_TIMEOUT` (capability-http-api-route.md "Limits and timing"). The checks that come
//! before routing (Host, Origin, bearer token, body size) belong to the HTTP API, not here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ilium_plugin_api::generated::types::net::Header;
use ilium_plugin_api::generated::types::{Event, HttpCalledEvent};
use ilium_plugin_host::imports::http_api::{HttpResponse, HttpResponseSink};
use ilium_plugin_host::runtime::{CataloguePlugin, PluginState};
use serde_json::Value;
use tokio::sync::oneshot;

use super::{log_errors, PluginHostHandle};

/// Largest request body a plugin route accepts: 1 MiB (api-http-api.md "Limits").
pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

/// How long an owner has to answer one request.
pub(crate) const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// The path prefix every plugin route lives under.
const ROUTE_PREFIX: &str = "/plugins/";

/// The capability argument family that registers a route.
const ROUTE_GRANT_PREFIX: &str = "http-api.route:";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every writer does one insert or remove, so the table stays consistent after a panic.
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// One segment of a route template.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    /// Matches exactly this text.
    Literal(String),
    /// A `{name}` segment: matches exactly one non-empty segment.
    Parameter,
}

/// One route of one plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Route {
    plugin: String,
    method: String,
    segments: Vec<Segment>,
}

impl Route {
    fn literal_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| matches!(segment, Segment::Literal(_)))
            .count()
    }

    fn matches(&self, path_segments: &[&str]) -> bool {
        self.segments.len() == path_segments.len()
            && self
                .segments
                .iter()
                .zip(path_segments)
                .all(|(segment, text)| match segment {
                    Segment::Literal(literal) => literal == text,
                    Segment::Parameter => !text.is_empty(),
                })
    }
}

/// Splits a path that starts with `/` into its segments; `/` has none.
fn path_segments(path: &str) -> Vec<&str> {
    match path.strip_prefix('/') {
        Some("") | None => Vec::new(),
        Some(rest) => rest.split('/').collect(),
    }
}

/// Parses a route template such as `/notes/{id}`. `None` for anything the manual does not
/// allow: no leading `/`, an empty segment, or a brace that is not a whole `{name}` segment.
fn parse_template(path: &str) -> Option<Vec<Segment>> {
    if !path.starts_with('/') {
        return None;
    }
    path_segments(path)
        .into_iter()
        .map(|text| {
            if text.is_empty() {
                return None;
            }
            match text.strip_prefix('{') {
                Some(rest) => {
                    let name = rest.strip_suffix('}')?;
                    (!name.is_empty() && !name.contains(['{', '}'])).then_some(Segment::Parameter)
                }
                None => (!text.contains(['{', '}'])).then(|| Segment::Literal(text.to_owned())),
            }
        })
        .collect()
}

/// A method is an upper-case token such as `GET` or `POST`.
fn valid_method(method: &str) -> bool {
    !method.is_empty() && method.bytes().all(|byte| byte.is_ascii_uppercase())
}

/// What the table makes of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// A route matches: the owner and the plugin-relative path with the query kept.
    Found { plugin: String, path: String },
    /// The path exists for other methods only; they are listed sorted.
    MethodNotAllowed(Vec<String>),
    /// No route.
    NotFound,
}

/// Every route the catalogue's plugins registered.
#[derive(Debug, Default)]
pub(crate) struct RouteTable {
    routes: Vec<Route>,
}

impl RouteTable {
    /// The routes of a catalogue, and one line for every entry that was refused. A route needs
    /// its `http-api.route:<path>` grant in `capabilities.server` (manifest.route-grant); an
    /// entry without one is not registered.
    pub(crate) fn from_catalogue(catalogue: &[CataloguePlugin]) -> (Self, Vec<String>) {
        let mut table = Self::default();
        let mut problems = Vec::new();
        for plugin in catalogue {
            let Ok(manifest) = serde_json::from_str::<Value>(&plugin.manifest) else {
                continue;
            };
            let granted: Vec<&str> = manifest
                .pointer("/capabilities/server")
                .and_then(Value::as_array)
                .map(|grants| {
                    grants
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(|grant| grant.strip_prefix(ROUTE_GRANT_PREFIX))
                        .collect()
                })
                .unwrap_or_default();
            let entries = manifest
                .pointer("/contributes/http_routes")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            for entry in entries {
                let method = entry.get("method").and_then(Value::as_str).unwrap_or("");
                let path = entry.get("path").and_then(Value::as_str).unwrap_or("");
                let Some(segments) = parse_template(path).filter(|_| valid_method(method)) else {
                    problems.push(format!(
                        "{}: route {method} {path} is not a valid method and path",
                        plugin.id
                    ));
                    continue;
                };
                if !granted.contains(&path) {
                    problems.push(format!(
                        "{}: route {method} {path} has no http-api.route grant",
                        plugin.id
                    ));
                    continue;
                }
                let route = Route {
                    plugin: plugin.id.clone(),
                    method: method.to_owned(),
                    segments,
                };
                if !table.routes.contains(&route) {
                    table.routes.push(route);
                }
            }
        }
        (table, problems)
    }

    /// Number of registered routes.
    pub(crate) fn len(&self) -> usize {
        self.routes.len()
    }

    /// Resolves a request. `target` is the request path with its query, as received.
    pub(crate) fn resolve(&self, method: &str, target: &str) -> Resolution {
        let (path, query) = match target.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (target, None),
        };
        let Some(rest) = path.strip_prefix(ROUTE_PREFIX) else {
            return Resolution::NotFound;
        };
        let (plugin, relative) = match rest.find('/') {
            Some(index) => rest.split_at(index),
            None => (rest, "/"),
        };
        let segments = path_segments(relative);
        let by_path: Vec<&Route> = self
            .routes
            .iter()
            .filter(|route| route.plugin == plugin && route.matches(&segments))
            .collect();
        if by_path.is_empty() {
            return Resolution::NotFound;
        }
        // The most specific route wins; ties keep manifest order.
        let best = by_path.iter().filter(|route| route.method == method).fold(
            None::<&&Route>,
            |best, route| match best {
                Some(held) if held.literal_count() >= route.literal_count() => Some(held),
                _ => Some(route),
            },
        );
        match best {
            Some(route) => Resolution::Found {
                plugin: route.plugin.clone(),
                path: match query {
                    Some(query) => format!("{relative}?{query}"),
                    None => relative.to_owned(),
                },
            },
            None => {
                let mut allowed: Vec<String> =
                    by_path.iter().map(|route| route.method.clone()).collect();
                allowed.sort();
                allowed.dedup();
                Resolution::MethodNotAllowed(allowed)
            }
        }
    }
}

/// The HTTP requests that wait for a plugin's `respond`, keyed by call id.
#[derive(Default)]
pub(crate) struct HttpCalls {
    next: AtomicU64,
    open: Mutex<HashMap<u64, OpenCall>>,
}

struct OpenCall {
    plugin: String,
    reply: oneshot::Sender<HttpResponse>,
}

/// Closes its call when dropped: a timeout, a client that hung up or a finished request all
/// leave the call unanswerable (`respond` then gets `not-found`).
struct OpenGuard {
    calls: Arc<HttpCalls>,
    call: u64,
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        lock(&self.calls.open).remove(&self.call);
    }
}

impl HttpCalls {
    /// Opens a call for `plugin`; the receiver yields the plugin's response.
    fn open(self: &Arc<Self>, plugin: &str) -> (u64, oneshot::Receiver<HttpResponse>, OpenGuard) {
        let call = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (reply, receiver) = oneshot::channel();
        lock(&self.open).insert(
            call,
            OpenCall {
                plugin: plugin.to_owned(),
                reply,
            },
        );
        (
            call,
            receiver,
            OpenGuard {
                calls: Arc::clone(self),
                call,
            },
        )
    }

    /// Number of requests currently waiting for an answer.
    #[cfg(test)]
    pub(crate) fn open_count(&self) -> usize {
        lock(&self.open).len()
    }
}

impl HttpResponseSink for HttpCalls {
    fn complete(&self, plugin: &str, call: u64, response: HttpResponse) -> bool {
        let mut open = lock(&self.open);
        // Another plugin's call is left alone: a plugin cannot answer a request it was not given.
        if open.get(&call).is_none_or(|entry| entry.plugin != plugin) {
            return false;
        }
        match open.remove(&call) {
            Some(entry) => entry.reply.send(response).is_ok(),
            None => false,
        }
    }
}

/// What delivering the event to its owner came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// The owner handled `http-called`.
    Delivered,
    /// The owner is disabled, has no code on this side or is not loaded.
    Unavailable,
    /// The owner could not take the event; the text says why.
    Failed(String),
}

/// How an `http-called` event reaches its owner. The server implements it over the runtime
/// thread; tests implement it with a fake.
pub(crate) trait RouteHost: Send + Sync {
    /// Delivers `event` to `plugin` and reports the outcome through `done`. Dropping `done`
    /// unanswered means the host stopped.
    fn deliver(&self, plugin: &str, event: HttpCalledEvent, done: oneshot::Sender<Delivery>);
}

impl RouteHost for PluginHostHandle {
    fn deliver(&self, plugin: &str, event: HttpCalledEvent, done: oneshot::Sender<Delivery>) {
        let plugin = plugin.to_owned();
        // A refused post drops the closure and with it `done`.
        let _posted = self.post(move |runtime| {
            let ready = match runtime.state(&plugin) {
                Some(PluginState::Active) => Ok(()),
                Some(PluginState::Loaded) => runtime.activate(&plugin).map_err(|error| {
                    Delivery::Failed(format!("{plugin} did not activate: {error}"))
                }),
                _ => Err(Delivery::Unavailable),
            };
            let outcome = match ready {
                Ok(()) => match runtime.deliver(&plugin, vec![Event::HttpCalled(event)]) {
                    Ok(()) => Delivery::Delivered,
                    Err(error) => Delivery::Failed(format!("{plugin} failed: {error}")),
                },
                Err(outcome) => outcome,
            };
            log_errors("http route pump", runtime.pump());
            // The waiting request may have given up already.
            let _ = done.send(outcome);
        });
    }
}

/// One request for a plugin route, in plain types.
#[derive(Debug, Clone)]
pub(crate) struct PluginRequest {
    pub(crate) method: String,
    /// The request path with its query, as received.
    pub(crate) target: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

/// What the HTTP API answers for a plugin route request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reply {
    /// The owner's response.
    Response(HttpResponse),
    /// 404: no such route.
    NotFound,
    /// 405: the path exists for the listed methods only.
    MethodNotAllowed(Vec<String>),
    /// 503: the owner is disabled, has no running code, failed, or the host stopped.
    Unavailable(String),
    /// 504: the owner did not answer in time.
    Timeout,
}

/// The route table, the open calls and the answer deadline.
pub(crate) struct HttpRoutes {
    table: RouteTable,
    calls: Arc<HttpCalls>,
    timeout: Duration,
}

impl HttpRoutes {
    /// The routes of `catalogue`, answering through `calls`. Refused entries are logged.
    pub(crate) fn new(catalogue: &[CataloguePlugin], calls: Arc<HttpCalls>) -> Self {
        Self::with_timeout(catalogue, calls, RESPONSE_TIMEOUT)
    }

    /// Like `new` with another answer deadline.
    pub(crate) fn with_timeout(
        catalogue: &[CataloguePlugin],
        calls: Arc<HttpCalls>,
        timeout: Duration,
    ) -> Self {
        let (table, problems) = RouteTable::from_catalogue(catalogue);
        for problem in problems {
            tracing::warn!("plugin http route not registered: {problem}");
        }
        Self {
            table,
            calls,
            timeout,
        }
    }

    /// True when at least one plugin registered a route.
    pub(crate) fn has_routes(&self) -> bool {
        self.table.len() > 0
    }

    /// The answer for a request that matches no route, or `None` when a route matches. The HTTP
    /// API asks this before it reads the body, so an unknown path never costs a 1 MiB read.
    pub(crate) fn precheck(&self, method: &str, target: &str) -> Option<Reply> {
        match self.table.resolve(method, target) {
            Resolution::Found { .. } => None,
            Resolution::MethodNotAllowed(allowed) => Some(Reply::MethodNotAllowed(allowed)),
            Resolution::NotFound => Some(Reply::NotFound),
        }
    }

    /// Resolves, delivers and waits. Never blocks a runtime thread: the wait is on channels.
    pub(crate) async fn dispatch(&self, host: &dyn RouteHost, request: PluginRequest) -> Reply {
        let (plugin, path) = match self.table.resolve(&request.method, &request.target) {
            Resolution::Found { plugin, path } => (plugin, path),
            Resolution::MethodNotAllowed(allowed) => return Reply::MethodNotAllowed(allowed),
            Resolution::NotFound => return Reply::NotFound,
        };
        let (call, response, _guard) = self.calls.open(&plugin);
        let event = HttpCalledEvent {
            call,
            method: request.method,
            path,
            headers: request
                .headers
                .into_iter()
                .map(|(name, value)| Header { name, value })
                .collect(),
            body: request.body,
        };
        let (done, delivered) = oneshot::channel();
        host.deliver(&plugin, event, done);
        let waiting = async {
            match delivered.await {
                Ok(Delivery::Delivered) => {}
                Ok(Delivery::Unavailable) => {
                    return Reply::Unavailable("the plugin is not running".to_owned())
                }
                Ok(Delivery::Failed(message)) => return Reply::Unavailable(message),
                Err(_) => return Reply::Unavailable("the plugin host stopped".to_owned()),
            }
            match response.await {
                Ok(response) => Reply::Response(response),
                Err(_) => Reply::Unavailable("the plugin stopped before it answered".to_owned()),
            }
        };
        tokio::time::timeout(self.timeout, waiting)
            .await
            .unwrap_or(Reply::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(id: &str, routes: &[(&str, &str)], grants: &[&str]) -> CataloguePlugin {
        let entries: Vec<Value> = routes
            .iter()
            .map(|(method, path)| serde_json::json!({ "method": method, "path": path }))
            .collect();
        let grants: Vec<String> = grants
            .iter()
            .map(|path| format!("{ROUTE_GRANT_PREFIX}{path}"))
            .collect();
        CataloguePlugin {
            id: id.to_owned(),
            manifest: serde_json::json!({
                "id": id,
                "capabilities": { "server": grants },
                "contributes": { "http_routes": entries },
            })
            .to_string(),
            component: None,
        }
    }

    fn table(catalogue: &[CataloguePlugin]) -> RouteTable {
        RouteTable::from_catalogue(catalogue).0
    }

    fn found(plugin: &str, path: &str) -> Resolution {
        Resolution::Found {
            plugin: plugin.to_owned(),
            path: path.to_owned(),
        }
    }

    /// REQ: API-005
    #[test]
    fn a_literal_route_is_served_under_the_plugin_prefix() {
        let table = table(&[plugin(
            "example.notes",
            &[("POST", "/notes/new")],
            &["/notes/new"],
        )]);
        assert_eq!(
            table.resolve("POST", "/plugins/example.notes/notes/new"),
            found("example.notes", "/notes/new")
        );
        assert_eq!(
            table.resolve("POST", "/notes/new"),
            Resolution::NotFound,
            "the bare path belongs to the kernel"
        );
        assert_eq!(
            table.resolve("POST", "/plugins/example.other/notes/new"),
            Resolution::NotFound,
            "another plugin's prefix does not reach it"
        );
    }

    /// REQ: API-005
    #[test]
    fn the_method_and_the_path_match_together() {
        let table = table(&[plugin("example.notes", &[("GET", "/notes")], &["/notes"])]);
        assert_eq!(
            table.resolve("POST", "/plugins/example.notes/notes"),
            Resolution::MethodNotAllowed(vec!["GET".to_owned()])
        );
        assert_eq!(
            table.resolve("GET", "/plugins/example.notes/other"),
            Resolution::NotFound
        );
    }

    /// REQ: API-005
    #[test]
    fn a_parameter_segment_matches_one_non_empty_segment() {
        let table = table(&[plugin(
            "example.notes",
            &[("GET", "/notes/{id}")],
            &["/notes/{id}"],
        )]);
        let get = |path: &str| table.resolve("GET", &format!("/plugins/example.notes{path}"));
        assert_eq!(get("/notes/42"), found("example.notes", "/notes/42"));
        assert_eq!(get("/notes/"), Resolution::NotFound, "empty segment");
        assert_eq!(get("/notes"), Resolution::NotFound, "missing segment");
        assert_eq!(get("/notes/4/2"), Resolution::NotFound, "two segments");
    }

    /// REQ: API-005
    #[test]
    fn the_query_string_stays_on_the_forwarded_path() {
        let table = table(&[plugin(
            "example.notes",
            &[("GET", "/notes/{id}")],
            &["/notes/{id}"],
        )]);
        assert_eq!(
            table.resolve("GET", "/plugins/example.notes/notes/42?full=1&x=2"),
            found("example.notes", "/notes/42?full=1&x=2")
        );
    }

    /// REQ: API-005
    #[test]
    fn a_literal_route_beats_a_parameter_route() {
        let table = table(&[
            plugin(
                "example.notes",
                &[("GET", "/notes/{id}"), ("GET", "/notes/new")],
                &["/notes/{id}", "/notes/new"],
            ),
            plugin("example.other", &[("GET", "/notes/new")], &["/notes/new"]),
        ]);
        let target = "/plugins/example.notes/notes/new";
        assert_eq!(
            table.resolve("GET", target),
            found("example.notes", "/notes/new")
        );
        // Both routes would match, so the answer cannot depend on the other plugin.
        assert_eq!(
            table.resolve("GET", "/plugins/example.other/notes/new"),
            found("example.other", "/notes/new")
        );
    }

    /// REQ: API-005
    #[test]
    fn a_route_without_its_grant_is_not_registered() {
        let (table, problems) = RouteTable::from_catalogue(&[plugin(
            "example.notes",
            &[("GET", "/granted"), ("GET", "/ungranted")],
            &["/granted"],
        )]);
        assert_eq!(table.len(), 1);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("/ungranted"), "{problems:?}");
        assert_eq!(
            table.resolve("GET", "/plugins/example.notes/ungranted"),
            Resolution::NotFound
        );
    }

    /// REQ: API-005
    #[test]
    fn malformed_routes_are_refused_with_a_reason() {
        let (table, problems) = RouteTable::from_catalogue(&[plugin(
            "example.notes",
            &[
                ("get", "/lower"),
                ("GET", "no-slash"),
                ("GET", "/a//b"),
                ("GET", "/half{id}"),
                ("GET", "/empty/{}"),
            ],
            &["/lower", "no-slash", "/a//b", "/half{id}", "/empty/{}"],
        )]);
        assert_eq!(table.len(), 0);
        assert_eq!(problems.len(), 5, "{problems:?}");
    }

    /// REQ: API-005
    #[test]
    fn the_bundled_fleet_route_is_registered_under_its_prefix() {
        let manifest = include_str!("../../../plugins/core/fleet/manifest.json");
        let fleet = CataloguePlugin {
            id: "ilium.core.fleet".to_owned(),
            manifest: manifest.to_owned(),
            component: None,
        };
        let (table, problems) = RouteTable::from_catalogue(&[fleet]);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            table.resolve("POST", "/plugins/ilium.core.fleet/create_agent"),
            found("ilium.core.fleet", "/create_agent")
        );
        assert_eq!(
            table.resolve("POST", "/create_agent"),
            Resolution::NotFound,
            "the kernel keeps the bare path"
        );
    }

    /// A host that answers every event through the sink, as a plugin calling `respond` would.
    struct AnsweringHost {
        calls: Arc<HttpCalls>,
        status: u16,
        seen: Mutex<Vec<HttpCalledEvent>>,
    }

    impl RouteHost for AnsweringHost {
        fn deliver(&self, plugin: &str, event: HttpCalledEvent, done: oneshot::Sender<Delivery>) {
            let call = event.call;
            lock(&self.seen).push(event);
            let answered = self.calls.complete(
                plugin,
                call,
                HttpResponse {
                    status: self.status,
                    headers: vec![("content-type".to_owned(), "text/plain".to_owned())],
                    body: b"answer".to_vec(),
                },
            );
            assert!(answered, "the open call accepts the first response");
            let _ = done.send(Delivery::Delivered);
        }
    }

    /// A host with a fixed delivery outcome that never responds.
    struct SilentHost(Delivery);

    impl RouteHost for SilentHost {
        fn deliver(&self, _plugin: &str, _event: HttpCalledEvent, done: oneshot::Sender<Delivery>) {
            let _ = done.send(self.0.clone());
        }
    }

    fn routes(timeout: Duration) -> (HttpRoutes, Arc<HttpCalls>) {
        let calls = Arc::new(HttpCalls::default());
        let routes = HttpRoutes::with_timeout(
            &[plugin("example.notes", &[("POST", "/new")], &["/new"])],
            Arc::clone(&calls),
            timeout,
        );
        (routes, calls)
    }

    fn request(target: &str) -> PluginRequest {
        PluginRequest {
            method: "POST".to_owned(),
            target: target.to_owned(),
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            body: b"{}".to_vec(),
        }
    }

    /// REQ: API-005
    #[tokio::test]
    async fn a_request_becomes_an_event_and_the_response_completes_it() {
        let (routes, calls) = routes(Duration::from_secs(5));
        let host = AnsweringHost {
            calls: Arc::clone(&calls),
            status: 201,
            seen: Mutex::new(Vec::new()),
        };
        let reply = routes
            .dispatch(&host, request("/plugins/example.notes/new?draft=1"))
            .await;
        let Reply::Response(response) = reply else {
            panic!("expected the plugin's response, got {reply:?}");
        };
        assert_eq!(response.status, 201);
        assert_eq!(response.body, b"answer".to_vec());
        let seen = lock(&host.seen);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].path, "/new?draft=1");
        assert_eq!(seen[0].body, b"{}".to_vec());
        assert_eq!(seen[0].headers[0].name, "content-type");
        assert_eq!(calls.open_count(), 0, "the call is closed after the answer");
    }

    /// REQ: API-005
    #[tokio::test]
    async fn no_answer_in_time_is_a_504_and_the_call_closes() {
        let (routes, calls) = routes(Duration::from_millis(60));
        let reply = routes
            .dispatch(
                &SilentHost(Delivery::Delivered),
                request("/plugins/example.notes/new"),
            )
            .await;
        assert_eq!(reply, Reply::Timeout);
        assert_eq!(calls.open_count(), 0);
        // A late `respond` finds nothing to complete.
        let late = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert!(!calls.complete("example.notes", 1, late));
    }

    /// REQ: API-005
    #[tokio::test]
    async fn a_disabled_owner_is_a_503() {
        let (routes, calls) = routes(Duration::from_secs(5));
        let reply = routes
            .dispatch(
                &SilentHost(Delivery::Unavailable),
                request("/plugins/example.notes/new"),
            )
            .await;
        assert!(matches!(reply, Reply::Unavailable(_)), "{reply:?}");
        assert_eq!(calls.open_count(), 0);
    }

    /// REQ: API-005
    #[tokio::test]
    async fn a_failing_owner_is_a_503() {
        let (routes, _calls) = routes(Duration::from_secs(5));
        let reply = routes
            .dispatch(
                &SilentHost(Delivery::Failed("trapped".to_owned())),
                request("/plugins/example.notes/new"),
            )
            .await;
        assert_eq!(reply, Reply::Unavailable("trapped".to_owned()));
    }

    /// REQ: API-005
    #[tokio::test]
    async fn a_stopped_host_is_a_503() {
        struct StoppedHost;
        impl RouteHost for StoppedHost {
            fn deliver(&self, _: &str, _: HttpCalledEvent, done: oneshot::Sender<Delivery>) {
                drop(done);
            }
        }
        let (routes, _calls) = routes(Duration::from_secs(5));
        let reply = routes
            .dispatch(&StoppedHost, request("/plugins/example.notes/new"))
            .await;
        assert!(matches!(reply, Reply::Unavailable(_)), "{reply:?}");
    }

    /// REQ: API-005
    #[tokio::test]
    async fn unknown_paths_and_methods_never_reach_the_host() {
        struct PanickingHost;
        impl RouteHost for PanickingHost {
            fn deliver(&self, _: &str, _: HttpCalledEvent, _: oneshot::Sender<Delivery>) {
                panic!("the host must not be asked");
            }
        }
        let (routes, calls) = routes(Duration::from_secs(5));
        assert_eq!(
            routes
                .dispatch(&PanickingHost, request("/plugins/example.notes/other"))
                .await,
            Reply::NotFound
        );
        let mut get = request("/plugins/example.notes/new");
        get.method = "GET".to_owned();
        assert_eq!(
            routes.dispatch(&PanickingHost, get).await,
            Reply::MethodNotAllowed(vec!["POST".to_owned()])
        );
        assert_eq!(calls.open_count(), 0);
    }

    /// REQ: API-005
    #[test]
    fn the_precheck_answers_only_for_requests_without_a_route() {
        let (routes, _calls) = routes(Duration::from_secs(5));
        assert_eq!(routes.precheck("POST", "/plugins/example.notes/new"), None);
        assert_eq!(
            routes.precheck("POST", "/plugins/example.notes/gone"),
            Some(Reply::NotFound)
        );
        assert_eq!(
            routes.precheck("PUT", "/plugins/example.notes/new"),
            Some(Reply::MethodNotAllowed(vec!["POST".to_owned()]))
        );
    }

    /// REQ: API-005
    #[test]
    fn a_plugin_cannot_answer_another_plugins_call() {
        let calls = Arc::new(HttpCalls::default());
        let (call, mut receiver, _guard) = calls.open("example.notes");
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert!(!calls.complete("example.other", call, response.clone()));
        assert!(
            receiver.try_recv().is_err(),
            "the owner's call is still open"
        );
        assert!(calls.complete("example.notes", call, response.clone()));
        assert_eq!(receiver.try_recv().ok(), Some(response.clone()));
        assert!(
            !calls.complete("example.notes", call, response),
            "a call is answered once"
        );
    }

    /// REQ: API-005
    #[test]
    fn call_ids_are_never_reused() {
        let calls = Arc::new(HttpCalls::default());
        let (first, _receiver_one, _guard_one) = calls.open("example.notes");
        let (second, _receiver_two, _guard_two) = calls.open("example.notes");
        assert_ne!(first, second);
        assert_eq!(calls.open_count(), 2);
    }
}
