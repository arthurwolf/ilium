//! Top-level layout: a left tree column, the focused pane's content on
//! the right, and a bottom status bar -- with the Explorer file-picker or
//! Help reference drawn as an overlay on top of everything else when
//! active. It consumes the shared animated `App::layout`; everything it
//! draws is delegated to `tree_ui`, `help`, or the pane runtimes themselves.

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
    CreateBoardState, CreateGroupState, CreateSplitMembersState, CreateSplitOrientationState,
    FocusTarget, Mode, PaneRuntime, RightPanelTarget, SubmenuItemAction,
    FROZEN_DIALOG_BUTTON_LABEL,
};
use crate::editor_pane::{EditorPane, EditorViewMode};
use crate::icon_settings::IconTarget;
use crate::scheduled_input::{ScheduledInputDialogState, ScheduledInputFocus};
use crate::{
    editor_chrome, editor_toolbar, explorer_overlay, help, markdown, minimap, modal, search_ui,
    terminal_selection, terminal_view, theme, tree_ui,
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let elapsed = app.started_at.elapsed();
    draw_at(frame, app, elapsed);
}

#[cfg(test)]
#[path = "ui_openstreetmap_tests.rs"]
mod osm_attribution_tests;

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
            || app.hovered_tree_node.is_some()
            || app.stats_popover.is_some()
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
    // Copied text can repaint an identical Braille glyph after composition.
    // Withhold this draw rather than crediting preview text as scene pixels.
    if app.smart_copy_preview.is_some() {
        app.animation_frame.discard_composed_receipt();
    }
    draw_smart_copy_preview(frame, area, app);
    draw_startup_dialog(frame, area, app);
    skip_vs16_continuation_cells(frame.buffer_mut(), layout.tree_area);
    if app.onboarding.is_none() && app.draw_plugin_permission_review(frame) {
        app.animation_frame.discard_composed_receipt();
        return None;
    }
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
                | Mode::Search(_)
        )
}

/// The "Preview" dialog: what Smart Copy light just put on the clipboard, with
/// a progress bar that counts down the second before it disappears.
fn draw_smart_copy_preview(frame: &mut Frame, area: Rect, app: &App) {
    use ratatui::widgets::LineGauge;

    let Some(preview) = &app.smart_copy_preview else {
        return;
    };
    if area.width < 12 || area.height < 7 {
        return;
    }
    let lines = preview.display_lines();
    let content_width = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .chain(std::iter::once(UnicodeWidthStr::width(
            preview.summary().as_str(),
        )))
        .max()
        .unwrap_or(0);
    // Borders (2) + text rows + summary + gauge.
    let height = (lines.len() as u16 + 4).min(area.height);
    let width = u16::try_from(content_width.saturating_add(4))
        .unwrap_or(u16::MAX)
        .max(28)
        .min(area.width);
    let dialog = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, dialog);
    let block = theme::block(true).title(theme::chrome_title("Preview"));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);
    if inner.height < 2 {
        return;
    }
    let text_height = inner.height - 2;
    let mut rows: Vec<Line<'static>> = lines
        .into_iter()
        .take(usize::from(text_height))
        .map(Line::from)
        .collect();
    rows.resize(usize::from(text_height), Line::from(""));
    frame.render_widget(
        Paragraph::new(rows),
        Rect::new(inner.x, inner.y, inner.width, text_height),
    );
    frame.render_widget(
        Paragraph::new(preview.summary()).style(Style::new().add_modifier(Modifier::DIM)),
        Rect::new(inner.x, inner.y + text_height, inner.width, 1),
    );
    let ratio = preview.remaining_fraction(Instant::now());
    frame.render_widget(
        LineGauge::default()
            .ratio(ratio)
            .filled_symbol("━")
            .unfilled_symbol("─")
            .filled_style(Style::new().add_modifier(Modifier::BOLD))
            .label(""),
        Rect::new(inner.x, inner.y + text_height + 1, inner.width, 1),
    );
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
    let Some(progress) = app.tree.pane_progress(pane_id) else {
        return;
    };
    let content = crate::progress_bar::details_tooltip(progress);
    crate::status_icons::render_tooltip(frame, app.layout.screen_area, anchor, &content);
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

fn draw_worktree_tooltip(frame: &mut Frame, app: &App) {
    let Some(hit) = app.hovered_tree_node.filter(|hit| hit.line == 1) else {
        return;
    };
    let anchor = Position::new(app.layout.tree_area.x.saturating_add(2), hit.row);
    if app.emitted_geometry.is_some() {
        if let Some(explanation) = app.emitted_worktree_tooltip(hit.id) {
            crate::status_icons::render_tooltip(frame, app.layout.screen_area, anchor, explanation);
        }
        return;
    }
    #[cfg(test)]
    if let Some(workspace) = app.tree.pane_workspace(hit.id) {
        let explanation = crate::status_icons::workspace_explanation(
            workspace,
            app.workspace_git_statuses.get(&hit.id),
        );
        crate::status_icons::render_tooltip(frame, app.layout.screen_area, anchor, &explanation);
    }
}

