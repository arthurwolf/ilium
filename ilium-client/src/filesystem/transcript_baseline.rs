//! Transcript evidence captured before a pane's ordered Enter is published.
//! Requests keep their original IPC debit while this finite I/O owner runs.
use crate::{
    app::{App, PaneRuntime, PendingLastPromptTranscriptCheck},
    ipc_preparation::AdmittedRequest,
};
use ilium_core::{AgentClass, NodeId};
use ilium_execution::{Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt};
use ilium_ipc::ClientRequest;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};

#[derive(Clone)]
struct Context {
    pane_id: NodeId,
    identity: Arc<()>,
    agent_class: AgentClass,
    session_id: String,
    project_path: PathBuf,
    home: PathBuf,
    submitted_after: chrono::DateTime<chrono::Utc>,
    prompt_epoch: String,
}
struct ReadBaseline(Arc<Context>);
struct Baseline {
    path: PathBuf,
    length: u64,
    #[cfg(test)]
    worker_thread: std::thread::ThreadId,
}
impl Job for ReadBaseline {
    type Output = Option<Baseline>;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, String> {
        if context.stop_requested() {
            return Err("Transcript baseline cancelled".into());
        }
        let locator = ilium_agent_session::TranscriptLocator::new_bounded(
            &self.0.home,
            &self.0.project_path,
            ilium_agent_session::TranscriptReadLimits {
                line_bytes: 64 * 1024,
                total_read_bytes: 8 * 1024 * 1024,
                scanned_entries: 4096,
                retained_path_bytes: 1024 * 1024,
            },
        );
        let transcript = locator.transcript_for_session(&self.0.agent_class, &self.0.session_id);
        if locator.read_limit_reached() {
            return Err("Transcript baseline scan exceeded bounded evidence limits".into());
        }
        if context.stop_requested() {
            return Err("Transcript baseline cancelled".into());
        }
        let Some(transcript) = transcript else {
            return Ok(None);
        };
        if !transcript.path.is_absolute() || transcript.path.capacity() > 64 * 1024 {
            return Err("Transcript baseline path exceeds retained limits".into());
        }
        let file = ilium_platform::secure_fs::open_regular_file(&transcript.path)
            .map_err(|error| format!("Transcript baseline: {error}"))?;
        let length = file
            .metadata()
            .map_err(|error| format!("Transcript baseline: {error}"))?
            .len();
        Ok(Some(Baseline {
            path: transcript.path,
            length,
            #[cfg(test)]
            worker_thread: std::thread::current().id(),
        }))
    }
}
enum State {
    Wanted,
    Reading(Receipt<ReadBaseline>),
    Ready(Result<Option<Baseline>, String>),
}
struct Envelope {
    request: AdmittedRequest,
    identity: Arc<()>,
    context: Option<Arc<Context>>,
    state: State,
    _result_hold: Option<ilium_execution::Retained<()>>,
}
pub(crate) struct BaselineFiles {
    client: Client,
    captured: HashMap<String, Arc<Context>>,
    panes: HashMap<NodeId, VecDeque<Envelope>>,
    active: usize,
    retiring: Vec<Receipt<ReadBaseline>>,
}
impl BaselineFiles {
    pub(crate) fn new(client: Client, ready: Arc<tokio::sync::Notify>) -> Self {
        Self {
            client: client.with_completion_wake(move || ready.notify_one()),
            captured: HashMap::new(),
            panes: HashMap::new(),
            active: 0,
            retiring: Vec::new(),
        }
    }
    pub(crate) fn pending(&self) -> bool {
        !self.panes.is_empty() || !self.captured.is_empty() || !self.retiring.is_empty()
    }
    pub(crate) fn needs_retry(&self) -> bool {
        self.active < 4
            && self.panes.values().any(|queue| {
                queue
                    .front()
                    .is_some_and(|head| matches!(head.state, State::Wanted))
            })
    }
    fn capture(&mut self, context: Context) -> Result<(), String> {
        if self.captured.contains_key(&context.prompt_epoch) {
            return Err("Transcript baseline epoch already admitted".into());
        }
        if self.captured.len()
            + self
                .panes
                .values()
                .map(|queue| queue.iter().filter(|entry| entry.context.is_some()).count())
                .sum::<usize>()
            >= 32
        {
            return Err(
                "Transcript baseline intent capacity reached; Enter was not admitted".into(),
            );
        }
        self.captured
            .insert(context.prompt_epoch.clone(), Arc::new(context));
        Ok(())
    }
    pub(crate) fn forget(&mut self, epoch: &str) {
        self.captured.remove(epoch);
    }
    fn hold(
        &mut self,
        request: AdmittedRequest,
        identity: Arc<()>,
    ) -> Result<(), Box<AdmittedRequest>> {
        let pane = match request.view() {
            ClientRequest::KeyInput { pane_id, .. }
            | ClientRequest::UserKeyInput { pane_id, .. }
            | ClientRequest::SubmitTerminalText { pane_id, .. }
            | ClientRequest::MouseInput { pane_id, .. } => *pane_id,
            _ => return Err(Box::new(request)),
        };
        let context = match request.view() {
            ClientRequest::UserKeyInput {
                prompt_epoch: Some(epoch),
                ..
            } => self.captured.remove(epoch),
            _ => None,
        };
        if context.is_none() && !self.panes.contains_key(&pane) {
            return Err(Box::new(request));
        }
        // Every envelope retains its already-admitted IPC debit. The single
        // outbound tenant bounds all transit/held/published requests together
        // to 32 jobs and 128 MiB + 64 KiB, rather than granting another queue.
        let state = if context.is_some() {
            State::Wanted
        } else {
            State::Ready(Ok(None))
        };
        self.panes.entry(pane).or_default().push_back(Envelope {
            request,
            identity,
            context,
            state,
            _result_hold: None,
        });
        Ok(())
    }
    fn retain_live(&mut self, live: impl Fn(NodeId, &Arc<()>) -> bool) -> usize {
        self.captured
            .retain(|_, context| live(context.pane_id, &context.identity));
        let mut removed = 0;
        let retiring = &mut self.retiring;
        self.panes.retain(|pane, queue| {
            queue.retain_mut(|entry| {
                if live(*pane, &entry.identity)
                    && entry
                        .context
                        .as_ref()
                        .is_none_or(|context| Arc::ptr_eq(&context.identity, &entry.identity))
                {
                    return true;
                }
                if let State::Reading(receipt) =
                    std::mem::replace(&mut entry.state, State::Ready(Ok(None)))
                {
                    receipt.cancel();
                    retiring.push(receipt);
                }
                removed += 1;
                false
            });
            !queue.is_empty()
        });
        removed
    }
    fn poll(&mut self) -> Option<(NodeId, Envelope)> {
        self.retiring
            .retain_mut(|receipt| match receipt.try_take() {
                JobPoll::Pending => true,
                _ => {
                    self.active -= 1;
                    false
                }
            });
        let panes: Vec<_> = self.panes.keys().copied().collect();
        for pane in panes {
            let queue = self.panes.get_mut(&pane)?;
            let head = queue.front_mut()?;
            if matches!(head.state, State::Wanted) && self.active < 4 {
                if let Some(context) = &head.context {
                    match self.client.try_submit(
                        Lane::Io,
                        JobCost {
                            input_bytes: 16 * 1024 * 1024,
                            result_bytes: 256 * 1024,
                        },
                        ReadBaseline(Arc::clone(context)),
                    ) {
                        Ok(receipt) => {
                            self.active += 1;
                            head.state = State::Reading(receipt);
                        }
                        Err(rejected) => {
                            if !matches!(
                                rejected.reason,
                                ilium_execution::RejectReason::Busy
                                    | ilium_execution::RejectReason::QueueFull
                                    | ilium_execution::RejectReason::JobLimit
                                    | ilium_execution::RejectReason::InputBytes
                                    | ilium_execution::RejectReason::ResultBytes
                            ) {
                                head.state = State::Ready(Err(format!(
                                    "Transcript baseline unavailable: {:?}",
                                    rejected.reason
                                )));
                            }
                        }
                    }
                }
            }
            if let State::Reading(receipt) = &mut head.state {
                match receipt.try_take() {
                    JobPoll::Pending => {}
                    JobPoll::Ready(outcome) => {
                        let mut result = None;
                        let hold = outcome.map(|outcome| {
                            result = Some(match outcome {
                                JobOutcome::Finished(result) => result,
                                _ => Err(
                                    "Transcript baseline worker stopped before evidence completion"
                                        .into(),
                                ),
                            })
                        });
                        self.active -= 1;
                        head._result_hold = Some(hold);
                        head.state =
                            State::Ready(result.unwrap_or_else(|| {
                                Err("Transcript baseline result missing".into())
                            }));
                    }
                    JobPoll::Lost | JobPoll::Taken => {
                        self.active -= 1;
                        head.state = State::Ready(Err("Transcript baseline receipt lost".into()));
                    }
                }
            }
            if matches!(head.state, State::Ready(_)) {
                let entry = queue.pop_front()?;
                if queue.is_empty() {
                    self.panes.remove(&pane);
                }
                return Some((pane, entry));
            }
        }
        None
    }
}

