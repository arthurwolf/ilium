//! Bounded text-trigger preview preparation on the process's shared CPU bank.
//! The interactive owner captures at most one bounded chunk per turn; regex
//! compilation, matching and final preview-string construction execute only on
//! existing CPU workers.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use ilium_execution::{
    Client, ClientLimits, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt,
    RejectReason, Reservation, RetirementReservation, Retiring, RetiringArc, SkipReason,
};
use tokio::sync::Notify;

use super::{TextTriggerDialogState, TextTriggerPreviewIssue};

const MIB: usize = 1024 * 1024;

const MAX_REGEXP_BYTES: usize = 256 * 1024;
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_SAMPLE_BYTES: usize = MIB;
const MAX_SAMPLE_LINES: usize = 16 * 1024;
const MAX_PREVIEW_TEXT_BYTES: usize = 2 * MIB;

const CAPTURE_STORAGE_BYTES: usize = 4 * MIB;
const OUTPUT_STORAGE_BYTES: usize = 4 * MIB;

const CAPTURE_BYTES_PER_TURN: usize = 64 * 1024;
const CAPTURE_LINES_PER_TURN: usize = 256;

const MAX_RUNNING: usize = 2;

const CAPTURE_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const ADMISSION_RETRY_INTERVAL: Duration = Duration::from_millis(25);

const JOB_COST: JobCost = JobCost {
    // Cooperative declared working cost, not an allocator/RSS bound. Pinned
    // regex defaults retain up to 10 MiB NFA and 2 MiB hybrid cache. Source
    // copies have separate 4 MiB retirement admission; output has separate
    // 4 MiB admission and this result debit. Regex compiler AST/temporary
    // scratch is opaque: these defaults do not strictly cap its peak.
    input_bytes: 16 * MIB,
    result_bytes: 4 * MIB,
};

