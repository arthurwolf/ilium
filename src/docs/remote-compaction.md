# Remote compaction

Claude Code and Codex compact their own context when it fills up, but only near the very end of the window and with a prompt you cannot change. Remote compaction does it earlier and under your control: Ilium stops the agent at a safe moment, summarizes the transcript with the model chosen in the **Inference** tab, rewrites the session file so it looks like the agent compacted it itself, and resumes the same session.

It is off by default. Turn it on in **Settings > Remote compaction**.

## What happens

1. **Wait for a safe pause.** The agent must be between turns with no tool call outstanding. If it is still busy after the pause timeout (default 120 s), Ilium sends it `Esc` once and waits a little longer.
2. **Stop the agent.** The pane freezes on its last screen and the agent process is terminated.
3. **Read and prepare.** The session transcript is parsed, old tool output is trimmed, secrets are redacted, and a deterministic ledger of files, commands and errors is built.
4. **Summarize.** The prepared history goes to your inference model. Large sessions are split into chunks that build on each other and are merged, with automatic re-chunking when the provider reports the input too long. If the model keeps failing, an offline fallback summary (the ledger plus your last requests) is used instead, and the status line says so.
5. **Write back.** The original file is copied to `<session>.pre-compaction-<UTC time>.bak` (the newest few are kept), then rewritten atomically. Claude Code gets a `compact_boundary` record and a summary message; Codex gets a `compacted` record with its replacement history. Nothing is written if the file changed while Ilium worked.
6. **Resume.** The pane restarts the agent on the same session id (`claude --resume`, `codex resume`).

While this runs a dialog over the frozen pane shows the steps, an overall progress bar, token bars (context before, conversation size, summarizer input and output, recent tail kept, context after, each as a share of the model window), the elapsed time and a log. `Esc` cancels before the file is written. If anything fails the transcript is untouched and `Enter` resumes the original session.

## Starting it

* **Manually:** the **Compact** button of the agent toolbar runs remote compaction instead of sending `/compact` while the feature is on and the pane has a verified transcript. Otherwise the button still sends `/compact`.
* **Automatically:** with **Automatic** on, the active agent pane is compacted when its context reaches the threshold (default 65 %, between 30 % and 88 %, always below the agents' own trigger). A pane is retried no sooner than the cooldown (default 10 minutes), is skipped after three failures in a row until its context drops, and only the pane you are looking at is considered because the dialog takes the keyboard.

## Techniques

Each agent has its own technique row, and one more row covers other agents.

| Technique | Prompt |
| --- | --- |
| Claude Code | The real nine-section prompt Claude Code uses (analysis scratchpad, then a structured summary). Default for Claude. |
| Codex | The real Codex handoff-summary prompt. Default for Codex. |
| opencode | An approximation of opencode's anchored-summary prompt. |
| Gemini CLI | An approximation of Gemini CLI's state-snapshot prompt. |
| Best of all worlds | The nine sections plus a scratchpad, the objective first and next step last, a guard that treats history as untrusted data, an anchored update of any earlier summary, and the deterministic ledger. |
| Custom | Your own prompt from the settings tab. |

Where the upstream prompt is not public the technique is an approximation that reuses Claude's nine sections.

## Privacy

The transcript, which can contain your prompts, code, tool output and possibly secrets, is sent to the configured provider and model. A notice at the top of the settings tab and of the dialog names that destination. Close it with `x`, `Delete` or its button; the choice is saved and the notice never returns. Secret redaction (on by default) masks tokens and keys before sending, but it is a heuristic.

## Limits

* Only Claude Code and Codex sessions with a verified transcript can be compacted.
* Encrypted Codex compactions and reasoning items cannot be read; they are kept out of the summary input.
* A running Ilium server keeps the binary it started with; restart Ilium after upgrading to use a new version of this feature.
