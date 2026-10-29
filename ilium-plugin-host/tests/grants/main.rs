//! Lane `grants` (host half): runtime grant behaviour (always granted, declined, per side,
//! revoked), the review dialog data, consent prompts, stores, the install pipeline order and the
//! approval rules (CAP-011..CAP-018, SEC-100..SEC-124). Run with `--features testing`.
//!
//! One module per manual page; `support` holds the shared helpers. Later parts of the lane add
//! modules here.

mod reference_capabilities;
mod reference_grant_review_and_consent;
mod support;
