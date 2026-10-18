//! Twin of ilium/tests/recap_isolated.rs: a selected shell screen reaches the model, redacted.

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium/tests/recap_isolated.rs::recap_selected_shell_screen_reaches_fake_inference_with_matching_binaries
#[test]
fn port_isolated_selected_shell_screen_reaches_the_fake_model() {
    let mut host = activated_server();
    open_gate(&mut host);
    let selected = host.tree().add(shell_pane("selected shell"));
    let other = host.tree().add(shell_pane("other shell"));
    host.tree().set_screen(
        selected,
        rows(&[
            "SELECTED_RECAP_MARKER",
            "export TOKEN_VALUE=fakecredential12345",
        ]),
    );
    host.tree()
        .set_screen(other, rows(&["UNSELECTED_RECAP_MARKER"]));
    let invocation = host
        .cli()
        .invoke(&["recap", "--pane", &selected.to_string()], b"");
    let evidence = job_evidence(&mut host);
    assert!(evidence.contains("SELECTED_RECAP_MARKER"));
    assert!(!evidence.contains("UNSELECTED_RECAP_MARKER"));
    assert!(!evidence.contains("fakecredential12345"));
    let records = finish_run(&mut host, &invocation, answer(4, 8));
    let record = first_recap(&records);
    assert_eq!(record["pane_name"], "selected shell");
    assert_eq!(record["short"]["text"], words(4));
    assert!(!host
        .cli()
        .refresh(&invocation)
        .stdout()
        .contains("fakecredential12345"));
}

/// REQ: PORT:ilium/tests/recap_isolated.rs::recap_selected_shell_screen_reaches_fake_inference_with_matching_binaries
#[test]
fn port_isolated_closed_gate_denies_without_inference() {
    let (mut host, _, _) = start_shell_run(&["SELECTED_RECAP_MARKER"]);
    close_gate(&mut host);
    let invocation = host.cli().invoke(&["recap"], b"");
    let record = first_recap(&jsonl(&host.cli().refresh(&invocation).stdout()));
    assert_eq!(record["state"], "denied");
}
