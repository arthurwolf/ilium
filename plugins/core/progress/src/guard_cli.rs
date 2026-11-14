//! The command line of the progress guard: `ilium progress-guard`.
//!
//! The `ilium` binary forwards `ilium progress guard` (the command the Claude Code hook runs)
//! to this command; the decision itself is `guard::hook_output`. The launcher hands the caller's
//! `ILIUM_PANE_ID` over as a leading `--ilium-env=ILIUM_PANE_ID=<id>` word (the manifest's `env`),
//! the hook payload arrives on standard input, and the exit status is always 0: a refusal is a
//! JSON `permissionDecision` on standard output, never a failing exit code.

use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_sdk::host::HostCli;
use ilium_plugin_sdk::HostApi;

use crate::guard::hook_output;

/// The name the manifest contributes.
pub const COMMAND_NAME: &str = "progress-guard";
/// The reserved prefix of the leading words that carry the caller's environment variables.
pub const ENV_ARGUMENT_PREFIX: &str = "--ilium-env=";
/// The variable that names the pane the caller runs in.
pub const PANE_ENVIRONMENT_VARIABLE: &str = "ILIUM_PANE_ID";
/// Bounded hook input; Claude Code sends one small JSON object.
pub const MAX_INPUT_BYTES: u32 = 1024 * 1024;

thread_local! {
    /// The invocations that wait for their standard input, by the request that reads it.
    static PENDING: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
}

/// Whether the caller runs inside an Ilium pane: a non-empty `ILIUM_PANE_ID` among the words.
pub fn inside_ilium_pane(argv: &[String]) -> bool {
    argv.iter()
        .filter_map(|word| word.strip_prefix(ENV_ARGUMENT_PREFIX))
        .filter_map(|pair| pair.split_once('='))
        .any(|(name, value)| name == PANE_ENVIRONMENT_VARIABLE && !value.is_empty())
}

/// Starts the guard for `invocation`. Outside a pane there is nothing to read or refuse.
pub fn on_invoked(api: &mut dyn HostApi, invocation: u64, argv: &[String]) {
    if !inside_ilium_pane(argv) {
        HostCli::finish(api, invocation, 0);
        return;
    }
    match HostCli::read_stdin(api, invocation, MAX_INPUT_BYTES) {
        Ok(request) => PENDING.with(|pending| {
            pending.borrow_mut().insert(request, invocation);
        }),
        // No input channel: the hook payload cannot be judged, so the call passes.
        Err(_) => HostCli::finish(api, invocation, 0),
    }
}

/// Continues the guard that waited for `request`; false when `request` is not one of its reads.
pub fn on_stdin_ready(api: &mut dyn HostApi, request: u64) -> bool {
    let Some(invocation) = PENDING.with(|pending| pending.borrow_mut().remove(&request)) else {
        return false;
    };
    let bytes = HostCli::take_stdin(api, request).unwrap_or_default();
    if let Some(output) = hook_output(&bytes, true) {
        // A closed output (the caller went away) is not an error the guard can act on.
        let _ = HostCli::write_line(api, invocation, output);
    }
    HostCli::finish(api, invocation, 0);
    true
}

/// A cancelled invocation stops waiting for its input.
pub fn on_cancelled(invocation: u64) {
    PENDING.with(|pending| {
        pending
            .borrow_mut()
            .retain(|_, waiting| *waiting != invocation);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn a_pane_id_in_the_environment_words_means_inside_a_pane() {
        assert!(inside_ilium_pane(&words(&["--ilium-env=ILIUM_PANE_ID=7"])));
    }

    #[test]
    fn a_missing_or_empty_pane_id_means_outside_a_pane() {
        assert!(!inside_ilium_pane(&[]));
        assert!(!inside_ilium_pane(&words(&["--ilium-env=ILIUM_PANE_ID="])));
        assert!(!inside_ilium_pane(&words(&["--ilium-env=OTHER=7"])));
        assert!(!inside_ilium_pane(&words(&["ILIUM_PANE_ID=7"])));
    }

    #[test]
    fn the_command_name_is_the_one_the_kernel_forwards_to() {
        assert_eq!(COMMAND_NAME, "progress-guard");
    }
}
