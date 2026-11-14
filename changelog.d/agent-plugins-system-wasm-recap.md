### Changed
- `ilium recap` is now provided by the `ilium.core.recap` WebAssembly plugin: the plugin reads the evidence of each pane in a `recap-pane` job, calls the configured model once for both lengths, validates pointers and writes the same JSONL or Markdown records. It also answers the `ilium.core.recap/recap` service and the `onboarding-state` client request, and honours `--timeout-seconds`, `--max-concurrency` and `--max-panes`.

### Removed
- The built-in `ilium recap` command and its isolated end-to-end test. Disabling the Recap plugin removes the command.
