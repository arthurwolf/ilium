//! UI-side ownership of bounded, ordered logging transitions. Filesystem work
//! belongs to ilium-logging's service; this adapter only awaits its receipts.
use std::collections::VecDeque;
use std::time::Duration;

use ilium_logging::{LoggingError, LoggingReceipt};

const MAX_TRANSITIONS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Client,
    Server,
}
#[derive(Clone, Copy)]
struct Request {
    enabled: bool,
    origin: Origin,
}
pub struct Completion {
    pub enabled: bool,
    pub origin: Origin,
    pub result: Result<(), LoggingError>,
    pub is_pending: bool,
}

pub struct DebugLogging {
    requests: VecDeque<Request>,
    active: Option<LoggingReceipt>,
    // Server broadcasts are replaceable state. Overflow retains the newest
    // authoritative state until the admitted FIFO has capacity; never echo it.
    pending_server: Option<bool>,
    failed_desired: Option<bool>,
    confirmed_enabled: bool,
}
impl Default for DebugLogging {
    fn default() -> Self {
        Self {
            requests: VecDeque::new(),
            active: None,
            pending_server: None,
            failed_desired: None,
            confirmed_enabled: ilium_logging::is_enabled(),
        }
    }
}
impl DebugLogging {
    pub fn is_pending(&self) -> bool {
        !self.requests.is_empty()
    }
    pub fn confirmed_enabled(&self) -> bool {
        self.confirmed_enabled
    }
    pub fn desired_enabled(&self) -> bool {
        self.pending_server
            .or_else(|| self.requests.back().map(|request| request.enabled))
            .or(self.failed_desired)
            .unwrap_or(self.confirmed_enabled)
    }
    pub fn request(&mut self, enabled: bool) -> Result<(), LoggingError> {
        self.enqueue(Request {
            enabled,
            origin: Origin::Client,
        })
    }
    pub fn synchronize(&mut self, enabled: bool) {
        if self.requests.len() >= MAX_TRANSITIONS {
            self.pending_server = Some(enabled);
        } else {
            self.requests.push_back(Request {
                enabled,
                origin: Origin::Server,
            });
        }
    }
    fn enqueue(&mut self, request: Request) -> Result<(), LoggingError> {
        if self.requests.len() >= MAX_TRANSITIONS {
            return Err(LoggingError::AdmissionBusy);
        }
        self.failed_desired = None;
        self.requests.push_back(request);
        Ok(())
    }
    /// Select this future alongside input/server events. Receipt completion
    /// wakes it; admission overflow retries at a bounded cadence without I/O.
    pub async fn next_completion(&mut self) -> Completion {
        loop {
            let Some(request) = self.requests.front().copied() else {
                return std::future::pending().await;
            };
            if self.active.is_none() {
                match ilium_logging::request_set_enabled(request.enabled) {
                    Ok(receipt) => self.active = Some(receipt),
                    Err(LoggingError::AdmissionBusy) => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    Err(error) => return self.complete(request, Err(error)),
                }
            }
            // Borrow the receipt in place: cancelling a select branch must not
            // discard an accepted transition or its completion acknowledgement.
            let result = match self.active.as_mut() {
                Some(receipt) => receipt.await,
                None => continue,
            };
            return self.complete(request, result);
        }
    }
    fn complete(&mut self, request: Request, result: Result<(), LoggingError>) -> Completion {
        self.active = None;
        self.requests.pop_front();
        self.confirmed_enabled = ilium_logging::is_enabled();
        self.failed_desired = result.is_err().then_some(request.enabled);
        if let Some(enabled) = self.pending_server.take() {
            self.requests.push_back(Request {
                enabled,
                origin: Origin::Server,
            });
        }
        Completion {
            enabled: request.enabled,
            origin: request.origin,
            result,
            is_pending: !self.requests.is_empty(),
        }
    }
    /// Finish already accepted and queued state changes before the final log.
    /// The process owner still owes a final flush/shutdown after its last event.
    pub async fn drain(&mut self) -> Result<(), LoggingError> {
        let mut first_error = None;
        while !self.requests.is_empty() {
            let completion = self.next_completion().await;
            if let Err(error) = completion.result {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_transitions_are_bounded_and_server_sync_never_becomes_client_origin() {
        // The production logger is a process-global singleton. A dedicated
        // child prevents this forcing test from changing concurrent App tests.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "debug_logging::tests::logging_transition_child",
                "--ignored",
                "--nocapture",
            ])
            .env("ILIUM_LOGGING_ADAPTER_TEST_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child forcing test failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    #[ignore = "process-global logging; launched by the isolated parent test"]
    async fn logging_transition_child() {
        assert_eq!(
            std::env::var("ILIUM_LOGGING_ADAPTER_TEST_CHILD").as_deref(),
            Ok("1")
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("logging.log");
        let quota = crate::execution::bootstrap_process_quota().unwrap();
        ilium_logging::initialize(&path, false, "client-logging-test", &quota).unwrap();
        let mut logging = DebugLogging::default();
        for index in 0..MAX_TRANSITIONS {
            logging.request(index % 2 == 0).unwrap();
        }
        assert!(matches!(
            logging.request(true),
            Err(LoggingError::AdmissionBusy)
        ));
        logging.synchronize(true);
        logging.synchronize(false); // only unadmitted replaceable server state coalesces
        for index in 0..MAX_TRANSITIONS {
            let completion = logging.next_completion().await;
            completion.result.unwrap();
            assert_eq!(completion.enabled, index % 2 == 0);
            assert_eq!(completion.origin, Origin::Client);
            assert_eq!(logging.confirmed_enabled(), completion.enabled);
        }
        let synchronized = logging.next_completion().await;
        synchronized.result.unwrap();
        assert_eq!(synchronized.origin, Origin::Server);
        assert!(!synchronized.enabled);
        assert!(!synchronized.is_pending);
        logging.request(true).unwrap();
        logging.drain().await.unwrap();
        tracing::info!("acknowledged client event");
        ilium_logging::request_flush().unwrap().await.unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("acknowledged client event"));
        ilium_logging::request_shutdown().unwrap().await.unwrap();
    }
}