/// Explains the tree-row state glyph under the pointer. The slot's signal is
/// retained with the actually emitted row; newer server evidence cannot
/// change its explanation until that row's next frame is acknowledged.
fn draw_status_tooltip(frame: &mut Frame, app: &App) {
    let Some((node_id, slot, anchor)) = app.hovered_status_slot else {
        return;
    };
    if app.emitted_geometry.is_some() {
        if let Some(tooltip) = app.emitted_status_tooltip(node_id, slot) {
            crate::status_icons::render_tooltip(frame, app.layout.screen_area, anchor, tooltip);
        }
        return;
    }
    #[cfg(test)]
    if let Some(tooltip) = status_tooltip_content(app, node_id, slot) {
        crate::status_icons::render_tooltip(frame, app.layout.screen_area, anchor, &tooltip);
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

    let node = app.tree.get(node_id)?;
    if slot == StatusSlot::Identity {
        let structural = match &node.kind {
            NodeKind::Container(container) => {
                let (title, kind) = match &container.kind {
                    ilium_core::ContainerKind::Project { .. } => ("Project", "project"),
                    ilium_core::ContainerKind::Group => ("Group", "group"),
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
        progress,
        scheduled_input,
        ..
    } = &node.kind
    else {
        return None;
    };
    let shell_output = app
        .terminal_activity
        .phase(node_id, app.started_at.elapsed().as_millis())
        .map(tree_ui::shell_output_phase);
    let signals = crate::agent_monitoring::displayed_pane_signals(
        app.ui_settings.agent_monitoring_mode,
        app.ui_settings.attention_progress_reports,
        app.ui_settings.attention_running_indicator,
        status,
        progress.as_deref(),
        scheduled_input.is_some(),
        shell_output,
    );
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
                ilium_core::ObjectiveSignal::Task(_) => progress.as_deref().map(|progress| {
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
                ilium_core::NowSignal::Parked => progress.as_deref().map(|progress| format!(
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

/// Draws the one full-screen root behind every stacked overlay. Settings and
/// Search replace the ordinary workspace only when they are the oldest layer.
fn draw_base_layer(frame: &mut Frame, area: Rect, app: &mut App) -> Option<Position> {
    if app.onboarding.is_some() {
        crate::onboarding::integration::render(frame, area, app);
        return None;
    }
    let root_mode = app.modal_stack.first().unwrap_or(&app.mode);
    if let Mode::Search(state) = root_mode {
        return search_ui::render_cursor(frame, area, state, &app.ui_settings.icons);
    }
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
    let mut painted_rows = tree_ui::render(
        frame,
        layout.tree_area,
        &app.tree,
        &mut app.tree_state,
        tree_ui::TreeRenderOptions {
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
            current_unix_millis: crate::scheduled_input::unix_millis_now(),
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
            workspace_git_statuses: &app.workspace_git_statuses,
            show_worktree_branch_line: app.git_settings.branch_line
                != crate::config::GitBranchLine::Off,
            tree_order,
            show_project_separators: app.ui_settings.show_project_separators,
            sidebar_density: app.ui_settings.sidebar_density,
            use_stable_glyphs: app.ui_settings.use_stable_glyphs,
            agent_monitoring_mode: app.ui_settings.agent_monitoring_mode,
            attention_progress_reports: app.ui_settings.attention_progress_reports,
            attention_running_indicator: app.ui_settings.attention_running_indicator,
            show_inferred_title_icons: app.ui_settings.show_inferred_title_icons,
            frozen_panes: &app.frozen_panes,
            cost: Some(app.cost_tracker.overlay().as_ref()),
            hover: tree_ui::TreeHoverState {
                node: app.hovered_tree_node,
                toolbar_hovered: app.tree_toolbar_hovered,
                toolbar_action: app.hovered_tree_toolbar_action,
                show_management_actions: app.ui_settings.show_tree_row_management_controls,
            },
            sidebar_files: &app.sidebar_snapshot,
            chatroom_projects: &app.chatroom_projects,
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
    app.record_composed_tree_rows(painted_rows);
    draw_cost_card(frame, app);
    draw_status_bar(frame, layout.status_area, app);
    None
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
        Mode::Help => help::render(
            frame,
            area,
            app.keyboard_settings.shortcut_base,
            app.keyboard_settings.navigation_shortcut_base,
            &app.keybindings,
        ),
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
            draw_context_menu(frame, menu, app.ui_settings.tree_order, &app.ui_settings);
        }
        Mode::TerminalPaneContextMenu(menu) => {
            draw_terminal_pane_context_menu(frame, menu, &app.ui_settings)
        }
        Mode::AgentToolbarModelSubmenu(state) => draw_agent_toolbar_model_submenu(frame, state),
        Mode::SmartCopy => {}
        Mode::AgentDebugLog(_) => {}
        Mode::AgentDebugSavePath(_, state) => {
            cursor = modal::render_text_prompt_cursor(
                frame,
                area,
                "Save agent debug log",
                state,
                "Save",
            )
            .or(cursor);
        }
        Mode::SchedulePaneInput(state) => {
            cursor = draw_scheduled_input_dialog(frame, area, app, state).or(cursor);
        }
        Mode::QueuePrompt(state) => draw_prompt_queue_dialog(frame, area, app, state),
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
        Mode::TextTriggerDialog(state) => draw_text_trigger_dialog(frame, area, state),
        Mode::AgentMessageDialog(state) => crate::agent_message_dialog::draw(frame, area, state),
        Mode::EditorLineContextMenu(menu) => {
            draw_editor_line_context_menu(frame, menu, &app.ui_settings)
        }
        Mode::CreateAgentFromLine(state) => draw_create_agent_from_line(frame, area, state),
        Mode::CreateAgentWorkspace(state) => {
            crate::worktree_dialog::draw_dialog(frame, area, state);
        }
        Mode::WorktreeManager(state) => crate::worktree_manager::render(frame, area, state),
        Mode::CreateGroup(state) => {
            cursor = draw_create_group(frame, app, state).or(cursor);
        }
        Mode::CreateSplitOrientation(state) => {
            draw_create_split_orientation(frame, area, state, &app.ui_settings.icons);
        }
        Mode::CreateSplitMembers(state) => draw_create_split_members(frame, area, state),
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
        Mode::Rename(state) => {
            cursor =
                modal::render_text_prompt_cursor(frame, area, "Rename", state, "Rename").or(cursor);
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
        Mode::ConvertSession => {
            if let Some(state) = &app.conversion {
                let pane_area = app
                    .pane_viewport(state.pane_id)
                    .map(|viewport| viewport.outer_area);
                crate::session_conversion::render(
                    frame,
                    crate::session_conversion::dialog_area(pane_area, area),
                    state,
                );
            }
        }
        Mode::RemoteCompaction => {
            if let Some(state) = &app.remote_compaction {
                let pane_area = app
                    .pane_viewport(state.pane_id)
                    .map(|viewport| viewport.outer_area);
                let banner_text = app
                    .remote_compaction_settings
                    .should_show_privacy_banner()
                    .then(|| {
                        crate::remote_compaction_settings_ui::privacy_banner_text(
                            &app.inference_settings,
                        )
                    });
                let banner_rows =
                    crate::remote_compaction_dialog::banner_rows(banner_text.as_deref());
                crate::remote_compaction_dialog::render(
                    frame,
                    crate::remote_compaction_dialog::dialog_area(pane_area, area, banner_rows),
                    state,
                    banner_text.as_deref(),
                );
            }
        }
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
        | Mode::Settings(_)
        | Mode::Search(_) => {}
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

fn draw_text_trigger_dialog(
    frame: &mut Frame,
    area: Rect,
    state: &crate::text_trigger_dialog::TextTriggerDialogState,
) {
    use crate::text_trigger_dialog::TextTriggerFocus;
    let layout = crate::text_trigger_dialog::layout(area);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title(if state.editing_index.is_some() {
            "Edit text trigger"
        } else {
            "Add text trigger"
        })),
        layout.popup,
    );
    let field = |label: &str, value: &str, active: bool| {
        Paragraph::new(format!("{label}: {value}")).style(if active {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        })
    };
    frame.render_widget(
        field(
            "Regexp",
            &state.regexp.buf,
            state.focus == TextTriggerFocus::Regexp,
        ),
        layout.regexp,
    );
    frame.render_widget(
        field(
            "Message",
            &state.message.buf,
            state.focus == TextTriggerFocus::Message,
        ),
        layout.message,
    );
    let scope_style = if state.focus == TextTriggerFocus::Target {
        theme::selected_style()
    } else {
        Style::new().fg(Color::Cyan)
    };
    crate::text_trigger_dialog::target_control(area, state).render(
        frame,
        crate::value_control::ControlStyles {
            background: Style::new(),
            label: scope_style,
            value: scope_style,
            button: scope_style,
            disabled: Style::new().add_modifier(Modifier::DIM),
        },
    );
    let delay_style = if state.focus == TextTriggerFocus::Delay {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    crate::text_trigger_dialog::delay_control(area, state).render(
        frame,
        crate::value_control::ControlStyles {
            background: Style::new(),
            label: delay_style,
            value: delay_style,
            button: delay_style,
            disabled: Style::new().add_modifier(Modifier::DIM),
        },
    );
    frame.render_widget(
        Paragraph::new("SAMPLE TEXT")
            .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Rect::new(
            layout.sample.x,
            layout.sample.y.saturating_sub(1),
            layout.sample.width,
            1,
        ),
    );
    frame.render_widget(&state.sample, layout.sample);
    frame.render_widget(
        Paragraph::new(format!(
            "[{}] Enabled",
            if state.enabled { "x" } else { " " }
        ))
        .style(if state.focus == TextTriggerFocus::Enabled {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        }),
        layout.enabled,
    );
    let (preview, preview_title) = state.preview_display();
    let preview_title = match state.preview_issue() {
        Some(issue) => Line::from(vec![
            Span::raw("LIVE PREVIEW · "),
            Span::raw(issue.message()),
        ]),
        None => Line::from(preview_title),
    };
    // Only visible rows need paragraph objects. The worker-owned text stays
    // borrowed; drawing never rebuilds or joins the complete prepared result.
    let preview_lines = preview
        .lines()
        .take(usize::from(layout.preview.height.saturating_sub(2)))
        .map(Line::from)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(preview_lines).block(theme::block(false).title(preview_title)),
        layout.preview,
    );
    frame.render_widget(
        Paragraph::new("[ Save trigger ]")
            .alignment(Alignment::Center)
            .style(if state.focus == TextTriggerFocus::Save {
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::Cyan)
            }),
        layout.save,
    );
    frame.render_widget(
        Paragraph::new("Tab fields · arrows select scope · Space toggle · Enter save · Esc cancel")
            .alignment(Alignment::Center)
            .style(Style::new().add_modifier(Modifier::DIM)),
        layout.hint,
    );
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

fn draw_create_split_orientation(
    frame: &mut Frame,
    area: Rect,
    state: &CreateSplitOrientationState,
    icons: &crate::icon_settings::IconSettings,
) {
    let popup = modal::create_split_orientation_dialog_area(area);
    frame.render_widget(Clear, popup);
    let block = theme::block(true).title(theme::chrome_title("New split view"));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let vertical_marker = if state.orientation == ilium_core::SplitOrientation::Vertical {
        "›"
    } else {
        " "
    };
    let horizontal_marker = if state.orientation == ilium_core::SplitOrientation::Horizontal {
        "›"
    } else {
        " "
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Choose how two or three panes are arranged:"),
            Line::from(""),
            Line::from(format!(
                "{vertical_marker} {}  Vertical — side by side",
                icons.glyph(IconTarget::SplitVertical)
            )),
            Line::from(format!(
                "{horizontal_marker} {}  Horizontal — stacked",
                icons.glyph(IconTarget::SplitHorizontal)
            )),
            Line::from(""),
            Line::from(Span::styled(
                "E  Create empty with this orientation",
                Style::new().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "←/→ choose · Enter choose panes · E create empty · Esc cancel",
                Style::new().add_modifier(Modifier::DIM),
            )),
        ]),
        inner,
    );
}

fn draw_create_split_members(frame: &mut Frame, area: Rect, state: &CreateSplitMembersState) {
    let popup = modal::create_split_members_dialog_area(area, state.choices.len());
    frame.render_widget(Clear, popup);
    let block = theme::block(true).title(theme::chrome_title("Add panes to split"));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let (start, end) =
        modal::create_split_member_visible_window(state.selected_index, state.choices.len());
    let selected_count = state
        .choices
        .iter()
        .filter(|choice| choice.selected)
        .count();
    let mut lines = vec![
        Line::from(format!(
            "Select up to four panes ({selected_count}/4). Selecting none creates an empty split."
        )),
        Line::from(""),
    ];
    if state.choices.is_empty() {
        lines.push(Line::from(Span::styled(
            "No eligible panes; all existing panes are already in split views.",
            Style::new().add_modifier(Modifier::DIM),
        )));
    } else {
        for (index, choice) in state.choices[start..end].iter().enumerate() {
            let absolute_index = start + index;
            let marker = if absolute_index == state.selected_index {
                "›"
            } else {
                " "
            };
            let checkbox = if choice.selected { "[x]" } else { "[ ]" };
            let style = if absolute_index == state.selected_index {
                theme::selected_style()
            } else {
                Style::new()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker} {checkbox} {}", choice.label),
                style,
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑/↓ navigate · Space toggle · Enter create · Esc cancel",
        Style::new().add_modifier(Modifier::DIM),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
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
fn draw_context_menu(
    frame: &mut Frame,
    menu: &ContextMenu,
    current_tree_order: crate::config::TreeOrder,
    ui: &crate::config::UiSettings,
) {
    let layout = menu.layout();
    let width = menu.area.width.saturating_sub(2);
    let icon_width = popup_icon_width(ui, menu.actions.iter().map(|action| action.icon_target()));
    let lines = popup_rows(&layout, width, |index| {
        let action = menu.actions[index];
        popup_action_line(
            &action.label(),
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
            Some(
                matches!(item.action, SubmenuItemAction::SetTreeOrder(order) if order == current_tree_order),
            )
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

/// Renders a deliberately spacious form: duration first, then payload, then
/// the Enter policy and one explicit confirmation button. The same geometry
/// drives `crate::mouse`, so every visible control has an exact hit target.
fn draw_scheduled_duration_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    state: &ScheduledInputDialogState,
    focus: ScheduledInputFocus,
) -> Option<Position> {
    let focused = state.focus == focus;
    let style = if focused {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        theme::block(focused).title(theme::chrome_title(label)),
        area,
    );
    let control = state.duration_control(focus, area);
    control.render(
        frame,
        crate::value_control::ControlStyles {
            background: style,
            label: style,
            value: style,
            button: style,
            disabled: style.add_modifier(Modifier::DIM),
        },
    );
    let geometry = control.geometry();
    if !focused || geometry.value_slot.width == 0 || geometry.value_slot.height == 0 {
        return None;
    }
    let cursor = match focus {
        ScheduledInputFocus::Hours => state.hours.cursor,
        ScheduledInputFocus::Minutes => state.minutes.cursor,
        ScheduledInputFocus::Seconds => state.seconds.cursor,
        _ => return None,
    };
    Some(Position::new(
        geometry.value.x.saturating_add(
            u16::try_from(cursor)
                .unwrap_or(u16::MAX)
                .min(geometry.value.width),
        ),
        geometry.value.y,
    ))
}

fn draw_scheduled_input_dialog(
    frame: &mut Frame,
    screen_area: Rect,
    app: &App,
    state: &ScheduledInputDialogState,
) -> Option<Position> {
    let mut cursor = None;
    let layout = crate::scheduled_input::dialog_layout(screen_area);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Schedule keystrokes")),
        layout.popup,
    );
    let pane_name = app
        .tree
        .get(state.pane_id)
        .map_or("terminal", |node| node.name.as_str());
    frame.render_widget(
        Paragraph::new(format!("Schedule input for {pane_name}"))
            .style(Style::new().add_modifier(Modifier::DIM)),
        layout.subtitle,
    );
    frame.render_widget(
        Paragraph::new("WHEN").style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        layout.duration_label,
    );
    cursor = draw_scheduled_duration_field(
        frame,
        layout.hours,
        "Hours",
        state,
        ScheduledInputFocus::Hours,
    )
    .or(cursor);
    cursor = draw_scheduled_duration_field(
        frame,
        layout.minutes,
        "Minutes",
        state,
        ScheduledInputFocus::Minutes,
    )
    .or(cursor);
    cursor = draw_scheduled_duration_field(
        frame,
        layout.seconds,
        "Seconds",
        state,
        ScheduledInputFocus::Seconds,
    )
    .or(cursor);
    frame.render_widget(
        Paragraph::new("WHAT TO HIT")
            .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        layout.payload_label,
    );
    cursor = draw_scheduled_input_field(
        frame,
        layout.text,
        "Text (optional)",
        &state.text,
        state.focus == ScheduledInputFocus::Text,
    )
    .or(cursor);

    let checkbox_style = if state.focus == ScheduledInputFocus::SendEnter {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    let checkbox = if state.send_enter { "[x]" } else { "[ ]" };
    frame.render_widget(
        Paragraph::new(format!("{checkbox} Send Enter after the text")).style(checkbox_style),
        layout.send_enter,
    );

    let button_style = if state.focus == ScheduledInputFocus::ScheduleButton {
        Style::new()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    };
    frame.render_widget(
        Paragraph::new("[ Schedule input ]")
            .style(button_style)
            .alignment(Alignment::Center),
        layout.schedule_button,
    );
    frame.render_widget(
        Paragraph::new("Tab field · Space toggle · Ctrl+Enter schedule · Esc cancel")
            .style(Style::new().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        layout.hint,
    );
    cursor
}

/// Keep enough grapheme context for the viewport while retaining the original
/// draft intact. Logical indices can be huge; local screen rows remain bounded.
fn queued_prompt_view(text: &crate::text_prompt::TextPromptState, area: Rect) -> (&str, usize) {
    let cursor_byte = text
        .buf
        .char_indices()
        .nth(text.cursor)
        .map_or(text.buf.len(), |(index, _)| index);
    let context = usize::from(area.width.max(1))
        .saturating_mul(usize::from(area.height.max(1)))
        .saturating_mul(2)
        .saturating_add(1);
    let mut prior = std::collections::VecDeque::new();
    let mut following = 0;
    let mut end = text.buf.len();
    for (index, _) in text.buf.grapheme_indices(true) {
        if index <= cursor_byte {
            prior.push_back(index);
            if prior.len() > context {
                prior.pop_front();
            }
        } else {
            following += 1;
            if following > context {
                end = index;
                break;
            }
        }
    }
    let start = prior.front().copied().unwrap_or(0);
    (&text.buf[start..end], cursor_byte - start)
}

fn draw_prompt_queue_dialog(
    frame: &mut Frame,
    screen_area: Rect,
    app: &App,
    state: &crate::prompt_queue::PromptQueueDialogState,
) {
    use crate::prompt_queue::PromptQueueFocus;
    use ilium_core::PromptQueueDelivery;

    let layout = crate::prompt_queue::dialog_layout(screen_area);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Queue prompt after agent finishes")),
        layout.popup,
    );
    let pane_name = app
        .tree
        .get(state.pane_id)
        .map_or("terminal", |node| node.name.as_str());
    frame.render_widget(
        Paragraph::new(format!(
            "The next detected bell for {pane_name} sends this prompt and Enter."
        ))
        .style(Style::new().add_modifier(Modifier::DIM)),
        layout.subtitle,
    );
    frame.render_widget(
        Paragraph::new("PROMPT").style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        layout.prompt_label,
    );
    let text_style = if state.focus == PromptQueueFocus::Text {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    // The authoritative draft/cursor stay in TextPromptState. This temporary
    // view follows that cursor, including drafts beyond the textarea's u16
    // viewport range, without imposing a limit on queued input.
    let (visible, cursor_byte) = queued_prompt_view(&state.text, layout.text);
    let mut text_view =
        ratatui_textarea::TextArea::from(visible[cursor_byte..].split('\n').map(str::to_owned));
    // Constructor starts at (0, 0); insertion positions the view cursor using
    // usize internally, avoiding CursorMove::Jump's narrowed row/column.
    text_view.insert_str(&visible[..cursor_byte]);
    text_view.set_wrap_mode(ratatui_textarea::WrapMode::WordOrGlyph);
    text_view.set_style(text_style);
    text_view.set_cursor_line_style(text_style);
    text_view.set_cursor_style(if state.focus == PromptQueueFocus::Text {
        text_style.add_modifier(Modifier::REVERSED)
    } else {
        text_style
    });
    text_view.set_block(
        theme::block(state.focus == PromptQueueFocus::Text)
            .title(theme::chrome_title("Multiline prompt")),
    );
    frame.render_widget(&text_view, layout.text);
    frame.render_widget(
        Paragraph::new("DELIVERY").style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        layout.delivery_label,
    );
    let delivery_style = if state.focus == PromptQueueFocus::Delivery {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    state.delivery_control(layout.delivery).render(
        frame,
        crate::value_control::ControlStyles {
            background: delivery_style,
            label: delivery_style,
            value: delivery_style,
            button: delivery_style,
            disabled: delivery_style.add_modifier(Modifier::DIM),
        },
    );
    let times_style = if state.focus == PromptQueueFocus::Times {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        theme::block(state.focus == PromptQueueFocus::Times)
            .title(theme::chrome_title("Repeat count")),
        layout.times,
    );
    let repeat_control = state.times_control(layout.times);
    repeat_control.render(
        frame,
        crate::value_control::ControlStyles {
            background: times_style,
            label: times_style,
            value: times_style,
            button: times_style,
            disabled: times_style.add_modifier(Modifier::DIM),
        },
    );
    if state.focus == PromptQueueFocus::Times
        && matches!(state.delivery_choice, PromptQueueDelivery::Times { .. })
    {
        let geometry = repeat_control.geometry();
        if geometry.value_slot.width > 0 && geometry.value_slot.height > 0 {
            frame.set_cursor_position(Position::new(
                geometry.value.x.saturating_add(
                    u16::try_from(state.times.cursor)
                        .unwrap_or(u16::MAX)
                        .min(geometry.value.width),
                ),
                geometry.value.y,
            ));
        }
    }
    let warning = match state.delivery_choice {
        PromptQueueDelivery::Once if layout.warning.width < 35 => "Once after finish.",
        PromptQueueDelivery::Times { .. } if layout.warning.width < 35 => {
            "Counted sends after finish."
        }
        PromptQueueDelivery::Forever if layout.warning.width < 35 => "Forever; monitor closely.",
        PromptQueueDelivery::Once if layout.warning.width < 84 => {
            "Sends once after the agent finishes."
        }
        PromptQueueDelivery::Times { .. } if layout.warning.width < 84 => {
            "One send per finish, up to count."
        }
        PromptQueueDelivery::Forever if layout.warning.width < 84 => {
            "Repeats forever; monitor closely."
        }
        PromptQueueDelivery::Forever => {
            "DANGER: this will re-send forever whenever the agent finishes. Do not run it unmonitored."
        }
        PromptQueueDelivery::Times { .. } => {
            "The same prompt is sent once per future finish, until the selected count is exhausted."
        }
        PromptQueueDelivery::Once => {
            "The prompt remains queued until the agent next finishes, then is sent once."
        }
    };
    frame.render_widget(
        Paragraph::new(warning).style(Style::new().fg(
            if matches!(state.delivery_choice, PromptQueueDelivery::Forever) {
                Color::Red
            } else {
                Color::Yellow
            },
        )),
        layout.warning,
    );
    let button_style = if state.focus == PromptQueueFocus::EnqueueButton {
        Style::new()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    };
    frame.render_widget(
        Paragraph::new("[ Enqueue ]")
            .style(button_style)
            .alignment(Alignment::Center),
        layout.enqueue_button,
    );
    frame.render_widget(
        Paragraph::new("Tab fields · ←/→ select delivery · Ctrl+Enter enqueue · Esc cancel")
            .style(Style::new().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        layout.hint,
    );
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

const GROUP_ACCENT: Color = Color::Rgb(0x7a, 0xa2, 0xf7);

/// Draws the "New group" destination picker: an always-editable name field
/// (optional -- left blank it defaults to "group") plus the flattened list
/// of every existing group, top level first, with the current selection
/// highlighted in the same accent used for the real tree's selected row.
fn draw_create_group(frame: &mut Frame, app: &App, state: &CreateGroupState) -> Option<Position> {
    let mut cursor = None;
    frame.render_widget(Clear, state.area);
    let block = theme::block(true).title(theme::chrome_title("New group"));
    let layout = modal::create_group_layout(state.area);
    frame.render_widget(block, state.area);

    let name_label = Span::styled("Name  ", Style::new().add_modifier(Modifier::DIM));
    let name_value = if state.name.buf.is_empty() {
        Span::styled(
            "group",
            Style::new().add_modifier(Modifier::DIM | Modifier::ITALIC),
        )
    } else {
        Span::raw(state.name.buf.as_str())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![name_label, name_value])),
        layout.name_row,
    );
    // `state.name.cursor` (see `TextPromptState`) is a *char* index, not a
    // display column -- measuring the prefix's `UnicodeWidthStr::width()`
    // (like `draw_scheduled_input_field` does) rather than the raw char
    // count is required so a wide (CJK/emoji) name doesn't land the cursor
    // to the left of where the character actually renders.
    if layout.name_row.width > 0 {
        let prefix: String = state.name.buf.chars().take(state.name.cursor).collect();
        // `saturating_add` throughout -- an unbounded prefix width could
        // otherwise overflow `u16` (panic in debug, wrap to a bogus column
        // in release) before the `.min()` clamp below ever got a chance to
        // run -- see the identical fix (and its rationale) in
        // `modal::render_text_prompt`.
        let cursor_x = layout
            .name_row
            .x
            .saturating_add(6)
            .saturating_add(u16::try_from(prefix.width()).unwrap_or(u16::MAX));
        // `Rect::right()` is exclusive (the first column *outside* the rect),
        // so clamping to it directly would let the cursor land one cell past
        // the row's real last cell -- see the identical fix in
        // `modal::render_text_prompt` for the same `Rect::right()` pitfall.
        let position = Position::new(
            cursor_x.min(layout.name_row.right().saturating_sub(1)),
            layout.name_row.y,
        );
        frame.set_cursor_position(position);
        cursor = Some(position);
    }

    frame.render_widget(
        Paragraph::new(Span::styled(
            "Create under:",
            Style::new().add_modifier(Modifier::DIM | Modifier::UNDERLINED),
        )),
        layout.label_row,
    );

    let (start, end) = modal::create_group_visible_window(
        state.selected_index,
        state.destinations.len(),
        modal::CREATE_GROUP_MAX_VISIBLE,
    );
    let rows: Vec<Line> = state.destinations[start..end]
        .iter()
        .enumerate()
        .map(|(offset, destination)| {
            let index = start + offset;
            let is_top_level = destination.id == ROOT_ID;
            let indent = "  ".repeat(destination.depth.saturating_sub(1));
            let icon = if is_top_level {
                app.ui_settings.icons.glyph(IconTarget::TopLevel)
            } else {
                app.ui_settings.icons.glyph(IconTarget::Group)
            };
            // Build name via format! to avoid cloning node.name — format! reads
            // the source and produces a new owned String only once needed.
            let name_str = if is_top_level {
                "Top level"
            } else {
                app.tree
                    .get(destination.id)
                    .map(|node| node.name.as_str())
                    .unwrap_or("group")
            };
            let row_style = if index == state.selected_index {
                theme::selected_style().add_modifier(Modifier::BOLD)
            } else if is_top_level {
                Style::new().fg(GROUP_ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            Line::from(Span::styled(
                format!(" {indent}{icon} {name_str}"),
                row_style,
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), layout.list_area);

    let scroll_hint = if state.destinations.len() > modal::CREATE_GROUP_MAX_VISIBLE {
        " · more above/below"
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("\u{2191}\u{2193} choose · Enter/click create · Esc cancel{scroll_hint}"),
            Style::new().add_modifier(Modifier::DIM),
        )),
        layout.hint_row,
    );
    cursor
}

/// Draws the focused pane's live content (terminal screen or editor
/// buffer), or a placeholder when nothing is focused.
#[cfg(test)]
fn draw_pane(frame: &mut Frame, area: Rect, app: &App) {
    let _ = draw_pane_sources(frame, area, app);
}
fn draw_pane_sources(
    frame: &mut Frame,
    area: Rect,
    app: &App,
) -> Vec<(ilium_core::NodeId, terminal_view::PaintedTerminal)> {
    let root_mode = app.modal_stack.first().unwrap_or(&app.mode);
    if let Mode::AgentDebugLog(state) = root_mode {
        crate::agent_debug_ui::render(frame, area, app, state);
        return Vec::new();
    }
    if let RightPanelTarget::Chatroom { project_id } = &app.right_panel_target {
        crate::chatroom_ui::render(frame, area, app, *project_id);
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
        let placeholder =
            Paragraph::new(message).block(theme::block(false).title(theme::chrome_title(title)));
        frame.render_widget(placeholder, area);
        return Vec::new();
    }

    let mut sources = Vec::with_capacity(viewports.len());
    for viewport in viewports {
        draw_pane_runtime(frame, app, viewport, &mut sources);
    }
    draw_screen_transfer_controls(frame, app);
    sources
}

/// Draws the two directional actions in the middle of every eligible split
/// separator. They are rendered after pane borders so the configured glyphs
/// remain directly clickable instead of being overwritten by chrome.
fn draw_screen_transfer_controls(frame: &mut Frame, app: &App) {
    for transfer in app.screen_transfers() {
        let icon_target = match transfer.direction {
            crate::split_layout::PaneDirection::Left => IconTarget::ScreenTransferLeft,
            crate::split_layout::PaneDirection::Right => IconTarget::ScreenTransferRight,
            crate::split_layout::PaneDirection::Up => IconTarget::ScreenTransferUp,
            crate::split_layout::PaneDirection::Down => IconTarget::ScreenTransferDown,
        };
        let glyph = app.ui_settings.icons.glyph(icon_target);
        let control = Paragraph::new(Span::styled(
            glyph,
            theme::selected_style().add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center);
        frame.render_widget(control, transfer.control_area);
    }
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
    let completed_agent_close_action = app.completed_agent_close_action(viewport);
    let Some(runtime) = app.panes.get(&viewport.pane_id) else {
        let placeholder = Paragraph::new(
            app.editor_load_error(viewport.pane_id)
                .unwrap_or("pane is loading"),
        )
        .block(theme::block(pane_focused).title(theme::chrome_title(&pane_title)));
        frame.render_widget(placeholder, viewport.outer_area);
        if let Some(action) = completed_agent_close_action {
            draw_completed_agent_close_action(frame, action.button_area);
        }
        return;
    };

    match runtime {
        PaneRuntime::Terminal(term) => {
            let block = theme::block(pane_focused).title(theme::chrome_title(&pane_title));
            frame.render_widget(block, viewport.outer_area);
            let frozen_dialog = app.frozen_dialog_layout(viewport);
            let completed_terminal_area = completed_agent_close_action
                .map_or(viewport.content_area, |action| action.terminal_area);
            let terminal_area = frozen_dialog
                .as_ref()
                .map_or(completed_terminal_area, |layout| layout.terminal_area);
            let smart_copy = app
                .smart_copy_session
                .as_ref()
                .filter(|session| session.pane_id == viewport.pane_id);
            if let Some(session) = smart_copy {
                terminal_view::render_frozen_screen(
                    &session.snapshot.screen,
                    terminal_area,
                    frame.buffer_mut(),
                );
                draw_smart_copy_highlights(frame, terminal_area, session);
            } else if let Some(source) = app.frozen_screens.get(&viewport.pane_id) {
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
                let display_error = term.admission_error.as_deref().or_else(|| {
                    (term.frontend.is_some() && !term.has_initial_display())
                        .then_some("waiting for initial terminal state")
                });
                if let Some(error) = display_error {
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
            let source = if smart_copy.is_some() {
                app.smart_copy_terminal_source(viewport.pane_id).cloned()
            } else {
                Some(
                    app.selection_terminal_source(viewport.pane_id)
                        .cloned()
                        .unwrap_or_else(|| term.painted_source()),
                )
            };
            if let Some(source) = source {
                sources.push((viewport.pane_id, source));
            }
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

    if let Some(toolbar_area) = viewport.toolbar_area {
        if matches!(runtime, PaneRuntime::Terminal(_)) {
            if let Some(session) = app
                .smart_copy_session
                .as_ref()
                .filter(|session| session.pane_id == viewport.pane_id)
            {
                draw_smart_copy_toolbar(frame, toolbar_area, session);
                if session.candidates.is_empty() && !session.is_light {
                    draw_smart_copy_progress_dialog(frame, viewport.content_area, session);
                }
                return;
            }
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
                    },
                    hovered,
                );
            }
        }
    }

    if let Some(last_prompt_area) = viewport.last_prompt_area {
        if matches!(runtime, PaneRuntime::Terminal(_))
            && app.shows_last_prompt_banner(viewport.pane_id)
        {
            crate::last_prompt_banner::render(
                frame,
                last_prompt_area,
                app.display_last_agent_prompt(viewport.pane_id),
                app.ui_settings.last_prompt_max_lines.into(),
                app.ui_settings.color_scheme,
            );
        }
    }

    if let Some(progress_area) = viewport.progress_area {
        if matches!(runtime, PaneRuntime::Terminal(_))
            && app.shows_progress_footer(viewport.pane_id)
        {
            crate::progress_bar::render(
                frame,
                progress_area,
                app.tree.pane_progress(viewport.pane_id),
                app.ui_settings.progress_max_lines.into(),
                app.ui_settings.color_scheme,
            );
        }
    }

    if let Some(action) = completed_agent_close_action {
        draw_completed_agent_close_action(frame, action.button_area);
    }
}

fn draw_smart_copy_toolbar(
    frame: &mut Frame,
    area: Rect,
    session: &crate::smart_copy::SmartCopySession,
) {
    use crate::smart_copy::SmartCopyPhase;

    let phase = match &session.phase {
        SmartCopyPhase::Connecting => "connecting",
        SmartCopyPhase::Streaming => "streaming",
        SmartCopyPhase::Complete => "complete",
        SmartCopyPhase::Failed(_) => "failed",
    };
    let usage = session.exact_output_tokens.map_or_else(
        || format!("~{} tokens", session.estimated_output_tokens()),
        |tokens| format!("{tokens} tokens"),
    );
    let current = session
        .current_candidate()
        .map_or_else(String::new, |candidate| {
            let overlap = session
                .overlap_position()
                .map_or_else(String::new, |(index, count)| format!(" · {index}/{count}"));
            format!(" · {}{overlap}", candidate.label)
        });
    let exit = crate::smart_copy::exit_button_rect(area);
    let elapsed = session.started_at.elapsed().as_secs_f32();
    let progress_area = Rect::new(area.x, area.y, exit.x.saturating_sub(area.x), area.height);
    if session.is_light {
        frame.render_widget(
            Paragraph::new(format!(
                "🧲 Smart copy light · {} selected · click to add, release the key to copy{current}",
                session.selected_count()
            )),
            progress_area,
        );
        frame.render_widget(
            Paragraph::new("[ Exit ]")
                .alignment(Alignment::Center)
                .style(Style::new().add_modifier(Modifier::BOLD)),
            exit,
        );
        return;
    }
    frame.render_widget(
        Paragraph::new(format!(
            "🧲 Smart copy · {phase} · {elapsed:.1}s · {} selections · {usage}{current}",
            session.candidates.len()
        )),
        progress_area,
    );
    frame.render_widget(
        Paragraph::new("[ Exit ]")
            .alignment(Alignment::Center)
            .style(Style::new().add_modifier(Modifier::BOLD)),
        exit,
    );
}

fn draw_smart_copy_progress_dialog(
    frame: &mut Frame,
    content_area: Rect,
    session: &crate::smart_copy::SmartCopySession,
) {
    use crate::smart_copy::SmartCopyPhase;

    if content_area.width < 4 || content_area.height < 4 {
        return;
    }
    let width = content_area.width.min(64);
    let height = content_area.height.min(7);
    let area = Rect::new(
        content_area.x + (content_area.width - width) / 2,
        content_area.y + (content_area.height - height) / 2,
        width,
        height,
    );
    let state = match &session.phase {
        SmartCopyPhase::Connecting => "Waiting for the model to answer…".to_string(),
        SmartCopyPhase::Streaming => {
            "Response started; waiting for the first valid block…".to_string()
        }
        SmartCopyPhase::Complete => "The model returned no valid selectable blocks.".to_string(),
        SmartCopyPhase::Failed(error) => format!("Request failed: {error}"),
    };
    let usage = session.exact_output_tokens.map_or_else(
        || {
            format!(
                "Estimated output: ~{} tokens",
                session.estimated_output_tokens()
            )
        },
        |tokens| format!("Output: {tokens} tokens"),
    );
    let elapsed = session.started_at.elapsed().as_secs_f32();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(state),
            Line::from(usage),
            Line::from(format!("Elapsed: {elapsed:.1}s")),
            Line::from(format!(
                "Received characters: {} · rejected records: {}",
                session.received_characters, session.invalid_lines
            )),
        ])
        .block(theme::block(true).title(theme::chrome_title("🧲 Smart copy")))
        .alignment(Alignment::Center),
        area,
    );
}

fn draw_smart_copy_highlights(
    frame: &mut Frame,
    content_area: Rect,
    session: &crate::smart_copy::SmartCopySession,
) {
    let now = Instant::now();
    for (candidate_index, candidate) in session.candidates.iter().enumerate() {
        let is_current = session
            .current_candidate()
            .is_some_and(|current| std::ptr::eq(current, candidate));
        let is_flashing =
            now.duration_since(candidate.arrived_at) < crate::smart_copy::ARRIVAL_FLASH_DURATION;
        // Clicked regions stay inverted, exactly like the hover style, so the
        // whole multi-selection reads as one highlighted set.
        let is_selected = session.is_selected(candidate_index);
        if !is_current && !is_flashing && !is_selected {
            continue;
        }
        for span in &candidate.spans {
            if span.row >= content_area.height {
                continue;
            }
            let last_column = span.end_column.min(content_area.width.saturating_sub(1));
            for column in span.start_column..=last_column {
                let position = Position::new(
                    content_area.x.saturating_add(column),
                    content_area.y.saturating_add(span.row),
                );
                if let Some(cell) = frame.buffer_mut().cell_mut(position) {
                    cell.modifier.insert(Modifier::REVERSED);
                    if is_flashing {
                        cell.modifier.insert(Modifier::BOLD);
                    }
                }
            }
        }
    }
}

/// Renders the completed-agent acknowledgement as a full-width destructive
/// action, visually distinct from ordinary terminal output and chrome.
fn draw_completed_agent_close_action(frame: &mut Frame, area: Rect) {
    let style = Style::new()
        .fg(Color::White)
        .bg(Color::Red)
        .add_modifier(Modifier::BOLD);
    let button = Paragraph::new(Span::styled(
        crate::completed_agent_action::CLOSE_COMPLETED_AGENT_LABEL,
        style,
    ))
    .alignment(Alignment::Center)
    .style(style);
    frame.render_widget(button, area);
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
                draw_source_scrollbar(frame, chrome.content_area, editor);
            } else {
                frame.render_widget(
                    Paragraph::new(
                        editor
                            .preparation_error
                            .as_deref()
                            .unwrap_or("Preparing source…"),
                    ),
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
                draw_rendered_scrollbar(frame, chrome.content_area, document, editor);
            }
            None => {
                frame.render_widget(
                    Paragraph::new(editor.preparation_error.as_deref().unwrap_or("Rendering…")),
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
    area: Rect,
    document: &markdown::render::RenderedDocument,
    editor: &EditorPane,
) {
    let total_height = markdown::view::content_height(document, area.width, editor.line_display);
    if total_height <= area.height {
        return;
    }
    let mut scrollbar_state = ScrollbarState::new(usize::from(total_height))
        .position(usize::from(editor.rendered_scroll));
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"));
    frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
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
            crate::pane_title::decorate_pane_title(status, &logical_title)
        }
        NodeKind::Container(_) | NodeKind::Folder { .. } => node.name.clone(),
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
        Mode::Rename(_) => "RENAME",
        Mode::CommandPrompt(_) => "RUN COMMAND",
        Mode::InferenceSettingPrompt(_, _) => "INFERENCE SETTING",
        Mode::VoiceSettingPrompt(_, _) => "VOICE SETTING",
        Mode::ApiSettingPrompt(_) => "HTTP API PORT",
        Mode::GitSettingPrompt(_, _) => "GIT SETTING",
        Mode::AnimationTextPrompt(_, _) => "ANIMATION SETTING",
        Mode::LocationPicker(_) => "LOCATION",
        Mode::AgentSetupPathPrompt(_, _) => "AGENT SETUP FILE",
        Mode::AgentSetupPrompt(_) => "AGENT SETUP",
        Mode::VoicePromptEditor(_) => "VOICE PROMPT",
        Mode::SaveAs(..) => "SAVE AS",
        Mode::Help => "HELP",
        Mode::Explorer(..) => "FILE PICKER",
        Mode::ExplorerFileMenu(_) => "FILE ACTIONS",
        Mode::FolderExplorer(..) => "FOLDER PICKER",
        Mode::ProjectFolderExplorer(..) => "PROJECT FOLDER PICKER",
        Mode::ContextMenu(..) => "TREE ACTIONS",
        Mode::TerminalPaneContextMenu(..) => "TERMINAL ACTIONS",
        Mode::AgentToolbarModelSubmenu(..) => "MODEL STRENGTH",
        Mode::SmartCopy => "SMART COPY",
        Mode::AgentDebugLog(..) => "AGENT DEBUG LOG",
        Mode::AgentDebugSavePath(..) => "SAVE AGENT DEBUG LOG",
        Mode::SchedulePaneInput(..) => "SCHEDULE INPUT",
        Mode::QueuePrompt(..) => "QUEUE PROMPT",
        Mode::ValueDialog(..) => "VALUE OPTIONS",
        Mode::TextTriggerDialog(..) => "TEXT TRIGGER",
        Mode::AgentMessageDialog(..) => "MESSAGE TO AGENTS",
        Mode::EditorLineContextMenu(..) => "LINE ACTIONS",
        Mode::CreateAgentFromLine(..) => "CREATE AGENT",
        Mode::CreateAgentWorkspace(..) => "CREATE AGENT WORKTREE",
        Mode::WorktreeManager(..) => "WORKTREES",
        Mode::WaitingWorkspaceCloseOffer { .. } => "CHECKING WORKTREE",
        Mode::ConfirmWorkspaceCloseOffer(..) => "WORKTREE CLOSE",
        Mode::CreateGroup(_) => "NEW GROUP",
        Mode::CreateSplitOrientation(_) => "NEW SPLIT",
        Mode::CreateSplitMembers(_) => "SELECT SPLIT PANES",
        Mode::CreateBoard(_) => "NEW BOARD",
        Mode::BoardPathPicker(_) => "BOARD PATH",
        Mode::BoardCardPrompt(_, _) => "NEW CARD",
        Mode::BoardColumnPrompt(_, _) => "NEW COLUMN",
        Mode::BoardRenamePrompt(_, _, _) => "RENAME BOARD ITEM",
        Mode::BoardDeleteConfirm(_, _) => "DELETE BOARD ITEM",
        Mode::ConfirmClose(_) => "CONFIRM CLOSE",
        Mode::ConvertSession => "CONVERTING SESSION",
        Mode::RemoteCompaction => "REMOTE COMPACTION",
        Mode::ConfirmRemoveWorkspace(_) => "REMOVE WORKTREE",
        Mode::ConfirmSessionRecovery { .. } => "SESSION RECOVERY",
        Mode::Search(_) => "SEARCH",
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
    if let Some(message) = &app.status_message {
        spans.push(Span::raw("  —  "));
        // Borrow rather than `message.clone()` -- `app` outlives this
        // function's local `spans`, so there is no need to allocate a new
        // `String` copy of the status message on every render frame.
        spans.push(Span::raw(message.as_str()));
    } else if let Some(error) = app.semantic_animation_error() {
        spans.push(Span::raw("  —  "));
        spans.push(Span::raw(error));
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
    let now = chrono::Utc::now();
    let standard_maximum_pill_width = usize::from(inner.width.saturating_sub(20));
    let mut reset_labels = Vec::new();
    for provider in [
        crate::reset_planning::ResetProvider::Claude,
        crate::reset_planning::ResetProvider::Codex,
    ] {
        if !provider.enabled(&app.reset_planning_settings) {
            continue;
        }
        if let Some(scheduled) = &app.reset_monitor_state.provider(provider).scheduled {
            reset_labels.push(crate::reset_planning::countdown_text(
                provider,
                scheduled,
                app.reset_planning_settings.time_style,
                now,
            ));
        } else if let Some(watch) = &app.reset_monitor_state.provider(provider).active_watch {
            if let Some(label) = crate::reset_planning::active_reset_watch_text(
                provider,
                watch,
                app.reset_planning_settings.time_style,
                now,
            ) {
                if UnicodeWidthStr::width(label.as_str()) + 2 <= standard_maximum_pill_width {
                    reset_labels.push(label);
                } else if let Some(compact_label) =
                    crate::reset_planning::compact_active_reset_watch_text(provider, watch, now)
                {
                    reset_labels.push(compact_label);
                }
            }
        }
    }
    let maximum_pill_width = usize::from(inner.width.saturating_sub(14));
    let combined = reset_labels.join("  ·  ");
    let reset_label = if !reset_labels.is_empty()
        && UnicodeWidthStr::width(combined.as_str()) + 2 <= maximum_pill_width
    {
        Some(combined)
    } else {
        reset_labels
            .into_iter()
            .find(|label| UnicodeWidthStr::width(label.as_str()) + 2 <= maximum_pill_width)
    };
    let pill_width = reset_label
        .as_ref()
        .map(|label| UnicodeWidthStr::width(label.as_str()) as u16 + 2)
        .unwrap_or(0);
    let left_width = inner
        .width
        .saturating_sub(pill_width.saturating_add(u16::from(pill_width > 0)));
    frame.render_widget(Paragraph::new("").style(bar_style), inner);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(bar_style),
        Rect::new(inner.x, inner.y, left_width, inner.height),
    );
    if let Some(label) = reset_label {
        let pill_area = Rect::new(
            inner.right().saturating_sub(pill_width),
            inner.y,
            pill_width,
            inner.height,
        );
        let pill_style = Style::new()
            .fg(Color::Rgb(0xd8, 0xed, 0xff))
            .bg(Color::Rgb(0x98, 0x45, 0x13));
        frame.render_widget(
            Paragraph::new(format!(" {label} ")).style(pill_style),
            pill_area,
        );
    }
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
    #[test]
    fn smart_copy_preview_fits_narrow_screens() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "synthetic-narrow-preview".into(),
            project.path().to_path_buf(),
        );
        app.smart_copy_preview = Some(crate::smart_copy_light::SmartCopyPreview::new(
            "界界界界界界界界界界界界".into(),
            1,
            true,
            Instant::now(),
        ));
        for width in [12, 16, 20, 27, 28, 40] {
            let area = Rect::new(0, 0, width, 9);
            let mut terminal = Terminal::new(TestBackend::new(width, area.height)).unwrap();
            terminal
                .draw(|frame| super::draw_smart_copy_preview(frame, area, &app))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let top = (0..area.height)
                .find(|y| (0..width).any(|x| buffer[(x, *y)].symbol() == "╭"))
                .expect("the preview frame must remain visible");
            let row: String = (0..width).map(|x| buffer[(x, top)].symbol()).collect();
            assert!(row.contains("Preview"));
            assert!(
                row.contains('╮'),
                "the right border must fit at width {width}"
            );
            crate::ui_capture::save(&format!("smart-copy-preview-{width}x9"), &terminal);
        }
    }

    #[test]
    fn smart_copy_preview_withholds_scene_attribution_and_recovers_after_close() {
        use crate::background_animation::test_support::{fake_host, FakeProbe};
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("preview-paint".into(), project.path().to_path_buf());
        let area = Rect::new(0, 0, 100, 30);
        app.set_screen_area(area);
        let probe = FakeProbe::new();
        *app.animation_frame.host_mut() = fake_host(&probe);
        app.animation_settings.enabled = true;
        app.animation_settings.kind = crate::background_animation::AnimationKind::Stars;
        app.animation_settings.density_percent = 100;
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| {
                draw_at_with_cursor(frame, &mut app, Duration::ZERO);
            })
            .unwrap();
        app.animation_frame.settle_for_test();
        terminal
            .draw(|frame| {
                draw_at_with_cursor(frame, &mut app, Duration::ZERO);
            })
            .unwrap();
        assert!(
            app.animation_frame
                .composed_bits()
                .iter()
                .any(|bits| *bits != 0),
            "Control draw must contain actual composed scene ink"
        );
        app.smart_copy_preview = Some(crate::smart_copy_light::SmartCopyPreview::new(
            "⣿⣿⣿⣿".into(),
            1,
            true,
            Instant::now(),
        ));
        let preview = terminal
            .draw(|frame| {
                draw_at_with_cursor(frame, &mut app, Duration::ZERO);
            })
            .unwrap();
        assert!(
            preview
                .buffer
                .content()
                .iter()
                .any(|cell| cell.symbol().contains('⣿')),
            "The actual copied Braille preview must render"
        );
        assert!(
            app.animation_frame.composed_bits().is_empty(),
            "Late arbitrary preview text must not retain scene attribution"
        );
        app.smart_copy_preview = None;
        terminal
            .draw(|frame| {
                draw_at_with_cursor(frame, &mut app, Duration::ZERO);
            })
            .unwrap();
        assert!(
            app.animation_frame
                .composed_bits()
                .iter()
                .any(|bits| *bits != 0),
            "Closing preview must restore ordinary scene attribution"
        );
    }
    use super::*;
    use crate::app::{PaneRuntime, RightPanelTarget};
    use crate::terminal_view::TerminalView;
    use ilium_core::{AgentActivity, GoalState, PaneContentKind, SplitOrientation};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use ratatui_textarea::TextArea;
    use std::path::PathBuf;

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
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(
                    AgentClass::Codex,
                    AgentActivity::WaitingApproval,
                    Some(GoalState::Blocked),
                ),
            )
            .unwrap();
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
        assert!(rendered.contains("Attention priority 1:"));
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
    fn scheduled_reset_uses_a_right_aligned_orange_and_light_blue_status_segment() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp"));
        let now = chrono::Utc::now();
        app.reset_monitor_state.codex.scheduled = Some(crate::reset_planning::ScheduledReset {
            announced_at: now,
            scheduled_for: Some(now + chrono::Duration::hours(2)),
            source_url: "https://x.com/thsottiaux/status/1".to_owned(),
            is_banked: false,
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 1)).unwrap();
        terminal
            .draw(|frame| draw_status_bar(frame, frame.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let line = (0..100)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        assert!(line.contains("Codex reset in"));
        let pill_cell = &buffer[(90, 0)];
        assert_eq!(pill_cell.fg, Color::Rgb(0xd8, 0xed, 0xff));
        assert_eq!(pill_cell.bg, Color::Rgb(0x98, 0x45, 0x13));

        app.reset_planning_settings.monitor_codex = false;
        terminal
            .draw(|frame| draw_status_bar(frame, frame.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let line = (0..100)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        assert!(!line.contains("Codex reset"));
    }

    #[test]
    fn active_codex_reset_watch_is_shown_as_a_possible_window_only_while_monitoring_is_enabled() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp"));
        app.reset_planning_settings.time_style = crate::reset_planning::ResetTimeStyle::Human;
        let now = chrono::Utc::now();
        let watch = crate::reset_planning::ActiveResetWatch {
            expires_at: now + chrono::Duration::days(2),
        };
        let expected = crate::reset_planning::active_reset_watch_text(
            crate::reset_planning::ResetProvider::Codex,
            &watch,
            app.reset_planning_settings.time_style,
            now,
        )
        .expect("an unexpired active watch should have status text");
        app.reset_monitor_state.codex.active_watch = Some(watch);
        let mut terminal = Terminal::new(TestBackend::new(140, 1)).unwrap();
        terminal
            .draw(|frame| draw_status_bar(frame, frame.area(), &app))
            .unwrap();
        let line = (0..140)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect::<String>();
        assert!(line.contains("Possible Codex reset watch window ends in"));
        assert!(line.contains(&expected));

        app.reset_planning_settings.monitor_codex = false;
        terminal
            .draw(|frame| draw_status_bar(frame, frame.area(), &app))
            .unwrap();
        let line = (0..140)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect::<String>();
        assert!(!line.contains("Possible Codex reset watch"));
    }

    #[test]
    fn active_codex_reset_watch_keeps_a_compact_label_at_typical_terminal_width() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp"));
        app.reset_planning_settings.time_style = crate::reset_planning::ResetTimeStyle::Human;
        app.reset_monitor_state.codex.active_watch =
            Some(crate::reset_planning::ActiveResetWatch {
                expires_at: chrono::Utc::now()
                    + chrono::Duration::days(2)
                    + chrono::Duration::hours(1),
            });
        let mut terminal = Terminal::new(TestBackend::new(58, 1)).unwrap();
        terminal
            .draw(|frame| draw_status_bar(frame, frame.area(), &app))
            .unwrap();
        let line = (0..58)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect::<String>();
        assert!(
            line.contains("Possible Codex reset watch ends in 2d1h"),
            "{line}"
        );
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
    fn right_panel_title_prefixes_done_without_mutating_the_pane_name() {
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

        assert_eq!(
            pane_title(&app, pane_id),
            "[done] Review authentication — Codex"
        );
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
    fn completed_agent_renders_a_full_width_red_close_action_at_the_panel_bottom() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "Completed review", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Codex, ilium_core::AgentActivity::Done, None),
            )
            .unwrap();
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
        );
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let action = app
            .completed_agent_close_action(app.pane_viewport(pane_id).unwrap())
            .expect("done agent should expose its close action");
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        terminal
            .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
            .unwrap();

        let button_cells = terminal
            .backend()
            .buffer()
            .content()
            .chunks(120)
            .nth(usize::from(action.button_area.y))
            .expect("button row is inside the test backend")
            .get(usize::from(action.button_area.x)..usize::from(action.button_area.right()))
            .expect("button spans the complete inner panel row");
        assert!(button_cells.iter().all(|cell| cell.bg == Color::Red));
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Close finished agent"));
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
        app.mode = Mode::Settings(crate::app::SettingsState {
            tab: crate::app::SettingsTab::VoiceControl,
            selected_row: 1,
            ..crate::app::SettingsState::default()
        });
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
    fn agent_debug_mode_replaces_the_complete_right_panel_with_aerated_history() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "codex", PaneContentKind::Terminal)
            .unwrap();
        let mut terminal_view = TerminalView::new(24, 80);
        terminal_view.feed(b"NORMAL TERMINAL MUST BE HIDDEN");
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(terminal_view)));
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.mode = Mode::AgentDebugLog(crate::app::AgentDebugLogViewState {
            pane_id,
            scroll_position: crate::app::AgentDebugLogScrollPosition::FromNewest(0),
        });
        app.agent_debug_logs.insert(
            pane_id,
            crate::app::AgentDebugLogCache {
                through_sequence: 1,
                log: ilium_ipc::PaneDebugLog {
                    entries: vec![ilium_ipc::AgentDebugEntry {
                        sequence: 1,
                        occurred_at_unix_millis: 1_700_000_000_000,
                        severity: ilium_ipc::AgentDebugSeverity::Success,
                        source: ilium_ipc::AgentDebugSource::SessionDiscovery,
                        kind: ilium_ipc::AgentDebugEventKind::SessionResolved,
                        summary: "Project-verified agent session resolved".to_string(),
                        fields: vec![ilium_ipc::AgentDebugField::sensitive(
                            "session ID",
                            "session-123",
                        )],
                        correlation_id: None,
                        context: ilium_ipc::AgentDebugContext::default(),
                        metadata: Default::default(),
                    }],
                    ..Default::default()
                },
                ..crate::app::AgentDebugLogCache::default()
            },
        );
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains('🐞'));
        assert!(rendered.contains("Agent debug log"));
        assert!(rendered.contains("SESSION"));
        assert!(rendered.contains("Project-verified agent session resolved"));
        assert!(rendered.contains('🔒'));
        assert!(rendered.contains("session ID: session-123"));
        assert!(rendered.contains("← Back to agent"));
        assert!(rendered.contains("Save log"));
        assert!(rendered.contains("Panel resizes hidden"));
        assert!(!rendered.contains("NORMAL TERMINAL MUST BE HIDDEN"));
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
        app.ui_settings.tree_order = crate::config::TreeOrder::AgeDescending;
        app.open_context_menu(ROOT_ID, 2, 2);
        let Mode::ContextMenu(mut menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("context menu should be open");
        };
        app.open_context_submenu(&mut menu, crate::app::ContextMenuAction::OrderBy);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

        terminal
            .draw(|frame| {
                draw_context_menu(
                    frame,
                    &menu,
                    crate::config::TreeOrder::AgeDescending,
                    &app.ui_settings,
                )
            })
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
        let active_row = format!("✓ {}{wide_cell_padding}  Oldest first", icon.trim_start());
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
                .draw(|frame| draw_context_menu(frame, &menu, crate::config::TreeOrder::Manual, ui))
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
    fn scheduled_input_dialog_renders_all_aerated_controls() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "release shell", PaneContentKind::Terminal)
            .unwrap();
        app.mode = Mode::SchedulePaneInput(Box::new(
            crate::scheduled_input::ScheduledInputDialogState::new(pane_id),
        ));
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("Schedule keystrokes"));
        assert!(rendered.contains("Schedule input for release shell"));
        assert!(rendered.contains("Hours"));
        assert!(rendered.contains("Minutes"));
        assert!(rendered.contains("Seconds"));
        assert!(rendered.contains("Text (optional)"));
        assert!(rendered.contains("[x] Send Enter after the text"));
        assert!(rendered.contains("[ Schedule input ]"));
    }

    #[test]
    fn text_trigger_dialog_visually_marks_matches_and_loop_risk() {
        use ilium_execution::{
            Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
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
                clients: 4,
                jobs: 4,
                service_jobs: 0,
                input_bytes: 64 * MIB,
                result_bytes: 32 * MIB,
                worker_threads: 1,
                worker_bytes: 64 * MIB,
            }),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 4,
                    priority: None,
                    resident_bytes_per_thread: MIB,
                },
                io: disabled,
                service: disabled,
            },
        )
        .expect("isolated actual preview CPU worker");
        let client = execution
            .client(crate::text_trigger_dialog::preview_limits())
            .unwrap();
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.configure_text_trigger_preview(client);
        let mut state = crate::text_trigger_dialog::TextTriggerDialogState::new(None);
        state.regexp.buf = "ready".to_owned();
        state.message.buf = "ready".to_owned();
        state.sample = TextArea::from(vec!["not yet".to_owned(), "system ready now".to_owned()]);
        app.mode = Mode::TextTriggerDialog(Box::new(state));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            app.reconcile_text_trigger_preview();
            let Mode::TextTriggerDialog(state) = &app.mode else {
                panic!("dialog lost");
            };
            if state.preview_display().0.contains("echoed input can loop") {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "actual CPU preview did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        app.set_screen_area(Rect::new(0, 0, 100, 34));
        let mut terminal = Terminal::new(TestBackend::new(100, 34)).unwrap();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("LIVE PREVIEW"));
        assert!(rendered.contains("Regexp: ready"));
        assert!(rendered.contains("Message: ready"));
        assert!(rendered.contains("· not yet"));
        assert!(rendered.contains("✓ system [ready] now"));
        assert!(rendered.contains("echoed input can loop"));
        let Mode::TextTriggerDialog(state) = &mut app.mode else {
            panic!("dialog lost");
        };
        state.regexp = crate::text_prompt::TextPromptState::new("a".repeat(256 * 1024 + 1));
        state.mark_preview_dirty();
        app.reconcile_text_trigger_preview();
        let Mode::TextTriggerDialog(state) = &app.mode else {
            panic!("dialog lost");
        };
        assert_eq!(
            state.preview_issue(),
            Some(crate::text_trigger_dialog::TextTriggerPreviewIssue::CaptureLimit)
        );
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let refused = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(refused.contains("bounded capture limit"));
        assert!(
            refused.contains("✓ system [ready] now"),
            "last completed preview must remain visible on refusal"
        );
        app.close_text_trigger_preview();
        execution.request_shutdown(ShutdownMode::Drain);
        let report = execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert!(report.shutdown_complete);
        app.collect_closed_text_trigger_preview();
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

    #[test]
    fn split_view_renders_configured_directional_screen_transfer_controls() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let left = app
            .tree
            .add_pane(group, "left", PaneContentKind::Terminal)
            .unwrap();
        let right = app
            .tree
            .add_pane(group, "right", PaneContentKind::Terminal)
            .unwrap();
        let split = app
            .tree
            .create_split_view(
                group,
                "Vertical split",
                SplitOrientation::Vertical,
                &[left, right],
            )
            .unwrap();
        app.panes.insert(
            left,
            PaneRuntime::Terminal(Box::new(TerminalView::new(20, 30))),
        );
        app.panes.insert(
            right,
            PaneRuntime::Terminal(Box::new(TerminalView::new(20, 30))),
        );
        app.right_panel_target = RightPanelTarget::SplitView {
            split_id: split,
            active_pane_id: Some(left),
        };
        app.ui_settings
            .icons
            .set(IconTarget::ScreenTransferRight, "⇢".to_string());
        app.ui_settings
            .icons
            .set(IconTarget::ScreenTransferLeft, "⇠".to_string());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let rightward = app
            .screen_transfers()
            .into_iter()
            .find(|transfer| {
                transfer.source_pane_id == left
                    && transfer.destination_pane_id == right
                    && transfer.direction == crate::split_layout::PaneDirection::Right
            })
            .unwrap();
        let leftward = app
            .screen_transfers()
            .into_iter()
            .find(|transfer| {
                transfer.source_pane_id == right
                    && transfer.destination_pane_id == left
                    && transfer.direction == crate::split_layout::PaneDirection::Left
            })
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        terminal
            .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(rightward.control_area.x, rightward.control_area.y)].symbol(),
            "⇢"
        );
        assert_eq!(
            buffer[(leftward.control_area.x, leftward.control_area.y)].symbol(),
            "⇠"
        );
    }
}

/// Real source-window UI route, prepared on the shared finite CPU bank.
#[cfg(test)]
pub(crate) fn test_source_window_pixels(
    editor: &mut EditorPane,
    width: u16,
    height: u16,
) -> ratatui::buffer::Buffer {
    editor.show_minimap = false;
    let area = Rect::new(0, 0, width, height + crate::editor_chrome::TOOLBAR_HEIGHT);
    let content = crate::editor_chrome::compute(area, false).content_area;
    assert_eq!(content.width, width);
    assert_eq!(content.height, height);
    editor.prepare_test_source_window_with_syntax(
        width,
        height,
        &ratatui_image::picker::Picker::halfblocks(),
    );
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(area.width, area.height))
            .unwrap();
    terminal
        .draw(|frame| draw_editor(frame, area, editor))
        .unwrap();
    let mut output = ratatui::buffer::Buffer::empty(Rect::new(0, 0, width, height));
    for row in 0..height {
        for column in 0..width {
            output[(column, row)] =
                terminal.backend().buffer()[(content.x + column, content.y + row)].clone();
        }
    }
    output
}

