//! Terminal-screen transfer eligibility and presentation vocabulary.
//!
//! `split_layout` owns raw separator geometry. This module turns that
//! geometry into a user action only when both endpoints are live terminals;
//! `App` owns the actual screen read and atomic PTY paste.

use ilium_core::NodeId;
use ratatui::layout::Rect;

use crate::split_layout::PaneDirection;

/// A directional, pane-scoped screen transfer exposed in a split view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenTransfer {
    pub source_pane_id: NodeId,
    pub destination_pane_id: NodeId,
    pub direction: PaneDirection,
    pub control_area: Rect,
}
