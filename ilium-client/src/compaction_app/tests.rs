//! Flow tests of the Optimization tab: keyboard and mouse through the real
//! settings handlers, real scans of fixture transcripts, and apply/revert
//! against temporary configuration files (never the real `~/.claude` or
//! `~/.codex`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use super::*;
use crate::compaction_scan::fixtures::{write_claude_corpus, write_codex_corpus};
use crate::compaction_scan::ScanView;

const CODEX_CURRENT: u64 = 123_456;
const CLAUDE_CURRENT: u64 = 777_000;

/// The shared test execution bank admits only a few 160 MiB scans at once, so
/// the fixtures take turns (the scan tests of other modules run beside them).
static FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    fn new(screen: Rect) -> Self {
        let turn = FIXTURES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let claude_dir = home.join(".claude");
        let codex_dir = home.join(".codex");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::create_dir_all(&codex_dir).unwrap();
        let paths = ConfigPaths::with_roots(claude_dir, codex_dir, root.path().join("locks"));
        let mut app = App::new("optimization-test".to_owned(), root.path().to_owned());
        app.set_screen_area(screen);
        app.optimization.paths = OptimizationPaths {
            home: Some(home),
            cache_dir: Some(root.path().join("cache")),
            record_dir: Some(root.path().join("data")),
            config: Some(paths),
        };
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Optimization,
            ..SettingsState::default()
        });
        Self {
            root,
            app,
            _turn: turn,
        }
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn config_path(&self, agent: AgentKind) -> PathBuf {
        let home = self.home();
        match agent {
            AgentKind::Codex => home.join(".codex/config.toml"),
            AgentKind::ClaudeCode => home.join(".claude/settings.json"),
        }
    }

    fn write_config(&self, agent: AgentKind, text: &str) {
        std::fs::write(self.config_path(agent), text).unwrap();
    }

    fn read_config(&self, agent: AgentKind) -> String {
        std::fs::read_to_string(self.config_path(agent)).unwrap()
    }

    fn with_codex_config(self) -> Self {
        self.write_config(
            AgentKind::Codex,
            &format!("# my settings\nmodel_auto_compact_token_limit = {CODEX_CURRENT}\n\n[projects.x]\ntrust = true\n"),
        );
        self
    }

    fn state(&self) -> &SettingsState {
        match &self.app.mode {
            Mode::Settings(state) => state,
            _ => panic!("the settings screen should be open"),
        }
    }

    fn content(&self) -> Rect {
        crate::settings_ui::compute_layout_for_mode(
            self.app.layout.screen_area,
            &self.app,
            self.state(),
        )
        .content_area
    }

    fn press(&mut self, code: KeyCode) {
        crate::keys::handle_event(
            &mut self.app,
            Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
        );
    }

    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) {
        crate::mouse::handle_mouse_event(
            &mut self.app,
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::empty(),
            },
        );
    }

    fn click(&mut self, column: u16, row: u16) {
        self.mouse(MouseEventKind::Down(MouseButton::Left), column, row);
    }

    fn wait_for_scan(&mut self, agent: AgentKind) {
        let deadline = Instant::now() + Duration::from_secs(120);
        while self.app.compaction_optimizer.is_scanning(agent) {
            assert!(Instant::now() < deadline, "scan did not finish");
            self.app.tick_compaction(Instant::now());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Presses `s` until the scan is admitted: other tests may hold the shared
    /// bank's budget for a moment.
    fn start_scan_with_key(&mut self) {
        self.start_scan_with(KeyCode::Char('s'));
    }

    fn start_scan_with(&mut self, code: KeyCode) {
        let agent = self.app.optimization.selected_agent;
        for _ in 0..400 {
            self.press(code);
            if self.app.compaction_optimizer.is_scanning(agent) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("the scan was never admitted");
    }

    /// Scans `agent` through the `s` key and waits for the report.
    fn scan(&mut self, agent: AgentKind) {
        match agent {
            AgentKind::Codex => write_codex_corpus(&self.home(), 8),
            AgentKind::ClaudeCode => write_claude_corpus(&self.home(), 8),
        }
        self.app.optimization.selected_agent = agent;
        self.start_scan_with_key();
        assert!(self.app.compaction_optimizer.is_scanning(agent));
        self.wait_for_scan(agent);
        assert!(
            matches!(
                self.app.compaction_optimizer.view(agent),
                ScanView::Ready(_)
            ),
            "ready report"
        );
    }

    fn recommended_value(&self, agent: AgentKind) -> u64 {
        u64::from(
            self.app
                .compaction_optimizer
                .report(agent)
                .and_then(|report| report.recommendation.as_ref())
                .expect("a recommendation")
                .setting_value,
        )
    }

    fn pending(&self) -> Option<&PendingApply> {
        self.app.optimization.pending_apply.as_ref()
    }

    fn note(&self, agent: AgentKind) -> Note {
        self.app
            .optimization
            .panel(agent)
            .note
            .clone()
            .expect("a note")
    }
}

fn screen() -> Rect {
    Rect::new(0, 0, 160, 50)
}

fn rendered(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|column| buffer[(column, row)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn draw(fixture: &Fixture) -> String {
    let area = fixture.app.layout.screen_area;
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal
        .draw(|frame| {
            crate::settings_ui::render(frame, frame.area(), &fixture.app, fixture.state());
        })
        .unwrap();
    rendered(&terminal)
}

// ------------------------------------------------------------- opening

#[test]
fn opening_the_tab_reads_the_current_settings_and_never_starts_a_scan() {
    let mut fixture = Fixture::new(screen()).with_codex_config();
    fixture.write_config(AgentKind::ClaudeCode, "{\"other\": 1}");
    assert_eq!(
        fixture.app.optimization.panel(AgentKind::Codex).current,
        CurrentSetting::Unknown
    );
    fixture.app.tick_compaction(Instant::now());
    let codex = fixture.app.optimization.panel(AgentKind::Codex);
    assert_eq!(codex.current, CurrentSetting::Value(CODEX_CURRENT));
    assert_eq!(
        codex.config_path.as_deref(),
        Some(
            fixture
                .config_path(AgentKind::Codex)
                .canonicalize()
                .unwrap()
                .as_path()
        )
    );
    assert_eq!(
        fixture
            .app
            .optimization
            .panel(AgentKind::ClaudeCode)
            .current,
        CurrentSetting::NotSet
    );
    for agent in OPTIMIZATION_AGENTS {
        assert!(!fixture.app.compaction_optimizer.is_scanning(agent));
        assert!(matches!(
            fixture.app.compaction_optimizer.view(agent),
            ScanView::Idle
        ));
    }
    let text = draw(&fixture);
    assert!(text.contains("Current model_auto_compact_token_limit: 123,456"));
    assert!(text.contains("[ Codex ]") || text.contains(" Codex "));
    assert!(fixture.app.compaction_next_poll(Instant::now()).is_none());
}

#[test]
fn a_missing_configuration_file_reads_as_not_set() {
    let mut fixture = Fixture::new(screen());
    fixture.app.tick_compaction(Instant::now());
    assert_eq!(
        fixture.app.optimization.panel(AgentKind::Codex).current,
        CurrentSetting::NoFile
    );
    assert!(draw(&fixture).contains("does not exist"));
}

#[test]
fn the_current_values_are_read_again_each_time_the_tab_becomes_visible() {
    let mut fixture = Fixture::new(screen()).with_codex_config();
    fixture.app.tick_compaction(Instant::now());
    fixture.write_config(
        AgentKind::Codex,
        "model_auto_compact_token_limit = 150000\n",
    );
    // Still visible: no re-read.
    fixture.app.tick_compaction(Instant::now());
    assert_eq!(
        fixture.app.optimization.panel(AgentKind::Codex).current,
        CurrentSetting::Value(CODEX_CURRENT)
    );
    // Leave through the Tab key, come back through BackTab.
    fixture.press(KeyCode::Tab);
    assert_eq!(fixture.state().tab, SettingsTab::RemoteCompaction);
    fixture.press(KeyCode::BackTab);
    assert_eq!(fixture.state().tab, SettingsTab::Optimization);
    assert_eq!(
        fixture.app.optimization.panel(AgentKind::Codex).current,
        CurrentSetting::Value(150_000)
    );
}

#[test]
fn scan_settings_carry_the_current_value_and_the_cost_tab_price_overrides() {
    let mut fixture = Fixture::new(screen()).with_codex_config();
    fixture.app.cost_settings.prices.insert(
        "gpt-test".to_owned(),
        crate::cost_model::ModelPrice {
            input: 1.0,
            output: 2.0,
            cache_read: 3.0,
            cache_write: 4.0,
        },
    );
    fixture.app.refresh_optimization_settings();
    let settings = fixture.app.optimization_scan_settings(AgentKind::Codex);
    assert_eq!(settings.current_setting_value, Some(CODEX_CURRENT));
    assert_eq!(settings.price_table.lookup("gpt-test").unwrap().input, 1.0);
    assert_eq!(
        fixture
            .app
            .optimization_scan_settings(AgentKind::ClaudeCode)
            .current_setting_value,
        None
    );
}

// ---------------------------------------------------- keys: agent, scroll

#[test]
fn left_right_and_clicks_switch_the_agent_and_reset_the_scroll() {
    let mut fixture = Fixture::new(screen());
    assert_eq!(fixture.app.optimization.selected_agent, AgentKind::Codex);
    if let Mode::Settings(state) = &mut fixture.app.mode {
        state.scroll = 7;
    }
    fixture.press(KeyCode::Right);
    assert_eq!(
        fixture.app.optimization.selected_agent,
        AgentKind::ClaudeCode
    );
    assert_eq!(fixture.state().scroll, 0);
    fixture.press(KeyCode::Right);
    assert_eq!(
        fixture.app.optimization.selected_agent,
        AgentKind::Codex,
        "wraps"
    );
    fixture.press(KeyCode::Char(']'));
    assert_eq!(
        fixture.app.optimization.selected_agent,
        AgentKind::ClaudeCode
    );
    fixture.press(KeyCode::Char('['));
    assert_eq!(fixture.app.optimization.selected_agent, AgentKind::Codex);

    let content = fixture.content();
    let header = compaction_ui::header(AgentKind::Codex, content.width);
    let claude = header
        .buttons
        .iter()
        .find(|button| button.action == Action::SelectAgent(AgentKind::ClaudeCode))
        .unwrap();
    fixture.click(content.x + claude.x_start + 1, content.y);
    assert_eq!(
        fixture.app.optimization.selected_agent,
        AgentKind::ClaudeCode
    );
    // The agents keep separate pages.
    assert!(draw(&fixture).contains("COMPACTION OPTIMIZER - CLAUDE CODE"));
}

#[test]
fn the_report_scrolls_with_arrows_pages_home_end_and_the_wheel_within_bounds() {
    let mut fixture = Fixture::new(Rect::new(0, 0, 160, 30)).with_codex_config();
    fixture.scan(AgentKind::Codex);
    let content = fixture.content();
    let max = compaction_ui::max_scroll(&fixture.app, content);
    assert!(max > 10, "the report is longer than the screen: {max}");
    let page = compaction_ui::page_height(content);

    fixture.press(KeyCode::Down);
    assert_eq!(fixture.state().scroll, 1);
    fixture.press(KeyCode::Char('j'));
    assert_eq!(fixture.state().scroll, 2);
    fixture.press(KeyCode::Up);
    assert_eq!(fixture.state().scroll, 1);
    fixture.press(KeyCode::PageDown);
    assert_eq!(fixture.state().scroll, 1 + page);
    fixture.press(KeyCode::PageUp);
    assert_eq!(fixture.state().scroll, 1);
    fixture.press(KeyCode::End);
    assert_eq!(fixture.state().scroll, max);
    fixture.press(KeyCode::Down);
    assert_eq!(fixture.state().scroll, max, "never past the end");
    fixture.press(KeyCode::Home);
    assert_eq!(fixture.state().scroll, 0);

    let inside = (content.x + 3, content.y + 5);
    fixture.mouse(MouseEventKind::ScrollDown, inside.0, inside.1);
    let after_wheel = fixture.state().scroll;
    assert!(after_wheel > 0 && after_wheel <= max);
    fixture.mouse(MouseEventKind::ScrollUp, inside.0, inside.1);
    assert_eq!(fixture.state().scroll, 0);
    for _ in 0..200 {
        fixture.mouse(MouseEventKind::ScrollDown, inside.0, inside.1);
    }
    assert_eq!(fixture.state().scroll, max);
}

// ----------------------------------------------------------------- scanning

#[test]
fn enter_scans_then_the_page_shows_the_report_with_the_apply_button_on_top() {
    let mut fixture = Fixture::new(screen()).with_codex_config();
    fixture.app.tick_compaction(Instant::now());
    assert_eq!(
        compaction_ui::primary_action(&fixture.app),
        Some(Action::Scan)
    );
    write_codex_corpus(&fixture.home(), 8);
    fixture.start_scan_with(KeyCode::Enter);
    assert!(fixture.app.compaction_next_poll(Instant::now()).is_some());
    assert_eq!(compaction_ui::primary_action(&fixture.app), None);
    // The page of a running scan is the progress page.
    let progress = draw(&fixture);
    assert!(
        progress.contains("SCANNING CODEX SESSIONS") || progress.contains("RECOMMENDATION"),
        "progress or an already finished report"
    );
    fixture.wait_for_scan(AgentKind::Codex);

    let report = fixture
        .app
        .compaction_optimizer
        .report(AgentKind::Codex)
        .expect("report");
    assert!(report.has_recommendation());
    // The scan received the current setting: the comparison has its row.
    assert!(report
        .comparison
        .iter()
        .any(|row| row.kind == crate::compaction_report::ComparisonKind::CurrentSetting));
    let text = draw(&fixture);
    assert!(text.contains("RECOMMENDATION"));
    assert!(text.contains("Apply to Codex"));
    assert!(text.contains("Re-scan"));
    assert_eq!(
        compaction_ui::primary_action(&fixture.app),
        Some(Action::Apply)
    );
    assert!(fixture.app.compaction_next_poll(Instant::now()).is_none());
}

#[test]
fn escape_cancels_a_running_scan_instead_of_closing_settings_and_keeps_the_report() {
    let mut fixture = Fixture::new(screen()).with_codex_config();
    fixture.scan(AgentKind::Codex);
    fixture.start_scan_with_key();
    fixture.press(KeyCode::Esc);
    assert!(
        matches!(fixture.app.mode, Mode::Settings(_)),
        "Esc cancelled the scan, it did not close Settings"
    );
    fixture.wait_for_scan(AgentKind::Codex);
    assert!(matches!(
        fixture.app.compaction_optimizer.view(AgentKind::Codex),
        ScanView::Cancelled | ScanView::Ready(_)
    ));
    assert!(
        fixture
            .app
            .compaction_optimizer
            .report(AgentKind::Codex)
            .is_some(),
        "the previous report survives a cancelled scan"
    );
    // With nothing running, Esc closes Settings as before.
    fixture.press(KeyCode::Esc);
    assert!(matches!(fixture.app.mode, Mode::Normal));
}

#[test]
fn a_second_scan_cannot_start_while_one_runs() {
    let mut fixture = Fixture::new(screen());
    write_codex_corpus(&fixture.home(), 3);
    fixture.start_scan_with_key();
    assert!(!fixture.app.optimization_start_scan(AgentKind::Codex));
    fixture.wait_for_scan(AgentKind::Codex);
}

#[test]
fn the_cancel_button_and_c_key_stop_a_running_scan() {
    let mut fixture = Fixture::new(screen());
    write_codex_corpus(&fixture.home(), 3);
    fixture.start_scan_with_key();
    fixture.press(KeyCode::Char('c'));
    assert!(matches!(fixture.app.mode, Mode::Settings(_)));
    fixture.wait_for_scan(AgentKind::Codex);
    assert!(matches!(
        fixture.app.compaction_optimizer.view(AgentKind::Codex),
        ScanView::Cancelled | ScanView::Ready(_)
    ));
}

// ------------------------------------------------------- apply and revert

#[test]
fn apply_confirm_writes_one_key_keeps_a_record_and_revert_restores_it() {
    let mut fixture = Fixture::new(screen()).with_codex_config();
    fixture.scan(AgentKind::Codex);
    let recommended = fixture.recommended_value(AgentKind::Codex);
    assert_ne!(recommended, CODEX_CURRENT);
    let before = fixture.read_config(AgentKind::Codex);

    // Enter opens the confirmation; nothing is written yet.
    fixture.press(KeyCode::Enter);
    let pending = fixture.pending().expect("confirmation");
    assert_eq!(pending.agent, AgentKind::Codex);
    assert_eq!(pending.plan.old_value, Some(CODEX_CURRENT));
    assert_eq!(pending.plan.new_value, recommended);
    let card_is_extrapolated = fixture
        .app
        .compaction_optimizer
        .report(AgentKind::Codex)
        .and_then(|report| report.recommendation.as_ref())
        .map(|card| card.extrapolated);
    assert_eq!(
        pending.extrapolated.is_some(),
        card_is_extrapolated.unwrap()
    );
    assert_eq!(fixture.read_config(AgentKind::Codex), before);
    let text = draw(&fixture);
    assert!(text.contains("Apply to Codex?"));
    assert!(text.contains("model_auto_compact_token_limit"));
    assert!(text.contains(&format!("123,456 -> {}", group_thousands(recommended))));
    assert!(text.contains("config.toml"));
    assert!(text.contains("NEW Codex sessions"));
    assert!(text.contains("Cancel") && text.contains("Apply"));

    // Esc cancels and keeps Settings open.
    fixture.press(KeyCode::Esc);
    assert!(fixture.pending().is_none());
    assert!(matches!(fixture.app.mode, Mode::Settings(_)));
    assert_eq!(fixture.read_config(AgentKind::Codex), before);

    // Confirm with Y.
    fixture.press(KeyCode::Char('a'));
    assert!(fixture.pending().is_some());
    fixture.press(KeyCode::Char('y'));
    assert!(fixture.pending().is_none());
    let note = fixture.note(AgentKind::Codex);
    assert_eq!(note.tone, NoteTone::Success, "{}", note.text);
    assert!(note.text.contains("Only new Codex sessions use it"));
    let after = fixture.read_config(AgentKind::Codex);
    assert!(after.contains(&format!("model_auto_compact_token_limit = {recommended}")));
    assert!(after.starts_with("# my settings\n"), "comments are kept");
    assert!(after.contains("[projects.x]\ntrust = true"));
    let panel = fixture.app.optimization.panel(AgentKind::Codex);
    assert_eq!(panel.current, CurrentSetting::Value(recommended));
    assert_eq!(
        panel.record.as_ref().map(|record| record.previous_value),
        Some(Some(CODEX_CURRENT))
    );
    let text = draw(&fixture);
    assert!(text.contains("Revert model_auto_compact_token_limit to 123,456"));

    // Revert with the key.
    fixture.press(KeyCode::Char('r'));
    let note = fixture.note(AgentKind::Codex);
    assert_eq!(note.tone, NoteTone::Success, "{}", note.text);
    assert!(note.text.contains("Reverted"));
    assert_eq!(fixture.read_config(AgentKind::Codex), before);
    let panel = fixture.app.optimization.panel(AgentKind::Codex);
    assert!(panel.record.is_none());
    assert_eq!(panel.current, CurrentSetting::Value(CODEX_CURRENT));
    assert!(!draw(&fixture).contains("Revert model_auto_compact_token_limit"));
}

#[test]
fn the_e_key_applies_the_extrapolated_alternative_through_the_same_confirmation() {
    let mut fixture = Fixture::new(screen()).with_codex_config();
    fixture.scan(AgentKind::Codex);
    let alternative = fixture
        .app
        .compaction_optimizer
        .report(AgentKind::Codex)
        .and_then(|report| report.recommendation.as_ref())
        .and_then(|card| card.extrapolated_alternative.clone());
    fixture.press(KeyCode::Char('e'));
    match alternative {
        Some(alternative) => {
            let pending = fixture.pending().expect("the same confirmation opens");
            assert_eq!(pending.plan.new_value, alternative.setting_value);
            assert_eq!(
                pending.extrapolated.as_deref(),
                Some(alternative.observed_support_text.as_str())
            );
            let lines = compaction_ui::modal_lines_for_tests(pending, 98).join(" ");
            assert!(lines.contains("EXTRAPOLATED"), "{lines}");
            fixture.press(KeyCode::Esc);
            assert!(fixture.pending().is_none());
        }
        None => assert!(fixture.pending().is_none(), "nothing to apply"),
    }
}

#[test]
fn revert_restores_the_previous_value_and_refuses_after_an_outside_edit() {
    let mut fixture = Fixture::new(screen()).with_codex_config();
    fixture.scan(AgentKind::Codex);
    fixture.press(KeyCode::Char('a'));
    fixture.press(KeyCode::Enter);
    assert!(fixture
        .app
        .optimization
        .panel(AgentKind::Codex)
        .record
        .is_some());
    // Someone else edits the value we wrote.
    fixture.write_config(AgentKind::Codex, "model_auto_compact_token_limit = 99999\n");
    fixture.press(KeyCode::Char('r'));
    let note = fixture.note(AgentKind::Codex);
    assert_eq!(note.tone, NoteTone::Error);
    assert!(note.text.starts_with("Not reverted"), "{}", note.text);
    assert_eq!(
        fixture.read_config(AgentKind::Codex),
        "model_auto_compact_token_limit = 99999\n"
    );
}

#[test]
fn a_stale_file_is_reported_in_the_tab_and_nothing_is_written() {
    let mut fixture = Fixture::new(screen()).with_codex_config();
    fixture.scan(AgentKind::Codex);
    fixture.press(KeyCode::Enter);
    assert!(fixture.pending().is_some());
    let outside = "model_auto_compact_token_limit = 100000\n# edited elsewhere\n";
    fixture.write_config(AgentKind::Codex, outside);
    fixture.press(KeyCode::Enter);
    assert!(fixture.pending().is_none());
    let note = fixture.note(AgentKind::Codex);
    assert_eq!(note.tone, NoteTone::Error);
    assert!(
        note.text.contains("changed since it was read"),
        "{}",
        note.text
    );
    assert!(note.text.contains("Nothing was written"));
    assert_eq!(fixture.read_config(AgentKind::Codex), outside);
    assert!(fixture
        .app
        .optimization
        .panel(AgentKind::Codex)
        .record
        .is_none());
    assert!(draw(&fixture).contains("changed since it was read"));
}

#[test]
fn applying_without_a_configuration_file_is_a_clear_error_not_a_created_file() {
    let mut fixture = Fixture::new(screen());
    fixture.scan(AgentKind::Codex);
    fixture.press(KeyCode::Enter);
    assert!(fixture.pending().is_none());
    let note = fixture.note(AgentKind::Codex);
    assert_eq!(note.tone, NoteTone::Error);
    assert!(note.text.contains("does not exist"), "{}", note.text);
    assert!(!fixture.config_path(AgentKind::Codex).exists());
}

#[test]
fn claude_apply_targets_settings_json_and_reports_environment_overrides() {
    let mut fixture = Fixture::new(screen());
    fixture.write_config(
        AgentKind::ClaudeCode,
        &format!("{{\n  \"theme\": \"dark\",\n  \"autoCompactWindow\": {CLAUDE_CURRENT}\n}}\n"),
    );
    if let Some(paths) = fixture.app.optimization.paths.config.as_mut() {
        paths.environment.insert(
            crate::agent_config_writer::CLAUDE_AUTO_COMPACT_ENV.to_owned(),
            "300000".to_owned(),
        );
    }
    fixture.press(KeyCode::Right);
    fixture.scan(AgentKind::ClaudeCode);
    let recommended = fixture.recommended_value(AgentKind::ClaudeCode);
    fixture.press(KeyCode::Enter);
    let pending = fixture.pending().expect("confirmation");
    assert_eq!(
        pending.plan.target,
        AgentConfigTarget::ClaudeAutoCompactWindow
    );
    assert!(pending.plan.path.ends_with("settings.json"));
    let lines = compaction_ui::modal_lines_for_tests(pending, 98);
    let joined = lines.join("\n");
    assert!(joined.contains("autoCompactWindow"));
    assert!(
        joined.contains("CLAUDE_CODE_AUTO_COMPACT_WINDOW=300000"),
        "{joined}"
    );
    assert!(joined.contains("Warnings"));
    assert!(joined.contains(&format!(
        "{} -> {}",
        group_thousands(CLAUDE_CURRENT),
        group_thousands(recommended)
    )));
    fixture.press(KeyCode::Enter);
    let after = fixture.read_config(AgentKind::ClaudeCode);
    assert!(
        after.contains(&format!("\"autoCompactWindow\": {recommended}")),
        "{after}"
    );
    assert!(after.contains("\"theme\": \"dark\""));
}

#[test]
fn the_confirmation_buttons_and_wheel_work_with_the_mouse() {
    let mut fixture = Fixture::new(Rect::new(0, 0, 80, 14)).with_codex_config();
    fixture.scan(AgentKind::Codex);
    let recommended = fixture.recommended_value(AgentKind::Codex);
    fixture.press(KeyCode::Char('a'));
    let screen = fixture.app.layout.screen_area;
    let layout = compaction_ui::modal_layout_for_tests(screen);

    // The text is taller than this small screen: the wheel scrolls it.
    let max = compaction_ui::modal_max_scroll(fixture.pending().unwrap(), screen);
    assert!(max > 0, "the confirmation overflows a 14-row screen");
    fixture.mouse(
        MouseEventKind::ScrollDown,
        layout.text_area.x,
        layout.text_area.y,
    );
    assert_eq!(fixture.pending().unwrap().scroll, 3.min(max));
    fixture.mouse(
        MouseEventKind::ScrollUp,
        layout.text_area.x,
        layout.text_area.y,
    );
    assert_eq!(fixture.pending().unwrap().scroll, 0);
    fixture.press(KeyCode::PageDown);
    assert!(fixture.pending().unwrap().scroll > 0);

    // Clicks outside the buttons are inert, including the Settings close button.
    fixture.click(0, 0);
    assert!(fixture.pending().is_some());
    assert!(matches!(fixture.app.mode, Mode::Settings(_)));

    // Cancel button.
    let cancel = layout.actions.cancel_button;
    fixture.click(cancel.x, cancel.y);
    assert!(fixture.pending().is_none());
    assert!(fixture
        .read_config(AgentKind::Codex)
        .contains(&CODEX_CURRENT.to_string()));

    // Apply button.
    fixture.press(KeyCode::Char('a'));
    let confirm = layout.actions.confirm_button;
    fixture.click(confirm.x, confirm.y);
    assert!(fixture.pending().is_none());
    assert!(fixture
        .read_config(AgentKind::Codex)
        .contains(&format!("= {recommended}")));
}

#[test]
fn clicking_the_page_buttons_runs_scan_apply_and_revert() {
    let mut fixture = Fixture::new(screen()).with_codex_config();
    fixture.app.tick_compaction(Instant::now());
    write_codex_corpus(&fixture.home(), 8);
    let content = fixture.content();
    let body_top = compaction_ui::body_area(content).y;
    let button = |fixture: &Fixture, action: Action| {
        *compaction_ui::build_body(&compaction_ui::Screen::of(&fixture.app), content.width)
            .buttons
            .iter()
            .find(|button| button.action == action)
            .unwrap_or_else(|| panic!("button {action:?}"))
    };
    // A refused admission leaves a note that moves the button: look it up again.
    let mut started = false;
    for _ in 0..400 {
        let scan = button(&fixture, Action::Scan);
        fixture.click(content.x + scan.x_start + 2, body_top + scan.first_line);
        started = fixture
            .app
            .compaction_optimizer
            .is_scanning(AgentKind::Codex);
        if started {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(started, "the scan was never admitted");
    fixture.wait_for_scan(AgentKind::Codex);

    let apply = button(&fixture, Action::Apply);
    fixture.click(content.x + apply.x_start + 2, body_top + apply.first_line);
    assert!(fixture.pending().is_some());
    fixture.press(KeyCode::Enter);
    let revert = button(&fixture, Action::Revert);
    fixture.click(content.x + revert.x_start + 2, body_top + revert.first_line);
    assert_eq!(fixture.note(AgentKind::Codex).tone, NoteTone::Success);
    assert!(fixture.read_config(AgentKind::Codex).contains("123456"));
}

#[test]
fn help_icons_open_the_help_topics_of_the_tab() {
    let mut fixture = Fixture::new(screen());
    fixture.app.tick_compaction(Instant::now());
    let layout = crate::settings_ui::compute_layout_for_mode(
        fixture.app.layout.screen_area,
        &fixture.app,
        fixture.state(),
    );
    let anchors = crate::settings_ui::settings_help_anchors(&layout, &fixture.app, fixture.state());
    let ids: Vec<&str> = anchors
        .iter()
        .map(|anchor| anchor.topic_id.as_str())
        .collect();
    assert!(
        ids.contains(&"OPT-01") && ids.contains(&"OPT-02"),
        "{ids:?}"
    );
    let scan_help = anchors
        .iter()
        .find(|anchor| anchor.topic_id == "OPT-02")
        .unwrap();
    fixture.click(scan_help.hit_area.x, scan_help.hit_area.y);
    assert!(matches!(fixture.app.mode, Mode::SettingsHelp(_)));
}

// --------------------------------------------------------------- helpers

#[test]
fn write_errors_get_actionable_wording() {
    let stale = describe_write_error(&WriteError::Stale {
        path: "/x/config.toml".into(),
        reason: "the content differs".to_owned(),
    });
    assert!(stale.starts_with("Nothing was written: /x/config.toml changed since it was read"));
    assert!(stale.ends_with("apply again."));
    let missing = describe_write_error(&WriteError::Missing {
        path: "/x/config.toml".into(),
    });
    assert!(missing.contains("never creates"));
    assert_eq!(
        config_target(AgentKind::Codex).key(),
        "model_auto_compact_token_limit"
    );
    assert_eq!(agent_label(AgentKind::ClaudeCode), "Claude Code");
    assert_eq!(adjacent_agent(AgentKind::Codex, 1), AgentKind::ClaudeCode);
    assert_eq!(adjacent_agent(AgentKind::Codex, -1), AgentKind::ClaudeCode);
    assert_eq!(adjacent_agent(AgentKind::ClaudeCode, 1), AgentKind::Codex);
}

#[test]
fn control_keys_and_unrelated_keys_fall_through_to_settings() {
    let mut fixture = Fixture::new(screen());
    let mut state = SettingsState {
        tab: SettingsTab::Optimization,
        ..SettingsState::default()
    };
    let control_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(!fixture.app.optimization_key(&mut state, control_s));
    let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    assert!(!fixture.app.optimization_key(&mut state, tab));
    let q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(!fixture.app.optimization_key(&mut state, q));
    assert!(!fixture
        .app
        .compaction_optimizer
        .is_scanning(AgentKind::Codex));
}
