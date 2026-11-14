//! Top-level layout: a left tree column, the focused pane's content on
//! the right, and a bottom status bar -- with the Explorer file-picker
//! drawn as an overlay on top of everything else when active. It consumes
//! the shared animated `App::layout`; everything it draws is delegated to
//! `tree_ui` or the pane runtimes themselves.

use std::time::{Duration, Instant};

use ilium_core::{AgentClass, AgentProvider, NodeId, NodeKind, PaneStatus, ROOT_ID};
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::agent_from_line::{CreateAgentFocus, CreateAgentFromLineState, EditorLineContextMenu};
use crate::app::{
    AgentToolbarModelSubmenuState, App, BoardDeleteTarget, BoardRenameTarget, ContextMenu,
    CreateBoardState, FocusTarget, Mode, PaneRuntime, RightPanelTarget, SubmenuItemAction,
    FROZEN_DIALOG_BUTTON_LABEL,
};
use crate::editor_pane::{EditorPane, EditorViewMode};
use crate::icon_settings::IconTarget;
use crate::{
    editor_chrome, editor_toolbar, explorer_overlay, markdown, minimap, modal, terminal_selection,
    terminal_view, theme, tree_ui,
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let elapsed = app.started_at.elapsed();
    draw_at(frame, app, elapsed);
}

#[cfg(test)]
#[path = "ui_openstreetmap_tests.rs"]
mod osm_attribution_tests;

#[cfg(test)]
#[path = "ui_polish_tests.rs"]
mod polish_tests;

/// Share the event loop's sampled animation time while retaining the public renderer.
pub(crate) fn draw_at(frame: &mut Frame, app: &mut App, animation_elapsed: Duration) {
    draw_at_with_cursor(frame, app, animation_elapsed);
}

/// Composition-only entry point returning the last explicit hardware cursor.
pub(crate) fn draw_at_with_cursor(
    frame: &mut Frame,
    app: &mut App,
    animation_elapsed: Duration,
) -> Option<Position> {
    app.composed_tree_rows = None;
    app.composed_terminal_sources.clear();
    tree_ui::set_tree_toolbar_always_visible(app.ui_settings.tree_toolbar_always_visible);
    app.reconcile_animation_presentation();
    app.composed_editor_sources.clear();
    let area = frame.area();
    let layout = app.layout;

    // Onboarding bypasses composition; an earlier pending scene receipt must
    // never be mistaken for pixels in its own completed terminal frame.
    if app.onboarding.is_some() {
        app.animation_frame.discard_composed_receipt();
    }
    let mut cursor = draw_base_layer(frame, area, app);
    if app.onboarding.is_none() {
        crate::background_composition::compose(frame.buffer_mut(), app, animation_elapsed);
        let osm_credit_area = draw_osm_attribution(frame, app);
        app.animation_frame
            .occlude_composed(frame.area(), osm_credit_area);
        draw_voice_control(frame, layout.voice_control_area, app);
        // This later chrome may repaint a Braille cell with an identical glyph.
        // Remove its entire known rectangle from scene attribution.
        app.animation_frame
            .occlude_composed(frame.area(), layout.voice_control_area);
    }
    if app.onboarding.is_none() && app.modal_stack.is_empty() && matches!(app.mode, Mode::Normal) {
        let popover_visible = app
            .agent_popover
            .as_ref()
            .is_some_and(|popover| popover.is_visible(Instant::now()));
        if app.hovered_status_slot.is_some()
            || app.hovered_progress.is_some()
            || app.hovered_goal_resume.is_some()
            || app.resources.hovered_action.is_some()
            || app.hovered_tree_node.is_some()
            || app.stats_popover.is_some()
            || app.goal_popover.is_some()
            || app.recap_popover.is_some()
            || app.hovered_recap_refresh.is_some()
            || popover_visible
        {
            // These late overlays own arbitrary blank pixels inside computed
            // layouts. Withhold this draw instead of guessing identical writes.
            app.animation_frame.discard_composed_receipt();
        }
        draw_status_tooltip(frame, app);
        draw_progress_tooltip(frame, app);
        draw_goal_resume_tooltip(frame, app);
        draw_worktree_tooltip(frame, app);
        draw_stats_popover(frame, app);
        app.draw_goal_popover(frame);
        draw_recap_tooltips(frame, app);
        if popover_visible {
            if let Some(popover) = &app.agent_popover {
                if let Some(geometry) = crate::popover::layout(app.layout.tree_area, popover) {
                    crate::popover::render(frame, &geometry, popover, app.ui_settings.color_scheme);
                }
            }
        }
    }

    // Every suspended parent draws before its child. This makes stack depth
    // a rendering concern rather than forcing child modes to clone or embed
    // the state of the screen they temporarily cover.
    for mode in &app.modal_stack {
        cursor = draw_mode_overlay(frame, area, app, mode).or(cursor);
    }
    cursor = draw_mode_overlay(frame, area, app, &app.mode).or(cursor);
    if app.is_animation_preview_visible()
        && app.animation_frame.visibility_warning()
        && area.height > 3
    {
        let credit_rows = if app.effective_animation_kind()
            == Some(crate::background_animation::AnimationKind::OpenStreetMap)
        {
            crate::layout::osm_attribution_area(area).height
        } else {
            0
        };
        let height = if area.width < 105 { 2 } else { 1 };
        if area.height > credit_rows + height + 2 {
            frame.render_widget(
                ratatui::widgets::Paragraph::new("Animation hard to see? Colors may blend with the background. Try Color preset, brightness or lightness.")
                    .wrap(ratatui::widgets::Wrap { trim: true })
                    .style(ratatui::style::Style::default().fg(ratatui::style::Color::Black).bg(ratatui::style::Color::Yellow)),
                ratatui::layout::Rect::new(area.x, area.bottom() - credit_rows - height - 2, area.width, height),
            );
        }
    }
    // The listed modes paint no late layer. Every other mode (or a suspended
    // parent) can touch a scene cell with the SAME Braille glyph, which a final
    // Buffer comparison cannot detect. Withhold its source history entirely.
    if !late_layers_are_source_transparent(app) {
        app.animation_frame.discard_composed_receipt();
    }
    draw_startup_dialog(frame, area, app);
    skip_vs16_continuation_cells(frame.buffer_mut(), layout.tree_area);
    if let Some(view) = &mut app.diagnostic_view {
        crate::diagnostic::render(frame, view);
        app.animation_frame.discard_composed_receipt();
        return None;
    }
    app.resource_overlay(frame);
    cursor
}

/// Centred progress dialog shown while the server's session is still
/// arriving; see `startup_dialog`.
fn draw_startup_dialog(frame: &mut Frame, area: Rect, app: &mut App) {
    use ratatui::style::{Modifier, Style};
    use ratatui::widgets::{Clear, Paragraph};

    if !app.is_startup_dialog_visible() {
        return;
    }
    let Some(dialog) = crate::startup_dialog::dialog_area(area) else {
        return;
    };
    app.animation_frame.discard_composed_receipt();
    let tick = (app.started_at.elapsed().as_millis() / 80) as u64;
    let lines = crate::startup_dialog::dialog_lines(dialog.width, &app.startup_dialog_text(), tick);
    frame.render_widget(Clear, dialog);
    for (offset, line) in lines.into_iter().enumerate() {
        let style = match offset {
            2 => Style::default().add_modifier(Modifier::BOLD),
            3 => Style::default().fg(ratatui::style::Color::Gray),
            4 => Style::default().fg(ratatui::style::Color::Cyan),
            _ => Style::default(),
        };
        frame.render_widget(
            Paragraph::new(line).style(style),
            Rect::new(dialog.x, dialog.y + offset as u16, dialog.width, 1),
        );
    }
}

/// This is an exact allowlist of no-op `draw_mode_overlay` branches. New late
/// render branches must either report their touched cells or stay excluded.
fn late_layers_are_source_transparent(app: &App) -> bool {
    app.modal_stack.is_empty()
        && matches!(
            app.mode,
            Mode::Normal
                | Mode::LeaderPending
                | Mode::NavigationLeaderPending
                | Mode::Move
                | Mode::Settings(_)
        )
}

/// Client chrome owns the credit. No terminal, animation cache or protected
/// workspace cell is changed to make the scene's native text visible.
fn draw_osm_attribution(frame: &mut Frame, app: &App) -> Rect {
    if app.effective_animation_kind()
        != Some(crate::background_animation::AnimationKind::OpenStreetMap)
    {
        return Rect::default();
    }
    let area = if app.is_animation_preview_visible() {
        crate::layout::osm_attribution_area(frame.area())
    } else if crate::background_composition::ambient_is_visible(app) {
        app.layout.osm_attribution_area
    } else {
        Rect::default()
    };
    if area.is_empty() {
        return area;
    }

    let style = theme::statusbar_style();
    frame.render_widget(Clear, area);
    frame.render_widget(ratatui::widgets::Block::default().style(style), area);
    // The caption is ASCII, so each byte chunk is a complete terminal cell.
    for (index, chunk) in crate::layout::OSM_ATTRIBUTION
        .as_bytes()
        .chunks(usize::from(area.width))
        .enumerate()
    {
        let segment = std::str::from_utf8(chunk).expect("ASCII OSM credit");
        frame.render_widget(
            Paragraph::new(segment).style(style.add_modifier(Modifier::BOLD)),
            Rect::new(area.x, area.y + index as u16, area.width, 1),
        );
    }
    area
}

