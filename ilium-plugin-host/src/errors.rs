//! The user-visible error codes with their one-sentence explanations (USR-347).

/// The cause of an error and the next step for the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    /// One sentence naming the cause.
    pub cause: String,
    /// One sentence naming the next step.
    pub next_step: String,
}

/// Catalogue rows: code, one-sentence cause, one-sentence next step.
const CATALOGUE: &[(&str, &str, &str)] = &[
    ("not-permitted", "The plugin tried something its grants or your policy do not allow.", "Review the plugin grants in the plugin details and grant the missing one if you trust the plugin."),
    ("not-found", "The thing the plugin asked for does not exist or is no longer there.", "Check that the pane, file or setting still exists and run the action again."),
    ("invalid-argument", "The plugin sent a value the host does not accept.", "Update the plugin or report the problem to its publisher."),
    ("conflict", "The request clashes with the current state of the thing it changes.", "Refresh the view and repeat the action."),
    ("quota-exceeded", "The plugin used up a limit that the host sets for it.", "Wait for the limit to reset or remove data the plugin no longer needs."),
    ("rate-limited", "The plugin made calls faster than the host allows.", "Wait a moment and run the action again."),
    ("busy", "The host is serving another request for the same resource.", "Run the action again in a few seconds."),
    ("timed-out", "The call did not finish within its time budget.", "Run the action again and report the plugin if it keeps timing out."),
    ("cancelled", "The call was stopped before it finished.", "Run the action again if you still need it."),
    ("unsupported", "This version of Ilium or this platform does not offer what the plugin asked for.", "Update Ilium or choose a plugin version that supports your setup."),
    ("pane-not-ready", "The pane has not finished starting.", "Wait until the pane shows its prompt and run the action again."),
    ("revoked", "A grant the plugin used was withdrawn while the call ran.", "Grant the permission again in the plugin details if you want the plugin to continue."),
    ("io", "Reading or writing a file failed.", "Check the disk space and the file permissions and run the action again."),
    ("network", "A network request of the plugin failed.", "Check your connection and the network policy for the plugin and run the action again."),
    ("internal", "The host hit an unexpected fault while serving the call.", "Run the action again and send a support bundle if the fault repeats."),
];

/// Every code a user can see.
pub fn user_visible_error_codes() -> Vec<&'static str> {
    CATALOGUE.iter().map(|(code, _, _)| *code).collect()
}

/// The explanation of a code, if it has one.
pub fn error_explanation(code: &str) -> Option<Explanation> {
    CATALOGUE
        .iter()
        .find(|(entry, _, _)| *entry == code)
        .map(|(_, cause, next_step)| Explanation {
            cause: (*cause).to_owned(),
            next_step: (*next_step).to_owned(),
        })
}
