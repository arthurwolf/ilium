//! Synthetic Settings buffer fixtures; compile and run only on ni-vm.
use std::{error::Error, path::PathBuf};

use ilium_client::{
    app::{App, SettingsState, SettingsTab},
    ui,
};
use ratatui::{backend::TestBackend, layout::Rect, Terminal};
use serde_json::json;

fn main() {
    if let Err(error) = capture() {
        println!("{}", json!({"type": "error", "message": error.to_string()}));
        std::process::exit(1);
    }
}

fn capture() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let output = match arguments.next().as_deref() {
        Some("--output-dir") => PathBuf::from(arguments.next().ok_or("missing output directory")?),
        Some("--service-output") => service_output_directory()?,
        _ => {
            return Err(
                "usage: ui_chrome_capture --output-dir <new-directory> | --service-output".into(),
            )
        }
    };
    if arguments.next().is_some() || !output.is_absolute() {
        return Err("provide exactly one absolute output directory or --service-output".into());
    }
    // Refuse an existing directory so fixture export never overwrites user data.
    std::fs::create_dir(&output)?;
    let fixture = output.join("synthetic-workspace");
    std::fs::create_dir(&fixture)?;
    let mut count = 0;
    for (width, height) in [(120, 40), (80, 24), (60, 20), (40, 12)] {
        for (index, tab) in SettingsTab::ALL.into_iter().enumerate() {
            for scroll in [0, 20] {
                let mut app = App::new("synthetic-ui-capture".to_owned(), fixture.clone());
                app.config_dir = Some(fixture.clone());
                app.set_screen_area(Rect::new(0, 0, width, height));
                if tab == SettingsTab::Inference {
                    app.inference_settings.selected_provider =
                        ilium_inference::InferenceProviderKind::KiloGateway;
                }
                app.mode = ilium_client::app::Mode::Settings(SettingsState {
                    tab,
                    scroll,
                    ..SettingsState::default()
                });
                let mut terminal = Terminal::new(TestBackend::new(width, height))?;
                terminal.draw(|frame| ui::draw(frame, &mut app))?;
                let buffer = terminal.backend().buffer();
                let cells: Vec<_> = buffer
                    .content
                    .iter()
                    .map(|cell| {
                        json!({
                            "text": cell.symbol(), "fg": format!("{:?}", cell.fg),
                            "bg": format!("{:?}", cell.bg),
                            "modifier": format!("{:?}", cell.modifier),
                        })
                    })
                    .collect();
                let path = output.join(format!(
                    "settings-{index:02}-scroll-{scroll}-{width}x{height}.json"
                ));
                std::fs::write(
                    &path,
                    serde_json::to_vec(&json!({
                        "type": "artifact", "synthetic": true, "tab": format!("{tab:?}"),
                        "provider": if tab == SettingsTab::Inference { Some("KiloGateway") } else { None },
                        "width": width, "height": height, "cells": cells, "requested_scroll": scroll,
                        "limitation": "Composed UI buffer; terminal font and live input are not tested",
                    }))?,
                )?;
                count += 1;
                println!(
                    "{}",
                    json!({"type": "artifact", "path": path,
                "tab": format!("{tab:?}"), "synthetic": true})
                );
            }
        }
        for (name, mode) in [
            ("main-empty", ilium_client::app::Mode::Normal),
            ("leader-pending", ilium_client::app::Mode::LeaderPending),
            (
                "navigation-leader-pending",
                ilium_client::app::Mode::NavigationLeaderPending,
            ),
            ("move-mode", ilium_client::app::Mode::Move),
            (
                "agent-toolbar-model-submenu",
                ilium_client::app::Mode::AgentToolbarModelSubmenu(
                    ilium_client::app::AgentToolbarModelSubmenuState {
                        pane_id: ilium_core::ROOT_ID,
                        tier_index: 0,
                        area: Rect::new(
                            width.saturating_sub(36).min(8),
                            2,
                            width.min(34),
                            height.saturating_sub(4).min(9),
                        ),
                        selected_index: 1,
                    },
                ),
            ),
            ("smart-copy", ilium_client::app::Mode::SmartCopy),
            ("context-menu", ilium_client::app::Mode::Normal),
            (
                "schedule-pane-input",
                ilium_client::app::Mode::SchedulePaneInput(Box::new({
                    let mut state =
                        ilium_client::scheduled_input::ScheduledInputDialogState::new(
                            ilium_core::ROOT_ID,
                        );
                    state.hours = ilium_client::text_prompt::TextPromptState::new("0");
                    state.minutes = ilium_client::text_prompt::TextPromptState::new("1");
                    state.seconds = ilium_client::text_prompt::TextPromptState::new("30");
                    state.text = ilium_client::text_prompt::TextPromptState::new(
                        "Synthetic delayed input",
                    );
                    state
                })),
            ),
            (
                "create-agent-from-line",
                ilium_client::app::Mode::CreateAgentFromLine(Box::new(
                    ilium_client::agent_from_line::CreateAgentFromLineState::new(
                        ilium_client::agent_from_line::EditorSourceLine {
                            pane_id: ilium_core::ROOT_ID,
                            path: output.join("synthetic-workspace/Synthetic notes.md"),
                            line_number: 7,
                            text: "Synthetic source line for contextual agent creation".into(),
                        },
                        ilium_core::ROOT_ID,
                    ),
                )),
            ),
            (
                "editor-line-context-menu",
                ilium_client::app::Mode::EditorLineContextMenu(
                    ilium_client::agent_from_line::EditorLineContextMenu {
                        source: ilium_client::agent_from_line::EditorSourceLine {
                            pane_id: ilium_core::ROOT_ID,
                            path: output.join("synthetic-workspace/Synthetic notes.md"),
                            line_number: 7,
                            text: "Synthetic source line for context-menu review".into(),
                        },
                        area: Rect::new(
                            width.saturating_sub(40).min(8),
                            2,
                            width.min(38),
                            height.saturating_sub(4).min(9),
                        ),
                        actions: vec![
                            ilium_client::agent_from_line::EditorLineContextAction::CopyLineToClipboard,
                            ilium_client::agent_from_line::EditorLineContextAction::CreateAgentFromLine,
                        ],
                        selected_index: 1,
                    },
                ),
            ),
            (
                "explorer-file-menu",
                ilium_client::app::Mode::ExplorerFileMenu(
                    ilium_client::app::ExplorerFileMenu {
                        target_group: ilium_core::ROOT_ID,
                        file_path: output.join("synthetic-workspace/Synthetic notes.md"),
                        area: Rect::new(
                            width.saturating_sub(38).min(6),
                            2,
                            width.min(36),
                            height.saturating_sub(4).min(7),
                        ),
                    },
                ),
            ),
            (
                "project-folder-explorer",
                ilium_client::app::Mode::ProjectFolderExplorer(
                    Box::new(
                        ilium_client::explorer_overlay::ExplorerOverlay::open_folder_for(
                            &output.join("synthetic-workspace"),
                            "Select synthetic project folder",
                        )?,
                    ),
                    ilium_client::app::ProjectFolderSelection::NewProject,
                ),
            ),
            (
                "create-agent-workspace",
                ilium_client::app::Mode::CreateAgentWorkspace(Box::new(
                    ilium_client::worktree_dialog::WorktreeDialogState::new(
                        ilium_core::ROOT_ID,
                        ilium_core::ROOT_ID,
                        ilium_core::BuiltinAgentProvider::Codex,
                        ilium_client::worktree_dialog::WorktreeDialogMode::New,
                    ),
                )),
            ),
            ("terminal-populated", ilium_client::app::Mode::Normal),
            ("terminal-split-horizontal", ilium_client::app::Mode::Normal),
            ("terminal-split-vertical", ilium_client::app::Mode::Normal),
            (
                "location-empty",
                synthetic_location_picker("empty", output.join("synthetic-workspace")),
            ),
            (
                "location-results-first",
                synthetic_location_picker("first", output.join("synthetic-workspace")),
            ),
            (
                "location-results-last",
                synthetic_location_picker("last", output.join("synthetic-workspace")),
            ),
            (
                "location-search-failed",
                synthetic_location_picker("failed", output.join("synthetic-workspace")),
            ),
            ("voice-prompt-empty", ilium_client::app::Mode::VoicePromptEditor(Box::new(ilium_client::voice_settings::VoicePromptEditorState::new("")))),
            ("voice-prompt-overflow", ilium_client::app::Mode::VoicePromptEditor(Box::new(ilium_client::voice_settings::VoicePromptEditorState::new(&"Synthetic voice instructions: preserve readable spacing and Unicode 界.\n".repeat(80))))),
            ("conversion-stopping", ilium_client::app::Mode::ConvertSession),
            ("conversion-starting", ilium_client::app::Mode::ConvertSession),
            ("conversion-failed-before-stop", ilium_client::app::Mode::ConvertSession),
            ("conversion-running", ilium_client::app::Mode::ConvertSession),
            ("conversion-failed", ilium_client::app::Mode::ConvertSession),
            ("stats-hover-preview", ilium_client::app::Mode::Normal),
            ("stats-pinned", ilium_client::app::Mode::Normal),
            ("queue-prompt-short", ilium_client::app::Mode::Normal),
            ("queue-prompt-long-tail", ilium_client::app::Mode::Normal),
            ("queue-prompt-long-middle", ilium_client::app::Mode::Normal),
            (
                "onboarding-keyboard-editor",
                ilium_client::app::Mode::Normal,
            ),
            (
                "onboarding-keyboard-editor-last",
                ilium_client::app::Mode::Normal,
            ),
            ("onboarding-sound-studio", ilium_client::app::Mode::Normal),
            ("onboarding-ai-choice", ilium_client::app::Mode::Normal),
            (
                "onboarding-ai-configuration",
                ilium_client::app::Mode::Normal,
            ),
            ("onboarding-sound-choice", ilium_client::app::Mode::Normal),
            (
                "onboarding-sound-configuration",
                ilium_client::app::Mode::Normal,
            ),
            (
                "onboarding-keyboard-choice",
                ilium_client::app::Mode::Normal,
            ),
            (
                "onboarding-keyboard-practice",
                ilium_client::app::Mode::Normal,
            ),
            ("onboarding-voice", ilium_client::app::Mode::Normal),
            ("board-populated-first", ilium_client::app::Mode::Normal),
            ("board-populated-last", ilium_client::app::Mode::Normal),
            ("board-details-title", ilium_client::app::Mode::Normal),
            ("board-details-notes", ilium_client::app::Mode::Normal),
            ("inference-key-prompt", ilium_client::app::Mode::Normal),
            ("inference-budget-prompt", ilium_client::app::Mode::Normal),
            ("git-setup-command-prompt", ilium_client::app::Mode::Normal),
            ("chatroom-empty", ilium_client::app::Mode::Normal),
            ("chatroom-history-oldest", ilium_client::app::Mode::Normal),
            ("chatroom-history-newest", ilium_client::app::Mode::Normal),
            ("settings-help", ilium_client::app::Mode::Normal),
            ("settings-value-number", ilium_client::app::Mode::Normal),
            ("settings-icon-catalogue", ilium_client::app::Mode::Normal),
            ("settings-keyboard-picker", ilium_client::app::Mode::Normal),
            ("voice-api-key-prompt", ilium_client::app::Mode::Normal),
            ("agent-setup-path-prompt", ilium_client::app::Mode::Normal),
            ("agent-setup-offer", ilium_client::app::Mode::Normal),
            ("animation-text-prompt", ilium_client::app::Mode::Normal),
            ("explorer-files", ilium_client::app::Mode::Normal),
            ("folder-explorer", ilium_client::app::Mode::Normal),
            ("board-path-picker", ilium_client::app::Mode::Normal),
            ("text-trigger-dialog", ilium_client::app::Mode::Normal),
            (
                "remote-compaction-running",
                ilium_client::app::Mode::RemoteCompaction,
            ),
            (
                "remote-compaction-failed",
                ilium_client::app::Mode::RemoteCompaction,
            ),
            (
                "agent-debug-history-oldest",
                ilium_client::app::Mode::Normal,
            ),
            (
                "agent-debug-history-newest",
                ilium_client::app::Mode::Normal,
            ),
            ("help", ilium_client::app::Mode::Help),
            (
                "search-empty",
                ilium_client::app::Mode::Search(Box::new(
                    ilium_client::search_ui::SearchState::new(),
                )),
            ),
            ("search-overflow", synthetic_search()),
            (
                "rename-prompt",
                ilium_client::app::Mode::Rename(ilium_client::text_prompt::TextPromptState::new(
                    "Synthetic pane name",
                )),
            ),
            (
                "command-prompt",
                ilium_client::app::Mode::CommandPrompt(
                    ilium_client::text_prompt::TextPromptState::new("synthetic-command --example"),
                ),
            ),
            (
                "api-port-prompt",
                ilium_client::app::Mode::ApiSettingPrompt(
                    ilium_client::text_prompt::TextPromptState::new("7421"),
                ),
            ),
            (
                "save-as-prompt",
                ilium_client::app::Mode::SaveAs(
                    ilium_core::ROOT_ID,
                    ilium_client::text_prompt::TextPromptState::new("/synthetic/notes/example.md"),
                ),
            ),
            (
                "debug-log-path-prompt",
                ilium_client::app::Mode::AgentDebugSavePath(
                    ilium_core::ROOT_ID,
                    ilium_client::text_prompt::TextPromptState::new(
                        "/synthetic/logs/agent-debug.txt",
                    ),
                ),
            ),
            (
                "worktree-close-waiting",
                ilium_client::app::Mode::WaitingWorkspaceCloseOffer {
                    request_id: 1,
                    pane_id: ilium_core::ROOT_ID,
                },
            ),
            (
                "worktree-close-no-workspace",
                ilium_client::app::Mode::ConfirmWorkspaceCloseOffer(ilium_core::ROOT_ID),
            ),
            (
                "worktree-remove-no-workspace",
                ilium_client::app::Mode::ConfirmRemoveWorkspace(ilium_core::ROOT_ID),
            ),
            (
                "close-workspace-confirm",
                ilium_client::app::Mode::ConfirmClose(ilium_core::ROOT_ID),
            ),
            ("agent-message-empty", synthetic_agent_message(0)),
            ("agent-message-overflow", synthetic_agent_message(40)),
            (
                "create-group-empty",
                ilium_client::app::Mode::CreateGroup(ilium_client::app::CreateGroupState {
                    area: Rect::new(0, 0, width, height),
                    destinations: Vec::new(),
                    selected_index: 0,
                    name: ilium_client::text_prompt::TextPromptState::new("Synthetic group"),
                }),
            ),
            (
                "split-members-empty",
                ilium_client::app::Mode::CreateSplitMembers(
                    ilium_client::app::CreateSplitMembersState {
                        parent_group: ilium_core::ROOT_ID,
                        orientation: ilium_core::SplitOrientation::Horizontal,
                        choices: Vec::new(),
                        selected_index: 0,
                    },
                ),
            ),
            (
                "board-card-prompt",
                ilium_client::app::Mode::BoardCardPrompt(
                    ilium_core::ROOT_ID,
                    ilium_client::text_prompt::TextPromptState::new("Synthetic card"),
                ),
            ),
            (
                "board-column-prompt",
                ilium_client::app::Mode::BoardColumnPrompt(
                    ilium_core::ROOT_ID,
                    ilium_client::text_prompt::TextPromptState::new("Synthetic column"),
                ),
            ),
            (
                "board-rename-card",
                ilium_client::app::Mode::BoardRenamePrompt(
                    ilium_core::ROOT_ID,
                    ilium_client::app::BoardRenameTarget::Card,
                    ilium_client::text_prompt::TextPromptState::new("Synthetic card name"),
                ),
            ),
            (
                "board-rename-column",
                ilium_client::app::Mode::BoardRenamePrompt(
                    ilium_core::ROOT_ID,
                    ilium_client::app::BoardRenameTarget::Column,
                    ilium_client::text_prompt::TextPromptState::new("Synthetic column name"),
                ),
            ),
            (
                "board-delete-card",
                ilium_client::app::Mode::BoardDeleteConfirm(
                    ilium_core::ROOT_ID,
                    ilium_client::app::BoardDeleteTarget::Card,
                ),
            ),
            (
                "board-delete-column",
                ilium_client::app::Mode::BoardDeleteConfirm(
                    ilium_core::ROOT_ID,
                    ilium_client::app::BoardDeleteTarget::Column,
                ),
            ),
            (
                "board-create-folder",
                synthetic_board(ilium_client::app::BoardStorageKind::Folder),
            ),
            (
                "board-create-markdown",
                synthetic_board(ilium_client::app::BoardStorageKind::MarkdownFile),
            ),
            (
                "split-horizontal",
                ilium_client::app::Mode::CreateSplitOrientation(
                    ilium_client::app::CreateSplitOrientationState {
                        orientation: ilium_core::SplitOrientation::Horizontal,
                    },
                ),
            ),
            (
                "split-vertical",
                ilium_client::app::Mode::CreateSplitOrientation(
                    ilium_client::app::CreateSplitOrientationState {
                        orientation: ilium_core::SplitOrientation::Vertical,
                    },
                ),
            ),
            (
                "worktrees-loading",
                ilium_client::app::Mode::WorktreeManager(Box::new(
                    ilium_client::worktree_manager::WorktreeManagerState::new(
                        ilium_core::ROOT_ID,
                        1,
                    ),
                )),
            ),
            (
                "session-recovery",
                ilium_client::app::Mode::ConfirmSessionRecovery { pane_count: 12 },
            ),
        ] {
            // Each dialog owns a fresh synthetic flow, so retained parents cannot
            // leak into later fixtures. Open the API prompt through real navigation.
            let mut app = App::new(
                "synthetic-dialog-capture".to_owned(),
                output.join("synthetic-workspace"),
            );
            app.config_dir = Some(output.join("synthetic-workspace"));
            app.set_screen_area(Rect::new(0, 0, width, height));
            if name == "settings-help" {
                app.mode = ilium_client::app::Mode::SettingsHelp(
                    ilium_client::settings_help::dialog::SettingsHelpState::new(
                        "VOICE-03",
                        1,
                        ilium_client::config::MotionLevel::Off,
                    ),
                );
            } else if name == "settings-icon-catalogue" {
                app.mode = ilium_client::app::Mode::Settings(SettingsState {
                    tab: SettingsTab::Icons,
                    icon_picker: Some(ilium_client::app::IconPickerState::new(
                        ilium_client::icon_settings::IconTarget::OtherAgent,
                    )),
                    ..SettingsState::default()
                });
            } else if name == "settings-keyboard-picker" {
                app.mode = ilium_client::app::Mode::Settings(SettingsState {
                    tab: SettingsTab::Keyboard,
                    keyboard_picker: Some(ilium_client::keymap::Action::CycleNextInGroup),
                    ..SettingsState::default()
                });
            } else if name == "settings-value-number" {
                let dialog = ilium_client::value_dialog_host::ValueDialogHost::settings_number(
                    ilium_client::value_settings::SettingsNumber::Ui(
                        ilium_client::value_settings::UiNumber::ProgressLines,
                    ),
                    &app,
                    output.join("synthetic-workspace"),
                )?;
                app.mode = ilium_client::app::Mode::ValueDialog(Box::new(dialog));
            } else if name == "voice-api-key-prompt" {
                app.mode = ilium_client::app::Mode::VoiceSettingPrompt(
                    ilium_client::voice_settings::VoiceSettingField::ApiKey,
                    ilium_client::text_prompt::TextPromptState::new(
                        "synthetic-voice-key-never-valid",
                    ),
                );
            } else if name == "agent-setup-path-prompt" {
                app.mode = ilium_client::app::Mode::AgentSetupPathPrompt(
                    ilium_client::agent_feature_setup::AgentFeature::Progress,
                    ilium_client::text_prompt::TextPromptState::new(
                        "/synthetic/agent-instructions.md",
                    ),
                );
            } else if name == "animation-text-prompt" {
                app.mode = ilium_client::app::Mode::AnimationTextPrompt(
                    ilium_client::app::AnimationPromptTarget {
                        control: "synthetic-title",
                        label: "Synthetic title".to_owned(),
                        hint: "Enter a title for the preview",
                        error: Some("Synthetic validation message".to_owned()),
                    },
                    ilium_client::text_prompt::TextPromptState::new(
                        "Synthetic animation title",
                    ),
                );
            } else if name == "agent-setup-offer" {
                app.mode = ilium_client::app::Mode::AgentSetupPrompt(Box::new(
                    ilium_client::setup_prompt::SetupPromptState::new(
                        ilium_client::setup_prompt::SetupPromptScope::Project(
                            output.join("synthetic-workspace"),
                        ),
                        true,
                        true,
                    ),
                ));
            } else if name == "explorer-files"
                || name == "folder-explorer"
                || name == "board-path-picker"
            {
                let directory = output.join("synthetic-workspace");
                std::fs::write(
                    directory.join("Synthetic notes.md"),
                    "Synthetic picker fixture; never opened or persisted.\n",
                )?;
                std::fs::create_dir_all(directory.join("Synthetic folder"))?;
                let overlay = if name == "folder-explorer" {
                    ilium_client::explorer_overlay::ExplorerOverlay::open_folder_for(
                        &directory,
                        "Use Folder",
                    )?
                } else {
                    ilium_client::explorer_overlay::ExplorerOverlay::open_at(&directory)?
                };
                let mode = if name == "explorer-files" {
                    ilium_client::app::Mode::Explorer(
                        Box::new(overlay),
                        ilium_core::ROOT_ID,
                    )
                } else if name == "folder-explorer" {
                    ilium_client::app::Mode::FolderExplorer(
                        Box::new(overlay),
                        ilium_core::ROOT_ID,
                    )
                } else {
                    ilium_client::app::Mode::BoardPathPicker(Box::new(overlay))
                };
                app.mode = mode;
            } else if name == "text-trigger-dialog" {
                app.mode = ilium_client::app::Mode::TextTriggerDialog(Box::new(
                    ilium_client::text_trigger_dialog::TextTriggerDialogState::new(None),
                ));
            } else if name.starts_with("stats-") {
                let group = app
                    .tree
                    .add_group(ilium_core::ROOT_ID, "Synthetic stats preview")?;
                let pane_id = app.tree.add_pane(
                    group,
                    "Synthetic terminal",
                    ilium_core::PaneContentKind::Terminal,
                )?;
                // Assign presentation directly: focus_pane queues server requests.
                app.right_panel_target = ilium_client::app::RightPanelTarget::Pane { pane_id };
                app.stats_popover = Some(ilium_client::session_stats_ui::StatsPopover::new(
                    pane_id,
                    name == "stats-pinned",
                    std::time::Instant::now(),
                ));
            } else if name.starts_with("queue-prompt-") {
                let mut state =
                    ilium_client::prompt_queue::PromptQueueDialogState::new(ilium_core::ROOT_ID);
                let text = if name == "queue-prompt-short" {
                    "Review the changes and report the remaining checks.".to_owned()
                } else {
                    (0..60)
                        .map(|row| format!("Synthetic draft line {row:02} — readable UTF-8 text\n"))
                        .collect::<String>()
                        + "EDITED-TAIL"
                };
                state.text = ilium_client::text_prompt::TextPromptState::new(text);
                if name == "queue-prompt-long-middle" {
                    state.text.cursor = state.text.buf.chars().count() / 2;
                }
                app.mode = ilium_client::app::Mode::QueuePrompt(Box::new(state));
            } else if name.starts_with("onboarding-") {
                use ilium_client::onboarding::state::{
                    AiChoice, KeyboardChoice, SoundChoice, Step,
                };
                app.onboarding = Some(Default::default());
                app.onboarding_progress.started = true;
                app.onboarding_progress.wizard.ai = Some(AiChoice::Local);
                app.onboarding_progress.wizard.sound = Some(SoundChoice::Bundled);
                app.onboarding_progress.wizard.keyboard = Some(KeyboardChoice::Tmux);
                app.onboarding_progress.wizard.step = match name {
                    "onboarding-ai-choice" => Step::AiChoice,
                    "onboarding-ai-configuration" => Step::AiConfiguration,
                    "onboarding-sound-choice" => Step::SoundChoice,
                    "onboarding-sound-configuration" => Step::SoundConfiguration,
                    "onboarding-keyboard-choice" => Step::KeyboardChoice,
                    "onboarding-keyboard-practice" => Step::KeyboardPractice,
                    "onboarding-voice" => Step::Voice,
                    "onboarding-keyboard-editor" | "onboarding-keyboard-editor-last" => {
                        Step::KeyboardPractice
                    }
                    "onboarding-sound-studio" => Step::SoundConfiguration,
                    _ => unreachable!("named onboarding fixture"),
                };
                if name.starts_with("onboarding-keyboard-editor") {
                    app.onboarding_progress.wizard.keyboard = Some(KeyboardChoice::Custom);
                    let ui = app.onboarding.as_mut().unwrap();
                    ui.keyboard_editing = true;
                    if name.ends_with("-last") {
                        ui.keyboard_ui.scroll = app.keybindings.len().saturating_sub(1);
                    }
                } else if name == "onboarding-sound-studio" {
                    app.onboarding_progress.wizard.sound = Some(SoundChoice::Custom);
                    app.onboarding.as_mut().unwrap().studio = Some(
                        ilium_client::onboarding::studio::SoundStudio::new(Default::default()),
                    );
                }
                // Rendering cached setup state never invokes setup, provider or demo actions.
            } else if name.starts_with("board-populated-") || name.starts_with("board-details-") {
                configure_synthetic_board_pane(
                    &mut app,
                    name,
                    output.join("synthetic-workspace"),
                    width,
                    height,
                )?;
            } else if name.starts_with("chatroom-") {
                configure_synthetic_chatroom(&mut app, name, output.join("synthetic-workspace"))?;
            } else if name.starts_with("terminal-") {
                configure_synthetic_terminals(&mut app, name, width, height)?;
            } else if name.starts_with("conversion-") {
                configure_synthetic_conversion(&mut app, name);
            } else if name.starts_with("remote-compaction-") {
                configure_synthetic_remote_compaction(&mut app, name == "remote-compaction-failed");
            } else if name.starts_with("agent-debug-history-") {
                configure_synthetic_debug_history(&mut app, name == "agent-debug-history-oldest")?;
            } else if name.starts_with("inference-") {
                app.mode = ilium_client::app::Mode::Settings(SettingsState {
                    tab: SettingsTab::Inference,
                    ..SettingsState::default()
                });
                // Always use an invented credential; never capture a real secret.
                app.inference_settings.openai.api_key = "synthetic-not-a-real-key".to_owned();
                app.settings_open_inference_field(if name == "inference-key-prompt" {
                    ilium_client::app::InferenceSettingField::OpenAiApiKey
                } else {
                    ilium_client::app::InferenceSettingField::RestructurePromptTokenLimit
                });
            } else if name == "git-setup-command-prompt" {
                app.mode = ilium_client::app::Mode::Settings(SettingsState {
                    tab: SettingsTab::Git,
                    ..SettingsState::default()
                });
                app.git_settings.setup_command =
                    "synthetic-setup --example --long-readable-argument".to_owned();
                app.settings_open_git_text_field(ilium_client::app::GitTextField::SetupCommand);
            } else if name == "api-port-prompt" {
                app.mode = ilium_client::app::Mode::Settings(SettingsState {
                    tab: SettingsTab::Api,
                    ..SettingsState::default()
                });
                app.settings_open_api_port();
            } else if name == "context-menu" {
                let group = app
                    .tree
                    .add_group(ilium_core::ROOT_ID, "Synthetic context menu")?;
                app.open_context_menu(group, width.saturating_sub(8), height / 2);
            } else {
                app.mode = mode;
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height))?;
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
            let buffer = terminal.backend().buffer();
            let cells: Vec<_> = buffer
                .content
                .iter()
                .map(|cell| {
                    json!({
                        "text": cell.symbol(), "fg": format!("{:?}", cell.fg),
                        "bg": format!("{:?}", cell.bg),
                        "modifier": format!("{:?}", cell.modifier),
                    })
                })
                .collect();
            let limitation = match name {
                name if name.starts_with("terminal-") =>
                    "Synthetic terminal parser buffers and tree; no PTY, shell command, server request, physical flush or live font is tested",
                name if name.starts_with("location-") =>
                    "Synthetic cached location picker; no search client, address lookup, network request or setting save is performed",
                name if name.starts_with("voice-prompt-") =>
                    "Synthetic voice instruction editor; no saving, provider request, microphone, audio or live input is performed",
                name if name.starts_with("conversion-") =>
                    "Synthetic cached conversion state; no agent termination, transcript conversion, worker, server request or resume action is performed",
                name if name.starts_with("stats-") =>
                    "Synthetic pane and empty cached stats presentation; no transcript loading, stats worker, server request, font or live input is tested",
                name if name.starts_with("queue-prompt-") =>
                    "Synthetic queued draft and cursor; no enqueue, bell detection or terminal submission is performed",
                name if name.starts_with("onboarding-") =>
                    "Synthetic cached onboarding step; no provider requests, saves, audio, voice or practice actions; no actual audio playback or live keyboard input is tested",
                "board-populated-first" | "board-populated-last" | "board-details-title" | "board-details-notes" =>
                    "Synthetic Markdown board loaded from new fixture input; no board saves, server requests, font or live input are tested",
                "chatroom-empty" | "chatroom-history-oldest" | "chatroom-history-newest" =>
                    "Synthetic cached chatroom only; no file writes, live participants, terminal font or input are tested",
                "remote-compaction-running" | "remote-compaction-failed" =>
                    "Synthetic compaction presentation only; no worker, provider request, transcript or process operation; font and live input are not tested",
                "agent-debug-history-oldest" | "agent-debug-history-newest" =>
                    "Synthetic cached debug events and tree-only pane; server journal, terminal font and live input are not tested",
                "save-as-prompt" | "debug-log-path-prompt" =>
                    "Synthetic prompt without a real pane or populated parent; terminal font and live input are not tested",
                "worktree-close-waiting" | "worktree-close-no-workspace" |
                "worktree-remove-no-workspace" | "close-workspace-confirm" =>
                    "Synthetic workspace fallback without a real workspace or close operation; terminal font and live input are not tested",
                _ => "Composed UI buffer; terminal font and live input are not tested",
            };
            let stats_popover_visible = if name.starts_with("stats-") {
                let popover = app
                    .stats_popover
                    .as_ref()
                    .ok_or("stats fixture state missing")?;
                let viewport = app
                    .pane_viewport(popover.pane_id)
                    .ok_or("stats fixture pane missing")?;
                let anchor = ilium_client::theme::chrome_stats_cell(viewport.outer_area);
                let visible =
                    ilium_client::session_stats_ui::geometry(app.layout.pane_area, anchor)
                        .is_some();
                let rows: Vec<String> = buffer
                    .content
                    .chunks(usize::from(width))
                    .map(|row| row.iter().map(|cell| cell.symbol()).collect())
                    .collect();
                if visible && !rows.iter().any(|row| row.contains("Costs & stats")) {
                    return Err("stats popover geometry fits but title did not render".into());
                }
                Some(visible)
            } else {
                None
            };
            let path = output.join(format!("{name}-{width}x{height}.json"));
            std::fs::write(
                &path,
                serde_json::to_vec(&json!({
                    "type": "artifact", "synthetic": true, "fixture": name,
                    "width": width, "height": height, "cells": cells,
                    "stats_popover_visible": stats_popover_visible,
                    "retained_parent": if name == "api-port-prompt" {
                        Some("Settings/Api")
                    } else if name == "settings-icon-catalogue" {
                        Some("Settings/Icons")
                    } else if name == "settings-keyboard-picker" {
                        Some("Settings/Keyboard")
                    } else if name.starts_with("inference-") {
                        Some("Settings/Inference")
                    } else if name == "git-setup-command-prompt" {
                        Some("Settings/Git")
                    } else { None },
                    "limitation": limitation,
                }))?,
            )?;
            count += 1;
            println!(
                "{}",
                json!({"type": "artifact", "path": path,
                "fixture": name, "synthetic": true})
            );
        }
        // A populated sidebar is separate from the empty-main fixture. No
        // runtime panes or commands are created by these tree-only fixtures.
        let mut populated = App::new(
            "synthetic-populated-sidebar".to_owned(),
            output.join("synthetic-workspace"),
        );
        populated.config_dir = Some(output.join("synthetic-workspace"));
        populated.set_screen_area(Rect::new(0, 0, width, height));
        let project = populated
            .tree
            .add_project(output.join("synthetic-workspace/project"))?;
        let group = populated
            .tree
            .add_group(project, "Synthetic project work")?;
        let mut pane_ids = Vec::new();
        for index in 0..48 {
            let pane = populated.tree.add_pane(
                group,
                format!("Synthetic task {index:02} — long readable title"),
                ilium_core::PaneContentKind::Terminal,
            )?;
            pane_ids.push(pane);
        }
        populated.tree_state.open(vec![project]);
        populated.tree_state.open(vec![project, group]);
        for (name, selected) in [
            ("main-populated-tree-first", pane_ids[0]),
            ("main-populated-tree-last", pane_ids[47]),
        ] {
            populated.tree_state.select(vec![project, group, selected]);
            let mut terminal = Terminal::new(TestBackend::new(width, height))?;
            terminal.draw(|frame| ui::draw(frame, &mut populated))?;
            let buffer = terminal.backend().buffer();
            let cells: Vec<_> = buffer
                .content
                .iter()
                .map(|cell| {
                    json!({
                        "text": cell.symbol(), "fg": format!("{:?}", cell.fg),
                        "bg": format!("{:?}", cell.bg), "modifier": format!("{:?}", cell.modifier),
                    })
                })
                .collect();
            let path = output.join(format!("{name}-{width}x{height}.json"));
            std::fs::write(
                &path,
                serde_json::to_vec(&json!({
                    "type": "artifact", "synthetic": true, "fixture": name,
                    "width": width, "height": height, "cells": cells,
                    "limitation": "Populated sidebar only; terminal pane contents, font and live input are not tested",
                }))?,
            )?;
            count += 1;
            println!(
                "{}",
                json!({"type": "artifact", "path": path,
                "fixture": name, "synthetic": true})
            );
        }
        let editor_pane = populated.tree.add_pane(
            group,
            "Synthetic readable editor",
            ilium_core::PaneContentKind::Editor,
        )?;
        let mut editor = ilium_client::editor_pane::EditorPane::empty();
        editor.textarea.insert_str(
            "# A synthetic project note\n\nThis buffer exists only to review the editor layout.\n\n- Clear headings\n- Comfortable line spacing\n- A second line that wraps naturally at compact widths\n\nThe cursor stays near the end of this sample.\nEnd marker: synthetic editor frame.\n",
        );
        editor
            .textarea
            .move_cursor(ratatui_textarea::CursorMove::Bottom);
        populated.panes.insert(
            editor_pane,
            ilium_client::app::PaneRuntime::Editor(Box::new(editor)),
        );
        populated
            .tree_state
            .select(vec![project, group, editor_pane]);
        populated.right_panel_target = ilium_client::app::RightPanelTarget::Pane {
            pane_id: editor_pane,
        };
        populated.focus = ilium_client::app::FocusTarget::Pane;
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        terminal.draw(|frame| ui::draw(frame, &mut populated))?;
        let buffer = terminal.backend().buffer();
        let cells: Vec<_> = buffer
            .content
            .iter()
            .map(|cell| {
                json!({
                    "text": cell.symbol(), "fg": format!("{:?}", cell.fg),
                    "bg": format!("{:?}", cell.bg), "modifier": format!("{:?}", cell.modifier),
                })
            })
            .collect();
        let path = output.join(format!("editor-populated-{width}x{height}.json"));
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "type": "artifact", "synthetic": true, "fixture": "editor-populated",
                "width": width, "height": height, "cells": cells,
                "limitation": "Synthetic editor buffer and tree only; no file write, font or live input is tested",
            }))?,
        )?;
        count += 1;
        println!(
            "{}",
            json!({"type": "artifact", "path": path,
            "fixture": "editor-populated", "synthetic": true})
        );
    }
    println!(
        "{}",
        json!({"type": "summary", "frames": count,
        "output_dir": output, "synthetic": true})
    );
    Ok(())
}