pub(crate) const fn limits() -> ClientLimits {
    ClientLimits {
        jobs: MAX_RUNNING,
        service_jobs: 0,
        input_bytes: MAX_RUNNING * JOB_COST.input_bytes,
        result_bytes: MAX_RUNNING * JOB_COST.result_bytes,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreviewKey {
    draft_id: String,
    revision: u64,
}

impl PreviewKey {
    fn from_state(state: &TextTriggerDialogState) -> Self {
        Self {
            draft_id: state.identity().to_owned(),
            revision: state.preview_revision(),
        }
    }

    fn matches_state(&self, state: &TextTriggerDialogState) -> bool {
        self.revision == state.preview_revision() && self.draft_id == state.identity()
    }
}

#[cfg(test)]
struct DropProbe(std::sync::mpsc::Sender<std::thread::ThreadId>);

#[cfg(test)]
impl Drop for DropProbe {
    fn drop(&mut self) {
        let _ = self.0.send(std::thread::current().id());
    }
}

#[derive(Default)]
struct CapturedDraft {
    regexp: String,
    message: String,
    sample_text: String,
    #[cfg(test)]
    _drop_probe: Option<DropProbe>,
}

struct PreviewJob {
    key: PreviewKey,
    captured: Retiring<CapturedDraft>,
    output: RetirementReservation<String>,
    #[cfg(test)]
    worker_probe: Option<std::sync::mpsc::Sender<std::thread::ThreadId>>,
    #[cfg(test)]
    panic_for_test: bool,
}

struct PreparedPreview {
    key: PreviewKey,
    text: RetiringArc<String>,
}

impl Job for PreviewJob {
    type Output = PreparedPreview;
    type Error = TextTriggerPreviewIssue;

    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        #[cfg(test)]
        if let Some(probe) = &self.worker_probe {
            let _ = probe.send(std::thread::current().id());
        }

        #[cfg(test)]
        let panic_for_test = self.panic_for_test;

        let Self {
            key,
            captured,
            output,
            ..
        } = self;

        // Every callback that actually starts consumes the captured draft
        // inside the CPU callback. Cancellation/error/panic therefore destroys
        // the captured strings on this CPU worker, never on the interactive
        // owner. Only JobOutcome::NotStarted requires a later retirement
        // handoff because Job::run was never entered.
        let text = captured
            .try_consume_on_cpu(|captured| {
                #[cfg(test)]
                if panic_for_test {
                    panic!("synthetic text-trigger preview panic");
                }

                if context.stop_requested() {
                    return Err(TextTriggerPreviewIssue::Cancelled);
                }

                build_preview(captured, &context)
            })
            .map_err(|returned| {
                drop(returned);
                TextTriggerPreviewIssue::CpuRequired
            })??;

        if context.stop_requested() {
            // Potentially large prepared text still dies on the CPU callback.
            drop(text);
            return Err(TextTriggerPreviewIssue::Cancelled);
        }

        Ok(PreparedPreview {
            key,
            text: output.attach_shared(text),
        })
    }
}

struct Capture {
    key: PreviewKey,
    reservation: Reservation,
    captured: Retiring<CapturedDraft>,
    output: RetirementReservation<String>,
    regexp_offset: usize,
    message_offset: usize,
    sample_line: usize,
    sample_line_offset: usize,
    sample_separator_pending: bool,
}

enum Pending {
    Capture(Capture),
    // Exact job returned by Reservation::submit. No source recapture.
    Prepared(PreviewJob),
}

impl Pending {
    fn key(&self) -> &PreviewKey {
        match self {
            Self::Capture(capture) => &capture.key,
            Self::Prepared(job) => &job.key,
        }
    }
}

struct Running {
    key: PreviewKey,
    receipt: Receipt<PreviewJob>,
}

pub(crate) struct TextTriggerPreview {
    client: Client,
    notification: Arc<Notify>,
    desired: Option<PreviewKey>,
    pending: Option<Pending>,
    running: Vec<Running>,
    ready: Option<PreparedPreview>,
    blocked: Option<(PreviewKey, TextTriggerPreviewIssue)>,
    retry_at: Option<Instant>,
    closing: bool,
    settlement_issue: Option<TextTriggerPreviewIssue>,

    #[cfg(test)]
    worker_probe: Option<std::sync::mpsc::Sender<std::thread::ThreadId>>,
    #[cfg(test)]
    source_drop_probe: Option<std::sync::mpsc::Sender<std::thread::ThreadId>>,
    #[cfg(test)]
    panic_next: bool,
}

impl TextTriggerPreview {
    pub(crate) fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = Arc::clone(&notification);

        Self {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            desired: None,
            pending: None,
            running: Vec::with_capacity(MAX_RUNNING),
            ready: None,
            blocked: None,
            retry_at: None,
            closing: false,
            settlement_issue: None,

            #[cfg(test)]
            worker_probe: None,
            #[cfg(test)]
            source_drop_probe: None,
            #[cfg(test)]
            panic_next: false,
        }
    }

    pub(crate) fn notification(&self) -> Arc<Notify> {
        Arc::clone(&self.notification)
    }

    pub(crate) fn retry_delay(&self, now: Instant) -> Option<Duration> {
        if self.closing {
            return None;
        }

        self.retry_at
            .map(|retry_at| retry_at.saturating_duration_since(now))
    }

    pub(crate) fn request(&mut self, state: &mut TextTriggerDialogState) -> bool {
        let mut changed = self.collect();
        changed |= self.install(state);

        if self.closing {
            return changed;
        }

        let key = PreviewKey::from_state(state);

        if self.desired.as_ref() != Some(&key) {
            self.replace_desired(key.clone());
        }

        if state.preview_is_current() {
            return changed;
        }

        if self.ready.as_ref().is_some_and(|ready| ready.key == key)
            || self
                .blocked
                .as_ref()
                .is_some_and(|(blocked, _)| blocked == &key)
        {
            return changed | self.install(state);
        }

        changed |= state.mark_preview_pending();

        let now = Instant::now();

        // Important batch-1 correction: a refused original is not retried on
        // every unrelated input/redraw event. The absolute deadline is honored.
        if self.retry_at.is_some_and(|retry_at| retry_at > now) {
            return changed;
        }
        self.retry_at = None;

        if self.running.iter().any(|running| running.key == key) {
            return changed;
        }

        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.key() == &key)
        {
            changed |= self.advance_pending(state, now);
            return changed | self.install(state);
        }

        if self.running.len() >= MAX_RUNNING {
            self.retry_at = Some(now + ADMISSION_RETRY_INTERVAL);
            return changed;
        }

        changed |= self.begin_capture(state, key, now);
        changed | self.install(state)
    }

    pub(crate) fn collect(&mut self) -> bool {
        let mut changed = false;
        let mut index = 0;

        while index < self.running.len() {
            match self.running[index].receipt.try_take() {
                JobPoll::Pending => {
                    index += 1;
                }

                JobPoll::Ready(outcome) => {
                    let running = self.running.remove(index);
                    let (outcome, retention) = outcome.into_parts();
                    changed = true;

                    match outcome {
                        JobOutcome::Finished(Ok(mut prepared)) => {
                            // The output has one Arc owner at this point. Carry
                            // the original job debit through its eventual
                            // Retiring<String> CPU destruction.
                            Arc::get_mut(&mut prepared.text)
                                .expect("preview result has one owner before publication")
                                .set_retention(retention);

                            if !self.closing && self.desired.as_ref() == Some(&prepared.key) {
                                self.ready = Some(prepared);
                                self.blocked = None;
                            } else {
                                // Stale large result: dropping this Arc performs
                                // only the bounded retirement handoff here.
                                drop(prepared);
                            }
                        }

                        JobOutcome::Finished(Err(issue)) => {
                            // Job::run started, so CapturedDraft and any
                            // partially built String already died on the CPU.
                            drop(retention);

                            if !self.closing && self.desired.as_ref() == Some(&running.key) {
                                self.ready = None;
                                self.blocked = Some((running.key, issue));
                            }
                        }

                        JobOutcome::NotStarted { mut job, reason } => {
                            let key = job.key.clone();

                            // Job::run never ran. The exact captured allocation
                            // still lives inside this exact returned job.
                            // Transfer the receipt's original debit into it
                            // before dropping the job on the interactive owner.
                            job.captured.set_retention(retention);
                            drop(job);

                            if !self.closing && self.desired.as_ref() == Some(&key) {
                                let issue = match reason {
                                    SkipReason::Cancelled => TextTriggerPreviewIssue::Cancelled,
                                    SkipReason::Shutdown => TextTriggerPreviewIssue::Shutdown,
                                };

                                self.ready = None;
                                self.blocked = Some((key, issue));
                            }
                        }

                        JobOutcome::Panicked => {
                            // PreviewJob places all meaningful computation
                            // inside try_consume_on_cpu, so unwinding destroys
                            // captured source on that CPU callback.
                            drop(retention);

                            if !self.closing && self.desired.as_ref() == Some(&running.key) {
                                self.ready = None;
                                self.blocked =
                                    Some((running.key, TextTriggerPreviewIssue::WorkerPanicked));
                            }
                        }
                    }
                }

                JobPoll::Lost | JobPoll::Taken => {
                    let running = self.running.remove(index);
                    changed = true;
                    // Lost is terminal observation, not proof of successful
                    // cancellation. Execution still owns physical recovery.
                    self.settlement_issue = Some(TextTriggerPreviewIssue::ReceiptLost);

                    if !self.closing && self.desired.as_ref() == Some(&running.key) {
                        self.ready = None;
                        self.blocked = Some((running.key, TextTriggerPreviewIssue::ReceiptLost));
                    }
                }
            }
        }

        changed
    }

    pub(crate) fn cancel_draft(&mut self, draft_id: &str) {
        if self
            .desired
            .as_ref()
            .is_some_and(|desired| desired.draft_id == draft_id)
        {
            self.desired = None;
            self.retry_at = None;
        }

        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.key().draft_id == draft_id)
        {
            if let Some(pending) = self.pending.take() {
                retire_pending(pending);
            }
        }

        if self
            .ready
            .as_ref()
            .is_some_and(|ready| ready.key.draft_id == draft_id)
        {
            self.ready = None;
        }

        if self
            .blocked
            .as_ref()
            .is_some_and(|(key, _)| key.draft_id == draft_id)
        {
            self.blocked = None;
        }

        // Keep accepted receipts until their real outcomes settle. Cancellation
        // changes computation, not ownership.
        for running in &self.running {
            if running.key.draft_id == draft_id {
                running.receipt.cancel();
            }
        }
    }

    pub(crate) fn cancel_all(&mut self) {
        self.desired = None;
        self.retry_at = None;

        if let Some(pending) = self.pending.take() {
            retire_pending(pending);
        }

        self.ready = None;
        self.blocked = None;

        for running in &self.running {
            running.receipt.cancel();
        }
    }

    /// Nonblocking: retain accepted receipts while the bank deadline runs.
    pub(crate) fn close(&mut self) {
        self.closing = true;
        self.cancel_all();
        self.collect();
    }

    /// Receipt settlement is distinct from physical retirement/bank join.
    pub(crate) fn is_settled(&self) -> bool {
        self.pending.is_none() && self.running.is_empty() && self.ready.is_none()
    }

    pub(crate) fn settlement_issue(&self) -> Option<TextTriggerPreviewIssue> {
        self.settlement_issue
    }

    fn replace_desired(&mut self, key: PreviewKey) {
        for running in &self.running {
            if running.key != key {
                running.receipt.cancel();
            }
        }

        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.key() != &key)
        {
            if let Some(pending) = self.pending.take() {
                retire_pending(pending);
            }
        }

        if self.ready.as_ref().is_some_and(|ready| ready.key != key) {
            self.ready = None;
        }

        if self
            .blocked
            .as_ref()
            .is_some_and(|(blocked, _)| blocked != &key)
        {
            self.blocked = None;
        }

        self.desired = Some(key);
        self.retry_at = None;
    }

    fn begin_capture(
        &mut self,
        state: &mut TextTriggerDialogState,
        key: PreviewKey,
        now: Instant,
    ) -> bool {
        if state.regexp.buf.len() > MAX_REGEXP_BYTES
            || state.message.buf.len() > MAX_MESSAGE_BYTES
            || state.sample.lines().len() > MAX_SAMPLE_LINES
        {
            self.block_current(key, TextTriggerPreviewIssue::CaptureLimit);
            return true;
        }

        let maximum = match self.client.maximum_cpu_job_cost() {
            Ok(maximum) => maximum,
            Err(reason) => {
                self.block_current(key, TextTriggerPreviewIssue::Admission(reason));
                return true;
            }
        };
        if JOB_COST.input_bytes > maximum.input_bytes
            || JOB_COST.result_bytes > maximum.result_bytes
        {
            self.block_current(
                key,
                TextTriggerPreviewIssue::Admission(RejectReason::InvalidCost),
            );
            return true;
        }

        // Ordinary job admission happens before any authored String copy.
        let reservation = match self.client.try_reserve(Lane::Cpu, JOB_COST) {
            Ok(reservation) => reservation,
            Err(reason) => {
                self.handle_admission_refusal(key, reason, now);
                return true;
            }
        };

        let retirement = self.client.retirement();

        // These reservations are still empty at this point. If either fails,
        // no large authored allocation has been copied.
        let captured = match retirement.try_reserve::<CapturedDraft>(CAPTURE_STORAGE_BYTES) {
            Ok(captured) => captured,
            Err(reason) => {
                self.handle_admission_refusal(key, reason, now);
                return true;
            }
        };

        let output = match retirement.try_reserve::<String>(OUTPUT_STORAGE_BYTES) {
            Ok(output) => output,
            Err(reason) => {
                self.handle_admission_refusal(key, reason, now);
                return true;
            }
        };

        let captured_value = CapturedDraft {
            #[cfg(test)]
            _drop_probe: self.source_drop_probe.clone().map(DropProbe),
            ..CapturedDraft::default()
        };

        self.pending = Some(Pending::Capture(Capture {
            key,
            reservation,
            captured: captured.attach(captured_value),
            output,
            regexp_offset: 0,
            message_offset: 0,
            sample_line: 0,
            sample_line_offset: 0,
            sample_separator_pending: false,
        }));

        self.retry_at = None;
        self.advance_pending(state, now)
    }

    fn advance_pending(&mut self, state: &mut TextTriggerDialogState, now: Instant) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };

        match pending {
            Pending::Capture(mut capture) => {
                if !capture.key.matches_state(state) {
                    retire_capture(capture);
                    return true;
                }

                match advance_capture(&mut capture, state) {
                    Ok(false) => {
                        self.pending = Some(Pending::Capture(capture));
                        self.retry_at = Some(now + CAPTURE_RETRY_INTERVAL);
                    }

                    Ok(true) => {
                        let Capture {
                            key,
                            reservation,
                            captured,
                            output,
                            ..
                        } = capture;

                        let job = PreviewJob {
                            key,
                            captured,
                            output,

                            #[cfg(test)]
                            worker_probe: self.worker_probe.clone(),

                            #[cfg(test)]
                            panic_for_test: std::mem::take(&mut self.panic_next),
                        };

                        self.submit_reserved(reservation, job, now);
                    }

                    Err(issue) => {
                        let key = capture.key.clone();

                        // This partial allocation was created while the
                        // Reservation's job hold was live; retain that exact
                        // debit until CPU retirement destroys the partial copy.
                        retire_capture(capture);
                        self.block_current(key, issue);
                    }
                }

                true
            }

            Pending::Prepared(job) => {
                let key = job.key.clone();

                if !key.matches_state(state) {
                    // submit() previously returned this exact job. It no
                    // longer has an ordinary JobHold, but its Retiring source
                    // keeps its own storage/physical-destruction admission.
                    drop(job);
                    return true;
                }

                if self.running.len() >= MAX_RUNNING {
                    self.pending = Some(Pending::Prepared(job));
                    self.retry_at = Some(now + ADMISSION_RETRY_INTERVAL);
                    return false;
                }

                let reservation = match self.client.try_reserve(Lane::Cpu, JOB_COST) {
                    Ok(reservation) => reservation,

                    Err(reason) if retryable(reason) => {
                        // Exact same captured job; no recapture.
                        self.pending = Some(Pending::Prepared(job));
                        self.retry_at = Some(now + ADMISSION_RETRY_INTERVAL);
                        return true;
                    }

                    Err(reason) => {
                        drop(job);
                        self.block_current(key, TextTriggerPreviewIssue::Admission(reason));
                        return true;
                    }
                };

                self.submit_reserved(reservation, job, now);
                true
            }
        }
    }

    fn submit_reserved(&mut self, reservation: Reservation, job: PreviewJob, now: Instant) {
        let key = job.key.clone();

        match reservation.submit(job) {
            Ok(receipt) => {
                self.running.push(Running { key, receipt });
                self.retry_at = None;
            }

            Err(rejected) if retryable(rejected.reason) => {
                // Reservation::submit returns the exact unpublished job.
                self.pending = Some(Pending::Prepared(rejected.value));
                self.retry_at = Some(now + ADMISSION_RETRY_INTERVAL);
            }

            Err(rejected) => {
                drop(rejected.value);
                self.block_current(key, TextTriggerPreviewIssue::Publication(rejected.reason));
            }
        }
    }

    fn handle_admission_refusal(&mut self, key: PreviewKey, reason: RejectReason, now: Instant) {
        if retryable(reason) {
            self.retry_at = Some(now + ADMISSION_RETRY_INTERVAL);
            return;
        }

        self.block_current(key, TextTriggerPreviewIssue::Admission(reason));
    }

    fn block_current(&mut self, key: PreviewKey, issue: TextTriggerPreviewIssue) {
        if let Some(pending) = self.pending.take() {
            retire_pending(pending);
        }

        self.ready = None;
        self.retry_at = None;
        self.blocked = Some((key, issue));
    }

    fn install(&mut self, state: &mut TextTriggerDialogState) -> bool {
        let key = PreviewKey::from_state(state);

        if self.ready.as_ref().is_some_and(|ready| ready.key == key) {
            let ready = self.ready.take().expect("matching ready preview");

            return state.install_preview(ready.key.revision, ready.text);
        }

        if let Some((_, issue)) = self.blocked.as_ref().filter(|(blocked, _)| blocked == &key) {
            return state.preview_unavailable(key.revision, *issue);
        }

        false
    }

    #[cfg(test)]
    fn set_worker_probe(&mut self, probe: std::sync::mpsc::Sender<std::thread::ThreadId>) {
        self.worker_probe = Some(probe);
    }

    #[cfg(test)]
    fn set_source_drop_probe(&mut self, probe: std::sync::mpsc::Sender<std::thread::ThreadId>) {
        self.source_drop_probe = Some(probe);
    }

    #[cfg(test)]
    fn panic_next_job(&mut self) {
        self.panic_next = true;
    }
}

