//! Read-only session commands. No command starts or mutates a session.
use std::time::{Duration, SystemTime};
use serde_json::json;
use super::{CliEnv, CliOutput, SessionRequest, SessionResponse, offline};

/// A real persisted log observation. `at` and local wall-clock text must come
/// from the log reader, never from command execution time.
#[derive(Debug, Clone)]
pub struct SessionLog {
    pub at: SystemTime,
    pub local_time: String,
    pub side: String,
    pub level: String,
    pub message: String,
}

/// Snapshot in original file order across rotation, with actual redaction
/// context. Unknown ids without retained logs must return `plugin.not-found`.
/// Implementations must enforce private-file admission before reading logs.
pub trait SessionLogReader: Send + Sync {
    fn read(&self, plugin: &str) -> Result<Vec<SessionLog>, String>;
    fn secrets(&self, plugin: &str) -> Result<Vec<String>, String>;
}

#[derive(Default)]
struct Options {
    id: Option<String>,
    json: bool,
    tail: Option<usize>,
    level: Option<String>,
    side: Option<String>,
    since: Option<String>,
    follow: bool,
}

fn usage(message: &str) -> CliOutput { offline::failure("usage.argument-invalid", message, 2) }
fn level(value: &str) -> Option<u8> {
    ["trace", "debug", "info", "warn", "error"].iter().position(|item| *item == value).map(|n| n as u8)
}
fn valid_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 19 || ![4,7,10,13,16].into_iter().all(|n| bytes[n] == match n {4|7 => b'-',10 => b' ',_ => b':'}) {
        return false;
    }
    if bytes.iter().enumerate().any(|(n, byte)| ![4,7,10,13,16].contains(&n) && !byte.is_ascii_digit()) { return false; }
    let number = |start, end| value[start..end].parse::<u32>().unwrap_or(u32::MAX);
    let year = number(0,4); let month = number(5,7); let day = number(8,10);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month { 4|6|9|11 => 30, 2 if leap => 29, 2 => 28, 1|3|5|7|8|10|12 => 31, _ => 0 };
    year > 0 && day > 0 && day <= days && number(11,13) < 24 && number(14,16) < 60 && number(17,19) < 60
}
fn duration(value: &str) -> Option<Duration> {
    let (digits, multiplier) = [("s",1_u64),("m",60),("h",3600),("d",86400)].into_iter()
        .find_map(|(suffix, multiplier)| value.strip_suffix(suffix).map(|digits| (digits, multiplier)))?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
    Some(Duration::from_secs(digits.parse::<u64>().ok()?.checked_mul(multiplier)?))
}
fn parse(command: &str, args: &[String]) -> Result<Options, CliOutput> {
    if !matches!(command,"list"|"logs"|"grants") { return Err(usage("Unsupported read-only session command")); }
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => options.json = true,
            "--quiet" | "--no-color" => {},
            "--follow" if command == "logs" => options.follow = true,
            "--tail" | "--level" | "--side" | "--since" if command == "logs" => {
                let value = args.next().ok_or_else(|| usage("A log filter value is required"))?;
                match arg.as_str() {
                    "--tail" => {
                        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) { return Err(usage("Tail must be an unsigned integer")); }
                        options.tail = Some(value.parse().map_err(|_| usage("Tail is too large"))?);
                    },
                    "--level" if level(value).is_some() => options.level = Some(value.clone()),
                    "--side" if matches!(value.as_str(),"server"|"client"|"scene"|"host") => options.side = Some(value.clone()),
                    "--since" if valid_time(value) || duration(value).is_some() => options.since = Some(value.clone()),
                    _ => return Err(usage("Invalid log filter")),
                }
            },
            value if value.starts_with('-') => return Err(offline::failure("usage.unknown-flag", "Unsupported read-only command flag", 2)),
            _ if command == "list" || options.id.is_some() => return Err(usage("Unexpected positional argument")),
            _ => options.id = Some(arg.clone()),
        }
    }
    if command != "list" && options.id.as_deref().is_none_or(str::is_empty) {
        return Err(offline::failure("usage.argument-missing", "A plugin id is required", 2));
    }
    Ok(options)
}
fn failure(code: String) -> CliOutput {
    let exit = match code.as_str() { "session.unreachable"|"session.not-found" => 6,"plugin.not-found" => 5,_ => 1 };
    offline::failure(&code, "Session command failed", exit)
}

/// Read actual session observations. Historical logs require the timestamped
/// reader seam because the legacy SessionResponse::Logs tuple has no timestamp.
pub fn run(command: &str, args: &[String], env: &CliEnv) -> CliOutput {
    run_with_log_reader(command, args, env, None)
}