fn synthetic_search() -> ilium_client::app::Mode {
    use ilium_client::search_ui::{SearchLocation, SearchObjectKind, SearchResult, SearchState};
    let mut search = SearchState::new();
    search.query = ilium_client::text_prompt::TextPromptState::new("example");
    search.replace_results(
        (0..60)
            .map(|index| SearchResult {
                pane_id: ilium_core::ROOT_ID,
                kind: SearchObjectKind::File,
                object_name: format!("Synthetic file {index:02}"),
                automatic_title: None,
                path: None,
                last_command: None,
                before: "Synthetic context before ".to_owned(),
                matched: "example".to_owned(),
                after: " and after the match".to_owned(),
                location: SearchLocation::Editor { line: index },
            })
            .collect(),
    );
    search.selected_index = 59;
    ilium_client::app::Mode::Search(Box::new(search))
}

fn synthetic_board(storage_kind: ilium_client::app::BoardStorageKind) -> ilium_client::app::Mode {
    ilium_client::app::Mode::CreateBoard(ilium_client::app::CreateBoardState {
        parent_group: ilium_core::ROOT_ID,
        name: ilium_client::text_prompt::TextPromptState::new("Synthetic project board"),
        path: ilium_client::text_prompt::TextPromptState::new("/synthetic/boards/project"),
        storage_kind,
        editing_path: false,
    })
}

