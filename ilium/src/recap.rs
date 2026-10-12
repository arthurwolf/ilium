//! `ilium recap`: a short and a long factual recap for each selected pane.
//!
//! Selection reuses the `panes` and `broadcast` selectors. Agent panes are
//! recapped only from a transcript the server bound to the pane's session and
//! the pane's project; shell panes use bounded current screen text from a bounded selected-pane request. All inference runs through the shared `ilium_client::recap`
//! pipeline, so the CLI adds no extractor or provider of its own.
//!
//! Output is JSONL on stdout: one `recap` record per pane, a `warning` for
//! each failed or stale pane and each unreadable session, then one `summary`.
//! `--format markdown` prints the same result as a Markdown report.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Args, ValueEnum};
use directories::BaseDirs;
use ilium_agent_session::{TranscriptLocator, INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS};
use ilium_client::recap as pipeline;
use ilium_client::recap::{
    fence::{PaneFence, RecapCache},
    model::{RecapLength, RecapPaneState},
    render, FenceLookup, PaneRecapInput, RecapBatch, RecapContext, RecapOptions, RecapSource,
};
use ilium_core::AgentClass;
use ilium_inference::{provider_from_settings, InferenceProvider, InferenceSettings};
use serde_json::{json, Value};
use tokio::sync::watch;

use crate::pane_filter::{PaneFacts, PaneFilterArgs, PaneKind};
use crate::pane_scan::{self, CallerPane, PaneEvidence};
use crate::CliError;

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;

#[derive(Args, Debug)]
pub(crate) struct RecapArgs {
    #[command(flatten)]
    pub(crate) filter: PaneFilterArgs,

    /// Recap length to print. Both lengths are produced by one model call.
    #[arg(long, value_enum, default_value_t = LengthChoice::Both)]
    pub(crate) length: LengthChoice,

    /// Output format: JSONL records, or a Markdown report.
    #[arg(long, value_enum, default_value_t = FormatChoice::Jsonl)]
    pub(crate) format: FormatChoice,

    /// Overall limit for the run in seconds. Work still running at the limit
    /// is cancelled and reported as failed panes.
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = DEFAULT_TIMEOUT_SECONDS,
        value_parser = clap::value_parser!(u64).range(1..=600)
    )]
    pub(crate) timeout_seconds: u64,

    /// Panes recapped at the same time.
    #[arg(
        long,
        value_name = "COUNT",
        default_value_t = 4,
        value_parser = clap::value_parser!(u64).range(1..=8)
    )]
    pub(crate) max_concurrency: u64,

    /// Most panes recapped in one run; extra selected panes are reported.
    #[arg(
        long,
        value_name = "COUNT",
        default_value_t = 32,
        value_parser = clap::value_parser!(u64).range(1..=64)
    )]
    pub(crate) max_panes: u64,

    /// Also recap the pane this command runs in.
    #[arg(long)]
    pub(crate) include_self: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LengthChoice {
    Short,
    Long,
    Both,
}

