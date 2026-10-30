//! `ilium plugin dev` (reference-plugin-cli.md DEV-180..189, DEV-196).
//!
//! Installs the project as a dev copy (tier T3) into the attached session, rebuilds after
//! debounced source changes and reloads the running instances. Time is virtual: the `DevEvent`
//! script in `CliEnv` carries the changes, the plugin calls and the Ctrl-C, so the whole run is a
//! deterministic function of its inputs.
use super::{
    build_command, dev::DEBOUNCE, offline, CliEnv, CliOutput, DevEvent, InstallRecord,
    SessionBackend, SessionRequest, SessionResponse,
};
use ilium_plugin_api::grants::is_high_risk;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A timing record is printed this often (DEV-196).
const TIMING_INTERVAL_MS: u64 = 10_000;
/// The deadline reported for an export call (reference-limits.md default export deadline).
const EXPORT_DEADLINE_MS: u64 = 20;
const IGNORED_DIRECTORIES: [&str; 5] = ["dist", "target", "node_modules", ".ilium", ".git"];

struct Options {
    project: PathBuf,
    grant_all: bool,
    release: bool,
    replace_installed: bool,
    purge_on_exit: bool,
    timings: bool,
}

fn parse(args: &[String], env: &CliEnv) -> Result<Options, CliOutput> {
    let usage = |code: &str, message: &str| offline::failure(code, message, 2);
    let mut project = None;
    let mut options = Options {
        project: env.cwd.clone(),
        grant_all: false,
        release: false,
        replace_installed: false,
        purge_on_exit: false,
        timings: false,
    };
    for arg in args {
        match arg.as_str() {
            "--json" | "--quiet" | "--no-color" => {}
            "--grant-all" => options.grant_all = true,
            "--release" => options.release = true,
            "--replace-installed" => options.replace_installed = true,
            "--purge-on-exit" => options.purge_on_exit = true,
            "--timings" => options.timings = true,
            flag if flag.starts_with('-') => {
                return Err(usage("usage.unknown-flag", "Unsupported flag"));
            }
            _ if project.is_none() => project = Some(arg.clone()),
            _ => return Err(usage("usage.argument-invalid", "Unexpected argument")),
        }
    }
    if let Some(project) = project {
        options.project = env.cwd.join(project);
    }
    Ok(options)
}

/// What the manifest says about the plugin.
struct Manifest {
    id: String,
    version: String,
    sides: Vec<&'static str>,
    grants: Vec<String>,
}

fn read_manifest(project: &Path) -> Result<Manifest, (&'static str, String)> {
    let text = std::fs::read_to_string(project.join("manifest.json"))
        .map_err(|error| ("manifest.file-unreadable", error.to_string()))?;
    let manifest: Value =
        serde_json::from_str(&text).map_err(|error| ("manifest.schema", error.to_string()))?;
    let id = manifest["id"]
        .as_str()
        .ok_or(("manifest.schema", "the manifest has no id".to_owned()))?
        .to_owned();
    let version = manifest["version"].as_str().unwrap_or("0.0.0").to_owned();
    let sides = ["server", "client", "scene"]
        .into_iter()
        .filter(|side| manifest.get(*side).is_some_and(|value| !value.is_null()))
        .collect();
    let mut grants: Vec<String> = manifest["grants"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|grant| grant["id"].as_str().or_else(|| grant.as_str()))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if let Some(capabilities) = manifest["capabilities"].as_object() {
        for list in capabilities.values().filter_map(Value::as_array) {
            for grant in list.iter().filter_map(Value::as_str) {
                if !grants.iter().any(|known| known == grant) {
                    grants.push(grant.to_owned());
                }
            }
        }
    }
    Ok(Manifest {
        id,
        version,
        sides,
        grants,
    })
}

