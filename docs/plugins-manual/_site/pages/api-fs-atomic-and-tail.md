# fs: atomic write, revisions, tail, agent homes

## fs: atomic write, revisions, tail, agent homes

The `fs` import also offers four things for plugins that share files with other programs: a read that returns a revision token, a write that succeeds only when the file is still at that revision, a cursor-based `tail` for growing files, and three more agent home scopes. They are specified in the `fs` interface of the [WIT reference](/pages/reference-wit/). The base functions (`read`, `write`, `lock`, `watch` and the scopes) are on the [fs](/pages/api-fs/) page; read that first.

### What you need

| Function | Grant | Kind |
|---|---|---|
| `read-with-revision` | `fs.read:<scope>` | async, completes with `take-revisioned` |
| `write-atomic` | `fs.write:<scope>` | async, completes with `take-written` |
| `tail` | `fs.read:<scope>` | sync |

No new grant exists. The grant of the scope is the grant of the call. Available on the server, client and scene sides.

The additional scopes:

| Scope (grant argument) | Preopen | Root | Risk |
|---|---|---|---|
| `agent-sessions:gemini` | `/agent-sessions/gemini` | the Gemini CLI transcripts, `~/.gemini/tmp/*/chats` | high for read; a write grant is refused at install, as for every `agent-sessions` scope |
| `agent-config:gemini` | `/agent-config/gemini` | the Gemini CLI configuration, `~/.gemini/settings.json` | normal for read, high for write |
| `agent-config:antigravity` | `/agent-config/antigravity` | the Antigravity configuration directory, `~/.gemini/antigravity` (a directory root, not a single file) | normal for read, high for write |

The kernel resolves each root per platform, so a plugin never builds the home path itself. A scope whose directory does not exist is still granted; the calls then return `not-found`.

```json
"capabilities": { "server": ["fs.read:agent-sessions:gemini", "fs.read:agent-config:gemini", "fs.write:agent-config:gemini"] }
```

### Signatures

```wit
// Excerpt of ilium:host/fs in reference-wit
type revision-token = string;     // opaque; equal tokens mean equal content
record revisioned { bytes: list<u8>, revision: revision-token }
record tail-chunk { bytes: list<u8>, cursor: u64, truncated: bool }

read-with-revision: func(path: fs-path) -> result<request-id, plugin-error>;
take-revisioned: func(request: request-id) -> option<revisioned>;
write-atomic: func(path: fs-path, data: list<u8>, expected: option<revision-token>) -> result<request-id, plugin-error>;
take-written: func(request: request-id) -> option<revision-token>;
tail: func(path: fs-path, cursor: u64, max-bytes: u32) -> result<tail-chunk, plugin-error>;
```

| Rust | TypeScript | Returns |
|---|---|---|
| `fs::read_with_revision(&path).await` | `await readWithRevision(path)` | `Revisioned { bytes, revision }` |
| `fs::write_atomic(&path, data, expected).await` | `await writeAtomic(path, data, expected)` | the new `RevisionToken` (`string`) |
| `fs::tail(&path, cursor, max_bytes)` | `tail(path, cursor, maxBytes)` | `TailChunk { bytes, cursor, truncated }` (`cursor` is `bigint` in TypeScript) |

A revision token is opaque. It is a hash of the content plus the size and the modification time, and two equal tokens mean equal content. A plugin stores it and passes it back; it never parses it.

### Atomic write and the expected revision

`write-atomic` writes to a temporary file in the directory of the target and renames it over the target, so a reader never sees a half-written file. The `expected` argument decides when the rename is allowed:

| `expected` | The write succeeds when | Otherwise |
|---|---|---|
| `none` | the file does not exist | completion `failed(conflict)` |
| `some(token)` | the file still has that revision | completion `failed(conflict)` |

On success the completion carries the revision of the new content, which is the `expected` value for the next write. The check and the rename are one step in the host, so two plugins that read the same revision cannot both win.

