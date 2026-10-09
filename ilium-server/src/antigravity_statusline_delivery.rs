use std::sync::Arc;
use std::time::Duration;

use ilium_core::{AgentClass, AgentProcessKey, NodeId};
use ilium_ipc::AntigravityStatuslineAction;
use ilium_pty::OwnerStatus;

use crate::ipc::handlers::{self, AgentInputInvocation, InputWriteOrigin};
use crate::ipc::DirectEventSender;
use crate::pane::PaneResource;
use crate::state::ServerState;

const READINESS_RECHECK_INTERVAL: Duration = Duration::from_millis(100);
const MAXIMUM_STATUSLINE_COMMAND_BYTES: usize = 4096;

fn statusline_acknowledgement_advanced(
    action: &AntigravityStatuslineAction,
    before: &str,
    after: &str,
) -> bool {
    let acknowledgement = match action {
        AntigravityStatuslineAction::SetCommand { command } => {
            format!("⎿ Statusline set to:\n  {command}")
        }
        AntigravityStatuslineAction::DeleteCommand => {
            "⎿ Custom statusline command cleared. Reverted to built-in default.".to_owned()
        }
        AntigravityStatuslineAction::CancelPending => return false,
        AntigravityStatuslineAction::DisableCommand => "⎿ Statusline off.".to_owned(),
    };
    after.matches(&acknowledgement).count() > before.matches(&acknowledgement).count()
}

pub(crate) fn statusline_command_input(
    action: &AntigravityStatuslineAction,
) -> Result<Option<Vec<u8>>, String> {
    let command = match action {
        AntigravityStatuslineAction::SetCommand { command } => {
            if command.is_empty() || command.len() > MAXIMUM_STATUSLINE_COMMAND_BYTES {
                return Err("Antigravity status-line command is empty or too large".to_owned());
            }
            if command.chars().any(char::is_control) {
                return Err(
                    "Antigravity status-line command contains control characters".to_owned(),
                );
            }
            format!("/statusline {command}")
        }
        AntigravityStatuslineAction::DeleteCommand => "/statusline delete".to_owned(),
        AntigravityStatuslineAction::CancelPending => return Ok(None),
        AntigravityStatuslineAction::DisableCommand => "/statusline off".to_owned(),
    };
    let mut bytes = command.into_bytes();
    bytes.push(b'\r');
    Ok(Some(bytes))
}