/// Whether a project-relative path is outside the watched set (DEV-185).
fn ignored(project: &Path, path: &str) -> bool {
    let path = path.trim_start_matches("./");
    let first = path.split('/').next().unwrap_or_default();
    if IGNORED_DIRECTORIES.contains(&first) {
        return true;
    }
    let Ok(text) = std::fs::read_to_string(project.join(".gitignore")) else {
        return false;
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .any(|pattern| {
            let pattern = pattern.trim_start_matches('/');
            let file_name = path.rsplit('/').next().unwrap_or(path);
            if let Some(directory) = pattern.strip_suffix('/') {
                return path.split('/').rev().skip(1).any(|part| part == directory)
                    || path.starts_with(&format!("{directory}/"));
            }
            match pattern.strip_prefix('*') {
                Some(suffix) => file_name.ends_with(suffix),
                None => path == pattern || file_name == pattern || path.starts_with(&format!("{pattern}/")),
            }
        })
}

fn percentile(sorted: &[u64], fraction: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// The whole run, as a state machine over virtual time.
struct Dev<'a> {
    options: &'a Options,
    env: &'a CliEnv,
    session: &'a dyn SessionBackend,
    manifest: Manifest,
    records: Vec<Value>,
    granted: Vec<String>,
    build_ms: u64,
    /// The finished-at time and result of the build in flight.
    in_flight: Option<(u64, Result<Vec<Value>, (&'static str, String)>)>,
    last_change: Option<u64>,
    clock: u64,
    queued: bool,
    next_tick: u64,
    window: BTreeMap<(String, String), Vec<u64>>,
}

impl Dev<'_> {
    fn request(&self, request: SessionRequest) -> Result<SessionResponse, String> {
        self.session.request(request)
    }

    fn start_build(&mut self, at: u64) {
        let result = build_command::build_for_dev(&self.options.project, self.options.release, self.env);
        self.in_flight = Some((at + self.build_ms, result));
    }

    /// Asks for the grants the manifest wants that the user has not given yet.
    fn review(&mut self) -> Result<(), (&'static str, String)> {
        let wanted: Vec<String> = self
            .manifest
            .grants
            .iter()
            .filter(|grant| !self.granted.contains(grant))
            .cloned()
            .collect();
        let (automatic, asked): (Vec<String>, Vec<String>) = if self.options.grant_all {
            wanted.into_iter().partition(|grant| !is_high_risk(grant))
        } else {
            (Vec::new(), wanted)
        };
        self.granted.extend(automatic);
        if asked.is_empty() {
            return Ok(());
        }
        match self.request(SessionRequest::Review {
            grants: asked.clone(),
        }) {
            Ok(SessionResponse::Decision(true)) => {
                self.granted.extend(asked);
                Ok(())
            }
            Ok(_) => Err(("dev.review-declined", "the grant review was declined".into())),
            Err(code) => Err(("dev.review-unavailable", code)),
        }
    }

    fn finish_build(&mut self, at: u64) {
        let Some((_, result)) = self.in_flight.take() else {
            return;
        };
        match result {
            Err((code, message)) => {
                self.records
                    .push(json!({"type":"error","code":code,"message":message}));
            }
            Ok(built) => {
                self.records.extend(built);
                if let Err((code, message)) = self.reload(at) {
                    self.records
                        .push(json!({"type":"error","code":code,"message":message}));
                }
            }
        }
    }

    fn reload(&mut self, _at: u64) -> Result<(), (&'static str, String)> {
        let manifest = read_manifest(&self.options.project)?;
        self.manifest = manifest;
        self.review()?;
        let clients = match self.request(SessionRequest::AttachedClients) {
            Ok(SessionResponse::Value(count)) => count.parse::<usize>().unwrap_or(0),
            _ => 0,
        };
        let mut targets: Vec<&str> = Vec::new();
        if self.manifest.sides.contains(&"server") {
            targets.push("server");
        }
        if self.manifest.sides.contains(&"client") {
            targets.extend(std::iter::repeat_n("client", clients));
        }
        for side in targets {
            let response = self
                .request(SessionRequest::Reload {
                    id: self.manifest.id.clone(),
                    side: side.to_owned(),
                })
                .map_err(|code| ("dev.reload-failed", code))?;
            let SessionResponse::Reloaded {
                restored_bytes,
                dropped,
            } = response
            else {
                return Err(("dev.reload-failed", "session.response-invalid".into()));
            };
            self.records.push(json!({"type":"reload","plugin":self.manifest.id,
                "side":side,"version":self.manifest.version,"restored_bytes":restored_bytes,
                "restore_dropped":dropped,"build_ms":self.build_ms}));
        }
        Ok(())
    }

    fn flush_timings(&mut self) {
        let window = std::mem::take(&mut self.window);
        for ((side, export), mut durations) in window {
            durations.sort_unstable();
            self.records.push(json!({"type":"timing","plugin":self.manifest.id,
                "side":side,"export":export,"calls":durations.len(),
                "p50_ms":percentile(&durations, 0.5),"p99_ms":percentile(&durations, 0.99),
                "max_ms":durations.last().copied().unwrap_or(0),
                "deadline_ms":EXPORT_DEADLINE_MS}));
        }
    }

    /// Lets virtual time run up to `until`: finishes builds, starts the debounced and the queued
    /// rebuild, and prints the timing records that fell due.
    fn advance(&mut self, until: u64) {
        loop {
            if let Some((end, _)) = &self.in_flight {
                let end = *end;
                if end > until {
                    break;
                }
                self.finish_build(end);
                if self.queued {
                    self.queued = false;
                    self.start_build(end);
                }
            } else if let Some(last) = self.last_change {
                let fire = last + DEBOUNCE.as_millis() as u64;
                if fire > until {
                    break;
                }
                self.last_change = None;
                self.start_build(fire);
            } else {
                break;
            }
        }
        while self.options.timings && self.next_tick <= until {
            self.flush_timings();
            self.next_tick += TIMING_INTERVAL_MS;
        }
    }

    fn event(&mut self, event: &DevEvent) {
        match event {
            DevEvent::Change { path, .. } => self.changed(path),
            DevEvent::WriteFile { path, bytes, .. } => {
                let target = self.options.project.join(path);
                let written = target
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| std::fs::write(&target, bytes));
                match written {
                    Ok(()) => self.changed(path),
                    Err(error) => self.records.push(json!({"type":"warning","code":"dev.write-failed",
                        "message":format!("could not write {path}: {error}")})),
                }
            }
            DevEvent::Log {
                at_ms,
                side,
                level,
                message,
            } => self.records.push(json!({"type":"log","at":at_ms,"plugin":self.manifest.id,
                "side":side,"level":level,"message":message})),
            DevEvent::Calls {
                side,
                export,
                duration_ms,
                ..
            } => self
                .window
                .entry((side.clone(), export.clone()))
                .or_default()
                .extend(duration_ms),
            DevEvent::Interrupt { .. } => {}
        }
    }

    fn changed(&mut self, path: &str) {
        if ignored(&self.options.project, path) {
            return;
        }
        let at = self.clock;
        if self.in_flight.is_some() {
            self.queued = true;
        } else {
            self.last_change = Some(at);
        }
    }
}

fn event_time(event: &DevEvent) -> u64 {
    match event {
        DevEvent::Change { at_ms, .. }
        | DevEvent::WriteFile { at_ms, .. }
        | DevEvent::Log { at_ms, .. }
        | DevEvent::Calls { at_ms, .. }
        | DevEvent::Interrupt { at_ms } => *at_ms,
    }
}

fn session_exit(code: &str) -> i32 {
    if code.starts_with("session.") {
        6
    } else {
        1
    }
}

/// Runs `ilium plugin dev`.
pub fn run(args: &[String], env: &CliEnv) -> CliOutput {
    let options = match parse(args, env) {
        Ok(options) => options,
        Err(output) => return output,
    };
    let Some(session) = env.session.as_ref() else {
        return offline::failure("session.unreachable", "No reachable session was supplied", 6);
    };
    if let Err(code) = session.request(SessionRequest::Connect) {
        return offline::failure(&code, "Session command failed", session_exit(&code));
    }
    let manifest = match read_manifest(&options.project) {
        Ok(manifest) => manifest,
        Err((code, message)) => return offline::failure(code, message, 1),
    };
    let id = manifest.id.clone();
    let mut prior_state = None;
    if let Ok(SessionResponse::Plugin(existing)) =
        session.request(SessionRequest::Describe { id: id.clone() })
    {
        if !existing.dev {
            if !options.replace_installed {
                return offline::failure(
                    "dev.id-installed",
                    format!("{id} is installed; pass --replace-installed to set it aside"),
                    1,
                );
            }
            if let Ok(SessionResponse::Value(state)) =
                session.request(SessionRequest::ReplaceInstalled { id: id.clone() })
            {
                prior_state = Some(state);
            }
        }
    }
    let mut dev = Dev {
        options: &options,
        env,
        session: session.as_ref(),
        manifest,
        records: Vec::new(),
        granted: Vec::new(),
        build_ms: env.build_duration_ms,
        in_flight: None,
        last_change: None,
        clock: env.build_duration_ms,
        queued: false,
        next_tick: TIMING_INTERVAL_MS,
        window: BTreeMap::new(),
    };
    let restore = |dev: &Dev<'_>| {
        if let Some(state) = &prior_state {
            let _ = dev.request(SessionRequest::RestoreReplaced {
                id: id.clone(),
                state: state.clone(),
            });
        }
    };
    let built = build_command::build_for_dev(&options.project, options.release, env);
    match built {
        Ok(records) => dev.records.extend(records),
        Err((code, message)) => {
            restore(&dev);
            return offline::failure(code, message, 1);
        }
    }
    let installed = dev.review().map_err(|(code, message)| (code.to_owned(), message)).and_then(|()| {
        dev.request(SessionRequest::RecordInstall {
            record: InstallRecord {
                id: id.clone(),
                version: dev.manifest.version.clone(),
                tier: "T3".into(),
                dev: true,
                data_dir: env.data_dir.join("dev-plugins").join(&id),
                grants: dev.granted.clone(),
            },
            state: "enabled".into(),
        })
        .and_then(|_| dev.request(SessionRequest::Enable { id: id.clone() }))
        .map_err(|code| (code, "Session command failed".to_owned()))
    });
    if let Err((code, message)) = installed {
        restore(&dev);
        let exit = session_exit(&code);
        return offline::failure(&code, message, exit);
    }
    let mut script: Vec<&DevEvent> = env.dev_script.iter().collect();
    script.sort_by_key(|event| event_time(event));
    for event in script {
        let at = event_time(event);
        dev.advance(at);
        dev.clock = at;
        if matches!(event, DevEvent::Interrupt { .. }) {
            break;
        }
        dev.event(event);
    }
    let _ = dev.request(SessionRequest::Disable { id: id.clone() });
    let _ = dev.request(SessionRequest::Uninstall {
        id: id.clone(),
        purge_data: options.purge_on_exit,
        purge_log: options.purge_on_exit,
    });
    restore(&dev);
    let records = std::mem::take(&mut dev.records);
    offline::finish(records, 130)
}
