//! Contributed onboarding steps (`contributes.onboarding_steps`) in the first-run flow.
//!
//! When the built-in wizard finishes (Continue on its last step, or Skip there), the steps
//! enabled plugins contribute are walked in `order`, lowest first; equal orders keep the
//! catalogue order. Each step opens as a plugin view in the right-hand area, through the same
//! path as the plugin list's "open" rows. The next step opens once the plugin closes the one
//! shown (`close-view` of its onboarding step), so a plugin decides when its step is done.

use std::collections::VecDeque;

use crate::app::App;

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

    /// Opens the next queued onboarding step, if any is left.
    pub(crate) fn open_next_plugin_onboarding_step(&mut self) {
        if let Some((plugin, step)) = self.plugin_onboarding_queue.pop_front() {
            self.show_plugin_view(&plugin, PluginViewKind::OnboardingStep, &step);
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
