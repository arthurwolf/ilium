### Changed
- Smart Copy now claims terminal selection and Smart Copy light gestures per pane, reads its settings through the kernel settings interface, and offers Copy, Copy path, Smart Copy and Transfer screen in the terminal menu, all from inside the plugin.
- Smart Copy holds a terminal snapshot while its picker is open instead of freezing the agent process.
- The Smart Copy after-copy hook chain can replace the copied text and the plugin publishes a copied topic.

### Fixed
- Plugin pane reads now respect the history, cell-count and row limits instead of failing on large requests.
