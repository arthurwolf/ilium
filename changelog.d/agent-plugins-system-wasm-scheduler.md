### Changed
- Scheduled input and the prompt queue now live entirely in the Scheduler core plugin: its server side keeps the schedules and queues, fires them and delivers each queued prompt when the agent finishes, and its client side draws the "Schedule input" and "Queue prompt" dialogs. With the plugin disabled the pane menu entries do nothing and nothing is scheduled or queued.

### Removed
- The built-in scheduled-input and prompt-queue code in the server and client, the matching wire requests, and the `ilium ctl schedule`, `queue` and `queue-clear` commands.
