---
layout: page
title: Requirements for pane primitives
permalink: /pages/reference-requirements-primitives/
---

## Requirements for pane primitives

Normative behaviour of the terminal, agent, editor and basic pane primitives.

The rows follow the format and the observation channels of the [requirements index](/pages/reference-requirements/). The id prefix is `PRI`. The id PRI-009 is intentionally unused: it was retired and ids are never reused, so the numbering runs from PRI-008 to PRI-010.

| Id | Requirement | Source | Observable test |
|---|---|---|---|
| [PRI-001] | A screen read MUST need terminal.read-screen. read-cells is a separate grant, terminal.read-cells, and MUST accept at most 1,000 rows per call and return quota-exceeded above that. | [edge cases and errors](/pages/primitives-terminal/#edge-cases-and-errors) | (sync-return) read-cells with 1,001 rows -> quota-exceeded. |
| [PRI-002] | history-text over 4 MiB MUST keep the newest 4 MiB. | [requestable limits](/pages/reference-errors-and-limits/#requestable-limits) | (sync-return) read history of 5 MiB -> 4 MiB, the newest. |
| [PRI-003] | send-input MUST accept at most 16 stages, a total stage delay of 10 s and 64 KiB of raw bytes. Above any of them it MUST refuse with invalid-argument. | [primitives terminal input](/pages/primitives-terminal-input/) | (completion) 17 stages -> refused(invalid-argument). |
| [PRI-004] | Check order for send-input MUST be: capability, stage and size rules, if-current precondition, then the lock. A failed precondition MUST refuse the intent before any lock is taken. | [capability terminal input lock](/pages/capability-terminal-input-lock/) | (completion) stale if-current on a locked pane -> refused(conflict) and the lock unchanged. |
| [PRI-005] | An input lock MUST last at most 2 s. There MUST be one lock per pane. A second lock MUST return conflict. | [requestable limits](/pages/reference-errors-and-limits/#requestable-limits) | (sync-return) lock a locked pane -> conflict. |
| [PRI-006] | A terminal output filter MUST accept at most rate_per_second matches per filter instance and pane. A match above the limit MUST be dropped and counted, not delivered. | [capability terminal output filter](/pages/capability-terminal-output-filter/) | (event-delivery) 500 matches in a second at rate 10 -> 10 delivered, the next batch starts with events-dropped. |
| [PRI-007] | A pointer claim MUST belong to one plugin at a time. A whole-pane claim belongs to one pane, so a second whole claim on the same pane MUST return conflict. | [primitives terminal pointer](/pages/primitives-terminal-pointer/) | (sync-return) two plugins claim one pane -> the second gets conflict. |
| [PRI-008] | screen-changed MUST be delivered at most 10 times per second per pane. | [requestable limits](/pages/reference-errors-and-limits/#requestable-limits) | (event-delivery) change a screen 100 times a second -> at most 10 events. |
| [PRI-010] | register-signature MUST return invalid-argument for a rule with a bad id, process names, aliases, icon, action count or key size, and conflict for an id the manifest already contributes. | [errors and edge cases](/pages/primitives-agent/#errors-and-edge-cases) | (sync-return) register a signature with an empty id -> invalid-argument. |
| [PRI-011] | A signature of the user MUST win over a plugin signature, which MUST win over a built-in one. An overlap MUST be logged as a warning. | [errors and edge cases](/pages/primitives-agent/#errors-and-edge-cases) | (log-line) two signatures match one process -> one warning, the user rule applies. |
| [PRI-012] | set-toolbar-state MUST return not-found for a button that is not the plugin own or a pane that does not exist, and not-permitted without the grant. | [errors and edge cases](/pages/primitives-agent/#errors-and-edge-cases) | (sync-return) set state for another plugin button -> not-found. |
| [PRI-013] | agents() MUST return an empty list when agent.read is declined. view, session and last_prompt MUST return none. | [errors and edge cases](/pages/primitives-agent/#errors-and-edge-cases) | (sync-return) call agents() without the grant -> empty list. |
| [PRI-014] | A transcript per call MUST hold at most 512 turns or 4 MiB. The oldest turns MUST be dropped. | [requestable limits](/pages/reference-errors-and-limits/#requestable-limits) | (sync-return) read a 600 turn transcript -> 512 turns, newest. |
| [PRI-015] | A pane MUST have no activity before its agent-identified event. A restored pane MUST report none. | [errors and edge cases](/pages/primitives-agent/#errors-and-edge-cases) | (sync-return) read activity of a restored pane -> none. |
| [PRI-020] | read-lines MUST accept at most 10,000 lines per call. Over that MUST return invalid-argument. | [limits](/pages/primitives-editor/#limits) | (sync-return) read 10,001 lines -> invalid-argument. |
| [PRI-021] | A plugin MUST hold at most 64 buffers. The 65th create-buffer MUST return quota-exceeded. create-buffer text MUST be at most 16 MiB. | [limits](/pages/primitives-editor/#limits) | (sync-return) create a 65th buffer -> quota-exceeded. |
| [PRI-022] | editor-apply-edits MUST accept at most 10,000 edits and refuse more with refused(invalid-argument). | [limits](/pages/primitives-editor/#limits) | (completion) 10,001 edits -> refused(invalid-argument) and the buffer unchanged. |
| [PRI-023] | An annotator MUST be called at most once per 100 ms per buffer. Extra calls MUST NOT be made. | [limits](/pages/primitives-editor/#limits) | (event-delivery) edit 20 times in 100 ms -> one annotator call. |
| [PRI-024] | Decorations or gutter marks MUST be at most 10,000 per layer; the session total MUST be at most 65,536. | [limits](/pages/primitives-editor/#limits) | (sync-return) 10,001 marks in a layer -> invalid-argument. |
| [PRI-030] | A basic pane MUST draw its body from views.render and MUST receive on-events like any plugin side. | [body and lifecycle](/pages/primitives-basic/#body-and-lifecycle) | (screen-text) open a basic pane -> the rendered body appears. |
| [PRI-031] | set-dirty with true MUST mark the pane as holding unsaved work, and closing it MUST ask before discarding. | [pane activity and dirty state](/pages/primitives-basic/#pane-activity-and-dirty-state) | (screen-text) set-dirty true, then close the pane -> a confirmation appears. |

### See also

- [Requirements index](/pages/reference-requirements/)
- [Error codes and limits](/pages/reference-errors-and-limits/)