pub fn run_with_log_reader(command: &str, args: &[String], env: &CliEnv, logs: Option<&dyn SessionLogReader>) -> CliOutput {
    let options = match parse(command,args) { Ok(options)=>options,Err(output)=>return output };
    let Some(session) = env.session.as_ref() else { return offline::failure("session.unreachable","No reachable session was supplied",6); };
    if options.follow {
        return offline::failure("logs.context-required","Following logs requires a cancellable live log subscription",1);
    }
    match session.request(SessionRequest::Connect) {
        Ok(SessionResponse::Done) => {}, Ok(_) => return failure("session.response-invalid".into()), Err(code) => return failure(code),
    }
    if command == "logs" {
        let Some(reader) = logs else { return offline::failure("logs.context-required","Timestamped persisted log reader and redaction context are required",1); };
        return render_logs(options, env.now, reader);
    }
    if command == "grants" {
        let id = options.id.unwrap_or_default();
        let plugin = match session.request(SessionRequest::Describe { id }) {
            Ok(SessionResponse::Plugin(plugin))=>plugin,Ok(_)=>return failure("session.response-invalid".into()),Err(code)=>return failure(code),
        };
        let mut text = String::new();
        let records = plugin.grants.into_iter().map(|(grant,risk,state)| {
            text.push_str(&format!("{grant} {risk} {state}\n"));
            json!({"type":"grant","plugin":plugin.id,"grant":grant,"risk":risk,"state":state})
        }).collect();
        let mut output = offline::finish(records,0);
        if !options.json { output.stdout_text = text; }
        return output;
    }
    let plugins = match session.request(SessionRequest::List) {
        Ok(SessionResponse::Plugins(plugins))=>plugins,Ok(_)=>return failure("session.response-invalid".into()),Err(code)=>return failure(code),
    };
    let mut text = String::new();
    let mut records = Vec::new();
    for plugin in plugins {
        let granted = plugin.grants.iter().filter(|(_,_,state)| state == "granted").count();
        let canonical = match plugin.state.as_str() {
            "running"|"stopped" => "installed-enabled", "disabled"=>"installed-disabled-by-user", "untrusted"=>"disabled-untrusted", state=>state,
        };
        let state = match canonical {
            "installed-enabled" => if session.plugin(&plugin.id).is_some_and(|instance| instance.running) {"running"}else{"stopped"},
            "stopped-by-failure" => "stopped", "installed-disabled-by-user"|"disabled-by-policy"=>"disabled", "disabled-untrusted"|"disabled-blocked"=>"untrusted", state=>state,
        };
        text.push_str(&format!("{} {} {} {}{} grants {} of {}\n",plugin.id,plugin.version,plugin.tier,if plugin.dev {"[dev] "}else{""},state,granted,plugin.grants.len()));
        records.push(json!({"type":"plugin","id":plugin.id,"version":plugin.version,"tier":plugin.tier,"state":canonical,"dev":plugin.dev,"grants_granted":granted,"grants_requested":plugin.grants.len()}));
    }
    let mut output = offline::finish(records,0);
    if !options.json { output.stdout_text=text; }
    output
}

fn render_logs(options: Options, now: SystemTime, reader: &dyn SessionLogReader) -> CliOutput {
    let id = options.id.unwrap_or_default();
    let records = match reader.read(&id) {Ok(records)=>records,Err(code)=>return failure(code)};
    let secrets = match reader.secrets(&id) {Ok(secrets)=>secrets,Err(code)=>return failure(code)};
    let minimum = options.level.as_deref().and_then(level).unwrap_or(0);
    let cutoff = options.since.as_deref().and_then(duration).and_then(|since| now.checked_sub(since));
    let absolute = options.since.as_deref().filter(|value| valid_time(value));
    let mut selected = Vec::new();
    for record in records {
        let Some(severity) = level(&record.level) else {return failure("logs.record-invalid".into());};
        if !valid_time(&record.local_time) || !matches!(record.side.as_str(),"server"|"client"|"scene"|"host") {return failure("logs.record-invalid".into());}
        if severity < minimum || options.side.as_ref().is_some_and(|side| *side != record.side)
            || cutoff.is_some_and(|cutoff| record.at < cutoff) || absolute.is_some_and(|cutoff| record.local_time.as_str() < cutoff) {continue;}
        selected.push(record);
    }
    let start = selected.len().saturating_sub(options.tail.unwrap_or(200));
    let mut text = String::new();
    let records = selected.into_iter().skip(start).map(|record| {
        let message = crate::secrets::redact(&record.message,&secrets).replace('\r',"\\r").replace('\n',"\\n");
        text.push_str(&format!("{} {:<5} {:<6} {} {}\n",record.local_time,record.level,record.side,id,message));
        json!({"type":"log","at":record.local_time,"plugin":id,"side":record.side,"level":record.level,"message":message})
    }).collect();
    let mut output = offline::finish(records,0);
    if !options.json {output.stdout_text=text;}
    output
}