fn synthetic_agent_message(count: u64) -> ilium_client::app::Mode {
    use ilium_client::agent_message_dialog::{
        AgentMessageDialog, Focus, Recipient, RecipientState,
    };
    let recipients = (0..count)
        .map(|index| Recipient {
            pane_id: ilium_core::NodeId(index + 1),
            label: format!("Synthetic agent {index:02}"),
            checked: true,
            state: RecipientState::Pending,
        })
        .collect();
    let mut dialog = AgentMessageDialog::new(
        ilium_core::ROOT_ID,
        "Synthetic project".to_owned(),
        recipients,
    );
    dialog
        .message
        .insert_str("Synthetic message preview; no message will be sent.");
    dialog.focus = Focus::Recipients;
    dialog.selected = count.saturating_sub(1) as usize;
    ilium_client::app::Mode::AgentMessageDialog(Box::new(dialog))
}

/// The dispatcher retrieves every hashed file under its target directory.
/// Keep captures outside source so rendering cannot invalidate source receipts.
fn service_output_directory() -> Result<PathBuf, Box<dyn Error>> {
    let job = std::env::var("NI_BUILD_REMOTE_JOB")?;
    if job.len() != 32 || !job.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("--service-output requires a valid ni-build remote job identity".into());
    }
    let target = PathBuf::from(
        std::env::var_os("CARGO_TARGET_DIR").ok_or("ni-build target directory is missing")?,
    );
    let owned_target = PathBuf::from("/data/ni-build-service/jobs")
        .join(job)
        .join("target");
    if target != owned_target {
        return Err("capture target must belong to the identified ni-build remote job".into());
    }
    Ok(target.join("ui-chrome-captures"))
}

