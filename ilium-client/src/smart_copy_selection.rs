//! Ordered light-selection preparation and acknowledged clipboard publication.
//! The UI moves the original session only after admission; CPU owns joins,
//! preview scans and disposal. Accepted requests are never coalesced/cancelled.
use crate::{
    smart_copy::SmartCopySession,
    smart_copy_light::SmartCopyPreview,
    terminal_clipboard::{ClipboardService, DeferredWriteTicket},
};
use ilium_execution::{
    Client, ClientLimits, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt,
    RejectReason, Retained, Retention, RetirementReservation, Retiring, RetiringArc,
};
use std::{collections::VecDeque, convert::Infallible, sync::Arc, time::Instant};
use tokio::sync::Notify;
const MAX_REQUESTS: usize = 8;
const MAX_TEXT: usize = 64 * 1024 * 1024;
const METADATA: usize = 8192;
pub(crate) fn limits() -> ClientLimits {
    ClientLimits {
        jobs: 2 * MAX_REQUESTS + 2,
        service_jobs: 0,
        input_bytes: MAX_TEXT + (2 * MAX_REQUESTS + 2) * METADATA,
        result_bytes: MAX_TEXT * 2 + (2 * MAX_REQUESTS + 2) * METADATA,
    }
}
struct Input {
    session: Retiring<SmartCopySession>,
}
struct SelectionJob {
    input: Retained<Input>,
    text: RetirementReservation<String>,
    bytes: usize,
}
struct Prepared {
    // Field order: original source and its capture hold die together on CPU.
    original: SmartCopySession,
    text: Option<RetiringArc<String>>,
    characters: usize,
    lines: Vec<String>,
    error: Option<String>,
}
enum Body {
    Prepared(Retiring<Prepared>),
    Original {
        session: Retiring<SmartCopySession>,
        error: String,
    },
    Lost(String),
}
impl Job for SelectionJob {
    type Output = Retained<Body>;
    type Error = Infallible;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        let permit = self.text;
        let expected = self.bytes;
        Ok(self.input.map(|input| {
            match input.session.try_map_on_cpu(|session| {
                let build = || -> Result<(String, usize, Vec<String>), String> {
                    if context.stop_requested() {
                        return Err("Selection preparation stopped; original retained".into());
                    }
                    let mut text = String::with_capacity(expected);
                    for (index, part) in session.selected_parts().enumerate() {
                        if index != 0 {
                            text.push_str("\n\n");
                        }
                        let mut offset = 0;
                        while offset < part.len() {
                            if context.stop_requested() {
                                return Err(
                                    "Selection preparation stopped; original retained".into()
                                );
                            }
                            let mut end = (offset + 64 * 1024).min(part.len());
                            while !part.is_char_boundary(end) {
                                end -= 1;
                            }
                            text.push_str(&part[offset..end]);
                            offset = end;
                        }
                    }
                    if text.len() != expected {
                        return Err("Selection size fence changed; original retained".into());
                    }
                    let characters = text.chars().count();
                    let lines = crate::smart_copy_light::preview_lines(
                        &text,
                        crate::smart_copy_light::PREVIEW_MAXIMUM_LINES,
                        crate::smart_copy_light::PREVIEW_MAXIMUM_WIDTH,
                    );
                    Ok((text, characters, lines))
                };
                match build() {
                    Ok((text, characters, lines)) => Prepared {
                        original: session,
                        text: Some(permit.attach_shared(text)),
                        characters,
                        lines,
                        error: None,
                    },
                    Err(error) => Prepared {
                        original: session,
                        text: None,
                        characters: 0,
                        lines: Vec::new(),
                        error: Some(error),
                    },
                }
            }) {
                Ok(prepared) => Body::Prepared(prepared),
                Err(session) => Body::Original {
                    session,
                    error: "Selection callback was not on CPU bank".into(),
                },
            }
        }))
    }
}
struct Request {
    sequence: u64,
    generation: u64,
    ticket: DeferredWriteTicket,
    acknowledgement: Option<(bool, Result<(), String>)>,
}
struct Ready {
    body: Body,
    _input_hold: Option<Retention>,
    job_hold: Option<Retention>,
    native_acknowledged: bool,
}
enum Queued {
    Input(Retained<Input>, Request),
    Recovery(Input, String, Request),
}
enum Active {
    Cpu {
        request: Request,
        receipt: Receipt<SelectionJob>,
    },
    Ready {
        request: Request,
        body: Box<Ready>,
    },
    Writing {
        request: Request,
        body: Box<Ready>,
    },
}
pub struct SelectionCompletion {
    pub sequence: u64,
    pub generation: u64,
    body: Box<Ready>,
    pub result: Result<(), String>,
}
impl SelectionCompletion {
    pub(crate) fn into_preview(self, now: Instant) -> Result<SmartCopyPreview, Self> {
        if !self.body.native_acknowledged || self.result.is_err() {
            return Err(self);
        }
        let (text, count, characters, lines) = match &self.body.body {
            Body::Prepared(prepared) => match &prepared.text {
                Some(text) => (
                    text.clone(),
                    prepared.original.selected_count(),
                    prepared.characters,
                    prepared.lines.clone(),
                ),
                None => return Err(self),
            },
            _ => return Err(self),
        };
        let Ready {
            body: _original,
            _input_hold,
            job_hold: hold,
            native_acknowledged: _,
        } = *self.body;
        Ok(SmartCopyPreview::from_prepared(
            text, count, characters, lines, true, now, hold,
        ))
    }
    pub fn original(&self) -> Option<&SmartCopySession> {
        match &self.body.body {
            Body::Prepared(prepared) => Some(&prepared.original),
            Body::Original { session, .. } => Some(session),
            Body::Lost(_) => None,
        }
    }
}
pub struct RestoredSelection {
    pub sequence: u64,
    pub generation: u64,
    pub original: SmartCopySession,
    pub error: String,
}
enum Restoration {
    Restored(Box<RestoredSelection>),
    Retained(SelectionCompletion),
}
struct RecoveryJob {
    original: Retained<SelectionCompletion>,
}
impl Job for RecoveryJob {
    type Output = Retained<Restoration>;
    type Error = Infallible;
    fn run(self, _: JobContext) -> Result<Self::Output, Self::Error> {
        Ok(self.original.map(|completion| {
            let SelectionCompletion {
                sequence,
                generation,
                body,
                result,
            } = completion;
            let Ready {
                body: value,
                _input_hold,
                job_hold,
                native_acknowledged,
            } = *body;
            let original = match value {
                Body::Prepared(value) => value
                    .try_consume_on_cpu(|prepared| prepared.original)
                    .map_err(Body::Prepared),
                Body::Original { session, error } => session
                    .try_consume_on_cpu(|session| session)
                    .map_err(|session| Body::Original { session, error }),
                Body::Lost(error) => Err(Body::Lost(error)),
            };
            match original {
                Ok(original) => Restoration::Restored(Box::new(RestoredSelection {
                    sequence,
                    generation,
                    original,
                    error: result
                        .err()
                        .unwrap_or_else(|| "Selection preparation failed; not copied".into()),
                })),
                Err(value) => Restoration::Retained(SelectionCompletion {
                    sequence,
                    generation,
                    body: Box::new(Ready {
                        body: value,
                        _input_hold,
                        job_hold,
                        native_acknowledged,
                    }),
                    result,
                }),
            }
        }))
    }
}
pub(crate) struct SelectionOwner {
    client: Client,
    notification: Arc<Notify>,
    queued: VecDeque<Queued>,
    active: Option<Active>,
    recoveries: VecDeque<Retained<SelectionCompletion>>,
    recovering: Option<(u64, u64, Receipt<RecoveryJob>)>,
    next: u64,
    closing: bool,
    fault: Option<RejectReason>,
}
impl SelectionOwner {
    pub(crate) fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = notification.clone();
        Self {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            queued: VecDeque::with_capacity(MAX_REQUESTS),
            active: None,
            recoveries: VecDeque::with_capacity(MAX_REQUESTS),
            recovering: None,
            next: 1,
            closing: false,
            fault: None,
        }
    }
    pub(crate) fn notification(&self) -> Arc<Notify> {
        self.notification.clone()
    }
    pub(crate) fn retry_delay(&self) -> Option<std::time::Duration> {
        self.pending()
            .then_some(std::time::Duration::from_millis(100))
    }
    pub(crate) fn pending_copy(&self) -> bool {
        self.active.is_some() || !self.queued.is_empty()
    }
    pub(crate) fn pending(&self) -> bool {
        self.active.is_some()
            || !self.queued.is_empty()
            || !self.recoveries.is_empty()
            || self.recovering.is_some()
    }
    pub(crate) fn close_admission(&mut self) {
        self.closing = true;
    }
    /// All guards and one existing global FIFO position precede Session take.
    pub(crate) fn admit(
        &mut self,
        slot: &mut Option<SmartCopySession>,
        clipboard: &ClipboardService,
    ) -> Result<u64, String> {
        let session = slot.as_ref().ok_or("No Smart Copy session")?;
        let used = self.queued.len()
            + usize::from(self.active.is_some())
            + self.recoveries.len()
            + usize::from(self.recovering.is_some());
        if self.closing || self.fault.is_some() || used >= MAX_REQUESTS {
            return Err("Selection queue closed or full; original retained".into());
        }
        if session.output_retention.is_none() {
            return Err("Capture custody missing; original retained".into());
        }
        let bytes = session
            .selected_bytes()
            .filter(|bytes| *bytes <= MAX_TEXT)
            .ok_or("Selection exceeds clipboard limit; original retained")?;
        if bytes == 0 {
            return Err("Selection is empty".into());
        }
        let next = self
            .next
            .checked_add(1)
            .ok_or("Selection sequence exhausted")?;
        let reservation = self
            .client
            .try_reserve_external(JobCost {
                input_bytes: METADATA,
                result_bytes: METADATA,
            })
            .map_err(|reason| format!("Selection admission: {reason:?}; original retained"))?;
        reservation
            .validate_value_type::<Input>()
            .map_err(|reason| format!("Selection custody: {reason:?}"))?;
        let original = self
            .client
            .retirement()
            .try_reserve::<SmartCopySession>(METADATA)
            .map_err(|reason| format!("Selection retirement: {reason:?}; original retained"))?;
        let recovery = self
            .client
            .retirement()
            .try_reserve::<SmartCopySession>(METADATA)
            .map_err(|reason| {
                format!("Selection recovery retirement: {reason:?}; original retained")
            })?;
        let ticket = clipboard
            .reserve_prepared_write()
            .map_err(|error| error.message())?;
        let generation = session.generation;
        let Some(mut session) = slot.take() else {
            ticket.fail("Selection source disappeared before preparation");
            return Err("Selection source disappeared".into());
        };
        // An old returned-session permit is empty: replacing it frees only metadata.
        session.return_retirement = Some(recovery);
        let sequence = self.next;
        let request = Request {
            sequence,
            generation,
            ticket,
            acknowledgement: None,
        };
        let input = Input {
            session: original.attach(session),
        };
        match reservation.retain(input) {
            Ok(input) => self.queued.push_back(Queued::Input(input, request)),
            Err(rejected) => self.queued.push_back(Queued::Recovery(
                rejected.value,
                "Selection admission invariant failed; original retained".into(),
                request,
            )),
        }
        self.next = next;
        Ok(sequence)
    }
    fn dispatch(&mut self) {
        if self.active.is_some() || self.queued.is_empty() {
            return;
        }
        if self.fault.is_some() || matches!(self.queued.front(), Some(Queued::Recovery(..))) {
            let Some(queued) = self.queued.pop_front() else {
                return;
            };
            let (input, hold, error, request) = match queued {
                Queued::Input(input, request) => {
                    let (input, hold) = input.into_parts();
                    (
                        input,
                        Some(hold),
                        "Selection bank closed; original retained".into(),
                        request,
                    )
                }
                Queued::Recovery(input, error, request) => (input, None, error, request),
            };
            self.active = Some(Active::Ready {
                request,
                body: Box::new(Ready {
                    body: Body::Original {
                        session: input.session,
                        error,
                    },
                    _input_hold: hold,
                    job_hold: None,
                    native_acknowledged: false,
                }),
            });
            return;
        }
        let Some(Queued::Input(input, _)) = self.queued.front() else {
            return;
        };
        let bytes = input
            .view()
            .session
            .selected_bytes()
            .unwrap_or(MAX_TEXT + 1);
        let reservation = match self.client.try_reserve(
            Lane::Cpu,
            JobCost {
                input_bytes: bytes + METADATA,
                result_bytes: METADATA,
            },
        ) {
            Ok(reservation) => reservation,
            Err(reason) => {
                if permanent(reason) {
                    self.fault = Some(reason);
                }
                return;
            }
        };
        let text = match self
            .client
            .retirement()
            .try_reserve::<String>(bytes + METADATA)
        {
            Ok(text) => text,
            Err(reason) => {
                if permanent(reason) {
                    self.fault = Some(reason);
                }
                return;
            }
        };
        let Some(Queued::Input(input, request)) = self.queued.pop_front() else {
            return;
        };
        match reservation.submit(SelectionJob { input, text, bytes }) {
            Ok(receipt) => self.active = Some(Active::Cpu { request, receipt }),
            Err(rejected) => {
                self.queued
                    .push_front(Queued::Input(rejected.value.input, request));
                if permanent(rejected.reason) {
                    self.fault = Some(rejected.reason);
                }
            }
        }
    }
    pub(crate) fn acknowledge(
        &mut self,
        clipboard_id: u64,
        result: &Result<String, String>,
    ) -> bool {
        let request = match &mut self.active {
            Some(
                Active::Cpu { request, .. }
                | Active::Ready { request, .. }
                | Active::Writing { request, .. },
            ) if request.ticket.id() == clipboard_id => Some(request),
            _ => self.queued.iter_mut().find_map(|queued| {
                let request = match queued {
                    Queued::Input(_, request) | Queued::Recovery(_, _, request) => request,
                };
                (request.ticket.id() == clipboard_id).then_some(request)
            }),
        };
        let Some(request) = request else {
            return false;
        };
        if request.acknowledgement.is_some() {
            return false;
        }
        request.acknowledgement = Some((true, result.as_ref().map(|_| ()).map_err(Clone::clone)));
        true
    }
    pub(crate) fn native_closed(&mut self) {
        self.close_admission();
        self.fault = Some(RejectReason::Closed);
        if let Some(Active::Cpu { receipt, .. }) = &self.active {
            receipt.cancel();
        }
        let unknown = "Clipboard closed without acknowledgement; delivery unknown";
        for queued in &mut self.queued {
            let request = match queued {
                Queued::Input(_, request) | Queued::Recovery(_, _, request) => request,
            };
            request
                .ticket
                .fail("Clipboard closed before preparation; not delivered");
            if request.acknowledgement.is_none() {
                request.acknowledgement = Some((false, Err(unknown.into())));
            }
        }
        if let Some(
            Active::Cpu { request, .. }
            | Active::Ready { request, .. }
            | Active::Writing { request, .. },
        ) = &mut self.active
        {
            request
                .ticket
                .fail("Clipboard closed before preparation; not delivered");
            if request.acknowledgement.is_none() {
                request.acknowledgement = Some((false, Err(unknown.into())));
            }
        }
    }
    pub(crate) fn collect(&mut self, _: Option<&ClipboardService>) -> Option<SelectionCompletion> {
        self.dispatch();
        match self.active.take()? {
            Active::Cpu {
                request,
                mut receipt,
            } => match receipt.try_take() {
                JobPoll::Pending => {
                    self.active = Some(Active::Cpu { request, receipt });
                    None
                }
                JobPoll::Ready(outcome) => {
                    let (outcome, job_hold) = outcome.into_parts();
                    let (body, input_hold) = match outcome {
                        JobOutcome::Finished(Ok(body)) => {
                            let (body, hold) = body.into_parts();
                            (body, Some(hold))
                        }
                        JobOutcome::Finished(Err(impossible)) => match impossible {},
                        JobOutcome::NotStarted { job, .. } => {
                            let (input, hold) = job.input.into_parts();
                            (
                                Body::Original {
                                    session: input.session,
                                    error: "Preparation cancelled; original retained".into(),
                                },
                                Some(hold),
                            )
                        }
                        JobOutcome::Panicked => (
                            Body::Lost("Worker panicked; original custody unknown".into()),
                            None,
                        ),
                    };
                    self.active = Some(Active::Ready {
                        request,
                        body: Box::new(Ready {
                            body,
                            _input_hold: input_hold,
                            job_hold: Some(job_hold),
                            native_acknowledged: false,
                        }),
                    });
                    None
                }
                JobPoll::Lost | JobPoll::Taken => {
                    request
                        .ticket
                        .fail("Selection receipt lost; copy not confirmed");
                    self.active = Some(Active::Writing {
                        request,
                        body: Box::new(Ready {
                            body: Body::Lost(
                                "Selection receipt lost; original custody unknown".into(),
                            ),
                            _input_hold: None,
                            job_hold: None,
                            native_acknowledged: false,
                        }),
                    });
                    None
                }
            },
            Active::Ready { request, body } => {
                if request.acknowledgement.is_none() {
                    match &body.body {
                        Body::Prepared(prepared) => match &prepared.text {
                            Some(text) => {
                                if let Err(error) = request.ticket.fill(text.clone()) {
                                    request.ticket.fail(&error);
                                }
                            }
                            None => request.ticket.fail(
                                prepared
                                    .error
                                    .as_deref()
                                    .unwrap_or("Preparation failed; not copied"),
                            ),
                        },
                        Body::Original { error, .. } | Body::Lost(error) => {
                            request.ticket.fail(error)
                        }
                    }
                }
                self.active = Some(Active::Writing { request, body });
                None
            }
            Active::Writing {
                mut request,
                mut body,
            } => match request.acknowledgement.take() {
                None => {
                    self.active = Some(Active::Writing { request, body });
                    None
                }
                Some((actual, result)) => {
                    body.native_acknowledged = actual;
                    Some(SelectionCompletion {
                        sequence: request.sequence,
                        generation: request.generation,
                        body,
                        result,
                    })
                }
            },
        }
    }
    pub(crate) fn recover(
        &mut self,
        original: SelectionCompletion,
    ) -> Result<(), SelectionCompletion> {
        if self.recoveries.len() + usize::from(self.recovering.is_some()) >= MAX_REQUESTS {
            return Err(original);
        }
        let reservation = match self.client.try_reserve_external(JobCost {
            input_bytes: METADATA,
            result_bytes: METADATA,
        }) {
            Ok(value) => value,
            Err(_) => return Err(original),
        };
        if reservation
            .validate_value_type::<SelectionCompletion>()
            .is_err()
        {
            return Err(original);
        }
        match reservation.retain(original) {
            Ok(original) => {
                self.recoveries.push_back(original);
                Ok(())
            }
            Err(rejected) => Err(rejected.value),
        }
    }
    pub(crate) fn collect_recovery(
        &mut self,
    ) -> Option<Result<RestoredSelection, SelectionCompletion>> {
        if self.closing && self.recovering.is_none() {
            let (original, _hold) = self.recoveries.pop_front()?.into_parts();
            return Some(Err(original)); // No new shutdown job for an unstarted restoration.
        }
        if self.recovering.is_none() {
            if self.recoveries.is_empty() {
                return None;
            }
            let reservation = match self.client.try_reserve(
                Lane::Cpu,
                JobCost {
                    input_bytes: METADATA,
                    result_bytes: METADATA,
                },
            ) {
                Ok(value) => value,
                Err(_) => return None,
            };
            let original = self.recoveries.pop_front()?;
            let sequence = original.view().sequence;
            let generation = original.view().generation;
            match reservation.submit(RecoveryJob { original }) {
                Ok(receipt) => self.recovering = Some((sequence, generation, receipt)),
                Err(rejected) => {
                    self.recoveries.push_front(rejected.value.original);
                    return None;
                }
            }
        }
        let (sequence, generation, mut receipt) = self.recovering.take()?;
        match receipt.try_take() {
            JobPoll::Pending => {
                self.recovering = Some((sequence, generation, receipt));
                None
            }
            JobPoll::Ready(outcome) => {
                let (outcome, hold) = outcome.into_parts();
                match outcome {
                    JobOutcome::Finished(Ok(restoration)) => {
                        let (restoration, input_hold) = restoration.into_parts();
                        match restoration {
                            Restoration::Restored(mut restored) => {
                                // Both job holds cover bounded delivery metadata;
                                // source/return retirement guards already own raw allocations.
                                restored.original.recovery_result_hold = Some(hold);
                                drop(input_hold);
                                Some(Ok(*restored))
                            }
                            Restoration::Retained(original) => Some(Err(original)),
                        }
                    }
                    JobOutcome::NotStarted { job, .. } => {
                        self.recoveries.push_front(job.original);
                        None
                    }
                    JobOutcome::Finished(Err(impossible)) => match impossible {},
                    JobOutcome::Panicked => Some(Err(SelectionCompletion {
                        sequence,
                        generation,
                        body: Box::new(Ready {
                            body: Body::Lost(
                                "Recovery worker panicked; original custody unknown".into(),
                            ),
                            _input_hold: None,
                            job_hold: Some(hold),
                            native_acknowledged: false,
                        }),
                        result: Err(
                            "Recovery failed; original custody unknown, copy not confirmed".into(),
                        ),
                    })),
                }
            }
            JobPoll::Lost | JobPoll::Taken => Some(Err(SelectionCompletion {
                sequence,
                generation,
                body: Box::new(Ready {
                    body: Body::Lost("Recovery receipt lost; original custody unknown".into()),
                    _input_hold: None,
                    job_hold: None,
                    native_acknowledged: false,
                }),
                result: Err("Recovery receipt lost; copy not confirmed".into()),
            })),
        }
    }
}