<figure>
<svg viewBox="0 0 440 250" role="img" aria-label="Two writers compete for one file with expected revisions">
<title>Compare and replace with revisions</title>
<desc>Plugin A and plugin B both read revision r1. A writes with expected r1 and succeeds, producing r2. B writes with expected r1 and receives conflict, then rereads r2 and retries.</desc>
<defs>
<marker id="fsat-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker>
</defs>
<text x="60" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin A</text>
<text x="220" y="16" font-size="12" text-anchor="middle" fill="currentColor">Host and file</text>
<text x="380" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin B</text>
<line x1="60" y1="24" x2="60" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="220" y1="24" x2="220" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="380" y1="24" x2="380" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="60" y1="44" x2="218" y2="44" stroke="currentColor" marker-end="url(#fsat-arrow)"/>
<text x="64" y="38" font-size="10" fill="currentColor">read-with-revision (r1)</text>
<line x1="380" y1="62" x2="222" y2="62" stroke="currentColor" marker-end="url(#fsat-arrow)"/>
<text x="376" y="56" font-size="10" text-anchor="end" fill="currentColor">read-with-revision (r1)</text>
<line x1="60" y1="94" x2="218" y2="94" stroke="currentColor" marker-end="url(#fsat-arrow)"/>
<text x="64" y="88" font-size="10" fill="currentColor">write-atomic expected r1</text>
<line x1="218" y1="114" x2="62" y2="114" stroke="currentColor" marker-end="url(#fsat-arrow)"/>
<text x="214" y="108" font-size="10" text-anchor="end" fill="currentColor">ok, new revision r2</text>
<line x1="380" y1="144" x2="222" y2="144" stroke="currentColor" marker-end="url(#fsat-arrow)"/>
<text x="376" y="138" font-size="10" text-anchor="end" fill="currentColor">write-atomic expected r1</text>
<line x1="222" y1="164" x2="378" y2="164" stroke="currentColor" marker-end="url(#fsat-arrow)"/>
<text x="226" y="158" font-size="10" fill="currentColor">failed(conflict)</text>
<line x1="380" y1="194" x2="222" y2="194" stroke="currentColor" marker-end="url(#fsat-arrow)"/>
<text x="376" y="188" font-size="10" text-anchor="end" fill="currentColor">read-with-revision (r2)</text>
<line x1="380" y1="224" x2="222" y2="224" stroke="currentColor" marker-end="url(#fsat-arrow)"/>
<text x="376" y="218" font-size="10" text-anchor="end" fill="currentColor">write-atomic expected r2</text>
</svg>
<figcaption>Only one writer holds each revision. The loser gets conflict, rereads and retries on the new revision.</figcaption>
</figure>

### Example: update a board file without losing another writer's change

A kanban plugin reads its board file with a revision, changes one card and writes it back. On `conflict` it rereads and applies the change again, at most three times. `move_card` is the plugin's own pure function.

**Grants:** `fs.read:project`, `fs.write:project`

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::fs;

const BOARD: &str = ".kanban/board.json";
const MAX_ATTEMPTS: u32 = 3;