#[cfg(test)]
mod text_trigger_numeric_control_tests {
    #[test]
    fn delay_renders_centered_number_with_decrement_increment_and_entry() {
        for (width, height) in [(80, 24), (120, 40)] {
            let state = crate::text_trigger_dialog::TextTriggerDialogState::new(None);
            let area = ratatui::layout::Rect::new(0, 0, width, height);
            let delay = crate::text_trigger_dialog::layout(area).delay;
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| super::draw_text_trigger_dialog(frame, area, &state))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let position = |symbol: &str| {
                (delay.x..delay.right())
                    .find(|&column| buffer[(column, delay.y)].symbol() == symbol)
                    .unwrap_or_else(|| panic!("{width}x{height}: delay lacks {symbol}"))
            };
            let decrement = position(crate::value_control::NUMBER_DECREMENT_GLYPH);
            let increment = position(crate::value_control::NUMBER_INCREMENT_GLYPH);
            let entry = position("*");
            let first_digit = position("6");
            let last_digit = position("0");
            assert!(decrement < first_digit && last_digit < increment && increment < entry);
            assert!(
                (i32::from(first_digit + last_digit) - i32::from(decrement + increment)).abs() <= 1,
                "{width}x{height}: delay value is not centered between its step buttons"
            );
        }
    }
}

