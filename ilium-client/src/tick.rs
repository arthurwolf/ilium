//! Periodic (non-input-driven) maintenance run once per event-loop tick or
//! whenever a background naming worker finishes: layout animation, editor
//! autosave, and applying finished `crate::naming_workers` results.

use std::time::Instant;

use crate::app::App;
use crate::naming_workers::{NamingWorkerEvent, NamingWorkers, TitleTrigger};
use crate::search_workers::SearchWorkers;

/// Runs every poll tick, regardless of whether any input/`ServerEvent`
/// fired this iteration. Returns whether anything it did actually changed
/// visible state, so `crate::run`'s event loop knows whether this
/// otherwise-silent tick still needs to force a redraw (a "Working"
/// spinner, a "Done" pulse, a recently-created flash, and the tree-width
/// hover animation are all wall-clock-driven and keep animating with no
/// new event at all -- see `App::has_active_animation`).
pub fn on_tick(app: &mut App, now: Instant, search_workers: &mut SearchWorkers) -> bool {
    // Read *before* advancing the animation so the tick that finishes an
    // in-progress transition still reports "was animating" and forces its
    // own final redraw -- `tick_layout_animation` and the animation
    // spinners/pulses below share this same "still active as of the start
    // of this tick" contract.
    let was_animating = app.has_active_animation();
    app.tick_layout_animation(now);
    let tree_transition_changed = app.tick_tree_transitions(now);
    let autosave_wrote = app.tick_autosave();
    let workspace_search_started = app.tick_workspace_search(now, search_workers);
    was_animating || tree_transition_changed || autosave_wrote || workspace_search_started
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
        NamingWorkerEvent::SessionTitle(pane_id, session_id, title_generation, result, trigger) => {
            workers.session_title_worker_finished(pane_id, &session_id);
            if app.agent_session_ids.get(&pane_id) == Some(&session_id)
                && app
                    .agent_title_generations
                    .get(&pane_id)
                    .copied()
                    .unwrap_or(0)
                    == title_generation
            {
                app.titles_loading.remove(&pane_id);
            } else {
                // Both automatic and user-requested workers read one exact
                // transcript. A `/resume` while either request is in flight
                // makes its result stale; manual changes overwrite semantics,
                // never the provenance requirement.
                return;
            }
            match result {
                Ok(title) => match trigger {
                    TitleTrigger::Automatic => {
                        app.inferred_title_session_ids
                            .insert(pane_id, session_id.clone());
                        app.request_session_pane_title(
                            pane_id,
                            session_id,
                            title_generation,
                            title.long,
                            Some(title.short),
                            ilium_core::PaneTitleSource::Automatic,
                        );
                    }
                    TitleTrigger::Manual => {
                        app.request_session_pane_title(
                            pane_id,
                            session_id,
                            title_generation,
                            title.long,
                            Some(title.short),
                            ilium_core::PaneTitleSource::UserSpecified,
                        );
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
        NamingWorkerEvent::InferenceTest {
            provider,
            elapsed,
            result,
        } => {
            workers.inference_test_worker_finished();
            app.finish_inference_test(provider, elapsed, result);
        }
        NamingWorkerEvent::OllamaModels {
            endpoint,
            elapsed,
            result,
        } => {
            workers.ollama_models_worker_finished();
            app.finish_ollama_model_discovery(
                endpoint,
                elapsed,
                result.map_err(|error| error.to_string()),
            );
        }
        NamingWorkerEvent::Restructure(project_id, result) => {
            workers.restructure_worker_finished(project_id);
            app.finish_project_restructure(project_id, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use ilium_core::NodeId;

    use super::*;
    use crate::naming::DualTitle;

    #[test]
    fn stale_manual_session_title_is_discarded_after_session_change() {
        let pane_id = NodeId(7);
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.agent_session_ids
            .insert(pane_id, "new-session".to_string());
        app.titles_loading.insert(pane_id);
        let (events_tx, _events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers =
            NamingWorkers::new(events_tx, ilium_inference::InferenceSettings::default());

        apply_naming_worker_event(
            &mut app,
            &mut workers,
            NamingWorkerEvent::SessionTitle(
                pane_id,
                "old-session".to_string(),
                0,
                Ok(DualTitle {
                    short: "Old Session".to_string(),
                    long: "Title From The Previous Agent Session".to_string(),
                }),
                TitleTrigger::Manual,
            ),
        );

        assert!(app.take_outbound_requests().is_empty());
        assert!(
            app.titles_loading.contains(&pane_id),
            "an old worker must not clear the new session's loading guard"
        );
    }
}
