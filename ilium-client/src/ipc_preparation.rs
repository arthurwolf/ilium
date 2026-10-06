//! Ordered codec work on real CPU banks; socket tasks retain transport ownership.
use ilium_execution::{
    Client, ClientLimits, Execution, ExecutionConfig, Job, JobCost, JobOutcome, JobPoll, Lane,
    LaneConfig, Receipt, RejectReason, Reservation, Retained,
};
use ilium_ipc::IpcError;
use std::{io, sync::Arc};
use tokio::sync::Notify;

const MIB: usize = 1024 * 1024;
pub(crate) const CODEC_COST: JobCost = JobCost {
    input_bytes: 128 * MIB,
    result_bytes: 128 * MIB,
};

#[derive(Clone)]
pub(crate) struct IpcPreparation {
    decoder: Client,
    encoder: Client,
    outbound: Client,
    decoded: Arc<DecodedStorage>,
    completed: Arc<Notify>,
}
impl IpcPreparation {
    pub(crate) fn new(decoder: Client, encoder: Client, outbound: Client) -> Self {
        let completed = crate::execution::admission_notification();
        let wake = Arc::clone(&completed);
        Self {
            decoder: decoder.with_completion_wake(move || wake.notify_waiters()),
            encoder: encoder.with_completion_wake({
                let wake = Arc::clone(&completed);
                move || wake.notify_waiters()
            }),
            outbound,
            decoded: Arc::new(DecodedStorage::default()),
            completed,
        }
    }

    /// One-shot CLI connections own their codec bank explicitly. Interactive
    /// clients inject a tenant of their existing process execution owner.
    pub(crate) fn standalone() -> Result<(Execution, Self), IpcError> {
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let owner = Execution::start(
            crate::execution::process_quota(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 2,
                    priority: Some(ilium_execution::WorkerPriority::BelowNormal),
                    resident_bytes_per_thread: 2 * MIB,
                },
                io: disabled,
                service: disabled,
            },
        )
        .map_err(|error| IpcError::Io(io::Error::other(error)))?;
        let decoder_group =
            crate::execution::codec_admission_group(crate::execution::CodecDirection::Decoder)
                .map_err(rejected)?;
        let encoder_group =
            crate::execution::codec_admission_group(crate::execution::CodecDirection::Encoder)
                .map_err(rejected)?;
        let decoder = owner
            .client_in_group(&decoder_group, codec_limits())
            .map_err(rejected)?;
        let encoder = owner
            .client_in_group(&encoder_group, codec_limits())
            .map_err(rejected)?;
        let aggregate = crate::execution::general_admission_group().map_err(rejected)?;
        let outbound = owner
            .client_in_group(&aggregate, request_limits())
            .map_err(rejected)?;
        Ok((owner, Self::new(decoder, encoder, outbound)))
    }

    pub(crate) fn outbound_client(&self) -> Client {
        self.outbound.clone()
    }

    pub(crate) async fn reserve_decoder(&self) -> Result<Reservation, IpcError> {
        self.reserve(&self.decoder).await
    }

    pub(crate) async fn reserve_encoder(&self) -> Result<Reservation, IpcError> {
        self.reserve(&self.encoder).await
    }

    async fn reserve(&self, client: &Client) -> Result<Reservation, IpcError> {
        loop {
            let notified = self.completed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match client.try_reserve(Lane::Cpu, CODEC_COST) {
                Ok(reservation) => return Ok(reservation),
                Err(RejectReason::Busy) => {
                    // A short admission mutex can unlock without releasing a
                    // debit. Yield via the timer rather than spinning the loop.
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
                Err(
                    RejectReason::QueueFull
                    | RejectReason::JobLimit
                    | RejectReason::InputBytes
                    | RejectReason::ResultBytes,
                ) => notified.await,
                Err(reason) => return Err(rejected(reason)),
            }
        }
    }

    pub(crate) async fn run_reserved<J: Job<Error = IpcError>>(
        &self,
        reservation: Reservation,
        job: J,
    ) -> Result<Retained<J::Output>, IpcError> {
        let mut receipt = CancelReceipt(
            reservation
                .submit(job)
                .map_err(|error| rejected(error.reason))?,
        );
        loop {
            let notified = self.completed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match receipt.0.try_take() {
                JobPoll::Pending => notified.await,
                JobPoll::Ready(outcome) => match outcome.view() {
                    JobOutcome::Finished(Ok(_)) => {
                        return Ok(outcome.map(|outcome| match outcome {
                            JobOutcome::Finished(Ok(output)) => output,
                            _ => unreachable!("exclusive validated codec outcome"),
                        }))
                    }
                    JobOutcome::Finished(Err(_)) => {
                        let error = outcome.map(|outcome| match outcome {
                            JobOutcome::Finished(Err(error)) => error,
                            _ => unreachable!("exclusive validated codec error"),
                        });
                        return Err(IpcError::Io(io::Error::other(ChargedCodecError(error))));
                    }
                    JobOutcome::NotStarted { .. } => {
                        return Err(IpcError::Io(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "codec cancelled before execution",
                        )))
                    }
                    JobOutcome::Panicked => {
                        return Err(IpcError::Io(io::Error::other("codec worker panicked")))
                    }
                },
                JobPoll::Lost | JobPoll::Taken => {
                    return Err(IpcError::Io(io::Error::other("codec completion lost")))
                }
            }
        }
    }
}

