//! The `ilium` CLI's own logic, exposed as a library so it can be tested.
//!
//! The binary (`main.rs`) is argument parsing and process orchestration over
//! these two modules. They live behind a lib target rather than inside the bin
//! because `tests/pty_tui_smoke.rs` needs [`session::resolve_project_session`]:
//! it drives the real CLI under a pty and then has to reach the *same* session
//! endpoint the CLI just resolved.
//!
//! Deriving that path a second time inside the test is not an option -- it
//! encodes a path slug, a digest and a socket-length budget, and a test that
//! reimplemented any of it would pass while disagreeing with the product. The
//! previous approach, scanning the runtime directory for a Unix socket file,
//! avoided the duplication but only worked on Unix: Windows sessions are named
//! pipes with no filesystem presence at all, which is what kept that test
//! Unix-only.

pub mod error;
pub mod session;
