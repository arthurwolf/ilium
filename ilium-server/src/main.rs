//! Detached server entrypoint. The `ilium` CLI resolves a project session and
//! passes its canonical root plus exact socket and snapshot paths here.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const RUNTIME_WORKER_THREADS: usize = 2;
const RUNTIME_MAX_BLOCKING_THREADS: usize = 4;
const RUNTIME_THREAD_STACK_BYTES: usize = 1024 * 1024;
const RUNTIME_BLOCKING_THREAD_KEEP_ALIVE: Duration = Duration::from_secs(5);

struct ServerLaunch {
    session_name: String,
    socket_path: PathBuf,
    snapshot_path: PathBuf,
    session_cwd: PathBuf,
    log_path: PathBuf,
}

fn main() -> ExitCode {
    let launch = match parse_launch(std::env::args().collect()) {
        Ok(launch) => launch,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    let config_dir = match ilium_server::paths::config_dir() {
        Ok(config_dir) => config_dir,
        Err(error) => {
            eprintln!("failed to resolve config directory: {error}");
            return ExitCode::FAILURE;
        }
    };
    let file_logging_enabled_hint =
        ilium_logging::file_logging_enabled_hint(&config_dir.join("config.toml"))
            .ok()
            .flatten()
            .unwrap_or(false);
    if let Err(error) =
        ilium_logging::initialize(&launch.log_path, file_logging_enabled_hint, "server")
    {
        eprintln!("failed to initialise server logging: {error}");
        return ExitCode::FAILURE;
    }
    ilium_logging::install_panic_logging();
    let server_config = match ilium_server::config::load(&config_dir) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(%error, "failed to load config, using defaults");
            eprintln!("failed to load config, using defaults: {error}");
            let mut config = ilium_server::config::ServerConfig::default();
            config.debug.file_logging_enabled = file_logging_enabled_hint;
            config
        }
    };
    if let Err(error) = ilium_logging::set_enabled(server_config.debug.file_logging_enabled) {
        tracing::error!(%error, "failed to apply server file logging configuration");
        eprintln!("failed to apply server file logging configuration: {error}");
        return ExitCode::FAILURE;
    }
    tracing::info!(
        session_name = launch.session_name,
        log_path = %launch.log_path.display(),
        "ilium-server starting"
    );

    // Configure this before any detection refresh can create sysinfo's
    // Linux process handles. The function is a no-op on other platforms.
    ilium_detect::configure_process_refresh();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        // A session coordinates IPC and timers; PTY reads already live on
        // their own threads. Mirroring every host CPU for every detached
        // session multiplies scheduler overhead without adding throughput.
        .worker_threads(RUNTIME_WORKER_THREADS)
        // Detection, snapshots, sounds, and notifications use short blocking
        // jobs. Bound their pool so many sessions cannot create an unbounded
        // fleet of retained helper threads.
        .max_blocking_threads(RUNTIME_MAX_BLOCKING_THREADS)
        .thread_keep_alive(RUNTIME_BLOCKING_THREAD_KEEP_ALIVE)
        .thread_stack_size(RUNTIME_THREAD_STACK_BYTES)
        .thread_name("ilium-server-rt")
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!(%error, error_debug = ?error, "failed to start the tokio runtime");
            eprintln!("failed to start the tokio runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(async_main(launch, config_dir, server_config))
}

async fn async_main(
    launch: ServerLaunch,
    config_dir: PathBuf,
    server_config: ilium_server::config::ServerConfig,
) -> ExitCode {
    let options = ilium_server::ServerOptions {
        session_name: launch.session_name,
        socket_path: launch.socket_path,
        snapshot_path: launch.snapshot_path,
        session_cwd: launch.session_cwd,
        home_dir: directories::BaseDirs::new()
            .map(|directories| directories.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/")),
        detection_config: server_config.detection,
        notifications_config: server_config.notifications,
        sound_settings: server_config.sound,
        sound_config_path: Some(config_dir.join("config.toml")),
        sound_player: std::sync::Arc::new(ilium_server::SystemSoundPlayer),
        custom_signatures: server_config.custom_signatures,
        session_recovery: server_config.session_recovery,
        agent_debug_menu_enabled: server_config.agent_debug_menu_enabled,
    };
    match ilium_server::run(options).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("server exited with an error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_launch(argv: Vec<String>) -> Result<ServerLaunch, String> {
    let mut values = argv.into_iter().skip(1);
    let mut session_name = None;
    let mut socket_path = None;
    let mut snapshot_path = None;
    let mut session_cwd = None;
    let mut log_path = None;
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--session-name" => session_name = Some(value),
            "--socket-path" => socket_path = Some(PathBuf::from(value)),
            "--snapshot-path" => snapshot_path = Some(PathBuf::from(value)),
            "--session-cwd" => session_cwd = Some(PathBuf::from(value)),
            "--log-path" => log_path = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    Ok(ServerLaunch {
        session_name: session_name.ok_or_else(usage)?,
        socket_path: socket_path.ok_or_else(usage)?,
        snapshot_path: snapshot_path.ok_or_else(usage)?,
        session_cwd: session_cwd.ok_or_else(usage)?,
        log_path: log_path.ok_or_else(usage)?,
    })
}

fn usage() -> String {
    "usage: ilium-server --session-name <name> --socket-path <path> --snapshot-path <path> --session-cwd <directory> --log-path <path>".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_project_session_paths() {
        let launch = parse_launch(vec![
            "ilium-server".to_string(),
            "--session-name".to_string(),
            "default".to_string(),
            "--socket-path".to_string(),
            "/run/user/1000/ilium/project.sock".to_string(),
            "--snapshot-path".to_string(),
            "/work/project/.ilium/sessions/default.json".to_string(),
            "--session-cwd".to_string(),
            "/work/project".to_string(),
            "--log-path".to_string(),
            "/tmp/.ilium/work-project-default/log-2026-07-19_12-00-00.000.txt".to_string(),
        ])
        .expect("valid launch");
        assert_eq!(launch.session_name, "default");
        assert_eq!(
            launch.socket_path,
            PathBuf::from("/run/user/1000/ilium/project.sock")
        );
        assert_eq!(
            launch.snapshot_path,
            PathBuf::from("/work/project/.ilium/sessions/default.json")
        );
        assert_eq!(launch.session_cwd, PathBuf::from("/work/project"));
        assert_eq!(
            launch.log_path,
            PathBuf::from("/tmp/.ilium/work-project-default/log-2026-07-19_12-00-00.000.txt")
        );
    }

    #[test]
    fn rejects_missing_project_paths() {
        assert!(parse_launch(vec!["ilium-server".to_string()]).is_err());
    }
}