pub(crate) fn codec_limits() -> ClientLimits {
    ClientLimits {
        jobs: 1,
        service_jobs: 0,
        input_bytes: 128 * MIB,
        result_bytes: 128 * MIB,
    }
}
fn rejected(reason: RejectReason) -> IpcError {
    IpcError::Io(io::Error::other(format!("codec admission: {reason:?}")))
}
struct CancelReceipt<J: Job>(Receipt<J>);
impl<J: Job> Drop for CancelReceipt<J> {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
struct ChargedCodecError(Retained<IpcError>);
impl std::fmt::Debug for ChargedCodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.0.view(), f)
    }
}
impl std::fmt::Display for ChargedCodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.0.view(), f)
    }
}
impl std::error::Error for ChargedCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.view())
    }
}

/// The producer still owns the original on refusal. Count allocation capacity,
/// not string length or encoded length; cap traversal so admission itself stays
/// bounded even for a malformed deeply nested model proposal.
pub(crate) fn request_retained_bytes(request: &ilium_ipc::ClientRequest) -> usize {
    use ilium_ipc::ClientRequest as R;
    let mut count = CapacityCount {
        bytes: std::mem::size_of::<R>(),
        visits: 0,
    };
    match request {
        R::Attach { session } | R::AttachInteractive { session } => count.string(session),
        R::KeyInput { bytes, .. } => count.add(bytes.capacity()),
        R::UserKeyInput {
            bytes,
            prompt_epoch,
            ..
        } => {
            count.add(bytes.capacity());
            count.optional_string(prompt_epoch);
        }
        R::SubmitTerminalText { text, .. }
        | R::SchedulePaneInput { text, .. }
        | R::EnqueuePrompt { text, .. } => count.string(text),
        R::RenameNode {
            title,
            short_title,
            inferred_icon,
            ..
        }
        | R::SetAutomaticPaneTitle {
            title,
            short_title,
            inferred_icon,
            ..
        } => {
            count.string(title);
            count.optional_string(short_title);
            count.optional_string(inferred_icon);
        }
        R::SetSessionPaneTitle {
            expected_session_id,
            title,
            short_title,
            inferred_icon,
            ..
        } => {
            count.string(expected_session_id);
            count.string(title);
            count.optional_string(short_title);
            count.optional_string(inferred_icon);
        }
        R::NewGroup { name, .. } => count.string(name),
        R::NewFolder { path, .. }
        | R::NewProject { path }
        | R::ChangeProjectFolder { path, .. } => count.path(path),
        R::NewPane { kind, .. } => match kind {
            ilium_ipc::NewPaneKind::PlainShell => {}
            ilium_ipc::NewPaneKind::Command(command) => count.string(command),
            ilium_ipc::NewPaneKind::Editor(path) => count.path(path),
            ilium_ipc::NewPaneKind::CommandWithInitialInput {
                command_line,
                initial_input,
            } => {
                count.string(command_line);
                count.string(initial_input);
            }
        },
        R::NewBoard { name, storage, .. } => {
            count.string(name);
            match storage {
                ilium_core::BoardStorage::Folder { path }
                | ilium_core::BoardStorage::MarkdownFile { path } => count.path(path),
            }
        }
        R::CreateSplitView { name, pane_ids, .. } => {
            count.string(name);
            count.vector(pane_ids);
        }
        R::SetVisiblePanes { pane_ids } => count.vector(pane_ids),
        R::DiscardTerminalDelivery { pane_ids } => count.vector(pane_ids),
        R::UpdateSoundSettings { settings } | R::PreviewSoundSettings { settings } => {
            if let Some(path) = &settings.file {
                count.path(path);
            }
        }
        R::PreviewSound { file, .. } => {
            if let Some(path) = file {
                count.path(path);
            }
        }
        R::ReportLastPromptFromTranscript {
            expected_session_id,
            last_prompt,
            ..
        } => {
            count.string(expected_session_id);
            count.string(last_prompt);
        }
        R::ReportAgentPromptFromTranscript {
            expected_session_id,
            prompt_epoch,
            last_prompt,
            ..
        } => {
            count.string(expected_session_id);
            count.string(prompt_epoch);
            count.string(last_prompt);
        }
        R::CheckPaneProgressMonitor { command, .. } | R::SetPaneProgressMonitor { command, .. } => {
            count.string(command)
        }
        R::ReplacePaneWithCommand { command_line, .. } => count.string(command_line),
        R::SubmitVoiceText { sentences, .. } => {
            count.vector(sentences);
            for text in sentences {
                if !count.visit() {
                    break;
                }
                count.string(text);
            }
        }
        R::AnswerVoiceText { result, .. } => {
            if let Err(error) = result {
                count.string(&error.message);
            }
        }
        R::RemoveWorkspace { force_path, .. } => {
            if let Some(path) = force_path {
                count.path(path);
            }
        }
        R::CreateAgentInWorkspace {
            spec,
            initial_input,
            ..
        } => {
            use ilium_ipc::WorkspaceCreateSpec as S;
            match spec {
                S::New {
                    branch,
                    base_ref,
                    path,
                } => {
                    count.string(branch);
                    count.string(base_ref);
                    count.path(path);
                }
                S::Existing { path } | S::ExistingWithOptions { path, .. } => count.path(path),
                S::NewAtDefaultPath { branch, base_ref } => {
                    count.string(branch);
                    count.optional_string(base_ref);
                }
                S::NewWithSetup {
                    branch,
                    base_ref,
                    path,
                    setup_command,
                }
                | S::NewWithOptions {
                    branch,
                    base_ref,
                    path,
                    setup_command,
                    ..
                } => {
                    count.string(branch);
                    count.string(base_ref);
                    count.path(path);
                    count.string(setup_command);
                }
                S::NewAtDefaultPathWithSetup {
                    branch,
                    base_ref,
                    setup_command,
                } => {
                    count.string(branch);
                    count.optional_string(base_ref);
                    count.string(setup_command);
                }
            }
            count.optional_string(initial_input);
        }
        R::PruneWorkspace { target, mode, .. } => {
            count.path(&target.repo_common_dir);
            count.path(&target.worktree_root);
            count.path(&target.metadata_directory);
            count.string(&target.workspace_id);
            count.string(&target.creation_branch);
            count.string(&target.base_ref);
            count.string(&target.base_commit);
            count.string(&target.expected_head);
            if let ilium_ipc::WorkspacePruneMode::DiscardFiles { confirmed_path } = mode {
                count.path(confirmed_path);
            }
        }
        R::UpdateAgentDetectionSettings { settings, .. } => {
            count.vector(&settings.custom_signatures);
            for signature in &settings.custom_signatures {
                if !count.visit() {
                    break;
                }
                count.string(&signature.name_substring);
                count.class(&signature.class);
            }
        }
        R::UpdateTextTriggers { settings } => {
            count.vector(&settings.triggers);
            for trigger in &settings.triggers {
                if !count.visit() {
                    break;
                }
                count.string(&trigger.id);
                count.string(&trigger.regexp);
                count.string(&trigger.message);
                count.string(&trigger.sample_text);
            }
        }
        R::RecordAgentDebugEvent {
            expected_session_id,
            event,
            ..
        } => {
            count.optional_string(expected_session_id);
            count.string(&event.summary);
            count.optional_string(&event.correlation_id);
            count.vector(&event.fields);
            for field in &event.fields {
                if !count.visit() {
                    break;
                }
                count.string(&field.label);
                count.string(&field.value);
            }
        }
        R::ApplyRestructurePlan {
            plan,
            title_observations,
        } => {
            count.nodes(&plan.children, 0);
            count.observations(title_observations);
        }
        R::ApplyProjectRestructurePlan {
            plan,
            title_observations,
            inference_activity_revisions,
            ..
        } => {
            count.nodes(&plan.children, 0);
            count.observations(title_observations);
            count.vector(inference_activity_revisions);
        }
        R::ApplyRecommendedProjectRestructurePlan {
            plan,
            title_observations,
            inference_activity_revisions,
            ..
        } => {
            count.nodes(&plan.structure.children, 0);
            count.observations(title_observations);
            count.vector(inference_activity_revisions);
            count.recommendation(&plan.project);
            count.vector(&plan.entries);
            for entry in &plan.entries {
                if !count.visit() {
                    break;
                }
                count.vector(&entry.path);
                count.recommendation(&entry.recommendation);
            }
        }
        R::ClosePane { .. }
        | R::MoveNode { .. }
        | R::ResizePane { .. }
        | R::MouseInput { .. }
        | R::Detach
        | R::KillSession
        | R::ReparentNode { .. }
        | R::SetPaneFocus { .. }
        | R::RestartServer
        | R::ClearPromptQueue { .. }
        | R::RevertLastRestructure
        | R::RevertProjectRestructure { .. }
        | R::ResolveSessionRecovery { .. }
        | R::UpdateDebugLogging { .. }
        | R::UpdateAgentDebugMenu { .. }
        | R::GetPaneDebugLog { .. }
        | R::SetNodeBookmarked { .. }
        | R::RecordNodeActivity { .. }
        | R::SetNodeExpanded { .. }
        | R::SetNodeLockedClosed { .. }
        | R::GetPaneProgressMonitorStatus { .. }
        | R::ClearPaneProgressMonitor { .. }
        | R::UpdateProgressMonitorEnabled { .. }
        | R::RegisterVoiceTextReceiver
        | R::QueryRepoFacts { .. }
        | R::RefreshPaneGitStatus { .. }
        | R::ClosePaneWithWorkspaceDisposition { .. }
        | R::QueryWorkspaceInventory { .. }
        | R::QueryWorkspaceCloseOffer { .. }
        | R::TerminatePaneProcess { .. }
        | R::FreezePane { .. } => {}
    }
    count.bytes
}

