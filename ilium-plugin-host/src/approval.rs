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

#[cfg(test)]
mod tests {
    use super::{ApprovalChannel, ApprovalError};
    use ilium_plugin_api::grants::Grant;

    /// Plugin id used by every request; the manual's examples use the `com.example.*` form.
    const PLUGIN_ID: &str = "com.example.approval";

    /// A normal (low-risk) grant, `tree.read` (reference-capabilities.md capability table).
    const NORMAL_GRANT: &str = "tree.read";

    /// A high-risk grant, `agent.transcript` (reference-capabilities.md marks it H).
    const HIGH_RISK_GRANT: &str = "agent.transcript";

    /// Parses a grant string from the manual; a parse failure panics with the text in the message.
    fn grant(text: &str) -> Grant {
        match Grant::parse(text) {
            Ok(parsed_grant) => parsed_grant,
            Err(diagnostic) => panic!("`{text}` should parse, but it was refused: {diagnostic}"),
        }
    }

    /// Without a client and without a scripted answer, a normal grant needs the review dialog.
    /// REQ: SEC-116
    #[test]
    fn headless_ask_without_scripted_answer_requires_review() {
        let mut channel = ApprovalChannel::without_client();
        let grants = [grant(NORMAL_GRANT)];

        let outcome = channel.ask(PLUGIN_ID, &grants);

        assert_eq!(outcome, Err(ApprovalError::ReviewRequired));
    }

    /// A scripted yes answers a normal-grant request without any client, as `--yes` does.
    /// REQ: SEC-116
    #[test]
    fn headless_scripted_yes_approves_normal_grant() {
        let mut channel = ApprovalChannel::without_client();
        channel.script_answer(true);
        let grants = [grant(NORMAL_GRANT)];

        let outcome = channel.ask(PLUGIN_ID, &grants);

        assert_eq!(outcome, Ok(true));
    }

    /// A scripted no answer is a decline: `Ok(false)`, not a review requirement.
    /// REQ: SEC-116
    #[test]
    fn headless_scripted_no_declines_normal_grant() {
        let mut channel = ApprovalChannel::without_client();
        channel.script_answer(false);
        let grants = [grant(NORMAL_GRANT)];

        let outcome = channel.ask(PLUGIN_ID, &grants);

        assert_eq!(outcome, Ok(false));
    }

    /// A scripted answer is for the next request only; the request after it needs review again.
    /// REQ: SEC-116
    #[test]
    fn scripted_answer_is_consumed_by_the_next_request() {
        let mut channel = ApprovalChannel::without_client();
        channel.script_answer(true);
        let grants = [grant(NORMAL_GRANT)];

        let first_outcome = channel.ask(PLUGIN_ID, &grants);
        let second_outcome = channel.ask(PLUGIN_ID, &grants);

        assert_eq!(first_outcome, Ok(true));
        assert_eq!(second_outcome, Err(ApprovalError::ReviewRequired));
    }

    /// Setting a second scripted answer before the next request replaces the first one.
    /// REQ: SEC-116
    #[test]
    fn later_scripted_answer_replaces_earlier_one() {
        let mut channel = ApprovalChannel::without_client();
        channel.script_answer(true);
        channel.script_answer(false);
        let grants = [grant(NORMAL_GRANT)];

        let outcome = channel.ask(PLUGIN_ID, &grants);

        assert_eq!(outcome, Ok(false));
    }

    /// A scripted yes does not confirm a high-risk grant: only `--grant <grant>` for that exact
    /// grant does, so the request still needs the review dialog.
    /// REQ: SEC-117
    #[test]
    fn scripted_yes_does_not_confirm_high_risk_grant() {
        let mut channel = ApprovalChannel::without_client();
        channel.script_answer(true);
        let grants = [grant(HIGH_RISK_GRANT)];

        let outcome = channel.ask(PLUGIN_ID, &grants);

        assert_eq!(outcome, Err(ApprovalError::ReviewRequired));
    }

    /// A list mixing a normal grant with a high-risk one is held to the high-risk rule.
    /// REQ: SEC-117
    #[test]
    fn scripted_yes_does_not_confirm_mixed_list_with_high_risk_grant() {
        let mut channel = ApprovalChannel::without_client();
        channel.script_answer(true);
        let grants = [grant(NORMAL_GRANT), grant(HIGH_RISK_GRANT)];

        let outcome = channel.ask(PLUGIN_ID, &grants);

        assert_eq!(outcome, Err(ApprovalError::ReviewRequired));
    }

    /// An empty grant list with a scripted yes is approved: there is nothing the review dialog
    /// would list. The manual does not state this case; see the report for the assumption.
    /// REQ: PKG-429
    #[test]
    fn scripted_yes_with_empty_grant_list_is_approved() {
        let mut channel = ApprovalChannel::without_client();
        channel.script_answer(true);

        let outcome = channel.ask(PLUGIN_ID, &[]);

        assert_eq!(outcome, Ok(true));
    }

    /// The only error of this channel is `ReviewRequired`, the variant behind the documented
    /// `install.review-required` code; a missing review yields exactly that value, and it is
    /// cloneable and comparable like the rest of the error type.
    /// REQ: PKG-430
    #[test]
    fn review_required_error_is_the_only_error_variant_for_missing_review() {
        let mut channel = ApprovalChannel::without_client();
        let grants = [grant(NORMAL_GRANT)];

        let outcome = channel.ask(PLUGIN_ID, &grants);

        assert_eq!(outcome, Err(ApprovalError::ReviewRequired));
        assert_eq!(
            ApprovalError::ReviewRequired.clone(),
            ApprovalError::ReviewRequired
        );
    }
}