/// A VS16 glyph occupies two terminal cells. Ratatui's diff can emit its
/// blank continuation at x+1 immediately after printing the wide glyph, while
/// CrosstermBackend assumes the previous print advanced only one cell. That
/// writes the blank at x+2 on terminals that honor the glyph's full width.
/// The glyph itself paints the continuation cell; omit the duplicate write.
fn skip_vs16_continuation_cells(buffer: &mut Buffer, tree_area: Rect) {
    let area = buffer.area;
    for row in area.top()..area.bottom() {
        for column in area.left()..area.right().saturating_sub(1) {
            let glyph = buffer[(column, row)].symbol();
            if glyph.contains('\u{fe0f}') && UnicodeWidthStr::width(glyph) > 1 {
                let trailing = &mut buffer[(column + 1, row)];
                if trailing.symbol().trim().is_empty() {
                    trailing.set_diff_option(CellDiffOption::Skip);
                }
                // A tree width change can move the title one cell left of its
                // previous terminal position. Repaint through the row's end so
                // the old final character is cleared even when Ratatui sees an
                // unchanged blank cell in its previous buffer.
                if row >= tree_area.top()
                    && row < tree_area.bottom()
                    && column >= tree_area.left()
                    && column < tree_area.right()
                {
                    for repaint_column in (column + 2)..tree_area.right() {
                        let cell = &mut buffer[(repaint_column, row)];
                        if cell.diff_option != CellDiffOption::Skip {
                            cell.set_diff_option(CellDiffOption::AlwaysUpdate);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod vs16_diff_tests {
    use super::*;

    #[test]
    fn tree_row_after_vs16_repaints_old_title_tail() {
        let area = Rect::new(0, 0, 24, 1);
        let mut buffer = Buffer::empty(area);
        buffer.set_string(0, 0, "│››   🖥️       Ledger  │", Style::default());
        skip_vs16_continuation_cells(&mut buffer, area);

        let icon_column = (0..area.width)
            .find(|&column| buffer[(column, 0)].symbol().contains('\u{fe0f}'))
            .expect("fixture has a VS16 icon");
        assert_eq!(
            buffer[(icon_column + 1, 0)].diff_option,
            CellDiffOption::Skip
        );
        for column in (icon_column + 2)..area.right() {
            assert_eq!(
                buffer[(column, 0)].diff_option,
                CellDiffOption::AlwaysUpdate,
                "tree column {column} must repaint to clear a stale title tail"
            );
        }
        // Ratatui can believe this blank is already on screen while the
        // terminal still has the final character of the former title there.
        let stale_tail_column = (icon_column + 2..area.right())
            .find(|&column| {
                buffer[(column, 0)].symbol().trim().is_empty()
                    && buffer[(column - 1, 0)].symbol() == "r"
            })
            .expect("fixture has a blank after Ledger");
        let previous = buffer.clone();
        assert!(previous.diff(&buffer).iter().any(|(column, row, cell)| {
            *column == stale_tail_column && *row == 0 && cell.symbol().trim().is_empty()
        }));
    }

    #[test]
    fn vs16_outside_tree_still_skips_only_its_continuation() {
        let area = Rect::new(0, 0, 32, 1);
        let mut buffer = Buffer::empty(area);
        buffer.set_string(26, 0, "🖥️  pane", Style::default());
        skip_vs16_continuation_cells(&mut buffer, Rect::new(0, 0, 24, 1));

        assert_eq!(buffer[(27, 0)].diff_option, CellDiffOption::Skip);
        assert_eq!(buffer[(28, 0)].diff_option, CellDiffOption::None);
    }
}

/// Shows a progress footer's multi-line `details` while the pointer rests on
/// it. The footer itself keeps only the compact one-line `message`.
fn draw_progress_tooltip(frame: &mut Frame, app: &App) {
    let Some((pane_id, anchor)) = app.hovered_progress else {
        return;
    };
    let Some(progress_area) = app
        .pane_viewports()
        .into_iter()
        .find(|viewport| viewport.pane_id == pane_id)
        .and_then(|viewport| viewport.progress_area)
    else {
        return;
    };
    let Some(progress) = app.progress_monitor_at(pane_id, progress_area, anchor) else {
        return;
    };
    let owner = app.progress_owner_label(pane_id);
    crate::progress_popover::render(
        frame,
        app.layout.screen_area,
        anchor,
        progress,
        app.progress_display_now_unix_millis(),
        owner.as_deref(),
    );
}

fn draw_goal_resume_tooltip(frame: &mut Frame, app: &App) {
    let Some((_, anchor)) = app.hovered_goal_resume else {
        return;
    };
    let content = crate::status_icons::TooltipContent {
        title: crate::goal_resume_link::GOAL_RESUME_TOOLTIP.to_owned(),
        body: format!(
            "Sends {} to this agent and presses Enter.",
            crate::goal_resume_link::GOAL_RESUME_COMMAND
        ),
        reason: None,
    };
    crate::status_icons::render_tooltip(frame, app.layout.screen_area, anchor, &content);
}

#[cfg_attr(not(test), allow(clippy::needless_return))] // The return guards the test-only fallback below.
fn draw_worktree_tooltip(frame: &mut Frame, app: &App) {
    let Some(hit) = app.hovered_tree_node.filter(|hit| hit.line == 1) else {
        return;
    };
    let anchor = Position::new(app.layout.tree_area.x.saturating_add(2), hit.row);
    if app.emitted_geometry.is_some() {
        if let Some(explanation) = app.emitted_worktree_tooltip(hit.id) {
            crate::status_icons::render_tooltip(frame, app.layout.screen_area, anchor, explanation);
        }
    }
    #[cfg(test)]
    if app.emitted_geometry.is_none() {
        if let Some(workspace) = app.tree.pane_workspace(hit.id) {
            let explanation = crate::status_icons::workspace_explanation(
                workspace,
                app.workspace_git_statuses.get(&hit.id),
            );
            crate::status_icons::render_tooltip(
                frame,
                app.layout.screen_area,
                anchor,
                &explanation,
            );
        }
    }
}

/// Explains the tree-row state glyph under the pointer. The slot's signal is
/// retained with the actually emitted row; newer server evidence cannot
/// change its explanation until that row's next frame is acknowledged.
#[cfg_attr(not(test), allow(clippy::needless_return))] // The return guards the test-only fallback below.
fn draw_status_tooltip(frame: &mut Frame, app: &App) {
    let Some((node_id, slot, anchor)) = app.hovered_status_slot else {
        return;
    };
    if app.emitted_geometry.is_some() {
        if let Some(tooltip) = app.emitted_status_tooltip(node_id, slot) {
            crate::status_icons::render_tooltip(frame, app.layout.screen_area, anchor, tooltip);
        }
    }
    #[cfg(test)]
    if app.emitted_geometry.is_none() {
        if let Some(tooltip) = status_tooltip_content(app, node_id, slot) {
            crate::status_icons::render_tooltip(frame, app.layout.screen_area, anchor, &tooltip);
        }
    }
}
fn status_tooltip_content(
    app: &App,
    node_id: ilium_core::NodeId,
    slot: crate::status_icons::StatusSlot,
) -> Option<crate::status_icons::TooltipContent> {
    use crate::status_icons::{
        identity_explanation, now_explanation, objective_explanation, StatusSlot,
    };

    // In Custom mode the error replaces the rule tooltip only when it is
    // also painted over the rule icon (Overlay); otherwise the custom slot
    // keeps explaining the winning rule. Group/session rows have no rule
    // icon, so they keep the error tooltip in every mode.
    let custom_mode = app.ui_settings.agent_monitoring_mode
        == crate::agent_monitoring::AgentMonitoringMode::Custom;
    let is_pane = app
        .tree
        .get(node_id)
        .is_some_and(|node| matches!(node.kind, NodeKind::Pane { .. }));
    let error_overrides_now = !custom_mode
        || !is_pane
        || app.ui_settings.blocking_error_display == crate::config::BlockingErrorDisplay::Overlay;
    if slot == StatusSlot::Now && error_overrides_now {
        if let Some(error) = app.blocking_error_tooltip(node_id) {
            return Some(error);
        }
    }
    let node = app.tree.get(node_id)?;
    if slot == StatusSlot::Identity {
        let structural = match &node.kind {
            NodeKind::Container(container) => {
                let (title, kind) = match &container.kind {
                    ilium_core::ContainerKind::Project { .. } => ("Project", "project"),
                    ilium_core::ContainerKind::Group => ("Group", "group"),
                    ilium_core::ContainerKind::Worktree { .. } => ("Worktree", "fixed worktree"),
                    ilium_core::ContainerKind::SplitView { .. } => ("Split view", "split view"),
                };
                Some((title, kind))
            }
            NodeKind::Folder { .. } => Some(("Folder", "folder")),
            NodeKind::Pane { .. } => None,
        };
        if let Some((title, kind)) = structural {
            let tooltip = crate::status_icons::TooltipContent {
                title: title.to_string(),
                body: format!(
                    "{} is a {} entry in the saved tree.",
                    crate::status_icons::safe_tooltip_text(&node.name),
                    kind
                ),
                reason: Some(format!(
                    "Why: the authoritative tree records this entry as a {kind}."
                )),
            };
            return Some(tooltip);
        }
    }
    let NodeKind::Pane {
        status,
        progress_monitors,
        scheduled_input,
        ..
    } = &node.kind
    else {
        return None;
    };
    // Custom mode shows one status slot whose content comes from the user's
    // rules, so its hover names the matched rule instead of a projection.
    if app.ui_settings.agent_monitoring_mode == crate::agent_monitoring::AgentMonitoringMode::Custom
    {
        match slot {
            // Custom has no objective icon; with Second icon this slot is the
            // error cell beside the custom icon (`PaintedTreeRows::status_at`).
            StatusSlot::Objective => {
                return (app.ui_settings.blocking_error_display
                    == crate::config::BlockingErrorDisplay::SecondIcon
                    && app.pane_has_blocking_error(node_id))
                .then(|| app.blocking_error_tooltip(node_id))
                .flatten();
            }
            StatusSlot::Now => return crate::custom_status_live::tooltip(app, node_id),
            StatusSlot::Identity => {}
        }
    }
    let shell_output = app
        .terminal_activity
        .phase(node_id, app.started_at.elapsed().as_millis())
        .map(tree_ui::shell_output_phase);
    let signals = crate::agent_monitoring::displayed_pane_signals(
        app.ui_settings.agent_monitoring_mode,
        app.ui_settings.attention_progress_reports,
        app.ui_settings.attention_running_indicator,
        status,
        progress_monitors,
        scheduled_input.is_some(),
        shell_output,
    );
    // The tree row shows one glyph for all of the pane's monitors; explain
    // the monitor that glyph represents.
    let progress = ilium_core::representative_progress(progress_monitors);
    if slot == StatusSlot::Now {
        if let Some(warning) = crate::status_icons::detection_freshness_warning(
            status,
            app.pane_detection_evidence.get(&node_id),
            crate::wall_clock::unix_millis_now(),
        ) {
            let last_activity = now_explanation(signals.now).map_or(
                "Not shown by the current monitoring preference",
                |explanation| explanation.title,
            );
            return Some(crate::status_icons::TooltipContent {
                title: "Agent activity needs a fresh check".to_string(),
                body: format!("{warning} Last known activity: {last_activity}."),
                reason: Some("Why: observation freshness affects only this display; the server's activity, goal and unread completion are retained.".to_string()),
            });
        }
    }
    let explanation = match slot {
        StatusSlot::Identity => Some(identity_explanation(status)),
        StatusSlot::Objective => objective_explanation(signals.objective),
        StatusSlot::Now => now_explanation(signals.now),
    };
    if let Some(explanation) = explanation {
        let detection = app
            .pane_detection_evidence
            .get(&node_id)
            .filter(|evidence| &evidence.applied_status == status);
        let recorded_reason = |reason: Option<&ilium_ipc::DetectionReason>| {
            reason.map(crate::status_icons::detection_reason_text)
        };
        let reason = match slot {
            StatusSlot::Identity => Some(crate::status_icons::identity_provenance_reason(
                status,
                detection.and_then(|evidence| evidence.identity.as_ref()),
            )),
            StatusSlot::Objective => match signals.objective {
                ilium_core::ObjectiveSignal::Goal(_) => recorded_reason(
                    detection.and_then(|evidence| evidence.goal.as_ref()),
                ).or_else(|| Some("Why: this goal phase is in the current server status; the confirming provider row is not available yet.".to_string())),
                ilium_core::ObjectiveSignal::Task(_) => progress.map(|progress| {
                    format!(
                        "Why: monitor #{} reported job «{}» as {:?} at {:.1}%; observation is {}. Report message «{}».{} Received at Unix millisecond {}.",
                        progress.monitor_id,
                        crate::status_icons::safe_tooltip_text(&progress.report.job_id),
                        progress.report.status,
                        progress.report.percent,
                        match &progress.monitor_health {
                            ilium_core::ProgressMonitorHealth::Healthy => "healthy",
                            ilium_core::ProgressMonitorHealth::Degraded { .. } => "degraded",
                            ilium_core::ProgressMonitorHealth::Failed { .. } => "failed",
                        },
                        crate::status_icons::safe_tooltip_text(&progress.report.message),
                        progress.report.error.as_deref().map(|error| format!(
                            " Report error «{}».",
                            crate::status_icons::safe_tooltip_text(error)
                        )).unwrap_or_default(),
                        progress.last_observed_unix_millis,
                    )
                }),
                ilium_core::ObjectiveSignal::ScheduledInput => scheduled_input.as_ref().map(|input| format!(
                    "Why: the server-owned tree has an input scheduled for Unix millisecond {}; it will {}.",
                    input.execute_at_unix_millis,
                    if input.send_enter { "submit with Enter" } else { "type without Enter" },
                )),
                ilium_core::ObjectiveSignal::None => None,
            },
            StatusSlot::Now => match signals.now {
                ilium_core::NowSignal::AgentUnavailable(availability) => Some(format!(
                    "Why: {}. Historical agent identity and recovery data remain available; the terminal has no confirmed live agent composer.",
                    availability.label(),
                )),
                ilium_core::NowSignal::Parked => progress.map(|progress| format!(
                    "Why: the agent is idle while live monitor #{} watches job «{}»; the monitor suppresses the finished alert.",
                    progress.monitor_id,
                    crate::status_icons::safe_tooltip_text(&progress.report.job_id),
                )),
                ilium_core::NowSignal::ShellOutput(phase) => {
                    let snapshot = app
                        .terminal_activity
                        .snapshot(node_id, app.started_at.elapsed().as_millis());
                    crate::status_icons::shell_output_reason(phase, snapshot)
                }
                ilium_core::NowSignal::FinishedUnread => recorded_reason(
                    detection.and_then(|evidence| evidence.activity.as_ref()),
                ).map(|reason| format!("{reason} The server retained this completed turn as unread until pane focus or input."))
                    .or_else(|| Some("Why: the server recorded a completed turn that has not yet been acknowledged by pane focus or input.".to_string())),
                ilium_core::NowSignal::NeedsApproval
                | ilium_core::NowSignal::Working
                | ilium_core::NowSignal::WaitingSubagents
                | ilium_core::NowSignal::Settling
                | ilium_core::NowSignal::Idle => recorded_reason(
                    detection.and_then(|evidence| evidence.activity.as_ref()),
                ).or_else(|| Some("Why: this is the server's current activity classification; the matching screen evidence has not arrived yet.".to_string())),
                ilium_core::NowSignal::None => None,
            },
        }
        .map(|reason| match slot {
            StatusSlot::Objective
                if app.ui_settings.agent_monitoring_mode
                    == crate::agent_monitoring::AgentMonitoringMode::Attention =>
            {
                format!("{reason} Attention selection rule: {}.", signals.objective_rule)
            }
            StatusSlot::Now
                if app.ui_settings.agent_monitoring_mode
                    == crate::agent_monitoring::AgentMonitoringMode::Attention =>
            {
                format!("{reason} Attention selection rule: {}.", signals.now_rule)
            }
            StatusSlot::Objective => format!("{reason} Projection rule {}.", signals.objective_rule),
            StatusSlot::Now => format!("{reason} Projection rule {}.", signals.now_rule),
            StatusSlot::Identity => reason,
        });
        let tooltip = crate::status_icons::TooltipContent {
            title: explanation.title.to_string(),
            body: explanation.body.to_string(),
            reason,
        };
        return Some(tooltip);
    }
    None
}

/// Draws the costs-and-stats popover of the pane whose second header icon was
/// hovered or clicked. The popover state is taken out of `app` for the draw so
/// rendering can record scroll bounds on it while reading everything else.
fn draw_stats_popover(frame: &mut Frame, app: &mut App) {
    let Some(mut popover) = app.stats_popover.take() else {
        return;
    };
    let anchor = app
        .pane_viewport(popover.pane_id)
        .map(|viewport| theme::chrome_stats_cell(viewport.outer_area));
    if let Some(anchor) = anchor {
        let entry = app.current_stats_entry(popover.pane_id);
        let idle = crate::session_stats_store::LoadState::Idle;
        let view = crate::session_stats_ui::StatsView {
            stats: entry.and_then(|entry| entry.stats.as_deref()),
            load: entry.map_or(&idle, |entry| &entry.state),
            supported: app.stats_agent_is_supported(popover.pane_id),
            has_session: app.known_agent_history_context(popover.pane_id).is_some(),
            now_ms: chrono::Utc::now().timestamp_millis(),
            animation_ms: app.started_at.elapsed().as_millis(),
            scheme: app.ui_settings.color_scheme,
        };
        crate::session_stats_ui::render(frame, app.layout.pane_area, anchor, &mut popover, &view);
    }
    app.stats_popover = Some(popover);
}

/// Draws the recap popover and the refresh-icon tooltip. Both show cached
/// text and the phase bar of a running recap.
fn draw_recap_tooltips(frame: &mut Frame, app: &App) {
    for (anchor, content) in [app.recap_popover_tooltip(), app.recap_refresh_tooltip()]
        .into_iter()
        .flatten()
    {
        crate::status_icons::render_tooltip(frame, app.layout.screen_area, anchor, &content);
    }
}

/// Styles the third header dot of an agent pane as the recap handle: accent
/// while a recap is cached or running, filled while its popover is open.
fn draw_recap_icon(frame: &mut Frame, app: &App, viewport: crate::split_layout::PaneViewport) {
    if viewport.outer_area.width < 12 || !app.is_known_agent_pane(viewport.pane_id) {
        return;
    }
    let is_open = app
        .recap_popover
        .as_ref()
        .is_some_and(|popover| popover.pane_id == viewport.pane_id);
    let style = if is_open {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::new()
            .fg(theme::accent_bg())
            .add_modifier(Modifier::BOLD)
    };
    let cell = theme::chrome_recap_cell(viewport.outer_area);
    frame.render_widget(
        Paragraph::new(Span::styled(
            app.ui_settings
                .icons
                .glyph(IconTarget::ChromeRecap)
                .to_string(),
            style,
        )),
        Rect::new(cell.x, cell.y, 1, 1),
    );
}

/// Styles the second header icon of an agent pane as the popover's handle:
/// accent-coloured when idle, filled while its popover is open.
fn draw_stats_icon(frame: &mut Frame, app: &App, viewport: crate::split_layout::PaneViewport) {
    if viewport.outer_area.width < 10 || !app.is_known_agent_pane(viewport.pane_id) {
        return;
    }
    let is_open = app
        .stats_popover
        .as_ref()
        .is_some_and(|popover| popover.pane_id == viewport.pane_id);
    let style = if is_open {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::new()
            .fg(theme::accent_bg())
            .add_modifier(Modifier::BOLD)
    };
    let cell = theme::chrome_stats_cell(viewport.outer_area);
    frame.render_widget(
        Paragraph::new(Span::styled("●", style)),
        Rect::new(cell.x, cell.y, 1, 1),
    );
}

/// Draws the one full-screen root behind every stacked overlay. Settings
/// replaces the ordinary workspace only when it is the oldest layer.
fn draw_base_layer(frame: &mut Frame, area: Rect, app: &mut App) -> Option<Position> {
    if app.onboarding.is_some() {
        crate::onboarding::integration::render(frame, area, app);
        return None;
    }
    let root_mode = app.modal_stack.first().unwrap_or(&app.mode);
    if let Mode::Settings(state) = root_mode {
        crate::settings_ui::render(frame, area, app, state);
        return None;
    }

    let layout = app.layout;
    // The pane renders before the tree because `PseudoTerminal` clears its
    // area; the tree then restores the connected shared-border joints.
    app.composed_terminal_sources = draw_pane_sources(frame, layout.pane_area, app);
    app.composed_editor_sources.clear();
    for viewport in app.pane_viewports() {
        let Some(crate::app::PaneRuntime::Editor(editor)) = app.panes.get(&viewport.pane_id) else {
            continue;
        };
        if editor.view_mode != EditorViewMode::Source {
            continue;
        }
        let Some(installed) = editor.installed_window() else {
            continue;
        };
        let chrome = editor_chrome::compute(viewport.content_area, editor.show_minimap);
        if installed.key.width != chrome.content_area.width
            || installed.key.height != chrome.content_area.height
        {
            continue;
        }
        app.composed_editor_sources.push((
            viewport.pane_id,
            crate::source_window_surface::PaintedWindow {
                installed: installed.clone(),
                content_area: chrome.content_area,
                minimap_area: chrome.minimap_area,
            },
        ));
    }
    let tree_focused = matches!(app.focus, FocusTarget::Tree);
    let focused_pane_id = app.focused_pane_id();
    let tree_order = app.effective_tree_order();
    let agent_models = app.current_tree_models();
    // Read before `render` takes `&mut app.tree_state`; the hover state only needs the mask.
    let pending_toolbar = app.pending_toolbar_mask();
    let mut custom_toolbar_pending: std::collections::HashSet<String> = app
        .pending_pane_creations
        .values()
        .filter(|pending| !pending.uncertainty_reported)
        .filter_map(|pending| pending.custom_profile_id.clone())
        .collect();
    if app.active_workspace_create_request_id.is_some() {
        if let Mode::CreateAgentWorkspace(state) = &app.mode {
            if let Some(identity) = &state.custom_agent {
                custom_toolbar_pending.insert(identity.profile_id.clone());
            }
        }
    }
    let profiles_by_id: std::collections::HashMap<_, _> = app
        .custom_agents
        .profiles
        .iter()
        .map(|profile| (profile.id.as_str(), profile))
        .collect();
    let custom_agent_labels: std::collections::HashMap<_, _> = app
        .custom_agent_panes
        .iter()
        .map(|(pane_id, identity)| {
            let label = profiles_by_id
                .get(identity.profile_id.as_str())
                .map(|profile| (profile.icon.clone(), profile.name.clone()))
                .unwrap_or_else(|| {
                    (
                        "?".to_string(),
                        format!("Missing profile ({})", identity.profile_id),
                    )
                });
            (*pane_id, label)
        })
        .collect();
    let tree_visibility = app.active_tree_projection(std::time::Instant::now());
    // Contributed plugin buttons, indexed like `TreeToolbarAction::Plugin`.
    let plugin_toolbar = app.plugin_toolbar_buttons();
    // The keys `TreeOrder::PluginKey` sorts by; empty unless a plugin ordering is chosen.
    let plugin_order_ranks = app.plugin_tree_order_ranks();
    // Owned once per frame so the render options can borrow it.
    let blocking_error_facts = app.blocking_error_facts();
    // Copied out before the mutable tree borrow below; the view facts read the whole app.
    let pane_view_facts = crate::custom_status_live::PaneViewFacts::from_app(app);
    let mut painted_rows = tree_ui::render(
        frame,
        layout.tree_area,
        &app.tree,
        &mut app.tree_state,
        tree_ui::TreeRenderOptions {
            visibility: tree_visibility.as_ref(),
            toolbar_layout: &app.ui_settings.tree_toolbar_layout,
            custom_toolbar_profiles: Some(&app.custom_agents.profiles),
            custom_toolbar_selected: app.custom_toolbar_selected.as_deref(),
            custom_toolbar_pending: Some(&custom_toolbar_pending),
            plugin_toolbar: &plugin_toolbar,
            plugin_order_ranks: Some(&plugin_order_ranks),
            custom_agent_labels: Some(&custom_agent_labels),
            focused: tree_focused,
            elapsed_ms: if matches!(
                app.ui_settings.motion_level,
                crate::config::MotionLevel::Off
            ) {
                0
            } else {
                app.started_at.elapsed().as_millis()
            },
            terminal_activity_elapsed_ms: app.started_at.elapsed().as_millis(),
            current_unix_millis: crate::wall_clock::unix_millis_now(),
            detection_evidence: Some(&app.pane_detection_evidence),
            project_name: app.project_name.as_deref(),
            project_icon: app.project_icon.as_deref(),
            is_project_name_loading: app.is_project_name_loading,
            titles_loading: &app.titles_loading,
            recently_created: &app.recently_created,
            terminal_activity: &app.terminal_activity,
            focused_pane_id,
            transitions: &app.tree_transitions,
            agent_identifiers: &app.ui_settings.agent_identifiers,
            agent_models: &agent_models,
            icons: &app.ui_settings.icons,
            row_layout: &app.ui_settings.tree_row_layout,
            workspace_git_statuses: &app.workspace_git_statuses,
            show_worktree_branch_line: app.git_settings.branch_line
                != crate::config::GitBranchLine::Off,
            tree_order,
            show_project_separators: app.ui_settings.show_project_separators,
            sidebar_density: app.ui_settings.sidebar_density,
            use_stable_glyphs: app.ui_settings.use_stable_glyphs,
            agent_monitoring_mode: app.ui_settings.agent_monitoring_mode,
            attention_progress_reports: app.ui_settings.attention_progress_reports,
            custom_status_rules: Some(&app.custom_status_rules),
            screen_observations: Some(&app.custom_status_screen_observations),
            screen_pane_id: None,
            icon_motion: app.ui_settings.motion_level,
            attention_running_indicator: app.ui_settings.attention_running_indicator,
            show_inferred_title_icons: app.ui_settings.show_inferred_title_icons,
            frozen_panes: &app.frozen_panes,
            blocking_errors: Some(&blocking_error_facts),
            pane_view: Some(pane_view_facts),
            cost: Some(app.cost_tracker.overlay().as_ref()),
            hover: tree_ui::TreeHoverState {
                node: app.hovered_tree_node,
                toolbar_hovered: app.tree_toolbar_hovered,
                toolbar_action: app.hovered_tree_toolbar_action,
                pending_toolbar,
                show_management_actions: app.ui_settings.show_tree_row_management_controls,
            },
            sidebar_files: &app.sidebar_snapshot,
            resources: Some(&app.resources),
            panes: &app.panes,
        },
    );
    let values = painted_rows
        .node_ids()
        .map(|node_id| {
            let status = [
                crate::status_icons::StatusSlot::Identity,
                crate::status_icons::StatusSlot::Objective,
                crate::status_icons::StatusSlot::Now,
            ]
            .map(|slot| status_tooltip_content(app, node_id, slot));
            let worktree = app.tree.pane_workspace(node_id).map(|workspace| {
                crate::status_icons::workspace_explanation(
                    workspace,
                    app.workspace_git_statuses.get(&node_id),
                )
            });
            // Only this displayed title prefix can reach the fixed-width card.
            // The row metadata lease is already held before composing this DTO.
            let cost_title = app
                .tree
                .get(node_id)
                .map(|node| node.name.chars().take(240).collect())
                .unwrap_or_default();
            let cost_card = app.cost_tracker.overlay().prepared_card(node_id);
            tree_ui::PaintedRowValues {
                node_id,
                status,
                worktree,
                cost_title,
                cost_card,
            }
        })
        .collect();
    painted_rows.retain_values(values);
    paint_tree_blocking_errors(frame, app, &mut painted_rows);
    // Read once per frame, not once per row.
    let frame_slots = app.plugin_chrome("pane_frame_slots");
    painted_rows
        .paint_plugin_decorations(frame, |node| app.plugin_decoration_text(node, &frame_slots));
    app.record_composed_tree_rows(painted_rows);
    draw_cost_card(frame, app);
    draw_status_bar(frame, layout.status_area, app);
    None
}

/// Shows retained blocking errors on the painted tree rows. Normal and
/// Attention modes always overlay the warning on the status cell. Custom mode
/// follows `[ui] blocking_error_display` for pane rows, because there the
/// cell holds the icon the user's ordered rules chose: Rules only paints
/// nothing (rules use the `blocking-error` / `session-error` conditions),
/// Overlay keeps the old triangle, Second icon uses the extra cell beside the
/// custom icon. Group, project and session rows are not covered by pane
/// rules, so they keep the overlay in every mode.
fn paint_tree_blocking_errors(
    frame: &mut Frame,
    app: &App,
    painted_rows: &mut tree_ui::PaintedTreeRows,
) {
    use crate::agent_monitoring::AgentMonitoringMode;
    use crate::config::BlockingErrorDisplay;
    let custom = app.ui_settings.agent_monitoring_mode == AgentMonitoringMode::Custom;
    let display = app.ui_settings.blocking_error_display;
    let mut overlay = std::collections::HashSet::new();
    let mut second_icon = std::collections::HashSet::new();
    for node_id in painted_rows.node_ids() {
        let is_pane = app
            .tree
            .get(node_id)
            .is_some_and(|node| matches!(node.kind, NodeKind::Pane { .. }));
        if !custom || !is_pane {
            if app.blocking_error_tooltip(node_id).is_some() {
                overlay.insert(node_id);
            }
            continue;
        }
        match display {
            BlockingErrorDisplay::RulesOnly => {}
            BlockingErrorDisplay::Overlay => {
                if app.blocking_error_tooltip(node_id).is_some() {
                    overlay.insert(node_id);
                }
            }
            BlockingErrorDisplay::SecondIcon => {
                if app.pane_has_blocking_error(node_id) {
                    second_icon.insert(node_id);
                }
            }
        }
    }
    if !second_icon.is_empty() {
        // A pane whose row layout hides the Activity slot has no error cell;
        // fall back to the overlay rather than hiding its error.
        overlay.extend(painted_rows.paint_second_error_icons(
            frame,
            &second_icon,
            app.ui_settings.use_stable_glyphs,
        ));
    }
    painted_rows.paint_blocking_errors(frame, &overlay);
}

/// Draws the per-agent cost card beside its tree entry when the detail-card
/// acknowledged row option makes it visible (see `App::emitted_cost_card_target`).
fn draw_cost_card(frame: &mut Frame, app: &App) {
    let Some((row, values)) = app.emitted_cost_card_target() else {
        return;
    };
    let Some(card) = values.cost_card.as_ref() else {
        return;
    };
    crate::cost_overlay::draw_detail_card(
        frame,
        frame.area(),
        app.layout.tree_area.right(),
        row,
        &values.cost_title,
        &card.lines,
    );
}

/// Draws one non-root layer without deciding which layer owns input. Input
/// dispatch remains top-only through `App::mode`; this function is read-only.
fn draw_mode_overlay(frame: &mut Frame, area: Rect, app: &App, mode: &Mode) -> Option<Position> {
    let mut cursor = None;
    match mode {
        Mode::Explorer(overlay, _)
        | Mode::FolderExplorer(overlay, _)
        | Mode::ProjectFolderExplorer(overlay, _)
        | Mode::PluginFilePicker(overlay, _)
        | Mode::BoardPathPicker(overlay) => {
            explorer_overlay::render(frame, area, overlay, std::time::SystemTime::now());
        }
        Mode::ExplorerFileMenu(menu) => {
            frame.render_widget(Clear, menu.area);
            let label = menu
                .file_path
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default();
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        "Markdown file",
                        Style::new().add_modifier(Modifier::DIM),
                    )),
                    Line::from(format!(
                        "{} Create board from {label}",
                        context_menu_icon(&app.ui_settings, IconTarget::Board)
                    )),
                ])
                .block(theme::block(true).title(theme::chrome_title("File actions"))),
                menu.area,
            );
        }
        Mode::SettingsHelp(state) => {
            let Some(topic) = crate::settings_help::catalog::by_id(&state.topic_id) else {
                crate::settings_help::dialog::render_missing(frame, area, &state.topic_id);
                return None;
            };
            crate::settings_help::dialog::render(
                frame,
                area,
                topic,
                state,
                app.ui_settings.motion_level,
                Instant::now(),
            );
        }
        Mode::ContextMenu(menu) => {
            draw_context_menu(frame, menu, &app.ui_settings);
        }
        Mode::TerminalPaneContextMenu(menu) => {
            draw_terminal_pane_context_menu(frame, menu, &app.ui_settings)
        }
        Mode::AgentToolbarModelSubmenu(state) => draw_agent_toolbar_model_submenu(frame, state),
        Mode::LastPromptDialog(state) => draw_last_prompt_dialog(frame, area, state),
        Mode::ValueDialog(host) => {
            let ink = match app.ui_settings.color_scheme {
                crate::theme::ColorScheme::Dark => Style::new().fg(Color::White).bg(Color::Black),
                crate::theme::ColorScheme::Light => Style::new().fg(Color::Black).bg(Color::White),
            };
            crate::value_dialog::PreparedValueDialog::new(area, &host.dialog).render(
                frame,
                crate::value_dialog::DialogStyles {
                    background: ink,
                    normal: ink,
                    highlighted: ink.add_modifier(Modifier::REVERSED),
                    current: ink.add_modifier(Modifier::BOLD),
                    disabled: ink.add_modifier(Modifier::DIM),
                    error: ink.fg(Color::Red),
                },
            );
        }
        Mode::EditorLineContextMenu(menu) => {
            draw_editor_line_context_menu(frame, menu, &app.ui_settings)
        }
        Mode::CreateAgentFromLine(state) => draw_create_agent_from_line(frame, area, state),
        Mode::CreateAgentWorkspace(state) => {
            crate::worktree_dialog::draw_dialog(frame, area, state);
        }
        Mode::WorktreeManager(state) => crate::worktree_manager::render(frame, area, state),
        Mode::PluginPopup(state) => crate::plugin_host::popup::draw(frame, area, state, app),
        Mode::CreateBoard(state) => {
            cursor = draw_create_board(frame, area, state).or(cursor);
        }
        Mode::BoardCardPrompt(_, state) => {
            cursor =
                modal::render_text_prompt_cursor(frame, area, "New card", state, "Create card")
                    .or(cursor);
        }
        Mode::BoardColumnPrompt(_, state) => {
            cursor =
                modal::render_text_prompt_cursor(frame, area, "New column", state, "Create column")
                    .or(cursor);
        }
        Mode::BoardRenamePrompt(_, target, state) => {
            let title = match target {
                BoardRenameTarget::Card => "Rename card",
                BoardRenameTarget::Column => "Rename column",
            };
            cursor =
                modal::render_text_prompt_cursor(frame, area, title, state, "Rename").or(cursor);
        }
        Mode::BoardDeleteConfirm(_, target) => {
            let (title, message) = match target {
                BoardDeleteTarget::Card => ("Delete card?", "Delete the selected card?"),
                BoardDeleteTarget::Column => {
                    ("Delete column?", "Delete the empty selected column?")
                }
            };
            modal::render_confirm(
                frame,
                area,
                title,
                message,
                modal::DialogActions::confirmation(
                    "Cancel",
                    modal::DialogButtonTone::Neutral,
                    "Delete",
                    modal::DialogButtonTone::Danger,
                ),
            );
        }
        Mode::CommandPrompt(state) => {
            cursor = modal::render_text_prompt_cursor(frame, area, "Run command", state, "Run")
                .or(cursor);
        }
        Mode::InferenceSettingPrompt(field, state) => {
            if *field == crate::app::InferenceSettingField::RestructurePromptTokenLimit {
                let (hint, is_error) = app.restructure_budget_input_hint(&state.buf);
                let style = if is_error {
                    Style::new().fg(Color::Red)
                } else {
                    Style::new().add_modifier(Modifier::DIM)
                };
                cursor = modal::render_text_prompt_with_hint_cursor(
                    frame,
                    area,
                    field.label(),
                    state,
                    "Close",
                    &hint,
                    style,
                )
                .or(cursor);
            } else if matches!(
                field,
                crate::app::InferenceSettingField::OpenAiApiKey
                    | crate::app::InferenceSettingField::AnthropicApiKey
                    | crate::app::InferenceSettingField::OpenRouterApiKey
            ) {
                cursor = modal::render_masked_text_prompt_cursor(
                    frame,
                    area,
                    field.label(),
                    state,
                    "Apply",
                )
                .or(cursor);
            } else {
                cursor =
                    modal::render_text_prompt_cursor(frame, area, field.label(), state, "Apply")
                        .or(cursor);
            }
        }
        Mode::VoiceSettingPrompt(field, state) => {
            cursor = modal::render_masked_text_prompt_cursor(
                frame,
                area,
                field.label(),
                state,
                "Replace",
            )
            .or(cursor);
        }
        Mode::ApiSettingPrompt(state) => {
            cursor = modal::render_text_prompt_cursor(frame, area, "HTTP API port", state, "Apply")
                .or(cursor);
        }
        Mode::PluginSettingPrompt(target, state) => {
            cursor =
                modal::render_text_prompt_cursor(frame, area, &target.title, state, "Apply")
                    .or(cursor);
        }
        Mode::GitSettingPrompt(field, state) => {
            cursor = modal::render_text_prompt_cursor(frame, area, field.label(), state, "Apply")
                .or(cursor);
        }
        Mode::AnimationTextPrompt(target, state) => {
            let (hint, style) = match &target.error {
                Some(message) => (
                    message.clone(),
                    Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                None => (
                    format!("{} \u{2014} Enter applies, Esc cancels", target.hint),
                    Style::new().add_modifier(Modifier::DIM),
                ),
            };
            cursor = modal::render_text_prompt_with_hint_cursor(
                frame,
                area,
                &target.label,
                state,
                "Apply",
                &hint,
                style,
            )
            .or(cursor);
        }
        Mode::LocationPicker(picker) => {
            cursor = crate::location_picker::render_cursor(frame, area, picker).or(cursor);
        }
        Mode::AgentSetupPathPrompt(feature, state) => {
            cursor = modal::render_text_prompt_cursor(
                frame,
                area,
                &format!("{} global instruction file", feature.label()),
                state,
                "Use file",
            )
            .or(cursor);
        }
        Mode::AgentSetupPrompt(state) => crate::setup_prompt::render(frame, area, state),
        Mode::VoicePromptEditor(state) => {
            crate::instruction_settings::render_editor(frame, area, state)
        }
        Mode::SaveAs(_, state) => {
            cursor =
                modal::render_text_prompt_cursor(frame, area, "Save As", state, "Save").or(cursor);
        }
        Mode::ConfirmClose(target) => draw_confirm_close(frame, area, app, *target),
        Mode::WaitingWorkspaceCloseOffer { .. } => {
            let popup_width = 64.min(area.width);
            let popup = modal::centered_fixed_rect(popup_width, 5, area);
            let status_lines = workspace_close_wait_lines(popup_width.saturating_sub(2));
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(status_lines.into_iter().map(Line::from).collect::<Vec<_>>())
                    .block(theme::block(true).title(theme::chrome_title("Worktree close"))),
                popup,
            );
        }
        Mode::ConfirmWorkspaceCloseOffer(pane_id) => {
            let message = app.tree.pane_workspace(*pane_id).map_or_else(
                || "Keep this worktree?".to_string(),
                |workspace| {
                    format!(
                        "Remove the clean, merged worktree at {} too? Enter keeps it.",
                        workspace.worktree_root.display()
                    )
                },
            );
            modal::render_confirm(
                frame,
                area,
                "Remove worktree too?",
                &message,
                modal::DialogActions::confirmation(
                    "Keep",
                    modal::DialogButtonTone::Neutral,
                    "Remove worktree",
                    modal::DialogButtonTone::Danger,
                ),
            );
        }
        Mode::ConfirmRemoveWorkspace(target) => {
            let message = app.tree.pane_workspace(*target).map_or_else(
                || "This pane has no worktree".to_string(),
                |workspace| format!("Remove the worktree at {}? The server checks for changes and running processes before removal. The branch is kept.", workspace.worktree_root.display()),
            );
            modal::render_confirm(
                frame,
                area,
                "Remove worktree?",
                &message,
                modal::DialogActions::confirmation(
                    "Keep",
                    modal::DialogButtonTone::Neutral,
                    "Remove",
                    modal::DialogButtonTone::Danger,
                ),
            );
        }
        Mode::ConfirmSessionRecovery { pane_count } => modal::render_confirm(
            frame,
            area,
            "Restore previous session?",
            &format!(
                "A stored snapshot contains {pane_count} pane(s). Restore it, or discard it and start fresh?"
            ),
            modal::DialogActions::confirmation(
                "Discard",
                modal::DialogButtonTone::Danger,
                "Restore",
                modal::DialogButtonTone::Primary,
            ),
        ),
        Mode::Normal
        | Mode::LeaderPending
        | Mode::NavigationLeaderPending
        | Mode::Move
        | Mode::Settings(_) => {}
    }
    cursor
}

fn workspace_close_wait_lines(inner_width: u16) -> Vec<&'static str> {
    const FULL_STATUS: &str = "Checking whether this worktree can be removed…  Esc cancels";
    if UnicodeWidthStr::width(FULL_STATUS) <= usize::from(inner_width) {
        return vec![FULL_STATUS];
    }
    if inner_width >= 30 {
        return vec!["Checking worktree… Esc cancels"];
    }
    if inner_width >= 19 {
        return vec!["Checking worktree…", "Esc cancels"];
    }
    if inner_width >= 11 {
        return vec!["Checking…", "Esc cancels"];
    }
    if inner_width >= 3 {
        return vec!["Checking…", "Esc"];
    }
    vec!["…"]
}

fn draw_create_board(frame: &mut Frame, area: Rect, state: &CreateBoardState) -> Option<Position> {
    let mut cursor = None;
    let layout = modal::create_board_dialog_layout(area);
    frame.render_widget(Clear, layout.popup);
    let block = theme::block(true).title(theme::chrome_title("New board"));
    frame.render_widget(block, layout.popup);
    cursor = draw_scheduled_input_field(
        frame,
        layout.name_box,
        "Board name",
        &state.name,
        !state.editing_path,
    )
    .or(cursor);

    modal::create_board_storage_control(area, state.storage_kind.label()).render(
        frame,
        crate::value_control::ControlStyles {
            background: Style::new(),
            label: Style::new().fg(Color::Cyan),
            value: theme::selected_style(),
            button: Style::new().fg(Color::Cyan),
            disabled: Style::new().add_modifier(Modifier::DIM),
        },
    );

    cursor = draw_scheduled_input_field(
        frame,
        layout.path_box,
        "Storage path",
        &state.path,
        state.editing_path,
    )
    .or(cursor);
    frame.render_widget(
        Paragraph::new("[ Browse path… ]")
            .style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        layout.browse_button,
    );
    modal::render_dialog_actions(
        frame,
        layout.actions,
        modal::DialogActions::form("Cancel", "Create board"),
    );
    frame.render_widget(
        Paragraph::new("Tab fields · Ctrl+P browse · Ctrl+Space storage list")
            .style(Style::new().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        layout.hint_row,
    );
    cursor
}

/// Draws the close-confirmation popup, falling back to a generic message
/// if the target somehow no longer exists (e.g. removed by another path
/// the same tick) rather than panicking mid-render.
fn draw_confirm_close(frame: &mut Frame, area: Rect, app: &App, target: NodeId) {
    let message = app
        .close_confirmation_message(target)
        .unwrap_or_else(|| "Close this item?".to_string());
    modal::render_confirm(
        frame,
        area,
        "Close this item?",
        &message,
        modal::DialogActions::confirmation(
            "Keep open",
            modal::DialogButtonTone::Neutral,
            "Close",
            modal::DialogButtonTone::Danger,
        ),
    );
}

/// Draws the actionable right-click popup. It is intentionally opaque and
/// rendered after panels so no terminal content leaks through its commands.
fn draw_context_menu(frame: &mut Frame, menu: &ContextMenu, ui: &crate::config::UiSettings) {
    let layout = menu.layout();
    let width = menu.area.width.saturating_sub(2);
    let icon_width = popup_icon_width(ui, menu.actions.iter().map(|action| action.icon_target()));
    let lines = popup_rows(&layout, width, |index| {
        let action = menu.actions[index];
        popup_action_line(
            &menu.label_of(action),
            popup_icon(ui, action.icon_target(), icon_width),
            None,
            action.has_submenu(),
            width,
            popup_action_style(index == menu.selected_index, false),
        )
    });
    let title = popup_title(app_menu_title(menu), &layout);
    let widget = Paragraph::new(lines).block(theme::block(true).title(theme::chrome_title(&title)));
    frame.render_widget(Clear, menu.area);
    frame.render_widget(widget, menu.area);

    let Some(submenu) = &menu.submenu else {
        return;
    };
    let layout = submenu.layout();
    let width = submenu.area.width.saturating_sub(2);
    let icon_width = popup_icon_width(ui, std::iter::once(submenu.parent.icon_target()));
    let lines = popup_rows(&layout, width, |index| {
        let item = &submenu.items[index];
        let checked = if matches!(submenu.parent, crate::app::ContextMenuAction::OrderBy) {
            Some(match &item.action {
                SubmenuItemAction::SetManualTreeOrder { active }
                | SubmenuItemAction::SetPluginTreeOrdering { active, .. } => *active,
                _ => false,
            })
        } else {
            None
        };
        popup_action_line(
            &item.label,
            popup_icon(ui, submenu.parent.icon_target(), icon_width),
            checked,
            false,
            width,
            popup_action_style(
                index == submenu.selected_index,
                item.disabled_reason.is_some(),
            ),
        )
    });
    let title = match submenu.parent {
        crate::app::ContextMenuAction::OrderBy => "Order by".to_string(),
        crate::app::ContextMenuAction::NewAgent(provider) => format!("New {}", provider.label()),
        crate::app::ContextMenuAction::Worktree => "Worktree".to_string(),
        _ => "Actions".to_string(),
    };
    let title = popup_title(&title, &layout);
    let widget = Paragraph::new(lines).block(theme::block(true).title(theme::chrome_title(&title)));
    frame.render_widget(Clear, submenu.area);
    frame.render_widget(widget, submenu.area);
}

fn draw_scheduled_input_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    state: &crate::text_prompt::TextPromptState,
    focused: bool,
) -> Option<Position> {
    let border_style = if focused {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    let block = theme::block(focused)
        .title(theme::chrome_title(label))
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(state.buf.as_str()), inner);
    if !focused || inner.width == 0 || inner.height == 0 {
        return None;
    }
    let prefix: String = state.buf.chars().take(state.cursor).collect();
    let cursor_offset = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
    let cursor_position = Position::new(
        inner
            .x
            .saturating_add(cursor_offset)
            .min(inner.right().saturating_sub(1)),
        inner.y,
    );
    frame.set_cursor_position(cursor_position);
    Some(cursor_position)
}

/// Draws the line-specific right-click action without implying that its file
/// target is the currently selected tree node.
fn draw_editor_line_context_menu(
    frame: &mut Frame,
    menu: &EditorLineContextMenu,
    ui: &crate::config::UiSettings,
) {
    let lines: Vec<Line> = menu
        .actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let style = if index == menu.selected_index {
                Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::new()
            };
            Line::from(Span::styled(
                format!(
                    "{} {}",
                    context_menu_icon(ui, action.icon_target()),
                    action.label()
                ),
                style,
            ))
        })
        .collect();
    let widget =
        Paragraph::new(lines).block(theme::block(true).title(theme::chrome_title("Line actions")));
    frame.render_widget(Clear, menu.area);
    frame.render_widget(widget, menu.area);
}