struct CapacityCount {
    bytes: usize,
    visits: usize,
}
impl CapacityCount {
    fn add(&mut self, bytes: usize) {
        self.bytes = self.bytes.saturating_add(bytes);
    }
    fn string(&mut self, value: &String) {
        self.add(value.capacity());
    }
    fn optional_string(&mut self, value: &Option<String>) {
        if let Some(value) = value {
            self.string(value);
        }
    }
    fn path(&mut self, value: &std::path::PathBuf) {
        self.add(value.capacity());
    }
    fn vector<T>(&mut self, value: &Vec<T>) {
        self.add(value.capacity().saturating_mul(std::mem::size_of::<T>()));
    }
    fn visit(&mut self) -> bool {
        self.visits += 1;
        if self.visits > 4096 || self.bytes > 64 * MIB {
            self.bytes = usize::MAX;
            false
        } else {
            true
        }
    }
    fn class(&mut self, class: &ilium_core::AgentClass) {
        if let ilium_core::AgentClass::Other(name) = class {
            self.string(name);
        }
    }
    fn observations(&mut self, observations: &Vec<ilium_ipc::PaneTitleObservation>) {
        self.vector(observations);
        for observation in observations {
            if !self.visit() {
                break;
            }
            self.optional_string(&observation.session_id);
            if let Some(class) = &observation.agent_class {
                self.class(class);
            }
        }
    }
    fn nodes(&mut self, nodes: &Vec<ilium_core::RestructureNode>, depth: usize) {
        if depth > 64 {
            self.bytes = usize::MAX;
            return;
        }
        self.vector(nodes);
        for node in nodes {
            if !self.visit() {
                break;
            }
            use ilium_core::RestructureNode as N;
            match node {
                N::Pane {
                    title,
                    short_title,
                    icon,
                    ..
                }
                | N::Folder {
                    title,
                    short_title,
                    icon,
                    ..
                } => {
                    self.string(title);
                    self.optional_string(short_title);
                    self.optional_string(icon);
                }
                N::Group {
                    title,
                    short_title,
                    icon,
                    children,
                } => {
                    self.string(title);
                    self.optional_string(short_title);
                    self.optional_string(icon);
                    self.nodes(children, depth + 1);
                }
                N::ExistingGroup { children, .. } | N::ExistingSplitView { children, .. } => {
                    self.nodes(children, depth + 1)
                }
            }
        }
    }
    fn recommendation(
        &mut self,
        value: &ilium_core::animation_recommendation::AnimationRecommendation,
    ) {
        self.string(&value.kind);
        self.vector(&value.parameters);
        for parameter in &value.parameters {
            if !self.visit() {
                break;
            }
            self.string(&parameter.id);
            if let ilium_core::animation_recommendation::AnimationValue::Choice { label, .. } =
                &parameter.value
            {
                self.string(label);
            }
        }
    }
}

