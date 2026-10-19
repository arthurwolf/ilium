---
layout: page
title: "Capability: agent-config.propose"
permalink: /pages/capability-agent-config-propose/
---

## agent-config.propose:&lt;agent&gt;

The `agent-config.propose:<agent>` capability lets a plugin propose changes to the files in one coding agent's configuration folder, such as `~/.claude/settings.json` or `~/.codex/config.toml`. The plugin never writes those files. It sends the host a proposal: a title and a list of whole new file contents. The host shows the user the full diff in a kernel dialog, and applies only the files the user checks, all together or not at all.

This is the only way a plugin changes agent configuration. A direct write grant, `fs.write:agent-config:<agent>`, is refused at install with `manifest.grant-scope` ([Sandbox reference](/pages/reference-sandbox/#file-scopes)), because the agent command-line tools run hooks and commands from these files with the user's rights. A proposal keeps the user in the loop for every change.

<div class="note">
<p>Grants are per side. A plugin that proposes from its server side needs the grant in <code>capabilities.server</code>; a client-side proposal needs it in <code>capabilities.client</code>. Scenes cannot propose.</p>
</div>

**You will need:**

- A grant `agent-config.propose:<agent>` for each agent whose configuration the plugin changes, on the side that sends the proposal. The agents are `claude`, `codex`, `gemini` and `antigravity`.
- The read grant `fs.read:agent-config:<agent>` for the same agent on the same side, so the plugin can read the current file and its revision first. Without it the install fails with `manifest.grant-dependency`.
- An attached TUI client, because only the user answers a proposal.

SDK paths: Rust `ilium_plugin_sdk::host::fs`, TypeScript `@ilium/plugin-sdk/host/fs`. Reads use [fs.read](/pages/capability-fs-read/) and the revision tokens of [atomic writes and revisions](/pages/api-fs-atomic-and-tail/).

### Grant syntax

The argument is exactly one agent name. The manifest checks the grant when the package is installed.

| Grant | Valid | Reason |
|---|---|---|
| `agent-config.propose:claude` | yes | the Claude configuration folder, high risk |
| `agent-config.propose:codex` | yes | the Codex configuration folder, high risk |
| `agent-config.propose:gemini`, `agent-config.propose:antigravity` | yes | the Gemini CLI and Antigravity configuration roots, high risk |
| `agent-config.propose` | no | `manifest.grant-argument-missing`: needs one agent name |
| `agent-config.propose:*` | no | `manifest.grant-wildcard`: a manifest that changes two agents lists two grants |
| `agent-config.propose:cursor` | no | `manifest.grant-scope`: not a known agent scope |
| `fs.write:agent-config:claude` | no | `manifest.grant-scope`: direct writes to agent configuration are refused |

The grant is high risk (H) at every tier and available to T1, T2 and T3 packages. The risk comes from what the files do: an agent runs the hooks and commands its configuration names, so an accepted change can run code at the agent's next start. The per-proposal review is what makes the grant safe to give to any tier.

### Functions

| Function | Rust (`fs::`) | TypeScript | Kind | Result |
|---|---|---|---|---|
| `propose-agent-config(agent, title, changes)` | `propose_agent_config(&str, &str, Vec<ConfigChange>)` | `proposeAgentConfig(agent, title, changes)` | async | `ProposalResult { applied, rejected, history }`, or an error |

```wit
// Excerpt of ilium:host/fs in reference-wit
record config-change {
  path: string,                     // relative to the agent's configuration folder
  content: option<list<u8>>,        // the whole new file; none removes the file
  revision: option<revision-token>, // from read-with-revision; none means the file must not exist yet
  summary: string,                  // one line shown beside the file in the dialog
}
record proposal-result {
  applied: list<string>,            // paths the user checked, now written
  rejected: list<string>,           // paths the user left unchecked, unchanged
  history: string,                  // path of the history entry in the plugin's data scope
}

propose-agent-config: func(agent: string, title: string, changes: list<config-change>) -> result<request-id, plugin-error>;
take-proposal: func(request: request-id) -> option<proposal-result>;
```

A change carries the whole new file, not a patch. The host computes the diff itself from the file on disk, so the user always sees what will really be written. The size limits of a proposal (number of changes, total new content, title and summary length) are in [Errors and limits](/pages/reference-errors-and-limits/#error-codes-of-the-extended-functions).

### How a proposal is handled

1. **Checks at the call.** The host checks the grant, the agent name, every path and the size limits. A path that leaves the configuration folder, names a credential file ([SEC-063]) or resolves into the deny set ([SEC-060]) refuses the whole proposal with `not-permitted` and reason `scope`; no dialog opens. Inside the agent's transcript folder (the folder of `agent-sessions:<agent>`) only a new file may be proposed: a change there with a `revision`, or one that removes a file, is refused the same way, so existing transcripts stay read only. Two changes to the same path, an empty list or a proposal over the limits return `invalid-argument`.
2. **Revision check.** For each change the host compares `revision` with the file on disk. A file that changed since the plugin read it, or that exists when `revision` is `none`, completes the proposal with `failed(conflict)` before the dialog opens.
3. **Consent dialog.** The kernel draws the dialog, modal over every plugin widget ([SEC-102]), with the protection against accidental approval of [SEC-103]. It names the plugin, the agent and the title, lists one row per file with its summary and its added and removed line counts, and shows the full diff of the selected file, scrolled one screen at a time; a new file is shown as its size, its line count and its lines. Every row starts unchecked, and the default focus is on **Reject all**. The prompt counts as a consent prompt under [SEC-105]: one pending proposal per plugin, the same hourly cap, and the same answer deadline.
4. **Apply.** When the user presses Enter with at least one row checked, the host checks every revision again. It then copies the previous content of each checked file into the history entry, writes each new file to a temporary sibling and renames it over the target. If any step fails, the host restores every file it already replaced from the history entry, and the proposal completes with `failed(io)`. The user therefore gets all checked files or none.
5. **Completion.** `request-completed` reports `ok`, and `take-proposal` returns the applied and rejected paths and the history path.

Nothing about a proposal is stored as a standing consent. The next proposal is shown in full again, even when it is identical, and the user's answer is never written to `consents.json`.

### History and revert

The history entry is the file `agent-config-history/<agent>/<request-id>.json` in the plugin's own `data` scope. The host writes it before it replaces any file. It holds, for each applied path, the previous content, or `null` when the file did not exist before. It counts against the plugin's `disk_mib` limit; when the limit leaves no room for it, the proposal completes with `failed(quota-exceeded)` and no file changes.

A revert is an ordinary proposal: the plugin reads the history entry, reads the current revisions, and proposes the old contents (or `content: none` for a file that the proposal created). The user sees the revert diff and accepts it like any other change. The host never reverts a file on its own. Uninstalling with `--purge` deletes the `data` scope and with it the revert history; the configuration files keep their current content.

### Completion

| Outcome | Meaning |
|---|---|
| `ok` | The checked files were written. `take-proposal` lists them in `applied` and the unchecked ones in `rejected`. |
| `failed(not-permitted)` | The user pressed **Reject all**, pressed Enter with no row checked, or closed the dialog. The message starts with `declined: `. |
| `failed(timed-out)` | The user did not answer within the [SEC-105] deadline. The dialog closes and nothing is written. |
| `failed(conflict)` | A file changed after the plugin read it, found before the dialog or just before the apply. Nothing is written. |
| `failed(quota-exceeded)` | The history entry does not fit in `disk_mib`. Nothing is written. |
| `failed(io)` | A disk error during the apply. Every file already replaced is restored. |
| `cancelled` | The plugin stopped, the grant was revoked or the host shut down while the dialog was open. The dialog closes and nothing is written. |

<figure>
<pre class="term">╭─<span class="t-hi"> Change agent configuration </span>───────────────────────────────────────────╮
│ Agent setup (ilium.core.agent-setup) proposes 2 changes to the         │
│ Claude configuration. Nothing is changed until you apply.              │
│                                                                        │
│ [x] CLAUDE.md, add the Ilium instruction block <span class="t-dim">……………………………………………</span> +4 −0 │
│ [ ] settings.json, add the notification hook <span class="t-dim">…………………………………………………</span> +6 −1 │
│                                                                        │
│ <span class="t-hi">CLAUDE.md (file 1 of 2)</span>                                                │
│   # Notes for agents                                                   │
│ <span class="t-ok">+ &lt;!-- ilium:begin chatroom v3 --&gt;</span>                                     │
│ <span class="t-ok">+ Talk to the other agents with `ilium chat send`.</span>                     │
│ <span class="t-ok">+ &lt;!-- ilium:end chatroom --&gt;</span>                                          │
│ <span class="t-ok">+</span>                                                                      │
│                                                                        │
│ <span class="t-dim">Space toggle · Tab next file · Enter apply checked · Esc reject all</span>    │
╰────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative. The kernel's proposal dialog after the user checked the first file. Rows start unchecked; Enter applies only the checked files, and Esc rejects the whole proposal.</figcaption>
</figure>

### Errors

| Code | Where | When |
|---|---|---|
| `not-permitted` | call | No grant, the grant was declined (reason `declined`), or a path is outside the scope, a credential file or in the deny set (reason `scope`). |
| `invalid-argument` | call | A bad path, an empty change list, two changes to one path, or a proposal over the size limits. |
| `busy` | call | A proposal or another consent prompt of this plugin is already pending ([SEC-105]); `retry-after-ms` is set. |
| `rate-limited` | call | The plugin reached the hourly prompt cap of [SEC-105]; `retry-after-ms` is set. |
| `revoked` | call | The grant was revoked while the call was running. |
| `not-permitted` | completion | The user rejected every change; the message starts with `declined: `. |
| `timed-out` | completion | No answer within the [SEC-105] deadline. |
| `conflict` | completion | A revision no longer matches the file. |
| `quota-exceeded` | completion | The history entry does not fit in `disk_mib`. |
| `io` | completion | A disk error during the apply; replaced files are restored. |

All of these are existing codes; the proposal adds none.

### Risk, review and logging

- The grant is H. A bundled T1 plugin is first activated without it and asks at first start ([SEC-111]). A T2 or T3 package shows it in the review dialog as an unchecked `HIGH` row with the wording `Proposes changes to the <agent> configuration`.
- Holding the grant never writes a file by itself. Every proposal opens its own dialog, and only the user's Enter applies it.
- Each applied file writes one `write-outside-data` record to the [audit log](/pages/reference-audit-log/), with the plugin id as actor, the grant string and the target `<agent>/<path>`. The plugin log gets one info line per applied file and one line per rejected or failed proposal.
- Revoking the grant in Settings > Plugins, or with `ilium plugin grants <id> --revoke <grant>`, closes an open dialog and completes the proposal as `cancelled`.

### When the grant is declined

A declined grant links the proposal import to a stub that returns `not-permitted` with reason `declined` at once, and no dialog opens. The plugin keeps running and can still read the configuration with its read grant, for example to show what it would change. A call whose capability the manifest never requested traps the instance, because that is a plugin bug.

### Examples

Both examples assume a server-side grant:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.agent-notes",
  "capabilities": { "server": ["fs.read:agent-config:claude", "agent-config.propose:claude", "fs.read:agent-config:codex", "agent-config.propose:codex"] }
}
```

**Example 1: propose an instruction block (Rust).** The plugin reads `CLAUDE.md` with its revision and proposes the file with the block added. A missing file is proposed as a new file with `revision: None`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::fs::{self, ConfigChange};

const BEGIN: &str = "<!-- ilium:begin chatroom v3 -->";
const BLOCK: &str = "<!-- ilium:begin chatroom v3 -->\nTalk to the other agents with `ilium chat send`.\n<!-- ilium:end chatroom -->\n";

// Proposes the block. Nothing is written until the user checks the file and presses Enter.
async fn propose_block() -> Result<Vec<String>, PluginError> {
    let path = FsPath { scope: "agent-config:claude".into(), path: "CLAUDE.md".into() };
    let (current, revision) = match fs::read_with_revision(&path).await {
        Ok(read) => (String::from_utf8_lossy(&read.bytes).into_owned(), Some(read.revision)),
        Err(error) if error.code == ErrorCode::NotFound => (String::new(), None),
        Err(error) => return Err(error),
    };
    if current.contains(BEGIN) {
        return Ok(Vec::new()); // already set up, nothing to propose
    }
    let change = ConfigChange {
        path: "CLAUDE.md".into(),
        content: Some(format!("{current}\n{BLOCK}").into_bytes()),
        revision,
        summary: "add the Ilium instruction block".into(),
    };
    let result = fs::propose_agent_config("claude", "Ilium instructions", vec![change]).await?;
    Ok(result.applied)
}
```

**Example 2: propose an auto-compact limit and handle the answer (TypeScript).** A rejection and a changed file are normal results; the plugin reports them and does not retry by itself.

**TypeScript**

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import { readWithRevision, proposeAgentConfig } from '@ilium/plugin-sdk/host/fs';

const KEY = 'model_auto_compact_token_limit';
const decoder = new TextDecoder();
const encoder = new TextEncoder();

// Sets the top-level key, or adds it before the first table of the file.
function withLimit(toml: string, limit: number): string {
  const line = `${KEY} = ${limit}`;
  const pattern = new RegExp(`^${KEY}\\s*=.*$`, 'm');
  return pattern.test(toml) ? toml.replace(pattern, line) : `${line}\n${toml}`;
}

async function proposeLimit(limit: number): Promise<'applied' | 'declined' | 'changed' | 'no-answer'> {
  const path = { scope: 'agent-config:codex', path: 'config.toml' };
  const current = await readWithRevision(path);
  const next = withLimit(decoder.decode(current.bytes), limit);
  try {
    await proposeAgentConfig('codex', 'Auto-compact threshold', [
      { path: 'config.toml', content: encoder.encode(next), revision: current.revision, summary: `set ${KEY} to ${limit}` },
    ]);
    return 'applied';
  } catch (error) {
    if (!(error instanceof PluginError)) throw error;
    if (error.code === 'not-permitted' && error.message.startsWith('declined: ')) return 'declined';
    if (error.code === 'conflict') return 'changed';
    if (error.code === 'timed-out') return 'no-answer';
    throw error;
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    proposeLimit(180000).then(
      (answer) => console.info(`auto-compact proposal: ${answer}`),
      (error: unknown) => console.error('auto-compact proposal failed', error),
    );
  },
  onEvents: () => [],
});
```

### What the user sees

The dialog above opens in the client that most recently had input. After the answer, the plugin log shows what was applied:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.agent-notes
<span class="t-dim">2026-10-19 09:14:02</span> <span class="t-ok">info</span>  propose agent-config:claude "Ilium instructions" 1 change
<span class="t-dim">2026-10-19 09:14:31</span> <span class="t-ok">info</span>  applied agent-config:claude CLAUDE.md 412 bytes
<span class="t-dim">2026-10-19 09:20:05</span> <span class="t-ok">info</span>  propose agent-config:codex "Auto-compact threshold" 1 change
<span class="t-dim">2026-10-19 09:20:12</span> <span class="t-warn">warn</span>  proposal declined by the user, nothing written
</pre>
<figcaption>Illustrative, not generated. One accepted and one rejected proposal.</figcaption>
</figure>

### Who requests it

First-party plugins from [feature traceability](/pages/reference-feature-traceability/):

- 37 Configure agent (Agent setup plugin): proposes edits to agent configuration files such as `settings.json` and `config.toml`; the user approves each one in the dialog.
- 63 Agent setup: proposes the Ilium instruction blocks and hooks in the files inside each agent's configuration folder. Instruction files in the project folder are written with `fs.write:project`.
- 82 Compaction optimiser (Compaction plugin): **Apply** proposes the recommended auto-compact threshold, and **Revert** proposes the previous value from the history entry. Both open the dialog.

Requirement ids for this page: [CAP-886] to [CAP-891]. See the [requirements index](/pages/reference-requirements/).

### See also

- [fs.read](/pages/capability-fs-read/) for reading the configuration and its revision.
- [fs.write](/pages/capability-fs-write/) for the scopes a plugin may write directly.
- [Atomic writes and revisions](/pages/api-fs-atomic-and-tail/) for revision tokens.
- [Grant review and consent](/pages/reference-grant-review-and-consent/) for the dialog rules and the prompt limits.
- [Errors and limits](/pages/reference-errors-and-limits/) for the codes and the proposal size limits.

### Security

The authoritative scope table is in [Sandbox reference](/pages/reference-sandbox/). Credential files stay excluded from every proposal ([SEC-063]), and nothing reaches the deny set ([SEC-060]).
