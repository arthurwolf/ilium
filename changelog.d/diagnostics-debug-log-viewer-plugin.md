### Changed
- The agent debug log viewer (the popup behind "Show debug log" in an agent pane's menu) now lives in the Diagnostics plugin. The client only asks the server for the retained history and hands it to `ilium.core.diagnostics/show-debug-log`; scrolling, the panel-resize filter and the report text are drawn by the plugin. Disabling Diagnostics removes the viewer.

### Removed
- The viewer's "save to a file" path prompt: `c` now copies the same report to the clipboard.