#[cfg(test)]
mod source_touch_provenance_tests {
    use super::*;

    #[test]
    fn late_modal_and_suspended_parent_withhold_scene_history() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("touch provenance".into(), project.path().to_path_buf());
        assert!(late_layers_are_source_transparent(&app));
        app.mode = Mode::Help;
        assert!(!late_layers_are_source_transparent(&app));
        app.mode = Mode::Normal;
        app.modal_stack.push(Mode::Help);
        assert!(!late_layers_are_source_transparent(&app));
    }
}

#[cfg(test)]
mod worktree_close_dialog_tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn waiting_workspace_close_keeps_status_and_cancel_visible_on_small_terminals() {
        for (width, height) in [(24, 10), (40, 12), (60, 20), (80, 24)] {
            let app = App::new(
                "worktree close visual regression".into(),
                std::env::temp_dir(),
            );
            let mode = Mode::WaitingWorkspaceCloseOffer {
                request_id: 1,
                pane_id: ROOT_ID,
            };
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    draw_mode_overlay(frame, frame.area(), &app, &mode);
                })
                .unwrap();

            let rendered = terminal
                .backend()
                .buffer()
                .content()
                .chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                rendered.contains("Worktree close"),
                "{width}x{height}: {rendered}"
            );
            assert!(
                rendered.contains("Checking"),
                "{width}x{height}: {rendered}"
            );
            assert!(
                rendered.contains("Esc cancels"),
                "{width}x{height}: {rendered}"
            );
            if width <= 40 {
                assert!(
                    rendered.contains("Checking worktree…"),
                    "narrow terminal should use the compact status: {rendered}"
                );
            }
            if width == 80 {
                assert!(
                    rendered.contains("Checking whether this worktree can be removed"),
                    "wide terminal should retain the full status: {rendered}"
                );
            }
        }
    }
}

