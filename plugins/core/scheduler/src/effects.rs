//! Coalescing mailbox of follow-up work requested by the detection sweep: one counter per kind
//! of request, so a busy consumer never grows a task list. Ported from the `Requests` value of
//! ilium-server/src/detection_effects.rs; the completion-epoch half lives in `queue.rs`.

/// Pending follow-up requests as monotonically increasing counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Requests {
    pub title_checks: u64,
    pub completions: u64,
    pub completion_epoch: u64,
}

impl Requests {
    /// Records a request; counters wrap rather than panic.
    pub fn request(&mut self, check_title: bool, deliver_prompt: bool, completion_epoch: u64) {
        if check_title {
            self.title_checks = self.title_checks.wrapping_add(1);
        }
        if deliver_prompt {
            self.completions = self.completions.wrapping_add(1);
            self.completion_epoch = completion_epoch;
        }
    }

    /// The work a consumer that last handled `handled` must still do.
    pub fn outstanding(&self, handled: &Requests) -> Outstanding {
        Outstanding {
            title_check: self.title_checks != handled.title_checks,
            completion: self.completions != handled.completions,
            completion_epoch: self.completion_epoch,
        }
    }
}

/// What a consumer must do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outstanding {
    pub title_check: bool,
    pub completion: bool,
    pub completion_epoch: u64,
}