/// Recoverable deadline custody; a timeout is not a delivery or join receipt.
/// The error can be downcast and retained while the original CPU receipt settles.
pub struct SelectionShutdownCustody {
    custody: std::sync::Mutex<ShutdownCustodyState>,
}
type ShutdownCustodyState = (
    SelectionOwner,
    VecDeque<SelectionCompletion>,
    Option<SmartCopyPreview>,
    VecDeque<RestoredSelection>,
    Option<SmartCopySession>,
);
impl SelectionShutdownCustody {
    pub(crate) fn new(
        owner: SelectionOwner,
        originals: VecDeque<SelectionCompletion>,
        preview: Option<SmartCopyPreview>,
        restored: VecDeque<RestoredSelection>,
        session: Option<SmartCopySession>,
    ) -> Self {
        Self {
            custody: std::sync::Mutex::new((owner, originals, preview, restored, session)),
        }
    }
    pub fn try_take_original(&self) -> Option<SelectionCompletion> {
        let mut custody = self.custody.try_lock().ok()?;
        if let Some(original) = custody.1.pop_front() {
            return Some(original);
        }
        match custody.0.collect_recovery() {
            Some(Err(original)) => return Some(original),
            Some(Ok(restored)) => custody.3.push_back(restored),
            None => {}
        }
        custody.0.collect(None)
    }
    pub fn try_take_restored(&self) -> Option<Result<RestoredSelection, SelectionCompletion>> {
        let mut custody = self.custody.try_lock().ok()?;
        if let Some(restored) = custody.3.pop_front() {
            return Some(Ok(restored));
        }
        custody.0.collect_recovery()
    }
    pub fn take_active_session(&self) -> Option<SmartCopySession> {
        self.custody.try_lock().ok()?.4.take()
    }
}
impl std::fmt::Debug for SelectionShutdownCustody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SelectionShutdownCustody(originals and native outcome retained)")
    }
}
impl std::fmt::Display for SelectionShutdownCustody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "Selection CPU shutdown deadline; original custody retained, delivery unconfirmed",
        )
    }
}
impl std::error::Error for SelectionShutdownCustody {}