pub(crate) type AdmittedRequest = Retained<ilium_ipc::ClientRequest>;

pub(crate) fn request_limits() -> ClientLimits {
    ClientLimits {
        jobs: 32,
        service_jobs: 0,
        input_bytes: 128 * MIB + 64 * 1024,
        result_bytes: 1024 * 1024,
    }
}

/// Admission precedes publication and survives the original allocation's
/// ownership moves through parser barrier, UI, channel and actual write.
pub(crate) fn admit_request(
    client: &Client,
    request: ilium_ipc::ClientRequest,
) -> Result<AdmittedRequest, Box<ilium_execution::Rejected<ilium_ipc::ClientRequest>>> {
    let bytes = request_retained_bytes(&request);
    match reserve_request(client, bytes) {
        Ok(reservation) => reservation.retain(request).map_err(Box::new),
        Err(reason) => Err(Box::new(ilium_execution::Rejected {
            reason,
            value: request,
        })),
    }
}

pub(crate) fn reserve_request(
    client: &Client,
    bytes: usize,
) -> Result<ilium_execution::ExternalReservation, RejectReason> {
    if bytes > request_limits().input_bytes {
        return Err(RejectReason::InputBytes);
    }
    client.try_reserve_external(JobCost {
        input_bytes: bytes.max(4096),
        result_bytes: 4096,
    })
}

