### Added
- Settings pages that plugins contribute appear in the Settings navigation rail after the built-in tabs; Tab, Shift+Tab and clicks reach them, and each lists that page's rows.
- Onboarding steps that plugins contribute open one after another, lowest `order` first, once the first-run wizard finishes.
- Client plugins can play sounds: the bundled `chirping` sound plays through the client's audio output, at most 4 per second per plugin, and stays silent when sounds are muted. Each plugin's sound catalogue lists its own `contributes.sounds` ids.
- Plugin pane frame slots with the `banner` and `footer-row` placements draw a row above or below a terminal pane, and the pane's terminal shrinks to make room.
- Plugin tree toolbar buttons show their `caption` beside the icon when the sidebar has room for every caption, and hide outside the context their `when` expression names; context menu entries honour `when` too.

### Changed
- The tree ordering chosen from a plugin is saved in `config.toml` (`plugin_tree_ordering`) and restored on the next start; choosing a built-in ordering clears it.
