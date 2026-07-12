//! Periodic (non-input-driven) maintenance run once per event-loop tick or
//! whenever a background naming worker finishes: layout animation, editor
//! autosave, and applying finished `crate::naming_workers` results.

use std::time::Instant;

use crate::app::App;
use crate::naming_workers::{NamingWorkerEvent, NamingWorkers};

/// Runs every redraw tick, regardless of whether any input event fired
/// this iteration.
pub fn on_tick(app: &mut App, now: Instant) {
    app.tick_layout_animation(now);
    app.tick_autosave();
}

/// Applies one finished background naming result to `app`, and tells
/// `workers` it's no longer in flight.
pub fn apply_naming_worker_event(
    app: &mut App,
    workers: &mut NamingWorkers,
    event: NamingWorkerEvent,
) {
    match event {
        NamingWorkerEvent::ProjectName(result) => {
            workers.project_name_worker_finished();
            app.is_project_name_loading = false;
            match result {
                Ok(bootstrap) => app.project_name = Some(bootstrap.project_name),
                Err(err) => {
                    app.status_message = Some(format!("Could not infer project name: {err}"))
                }
            }
        }
        NamingWorkerEvent::SessionTitle(pane_id, result) => {
            workers.session_title_worker_finished(pane_id);
            app.titles_loading.remove(&pane_id);
            match result {
                Ok(title) => app.request_rename(pane_id, title),
                Err(err) => {
                    app.status_message = Some(format!("Could not infer session title: {err}"))
                }
            }
        }
    }
}
