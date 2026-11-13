---
layout: page
title: "intents: pane lifecycle"
permalink: /pages/api-intent-pane-lifecycle/
---

## Pane lifecycle intents

Five intents let a server-side plugin control the process of a terminal pane: stop it without killing it, restart it with another command, terminate it politely, and ask for a size. All five need the grant `panes.lifecycle`, which is high risk and offered to the server side only. A high-risk grant needs an explicit user grant even for a bundled first-party plugin. The kernel keeps what stays kernel: PTY spawn, the resize itself, and the durable resume binding of agent sessions.

**You need:** `panes.lifecycle` in the `server` capability list of the manifest. To observe results you also want `panes.read`, which delivers `pane-lifecycle-changed`. See [panes](/pages/api-panes/) for the read side and [intents](/pages/api-intents/) for batches and keys.

```json
"capabilities": { "server": ["panes.read", "panes.lifecycle"] }
```

### Records

```wit
record freeze-pane-spec { pane: node-id }
record unfreeze-pane-spec { pane: node-id }
record replace-pane-command-spec {
  pane: node-id, command: list<string>, cwd: option<fs-path>, keep-scrollback: bool,
}
record terminate-pane-process-spec { pane: node-id, grace-ms: u32 }       // at most 10,000
record resize-pane-spec { pane: node-id, columns: option<u16>, rows: option<u16> }

enum pane-lifecycle-state { running, frozen, restarting, exited }
// event: pane-lifecycle-changed { pane: node-id, state: pane-lifecycle-state, by: option<plugin-id> }
```

### Intents

| Intent | Effect | Outcome |
|---|---|---|
| `freeze-pane` | The process is stopped with SIGSTOP on Unix. Output stops and the pane shows the frozen mark. | `ok`; `refused(unsupported)` on Windows; `refused(conflict)` when the pane is already frozen or its process has exited |
| `unfreeze-pane` | The process continues. | `ok`; `refused(conflict)` when the pane is not frozen |
| `replace-pane-command` | The current process ends and the new `command` runs in the same pane, with `cwd` if given. `keep-scrollback` keeps the history above the new output. | `ok` once the new process runs; `refused(invalid-argument)` when the `cwd` does not exist or the program cannot be found, checked before the old process is ended, so the old process keeps running. The durable resume binding stays with the kernel. |
| `terminate-pane-process` | Sends a terminate request, waits `grace-ms`, then kills. At most 10,000 ms. | `ok` when the process has exited. The pane stays open and shows its exit status. `refused(internal)` when the kill fails; the pane stays open and its status is unchanged. |
| `resize-pane` | Asks the layout for `columns` and `rows`. A field left unset keeps its size. | `ok`; `refused(conflict)` when the layout cannot give the size. The message names the limiting neighbour. |

An exited pane has no process to stop or continue, so `freeze-pane` and `unfreeze-pane` refuse it with `refused(conflict)`. `terminate-pane-process` on an exited pane is a no-op that returns `ok`, `resize-pane` and `replace-pane-command` still work on it (the latter starts a new process). Freezing a frozen pane is refused with `refused(conflict)`. `terminate-pane-process` on a frozen pane first sends SIGCONT so that the process can act on the terminate request, then proceeds as usual and returns `ok`. `replace-pane-command` on a frozen pane continues the old process, ends it, and starts the new one; the new process is not frozen and the frozen mark is cleared.

Common refusals: `refused(not-permitted)` without the grant, `refused(not-found)` for a missing pane, `refused(pane-not-ready)` on a restored pane that has not started, `refused(invalid-argument)` for an empty command or a `grace-ms` over 10,000.

<figure>
<svg viewBox="0 0 420 130" role="img" aria-label="Pane lifecycle states and the intents that move between them">
  <defs>
    <marker id="lifecycle-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="40" width="80" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="48" y="61" font-size="12" text-anchor="middle" fill="currentColor">running</text>
  <rect x="170" y="8" width="80" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="29" font-size="12" text-anchor="middle" fill="currentColor">frozen</text>
  <rect x="170" y="88" width="80" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="109" font-size="12" text-anchor="middle" fill="currentColor">restarting</text>
  <rect x="332" y="40" width="80" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="372" y="61" font-size="12" text-anchor="middle" fill="currentColor">exited</text>
  <line x1="88" y1="48" x2="168" y2="28" stroke="currentColor" marker-end="url(#lifecycle-arrow)"/>
  <text x="108" y="30" font-size="9" fill="currentColor">freeze-pane</text>
  <line x1="170" y1="36" x2="90" y2="56" stroke="currentColor" marker-end="url(#lifecycle-arrow)"/>
  <text x="96" y="76" font-size="9" fill="currentColor">unfreeze-pane</text>
  <line x1="88" y1="68" x2="168" y2="98" stroke="currentColor" marker-end="url(#lifecycle-arrow)"/>
  <text x="96" y="100" font-size="9" fill="currentColor">replace-pane-command</text>
  <line x1="250" y1="104" x2="330" y2="68" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#lifecycle-arrow)"/>
  <line x1="170" y1="104" x2="88" y2="72" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#lifecycle-arrow)"/>
  <line x1="88" y1="57" x2="330" y2="57" stroke="currentColor" marker-end="url(#lifecycle-arrow)"/>
  <text x="240" y="52" font-size="9" fill="currentColor">terminate-pane-process</text>
