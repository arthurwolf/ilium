//! The approval channel of a grant request: a connected client answers, or the
//! request needs the review dialog (reference-grant-review-and-consent.md).

use ilium_plugin_api::grants::Grant;

/// Why a grant request could not be answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalError {
    /// No client is attached and no script answer is set: the review dialog is required.
    ReviewRequired,
}

/// Where a grant request is answered.
#[derive(Debug)]
pub struct ApprovalChannel {
    client_attached: bool,
    next_answer: Option<bool>,
}

impl ApprovalChannel {
    /// A channel with an attached client.
    pub fn with_client() -> ApprovalChannel {
        ApprovalChannel { client_attached: true, next_answer: None }
    }

    /// A channel without a client (headless CLI).
    pub fn without_client() -> ApprovalChannel {
        ApprovalChannel { client_attached: false, next_answer: None }
    }

    /// Injects the attached client's next answer for controlled tests. This is
    /// not an implementation of `--yes` or permission to bypass a client.
    pub fn script_answer(&mut self, yes: bool) {
        self.next_answer = Some(yes);
    }

    /// Asks for `grants` of `plugin`; `Ok(true)` when approved.
    pub fn ask(&mut self, _plugin: &str, _grants: &[Grant]) -> Result<bool, ApprovalError> {
        let answer = self.next_answer.take();
        if !self.client_attached {
            return Err(ApprovalError::ReviewRequired);
        }
        answer.ok_or(ApprovalError::ReviewRequired)
    }
}
