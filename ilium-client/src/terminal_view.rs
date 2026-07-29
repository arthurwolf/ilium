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

use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Starting geometry for a freshly created pane, before the first real
/// `ResizePane` request (sent once the client knows the pane's actual
/// on-screen content box) corrects it.
pub const DEFAULT_ROWS: u16 = 24;
pub const DEFAULT_COLS: u16 = 80;

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Hashes only the visible character contents of each terminal cell.
///
/// Cursor movement, color/style changes, terminal modes, and scrollback do
/// not affect this value. Iterating cells avoids allocating `Screen::contents`
/// for every already-coalesced live output batch.
fn visible_text_fingerprint(screen: &vt100::Screen) -> u64 {
    let mut hasher = DefaultHasher::new();
    let (rows, columns) = screen.size();

    for row in 0..rows {
        for column in 0..columns {
            screen
                .cell(row, column)
                .map(vt100::Cell::contents)
                .unwrap_or_default()
                .hash(&mut hasher);
        }
    }

    hasher.finish()
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

/// Computes the raw-history retention cap in bytes for a given MiB budget.
/// Unlike the render parser's row cap above, this figure is exact -- the
/// journal it governs (`TerminalView::history_bytes`) stores raw output
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

pub struct TerminalView {
    /// Authoritative live terminal state. Its scrollback offset stays at zero;
    /// historical navigation happens only through `historical_viewport`.
    parser: vt100::Parser,
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
    history_bytes: Arc<Vec<u8>>,
    /// Current raw-history retention cap in bytes, derived from the
    /// configured MiB budget via `history_budget_bytes`. Stored so
    /// `set_scrollback_budget_mib` can re-trim `history_bytes` in place
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
    /// decision without any timer-driven screen scan.
    visible_text_fingerprint: u64,
}

impl TerminalView {
    /// Starts a fresh, blank screen at `rows`x`cols` -- the caller should
    /// send `ClientRequest::ResizePane` promptly after creating a pane so
    /// the server-side PTY matches, and this view's own `resize` keeps the
    /// local parser matching whatever the client's own layout computed.
    pub fn new(rows: u16, cols: u16) -> Self {
        Self::with_scrollback_budget_mib(rows, cols, 32)
    }

    pub fn with_scrollback_budget_mib(rows: u16, cols: u16, budget_mib: u16) -> Self {
        let parser = vt100::Parser::new(rows, cols, RENDER_SCROLLBACK_ROWS);
        let visible_text_fingerprint = visible_text_fingerprint(parser.screen());

        Self {
            parser,
            scrollback_total: 0,
            historical_viewport: None,
            last_output_sequence: 0,
            history_bytes: Arc::new(Vec::new()),
            history_budget_bytes: history_budget_bytes(budget_mib),
            osc8_links: VecDeque::new(),
            osc8_stream: Vec::new(),
            visible_text_fingerprint,
        }
    }

    /// Re-caps the raw-history journal under a new MiB budget and
    /// immediately trims `history_bytes` down to it if the budget shrank.
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
    pub fn feed(&mut self, bytes: &[u8]) {
        self.observe_osc8_links(bytes);
        self.append_history(bytes);
        self.parser.process(bytes);
        self.refresh_scrollback_total();
        self.refresh_visible_text_fingerprint();
    }

    /// Replaces the live parser with the server-owned retained output.
    /// Attach starts from a blank view; lag repair leaves a detached
    /// historical snapshot untouched and ignores stale replays so recovery
    /// cannot move the visible viewport or roll the live screen backward.
    pub fn apply_replay(&mut self, bytes: &[u8], through_sequence: u64, _is_complete: bool) {
        if through_sequence <= self.last_output_sequence {
            return;
        }

        let (rows, cols) = self.parser.screen().size();
        Arc::make_mut(&mut self.history_bytes).clear();
        self.osc8_links.clear();
        self.osc8_stream.clear();
        self.observe_osc8_links(bytes);
        self.append_history(bytes);
        self.last_output_sequence = through_sequence;

        self.parser = vt100::Parser::new(rows, cols, RENDER_SCROLLBACK_ROWS);
        self.scrollback_total = 0;
        self.parser.process(bytes);
        self.refresh_scrollback_total();
        self.refresh_visible_text_fingerprint();
    }

    /// Applies a live output chunk only when it was not already included in
    /// the attach replay. Output sequence numbers are pane-local and
    /// monotonic for one server lifetime. Returns `true` only when the
    /// accepted bytes changed at least one visible character cell and the
    /// caller requested ordinary-terminal activity tracking. Known agent
    /// panes skip the O(visible cells) fingerprint entirely.
    pub fn apply_live_output(
        &mut self,
        first_sequence: u64,
        sequence: u64,
        bytes: &[u8],
        should_track_visible_text_change: bool,
    ) -> bool {
        if sequence <= self.last_output_sequence {
            return false;
        }
        let expected_first_sequence = self.last_output_sequence.saturating_add(1);
        if first_sequence != expected_first_sequence {
            tracing::error!(
                first_sequence,
                sequence,
                last_output_sequence = self.last_output_sequence,
                "ignoring non-contiguous terminal output"
            );
            return false;
        }
        self.append_history(bytes);
        self.observe_osc8_links(bytes);
        self.parser.process(bytes);
        self.refresh_scrollback_total();
        self.last_output_sequence = sequence;

        should_track_visible_text_change && self.refresh_visible_text_fingerprint()
    }

    /// Re-establishes the visible-text baseline when a detected agent becomes
    /// an ordinary terminal again after its live output skipped fingerprinting.
    pub fn synchronize_visible_text_fingerprint(&mut self) {
        self.refresh_visible_text_fingerprint();
    }

    /// Updates the saved visible-text baseline and reports whether it changed.
    fn refresh_visible_text_fingerprint(&mut self) -> bool {
        let previous_fingerprint = self.visible_text_fingerprint;
        self.visible_text_fingerprint = visible_text_fingerprint(self.parser.screen());
        self.visible_text_fingerprint != previous_fingerprint
    }

    pub fn osc8_link_at(&self, line: &str, column: usize) -> Option<String> {
        self.osc8_links.iter().rev().find_map(|(label, target)| {
            let start = line.find(label)?;
            (start..start + label.len())
                .contains(&column)
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
            let Some(open_start) = find_bytes(&self.osc8_stream, OPEN) else {
                // No opener anywhere in the buffer. Keep only the longest
                // tail that is itself a genuine prefix of `OPEN` -- that's
                // the part that can still complete once the next chunk
                // arrives. `truncate` would keep the *head* of the buffer
                // instead, which discards exactly the partial-opener bytes
                // this cross-chunk parse depends on.
                let partial_opener_len = (1..OPEN.len())
                    .rev()
                    .find(|length| self.osc8_stream.ends_with(&OPEN[..*length]))
                    .unwrap_or(0);
                let drop_through = self.osc8_stream.len().saturating_sub(partial_opener_len);
                self.osc8_stream.drain(..drop_through);
                return;
            };
            if open_start > 0 {
                self.osc8_stream.drain(..open_start);
            }
            let Some((header_end, header_terminator_width)) =
                osc_terminator(&self.osc8_stream[OPEN.len()..])
            else {
                return;
            };
            let header_end = OPEN.len() + header_end;
            let target = String::from_utf8_lossy(&self.osc8_stream[OPEN.len()..header_end])
                .rsplit(';')
                .next()
                .unwrap_or_default()
                .to_string();
            let label_start = header_end + header_terminator_width;
            let Some(close_start) = find_bytes(&self.osc8_stream[label_start..], CLOSE)
                .map(|offset| label_start + offset)
            else {
                return;
            };
            let close_after = close_start + CLOSE.len();
            let Some((close_end, close_terminator_width)) =
                osc_terminator(&self.osc8_stream[close_after..])
            else {
                return;
            };
            let label = String::from_utf8_lossy(&self.osc8_stream[label_start..close_start])
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

    /// Resizes the authoritative live screen to match a `ResizePane` request
    /// just sent to the server, so the client's own rendering never waits on
    /// a round trip before reflowing. An active historical viewport retains
    /// the geometry and rows the user was inspecting; the foreground
    /// application may redraw the live parser at the new geometry without
    /// corrupting that frozen view.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
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
    }

    /// Jumps back to the live tail -- called whenever the pane sends fresh
    /// input, matching how an ordinary terminal emulator drops you back to
    /// the prompt the moment you start typing again.
    pub fn scroll_to_bottom(&mut self) {
        self.historical_viewport = None;
        self.parser.screen_mut().set_scrollback(0);
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

    /// Number of rows in the currently visible terminal screen, used as the
    /// scrollbar's viewport length rather than its default one-row thumb.
    pub fn viewport_rows(&self) -> u16 {
        self.with_screen(|screen| screen.size().0)
    }

    /// Returns all output retained for workspace search, including bytes the
    /// visible parser has already rotated out of its render scrollback.
    pub fn searchable_history(&self) -> &[u8] {
        self.history_bytes.as_slice()
    }

    /// Returns all retained terminal output as clipboard-safe plain text.
    /// Escape sequences control the terminal display rather than representing
    /// user-visible history, so copying them would leak cursor and color
    /// commands into the destination application.
    pub fn copyable_history(&self) -> String {
        String::from_utf8_lossy(&strip_ansi_escapes::strip(self.history_bytes.as_slice()))
            .into_owned()
    }

    /// Produces an O(1) immutable view of retained output for the search
    /// worker. Later PTY output uses `Arc::make_mut`, so a worker can scan a
    /// stable history snapshot without blocking the interactive event loop.
    pub fn searchable_history_snapshot(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.history_bytes)
    }

    /// Builds a historical terminal around one raw-output offset found by
    /// workspace search. The matching bytes become the newest visible output,
    /// so the result opens at its actual place rather than merely focusing the
    /// correct pane. The authoritative live parser keeps processing output;
    /// the ordinary live view returns on the next input or wheel journey to
    /// the bottom.
    pub fn jump_to_history_byte(&mut self, end_byte: usize) {
        let (rows, cols) = self.parser.screen().size();
        let end_byte = end_byte.min(self.history_bytes.len());
        let mut historical_parser = vt100::Parser::new(rows, cols, RENDER_SCROLLBACK_ROWS);
        historical_parser.process(&self.history_bytes[..end_byte]);
        let scrollback_total = Self::measure_scrollback_total(historical_parser.screen_mut());
        self.historical_viewport = Some(HistoricalViewport {
            screen: historical_parser.screen().clone(),
            scrollback_total,
        });
    }

    #[cfg(test)]
    pub fn last_output_sequence(&self) -> u64 {
        self.last_output_sequence
    }

    /// `true` once the pane's foreground app has negotiated an xterm mouse
    /// protocol -- when it has, wheel events belong to that app (it's
    /// asking to receive them), not to this view's own scrollback
    /// navigation. Most agent CLIs (and a plain shell prompt) never
    /// negotiate one, which is exactly the case this feature targets.
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
        Arc::make_mut(&mut self.history_bytes).extend_from_slice(bytes);
        self.trim_history_to_budget();
    }

    /// Drops the oldest retained bytes past `history_budget_bytes`. Called
    /// after every append and whenever the budget itself shrinks.
    fn trim_history_to_budget(&mut self) {
        let excess = self
            .history_bytes
            .len()
            .saturating_sub(self.history_budget_bytes);
        if excess > 0 {
            Arc::make_mut(&mut self.history_bytes).drain(..excess);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn copyable_history_omits_terminal_escape_sequences() {
        let mut view = TerminalView::new(4, 20);
        view.feed(b"\x1b[32mgreen\x1b[0m\r\nplain");

        assert_eq!(view.copyable_history(), "green\nplain");
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
        let history_before_gap = view.history_bytes.as_ref().clone();

        view.apply_live_output(12, 12, b"output after a gap\r\n", true);

        assert_eq!(view.last_output_sequence(), 10);
        assert_eq!(view.history_bytes.as_ref(), &history_before_gap);
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
