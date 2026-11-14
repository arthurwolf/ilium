### Changed
- Text triggers now run as the core plugin `ilium.core.text-triggers`: the server scans a pane's screen, tracks match instances and sends each reply after its rule's delay, and the plugin shows the rules panel and a rule editor with a live preview. The rule list is still stored in the `[text_triggers]` table of `config.toml`; disabling the plugin removes the feature.

### Removed
- The built-in Text Triggers settings tab, its dialog and the in-kernel text-trigger matcher were removed in favour of the plugin.