/// Populate presentation data only; no terminal process or server journal exists.
fn configure_synthetic_debug_history(app: &mut App, oldest: bool) -> Result<(), Box<dyn Error>> {
    use ilium_client::app::{
        AgentDebugLogCache, AgentDebugLogScrollPosition, AgentDebugLogViewState, Mode,
    };
    let group = app
        .tree
        .add_group(ilium_core::ROOT_ID, "Synthetic debug history")?;
    let pane_id = app.tree.add_pane(
        group,
        "Synthetic agent",
        ilium_core::PaneContentKind::Terminal,
    )?;
    let entries = (1..=80).map(|sequence| ilium_ipc::AgentDebugEntry {
        sequence,
        occurred_at_unix_millis: 1_700_000_000_000 + (sequence as i64) * 1000,
        severity: ilium_ipc::AgentDebugSeverity::Success,
        source: ilium_ipc::AgentDebugSource::SessionDiscovery,
        kind: ilium_ipc::AgentDebugEventKind::SessionResolved,
        summary: format!("Synthetic event {sequence:02}: project session discovery completed with a long explanatory message"),
        fields: vec![ilium_ipc::AgentDebugField::sensitive("session ID", "synthetic-session")],
        correlation_id: None,
        context: ilium_ipc::AgentDebugContext::default(),
        metadata: Default::default(),
    }).collect();
    app.agent_debug_logs.insert(
        pane_id,
        AgentDebugLogCache {
            through_sequence: 80,
            has_loaded_retained_history: true,
            log: ilium_ipc::PaneDebugLog {
                entries,
                next_sequence: 81,
                ..Default::default()
            },
            ..AgentDebugLogCache::default()
        },
    );
    app.mode = Mode::AgentDebugLog(AgentDebugLogViewState {
        pane_id,
        scroll_position: if oldest {
            AgentDebugLogScrollPosition::FromOldest(0)
        } else {
            AgentDebugLogScrollPosition::FromNewest(0)
        },
    });
    Ok(())
}