#[cfg(test)]
mod context_menu_visual_tests {
    use super::*;
    use crate::terminal_context_menu::{TerminalContextAction, TerminalPaneContextMenu};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn context_menu_joined_emoji_label_preserves_row_width() {
        let line = popup_action_line("👩‍💻 work", String::new(), None, true, 20, Style::new());
        assert_eq!(UnicodeWidthStr::width(line.spans[0].content.as_ref()), 20);
        assert!(line.spans[0].content.ends_with(" ▸ "));
    }

    #[test]
    fn context_menu_custom_icon_gutter_aligns_labels_and_submenu_arrows() {
        let mut ui = crate::config::UiSettings::default();
        ui.icons.terminal = "T".into();
        ui.icons.editor = "📝".into();
        let gutter = popup_icon_width(&ui, [IconTarget::Terminal, IconTarget::Editor].into_iter());
        let line = |target| {
            popup_action_line(
                "Command",
                popup_icon(&ui, target, gutter),
                None,
                true,
                30,
                Style::new(),
            )
        };
        let narrow = line(IconTarget::Terminal);
        let wide = line(IconTarget::Editor);
        let narrow_text = &narrow.spans[0].content;
        let wide_text = &wide.spans[0].content;
        let column = |text: &str| UnicodeWidthStr::width(&text[..text.find("Command").unwrap()]);
        assert_eq!(column(narrow_text), column(wide_text));
        assert_eq!(UnicodeWidthStr::width(narrow_text.as_ref()), 30);
        assert_eq!(UnicodeWidthStr::width(wide_text.as_ref()), 30);
        assert!(narrow_text.ends_with(" ▸ "));
        assert!(wide_text.ends_with(" ▸ "));
        ui.show_context_menu_icons = false;
        assert_eq!(popup_icon_width(&ui, [IconTarget::Terminal].into_iter()), 0);
        let text_only = popup_action_line(
            "Command",
            popup_icon(&ui, IconTarget::Terminal, 0),
            None,
            false,
            30,
            Style::new(),
        );
        assert!(text_only.spans[0].content.starts_with("  Command"));
    }

