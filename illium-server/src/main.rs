//! `illium-server` binary entrypoint: resolves this session's real
//! platform paths and config, then hands off to `illium_server::run`.
//!
//! Argument surface is deliberately minimal -- per `CLAUDE.md`, this
//! binary is meant to be spawned by the `illium` CLI wrapper (a later
//! stage), not typed by a human directly, so there's no reason for a
//! `clap`-derived multi-flag parser here.
//!
//! Usage: `illium-server <session-name>`

use std::process::ExitCode;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let session_name = match parse_session_name(std::env::args().collect()) {
        Ok(name) => name,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    // A single multi-thread runtime for the whole process, per
    // `CLAUDE.md`: PTY readers are real OS threads already (see
    // `illium-pty`), and the detection loop's `sysinfo` refresh uses
    // `spawn_blocking` -- both want a runtime that can actually run async
    // tasks concurrently with those, not the single-threaded flavor.
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

    runtime.block_on(async_main(session_name))
}

async fn async_main(session_name: String) -> ExitCode {
    let paths = match illium_server::paths::resolve(&session_name) {
        Ok(paths) => paths,
        Err(error) => {
            tracing::error!("failed to resolve session paths: {error}");
            return ExitCode::FAILURE;
        }
    };

    // A config file that fails to load is a warning, not a fatal error --
    // see `illium_server::config::load`'s doc comment.
    let detection_config = match illium_server::config::load(&paths.config_dir) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!("failed to load config, using defaults: {error}");
            illium_server::config::DetectionConfig::default()
        }
    };

    let options = illium_server::ServerOptions {
        session_name,
        socket_path: paths.socket_path,
        snapshot_path: paths.snapshot_path,
        detection_config,
    };

    match illium_server::run(options).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("server exited with an error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Parses `argv[1]` as the session name to serve. `argv[0]` is the
/// program path (per POSIX convention, always present); exactly one
/// further argument is required.
fn parse_session_name(argv: Vec<String>) -> Result<String, String> {
    match argv.as_slice() {
        [_program] => Err("usage: illium-server <session-name>".to_string()),
        [_program, session_name] => Ok(session_name.clone()),
        [program, ..] => Err(format!("usage: {program} <session-name>")),
        [] => Err("usage: illium-server <session-name>".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_session_name_is_an_error() {
        let result = parse_session_name(vec!["illium-server".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn a_single_session_name_argument_parses() {
        let result = parse_session_name(vec!["illium-server".to_string(), "main".to_string()]);
        assert_eq!(result, Ok("main".to_string()));
    }

    #[test]
    fn extra_arguments_are_rejected() {
        let result = parse_session_name(vec![
            "illium-server".to_string(),
            "main".to_string(),
            "extra".to_string(),
        ]);
        assert!(result.is_err());
    }
}