/// Exercise the composed dialog without submitting a compaction worker job.
fn configure_synthetic_remote_compaction(app: &mut App, failed: bool) {
    use ilium_client::remote_compaction_dialog::{
        RemoteCompactionDialogState, RemoteCompactionPhase, RemoteCompactionPlan,
    };
    let mut state = RemoteCompactionDialogState::new(RemoteCompactionPlan {
        pane_id: ilium_core::ROOT_ID,
        provider: ilium_core::BuiltinAgentProvider::Claude,
        agent: ilium_remote_compaction::AgentKind::Claude,
        session_id: "synthetic-session".to_owned(),
        project_cwd: PathBuf::from("/synthetic/project"),
        transcript_path: PathBuf::from("/synthetic/project/transcript.jsonl"),
        technique: ilium_remote_compaction::Technique::ClaudeCode,
        destination: "Synthetic provider / synthetic model".to_owned(),
        is_automatic: false,
    });
    state.phase = RemoteCompactionPhase::Compacting;
    state.is_agent_stopped = true;
    state.current_step = 3;
    state.progress = 0.6;
    for index in 0..40 {
        state.push_log(format!(
            "Synthetic log {index:02}: transcript section prepared for summarization"
        ));
    }
    if failed {
        state.fail("Synthetic provider failure: the session can be resumed safely".to_owned());
    }
    app.remote_compaction = Some(Box::new(state));
    app.mode = ilium_client::app::Mode::RemoteCompaction;
}

