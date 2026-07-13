//! Detached server entrypoint. The `ilium` CLI resolves a project session and
//! passes its canonical root plus exact socket and snapshot paths here.

use std::path::PathBuf;
use std::process::ExitCode;

struct ServerLaunch {
    session_name: String,
    socket_path: PathBuf,
    snapshot_path: PathBuf,
    session_cwd: PathBuf,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let launch = match parse_launch(std::env::args().collect()) {
        Ok(launch) => launch,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

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

fn parse_launch(argv: Vec<String>) -> Result<ServerLaunch, String> {
    let mut values = argv.into_iter().skip(1);
    let mut session_name = None;
    let mut socket_path = None;
    let mut snapshot_path = None;
    let mut session_cwd = None;
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--session-name" => session_name = Some(value),
            "--socket-path" => socket_path = Some(PathBuf::from(value)),
            "--snapshot-path" => snapshot_path = Some(PathBuf::from(value)),
            "--session-cwd" => session_cwd = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    Ok(ServerLaunch {
        session_name: session_name.ok_or_else(usage)?,
        socket_path: socket_path.ok_or_else(usage)?,
        snapshot_path: snapshot_path.ok_or_else(usage)?,
        session_cwd: session_cwd.ok_or_else(usage)?,
    })
}

fn usage() -> String {
    "usage: ilium-server --session-name <name> --socket-path <path> --snapshot-path <path> --session-cwd <directory>".to_string()
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
        ])
        .expect("valid launch");
        assert_eq!(launch.session_name, "default");
        assert_eq!(launch.session_cwd, PathBuf::from("/work/project"));
    }

    #[test]
    fn rejects_missing_project_paths() {
        assert!(parse_launch(vec!["ilium-server".to_string()]).is_err());
    }
}
