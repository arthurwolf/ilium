### Changed
- The kernel no longer stores per-pane scheduled input or a prompt queue: the two tree fields, their tree methods, snapshot persistence, the queue and countdown labels in the tree, and the matching status-rule facts are gone. The Scheduler plugin owns that state. The `queued_prompts` and `prompt_queue_held` rule facts are now unknown unless an owner supplies them.
- The IPC frame schema tag changed because the tree snapshot layout changed; a client and server from before this change refuse each other's frames loudly.

### Removed
- The voice-control prompt templates for scheduling terminal input, queuing a prompt, clearing the queue and the "runs is required" policy message, plus the unused `write_scheduled_key_input` server helper.