impl LengthChoice {
    fn lengths(self) -> Vec<RecapLength> {
        match self {
            Self::Short => vec![RecapLength::Short],
            Self::Long => vec![RecapLength::Long],
            Self::Both => vec![RecapLength::Short, RecapLength::Long],
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormatChoice {
    Jsonl,
    Markdown,
}

pub(crate) async fn recap(args: RecapArgs, cwd: &Path) -> Result<(), CliError> {
    let filter = match args.filter.compile(cwd) {
        Ok(filter) => filter,
        Err(message) => {
            println!(
                "{}",
                json!({"type": "error", "command": "recap", "code": "invalid-selection", "message": message})
            );
            return Err(CliError::ExitStatus(2));
        }
    };
    let started = std::time::Instant::now();
    let run_limit = Duration::from_secs(args.timeout_seconds);
    let remaining = || run_limit.saturating_sub(started.elapsed());
    let collection_stop = interrupted(run_limit);
    tokio::pin!(collection_stop);
    let mut collection_cancelled = false;
    let lengths = args.length.lengths();
    let caller = CallerPane::from_env();
    let sockets = pane_scan::live_sockets()?;
    let home = BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());

    let mut warnings: Vec<Value> = Vec::new();
    let mut inputs: Vec<PaneRecapInput> = Vec::new();
    let mut project_roots: Vec<PathBuf> = Vec::new();
    let mut skipped_over_limit = 0_usize;
    for socket in &sockets {
        let attached_result = tokio::select! {
            biased;
            () = &mut collection_stop => { collection_cancelled = true; break; }
            result = pane_scan::attach_with_evidence(socket) => result,
        };
        let (mut attached, mut evidence) = match attached_result {
            Ok(pair) => pair,
            Err(message) => {
                warnings.push(unreadable_session_warning(&socket.socket_path, &message));
                continue;
            }
        };
        let mut selected = Vec::new();
        for pane in attached.panes(caller.as_ref()) {
            if !filter.accepts(&pane)
                || !matches!(
                    pane.kind,
                    PaneKind::Agent | PaneKind::Shell | PaneKind::UnavailableAgent
                )
            {
                continue;
            }
            if pane.is_self && !args.include_self {
                continue;
            }
            if (inputs.len() + selected.len()) as u64 >= args.max_panes {
                skipped_over_limit += 1;
                continue;
            }
            selected.push(pane);
        }
        let shell_ids: Vec<_> = selected
            .iter()
            .filter(|pane| {
                pane.kind == PaneKind::Shell
                    && project_directory(pane)
                        .is_some_and(|cwd| pipeline::custody::denied_path_reason(&cwd).is_none())
            })
            .map(|pane| pane.pane_id)
            .collect();
        tokio::select! {
            biased;
            () = &mut collection_stop => { collection_cancelled = true; }
            () = attached.collect_shell_screens(&shell_ids, &mut evidence) => {}
        }
        for pane in selected {
            if let Some(message) = evidence.screen_errors.get(&pane.pane_id) {
                warnings.push(json!({"type": "warning", "pane_id": pane.pane_id.0.to_string(), "message": message}));
            }
            if let Some(root) = project_directory(&pane) {
                project_roots.push(root);
            }
            inputs.push(recap_input(&pane, &evidence, home.as_deref()));
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), attached.detach()).await;
        if collection_cancelled {
            break;
        }
    }
    if collection_cancelled {
        warnings.push(json!({"type": "warning", "message": "context collection cancelled; returning partial results"}));
    }
    project_roots.sort();
    project_roots.dedup();
    if skipped_over_limit > 0 {
        warnings.push(json!({
            "type": "warning",
            "message": format!("pane limit of {} reached; {skipped_over_limit} selected panes not recapped", args.max_panes),
        }));
    }

    let captured: HashMap<String, PaneFence> = inputs
        .iter()
        .map(|input| (input.pane_id.clone(), input.fence.clone()))
        .collect();
    let mut batch = if inputs.is_empty() {
        RecapBatch {
            panes: Vec::new(),
            cancelled: false,
        }
    } else {
        match load_settings() {
            Err(message) => {
                println!(
                    "{}",
                    json!({"type": "error", "command": "recap", "code": "configuration", "message": message})
                );
                return Err(CliError::ExitStatus(1));
            }
            Ok(settings) if !settings.automatic_ai_allowed => pipeline::permission_denied(
                &inputs,
                "AI features are off or onboarding is not finished",
            ),
            Ok(settings) => {
                let provider: Arc<dyn InferenceProvider> =
                    Arc::from(provider_from_settings(&settings.inference));
                let (cancel_sender, cancel_receiver) = watch::channel(collection_cancelled);
                let context = recap_context(
                    provider,
                    cancel_receiver,
                    &lengths,
                    &args,
                    project_roots,
                    captured.clone(),
                );
                let mut batch = if collection_cancelled {
                    pipeline::run_recaps(inputs, context).await
                } else {
                    run_with_deadline(inputs, context, cancel_sender, remaining()).await
                };
                if batch
                    .panes
                    .iter()
                    .any(|pane| pane.state == RecapPaneState::Summarized)
                {
                    let latest = tokio::time::timeout(
                        remaining(),
                        current_fences(&sockets, caller.as_ref()),
                    )
                    .await
                    .unwrap_or_default();
                    pipeline::apply_final_fence(&mut batch, &captured, &|pane_id: &str| {
                        latest.get(pane_id).cloned()
                    });
                }
                batch
            }
        }
    };
    batch.cancelled |= collection_cancelled;
    batch
        .panes
        .sort_by(|left, right| left.pane_name.cmp(&right.pane_name));

    match args.format {
        FormatChoice::Jsonl => {
            for warning in &warnings {
                println!("{warning}");
            }
            for record in render::render_records(&batch, &lengths) {
                println!("{record}");
            }
        }
        FormatChoice::Markdown => {
            print!("{}", render::render_markdown(&batch, &lengths));
            for warning in &warnings {
                eprintln!("{}", warning["message"].as_str().unwrap_or("warning"));
            }
        }
    }
    Ok(())
}

/// Runs the pipeline and cancels it at the run limit or on Ctrl-C. Cancelled
/// panes come back labelled as failed; finished panes keep their text.
async fn run_with_deadline(
    inputs: Vec<PaneRecapInput>,
    context: RecapContext,
    cancel_sender: watch::Sender<bool>,
    timeout: Duration,
) -> RecapBatch {
    let run = pipeline::run_recaps(inputs, context);
    tokio::pin!(run);
    tokio::select! {
        batch = &mut run => batch,
        () = interrupted(timeout) => {
            let _ = cancel_sender.send(true);
            run.await
        }
    }
}

async fn interrupted(timeout: Duration) {
    tokio::select! {
        () = tokio::time::sleep(timeout) => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

fn recap_context(
    provider: Arc<dyn InferenceProvider>,
    cancel: watch::Receiver<bool>,
    lengths: &[RecapLength],
    args: &RecapArgs,
    project_roots: Vec<PathBuf>,
    captured: HashMap<String, PaneFence>,
) -> RecapContext {
    // The attach-time snapshot is the identity each pane was recapped under;
    // the final fence in `recap` compares it with the panes as they are after
    // inference.
    let fence_lookup: FenceLookup = Arc::new(move |pane_id: &str| captured.get(pane_id).cloned());
    RecapContext {
        provider,
        options: RecapOptions {
            lengths: lengths.to_vec(),
            max_concurrency: args.max_concurrency as usize,
            project_roots,
        },
        cancel,
        fence_lookup,
        cache: Arc::new(Mutex::new(RecapCache::default())),
    }
}

/// Fresh identity of every pane, read after inference for the final fence.
async fn current_fences(
    sockets: &[ilium::session::LiveSessionSocket],
    caller: Option<&CallerPane>,
) -> HashMap<String, PaneFence> {
    let mut fences = HashMap::new();
    for socket in sockets {
        let Ok((attached, evidence)) = pane_scan::attach_with_evidence(socket).await else {
            continue;
        };
        for pane in attached.panes(caller) {
            let pane_id = pane.pane_id.0.to_string();
            let session_id = evidence.session_ids.get(&pane.pane_id).cloned();
            fences.insert(
                pane_id.clone(),
                PaneFence {
                    pane_id,
                    session_id,
                },
            );
        }
        attached.detach().await;
    }
    fences
}

fn project_directory(pane: &PaneFacts) -> Option<PathBuf> {
    pane.project.clone().or_else(|| pane.cwd.clone())
}

/// Builds the evidence source for one pane. Agent evidence is bound here:
/// the transcript is used only when the session id the server reported
/// resolves to a transcript inside this pane's project.
fn recap_input(pane: &PaneFacts, evidence: &PaneEvidence, home: Option<&Path>) -> PaneRecapInput {
    let pane_id = pane.pane_id.0.to_string();
    let session_id = evidence.session_ids.get(&pane.pane_id).cloned();
    let directory = project_directory(pane).unwrap_or_default();
    let (source, fence_session) = match pane.kind {
        PaneKind::Agent | PaneKind::UnavailableAgent => {
            let class = agent_class(pane.agent.as_deref());
            let verified_transcript = match (pane.kind, session_id.as_deref(), home) {
                (PaneKind::Agent, Some(session_id), Some(home)) => TranscriptLocator::new_bounded(
                    home,
                    &directory,
                    INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS,
                )
                .transcript_for_session(&class, session_id)
                .map(|verified| verified.path),
                _ => None,
            };
            (
                RecapSource::Agent {
                    class,
                    verified_transcript,
                    project_dir: directory,
                    activity: activity_label(pane),
                },
                session_id,
            )
        }
        _ => (
            RecapSource::Shell {
                cwd: directory,
                screen_text: evidence.screen_text.get(&pane.pane_id).cloned(),
            },
            None,
        ),
    };
    PaneRecapInput {
        pane_id: pane_id.clone(),
        pane_name: pipeline::custody::redact_secrets(&pane.name).text,
        source,
        fence: PaneFence {
            pane_id,
            session_id: fence_session,
        },
    }
}

/// Maps the display name the server reports to a transcript reader class.
/// Unknown agents keep their name; the pipeline reports them as unavailable.
fn agent_class(agent: Option<&str>) -> AgentClass {
    let name = agent.unwrap_or_default().to_ascii_lowercase();
    if name.contains("claude") {
        AgentClass::Claude
    } else if name.contains("codex") {
        AgentClass::Codex
    } else if name.contains("antigravity") || name.contains("antimatter") || name == "agy" {
        AgentClass::Antigravity
    } else {
        AgentClass::Other(name)
    }
}

fn activity_label(pane: &PaneFacts) -> String {
    pane.state.map_or_else(
        || "none".to_string(),
        |state| format!("{state:?}").to_ascii_lowercase(),
    )
}

struct RecapSettings {
    inference: InferenceSettings,
    automatic_ai_allowed: bool,
}

/// Reads the same configuration the TUI reads: the inference settings and the
/// onboarding gate that decides whether automatic AI may run.
fn load_settings() -> Result<RecapSettings, String> {
    let directory = ilium_platform::paths::config_dir()
        .ok_or_else(|| "no configuration directory on this system".to_string())?;
    let config = ilium_client::config::load(&directory).map_err(|error| error.to_string())?;
    Ok(RecapSettings {
        automatic_ai_allowed: config.onboarding.automatic_ai_allowed(),
        inference: config.inference,
    })
}

fn unreadable_session_warning(socket_path: &Path, message: &str) -> Value {
    json!({
        "type": "warning",
        "command": "recap",
        "socket": socket_path.display().to_string(),
        "message": format!("session could not be read: {message}"),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ilium_core::NodeId;

    use super::*;

    fn facts(kind: PaneKind, agent: Option<&str>) -> PaneFacts {
        PaneFacts {
            session_name: "default".to_string(),
            pane_id: NodeId(7),
            name: "parser".to_string(),
            short_name: None,
            project: Some(PathBuf::from("/home/me/dev/ilium")),
            cwd: Some(PathBuf::from("/home/me/dev/ilium")),
            kind,
            agent: agent.map(str::to_string),
            state: None,
            is_self: false,
        }
    }

    #[test]
    fn agent_display_names_map_to_transcript_classes() {
        assert_eq!(agent_class(Some("Claude Code")), AgentClass::Claude);
        assert_eq!(agent_class(Some("Codex")), AgentClass::Codex);
        assert_eq!(agent_class(Some("Antigravity")), AgentClass::Antigravity);
        assert_eq!(
            agent_class(Some("Custom Tool")),
            AgentClass::Other("custom tool".to_string())
        );
        assert_eq!(agent_class(None), AgentClass::Other(String::new()));
    }

    #[test]
    fn agent_without_recorded_session_has_no_transcript() {
        let input = recap_input(
            &facts(PaneKind::Agent, Some("Claude")),
            &PaneEvidence::default(),
            Some(Path::new("/home/me")),
        );
        match input.source {
            RecapSource::Agent {
                verified_transcript,
                ..
            } => assert!(verified_transcript.is_none()),
            RecapSource::Shell { .. } => panic!("agent pane must use agent evidence"),
        }
        assert_eq!(input.fence.session_id, None);
    }

    #[test]
    fn unavailable_agent_never_resolves_or_reads_a_historical_transcript() {
        let mut evidence = PaneEvidence::default();
        evidence
            .session_ids
            .insert(NodeId(7), "retained-old-session".to_owned());
        let input = recap_input(
            &facts(PaneKind::UnavailableAgent, Some("claude")),
            &evidence,
            Some(Path::new("/nonexistent-agent-home")),
        );
        assert!(matches!(
            input.source,
            RecapSource::Agent {
                verified_transcript: None,
                ..
            }
        ));
    }

    #[test]
    fn shell_pane_screen_comes_from_its_selected_current_frame() {
        let mut evidence = PaneEvidence::default();
        evidence
            .screen_text
            .insert(NodeId(7), "$ cargo test\nok".to_owned());
        let input = recap_input(&facts(PaneKind::Shell, None), &evidence, None);
        match input.source {
            RecapSource::Shell { screen_text, .. } => {
                let text = screen_text.expect("selected frame gives screen text");
                assert!(text.contains("$ cargo test"));
                assert!(text.contains("ok"));
            }
            RecapSource::Agent { .. } => panic!("shell pane must use shell evidence"),
        }
    }

    #[test]
    fn shell_pane_without_current_frame_has_no_screen_text() {
        let input = recap_input(
            &facts(PaneKind::Shell, None),
            &PaneEvidence::default(),
            None,
        );
        match input.source {
            RecapSource::Shell { screen_text, .. } => assert!(screen_text.is_none()),
            RecapSource::Agent { .. } => panic!("shell pane must use shell evidence"),
        }
    }
}
