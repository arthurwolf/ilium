//! Native clipboard custody lives in a memory-limited helper process. The
//! parent owns one blocking pipe thread; interactive code only admits commands
//! and consumes acknowledgements. Selection ownership survives each write.
use ilium_execution::{Client, ExternalReservation, JobCost, Retained};
use ilium_platform::owned_worker::{spawn_owned, OwnedWorker, StopToken, WorkerKind};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Condvar, Mutex, TryLockError},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
const MAX_TEXT: usize = 64 * 1024 * 1024;
const MAX_ERROR: usize = 8192;
const HELPER_BYTES: usize = 256 * 1024 * 1024;
const MAX_COMMANDS: usize = 8;
const MAGIC: [u8; 4] = *b"ICB1";
const READ: u8 = 1;
const WRITE: u8 = 2;
const CLOSE: u8 = 3;
const OK: u8 = 4;
const ERROR: u8 = 5;

pub(crate) enum Operation {
    Read,
    DeferredWrite(Arc<DeferredState>),
    Write(String),
    /// The original independently admitted leaf; pipe writing borrows it.
    WriteShared(ilium_execution::RetiringArc<String>),
}
enum DeferredValue {
    Waiting,
    Ready(Result<ilium_execution::RetiringArc<String>, String>),
    Consumed,
}
pub(crate) struct DeferredState {
    value: Mutex<DeferredValue>,
}
/// One already-admitted position in the existing native clipboard FIFO.
/// Dropping an unfilled ticket explicitly fails that position, never skips it.
pub(crate) struct DeferredWriteTicket {
    id: u64,
    state: Arc<DeferredState>,
    shared: std::sync::Weak<Shared>,
}
impl DeferredWriteTicket {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }
    pub(crate) fn fill(&self, text: ilium_execution::RetiringArc<String>) -> Result<(), String> {
        if text.capacity() > MAX_TEXT {
            self.fail("Selection exceeds clipboard limit");
            return Err("Selection exceeds clipboard limit".into());
        }
        let mut state = self
            .state
            .value
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !matches!(*state, DeferredValue::Waiting) {
            return Err("Clipboard position already resolved or closed".into());
        }
        *state = DeferredValue::Ready(Ok(text));
        drop(state);
        self.wake();
        Ok(())
    }
    pub(crate) fn fail(&self, message: &str) {
        let mut state = self
            .state
            .value
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if matches!(*state, DeferredValue::Waiting) {
            *state = DeferredValue::Ready(Err(message.chars().take(MAX_ERROR / 4).collect()));
        }
        drop(state);
        self.wake();
    }
    fn wake(&self) {
        if let Some(shared) = self.shared.upgrade() {
            shared.changed.notify_all();
            shared.notification.notify_one();
        }
    }
}
impl Drop for DeferredWriteTicket {
    fn drop(&mut self) {
        self.fail("Selected copy cancelled before clipboard preparation; not delivered");
    }
}
pub(crate) enum WriteAdmissionError {
    Busy(String),
    Closed(String),
    Failed(String),
}
impl WriteAdmissionError {
    pub(crate) fn message(self) -> String {
        match self {
            Self::Busy(message) | Self::Closed(message) | Self::Failed(message) => message,
        }
    }
}
pub(crate) struct Completion {
    pub id: u64,
    pub result: Result<String, String>,
}
struct Pending {
    id: u64,
    operation: Operation,
    reservation: ExternalReservation,
}
#[derive(Default)]
struct Queue {
    commands: VecDeque<Pending>,
    results: VecDeque<Retained<Completion>>,
    closing: bool,
    exited: bool,
    active: bool,
    failure: Option<String>,
    next_id: u64,
}
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
    notification: Arc<Notify>,
    child: Mutex<Option<Child>>,
    #[cfg(test)]
    helper_command: Mutex<Option<(std::path::PathBuf, Vec<std::ffi::OsString>)>>,
}
pub(crate) struct ClipboardService {
    shared: Arc<Shared>,
    client: Client,
    worker: Option<OwnedWorker>,
}
impl ClipboardService {
    pub(crate) fn start(client: Client) -> io::Result<Self> {
        let lease = crate::execution::process_quota()
            .reserve_external_worker(3, HELPER_BYTES + 8 * 1024 * 1024)
            .map_err(|error| io::Error::other(format!("clipboard worker admission: {error:?}")))?;
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                next_id: 1,
                ..Queue::default()
            }),
            changed: Condvar::new(),
            notification: Arc::new(Notify::new()),
            child: Mutex::new(None),
            #[cfg(test)]
            helper_command: Mutex::new(None),
        });
        let wake_shared = shared.clone();
        let body_shared = shared.clone();
        let worker = spawn_owned(
            "ilium-clipboard",
            WorkerKind::SynchronousIo,
            StopToken::default(),
            move || {
                let _retain_until_join = &lease;
                wake_shared.changed.notify_all();
                wake_shared.notification.notify_one();
            },
            move |stop| run_owner(body_shared, stop),
        )?;
        Ok(Self {
            shared,
            client,
            worker: Some(worker),
        })
    }
    #[cfg(test)]
    pub(crate) fn start_with_native_helper(
        client: Client,
        executable: &std::path::Path,
    ) -> io::Result<Self> {
        if !executable.is_absolute() || !executable.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Native clipboard test requires an absolute existing helper executable",
            ));
        }
        let service = Self::start(client)?;
        *service
            .shared
            .helper_command
            .lock()
            .unwrap_or_else(|error| error.into_inner()) =
            Some((executable.to_owned(), vec!["clipboard-helper".into()]));
        Ok(service)
    }
    #[cfg(test)]
    pub(crate) fn fixture_queue(client: Client) -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue {
                    next_id: 1,
                    ..Queue::default()
                }),
                changed: Condvar::new(),
                notification: Arc::new(Notify::new()),
                child: Mutex::new(None),
                helper_command: Mutex::new(None),
            }),
            client,
            worker: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn fixture_process_next(
        &self,
        apply: impl FnOnce(Operation) -> Result<String, String>,
    ) -> Option<Retained<Completion>> {
        let mut queue = self.shared.queue.lock().unwrap();
        let front = queue.commands.front()?;
        let deferred = match &front.operation {
            Operation::DeferredWrite(state) => Some(try_deferred(state)?),
            _ => None,
        };
        let pending = queue.commands.pop_front().unwrap();
        queue.active = true;
        drop(queue);
        let result = match deferred {
            Some(Ok(operation)) => apply(operation),
            Some(Err(error)) => Err(error),
            None => apply(pending.operation),
        };
        let completion = pending
            .reservation
            .retain(Completion {
                id: pending.id,
                result,
            })
            .unwrap();
        self.shared.queue.lock().unwrap().active = false;
        Some(completion)
    }
    pub(crate) fn notification(&self) -> Arc<Notify> {
        self.shared.notification.clone()
    }
    pub(crate) fn submit(&self, operation: Operation) -> Result<u64, (Operation, String)> {
        self.submit_classified(operation)
            .map_err(|(original, error)| (original, error.message()))
    }
    pub(crate) fn reserve_prepared_write(
        &self,
    ) -> Result<DeferredWriteTicket, WriteAdmissionError> {
        // Construct bounded metadata only; the normal admission counts this
        // same command among all queued/active/results before any CPU work.
        let state = Arc::new(DeferredState {
            value: Mutex::new(DeferredValue::Waiting),
        });
        let id = self
            .submit_classified(Operation::DeferredWrite(state.clone()))
            .map_err(|(_, error)| error)?;
        Ok(DeferredWriteTicket {
            id,
            state,
            shared: Arc::downgrade(&self.shared),
        })
    }
    fn submit_classified(
        &self,
        operation: Operation,
    ) -> Result<u64, (Operation, WriteAdmissionError)> {
        let input = match &operation {
            Operation::Read | Operation::DeferredWrite(_) => 0,
            Operation::Write(text) => text.capacity(),
            Operation::WriteShared(_) => 0,
        };
        let text_bytes = match &operation {
            Operation::WriteShared(text) => text.capacity(),
            _ => input,
        };
        if text_bytes > MAX_TEXT {
            return Err((
                operation,
                WriteAdmissionError::Failed("Clipboard text exceeds 64 MiB".into()),
            ));
        }
        let result = match &operation {
            Operation::Read => MAX_TEXT + MAX_ERROR,
            Operation::DeferredWrite(_) | Operation::Write(_) | Operation::WriteShared(_) => {
                MAX_ERROR
            }
        };
        let reservation = match self.client.try_reserve_external(JobCost {
            input_bytes: input + 4096,
            result_bytes: result + 4096,
        }) {
            Ok(reservation) => reservation,
            Err(error) => {
                let message = format!("Clipboard admission: {error:?}");
                let reason = if matches!(error, ilium_execution::RejectReason::Closed) {
                    WriteAdmissionError::Closed(message)
                } else if matches!(
                    error,
                    ilium_execution::RejectReason::InvalidCost
                        | ilium_execution::RejectReason::AccountingPoisoned
                ) {
                    WriteAdmissionError::Failed(message)
                } else {
                    WriteAdmissionError::Busy(message)
                };
                return Err((operation, reason));
            }
        };
        if let Err(error) = reservation.validate_value_type::<Completion>() {
            return Err((
                operation,
                WriteAdmissionError::Failed(format!("Clipboard completion admission: {error:?}")),
            ));
        }
        let mut queue = match self.shared.queue.try_lock() {
            Ok(queue) => queue,
            Err(TryLockError::WouldBlock) => {
                return Err((
                    operation,
                    WriteAdmissionError::Busy("Clipboard queue busy; retry".into()),
                ));
            }
            Err(TryLockError::Poisoned(_)) => {
                return Err((
                    operation,
                    WriteAdmissionError::Failed("Clipboard owner failed".into()),
                ));
            }
        };
        if queue.closing
            || queue.exited
            || queue.commands.len() + queue.results.len() + usize::from(queue.active)
                >= MAX_COMMANDS
        {
            return Err((
                operation,
                if queue.closing || queue.exited {
                    WriteAdmissionError::Closed(
                        queue
                            .failure
                            .clone()
                            .unwrap_or_else(|| "Clipboard owner closed".into()),
                    )
                } else {
                    WriteAdmissionError::Busy("Clipboard queue unavailable; retry".into())
                },
            ));
        }
        let id = queue.next_id;
        let Some(next) = id.checked_add(1) else {
            return Err((
                operation,
                WriteAdmissionError::Failed("Clipboard sequence exhausted".into()),
            ));
        };
        queue.next_id = next;
        queue.commands.push_back(Pending {
            id,
            operation,
            reservation,
        });
        drop(queue);
        self.shared.changed.notify_one();
        Ok(id)
    }
    pub(crate) fn try_take(&self) -> Option<Retained<Completion>> {
        self.shared.queue.try_lock().ok()?.results.pop_front()
    }
    pub(crate) fn pending(&self) -> bool {
        self.shared
            .queue
            .try_lock()
            .map(|queue| queue.active || !queue.commands.is_empty() || !queue.results.is_empty())
            .unwrap_or(true)
    }
    pub(crate) fn request_shutdown(&self) {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closing = true;
        self.shared.changed.notify_all();
    }
    /// Only observes actual OS-thread joins. A stuck helper is this service's
    /// own child; killing it breaks pipe I/O without touching user processes.
    #[cfg(test)]
    pub(crate) async fn shutdown(self) -> io::Result<()> {
        self.shutdown_with_acknowledgements().await.0
    }
    /// Returns every retained acknowledgement available at the actual join
    /// boundary. A deadline/panic is an unknown native outcome, never success;
    /// the supervisor retains the surviving owner and its native input leaf.
    pub(crate) async fn shutdown_with_acknowledgements(
        mut self,
    ) -> (io::Result<()>, Vec<Retained<Completion>>) {
        self.request_shutdown();
        let Some(worker) = self.worker.take() else {
            return (Err(io::Error::other("clipboard owner missing")), Vec::new());
        };
        let shared = self.shared.clone();
        match tokio::task::spawn_blocking(move || {
            let ticket = worker.ticket();
            let deadline = Instant::now() + Duration::from_secs(5);
            let joined = ticket.join_until(deadline - Duration::from_millis(500));
            if joined.is_err() {
                kill_owned_helper(&shared);
                ticket.cancel();
            }
            let exit = ticket.join_until(deadline);
            drop(worker); // A still-running owner remains supervisor-owned.
            let mut queue = shared
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let completions = queue.results.drain(..).collect::<Vec<_>>(); // <= eight, original holds.
            let result = match exit {
                Err(_) => Err(io::Error::other(
                    "Clipboard owner remains in retiring custody after shutdown deadline",
                )),
                Ok(_) if joined.is_err() => Err(io::Error::other(
                    "Clipboard helper required forced shutdown",
                )),
                Ok(exit) if exit != ilium_platform::owned_worker::WorkerExit::Joined => {
                    Err(io::Error::other("Clipboard owner panicked"))
                }
                Ok(_) => match &queue.failure {
                    Some(error) => Err(io::Error::other(error.clone())),
                    None => Ok(()),
                },
            };
            (result, completions)
        })
        .await
        {
            Ok(receipt) => receipt,
            Err(error) => (Err(io::Error::other(error)), Vec::new()),
        }
    }
}
impl Drop for ClipboardService {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

struct Helper {
    stdin: ChildStdin,
    stdout: ChildStdout,
    shared: Arc<Shared>,
}
impl Helper {
    fn start(shared: Arc<Shared>) -> io::Result<Self> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("clipboard-helper")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(test)]
        if let Some((path, arguments)) = shared
            .helper_command
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            command = Command::new(path);
            command
                .args(arguments)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
        }
        match ilium_platform::child_limits::configure_child_address_space_limit(
            &mut command,
            HELPER_BYTES,
        ) {
            Ok(()) => {}
            // Other platforms retain native clipboard behavior in isolation.
            // Native inbound allocations have no verified hard cap there.
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {}
            Err(error) => return Err(error),
        }
        let mut child = command.spawn()?;
        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("clipboard helper stdin missing"));
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("clipboard helper stdout missing"));
        };
        *shared
            .child
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(child);
        Ok(Self {
            stdin,
            stdout,
            shared,
        })
    }
    fn command(&mut self, id: u64, operation: Operation) -> Result<String, String> {
        exchange(&mut self.stdin, &mut self.stdout, id, operation)
    }
    fn close(&mut self) -> io::Result<()> {
        write_packet(&mut self.stdin, CLOSE, 0, &[])?;
        let (status, id, bytes) = read_packet_limit(&mut self.stdout, 0)?;
        if status != OK || id != 0 || !bytes.is_empty() {
            return Err(io::Error::other(
                "Clipboard shutdown acknowledgement invalid",
            ));
        }
        wait_owned_helper(&self.shared)
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        kill_owned_helper(&self.shared);
        let _ = wait_owned_helper(&self.shared);
    }
}
fn kill_owned_helper(shared: &Shared) {
    if let Some(child) = shared
        .child
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_mut()
    {
        let _ = child.kill();
    }
}
fn wait_owned_helper(shared: &Shared) -> io::Result<()> {
    loop {
        let status = {
            let mut child = shared
                .child
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match child.as_mut() {
                None => return Ok(()),
                Some(child) => child.try_wait()?,
            }
        };
        if let Some(status) = status {
            let child = shared
                .child
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
            if let Some(mut child) = child {
                let _ = child.wait()?;
            }
            return if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!(
                    "Clipboard helper exited: {status}"
                )))
            };
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn exchange<W: Write, R: Read>(
    writer: &mut W,
    reader: &mut R,
    id: u64,
    operation: Operation,
) -> Result<String, String> {
    let (action, bytes): (u8, &[u8]) = match &operation {
        Operation::DeferredWrite(_) => {
            return Err("Unprepared clipboard position reached native exchange".into());
        }
        Operation::Read => (READ, &[]),
        Operation::Write(text) => (WRITE, text.as_bytes()),
        Operation::WriteShared(text) => (WRITE, text.as_bytes()),
    };
    write_packet(writer, action, id, bytes).map_err(|error| error.to_string())?;
    // Keep operation (and its shared original guard) through response read.
    let (status, returned_id, bytes) =
        read_packet_limit(reader, if action == READ { MAX_TEXT } else { 0 })
            .map_err(|error| error.to_string())?;
    if returned_id != id || !matches!(status, OK | ERROR) {
        return Err("Clipboard helper protocol mismatch".into());
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| "Clipboard helper returned invalid UTF-8".to_string())?;
    if status == ERROR {
        Err(text)
    } else {
        Ok(text)
    }
}

