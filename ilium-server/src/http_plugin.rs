//! The plugin side of the loopback HTTP API (G17, REQ API-005).
//!
//! Every request that no kernel route answers lands in `serve`. A path under `/plugins/` is
//! checked in the manual's order (capability-http-api-route.md "Authentication"): the `Host`
//! header, the `Origin` header, the bearer token, then the route and method, then the body size.
//! Only a request that passes all of them reaches the plugin as `http-called`. The token is
//! fresh at every server start and lives with the port in two owner-only files beside the
//! session socket.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::header::{self, HeaderMap, HeaderName, HeaderValue};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use ilium_platform::secure_fs;
use uuid::Uuid;

use crate::plugin_host::{PluginRequest, Reply, MAX_REQUEST_BODY_BYTES};
use crate::state::ServerState;

/// File names of the published credentials (capability-http-api-route.md "Authentication").
pub(crate) const TOKEN_FILE: &str = "http-api.token";
pub(crate) const PORT_FILE: &str = "http-api.port";

/// The path prefix that belongs to plugins.
const PLUGIN_PREFIX: &str = "/plugins/";

/// What a caller must present to reach a plugin route.
#[derive(Debug)]
pub(crate) struct RouteGuard {
    port: u16,
    token: String,
}

impl RouteGuard {
    /// A guard for the port the listener is bound to, with a fresh token.
    pub(crate) fn new(port: u16) -> Self {
        Self::with_token(port, fresh_token())
    }

    pub(crate) fn with_token(port: u16, token: String) -> Self {
        Self { port, token }
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub(crate) const fn port(&self) -> u16 {
        self.port
    }

    /// The first three checks of the manual's order. `Err` carries the refusing status.
    pub(crate) fn admit(&self, headers: &HeaderMap) -> Result<(), StatusCode> {
        if !self.host_is_loopback(headers) {
            return Err(StatusCode::FORBIDDEN);
        }
        if headers.contains_key(header::ORIGIN) {
            return Err(StatusCode::FORBIDDEN);
        }
        if !self.token_matches(headers) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        Ok(())
    }

    fn host_is_loopback(&self, headers: &HeaderMap) -> bool {
        let Some(host) = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
        else {
            return false;
        };
        let port = self.port;
        host == format!("127.0.0.1:{port}")
            || host.eq_ignore_ascii_case(&format!("localhost:{port}"))
    }

    fn token_matches(&self, headers: &HeaderMap) -> bool {
        let Some(presented) = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| {
                let (scheme, rest) = value.split_once(' ')?;
                scheme.eq_ignore_ascii_case("bearer").then_some(rest.trim())
            })
        else {
            return false;
        };
        constant_time_equal(presented.as_bytes(), self.token.as_bytes())
    }
}

/// 244 random bits as hex: two version 4 UUIDs from the operating system's generator.
fn fresh_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Compares without stopping at the first difference, so timing does not reveal a prefix.
fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

/// Writes the token and the port as owner-only files in `directory`, replacing older ones.
pub(crate) fn publish_credentials(directory: &Path, guard: &RouteGuard) -> std::io::Result<()> {
    write_private(&directory.join(TOKEN_FILE), guard.token())?;
    write_private(&directory.join(PORT_FILE), &guard.port().to_string())
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut temporary_name = path.as_os_str().to_owned();
    temporary_name.push(".tmp");
    let temporary = std::path::PathBuf::from(temporary_name);
    match std::fs::remove_file(&temporary) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    // `create_new` refuses a symlink planted at the temporary name; the descriptor is
    // restricted before any secret is written to it.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    secure_fs::restrict_open_file_to_owner(&file)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    secure_fs::replace_file_durably(&temporary, path)
}

/// The fallback of the loopback router: plugin routes, or a plain 404.
pub(crate) async fn serve(
    state: Arc<ServerState>,
    guard: Arc<RouteGuard>,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    if !parts.uri.path().starts_with(PLUGIN_PREFIX) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(host) = state.plugins.get() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !host.has_http_routes() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Err(status) = guard.admit(&parts.headers) {
        return refusal(status);
    }
    let method = parts.method.as_str().to_owned();
    let target = parts.uri.path_and_query().map_or_else(
        || parts.uri.path().to_owned(),
        |target| target.as_str().to_owned(),
    );
    if let Some(reply) = host.http_precheck(&method, &target) {
        return reply_response(reply);
    }
    let body = match read_body(&parts.headers, body).await {
        Ok(body) => body,
        Err(status) => return refusal(status),
    };
    let headers = parts
        .headers
        .iter()
        // The caller's token is for the listener; the plugin never sees it.
        .filter(|(name, _)| *name != header::AUTHORIZATION)
        .filter_map(|(name, value)| {
            Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
        })
        .collect();
    let reply = host
        .http_request(PluginRequest {
            method,
            target,
            headers,
            body,
        })
        .await;
    reply_response(reply)
}

