//! Native audio retirement on the existing finite I/O bank.
use std::time::{Duration, Instant};

use ilium_execution::{Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt};
use ilium_voice::AudioCustody;

struct JoinAudio {
    custody: AudioCustody,
}
struct JoinedAudio {
    custody: AudioCustody,
    result: std::io::Result<()>,
}
impl Job for JoinAudio {
    type Output = JoinedAudio;
    type Error = std::convert::Infallible;
    fn run(self, _context: JobContext) -> Result<Self::Output, Self::Error> {
        // A bounded observation deadline is not permission to release owners.
        let result = self
            .custody
            .join_until(Instant::now() + Duration::from_secs(2));
        Ok(JoinedAudio {
            custody: self.custody,
            result,
        })
    }
}

/// One native retirement per voice role. No replacement actor may start while
/// this owner is pending. A failed admission retains the exact original custody.
pub(crate) struct VoiceRetirement {
    client: Client,
    pending: Option<AudioCustody>,
    receipt: Option<Receipt<JoinAudio>>,
    active_custody: Option<AudioCustody>,
    retry_at: Option<Instant>,
    failure: Option<String>,
    terminal_failure: bool,
}
impl VoiceRetirement {
    pub(crate) fn new(client: Client) -> Self {
        Self {
            client,
            pending: None,
            receipt: None,
            active_custody: None,
            retry_at: None,
            failure: None,
            terminal_failure: false,
        }
    }
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.is_some() || self.receipt.is_some()
    }
    pub(crate) fn retain(&mut self, custody: AudioCustody) -> Result<(), AudioCustody> {
        if self.is_pending() {
            return Err(custody);
        }
        self.pending = Some(custody);
        self.retry_at = None;
        Ok(())
    }
    pub(crate) fn collect(&mut self, now: Instant) -> bool {
        if let Some(receipt) = &mut self.receipt {
            let outcome = match receipt.try_take() {
                JobPoll::Pending => return false,
                JobPoll::Ready(outcome) => Some(outcome),
                JobPoll::Lost | JobPoll::Taken => None,
            };
            self.receipt = None;
            let Some(outcome) = outcome else {
                self.pending = self.active_custody.take();
                self.failure = Some(
                    "Voice native observer lost its receipt; original custody retained".into(),
                );
                self.retry_at = Some(now + Duration::from_millis(100));
                return false;
            };
            let (outcome, _envelope) = outcome.into_parts();
            match outcome {
                JobOutcome::Finished(Ok(joined)) => {
                    self.active_custody = None;
                    if let Err(error) = joined.result {
                        self.failure = Some(format!("Voice native retirement: {error}"));
                        // A panic is an observed joined failure; a deadline can
                        // still leave owners pending and must retain the custody.
                        if joined.custody.pending_owners() != 0 {
                            self.pending = Some(joined.custody);
                            self.retry_at = Some(now + Duration::from_millis(100));
                        } else {
                            self.terminal_failure = true;
                        }
                    }
                }
                JobOutcome::NotStarted { job, .. } => {
                    self.active_custody = None;
                    self.pending = Some(job.custody);
                    self.retry_at = Some(now + Duration::from_millis(100));
                }
                JobOutcome::Panicked => {
                    self.pending = self.active_custody.take();
                    self.retry_at = Some(now + Duration::from_millis(100));
                    self.failure = Some(
                        "Voice native observer panicked; physical retirement is unverified".into(),
                    );
                }
                JobOutcome::Finished(Err(never)) => match never {},
            }
        }
        if self.retry_at.is_some_and(|deadline| now < deadline) {
            return false;
        }
        let Some(custody) = self.pending.take() else {
            return true;
        };
        let original = custody.clone();
        match self.client.try_submit(
            Lane::Io,
            JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            },
            JoinAudio { custody },
        ) {
            Ok(receipt) => {
                self.receipt = Some(receipt);
                self.active_custody = Some(original);
                self.retry_at = None;
            }
            Err(rejected) => {
                self.pending = Some(rejected.value.custody);
                self.retry_at = Some(now + Duration::from_millis(100));
                self.failure = Some(format!(
                    "Voice native observer admission: {:?}",
                    rejected.reason
                ));
            }
        }
        false
    }
    pub(crate) fn has_terminal_failure(&self) -> bool {
        self.terminal_failure
    }
    pub(crate) fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }
}