pub(crate) async fn update_running_panes(
    state: &Arc<ServerState>,
    generation: u64,
    action: AntigravityStatuslineAction,
    direct_tx: &DirectEventSender,
) {
    let command = match statusline_command_input(&action) {
        Ok(command) => command,
        Err(error) => {
            tracing::warn!(generation, %error, "rejected Antigravity status-line action");
            handlers::send_antigravity_statusline_completed(direct_tx, generation, Err(error))
                .await;
            return;
        }
    };
    let mut pending = Vec::new();
    let rejected_generation;
    {
        let mut panes = state.panes.write().await;
        let antigravity_panes = panes
            .iter()
            .filter_map(|(pane_id, resource)| match resource {
                PaneResource::Terminal(runtime)
                    if runtime
                        .agent_process_key
                        .as_ref()
                        .is_some_and(|process| process.class == AgentClass::Antigravity) =>
                {
                    Some(*pane_id)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        rejected_generation = antigravity_panes.iter().any(|pane_id| {
            matches!(panes.get(pane_id), Some(PaneResource::Terminal(runtime))
                if generation <= runtime.antigravity_statusline_generation)
        });
        if !rejected_generation {
            for pane_id in antigravity_panes {
                let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                    continue;
                };
                runtime.cancel_antigravity_statusline_delivery(generation);
                if let (Some(process), true) =
                    (runtime.agent_process_key.clone(), command.is_some())
                {
                    pending.push((
                        pane_id,
                        runtime.session.input_handle(),
                        runtime.agent_generation,
                        process,
                    ));
                }
            }
        }
    }
    if rejected_generation {
        handlers::send_antigravity_statusline_completed(
            direct_tx,
            generation,
            Err("Antigravity status-line generation is stale for a live pane".to_owned()),
        )
        .await;
        return;
    }
    let Some(command) = command else {
        handlers::send_antigravity_statusline_completed(direct_tx, generation, Ok(())).await;
        return;
    };
    let expected_completions = pending.len();
    let (completion_tx, mut completion_rx) =
        tokio::sync::mpsc::channel::<Result<(), String>>(expected_completions.max(1));
    for (pane_id, input, agent_generation, process) in pending {
        let state = Arc::clone(state);
        let command = command.clone();
        let action = action.clone();
        let completion_tx = completion_tx.clone();
        let task = tokio::spawn(async move {
            let result = deliver_when_ready(
                &state,
                pane_id,
                generation,
                agent_generation,
                process,
                input,
                command,
                action,
            )
            .await;
            if let Err(error) = &result {
                tracing::debug!(pane_id = pane_id.0, generation, %error, "Antigravity status-line command was not delivered");
            }
            let _ = completion_tx.send(result).await;
        });
        let mut panes = state.panes.write().await;
        if let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) {
            if runtime.antigravity_statusline_generation == generation {
                runtime.set_antigravity_statusline_delivery(task);
            } else {
                task.abort();
            }
        } else {
            task.abort();
        }
    }
    drop(completion_tx);
    if expected_completions == 0 {
        handlers::send_antigravity_statusline_completed(direct_tx, generation, Ok(())).await;
        return;
    }
    let direct_tx = direct_tx.clone();
    tokio::spawn(async move {
        let mut failure = None;
        for _ in 0..expected_completions {
            match completion_rx.recv().await {
                Some(Ok(())) => {}
                Some(Err(error)) => {
                    failure.get_or_insert(error);
                }
                None => return,
            }
        }
        handlers::send_antigravity_statusline_completed(
            &direct_tx,
            generation,
            failure.map_or(Ok(()), Err),
        )
        .await;
    });
}

async fn deliver_when_ready(
    state: &ServerState,
    pane_id: NodeId,
    statusline_generation: u64,
    agent_generation: u64,
    process: AgentProcessKey,
    input: ilium_pty::PtyInput,
    command: Vec<u8>,
    action: AntigravityStatuslineAction,
) -> Result<(), String> {
    let mut screen_changed = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err("Antigravity pane closed before status-line delivery".to_owned());
        };
        if !input.same_session(&runtime.session.input_handle()) {
            return Err("Antigravity pane session changed before status-line delivery".to_owned());
        }
        runtime.session.subscribe_screen_changed()
    };

    loop {
        let input_gate = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                return Err("Antigravity pane closed before status-line delivery".to_owned());
            };
            if !delivery_is_current(
                runtime,
                statusline_generation,
                agent_generation,
                &process,
                &input,
            ) {
                return Err(
                    "Antigravity pane ownership changed before status-line delivery".to_owned(),
                );
            }
            crate::agent_delivery::runtime_has_ready_composer(runtime)
                .then(|| Arc::clone(&runtime.input_gate))
        };
        if let Some(input_gate) = input_gate {
            let _input_guard = input_gate.lock().await;
            let ready = {
                let panes = state.panes.read().await;
                let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                    return Err("Antigravity pane closed before status-line delivery".to_owned());
                };
                Arc::ptr_eq(&input_gate, &runtime.input_gate)
                    && delivery_is_current(
                        runtime,
                        statusline_generation,
                        agent_generation,
                        &process,
                        &input,
                    )
                    && crate::agent_delivery::runtime_has_ready_composer(runtime)
            };
            if ready {
                let before_screen = {
                    let panes = state.panes.read().await;
                    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                        return Err(
                            "Antigravity pane closed before status-line delivery".to_owned()
                        );
                    };
                    if !delivery_is_current(
                        runtime,
                        statusline_generation,
                        agent_generation,
                        &process,
                        &input,
                    ) {
                        return Err(
                            "Antigravity pane ownership changed before status-line delivery"
                                .to_owned(),
                        );
                    }
                    runtime.session.screen_text()
                };
                let invocation = {
                    let panes = state.panes.read().await;
                    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                        return Err(
                            "Antigravity pane closed before status-line delivery".to_owned()
                        );
                    };
                    AgentInputInvocation {
                        generation: runtime.agent_generation,
                        process: runtime.agent_process_key.clone(),
                        input_cancel_generation: *runtime.agent_input_cancel.borrow(),
                    }
                };
                handlers::write_key_input_unlocked_with_screen_marker(
                    state,
                    pane_id,
                    &command,
                    None,
                    InputWriteOrigin {
                        is_initial_prompt: false,
                        is_user_directed: false,
                        prompt_epoch: None,
                        expected_invocation: Some(&invocation),
                        required_ready_agent_class: Some(AgentClass::Antigravity),
                        required_statusline_generation: Some(statusline_generation),
                    },
                    &input_gate,
                    &mut screen_changed,
                )
                .await?;
                loop {
                    let screen_changed = tokio::select! {
                        changed = screen_changed.changed() => {
                            if changed.is_err() {
                                return Err("Antigravity pane closed before status-line processing was visible".to_owned());
                            }
                            true
                        }
                        () = tokio::time::sleep(READINESS_RECHECK_INTERVAL) => false,
                    };
                    let (still_current, processed) = {
                        let panes = state.panes.read().await;
                        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                            return Err(
                                "Antigravity pane closed after status-line delivery".to_owned()
                            );
                        };
                        let still_current = delivery_is_current(
                            runtime,
                            statusline_generation,
                            agent_generation,
                            &process,
                            &input,
                        );
                        let after_screen = runtime.session.screen_text();
                        (
                            still_current,
                            still_current
                                && screen_changed
                                && crate::agent_delivery::runtime_has_ready_composer(runtime)
                                && statusline_acknowledgement_advanced(
                                    &action,
                                    &before_screen,
                                    &after_screen,
                                ),
                        )
                    };
                    if !still_current {
                        return Err(
                            "Antigravity pane ownership changed after status-line delivery"
                                .to_owned(),
                        );
                    }
                    if processed {
                        return Ok(());
                    }
                }
            }
        }
        if !matches!(input.status(), OwnerStatus::Running) {
            return Err("Antigravity PTY stopped before status-line delivery".to_owned());
        }
        tokio::select! {
            changed = screen_changed.changed() => {
                if changed.is_err() {
                    return Err("Antigravity pane closed before status-line delivery".to_owned());
                }
            }
            () = tokio::time::sleep(READINESS_RECHECK_INTERVAL) => {}
        }
    }
}

