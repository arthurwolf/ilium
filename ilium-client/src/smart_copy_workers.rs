//! Owned blocking worker for one streamed Smart Copy inference request.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ilium_core::NodeId;
use ilium_inference::{InferenceRequest, InferenceSettings, InferenceStreamEvent};
use tokio::sync::mpsc::Sender;

use crate::smart_copy::MAXIMUM_JSONL_LINE_BYTES;

pub struct SmartCopyWorkerRequest {
    pub generation: u64,
    pub pane_id: NodeId,
    pub inference_settings: InferenceSettings,
    pub request: InferenceRequest,
}

#[derive(Debug)]
pub struct SmartCopyWorkerEvent {
    pub generation: u64,
    pub pane_id: NodeId,
    pub update: SmartCopyWorkerUpdate,
}

#[derive(Debug)]
pub enum SmartCopyWorkerUpdate {
    ResponseStarted,
    Progress { received_characters: usize },
    JsonLine(String),
    ExactOutputTokens(u64),
    Finished,
    Failed(String),
}

pub struct SmartCopyWorkers {
    events_tx: Sender<SmartCopyWorkerEvent>,
    cancellation: Option<Arc<AtomicBool>>,
    active_generation: Option<u64>,
}

impl SmartCopyWorkers {
    pub fn new(events_tx: Sender<SmartCopyWorkerEvent>) -> Self {
        Self {
            events_tx,
            cancellation: None,
            active_generation: None,
        }
    }

    pub fn start(&mut self, request: SmartCopyWorkerRequest) {
        self.cancel();
        let cancellation = Arc::new(AtomicBool::new(false));
        self.cancellation = Some(Arc::clone(&cancellation));
        self.active_generation = Some(request.generation);
        let events_tx = self.events_tx.clone();
        std::thread::spawn(move || {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::BelowNormal,
            );
            run_worker(request, cancellation, events_tx);
        });
    }

    pub fn cancel(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            cancellation.store(true, Ordering::Release);
        }
        self.active_generation = None;
    }

    pub fn finish(&mut self, generation: u64) {
        if self.active_generation == Some(generation) {
            self.cancellation = None;
            self.active_generation = None;
        }
    }
}

fn run_worker(
    request: SmartCopyWorkerRequest,
    cancellation: Arc<AtomicBool>,
    events_tx: Sender<SmartCopyWorkerEvent>,
) {
    let provider = ilium_inference::provider_from_settings(&request.inference_settings);
    let mut buffer = String::new();
    let mut received_characters = 0usize;
    let mut response_started = false;
    let generation = request.generation;
    let pane_id = request.pane_id;
    let result = provider.stream(&request.request, &mut |event| {
        if cancellation.load(Ordering::Acquire) {
            return false;
        }
        match event {
            InferenceStreamEvent::TextDelta(delta) => {
                if !response_started {
                    response_started = true;
                    if !send_update(
                        &events_tx,
                        generation,
                        pane_id,
                        SmartCopyWorkerUpdate::ResponseStarted,
                    ) {
                        return false;
                    }
                }
                received_characters = received_characters.saturating_add(delta.chars().count());
                buffer.push_str(&delta);
                while let Some(newline) = buffer.find('\n') {
                    let mut remainder = buffer.split_off(newline + 1);
                    std::mem::swap(&mut remainder, &mut buffer);
                    let line = remainder.trim_end_matches(['\r', '\n']).trim();
                    if !line.is_empty()
                        && line != "```"
                        && line != "```jsonl"
                        && !send_update(
                            &events_tx,
                            generation,
                            pane_id,
                            SmartCopyWorkerUpdate::JsonLine(line.to_string()),
                        )
                    {
                        return false;
                    }
                }
                if buffer.len() > MAXIMUM_JSONL_LINE_BYTES {
                    let _ = send_update(
                        &events_tx,
                        generation,
                        pane_id,
                        SmartCopyWorkerUpdate::Failed(
                            "Smart Copy JSONL record exceeded 64 KiB".to_string(),
                        ),
                    );
                    return false;
                }
                let _ = events_tx.try_send(SmartCopyWorkerEvent {
                    generation,
                    pane_id,
                    update: SmartCopyWorkerUpdate::Progress {
                        received_characters,
                    },
                });
            }
            InferenceStreamEvent::OutputTokens(tokens) => {
                if !send_update(
                    &events_tx,
                    generation,
                    pane_id,
                    SmartCopyWorkerUpdate::ExactOutputTokens(tokens),
                ) {
                    return false;
                }
            }
        }
        !cancellation.load(Ordering::Acquire)
    });

    if cancellation.load(Ordering::Acquire) {
        return;
    }
    if let Err(error) = result {
        let _ = send_update(
            &events_tx,
            generation,
            pane_id,
            SmartCopyWorkerUpdate::Failed(error.to_string()),
        );
        return;
    }
    if !send_update(
        &events_tx,
        generation,
        pane_id,
        SmartCopyWorkerUpdate::Progress {
            received_characters,
        },
    ) {
        return;
    }
    let tail = buffer.trim();
    if !tail.is_empty()
        && tail != "```"
        && tail != "```jsonl"
        && !send_update(
            &events_tx,
            generation,
            pane_id,
            SmartCopyWorkerUpdate::JsonLine(tail.to_string()),
        )
    {
        return;
    }
    let _ = send_update(
        &events_tx,
        generation,
        pane_id,
        SmartCopyWorkerUpdate::Finished,
    );
}

fn send_update(
    events_tx: &Sender<SmartCopyWorkerEvent>,
    generation: u64,
    pane_id: NodeId,
    update: SmartCopyWorkerUpdate,
) -> bool {
    events_tx
        .blocking_send(SmartCopyWorkerEvent {
            generation,
            pane_id,
            update,
        })
        .is_ok()
}
