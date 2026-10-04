//! Local, read-only render cache for one PTY-backed pane's screen.
//!
//! ilium-client never owns a real PTY (ilium-server does -- see the
//! crate's module docs). What it owns instead is a `vt100::Parser` fed
//! purely from `ServerEvent::ScreenUpdate` byte chunks, so `tui_term`
//! still has a real `vt100::Screen` to render from without this crate
//! needing a second, IPC-specific screen representation (see
//! `ilium_ipc::ServerEvent::ScreenUpdate`'s own doc comment for why the
//! wire carries raw bytes rather than a pre-diffed cell format).
//!
//! Scrollback lives entirely here, client-side -- not on `ilium-server`'s
//! copy (see `ilium-pty::PtySession::spawn`, which deliberately keeps its
//! own parser's scrollback at zero: the server only needs the *current*
//! screen for agent detection and mouse-protocol negotiation). The live
//! `vt100::Parser` therefore always follows PTY output at offset zero. When
//! the user scrolls away from the tail, this module clones its `Screen` into
//! a read-only historical viewport and navigates that snapshot instead.
//! Keeping the two roles explicit is essential for inline agent TUIs: Codex
//! clears and replays its transcript after a resize, and mutating one
//! scrolled parser with that live redraw splices old rows into the replay
//! while continuously increasing the distance back to the tail.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use tui_term::widget::PseudoTerminal;

use crate::terminal_activity::{VisibleRowEvidence, VisibleTextEvidence};

/// Starting geometry for a freshly created pane, before the first real
/// `ResizePane` request (sent once the client knows the pane's actual
/// on-screen content box) corrects it.
pub const DEFAULT_ROWS: u16 = 24;
pub const DEFAULT_COLS: u16 = 80;

/// Renders an immutable screen without consulting or mutating a live
/// `TerminalView`. Smart Copy uses this while live PTY output continues to be
/// applied behind the frozen viewport.
pub fn render_frozen_screen(screen: &vt100::Screen, area: Rect, destination: &mut Buffer) {
    if !area.is_empty() {
        PseudoTerminal::new(screen).render(area, destination);
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Hashes only the visible character contents of each terminal cell with a
/// compact FNV-1a stream. Terminal text is untrusted but this is change
/// detection, not a security boundary; avoiding SipHash's keyed rounds is the
/// important property on the per-output hot path.
///
/// Cursor movement, color/style changes, terminal modes, and scrollback do
/// not affect this value. Iterating cells avoids allocating `Screen::contents`
/// for every already-coalesced live output batch.
fn visible_text_fingerprint(screen: &vt100::Screen) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    const CELL_BOUNDARY: u8 = 0xff;

    let mut fingerprint = FNV_OFFSET_BASIS;
    let (rows, columns) = screen.size();

    for dimension_byte in rows.to_le_bytes().into_iter().chain(columns.to_le_bytes()) {
        fingerprint ^= u64::from(dimension_byte);
        fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
    }

    for row in 0..rows {
        for column in 0..columns {
            let contents = screen
                .cell(row, column)
                .map(vt100::Cell::contents)
                .unwrap_or_default();
            for byte in contents.bytes() {
                fingerprint ^= u64::from(byte);
                fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
            }
            // UTF-8 never contains 0xff, so cell boundaries cannot be
            // confused with a byte from visible text.
            fingerprint ^= u64::from(CELL_BOUNDARY);
            fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
        }
    }

    fingerprint
}

fn visible_row_fingerprints(screen: &vt100::Screen) -> Vec<u64> {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    const CELL_BOUNDARY: u8 = 0xff;

    let (rows, columns) = screen.size();
    (0..rows)
        .map(|row| {
            let mut fingerprint = FNV_OFFSET_BASIS;
            for column in 0..columns {
                let contents = screen
                    .cell(row, column)
                    .map(vt100::Cell::contents)
                    .unwrap_or_default();
                for byte in contents.bytes() {
                    fingerprint ^= u64::from(byte);
                    fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
                }
                fingerprint ^= u64::from(CELL_BOUNDARY);
                fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
            }
            fingerprint
        })
        .collect()
}

fn visible_row_evidence(screen: &vt100::Screen, row: u16) -> VisibleRowEvidence {
    const MAX_SAMPLE_CHARS: usize = 48;

    let (_, columns) = screen.size();
    let mut text = String::new();
    let mut sampled_chars = 0;
    let mut blank = true;
    let mut truncated = false;
    for column in 0..columns {
        let contents = screen
            .cell(row, column)
            .map(vt100::Cell::contents)
            .unwrap_or_default();
        for character in contents.chars() {
            blank &= character.is_whitespace();
            if sampled_chars < MAX_SAMPLE_CHARS {
                text.push(character);
                sampled_chars += 1;
            } else {
                truncated = true;
            }
        }
    }

    VisibleRowEvidence {
        row_number: row.saturating_add(1),
        text,
        blank,
        truncated,
    }
}

/// Returns the offset before, and width of, a BEL or ST OSC terminator.
fn osc_terminator(bytes: &[u8]) -> Option<(usize, usize)> {
    bytes.iter().enumerate().find_map(|(index, byte)| {
        if *byte == 0x07 {
            Some((index, 1))
        } else if *byte == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
            Some((index, 2))
        } else {
            None
        }
    })
}

/// Fixed row cap for the *rendered* `vt100::Parser` scrollback, applied per
/// terminal pane, client-side only -- see the module docs for why the
/// server's own parser keeps none at all.
///
/// This is intentionally a constant, not derived from the user-facing MiB
/// budget below. A vt100 scrollback row is a full-width `Vec<vt100::Cell>`
/// (`vendor/vt100/src/cell.rs` statically asserts `size_of::<Cell>() == 32`,
/// and `vendor/vt100/src/row.rs` allocates every row at the pane's current
/// column count), so its true cost is `cols * 32` bytes -- not a fixed
/// per-line estimate, and not something that can be kept honest across a
/// pane resize (`vt100::Grid` has no setter for its row cap once
/// constructed, so re-deriving it from `cols` would require reparsing the
/// pane's retained history on every column-count change). Pinning this cap
/// keeps rendered-scrollback memory small and predictable (worst case
/// `10_000 * cols * 32` bytes) regardless of pane width or the configured
/// budget.
const RENDER_SCROLLBACK_ROWS: usize = 10_000;

/// An immutable-in-time terminal screen the user is inspecting while the
/// live parser continues to absorb output and resize redraws behind it.
struct HistoricalViewport {
    screen: vt100::Screen,
    scrollback_total: usize,
}

/// Ratatui cells derived from one visible vt100 screen generation and area.
/// Sidebar animation can copy these cells without reinterpreting every vt100
/// cell's symbol, color, and modifier on each frame.
struct TerminalRenderCache {
    revision: u64,
    area: Rect,
    buffer: Buffer,
}

#[derive(Debug)]
struct HistorySegment {
    bytes: Arc<Vec<u8>>,
    start: usize,
}

/// Immutable segmented history handed to a search worker. A live append
/// detects the shared newest segment and starts a fresh one, so it never
/// clones the complete retained journal while this snapshot is alive.
#[derive(Clone, Debug)]
pub struct TerminalHistorySnapshot {
    segments: Arc<Vec<(Arc<Vec<u8>>, usize)>>,
    retained_len: usize,
    pub(crate) origin: Arc<()>,
    _allocation_charge: Option<Arc<crate::terminal_parsing::SnapshotCharge>>,
    _pin_charge: Option<Arc<crate::terminal_parsing::SnapshotPin>>,
}

impl TerminalHistorySnapshot {
    pub(crate) fn retain_charge(&mut self, charge: Arc<crate::terminal_parsing::SnapshotCharge>) {
        self._allocation_charge = Some(charge);
    }
    pub fn len(&self) -> usize {
        self.retained_len
    }

    pub fn is_empty(&self) -> bool {
        self.retained_len == 0
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.retained_len);
        for (segment, start) in self.segments.iter() {
            bytes.extend_from_slice(&segment[*start..]);
        }
        bytes
    }
}

/// Computes the raw-history retention cap in bytes for a given MiB budget.
/// Unlike the render parser's row cap above, this figure is exact -- the
/// journal it governs (`TerminalView::history_segments`) stores raw output
/// bytes directly, with no per-cell/per-row estimation involved, so "N MiB"
/// here means exactly N MiB retained. Floored at 1 MiB so a pathological
/// caller-supplied `budget_mib` of `0` doesn't collapse search history to
/// nothing; `TerminalSettings::MIN_SCROLLBACK_BUDGET_MIB` (4) already keeps
/// the settings UI well above that floor.
fn history_budget_bytes(budget_mib: u16) -> usize {
    usize::from(budget_mib)
        .saturating_mul(1024 * 1024)
        .max(1024 * 1024)
}

/// Admission shadow of private vte std OSC allocation, including retained
/// capacity after termination. State changes only alongside exact parser bytes.
#[derive(Default, Clone, Copy)]
struct OscAllocation {
    escaped: bool,
    active: bool,
    length: usize,
    highwater: usize,
}
impl OscAllocation {
    fn advance(mut self, bytes: &[u8]) -> Self {
        for &byte in bytes {
            if self.active {
                match byte {
                    7 | 0x18 | 0x1a => {
                        self.active = false;
                        self.length = 0;
                    }
                    0x1b => {
                        self.active = false;
                        self.length = 0;
                        self.escaped = true;
                    }
                    0..=6 | 8..=0x17 | 0x19 | 0x1c..=0x1f | b';' => {}
                    _ => {
                        self.length = self.length.saturating_add(1);
                        self.highwater = self.highwater.max(self.length);
                    }
                }
            } else if self.escaped {
                match byte {
                    b']' => {
                        self.active = true;
                        self.length = 0;
                        self.escaped = false;
                    }
                    0x1b => {}
                    0x18 | 0x1a => self.escaped = false,
                    0..=0x1f | 0x7f..=0xff => {}
                    _ => self.escaped = false,
                }
            } else if byte == 0x1b {
                self.escaped = true;
            }
        }
        self
    }
    fn retained_bytes(self) -> usize {
        self.highwater.saturating_mul(2).max(8)
    }
}
#[derive(Default)]
pub(crate) struct OrderedOutputCursor {
    pub consumed: usize,
    pub replay_started: bool,
}