impl Drop for TextTriggerPreview {
    fn drop(&mut self) {
        // Production close retains receipts until collected. This fallback is
        // only cooperative cancellation; it deliberately does not claim that
        // accepted receipts physically settled synchronously in Drop.
        self.cancel_all();
    }
}

// A partially captured source already consumed an ordinary Reservation.
// Transfer that exact JobHold into the Retiring source before the reservation
// is released by this caller.
fn retire_capture(mut capture: Capture) {
    capture
        .captured
        .set_retention(capture.reservation.retention());

    drop(capture);
}

fn retire_pending(pending: Pending) {
    match pending {
        Pending::Capture(capture) => retire_capture(capture),
        Pending::Prepared(job) => drop(job),
    }
}

fn retryable(reason: RejectReason) -> bool {
    matches!(
        reason,
        RejectReason::Busy
            | RejectReason::QueueFull
            | RejectReason::JobLimit
            | RejectReason::InputBytes
            | RejectReason::ResultBytes
            | RejectReason::WorkerBytes
    )
}

fn advance_capture(
    capture: &mut Capture,
    state: &TextTriggerDialogState,
) -> Result<bool, TextTriggerPreviewIssue> {
    let mut bytes_left = CAPTURE_BYTES_PER_TURN;
    let mut lines_left = CAPTURE_LINES_PER_TURN;

    if capture.regexp_offset < state.regexp.buf.len() {
        capture.regexp_offset = copy_utf8_chunk(
            &state.regexp.buf,
            &mut capture.captured.regexp,
            capture.regexp_offset,
            &mut bytes_left,
        );

        return Ok(false);
    }

    if capture.message_offset < state.message.buf.len() {
        capture.message_offset = copy_utf8_chunk(
            &state.message.buf,
            &mut capture.captured.message,
            capture.message_offset,
            &mut bytes_left,
        );

        return Ok(false);
    }

    let lines = state.sample.lines();

    while capture.sample_line < lines.len() && lines_left > 0 && bytes_left > 0 {
        if capture.sample_separator_pending {
            if capture.captured.sample_text.len() >= MAX_SAMPLE_BYTES {
                return Err(TextTriggerPreviewIssue::CaptureLimit);
            }

            capture.captured.sample_text.push('\n');
            capture.sample_separator_pending = false;
            bytes_left -= 1;

            if bytes_left == 0 {
                break;
            }
        }

        let line = &lines[capture.sample_line];

        if line.len() > MAX_SAMPLE_BYTES {
            return Err(TextTriggerPreviewIssue::CaptureLimit);
        }

        let sample_capacity_left = MAX_SAMPLE_BYTES - capture.captured.sample_text.len();

        if sample_capacity_left == 0 && capture.sample_line_offset < line.len() {
            return Err(TextTriggerPreviewIssue::CaptureLimit);
        }

        if capture.sample_line_offset < line.len() {
            let next_width = line[capture.sample_line_offset..]
                .chars()
                .next()
                .expect("remaining character")
                .len_utf8();
            if next_width > sample_capacity_left {
                // Global space cannot grow next turn: refuse instead of retrying
                // the same UTF-8 boundary forever. A turn budget alone may retry.
                return Err(TextTriggerPreviewIssue::CaptureLimit);
            }
        }

        let mut sample_turn_bytes = bytes_left.min(sample_capacity_left);

        let before = sample_turn_bytes;

        capture.sample_line_offset = copy_utf8_chunk(
            line,
            &mut capture.captured.sample_text,
            capture.sample_line_offset,
            &mut sample_turn_bytes,
        );

        bytes_left -= before - sample_turn_bytes;

        if capture.sample_line_offset < line.len() {
            break;
        }

        capture.sample_line += 1;
        capture.sample_line_offset = 0;
        capture.sample_separator_pending = capture.sample_line < lines.len();
        lines_left -= 1;
    }

    if capture.sample_line < lines.len() || capture.sample_separator_pending {
        return Ok(false);
    }

    Ok(true)
}