fn delivery_is_current(
    runtime: &crate::pane::TerminalPaneRuntime,
    statusline_generation: u64,
    agent_generation: u64,
    process: &AgentProcessKey,
    input: &ilium_pty::PtyInput,
) -> bool {
    runtime.antigravity_statusline_generation == statusline_generation
        && runtime.agent_generation == agent_generation
        && runtime.agent_process_key.as_ref() == Some(process)
        && input.same_session(&runtime.session.input_handle())
        && matches!(input.status(), OwnerStatus::Running)
}

#[cfg(test)]
mod tests {
    use super::{statusline_acknowledgement_advanced, statusline_command_input};
    use ilium_ipc::AntigravityStatuslineAction;

    #[test]
    fn statusline_command_is_one_carriage_return_terminated_frame() {
        assert_eq!(
            statusline_command_input(&AntigravityStatuslineAction::SetCommand {
                command: "/usr/bin/ilium __antigravity-model-statusline --format 'a b'".into(),
            })
            .unwrap(),
            Some(
                b"/statusline /usr/bin/ilium __antigravity-model-statusline --format 'a b'\r"
                    .to_vec()
            ),
        );
        assert_eq!(
            statusline_command_input(&AntigravityStatuslineAction::DeleteCommand).unwrap(),
            Some(b"/statusline delete\r".to_vec()),
        );
        assert_eq!(
            statusline_command_input(&AntigravityStatuslineAction::CancelPending).unwrap(),
            None,
        );
        assert_eq!(
            statusline_command_input(&AntigravityStatuslineAction::DisableCommand).unwrap(),
            Some(b"/statusline off\r".to_vec()),
        );
    }

    #[test]
    fn only_a_new_action_specific_antigravity_reply_confirms_delivery() {
        let command = "/usr/bin/ilium __antigravity-model-statusline";
        let set = AntigravityStatuslineAction::SetCommand {
            command: command.into(),
        };
        let prior_set_reply =
            format!("> /statusline {command}\n⎿ Statusline set to:\n  {command}\n");
        assert!(statusline_acknowledgement_advanced(
            &set,
            "",
            &prior_set_reply
        ));
        assert!(!statusline_acknowledgement_advanced(
            &set,
            &prior_set_reply,
            &format!("{prior_set_reply} unrelated terminal output\n"),
        ));
        assert!(!statusline_acknowledgement_advanced(
            &set,
            "",
            &format!("> /statusline {command}\ncomposer ready\n"),
        ));
        assert!(statusline_acknowledgement_advanced(
            &set,
            &prior_set_reply,
            &format!(
                "{prior_set_reply}> /statusline {command}\n⎿ Statusline set to:\n  {command}\n"
            ),
        ));

        for (action, reply, unrelated_reply) in [
            (
                AntigravityStatuslineAction::DisableCommand,
                "⎿ Statusline off.",
                "⎿ Custom statusline command cleared. Reverted to built-in default.",
            ),
            (
                AntigravityStatuslineAction::DeleteCommand,
                "⎿ Custom statusline command cleared. Reverted to built-in default.",
                "⎿ Statusline off.",
            ),
        ] {
            let previous = format!("> /statusline off\n{reply}\n");
            assert!(statusline_acknowledgement_advanced(&action, "", &previous));
            assert!(!statusline_acknowledgement_advanced(
                &action,
                &previous,
                &format!("{previous}unrelated output\n"),
            ));
            assert!(!statusline_acknowledgement_advanced(
                &action,
                "",
                &format!("> /statusline off\n{unrelated_reply}\n"),
            ));
            assert!(statusline_acknowledgement_advanced(
                &action,
                &previous,
                &format!("> /statusline off\n{previous}{reply}\n"),
            ));
        }
    }

    #[test]
    fn statusline_command_rejects_control_characters_and_oversized_commands() {
        for command in [
            "custom-statusline\nsecond-command",
            "custom-statusline\rsecond-command",
            "custom-statusline\u{1b}[31m",
        ] {
            assert!(
                statusline_command_input(&AntigravityStatuslineAction::SetCommand {
                    command: command.into()
                })
                .is_err()
            );
        }
        assert!(
            statusline_command_input(&AntigravityStatuslineAction::SetCommand {
                command: "x".repeat(4097)
            })
            .is_err()
        );
    }
}
