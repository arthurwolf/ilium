//! Narrow App integration. The wizard keeps the existing Settings mode as its
//! parent, so normal settings prompts retain their save and dismissal paths.

use crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ilium_inference::InferenceProviderKind as Provider;
use ratatui::{
    layout::{Position, Rect},
    style::{Color, Style},
    text::Line,
    widgets::Paragraph,
    Frame,
};

use super::{
    screen::{self, Geometry, Hit, WizardUi},
    state::{AiChoice, KeyboardChoice, Navigation, SoundChoice, Step},
};
use crate::{
    app::{App, InferenceSettingField, Mode},
    keymap::KeymapPreset,
};

pub fn open(app: &mut App, from_explicit_entry: bool) {
    app.onboarding_progress.begin();
    app.onboarding_revision = app.onboarding_revision.wrapping_add(1);
    if from_explicit_entry {
        app.onboarding_progress.wizard.step = Step::AiChoice;
    }
    app.onboarding = Some(step_ui(app));
    persist(app);
}

fn persist(app: &mut App) {
    if let Some(directory) = &app.config_dir {
        if let Err(error) =
            crate::config::save_onboarding_progress(directory, &app.onboarding_progress)
        {
            app.status_message = Some(format!("Could not save setup progress: {error}"));
        }
    }
}

pub fn owns_input(app: &App) -> bool {
    app.onboarding.is_some()
        && app.modal_stack.is_empty()
        && matches!(app.mode, Mode::Normal | Mode::Settings(_))
}

fn is_choice(step: Step) -> bool {
    matches!(
        step,
        Step::AiChoice | Step::SoundChoice | Step::KeyboardChoice
    )
}

#[derive(Clone, Copy)]
enum Control {
    Field(InferenceSettingField),
    PaidProvider,
    Model,
    OpenAiModel,
    Refresh,
    Test,
}

fn provider_rows(app: &App) -> Vec<(String, Control)> {
    let settings = &app.inference_settings;
    let mut rows = Vec::new();
    if app.onboarding_progress.wizard.ai == Some(AiChoice::Paid) {
        rows.push((
            format!("Provider  ·  {}", settings.selected_provider.label()),
            Control::PaidProvider,
        ));
    }
    let fields = match settings.selected_provider {
        Provider::KiloGateway => {
            rows.push((
                format!("Model  ·  {}", settings.kilo_gateway.model),
                Control::Model,
            ));
            Vec::new()
        }
        Provider::Ollama => vec![
            (
                InferenceSettingField::OllamaUrl,
                settings.ollama.base_url.clone(),
            ),
            (
                InferenceSettingField::OllamaModel,
                settings.ollama.model.clone(),
            ),
        ],
        Provider::OpenAi => vec![
            (
                InferenceSettingField::OpenAiUrl,
                settings.openai.base_url.clone(),
            ),
            (
                InferenceSettingField::OpenAiApiKey,
                key_status(&settings.openai.api_key),
            ),
            (
                InferenceSettingField::OpenAiModel,
                settings.openai.model.clone(),
            ),
        ],
        Provider::Anthropic => vec![
            (
                InferenceSettingField::AnthropicUrl,
                settings.anthropic.base_url.clone(),
            ),
            (
                InferenceSettingField::AnthropicApiKey,
                key_status(&settings.anthropic.api_key),
            ),
            (
                InferenceSettingField::AnthropicModel,
                settings.anthropic.model.clone(),
            ),
        ],
        Provider::OpenRouter => vec![
            (
                InferenceSettingField::OpenRouterApiKey,
                key_status(&settings.openrouter.api_key),
            ),
            (
                InferenceSettingField::OpenRouterModel,
                settings.openrouter.model.clone(),
            ),
        ],
    };
    rows.extend(fields.into_iter().map(|(field, value)| {
        (
            format!("{}  ·  {value}", field.label()),
            Control::Field(field),
        )
    }));
    if settings.selected_provider == Provider::OpenAi {
        rows.push((
            format!(
                "Choose discovered model  ·  {} model(s)",
                app.openai_models.len()
            ),
            Control::OpenAiModel,
        ));
    }
    rows.push(("Refresh available models".into(), Control::Refresh));
    rows.push(("Test connection and organization".into(), Control::Test));
    rows
}