    fn save_frame(name: &str, terminal: &Terminal<TestBackend>) {
        crate::ui_capture::save_with_env(name, terminal, "ILIUM_MENU_RENDER_DIR");
    }

    #[test]
    fn context_menu_visual_target_matrix_uses_production_actions() {
        use ilium_core::{
            AgentActivity, AgentClass, PaneContentKind, PaneStatus, SplitOrientation,
        };

        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "synthetic menu matrix".into(),
            directory.path().to_path_buf(),
        );
        let project = app
            .tree
            .add_project(directory.path().to_path_buf())
            .unwrap();
        let group = app.tree.add_group(project, "work").unwrap();
        let folder = app
            .tree
            .add_folder(group, directory.path().join("files"))
            .unwrap();
        let split = app
            .tree
            .create_split_view(group, "split", SplitOrientation::Vertical, &[])
            .unwrap();
        let shell = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let editor = app
            .tree
            .add_pane(group, "notes", PaneContentKind::Editor)
            .unwrap();
        let markdown = app
            .tree
            .add_pane(group, "tasks.md", PaneContentKind::Editor)
            .unwrap();
        app.restored_editor_paths
            .insert(markdown, directory.path().join("tasks.md"));
        let board = app
            .tree
            .add_pane(group, "board", PaneContentKind::Board)
            .unwrap();
        let mut targets = vec![
            ("root", ROOT_ID),
            ("project", project),
            ("group", group),
            ("folder", folder),
            ("split", split),
            ("shell", shell),
            ("editor", editor),
            ("markdown", markdown),
            ("board", board),
        ];
        for (name, class) in [
            ("claude", AgentClass::Claude),
            ("codex", AgentClass::Codex),
            ("antigravity", AgentClass::Antigravity),
        ] {
            let pane = app
                .tree
                .add_pane(group, name, PaneContentKind::Terminal)
                .unwrap();
            app.tree
                .set_pane_status(
                    pane,
                    PaneStatus::from_activity(class, AgentActivity::Idle, None),
                )
                .unwrap();
            targets.push((name, pane));
        }
        let worktree = app
            .tree
            .add_pane(group, "worktree", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_workspace(
                worktree,
                Some(ilium_core::PaneWorkspace {
                    workspace_id: Some("synthetic-menu-workspace".into()),
                    repo_common_dir: directory.path().join("repo.git"),
                    worktree_root: directory.path().join("worktree"),
                    branch: "synthetic-branch".into(),
                    base_ref: "main".into(),
                    base_commit: "synthetic".into(),
                    created_by_ilium: true,
                    created_at_unix: 0,
                }),
            )
            .unwrap();
        targets.push(("worktree", worktree));
        app.set_screen_area(Rect::new(0, 0, 100, 40));
        for (name, target) in targets {
            app.open_context_menu(target, 2, 2);
            let Mode::ContextMenu(menu) = &app.mode else {
                panic!("{name} menu");
            };
            let layout = menu.layout();
            for (row, visual_row) in layout.rows.iter().enumerate() {
                if let crate::context_menu_layout::MenuRow::Action(index) = visual_row {
                    assert_eq!(layout.action_at(row as u16), Some(*index), "{name}");
                }
            }
            let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
            terminal
                .draw(|frame| {
                    draw_context_menu(frame, menu, app.ui_settings.tree_order, &app.ui_settings)
                })
                .unwrap();
            save_frame(&format!("tree-target-{name}"), &terminal);
        }
    }