const MAX_DECODED_STORAGE_BYTES: usize = 64 * MIB;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StorageRefusal {
    SizeOverflow,
    TooLarge {
        requested: usize,
        capacity: usize,
    },
    LocalCapacity {
        used: usize,
        nonterminal: usize,
        blocked_projections: usize,
        requested: usize,
        capacity: usize,
    },
    ProcessQuota(RejectReason),
}
impl StorageRefusal {
    fn reason(self) -> RejectReason {
        match self {
            Self::ProcessQuota(reason) => reason,
            _ => RejectReason::WorkerBytes,
        }
    }

    /// Terminal kind alone does not prove progress: a refused projection head
    /// can prevent later terminal bodies from reaching the parser. Its owner
    /// revokes this wait and wakes an already-registered receiver. Nonterminal
    /// allocations and derivations remain excluded from eligible release credit.
    fn can_wait_for_terminal_release(self) -> bool {
        matches!(
            self,
            Self::LocalCapacity {
                nonterminal,
                blocked_projections: 0,
                requested,
                capacity,
                ..
            } if requested <= capacity.saturating_sub(nonterminal)
        )
    }

    fn into_error(self, terminal: bool) -> IpcError {
        let reason = self.reason();
        let kind = match reason {
            RejectReason::Closed => io::ErrorKind::Interrupted,
            RejectReason::InvalidCost => io::ErrorKind::InvalidInput,
            RejectReason::AccountingPoisoned => io::ErrorKind::Other,
            _ => io::ErrorKind::OutOfMemory,
        };
        IpcError::Io(io::Error::new(
            kind,
            format!(
                "decoded event storage admission refused: {reason:?}; {self:?}; terminal={terminal}; existing projections are retained"
            ),
        ))
    }
}

pub(crate) struct DecodedStorage {
    bytes: std::sync::atomic::AtomicUsize,
    // This is a conservative receive-wait floor, not another capacity or an
    // assertion that every nonterminal event actually installs a projection.
    nonterminal_bytes: std::sync::atomic::AtomicUsize,
    blocked_projections: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    capacity_waits: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    refusal_hook: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
    capacity_bytes: usize,
    quota: ilium_execution::QuotaGroup,
    changed: Notify,
}
impl Default for DecodedStorage {
    fn default() -> Self {
        Self::new(MAX_DECODED_STORAGE_BYTES, crate::execution::process_quota())
    }
}
impl DecodedStorage {
    fn new(capacity_bytes: usize, quota: ilium_execution::QuotaGroup) -> Self {
        Self {
            bytes: std::sync::atomic::AtomicUsize::new(0),
            nonterminal_bytes: std::sync::atomic::AtomicUsize::new(0),
            blocked_projections: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            capacity_waits: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            refusal_hook: std::sync::Mutex::new(None),
            capacity_bytes,
            quota,
            changed: Notify::new(),
        }
    }

    fn admit(self: &Arc<Self>, bytes: usize) -> Result<Arc<DecodedAllocation>, RejectReason> {
        self.admit_classified(bytes, false)
            .map_err(StorageRefusal::reason)
    }