/// Draws the Codex Sol/Astra/Luna reasoning-strength submenu -- one row per
/// `agent_toolbar::codex_reasoning_levels(tier_index)` entry, highlighting
/// the row under keyboard/mouse selection. Each level gets a distinct
/// growth-progression glyph (mirroring `agent_toolbar`'s
/// `CLAUDE_MODEL_ICONS`), since Low..Ultra is itself a progression.
fn draw_agent_toolbar_model_submenu(frame: &mut Frame, state: &AgentToolbarModelSubmenuState) {
    const LEVEL_ICONS: [&str; 6] = [
        "\u{b7}", "\u{25d4}", "\u{25d1}", "\u{25d5}", "\u{25cf}", "\u{2726}",
    ];
    // Invariant: `tier_index` is only ever constructed by enumerating
    // `CODEX_MODEL_TIERS` itself (see the `index as u8` in
    // `agent_toolbar::render`'s button loop), so it is always in bounds here.
    let tier = &crate::agent_toolbar::CODEX_MODEL_TIERS[usize::from(state.tier_index)];
    let levels = crate::agent_toolbar::codex_reasoning_levels(usize::from(state.tier_index));
    let lines: Vec<Line> = levels
        .iter()
        .enumerate()
        .map(|(index, level)| {
            let style = if index == state.selected_index {
                Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::new()
            };
            let icon = LEVEL_ICONS.get(index).copied().unwrap_or("\u{b7}");
            Line::from(Span::styled(format!(" {icon} {}", level.label), style))
        })
        .collect();
    let widget = Paragraph::new(lines).block(theme::block(true).title(theme::chrome_title(
        &format!("{} {}", tier.glyph, tier.label),
    )));
    frame.render_widget(Clear, state.area);
    frame.render_widget(widget, state.area);
}

