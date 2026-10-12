//! Pure, I/O-free model of what an outside program may learn about a session.
//!
//! The `ilium ctl` command family and the loopback HTTP API both answer from
//! this crate, so a tree fetched over a socket by the CLI and a tree fetched
//! over HTTP are the same document. Nothing here touches a socket, a file or a
//! clock: every function takes a [`Tree`](ilium_core::Tree) snapshot and
//! returns data.
//!
//! * [`view`] turns tree nodes into JSON values (whole tree, one project, one
//!   pane).
//! * [`select`] resolves human selectors (`12`, `myproject`, `/abs/path`) to
//!   node ids with explicit ambiguity errors.
//! * [`render`] writes a JSON value as JSON, YAML or Markdown.
//! * [`guide`] holds the agent-facing help topics.

pub mod documents;
pub mod guide;
pub mod operation;
pub mod render;
pub mod select;
pub mod view;

pub use operation::{examples as operation_examples, Operation, OperationError};
pub use render::{render_document, Format, Shape};
pub use select::{resolve_node, resolve_pane, resolve_project, SelectError};
pub use view::{node_view, pane_detail, tree_view, ViewOptions};
