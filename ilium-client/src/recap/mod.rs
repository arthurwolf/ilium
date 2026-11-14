//! Recap evidence helpers the project restructure still uses. Pane recaps themselves, the
//! `ilium recap` command and the in-pane recap line are the `ilium.core.recap` plugin; this
//! module keeps only the bounded, redacted evidence digest, the batch evidence block and the
//! title-versus-recap comparison that `restructure` feeds to its model call.

pub mod batch;
pub mod comparison;
pub mod custody;
pub mod evidence;
pub mod model;
