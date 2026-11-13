# Codex shutdown resize integration

`shutdown-resize-v0.167.0.patch` applies to the upstream Codex `rust-v0.167.0` source. It adds a regression test and obtains one terminal size for both layout and the shutdown draw. The unmodified executable can panic when the terminal shrinks immediately before shutdown, because the layout can use a stale size while the drawing buffer uses the new size.

Build the patched release through the configured whole-job `ni-build` service, then exercise the resulting executable with the shutdown resize regression before installing it. The upstream source includes a `codex-rs/vendor/bubblewrap/LICENSE` symlink; package its same-tree `COPYING` contents as a regular file when using a dispatcher that rejects symlink inputs. An upstream package update can replace the installed executable. Rebase and reverify the patch for a new Codex release; do not assume it applies unchanged.

This fix covers one confirmed Codex shutdown path. Ilium also keeps enabled prompt and progress slots reserved during ordinary content changes to reduce resize exposure, but genuine window, layout, and setting changes still resize terminals.