/// Reads the request body up to the limit: 413 above it, 400 when the stream breaks.
async fn read_body(headers: &HeaderMap, body: Body) -> Result<Vec<u8>, StatusCode> {
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok());
    if declared.is_some_and(|length| length > MAX_REQUEST_BODY_BYTES) {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    match axum::body::to_bytes(body, MAX_REQUEST_BODY_BYTES).await {
        Ok(bytes) => Ok(bytes.to_vec()),
        Err(error) if is_length_limit(&error) => Err(StatusCode::PAYLOAD_TOO_LARGE),
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

/// True when `error` or one of its sources is the body-limit error.
fn is_length_limit(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.to_string().contains("length limit exceeded") {
            return true;
        }
        current = error.source();
    }
    false
}

fn refusal(status: StatusCode) -> Response {
    let mut response = status.into_response();
    if status == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    response
}

/// Turns what the plugin host answered into the HTTP response.
pub(crate) fn reply_response(reply: Reply) -> Response {
    match reply {
        Reply::Response(plugin) => plugin_response(plugin),
        Reply::NotFound => StatusCode::NOT_FOUND.into_response(),
        Reply::MethodNotAllowed(allowed) => {
            let mut response = StatusCode::METHOD_NOT_ALLOWED.into_response();
            if let Ok(value) = HeaderValue::from_str(&allowed.join(", ")) {
                response.headers_mut().insert(header::ALLOW, value);
            }
            response
        }
        Reply::Unavailable(message) => (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({ "error": message })),
        )
            .into_response(),
        Reply::Timeout => StatusCode::GATEWAY_TIMEOUT.into_response(),
    }
}

