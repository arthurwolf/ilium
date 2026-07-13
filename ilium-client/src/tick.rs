//! Periodic (non-input-driven) maintenance run once per event-loop tick or
//! whenever a background naming worker finishes: layout animation, editor
//! autosave, and applying finished `crate::naming_workers` results.

use std::time::Instant;

use crate::app::App;
use crate::naming_workers::{NamingWorkerEvent, NamingWorkers, TitleTrigger};

/// Runs every poll tick, regardless of whether any input/`ServerEvent`
/// fired this iteration. Returns whether anything it did actually changed
/// visible state, so `crate::run`'s event loop knows whether this
/// otherwise-silent tick still needs to force a redraw (a "Working"
/// spinner, a "Done" pulse, a recently-created flash, and the tree-width
/// hover animation are all wall-clock-driven and keep animating with no
/// new event at all -- see `App::has_active_animation`).
pub fn on_tick(app: &mut App, now: Instant) -> bool {
    // Read *before* advancing the animation so the tick that finishes an
    // in-progress transition still reports "was animating" and forces its
    // own final redraw -- `tick_layout_animation` and the animation
    // spinners/pulses below share this same "still active as of the start
    // of this tick" contract.
    let was_animating = app.has_active_animation();
    app.tick_layout_animation(now);
    let autosave_wrote = app.tick_autosave();
    was_animating || autosave_wrote
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
        NamingWorkerEvent::SessionTitle(pane_id, session_id, result, trigger) => {
            workers.session_title_worker_finished(pane_id, &session_id);
            match trigger {
                TitleTrigger::Automatic => {
                    if app.agent_session_ids.get(&pane_id) == Some(&session_id) {
                        app.titles_loading.remove(&pane_id);
                    }
                    if app.agent_session_ids.get(&pane_id) != Some(&session_id) {
                        // The pane changed sessions while the request was in
                        // flight. Never apply an old transcript's title to
                        // the new session.
                        return;
                    }
                }
                TitleTrigger::Manual => {
                    // A manual retitle click isn't keyed to a particular
                    // session snapshot the way the automatic retry path is
                    // -- the user asked for a fresh title for whatever this
                    // pane is now, so it always applies (or reports its
                    // error) rather than getting dropped for a `/resume`
                    // that happened mid-flight.
                    app.titles_loading.remove(&pane_id);
                }
            }
            match result {
                Ok(title) => match trigger {
                    TitleTrigger::Automatic => {
                        // Not `request_rename`: that would permanently mark
                        // the pane user-specified (see `Tree::rename_node`),
                        // which would (a) treat an automatic inference as if
                        // the user had typed the name themselves, and (b)
                        // block any *other* automatic title source (e.g.
                        // the plain-shell typed-command titler) from ever
                        // updating it again. `request_automatic_pane_title`
                        // keeps this an automatic proposal the server only
                        // accepts while the pane hasn't been genuinely
                        // user-renamed.
                        app.inferred_title_session_ids.insert(pane_id, session_id);
                        app.request_automatic_pane_title(pane_id, title.long, Some(title.short));
                    }
                    TitleTrigger::Manual => {
                        // Here `request_rename` is exactly right: the user
                        // explicitly asked for this title just now, so it
                        // should win unconditionally -- see
                        // `TitleTrigger::Manual`'s doc comment.
                        app.request_rename(pane_id, title.long, Some(title.short));
                    }
                },
                Err(err) => {
                    // Deliberately no permanent failure marker here (unlike
                    // the pre-client/server bin crate's `titles_inference_failed`):
                    // `title_inference::MAX_ATTEMPTS` already bounds the
                    // retries `title_inference::pane_ready_for_inference`'s
                    // `PaneBecameDone` trigger drives, so a merely-unlucky
                    // attempt (e.g. the transcript had nothing to
                    // summarize yet) gets a few more chances instead of
                    // silently never being retried for the rest of the run.
                    app.status_message = Some(format!("Could not infer session title: {err}"));
                }
            }
        }
        NamingWorkerEvent::TerminalTitle(pane_id, result, trigger) => {
            workers.terminal_title_worker_finished(pane_id);
            app.titles_loading.remove(&pane_id);
            match result {
                Ok(title) => match trigger {
                    TitleTrigger::Automatic => {
                        app.request_automatic_pane_title(pane_id, title.long, Some(title.short));
                    }
                    TitleTrigger::Manual => {
                        app.request_rename(pane_id, title.long, Some(title.short));
                    }
                },
                Err(err) => {
                    app.status_message = Some(format!("Could not infer terminal title: {err}"));
                }
            }
        }
    }
}
