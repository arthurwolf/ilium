//! Rendered-frame captures for the whole-UI polish pass. Each test draws one
//! production surface into a `TestBackend` and saves the cell buffer through
//! `ui_capture`. The artifacts are written only when the ni-build job or
//! `ILIUM_POLISH_RENDER_DIR` names a capture directory.

use std::time::Duration;

use ilium_core::{PaneContentKind, ROOT_ID};
use ratatui::{backend::TestBackend, Terminal};

use crate::app::{App, SettingsState, SettingsTab};
use crate::settings_ui;

const POLISH_CAPTURE_VARIABLE: &str = "ILIUM_POLISH_RENDER_DIR";

/// Terminal sizes the polish pass reviews: the default, a common laptop
/// width, and the narrowest size the chrome must still survive.
const POLISH_SIZES: [(u16, u16); 3] = [(140, 40), (100, 30), (60, 20)];

fn synthetic_app(label: &str) -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new(label.into(), directory.path().to_path_buf());
    let group = app.tree.add_group(ROOT_ID, "work").unwrap();
    app.tree
        .add_pane(group, "shell", PaneContentKind::Terminal)
        .unwrap();
    (directory, app)
}

#[test]
fn polish_main_ui_frames_at_review_sizes() {
    for (width, height) in POLISH_SIZES {
        let (_directory, mut app) = synthetic_app("polish-main");
        app.set_screen_area(ratatui::layout::Rect::new(0, 0, width, height));
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::draw_at(frame, &mut app, Duration::ZERO);
            })
            .unwrap();
        crate::ui_capture::save_with_env(
            &format!("polish-main-{width}x{height}"),
            &terminal,
            POLISH_CAPTURE_VARIABLE,
        );
    }
}

#[test]
fn polish_settings_tabs_at_review_sizes() {
    let (_directory, app) = synthetic_app("polish-settings");
    for (width, height) in POLISH_SIZES {
        for tab in SettingsTab::ALL {
            let state = SettingsState {
                tab,
                ..SettingsState::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| settings_ui::render(frame, frame.area(), &app, &state))
                .unwrap();
            crate::ui_capture::save_with_env(
                &format!("polish-settings-{tab:?}-{width}x{height}"),
                &terminal,
                POLISH_CAPTURE_VARIABLE,
            );
        }
    }
}