fn key_status(key: &str) -> String {
    if key.trim().is_empty() {
        "Add API key"
    } else {
        "Configured · hidden"
    }
    .into()
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let Some(ui) = &app.onboarding else {
        return;
    };
    let step = app.onboarding_progress.wizard.step;
    let geometry = screen::render_shell(frame, area, step);
    if let Some(message) = &app.status_message {
        frame.render_widget(
            Paragraph::new(message.as_str()).style(Style::new().fg(Color::Rgb(194, 176, 148))),
            Rect::new(
                geometry.header.x,
                geometry.header.y.saturating_add(1),
                geometry.header.width,
                geometry.header.height.saturating_sub(1).min(1),
            ),
        );
    }

    if let Some(index) = ui.footer_focus {
        let (rect, label) = match index {
            0 => (geometry.back, "‹ Back"),
            1 => (geometry.skip, "Skip"),
            _ => (
                geometry.next,
                if step == Step::Voice {
                    "Finish ›"
                } else {
                    "Continue ›"
                },
            ),
        };
        frame.render_widget(
            Paragraph::new(label).centered().style(
                Style::new()
                    .fg(Color::Rgb(13, 16, 23))
                    .bg(Color::Rgb(242, 188, 105)),
            ),
            rect,
        );
    }
    if is_choice(step) {
        screen::render_choices(frame, &geometry, step, ui);
        return;
    }
    if step == Step::SoundConfiguration {
        if app.onboarding_progress.wizard.sound == Some(SoundChoice::Custom) {
            if let Some(studio) = &ui.studio {
                super::studio_ui::render(frame, geometry.content, studio, &ui.studio_ui);
            }
        } else {
            render_sound_catalog(frame, geometry.content, app);
        }
        return;
    }
    if step == Step::KeyboardPractice {
        if ui.keyboard_editing {
            super::keyboard_ui::render(
                frame,
                geometry.content,
                &ui.keyboard_ui,
                &app.keyboard_settings,
                &app.keybindings,
                app.status_message.as_deref(),
            );
            return;
        }
        super::practice::render(
            frame,
            geometry.content,
            &ui.practice,
            &app.keyboard_settings,
            &app.keybindings,
            app.ui_settings.motion_level,
            practice_elapsed(ui),
        );
        frame.render_widget(
            Paragraph::new("Edit shortcuts").centered().style(
                Style::new()
                    .fg(Color::Rgb(242, 188, 105))
                    .bg(Color::Rgb(24, 29, 40)),
            ),
            keyboard_edit_area(geometry.content),
        );
        frame.render_widget(
            Paragraph::new(if geometry.content.width < 60 {
                "E keys · PgUp/PgDn hints · Tab footer"
            } else {
                "E edit keys · PgUp/PgDn hints · Tab footer · Esc cancel"
            })
            .style(Style::new().fg(Color::Rgb(144, 157, 179))),
            Rect::new(
                geometry.content.x,
                geometry.content.bottom().saturating_sub(1),
                geometry.content.width,
                geometry.content.height.min(1),
            ),
        );
        return;
    }
    if step == Step::Voice {
        super::voice_ui::render(
            frame,
            geometry.content,
            &app.voice_settings,
            &ui.voice_state,
            &ui.voice_ui,
        );
        return;
    }
    if step == Step::AiConfiguration {
        let rows = provider_rows(app);
        let form_area = Rect {
            height: geometry.content.height.saturating_sub(2),
            ..geometry.content
        };
        let lines = rows
            .iter()
            .enumerate()
            .flat_map(|(index, (label, _))| {
                let style =
                    if ui.hovered == Some(index) || (ui.hovered.is_none() && ui.focus == index) {
                        Style::new()
                            .fg(Color::Rgb(13, 16, 23))
                            .bg(Color::Rgb(242, 188, 105))
                    } else {
                        Style::new().fg(Color::Rgb(224, 231, 244))
                    };
                [
                    Line::styled(format!("  {label}  ›"), style),
                    Line::default(),
                ]
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines).scroll((ui.scroll, 0)), form_area);
        let test_message = match &app.inference_test_state {
            crate::app::InferenceTestState::Idle => None,
            crate::app::InferenceTestState::Running {
                provider,
                started_at,
            } => Some(format!(
                "{} · testing · {:.1}s",
                provider.label(),
                started_at.elapsed().as_secs_f32()
            )),
            crate::app::InferenceTestState::Succeeded { provider, elapsed } => Some(format!(
                "{} · passed · {:.1}s",
                provider.label(),
                elapsed.as_secs_f32()
            )),
            crate::app::InferenceTestState::Failed {
                provider,
                error,
                elapsed,
            } => Some(format!(
                "{} · failed · {:.1}s · {error}",
                provider.label(),
                elapsed.as_secs_f32()
            )),
        };
        if let Some(message) = test_message.as_ref().or(app.status_message.as_ref()) {
            let row = Rect::new(
                geometry.content.x,
                geometry.content.bottom().saturating_sub(1),
                geometry.content.width,
                geometry.content.height.min(1),
            );
            frame.render_widget(
                Paragraph::new(message.as_str()).style(Style::new().fg(Color::Rgb(144, 157, 179))),
                row,
            );
        }
    }
}