pub(crate) struct TerminalState {
    /// Authoritative live terminal state. Its scrollback offset stays at zero;
    /// historical navigation happens only through `historical_viewport`.
    parser: vt100::Parser,
    osc_allocation: OscAllocation,
    allocated_max_columns: u16,
    allocated_max_rows: u16,
    // `vt100::Screen` exposes the current scroll *offset*
    // (`Screen::scrollback`) but no direct "how many rows have
    // accumulated" accessor -- only `Screen::set_scrollback`'s internal
    // clamp knows that. `refresh_scrollback_total` reads it via that
    // clamp (set to `usize::MAX`, read back, restore) and caches the
    // result here so rendering (`scrollback_total`) stays a cheap `&self`
    // read instead of needing a `&mut Screen` on every frame.
    scrollback_total: usize,
    /// Present only while the user is reading retained history or a workspace
    /// search result. A `Screen` clone is sufficient because this view never
    /// parses output; it only changes its independent scrollback offset.
    historical_viewport: Option<HistoricalViewport>,
    /// Newest server output chunk represented by `parser`. This turns an
    /// attach-time replay plus the connection's already-queued live updates
    /// into one exactly-once stream.
    last_output_sequence: u64,
    /// Raw retained output used by the workspace search. This deliberately
    /// mirrors server replay bytes rather than attempting to reverse a
    /// rendered `vt100::Screen`, which only keeps a bounded visual window.
    /// Capped at `history_budget_bytes`, which is exactly the byte figure
    /// the settings UI's "Scrollback budget (MiB)" knob promises -- unlike
    /// the render parser's fixed `RENDER_SCROLLBACK_ROWS` cap, this journal
    /// is raw bytes with no per-row estimation, so the cap is exact.
    history_segments: VecDeque<HistorySegment>,
    history_retained_len: usize,
    history_origin: Arc<()>,
    /// Current raw-history retention cap in bytes, derived from the
    /// configured MiB budget via `history_budget_bytes`. Stored so
    /// `set_scrollback_budget_mib` can re-trim retained segments in place
    /// without touching the (budget-independent) render parser.
    history_budget_bytes: usize,
    /// Recent OSC-8 label/target pairs observed in the byte stream. vt100
    /// renders the label but deliberately discards the metadata, so the
    /// client retains this narrow side-channel for click activation.
    osc8_links: VecDeque<(String, String)>,
    /// Unconsumed raw output while an OSC-8 sequence spans IPC chunks.
    osc8_stream: Vec<u8>,
    /// Fingerprint of `parser`'s visible character cells after the most recent
    /// replay, resize, feed, or accepted live output. This baseline turns a
    /// live PTY event into an O(screen cells), allocation-free text-change
    /// decision without any timer-driven screen scan. Changed updates also
    /// retain bounded row samples for the plain-terminal WHY tooltip.
    visible_text_fingerprint: u64,
    /// Per-row fingerprints from the same baseline as `visible_text_fingerprint`.
    visible_row_fingerprints: Vec<u64>,
    /// Dimensions that gave meaning to the previous row fingerprints.
    visible_text_dimensions: (u16, u16),
    /// Changes only when the screen visible to the user changes.
    render_revision: u64,
    /// Presentation cache uses interior mutability because drawing is a
    /// logically read-only operation on terminal state.
    #[cfg(test)]
    render_cache: RefCell<Option<TerminalRenderCache>>,
}

impl TerminalState {
    /// Starts a fresh, blank screen at `rows`x`cols` -- the caller should
    /// send `ClientRequest::ResizePane` promptly after creating a pane so
    /// the server-side PTY matches, and this view's own `resize` keeps the
    /// local parser matching whatever the client's own layout computed.
    #[cfg(test)]
    pub fn new(rows: u16, cols: u16) -> Self {
        Self::with_scrollback_budget_mib(rows, cols, 32)
    }

    pub fn with_scrollback_budget_mib(rows: u16, cols: u16, budget_mib: u16) -> Self {
        let parser = vt100::Parser::new(rows, cols, RENDER_SCROLLBACK_ROWS);
        let visible_text_fingerprint = visible_text_fingerprint(parser.screen());
        let visible_row_fingerprints = visible_row_fingerprints(parser.screen());
        let visible_text_dimensions = parser.screen().size();

        Self {
            parser,
            osc_allocation: OscAllocation::default(),
            allocated_max_columns: cols,
            allocated_max_rows: rows,
            scrollback_total: 0,
            historical_viewport: None,
            last_output_sequence: 0,
            history_segments: VecDeque::from([HistorySegment {
                bytes: Arc::new(Vec::new()),
                start: 0,
            }]),
            history_retained_len: 0,
            history_origin: Arc::new(()),
            history_budget_bytes: history_budget_bytes(budget_mib),
            osc8_links: VecDeque::new(),
            osc8_stream: Vec::new(),
            visible_text_fingerprint,
            visible_row_fingerprints,
            visible_text_dimensions,
            render_revision: 0,
            #[cfg(test)]
            render_cache: RefCell::new(None),
        }
    }

    /// Re-caps the raw-history journal under a new MiB budget and
    /// immediately trims retained history down to it if the budget shrank.
    /// The render parser is untouched -- its `RENDER_SCROLLBACK_ROWS` cap is
    /// fixed and independent of this budget (see that constant's doc
    /// comment), so there is nothing to rebuild here.
    pub fn set_scrollback_budget_mib(&mut self, budget_mib: u16) {
        self.history_budget_bytes = history_budget_bytes(budget_mib);
        self.trim_history_to_budget();
    }

    /// Feeds one raw PTY output chunk into the live parser. An active
    /// historical viewport is intentionally untouched, so streaming output
    /// cannot move the rows the user is reading or lengthen their route back
    /// to the live tail.
    #[cfg(test)]
    pub fn feed(&mut self, bytes: &[u8]) {
        self.observe_osc8_links(bytes);
        self.append_history(bytes);
        self.osc_allocation = self.osc_allocation.advance(bytes);
        self.parser.process(bytes);
        self.invalidate_live_render_if_visible();
        self.refresh_scrollback_total();
        self.refresh_visible_text_fingerprint();
    }

    /// Replaces the live parser with the server-owned retained output.
    /// Attach starts from a blank view; lag repair leaves a detached
    /// historical snapshot untouched and ignores stale replays so recovery
    /// cannot move the visible viewport or roll the live screen backward.
    #[cfg(test)]
    pub fn apply_replay(&mut self, bytes: &[u8], through_sequence: u64, _is_complete: bool) {
        if through_sequence <= self.last_output_sequence {
            return;
        }

        let (rows, cols) = self.parser.screen().size();
        self.history_segments.clear();
        self.history_segments.push_back(HistorySegment {
            bytes: Arc::new(Vec::new()),
            start: 0,
        });
        self.history_retained_len = 0;
        self.history_origin = Arc::new(());
        self.osc8_links.clear();
        self.osc8_stream.clear();
        self.observe_osc8_links(bytes);
        self.append_history(bytes);
        self.last_output_sequence = through_sequence;

        self.parser = vt100::Parser::new(rows, cols, RENDER_SCROLLBACK_ROWS);
        self.osc_allocation = OscAllocation::default();
        self.scrollback_total = 0;
        self.osc_allocation = self.osc_allocation.advance(bytes);
        self.parser.process(bytes);
        self.invalidate_live_render_if_visible();
        self.refresh_scrollback_total();
        self.refresh_visible_text_fingerprint();
    }

    /// Applies a live output chunk only when it was not already included in
    /// the attach replay. Output sequence numbers are pane-local and
    /// monotonic for one server lifetime. Returns `true` only when the
    /// accepted bytes changed at least one visible character cell and the
    /// caller requested ordinary-terminal activity tracking. The fingerprint
    /// check is allocation-free; changed updates capture bounded row evidence.
    /// Known agent panes skip the O(visible cells) fingerprint entirely.
    #[cfg(test)]
    pub fn apply_live_output(
        &mut self,
        first_sequence: u64,
        sequence: u64,
        bytes: &[u8],
        should_track_visible_text_change: bool,
    ) -> bool {
        self.apply_live_output_with_evidence(
            first_sequence,
            sequence,
            bytes,
            should_track_visible_text_change,
        )
        .is_some()
    }

    /// Applies one accepted output batch and returns bounded parsed-screen
    /// evidence only when tracked visible text actually changed.
    #[cfg(test)]
    pub(crate) fn apply_live_output_with_evidence(
        &mut self,
        first_sequence: u64,
        sequence: u64,
        bytes: &[u8],
        should_track_visible_text_change: bool,
    ) -> Option<VisibleTextEvidence> {
        if sequence <= self.last_output_sequence {
            return None;
        }
        let expected_first_sequence = self.last_output_sequence.saturating_add(1);
        if first_sequence != expected_first_sequence {
            tracing::error!(
                first_sequence,
                sequence,
                last_output_sequence = self.last_output_sequence,
                "ignoring non-contiguous terminal output"
            );
            return None;
        }
        self.append_history(bytes);
        self.observe_osc8_links(bytes);
        self.osc_allocation = self.osc_allocation.advance(bytes);
        self.parser.process(bytes);
        self.invalidate_live_render_if_visible();
        self.refresh_scrollback_total();
        self.last_output_sequence = sequence;

        if should_track_visible_text_change {
            self.capture_visible_text_change(first_sequence, sequence)
        } else {
            None
        }
    }

    /// Re-establishes the visible-text baseline when a detected agent becomes
    /// an ordinary terminal again after its live output skipped fingerprinting.
    pub fn synchronize_visible_text_fingerprint(&mut self) {
        self.refresh_visible_text_fingerprint();
    }

    /// Updates the saved visible-text baseline and reports whether it changed.
    fn refresh_visible_text_fingerprint(&mut self) -> bool {
        let fingerprint = visible_text_fingerprint(self.parser.screen());
        let did_change = fingerprint != self.visible_text_fingerprint;
        self.visible_text_fingerprint = fingerprint;
        if did_change {
            self.visible_row_fingerprints = visible_row_fingerprints(self.parser.screen());
            self.visible_text_dimensions = self.parser.screen().size();
        }
        did_change
    }