async fn move_card_on_disk(card: String, column: String) -> Result<(), PluginError> {
    let path = FsPath { scope: "project".into(), path: BOARD.into() };
    for _attempt in 0..MAX_ATTEMPTS {
        let current = fs::read_with_revision(&path).await?;
        let updated = move_card(&current.bytes, &card, &column)?;
        match fs::write_atomic(&path, updated, Some(current.revision)).await {
            Ok(_new_revision) => return Ok(()),
            Err(error) if error.code == ErrorCode::Conflict => continue,
            Err(error) => return Err(error),
        }
    }
    Err(PluginError {
        code: ErrorCode::Conflict,
        message: "the board kept changing; gave up after 3 attempts".to_string(),
        retry_after_ms: None,
    })
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { readWithRevision, writeAtomic } from '@ilium/plugin-sdk/host/fs';
import { moveCard } from './board';

const BOARD = { scope: 'project', path: '.kanban/board.json' };
const MAX_ATTEMPTS = 3;

export async function moveCardOnDisk(card: string, column: string): Promise<void> {
  for (let attempt = 0; attempt < MAX_ATTEMPTS; attempt++) {
    const current = await readWithRevision(BOARD);
    const updated = moveCard(current.bytes, card, column);
    try {
      await writeAtomic(BOARD, updated, current.revision);
      return;
    } catch (error) {
      if (error instanceof PluginError && error.code === 'conflict') {
        continue;
      }
      throw error;
    }
  }
  throw new PluginError('conflict', 'the board kept changing; gave up after 3 attempts');
}
```

To create a file that must not exist yet, pass `None` (`undefined` in TypeScript) as `expected`. Combine `write-atomic` with `lock` when several plugins must also serialize longer read-modify-write sequences; the revision alone detects a lost update but does not queue writers.

### Tail: read what was appended

`tail(path, cursor, max-bytes)` returns the bytes after `cursor` and the cursor to use next. Start with cursor 0. It never blocks and returns at most 1 MiB per call.

| Field | Meaning |
|---|---|
| `bytes` | New bytes since the cursor, possibly empty. |
| `cursor` | Offset to pass to the next call. |
| `truncated` | `true` when the file became shorter than the cursor or was replaced. The read then restarted at 0 and `bytes` is the new start of the file. Drop any partial line you kept. |

A cursor may end in the middle of a line, because a writer can be between two `write` calls. Keep the incomplete tail of the last chunk in the plugin and prepend it to the next one.

<figure>
<svg viewBox="0 0 440 120" role="img" aria-label="Tail cursor over a growing file and a truncation">
<title>Tail cursor</title>
<desc>A file bar with the cursor at byte 4096. New bytes are appended and tail returns them with a new cursor. After the file is replaced by a shorter one, tail restarts at zero and reports truncated.</desc>
<defs>
<marker id="fstl-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker>
</defs>
<rect x="10" y="20" width="260" height="22" fill="none" stroke="currentColor"/>
<line x1="150" y1="20" x2="150" y2="42" stroke="currentColor"/>
<text x="80" y="35" font-size="10" text-anchor="middle" fill="currentColor">already read</text>
<text x="210" y="35" font-size="10" text-anchor="middle" fill="currentColor">new bytes</text>
<text x="150" y="14" font-size="10" text-anchor="middle" fill="currentColor">cursor 4096</text>
<line x1="280" y1="31" x2="330" y2="31" stroke="currentColor" marker-end="url(#fstl-arrow)"/>
<text x="385" y="28" font-size="10" text-anchor="middle" fill="currentColor">bytes, cursor 5120</text>
<text x="385" y="42" font-size="10" text-anchor="middle" fill="currentColor">truncated false</text>
<rect x="10" y="74" width="100" height="22" fill="none" stroke="currentColor"/>
<text x="60" y="89" font-size="10" text-anchor="middle" fill="currentColor">file replaced, 1 KiB</text>
<line x1="280" y1="85" x2="330" y2="85" stroke="currentColor" marker-end="url(#fstl-arrow)"/>
<text x="385" y="82" font-size="10" text-anchor="middle" fill="currentColor">bytes from 0, cursor 1024</text>
<text x="385" y="96" font-size="10" text-anchor="middle" fill="currentColor">truncated true</text>
<text x="10" y="114" font-size="10" fill="currentColor">Cursor 5120 is past the new end, so the read restarts at 0.</text>
</svg>
<figcaption>A tail cursor follows growth. A shorter or replaced file reports truncated and restarts at 0.</figcaption>
</figure>

### Example: follow a log and react to each complete line

The plugin follows a log file when `fs-changed` arrives. It keeps the cursor and the unfinished last line.

**Grants:** `fs.read:project`, `fs.watch:project`

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::types::events::FsChangedEvent;

const LOG: &str = ".ilium/agent.log";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct LogFollower {
    watch: Option<u64>,
    cursor: u64,
    partial: Vec<u8>,
}

impl Lifecycle for LogFollower {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        let path = FsPath { scope: "project".into(), path: LOG.into() };
        self.watch = Some(fs::watch(&path, false)?);
        Ok(())
    }
}

impl Handler for LogFollower {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::FsChanged(FsChangedEvent { watch, .. }) = event else { continue };
            if self.watch != Some(watch) {
                continue;
            }
            self.drain();
        }
        Vec::new()
    }
}

impl LogFollower {
    fn drain(&mut self) {
        let path = FsPath { scope: "project".into(), path: LOG.into() };
        loop {
            let Ok(chunk) = fs::tail(&path, self.cursor, 65_536) else { return };
            if chunk.truncated {
                self.partial.clear();
            }
            self.cursor = chunk.cursor;
            let empty = chunk.bytes.is_empty();
            self.partial.extend_from_slice(&chunk.bytes);
            while let Some(end) = self.partial.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.partial.drain(..=end).collect();
                handle_line(&String::from_utf8_lossy(&line));
            }
            if empty {
                return;
            }
        }
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { tail, watch } from '@ilium/plugin-sdk/host/fs';

const LOG = { scope: 'project', path: '.ilium/agent.log' };
let logWatch: bigint | undefined;
let cursor = 0n;
let partial = '';
const decoder = new TextDecoder();

function drain(): void {
  for (;;) {
    const chunk = tail(LOG, cursor, 65536);
    if (chunk.truncated) {
      partial = '';
    }
    cursor = chunk.cursor;
    partial += decoder.decode(chunk.bytes, { stream: true });
    let end = partial.indexOf('\n');
    while (end >= 0) {
      handleLine(partial.slice(0, end));
      partial = partial.slice(end + 1);
      end = partial.indexOf('\n');
    }
    if (chunk.bytes.length === 0) {
      return;
    }
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    logWatch = watch(LOG, false);
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'fs-changed' && event.val.watch === logWatch) {
        drain();
      }
    }
    return [];
  },
});
```

`handle_line` and `handleLine` are the plugin's own code. A watch event says that something changed; it does not carry bytes, so `tail` is how the plugin gets them.

### Agent homes

The three scopes above let a plugin read what the Gemini CLI and Antigravity store, in the same way the Claude and Codex scopes work. Typical uses: list Gemini chats for a transcript view, read the auto-compact setting of an agent, or write a hook entry into an agent configuration. A write to a configuration file should use `write-atomic` with the revision read just before, so an edit by the agent itself is never overwritten.

**Rust**

```rust
let settings = FsPath { scope: "agent-config:gemini".into(), path: "settings.json".into() };
let current = fs::read_with_revision(&settings).await?;
let updated = add_hook(&current.bytes)?;
fs::write_atomic(&settings, updated, Some(current.revision)).await?;
```

**TypeScript**

```ts
const settings = { scope: 'agent-config:gemini', path: 'settings.json' };
const current = await readWithRevision(settings);
await writeAtomic(settings, addHook(current.bytes), current.revision);
```

Paths inside an agent scope are relative to the root the kernel resolved, so a Gemini chat is addressed as `<project-hash>/chats/<file>` under `agent-sessions:gemini`. Use `list-dir` or `scan` with a glob to find them.

### What the user sees

The grant dialog shows the new scopes in plain words. A high-risk grant is drawn in the warning style, and a write to an agent configuration is logged at info level like every write outside `data` and `cache`.

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.kanban --tail 4
<span class="t-dim">10:02:11</span> <span class="t-ok">info</span>  fs.write project .kanban/board.json replace
<span class="t-dim">10:02:13</span> <span class="t-warn">warn</span>  fs.write-atomic .kanban/board.json: conflict
<span class="t-dim">10:02:13</span> <span class="t-ok">info</span>  fs.write project .kanban/board.json replace
<span class="t-dim">10:02:40</span> <span class="t-ok">info</span>  fs.write agent-config:gemini settings.json
<span class="t-prompt">$</span> <span class="t-dim">4 lines shown, 0 dropped</span>
</pre>

Illustrative, not generated. The log of a plugin that lost one write race, retried on the new revision and then edited the Gemini configuration.

### Errors and limits

| Function | Sync codes | Completion codes |
|---|---|---|
| `read-with-revision` | `not-permitted`, `invalid-argument` | `not-found`, `quota-exceeded` (over `fs_read_all_mib`), `io` |
| `write-atomic` | `not-permitted`, `invalid-argument` (over `fs_write_mib`, read-only scope) | `conflict` (revision differs, or the file exists and `expected` is `none`), `not-found` (parent folder missing), `io` |
| `tail` | `not-permitted`, `not-found`, `invalid-argument` (`max-bytes` over 1 MiB), `io` | none, the call is sync |

Limits: `tail` returns at most 1 MiB per call; `read-with-revision` is bounded by `fs_read_all_mib`; `write-atomic` by `fs_write_mib` (16 MiB by default). The temporary file counts against `disk_mib` for `data` and `cache` until the rename.

### See also

- [fs](/pages/api-fs/) for scopes, lock, watch and picker.
- [Capability: fs.read](/pages/capability-fs-read/), [fs.write](/pages/capability-fs-write/) and [fs.watch](/pages/capability-fs-watch/) for the grants.
- [Requests and outcomes](/pages/requests-and-outcomes/) for how the async calls complete.
- [Events](/pages/api-events/) for `fs-changed`.
- [Errors and limits](/pages/reference-errors-and-limits/) for the shared codes.
