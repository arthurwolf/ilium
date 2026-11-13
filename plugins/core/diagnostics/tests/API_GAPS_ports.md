# API gaps of the diagnostics port tests

Assumed, on top of API_GAPS.md item 1 (service `ilium.core.diagnostics/agent-debug`):

- `log_request` { request, session?, name?, ok, error? } -> { logged }: the server writes a request line
  `request_name="<request>"`, plus `request failed` and the error text when `ok` is false. `enable` also writes the
  start lines `ilium-server starting` and `detached session server listening`.
- `read_log` {} -> { text }: the text of the server diagnostic file.
- `file_mode` { kind: "log-directory" | "log-file" } -> { mode } (0o700 and 0o600), next to the path form of the feature tests.
- `append` accepts `resize_cause`; `entries` returns it, `is_tree_panel_animation_resize`, and a field {label "cause", value}.
- `view` and `save_log` accept `hide_tree_animation`; `view` returns `visible` and `hidden`; the saved report carries the
  `Filter: left-panel focus/hover animation resize events excluded|included` line.
- Not expressible: the real process boundary (detached server binary, socket, PTY-driven TUI clicks and scrolling). The
  twins check the observable results through the service only.
