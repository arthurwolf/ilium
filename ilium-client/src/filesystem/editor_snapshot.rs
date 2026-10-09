//! CPU-owned measurement and capture of an editor's immutable save revision.
//!
//! The coordinator first measures an owned editor model, reserves the exact
//! FIFO writer cost, and only then submits capture. These helpers deliberately
//! do not clone source text before that reservation.

use super::editor::{EditorWrite, MAX_EDITOR_LINES, MAX_EDITOR_SOURCE_BYTES};
use crate::editor_pane::EditorPane;
use ilium_execution::{Job, JobContext, JobCost};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SnapshotMeasure {
    pub line_count: usize,
    pub source_bytes: usize,
    pub retained_bytes: usize,
}

pub(crate) fn measure_lines(lines: &[String]) -> Result<SnapshotMeasure, String> {
    if lines.len() > MAX_EDITOR_LINES {
        return Err("Editor save exceeds line limit".into());
    }
    let mut source_bytes = 0_usize;
    let mut retained_bytes = 0_usize;
    for line in lines {
        source_bytes = source_bytes
            .checked_add(line.len())
            .and_then(|bytes| bytes.checked_add(1))
            .ok_or_else(|| "Editor source byte overflow".to_string())?;
        retained_bytes = retained_bytes
            .checked_add(line.capacity())
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<String>()))
            .ok_or_else(|| "Editor retained byte overflow".to_string())?;
        if source_bytes > MAX_EDITOR_SOURCE_BYTES {
            return Err("Editor save exceeds 32 MiB source limit".into());
        }
        if retained_bytes > super::editor::MAX_EDITOR_RETAINED_BYTES {
            return Err("Editor save exceeds retained byte limit".into());
        }
    }
    Ok(SnapshotMeasure {
        line_count: lines.len(),
        source_bytes,
        retained_bytes,
    })
}

/// Owns the entire pane while measuring its exact save bound on a CPU worker.
pub(crate) struct MeasureEditorSave {
    pub pane: Box<EditorPane>,
}

pub(crate) struct MeasuredEditorSave {
    pub pane: Box<EditorPane>,
    pub measure: SnapshotMeasure,
}

pub(crate) struct EditorSnapshotError {
    pub pane: Box<EditorPane>,
    pub message: String,
}

impl std::fmt::Debug for EditorSnapshotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EditorSnapshotError")
            .field("message", &self.message)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for EditorSnapshotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Job for MeasureEditorSave {
    type Output = MeasuredEditorSave;
    type Error = EditorSnapshotError;

    fn run(self, _context: JobContext) -> Result<Self::Output, Self::Error> {
        let measured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            measure_lines(self.pane.textarea.lines())
        }));
        match measured {
            Ok(Ok(measure)) => Ok(MeasuredEditorSave {
                pane: self.pane,
                measure,
            }),
            Ok(Err(message)) => Err(EditorSnapshotError {
                pane: self.pane,
                message,
            }),
            Err(_) => Err(EditorSnapshotError {
                pane: self.pane,
                message: "Editor save measurement panicked; buffer was restored".into(),
            }),
        }
    }
}

/// Captures the exact immutable revision only after its ordered writer cost
/// has already been admitted. The editable model returns with the snapshot.
pub(crate) struct CaptureEditorSave {
    pub pane: Box<EditorPane>,
    pub measure: SnapshotMeasure,
}

pub(crate) struct CapturedEditorSave {
    pub pane: Box<EditorPane>,
    pub lines: Arc<[String]>,
    pub worker_thread: std::thread::ThreadId,
}

pub(crate) struct RetireEditorPane {
    pub pane: Option<Box<EditorPane>>,
}

impl Job for RetireEditorPane {
    type Output = ();
    type Error = String;

    fn run(mut self, _context: JobContext) -> Result<(), String> {
        drop(self.pane.take());
        Ok(())
    }
}

impl Job for CaptureEditorSave {
    type Output = CapturedEditorSave;
    type Error = EditorSnapshotError;

    fn run(self, _context: JobContext) -> Result<Self::Output, Self::Error> {
        let captured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let lines = self.pane.textarea.lines();
            let actual = measure_lines(lines)?;
            if actual != self.measure {
                return Err("Editor changed while its save revision was loaned".to_string());
            }
            Ok::<_, String>(lines.to_vec())
        }));
        match captured {
            Ok(Ok(lines)) => Ok(CapturedEditorSave {
                pane: self.pane,
                lines: lines.into(),
                worker_thread: std::thread::current().id(),
            }),
            Ok(Err(message)) => Err(EditorSnapshotError {
                pane: self.pane,
                message,
            }),
            Err(_) => Err(EditorSnapshotError {
                pane: self.pane,
                message: "Editor save capture panicked; buffer was restored".into(),
            }),
        }
    }
}

pub(crate) fn capture_cost(measure: SnapshotMeasure, path_capacity: usize) -> JobCost {
    let snapshot_bytes = measure.retained_bytes.saturating_add(256 * 1024);
    JobCost {
        input_bytes: measure
            .retained_bytes
            .saturating_add(path_capacity)
            .saturating_add(256 * 1024),
        // The result keeps the editable pane and a distinct immutable source
        // snapshot alive at the same time.
        result_bytes: snapshot_bytes
            .saturating_add(measure.retained_bytes)
            .saturating_add(path_capacity),
    }
}

pub(crate) fn writer_cost(measure: SnapshotMeasure, path_capacity: usize) -> JobCost {
    let bytes = std::mem::size_of::<EditorWrite>()
        .saturating_add(path_capacity)
        .saturating_add(measure.retained_bytes);
    JobCost {
        input_bytes: bytes.saturating_add(256 * 1024),
        result_bytes: path_capacity.saturating_add(256 * 1024),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_counts_file_newlines_and_retained_string_capacity() {
        let lines = vec![String::from("abc"), String::new()];
        let measured = measure_lines(&lines).expect("small editor snapshot is valid");

        assert_eq!(measured.line_count, 2);
        assert_eq!(measured.source_bytes, 5);
        assert_eq!(
            measured.retained_bytes,
            lines
                .iter()
                .map(|line| line.capacity() + std::mem::size_of::<String>())
                .sum::<usize>()
        );
    }

    #[test]
    fn measurement_refuses_more_than_the_editor_line_limit() {
        let lines = vec![String::new(); MAX_EDITOR_LINES + 1];

        assert_eq!(
            measure_lines(&lines).unwrap_err(),
            "Editor save exceeds line limit"
        );
    }
}