    fn admit_classified(
        self: &Arc<Self>,
        bytes: usize,
        terminal: bool,
    ) -> Result<Arc<DecodedAllocation>, StorageRefusal> {
        use std::sync::atomic::Ordering;
        let bytes = bytes.checked_add(512).ok_or(StorageRefusal::SizeOverflow)?;
        if bytes > self.capacity_bytes {
            return Err(StorageRefusal::TooLarge {
                requested: bytes,
                capacity: self.capacity_bytes,
            });
        }
        let mut used = self.bytes.load(Ordering::Acquire);
        loop {
            let Some(next) = used
                .checked_add(bytes)
                .filter(|next| *next <= self.capacity_bytes)
            else {
                #[cfg(test)]
                let hook = self
                    .refusal_hook
                    .try_lock()
                    .expect("exclusive test hook configuration")
                    .take();
                #[cfg(test)]
                if let Some(hook) = hook {
                    hook();
                }
                return Err(StorageRefusal::LocalCapacity {
                    used,
                    nonterminal: self.nonterminal_bytes.load(Ordering::Acquire),
                    blocked_projections: self.blocked_projections.load(Ordering::Acquire),
                    requested: bytes,
                    capacity: self.capacity_bytes,
                });
            };
            match self
                .bytes
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(current) => used = current,
            }
        }
        match self.quota.reserve_external_storage(bytes) {
            Ok(debit) => {
                if !terminal {
                    self.nonterminal_bytes.fetch_add(bytes, Ordering::AcqRel);
                    // A concurrent derived reservation can make an existing
                    // receive wait infeasible. Reclassify it; do not park it
                    // forever on a release requiring the blocked reader.
                    self.changed.notify_waiters();
                }
                Ok(Arc::new(DecodedAllocation {
                    owner: Arc::clone(self),
                    bytes,
                    terminal,
                    projection_blocked: std::sync::atomic::AtomicBool::new(false),
                    debit: Some(debit),
                }))
            }
            Err(reason) => {
                // The nonterminal floor was not installed on this failed
                // process-admission path; roll back only the total counter.
                self.release(bytes, true);
                Err(StorageRefusal::ProcessQuota(reason))
            }
        }
    }

    fn release(&self, bytes: usize, terminal: bool) {
        use std::sync::atomic::Ordering;
        if !terminal {
            self.nonterminal_bytes.fetch_sub(bytes, Ordering::AcqRel);
        }
        self.bytes.fetch_sub(bytes, Ordering::AcqRel);
        // Rollback is a release too. Notify after all local counters change.
        self.changed.notify_waiters();
        crate::execution::admission_notification().notify_waiters();
    }
}
/// Last Arc owns the storage independently of the decoder's finite receipt.
pub(crate) struct DecodedAllocation {
    owner: Arc<DecodedStorage>,
    bytes: usize,
    terminal: bool,
    projection_blocked: std::sync::atomic::AtomicBool,
    debit: Option<ilium_execution::StorageAdmission>,
}
impl DecodedAllocation {
    pub(crate) fn note_projection_result(&self, reason: Option<RejectReason>) {
        match reason {
            None => self.set_projection_blocked(false),
            Some(RejectReason::WorkerBytes) => self.set_projection_blocked(true),
            _ => {}
        }
    }
    fn set_projection_blocked(&self, blocked: bool) {
        use std::sync::atomic::Ordering;
        if !blocked {
            if self.projection_blocked.swap(false, Ordering::AcqRel) {
                self.owner
                    .blocked_projections
                    .fetch_sub(1, Ordering::AcqRel);
                self.owner.changed.notify_waiters();
            }
            return;
        }
        self.owner
            .blocked_projections
            .fetch_add(1, Ordering::AcqRel);
        if self.projection_blocked.swap(true, Ordering::AcqRel) {
            self.owner
                .blocked_projections
                .fetch_sub(1, Ordering::AcqRel);
            return;
        }
        self.owner.changed.notify_waiters();
    }
    pub(crate) fn declared_bytes(&self) -> usize {
        self.bytes
    }
    pub(crate) fn try_reserve_derived(&self, bytes: usize) -> Result<Arc<Self>, RejectReason> {
        // A derivation may outlive terminal consumption. It is never eligible
        // terminal-release credit merely because its source was a terminal.
        self.owner.admit(bytes)
    }
}
impl std::fmt::Debug for DecodedAllocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodedAllocation")
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}
impl Drop for DecodedAllocation {
    fn drop(&mut self) {
        // A local-capacity waiter must not observe free local credit while the
        // same allocation still holds process credit and falsely fail there.
        drop(self.debit.take());
        self.set_projection_blocked(false);
        self.owner.release(self.bytes, self.terminal);
    }
}
/// Shared by every connection in the process: clones and standalone owners
/// cannot multiply the encoded directional envelope. Payload storage is
/// admitted before encoding, so a blocked stream never holds a CPU job debit.
fn encoded_storage() -> &'static Arc<DecodedStorage> {
    static STORAGE: std::sync::OnceLock<Arc<DecodedStorage>> = std::sync::OnceLock::new();
    STORAGE.get_or_init(|| {
        Arc::new(DecodedStorage::new(
            64 * MIB + 64 * 1024,
            crate::execution::process_quota(),
        ))
    })
}

pub(crate) struct StoredEncodedFrame {
    pub(crate) frame: ilium_ipc::EncodedFrame,
    _storage: Arc<DecodedAllocation>,
}
impl IpcPreparation {
    #[cfg(test)]
    pub(crate) fn encoder_jobs(&self) -> usize {
        self.encoder.usage().jobs
    }

    pub(crate) async fn prepare_encoded(
        &self,
        request: ilium_ipc::ClientRequest,
    ) -> Result<StoredEncodedFrame, IpcError> {
        let reservation = self.reserve_encoder().await?;
        let sized = self
            .run_reserved(reservation, move |_| {
                let bytes = ilium_ipc::encoded_capacity_bound(&request)?;
                Ok((request, bytes))
            })
            .await?;
        let ((request, bytes), sizing_retention) = sized.into_parts();
        drop(sizing_retention);
        let storage = loop {
            let notified = self.completed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match encoded_storage().admit(bytes) {
                Ok(storage) => break storage,
                Err(RejectReason::Busy) => {
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await
                }
                Err(RejectReason::WorkerBytes) => notified.await,
                Err(error) => return Err(rejected(error)),
            }
        };
        let reservation = self.reserve_encoder().await?;
        let encoded = self
            .run_reserved(reservation, move |_| ilium_ipc::encode_frame(&request))
            .await?;
        if encoded.view().retained_bytes() > bytes {
            return Err(IpcError::Io(io::Error::other(
                "encoded frame exceeded preadmitted allocation capacity",
            )));
        }
        let (frame, codec_retention) = encoded.into_parts();
        let stored = StoredEncodedFrame {
            frame,
            _storage: storage,
        };
        drop(codec_retention);
        Ok(stored)
    }
}

