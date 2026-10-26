# Requirements for rendering

## Requirements for rendering

Normative behaviour of frames, the frame budget, scenes, surfaces and the shared look.

The rows follow the format and the observation channels of the [requirements index](/pages/reference-requirements/). The id prefix is `RND`.

| Id | Requirement | Source | Observable test |
|---|---|---|---|
| [RND-001] | The frame budget MUST be 1,000 ms divided by the effective frame rate, rounded down: 33 ms at 30 fps. frame_deadline_ms MAY raise it to at most 50 ms. | [api exports](/pages/api-exports/) | (trap) render 40 ms at 30 fps -> the frame is skipped. |
| [RND-002] | A frame that passes the budget MUST be skipped. Thirty breaches in a row MUST trap. | [edge cases and errors](/pages/api-scenes/#edge-cases-and-errors) | (trap) 30 slow frames in a row -> trap on the 30th. |
| [RND-003] | A render call that returns without present MUST drop that frame, and a warn line MUST be written once per second. | [surface rules](/pages/api-scenes/#surface-rules) | (log-line) return without present for 3 s -> three warn lines. |
| [RND-004] | write_rows MUST accept whole rows only: width bytes in gray8, 3 x width in rgb8, 4 x width in rgba8. A remainder MUST be dropped and a warn line written once per second. | [surface rules](/pages/api-scenes/#surface-rules) | (log-line) write width + 1 bytes in gray8 -> the extra byte is dropped. |
| [RND-005] | Rows at or past height MUST be ignored. | [surface rules](/pages/api-scenes/#surface-rules) | (screen-text) write row height + 1 -> no change on screen. |
| [RND-006] | write_cell_colors MUST be ignored unless uses-cell-colors is true. occupancy() MUST return none unless wants-occupancy is true. pointer() MUST return none outside the scene area or when wants-pointer is false. | [surface rules](/pages/api-scenes/#surface-rules) | (sync-return) call occupancy() with wants-occupancy false -> none. |
| [RND-007] | readiness that returns Failed, or stays Preparing for more than 10 s, MUST count as one failure. The host MUST stop calling render until the restart. | [edge cases and errors](/pages/api-scenes/#edge-cases-and-errors) | (restart-schedule) stay Preparing for 11 s -> one failure, restart after 1 s. |
| [RND-008] | reconfigure returning false MUST restart the renderer with the new spec and MUST NOT count as a failure. | [edge cases and errors](/pages/api-scenes/#edge-cases-and-errors) | (restart-schedule) reconfigure returns false -> new renderer, failure count unchanged. |
| [RND-009] | Settings that fail settings_schema MUST be refused before the scene sees them and the previous values MUST stay. | [edge cases and errors](/pages/api-scenes/#edge-cases-and-errors) | (event-delivery) change a value to an invalid one -> the scene sees no change. |
| [RND-010] | Revoking a grant MUST restart the renderer with the reduced grants and MUST NOT count as a failure. | [edge cases and errors](/pages/api-scenes/#edge-cases-and-errors) | (restart-schedule) revoke http -> a new renderer, failure count unchanged. |
| [RND-011] | A scene MUST follow the shared look: the host applies colour, palette, brightness, contrast, dithering and display controls per cell, and a scene MUST NOT add its own. | [renderer surface](/pages/api-scenes/#renderer-surface) | (screen-text) change the palette in Settings -> the scene colours follow with no plugin code. |
| [RND-012] | A scene MUST receive the current palette as palette_rgb (empty in greyscale and monotone modes) and MUST get set-palette when it changes. | [renderer surface](/pages/api-scenes/#renderer-surface) | (event-delivery) change the palette -> set-palette is called with the new 0xRRGGBB values. |
| [RND-013] | A scene failure MUST count as a failure of the whole plugin on that side. | [edge cases and errors](/pages/api-scenes/#edge-cases-and-errors) | (restart-schedule) trap in a scene frame -> the client side restarts after 1 s. |
| [RND-014] | A scene memory limit of 32 MiB MUST be the default, with ceilings 256 MiB (T1) and 128 MiB (other tiers). A breach MUST trap. | [edge cases and errors](/pages/api-scenes/#edge-cases-and-errors) | (trap) allocate 64 MiB at the default -> trap. |
| [RND-015] | views.render MUST finish within its deadline (10 ms by default, ceiling 20 ms). A breach MUST trap with ui.render-timeout, and the previous tree MUST stay on screen marked stale. | [traps](/pages/reference-errors-and-limits/#traps) | (trap) render sleeps 200 ms -> trap ui.render-timeout. |
| [RND-016] | write-glyphs MUST accept at most 131,072 cells per frame. Over that MUST return quota-exceeded. | [requestable limits](/pages/reference-errors-and-limits/#requestable-limits) | (sync-return) 131,073 cells -> quota-exceeded. |
| [RND-017] | A pre-rendered scene loop MUST be at most 30 fps, 3,600 frames and 128 MiB. Over that the install MUST fail with manifest.scene-prerender. | [requestable limits](/pages/reference-errors-and-limits/#requestable-limits) | (install-error) a 3,601 frame loop -> manifest.scene-prerender. |
| [RND-018] | Decoration frames MUST be at most 16, each shown for at least 90 ms. Otherwise invalid-argument. | [requestable limits](/pages/reference-errors-and-limits/#requestable-limits) | (sync-return) 17 frames -> invalid-argument. |
| [RND-019] | Plugin text MUST be sanitised before drawing: control characters and escape sequences are removed, TAB expands to the next multiple of 4 cells, and single-line fields turn CR, LF and CRLF into one space. The host MUST log at most one ui.text-sanitized warning per view. | [text sanitising](/pages/ui-model/#text-sanitising) | (screen-text) a label with ESC [31m -> shown without the sequence, one warning. |
| [RND-020] | The depth limit of a ui-tree MUST be 32 levels with the root as level 1. | [limits](/pages/reference-host-rendering/) | (trap) 33 levels -> ui.too-deep. |

### See also

- [Requirements index](/pages/reference-requirements/)
- [Error codes and limits](/pages/reference-errors-and-limits/)
