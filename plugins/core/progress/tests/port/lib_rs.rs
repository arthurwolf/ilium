//! Status-message classification (original ilium-client/src/lib.rs).

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/lib.rs::user_visible_failures_are_promoted_without_misclassifying_progress
#[test]
fn port_061_the_running_status_segment_is_progress_and_not_a_reported_failure() {
    let mut host = activated_client();
    show(&mut host, vec![monitor(1, "running", 10.0, "Testing inference provider")]);
    let texts = format!("{:?}", segments(&host));
    assert!(texts.contains("1 running"), "{texts}");
    assert!(host.failures().is_empty(), "{:?}", host.failures());
}
