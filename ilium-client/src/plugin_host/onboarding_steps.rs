//! Contributed onboarding steps (`contributes.onboarding_steps`) in the first-run flow.
//!
//! When enabled plugins contribute steps, the built-in wizard gains a last step,
//! [`Step::Plugin`](crate::onboarding::state::Step::Plugin), after Voice. Entering it walks the
//! contributed steps in `order`, lowest first; equal orders keep the catalogue order. Each step
//! opens as a plugin view through the same path as the plugin list's "open" rows, and the
//! wizard draws it in its own content frame, with the wizard's Back, Skip and Finish around
//! it. The next step opens once the plugin closes the one shown (`close-view` of its
//! onboarding step), so a plugin decides when its step is done; when the last one closes, the
//! wizard finishes. Finish or Skip on the plugin step finishes at once.

use std::collections::VecDeque;

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use ilium_plugin_api::generated::types::ui::ViewTarget;

use crate::app::{App, RightPanelTarget};
use crate::onboarding::state::Step;

use super::host::ChromeItem;
use super::views::PluginViewKind;

/// `(plugin, step id)` of `steps`, sorted by `order`; the sort is stable.
pub(crate) fn walk_order(mut steps: Vec<ChromeItem>) -> VecDeque<(String, String)> {
    steps.sort_by_key(|step| step.order);
    steps
        .into_iter()
        .map(|step| (step.plugin, step.id))
        .collect()
}

impl App {
    /// The built-in first-run wizard finished: queue every contributed onboarding step and
    /// open the first one.
    pub(crate) fn begin_plugin_onboarding_steps(&mut self) {
        let steps = self.wasm_plugins.as_ref().map_or_else(Vec::new, |plugins| {
            plugins.contributed(PluginViewKind::OnboardingStep.contribution())
        });
        self.plugin_onboarding_queue = walk_order(steps);
        self.open_next_plugin_onboarding_step();
    }

    /// Opens the next queued onboarding step. With none left while the wizard shows its
    /// plugin step, the wizard finishes.
    pub(crate) fn open_next_plugin_onboarding_step(&mut self) {
        if let Some((plugin, step)) = self.plugin_onboarding_queue.pop_front() {
            self.show_plugin_view(&plugin, PluginViewKind::OnboardingStep, &step);
        } else {
            crate::onboarding::integration::finish_after_plugin_steps(self);
        }
    }

    /// Whether enabled plugins contribute onboarding steps, so the wizard has a plugin step.
    pub(crate) fn has_plugin_onboarding_steps(&self) -> bool {
        self.wasm_plugins.as_ref().is_some_and(|plugins| {
            !plugins
                .contributed(PluginViewKind::OnboardingStep.contribution())
                .is_empty()
        })
    }

    /// The wizard leaves its plugin step (Back, Finish or Skip): the steps not yet shown are
    /// dropped and the shown one leaves the screen (the host then closes it).
    pub(crate) fn end_plugin_onboarding_steps(&mut self) {
        self.plugin_onboarding_queue.clear();
        if self
            .shown_plugin_view()
            .is_some_and(|(_, view)| matches!(view, ViewTarget::OnboardingStep(_)))
        {
            self.right_panel_target = RightPanelTarget::Empty;
        }
    }

    /// The cells the wizard gives a contributed step while it shows its plugin step; `None`
    /// otherwise, when an onboarding step view is drawn in the right-hand area as before.
    pub(crate) fn plugin_onboarding_step_area(&self) -> Option<Rect> {
        (self.onboarding.is_some() && self.onboarding_progress.wizard.step == Step::Plugin)
            .then(|| crate::onboarding::screen::Geometry::new(self.layout.screen_area).content)
    }
}

/// Draws the contributed step the wizard shows in `area`, its content frame, and tells the
/// host how many cells it has. Before the plugin has drawn, or with no step shown, a line
/// says what is happening.
pub(crate) fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let shown = app
        .shown_plugin_view()
        .filter(|(_, view)| matches!(view, ViewTarget::OnboardingStep(_)));
    let tree = shown.as_ref().and_then(|(plugin, view)| {
        let plugins = app.wasm_plugins.as_ref()?;
        plugins.set_view_area(plugin, view, (area.width, area.height));
        plugins.view_tree(plugin, view)
    });
    match tree {
        Some(tree) => {
            let scene = super::view::compose_with(&tree, area, &app.plugin_view_cursor.states);
            super::view::draw(frame, &scene, &app.plugin_view_cursor, true);
        }
        None => {
            let text = if shown.is_some() {
                "…"
            } else {
                "Your plugins have nothing more to set up. Finish to start using Ilium."
            };
            frame.render_widget(Paragraph::new(Line::from(text)), area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(plugin: &str, id: &str, order: i64) -> ChromeItem {
        ChromeItem {
            plugin: plugin.to_owned(),
            id: id.to_owned(),
            title: id.to_owned(),
            icon: None,
            command: None,
            target: None,
            order,
            popup_view: None,
            slot: None,
            placement: None,
            when: None,
            caption: None,
            dynamic: false,
            menu_item: None,
        }
    }

    #[test]
    fn steps_walk_by_order_and_keep_the_catalogue_order_on_ties() {
        let walk = walk_order(vec![
            step("com.example.b", "late", 20),
            step("com.example.a", "first", 1),
            step("com.example.b", "tie-one", 10),
            step("com.example.a", "tie-two", 10),
        ]);
        let ids: Vec<&str> = walk.iter().map(|(_, id)| id.as_str()).collect();
        assert_eq!(ids, ["first", "tie-one", "tie-two", "late"]);
        assert_eq!(walk[0].0, "com.example.a");
    }
}