/// Draws terminal copy actions without implying that a plain shell is an agent.
fn draw_terminal_pane_context_menu(
    frame: &mut Frame,
    menu: &crate::terminal_context_menu::TerminalPaneContextMenu,
    ui: &crate::config::UiSettings,
) {
    let layout = menu.layout();
    let width = menu.area.width.saturating_sub(2);
    let icon_width = popup_icon_width(ui, menu.actions.iter().map(|action| action.icon_target()));
    let lines = popup_rows(&layout, width, |index| {
        let action = &menu.actions[index];
        let unavailable = matches!(
            action,
            crate::terminal_context_menu::TerminalContextAction::LastSubmittedPromptUnavailable
        );
        popup_action_line(
            &action.label(),
            popup_icon(ui, action.icon_target(), icon_width),
            None,
            false,
            width,
            popup_action_style(index == menu.selected_index, unavailable),
        )
    });
    let title = popup_title("Terminal actions", &layout);
    let widget = Paragraph::new(lines).block(theme::block(true).title(theme::chrome_title(&title)));
    frame.render_widget(Clear, menu.area);
    frame.render_widget(widget, menu.area);
}

/// Measure one gutter across the menu so narrow and wide glyphs align labels.
fn popup_icon_width(
    ui: &crate::config::UiSettings,
    targets: impl Iterator<Item = IconTarget>,
) -> usize {
    if !ui.show_context_menu_icons {
        return 0;
    }
    targets
        .map(|target| UnicodeWidthStr::width(ui.icons.glyph(target)))
        .max()
        .unwrap_or(0)
}

fn popup_icon(ui: &crate::config::UiSettings, target: IconTarget, width: usize) -> String {
    if !ui.show_context_menu_icons {
        return String::new();
    }
    let glyph = ui.icons.glyph(target);
    format!(
        "{glyph}{}",
        " ".repeat(width.saturating_sub(UnicodeWidthStr::width(glyph)))
    )
}

/// All popup surfaces share decoration, horizontal padding and selection.
fn popup_rows(
    layout: &crate::context_menu_layout::MenuLayout,
    width: u16,
    action_line: impl Fn(usize) -> Line<'static>,
) -> Vec<Line<'static>> {
    use crate::context_menu_layout::MenuRow;
    layout
        .rows
        .iter()
        .map(|row| match row {
            MenuRow::Padding => Line::from(""),
            MenuRow::Separator => Line::from(Span::styled(
                format!("  {}", "─".repeat(usize::from(width.saturating_sub(4)))),
                theme::border_style(false),
            )),
            MenuRow::Action(index) => action_line(*index),
        })
        .collect()
}