/// Presentation-only cache: never initialize or append a real chatroom file.
fn configure_synthetic_chatroom(
    app: &mut App,
    name: &str,
    project_path: PathBuf,
) -> Result<(), Box<dyn Error>> {
    let project_id = app.tree.add_project(project_path)?;
    let mut room = ilium_client::app::ChatroomViewState::default();
    if name != "chatroom-empty" {
        room.messages = (0..60).map(|index| ilium_client::chatroom::ChatMessage {
            timestamp: format!("2026-10-08 12:{index:02}:00 +02:00"),
            author: "synthetic-agent".to_owned(),
            content: format!("Synthetic coordination record {index:02}: a long explanation tests wrapped message spacing and the history overflow track."),
        }).collect();
        room.draft = "Synthetic unsent draft — long enough to exercise horizontal composer scrolling and keep the insertion point visible".to_owned();
        if name == "chatroom-history-oldest" {
            room.scroll_from_newest = usize::MAX;
        }
    }
    app.chatrooms.insert(project_id, room);
    app.right_panel_target = ilium_client::app::RightPanelTarget::Chatroom { project_id };
    app.focus = ilium_client::app::FocusTarget::Pane;
    app.mode = ilium_client::app::Mode::Normal;
    Ok(())
}

/// A real cached BoardPane in the compositor, without a server or a board save.
fn configure_synthetic_board_pane(
    app: &mut App,
    name: &str,
    directory: PathBuf,
    width: u16,
    height: u16,
) -> Result<(), Box<dyn Error>> {
    use ilium_client::board::{BoardPane, CardEditorField};
    use std::io::Write;
    let path = directory.join(format!("synthetic-{name}-{width}x{height}.md"));
    let mut markdown = String::from("# Synthetic Kanban capture\n");
    for column in 0..6 {
        markdown.push_str(&format!("\n## Synthetic column {column}\n"));
        for card in 0..30 {
            markdown.push_str(&format!(
                "- Synthetic card {card:02} — readable long title\n"
            ));
            for line in 0..24 {
                markdown.push_str(&format!(
                    "  Synthetic note {line:02}: contextual details for this fixture card.\n"
                ));
            }
        }
    }
    let mut input = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    input.write_all(markdown.as_bytes())?;
    drop(input);
    let mut board = BoardPane::load(ilium_core::BoardStorage::MarkdownFile { path })?;
    if board.columns.len() != 6 || board.columns.iter().any(|column| column.cards.len() != 30) {
        return Err("synthetic Markdown board did not produce the planned six columns and thirty cards per column".into());
    }
    if name == "board-populated-last" {
        board.select_card(5, 29);
        board.column_scroll = 5;
    } else if name.starts_with("board-details-") {
        board.open_card_details(0, 0);
        if name == "board-details-notes" {
            let editor = board
                .detail_editor
                .as_mut()
                .ok_or("synthetic detail editor missing")?;
            editor.focus = CardEditorField::Body;
            editor
                .body
                .move_cursor(ratatui_textarea::CursorMove::Jump(20, 0));
        }
    } else {
        board.select_card(0, 0);
    }
    let project = app.tree.add_project(directory)?;
    let pane = app.tree.add_pane(
        project,
        "Synthetic Kanban board",
        ilium_core::PaneContentKind::Board,
    )?;
    app.tree_state.open(vec![project]);
    app.tree_state.select(vec![project, pane]);
    app.panes
        .insert(pane, ilium_client::app::PaneRuntime::Board(Box::new(board)));
    app.right_panel_target = ilium_client::app::RightPanelTarget::Pane { pane_id: pane };
    app.focus = ilium_client::app::FocusTarget::Pane;
    app.mode = ilium_client::app::Mode::Normal;
    Ok(())
}

