//! G17: plugin routes on the loopback HTTP API, through a real in-process server.
//!
//! The server runs the bundled fleet plugin's manifest without its component, so the plugin is
//! listed (its `POST /create_agent` route and grant are registered) but has no code to answer.
//! That is enough to check every step the listener owns, in the manual's order: Host, Origin,
//! bearer token, route and method, body size, and a stopped owner. The plugin's own response and
//! the 30 s timeout are covered by the unit tests in `plugin_host/http_routes.rs`, which drive
//! the same dispatcher with a fake host; no compiler for plugin components exists in this
//! environment, so a test plugin that calls `respond` cannot be built here.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ilium_server::config::{DetectionConfig, HttpApiConfig, NotificationsConfig};
use ilium_server::plugin_host::PluginStart;
use ilium_server::{run_with_plugins, NoopSoundPlayer, ServerOptions, ServerResources};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod common;
use common::{short_socket_dir, wait_until};

const FLEET_ROUTE: &str = "/plugins/ilium.core.fleet/create_agent";

/// A running in-process server with the fleet manifest as its only plugin.
struct Fixture {
    port: u16,
    token: String,
    task: tokio::task::JoinHandle<()>,
    _socket_dir: tempfile::TempDir,
    _data_dir: tempfile::TempDir,
    _project: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    async fn start() -> Self {
        let socket_dir = short_socket_dir();
        let data_dir = tempfile::tempdir().expect("plugin data dir");
        let project = tempfile::tempdir().expect("project dir");
        let canonical = ilium_platform::paths::canonicalize(project.path()).expect("canonical");
        let fleet = ilium_plugin_bundle::find("ilium.core.fleet").expect("bundled fleet plugin");
        let catalogue = ilium_plugin_host::wiring::catalogue([(fleet.id, fleet.manifest, None)]);
        let options = ServerOptions {
            resource_catalog_path: None,
            session_name: "g17".to_owned(),
            socket_path: socket_dir.path().join("g17.sock"),
            snapshot_path: project.path().join("g17.snapshot.json"),
            ready_log_metadata: None,
            session_cwd: canonical.clone(),
            home_dir: canonical,
            detection_config: DetectionConfig::default(),
            notifications_config: NotificationsConfig {
                enabled: false,
                ..NotificationsConfig::default()
            },
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_config_path: None,
            sound_player: Arc::new(NoopSoundPlayer),
            custom_signatures: Vec::new(),
            session_recovery: ilium_server::config::SessionRecoveryConfig::RestoreAutomatically,
            agent_debug_menu_enabled: false,
            http_api: HttpApiConfig { port: 0 },
            progress_monitor_enabled: true,
            session_backups_enabled: false,
        };
        let start = PluginStart {
            catalogue,
            data_dir: data_dir.path().to_path_buf(),
            mode: ilium_plugin_host::lifecycle::StartMode::Normal,
        };
        let task = tokio::spawn(async move {
            if let Err(error) = run_with_plugins(options, ServerResources::new(), Some(start)).await
            {
                eprintln!("ilium-server exited with an error: {error}");
            }
        });
        let port_file = socket_dir.path().join("http-api.port");
        let token_file = socket_dir.path().join("http-api.token");
        let published = wait_until(
            || {
                std::fs::read_to_string(&port_file)
                    .is_ok_and(|port| port.trim().parse::<u16>().is_ok())
                    && std::fs::read_to_string(&token_file).is_ok_and(|token| !token.is_empty())
            },
            Duration::from_secs(20),
        )
        .await;
        assert!(published, "the server published its port and token files");
        let port = std::fs::read_to_string(&port_file)
            .expect("port file")
            .trim()
            .parse()
            .expect("port number");
        let token = std::fs::read_to_string(&token_file).expect("token file");
        Self {
            port,
            token,
            task,
            _socket_dir: socket_dir,
            _data_dir: data_dir,
            _project: project,
        }
    }

    fn credentials_dir(&self) -> PathBuf {
        self._socket_dir.path().to_path_buf()
    }

    /// The default request: right Host, the token, no Origin.
    fn request(&self, method: &str, path: &str) -> Request {
        Request {
            method: method.to_owned(),
            path: path.to_owned(),
            headers: vec![
                ("Host".to_owned(), format!("127.0.0.1:{}", self.port)),
                ("Authorization".to_owned(), format!("Bearer {}", self.token)),
            ],
            body: Vec::new(),
            declared_length: None,
        }
    }

    async fn send(&self, request: Request) -> Reply {
        request.send(self.port).await
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// A `Content-Length` that differs from the body actually sent.
    declared_length: Option<usize>,
}

impl Request {
    fn without_header(mut self, name: &str) -> Self {
        self.headers
            .retain(|(held, _)| !held.eq_ignore_ascii_case(name));
        self
    }

    fn with_header(mut self, name: &str, value: &str) -> Self {
        self = self.without_header(name);
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    async fn send(self, port: u16) -> Reply {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect to the loopback API");
        let length = self.declared_length.unwrap_or(self.body.len());
        let mut head = format!("{} {} HTTP/1.1\r\n", self.method, self.path);
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!(
            "Content-Length: {length}\r\nConnection: close\r\n\r\n"
        ));
        stream.write_all(head.as_bytes()).await.expect("write head");
        stream.write_all(&self.body).await.expect("write body");
        let mut raw = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut raw));
        read.await.expect("the server answers in time").ok();
        Reply::parse(&raw)
    }
}

