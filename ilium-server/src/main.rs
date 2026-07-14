//! Detached server entrypoint. The `ilium` CLI resolves a project session and
//! passes its canonical root plus exact socket and snapshot paths here.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

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
    let _log_guard = match initialize_tracing(&launch.log_path) {
        Ok(log_guard) => log_guard,
        Err(error) => {
            eprintln!(
                "failed to initialise server log {}: {error}",
                launch.log_path.display()
            );
            return ExitCode::FAILURE;
        }
    };
    install_panic_logging();
    tracing::info!("ilium-server starting; log={}", launch.log_path.display());

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to start the tokio runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(async_main(launch))
}

async fn async_main(launch: ServerLaunch) -> ExitCode {
    let config_dir = match ilium_server::paths::config_dir() {
        Ok(config_dir) => config_dir,
        Err(error) => {
            tracing::error!("failed to resolve config directory: {error}");
            return ExitCode::FAILURE;
        }
    };
    let server_config = match ilium_server::config::load(&config_dir) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!("failed to load config, using defaults: {error}");
            ilium_server::config::ServerConfig::default()
        }
    };

    let options = ilium_server::ServerOptions {
        session_name: launch.session_name,
        socket_path: launch.socket_path,
        snapshot_path: launch.snapshot_path,
        session_cwd: launch.session_cwd,
        detection_config: server_config.detection,
        notifications_config: server_config.notifications,
        sound_settings: server_config.sound,
        sound_config_path: Some(config_dir.join("config.toml")),
        sound_player: std::sync::Arc::new(ilium_server::SystemSoundPlayer),
        custom_signatures: server_config.custom_signatures,
    };
    match ilium_server::run(options).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("server exited with an error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Initializes non-blocking, append-only diagnostics before the server starts
/// work. The detached launcher intentionally has no terminal, so this file is
/// the authoritative record of ordinary errors and panics for one session.
fn initialize_tracing(
    log_path: &Path,
) -> Result<tracing_appender::non_blocking::WorkerGuard, std::io::Error> {
    let log_directory = log_path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "server log path has no parent",
        )
    })?;
    let log_file_name = log_path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "server log path has no filename",
        )
    })?;
    std::fs::create_dir_all(log_directory)?;
    let file_appender = tracing_appender::rolling::never(log_directory, log_file_name);
    let (writer, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(writer)
        .init();
    Ok(guard)
}

/// Records a forced backtrace before the normal panic hook terminates the
/// process, making otherwise silent detached crashes diagnosable.
fn install_panic_logging() {
    std::panic::set_hook(Box::new(|panic_info| {
        tracing::error!(
            panic = %panic_info,
            backtrace = %std::backtrace::Backtrace::force_capture(),
            "ilium-server panicked"
        );
    }));
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
            "/work/project/.ilium/logs/default.log".to_string(),
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
            PathBuf::from("/work/project/.ilium/logs/default.log")
        );
    }

    #[test]
    fn rejects_missing_project_paths() {
        assert!(parse_launch(vec!["ilium-server".to_string()]).is_err());
    }
}
