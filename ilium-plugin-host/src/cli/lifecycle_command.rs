//! `uninstall`, `enable`, `disable` and `log-level` (reference-plugin-cli.md, DEV-155..161, DEV-195).
//!
//! Every command talks to the session through typed requests; none touches the plugin store
//! directly, so the session stays the single authority on live state.
use super::{offline, CliEnv, CliOutput, SessionBackend, SessionRequest, SessionResponse};
use serde_json::{json, Value};

const LEVELS: [&str; 5] = ["trace", "debug", "info", "warn", "error"];

#[derive(Default)]
struct Options {
    id: Option<String>,
    level: Option<String>,
    json: bool,
    purge: bool,
    keep_data: bool,
    reset: bool,
}

fn usage(code: &str, message: &str) -> CliOutput {
    offline::failure(code, message, 2)
}

fn parse(command: &str, args: &[String]) -> Result<Options, CliOutput> {
    let mut options = Options::default();
    for arg in args {
        match arg.as_str() {
            "--json" => options.json = true,
            "--quiet" | "--no-color" => {}
            "--purge" if command == "uninstall" => options.purge = true,
            "--keep-data" if command == "uninstall" => options.keep_data = true,
            "--reset" if command == "log-level" => options.reset = true,
            flag if flag.starts_with('-') => {
                return Err(usage("usage.unknown-flag", "Unsupported flag"));
            }
            _ if options.id.is_none() => options.id = Some(arg.clone()),
            _ if command == "log-level" && options.level.is_none() => {
                options.level = Some(arg.clone());
            }
            _ => return Err(usage("usage.argument-invalid", "Unexpected argument")),
        }
    }
    if options.id.as_deref().is_none_or(str::is_empty) {
        return Err(usage("usage.missing-argument", "A plugin id is required"));
    }
    if options.purge && options.keep_data {
        return Err(usage(
            "usage.conflict",
            "--keep-data and --purge cannot be combined",
        ));
    }
    if let Some(level) = &options.level {
        if !LEVELS.contains(&level.as_str()) {
            return Err(usage("usage.invalid-value", "Unknown log level"));
        }
        if options.reset {
            return Err(usage(
                "usage.conflict",
                "A level and --reset cannot be combined",
            ));
        }
    }
    Ok(options)
}

fn failure(code: String) -> CliOutput {
    let exit = match code.as_str() {
        "session.unreachable" | "session.not-found" => 6,
        "plugin.not-found" => 5,
        _ => 1,
    };
    offline::failure(&code, "Session command failed", exit)
}

fn send(session: &dyn SessionBackend, request: SessionRequest) -> Result<SessionResponse, CliOutput> {
    session.request(request).map_err(failure)
}

/// Runs one of the lifecycle commands.
pub fn run(command: &str, args: &[String], env: &CliEnv) -> CliOutput {
    let options = match parse(command, args) {
        Ok(options) => options,
        Err(output) => return output,
    };
    let Some(session) = env.session.as_ref() else {
        return offline::failure("session.unreachable", "No reachable session was supplied", 6);
    };
    let id = options.id.clone().unwrap_or_default();
    execute(command, &options, &id, session.as_ref()).unwrap_or_else(|output| output)
}

fn execute(
    command: &str,
    options: &Options,
    id: &str,
    session: &dyn SessionBackend,
) -> Result<CliOutput, CliOutput> {
    send(session, SessionRequest::Connect)?;
    let described = send(session, SessionRequest::Describe { id: id.to_owned() })?;
    let SessionResponse::Plugin(plugin) = described else {
        return Err(failure("session.response-invalid".into()));
    };
    let mut records: Vec<Value> = Vec::new();
    let mut text = String::new();
    match command {
        "disable" => {
            if plugin.state != "disabled" {
                send(session, SessionRequest::Disable { id: id.to_owned() })?;
            }
            text.push_str(&format!("{id} disabled\n"));
        }
        "enable" => {
            if plugin.state != "running" {
                send(session, SessionRequest::Enable { id: id.to_owned() })?;
            }
            text.push_str(&format!("{id} enabled\n"));
        }
        "uninstall" => {
            send(session, SessionRequest::Disable { id: id.to_owned() })?;
            send(
                session,
                SessionRequest::Uninstall {
                    id: id.to_owned(),
                    purge_data: options.purge,
                    purge_log: options.purge,
                },
            )?;
            if options.purge {
                let mut removed: Vec<String> = Vec::new();
                if plugin.data_exists {
                    removed.push("data folder".into());
                }
                if plugin.cache_exists {
                    removed.push("cache folder".into());
                }
                for _ in 0..plugin.storage_buckets {
                    removed.push("storage bucket".into());
                }
                for _ in 0..plugin.settings_values {
                    removed.push("settings value".into());
                }
                if plugin.log_kept {
                    removed.push("log".into());
                }
                for item in removed {
                    records.push(json!({"type":"warning","code":"uninstall.purged",
                        "message":format!("Removed {item} of {id}")}));
                }
            }
            text.push_str(&format!("{id} uninstalled\n"));
        }
        _ => {
            if options.reset {
                send(session, SessionRequest::ResetLogLevel { id: id.to_owned() })?;
            } else if let Some(level) = &options.level {
                send(
                    session,
                    SessionRequest::SetLogLevel {
                        id: id.to_owned(),
                        level: level.clone(),
                    },
                )?;
            }
            let SessionResponse::Value(level) =
                send(session, SessionRequest::GetLogLevel { id: id.to_owned() })?
            else {
                return Err(failure("session.response-invalid".into()));
            };
            records.push(json!({"type":"log-level","plugin":id,"level":level}));
            text.push_str(&format!("{id} {level}\n"));
        }
    }
    let mut output = offline::finish(records, 0);
    if !options.json {
        output.stdout_text = text;
    }
    Ok(output)
}
