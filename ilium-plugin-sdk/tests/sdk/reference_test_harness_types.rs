//! reference-test-harness.md "Testing module surface": DEV-563, and the DEV-522 strategy.

use ilium_plugin_sdk::testing::{Cell, Diagnostic, Mode, PluginHandle, Restart, Screen};

/// REQ: DEV-563
#[test]
fn dev_563_mode_has_native_and_component() {
    assert_ne!(Mode::Native, Mode::Component);
}

/// REQ: DEV-563
#[test]
fn dev_563_screen_and_cell_are_constructible_with_every_field() {
    let cell = Cell::default();
    let screen = Screen {
        columns: 1,
        rows: 1,
        cells: vec![cell.clone()],
    };
    assert_eq!(screen.cells[0], cell);
}

/// REQ: DEV-563
#[test]
fn dev_563_plugin_handle_diagnostic_and_restart_have_their_fields() {
    let handle = PluginHandle {
        side: "server".into(),
        instance: 1,
    };
    let diagnostic = Diagnostic {
        code: "x.y".into(),
        message: "m".into(),
    };
    let restart = Restart {
        at_ms: 5,
        delay_ms: 10,
    };
    assert_eq!(handle.instance, 1);
    assert_eq!(diagnostic.code, "x.y");
    assert_eq!(restart.at_ms + u64::from(restart.delay_ms), 15);
}

/// REQ: DEV-563
#[cfg(testing_reexports)]
#[test]
fn dev_563_invocation_and_submission_are_reexported() {
    use ilium_plugin_sdk::testing::{Invocation, Submission};
    let _ = std::any::type_name::<Invocation>();
    let _ = std::any::type_name::<Submission>();
}

/// REQ: DEV-522
#[cfg(testing_reexports)]
#[test]
fn dev_522_arbitrary_event_takes_no_argument_and_is_a_strategy() {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::TestRunner;
    let strategy = ilium_plugin_sdk::testing::arbitrary_event();
    let mut runner = TestRunner::deterministic();
    let tree = strategy.new_tree(&mut runner).expect("a value");
    let _event = tree.current();
}