/// Keep this feature's original-custody error even if unrelated shutdown also
/// failed. Result::and would otherwise discard it before reaching the caller.
pub struct SelectionShutdownErrors {
    errors: std::sync::Mutex<(crate::error::ClientError, Option<crate::error::ClientError>)>,
}
impl SelectionShutdownErrors {
    pub fn into_errors(self) -> (crate::error::ClientError, Option<crate::error::ClientError>) {
        self.errors
            .into_inner()
            .unwrap_or_else(|error| error.into_inner())
    }
}
impl std::fmt::Debug for SelectionShutdownErrors {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "SelectionShutdownErrors(original selection error and other shutdown error retained)",
        )
    }
}
impl std::fmt::Display for SelectionShutdownErrors {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.errors.try_lock() {
            Ok(errors) => {
                write!(formatter, "Selection shutdown: {}", errors.0)?;
                if let Some(other) = &errors.1 {
                    write!(formatter, "; accompanying cleanup: {other}")?;
                }
                Ok(())
            }
            Err(_) => formatter.write_str(
                "Selection shutdown failed; original custody and accompanying error retained",
            ),
        }
    }
}
impl std::error::Error for SelectionShutdownErrors {}
pub(crate) fn preserve_shutdown_error(
    selection: crate::error::ClientError,
    other: Option<crate::error::ClientError>,
) -> crate::error::ClientError {
    crate::error::ClientError::TerminalSetup(std::io::Error::other(SelectionShutdownErrors {
        errors: std::sync::Mutex::new((selection, other)),
    }))
}