fn configure_synthetic_conversion(app: &mut App, name: &str) {
    use ilium_client::session_conversion::{ConversionDialogState, ConversionPhase};
    use ilium_core::BuiltinAgentProvider;
    let mut state = ConversionDialogState::new(
        ilium_core::ROOT_ID,
        BuiltinAgentProvider::Claude,
        BuiltinAgentProvider::Codex,
        "synthetic-session".to_owned(),
        PathBuf::from("/synthetic/project"),
    );
    state.is_agent_stopped = !matches!(
        name,
        "conversion-stopping" | "conversion-failed-before-stop"
    );
    state.progress = 0.55;
    state.steps = vec![
        "Read source conversation".to_owned(),
        "Translate messages".to_owned(),
        "Write target conversation".to_owned(),
    ];
    state.total_steps = state.steps.len();
    state.current_step = 2;
    state.log = (0..40)
        .map(|index| {
            format!("Synthetic conversion log line {index}: translating conversation messages")
        })
        .collect();
    state.phase = match name {
        "conversion-stopping" => ConversionPhase::StoppingAgent,
        "conversion-starting" => ConversionPhase::Starting,
        "conversion-failed" | "conversion-failed-before-stop" => ConversionPhase::Failed(
            "Synthetic conversion failure: target provider unavailable".to_owned(),
        ),
        _ => ConversionPhase::Converting,
    };
    app.conversion = Some(Box::new(state));
    app.mode = ilium_client::app::Mode::ConvertSession;
}

