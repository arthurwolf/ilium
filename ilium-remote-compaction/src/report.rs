//! Progress reporting shared by the pipeline stages.

use crate::types::{CompactionEvent, TokenBreakdown};

pub(crate) struct Reporter<'a> {
    emit: &'a mut dyn FnMut(CompactionEvent),
    pub(crate) tokens: TokenBreakdown,
}

impl<'a> Reporter<'a> {
    pub(crate) fn new(emit: &'a mut dyn FnMut(CompactionEvent)) -> Self {
        Self {
            emit,
            tokens: TokenBreakdown::default(),
        }
    }

    pub(crate) fn step(&mut self, index: usize, total: usize, title: &str) {
        (self.emit)(CompactionEvent::Step {
            index,
            total,
            title: title.to_string(),
        });
    }

    pub(crate) fn progress(&mut self, fraction: f32) {
        (self.emit)(CompactionEvent::Progress(fraction.clamp(0.0, 1.0)));
    }

    pub(crate) fn log(&mut self, message: impl Into<String>) {
        (self.emit)(CompactionEvent::Log(message.into()));
    }

    /// Publishes the current breakdown.
    pub(crate) fn publish_tokens(&mut self) {
        (self.emit)(CompactionEvent::Tokens(self.tokens.clone()));
    }
}