fn try_deferred(state: &DeferredState) -> Option<Result<Operation, String>> {
    let mut value = state
        .value
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if matches!(*value, DeferredValue::Waiting) {
        return None;
    }
    match std::mem::replace(&mut *value, DeferredValue::Consumed) {
        DeferredValue::Ready(result) => Some(result.map(Operation::WriteShared)),
        DeferredValue::Consumed => Some(Err("Clipboard position already consumed".into())),
        DeferredValue::Waiting => None,
    }
}
fn resolve_deferred(
    operation: Operation,
    shared: &Shared,
    stop: &StopToken,
) -> Result<Operation, String> {
    let Operation::DeferredWrite(state) = operation else {
        return Ok(operation);
    };
    loop {
        if let Some(result) = try_deferred(&state) {
            return result;
        }
        let queue = shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if queue.closing || stop.is_stopped() {
            drop(queue);
            let mut value = state
                .value
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if matches!(*value, DeferredValue::Waiting) {
                *value = DeferredValue::Ready(Err(
                    "Clipboard shutdown cancelled unprepared copy; not delivered".into(),
                ));
            }
            continue;
        }
        // One existing owner waits at the FIFO head; no later native command
        // is processed and no queue lock spans pipe/native operations.
        drop(
            shared
                .changed
                .wait_timeout(queue, Duration::from_millis(100))
                .unwrap_or_else(|error| error.into_inner()),
        );
    }
}
fn run_owner(shared: Arc<Shared>, stop: StopToken) {
    let mut helper: Option<Helper> = None;
    loop {
        let pending = {
            let mut queue = shared
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            loop {
                if let Some(pending) = queue.commands.pop_front() {
                    queue.active = true;
                    break Some(pending);
                }
                if queue.closing || stop.is_stopped() {
                    break None;
                }
                queue = shared
                    .changed
                    .wait_timeout(queue, Duration::from_millis(100))
                    .unwrap_or_else(|error| error.into_inner())
                    .0;
            }
        };
        let Some(pending) = pending else {
            break;
        };
        let result = match resolve_deferred(pending.operation, &shared, &stop) {
            Err(error) => Err(error),
            Ok(operation) => match helper.as_mut() {
                Some(helper) => helper.command(pending.id, operation),
                None => match Helper::start(shared.clone()) {
                    Ok(mut started) => {
                        let result = started.command(pending.id, operation);
                        helper = Some(started);
                        result
                    }
                    Err(error) => Err(format!("Clipboard helper startup: {error}")),
                },
            },
        };
        let completion = Completion {
            id: pending.id,
            result,
        };
        match pending.reservation.retain(completion) {
            Ok(completion) => shared
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .results
                .push_back(completion),
            Err(_) => {
                shared
                    .queue
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .failure = Some("Clipboard result exceeded reservation".into());
            }
        }
        shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active = false;
        shared.notification.notify_one();
    }
    let close = helper.as_mut().map(Helper::close);
    drop(helper);
    let mut queue = shared
        .queue
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(Err(error)) = close {
        queue.failure = Some(error.to_string());
    }
    queue.exited = true;
    drop(queue);
    shared.notification.notify_one();
}
fn write_packet(writer: &mut impl Write, kind: u8, id: u64, bytes: &[u8]) -> io::Result<()> {
    let limit = if kind == ERROR { MAX_ERROR } else { MAX_TEXT };
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "clipboard packet exceeds limit",
        ));
    }
    let mut header = [0u8; 17];
    header[..4].copy_from_slice(&MAGIC);
    header[4] = kind;
    header[5..13].copy_from_slice(&id.to_le_bytes());
    header[13..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    writer.write_all(&header)?;
    writer.write_all(bytes)?;
    writer.flush()
}
fn read_packet(reader: &mut impl Read) -> io::Result<(u8, u64, Vec<u8>)> {
    read_packet_limit(reader, MAX_TEXT)
}
fn read_packet_limit(reader: &mut impl Read, max_payload: usize) -> io::Result<(u8, u64, Vec<u8>)> {
    let mut header = [0u8; 17];
    reader.read_exact(&mut header)?;
    if header[..4] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "clipboard helper version mismatch",
        ));
    }
    let kind = header[4];
    let id = u64::from_le_bytes(header[5..13].try_into().map_err(io::Error::other)?);
    let length = u32::from_le_bytes(header[13..17].try_into().map_err(io::Error::other)?) as usize;
    let limit = if kind == ERROR {
        MAX_ERROR
    } else {
        max_payload
    };
    if length > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "clipboard advertised payload exceeds limit",
        ));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok((kind, id, bytes))
}
/// Hidden CLI entrypoint, called before building any Tokio runtime. The caller
/// must configure the address-space limit before spawning this helper.
pub fn run_helper() -> io::Result<()> {
    match ilium_platform::child_limits::verify_current_address_space_limit(HELPER_BYTES) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {}
        Err(error) => return Err(error),
    };
    let mut clipboard = arboard::Clipboard::new().map_err(io::Error::other)?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let (kind, id, bytes) = read_packet(&mut input)?;
        if kind == CLOSE {
            if !bytes.is_empty() {
                return Err(io::Error::other("invalid clipboard close"));
            }
            drop(clipboard);
            write_packet(&mut output, OK, id, &[])?;
            return Ok(());
        }
        let result = match kind {
            // Other platforms retain existing paste semantics in the isolated
            // helper. Their native inbound allocation is not a proven hard cap;
            // Linux validates its inherited AS limit before native startup.
            READ if bytes.is_empty() => clipboard.get_text().map_err(|error| error.to_string()),
            WRITE => String::from_utf8(bytes)
                .map_err(|error| error.to_string())
                .and_then(|text| {
                    clipboard
                        .set_text(text)
                        .map(|()| String::new())
                        .map_err(|error| error.to_string())
                }),
            _ => Err("invalid clipboard command".into()),
        };
        match result {
            Ok(text) if text.len() <= MAX_TEXT => {
                write_packet(&mut output, OK, id, text.as_bytes())?
            }
            Ok(_) => write_packet(&mut output, ERROR, id, b"Clipboard text exceeds 64 MiB")?,
            Err(error) => {
                let end = error
                    .char_indices()
                    .map(|(index, _)| index)
                    .take_while(|index| *index <= MAX_ERROR)
                    .last()
                    .unwrap_or(0);
                let error = if error.len() > MAX_ERROR {
                    &error[..end]
                } else {
                    &error
                };
                write_packet(&mut output, ERROR, id, error.as_bytes())?;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn blocked_owned_child_preserves_fifo_and_shutdown_observes_actual_exit() {
        let fixture = tempfile::tempdir().unwrap();
        let ready = fixture.path().join("ready");
        let release = fixture.path().join("release");
        // Fixture runs only in a task-owned child and retains clipboard state.
        // The native library is deliberately not faked in release acceptance;
        // here the forcing property is OS pipe blocking and ownership/order.
        let script = format!(
            r#"import struct,sys,time,pathlib
ready=pathlib.Path({ready:?})
release=pathlib.Path({release:?})
text=b'seed'
first=True
def exact(n):
    b=b''
    while len(b)<n:
        part=sys.stdin.buffer.read(n-len(b))
        if not part: raise EOFError()
        b+=part
    return b
while True:
    magic,kind,identity,length=struct.unpack('<4sBQI',exact(17))
    if magic!=b'ICB1' or length>67108864: raise RuntimeError('bad frame')
    data=exact(length)
    if kind==3:
        sys.stdout.buffer.write(struct.pack('<4sBQI',b'ICB1',4,identity,0));sys.stdout.buffer.flush();break
    if kind==1 and first:
        ready.write_text('blocked');first=False
        while not release.exists():time.sleep(0.001)
    if kind==2:text=data
    output=text if kind==1 else b''
    sys.stdout.buffer.write(struct.pack('<4sBQI',b'ICB1',4,identity,len(output))+output)
    sys.stdout.buffer.flush()
"#,
            ready = ready.to_string_lossy(),
            release = release.to_string_lossy()
        );
        let service = ClipboardService::start(crate::execution::test_client()).unwrap();
        struct Cleanup(Arc<Shared>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                self.0
                    .queue
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .closing = true;
                self.0.changed.notify_all();
                kill_owned_helper(&self.0);
            }
        }
        let _cleanup = Cleanup(service.shared.clone());
        *service.shared.helper_command.lock().unwrap() = Some((
            std::path::PathBuf::from("/usr/bin/python3"),
            vec!["-c".into(), script.into()],
        ));
        let read = service
            .submit(Operation::Read)
            .map_err(|(_, error)| error)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(
                Instant::now() < deadline,
                "owned child failed to enter blocked read"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let began = Instant::now();
        let write = service
            .submit(Operation::Write("after".into()))
            .map_err(|(_, error)| error)
            .unwrap();
        assert!(
            began.elapsed() < Duration::from_millis(100),
            "producer waited for blocked native I/O"
        );
        let tail = service
            .submit(Operation::Read)
            .map_err(|(_, error)| error)
            .unwrap();
        assert!(service.try_take().is_none());
        assert!(service.pending());
        std::fs::write(&release, b"release").unwrap();
        let mut results = Vec::new();
        while results.len() < 3 {
            if let Some(result) = service.try_take() {
                let mut owned = None;
                let hold = result.map(|result| owned = Some((result.id, result.result)));
                results.push(owned.unwrap());
                drop(hold);
            } else {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert_eq!(
            results,
            vec![
                (read, Ok("seed".into())),
                (write, Ok(String::new())),
                (tail, Ok("after".into()))
            ]
        );
        let shared = service.shared.clone();
        service.shutdown().await.unwrap();
        assert!(shared.child.lock().unwrap().is_none());
        assert!(shared.queue.lock().unwrap().exited);
    }
    #[test]
    fn framed_commands_preserve_unicode_and_order() {
        let mut bytes = Vec::new();
        write_packet(&mut bytes, WRITE, 1, "雪\u{1b}[31m".as_bytes()).unwrap();
        write_packet(&mut bytes, READ, 2, &[]).unwrap();
        let mut reader = bytes.as_slice();
        assert_eq!(
            read_packet(&mut reader).unwrap(),
            (WRITE, 1, "雪\u{1b}[31m".as_bytes().to_vec())
        );
        assert_eq!(read_packet(&mut reader).unwrap(), (READ, 2, Vec::new()));
        assert!(reader.is_empty());
    }
    #[test]
    fn oversized_header_is_rejected_before_payload_read() {
        let mut header = [0u8; 17];
        header[..4].copy_from_slice(&MAGIC);
        header[4] = OK;
        header[13..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            read_packet(&mut header.as_slice()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    #[test]
    fn truncated_reply_never_acknowledges() {
        let mut bytes = Vec::new();
        write_packet(&mut bytes, OK, 9, b"partial").unwrap();
        bytes.pop();
        assert_eq!(
            read_packet(&mut bytes.as_slice()).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}

#[cfg(test)]
mod shared_write_tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::mpsc;
    struct DelayedResponse {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        response: Cursor<Vec<u8>>,
        waiting: bool,
    }
    impl Read for DelayedResponse {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if !self.waiting {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
                self.waiting = true;
            }
            self.response.read(bytes)
        }
    }
    #[test]
    fn original_shared_leaf_is_kept_through_blocking_response_without_text_copy() {
        let client = crate::execution::test_client();
        let leaf = client
            .retirement()
            .try_reserve::<String>(8192)
            .unwrap()
            .attach_shared("exact original α".into());
        let weak = Arc::downgrade(&leaf);
        let pointer = leaf.as_ptr();
        let mut reply = Vec::new();
        write_packet(&mut reply, OK, 83, &[]).unwrap();
        let (entered, receive_entered) = mpsc::channel();
        let (release, receive_release) = mpsc::channel();
        let join = std::thread::spawn(move || {
            let mut writer = Vec::new();
            let mut reader = DelayedResponse {
                entered,
                release: receive_release,
                response: Cursor::new(reply),
                waiting: false,
            };
            let result = exchange(&mut writer, &mut reader, 83, Operation::WriteShared(leaf));
            (result, writer)
        });
        receive_entered
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let retained = weak
            .upgrade()
            .expect("original must survive while native response is blocked");
        assert_eq!(retained.as_ptr(), pointer);
        assert_eq!(retained.as_str(), "exact original α");
        drop(retained);
        release.send(()).unwrap();
        let (result, wire) = join.join().unwrap();
        assert!(result.is_ok());
        assert!(
            weak.upgrade().is_none(),
            "last shared input wrapper ends only after response"
        );
        let (action, id, bytes) = read_packet_limit(&mut Cursor::new(wire), MAX_TEXT).unwrap();
        assert_eq!((action, id), (WRITE, 83));
        assert_eq!(bytes, "exact original α".as_bytes());
    }
    #[test]
    fn raw_write_protocol_remains_identical_and_bad_ack_is_not_success() {
        let mut response = Vec::new();
        write_packet(&mut response, OK, 92, &[]).unwrap();
        let mut wire = Vec::new();
        let result = exchange(
            &mut wire,
            &mut Cursor::new(response),
            91,
            Operation::Write("raw original".into()),
        );
        assert!(result.is_err());
        let (action, id, bytes) = read_packet_limit(&mut Cursor::new(wire), MAX_TEXT).unwrap();
        assert_eq!((action, id), (WRITE, 91));
        assert_eq!(bytes, b"raw original");
    }
}
