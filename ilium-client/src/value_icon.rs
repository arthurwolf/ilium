//! Icon catalog reading controls retain the original picker and search results.
use crate::app::{App, IconPickerState, Mode};
use crate::value_control::{ControlKind, ControlSpec, ValueControl};
use crate::value_dialog_host::ValueDialogHost;
use ratatui::layout::Rect;

pub fn column_control(screen: Rect, picker: &IconPickerState) -> ValueControl {
    ValueControl::new(
        crate::settings_ui::icon_picker_layout(screen).view_switch_area,
        ControlSpec {
            kind: ControlKind::Choice,
            label: "View",
            value: picker.column_mode.label(),
            label_width: 5,
            previous_enabled: true,
            next_enabled: true,
            open_enabled: true,
        },
    )
}
impl App {
    pub(crate) fn begin_icon_column_dialog(&mut self) {
        let result = match &self.mode {
            Mode::Settings(state) => state
                .icon_picker
                .as_ref()
                .ok_or("The icon catalog is no longer open")
                .and_then(|picker| {
                    ValueDialogHost::icon_columns(picker)
                        .map_err(|_| "The icon catalog choices are unavailable")
                }),
            _ => Err("The icon catalog is no longer open"),
        };
        match result {
            Ok(host) => self.push_modal(Mode::ValueDialog(Box::new(host))),
            Err(error) => self.status_message = Some(error.into()),
        }
    }
    pub(crate) fn retained_icon_picker_mut(&mut self) -> Option<&mut IconPickerState> {
        match &mut self.mode {
            Mode::Settings(state) => state.icon_picker.as_mut(),
            Mode::ValueDialog(host) => match self.modal_stack.last_mut() {
                Some(Mode::Settings(state)) => state
                    .icon_picker
                    .as_mut()
                    .filter(|picker| host.matches_icon_picker(picker)),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{IconPickerColumnMode, IconPickerSearchStatus, SettingsState};
    use crate::icon_search_workers::IconSemanticSearchEvent;
    use crate::icon_settings::{semantic_picker_search_results, IconTarget};
    use crate::value_control::{ControlAction, PointerButton};
    use crate::value_dialog::ValueDialogState;
    use ratatui::layout::Position;

    #[test]
    fn both_layouts_are_complete_and_reopened_picker_is_rejected() {
        let mut picker = IconPickerState::new(IconTarget::Terminal);
        picker.search_query = "authored query".into();
        picker.selected_entry = 3;
        let host = ValueDialogHost::icon_columns(&picker).unwrap();
        let ValueDialogState::Choice(dialog) = &host.dialog else {
            panic!("choice catalog")
        };
        assert_eq!(dialog.options().len(), IconPickerColumnMode::ALL.len());
        let mut state = SettingsState {
            icon_picker: Some(picker),
            ..SettingsState::default()
        };
        host.apply_icon_column_choice(&mut state, "SingleColumn", Rect::new(0, 0, 80, 30))
            .unwrap();
        let retained = state.icon_picker.as_ref().unwrap();
        assert_eq!(retained.column_mode, IconPickerColumnMode::SingleColumn);
        assert_eq!(retained.search_query, "authored query");
        assert_eq!(retained.selected_entry, 3);
        state.icon_picker = Some(IconPickerState::new(IconTarget::Terminal));
        assert!(host
            .apply_icon_column_choice(&mut state, "SingleColumn", Rect::new(0, 0, 80, 30))
            .is_err());
        assert_eq!(
            state.icon_picker.unwrap().column_mode,
            IconPickerColumnMode::MultiColumn
        );
    }

    #[test]
    fn responsive_chrome_has_exact_forward_reverse_and_catalog_hits() {
        let picker = IconPickerState::new(IconTarget::Terminal);
        for width in [40, 80, 140] {
            let control = column_control(Rect::new(0, 0, width, 30), &picker);
            let g = control.geometry();
            assert!(g.value.width > 0 && g.open.width > 0);
            for (area, button, action) in [
                (
                    g.previous,
                    PointerButton::Left,
                    ControlAction::PreviousChoice,
                ),
                (g.next, PointerButton::Left, ControlAction::NextChoice),
                (g.open, PointerButton::Left, ControlAction::OpenChoices),
                (g.value, PointerButton::Left, ControlAction::NextChoice),
                (g.value, PointerButton::Right, ControlAction::PreviousChoice),
            ] {
                assert_eq!(
                    control.hit(Position::new(area.x, area.y), button),
                    Some(action)
                );
            }
            assert_eq!(
                control.hit(Position::new(g.label.x, g.label.y), PointerButton::Left),
                None
            );
        }
    }

    #[test]
    fn real_settings_renderer_paints_layout_chrome_at_hit_geometry() {
        use ratatui::{backend::TestBackend, Terminal};
        let directory = tempfile::tempdir().unwrap();
        let app = App::new("icon-layout-render".into(), directory.path().into());
        let state = SettingsState {
            icon_picker: Some(IconPickerState::new(IconTarget::Terminal)),
            ..Default::default()
        };
        for width in [40, 80, 140] {
            let screen = Rect::new(0, 0, width, 30);
            let g = column_control(screen, state.icon_picker.as_ref().unwrap()).geometry();
            let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
            terminal
                .draw(|frame| crate::settings_ui::render(frame, screen, &app, &state))
                .unwrap();
            for (rect, glyph) in [(g.previous, "←"), (g.open, "+"), (g.next, "→")] {
                assert_eq!(
                    terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                    glyph
                );
            }
        }
    }

    #[test]
    fn icon_assignment_rows_render_exact_chrome_with_authored_glyphs_and_scroll() {
        use ratatui::{backend::TestBackend, Terminal};
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("icon-assignment-render".into(), directory.path().into());
        let targets = crate::agent_monitoring::general_icon_targets();
        app.ui_settings.icons.set(targets[0], "☀️".into());
        for width in [60, 100, 170] {
            for scroll in [0, 3] {
                let screen = Rect::new(0, 0, width, 45);
                let state = SettingsState {
                    tab: crate::app::SettingsTab::Icons,
                    scroll,
                    selected_row: usize::from(scroll),
                    ..Default::default()
                };
                let mut area =
                    crate::settings_ui::compute_layout_for_mode(screen, &app, &state).content_area;
                let instruction_height = crate::instruction_settings::panel_height(state.tab, area);
                area.y += instruction_height;
                area.height = area.height.saturating_sub(instruction_height);
                let index = usize::from(scroll);
                let control = crate::settings_ui::icon_assignment_control(
                    area,
                    scroll,
                    index,
                    app.ui_settings.icons.glyph(targets[index]),
                )
                .unwrap();
                let g = control.geometry();
                let mut terminal = Terminal::new(TestBackend::new(width, 45)).unwrap();
                terminal
                    .draw(|frame| crate::settings_ui::render(frame, screen, &app, &state))
                    .unwrap();
                for (rect, glyph) in [(g.previous, "←"), (g.open, "+"), (g.next, "→")] {
                    assert_eq!(
                        terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                        glyph
                    );
                }
                assert_eq!(
                    control.hit(Position::new(g.value.x, g.value.y), PointerButton::Left),
                    Some(ControlAction::NextChoice)
                );
                assert_eq!(
                    control.hit(Position::new(g.value.x, g.value.y), PointerButton::Right),
                    Some(ControlAction::PreviousChoice)
                );
                assert_eq!(
                    control.hit(Position::new(g.label.x, g.label.y), PointerButton::Left),
                    None
                );
                assert!(crate::settings_ui::icons_table_hit(
                    area,
                    scroll,
                    Position::new(g.label.x, g.label.y)
                )
                .is_none());
                if scroll > 0 {
                    assert!(
                        crate::settings_ui::icon_assignment_control(area, scroll, 0, "☀️")
                            .is_none()
                    );
                }
            }
        }
    }

    #[test]
    fn matching_search_reply_reaches_retained_picker_and_stale_revision_is_ignored() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("icon-layout-reply".into(), project.path().into());
        let mut picker = IconPickerState::new(IconTarget::Terminal);
        picker.search_query = "rocket".into();
        let request = picker.begin_semantic_search().unwrap();
        app.mode = Mode::Settings(SettingsState {
            icon_picker: Some(picker),
            ..SettingsState::default()
        });
        app.begin_icon_column_dialog();
        assert!(matches!(app.mode, Mode::ValueDialog(_)));
        app.apply_icon_semantic_search_event(IconSemanticSearchEvent::Results {
            revision: request.revision.wrapping_sub(1),
            results: semantic_picker_search_results(Vec::new()),
        });
        assert!(matches!(
            app.retained_icon_picker_mut().unwrap().search_status,
            IconPickerSearchStatus::Searching
        ));
        app.apply_icon_semantic_search_event(IconSemanticSearchEvent::Results {
            revision: request.revision,
            results: semantic_picker_search_results(Vec::new()),
        });
        let retained = app.retained_icon_picker_mut().unwrap();
        assert_eq!(retained.search_query, "rocket");
        assert!(matches!(
            retained.search_status,
            IconPickerSearchStatus::Browse
        ));
        assert!(matches!(app.mode, Mode::ValueDialog(_)));
        let Mode::Settings(state) = app.modal_stack.last_mut().unwrap() else {
            panic!("retained settings")
        };
        state.icon_picker = Some(IconPickerState::new(IconTarget::Terminal));
        assert!(app.retained_icon_picker_mut().is_none());
    }
}
