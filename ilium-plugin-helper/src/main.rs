//! `ilium-plugin-helper`: spawned by the plugin host, serves on stdin and stdout.

use ilium_plugin_helper::{serve, HelperConfig};

fn main() -> std::process::ExitCode {
    let config = HelperConfig {
        epoch_tick_ms: 10,
        apply_sandbox: true,
    };
    match serve(std::io::stdin().lock(), std::io::stdout().lock(), config) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ilium-plugin-helper: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