fn activate(app: &mut App, hit: Hit) {
    let step = app.onboarding_progress.wizard.step;
    match hit {
        Hit::DefaultPreview => {
            let mut settings = app.sound_settings.clone();
            settings.source = ilium_sound::SoundSourceKind::BundledChirping;
            app.queue_request(ilium_ipc::ClientRequest::PreviewSoundSettings { settings });
            app.status_message = Some("Playing the Ilium signature sound".into());
        }
        Hit::Back => app.onboarding_progress.wizard.back(),
        Hit::Continue => {
            if step == Step::SoundConfiguration
                && app.onboarding_progress.wizard.sound == Some(SoundChoice::Custom)
            {
                apply_studio_action(app, super::studio_ui::StudioAction::Save);
            }
            match app.onboarding_progress.wizard.advance() {
                Ok(Navigation::Finish) => {
                    app.onboarding_progress.finish();
                    app.onboarding_revision = app.onboarding_revision.wrapping_add(1);
                    app.onboarding = None;
                }
                Ok(Navigation::Moved) => {}
                Err(_) => {
                    app.status_message = Some("Choose an option to continue".into());
                    return;
                }
            }
        }
        Hit::Skip if matches!(step, Step::AiChoice | Step::AiConfiguration) => {
            app.onboarding_progress.choose_ai(AiChoice::Disabled);
            app.onboarding_revision = app.onboarding_revision.wrapping_add(1);
        }
        Hit::Skip if step == Step::Voice => {
            app.onboarding_progress.finish();
            app.onboarding_revision = app.onboarding_revision.wrapping_add(1);
            app.onboarding = None;
        }
        Hit::Skip if matches!(step, Step::SoundChoice | Step::SoundConfiguration) => {
            app.settings_select_sound_source(ilium_sound::SoundSourceKind::Muted);
            app.onboarding_progress.wizard.step = Step::KeyboardChoice;
        }
        Hit::Skip if step == Step::KeyboardChoice => app
            .onboarding_progress
            .wizard
            .choose_keyboard(KeyboardChoice::Custom),
        Hit::Skip if step == Step::KeyboardPractice => {
            app.onboarding_progress.wizard.step = Step::Voice
        }
        Hit::Skip => return,
        Hit::Choice(index) => match step {
            Step::AiChoice => {
                let (choice, provider) = match index {
                    0 => (AiChoice::Kilo, Provider::KiloGateway),
                    1 => (
                        AiChoice::Paid,
                        if matches!(
                            app.inference_settings.selected_provider,
                            Provider::OpenAi | Provider::Anthropic | Provider::OpenRouter
                        ) {
                            app.inference_settings.selected_provider
                        } else {
                            Provider::OpenAi
                        },
                    ),
                    2 => (AiChoice::Local, Provider::Ollama),
                    _ => return,
                };
                app.onboarding_progress.choose_ai(choice);
                app.onboarding_revision = app.onboarding_revision.wrapping_add(1);
                app.settings_select_inference_provider(provider);
            }
            Step::SoundChoice => {
                let choice = match index {
                    0 => SoundChoice::Bundled,
                    1 => SoundChoice::System,
                    2 => SoundChoice::Custom,
                    _ => return,
                };
                app.onboarding_progress.wizard.choose_sound(choice);
                app.settings_select_sound_source(match choice {
                    SoundChoice::Bundled => ilium_sound::SoundSourceKind::BundledChirping,
                    SoundChoice::System => ilium_sound::SoundSourceKind::SoundFile,
                    SoundChoice::Custom => ilium_sound::SoundSourceKind::Generated,
                });
            }
            Step::KeyboardChoice => {
                let choice = match index {
                    0 => {
                        app.settings_apply_keymap_preset(KeymapPreset::Tmux);
                        KeyboardChoice::Tmux
                    }
                    1 => {
                        app.settings_apply_keymap_preset(KeymapPreset::Screen);
                        KeyboardChoice::Screen
                    }
                    2 => KeyboardChoice::Custom,
                    _ => return,
                };
                app.onboarding_progress.wizard.choose_keyboard(choice);
            }
            Step::SoundConfiguration => {
                let events = ilium_sound::SoundEvent::ALL;
                if index == 0 {
                    app.settings_preview_sound();
                } else if index <= events.len() {
                    app.settings_toggle_sound_event(events[index - 1]);
                } else {
                    app.settings_select_sound_file(index - events.len() - 1);
                }
            }
            Step::AiConfiguration => {
                let Some((_, control)) = provider_rows(app).get(index).cloned() else {
                    return;
                };
                match control {
                    Control::Field(field) => app.settings_open_inference_field(field),
                    Control::PaidProvider => {
                        let providers =
                            [Provider::OpenAi, Provider::Anthropic, Provider::OpenRouter];
                        let index = providers
                            .iter()
                            .position(|provider| {
                                *provider == app.inference_settings.selected_provider
                            })
                            .unwrap_or(0);
                        app.settings_select_inference_provider(
                            providers[(index + 1) % providers.len()],
                        );
                    }
                    Control::Model => app.settings_adjust_kilo_gateway_model(1),
                    Control::OpenAiModel => app.settings_adjust_openai_model(1),
                    Control::Refresh => app.request_model_refresh(),
                    Control::Test => app.request_inference_test(),
                }
            }
            _ => {}
        },
    }
    if step != app.onboarding_progress.wizard.step && app.onboarding.is_some() {
        app.onboarding = Some(step_ui(app));
    }
    persist(app);
}