    fn capture_visible_text_change(
        &mut self,
        first_sequence: u64,
        sequence: u64,
    ) -> Option<VisibleTextEvidence> {
        let previous_rows = self.visible_row_fingerprints.clone();
        let previous_dimensions = self.visible_text_dimensions;
        if !self.refresh_visible_text_fingerprint() {
            return None;
        }

        let dimensions = self.parser.screen().size();
        let changed_positions = if dimensions == previous_dimensions {
            Some(
                previous_rows
                    .iter()
                    .zip(&self.visible_row_fingerprints)
                    .enumerate()
                    .filter(|(_, (previous, current))| previous != current)
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };
        let changed_rows = changed_positions.as_ref().map(Vec::len);
        let rows = changed_positions
            .into_iter()
            .flatten()
            .take(2)
            .map(|index| visible_row_evidence(self.parser.screen(), index as u16))
            .collect();

        Some(VisibleTextEvidence {
            first_sequence,
            sequence,
            changed_rows,
            rows,
        })
    }

    #[cfg(test)]
    pub fn osc8_link_at(&self, line: &str, column: usize) -> Option<String> {
        // `column` is a terminal *cell* index (as reported by the mouse
        // event), while `label.len()`/`line.find` operate in bytes. Agent
        // CLI panes routinely prefix a link's label with a multi-byte status
        // glyph (`⏺`, `⎿`, `→`), so comparing a raw cell index against a byte
        // range here would land inside that glyph's encoding rather than the
        // clicked label -- the same class of bug `terminal_links::link_at`
        // already guards against via this identical conversion.
        let byte_column = crate::terminal_links::cell_column_to_byte_offset(line, column);
        self.osc8_links.iter().rev().find_map(|(label, target)| {
            let start = line.find(label)?;
            (start..start + label.len())
                .contains(&byte_column)
                .then(|| target.clone())
        })
    }

    fn observe_osc8_links(&mut self, bytes: &[u8]) {
        // Ordinary shell and agent output very rarely contains an escape
        // byte at all.  Avoid allocating/copying that common-path output
        // into the OSC parser's scratch buffer; an incomplete sequence from
        // an earlier chunk keeps `osc8_stream` non-empty and therefore still
        // takes the full cross-chunk parser path below.
        if self.osc8_stream.is_empty() && !bytes.contains(&0x1b) {
            return;
        }
        self.osc8_stream.extend_from_slice(bytes);
        // A well-formed OSC-8 open/label/close sequence never needs anywhere
        // near this much buffering. Without a cap, a pane emitting a
        // never-terminated (malformed, or adversarial-content) `\x1b]8;`
        // opener would make every subsequent `feed` grow `osc8_stream`
        // forever, since every early return below (no terminator found yet)
        // leaves the buffer untouched. Capping here keeps that pending-match
        // buffer bounded regardless of how the loop below exits.
        const OSC8_STREAM_CAP_BYTES: usize = 64 * 1024;
        let excess = self.osc8_stream.len().saturating_sub(OSC8_STREAM_CAP_BYTES);
        if excess > 0 {
            self.osc8_stream.drain(..excess);
        }
        const OPEN: &[u8] = b"\x1b]8;";
        const CLOSE: &[u8] = b"\x1b]8;;";
        loop {
            let pending = &self.osc8_stream[..];
            let Some(open_start) = find_bytes(pending, OPEN) else {
                // No opener anywhere in the buffer. Keep only the longest
                // tail that is itself a genuine prefix of `OPEN` -- that's
                // the part that can still complete once the next chunk
                // arrives. `truncate` would keep the *head* of the buffer
                // instead, which discards exactly the partial-opener bytes
                // this cross-chunk parse depends on.
                let partial_opener_len = (1..OPEN.len())
                    .rev()
                    .find(|length| pending.ends_with(&OPEN[..*length]))
                    .unwrap_or(0);
                let drop_through = pending.len().saturating_sub(partial_opener_len);
                self.osc8_stream.drain(..drop_through);
                return;
            };
            if open_start > 0 {
                self.osc8_stream.drain(..open_start);
            }
            let pending = &self.osc8_stream[..];
            let Some((header_end, header_terminator_width)) =
                osc_terminator(&pending[OPEN.len()..])
            else {
                return;
            };
            let header_end = OPEN.len() + header_end;
            let target = String::from_utf8_lossy(&pending[OPEN.len()..header_end])
                .rsplit(';')
                .next()
                .unwrap_or_default()
                .to_string();
            let label_start = header_end + header_terminator_width;
            if target.is_empty() {
                // An OSC 8 tag with an empty URI *is* the close tag
                // (`\x1b]8;;ST`), never an opener. Reaching one here means
                // its matching opener was never seen -- discarded by this
                // buffer's own stream-cap drain above, or by the server's
                // retained-history cap on a replay -- so treat it as a
                // stray close and resume scanning after it. Without this,
                // the `find_bytes` search below would look for a "close"
                // starting from `label_start`, and since `CLOSE` is a
                // byte-for-byte prefix of `OPEN`, it would match the next
                // real link's own opener instead: silently swallowing that
                // whole link into a bogus, target-less entry.
                self.osc8_stream.drain(..label_start);
                continue;
            }
            let Some(close_start) =
                find_bytes(&pending[label_start..], CLOSE).map(|offset| label_start + offset)
            else {
                return;
            };
            let close_after = close_start + CLOSE.len();
            let Some((close_end, close_terminator_width)) = osc_terminator(&pending[close_after..])
            else {
                return;
            };
            let label = String::from_utf8_lossy(&pending[label_start..close_start])
                .replace(['\r', '\n'], "");
            self.osc8_stream
                .drain(..close_after + close_end + close_terminator_width);
            if !target.is_empty() && !label.is_empty() {
                self.osc8_links.push_back((label, target));
                if self.osc8_links.len() > 256 {
                    self.osc8_links.pop_front();
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn observe_osc8_for_benchmark(&mut self, bytes: &[u8]) {
        self.observe_osc8_links(bytes);
    }

    /// Resizes the authoritative live screen to match a `ResizePane` request
    /// just sent to the server, so the client's own rendering never waits on
    /// a round trip before reflowing. An active historical viewport retains
    /// the geometry and rows the user was inspecting; the foreground
    /// application may redraw the live parser at the new geometry without
    /// corrupting that frozen view.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.allocated_max_columns = self.allocated_max_columns.max(cols);
        self.allocated_max_rows = self.allocated_max_rows.max(rows);
        self.parser.screen_mut().set_size(rows, cols);
        self.invalidate_live_render_if_visible();
        self.refresh_scrollback_total();
        self.refresh_visible_text_fingerprint();
    }

    /// Runs `f` with the screen currently visible to the user, for rendering
    /// via `tui_term::widget::PseudoTerminal::new(screen)`.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        match self.historical_viewport.as_ref() {
            Some(viewport) => f(&viewport.screen),
            None => f(self.parser.screen()),
        }
    }

    /// Copies cached terminal cells into the frame, rebuilding the cache only
    /// after visible terminal state or geometry changes.
    #[cfg(test)]
    pub fn render_screen(&self, area: Rect, destination: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let mut render_cache = self.render_cache.borrow_mut();
        let needs_rebuild = render_cache
            .as_ref()
            .is_none_or(|cache| cache.revision != self.render_revision || cache.area != area);
        if needs_rebuild {
            let mut buffer = Buffer::empty(area);
            self.with_screen(|screen| PseudoTerminal::new(screen).render(area, &mut buffer));
            *render_cache = Some(TerminalRenderCache {
                revision: self.render_revision,
                area,
                buffer,
            });
        }
        let Some(cache) = render_cache.as_ref() else {
            return;
        };
        for row in area.y..area.bottom() {
            let source_start = cache.buffer.index_of(area.x, row);
            let destination_start = destination.index_of(area.x, row);
            let width = usize::from(area.width);
            destination.content[destination_start..destination_start + width]
                .clone_from_slice(&cache.buffer.content[source_start..source_start + width]);
        }
    }

    /// Invalidates the live-screen cache without disturbing a frozen
    /// historical viewport that remains visibly unchanged behind new output.
    fn invalidate_live_render_if_visible(&mut self) {
        if self.historical_viewport.is_none() {
            self.render_revision = self.render_revision.wrapping_add(1);
        }
    }

    /// Invalidates presentation after an explicit historical-view movement.
    fn invalidate_render(&mut self) {
        self.render_revision = self.render_revision.wrapping_add(1);
    }

    /// Freezes the current live screen on first use, then scrolls that
    /// immutable-in-time snapshot further into history. Live PTY output and
    /// resize redraws can continue behind it without moving the visible rows.
    pub fn scroll_up(&mut self, lines: u16) {
        if lines == 0 {
            return;
        }

        if self.historical_viewport.is_none() {
            if self.scrollback_total == 0 {
                return;
            }
            self.historical_viewport = Some(HistoricalViewport {
                screen: self.parser.screen().clone(),
                scrollback_total: self.scrollback_total,
            });
        }

        let Some(viewport) = self.historical_viewport.as_mut() else {
            return;
        };
        let current = viewport.screen.scrollback();
        viewport
            .screen
            .set_scrollback(current.saturating_add(usize::from(lines)));
        self.invalidate_render();
    }

    /// Scrolls the frozen historical viewport toward its captured tail.
    /// Reaching offset zero discards the snapshot and atomically reveals the
    /// current live parser rather than making the user traverse output that
    /// arrived while they were reading.
    pub fn scroll_down(&mut self, lines: u16) {
        let Some(viewport) = self.historical_viewport.as_mut() else {
            return;
        };
        let current = viewport.screen.scrollback();
        viewport
            .screen
            .set_scrollback(current.saturating_sub(usize::from(lines)));
        if viewport.screen.scrollback() == 0 {
            self.historical_viewport = None;
        }
        self.invalidate_render();
    }

    /// Jumps back to the live tail -- called whenever the pane sends fresh
    /// input, matching how an ordinary terminal emulator drops you back to
    /// the prompt the moment you start typing again.
    pub fn scroll_to_bottom(&mut self) {
        self.historical_viewport = None;
        self.parser.screen_mut().set_scrollback(0);
        self.invalidate_render();
    }

    /// `true` once the view has scrolled away from the live tail.
    pub fn is_scrolled_back(&self) -> bool {
        self.historical_viewport.is_some()
    }

    /// Current offset into scrollback: `0` at the live tail, increasing
    /// toward the oldest retained row.
    pub fn scrollback_position(&self) -> usize {
        self.historical_viewport
            .as_ref()
            .map_or(0, |viewport| viewport.screen.scrollback())
    }

    /// Total rows retained by the currently visible screen. A historical
    /// viewport reports its captured total so live output cannot change its
    /// scrollbar or the distance back to its captured tail.
    pub fn scrollback_total(&self) -> usize {
        self.historical_viewport
            .as_ref()
            .map_or(self.scrollback_total, |viewport| viewport.scrollback_total)
    }

    /// Returns all output retained for workspace search, including bytes the
    /// visible parser has already rotated out of its render scrollback.
    #[cfg(test)]
    pub fn searchable_history(&self) -> Vec<u8> {
        self.collect_retained_history()
    }

    /// Returns all retained terminal output as clipboard-safe plain text.
    /// Escape sequences control the terminal display rather than representing
    /// user-visible history, so copying them would leak cursor and color
    /// commands into the destination application.
    #[cfg(test)]
    pub fn copyable_history(&self) -> String {
        let retained_history = self.collect_retained_history();
        String::from_utf8_lossy(&strip_ansi_escapes::strip(&retained_history)).into_owned()
    }

    /// Produces an O(1) immutable view of retained output for the search
    /// worker. Later PTY output uses `Arc::make_mut`, so a worker can scan a
    /// stable history snapshot without blocking the interactive event loop.
    pub(crate) fn history_origin_matches(&self, origin: &Arc<()>) -> bool {
        Arc::ptr_eq(origin, &self.history_origin)
    }

    pub fn searchable_history_snapshot(&self) -> TerminalHistorySnapshot {
        TerminalHistorySnapshot {
            segments: Arc::new(
                self.history_segments
                    .iter()
                    .filter(|segment| segment.start < segment.bytes.len())
                    .map(|segment| (Arc::clone(&segment.bytes), segment.start))
                    .collect(),
            ),
            retained_len: self.history_retained_len,
            origin: Arc::clone(&self.history_origin),
            _allocation_charge: None,
            _pin_charge: None,
        }
    }

    /// Builds a historical terminal around one raw-output offset found by
    /// workspace search. The matching bytes become the newest visible output,
    /// so the result opens at its actual place rather than merely focusing the
    /// correct pane. The authoritative live parser keeps processing output;
    /// the ordinary live view returns on the next input or wheel journey to
    /// the bottom.
    pub(crate) fn history_rebuild_peak_bytes(&self, end_byte: usize) -> usize {
        let mut remaining = end_byte.min(self.history_retained_len);
        let mut osc = OscAllocation::default();
        for segment in &self.history_segments {
            let bytes = &segment.bytes[segment.start..];
            let take = remaining.min(bytes.len());
            osc = osc.advance(&bytes[..take]);
            remaining -= take;
            if remaining == 0 {
                break;
            }
        }
        let (rows, columns) = self.parser.screen().size();
        let rows = usize::from(rows)
            .saturating_mul(2)
            .saturating_add(RENDER_SCROLLBACK_ROWS);
        self.retained_allocation_bytes()
            .saturating_add(
                rows.saturating_mul(usize::from(columns))
                    .saturating_mul(128),
            )
            .saturating_add(rows.saturating_mul(256))
            .saturating_add(osc.retained_bytes())
    }
    pub fn jump_to_history_byte(&mut self, end_byte: usize) {
        let (rows, cols) = self.parser.screen().size();
        let mut remaining_bytes = end_byte.min(self.history_retained_len);
        let mut historical_parser = vt100::Parser::new(rows, cols, RENDER_SCROLLBACK_ROWS);
        for segment in &self.history_segments {
            if remaining_bytes == 0 {
                break;
            }
            let retained_segment = &segment.bytes[segment.start..];
            let consumed_bytes = remaining_bytes.min(retained_segment.len());
            historical_parser.process(&retained_segment[..consumed_bytes]);
            remaining_bytes -= consumed_bytes;
        }
        let scrollback_total = Self::measure_scrollback_total(historical_parser.screen_mut());
        self.historical_viewport = Some(HistoricalViewport {
            screen: historical_parser.screen().clone(),
            scrollback_total,
        });
        self.invalidate_render();
    }

    #[cfg(test)]
    pub fn last_output_sequence(&self) -> u64 {
        self.last_output_sequence
    }

    /// `true` once the pane's foreground app has negotiated an xterm mouse
    /// protocol -- when it has, wheel events belong to that app (it's
    /// asking to receive them), not to this view's own scrollback
    /// navigation. Agent CLIs differ here: Claude Code negotiates full
    /// tracking (DECSET 1000/1002/1003/1006), Codex negotiates none, and a
    /// plain shell prompt never does -- so this must be read per pane and
    /// never assumed from the pane being an agent.
    pub fn wants_mouse_protocol(&self) -> bool {
        self.parser.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None
    }

    /// Returns whether the foreground application asked its terminal for
    /// bracketed paste. The client uses this negotiated mode to decide whether
    /// one host paste needs inner `CSI 200~` / `CSI 201~` delimiters or should
    /// remain one unadorned bulk write for a plain shell/readline consumer.
    pub fn wants_bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    /// Re-derives `scrollback_total` (see the field doc comment) and
    /// re-clamps the current scroll position against it in the same pass,
    /// since `set_scrollback` is the only operation that performs that
    /// clamp.
    fn refresh_scrollback_total(&mut self) {
        self.scrollback_total = Self::measure_scrollback_total(self.parser.screen_mut());
    }

    /// Asks `vt100` to clamp an impossible offset, which is its only public
    /// mechanism for exposing the number of accumulated scrollback rows.
    /// Restoring the original offset keeps this helper safe for temporary
    /// search parsers as well as the live parser.
    fn measure_scrollback_total(screen: &mut vt100::Screen) -> usize {
        let original = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        let total = screen.scrollback();
        screen.set_scrollback(original);
        total
    }

    fn append_history(&mut self, bytes: &[u8]) {
        if self.history_segments.len() >= 4096 {
            let retained = self.collect_retained_history();
            self.history_segments.clear();
            self.history_segments.push_back(HistorySegment {
                bytes: Arc::new(retained),
                start: 0,
            });
        }
        let needs_fresh_segment = self
            .history_segments
            .back()
            .is_some_and(|segment| Arc::strong_count(&segment.bytes) > 1);
        if needs_fresh_segment {
            self.history_segments.push_back(HistorySegment {
                bytes: Arc::new(Vec::new()),
                start: 0,
            });
        }
        let active_segment = self
            .history_segments
            .back_mut()
            .expect("history always owns an active segment");
        Arc::make_mut(&mut active_segment.bytes).extend_from_slice(bytes);
        self.history_retained_len = self.history_retained_len.saturating_add(bytes.len());
        self.trim_history_to_budget();
    }

    fn collect_retained_history(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.history_retained_len);
        for segment in &self.history_segments {
            bytes.extend_from_slice(&segment.bytes[segment.start..]);
        }
        bytes
    }

    #[cfg(test)]
    pub(crate) fn append_history_for_benchmark(&mut self, bytes: &[u8]) {
        self.append_history(bytes);
    }

    /// Drops the oldest retained bytes past `history_budget_bytes`. Called
    /// after every append and whenever the budget itself shrinks.
    fn trim_history_to_budget(&mut self) {
        let mut excess = self
            .history_retained_len
            .saturating_sub(self.history_budget_bytes);
        if excess > 0 {
            self.history_origin = Arc::new(());
        }
        while excess > 0 {
            let Some(oldest_segment) = self.history_segments.front_mut() else {
                break;
            };
            let segment_len = oldest_segment
                .bytes
                .len()
                .saturating_sub(oldest_segment.start);
            let removed = excess.min(segment_len);
            oldest_segment.start = oldest_segment.start.saturating_add(removed);
            self.history_retained_len = self.history_retained_len.saturating_sub(removed);
            excess -= removed;
            if oldest_segment.start == oldest_segment.bytes.len() && self.history_segments.len() > 1
            {
                self.history_segments.pop_front();
            }
        }

        let compaction_threshold = (self.history_budget_bytes / 2).max(64 * 1024);
        if let Some(oldest_segment) = self.history_segments.front_mut() {
            if oldest_segment.start >= compaction_threshold
                && Arc::strong_count(&oldest_segment.bytes) == 1
            {
                Arc::make_mut(&mut oldest_segment.bytes).drain(..oldest_segment.start);
                oldest_segment.start = 0;
            }
        }
    }
}

/// Published state contains no mutable parser. Large clones are made only by
/// the persistent parsing owner; readers share its immutable allocation.
pub(crate) struct TerminalSnapshot {
    pub visible: Arc<vt100::Screen>,
    pub history: TerminalHistorySnapshot,
    pub links: VecDeque<(String, String)>,
    pub scrollback_total: usize,
    pub scrollback_position: usize,
    pub scrolled_back: bool,
    pub sequence: u64,
    pub revision: u64,
    pub mouse: bool,
    pub paste: bool,
    pub allocation_charge: Option<Arc<crate::terminal_parsing::SnapshotCharge>>,
}
#[derive(Clone)]
pub(crate) struct PreparationSnapshot {
    snapshot: Arc<TerminalSnapshot>,
    _pin: Option<Arc<crate::terminal_parsing::SnapshotPin>>,
}
/// Exact terminal source retained with one composed frame and installed only
/// after actual output acknowledgement. Clones share its existing allocation.
#[derive(Clone)]
pub(crate) struct PaintedTerminal {
    pub identity: Arc<()>,
    pub ordinal: u64,
    snapshot: Arc<TerminalSnapshot>,
    _live: Option<Arc<crate::terminal_parsing::SnapshotLiveLease>>,
    _pin: Option<Arc<crate::terminal_parsing::SnapshotPin>>,
}
impl PaintedTerminal {
    pub(crate) fn with_screen<R>(&self, read: impl FnOnce(&vt100::Screen) -> R) -> R {
        read(&self.snapshot.visible)
    }
    /// Long-lived interactions leave the live/replacement category and obtain
    /// a bounded history pin before retaining the original allocation.
    pub(crate) fn pinned(&self) -> Result<Self, String> {
        let prepared = self.preparation_snapshot()?;
        Ok(Self {
            identity: self.identity.clone(),
            ordinal: self.ordinal,
            snapshot: prepared.snapshot,
            _live: None,
            _pin: prepared._pin,
        })
    }
    pub(crate) fn scrollback_metrics(&self) -> (usize, usize, u16) {
        (
            self.snapshot.scrollback_total,
            self.snapshot.scrollback_position,
            self.snapshot.visible.size().0,
        )
    }
    #[cfg(test)]
    pub(crate) fn osc8_link_at(&self, line: &str, column: usize) -> Option<String> {
        let column = crate::terminal_links::cell_column_to_byte_offset(line, column);
        self.snapshot
            .links
            .iter()
            .rev()
            .find_map(|(label, target)| {
                let start = line.find(label)?;
                (start..start + label.len())
                    .contains(&column)
                    .then(|| target.clone())
            })
    }
    pub(crate) fn capture_cost(&self) -> Result<ilium_execution::JobCost, String> {
        let (rows, columns) = self.snapshot.visible.size();
        // vt100's native screen includes its retained scrollback; words,
        // detected regions, escaped JSON and serialization scratch coexist.
        let bytes = (usize::from(rows) * 2 + RENDER_SCROLLBACK_ROWS)
            .saturating_mul(usize::from(columns))
            .saturating_mul(128)
            .saturating_add(
                usize::from(rows)
                    .saturating_mul(usize::from(columns))
                    .saturating_mul(1024),
            )
            .saturating_add(128 * 1024);
        if bytes > 128 * 1024 * 1024 {
            return Err(format!("Smart Copy capture exceeds its128MiB single-capture limit ({bytes} bytes required)"));
        }
        Ok(ilium_execution::JobCost {
            input_bytes: bytes,
            result_bytes: bytes,
        })
    }
    pub(crate) fn capture_charge(
        &self,
        bytes: usize,
    ) -> Result<Option<Arc<crate::terminal_parsing::SnapshotCharge>>, String> {
        self.snapshot
            .allocation_charge
            .as_ref()
            .map(|charge| charge.reserve_capture(bytes))
            .transpose()
    }
    pub(crate) fn allocation_bytes(&self) -> usize {
        self.snapshot
            .allocation_charge
            .as_ref()
            .map_or(128 * 1024, |charge| charge.bytes())
    }
    pub(crate) fn preparation_cost(&self) -> Result<ilium_execution::JobCost, String> {
        context_cost(&self.snapshot)
    }
    pub(crate) fn preparation_snapshot(&self) -> Result<PreparationSnapshot, String> {
        let pin = self
            .snapshot
            .allocation_charge
            .as_ref()
            .map(|charge| charge.pin())
            .transpose()?;
        Ok(PreparationSnapshot {
            snapshot: self.snapshot.clone(),
            _pin: pin,
        })
    }
}
impl PreparationSnapshot {
    pub(crate) fn painted(self, identity: Arc<()>, ordinal: u64) -> PaintedTerminal {
        PaintedTerminal {
            identity,
            ordinal,
            snapshot: self.snapshot,
            _live: None,
            _pin: self._pin,
        }
    }
}
impl std::ops::Deref for PreparationSnapshot {
    type Target = TerminalSnapshot;
    fn deref(&self) -> &TerminalSnapshot {
        &self.snapshot
    }
}
impl TerminalState {
    pub(crate) fn publish(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            visible: Arc::new(self.with_screen(Clone::clone)),
            history: self.searchable_history_snapshot(),
            links: self.osc8_links.clone(),
            scrollback_total: self.scrollback_total(),
            scrollback_position: self.scrollback_position(),
            scrolled_back: self.is_scrolled_back(),
            sequence: self.last_output_sequence,
            revision: self.render_revision,
            mouse: self.wants_mouse_protocol(),
            paste: self.wants_bracketed_paste(),
            allocation_charge: None,
        }
    }
    pub(crate) fn output_sequence(&self) -> u64 {
        self.last_output_sequence
    }
    pub(crate) fn input_peak_bytes(&self, bytes: &[u8]) -> usize {
        let chunk = bytes.len();
        let columns = usize::from(self.parser.screen().size().1);
        let rows = RENDER_SCROLLBACK_ROWS
            .saturating_sub(self.scrollback_total)
            .min(chunk);
        let history_extra = self.history_segments.back().map_or(chunk, |segment| {
            if Arc::strong_count(&segment.bytes) > 1 {
                chunk.saturating_mul(2)
            } else if segment.bytes.len().saturating_add(chunk) > segment.bytes.capacity() {
                segment.bytes.capacity().max(chunk).saturating_mul(2)
            } else {
                0
            }
        });
        self.retained_allocation_bytes()
            .saturating_add(
                self.osc_allocation
                    .advance(bytes)
                    .retained_bytes()
                    .saturating_sub(self.osc_allocation.retained_bytes()),
            )
            .saturating_add(
                self.osc8_stream
                    .len()
                    .saturating_add(chunk)
                    .saturating_mul(2)
                    .saturating_sub(self.osc8_stream.capacity()),
            )
            .saturating_add(if self.osc8_stream.is_empty() && !bytes.contains(&0x1b) {
                0
            } else {
                // Existing OSC8 observer caps source scratch at64KiB. Account
                // lossy UTF-8 conversion, label/URI clones and deque growth.
                64 * 1024 * 8 + self.osc8_links.capacity() * std::mem::size_of::<(String, String)>()
            })
            .saturating_add(rows.saturating_mul(columns).saturating_mul(64))
            .saturating_add(history_extra)
            .saturating_add(if self.history_segments.len() >= 4096 {
                self.history_retained_len
            } else {
                0
            })
            .saturating_add(4096)
    }
    pub(crate) fn apply_ordered_output(
        &mut self,
        first: u64,
        sequence: u64,
        bytes: &[u8],
        track: bool,
        cursor: &mut OrderedOutputCursor,
        mut before: impl FnMut(&Self, &[u8]) -> Result<(), String>,
    ) -> Result<Option<VisibleTextEvidence>, String> {
        if sequence <= self.last_output_sequence {
            return Ok(None);
        }
        if first != self.last_output_sequence.saturating_add(1) {
            return Err("non-contiguous terminal output requires replay".into());
        }
        while cursor.consumed < bytes.len() {
            let end = cursor.consumed.saturating_add(64).min(bytes.len());
            let chunk = &bytes[cursor.consumed..end];
            before(self, chunk)?;
            self.append_history(chunk);
            self.observe_osc8_links(chunk);
            self.osc_allocation = self.osc_allocation.advance(chunk);
            self.parser.process(chunk);
            cursor.consumed = end;
            self.invalidate_live_render_if_visible();
            self.refresh_scrollback_total();
        }
        self.last_output_sequence = sequence;
        Ok(if track {
            self.capture_visible_text_change(first, sequence)
        } else {
            None
        })
    }
    pub(crate) fn apply_ordered_replay(
        &mut self,
        bytes: &[u8],
        sequence: u64,
        cursor: &mut OrderedOutputCursor,
        mut before: impl FnMut(&Self, &[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        if sequence <= self.last_output_sequence {
            return Ok(());
        }
        if !cursor.replay_started {
            let (rows, cols) = self.parser.screen().size();
            self.history_segments.clear();
            self.history_segments.push_back(HistorySegment {
                bytes: Arc::new(Vec::new()),
                start: 0,
            });
            self.history_retained_len = 0;
            self.history_origin = Arc::new(());
            self.osc8_links.clear();
            self.osc8_stream.clear();
            self.parser = vt100::Parser::new(rows, cols, RENDER_SCROLLBACK_ROWS);
            self.osc_allocation = OscAllocation::default();
            self.scrollback_total = 0;
            cursor.replay_started = true;
        }
        while cursor.consumed < bytes.len() {
            let end = cursor.consumed.saturating_add(64).min(bytes.len());
            let chunk = &bytes[cursor.consumed..end];
            before(self, chunk)?;
            self.append_history(chunk);
            self.observe_osc8_links(chunk);
            self.osc_allocation = self.osc_allocation.advance(chunk);
            self.parser.process(chunk);
            cursor.consumed = end;
            self.refresh_scrollback_total();
        }
        self.last_output_sequence = sequence;
        self.invalidate_live_render_if_visible();
        self.refresh_visible_text_fingerprint();
        Ok(())
    }
    pub(crate) fn retained_allocation_bytes(&self) -> usize {
        // Installed vt100 0.16.2 cells are fixed 32 bytes. Charge twice that
        // for retained Vec growth, plus row/deque metadata and parser scratch.
        let rows = usize::from(self.allocated_max_rows) * 2 + self.scrollback_total;
        let historic = self.historical_viewport.as_ref().map_or(0, |view| {
            usize::from(view.screen.size().0) * 2 + view.scrollback_total
        });
        let screen_rows = rows.saturating_add(historic);
        let history = self
            .history_segments
            .iter()
            .map(|segment| segment.bytes.capacity())
            .sum::<usize>();
        let links = self
            .osc8_links
            .iter()
            .map(|(label, target)| label.capacity().saturating_add(target.capacity()))
            .sum::<usize>();
        history
            .saturating_add(self.osc_allocation.retained_bytes())
            .saturating_add(
                screen_rows
                    .saturating_mul(usize::from(self.allocated_max_columns))
                    .saturating_mul(64),
            )
            .saturating_add(screen_rows.saturating_mul(128))
            .saturating_add(
                self.history_segments
                    .capacity()
                    .saturating_mul(std::mem::size_of::<HistorySegment>() + 32),
            )
            .saturating_add(
                self.osc8_links
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(String, String)>()),
            )
            .saturating_add(links)
            .saturating_add(self.osc8_stream.capacity())
            .saturating_add(self.visible_row_fingerprints.capacity().saturating_mul(8))
            .saturating_add(128 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalState as TerminalView;
    use super::*;

    #[test]
    fn private_osc_highwater_survives_termination_and_split_openers() {
        let mut allocation = OscAllocation::default();
        for byte in b"\x1b\0]0;"
            .iter()
            .copied()
            .chain(std::iter::repeat_n(b'x', 4096))
        {
            allocation = allocation.advance(&[byte]);
        }
        assert!(allocation.active);
        assert_eq!(allocation.highwater, 4097);
        let retained = allocation.retained_bytes();
        allocation = allocation.advance(b"\0;\x07plain text");
        assert!(!allocation.active);
        assert_eq!(allocation.retained_bytes(), retained);
        let interrupted = OscAllocation::default().advance(b"\x1b]52;payload\x1b]0;new\x1a");
        assert!(!interrupted.active);
        assert!(interrupted.highwater >= 9);
    }
    #[test]
    fn ordered_output_resumes_exact_remaining_bytes_after_admission_refusal() {
        let mut bytes = vec![b'a'; 63];
        bytes.extend_from_slice(b"\x1b]0;");
        bytes.extend(std::iter::repeat_n(b'x', 512));
        bytes.extend_from_slice(b"\x07\r\nlast line");
        let mut expected = TerminalState::new(4, 40);
        expected
            .apply_ordered_output(
                1,
                1,
                &bytes,
                true,
                &mut OrderedOutputCursor::default(),
                |_, _| Ok(()),
            )
            .unwrap();
        let mut state = TerminalState::new(4, 40);
        let mut cursor = OrderedOutputCursor::default();
        let mut chunks = 0;
        assert!(state
            .apply_ordered_output(1, 1, &bytes, true, &mut cursor, |_, _| {
                chunks += 1;
                if chunks > 2 {
                    Err("synthetic admission pressure".into())
                } else {
                    Ok(())
                }
            })
            .is_err());
        assert_eq!(cursor.consumed, 128);
        assert_eq!(state.last_output_sequence, 0);
        assert_eq!(state.collect_retained_history(), bytes[..128]);
        let private_before = state.osc_allocation.retained_bytes();
        state
            .apply_ordered_output(1, 1, &bytes, true, &mut cursor, |state, chunk| {
                assert!(state.input_peak_bytes(chunk) >= state.retained_allocation_bytes());
                Ok(())
            })
            .unwrap();
        assert_eq!(state.collect_retained_history(), bytes);
        assert_eq!(state.last_output_sequence, 1);
        assert_eq!(
            state.parser.screen().contents(),
            expected.parser.screen().contents()
        );
        assert!(state.osc_allocation.retained_bytes() >= private_before);
    }
    #[test]
    fn partial_replay_retains_origin_and_does_not_reset_processed_prefix_on_retry() {
        let mut state = TerminalState::new(4, 40);
        state.feed(b"old live history");
        let mut bytes = b"\x1b]0;".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', 256));
        bytes.extend_from_slice(b"\x07replacement");
        let mut cursor = OrderedOutputCursor::default();
        let mut chunks = 0;
        assert!(state
            .apply_ordered_replay(&bytes, 7, &mut cursor, |_, _| {
                chunks += 1;
                if chunks > 1 {
                    Err("synthetic replay pressure".into())
                } else {
                    Ok(())
                }
            })
            .is_err());
        assert_eq!(cursor.consumed, 64);
        assert!(cursor.replay_started);
        let origin = state.history_origin.clone();
        state
            .apply_ordered_replay(&bytes, 7, &mut cursor, |_, _| Ok(()))
            .unwrap();
        assert!(Arc::ptr_eq(&state.history_origin, &origin));
        assert_eq!(state.collect_retained_history(), bytes);
        assert_eq!(state.last_output_sequence, 7);
    }
    #[test]
    fn osc8_links_survive_split_output_chunks() {
        let mut view = TerminalView::new(4, 40);
        view.feed(b"\x1b]8;;https://example.test\x1b\\doc");
        view.feed(b"s\x1b]8;;\x1b\\");
        assert_eq!(
            view.osc8_link_at("docs", 1),
            Some("https://example.test".to_string())
        );
    }

    #[test]
    fn osc8_links_survive_a_split_that_lands_inside_the_opener_marker() {
        let mut view = TerminalView::new(4, 40);
        // The opener escape itself is split mid-marker, with unrelated
        // preceding output in the same first chunk. A buggy cross-chunk
        // parser that keeps the wrong end of its scratch buffer here would
        // discard the partial "\x1b]8" prefix and never recover it.
        view.feed(b"hello\x1b]8");
        view.feed(b";;https://example.test\x1b\\label\x1b]8;;\x1b\\");
        assert_eq!(
            view.osc8_link_at("label", 1),
            Some("https://example.test".to_string())
        );
    }

    #[test]
    fn a_stray_close_tag_does_not_swallow_the_link_that_follows_it() {
        let mut view = TerminalView::new(4, 40);
        // A close tag with no matching opener can reach this parser at the
        // front of the buffer -- after the stream-cap drain above discards
        // an opener, or after the server's own retained-history cap trims
        // one out of a replay. This synthesizes that directly: an orphan
        // close immediately followed by a genuine link. `CLOSE` is a
        // byte-for-byte prefix of `OPEN`, so misreading the orphan as an
        // opener would search for its "close" inside the real link's own
        // header and swallow the whole link.
        view.feed(b"\x1b]8;;\x1b\\\x1b]8;;https://example.test\x1b\\docs\x1b]8;;\x1b\\");
        assert_eq!(
            view.osc8_link_at("docs", 1),
            Some("https://example.test".to_string())
        );
    }

    #[test]
    fn osc8_link_lookup_uses_cell_columns_not_byte_offsets() {
        let mut view = TerminalView::new(4, 40);
        view.feed(b"\x1b]8;;https://example.test\x1b\\docs\x1b]8;;\x1b\\");
        // "界" is a double-width CJK character occupying byte offsets 0..3
        // but only cells 0..2, so cell 3 (the "d" of "docs") converts to
        // byte offset 4 -- inside "docs"'s 4..8 byte range. A raw,
        // unconverted comparison of the cell index against that byte range
        // would miss the label entirely.
        assert_eq!(
            view.osc8_link_at("界 docs", 3),
            Some("https://example.test".to_string())
        );
    }

    #[test]
    fn osc8_link_history_evicts_the_oldest_entry_at_constant_time() {
        let mut view = TerminalView::new(4, 40);
        for index in 0..257 {
            view.feed(
                format!("\x1b]8;;https://example.test/{index}\x1b\\link-{index}\x1b]8;;\x1b\\")
                    .as_bytes(),
            );
        }

        assert_eq!(view.osc8_links.len(), 256);
        assert_eq!(view.osc8_link_at("link-0", 1), None);
        assert_eq!(
            view.osc8_link_at("link-256", 1),
            Some("https://example.test/256".to_string())
        );
    }

    #[test]
    fn feed_updates_the_rendered_screen_text() {
        let mut view = TerminalView::new(4, 20);
        view.feed(b"hello");
        let text = view.with_screen(|screen| screen.contents());
        assert!(text.contains("hello"));
        assert!(
            view.osc8_stream.is_empty(),
            "ordinary output must not allocate OSC parsing scratch state"
        );
    }

    #[test]
    fn rendered_cell_cache_reuses_and_invalidates_the_visible_generation() {
        let mut view = TerminalView::new(3, 20);
        let area = Rect::new(0, 0, 20, 3);
        view.feed(b"first");
        let mut first_frame = Buffer::empty(area);
        view.render_screen(area, &mut first_frame);
        let cached_revision = view
            .render_cache
            .borrow()
            .as_ref()
            .expect("first render populates cache")
            .revision;

        let mut repeated_frame = Buffer::empty(area);
        view.render_screen(area, &mut repeated_frame);
        assert_eq!(repeated_frame, first_frame);
        assert_eq!(view.render_revision, cached_revision);

        view.feed(b"\rsecond");
        assert_ne!(view.render_revision, cached_revision);
        let mut changed_frame = Buffer::empty(area);
        view.render_screen(area, &mut changed_frame);
        assert_ne!(changed_frame, first_frame);
        assert_eq!(
            view.render_cache
                .borrow()
                .as_ref()
                .expect("changed render refreshes cache")
                .revision,
            view.render_revision
        );
    }

    #[test]
    fn copyable_history_omits_terminal_escape_sequences() {
        let mut view = TerminalView::new(4, 20);
        view.feed(b"\x1b[32mgreen\x1b[0m\r\nplain");

        assert_eq!(view.copyable_history(), "green\nplain");
    }

    #[test]
    fn logical_history_head_preserves_the_exact_bounded_tail_across_compaction() {
        let mut view = TerminalView::with_scrollback_budget_mib(4, 20, 1);
        let budget_bytes = 1024 * 1024;
        let chunk = vec![b'a'; 4096];
        let mut expected = Vec::new();

        for chunk_number in 0..400_u16 {
            let mut numbered_chunk = chunk.clone();
            numbered_chunk[0..2].copy_from_slice(&chunk_number.to_le_bytes());
            view.append_history(&numbered_chunk);
            expected.extend_from_slice(&numbered_chunk);
            let excess = expected.len().saturating_sub(budget_bytes);
            if excess > 0 {
                expected.drain(..excess);
            }
        }

        assert_eq!(view.searchable_history(), expected);
        assert_eq!(view.searchable_history().len(), budget_bytes);
        assert!(view
            .history_segments
            .front()
            .is_some_and(|segment| segment.start < view.history_budget_bytes / 2));
    }

    #[test]
    fn search_snapshot_remains_stable_while_live_history_rotates_segments() {
        let mut view = TerminalView::with_scrollback_budget_mib(4, 20, 1);
        let retained = vec![b'a'; 1024 * 1024];
        view.append_history(&retained);
        let snapshot = view.searchable_history_snapshot();

        view.append_history(&vec![b'b'; 4096]);

        assert_eq!(snapshot.to_vec(), retained);
        assert_eq!(snapshot.len(), 1024 * 1024);
        assert_eq!(view.history_segments.len(), 2);
        assert!(view.searchable_history().ends_with(&vec![b'b'; 4096]));
    }

    #[test]
    fn resize_changes_the_screen_dimensions() {
        let mut view = TerminalView::new(4, 20);
        view.resize(10, 40);
        let (rows, cols) = view.with_screen(|screen| screen.size());
        assert_eq!((rows, cols), (10, 40));
    }

    #[test]
    fn resize_truncating_wide_character_keeps_render_parser_usable() {
        let mut view = TerminalView::new(2, 216);
        view.feed("\u{1b}[215G界".as_bytes());

        view.resize(2, 215);
        view.feed(b"\x1b[0Jrender-cache-alive");

        assert_eq!(view.with_screen(|screen| screen.size()), (2, 215));
        assert!(view
            .with_screen(|screen| screen.contents())
            .contains("render-cache-alive"));
    }

    /// Pushes enough `\n`-terminated lines to overflow a small screen so
    /// several rows scroll off into `vt100`'s scrollback buffer.
    fn feed_lines(view: &mut TerminalView, count: usize) {
        for line in 0..count {
            view.feed(format!("line {line}\r\n").as_bytes());
        }
    }

    #[test]
    fn scrolled_off_rows_accumulate_in_scrollback() {
        let mut view = TerminalView::new(4, 20);
        assert_eq!(view.scrollback_total(), 0);
        feed_lines(&mut view, 10);
        assert!(view.scrollback_total() > 0);
    }

    #[test]
    fn top_anchored_scroll_region_accumulates_agent_history() {
        let mut view = TerminalView::new(4, 20);
        // Codex reserves lower rows for its input/status UI and scrolls the
        // transcript through a DECSTBM region anchored at the screen top.
        // Those rows still leave the physical viewport and must remain
        // available to ilium's wheel scrollback.
        view.feed(b"\x1b[1;3rfirst\r\nsecond\r\nthird\r\nfourth\r\n");

        let total = view.scrollback_total();
        assert!(total > 0);
        view.scroll_up(total as u16);
        assert!(view
            .with_screen(|screen| screen.contents())
            .contains("first"));
    }

    #[test]
    fn lower_scroll_region_does_not_pollute_terminal_history() {
        let mut view = TerminalView::new(4, 20);
        // A region below row zero is an application-owned widget. Rotating
        // it must not create fake terminal history entries.
        view.feed(b"\x1b[2;4rfirst\r\nsecond\r\nthird\r\nfourth\r\n");

        assert_eq!(view.scrollback_total(), 0);
    }

    #[test]
    fn scroll_up_and_down_move_the_position_and_clamp_at_both_ends() {
        let mut view = TerminalView::new(4, 20);
        feed_lines(&mut view, 10);
        let total = view.scrollback_total();

        view.scroll_up(3);
        assert_eq!(view.scrollback_position(), 3);
        assert!(view.is_scrolled_back());

        // Scrolling past the oldest row clamps at the total instead of
        // going further.
        view.scroll_up(total as u16 + 5);
        assert_eq!(view.scrollback_position(), total);

        view.scroll_down(total as u16 + 5);
        assert_eq!(view.scrollback_position(), 0);
        assert!(!view.is_scrolled_back());
    }

    #[test]
    fn scroll_to_bottom_resets_to_the_live_tail() {
        let mut view = TerminalView::new(4, 20);
        feed_lines(&mut view, 10);
        view.scroll_up(5);
        assert!(view.is_scrolled_back());

        view.scroll_to_bottom();
        assert_eq!(view.scrollback_position(), 0);
        assert!(!view.is_scrolled_back());
    }

    #[test]
    fn live_output_cannot_move_history_or_lengthen_the_return_to_live_tail() {
        let mut view = TerminalView::new(4, 24);
        feed_lines(&mut view, 16);
        view.scroll_up(5);
        let frozen_screen = view.with_screen(|screen| screen.contents());
        let frozen_position = view.scrollback_position();
        let frozen_total = view.scrollback_total();

        for line in 0..80 {
            view.feed(format!("live {line:02}\r\n").as_bytes());
        }

        assert_eq!(view.with_screen(|screen| screen.contents()), frozen_screen);
        assert_eq!(view.scrollback_position(), frozen_position);
        assert_eq!(view.scrollback_total(), frozen_total);

        view.scroll_down(frozen_position as u16);

        assert!(!view.is_scrolled_back());
        assert_eq!(view.scrollback_position(), 0);
        assert!(view
            .with_screen(|screen| screen.contents())
            .contains("live 79"));
    }

    #[test]
    fn live_resize_and_reflow_cannot_mutate_a_frozen_historical_viewport() {
        let mut view = TerminalView::new(4, 24);
        feed_lines(&mut view, 16);
        view.scroll_up(5);
        let frozen_screen = view.with_screen(|screen| screen.contents());
        let frozen_size = view.with_screen(vt100::Screen::size);

        view.resize(7, 48);
        view.feed(b"\x1b[r\x1b[0m\x1b[H\x1b[2J\x1b[3J\x1b[H");
        for line in 0..12 {
            view.feed(format!("canonical {line:02}\r\n").as_bytes());
        }

        assert_eq!(view.with_screen(vt100::Screen::size), frozen_size);
        assert_eq!(view.with_screen(|screen| screen.contents()), frozen_screen);

        view.scroll_to_bottom();

        assert_eq!(view.with_screen(vt100::Screen::size), (7, 48));
        assert!(view
            .with_screen(|screen| screen.contents())
            .contains("canonical 11"));
    }

    #[test]
    fn ansi_scrollback_purge_replaces_old_rows_during_agent_reflow() {
        let mut view = TerminalView::new(4, 24);
        for line in 0..16 {
            view.feed(format!("obsolete {line:02}\r\n").as_bytes());
        }
        assert!(view.scrollback_total() > 0);

        view.feed(b"\x1b[r\x1b[0m\x1b[H\x1b[2J\x1b[3J\x1b[H");
        assert_eq!(view.scrollback_total(), 0);
        for line in 0..12 {
            view.feed(format!("canonical {line:02}\r\n").as_bytes());
        }

        view.scroll_up(u16::MAX);
        let complete_reflow = view.with_screen(|screen| screen.contents());
        assert!(complete_reflow.contains("canonical 00"));
        assert!(!complete_reflow.contains("obsolete"));
    }

    #[test]
    fn a_fresh_view_reports_no_negotiated_mouse_protocol() {
        let view = TerminalView::new(4, 20);
        assert!(!view.wants_mouse_protocol());
    }

    #[test]
    fn bracketed_paste_mode_tracks_the_foreground_applications_negotiation() {
        let mut view = TerminalView::new(4, 20);
        assert!(!view.wants_bracketed_paste());

        view.feed(b"\x1b[?2004h");
        assert!(view.wants_bracketed_paste());

        view.feed(b"\x1b[?2004l");
        assert!(!view.wants_bracketed_paste());
    }

    #[test]
    fn input_modes_follow_the_live_application_while_history_is_frozen() {
        let mut view = TerminalView::new(4, 20);
        view.feed(b"\x1b[?1000h\x1b[?2004h");
        feed_lines(&mut view, 10);
        view.scroll_up(3);
        assert!(view.wants_mouse_protocol());
        assert!(view.wants_bracketed_paste());

        view.feed(b"\x1b[?1000l\x1b[?2004l");

        assert!(!view.wants_mouse_protocol());
        assert!(!view.wants_bracketed_paste());
        assert!(view.is_scrolled_back());
    }

    #[test]
    fn live_output_reports_only_visible_character_changes() {
        let mut view = TerminalView::new(3, 20);

        assert!(view.apply_live_output(1, 1, b"A", true));
        assert!(!view.apply_live_output(2, 2, b"\x1b[31m", true));
        assert!(!view.apply_live_output(3, 3, b"\x1b[2;2H", true));
        assert!(!view.apply_live_output(4, 4, b"\x1b[1;1HA", true));
        assert!(view.apply_live_output(5, 5, b"\x1b[1;1HB", true));
    }

    #[test]
    fn live_output_evidence_identifies_changed_visible_row_and_ignores_style_only_updates() {
        let mut view = TerminalView::new(3, 20);

        let evidence = view
            .apply_live_output_with_evidence(1, 1, b"hello", true)
            .expect("visible text change");

        assert_eq!(evidence.first_sequence, 1);
        assert_eq!(evidence.sequence, 1);
        assert_eq!(evidence.changed_rows, Some(1));
        assert_eq!(evidence.rows.len(), 1);
        assert_eq!(evidence.rows[0].row_number, 1);
        assert_eq!(evidence.rows[0].text, "hello");
        assert!(!evidence.rows[0].blank);
        assert!(view
            .apply_live_output_with_evidence(2, 2, b"\x1b[31m", true)
            .is_none());
    }

    #[test]
    fn replay_and_resize_refresh_the_activity_fingerprint_baseline() {
        let mut view = TerminalView::new(3, 20);

        view.apply_replay(b"one long terminal line", 4, true);
        view.resize(4, 8);

        assert!(!view.apply_live_output(5, 5, b"\x1b[32m", true));
    }

    #[test]
    fn agent_output_skips_fingerprinting_and_plain_shell_transition_resynchronizes() {
        let mut view = TerminalView::new(3, 20);

        assert!(!view.apply_live_output(1, 1, b"agent output", false));
        view.synchronize_visible_text_fingerprint();

        assert!(!view.apply_live_output(2, 2, b"\x1b[33m", true));
        assert!(view.apply_live_output(3, 3, b"\rplain output", true));
    }

    #[test]
    fn replay_restores_scrollback_and_deduplicates_queued_live_output() {
        let mut original = TerminalView::new(4, 20);
        feed_lines(&mut original, 12);

        let replay = (0..12)
            .map(|line| format!("line {line}\r\n"))
            .collect::<String>();
        let mut reattached = TerminalView::new(4, 20);
        reattached.apply_replay(replay.as_bytes(), 12, true);
        let total_after_replay = reattached.scrollback_total();
        assert!(total_after_replay > 0);

        reattached.apply_live_output(12, 12, b"duplicate\r\n", true);
        assert_eq!(reattached.scrollback_total(), total_after_replay);
        assert_eq!(reattached.last_output_sequence(), 12);

        reattached.apply_live_output(13, 13, b"new live output\r\n", true);
        assert_eq!(reattached.last_output_sequence(), 13);
        assert!(reattached
            .with_screen(|screen| screen.contents())
            .contains("new live output"));
    }

    #[test]
    fn stale_replay_cannot_roll_a_live_terminal_backward() {
        let mut view = TerminalView::new(4, 30);
        view.apply_replay(b"authoritative replay\r\n", 10, true);
        view.apply_live_output(11, 11, b"newer live output\r\n", true);
        let screen_before_stale_replay = view.with_screen(|screen| screen.contents());

        view.apply_replay(b"obsolete screen\r\n", 10, true);

        assert_eq!(
            view.with_screen(|screen| screen.contents()),
            screen_before_stale_replay
        );
        assert_eq!(view.last_output_sequence(), 11);
        assert!(!view
            .with_screen(|screen| screen.contents())
            .contains("obsolete screen"));
    }

    #[test]
    fn non_contiguous_live_output_cannot_corrupt_terminal_history() {
        let mut view = TerminalView::new(4, 30);
        view.apply_replay(b"authoritative replay\r\n", 10, true);
        let history_before_gap = view.searchable_history();

        view.apply_live_output(12, 12, b"output after a gap\r\n", true);

        assert_eq!(view.last_output_sequence(), 10);
        assert_eq!(view.searchable_history(), history_before_gap);
        assert!(!view
            .with_screen(|screen| screen.contents())
            .contains("output after a gap"));
    }

    #[test]
    fn live_replay_preserves_a_scrolled_historical_viewport() {
        let original_replay = (0..12)
            .map(|line| format!("line {line}\r\n"))
            .collect::<String>();
        let repaired_replay = (0..15)
            .map(|line| format!("line {line}\r\n"))
            .collect::<String>();
        let mut view = TerminalView::new(4, 20);
        view.apply_replay(original_replay.as_bytes(), 12, true);
        view.scroll_up(4);
        let historical_screen = view.with_screen(|screen| screen.contents());

        view.apply_replay(repaired_replay.as_bytes(), 15, true);

        assert_eq!(
            view.with_screen(|screen| screen.contents()),
            historical_screen,
            "lag repair must retain the same historical rows"
        );
        assert_eq!(view.last_output_sequence(), 15);
        assert!(view.is_scrolled_back());
    }

    #[test]
    fn live_replay_updates_history_without_discarding_a_search_anchor() {
        let mut view = TerminalView::new(3, 30);
        let initial = b"first\r\nneedle lives here\r\nlast\r\n";
        view.apply_replay(initial, 4, true);
        let needle_end = initial
            .windows(b"needle".len())
            .position(|window| window == b"needle")
            .unwrap()
            + b"needle".len();
        view.jump_to_history_byte(needle_end);
        let anchored_screen = view.with_screen(|screen| screen.contents());

        view.apply_replay(
            b"first\r\nneedle lives here\r\nlast\r\nnew live output\r\n",
            5,
            true,
        );

        assert_eq!(
            view.with_screen(|screen| screen.contents()),
            anchored_screen
        );
        assert!(view
            .searchable_history()
            .windows(b"new live output".len())
            .any(|window| window == b"new live output"));
        assert_eq!(view.last_output_sequence(), 5);
    }

    #[test]
    fn search_jump_rebuilds_the_terminal_around_an_old_history_match() {
        let mut view = TerminalView::new(3, 30);
        view.apply_replay(b"first\r\nneedle lives here\r\nlast\r\n", 4, true);
        let end = view
            .searchable_history()
            .windows(b"needle".len())
            .position(|window| window == b"needle")
            .expect("needle in retained history")
            + b"needle".len();

        view.jump_to_history_byte(end);

        assert!(view
            .with_screen(|screen| screen.contents())
            .contains("needle"));
    }
}

/// Interactive facade: reads immutable published state and submits ordered
/// semantic commands. Standalone mutable engines exist only in test fixtures.
pub struct TerminalView {
    snapshot: Arc<TerminalSnapshot>,
    pub(crate) frontend: Option<crate::terminal_parsing::PaneFrontend>,
    pub(crate) identity: Arc<()>,
    pub(crate) desired_size: (u16, u16),
    pub(crate) budget_mib: u16,
    pub(crate) admission_error: Option<String>,
    pub(crate) applied_ordinal: u64,
    render_cache: RefCell<Option<TerminalRenderCache>>,
    #[cfg(test)]
    standalone: Option<TerminalState>,
}
impl TerminalView {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self::with_scrollback_budget_mib(rows, cols, 32)
    }
    pub fn with_scrollback_budget_mib(rows: u16, cols: u16, budget_mib: u16) -> Self {
        // Bootstrap-sized blank placeholder only; user geometry is allocated by
        // the OS owner after admission. No input byte is parsed on this path.
        let blank =
            TerminalState::with_scrollback_budget_mib(DEFAULT_ROWS, DEFAULT_COLS, 0).publish();
        Self {
            snapshot: Arc::new(blank),
            frontend: None,
            identity: Arc::new(()),
            desired_size: (rows, cols),
            budget_mib,
            admission_error: None,
            applied_ordinal: 0,
            render_cache: RefCell::new(None),
            #[cfg(test)]
            standalone: Some(TerminalState::with_scrollback_budget_mib(
                rows, cols, budget_mib,
            )),
        }
    }
    pub(crate) fn painted_source(&self) -> PaintedTerminal {
        #[cfg(test)]
        let snapshot = self
            .standalone
            .as_ref()
            .map(|state| Arc::new(state.publish()))
            .unwrap_or_else(|| self.snapshot.clone());
        #[cfg(not(test))]
        let snapshot = self.snapshot.clone();
        let live = snapshot
            .allocation_charge
            .as_ref()
            .map(|charge| charge.lease_live());
        PaintedTerminal {
            identity: self.identity.clone(),
            ordinal: self.applied_ordinal,
            snapshot,
            _live: live,
            _pin: None,
        }
    }
    pub(crate) fn is_confirmed_removed(&self) -> bool {
        self.frontend
            .as_ref()
            .is_some_and(|frontend| frontend.is_confirmed_removed())
    }
    pub(crate) fn has_suspended_output(&self) -> bool {
        self.frontend
            .as_ref()
            .is_some_and(|frontend| frontend.has_suspended_output())
    }
    pub(crate) fn confirm_removed(&mut self, identity: &Arc<()>) -> bool {
        if !Arc::ptr_eq(identity, &self.identity) {
            return false;
        }
        self.frontend
            .as_mut()
            .is_some_and(|frontend| frontend.confirm_removed(identity))
    }
    pub(crate) fn try_preparation_snapshot(&self) -> Result<PreparationSnapshot, String> {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return Ok(PreparationSnapshot {
                snapshot: Arc::new(state.publish()),
                _pin: None,
            });
        }
        let pin = self
            .snapshot
            .allocation_charge
            .as_ref()
            .map(|charge| charge.pin())
            .transpose()?;
        Ok(PreparationSnapshot {
            snapshot: self.snapshot.clone(),
            _pin: pin,
        })
    }
    pub(crate) fn install(&mut self, snapshot: Arc<TerminalSnapshot>, ordinal: u64) {
        self.applied_ordinal = ordinal;
        if !Arc::ptr_eq(&self.snapshot, &snapshot) {
            if let Some(charge) = &self.snapshot.allocation_charge {
                charge.retire_live();
            }
        }
        self.snapshot = snapshot;
        *self.render_cache.get_mut() = None;
        self.admission_error = None;
    }
    fn command(&mut self, command: crate::terminal_parsing::PaneCommand) {
        if let Some(frontend) = &mut self.frontend {
            if let Err(error) = frontend.submit_intent(command) {
                self.admission_error = Some(error);
            }
        } else {
            self.admission_error = Some("terminal parser is awaiting registration".into());
        }
    }
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return state.with_screen(f);
        }
        f(&self.snapshot.visible)
    }
    pub fn render_screen(&self, area: Rect, destination: &mut Buffer) {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            state.render_screen(area, destination);
            return;
        }
        if area.is_empty() {
            return;
        }
        let mut cache = self.render_cache.borrow_mut();
        if cache
            .as_ref()
            .is_none_or(|cache| cache.revision != self.snapshot.revision || cache.area != area)
        {
            let mut buffer = Buffer::empty(area);
            render_frozen_screen(&self.snapshot.visible, area, &mut buffer);
            *cache = Some(TerminalRenderCache {
                revision: self.snapshot.revision,
                area,
                buffer,
            });
        }
        if let Some(cache) = cache.as_ref() {
            for row in area.y..area.bottom() {
                let source = cache.buffer.index_of(area.x, row);
                let target = destination.index_of(area.x, row);
                let width = usize::from(area.width);
                destination.content[target..target + width]
                    .clone_from_slice(&cache.buffer.content[source..source + width]);
            }
        }
    }
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.admit_resize(rows, cols);
    }
    /// Reject permanently unsupported geometry before sending a server resize.
    pub(crate) fn admit_resize(&mut self, rows: u16, cols: u16) -> bool {
        if let Err(error) = crate::terminal_parsing::validate_geometry(rows, cols) {
            self.admission_error = Some(error);
            return false;
        }
        #[cfg(test)]
        if let Some(state) = &mut self.standalone {
            state.resize(rows, cols);
            self.desired_size = (rows, cols);
            return true;
        }
        if self.desired_size == (rows, cols) {
            return true;
        }
        let Some(frontend) = &mut self.frontend else {
            // Registration uses this desired geometry before accepting any bytes.
            self.desired_size = (rows, cols);
            return true;
        };
        if let Err(error) =
            frontend.submit_intent(crate::terminal_parsing::PaneCommand::Resize(rows, cols))
        {
            self.admission_error = Some(error);
            return false;
        }
        self.desired_size = (rows, cols);
        true
    }
    pub fn set_scrollback_budget_mib(&mut self, budget: u16) {
        #[cfg(test)]
        if let Some(state) = &mut self.standalone {
            state.set_scrollback_budget_mib(budget);
            self.budget_mib = budget;
            return;
        }
        if self.budget_mib == budget {
            return;
        }
        self.budget_mib = budget;
        self.command(crate::terminal_parsing::PaneCommand::Budget(budget));
    }
    pub fn scroll_up(&mut self, lines: u16) {
        #[cfg(test)]
        if let Some(state) = &mut self.standalone {
            state.scroll_up(lines);
            return;
        }
        self.command(crate::terminal_parsing::PaneCommand::ScrollUp(lines));
    }
    pub fn scroll_down(&mut self, lines: u16) {
        #[cfg(test)]
        if let Some(state) = &mut self.standalone {
            state.scroll_down(lines);
            return;
        }
        self.command(crate::terminal_parsing::PaneCommand::ScrollDown(lines));
    }
    pub fn scroll_to_bottom(&mut self) {
        #[cfg(test)]
        if let Some(state) = &mut self.standalone {
            state.scroll_to_bottom();
            return;
        }
        self.command(crate::terminal_parsing::PaneCommand::Bottom);
    }
    pub(crate) fn jump_to_search_history(&mut self, byte: usize, origin: Arc<()>) {
        #[cfg(test)]
        if let Some(state) = &mut self.standalone {
            if state.history_origin_matches(&origin) {
                state.jump_to_history_byte(byte);
            } else {
                self.admission_error =
                    Some("Search history origin changed before navigation".into());
            }
            return;
        }
        self.command(crate::terminal_parsing::PaneCommand::HistoryFenced { byte, origin });
    }
    pub fn jump_to_history_byte(&mut self, byte: usize) {
        #[cfg(test)]
        if let Some(state) = &mut self.standalone {
            state.jump_to_history_byte(byte);
            return;
        }
        self.command(crate::terminal_parsing::PaneCommand::History(byte));
    }
    pub fn synchronize_visible_text_fingerprint(&mut self) {
        #[cfg(test)]
        if let Some(state) = &mut self.standalone {
            state.synchronize_visible_text_fingerprint();
            return;
        }
        self.command(crate::terminal_parsing::PaneCommand::Fingerprint);
    }
    pub fn is_scrolled_back(&self) -> bool {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return state.is_scrolled_back();
        }
        self.snapshot.scrolled_back
    }
    pub fn scrollback_position(&self) -> usize {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return state.scrollback_position();
        }
        self.snapshot.scrollback_position
    }
    pub fn scrollback_total(&self) -> usize {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return state.scrollback_total();
        }
        self.snapshot.scrollback_total
    }
    pub fn viewport_rows(&self) -> u16 {
        self.with_screen(|screen| screen.size().0)
    }
    pub fn wants_mouse_protocol(&self) -> bool {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return state.wants_mouse_protocol();
        }
        self.snapshot.mouse
    }
    pub fn wants_bracketed_paste(&self) -> bool {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return state.wants_bracketed_paste();
        }
        self.snapshot.paste
    }
    pub(crate) fn search_history_origin(&self) -> Arc<()> {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return Arc::clone(&state.history_origin);
        }
        Arc::clone(&self.snapshot.history.origin)
    }
    pub(crate) fn search_history_len(&self) -> usize {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return state.history_retained_len;
        }
        self.snapshot.history.len()
    }
    pub(crate) fn try_searchable_history_snapshot(
        &self,
    ) -> Result<TerminalHistorySnapshot, String> {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return Ok(state.searchable_history_snapshot());
        }
        let pin = self
            .snapshot
            .allocation_charge
            .as_ref()
            .map(|charge| charge.pin())
            .transpose()?;
        let mut history = self.snapshot.history.clone();
        history._pin_charge = pin;
        Ok(history)
    }
    #[cfg(test)]
    pub fn searchable_history_snapshot(&self) -> TerminalHistorySnapshot {
        self.try_searchable_history_snapshot()
            .expect("test history pin admission")
    }
    #[cfg(test)]
    pub fn searchable_history(&self) -> Vec<u8> {
        self.searchable_history_snapshot().to_vec()
    }
    #[cfg(test)]
    pub fn copyable_history(&self) -> String {
        String::from_utf8_lossy(&strip_ansi_escapes::strip(self.searchable_history())).into_owned()
    }
    #[cfg(test)]
    pub fn osc8_link_at(&self, line: &str, column: usize) -> Option<String> {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return state.osc8_link_at(line, column);
        }
        let column = crate::terminal_links::cell_column_to_byte_offset(line, column);
        self.snapshot
            .links
            .iter()
            .rev()
            .find_map(|(label, target)| {
                let start = line.find(label)?;
                (start..start + label.len())
                    .contains(&column)
                    .then(|| target.clone())
            })
    }
    #[cfg(test)]
    pub(crate) fn observe_osc8_for_benchmark(&mut self, bytes: &[u8]) {
        if let Some(state) = &mut self.standalone {
            state.observe_osc8_for_benchmark(bytes);
        }
    }
    #[cfg(test)]
    pub(crate) fn append_history_for_benchmark(&mut self, bytes: &[u8]) {
        if let Some(state) = &mut self.standalone {
            state.append_history_for_benchmark(bytes);
        }
    }
    #[cfg(test)]
    pub fn feed(&mut self, bytes: &[u8]) {
        if let Some(state) = &mut self.standalone {
            state.feed(bytes);
        }
    }
    #[cfg(test)]
    pub fn apply_replay(&mut self, bytes: &[u8], sequence: u64, complete: bool) {
        if let Some(state) = &mut self.standalone {
            state.apply_replay(bytes, sequence, complete);
        }
    }
    #[cfg(test)]
    pub fn apply_live_output(
        &mut self,
        first: u64,
        sequence: u64,
        bytes: &[u8],
        track: bool,
    ) -> bool {
        self.apply_live_output_with_evidence(first, sequence, bytes, track)
            .is_some()
    }
    #[cfg(test)]
    pub(crate) fn apply_live_output_with_evidence(
        &mut self,
        first: u64,
        sequence: u64,
        bytes: &[u8],
        track: bool,
    ) -> Option<VisibleTextEvidence> {
        self.standalone
            .as_mut()
            .and_then(|state| state.apply_live_output_with_evidence(first, sequence, bytes, track))
    }
    pub fn last_output_sequence(&self) -> u64 {
        #[cfg(test)]
        if let Some(state) = &self.standalone {
            return state.last_output_sequence;
        }
        self.snapshot.sequence
    }
    #[cfg(test)]
    pub(crate) fn attach_frontend(&mut self, frontend: crate::terminal_parsing::PaneFrontend) {
        self.standalone = None;
        self.frontend = Some(frontend);
    }
    #[cfg(not(test))]
    pub(crate) fn attach_frontend(&mut self, frontend: crate::terminal_parsing::PaneFrontend) {
        self.frontend = Some(frontend);
    }
}

