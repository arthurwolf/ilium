//! Periodic (non-input-driven) maintenance run once per event-loop tick or
//! whenever a background naming worker finishes: layout animation, editor
//! autosave, and applying finished `crate::naming_workers` results.

use std::time::Instant;

use ilium_core::NodeId;

use crate::app::App;
use crate::naming_workers::{NamingWorkerEvent, NamingWorkers, TitleTrigger};
use crate::search_workers::SearchWorkers;

/// Runs every poll tick, regardless of whether any input/`ServerEvent`
/// fired this iteration. Returns whether anything it did actually changed
/// visible state, so `crate::run`'s event loop knows whether this
/// otherwise-silent tick still needs to force a redraw (a "Working"
/// spinner, a "Done" pulse, a recently-created flash, and the tree-width
/// hover animation are all wall-clock-driven and keep animating with no
/// new event at all. `was_animating` reuses the schedule observation made
/// before the timer sleep, preserving the final-frame contract without a
/// second pane traversal.
pub fn on_tick(
    app: &mut App,
    now: Instant,
    was_animating: bool,
    search_workers: &mut SearchWorkers,
) -> bool {
    // Read *before* advancing the animation so the tick that finishes an
    // in-progress transition still reports "was animating" and forces its
    // own final redraw -- `tick_layout_animation` and the animation
    // spinners/pulses below share this same "still active as of the start
    // of this tick" contract.
    app.tick_layout_animation(now);
    let tree_transition_changed = app.tick_tree_transitions(now);
    let terminal_activity_changed = app.tick_terminal_activity(now);
    let autosave_wrote = app.tick_autosave();
    let workspace_search_started = app.tick_workspace_search(now, search_workers);
    let chatroom_changed = app.tick_chatroom_projects(now);
    let session_stats_changed = app.tick_session_stats(now);
    let cost_changed = app.tick_cost(now);
    let context_menu_hover_changed = app.tick_context_menu_hover(now);
    let agent_popover_changed = app.tick_agent_popover(now);
    let animation_hover_changed = app.tick_animation_hover(now);
    let location_picker_changed = app.tick_location_picker();
    app.drain_pending_staged_keystrokes(now);
    let setup_prompt_was_open = matches!(app.mode, crate::app::Mode::AgentSetupPrompt(_));
    app.maybe_show_agent_setup_prompt();
    let setup_prompt_changed =
        !setup_prompt_was_open && matches!(app.mode, crate::app::Mode::AgentSetupPrompt(_));
    was_animating
        || tree_transition_changed
        || terminal_activity_changed
        || autosave_wrote
        || workspace_search_started
        || chatroom_changed
        || session_stats_changed
        || cost_changed
        || context_menu_hover_changed
        || agent_popover_changed
        || animation_hover_changed
        || location_picker_changed
        || setup_prompt_changed
}

/// Re-fires a manual retitle that landed while `titles_loading` already
/// held `pane_id`, now that whatever previously held it is done -- see
/// `App::pending_manual_retitles`'s doc comment for why the click can't
/// simply cancel the earlier worker instead. A no-op if no manual click is
/// waiting. Queuing through `App::action_request_retitle` (rather than
/// pushing a `PendingRetitleRequest` directly) re-validates the pane's
/// current kind/session/title-source, since both may have changed while
/// the superseded worker was running.
fn refire_pending_manual_retitle(app: &mut App, pane_id: NodeId) {
    if app.pending_manual_retitles.remove(&pane_id) {
        app.action_request_retitle(pane_id);
    }
}