    #[test]
    fn context_menu_visual_tree_groups_padding_and_short_screen_reachability() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("menu visual fixture".into(), project.path().to_path_buf());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        for (width, height) in [(100, 40), (80, 24), (40, 10)] {
            app.set_screen_area(Rect::new(0, 0, width, height));
            app.open_context_menu(pane, 2, 2);
            let Mode::ContextMenu(mut menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
                panic!("menu");
            };
            for (name, selected) in [("first", 0), ("last", menu.actions.len() - 1)] {
                menu.selected_index = selected;
                let layout = menu.layout();
                let row = layout
                    .row_for_action(selected)
                    .expect("selected action is visible");
                assert_eq!(layout.action_at(row), Some(selected));
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| {
                        draw_context_menu(
                            frame,
                            &menu,
                            app.ui_settings.tree_order,
                            &app.ui_settings,
                        )
                    })
                    .unwrap();
                let cell = &terminal.backend().buffer()[(menu.area.x + 1, menu.area.y + 1 + row)];
                assert_eq!(Some(cell.bg), theme::selected_style().bg);
                assert_eq!(cell.symbol(), " ", "selected rows retain left padding");
                save_frame(&format!("tree-{width}x{height}-{name}"), &terminal);
            }
        }
        app.set_screen_area(Rect::new(0, 0, 100, 40));
        app.open_context_menu(ROOT_ID, 2, 2);
        let Mode::ContextMenu(mut menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("menu");
        };
        let order = menu
            .actions
            .iter()
            .position(|action| *action == crate::app::ContextMenuAction::OrderBy)
            .unwrap();
        menu.selected_index = order;
        app.open_context_submenu(&mut menu, crate::app::ContextMenuAction::OrderBy);
        let submenu = menu.submenu.as_ref().unwrap();
        assert_eq!(
            submenu.area.y,
            menu.area.y + 1 + menu.layout().row_for_action(order).unwrap()
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        terminal
            .draw(|frame| {
                draw_context_menu(frame, &menu, app.ui_settings.tree_order, &app.ui_settings)
            })
            .unwrap();
        save_frame("tree-order-submenu", &terminal);
    }

    #[test]
    fn context_menu_visual_terminal_uses_the_same_groups_and_selection() {
        let mut actions = vec![
            TerminalContextAction::ShowAgentDebugLog,
            TerminalContextAction::ToggleAgentToolbar {
                currently_visible: true,
            },
            TerminalContextAction::CopySelectionToClipboard,
            TerminalContextAction::CopyLastSubmittedPromptToClipboard {
                prompt: "synthetic prompt".into(),
            },
            TerminalContextAction::CopyLineToClipboard,
            TerminalContextAction::CopyVisibleTerminalToClipboard,
            TerminalContextAction::CopyFullTerminalHistoryToClipboard,
            TerminalContextAction::CopyHistoryFilePathToClipboard {
                path: "/synthetic/history.jsonl".into(),
            },
            TerminalContextAction::PasteClipboard,
        ];
        actions.sort_by_key(TerminalContextAction::menu_order);
        let mut menu = TerminalPaneContextMenu {
            pane_id: ROOT_ID,
            source_line_text: "synthetic line".into(),
            visible_contents: "synthetic screen".into(),
            full_history: "synthetic history".into(),
            selection_text: Some("synthetic selection".into()),
            area: Rect::new(2, 2, 44, 17),
            actions,
            selected_index: 0,
            row_offset: 0,
            preparation_generation: 0,
            _preparation_hold: None,
        };
        assert_eq!(
            menu.actions.first(),
            Some(&TerminalContextAction::CopySelectionToClipboard)
        );
        assert_eq!(
            menu.actions.last(),
            Some(&TerminalContextAction::ShowAgentDebugLog)
        );
        for icons in [true, false] {
            let mut ui = crate::config::UiSettings::default();
            ui.show_context_menu_icons = icons;
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|frame| draw_terminal_pane_context_menu(frame, &menu, &ui))
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(3, 3)].symbol(), " ", "top breathing room");
            let separators = menu
                .layout()
                .rows
                .iter()
                .filter(|row| matches!(row, crate::context_menu_layout::MenuRow::Separator))
                .count();
            assert_eq!(separators, 3);
            save_frame(
                if icons {
                    "terminal-icons"
                } else {
                    "terminal-text"
                },
                &terminal,
            );
        }
        menu.area = Rect::new(0, 0, 40, 10);
        menu.selected_index = menu.actions.len() - 1;
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal
            .draw(|frame| {
                draw_terminal_pane_context_menu(frame, &menu, &crate::config::UiSettings::default())
            })
            .unwrap();
        save_frame("terminal-short-last", &terminal);

        let directory = tempfile::tempdir().unwrap();
        for (width, height, menu_area, icon_preference) in [
            (80, 24, Rect::new(2, 2, 44, 17), true),
            (80, 24, Rect::new(2, 2, 44, 17), false),
            (40, 10, Rect::new(0, 0, 40, 10), true),
        ] {
            let mut app = App::new("composed terminal menu".into(), directory.path().into());
            app.set_screen_area(Rect::new(0, 0, width, height));
            app.ui_settings.show_context_menu_icons = icon_preference;
            app.mode = Mode::TerminalPaneContextMenu(TerminalPaneContextMenu {
                pane_id: ROOT_ID,
                source_line_text: menu.source_line_text.clone(),
                visible_contents: menu.visible_contents.clone(),
                full_history: menu.full_history.clone(),
                selection_text: menu.selection_text.clone(),
                area: menu_area,
                actions: menu.actions.clone(),
                selected_index: 0,
                row_offset: 0,
                preparation_generation: 0,
                _preparation_hold: None,
            });
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            assert_eq!(
                buffer[(menu_area.x, menu_area.y)].symbol(),
                "╭",
                "the composed menu keeps its rounded frame at {width}x{height}"
            );
            let rendered = buffer
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(
                rendered.contains("Terminal actions"),
                "the composed menu title is visible at {width}x{height}"
            );
            assert!(
                rendered.contains("Copy selection"),
                "the composed menu action is visible at {width}x{height}"
            );
            save_frame(
                &format!("composed-terminal-menu-{width}x{height}-icons-{icon_preference}"),
                &terminal,
            );
        }
    }
}

