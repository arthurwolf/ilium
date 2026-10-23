use ilium_plugin_host::approval::{ApprovalChannel, ApprovalError};

#[test]
fn attachment_is_not_approval_and_answers_are_consumed_once() {
    let mut channel = ApprovalChannel::with_client();
    assert_eq!(channel.ask("example.plugin", &[]), Err(ApprovalError::ReviewRequired));
    channel.script_answer(true);
    assert_eq!(channel.ask("example.plugin", &[]), Ok(true));
    assert_eq!(channel.ask("example.plugin", &[]), Err(ApprovalError::ReviewRequired));
    channel.script_answer(false);
    assert_eq!(channel.ask("example.plugin", &[]), Ok(false));
}

#[test]
fn headless_script_cannot_bypass_attached_client_confirmation() {
    let mut channel = ApprovalChannel::without_client();
    channel.script_answer(true);
    assert_eq!(channel.ask("example.plugin", &[]), Err(ApprovalError::ReviewRequired));
    assert_eq!(channel.ask("example.plugin", &[]), Err(ApprovalError::ReviewRequired));
}