/// Applies one finished background naming result to `app`, and tells
/// `workers` it's no longer in flight.
pub fn apply_naming_worker_event(
    app: &mut App,
    workers: &mut NamingWorkers,
    event: NamingWorkerEvent,
) {
    match event {
        NamingWorkerEvent::ProjectName { decision, result } => {
            workers.project_name_worker_finished();
            app.is_project_name_loading = false;
            if app.onboarding.is_some()
                || !app.onboarding_progress.automatic_ai_allowed()
                || !workers.is_current_automatic_ai_decision(decision)
            {
                tracing::debug!("discarding project name after AI decision changed");
                return;
            }
            // The worker only computed a proposal. This synchronous commit is
            // serialized with the event loop's onboarding decision changes.
            match result.and_then(|proposal| {
                crate::project_naming::persist_inferred_project_name(&app.session_cwd, proposal)
            }) {
                Ok(bootstrap) => {
                    tracing::info!(
                        project_name = %bootstrap.project_name,
                        project_icon = ?bootstrap.icon,
                        "project naming completed"
                    );
                    app.project_name = Some(bootstrap.project_name);
                    app.project_icon = bootstrap.icon;
                }
                Err(err) => {
                    tracing::error!(
                        error_characters = err.to_string().chars().count(),
                        "project naming failed"
                    );
                    tracing::debug!(error = %err, error_debug = ?err, "project naming failure details");
                    app.status_message = Some(format!("Could not infer project name: {err}"))
                }
            }
        }
        NamingWorkerEvent::SessionTitle(outcome) => {
            let crate::naming_workers::SessionTitleWorkerResult {
                pane_id,
                session_id,
                title_generation,
                provider,
                elapsed,
                rendered_prompt,
                raw_response,
                result,
                trigger,
                automatic_ai_decision,
            } = outcome;
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
                refire_pending_manual_retitle(app, pane_id);
            } else {
                // Both automatic and user-requested workers read one exact
                // transcript. A `/resume` while either request is in flight
                // makes its result stale; manual changes overwrite semantics,
                // never the provenance requirement.
                let mut fields = vec![
                    ilium_ipc::AgentDebugField::plain("provider", provider.label()),
                    ilium_ipc::AgentDebugField::plain(
                        "elapsed milliseconds",
                        elapsed.as_millis().to_string(),
                    ),
                    ilium_ipc::AgentDebugField::sensitive("expected session", session_id),
                    ilium_ipc::AgentDebugField::plain(
                        "expected title generation",
                        title_generation.to_string(),
                    ),
                ];
                append_title_inference_trace_fields(&mut fields, rendered_prompt, raw_response);
                app.record_agent_debug_event(
                    pane_id,
                    ilium_ipc::AgentDebugEventDraft {
                        severity: ilium_ipc::AgentDebugSeverity::Warning,
                        kind: ilium_ipc::AgentDebugEventKind::TitleInferenceDiscarded,
                        summary: "Completed title inference was stale".to_string(),
                        fields,
                        correlation_id: None,
                        metadata: Default::default(),
                    },
                );
                // Stale doesn't necessarily mean another worker still owns
                // `titles_loading` -- e.g. a `/resume` clears it outright in
                // `render_cache::apply` with nothing new queued. Re-check
                // here too, or a manual click that arrived just before that
                // clear would wait forever for a completion event that,
                // for this pane_id, never lands again.
                refire_pending_manual_retitle(app, pane_id);
                return;
            }
            if app.onboarding.is_some()
                || !app.onboarding_progress.automatic_ai_allowed()
                || !workers.is_current_automatic_ai_decision(automatic_ai_decision)
            {
                return;
            }
            match result {
                Ok(title) => {
                    tracing::info!(
                        ?pane_id,
                        %session_id,
                        ?trigger,
                        icon = %title.icon,
                        short_title = %title.short,
                        long_title = %title.long,
                        "session title inference completed"
                    );
                    let mut fields = vec![
                        ilium_ipc::AgentDebugField::plain("provider", provider.label()),
                        ilium_ipc::AgentDebugField::plain(
                            "elapsed milliseconds",
                            elapsed.as_millis().to_string(),
                        ),
                        ilium_ipc::AgentDebugField::plain("trigger", format!("{trigger:?}")),
                        ilium_ipc::AgentDebugField::plain("icon", title.icon.clone()),
                        ilium_ipc::AgentDebugField::plain("short title", title.short.clone()),
                        ilium_ipc::AgentDebugField::plain("long title", title.long.clone()),
                    ];
                    append_title_inference_trace_fields(&mut fields, rendered_prompt, raw_response);
                    app.record_agent_debug_event(
                        pane_id,
                        ilium_ipc::AgentDebugEventDraft {
                            severity: ilium_ipc::AgentDebugSeverity::Success,
                            kind: ilium_ipc::AgentDebugEventKind::TitleInferenceSucceeded,
                            summary: "LLM returned a valid agent title".to_string(),
                            fields,
                            correlation_id: None,
                            metadata: Default::default(),
                        },
                    );
                    match trigger {
                        TitleTrigger::Automatic => {
                            app.inferred_title_session_ids
                                .insert(pane_id, session_id.clone());
                            app.request_session_pane_title(crate::app::SessionPaneTitleRequest {
                                pane_id,
                                expected_session_id: session_id,
                                expected_title_generation: title_generation,
                                title: title.long,
                                short_title: Some(title.short),
                                inferred_icon: Some(title.icon),
                                title_source: ilium_core::PaneTitleSource::Automatic,
                            });
                        }
                        TitleTrigger::Manual => {
                            app.request_session_pane_title(crate::app::SessionPaneTitleRequest {
                                pane_id,
                                expected_session_id: session_id,
                                expected_title_generation: title_generation,
                                title: title.long,
                                short_title: Some(title.short),
                                inferred_icon: Some(title.icon),
                                title_source: ilium_core::PaneTitleSource::UserSpecified,
                            });
                        }
                    }
                }
                Err(err) => {
                    // Deliberately no permanent failure marker here (unlike
                    // the pre-client/server bin crate's `titles_inference_failed`):
                    // `title_inference::MAX_ATTEMPTS` already bounds the
                    // retries `title_inference::pane_ready_for_inference`'s
                    // `PaneBecameDone` trigger drives, so a merely-unlucky
                    // attempt (e.g. the transcript had nothing to
                    // summarize yet) gets a few more chances instead of
                    // silently never being retried for the rest of the run.
                    tracing::error!(
                        ?pane_id,
                        %session_id,
                        ?trigger,
                        error_characters = err.to_string().chars().count(),
                        "session title inference failed"
                    );
                    tracing::debug!(?pane_id, %session_id, ?trigger, error = %err, error_debug = ?err, "session title inference failure details");
                    let mut fields = vec![
                        ilium_ipc::AgentDebugField::plain("provider", provider.label()),
                        ilium_ipc::AgentDebugField::plain(
                            "elapsed milliseconds",
                            elapsed.as_millis().to_string(),
                        ),
                        ilium_ipc::AgentDebugField::plain("trigger", format!("{trigger:?}")),
                        ilium_ipc::AgentDebugField::sensitive("error", err.to_string()),
                    ];
                    append_title_inference_trace_fields(&mut fields, rendered_prompt, raw_response);
                    app.record_agent_debug_event(
                        pane_id,
                        ilium_ipc::AgentDebugEventDraft {
                            severity: ilium_ipc::AgentDebugSeverity::Error,
                            kind: ilium_ipc::AgentDebugEventKind::TitleInferenceFailed,
                            summary: "Agent title inference failed".to_string(),
                            fields,
                            correlation_id: None,
                            metadata: Default::default(),
                        },
                    );
                    app.status_message = Some(format!("Could not infer session title: {err}"));
                }
            }
        }
        NamingWorkerEvent::TerminalTitle(pane_id, result, trigger, automatic_ai_decision) => {
            workers.terminal_title_worker_finished(pane_id);
            app.titles_loading.remove(&pane_id);
            refire_pending_manual_retitle(app, pane_id);
            if app.onboarding.is_some()
                || !app.onboarding_progress.automatic_ai_allowed()
                || !workers.is_current_automatic_ai_decision(automatic_ai_decision)
            {
                return;
            }
            match result {
                Ok(title) => {
                    tracing::info!(
                        ?pane_id,
                        ?trigger,
                        icon = %title.icon,
                        short_title = %title.short,
                        long_title = %title.long,
                        "terminal title inference completed"
                    );
                    match trigger {
                        TitleTrigger::Automatic => {
                            app.request_automatic_pane_title(
                                pane_id,
                                title.long,
                                Some(title.short),
                                Some(title.icon),
                            );
                        }
                        TitleTrigger::Manual => {
                            app.request_rename(
                                pane_id,
                                title.long,
                                Some(title.short),
                                Some(title.icon),
                            );
                        }
                    }
                }
                Err(err) => {
                    tracing::error!(
                        ?pane_id,
                        ?trigger,
                        error_characters = err.to_string().chars().count(),
                        "terminal title inference failed"
                    );
                    tracing::debug!(?pane_id, ?trigger, error = %err, error_debug = ?err, "terminal title inference failure details");
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
            match &result {
                Ok(_) => tracing::info!(?provider, ?elapsed, "inference test completed"),
                Err(error) => tracing::error!(
                    ?provider,
                    ?elapsed,
                    error_characters = error.to_string().chars().count(),
                    "inference test failed"
                ),
            }
            if let Err(error) = &result {
                tracing::debug!(?provider, ?elapsed, error = %error, error_debug = ?error, "inference test failure details");
            }
            app.finish_inference_test(provider, elapsed, result);
        }
        NamingWorkerEvent::ProviderModels {
            provider,
            endpoint,
            elapsed,
            result,
        } => {
            workers.model_discovery_worker_finished();
            match &result {
                Ok(models) => tracing::info!(
                    ?provider,
                    %endpoint,
                    ?elapsed,
                    model_count = models.len(),
                    "inference model discovery completed"
                ),
                Err(error) => tracing::error!(
                    ?provider,
                    %endpoint,
                    ?elapsed,
                    error_characters = error.to_string().chars().count(),
                    "inference model discovery failed"
                ),
            }
            if let Err(error) = &result {
                tracing::debug!(?provider, %endpoint, ?elapsed, error = %error, error_debug = ?error, "inference model discovery failure details");
            }
            app.finish_model_discovery(
                provider,
                endpoint,
                elapsed,
                result.map_err(|error| error.to_string()),
            );
        }
        NamingWorkerEvent::Restructure(outcome) => {
            let crate::naming_workers::RestructureWorkerResult {
                project_id,
                inference_activity_revisions,
                automatic_ai_decision,
                result,
            } = outcome;
            workers.restructure_worker_finished(project_id);
            if app.onboarding.is_some()
                || !app.onboarding_progress.automatic_ai_allowed()
                || !workers.is_current_automatic_ai_decision(automatic_ai_decision)
            {
                app.cancel_project_restructure_after_ai_decision(project_id);
                return;
            }
            match &result {
                Ok(_) => tracing::info!(?project_id, "project restructure inference completed"),
                Err(error) => tracing::error!(
                    ?project_id,
                    error_characters = error.to_string().chars().count(),
                    "project restructure inference failed"
                ),
            }
            if let Err(error) = &result {
                tracing::debug!(?project_id, error = %error, error_debug = ?error, "project restructure inference failure details");
            }
            app.finish_project_restructure(project_id, &inference_activity_revisions, result);
        }
        NamingWorkerEvent::LastPromptTranscript(result) => {
            let crate::naming_workers::LastPromptTranscriptWorkerResult {
                pane_id,
                session_id,
                last_prompt,
            } = result;
            // A `None` result (no transcript yet, or none of its entries
            // were a user message) leaves whatever live keystroke tracking
            // already recorded untouched -- this check only ever upgrades,
            // never clears, the banner. Matching what's already in the
            // local tree mirror avoids sending an IPC request for every
            // Enter press when the transcript just confirms what live
            // tracking already got right.
            if let Some(last_prompt) = last_prompt {
                if !last_prompt.is_empty()
                    && app.tree.last_prompt(pane_id) != Some(last_prompt.as_str())
                {
                    app.queue_request(ilium_ipc::ClientRequest::ReportLastPromptFromTranscript {
                        pane_id,
                        expected_session_id: session_id,
                        last_prompt,
                    });
                }
            }
        }
    }
}

/// Adds exact provider-boundary payloads only when that boundary was reached.
/// Both values use the sensitive presentation because prompts can contain
/// transcript/project text and responses can echo it.
fn append_title_inference_trace_fields(
    fields: &mut Vec<ilium_ipc::AgentDebugField>,
    rendered_prompt: Option<String>,
    raw_response: Option<String>,
) {
    if let Some(rendered_prompt) = rendered_prompt {
        fields.push(ilium_ipc::AgentDebugField::sensitive(
            "rendered LLM request",
            rendered_prompt,
        ));
    }
    if let Some(raw_response) = raw_response {
        fields.push(ilium_ipc::AgentDebugField::sensitive(
            "raw LLM response",
            raw_response,
        ));
    }
}

#[cfg(test)]
mod tests {
    use ilium_core::{AgentActivity, AgentClass, NodeId, PaneContentKind, PaneStatus, ROOT_ID};

    use super::*;
    use crate::app::PendingRetitleRequest;
    use crate::naming::DualTitle;

    #[test]
    fn project_name_result_after_ai_opt_out_does_not_write_or_publish() {
        let cwd = tempfile::tempdir().unwrap();
        let mut app = App::new("test".to_string(), cwd.path().to_path_buf());
        app.is_project_name_loading = true;
        let (events_tx, _events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers =
            NamingWorkers::new(events_tx, ilium_inference::InferenceSettings::default());
        let old_decision = crate::naming_workers::AutomaticAiDecision::new(0, true);
        app.onboarding_progress.begin();
        app.onboarding_progress
            .choose_ai(crate::onboarding::state::AiChoice::Disabled);
        app.onboarding_progress.finish();
        app.onboarding_revision = 1;
        workers.set_automatic_ai_decision(1, false);

        apply_naming_worker_event(
            &mut app,
            &mut workers,
            NamingWorkerEvent::ProjectName {
                decision: old_decision,
                result: Ok(crate::project_naming::ProjectNameBootstrap {
                    project_name: "Unwanted Name".to_string(),
                    icon: Some("✦".to_string()),
                    source: crate::project_naming::ProjectNameSource::Inferred,
                }),
            },
        );

        assert!(!app.is_project_name_loading);
        assert_eq!(app.project_name, None);
        assert_eq!(app.project_icon, None);
        assert!(!cwd.path().join(".ilium/config.yaml").exists());
    }

    #[test]
    fn reenabled_ai_still_rejects_an_old_project_name_result() {
        let cwd = tempfile::tempdir().unwrap();
        let mut app = App::new("test".to_string(), cwd.path().to_path_buf());
        let (events_tx, _events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers =
            NamingWorkers::new(events_tx, ilium_inference::InferenceSettings::default());
        let old_decision = crate::naming_workers::AutomaticAiDecision::new(0, true);
        workers.set_automatic_ai_decision(1, false);
        workers.set_automatic_ai_decision(2, true);
        app.onboarding_revision = 2;

        apply_naming_worker_event(
            &mut app,
            &mut workers,
            NamingWorkerEvent::ProjectName {
                decision: old_decision,
                result: Ok(crate::project_naming::ProjectNameBootstrap {
                    project_name: "Old Provider Name".to_string(),
                    icon: Some("✦".to_string()),
                    source: crate::project_naming::ProjectNameSource::Inferred,
                }),
            },
        );

        assert_eq!(app.project_name, None);
        assert!(!cwd.path().join(".ilium/config.yaml").exists());
    }

    #[test]
    fn accepted_project_name_result_commits_before_publication() {
        let cwd = tempfile::tempdir().unwrap();
        let mut app = App::new("test".to_string(), cwd.path().to_path_buf());
        app.is_project_name_loading = true;
        let (events_tx, _events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers =
            NamingWorkers::new(events_tx, ilium_inference::InferenceSettings::default());

        apply_naming_worker_event(
            &mut app,
            &mut workers,
            NamingWorkerEvent::ProjectName {
                decision: crate::naming_workers::AutomaticAiDecision::new(0, true),
                result: Ok(crate::project_naming::ProjectNameBootstrap {
                    project_name: "Accepted Name".to_string(),
                    icon: Some("✦".to_string()),
                    source: crate::project_naming::ProjectNameSource::Inferred,
                }),
            },
        );

        assert!(!app.is_project_name_loading);
        assert_eq!(app.project_name.as_deref(), Some("Accepted Name"));
        assert_eq!(app.project_icon.as_deref(), Some("✦"));
        let saved = crate::project_config::load(cwd.path()).unwrap();
        assert_eq!(saved.project_name.as_deref(), Some("Accepted Name"));
        assert_eq!(saved.project_icon.as_deref(), Some("✦"));
    }

    #[test]
    fn fresh_session_title_completion_refires_a_queued_manual_retitle() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None),
            )
            .unwrap();
        app.agent_session_ids
            .insert(pane_id, "session-1".to_string());
        app.agent_title_generations.insert(pane_id, 3);
        // Models the manual click landing while an earlier (automatic)
        // worker already owns this pane's `titles_loading` slot -- see
        // `App::pending_manual_retitles`.
        app.titles_loading.insert(pane_id);
        app.pending_manual_retitles.insert(pane_id);
        let (events_tx, _events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers =
            NamingWorkers::new(events_tx, ilium_inference::InferenceSettings::default());

        apply_naming_worker_event(
            &mut app,
            &mut workers,
            NamingWorkerEvent::SessionTitle(crate::naming_workers::SessionTitleWorkerResult {
                pane_id,
                session_id: "session-1".to_string(),
                title_generation: 3,
                provider: ilium_inference::InferenceProviderKind::KiloGateway,
                elapsed: std::time::Duration::from_millis(9),
                rendered_prompt: None,
                raw_response: None,
                result: Ok(DualTitle {
                    icon: "🛰️".to_string(),
                    short: "Superseded".to_string(),
                    long: "The Now-Superseded Automatic Title".to_string(),
                }),
                trigger: TitleTrigger::Automatic,
                automatic_ai_decision: crate::naming_workers::AutomaticAiDecision::new(0, true),
            }),
        );

        assert!(
            !app.pending_manual_retitles.contains(&pane_id),
            "the queued click must be drained once its wait is over"
        );
        assert!(
            app.titles_loading.contains(&pane_id),
            "the re-fired manual request is itself now in flight"
        );
        let pending = app.take_pending_retitle_requests();
        let [PendingRetitleRequest::Session { trigger, .. }] = pending.as_slice() else {
            panic!("the queued manual click must re-fire as a fresh session-title request");
        };
        assert_eq!(*trigger, TitleTrigger::Manual);
    }

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
            NamingWorkerEvent::SessionTitle(crate::naming_workers::SessionTitleWorkerResult {
                pane_id,
                session_id: "old-session".to_string(),
                title_generation: 0,
                provider: ilium_inference::InferenceProviderKind::KiloGateway,
                elapsed: std::time::Duration::from_millis(42),
                rendered_prompt: Some("rendered request".to_string()),
                raw_response: Some("raw response".to_string()),
                result: Ok(DualTitle {
                    icon: "📜".to_string(),
                    short: "Old Session".to_string(),
                    long: "Title From The Previous Agent Session".to_string(),
                }),
                trigger: TitleTrigger::Manual,
                automatic_ai_decision: crate::naming_workers::AutomaticAiDecision::new(0, true),
            }),
        );

        assert!(app.take_outbound_requests().is_empty());
        assert!(
            app.titles_loading.contains(&pane_id),
            "an old worker must not clear the new session's loading guard"
        );
    }

    #[test]
    fn successful_title_debug_event_keeps_exact_rendered_request_and_raw_response() {
        let pane_id = NodeId(9);
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.ui_settings.agent_debug_menu_enabled = true;
        app.agent_session_ids
            .insert(pane_id, "current-session".to_string());
        app.agent_title_generations.insert(pane_id, 2);
        app.titles_loading.insert(pane_id);
        let (events_tx, _events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers =
            NamingWorkers::new(events_tx, ilium_inference::InferenceSettings::default());

        apply_naming_worker_event(
            &mut app,
            &mut workers,
            NamingWorkerEvent::SessionTitle(crate::naming_workers::SessionTitleWorkerResult {
                pane_id,
                session_id: "current-session".to_string(),
                title_generation: 2,
                provider: ilium_inference::InferenceProviderKind::KiloGateway,
                elapsed: std::time::Duration::from_millis(17),
                rendered_prompt: Some("<agent-session>full request</agent-session>".to_string()),
                raw_response: Some("{\"session_title_short\":\"Debug Log\"}".to_string()),
                result: Ok(DualTitle {
                    icon: "🐞".to_string(),
                    short: "Debug Log".to_string(),
                    long: "Build Persisted Agent Debug Timeline".to_string(),
                }),
                trigger: TitleTrigger::Automatic,
                automatic_ai_decision: crate::naming_workers::AutomaticAiDecision::new(0, true),
            }),
        );

        let requests = app.take_outbound_requests();
        let debug_event = requests.iter().find_map(|request| match request {
            ilium_ipc::ClientRequest::RecordAgentDebugEvent { event, .. }
                if event.kind == ilium_ipc::AgentDebugEventKind::TitleInferenceSucceeded =>
            {
                Some(event)
            }
            _ => None,
        });
        let debug_event = debug_event.expect("successful result should queue diagnostic evidence");
        assert!(debug_event.fields.iter().any(|field| {
            field.label == "rendered LLM request"
                && field.value == "<agent-session>full request</agent-session>"
        }));
        assert!(debug_event.fields.iter().any(|field| {
            field.label == "raw LLM response"
                && field.value == "{\"session_title_short\":\"Debug Log\"}"
        }));
    }
}