fn popup_action_style(selected: bool, muted: bool) -> Style {
    let style = if selected {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    if muted {
        style.add_modifier(Modifier::DIM)
    } else {
        style
    }
}

fn popup_action_line(
    label: &str,
    icon: String,
    checked: Option<bool>,
    submenu: bool,
    width: u16,
    style: Style,
) -> Line<'static> {
    let marker = match checked {
        Some(true) => "✓ ",
        Some(false) => "  ",
        None => "",
    };
    let icon = icon.trim_start();
    let prefix = if icon.is_empty() {
        format!("  {marker}")
    } else {
        format!("  {marker}{icon}  ")
    };
    let suffix = if submenu { " ▸ " } else { "  " };
    let available = usize::from(width)
        .saturating_sub(UnicodeWidthStr::width(prefix.as_str()) + UnicodeWidthStr::width(suffix));
    let mut clipped = String::new();
    let mut columns = 0;
    for grapheme in label.graphemes(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if columns + grapheme_width > available {
            break;
        }
        clipped.push_str(grapheme);
        columns += grapheme_width;
    }
    let gap = " ".repeat(available.saturating_sub(columns));
    Line::from(Span::styled(
        format!("{prefix}{clipped}{gap}{suffix}"),
        style,
    ))
}

fn popup_title(base: &str, layout: &crate::context_menu_layout::MenuLayout) -> String {
    format!(
        "{base}{}{}",
        if layout.has_rows_above { " ↑" } else { "" },
        if layout.has_rows_below { " ↓" } else { "" }
    )
}

/// Keeps menu renderers text-only when the accessibility preference is off,
/// while every enabled menu entry receives its current configurable glyph.
fn context_menu_icon(ui: &crate::config::UiSettings, target: IconTarget) -> String {
    if ui.show_context_menu_icons {
        format!(" {}", ui.icons.glyph(target))
    } else {
        String::new()
    }
}

/// Draws the agent selector, editable multi-line prompt, and explicit submit
/// button using geometry shared with `crate::mouse`.
fn draw_create_agent_from_line(
    frame: &mut Frame,
    screen_area: Rect,
    state: &CreateAgentFromLineState,
) {
    let layout = crate::agent_from_line::dialog_layout(screen_area);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Create agent from line")),
        layout.popup,
    );

    let selector_style = if state.focus == CreateAgentFocus::AgentType {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    crate::agent_from_line::provider_control(screen_area, state).render(
        frame,
        crate::value_control::ControlStyles {
            background: Style::new(),
            label: selector_style,
            value: selector_style,
            button: selector_style,
            disabled: Style::new().add_modifier(Modifier::DIM),
        },
    );

    let prompt_border_style = if state.focus == CreateAgentFocus::Prompt {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    let prompt_block = theme::block(state.focus == CreateAgentFocus::Prompt)
        .title(theme::chrome_title("Task prompt"))
        .border_style(prompt_border_style);
    let prompt_inner = prompt_block.inner(layout.prompt_area);
    frame.render_widget(prompt_block, layout.prompt_area);
    frame.render_widget(&state.prompt, prompt_inner);

    let button_style = if state.focus == CreateAgentFocus::CreateButton {
        Style::new()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    };
    frame.render_widget(
        Paragraph::new(Span::styled("[ Create agent ]", button_style)).alignment(Alignment::Center),
        layout.create_button,
    );
    frame.render_widget(
        Paragraph::new("Tab field · Enter newline · Ctrl+Enter create · Esc cancel")
            .style(Style::new().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        layout.hint_row,
    );
}

/// Uses the target identifier only as context; the menu labels make the
/// command's effect clear without duplicating potentially long tree names.
fn app_menu_title(_menu: &ContextMenu) -> &'static str {
    "Tree actions"
}

/// Draws the focused pane's live content (terminal screen or editor
/// buffer), or a placeholder when nothing is focused.
#[cfg(test)]
fn draw_pane(frame: &mut Frame, area: Rect, app: &App) {
    let _ = draw_pane_sources(frame, area, app);
}
/// Centred dialog with the full, wrapped text of a pane's last prompt and
/// Copy / Close buttons on its bottom border.
fn draw_last_prompt_dialog(
    frame: &mut Frame,
    screen: Rect,
    state: &crate::last_prompt_toolbar::LastPromptDialogState,
) {
    let dialog = crate::last_prompt_toolbar::dialog_area(screen);
    frame.render_widget(Clear, dialog);
    let block = theme::block(true).title(theme::chrome_title("Last prompt"));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);
    let total_lines = u16::try_from(
        crate::last_prompt_banner::wrap_lines(&state.text, inner.width.saturating_sub(2)).len(),
    )
    .unwrap_or(u16::MAX);
    let max_scroll = total_lines.saturating_sub(inner.height);
    let text_area = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    frame.render_widget(
        Paragraph::new(state.text.as_str())
            .wrap(ratatui::widgets::Wrap { trim: false })
            .scroll((state.scroll.min(max_scroll), 0)),
        text_area,
    );
    for (rect, label) in [
        (
            crate::last_prompt_toolbar::dialog_copy_button(dialog),
            crate::last_prompt_toolbar::dialog_copy_label(),
        ),
        (
            crate::last_prompt_toolbar::dialog_close_button(dialog),
            crate::last_prompt_toolbar::dialog_close_label(),
        ),
    ] {
        frame.render_widget(Paragraph::new(label), rect);
    }
}

fn draw_pane_sources(
    frame: &mut Frame,
    area: Rect,
    app: &App,
) -> Vec<(ilium_core::NodeId, terminal_view::PaintedTerminal)> {
    if matches!(app.right_panel_target, RightPanelTarget::Resource { .. }) {
        app.resource_detail(frame, area);
        return Vec::new();
    }
    crate::plugin_host::views::release_hidden(app);
    if matches!(app.right_panel_target, RightPanelTarget::PluginView { .. }) {
        crate::plugin_host::views::draw(frame, area, app);
        return Vec::new();
    }
    let viewports = app.pane_viewports();
    if viewports.is_empty() {
        let (title, message) = match app.right_panel_target {
            RightPanelTarget::SplitView { split_id, .. } => (
                app.tree
                    .get(split_id)
                    .map(|node| node.name.as_str())
                    .unwrap_or("Split view"),
                "Split view is empty\nAdd up to four panes from the tree",
            ),
            _ => ("Terminal", "no pane selected"),
        };
        // Placeholder copy gets the same inner breathing room as the pane frames.
        let placeholder = Paragraph::new(message)
            .block(
                theme::block(false)
                    .title(theme::chrome_title(title))
                    .padding(ratatui::widgets::Padding::new(2, 1, 1, 0)),
            )
            .style(Style::new().add_modifier(Modifier::DIM));
        frame.render_widget(placeholder, area);
        return Vec::new();
    }

    let mut sources = Vec::with_capacity(viewports.len());
    for viewport in viewports {
        draw_pane_runtime(frame, app, viewport, &mut sources);
    }
    sources
}

fn blocking_terminal_error(error: &crate::status_icons::TooltipContent) -> String {
    match error.reason.as_deref() {
        Some(reason) => format!("{}\n{reason}", error.body),
        None => error.body.clone(),
    }
}

fn initial_terminal_error(
    term: &terminal_view::TerminalView,
    blocking_error: Option<&crate::status_icons::TooltipContent>,
) -> Option<String> {
    if let Some(error) = blocking_error {
        return Some(blocking_terminal_error(error));
    }
    term.admission_error.clone().or_else(|| {
        (term.frontend.is_some() && !term.has_initial_display())
            .then(|| "waiting for initial terminal state".to_string())
    })
}

fn draw_pane_runtime(
    frame: &mut Frame,
    app: &App,
    viewport: crate::split_layout::PaneViewport,
    sources: &mut Vec<(ilium_core::NodeId, terminal_view::PaintedTerminal)>,
) {
    let pane_focused =
        matches!(app.focus, FocusTarget::Pane) && app.active_pane_id() == Some(viewport.pane_id);
    let pane_title = pane_title(app, viewport.pane_id);
    // A plugin pane has no local runtime: its owner plugin draws the body (gap G14b).
    if crate::plugin_host::plugin_panes::draw(frame, app, &viewport, pane_focused, &pane_title) {
        return;
    }
    let Some(runtime) = app.panes.get(&viewport.pane_id) else {
        let blocking_error = app
            .blocking_error_tooltip(viewport.pane_id)
            .map(|error| blocking_terminal_error(&error));
        let placeholder = Paragraph::new(
            blocking_error
                .as_deref()
                .or_else(|| app.editor_load_error(viewport.pane_id))
                .unwrap_or("pane is loading"),
        )
        .wrap(ratatui::widgets::Wrap { trim: true })
        .block(theme::block(pane_focused).title(theme::chrome_title(&pane_title)));
        frame.render_widget(placeholder, viewport.outer_area);
        return;
    };

    match runtime {
        PaneRuntime::Terminal(term) => {
            let block = theme::block(pane_focused).title(theme::chrome_title(&pane_title));
            frame.render_widget(block, viewport.outer_area);
            let frozen_dialog = app.frozen_dialog_layout(viewport);
            let terminal_area = frozen_dialog
                .as_ref()
                .map_or(viewport.content_area, |layout| layout.terminal_area);
            if let Some(source) = app.frozen_screens.get(&viewport.pane_id) {
                source.with_screen(|screen| {
                    terminal_view::render_frozen_screen(screen, terminal_area, frame.buffer_mut());
                });
                if let Some(dialog) = frozen_dialog.as_ref() {
                    frame.render_widget(Clear, dialog.dialog_area);
                    frame.render_widget(
                        Paragraph::new(vec![
                            Line::from(Span::styled(
                                "Agent frozen",
                                Style::default().add_modifier(Modifier::BOLD),
                            )),
                            Line::from("Click the button or press Enter to resume it"),
                            Line::from(Span::styled(
                                FROZEN_DIALOG_BUTTON_LABEL,
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::LightGreen)
                                    .add_modifier(Modifier::BOLD),
                            )),
                        ])
                        .alignment(Alignment::Center)
                        .block(theme::block(true).title(theme::chrome_title("Frozen agent"))),
                        dialog.dialog_area,
                    );
                }
            } else if let Some(source) = app.selection_terminal_source(viewport.pane_id) {
                source.with_screen(|screen| {
                    terminal_view::render_frozen_screen(screen, terminal_area, frame.buffer_mut());
                    draw_terminal_selection(app, frame, viewport.pane_id, terminal_area, screen);
                });
            } else {
                term.render_screen(terminal_area, frame.buffer_mut());
                let blocking_error = (!term.has_initial_display())
                    .then(|| app.blocking_error_tooltip(viewport.pane_id))
                    .flatten();
                let display_error = initial_terminal_error(term, blocking_error.as_ref());
                if let Some(error) = display_error.as_deref() {
                    use crate::terminal_parsing::ParserPressure;

                    let message = match term.parser_pressure() {
                        Some(ParserPressure::StatePool) => format!(
                            "Loading terminal: {error} Turn pooling Off (0) or increase its budget in Settings > Terminal."
                        ),
                        Some(ParserPressure::SnapshotPool) => format!(
                            "Loading terminal: {error} Retained snapshot memory returns only after its last holder releases it. Turn pooling Off (0) or increase its budget in Settings > Terminal."
                        ),
                        Some(ParserPressure::PaneLimit) => format!(
                            "Terminal allocation limit: {error} The 128 MiB per-pane operation cap remains in effect with pooling Off."
                        ),
                        Some(ParserPressure::StorageBusy) => {
                            format!("Loading terminal: {error} Retrying allocation bookkeeping.")
                        }
                        Some(ParserPressure::ProcessStorage) => format!(
                            "Loading terminal: {error} The parser storage ledger refused this reservation; safe hidden engines may be reclaimed while active and retiring owners keep their credits."
                        ),
                        None if term.frontend.is_none() || term.applied_ordinal == 0 => {
                            format!("Loading terminal: {error}")
                        }
                        None => format!("Terminal status: {error}"),
                    };
                    let status_area = {
                        let buffer = frame.buffer_mut();
                        let mut run_start = terminal_area.y;
                        let mut run_height = 0_u16;
                        let mut best_start = terminal_area.y;
                        let mut best_height = 0_u16;

                        // Inspect rendered cells so clipping and terminal dimensions remain unchanged.
                        for row in terminal_area.y..terminal_area.bottom() {
                            let row_is_blank =
                                (terminal_area.x..terminal_area.right()).all(|column| {
                                    buffer.content[buffer.index_of(column, row)]
                                        .symbol()
                                        .chars()
                                        .all(char::is_whitespace)
                                });
                            if !row_is_blank {
                                run_height = 0;
                                continue;
                            }
                            if run_height == 0 {
                                run_start = row;
                            }
                            run_height += 1;
                            if run_height <= best_height {
                                continue;
                            }
                            best_start = run_start;
                            best_height = run_height;
                        }

                        (best_height != 0).then_some(ratatui::layout::Rect::new(
                            terminal_area.x,
                            best_start,
                            terminal_area.width,
                            best_height,
                        ))
                    };
                    // Never cover retained terminal text when a full screen has no blank rows.
                    if let Some(status_area) = status_area {
                        frame.render_widget(
                            ratatui::widgets::Paragraph::new(message)
                                .wrap(ratatui::widgets::Wrap { trim: true })
                                .style(
                                    ratatui::style::Style::default()
                                        .add_modifier(ratatui::style::Modifier::DIM),
                                ),
                            status_area,
                        );
                    }
                }
                crate::goal_resume_link::draw_goal_resume_link(
                    app,
                    viewport.pane_id,
                    term.as_ref(),
                    terminal_area,
                    frame.buffer_mut(),
                );
                term.with_screen(|screen| {
                    // Highlight against the same rect the screen was just drawn
                    // into -- when a completed-agent close action reserves the
                    // bottom row, `terminal_area` is shorter than
                    // `viewport.content_area`, and mapping the selection to the
                    // full content area would let it claim a row of cells that
                    // were never actually painted with terminal content.
                    draw_terminal_selection(app, frame, viewport.pane_id, terminal_area, screen);
                });
            }
            let source = app
                .selection_terminal_source(viewport.pane_id)
                .cloned()
                .unwrap_or_else(|| term.painted_source());
            sources.push((viewport.pane_id, source));
            if let Some(source) = app.selection_terminal_source(viewport.pane_id) {
                draw_terminal_scrollbar_metrics(
                    frame,
                    viewport.outer_area,
                    source.scrollback_metrics(),
                );
            } else {
                draw_terminal_scrollbar(frame, viewport.outer_area, term.as_ref());
            }
        }
        PaneRuntime::Editor(editor) => {
            let block = theme::block(pane_focused).title(theme::chrome_title(&pane_title));
            let inner = block.inner(viewport.outer_area);
            frame.render_widget(block, viewport.outer_area);
            draw_editor(frame, inner, editor.as_ref());
        }
        PaneRuntime::Board(board) => {
            let block = theme::block(pane_focused).title(theme::chrome_title(&pane_title));
            let inner = block.inner(viewport.outer_area);
            frame.render_widget(block, viewport.outer_area);
            crate::board_ui::render(
                frame,
                inner,
                board.as_ref(),
                app.kanban_board_settings.card_preview_lines,
                app.kanban_board_settings.minimum_column_width,
            );
        }
    }

    draw_stats_icon(frame, app, viewport);
    draw_recap_icon(frame, app, viewport);

    if let Some(toolbar_area) = viewport.toolbar_area {
        if matches!(runtime, PaneRuntime::Terminal(_)) {
            if app.shows_agent_toolbar(viewport.pane_id) {
                let below_row = Rect::new(
                    viewport.content_area.x,
                    viewport.content_area.y,
                    viewport.content_area.width,
                    viewport.content_area.height.min(1),
                );
                let hovered = app
                    .hovered_agent_toolbar_action
                    .and_then(|(pane_id, action)| (pane_id == viewport.pane_id).then_some(action));
                crate::agent_toolbar::render(
                    frame,
                    toolbar_area,
                    below_row,
                    crate::agent_toolbar::ToolbarContext {
                        provider: app.agent_toolbar_provider(viewport.pane_id),
                        icons: &app.ui_settings.icons,
                        effort: app
                            .agent_toolbar_effort
                            .get(&viewport.pane_id)
                            .copied()
                            .unwrap_or_default(),
                        show_labels: app.ui_settings.show_toolbar_labels,
                        selection_enabled: app.ui_settings.terminal_text_selection_enabled,
                        show_prompt_reopen: app.last_prompt_reopen_available(viewport.pane_id),
                        show_smart_copy: app.smart_copy_available(),
                        show_goal: app.ui_settings.goal_toolbar_icon_enabled
                            && app.current_goal_phase(viewport.pane_id).is_some(),
                    },
                    hovered,
                );
            }
        }
    }

    if matches!(runtime, PaneRuntime::Terminal(_)) {
        draw_section_rules(frame, app, viewport, pane_focused);
    }

    if let Some(last_prompt_area) = viewport.last_prompt_area {
        if matches!(runtime, PaneRuntime::Terminal(_))
            && app.shows_last_prompt_banner(viewport.pane_id)
        {
            let tabs = app.ui_settings.goal_tab_enabled
                && app.current_goal_phase(viewport.pane_id).is_some()
                && crate::last_prompt_banner::tab_rects(
                    last_prompt_area,
                    crate::last_prompt_toolbar::has_toolbar(last_prompt_area.width),
                )
                .is_some();
            let selected_goal = tabs && app.goal_tab_selected.contains(&viewport.pane_id);
            let goal_text = selected_goal.then(|| {
                app.goal_observation(viewport.pane_id)
                    .map(crate::session_goal_display::goal_banner_text)
                    .unwrap_or_else(|| {
                        format!(
                            "{}: Goal text unavailable",
                            app.current_goal_phase(viewport.pane_id).map_or(
                                "Status unavailable",
                                crate::session_goal_display::phase_label
                            )
                        )
                    })
            });
            let prompt = if selected_goal {
                goal_text.as_deref()
            } else {
                app.display_last_agent_prompt(viewport.pane_id)
                    .or(tabs.then_some(""))
            };
            crate::last_prompt_banner::render(
                frame,
                last_prompt_area,
                prompt,
                app.ui_settings.last_prompt_max_lines.into(),
                app.ui_settings.color_scheme,
                crate::last_prompt_banner::BannerControls {
                    toolbar: crate::last_prompt_toolbar::has_toolbar(last_prompt_area.width)
                        .then(|| app.last_prompt_toolbar_visibility(viewport.pane_id)),
                    hovered: app
                        .hovered_last_prompt_button
                        .and_then(|(pane_id, button)| {
                            (pane_id == viewport.pane_id).then_some(button)
                        }),
                    selected_tab: tabs.then_some(if selected_goal {
                        crate::last_prompt_banner::BannerTab::Goal
                    } else {
                        crate::last_prompt_banner::BannerTab::Prompt
                    }),
                },
            );
        }
    }

    draw_plugin_frame_rows(frame, app, viewport);

    if let Some(progress_area) = viewport.progress_area {
        if matches!(runtime, PaneRuntime::Terminal(_))
            && app.shows_progress_footer(viewport.pane_id)
        {
            let native_rows = app.resource_wait_footer_rows(viewport.pane_id, progress_area.height);
            let native_wait = native_rows > 0;
            let ordinary_area = if native_wait {
                Rect::new(
                    progress_area.x,
                    progress_area.y.saturating_add(native_rows),
                    progress_area.width,
                    progress_area.height.saturating_sub(native_rows),
                )
            } else {
                progress_area
            };
            if native_wait {
                app.resource_wait_footer(
                    frame,
                    Rect::new(
                        progress_area.x,
                        progress_area.y,
                        progress_area.width,
                        native_rows,
                    ),
                    viewport.pane_id,
                );
            }
            crate::progress_bar::render_monitors(
                frame,
                ordinary_area,
                &app.visible_progress_monitors(viewport.pane_id),
                app.ui_settings.progress_max_lines.into(),
                app.ui_settings.color_scheme,
                app.progress_display_now_unix_millis(),
            );
        }
    }
}

