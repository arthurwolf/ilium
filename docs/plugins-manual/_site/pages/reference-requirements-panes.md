# Requirements for panes

## Requirements for panes

Normative behaviour of pane states, restored panes, lifecycle intents, restructuring and pane creation.

The rows follow the format and the observation channels of the [requirements index](/pages/reference-requirements/). The id prefix is `PAN`.

| Id | Requirement | Source | Observable test |
|---|---|---|---|
| [PAN-001] | A pane process MUST be in exactly one of running, frozen, restarting, exited. Every change MUST be reported by pane-lifecycle-changed with pane, state and by. | [states of a pane process](/pages/api-pane-lifecycle/#states-of-a-pane-process) | (event-delivery) freeze, unfreeze -> two events with states frozen and running. |
| [PAN-002] | freeze-pane MUST move running to frozen. unfreeze-pane MUST move frozen to running. unfreeze-pane on a pane that is not frozen MUST be refused(conflict). | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (completion) unfreeze a running pane -> refused(conflict). |
| [PAN-003] | freeze-pane on Windows MUST be refused(unsupported). | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (completion) freeze on Windows -> refused(unsupported). |
| [PAN-004] | replace-pane-command MUST put the pane in restarting, keep node id, title, place and size, and end in running once the new process started. A cwd of none MUST keep the pane directory. | [replace the command terminate the process](/pages/api-pane-lifecycle/#replace-the-command-terminate-the-process) | (event-delivery) replace the command -> restarting then running, same node id. |
| [PAN-005] | terminate-pane-process MUST ask the process to end, wait grace-ms, then kill. The outcome MUST be ok when the process has exited. The pane MUST stay open and show the exit status. | [replace the command terminate the process](/pages/api-pane-lifecycle/#replace-the-command-terminate-the-process) | (completion) terminate with grace 3,000 -> ok and pane-exited; the node stays. |
| [PAN-006] | terminate-pane-process with grace-ms above 10,000 MUST be refused(invalid-argument). | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (completion) grace-ms 10,001 -> refused(invalid-argument). |
| [PAN-007] | A pane that exited MUST move to restarting with replace-pane-command. | [states of a pane process](/pages/api-pane-lifecycle/#states-of-a-pane-process) | (event-delivery) replace the command of an exited pane -> restarting then running. |
| [PAN-008] | For one node the event order MUST be pane-created, other events of that pane, pane-exited, then node-removed. A plugin MUST NOT see tree-changed for the node after node-removed. | [pane events](/pages/api-panes/#pane-events) | (event-delivery) close a running pane -> pane-exited, node-removed, nothing after. |
| [PAN-010] | A restored pane MUST exist in the tree with its saved title and size before its process starts. It MUST NEVER report a size of zero. | [pane states after a restore](/pages/api-panes/#pane-states-after-a-restore) | (sync-return) read a restored pane -> is-restored true, size from the snapshot. |
| [PAN-011] | An intent that needs a running process (send-input, signal-pane) sent to a restored pane MUST be refused(pane-not-ready). signal-pane to an exited pane MUST be refused(not-found). | [edge cases and errors](/pages/api-panes/#edge-cases-and-errors) | (completion) send-input to a restored pane -> refused(pane-not-ready). |
| [PAN-012] | is-restored MUST be false after the process starts. | [pane states after a restore](/pages/api-panes/#pane-states-after-a-restore) | (sync-return) read after pane-created and tree-changed -> is-restored false. |
| [PAN-020] | resize-pane MUST be refused(conflict) when the layout cannot give the size, and the message MUST name the neighbour. | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (completion) resize beyond the neighbour minimum -> refused(conflict). |
| [PAN-021] | restructure MUST accept at most 256 operations. More MUST be refused(invalid-argument). | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (completion) 257 operations -> refused(invalid-argument). |
| [PAN-022] | restructure with a failing operation MUST be refused(conflict) with the operation index in the message, and nothing MUST change. | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (completion) operation 3 of 5 fails -> refused(conflict) naming index 3, tree unchanged. |
| [PAN-023] | revert-restructure after a later change touched the nodes MUST be refused(conflict). | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (completion) revert after renaming a moved node -> refused(conflict). |
| [PAN-024] | rename-node with a stale expected-revision MUST be refused(conflict). | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (completion) rename with revision 3 when it is 4 -> refused(conflict). |
| [PAN-025] | set-inferred-icon over a manual icon MUST be refused(conflict). | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (completion) set inferred icon on a manually set icon -> refused(conflict). |
| [PAN-026] | open-external with a scheme other than http, https or mailto MUST be refused(not-permitted). | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (completion) open-external with file:// -> refused(not-permitted). |
| [PAN-030] | create-pane with a cwd whose scope is not granted MUST be refused(not-permitted). With a cwd folder that does not exist it MUST be refused(not-found). With a program that cannot start it MUST complete failed(io). | [errors and edge cases](/pages/capability-panes-create/#edge-cases-and-errors) | (completion) cwd project/missing -> refused(not-found). command ["nope"] -> failed(io). |
| [PAN-031] | create-pane with command none and content-kind ilium.terminal MUST run the login shell from SHELL, or /bin/sh when SHELL is unset. | [errors and edge cases](/pages/capability-panes-create/#edge-cases-and-errors) | (screen-text) create with SHELL unset -> /bin/sh prompt. |
| [PAN-032] | panes.lifecycle MUST be a high-risk grant and every use MUST be written to the plugin log. | [errors and limits](/pages/api-pane-lifecycle/#errors-and-limits) | (log-line) freeze a pane -> a log line names the intent and the pane. |

### See also

- [Requirements index](/pages/reference-requirements/)
- [Error codes and limits](/pages/reference-errors-and-limits/)
