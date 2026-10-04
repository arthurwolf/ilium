//! Continuation engine for authored sources beyond the whole-snapshot bound.
//! No full-line clone or full-document Arc is required. One CPU continuation
//! owns this state; the UI fetches its next UTF-safe chunk from the exact fenced
//! TextArea revision, then moves state+chunk to the existing document CPU bank.
use ilium_execution::{
    Client, Job, JobContext, RejectReason, RetirementReservation, Retiring, RetiringArc,
    StorageAdmission,
};
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
pub(crate) const CHUNK: usize = 64 * 1024;
const MAX_SAMPLES: usize = 32768;
/// Checkpoint slab, two bounded UTF/grapheme buffers, boundary scratch and fixed state.
pub(crate) const STATE_STORAGE_BYTES: usize = MAX_SAMPLES * std::mem::size_of::<Checkpoint>()
    + CHUNK * 6
    + CHUNK * 2 * std::mem::size_of::<usize>()
    + std::mem::size_of::<Stream>()
    + (u16::MAX as usize) * std::mem::size_of::<crate::minimap::BucketFacts>()
    + 4096;
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cursor {
    pub physical: usize,
    pub byte: usize,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Checkpoint {
    pub visual: usize,
    pub physical: usize,
    pub byte: usize,
    pub column: usize,
    facts: crate::source_line_facts::LineFacts,
}
pub(crate) struct Glyph {
    pub text: String,
    pub physical: usize,
    pub byte: usize,
    pub source_bytes: usize,
    pub column: usize,
    pub columns: usize,
    pub cells: usize,
    pub display: usize,
}
pub(crate) struct Row {
    pub visual: usize,
    pub glyphs: Vec<Glyph>,
    pub physical: usize,
    pub start_byte: usize,
    pub start_column: usize,
    pub last: bool,
    pub checkbox: Option<(usize, bool)>,
    #[cfg(test)]
    _drop_probe: Option<RowDropProbe>,
}
#[cfg(test)]
struct RowDropProbe(std::sync::mpsc::Sender<std::thread::ThreadId>);
#[cfg(test)]
impl Drop for RowDropProbe {
    fn drop(&mut self) {
        let _ = self.0.send(std::thread::current().id());
    }
}
pub(crate) struct Viewport {
    pub identity: Arc<()>,
    pub top: usize,
    pub left: usize,
    pub rows: RetiringArc<Vec<Row>>,
    pub total_rows: Option<usize>,
    pub scanned_until: Cursor,
    pub minimap: Option<RetiringArc<crate::minimap::PreparedMinimap>>,
    pub styles: Option<ilium_execution::RetiringArc<crate::source_window_syntax::WindowStyles>>,
    pub allocation: Arc<StorageAdmission>,
}
/// Three independently clonable final owners share the original frame guard.
/// Every permit is acquired before any CPU callback constructs its projection.
pub(crate) struct FrameCustody {
    viewport: RetirementReservation<Viewport>,
    rows: RetirementReservation<Vec<Row>>,
    minimap: RetirementReservation<crate::minimap::PreparedMinimap>,
}
impl FrameCustody {
    pub fn reserve(client: &Client) -> Result<Self, RejectReason> {
        let retirement = client.retirement();
        Ok(Self {
            viewport: retirement.try_reserve::<Viewport>(std::mem::size_of::<Viewport>() + 4096)?,
            rows: retirement.try_reserve::<Vec<Row>>(crate::presentation::MAX_FRAME_BYTES)?,
            minimap: retirement.try_reserve::<crate::minimap::PreparedMinimap>(
                crate::presentation::MAX_FRAME_BYTES,
            )?,
        })
    }
}
pub(crate) struct Stream {
    identity: Arc<()>,
    cursor: Cursor,
    physical_count: usize,
    visual: usize,
    column: usize,
    facts: crate::source_line_facts::LineFacts,
    row_byte: usize,
    row_column: usize,
    row_width: usize,
    width: usize,
    minimap_content_width: u16,
    wanted_cursor: Option<(usize, usize)>,
    cursor_row: Option<usize>,
    cursor_display: Option<usize>,
    clip: bool,
    left: usize,
    tab: usize,
    pending: String,
    pending_byte: usize,
    top: usize,
    height: usize,
    rows: Vec<Row>,
    published: bool,
    last_window: Option<RetiringArc<Viewport>>,
    frame_custody: Option<FrameCustody>,
    frame_bytes: usize,
    samples: Vec<Checkpoint>,
    stride: usize,
    minimap: Vec<crate::minimap::BucketFacts>,
    completed_facts: usize,
    #[cfg(test)]
    last_seek_thread: Option<std::thread::ThreadId>,
    #[cfg(test)]
    row_drop_probe: Option<std::sync::mpsc::Sender<std::thread::ThreadId>>,
    allocation: Arc<ilium_execution::StorageAdmission>,
    window_allocation: Arc<ilium_execution::StorageAdmission>,
}
impl Stream {
    pub fn new(
        identity: Arc<()>,
        physical_count: usize,
        width: usize,
        clip: bool,
        left: usize,
        tab: u8,
        top: usize,
        height: u16,
        allocation: Arc<ilium_execution::StorageAdmission>,
        window_allocation: Arc<ilium_execution::StorageAdmission>,
        frame_custody: FrameCustody,
    ) -> Result<Self, String> {
        let height = usize::from(height);
        let rows_bytes = height
            .checked_mul(std::mem::size_of::<Row>())
            .ok_or("source window row metadata overflow")?;
        let glyph_bytes = height
            .checked_mul(width.max(1))
            .and_then(|count| count.checked_mul(std::mem::size_of::<Glyph>()))
            .ok_or("source window glyph metadata overflow")?;
        let minimap_bytes = height
            .checked_mul(
                2 * std::mem::size_of::<ratatui::text::Line>()
                    + 4 * std::mem::size_of::<ratatui::text::Span>()
                    + usize::from(crate::editor_chrome::MINIMAP_WIDTH) * 6
                    + 128,
            )
            .ok_or("source minimap projection capacity overflow")?;
        let frame_bytes = rows_bytes
            .checked_add(minimap_bytes)
            .and_then(|bytes| bytes.checked_add(glyph_bytes))
            .and_then(|bytes| bytes.checked_add(1024))
            .ok_or("source window metadata overflow")?;
        if frame_bytes > crate::presentation::MAX_FRAME_BYTES {
            return Err("source viewport metadata exceeds admitted frame storage; previous usable frame retained".into());
        }
        Ok(Self {
            identity,
            cursor: Cursor {
                physical: 0,
                byte: 0,
            },
            physical_count,
            visual: 0,
            column: 0,
            facts: crate::source_line_facts::LineFacts::default(),
            row_byte: 0,
            row_column: 0,
            row_width: 0,
            width: width.max(1),
            minimap_content_width: width.min(usize::from(u16::MAX)) as u16,
            wanted_cursor: None,
            cursor_row: None,
            cursor_display: None,
            clip,
            left,
            tab: usize::from(tab).max(1),
            pending: String::with_capacity(CHUNK * 2),
            pending_byte: 0,
            top,
            height,
            rows: Vec::with_capacity(height),
            published: false,
            last_window: None,
            frame_custody: Some(frame_custody),
            frame_bytes,
            samples: Vec::with_capacity(MAX_SAMPLES),
            stride: 64,
            minimap: (0..height)
                .map(|_| crate::minimap::BucketFacts::default())
                .collect(),
            completed_facts: 0,
            #[cfg(test)]
            last_seek_thread: None,
            #[cfg(test)]
            row_drop_probe: None,
            allocation,
            window_allocation,
        })
    }
    pub fn set_cursor(&mut self, cursor: (usize, usize), follow: bool) {
        let wanted = follow.then_some(cursor);
        if self.wanted_cursor != wanted {
            self.cursor_row = None;
            self.cursor_display = None;
            self.wanted_cursor = wanted;
        }
    }
    pub fn seek_cursor(
        self,
        cursor: (usize, usize),
        height: u16,
        left: usize,
        allocation: Arc<ilium_execution::StorageAdmission>,
    ) -> Result<Self, String> {
        let top = self
            .samples
            .iter()
            .rfind(|point| (point.physical, point.column) <= cursor)
            .map_or(0, |point| point.visual);
        self.seek(top, height, left, allocation)
    }
    pub fn set_minimap_content_width(&mut self, width: u16) {
        self.minimap_content_width = width;
    }
    pub fn ensure_frame_custody(&mut self, client: &Client) -> Result<(), RejectReason> {
        if self.frame_custody.is_none() {
            self.frame_custody = Some(FrameCustody::reserve(client)?);
        }
        Ok(())
    }
    /// A viewport change reuses bounded sampled geometry from the SAME source
    /// and starts at an exact grapheme/wrap boundary. It does not recapture a
    /// whole physical line. A content/width/tab revision requires a new Stream.
    pub fn seek(
        mut self,
        top: usize,
        height: u16,
        left: usize,
        window_allocation: Arc<ilium_execution::StorageAdmission>,
    ) -> Result<Self, String> {
        let height = usize::from(height);
        let frame_bytes = height
            .checked_mul(
                std::mem::size_of::<Row>()
                    + 2 * std::mem::size_of::<ratatui::text::Line>()
                    + 4 * std::mem::size_of::<ratatui::text::Span>()
                    + usize::from(crate::editor_chrome::MINIMAP_WIDTH) * 6
                    + 128,
            )
            .and_then(|rows| {
                height
                    .checked_mul(self.width)
                    .and_then(|n| n.checked_mul(std::mem::size_of::<Glyph>()))
                    .and_then(|glyphs| rows.checked_add(glyphs))
            })
            .ok_or("source viewport metadata overflow")?;
        if frame_bytes > crate::presentation::MAX_FRAME_BYTES {
            return Err("source viewport exceeds admitted frame metadata".into());
        }
        let point = self
            .samples
            .iter()
            .rfind(|point| point.visual <= top)
            .copied()
            .unwrap_or(Checkpoint {
                visual: 0,
                physical: 0,
                byte: 0,
                column: 0,
                facts: crate::source_line_facts::LineFacts::default(),
            });
        self.cursor = Cursor {
            physical: point.physical,
            byte: point.byte,
        };
        self.visual = point.visual;
        self.column = point.column;
        self.facts = point.facts;
        self.row_byte = point.byte;
        self.row_column = point.column;
        self.row_width = 0;
        self.pending.clear();
        self.pending_byte = point.byte;
        self.top = top;
        self.left = left;
        self.height = height;
        #[cfg(test)]
        {
            self.last_seek_thread = Some(std::thread::current().id());
        }
        self.rows = Vec::with_capacity(height);
        self.frame_bytes = frame_bytes;
        self.window_allocation = window_allocation;
        self.published = false;
        self.last_window = None;
        Ok(self)
    }

    /// UI capture has no iteration over a prefix: exact byte offset is supplied
    /// by the last CPU continuation. Caller checks instance/revision/settings.
    pub fn capture(&self, lines: &[String], byte_limit: usize) -> Result<Chunk, String> {
        if lines.len() != self.physical_count {
            return Err("source revision changed".into());
        }
        let line = lines
            .get(self.cursor.physical)
            .ok_or("source stream is complete")?;
        if !line.is_char_boundary(self.cursor.byte) {
            return Err("source byte fence changed".into());
        }
        let mut end = self
            .cursor
            .byte
            .saturating_add(CHUNK.min(byte_limit))
            .min(line.len());
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        let mut text = String::with_capacity(end - self.cursor.byte);
        text.push_str(&line[self.cursor.byte..end]);
        Ok(Chunk {
            text,
            end_of_line: end == line.len(),
            at: self.cursor,
        })
    }
    fn ensure_row(&mut self) {
        if self.published {
            return;
        }
        if self.visual < self.top || self.visual >= self.top.saturating_add(self.height) {
            return;
        }
        if self.rows.last().is_none_or(|row| row.visual != self.visual) {
            self.rows.push(Row {
                visual: self.visual,
                glyphs: Vec::with_capacity(self.width),
                physical: self.cursor.physical,
                start_byte: self.row_byte,
                start_column: self.row_column,
                last: false,
                checkbox: self.facts.checkbox(),
                #[cfg(test)]
                _drop_probe: self.row_drop_probe.clone().map(RowDropProbe),
            });
        }
    }
    fn checkpoint(&mut self) {
        if self.visual % self.stride != 0 {
            return;
        }
        if self.samples.len() == MAX_SAMPLES {
            self.stride = self.stride.saturating_mul(2);
            let stride = self.stride;
            self.samples.retain(|point| point.visual % stride == 0);
        }
        if self
            .samples
            .last()
            .is_none_or(|point| point.visual < self.visual)
        {
            self.samples.push(Checkpoint {
                visual: self.visual,
                physical: self.cursor.physical,
                byte: self.row_byte,
                column: self.row_column,
                facts: self.facts,
            });
        }
    }
    fn accept_grapheme(&mut self, text: &str, byte: usize) -> Result<(), String> {
        let initial = if text == "\t" {
            self.tab - self.row_width % self.tab
        } else {
            text.width().max(1)
        };
        let wrap =
            !self.clip && self.row_width > 0 && self.row_width.saturating_add(initial) > self.width;
        if wrap {
            self.visual = self
                .visual
                .checked_add(1)
                .ok_or("source visual row overflow")?;
            self.row_width = 0;
            self.row_byte = byte;
            self.row_column = self.column;
        }
        self.checkpoint();
        if self.wanted_cursor.is_some_and(|(physical, column)| {
            physical == self.cursor.physical
                && column >= self.column
                && column < self.column + text.chars().count()
        }) {
            self.cursor_row = Some(self.visual);
            self.cursor_display = Some(self.row_width);
        }
        self.ensure_row();
        let cells = if text == "\t" {
            self.tab - self.row_width % self.tab
        } else {
            initial
        };
        let columns = text.chars().count();
        let visible = !self.clip
            || (self.row_width.saturating_add(cells) > self.left
                && self.row_width < self.left.saturating_add(self.width));
        if let Some(row) = self
            .rows
            .last_mut()
            .filter(|row| row.visual == self.visual && visible)
        {
            // CPU chooses every displayed glyph; paint never scans an original
            // line, including a horizontal prefix or an oversized source.
            let displayed_bytes = if text == "\t" { cells } else { text.len() };
            let frame_bytes = self
                .frame_bytes
                .checked_add(displayed_bytes)
                .ok_or("source frame text capacity overflow")?;
            if frame_bytes > crate::presentation::MAX_FRAME_BYTES {
                return Err(
                    "source glyph bytes exceed admitted frame storage; original source retained"
                        .into(),
                );
            }
            self.frame_bytes = frame_bytes;
            let mut owned = String::with_capacity(displayed_bytes);
            if text == "\t" {
                owned.extend(std::iter::repeat_n(' ', cells));
            } else {
                owned.push_str(text);
            }
            row.glyphs.push(Glyph {
                text: owned,
                physical: self.cursor.physical,
                byte,
                source_bytes: text.len(),
                column: self.column,
                columns,
                cells,
                display: self.row_width,
            });
        }
        for ch in text.chars() {
            self.facts.accept(ch);
        }
        if let Some(checkbox) = self.facts.checkbox() {
            for row in self
                .rows
                .iter_mut()
                .filter(|row| row.physical == self.cursor.physical)
            {
                row.checkbox = Some(checkbox);
            }
        }
        self.column = self
            .column
            .checked_add(columns)
            .ok_or("source character column overflow")?;
        self.row_width = self
            .row_width
            .checked_add(cells)
            .ok_or("source display column overflow")?;
        Ok(())
    }
    fn consume(
        mut self,
        chunk: Chunk,
        cancelled: impl Fn() -> bool,
    ) -> Result<RawContinuation, String> {
        if chunk.at.physical != self.cursor.physical || chunk.at.byte != self.cursor.byte {
            return Err("out of order source continuation".into());
        }
        if self.pending.len().saturating_add(chunk.text.len()) > CHUNK * 2 {
            return Err("one grapheme exceeds bounded continuation scratch; original authored source retained".into());
        }
        if self.pending.is_empty() {
            self.pending_byte = self.cursor.byte;
        }
        self.pending.push_str(&chunk.text);
        self.cursor.byte += chunk.text.len();
        // Only the final cluster can require the next chunk. Extended cluster
        // context is preserved by retaining that complete suffix, never split.
        // Only the trailing boundary is needed. Retaining all per-scalar
        // offsets creates avoidable CPU scratch proportional to this page.
        let final_boundary = self
            .pending
            .grapheme_indices(true)
            .last()
            .map(|(byte, _)| byte);
        let until = if chunk.end_of_line {
            self.pending.len()
        } else {
            final_boundary.unwrap_or(0)
        };
        let pending = std::mem::replace(&mut self.pending, String::with_capacity(CHUNK * 2));
        for (offset, text) in pending[..until].grapheme_indices(true) {
            if cancelled() {
                return Err("source continuation cancelled".into());
            }
            self.accept_grapheme(text, self.pending_byte + offset)?;
        }
        self.pending.push_str(&pending[until..]);
        self.pending_byte += until;
        if chunk.end_of_line {
            if self.wanted_cursor == Some((self.cursor.physical, self.column)) {
                self.cursor_row = Some(self.visual);
                self.cursor_display = Some(self.row_width);
            }
            if self.cursor.physical >= self.completed_facts {
                let bucket = self.physical_count.div_ceil(self.height.max(1)).max(1);
                if let Some(facts) = self.minimap.get_mut(self.cursor.physical / bucket) {
                    facts.add(&self.facts)?;
                }
                self.completed_facts = self.cursor.physical + 1;
            }
            self.ensure_row();
            if let Some(row) = self.rows.last_mut().filter(|row| row.visual == self.visual) {
                row.last = true;
            }
            self.visual = self
                .visual
                .checked_add(1)
                .ok_or("source visual row overflow")?;
            self.cursor.physical += 1;
            self.cursor.byte = 0;
            self.pending_byte = 0;
            self.column = 0;
            self.facts = crate::source_line_facts::LineFacts::default();
            self.row_byte = 0;
            self.row_column = 0;
            self.row_width = 0;
        }
        if let Some(cursor_row) = self.cursor_row {
            let desired = if cursor_row < self.top {
                cursor_row
            } else if cursor_row >= self.top.saturating_add(self.height) {
                cursor_row.saturating_sub(self.height).saturating_add(1)
            } else {
                self.top
            };
            let display = self.cursor_display.unwrap_or(self.left);
            let left = if self.clip && display < self.left {
                display
            } else if self.clip && display >= self.left.saturating_add(self.width) {
                display.saturating_sub(self.width).saturating_add(1)
            } else {
                self.left
            };
            if desired != self.top || left != self.left {
                let height = self.height as u16;
                let allocation = self.window_allocation.clone();
                // No frame has been published for this capture yet while the
                // cursor was unresolved. Replaying from a bounded checkpoint
                // rebuilds only the wanted window on the same CPU owner.
                let stream = self.seek(desired, height, left, allocation)?;
                return Ok(RawContinuation {
                    total_rows: None,
                    stream: Some(stream),
                    window: None,
                });
            }
        }
        let complete = self.cursor.physical == self.physical_count;
        let ready = !self.published
            && (self.wanted_cursor.is_none() || self.cursor_row.is_some())
            && (complete
                || self.visual >= self.top.saturating_add(self.height)
                || self.clip
                    && self.visual.saturating_add(1) >= self.top.saturating_add(self.height)
                    && self.row_width >= self.left.saturating_add(self.width));
        if ready {
            self.published = true;
        }
        let window = if ready {
            let FrameCustody {
                viewport,
                rows,
                minimap,
            } = self
                .frame_custody
                .take()
                .ok_or("source viewport retirement owner unavailable")?;
            let minimap = if complete {
                let mut prepared = minimap.attach(crate::minimap::prepare_facts(
                    &self.minimap,
                    crate::editor_chrome::MINIMAP_WIDTH,
                    self.minimap_content_width,
                ));
                prepared.set_storage_guard(self.window_allocation.clone());
                Some(Arc::new(prepared))
            } else {
                None
            };
            let mut rows = rows.attach(std::mem::take(&mut self.rows));
            rows.set_storage_guard(self.window_allocation.clone());
            let window = viewport.attach_shared(Viewport {
                identity: self.identity.clone(),
                top: self.top,
                left: self.left,
                rows: Arc::new(rows),
                total_rows: complete.then_some(self.visual),
                scanned_until: self.cursor,
                minimap,
                styles: None,
                allocation: self.window_allocation.clone(),
            });
            self.last_window = Some(window.clone());
            Some(window)
        } else if complete {
            // Publish final total-row metadata even when the usable glyph
            // window was emitted earlier. Arc rows are shared, never copied
            // under a second unaccounted allocation.
            let FrameCustody {
                viewport, minimap, ..
            } = self
                .frame_custody
                .take()
                .ok_or("source final-geometry retirement owner unavailable")?;
            let mut prepared = minimap.attach(crate::minimap::prepare_facts(
                &self.minimap,
                crate::editor_chrome::MINIMAP_WIDTH,
                self.minimap_content_width,
            ));
            prepared.set_storage_guard(self.window_allocation.clone());
            let minimap = Arc::new(prepared);
            self.last_window.as_ref().map(|old| {
                viewport.attach_shared(Viewport {
                    identity: old.identity.clone(),
                    top: old.top,
                    left: old.left,
                    rows: old.rows.clone(),
                    total_rows: Some(self.visual),
                    scanned_until: self.cursor,
                    minimap: Some(minimap),
                    styles: None,
                    allocation: old.allocation.clone(),
                })
            })
        } else {
            None
        };
        Ok(RawContinuation {
            total_rows: complete.then_some(self.visual),
            stream: Some(self),
            window,
        })
    }
}
pub(crate) struct Chunk {
    text: String,
    end_of_line: bool,
    at: Cursor,
}
impl Chunk {
    pub fn captured_bytes(&self) -> usize {
        self.text.len()
    }
    pub fn completed_line(&self) -> usize {
        usize::from(self.end_of_line)
    }
}
struct RawContinuation {
    pub total_rows: Option<usize>,
    pub stream: Option<Stream>,
    pub window: Option<RetiringArc<Viewport>>,
}
pub(crate) struct Continuation {
    pub total_rows: Option<usize>,
    pub stream: Option<Retiring<Stream>>,
    pub window: Option<RetiringArc<Viewport>>,
}
pub(crate) enum ContinueJob {
    Capture {
        stream: Retiring<Stream>,
        chunk: Retiring<Chunk>,
    },
    // Replacing old row/glyph buffers can be substantial deallocation. Seek
    // owns them on the existing CPU bank, never in the UI request producer.
    Seek {
        stream: Retiring<Stream>,
        top: usize,
        height: u16,
        left: usize,
        cursor: Option<(usize, usize)>,
        allocation: Arc<ilium_execution::StorageAdmission>,
    },
}
impl Job for ContinueJob {
    type Output = Continuation;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Continuation, String> {
        if context.stop_requested() {
            return Err("source continuation cancelled".into());
        }
        match self {
            Self::Capture { stream, chunk } => {
                let chunk = chunk
                    .try_consume_on_cpu(|chunk| chunk)
                    .map_err(|_chunk| "source capture has no CPU retirement owner".to_owned())?;
                let mut produced = None;
                let stream = stream
                    .try_update_on_cpu(|stream| {
                        let mut result = stream.consume(chunk, || context.stop_requested())?;
                        let next = result
                            .stream
                            .take()
                            .ok_or("source continuation state unavailable")?;
                        produced = Some((result.total_rows, result.window.take()));
                        Ok::<Stream, String>(next)
                    })
                    .map_err(|_stream| {
                        "source continuation has no CPU retirement owner".to_owned()
                    })??;
                let (total_rows, window) =
                    produced.ok_or("source continuation output unavailable")?;
                Ok(Continuation {
                    total_rows,
                    stream: Some(stream),
                    window,
                })
            }
            Self::Seek {
                stream,
                top,
                height,
                left,
                cursor,
                allocation,
            } => {
                let stream = stream
                    .try_update_on_cpu(|stream| match cursor {
                        Some(cursor) => stream.seek_cursor(cursor, height, left, allocation),
                        None => stream.seek(top, height, left, allocation),
                    })
                    .map_err(|_stream| "source seek has no CPU retirement owner".to_owned())??;
                Ok(Continuation {
                    total_rows: None,
                    stream: Some(stream),
                    window: None,
                })
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_bank_seek_retires_old_buffers_on_cpu_and_preserves_emitted_window() {
        use ilium_execution::{JobCost, JobOutcome, JobPoll, Lane};
        let client = crate::execution::test_document_client();
        let state = Arc::new(
            client
                .quota_group()
                .reserve_external_storage(STATE_STORAGE_BYTES)
                .unwrap(),
        );
        let frame = Arc::new(
            client
                .quota_group()
                .reserve_external_storage(crate::presentation::MAX_FRAME_BYTES)
                .unwrap(),
        );
        let source = vec!["x".repeat(1024)];
        let stream_retirement = client
            .retirement()
            .try_reserve::<Stream>(STATE_STORAGE_BYTES + crate::presentation::MAX_FRAME_BYTES)
            .unwrap();
        let mut stream = stream_retirement.attach(
            Stream::new(
                Arc::new(()),
                1,
                4,
                false,
                0,
                4,
                0,
                2,
                state,
                frame,
                FrameCustody::reserve(&client).unwrap(),
            )
            .unwrap(),
        );
        let (drop_sender, drop_receiver) = std::sync::mpsc::channel();
        stream.row_drop_probe = Some(drop_sender);
        let chunk_retirement = client
            .retirement()
            .try_reserve::<Chunk>(CHUNK + 4096)
            .unwrap();
        let chunk = chunk_retirement.attach(stream.capture(&source, CHUNK).unwrap());
        let cost = JobCost {
            input_bytes: STATE_STORAGE_BYTES + crate::presentation::MAX_FRAME_BYTES,
            result_bytes: crate::presentation::MAX_FRAME_BYTES,
        };
        let mut receipt = client
            .try_reserve(Lane::Cpu, cost)
            .unwrap()
            .submit(ContinueJob::Capture { stream, chunk })
            .ok()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let retained = loop {
            match receipt.try_take() {
                JobPoll::Ready(outcome) => break outcome,
                JobPoll::Pending => {}
                _ => panic!("capture receipt lost"),
            };
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (outcome, hold) = retained.into_parts();
        let JobOutcome::Finished(Ok(mut captured)) = outcome else {
            panic!("CPU capture failed")
        };
        drop(hold);
        drop(receipt);
        let emitted = captured.window.take().unwrap();
        let original_rows = emitted.rows.clone();
        let mut stream = captured.stream.take().unwrap();
        stream.ensure_frame_custody(&client).unwrap();
        let frame = Arc::new(
            client
                .quota_group()
                .reserve_external_storage(crate::presentation::MAX_FRAME_BYTES)
                .unwrap(),
        );
        let mut receipt = client
            .try_reserve(Lane::Cpu, cost)
            .unwrap()
            .submit(ContinueJob::Seek {
                stream,
                top: 3,
                height: 2,
                left: 0,
                cursor: None,
                allocation: frame,
            })
            .ok()
            .unwrap();
        let retained = loop {
            match receipt.try_take() {
                JobPoll::Ready(outcome) => break outcome,
                JobPoll::Pending => {}
                _ => panic!("seek receipt lost"),
            };
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (outcome, hold) = retained.into_parts();
        let JobOutcome::Finished(Ok(seeked)) = outcome else {
            panic!("CPU seek failed")
        };
        let stream = seeked.stream.as_ref().unwrap();
        assert_ne!(stream.last_seek_thread, Some(std::thread::current().id()));
        assert_eq!(stream.top, 3);
        assert!(seeked.window.is_none());
        assert!(Arc::ptr_eq(&emitted.rows, &original_rows));
        assert_eq!(emitted.top, 0);
        assert_eq!(emitted.rows[0].glyphs[0].text, "x");
        let row_count = original_rows.len();
        let ui_thread = std::thread::current().id();
        drop(emitted);
        drop(original_rows);
        for _ in 0..row_count {
            assert_ne!(
                drop_receiver
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .unwrap(),
                ui_thread
            );
        }
        drop(hold);
    }
    #[test]
    fn authored_line_beyond_loader_bound_projects_without_whole_original_copy() {
        let client = crate::execution::test_document_client();
        let source = vec!["a".repeat(crate::filesystem::editor::MAX_EDITOR_SOURCE_BYTES + 1)];
        let allocation = Arc::new(
            client
                .quota_group()
                .reserve_external_storage(4 * 1024 * 1024)
                .unwrap(),
        );
        let identity = Arc::new(());
        let stream_retirement = client
            .retirement()
            .try_reserve::<Stream>(STATE_STORAGE_BYTES + crate::presentation::MAX_FRAME_BYTES)
            .unwrap();
        let stream = stream_retirement.attach(
            Stream::new(
                identity.clone(),
                1,
                80,
                false,
                0,
                4,
                0,
                4,
                allocation.clone(),
                allocation,
                FrameCustody::reserve(&client).unwrap(),
            )
            .unwrap(),
        );
        // The first window becomes usable after64KiB, while the source still
        // owns its original33MiB TextArea authority. No full-original capture.
        let chunk_retirement = client
            .retirement()
            .try_reserve::<Chunk>(CHUNK + 4096)
            .unwrap();
        let chunk = chunk_retirement.attach(stream.capture(&source, CHUNK).unwrap());
        assert!(chunk.text.capacity() <= CHUNK);
        let receipt = client
            .try_reserve(
                ilium_execution::Lane::Cpu,
                ilium_execution::JobCost {
                    input_bytes: 4 * 1024 * 1024,
                    result_bytes: 4 * 1024 * 1024,
                },
            )
            .unwrap()
            .submit(ContinueJob::Capture { stream, chunk })
            .unwrap();
        let mut receipt = receipt;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let outcome = loop {
            match receipt.try_take() {
                ilium_execution::JobPoll::Ready(result) => break result,
                ilium_execution::JobPoll::Pending => {
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::yield_now();
                }
                _ => panic!("receipt lost"),
            }
        };
        let ilium_execution::JobOutcome::Finished(Ok(continuation)) = outcome.view() else {
            panic!("CPU continuation failed")
        };
        let window = continuation.window.as_ref().unwrap();
        assert_eq!(window.rows.len(), 4);
        assert!(window.total_rows.is_none());
        assert!(Arc::ptr_eq(&identity, &window.identity));
        assert_eq!(window.rows[3].start_column, 240);
        assert_eq!(window.rows[3].glyphs.len(), 80);
        assert!(continuation.stream.is_some());
        assert_eq!(
            source[0].len(),
            crate::filesystem::editor::MAX_EDITOR_SOURCE_BYTES + 1
        );
    }
}