pub fn handle_event(app: &mut App, event: &Event) -> bool {
    if !owns_input(app) {
        return false;
    }
    let Event::Key(key) = event else {
        if matches!(event, Event::Resize(_, _)) {
            let area = Geometry::new(app.layout.screen_area).content;
            if let Some(ui) = &mut app.onboarding {
                ui.keyboard_ui.resize(area, &app.keybindings);
                ui.studio_ui.reveal_focus(area);
                ui.voice_ui.reveal_focus(area, &ui.voice_state);
            }
        }
        return true;
    };
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return true;
    }
    let step = app.onboarding_progress.wizard.step;
    let geometry = Geometry::new(app.layout.screen_area);
    if step == Step::KeyboardPractice
        && app
            .onboarding
            .as_ref()
            .is_some_and(|ui| ui.keyboard_editing)
    {
        // The editor owns key capture, while its ordinary controls keep the
        // wizard's advertised navigation available (including after Back).
        if app
            .onboarding
            .as_ref()
            .is_some_and(|ui| ui.keyboard_ui.pending_rebind.is_none())
        {
            let navigation = match key.code {
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Hit::Skip)
                }
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Hit::Continue)
                }
                KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => Some(Hit::Back),
                _ => None,
            };
            if let Some(target) = navigation {
                activate(app, target);
                return true;
            }
        }
        let request = app.onboarding.as_mut().and_then(|ui| {
            ui.keyboard_ui.handle_key(
                key,
                &app.keyboard_settings,
                &app.keybindings,
                geometry.content,
            )
        });
        if let Some(request) = request {
            apply_keyboard_request(app, request);
        }
        return true;
    }
    // Configured prefixes and an active demo prompt own their input before
    // wizard shortcuts. In ordinary terminals Ctrl+I/M alias Tab/Enter.
    if step == Step::KeyboardPractice
        && app.onboarding.as_ref().is_some_and(|ui| {
            ui.practice.interaction_pending()
                || crate::keymap::is_leader_key(key, app.keyboard_settings.shortcut_base)
                || crate::keymap::is_leader_key(key, app.keyboard_settings.navigation_shortcut_base)
        })
    {
        if let Some(ui) = &mut app.onboarding {
            ui.footer_focus = None;
            let elapsed = practice_elapsed(ui);
            ui.practice
                .handle_key(key, &app.keyboard_settings, &app.keybindings, elapsed);
        }
        return true;
    }
    if step == Step::KeyboardPractice
        && key.code == KeyCode::Char('e')
        && key.modifiers == KeyModifiers::NONE
        && app
            .onboarding
            .as_ref()
            .is_some_and(|ui| !ui.practice.interaction_pending())
    {
        if let Some(ui) = &mut app.onboarding {
            ui.keyboard_editing = true;
        }
        return true;
    }
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        activate(app, Hit::Skip);
        return true;
    }
    let practice_pending = app
        .onboarding
        .as_ref()
        .is_some_and(|ui| ui.practice.interaction_pending());
    if !practice_pending
        && matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
        && (key.modifiers.contains(KeyModifiers::CONTROL)
            || step == Step::KeyboardPractice
            || app
                .onboarding
                .as_ref()
                .is_some_and(|ui| ui.footer_focus.is_some()))
    {
        if let Some(ui) = &mut app.onboarding {
            ui.footer_focus = Some(match ui.footer_focus {
                None => 0,
                Some(index) => {
                    if key.code == KeyCode::BackTab {
                        (index + 2) % 3
                    } else {
                        (index + 1) % 3
                    }
                }
            });
        }
        return true;
    }
    if !practice_pending
        && app
            .onboarding
            .as_ref()
            .is_some_and(|ui| ui.footer_focus.is_some())
    {
        if key.code == KeyCode::Enter {
            let index = app
                .onboarding
                .as_ref()
                .and_then(|ui| ui.footer_focus)
                .unwrap_or(0);
            activate(
                app,
                match index {
                    0 => Hit::Back,
                    1 => Hit::Skip,
                    _ => Hit::Continue,
                },
            );
            return true;
        }
        if let Some(ui) = &mut app.onboarding {
            ui.footer_focus = None;
        }
    }
    let wizard_navigation_key = matches!(key.code, KeyCode::Esc)
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter)
        || (key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Left);
    if step == Step::Voice && !wizard_navigation_key {
        let action = if let Some(ui) = &mut app.onboarding {
            match key.code {
                KeyCode::Tab | KeyCode::Down => {
                    ui.voice_ui.move_focus(geometry.content, &ui.voice_state, 1);
                    None
                }
                KeyCode::BackTab | KeyCode::Up => {
                    ui.voice_ui
                        .move_focus(geometry.content, &ui.voice_state, -1);
                    None
                }
                KeyCode::PageUp => {
                    ui.voice_ui.scroll_by(geometry.content, &ui.voice_state, -8);
                    None
                }
                KeyCode::PageDown => {
                    ui.voice_ui.scroll_by(geometry.content, &ui.voice_state, 8);
                    None
                }
                KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right | KeyCode::Left => ui
                    .voice_ui
                    .focused_action(&ui.voice_state, &app.voice_settings),
                _ => None,
            }
        } else {
            None
        };
        if let Some(action) = action {
            apply_voice_action(app, action, if key.code == KeyCode::Left { -1 } else { 1 });
        }
        return true;
    }
    if step == Step::KeyboardPractice
        && !(key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter)
        && !(key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Left)
    {
        if let Some(ui) = &mut app.onboarding {
            let elapsed = practice_elapsed(ui);
            ui.practice
                .handle_key(key, &app.keyboard_settings, &app.keybindings, elapsed);
        }
        return true;
    }
    if step == Step::SoundConfiguration
        && app.onboarding_progress.wizard.sound == Some(SoundChoice::Custom)
        && !matches!(key.code, KeyCode::Esc)
        && !(key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter)
        && !(key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Left)
    {
        let action = if let Some(ui) = &mut app.onboarding {
            match key.code {
                KeyCode::Up | KeyCode::BackTab => {
                    ui.studio_ui.move_focus(geometry.content, -1);
                    None
                }
                KeyCode::Down | KeyCode::Tab => {
                    ui.studio_ui.move_focus(geometry.content, 1);
                    None
                }
                KeyCode::Left | KeyCode::Right => ui.studio.as_ref().and_then(|studio| {
                    ui.studio_ui
                        .adjust_focused(studio, if key.code == KeyCode::Left { -1 } else { 1 })
                }),
                KeyCode::Enter | KeyCode::Char(' ') => ui.studio_ui.focused_action(),
                KeyCode::PageUp => {
                    ui.studio_ui.scroll_by(geometry.content, -8);
                    None
                }
                KeyCode::PageDown => {
                    ui.studio_ui.scroll_by(geometry.content, 8);
                    None
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(action) = action {
            apply_studio_action(app, action);
        }
        return true;
    }
    let count = if is_choice(step) {
        3
    } else if step == Step::AiConfiguration {
        provider_rows(app).len()
    } else if step == Step::SoundConfiguration {
        1 + ilium_sound::SoundEvent::ALL.len()
            + if app.onboarding_progress.wizard.sound == Some(SoundChoice::System) {
                app.sound_discovery.sounds.len()
            } else {
                0
            }
    } else {
        0
    };
    match key.code {
        KeyCode::Esc => {
            app.onboarding = None;
            persist(app);
        }
        KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => activate(app, Hit::Back),
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
            activate(app, Hit::Continue)
        }
        KeyCode::Char('s')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                || matches!(step, Step::AiChoice | Step::Voice) =>
        {
            activate(app, Hit::Skip)
        }
        KeyCode::Enter => {
            if let Some(ui) = &app.onboarding {
                activate(app, Hit::Choice(ui.focus));
            }
        }
        KeyCode::Char(' ') if step == Step::SoundChoice => {
            if app.onboarding.as_ref().is_some_and(|ui| ui.focus == 0) {
                activate(app, Hit::DefaultPreview);
            }
        }
        KeyCode::Up | KeyCode::BackTab | KeyCode::Down | KeyCode::Tab => {
            if let Some(ui) = &mut app.onboarding {
                if count > 0 {
                    let previous = matches!(key.code, KeyCode::Up | KeyCode::BackTab);
                    ui.focus = if previous {
                        (ui.focus + count - 1) % count
                    } else {
                        (ui.focus + 1) % count
                    };
                    ui.hovered = None;
                    ui.scroll = if is_choice(step) && app.layout.screen_area.width < 90 {
                        Geometry::new(app.layout.screen_area).card_offset(step, ui.focus)
                    } else if is_choice(step) {
                        0
                    } else {
                        (ui.focus as u16 * 2).saturating_sub(
                            Geometry::new(app.layout.screen_area).content.height / 2,
                        )
                    };
                }
            }
        }
        _ => {}
    }
    true
}

pub fn handle_mouse(app: &mut App, mouse: MouseEvent) -> bool {
    if !owns_input(app) {
        return false;
    }
    let geometry = Geometry::new(app.layout.screen_area);
    let step = app.onboarding_progress.wizard.step;
    let scroll = app.onboarding.as_ref().map_or(0, |ui| ui.scroll);
    let position = Position::new(mouse.column, mouse.row);
    if step == Step::KeyboardPractice
        && app
            .onboarding
            .as_ref()
            .is_some_and(|ui| !ui.keyboard_editing)
        && keyboard_edit_area(geometry.content).contains(position)
        && mouse.kind == MouseEventKind::Down(MouseButton::Left)
    {
        if let Some(ui) = &mut app.onboarding {
            ui.keyboard_editing = true;
        }
        return true;
    }
    if step == Step::KeyboardPractice
        && app
            .onboarding
            .as_ref()
            .is_some_and(|ui| ui.keyboard_editing)
        && geometry.content.contains(position)
    {
        let request = if let Some(ui) = &mut app.onboarding {
            let keyboard_geometry = super::keyboard_ui::geometry(geometry.content, &ui.keyboard_ui);
            match mouse.kind {
                MouseEventKind::Moved => {
                    ui.keyboard_ui.hover(&keyboard_geometry, position);
                    None
                }
                MouseEventKind::Down(MouseButton::Left) => ui.keyboard_ui.click(
                    &keyboard_geometry,
                    position,
                    &app.keyboard_settings,
                    &app.keybindings,
                ),
                MouseEventKind::ScrollUp => {
                    ui.keyboard_ui.scroll_by(-3, &app.keybindings);
                    None
                }
                MouseEventKind::ScrollDown => {
                    ui.keyboard_ui.scroll_by(3, &app.keybindings);
                    None
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(request) = request {
            apply_keyboard_request(app, request);
        }
        return true;
    }
    if step == Step::Voice && geometry.content.contains(position) {
        let action = if let Some(ui) = &mut app.onboarding {
            match mouse.kind {
                MouseEventKind::Moved => {
                    ui.voice_ui.hovered = super::voice_ui::focus_at(
                        geometry.content,
                        &ui.voice_ui,
                        &ui.voice_state,
                        position,
                    );
                    None
                }
                MouseEventKind::Down(MouseButton::Left) => super::voice_ui::hit(
                    geometry.content,
                    &ui.voice_ui,
                    &ui.voice_state,
                    &app.voice_settings,
                    position,
                ),
                MouseEventKind::ScrollUp => {
                    ui.voice_ui
                        .scroll_at(geometry.content, &ui.voice_state, -3, position);
                    None
                }
                MouseEventKind::ScrollDown => {
                    ui.voice_ui
                        .scroll_at(geometry.content, &ui.voice_state, 3, position);
                    None
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(action) = action {
            apply_voice_action(app, action, 1);
        }
        return true;
    }
    if step == Step::SoundConfiguration
        && app.onboarding_progress.wizard.sound == Some(SoundChoice::Custom)
        && geometry.content.contains(position)
    {
        let action = if let Some(ui) = &mut app.onboarding {
            match mouse.kind {
                MouseEventKind::Moved => {
                    ui.studio_ui.hovered =
                        super::studio_ui::focus_at(geometry.content, &ui.studio_ui, position);
                    None
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(focus) =
                        super::studio_ui::focus_at(geometry.content, &ui.studio_ui, position)
                    {
                        ui.studio_ui.focus = focus;
                        ui.studio_ui.dragging = match super::studio_ui::focus_targets().get(focus) {
                            Some(super::studio_ui::StudioTarget::Slider(control)) => Some(*control),
                            _ => None,
                        };
                    }
                    super::studio_ui::hit(geometry.content, &ui.studio_ui, position)
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    super::studio_ui::drag(geometry.content, &ui.studio_ui, position)
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    ui.studio_ui.dragging = None;
                    None
                }
                MouseEventKind::ScrollUp => {
                    ui.studio_ui.scroll_by(geometry.content, -3);
                    None
                }
                MouseEventKind::ScrollDown => {
                    ui.studio_ui.scroll_by(geometry.content, 3);
                    None
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(action) = action {
            apply_studio_action(app, action);
        }
        return true;
    }
    if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
        if let Some(ui) = &mut app.onboarding {
            ui.studio_ui.dragging = None;
        }
    }
    let mut hit = geometry.hit_for_step(step, position, scroll);
    if !is_choice(step) && geometry.content.contains(position) {
        hit = if matches!(step, Step::AiConfiguration | Step::SoundConfiguration)
            && !(step == Step::AiConfiguration
                && position.y >= geometry.content.bottom().saturating_sub(2))
        {
            Some(Hit::Choice(
                usize::from(position.y - geometry.content.y + scroll) / 2,
            ))
        } else {
            None
        };
    }
    let provider_count = provider_rows(app).len() as u16;
    let sound_count =
        (1 + ilium_sound::SoundEvent::ALL.len() + app.sound_discovery.sounds.len()) as u16;
    match mouse.kind {
        MouseEventKind::Moved => {
            if let Some(ui) = &mut app.onboarding {
                ui.hovered = match hit {
                    Some(Hit::Choice(index)) => Some(index),
                    Some(Hit::DefaultPreview) => Some(0),
                    _ => None,
                };
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(hit) = hit {
                activate(app, hit);
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if let Some(ui) = &mut app.onboarding {
                ui.scroll = if mouse.kind == MouseEventKind::ScrollUp {
                    ui.scroll.saturating_sub(3)
                } else {
                    ui.scroll.saturating_add(3).min(if is_choice(step) {
                        geometry.max_scroll(step)
                    } else {
                        (if step == Step::AiConfiguration {
                            provider_count
                        } else {
                            sound_count
                        })
                        .saturating_mul(2)
                        .saturating_sub(
                            if step == Step::AiConfiguration {
                                geometry.content.height.saturating_sub(2)
                            } else {
                                geometry.content.height
                            },
                        )
                    })
                };
            }
        }
        _ => {}
    }
    true
}

fn practice_elapsed(ui: &WizardUi) -> std::time::Duration {
    ui.practice_started
        .map_or(std::time::Duration::ZERO, |started| started.elapsed())
}

pub fn is_animating(app: &App) -> bool {
    if app.onboarding.is_some() && app.inference_test_state.is_loading() {
        return true;
    }
    app.onboarding.as_ref().is_some_and(|ui| {
        app.onboarding_progress.wizard.step == Step::KeyboardPractice
            && ui
                .practice
                .is_animating(practice_elapsed(ui), app.ui_settings.motion_level)
    })
}

fn apply_studio_action(app: &mut App, action: super::studio_ui::StudioAction) {
    use super::studio_ui::StudioAction;
    let Some(ui) = &mut app.onboarding else {
        return;
    };
    let Some(studio) = &mut ui.studio else {
        return;
    };
    match action {
        StudioAction::SetValue(control, value) => control.set(&mut studio.draft.design, value),
        StudioAction::SetPosition {
            control,
            numerator,
            denominator,
        } => control.set_position(&mut studio.draft.design, numerator, denominator),
        StudioAction::Preset(preset) => studio.draft.design = preset.design(),
        StudioAction::ToggleEvent(event) => studio.draft.events.toggle(event),
        StudioAction::Play => {
            let settings = studio.draft.clone();
            app.queue_request(ilium_ipc::ClientRequest::PreviewSoundSettings { settings });
            app.status_message = Some("Playing your current sound design".into());
            return;
        }
        StudioAction::Save => {
            let settings = studio.draft.clone();
            app.status_message = Some("Sound design saved".into());
            app.apply_and_persist_sound_settings(settings);
            return;
        }
    }
    studio.changed();
    let settings = studio.draft.clone();
    app.status_message = Some("Sound design updated".into());
    app.apply_and_persist_sound_settings(settings);
}

fn render_sound_catalog(frame: &mut Frame, area: Rect, app: &App) {
    let Some(ui) = &app.onboarding else {
        return;
    };
    let mut labels = vec!["▶ Play selected sound".to_string()];
    labels.extend(ilium_sound::SoundEvent::ALL.iter().map(|event| {
        format!(
            "{} {}",
            if app.sound_settings.events.is_enabled(*event) {
                "[×]"
            } else {
                "[ ]"
            },
            event.label()
        )
    }));
    if app.onboarding_progress.wizard.sound == Some(SoundChoice::System) {
        labels.extend(app.sound_discovery.sounds.iter().map(|entry| {
            format!(
                "{} {}",
                if app.sound_settings.file.as_ref() == Some(&entry.path) {
                    "●"
                } else {
                    "○"
                },
                entry.display_name
            )
        }));
        if app.sound_discovery.sounds.is_empty() {
            labels.push("No system sound files found. Choose bundled or custom.".into());
        }
    }
    let lines = labels
        .into_iter()
        .enumerate()
        .flat_map(|(index, label)| {
            [
                Line::styled(
                    format!("  {label}"),
                    if ui.focus == index || ui.hovered == Some(index) {
                        Style::new()
                            .fg(Color::Rgb(13, 16, 23))
                            .bg(Color::Rgb(242, 188, 105))
                    } else {
                        Style::new().fg(Color::Rgb(224, 231, 244))
                    },
                ),
                Line::default(),
            ]
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).scroll((ui.scroll, 0)), area);
}

fn step_ui(app: &App) -> WizardUi {
    let mut ui = WizardUi::default();
    match app.onboarding_progress.wizard.step {
        Step::SoundConfiguration => {
            let mut sound = app.sound_settings.clone();
            if app.onboarding_progress.wizard.sound == Some(SoundChoice::Custom) {
                sound.source = ilium_sound::SoundSourceKind::Generated;
            }
            ui.studio = Some(super::studio::SoundStudio::new(sound));
        }
        Step::KeyboardPractice => {
            ui.practice_started = Some(std::time::Instant::now());
            ui.keyboard_editing =
                app.onboarding_progress.wizard.keyboard == Some(KeyboardChoice::Custom);
            if ui.keyboard_editing {
                ui.keyboard_ui.focus = 2;
            }
        }
        _ => {}
    }
    ui
}

fn apply_voice_action(app: &mut App, action: super::voice_ui::VoiceAction, direction: i32) {
    match action {
        super::voice_ui::VoiceAction::Configure(row) => {
            app.settings_adjust_voice_row(row, direction)
        }
        action => {
            let area = Geometry::new(app.layout.screen_area).content;
            if let Some(ui) = &mut app.onboarding {
                if action == super::voice_ui::VoiceAction::Test {
                    ui.voice_ui.scroll_by(area, &ui.voice_state, i32::MAX);
                }
                ui.voice_action = Some(action);
            }
        }
    }
}

fn apply_keyboard_request(app: &mut App, request: super::keyboard_ui::KeyboardRequest) {
    use super::keyboard_ui::KeyboardRequest;
    match request {
        KeyboardRequest::ApplyPreset(preset) => app.settings_apply_keymap_preset(preset),
        KeyboardRequest::SetGeneralPrefix(prefix) => app.settings_set_shortcut_base(prefix),
        KeyboardRequest::SetNavigationPrefix(prefix) => {
            app.settings_set_navigation_shortcut_base(prefix)
        }
        KeyboardRequest::AssignKey(action, key) => {
            app.status_message = Some("Shortcut updated. Try it in the playground.".into());
            app.settings_assign_key(action, key);
            if app
                .keybindings
                .iter()
                .any(|binding| binding.action == action && binding.key == key)
            {
                if let Some(ui) = &mut app.onboarding {
                    ui.keyboard_ui.finish_rebind();
                }
            }
        }
        KeyboardRequest::Close => {
            if let Some(ui) = &mut app.onboarding {
                ui.keyboard_editing = false;
            }
        }
        KeyboardRequest::Rebind(_) => app.status_message = None,
    }
}

pub fn resize(app: &mut App) {
    let geometry = Geometry::new(app.layout.screen_area);
    if let Some(ui) = &mut app.onboarding {
        ui.keyboard_ui.resize(geometry.content, &app.keybindings);
        ui.studio_ui.reveal_focus(geometry.content);
        ui.voice_ui.reveal_focus(geometry.content, &ui.voice_state);
        if is_choice(app.onboarding_progress.wizard.step) {
            ui.scroll = if geometry.content.width < 88 {
                geometry
                    .card_offset(app.onboarding_progress.wizard.step, ui.focus.min(2))
                    .min(geometry.max_scroll(app.onboarding_progress.wizard.step))
            } else {
                0
            };
        }
    }
}

fn keyboard_edit_area(content: Rect) -> Rect {
    let width = content.width.min(16);
    Rect::new(
        content.right().saturating_sub(width),
        content.y,
        width,
        content.height.min(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    use ilium_ipc::ClientRequest;

    fn app_at(step: Step) -> App {
        let mut app = App::new("setup-test".into(), std::env::temp_dir());
        app.onboarding_progress.begin();
        app.onboarding_progress.wizard.step = step;
        app.onboarding = Some(WizardUi::default());
        app.layout.screen_area = Rect::new(0, 0, 80, 24);
        app
    }

    #[test]
    fn openai_catalog_choice_in_wizard_persists_explicit_selection() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = app_at(Step::AiConfiguration);
        app.config_dir = Some(directory.path().to_path_buf());
        app.inference_settings.selected_provider = Provider::OpenAi;
        app.onboarding_progress.wizard.ai = Some(AiChoice::Paid);
        app.inference_settings.openai.model = "model-a".into();
        app.openai_models = vec!["model-a".into(), "model-b".into()];
        let index = provider_rows(&app)
            .iter()
            .position(|(_, control)| matches!(control, Control::OpenAiModel))
            .unwrap();
        app.onboarding.as_mut().unwrap().focus = index;
        handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert_eq!(app.inference_settings.openai.model, "model-b");
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .inference
                .openai
                .model,
            "model-b"
        );
        assert_eq!(app.onboarding_progress.wizard.step, Step::AiConfiguration);
        assert!(app.onboarding.is_some());
    }

    #[test]
    fn custom_keyboard_editor_keeps_wizard_navigation_available() {
        for (code, modifiers, expected) in [
            (KeyCode::Char('s'), KeyModifiers::CONTROL, Step::Voice),
            (KeyCode::Enter, KeyModifiers::CONTROL, Step::Voice),
            (KeyCode::Left, KeyModifiers::ALT, Step::KeyboardChoice),
        ] {
            let mut app = app_at(Step::KeyboardPractice);
            app.onboarding.as_mut().unwrap().keyboard_editing = true;
            let general_prefix = app.keyboard_settings.shortcut_base;
            let navigation_prefix = app.keyboard_settings.navigation_shortcut_base;
            let bindings = app.keybindings.clone();
            handle_event(&mut app, &Event::Key(KeyEvent::new(code, modifiers)));
            assert_eq!(app.onboarding_progress.wizard.step, expected);
            assert_eq!(app.keyboard_settings.shortcut_base, general_prefix);
            assert_eq!(
                app.keyboard_settings.navigation_shortcut_base,
                navigation_prefix
            );
            assert_eq!(app.keybindings, bindings);
        }
    }

    #[test]
    fn keyboard_binding_capture_keeps_ownership_of_navigation_keys() {
        use crate::keymap::Action;
        let mut app = app_at(Step::KeyboardPractice);
        let ui = app.onboarding.as_mut().unwrap();
        ui.keyboard_editing = true;
        ui.keyboard_ui.pending_rebind = Some(Action::NewTerminal);
        handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        );
        assert_eq!(app.onboarding_progress.wizard.step, Step::KeyboardPractice);
        assert_eq!(
            app.onboarding.as_ref().unwrap().keyboard_ui.pending_rebind,
            Some(Action::NewTerminal)
        );
    }

    #[test]
    fn custom_control_prefixes_own_wizard_shortcut_aliases() {
        use crate::keymap::{Action, BindingKey, ShortcutBase};
        for (base, code, modifiers) in [
            ("s", KeyCode::Char('s'), KeyModifiers::CONTROL),
            ("i", KeyCode::Tab, KeyModifiers::NONE),
            ("m", KeyCode::Enter, KeyModifiers::NONE),
        ] {
            let mut app = app_at(Step::KeyboardPractice);
            app.keyboard_settings.shortcut_base = ShortcutBase::parse(base).unwrap();
            app.onboarding.as_mut().unwrap().footer_focus = Some(1);
            let before = app
                .onboarding
                .as_ref()
                .unwrap()
                .practice
                .tree()
                .panes()
                .count();
            handle_event(&mut app, &Event::Key(KeyEvent::new(code, modifiers)));
            assert_eq!(app.onboarding_progress.wizard.step, Step::KeyboardPractice);
            assert!(app.onboarding.as_ref().unwrap().practice.prefix_pending());
            assert!(app.onboarding.as_ref().unwrap().footer_focus.is_none());
            let key = app
                .keybindings
                .iter()
                .find(|binding| binding.action == Action::NewTerminal)
                .unwrap()
                .key;
            let BindingKey::Character(character) = key else {
                panic!("preset terminal key is a character")
            };
            handle_event(
                &mut app,
                &Event::Key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
            );
            assert!(
                app.onboarding
                    .as_ref()
                    .unwrap()
                    .practice
                    .tree()
                    .panes()
                    .count()
                    > before
            );
            assert!(app.take_outbound_requests().is_empty());
        }
    }

    #[test]
    fn starting_a_new_binding_capture_clears_previous_feedback() {
        let mut app = app_at(Step::KeyboardPractice);
        app.status_message = Some("previous collision".into());
        apply_keyboard_request(
            &mut app,
            super::super::keyboard_ui::KeyboardRequest::Rebind(crate::keymap::Action::NewTerminal),
        );
        assert!(app.status_message.is_none());
    }

    #[test]
    fn resumed_step_initializes_studio_and_practice_clock() {
        let mut app = app_at(Step::SoundConfiguration);
        app.onboarding_progress.wizard.sound = Some(SoundChoice::Custom);
        open(&mut app, false);
        let studio = app.onboarding.as_ref().unwrap().studio.as_ref().unwrap();
        assert_eq!(studio.draft.source, ilium_sound::SoundSourceKind::Generated);
        app.onboarding_progress.wizard.step = Step::KeyboardPractice;
        open(&mut app, false);
        assert!(app.onboarding.as_ref().unwrap().practice_started.is_some());
    }

    #[test]
    fn every_step_renders_at_each_acceptance_size_without_exposing_credentials() {
        use ratatui::{backend::TestBackend, Terminal};
        for (width, height) in [(40, 16), (80, 24), (120, 40), (200, 60)] {
            for step in Step::ALL {
                let mut app = app_at(step);
                app.layout.screen_area = Rect::new(0, 0, width, height);
                app.onboarding_progress.wizard.sound = Some(SoundChoice::Custom);
                app.inference_settings.selected_provider = Provider::OpenAi;
                app.onboarding_progress.wizard.ai = Some(AiChoice::Paid);
                app.inference_settings.openai.api_key = "fixture-secret-do-not-render".into();
                app.voice_settings.api_key = "fixture-secret-do-not-render".into();
                app.onboarding = Some(step_ui(&app));
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| render(frame, frame.area(), &app))
                    .unwrap();
                let text = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                assert!(text.contains("ILIUM"), "{step:?} at{width}x{height}");
                assert!(!text.contains("fixture-secret-do-not-render"));
            }
        }
    }

    #[test]
    fn sound_save_failure_stays_visible_in_wizard_status_strip() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut app = app_at(Step::SoundConfiguration);
        app.status_message = Some("Could not save sound settings: fixture failure".into());
        app.onboarding_progress.wizard.sound = Some(SoundChoice::Custom);
        app.onboarding = Some(step_ui(&app));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app))
            .unwrap();
        assert!(terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("fixture failure"));
    }

    #[test]
    fn default_preview_keeps_selected_source_and_uses_full_settings() {
        let mut app = app_at(Step::SoundChoice);
        let original = app.sound_settings.clone();
        activate(&mut app, Hit::DefaultPreview);
        assert_eq!(app.sound_settings, original);
        assert!(
            matches!(app.take_outbound_requests().as_slice(),[ClientRequest::PreviewSoundSettings{settings}] if settings.source==ilium_sound::SoundSourceKind::BundledChirping)
        );
    }

    #[test]
    fn failed_sound_save_cannot_be_reported_as_success() {
        let mut app = app_at(Step::SoundChoice);
        activate(&mut app, Hit::Choice(2));
        let invalid_directory = tempfile::NamedTempFile::new().unwrap();
        app.config_dir = Some(invalid_directory.path().to_path_buf());
        apply_studio_action(&mut app, super::super::studio_ui::StudioAction::Save);
        assert!(app
            .status_message
            .as_deref()
            .unwrap()
            .starts_with("Could not save sound settings:"));
    }

    #[test]
    fn studio_edit_persists_real_design_and_preview_does_not_create_pane() {
        let mut app = app_at(Step::SoundChoice);
        activate(&mut app, Hit::Choice(2));
        app.take_outbound_requests();
        apply_studio_action(
            &mut app,
            super::super::studio_ui::StudioAction::SetValue(
                super::super::studio::SoundControl::Pitch,
                880,
            ),
        );
        assert_eq!(app.sound_settings.design.pitch_hz, 880);
        assert!(
            matches!(app.take_outbound_requests().as_slice(),[ClientRequest::UpdateSoundSettings{settings}] if settings.design.pitch_hz==880)
        );
        apply_studio_action(&mut app, super::super::studio_ui::StudioAction::Play);
        assert!(
            matches!(app.take_outbound_requests().as_slice(),[ClientRequest::PreviewSoundSettings{settings}] if settings.design.pitch_hz==880)
        );
    }

    #[test]
    fn practice_uses_real_prefix_without_live_outbound_requests() {
        let mut app = app_at(Step::KeyboardPractice);
        app.settings_apply_keymap_preset(KeymapPreset::Tmux);
        app.take_outbound_requests();
        let before = app.tree.clone();
        let prefix = Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert!(handle_event(&mut app, &prefix));
        assert!(app.onboarding.as_ref().unwrap().practice.prefix_pending());
        let key = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(handle_event(&mut app, &key));
        assert_eq!(app.tree, before);
        assert!(app.take_outbound_requests().is_empty());
    }

    #[test]
    fn practice_tab_reaches_finish_without_leaking_input() {
        let mut app = app_at(Step::KeyboardPractice);
        for _ in 0..3 {
            handle_event(
                &mut app,
                &Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            );
        }
        assert_eq!(app.onboarding.as_ref().unwrap().footer_focus, Some(2));
        handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert_eq!(app.onboarding_progress.wizard.step, Step::Voice);
        assert!(app.take_outbound_requests().is_empty());
    }
}