impl IpcPreparation {
    /// Preserve the original one-argument fixture API.
    /// Production supplies the connection's stop/consumer-closure future below.
    #[cfg(test)]
    pub(crate) async fn retain_decoded(
        &self,
        event: Retained<(ilium_ipc::ServerEvent, usize)>,
    ) -> Result<crate::connection::Received<ilium_ipc::ServerEvent>, IpcError> {
        self.retain_decoded_cancellable(event, std::future::pending::<&'static str>())
            .await
    }

    pub(crate) async fn retain_decoded_cancellable<F>(
        &self,
        event: Retained<(ilium_ipc::ServerEvent, usize)>,
        cancelled: F,
    ) -> Result<crate::connection::Received<ilium_ipc::ServerEvent>, IpcError>
    where
        F: std::future::Future<Output = &'static str>,
    {
        let terminal = matches!(
            &event.view().0,
            ilium_ipc::ServerEvent::ScreenUpdate { .. }
                | ilium_ipc::ServerEvent::TerminalReplay { .. }
        );
        let bytes = event
            .view()
            .1
            .checked_add(crate::incoming_projection::projection_metadata_bytes(
                &event.view().0,
            ))
            .ok_or_else(|| StorageRefusal::SizeOverflow.into_error(terminal))?;
        tokio::pin!(cancelled);
        let mut waited_for_capacity = false;
        let allocation = loop {
            // Preserve the admitted-frame drain contract until a real capacity
            // wait is necessary. Once blocked, a ready stop wins over a ready
            // release: no successful handoff is claimed for the cancelled item.
            if waited_for_capacity {
                tokio::select! {
                    biased;
                    reason = &mut cancelled => return Err(decoded_wait_cancelled(reason, bytes)),
                    () = std::future::ready(()) => {}
                }
            }
            let notified = self.decoded.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.decoded.admit_classified(bytes, terminal) {
                Ok(allocation) => break allocation,
                Err(StorageRefusal::ProcessQuota(RejectReason::Busy)) => {
                    // The process admission mutex can unlock without releasing
                    // any debit. Preserve its bounded timer retry, not a spin.
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
                Err(refusal) if refusal.can_wait_for_terminal_release() => {
                    if !waited_for_capacity {
                        tracing::debug!(
                            ?refusal,
                            terminal,
                            "decoded storage waiting for terminal release"
                        );
                        waited_for_capacity = true;
                        #[cfg(test)]
                        self.decoded
                            .capacity_waits
                            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    }
                    // The original Retained event still owns its finite codec
                    // credit. No next frame is read, no payload is recopied and
                    // no CPU callback is occupied by this asynchronous wait.
                    tokio::select! {
                        biased;
                        reason = &mut cancelled => return Err(decoded_wait_cancelled(reason, bytes)),
                        () = notified => {}
                    }
                }
                Err(refusal) => return Err(refusal.into_error(terminal)),
            }
        };
        if waited_for_capacity {
            tracing::debug!(
                terminal,
                requested = allocation.declared_bytes(),
                "decoded storage wait admitted original event"
            );
        }
        let ((event, _bytes), finite_credit) = event.into_parts();
        let event = crate::connection::Received::with_retention(
            event,
            Some(crate::connection::EventRetention::new(allocation)),
        );
        // Independently admitted storage exists before releasing finite codec
        // job credit; a persistent tree can never monopolize decoder admission.
        drop(finite_credit);
        Ok(event)
    }
}

fn decoded_wait_cancelled(reason: &str, bytes: usize) -> IpcError {
    IpcError::Io(io::Error::new(
        io::ErrorKind::Interrupted,
        format!(
            "decoded storage wait cancelled: {reason}; unpublished original event ({bytes} declared payload/metadata bytes) released without a delivery acknowledgement"
        ),
    ))
}

#[cfg(test)]
impl IpcPreparation {
    pub(crate) fn with_test_decoded_capacity(mut self, capacity: usize) -> Self {
        assert!(capacity > 0 && capacity <= MAX_DECODED_STORAGE_BYTES);
        assert_eq!(self.decoded_storage_bytes(), 0);
        self.decoded = Arc::new(DecodedStorage::new(capacity, self.decoded.quota.clone()));
        self
    }
    pub(crate) fn set_test_decoded_refusal_hook(&self, hook: impl FnOnce() + Send + 'static) {
        let mut slot = self.decoded.refusal_hook.lock().expect("test refusal hook");
        assert!(slot.is_none(), "only one refusal hook may be armed");
        *slot = Some(Box::new(hook));
    }
    pub(crate) fn decoded_capacity_waits(&self) -> usize {
        self.decoded
            .capacity_waits
            .load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn decoded_blocked_projections(&self) -> usize {
        self.decoded
            .blocked_projections
            .load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn decoder_usage_jobs(&self) -> usize {
        self.decoder.usage().jobs
    }
    pub(crate) fn decoded_storage_bytes(&self) -> usize {
        self.decoded
            .bytes
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

#[cfg(test)]
mod decoded_pressure_tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::task::{Context, Poll, Waker};
    fn storage(capacity: usize, process_capacity: usize) -> Arc<DecodedStorage> {
        Arc::new(DecodedStorage::new(
            capacity,
            ilium_execution::QuotaGroup::new(ilium_execution::QuotaLimits {
                clients: 0,
                jobs: 0,
                service_jobs: 0,
                input_bytes: 0,
                result_bytes: 0,
                worker_threads: 0,
                worker_bytes: process_capacity,
            }),
        ))
    }
    #[tokio::test]
    async fn process_refusal_rolls_back_and_wakes_without_spending_foreign_credit() {
        let storage = storage(4096, 1024);
        let foreign = storage
            .quota
            .reserve_external_storage(1024)
            .expect("foreign owner");
        let notified = storage.changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        assert_eq!(
            storage.admit_classified(256, false).unwrap_err(),
            StorageRefusal::ProcessQuota(RejectReason::WorkerBytes)
        );
        assert_eq!(storage.bytes.load(Ordering::Acquire), 0);
        assert_eq!(storage.nonterminal_bytes.load(Ordering::Acquire), 0);
        assert_eq!(storage.quota.snapshot().worker_bytes, 1024);
        assert!(matches!(
            std::future::Future::poll(notified.as_mut(), &mut Context::from_waker(Waker::noop())),
            Poll::Ready(())
        ));
        drop(foreign);
        let owner = storage
            .admit_classified(512, false)
            .expect("admit after foreign release");
        assert_eq!(storage.bytes.load(Ordering::Acquire), 1024);
        assert_eq!(storage.quota.snapshot().worker_bytes, 1024);
        drop(owner);
        assert_eq!(storage.quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn impossible_sizes_refuse_before_any_debit() {
        let storage = storage(4096, 4096);
        assert_eq!(
            storage.admit_classified(usize::MAX, true).unwrap_err(),
            StorageRefusal::SizeOverflow
        );
        assert_eq!(
            storage.admit_classified(3585, true).unwrap_err(),
            StorageRefusal::TooLarge {
                requested: 4097,
                capacity: 4096
            }
        );
        assert_eq!(storage.bytes.load(Ordering::Acquire), 0);
        assert_eq!(storage.quota.snapshot().worker_bytes, 0);
        let exact = storage
            .admit_classified(3584, true)
            .expect("inclusive exact capacity");
        assert_eq!(storage.bytes.load(Ordering::Acquire), 4096);
        assert_eq!(storage.quota.snapshot().worker_bytes, 4096);
        drop(exact);
        assert_eq!(storage.bytes.load(Ordering::Acquire), 0);
        assert_eq!(storage.quota.snapshot().worker_bytes, 0);
        assert!(
            std::mem::size_of::<DecodedAllocation>()
                + std::mem::size_of::<DecodedStorage>()
                + 4 * std::mem::size_of::<usize>()
                <= 512
        );
    }
    #[tokio::test]
    async fn last_clone_and_projection_barrier_have_exact_release_lifetimes() {
        let storage = storage(4096, 4096);
        let owner = storage
            .admit_classified(1024, false)
            .expect("original owner");
        let clone = Arc::clone(&owner);
        owner.note_projection_result(Some(RejectReason::WorkerBytes));
        clone.note_projection_result(Some(RejectReason::WorkerBytes));
        assert_eq!(storage.blocked_projections.load(Ordering::Acquire), 1);
        owner.note_projection_result(Some(RejectReason::Busy));
        assert_eq!(storage.blocked_projections.load(Ordering::Acquire), 1);
        owner.note_projection_result(None);
        assert_eq!(storage.blocked_projections.load(Ordering::Acquire), 0);
        owner.note_projection_result(Some(RejectReason::WorkerBytes));
        let notified = storage.changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        drop(owner);
        assert_eq!(storage.bytes.load(Ordering::Acquire), 1536);
        assert_eq!(storage.blocked_projections.load(Ordering::Acquire), 1);
        assert!(matches!(
            std::future::Future::poll(notified.as_mut(), &mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        drop(clone);
        assert!(matches!(
            std::future::Future::poll(notified.as_mut(), &mut Context::from_waker(Waker::noop())),
            Poll::Ready(())
        ));
        assert_eq!(storage.bytes.load(Ordering::Acquire), 0);
        assert_eq!(storage.nonterminal_bytes.load(Ordering::Acquire), 0);
        assert_eq!(storage.blocked_projections.load(Ordering::Acquire), 0);
        assert_eq!(storage.quota.snapshot().worker_bytes, 0);
    }
}
