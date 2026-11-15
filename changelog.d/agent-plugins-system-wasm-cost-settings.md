### Changed
- The Agent Cost settings page now belongs to the Cost plugin. It appears in the Settings list while the plugin is enabled and edits the same `[cost]` keys of `config.toml` (measure, rating, bands, budgets, history window, indicators, sparkline, sort and spending medals). Disabling the plugin removes the page. The spending-medal glyphs are still chosen in Settings > Icons.
- Plugin settings rows can offer presets that are lists of numbers, such as four cut points, as a choice row.
- Disabling the Cost plugin now also stops spend tracking: the tree shows no cost indicators, medals or totals and no longer sorts by cost until the plugin is enabled again.
- "Show debug log" on an agent pane is offered only while the Diagnostics plugin, which draws the viewer, is enabled.