fn synthetic_location_picker(kind: &str, directory: PathBuf) -> ilium_client::app::Mode {
    let mut app = App::new("synthetic-location-capture".to_owned(), directory.clone());
    app.config_dir = Some(directory);
    app.open_location_picker();
    let ilium_client::app::Mode::LocationPicker(picker) = &mut app.mode else {
        return app.mode;
    };
    picker.candidate = ilium_ambient::GeoLocation::new("Synthetic observer", 48.86, 2.35);
    if matches!(kind, "first" | "last") {
        picker.results = (0..24)
            .map(|index| {
                ilium_ambient::GeoLocation::new(
                    format!("Synthetic location {index}: readable long result label 界"),
                    48.0 + f64::from(index) * 0.01,
                    2.35,
                )
            })
            .collect();
        picker.selected_result = if kind == "last" { 23 } else { 0 };
        picker.candidate = picker.results[picker.selected_result].clone();
    } else if kind == "failed" {
        picker.status =
            Some("Synthetic address provider unavailable; enter coordinates directly".to_owned());
    }
    std::mem::replace(&mut app.mode, ilium_client::app::Mode::Normal)
}

fn configure_synthetic_terminals(
    app: &mut App,
    name: &str,
    width: u16,
    height: u16,
) -> Result<(), Box<dyn Error>> {
    use ilium_client::app::{FocusTarget, PaneRuntime, RightPanelTarget};
    use ilium_client::terminal_view::TerminalView;
    use ilium_core::{PaneContentKind, SplitOrientation, ROOT_ID};
    let group = app.tree.add_group(ROOT_ID, "Synthetic workspace")?;
    let first = app
        .tree
        .add_pane(group, "Build output", PaneContentKind::Terminal)?;
    let mut first_view = TerminalView::new(height, width);
    app.panes
        .insert(first, PaneRuntime::Terminal(Box::new(first_view)));
    app.right_panel_target = if name == "terminal-populated" {
        RightPanelTarget::Pane { pane_id: first }
    } else {
        let second = app
            .tree
            .add_pane(group, "Review notes", PaneContentKind::Terminal)?;
        let mut second_view = TerminalView::new(height, width);
        app.panes
            .insert(second, PaneRuntime::Terminal(Box::new(second_view)));
        let orientation = if name == "terminal-split-horizontal" {
            SplitOrientation::Horizontal
        } else {
            SplitOrientation::Vertical
        };
        let split_id =
            app.tree
                .create_split_view(group, "Synthetic split", orientation, &[first, second])?;
        RightPanelTarget::SplitView {
            split_id,
            active_pane_id: Some(second),
        }
    };
    app.focus = FocusTarget::Pane;
    app.set_screen_area(Rect::new(0, 0, width, height));
    app.mode = ilium_client::app::Mode::Normal;
    Ok(())
}
