### Fixed

- `ilium resource ...` run inside an Ilium pane no longer ignores an explicit `--cwd`: the pane's own server is used only when no `--cwd`, `--project` or `--session` is given, so test and capture commands can no longer create or change resources in the live session.