#[derive(Debug)]
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn parse(raw: &[u8]) -> Self {
        let text = String::from_utf8_lossy(raw).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let mut lines = head.lines();
        let status = lines
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status line in {text:?}"));
        let headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        Self {
            status,
            headers,
            body: body.to_owned(),
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(held, _)| held == name)
            .map(|(_, value)| value.as_str())
    }
}

/// REQ: API-005
#[tokio::test]
async fn the_server_publishes_a_fresh_token_and_the_port_owner_only() {
    let fixture = Fixture::start().await;
    assert!(fixture.token.len() >= 32, "a long token: {}", fixture.token);
    use std::os::unix::fs::PermissionsExt;
    for name in ["http-api.token", "http-api.port"] {
        let mode = std::fs::metadata(fixture.credentials_dir().join(name))
            .expect("credential file")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{name} is owner-only");
    }
}

/// REQ: API-005
#[tokio::test]
async fn host_origin_and_token_are_checked_before_the_route() {
    let fixture = Fixture::start().await;
    let wrong_host = fixture
        .send(
            fixture
                .request("POST", FLEET_ROUTE)
                .with_header("Host", "evil.example"),
        )
        .await;
    assert_eq!(wrong_host.status, 403, "{wrong_host:?}");
    let wrong_port = fixture
        .send(
            fixture
                .request("POST", FLEET_ROUTE)
                .with_header("Host", "127.0.0.1:1"),
        )
        .await;
    assert_eq!(wrong_port.status, 403, "{wrong_port:?}");
    let browser = fixture
        .send(
            fixture
                .request("POST", FLEET_ROUTE)
                .with_header("Origin", "https://evil.example"),
        )
        .await;
    assert_eq!(browser.status, 403, "{browser:?}");
    let anonymous = fixture
        .send(
            fixture
                .request("POST", FLEET_ROUTE)
                .without_header("Authorization"),
        )
        .await;
    assert_eq!(anonymous.status, 401, "{anonymous:?}");
    assert_eq!(anonymous.header("www-authenticate"), Some("Bearer"));
    let wrong_token = fixture
        .send(
            fixture
                .request("POST", FLEET_ROUTE)
                .with_header("Authorization", "Bearer not-the-token"),
        )
        .await;
    assert_eq!(wrong_token.status, 401, "{wrong_token:?}");
    // An unknown plugin gets the same answer as a known one without the token: no oracle.
    let probe = fixture
        .send(
            fixture
                .request("POST", "/plugins/no.such.plugin/x")
                .without_header("Authorization"),
        )
        .await;
    assert_eq!(probe.status, 401, "{probe:?}");
}

/// REQ: API-005
#[tokio::test]
async fn unknown_routes_and_wrong_methods_never_reach_the_plugin() {
    let fixture = Fixture::start().await;
    let unknown_path = fixture
        .send(fixture.request("POST", "/plugins/ilium.core.fleet/nothing"))
        .await;
    assert_eq!(unknown_path.status, 404, "{unknown_path:?}");
    let unknown_plugin = fixture
        .send(fixture.request("POST", "/plugins/no.such.plugin/create_agent"))
        .await;
    assert_eq!(unknown_plugin.status, 404, "{unknown_plugin:?}");
    let wrong_method = fixture.send(fixture.request("GET", FLEET_ROUTE)).await;
    assert_eq!(wrong_method.status, 405, "{wrong_method:?}");
    assert_eq!(wrong_method.header("allow"), Some("POST"));
}

/// REQ: API-005
#[tokio::test]
async fn a_body_over_one_mebibyte_is_refused_with_413() {
    let fixture = Fixture::start().await;
    let mut oversized = fixture.request("POST", FLEET_ROUTE);
    oversized.declared_length = Some(1024 * 1024 + 1);
    let reply = fixture.send(oversized).await;
    assert_eq!(reply.status, 413, "{reply:?}");
}

/// REQ: API-005
#[tokio::test]
async fn an_owner_without_running_code_answers_503() {
    let fixture = Fixture::start().await;
    let mut request = fixture.request("POST", &format!("{FLEET_ROUTE}?draft=1"));
    request.body = br#"{"agent_type":"claude"}"#.to_vec();
    let reply = fixture.send(request).await;
    assert_eq!(reply.status, 503, "{reply:?}");
    assert!(reply.body.contains("error"), "{reply:?}");
}

/// REQ: API-005
#[tokio::test]
async fn kernel_routes_keep_the_bare_paths_and_need_no_token() {
    let fixture = Fixture::start().await;
    // The bare `/create_agent` is the kernel's: it answers (here a client error for the empty
    // body), never the plugin's 401/503, and the plugin prefix does not shadow it.
    let kernel = fixture
        .send(
            fixture
                .request("POST", "/create_agent")
                .without_header("Authorization"),
        )
        .await;
    assert!(
        matches!(kernel.status, 400 | 415 | 422),
        "the kernel handles the bare path: {kernel:?}"
    );
    let unrelated = fixture
        .send(
            fixture
                .request("GET", "/no/such/path")
                .without_header("Authorization"),
        )
        .await;
    assert_eq!(unrelated.status, 404, "{unrelated:?}");
}