fn copy_utf8_chunk(
    source: &str,
    target: &mut String,
    start: usize,
    bytes_left: &mut usize,
) -> usize {
    if start >= source.len() || *bytes_left == 0 {
        return start;
    }

    let mut end = start.saturating_add(*bytes_left).min(source.len());

    while end > start && !source.is_char_boundary(end) {
        end -= 1;
    }

    if end == start {
        let character_bytes = source[start..].chars().next().map_or(0, char::len_utf8);

        if character_bytes > *bytes_left {
            return start;
        }

        end = start + character_bytes;
    }

    target.push_str(&source[start..end]);
    *bytes_left -= end - start;

    end
}

fn build_preview(
    captured: CapturedDraft,
    context: &JobContext,
) -> Result<String, TextTriggerPreviewIssue> {
    if captured.regexp.is_empty() {
        return Ok("Enter a regexp to preview matching sample lines.".to_owned());
    }

    let regex = match regex::Regex::new(&captured.regexp) {
        Ok(regex) => regex,

        Err(error) => {
            let text = format!("Invalid regexp: {error}");

            if text.len() > MAX_PREVIEW_TEXT_BYTES {
                return Err(TextTriggerPreviewIssue::OutputLimit);
            }

            return Ok(text);
        }
    };

    let mut output = String::new();

    // captured.sample_text is exactly TextArea::lines().join("\n").
    // Applying str::lines() here reproduces the old renderer's treatment of
    // empty and trailing-final-empty sample rows.
    for line in captured.sample_text.lines() {
        if context.stop_requested() {
            return Err(TextTriggerPreviewIssue::Cancelled);
        }

        append_preview_line(&mut output, &regex, line)?;
    }

    if regex.is_match(&captured.message) {
        append_output_parts(
            &mut output,
            &["⚠ Reply also matches this regexp; echoed input can loop."],
        )?;
    }

    Ok(output)
}

fn append_preview_line(
    output: &mut String,
    regex: &regex::Regex,
    line: &str,
) -> Result<(), TextTriggerPreviewIssue> {
    let Some(found) = regex.find(line) else {
        return append_output_parts(output, &["· ", line]);
    };

    let matched = &line[found.start()..found.end()];

    if matched.is_empty() {
        return append_output_parts(output, &["✓ ", line, "  ⟪zero-width match⟫"]);
    }

    append_output_parts(
        output,
        &[
            "✓ ",
            &line[..found.start()],
            "[",
            matched,
            "]",
            &line[found.end()..],
        ],
    )
}

fn append_output_parts(output: &mut String, parts: &[&str]) -> Result<(), TextTriggerPreviewIssue> {
    let separator = usize::from(!output.is_empty());

    let addition = parts
        .iter()
        .try_fold(separator, |total, part| total.checked_add(part.len()))
        .ok_or(TextTriggerPreviewIssue::OutputLimit)?;

    if output.len().saturating_add(addition) > MAX_PREVIEW_TEXT_BYTES {
        return Err(TextTriggerPreviewIssue::OutputLimit);
    }

    if separator != 0 {
        output.push('\n');
    }

    for part in parts {
        output.push_str(part);
    }

    Ok(())
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