impl Drop for TerminalView {
    fn drop(&mut self) {
        if let Some(charge) = &self.snapshot.allocation_charge {
            charge.retire_live();
        }
    }
}

fn context_cost(snapshot: &TerminalSnapshot) -> Result<ilium_execution::JobCost, String> {
    let (rows, cols) = snapshot.visible.size();
    let cells = usize::from(rows).saturating_mul(usize::from(cols));
    let history = snapshot.history.len();
    let source = snapshot
        .allocation_charge
        .as_ref()
        .map_or(128 * 1024, |charge| charge.bytes());
    let input_bytes = source
        .saturating_add(history.saturating_mul(2))
        .saturating_add(cells.saturating_mul(44))
        .saturating_add(128 * 1024);
    let result_bytes = history
        .saturating_mul(3)
        .saturating_add(cells.saturating_mul(44))
        .saturating_add(128 * 1024);
    if input_bytes > 384 * 1024 * 1024 || result_bytes > 256 * 1024 * 1024 {
        return Err("Context text exceeds declared preparation admission; reduce retained history or pane geometry".into());
    }
    Ok(ilium_execution::JobCost {
        input_bytes,
        result_bytes,
    })
}

#[cfg(test)]
mod search_origin_tests {
    use super::*;
    #[test]
    fn retained_search_journal_origin_survives_append_but_changes_on_trim_and_replay() {
        let mut view = TerminalView::with_scrollback_budget_mib(4, 40, 1);
        view.feed(b"old needle\r\n");
        let retained = view.try_searchable_history_snapshot().unwrap();
        let origin = view.search_history_origin();
        view.feed(b"new output\r\n");
        assert!(Arc::ptr_eq(&origin, &view.search_history_origin()));
        assert_eq!(retained.to_vec(), b"old needle\r\n");
        view.append_history_for_benchmark(&vec![b'x'; 1024 * 1024]);
        let trimmed = view.search_history_origin();
        assert!(!Arc::ptr_eq(&origin, &trimmed));
        assert_eq!(retained.to_vec(), b"old needle\r\n");
        view.apply_replay(b"replacement", 100, true);
        assert!(!Arc::ptr_eq(&trimmed, &view.search_history_origin()));
    }
}