fn permanent(reason: RejectReason) -> bool {
    matches!(
        reason,
        RejectReason::Closed | RejectReason::InvalidCost | RejectReason::AccountingPoisoned
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_clipboard::Operation;
    use ilium_execution::{
        Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    use std::{sync::mpsc, time::Duration};
    fn bank() -> (Execution, Client, Client, ClipboardService, QuotaGroup) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 16,
            jobs: 64,
            service_jobs: 0,
            input_bytes: 512 * 1024 * 1024,
            result_bytes: 512 * 1024 * 1024,
            worker_threads: 2,
            worker_bytes: 512 * 1024 * 1024,
        });
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 8,
            priority: None,
            resident_bytes_per_thread: 64 * 1024,
        };
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane,
                io: lane,
                service: disabled,
            },
        )
        .unwrap();
        let client = execution.client(limits()).unwrap();
        let capture = execution
            .client(ClientLimits {
                jobs: 16,
                service_jobs: 0,
                input_bytes: 16 * METADATA,
                result_bytes: 16 * METADATA,
            })
            .unwrap();
        let clipboard = ClipboardService::fixture_queue(
            execution
                .client(ClientLimits {
                    jobs: 8,
                    service_jobs: 0,
                    input_bytes: MAX_TEXT + METADATA * 8,
                    result_bytes: MAX_TEXT * 2 + METADATA * 8,
                })
                .unwrap(),
        );
        (execution, client, capture, clipboard, quota)
    }
    fn original(client: &Client, generation: u64, parts: &[&str]) -> Option<SmartCopySession> {
        let mut session = SmartCopySession::fixture_selection(generation, parts);
        let capture = client
            .try_reserve_external(JobCost {
                input_bytes: METADATA,
                result_bytes: METADATA,
            })
            .unwrap()
            .retain(())
            .unwrap();
        let (_, hold) = capture.into_parts();
        session.output_retention = Some(hold);
        Some(session)
    }
    fn collect(
        owner: &mut SelectionOwner,
        clipboard: &ClipboardService,
        succeed: bool,
    ) -> SelectionCompletion {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(completion) = owner.collect(Some(clipboard)) {
                return completion;
            }
            if let Some(reply) = clipboard.fixture_process_next(|_| {
                if succeed {
                    Ok(String::new())
                } else {
                    Err("temporary native failure".into())
                }
            }) {
                owner.acknowledge(reply.view().id, &reply.view().result);
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    fn restore(owner: &mut SelectionOwner, completion: SelectionCompletion) -> RestoredSelection {
        owner.recover(completion).ok().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = owner.collect_recovery() {
                return result.ok().unwrap();
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    struct Block(mpsc::Sender<()>, mpsc::Receiver<()>);
    impl Job for Block {
        type Output = ();
        type Error = Infallible;
        fn run(self, _: JobContext) -> Result<(), Infallible> {
            self.0.send(()).unwrap();
            self.1.recv().unwrap();
            Ok(())
        }
    }
    #[test]
    fn blocked_cpu_selected_copy_reserves_global_fifo_before_read_and_later_write() {
        let (mut execution, client, capture, clipboard, quota) = bank();
        let baseline = quota.snapshot().worker_bytes;
        let (entered, receive_entered) = mpsc::channel();
        let (release, receive_release) = mpsc::channel();
        let mut blocked = client
            .try_submit(
                Lane::Cpu,
                JobCost {
                    input_bytes: METADATA,
                    result_bytes: METADATA,
                },
                Block(entered, receive_release),
            )
            .unwrap();
        receive_entered
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let mut owner = SelectionOwner::new(client.clone());
        let mut session = original(&capture, 11, &["second\nline", "α first"]);
        owner.admit(&mut session, &clipboard).unwrap();
        assert!(session.is_none());
        let read = clipboard
            .submit(Operation::Read)
            .map_err(|(_, error)| error)
            .unwrap();
        let write = clipboard
            .submit(Operation::Write("later overwrite".into()))
            .map_err(|(_, error)| error)
            .unwrap();
        assert!(owner.collect(Some(&clipboard)).is_none());
        let mut native = "previous clipboard".to_owned();
        let mut order = Vec::new();
        assert!(clipboard
            .fixture_process_next(|_| panic!("unprepared copy must block later commands"))
            .is_none());
        assert_eq!(native, "previous clipboard");
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let done = loop {
            if let Some(done) = owner.collect(Some(&clipboard)) {
                break done;
            }
            if let Some(reply) = clipboard.fixture_process_next(|operation| match operation {
                Operation::WriteShared(text) => {
                    native = text.as_str().to_owned();
                    Ok(String::new())
                }
                _ => panic!("later read/write overtook selected Copy"),
            }) {
                order.push(reply.view().id);
                assert!(owner.acknowledge(reply.view().id, &reply.view().result));
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert!(done.result.is_ok());
        assert_eq!(native, "second\nline\n\nα first");
        let preview = done.into_preview(Instant::now()).ok().unwrap();
        assert!(preview.copied);
        let read_reply = clipboard
            .fixture_process_next(|operation| {
                assert!(matches!(operation, Operation::Read));
                Ok(native.clone())
            })
            .unwrap();
        order.push(read_reply.view().id);
        assert_eq!(read_reply.view().id, read);
        assert_eq!(
            read_reply.view().result.as_ref().unwrap(),
            "second\nline\n\nα first"
        );
        let write_reply = clipboard
            .fixture_process_next(|operation| {
                let Operation::Write(text) = operation else {
                    panic!("wrong last operation");
                };
                native = text;
                Ok(String::new())
            })
            .unwrap();
        order.push(write_reply.view().id);
        assert_eq!(write_reply.view().id, write);
        assert!(order[0] < read && read < write);
        assert_eq!(native, "later overwrite");
        drop(read_reply);
        drop(write_reply);
        drop(preview);
        drop(owner);
        drop(clipboard);
        while matches!(blocked.try_take(), JobPoll::Pending) {
            std::thread::yield_now();
        }
        drop(blocked);
        while quota.snapshot().jobs != 0 || quota.snapshot().worker_bytes != baseline {
            assert!(
                Instant::now() < deadline,
                "original/leaf/FIFO retirement did not return to baseline"
            );
            std::thread::yield_now();
        }
        execution.request_shutdown(ShutdownMode::Drain);
        execution.join_until_background(deadline).unwrap();
    }
    #[test]
    fn ninth_global_slot_refusal_preserves_identical_original_and_all_eight_failures_ack_in_order()
    {
        let (mut execution, client, capture, clipboard, _) = bank();
        let mut owner = SelectionOwner::new(client.clone());
        for generation in 1..=8 {
            let mut session = original(&capture, generation, &["original"]);
            owner.admit(&mut session, &clipboard).unwrap();
        }
        let mut ninth = original(&capture, 9, &["ninth exact source"]);
        let pointer = ninth
            .as_ref()
            .unwrap()
            .selected_parts()
            .next()
            .unwrap()
            .as_ptr();
        assert!(owner.admit(&mut ninth, &clipboard).is_err());
        assert_eq!(
            ninth
                .as_ref()
                .unwrap()
                .selected_parts()
                .next()
                .unwrap()
                .as_ptr(),
            pointer
        );
        assert_eq!(ninth.as_ref().unwrap().generation, 9);
        owner.close_admission();
        for expected in 1..=8 {
            let done = collect(&mut owner, &clipboard, false);
            assert_eq!((done.sequence, done.generation), (expected, expected));
            assert!(done.result.is_err());
            drop(done);
        }
        assert!(!owner.pending());
        drop(ninth);
        drop(owner);
        drop(clipboard);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }
    #[test]
    fn native_failure_restores_same_session_allocation_and_explicit_retry_uses_new_fifo_slot() {
        let (mut execution, client, capture, clipboard, _) = bank();
        let mut owner = SelectionOwner::new(client.clone());
        let mut session = original(&capture, 41, &["β original", "second\nline"]);
        let pointer = session
            .as_ref()
            .unwrap()
            .selected_parts()
            .next()
            .unwrap()
            .as_ptr();
        owner.admit(&mut session, &clipboard).unwrap();
        let failed = collect(&mut owner, &clipboard, false);
        assert!(failed.result.is_err());
        let restored = restore(&mut owner, failed);
        assert_eq!(
            restored.original.selected_parts().next().unwrap().as_ptr(),
            pointer
        );
        assert_eq!(
            restored.original.selected_parts().collect::<Vec<_>>(),
            ["β original", "second\nline"]
        );
        assert!(restored.original.return_retirement.is_some());
        let mut session = Some(restored.original);
        owner.admit(&mut session, &clipboard).unwrap();
        let done = collect(&mut owner, &clipboard, true);
        assert_eq!(done.sequence, 2);
        assert_eq!(
            done.original()
                .unwrap()
                .selected_parts()
                .next()
                .unwrap()
                .as_ptr(),
            pointer
        );
        let preview = done.into_preview(Instant::now()).ok().unwrap();
        assert_eq!(preview.text.as_str(), "β original\n\nsecond\nline");
        assert!(preview.copied);
        drop(preview);
        drop(owner);
        drop(clipboard);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }
    #[test]
    fn wrong_duplicate_ack_and_stale_generation_cannot_claim_or_replace_newer_preview() {
        let (mut execution, client, capture, clipboard, _) = bank();
        let mut owner = SelectionOwner::new(client.clone());
        let mut session = original(&capture, 51, &["frozen"]);
        owner.admit(&mut session, &clipboard).unwrap();
        assert!(!owner.acknowledge(999, &Ok(String::new())));
        let done = collect(&mut owner, &clipboard, true);
        assert!(!owner.acknowledge(1, &Ok(String::new())));
        assert_ne!(Some(done.generation), Some(52));
        let now = Instant::now();
        let preview = done.into_preview(now).ok().unwrap();
        assert_eq!(preview.started_at, now);
        assert!(preview.copied);
        drop(preview);
        drop(owner);
        drop(clipboard);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }
    #[test]
    fn cancelled_unprepared_copy_explicitly_fails_slot_before_following_read() {
        let (mut execution, client, capture, clipboard, _) = bank();
        let mut owner = SelectionOwner::new(client.clone());
        let mut session = original(&capture, 61, &["before stop"]);
        owner.admit(&mut session, &clipboard).unwrap();
        let read = clipboard
            .submit(Operation::Read)
            .map_err(|(_, error)| error)
            .unwrap();
        owner.native_closed();
        let deadline = Instant::now() + Duration::from_secs(5);
        let done = loop {
            if let Some(done) = owner.collect(Some(&clipboard)) {
                break done;
            }
            if let Some(reply) = clipboard.fixture_process_next(|_| {
                panic!("unprepared cancellation must never call native write")
            }) {
                owner.acknowledge(reply.view().id, &reply.view().result);
            }
            assert!(Instant::now() < deadline);
        };
        assert!(done.result.is_err());
        assert_eq!(
            done.original().unwrap().selected_parts().next(),
            Some("before stop")
        );
        drop(done);
        let read_reply = clipboard
            .fixture_process_next(|operation| {
                assert!(matches!(operation, Operation::Read));
                Ok("previous clipboard".into())
            })
            .unwrap();
        assert_eq!(read_reply.view().id, read);
        drop(read_reply);
        drop(owner);
        drop(clipboard);
        execution.request_shutdown(ShutdownMode::Drain);
        execution.join_until_background(deadline).unwrap();
    }
    #[test]
    fn closed_bank_returns_original_and_explicit_slot_error_without_copy_claim() {
        let (mut execution, client, capture, clipboard, _) = bank();
        let mut owner = SelectionOwner::new(client.clone());
        let mut session = original(&capture, 71, &["original before closure"]);
        owner.admit(&mut session, &clipboard).unwrap();
        execution.request_shutdown(ShutdownMode::Drain);
        let done = collect(&mut owner, &clipboard, false);
        assert!(done.result.is_err());
        assert_eq!(
            done.original().unwrap().selected_parts().next(),
            Some("original before closure")
        );
        assert!(done.into_preview(Instant::now()).is_err());
        drop(owner);
        drop(clipboard);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }
    #[test]
    fn earlier_cleanup_error_keeps_original_deadline_custody_and_native_slot_failure() {
        let (mut execution, client, capture, clipboard, _) = bank();
        let mut owner = SelectionOwner::new(client.clone());
        let mut session = original(&capture, 81, &["deadline original"]);
        owner.admit(&mut session, &clipboard).unwrap();
        owner.native_closed();
        let custody =
            SelectionShutdownCustody::new(owner, VecDeque::new(), None, VecDeque::new(), None);
        let selection = crate::error::ClientError::TerminalSetup(std::io::Error::other(custody));
        let earlier =
            crate::error::ClientError::TerminalSetup(std::io::Error::other("earlier failure"));
        let combined = preserve_shutdown_error(selection, Some(earlier));
        let crate::error::ClientError::TerminalSetup(error) = combined else {
            panic!("wrong envelope");
        };
        let (selection, earlier) = error
            .into_inner()
            .unwrap()
            .downcast::<SelectionShutdownErrors>()
            .unwrap()
            .into_errors();
        assert!(earlier.is_some());
        let crate::error::ClientError::TerminalSetup(error) = selection else {
            panic!("lost original");
        };
        let custody = error
            .into_inner()
            .unwrap()
            .downcast::<SelectionShutdownCustody>()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let done = loop {
            if let Some(done) = custody.try_take_original() {
                break done;
            }
            assert!(Instant::now() < deadline);
        };
        assert_eq!(
            done.original().unwrap().selected_parts().next(),
            Some("deadline original")
        );
        assert!(done.result.is_err());
        drop(done);
        drop(custody);
        let failed = clipboard
            .fixture_process_next(|_| panic!("no native copy after stop"))
            .unwrap();
        assert!(failed.view().result.is_err());
        drop(failed);
        drop(clipboard);
        execution.request_shutdown(ShutdownMode::Drain);
        execution.join_until_background(deadline).unwrap();
    }
}