/// Frame rules around the toolbar, prompt banner and progress footer. Each
/// rule is drawn only when its section has something to show; an empty
/// reserved slot stays blank, exactly as it did before frames existed.
fn draw_section_rules(
    frame: &mut Frame,
    app: &App,
    viewport: crate::split_layout::PaneViewport,
    focused: bool,
) {
    let pane_id = viewport.pane_id;
    let prompt_shown = app.shows_last_prompt_banner(pane_id);
    if let Some(rule) = viewport.toolbar_rule {
        if app.shows_agent_toolbar(pane_id) {
            let title = prompt_shown.then_some("\u{276f} last prompt");
            crate::section_frame::render_rule(frame, rule, title, focused);
        }
    }
    if let Some(rule) = viewport.last_prompt_bottom_rule {
        if prompt_shown {
            crate::section_frame::render_rule(frame, rule, None, focused);
        }
    }
    if let Some(rule) = viewport.progress_rule {
        let monitors = app.visible_progress_monitors(pane_id).len();
        if monitors > 0 {
            let title = if monitors == 1 {
                "progress".to_string()
            } else {
                format!("progress \u{b7} {monitors} tasks")
            };
            crate::section_frame::render_rule(frame, rule, Some(&title), focused);
        }
    }
}

/// Draws a vertical scrollbar merged into the terminal pane block's right
/// border, mirroring `tree_ui::draw_scrollbar` -- shown only once the pane
/// actually has scrollback history to navigate (the leader key isn't involved:
/// see `App::handle_pane_key`/`handle_pane_mouse` for Shift+PageUp/
/// PageDown, Shift+End, and wheel navigation).
fn draw_terminal_scrollbar(frame: &mut Frame, area: Rect, term: &terminal_view::TerminalView) {
    draw_terminal_scrollbar_metrics(
        frame,
        area,
        (
            term.scrollback_total(),
            term.scrollback_position(),
            term.viewport_rows(),
        ),
    );
}
fn draw_terminal_scrollbar_metrics(
    frame: &mut Frame,
    area: Rect,
    (total, position, rows): (usize, usize, u16),
) {
    if total == 0 {
        return;
    }
    let mut scrollbar_state = ScrollbarState::new(total.saturating_add(1))
        .position(total.saturating_sub(position))
        .viewport_content_length(usize::from(rows));
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .style(theme::border_style(false));
    frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

/// Highlights `pane_id`'s active text selection, if any, over its just-drawn
/// terminal content. A no-op for every pane but the one the selection
/// belongs to, and for an unmoved (empty) selection.
fn draw_terminal_selection(
    app: &App,
    frame: &mut Frame,
    pane_id: ilium_core::NodeId,
    content_area: Rect,
    screen: &vt100::Screen,
) {
    let Some(selection) = app
        .terminal_selection
        .as_ref()
        .filter(|selection| selection.pane_id == pane_id)
    else {
        return;
    };
    terminal_selection::render_highlight(frame, content_area, screen.size(), selection);
}

/// Draws one editor pane's chrome (always-visible toolbar, main content,
/// optional minimap) into its content rect.
fn draw_editor(frame: &mut Frame, area: Rect, editor: &EditorPane) {
    let chrome = editor_chrome::compute(area, editor.show_minimap);
    editor_toolbar::render(frame, chrome.toolbar_area, editor);

    match editor.view_mode {
        EditorViewMode::Source => {
            if let Some(window) = editor.installed_window().filter(|window| {
                window.key.width == chrome.content_area.width
                    && window.key.height == chrome.content_area.height
            }) {
                let current = window.key.revision == editor.content_revision()
                    && window.key.path.as_path()
                        == editor.path.as_deref().unwrap_or(std::path::Path::new(""))
                    && window.key.gutter == editor.show_line_numbers
                    && window.key.line_display == editor.line_display
                    && window.key.tab == editor.textarea.tab_length();
                crate::source_window_surface::render(
                    frame,
                    chrome.content_area,
                    window,
                    {
                        let cursor = editor.textarea.cursor();
                        (cursor.0, cursor.1)
                    },
                    editor.textarea.selection_range(),
                    current,
                );
                draw_source_scrollbar(frame, chrome.scrollbar_area, editor);
            } else {
                frame.render_widget(
                    Paragraph::new(editor.preparation_error.as_deref().map_or_else(
                        || "Preparing source…".to_owned(),
                        |error| format!("F9 details · {error}"),
                    ))
                    .wrap(ratatui::widgets::Wrap { trim: false }),
                    chrome.content_area,
                );
            }
        }
        EditorViewMode::Rendered => match &editor.rendered {
            Some(document) => {
                markdown::view::render(
                    frame,
                    chrome.content_area,
                    document,
                    editor.rendered_scroll,
                    editor.line_display,
                );
                draw_rendered_scrollbar(
                    frame,
                    chrome.content_area,
                    chrome.scrollbar_area,
                    document,
                    editor,
                );
            }
            None => {
                frame.render_widget(
                    Paragraph::new(editor.preparation_error.as_deref().map_or_else(
                        || "Rendering…".to_owned(),
                        |error| format!("F9 details · {error}"),
                    ))
                    .wrap(ratatui::widgets::Wrap { trim: false }),
                    chrome.content_area,
                );
            }
        },
    }

    if let Some(minimap_area) = chrome.minimap_area {
        if editor.view_mode == EditorViewMode::Rendered {
            let lines = editor.textarea.lines();
            let highlight = minimap_highlight_line(editor, lines.len(), chrome.content_area.width);
            minimap::render(
                frame,
                minimap_area,
                lines,
                highlight,
                chrome.content_area.width,
            );
        } else if let Some(window) = editor.installed_window() {
            if let Some(minimap) = &window.viewport.minimap {
                crate::minimap::render_prepared(
                    frame,
                    minimap_area,
                    minimap,
                    editor.textarea.cursor().0,
                    editor.textarea.lines().len(),
                );
            }
        }
    }
}

/// Draws a Source-mode scrollbar only when the buffer extends beyond the
/// available editor body. It uses Ratatui's own scrollbar widget and shares
/// the same authoritative viewport position as wheel navigation.
fn draw_source_scrollbar(frame: &mut Frame, area: Rect, editor: &EditorPane) {
    let Some(total_lines) = editor
        .installed_window()
        .and_then(|window| window.viewport.total_rows)
    else {
        return;
    };
    if total_lines <= usize::from(area.height) {
        return;
    }
    let mut scrollbar_state =
        ScrollbarState::new(total_lines).position(usize::from(editor.source_scroll_row()));
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"));
    frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

/// Draws a vertical scrollbar for a Rendered-mode markdown document, only
/// once its content is actually taller than the visible area -- matches
/// `tree_ui::draw_scrollbar`'s same "don't show a full track for content
/// that already fits" rule.
fn draw_rendered_scrollbar(
    frame: &mut Frame,
    text_area: Rect,
    scrollbar_area: Rect,
    document: &markdown::render::RenderedDocument,
    editor: &EditorPane,
) {
    let total_height =
        markdown::view::content_height(document, text_area.width, editor.line_display);
    if total_height <= text_area.height {
        return;
    }
    let mut scrollbar_state = ScrollbarState::new(usize::from(total_height))
        .position(usize::from(editor.rendered_scroll));
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"));
    frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
}

/// The source line the minimap should mark as "you are here": the cursor
/// row in Source mode (known exactly), or the scroll-proportional line in
/// Rendered mode (the rendered view's scroll is measured in rendered
/// rows, not source lines, so this is an approximation across headers/
/// images whose rendered height differs from their one source line).
fn minimap_highlight_line(editor: &EditorPane, total_lines: usize, rendered_width: u16) -> usize {
    match editor.view_mode {
        EditorViewMode::Source => editor.textarea.cursor().0,
        EditorViewMode::Rendered => {
            let Some(document) = &editor.rendered else {
                return 0;
            };
            let total_height =
                markdown::view::content_height(document, rendered_width, editor.line_display)
                    .max(1);
            let fraction = f64::from(editor.rendered_scroll) / f64::from(total_height);
            ((fraction * total_lines as f64).round() as usize).min(total_lines.saturating_sub(1))
        }
    }
}

/// Builds the selected right-panel title from logical pane naming plus
/// whatever agent class the render-cache tree currently knows for it.
///
/// Unlike the pre-client/server design, this never shows a real PID or
/// session ID: those are volatile OS facts the server discovers by
/// walking the pane's actual process tree, and `ilium_ipc::PaneStatus`
/// (the only agent information carried over the wire, via
/// `ServerEvent::PaneStatusChanged`) only carries `AgentClass` +
/// `AgentActivity` -- see `crate::naming_workers`'s module docs for the
/// matching gap on the session-title-inference side. Extending the wire
/// protocol to carry PID/session-id for display is a reasonable future
/// addition, not something this stage's scope covers.
fn pane_title(app: &App, id: NodeId) -> String {
    let Some(node) = app.tree.get(id) else {
        return "Terminal".to_string();
    };
    match &node.kind {
        NodeKind::Pane { status, .. } => {
            let logical_title = match status {
                PaneStatus::Agent(agent) => {
                    format!("{} — {}", node.name, agent_class_title(&agent.class))
                }
                PaneStatus::AgentUnavailable(recovery) => format!(
                    "{} — {} unavailable",
                    node.name,
                    agent_class_title(&recovery.process.class)
                ),
                _ => node.name.clone(),
            };
            let title = logical_title;
            // Header slots plugins contribute (`pane_frame_slots`) follow the title.
            match app.plugin_frame_header_text(id) {
                Some(plugin_text) => format!("{title} {plugin_text}"),
                None => title,
            }
        }
        NodeKind::Container(_) | NodeKind::Folder { .. } => node.name.clone(),
    }
}

/// Draws the plugin banner and footer rows (`pane_frame_slots` placed `banner` and
/// `footer-row`) in the rows [`App::pane_viewports`] reserved for them. The banner uses the
/// native prompt banner's look; the footer is a dim status line.
fn draw_plugin_frame_rows(
    frame: &mut Frame,
    app: &App,
    viewport: crate::split_layout::PaneViewport,
) {
    use crate::plugin_host::views::FrameRowPlacement;
    let rows = [
        (
            viewport.plugin_banner_area,
            FrameRowPlacement::Banner,
            crate::theme::last_prompt_style(app.ui_settings.color_scheme),
        ),
        (
            viewport.plugin_footer_area,
            FrameRowPlacement::FooterRow,
            Style::new().add_modifier(Modifier::DIM),
        ),
    ];
    for (area, placement, style) in rows {
        let Some(area) = area else {
            continue;
        };
        let Some(text) = app.plugin_frame_row_text(viewport.pane_id, placement) else {
            continue;
        };
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(format!(" {text}")).style(style), area);
    }
}

/// Compact, stable class name for the selected-terminal title.
fn agent_class_title(class: &AgentClass) -> &str {
    class.label()
}

/// Draws the one-line status bar: the current mode, plus any pending
/// status message. Rendered as a rounded pill -- inset by one column on
/// each side, with a powerline round-cap glyph closing off each end --
/// rather than a bar that runs flush into the screen's edges.
fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    if area.is_empty() {
        return;
    }

    // Every arm here is a compile-time-constant label -- borrowing a
    // `&'static str` instead of building a fresh owned `String` avoids a
    // needless heap allocation on every single render frame (this function
    // runs on the redraw hot path, potentially many times per second).
    let mode_label: &'static str = match &app.mode {
        Mode::Normal => "NORMAL",
        Mode::LeaderPending => "LEADER (press a letter — ? for help)",
        Mode::NavigationLeaderPending => "TREE NAVIGATION (n p ( ))",
        Mode::Move => "MOVE",
        // The buffer itself is shown in the modal popup (see `draw`), not
        // here -- the status bar only names the mode while one is open.
        Mode::CommandPrompt(_) => "RUN COMMAND",
        Mode::InferenceSettingPrompt(_, _) => "INFERENCE SETTING",
        Mode::VoiceSettingPrompt(_, _) => "VOICE SETTING",
        Mode::ApiSettingPrompt(_) => "HTTP API PORT",
        Mode::PluginSettingPrompt(_, _) => "PLUGIN SETTING",
        Mode::GitSettingPrompt(_, _) => "GIT SETTING",
        Mode::AnimationTextPrompt(_, _) => "ANIMATION SETTING",
        Mode::LocationPicker(_) => "LOCATION",
        Mode::AgentSetupPathPrompt(_, _) => "AGENT SETUP FILE",
        Mode::AgentSetupPrompt(_) => "AGENT SETUP",
        Mode::VoicePromptEditor(_) => "VOICE PROMPT",
        Mode::SaveAs(..) => "SAVE AS",
        Mode::Explorer(..) => "FILE PICKER",
        Mode::ExplorerFileMenu(_) => "FILE ACTIONS",
        Mode::FolderExplorer(..) => "FOLDER PICKER",
        Mode::ProjectFolderExplorer(..) => "PROJECT FOLDER PICKER",
        Mode::PluginFilePicker(..) => "PLUGIN FILE PICKER",
        Mode::ContextMenu(..) => "TREE ACTIONS",
        Mode::TerminalPaneContextMenu(..) => "TERMINAL ACTIONS",
        Mode::AgentToolbarModelSubmenu(..) => "MODEL STRENGTH",
        Mode::LastPromptDialog(..) => "LAST PROMPT",
        Mode::ValueDialog(..) => "VALUE OPTIONS",
        Mode::EditorLineContextMenu(..) => "LINE ACTIONS",
        Mode::CreateAgentFromLine(..) => "CREATE AGENT",
        Mode::CreateAgentWorkspace(..) => "CREATE AGENT WORKTREE",
        Mode::WorktreeManager(..) => "WORKTREES",
        Mode::WaitingWorkspaceCloseOffer { .. } => "CHECKING WORKTREE",
        Mode::ConfirmWorkspaceCloseOffer(..) => "WORKTREE CLOSE",
        Mode::PluginPopup(_) => "PLUGIN DIALOG",
        Mode::CreateBoard(_) => "NEW BOARD",
        Mode::BoardPathPicker(_) => "BOARD PATH",
        Mode::BoardCardPrompt(_, _) => "NEW CARD",
        Mode::BoardColumnPrompt(_, _) => "NEW COLUMN",
        Mode::BoardRenamePrompt(_, _, _) => "RENAME BOARD ITEM",
        Mode::BoardDeleteConfirm(_, _) => "DELETE BOARD ITEM",
        Mode::ConfirmClose(_) => "CONFIRM CLOSE",
        Mode::ConfirmRemoveWorkspace(_) => "REMOVE WORKTREE",
        Mode::ConfirmSessionRecovery { .. } => "SESSION RECOVERY",
        // Unreachable in practice -- `draw` returns before this ever runs
        // while `Mode::Settings` is active (the settings view replaces the
        // whole screen, status bar included). Kept as a real arm rather
        // than a wildcard so this stays exhaustive if that early return is
        // ever removed.
        Mode::Settings(_) => "SETTINGS",
        Mode::SettingsHelp(_) => "SETTINGS HELP",
    };

    let bar_style = theme::statusbar_style();
    let mut spans = vec![
        Span::raw("\u{2139} "),
        Span::styled(mode_label, bar_style.add_modifier(Modifier::BOLD)),
    ];
    if app.structure_loading {
        let elapsed_ms = app.started_at.elapsed().as_millis();
        let frame_index =
            (elapsed_ms / tree_ui::SPINNER_FRAME_MS) as usize % tree_ui::SPINNER_FRAMES.len();
        let spinner = tree_ui::SPINNER_FRAMES[frame_index];
        spans.push(Span::raw("  —  "));
        spans.push(Span::styled(
            app.restructure_status_text()
                .unwrap_or_else(|| format!("{spinner} Restructuring projects with AI…")),
            bar_style.add_modifier(Modifier::BOLD),
        ));
    } else if let Some(status) = app.restructure_status_text() {
        spans.push(Span::raw("  —  "));
        spans.push(Span::raw(status));
    }
    if let Some(activity) = app.pending_menu_activity_text(std::time::Instant::now()) {
        spans.push(Span::raw("  —  "));
        spans.push(Span::styled(
            activity,
            bar_style.add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(notice) = app.request_refusal_notice(std::time::Instant::now()) {
        spans.push(Span::raw("  —  "));
        spans.push(Span::styled(notice, bar_style.add_modifier(Modifier::BOLD)));
    }
    if let Some(segments) = app.plugin_status_text() {
        spans.push(Span::raw("  —  "));
        spans.push(Span::raw(segments));
    }
    if let Some(message) = &app.status_message {
        spans.push(Span::raw("  —  "));
        // Borrow rather than `message.clone()` -- `app` outlives this
        // function's local `spans`, so there is no need to allocate a new
        // `String` copy of the status message on every render frame.
        spans.push(Span::raw(format!("F9 details · {message}")));
    } else if let Some(error) = app.semantic_animation_error() {
        spans.push(Span::raw("  —  "));
        spans.push(Span::raw(format!("F9 details · {error}")));
    }

    let cap_style = theme::statusbar_cap_style();
    let left_cap = Rect::new(area.x, area.y, 1, area.height);
    let right_cap = Rect::new(area.right().saturating_sub(1), area.y, 1, area.height);
    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );

    frame.render_widget(
        Paragraph::new(theme::STATUSBAR_CAP_LEFT).style(cap_style),
        left_cap,
    );
    frame.render_widget(Paragraph::new("").style(bar_style), inner);
    frame.render_widget(Paragraph::new(Line::from(spans)).style(bar_style), inner);
    frame.render_widget(
        Paragraph::new(theme::STATUSBAR_CAP_RIGHT).style(cap_style),
        right_cap,
    );
}

/// Draws the persistent voice affordance to the right of the purple status
/// bar. The dot represents the persisted on/off state; the adjacent label
/// exposes the live transport state without turning provider details into UI
/// dependencies.
fn draw_voice_control(frame: &mut Frame, area: Rect, app: &App) {
    if area.is_empty() {
        return;
    }

    let is_enabled = app.voice_settings.enabled;
    let is_recording = matches!(
        app.voice_connection_state,
        ilium_voice::VoiceConnectionState::Recording
    );
    let dot_style = if is_enabled {
        let style = Style::new().fg(Color::Red);
        if is_recording {
            style.add_modifier(Modifier::BOLD | Modifier::RAPID_BLINK)
        } else {
            style.add_modifier(Modifier::BOLD)
        }
    } else {
        Style::new().fg(Color::Black)
    };
    let state_label = if !is_enabled {
        "OFF"
    } else {
        match &app.voice_connection_state {
            ilium_voice::VoiceConnectionState::Disabled => "STARTING",
            ilium_voice::VoiceConnectionState::Connecting => "CONNECTING",
            ilium_voice::VoiceConnectionState::Listening => "LISTENING",
            ilium_voice::VoiceConnectionState::Recording => "RECORDING",
            ilium_voice::VoiceConnectionState::Thinking => "THINKING",
            ilium_voice::VoiceConnectionState::Speaking => "SPEAKING",
            ilium_voice::VoiceConnectionState::Failed(_) => "FAILED",
        }
    };
    let shortcut = if matches!(
        app.voice_settings.input_mode,
        ilium_voice::VoiceInputMode::PushToTalk
    ) {
        "F8 hold"
    } else {
        "F8"
    };
    let content = Line::from(vec![
        Span::styled(" ● ", dot_style),
        Span::styled(
            format!("VOICE {state_label}"),
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {shortcut}"), Style::new().fg(Color::DarkGray)),
    ]);
    frame.render_widget(
        Paragraph::new(content)
            .alignment(Alignment::Center)
            .style(Style::new().bg(Color::Rgb(24, 24, 28))),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{PaneRuntime, RightPanelTarget};
    use crate::terminal_view::TerminalView;
    use ilium_core::{AgentActivity, GoalState, PaneContentKind, SplitOrientation};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use ratatui_textarea::TextArea;
    use std::path::PathBuf;

    #[test]
    fn terminal_placeholder_prioritizes_concrete_blocking_failure() {
        let mut term = TerminalView::new(24, 80);
        term.admission_error = Some("parser allocation unavailable".into());
        let error = crate::status_icons::TooltipContent {
            title: "Cannot start terminal".into(),
            body: "villainess-study: start agent: git exited 128".into(),
            reason: Some("fatal: malformed gitfile\nFix the repository metadata".into()),
        };
        let placeholder = initial_terminal_error(&term, Some(&error)).unwrap();
        assert!(placeholder.contains("git exited 128"));
        assert!(placeholder.contains("fatal: malformed gitfile"));
        assert!(!placeholder.contains("waiting for initial terminal state"));
        assert_eq!(
            initial_terminal_error(&term, None).as_deref(),
            Some("parser allocation unavailable")
        );
    }

    #[test]
    fn status_tooltip_values_advance_only_after_the_original_row_is_emitted() {
        let mut app = App::new("painted-tooltip".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "original shell", PaneContentKind::Terminal)
            .unwrap();
        app.tree_state.select(vec![group, pane]);
        app.tree_state.open(vec![group]);
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Working, None),
            )
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| {
                draw_base_layer(frame, frame.area(), &mut app);
            })
            .unwrap();
        let original = app.capture_emitted_geometry(71);
        let original_value =
            status_tooltip_content(&app, pane, crate::status_icons::StatusSlot::Now).unwrap();
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Codex, AgentActivity::WaitingApproval, None),
            )
            .unwrap();
        let newer_value =
            status_tooltip_content(&app, pane, crate::status_icons::StatusSlot::Now).unwrap();
        assert_ne!(original_value, newer_value);
        app.commit_emitted_geometry(original);
        assert_eq!(
            app.emitted_status_tooltip(pane, crate::status_icons::StatusSlot::Now),
            Some(&original_value)
        );
        terminal
            .draw(|frame| {
                draw_base_layer(frame, frame.area(), &mut app);
            })
            .unwrap();
        let newer = app.capture_emitted_geometry(72);
        assert_eq!(
            app.emitted_status_tooltip(pane, crate::status_icons::StatusSlot::Now),
            Some(&original_value)
        );
        app.commit_emitted_geometry(newer);
        assert_eq!(
            app.emitted_status_tooltip(pane, crate::status_icons::StatusSlot::Now),
            Some(&newer_value)
        );
    }

    #[test]
    fn attention_mode_hides_tooltips_for_status_slots_without_a_glyph() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "waiting agent", PaneContentKind::Terminal)
            .unwrap();
        let status = PaneStatus::from_activity(
            AgentClass::Codex,
            AgentActivity::WaitingApproval,
            Some(GoalState::Blocked),
        );
        app.tree.set_pane_status(pane_id, status.clone()).unwrap();
        // This test exercises the fresh approval projection; unchecked agents
        // intentionally display a freshness warning instead of live approval.
        app.pane_detection_evidence.insert(
            pane_id,
            ilium_ipc::PaneDetectionEvidence {
                applied_status: status,
                observed_at_unix_millis: crate::wall_clock::unix_millis_now(),
                stale_after_millis: 30_000,
                identity: None,
                activity: Some(ilium_ipc::DetectionReason {
                    rule: "approval prompt".to_string(),
                    observed: Some(
                        include_str!(
                            "../../ilium-detect/tests/fixtures/codex_awaiting_approval.txt"
                        )
                        .lines()
                        .next()
                        .unwrap()
                        .to_string(),
                    ),
                    context: "Captured Codex approval fixture".to_string(),
                }),
                goal: None,
            },
        );
        app.ui_settings.agent_monitoring_mode =
            crate::agent_monitoring::AgentMonitoringMode::Attention;
        app.set_screen_area(Rect::new(0, 0, 100, 24));
        app.hovered_status_slot = Some((
            pane_id,
            crate::status_icons::StatusSlot::Objective,
            Position::new(12, 3),
        ));

        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|frame| draw_status_tooltip(frame, &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(
            !rendered.contains("Goal stalled"),
            "Attention mode does not draw the blocked-goal icon when approval is selected"
        );

        app.hovered_status_slot = Some((
            pane_id,
            crate::status_icons::StatusSlot::Now,
            Position::new(15, 3),
        ));
        terminal
            .draw(|frame| draw_status_tooltip(frame, &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        let rendered = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(rendered.contains("Needs your approval"));
        assert!(rendered.contains("Attention selection rule:"));
        // Terminal wrapping can split a word inside the provenance text.
        let unwrapped: String = rendered
            .chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '│')
            .collect();
        assert!(unwrapped.contains("Attentionpriority1:"), "{rendered}");
    }

    #[test]
    fn editor_identity_why_reports_the_authoritative_dirty_flag() {
        for (dirty, expected_reason) in [(true, "dirty=true"), (false, "dirty=false")] {
            let mut app = App::new("test".to_owned(), std::env::temp_dir());
            let group = app.tree.add_group(ROOT_ID, "work").unwrap();
            let pane_id = app
                .tree
                .add_pane(group, "notes.md", PaneContentKind::Editor)
                .unwrap();
            app.tree
                .set_pane_status(pane_id, PaneStatus::Editor { dirty })
                .unwrap();
            app.set_screen_area(Rect::new(0, 0, 100, 24));
            app.hovered_status_slot = Some((
                pane_id,
                crate::status_icons::StatusSlot::Identity,
                Position::new(8, 3),
            ));

            let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
            terminal
                .draw(|frame| draw_status_tooltip(frame, &app))
                .unwrap();
            let rendered = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();

            assert!(
                rendered.contains(expected_reason),
                "editor WHY must expose the server-owned {expected_reason} state"
            );
        }
    }

    #[test]
    fn terminal_scrollbar_thumb_tracks_the_full_viewport_from_top_to_tail() {
        let mut view = TerminalView::new(4, 20);
        for line in 0..10 {
            view.feed(format!("line {line}\r\n").as_bytes());
        }
        let mut terminal = Terminal::new(TestBackend::new(3, 10)).unwrap();

        terminal
            .draw(|frame| draw_terminal_scrollbar(frame, frame.area(), &view))
            .unwrap();
        let tail_thumb_rows = terminal
            .backend()
            .buffer()
            .content()
            .chunks(3)
            .enumerate()
            .filter_map(|(row, cells)| (cells[2].symbol() == "█").then_some(row))
            .collect::<Vec<_>>();
        assert!(tail_thumb_rows.len() > 1);
        assert_eq!(tail_thumb_rows.last().copied(), Some(9));

        view.scroll_up(u16::MAX);
        terminal
            .draw(|frame| draw_terminal_scrollbar(frame, frame.area(), &view))
            .unwrap();
        let top_thumb_rows = terminal
            .backend()
            .buffer()
            .content()
            .chunks(3)
            .enumerate()
            .filter_map(|(row, cells)| (cells[2].symbol() == "█").then_some(row))
            .collect::<Vec<_>>();
        assert_eq!(top_thumb_rows.first().copied(), Some(0));
    }

    #[test]
    fn right_panel_title_carries_no_completion_marker_and_keeps_the_pane_name() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "Review authentication", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Codex, ilium_core::AgentActivity::Done, None),
            )
            .unwrap();

        assert_eq!(pane_title(&app, pane_id), "Review authentication — Codex");
        assert_eq!(app.tree.get(pane_id).unwrap().name, "Review authentication");

        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(
                    AgentClass::Codex,
                    ilium_core::AgentActivity::Working,
                    None,
                ),
            )
            .unwrap();
        assert_eq!(pane_title(&app, pane_id), "Review authentication — Codex");
    }

    #[test]
    fn a_displayed_terminal_without_an_engine_explains_itself_instead_of_staying_black() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "Waiting", PaneContentKind::Terminal)
            .unwrap();
        let mut view = TerminalView::new(24, 80);
        view.admission_error = Some("terminal parser registration backpressure".into());
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(view)));
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Loading terminal"));
    }
    #[test]
    fn a_displayed_terminal_with_an_engine_reports_stalled_publication() {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
            ShutdownMode,
        };
        const MIB: usize = 1024 * 1024;
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let mut execution = Execution::start(
            QuotaGroup::new(QuotaLimits {
                clients: 1,
                jobs: 1,
                service_jobs: 1,
                input_bytes: 128 * MIB,
                result_bytes: 2 * MIB,
                worker_threads: 1,
                // Reserve the service resident bytes plus retained bank metadata.
                worker_bytes: 132 * MIB,
            }),
            ExecutionConfig {
                cpu: disabled,
                io: disabled,
                service: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: 128 * MIB,
                },
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 1,
                service_jobs: 1,
                input_bytes: 128 * MIB,
                result_bytes: 2 * MIB,
            })
            .unwrap();
        let parsing = crate::terminal_parsing::TerminalParsing::start(client, 256).unwrap();
        let mut app = App::new("stalled-publication".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "Waiting", PaneContentKind::Terminal)
            .unwrap();
        let mut view = TerminalView::new(24, 80);
        parsing.attach(pane_id, &mut view).unwrap();
        assert!(
            view.frontend.is_some(),
            "exercise an allocated parser handle"
        );
        view.admission_error = Some("parser publication capacity is in use".into());
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(view)));
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        // Shut down the isolated owner even when the rendering assertion fails.
        drop(app);
        drop(parsing);
        execution.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            execution
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
        assert!(rendered.contains("parser publication capacity is in use"));
    }

    fn select_parser_regression_pane(app: &mut App, pane_id: ilium_core::NodeId) {
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
    }

    #[test]
    fn initialized_off_panes_paint_all_128_retained_snapshots_on_the_next_draw() {
        use crate::terminal_parsing::app_regressions::{initialized_app, view};
        use std::time::{Duration, Instant};
        let (_bank, mut app, ids) = initialized_app(128, 0, select_parser_regression_pane);
        let retained: Vec<_> = ids
            .iter()
            .map(|id| {
                let terminal = view(&app, *id);
                (
                    terminal.identity.clone(),
                    terminal.applied_ordinal,
                    terminal.with_screen(|screen| screen.contents()),
                )
            })
            .collect();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut longest = Duration::ZERO;
        for pass in 0..3 {
            for visit in 0..ids.len() {
                let index = if pass % 2 == 0 {
                    visit
                } else {
                    ids.len() - 1 - visit
                };
                let selected_at = Instant::now();
                select_parser_regression_pane(&mut app, ids[index]);
                // No parser collection or replay occurs between selection and this draw.
                terminal
                    .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
                    .unwrap();
                longest = longest.max(selected_at.elapsed());
                let rendered = terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                assert!(
                    rendered.contains(&format!("pane-{index:03}")),
                    "pass {pass}, pane {index}"
                );
                let current = view(&app, ids[index]);
                assert!(std::sync::Arc::ptr_eq(
                    &current.identity,
                    &retained[index].0
                ));
                assert_eq!(current.applied_ordinal, retained[index].1);
                assert_eq!(
                    current.with_screen(|screen| screen.contents()),
                    retained[index].2
                );
                assert!(current.frontend.is_some());
            }
        }
        assert_eq!(
            app.terminal_parsing.as_ref().unwrap().pending_work(),
            Some((0, 0))
        );
        eprintln!(
            "test_backend_off_revisits=384 max_selection_to_draw_us={}",
            longest.as_micros()
        );
    }

    #[test]
    fn stalled_status_preserves_retained_terminal_cells_and_geometry() {
        use crate::terminal_parsing::app_regressions::{initialized_app, view};
        let (_bank, mut app, ids) = initialized_app(1, 0, select_parser_regression_pane);
        let id = ids[0];
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
            .unwrap();
        let nonblank: Vec<_> = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .enumerate()
            .filter(|(_, cell)| !cell.symbol().chars().all(char::is_whitespace))
            .map(|(index, cell)| (index, cell.symbol().to_owned()))
            .collect();
        let before_size = view(&app, id).with_screen(|screen| screen.size());
        let before_ordinal = view(&app, id).applied_ordinal;
        let Some(PaneRuntime::Terminal(current)) = app.panes.get_mut(&id) else {
            unreachable!()
        };
        current.admission_error = Some("parser publication capacity is in use".into());
        terminal
            .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
            .unwrap();
        for (index, symbol) in nonblank {
            assert_eq!(
                terminal.backend().buffer().content()[index].symbol(),
                symbol
            );
        }
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("pane-000"));
        assert!(rendered.contains("parser publication capacity is in use"));
        assert_eq!(
            view(&app, id).with_screen(|screen| screen.size()),
            before_size
        );
        assert_eq!(view(&app, id).applied_ordinal, before_ordinal);
    }

    #[test]
    fn stale_parser_results_do_not_reach_any_app_consumer() {
        crate::terminal_parsing::app_regressions::stale_domain_attachment_and_dead_target_never_reach_any_app_result_consumer(select_parser_regression_pane);
    }

    #[test]
    fn parser_app_ordinals_are_applied_once_without_losing_retained_errors() {
        crate::terminal_parsing::app_regressions::app_completes_each_ordinal_once_and_ack_preserves_picture_and_error(select_parser_regression_pane);
    }

    #[test]
    fn focused_split_member_updates_its_parser_priority() {
        crate::terminal_parsing::app_regressions::focused_split_member_updates_parser_priority(
            select_parser_regression_pane,
        );
    }

    #[test]
    fn parser_pool_app_eviction_protects_busy_displayed_and_retiring_owners() {
        crate::terminal_parsing::app_regressions::pooled_app_protects_displayed_and_busy_panes_and_waits_for_retiring_claims(select_parser_regression_pane);
    }

    #[test]
    fn parser_pool_exact_setter_rejects_before_runtime_mutation() {
        crate::terminal_parsing::app_regressions::invalid_exact_pool_setter_rejects_before_mutating_any_live_setting(select_parser_regression_pane);
    }

    #[test]
    fn parser_pool_custom_values_reach_existing_app_consumers() {
        crate::terminal_parsing::app_regressions::custom_pool_values_reach_existing_app_consumers(
            select_parser_regression_pane,
        );
    }

    #[test]
    fn rapid_terminal_switches_never_mix_the_previous_pane_into_the_right_panel() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let first = app
            .tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = app
            .tree
            .add_pane(group, "second", PaneContentKind::Terminal)
            .unwrap();
        let mut first_view = TerminalView::new(24, 80);
        first_view.apply_replay(b"FIRST-PANE-ONLY\r\n", 1, true);
        let mut second_view = TerminalView::new(24, 80);
        second_view.apply_replay(b"SECOND-PANE-ONLY\r\n", 1, true);
        app.panes
            .insert(first, PaneRuntime::Terminal(Box::new(first_view)));
        app.panes
            .insert(second, PaneRuntime::Terminal(Box::new(second_view)));
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        for (pane_id, expected, forbidden) in [
            (first, "FIRST-PANE-ONLY", "SECOND-PANE-ONLY"),
            (second, "SECOND-PANE-ONLY", "FIRST-PANE-ONLY"),
            (first, "FIRST-PANE-ONLY", "SECOND-PANE-ONLY"),
            (second, "SECOND-PANE-ONLY", "FIRST-PANE-ONLY"),
        ] {
            app.focus_pane(pane_id);
            terminal
                .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
                .unwrap();
            let rendered = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();

            assert!(rendered.contains(expected));
            assert!(
                !rendered.contains(forbidden),
                "the prior pane must be fully cleared in the same frame"
            );
        }
    }

    #[test]
    fn voice_control_dot_is_black_when_disabled_and_red_when_enabled() {
        let backend = TestBackend::new(22, 1);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new("test".to_owned(), std::env::temp_dir());

        terminal
            .draw(|frame| draw_voice_control(frame, frame.area(), &app))
            .expect("render disabled voice control");
        let disabled_dot = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .find(|cell| cell.symbol() == "●")
            .expect("disabled dot");
        assert_eq!(disabled_dot.fg, Color::Black);

        app.voice_settings.enabled = true;
        app.voice_connection_state = ilium_voice::VoiceConnectionState::Recording;
        terminal
            .draw(|frame| draw_voice_control(frame, frame.area(), &app))
            .expect("render enabled voice control");
        let enabled_dot = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .find(|cell| cell.symbol() == "●")
            .expect("enabled dot");
        assert_eq!(enabled_dot.fg, Color::Red);
        assert!(enabled_dot.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn stacked_voice_credential_prompt_renders_over_live_settings() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.mode = Mode::Settings(Box::new(crate::app::SettingsState {
            tab: crate::app::SettingsTab::VoiceControl,
            selected_row: 1,
            ..crate::app::SettingsState::default()
        }));
        app.settings_adjust_voice_row(crate::voice_settings::VoiceRow::ApiKey, 1);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("⚙ Settings"));
        assert!(rendered.contains("Voice control"));
        assert!(rendered.contains("OpenAI API key"));
        assert!(rendered.contains("Protected value"));
        assert!(rendered.contains("Keep existing"));
        assert!(rendered.contains("Replace"));
    }

    #[test]
    fn source_scrollbar_renders_only_for_overflowing_buffers() {
        let mut editor = EditorPane::empty();
        editor.textarea = TextArea::from((0..10).map(|row| format!("line {row}")));

        editor.prepare_test_source_window(12, 4, &ratatui_image::picker::Picker::halfblocks());

        let backend = TestBackend::new(12, 4);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| draw_source_scrollbar(frame, Rect::new(0, 0, 12, 4), &editor))
            .expect("render overflowing source scrollbar");
        let buffer = terminal.backend().buffer();
        assert!(
            (0..4).any(|row| buffer[(11, row)].symbol() != " "),
            "overflowing source should render a scrollbar thumb"
        );

        editor.replace_contents("short");
        editor.prepare_test_source_window(12, 4, &ratatui_image::picker::Picker::halfblocks());
        terminal
            .draw(|frame| draw_source_scrollbar(frame, Rect::new(0, 0, 12, 4), &editor))
            .expect("render fitting source without scrollbar");
        let buffer = terminal.backend().buffer();
        assert!(
            (0..4).all(|row| buffer[(11, row)].symbol() == " "),
            "fitting source should not render a scrollbar"
        );
    }

    #[test]
    fn create_agent_dialog_renders_selector_editable_prompt_and_button() {
        let mut state = CreateAgentFromLineState::new(
            crate::agent_from_line::EditorSourceLine {
                pane_id: NodeId(2),
                path: PathBuf::from("/work/main.rs"),
                line_number: 9,
                text: "finish_feature();".to_string(),
            },
            NodeId(1),
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

        for provider in crate::agent_from_line::AgentLaunchType::ALL {
            state.agent_type = provider;
            terminal
                .draw(|frame| draw_create_agent_from_line(frame, frame.area(), &state))
                .unwrap();
            let rendered = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();

            assert!(rendered.contains("Create agent from line"));
            assert!(rendered.contains(provider.label()));
            assert!(rendered.contains("/goal please do the following task"));
            assert!(rendered.contains("[ Create agent ]"));
            let controls =
                crate::agent_from_line::provider_control(Rect::new(0, 0, 100, 30), &state)
                    .geometry();
            for (rect, glyph) in [
                (controls.previous, "←"),
                (controls.open, "+"),
                (controls.next, "→"),
            ] {
                assert_eq!(
                    terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                    glyph
                );
            }
        }
    }

    #[test]
    fn tree_order_submenu_renders_one_check_before_the_active_option() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        app.open_context_menu(ROOT_ID, 2, 2);
        let Mode::ContextMenu(mut menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("context menu should be open");
        };
        app.open_context_submenu(&mut menu, crate::app::ContextMenuAction::OrderBy);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

        terminal
            .draw(|frame| draw_context_menu(frame, &menu, &app.ui_settings))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        // A configurable icon renders between the check mark and the label, so
        // the expected row is composed through the same helper the renderer
        // uses; pinning the literal pair would break whenever that icon
        // changes or the user turns context-menu icons off entirely.
        // A double-width glyph occupies two buffer cells, and the second cell
        // is read back as one extra space.
        let icon = context_menu_icon(&app.ui_settings, IconTarget::TopLevel);
        let wide_cell_padding = " ".repeat(icon.width().saturating_sub(icon.chars().count()));
        let active_row = format!("✓ {}{wide_cell_padding}  Manual order", icon.trim_start());
        assert!(rendered.contains(&active_row));
        assert_eq!(rendered.matches('✓').count(), 1);
    }

    #[test]
    fn context_menu_icons_follow_the_live_ui_preference() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        app.open_context_menu(ROOT_ID, 2, 2);
        let Mode::ContextMenu(menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("context menu should open");
        };
        let render = |ui: &crate::config::UiSettings| {
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
            terminal
                .draw(|frame| draw_context_menu(frame, &menu, ui))
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };

        let with_icons = render(&app.ui_settings);
        assert!(with_icons.contains(app.ui_settings.icons.glyph(IconTarget::ToolbarSearch)));

        app.ui_settings.show_context_menu_icons = false;
        let text_only = render(&app.ui_settings);
        assert!(!text_only.contains(app.ui_settings.icons.glyph(IconTarget::ToolbarSearch)));
        assert!(text_only.contains("Search workspace…"));
    }

    #[test]
    fn split_view_renders_both_terminal_members_and_active_slot_chrome() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let first = app
            .tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = app
            .tree
            .add_pane(group, "second", PaneContentKind::Terminal)
            .unwrap();
        let split = app
            .tree
            .create_split_view(
                group,
                "Vertical split",
                SplitOrientation::Vertical,
                &[first, second],
            )
            .unwrap();
        let mut first_view = TerminalView::new(20, 30);
        first_view.feed(b"LEFT-PANE");
        let mut second_view = TerminalView::new(20, 30);
        second_view.feed(b"RIGHT-PANE");
        app.panes
            .insert(first, PaneRuntime::Terminal(Box::new(first_view)));
        app.panes
            .insert(second, PaneRuntime::Terminal(Box::new(second_view)));
        app.right_panel_target = RightPanelTarget::SplitView {
            split_id: split,
            active_pane_id: Some(second),
        };
        app.focus = FocusTarget::Pane;
        app.set_screen_area(Rect::new(0, 0, 120, 40));

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("LEFT-PANE"));
        assert!(rendered.contains("RIGHT-PANE"));
        assert!(rendered.contains("first"));
        assert!(rendered.contains("second"));
    }
}