#[cfg(test)]
mod prompt_queue_visual_tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn queued_view_preserves_large_middle_cursor_and_grapheme_boundaries() {
        let draft = "界e\u{301}\n".repeat(70_000) + "TAIL";
        let mut text = crate::text_prompt::TextPromptState::new(draft.clone());
        text.cursor = 140_001;
        let cursor = text.cursor;
        let (view, local_cursor) = queued_prompt_view(&text, Rect::new(0, 0, 70, 6));
        let prefix: String = text.buf.chars().take(cursor).collect();
        assert!(prefix.ends_with(&view[..local_cursor]));
        let start = prefix.len() - local_cursor;
        assert!(text
            .buf
            .grapheme_indices(true)
            .any(|(index, _)| index == start));
        assert!(view.len() < text.buf.len());
        let mut textarea =
            ratatui_textarea::TextArea::from(view[local_cursor..].split('\n').map(str::to_owned));
        textarea.insert_str(&view[..local_cursor]);
        assert_eq!(textarea.lines().join("\n"), view);
        assert_eq!(text.buf, draft);
        assert_eq!(text.cursor, cursor);
    }

    #[test]
    fn queue_prompt_long_draft_keeps_edited_tail_visible() {
        for (width, height) in [(120, 40), (80, 24), (60, 20), (40, 12)] {
            let app = App::new(
                "synthetic-queue-visibility".to_owned(),
                std::path::PathBuf::from("/synthetic/ui-queue"),
            );
            let mut state = crate::prompt_queue::PromptQueueDialogState::new(ilium_core::ROOT_ID);
            let mut draft = (0..60)
                .map(|index| format!("Synthetic line {index:02}\n"))
                .collect::<String>();
            draft.push_str("EDITED-TAIL");
            state.text = crate::text_prompt::TextPromptState::new(draft);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| draw_prompt_queue_dialog(frame, frame.area(), &app, &state))
                .unwrap();
            crate::ui_capture::save(&format!("queue-long-tail-{width}x{height}"), &terminal);
            let layout = crate::prompt_queue::dialog_layout(Rect::new(0, 0, width, height));
            let inner = theme::block(true).inner(layout.text);
            assert!(
                inner.width >= 11 && inner.height > 0,
                "fixture must offer room for edited line at {width}x{height}"
            );
            let visible = (inner.y..inner.bottom())
                .map(|y| {
                    (inner.x..inner.right())
                        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                visible.contains("EDITED-TAIL"),
                "long draft hides current edit at {width}x{height}: {visible:?}"
            );
        }
    }
}

#[cfg(test)]
mod editor_polish_capture_tests {
    use super::*;
    #[test]
    fn export_populated_editor_visual_matrix() {
        use crate::app::{FocusTarget, PaneRuntime, RightPanelTarget};
        use ilium_core::{PaneContentKind, ROOT_ID};
        use ratatui::{backend::TestBackend, Terminal};
        for (width, height) in [(120, 40), (80, 24), (60, 20), (40, 12)] {
            for tail in [false, true] {
                let mut app = App::new(
                    "synthetic-editor-capture".to_owned(),
                    std::path::PathBuf::from("/synthetic/project"),
                );
                let group = app.tree.add_group(ROOT_ID, "Synthetic workspace").unwrap();
                let pane = app
                    .tree
                    .add_pane(group, "notes.rs", PaneContentKind::Editor)
                    .unwrap();
                let mut editor = EditorPane::empty();
                let text = (0..80).map(|index| format!("let synthetic_value_{index} = \"Readable source with Unicode 界 and a long line for wrap inspection\";" )).collect::<Vec<_>>().join("\n");
                editor.replace_contents(&text);
                if tail {
                    editor
                        .textarea
                        .move_cursor(ratatui_textarea::CursorMove::Bottom);
                }
                app.panes
                    .insert(pane, PaneRuntime::Editor(Box::new(editor)));
                app.right_panel_target = RightPanelTarget::Pane { pane_id: pane };
                app.focus = FocusTarget::Pane;
                app.set_screen_area(Rect::new(0, 0, width, height));
                // Existing fixture installs the real source window and commits geometry after flush ACK.
                app.prepare_test_editor_source_frame(pane);
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|frame| draw(frame, &mut app)).unwrap();
                crate::ui_capture::save(
                    &format!(
                        "editor-populated-{}-{width}x{height}",
                        if tail { "tail" } else { "top" }
                    ),
                    &terminal,
                );
            }
        }
    }
}
