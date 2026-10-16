//! reference-test-harness.md TypeScript and Bun rules: DEV-422, DEV-427, DEV-429, DEV-452.
//!
//! The TypeScript harness is not part of this crate. Each rule is checked in two places that exist
//! here: the normative statement and its code sample on the manual page (`doc_` tests, so the page
//! cannot drift from the rule), and the Rust half of the same behaviour where one exists.

use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::FakeHost;

use crate::common::{Spy, spy, spy_reset, with_clean_env};

fn page_text() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages/reference-test-harness.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("manual page {}: {error}", path.display()))
}

fn rule_line(id: &str) -> String {
    let marker = format!("**[{id}]**");
    page_text().lines().find(|line| line.contains(&marker)).unwrap_or_else(|| panic!("{id} missing on the page")).to_string()
}

/// REQ: DEV-422
#[test]
fn doc_422_the_page_requires_a_fresh_import_and_forbids_a_cached_module() {
    let line = rule_line("DEV-422");
    assert!(line.contains("`activateModule` MUST import a fresh copy of the module on each call"), "{line}");
    assert!(line.contains("A module cached by the runtime MUST NOT be reused."), "{line}");
    assert!(page_text().contains("| none | `host.activateModule(path): Promise<PluginHandle>` | A `PluginHandle`. Imports a fresh copy of the module. |"));
}

/// REQ: DEV-422
#[test]
fn dev_422_each_activation_builds_a_fresh_plugin_so_state_never_leaks_between_hosts() {
    with_clean_env(|| {
        spy_reset();
        let mut first = FakeHost::new();
        first.activate::<Spy>();
        let mut second = FakeHost::new();
        second.activate::<Spy>();
        let log = spy();
        assert_eq!(log.constructed, 2, "one new plugin value per activation");
        assert_eq!(log.activations.len(), 2);
    });
}

/// REQ: DEV-427
#[test]
fn doc_427_the_page_names_the_preload_file_and_the_activate_module_advice() {
    let line = rule_line("DEV-427");
    assert!(line.contains("`@ilium/plugin-sdk/testing/setup` MUST reset the harness registry before each test"), "{line}");
    assert!(line.contains("Import the plugin with `activateModule`, not with a top-level `import`"), "{line}");
}

/// REQ: DEV-427
#[test]
fn dev_427_a_second_host_in_the_same_process_starts_from_a_clean_registry() {
    with_clean_env(|| {
        spy_reset();
        let mut first = FakeHost::new();
        first.activate::<Spy>();
        spy_reset();
        let mut second = FakeHost::new();
        second.activate::<Spy>();
        assert_eq!(spy().activations.len(), 1, "nothing from the first host is visible after the reset");
    });
}

/// REQ: DEV-429
#[test]
fn doc_429_the_page_lists_the_promise_returning_calls_and_the_synchronous_deliver() {
    let line = rule_line("DEV-429");
    for call in ["`activate({ lifecycle, handler })`", "`tryActivate`", "`activateModule(path)`", "`deactivate`", "`settle()`"] {
        assert!(line.contains(call), "{call} missing in {line}");
    }
    assert!(line.contains("MUST return promises"));
    assert!(line.contains("`deliver(events)` MUST be synchronous and return `Submission[]`"));
    let text = page_text();
    assert!(text.contains("`host.settle(): Promise<void>`"));
    assert!(text.contains("`host.deactivate(reason): Promise<Uint8Array \\| undefined>`"));
}

/// REQ: DEV-429
#[test]
fn dev_429_rust_deliver_is_synchronous_and_returns_the_submissions() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        let submissions: Vec<ilium_plugin_sdk_core::types::Submission> = host.deliver(crate::common::client_batch(1));
        assert!(submissions.is_empty(), "the spy script is empty");
        assert_eq!(spy().batches.len(), 1);
    });
}

/// REQ: DEV-452
#[test]
fn doc_452_the_page_states_the_three_refused_arguments_and_the_code() {
    let line = rule_line("DEV-452");
    assert!(line.contains("A negative number, a fraction or an unsafe integer passed to `advanceClock` MUST fail with `harness.clock-argument`."), "{line}");
    assert!(page_text().contains("A number must be a non-negative safe integer."));
}

/// REQ: DEV-452
#[test]
fn dev_452_the_clock_argument_code_is_the_dotted_harness_code() {
    assert_eq!(codes::CLOCK_ARGUMENT, "harness.clock-argument");
    assert_ne!(codes::CLOCK_ARGUMENT, codes::CLOCK_BACKWARDS);
}

/// REQ: DEV-452
#[test]
fn dev_452_rust_advance_clock_accepts_every_non_negative_integer_up_to_the_safe_limit() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        let start = host.clock().now();
        host.advance_clock((1u64 << 53) - 1);
        assert_eq!(host.clock().now(), start + (1u64 << 53) - 1);
    });
}