fn plugin_response(plugin: ilium_plugin_host::imports::http_api::HttpResponse) -> Response {
    let status = StatusCode::from_u16(plugin.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut response = Response::new(Body::from(plugin.body));
    *response.status_mut() = status;
    for (name, value) in plugin.headers {
        // The length is the body's own; a plugin-supplied one could disagree with it.
        if name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        let parsed = HeaderName::from_bytes(name.as_bytes())
            .ok()
            .zip(HeaderValue::from_str(&value).ok());
        match parsed {
            Some((name, value)) => {
                response.headers_mut().append(name, value);
            }
            None => tracing::warn!("a plugin response header was dropped: {name:?} is not valid"),
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_host::imports::http_api::HttpResponse;

    fn guard() -> RouteGuard {
        RouteGuard::with_token(8872, "secret-token".to_owned())
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                HeaderValue::from_str(value).expect("header value"),
            );
        }
        map
    }

    /// REQ: API-005
    #[test]
    fn a_loopback_host_and_the_token_are_admitted() {
        for host in ["127.0.0.1:8872", "localhost:8872", "LocalHost:8872"] {
            let map = headers(&[("host", host), ("authorization", "Bearer secret-token")]);
            assert_eq!(guard().admit(&map), Ok(()), "{host}");
        }
    }

    /// REQ: API-005
    #[test]
    fn another_host_is_forbidden_before_the_token_is_looked_at() {
        for host in [
            "evil.example:8872",
            "127.0.0.1:9999",
            "127.0.0.1",
            "localhost",
        ] {
            let map = headers(&[("host", host), ("authorization", "Bearer secret-token")]);
            assert_eq!(guard().admit(&map), Err(StatusCode::FORBIDDEN), "{host}");
        }
        let without_host = headers(&[("authorization", "Bearer secret-token")]);
        assert_eq!(guard().admit(&without_host), Err(StatusCode::FORBIDDEN));
    }

    /// REQ: API-005
    #[test]
    fn an_origin_header_is_forbidden_even_with_the_token() {
        let map = headers(&[
            ("host", "127.0.0.1:8872"),
            ("origin", "https://evil.example"),
            ("authorization", "Bearer secret-token"),
        ]);
        assert_eq!(guard().admit(&map), Err(StatusCode::FORBIDDEN));
    }

    /// REQ: API-005
    #[test]
    fn a_missing_or_wrong_token_is_unauthorized() {
        let host = ("host", "127.0.0.1:8872");
        for authorization in [
            None,
            Some("Bearer wrong"),
            Some("Bearer secret-toke"),
            Some("Bearer secret-token-and-more"),
            Some("Basic secret-token"),
            Some("secret-token"),
        ] {
            let mut pairs = vec![host];
            if let Some(value) = authorization {
                pairs.push(("authorization", value));
            }
            assert_eq!(
                guard().admit(&headers(&pairs)),
                Err(StatusCode::UNAUTHORIZED),
                "{authorization:?}"
            );
        }
    }

    /// REQ: API-005
    #[test]
    fn tokens_are_long_random_and_unique() {
        let first = fresh_token();
        let second = fresh_token();
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    /// REQ: API-005
    #[test]
    fn the_credentials_are_published_owner_only() {
        let directory = tempfile::tempdir().expect("temp dir");
        let guard = RouteGuard::with_token(4321, "published-token".to_owned());
        publish_credentials(directory.path(), &guard).expect("publish");
        let token_path = directory.path().join(TOKEN_FILE);
        let port_path = directory.path().join(PORT_FILE);
        assert_eq!(
            std::fs::read_to_string(&token_path).expect("token"),
            "published-token"
        );
        assert_eq!(std::fs::read_to_string(&port_path).expect("port"), "4321");
        #[cfg(unix)]
        for path in [&token_path, &port_path] {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{}", path.display());
        }
        // A restart replaces both files and leaves no temporary file behind.
        let next = RouteGuard::with_token(4322, "next-token".to_owned());
        publish_credentials(directory.path(), &next).expect("republish");
        assert_eq!(
            std::fs::read_to_string(&token_path).expect("token"),
            "next-token"
        );
        let leftovers: Vec<_> = std::fs::read_dir(directory.path())
            .expect("list")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    async fn body_text(response: Response) -> (StatusCode, HeaderMap, String) {
        let (parts, body) = response.into_parts();
        let bytes = axum::body::to_bytes(body, 1024 * 1024).await.expect("body");
        (
            parts.status,
            parts.headers,
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    /// REQ: API-005
    #[tokio::test]
    async fn a_plugin_response_keeps_status_headers_and_body() {
        let response = reply_response(Reply::Response(HttpResponse {
            status: 201,
            headers: vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Content-Length".to_owned(), "9999".to_owned()),
                ("X-Note".to_owned(), "a".to_owned()),
                ("X-Note".to_owned(), "b".to_owned()),
                ("bad name".to_owned(), "dropped".to_owned()),
            ],
            body: b"{\"ok\":true}".to_vec(),
        }));
        let (status, headers, body) = body_text(response).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        assert_eq!(headers.get_all("x-note").iter().count(), 2);
        assert_eq!(body, "{\"ok\":true}");
        assert_ne!(
            headers.get(header::CONTENT_LENGTH),
            Some(&HeaderValue::from_static("9999")),
            "the plugin's own length is not trusted"
        );
        assert!(!headers.contains_key("bad name"));
    }

    /// REQ: API-005
    #[tokio::test]
    async fn the_other_replies_map_to_their_statuses() {
        let (status, _, _) = body_text(reply_response(Reply::NotFound)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, headers, _) = body_text(reply_response(Reply::MethodNotAllowed(vec![
            "GET".to_owned(),
            "POST".to_owned(),
        ])))
        .await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(headers[header::ALLOW], "GET, POST");
        let (status, _, body) =
            body_text(reply_response(Reply::Unavailable("stopped".to_owned()))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains("stopped"), "{body}");
        let (status, _, _) = body_text(reply_response(Reply::Timeout)).await;
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    }

    /// REQ: API-005
    #[tokio::test]
    async fn a_body_over_one_mebibyte_is_413_and_a_smaller_one_is_read() {
        let small = vec![7_u8; MAX_REQUEST_BODY_BYTES];
        assert_eq!(
            read_body(&HeaderMap::new(), Body::from(small.clone())).await,
            Ok(small)
        );
        let large = vec![7_u8; MAX_REQUEST_BODY_BYTES + 1];
        assert_eq!(
            read_body(&HeaderMap::new(), Body::from(large)).await,
            Err(StatusCode::PAYLOAD_TOO_LARGE),
            "streamed past the limit"
        );
        let declared = headers(&[("content-length", "1048577")]);
        assert_eq!(
            read_body(&declared, Body::empty()).await,
            Err(StatusCode::PAYLOAD_TOO_LARGE),
            "declared above the limit, refused before reading"
        );
    }

    /// REQ: API-005
    #[tokio::test]
    async fn paths_outside_the_plugin_prefix_stay_a_plain_404_without_credentials() {
        let directory = tempfile::tempdir().expect("temp dir");
        let state = Arc::new(crate::state::test_server_state(&directory));
        let request = Request::builder()
            .uri("/nothing/here")
            .body(Body::empty())
            .expect("request");
        let response = serve(state, Arc::new(guard()), request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
