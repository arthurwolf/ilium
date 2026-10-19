# Capability: process.long-lived

## process.long-lived

The `process.long-lived` capability lets a plugin start a helper program that keeps running, feed it through standard input, and receive its output line by line. A one-shot run with [process.run](/pages/capability-process-run/) waits for the program to end; a spawned child stays alive until it exits, the plugin drops it, or the plugin stops. The program must also match a `process.run:<program>` grant. Long-lived children are high risk (**H**).

**You will need:**

- Two grants: `process.long-lived` and `process.run:<program>` for the same program.
- A plan for the child's lifetime: dropping the handle or stopping the plugin ends it (SIGTERM, then SIGKILL after 2 s).

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A child process is spawned, streams lines to the plugin and ends on exit or when the plugin drops it">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls process.spawn(spawn-spec)</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">program must match process.run</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host starts the child at low priority</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">counts against the 16 per session (session_processes)</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Output arrives as child-output events</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">whole lines with line-mode; child-exited at the end</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">child handle</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">not-permitted, quota-exceeded</text>
</svg>
<figcaption>The host owns the process. Dropping the handle or stopping the plugin ends the child.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `process.long-lived` | yes | no argument; the program is named by `process.run:<program>` |
| `process.long-lived:tail` | no | no argument (`manifest.grant-argument`) |
| `process.spawn` | no | unknown grant name |

High risk. Without a matching `process.run:<program>` grant, `spawn` fails with `not-permitted`. Children run at low priority under the plugin CPU quota.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `process.spawn` | function | `spawn(spawn-spec { process, stdin-open, line-mode }) -> result<child, plugin-error>`. |
| `child` | resource | `pid-hint()` (display only), `write-stdin(bytes)`, `close-stdin()`, `signal(one of interrupt, terminate, kill, hangup)`. |
| `child-output` | event | `{ child, stream, bytes }`; whole lines with `line-mode`. |
| `child-exited` | event | `{ child, status, signal }`. |

One-shot runs are on [Process control](/pages/api-process-control/). The `timeout-millis` of a one-shot spec is ignored for children.

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.watcher",
  "capabilities": {
    "server": [
      "process.run:tail",
      "process.long-lived"
    ]
  }
}
```

### Worked example

This plugin starts `tail -f` on a log file, receives each line as an event, and stops the child when the command `com.example.watcher/stop` runs.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::host::process::{self, Child, ProcessSpec, SpawnSpec};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Watcher { child: Option<Child> }

impl Handler for Watcher {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command == "com.example.watcher/start" => {
                    let spec = SpawnSpec {
                        process: ProcessSpec::new("tail", &["-f", "/var/log/syslog"]),
                        stdin_open: false,
                        line_mode: true,
                    };
                    self.child = process::spawn(spec).ok();
                }
                Event::CommandInvoked(invoked) if invoked.command == "com.example.watcher/stop" => {
                    if let Some(child) = self.child.take() { let _ = child.signal("terminate"); }
                }
                Event::ChildOutput(output) => log::log(Level::Info, &String::from_utf8_lossy(&output.bytes)),
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { spawn, type Child } from '@ilium/plugin-sdk/host/process';
import { log } from '@ilium/plugin-sdk/host/log';

let child: Child | undefined;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'com.example.watcher/start') {
        child = spawn({ process: { program: 'tail', args: ['-f', '/var/log/syslog'] }, stdinOpen: false, lineMode: true });
      } else if (event.tag === 'command-invoked' && event.val.command === 'com.example.watcher/stop') {
        child?.signal('terminate');
        child = undefined;
      } else if (event.tag === 'child-output') {
        log('info', new TextDecoder().decode(event.val.bytes));
      }
    }
    return [];
  },
});
```

Keep the handle in plugin state. Dropping it ends the child, which is also the clean-up path when the plugin deactivates.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `not-permitted` | Use a one-shot `process.run` if granted. |
| Program not covered by `process.run:<program>` | `not-permitted` | Add the matching grant. |
| More than 16 per session (`session_processes`), shared by `process.run` (`busy`) and `process.spawn` (`quota-exceeded`) | `quota-exceeded` | Reuse one child. |
| Program not found | `not-found` | Check the path or tell the user. |
| Child exits on its own | `child-exited` event | Restart only on user action. |
| Plugin stops | child gets SIGTERM, then SIGKILL after 2 s | Nothing. |

Output of a child can carry secrets from the environment. The host passes a minimal environment unless the spec names variables.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line is high risk and starts unchecked. The matching program grant is listed on the next line.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.watcher 0.1.0 by Example Ltd                                   │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Runs the program tail</span> <span class="t-dim">……………………………………………………………………………</span> <span class="t-warn">process.run:tail</span> │
│ <span class="t-warn">! [ ] Keeps helper programs running</span> <span class="t-dim">…………………………………………………</span> <span class="t-warn">process.long-lived</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests process.long-lived, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.watcher
<span class="t-dim">Grant                             Risk     State</span>
process.run:tail                  high     <span class="t-warn">declined</span>
process.long-lived                high     <span class="t-warn">declined</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.watcher > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [process.run](/pages/capability-process-run/).
- [jobs.run](/pages/capability-jobs-run/).
- [Process control](/pages/api-process-control/).
- [Errors and limits](/pages/errors-and-limits/).

### Security

Long-lived children follow the same confinement as `process.run` ([SEC-032], [SEC-033]). See [Sandbox reference](/pages/reference-sandbox/).
