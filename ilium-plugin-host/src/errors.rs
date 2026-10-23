//! The user-visible error codes with their one-sentence explanations (USR-347).

/// The cause of an error and the next step for the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    /// One sentence naming the cause.
    pub cause: String,
    /// One sentence naming the next step.
    pub next_step: String,
}

/// Every code a user can see.
pub fn user_visible_error_codes() -> Vec<&'static str> {
    unimplemented!("host::errors::user_visible_error_codes")
}

/// The explanation of a code, if it has one.
pub fn error_explanation(_code: &str) -> Option<Explanation> {
    unimplemented!("host::errors::error_explanation")
}
