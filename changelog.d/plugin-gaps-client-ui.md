### Added
- Plugin popups show text inputs and number steppers. Typing, pasting, clicking to place the caret and the stepper arrows all work. A pending input change always reaches the plugin before the button that acts on it.
- A paste into a shown plugin panel goes to its focused text input as one change. Debounced panel and popup edits are sent on time even when no key is pressed.
- Plugins can recognise their own links in terminal output. Ctrl+O on such a link runs the plugin's link command; links no plugin claims still open as before.
- Terminal context-menu entries contributed by plugins appear in the terminal right-click menu.
- Text, path, colour and number settings contributed by plugins can be typed into a prompt from Settings, Plugins. Left and Right step a setting backwards and forwards. The plugin is told when the user changes one of its settings.
