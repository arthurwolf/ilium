//! The approval channel of a grant request: a connected client answers, a script answers, or the
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
pub struct ApprovalChannel;

impl ApprovalChannel {
    /// A channel with an attached client.
    pub fn with_client() -> ApprovalChannel {
        unimplemented!("host::approval::ApprovalChannel::with_client")
    }

    /// A channel without a client (headless CLI).
    pub fn without_client() -> ApprovalChannel {
        unimplemented!("host::approval::ApprovalChannel::without_client")
    }

    /// A scripted answer for the next request.
    pub fn script_answer(&mut self, _yes: bool) {
        unimplemented!("host::approval::ApprovalChannel::script_answer")
    }

    /// Asks for `grants` of `plugin`; `Ok(true)` when approved.
    pub fn ask(&mut self, _plugin: &str, _grants: &[Grant]) -> Result<bool, ApprovalError> {
        unimplemented!("host::approval::ApprovalChannel::ask")
    }
}