impl Drop for BaselineFiles {
    fn drop(&mut self) {
        for queue in self.panes.values() {
            for entry in queue {
                if let State::Reading(receipt) = &entry.state {
                    receipt.cancel();
                }
            }
        }
    }
}
impl App {
    pub(crate) fn capture_terminal_baseline(
        &mut self,
        pane: NodeId,
        epoch: &str,
        submitted_after: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, String> {
        let Some(files) = &mut self.terminal_baselines else {
            return Ok(false);
        };
        let Some(PaneRuntime::Terminal(view)) = self.panes.get(&pane) else {
            return Ok(false);
        };
        let Some(agent) = self.tree.get(pane).and_then(|node| match &node.kind {
            ilium_core::NodeKind::Pane { status, .. } => status.agent_state(),
            _ => None,
        }) else {
            return Ok(false);
        };
        if !matches!(agent.class, AgentClass::Claude | AgentClass::Codex) {
            return Ok(false);
        }
        let Some(session) = self.agent_session_ids.get(&pane) else {
            return Ok(false);
        };
        let project = self.tree.pane_cwd(pane).unwrap_or(&self.session_cwd);
        let Some(home) = directories::BaseDirs::new() else {
            return Ok(false);
        };
        if session.capacity() > 64 * 1024
            || project.as_os_str().len() > 64 * 1024
            || home.home_dir().as_os_str().len() > 64 * 1024
            || epoch.len() > 128
        {
            return Err(
                "Transcript baseline context exceeds retained limits; Enter was not admitted"
                    .into(),
            );
        }
        files.capture(Context {
            pane_id: pane,
            identity: Arc::clone(&view.identity),
            agent_class: agent.class.clone(),
            session_id: session.clone(),
            project_path: project.to_path_buf(),
            home: home.home_dir().to_path_buf(),
            submitted_after,
            prompt_epoch: epoch.to_owned(),
        })?;
        Ok(true)
    }
    pub(crate) fn try_hold_terminal_request_after_baseline(
        &mut self,
        request: AdmittedRequest,
    ) -> Result<(), Box<AdmittedRequest>> {
        let pane = match request.view() {
            ClientRequest::KeyInput { pane_id, .. }
            | ClientRequest::UserKeyInput { pane_id, .. }
            | ClientRequest::SubmitTerminalText { pane_id, .. }
            | ClientRequest::MouseInput { pane_id, .. } => *pane_id,
            _ => return Err(Box::new(request)),
        };
        let Some(PaneRuntime::Terminal(view)) = self.panes.get(&pane) else {
            return Err(Box::new(request));
        };
        let identity = Arc::clone(&view.identity);
        let Some(files) = &mut self.terminal_baselines else {
            return Err(Box::new(request));
        };
        files.hold(request, identity)
    }
    pub(crate) fn collect_terminal_baselines(&mut self) -> bool {
        if self.pending_last_prompt_transcript_checks.len() >= 32 {
            return false;
        }
        let Some(mut files) = self.terminal_baselines.take() else {
            return false;
        };
        let panes = &self.panes;
        let removed = files.retain_live(|pane, identity| matches!(panes.get(&pane), Some(PaneRuntime::Terminal(view)) if Arc::ptr_eq(&view.identity, identity)));
        if removed != 0 {
            self.status_message = Some(format!(
                "Cancelled {removed} pending terminal inputs for removed or replaced panes"
            ));
        }
        let mut changed = removed != 0;
        while self.pending_last_prompt_transcript_checks.len() < 32 {
            let Some((pane, envelope)) = files.poll() else {
                break;
            };
            changed = true;
            let live = matches!(self.panes.get(&pane), Some(PaneRuntime::Terminal(view)) if Arc::ptr_eq(&view.identity, &envelope.identity));
            if !live {
                self.status_message = Some(
                    "Pending terminal input cancelled because its pane was replaced or removed"
                        .into(),
                );
                continue;
            }
            if let Some(context) = envelope.context {
                let current = self.last_prompt_transcript_context(pane).is_some_and(
                    |(class, session, path)| {
                        class == context.agent_class
                            && session == context.session_id
                            && path == context.project_path
                    },
                );
                match envelope.state {
                    State::Ready(Ok(Some(baseline))) if current => self
                        .pending_last_prompt_transcript_checks
                        .push(PendingLastPromptTranscriptCheck {
                            pane_id: context.pane_id,
                            agent_class: context.agent_class.clone(),
                            session_id: context.session_id.clone(),
                            project_path: context.project_path.clone(),
                            verified_path: baseline.path,
                            baseline_length: baseline.length,
                            submitted_after: context.submitted_after,
                            prompt_epoch: context.prompt_epoch.clone(),
                        }),
                    State::Ready(Err(error)) => {
                        self.status_message = Some(format!(
                            "Input forwarded without exact transcript evidence: {error}"
                        ));
                    }
                    _ => {}
                }
            }
            self.publish_terminal_request_after_baseline(envelope.request);
        }
        self.terminal_baselines = Some(files);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    use std::time::{Duration, Instant};

    fn execution() -> (Execution, Client, Client) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 8,
            service_jobs: 0,
            input_bytes: 64 * 1024 * 1024,
            result_bytes: 2 * 1024 * 1024,
            worker_threads: 1,
            // Includes all three bank metadata blocks as well as IO resident storage.
            worker_bytes: 64 * 1024,
        });
        let lane = |threads| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 4 },
            priority: None,
            resident_bytes_per_thread: if threads == 0 { 0 } else { 4096 },
        };
        let execution = Execution::start(
            quota,
            ExecutionConfig {
                cpu: lane(0),
                io: lane(1),
                service: lane(0),
            },
        )
        .unwrap();
        let filesystem = execution
            .client(ClientLimits {
                jobs: 1,
                service_jobs: 0,
                input_bytes: 32 * 1024 * 1024,
                result_bytes: 1024 * 1024,
            })
            .unwrap();
        let outbound = execution
            .client(crate::ipc_preparation::request_limits())
            .unwrap();
        (execution, filesystem, outbound)
    }
    fn context(home: &std::path::Path, project: &std::path::Path, identity: Arc<()>) -> Context {
        Context {
            pane_id: NodeId(71),
            identity,
            agent_class: AgentClass::Codex,
            session_id: "22222222-2222-4222-8222-222222222222".into(),
            project_path: project.to_path_buf(),
            home: home.to_path_buf(),
            submitted_after: chrono::Utc::now(),
            prompt_epoch: "synthetic-enter-epoch".into(),
        }
    }
    #[test]
    fn baseline_admission_is_nonblocking_and_later_bytes_remain_ordered() {
        let (mut execution, client, outbound) = execution();
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let project = directory.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let identity = Arc::new(());
        let context = context(&home, &project, Arc::clone(&identity));
        let path = home.join(".codex/sessions/2026/10/03").join(format!(
            "rollout-2026-10-03T00-00-00-{}.jsonl",
            context.session_id
        ));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = serde_json::json!({"type":"session_meta","payload":{"id":context.session_id,"cwd":project}}).to_string() + "\n";
        std::fs::write(&path, &original).unwrap();
        let held = client
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes: 1,
                    result_bytes: 1,
                },
            )
            .unwrap();
        let mut files = BaselineFiles::new(client, Arc::new(tokio::sync::Notify::new()));
        files.capture(context).unwrap();
        let requests = [
            ClientRequest::UserKeyInput {
                pane_id: NodeId(71),
                bytes: vec![b'\r'],
                submission: Some(ilium_ipc::PromptSubmissionSource::Keyboard),
                prompt_epoch: Some("synthetic-enter-epoch".into()),
            },
            ClientRequest::KeyInput {
                pane_id: NodeId(71),
                bytes: b"later bytes".to_vec(),
                submission: None,
            },
            ClientRequest::SubmitTerminalText {
                pane_id: NodeId(71),
                text: "later semantic submission".into(),
                source: ilium_ipc::PromptSubmissionSource::ToolbarAction,
            },
        ];
        let expected = requests
            .iter()
            .map(|request| format!("{request:?}"))
            .collect::<Vec<_>>();
        let started = Instant::now();
        for request in requests {
            let request = crate::ipc_preparation::admit_request(&outbound, request).unwrap();
            assert!(files.hold(request, Arc::clone(&identity)).is_ok());
        }
        assert!(files.poll().is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(files.pending());
        drop(held);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut emitted = Vec::new();
        while emitted.len() != 3 {
            if let Some((_, entry)) = files.poll() {
                if emitted.is_empty() {
                    let State::Ready(Ok(Some(baseline))) = entry.state else {
                        panic!("verified baseline missing")
                    };
                    assert_eq!(baseline.path, path);
                    assert_eq!(baseline.length, original.len() as u64);
                    assert_ne!(baseline.worker_thread, std::thread::current().id());
                }
                emitted.push(format!("{:?}", entry.request.view()));
            }
            assert!(
                Instant::now() < deadline,
                "baseline input FIFO did not finish"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(emitted, expected);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(!files.pending());
        drop(files);
        drop(outbound);
        execution.request_shutdown(ShutdownMode::Drain);
        assert_eq!(
            execution
                .join_until_background(Instant::now() + Duration::from_secs(3))
                .unwrap()
                .remaining_workers,
            0
        );
    }
    #[test]
    fn replaced_identity_cancels_exact_accepted_input_without_crediting_a_new_pane() {
        let (mut execution, client, outbound) = execution();
        let directory = tempfile::tempdir().unwrap();
        let identity = Arc::new(());
        let mut files = BaselineFiles::new(client, Arc::new(tokio::sync::Notify::new()));
        files
            .capture(context(
                directory.path(),
                directory.path(),
                Arc::clone(&identity),
            ))
            .unwrap();
        let request = crate::ipc_preparation::admit_request(
            &outbound,
            ClientRequest::UserKeyInput {
                pane_id: NodeId(71),
                bytes: vec![b'\r'],
                submission: Some(ilium_ipc::PromptSubmissionSource::Keyboard),
                prompt_epoch: Some("synthetic-enter-epoch".into()),
            },
        )
        .unwrap();
        assert!(files.hold(request, Arc::clone(&identity)).is_ok());
        let replacement = Arc::new(());
        assert_eq!(
            files.retain_live(|_, old| Arc::ptr_eq(old, &replacement)),
            1
        );
        assert!(!files.pending());
        assert!(files.poll().is_none());
        drop(files);
        drop(outbound);
        execution.request_shutdown(ShutdownMode::Drain);
        assert_eq!(
            execution
                .join_until_background(Instant::now() + Duration::from_secs(3))
                .unwrap()
                .remaining_workers,
            0
        );
    }
}