</svg>
<figcaption>Every transition is confirmed by <code>pane-lifecycle-changed</code>. <code>resize-pane</code> does not change the state.</figcaption>
</figure>

### Example: freeze on a lock-up, unfreeze after a pause

This plugin freezes a pane whose agent has been working too long, and unfreezes it again on a timer. The freeze key stops a repeated event from freezing twice. A timer comes from the [clock and timers](/pages/api-clock-and-timers/) interface.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{FreezePaneSpec, TerminatePaneProcessSpec, UnfreezePaneSpec};

fn on_runaway(pane: NodeId) -> Vec<Submission> {
    vec![Submission::keyed(Intent::FreezePane(FreezePaneSpec { pane }), format!("freeze-{pane}"))]
}

fn on_timer(pane: NodeId) -> Vec<Submission> {
    vec![Submission::keyed(Intent::UnfreezePane(UnfreezePaneSpec { pane }), format!("thaw-{pane}"))]
}

fn stop_hard(pane: NodeId) -> Submission {
    Submission::new(Intent::TerminatePaneProcess(TerminatePaneProcessSpec { pane, grace_ms: 3_000 }))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

const freeze = (pane: bigint): Submission =>
  ({ intent: { tag: 'freeze-pane', val: { pane } }, correlation: 1n, dedupKey: `freeze-${pane}` });
const thaw = (pane: bigint): Submission =>
  ({ intent: { tag: 'unfreeze-pane', val: { pane } }, correlation: 2n, dedupKey: `thaw-${pane}` });
const stopHard = (pane: bigint): Submission =>
  ({ intent: { tag: 'terminate-pane-process', val: { pane, graceMs: 3000 } }, correlation: 3n, dedupKey: undefined });
```

### Example: replace the command and ask for a size

**Rust**

```rust
let replace = Intent::ReplacePaneCommand(ReplacePaneCommandSpec {
    pane,
    command: vec!["bash".into(), "-lc".into(), "make watch".into()],
    cwd: Some("/work/app".into()),
    keep_scrollback: true,
});
let resize = Intent::ResizePane(ResizePaneSpec { pane, columns: Some(100), rows: None });
```

**TypeScript**

```ts
const replace = { tag: 'replace-pane-command', val: {
  pane, command: ['bash', '-lc', 'make watch'], cwd: '/work/app', keepScrollback: true } } as const;
const resize = { tag: 'resize-pane', val: { pane, columns: 100, rows: undefined } } as const;
```

When the layout cannot give 100 columns, the outcome is `refused(conflict)` and the message names the neighbour that limits the pane, for example `cannot widen: pane 9 is at its minimum width`. The exact wording is up to the kernel.

### Handling the confirmations

**Rust**

```rust
Event::PaneLifecycleChanged(PaneLifecycleChangedEvent { pane, state, by }) => match state {
    PaneLifecycleState::Frozen => self.frozen.insert(pane),
    PaneLifecycleState::Running | PaneLifecycleState::Exited => self.frozen.remove(&pane),
    PaneLifecycleState::Restarting => false,
},
```

**TypeScript**

```ts
if (event.tag === 'pane-lifecycle-changed') {
  const { pane, state } = event.val;
  if (state === 'frozen') frozen.add(pane); else if (state !== 'restarting') frozen.delete(pane);
}
```

### Errors and limits

| Condition | Result |
|---|---|
| `freeze-pane` on Windows | `refused(unsupported)` |
| `freeze-pane` on a frozen pane | `refused(conflict)` |
| `unfreeze-pane` on a running pane | `refused(conflict)` |
| `terminate-pane-process` or `replace-pane-command` on a frozen pane | The process is continued first, then the intent proceeds; the frozen mark is cleared and the outcome is `ok` |
| `grace-ms` over 10,000 | `refused(invalid-argument)` |
| Layout cannot satisfy `resize-pane` | `refused(conflict)`, message names the neighbour |
| Pane not started after a restore | `refused(pane-not-ready)` |
| Pane missing | `refused(not-found)` |

### What the user sees

A frozen pane shows the frozen mark in its tree row and its frame, and its output stops. The tree row uses the glyph of the frozen slot from the client's icon set.

<pre class="term"><span class="t-dim">▼ 📂 Ilium</span>
<span class="t-hi">›   🖥️ ❄ build-watch</span>
<span class="t-dim">›   🖥️    logs</span>
<span class="t-dim">pane 5  freeze-pane   outcome ok</span>
<span class="t-dim">pane 5  state: running -> frozen  by com.example.guard</span>
</pre>

Illustrative, not generated: glyphs come from the client icon inventory; the two log lines are an example layout.

### See also

- [Intents](/pages/api-intents/) and [events](/pages/api-events/) for batches and `pane-lifecycle-changed`.
- [Panes](/pages/api-panes/), [terminal primitives](/pages/primitives-terminal/) and [panes.signal](/pages/capability-panes-signal/), the lighter signal intent.
- [send-input stages](/pages/api-intent-send-input/) to type into a pane after `replace-pane-command`.

### Security

**[SEC-041]** Pane lifecycle intents from a plugin are checked against its grants like host calls. See [Sandbox reference](/pages/reference-sandbox/).
