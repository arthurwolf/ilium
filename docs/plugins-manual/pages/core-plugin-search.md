---
layout: page
title: Workspace search
permalink: /pages/core-plugin-search/
---

## Workspace search

The Workspace search plugin (id `ilium.core.search`, sides: server and client) is the plugin form of Ilium's workspace search. One panel finds agents, terminals and files by name, by the text on their screens, and by meaning, and shows the results in groups. The server side keeps the index and scans files. The client side draws the panel.

Reading a terminal's screen is a kernel service, and the plugin uses it through the `terminal.read-screen` grant. The vector maths for search by meaning is done by the host, so the plugin ships no search code of its own.

### What it covers

- **Workspace search.** Agents, terminals and files, grouped in one tree. See Workspace search in the Panes and layout page of the user documentation.

### See it

<figure><img src="/assets/demos/10-workspace-search.gif" alt="The workspace search panel filters agents, terminals and files as the user types" loading="lazy"><figcaption>Workspace search: type a query, move through the grouped results and jump to a pane or file.</figcaption></figure>

### What the user sees

<figure><pre class="term">╭─ Workspace search ───────────────────────────╮
│ <span class="t-hi">Search</span> deploy▏                               │
│                                              │
│ <span class="t-hi">Agents</span>                                       │
│   ▸ claude: deploy script fix                │
│ <span class="t-hi">Terminals</span>                                    │
│     build shell                              │
│ <span class="t-hi">Files</span>                                        │
│     docs/deploy.md                           │
│     ops/deploy.sh                            │
│                                              │
╰──────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. The query field, the results grouped by kind in a tree, and the selected row marked with ▸.</figcaption></figure>
<figure><pre class="term">╭─ Workspace search ───────────────────────────╮
│ <span class="t-hi">Search</span> why did the build fail▏               │
│                                              │
│ <span class="t-hi">Meaning</span>                                      │
│   ▸ build shell                              │
│     <span class="t-ok">score 0.82</span>   <span class="t-dim">pane: build shell</span>           │
│     claude: fix linker error                 │
│     <span class="t-warn">score 0.64</span>   <span class="t-dim">pane: claude</span>                │
│                                              │
╰──────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. A hit found by meaning, with its cosine score and the pane it came from.</figcaption></figure>
### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.search",
  "name": "Search",
  "version": "0.1.0",
  "publisher": "Ilium",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["job-runner"]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "capabilities": {
    "server": ["inference.embed", "tree.read", "terminal.read-screen",
               "fs.read:project", "jobs.run"],
    "client": ["ui.panels"]
  },
  "contributes": {
    "commands": [{ "id": "open", "title": "Search the workspace", "category": "Search" }],
    "keybindings": [
      { "command": "ilium.core.search/open", "chord": "/", "mode": "prefix" }
    ],
    "panels": [{ "id": "search", "title": "Search", "modal": true }],
    "jobs": [{ "kind": "scan-files", "deadline_ms": 60000, "memory_mib": 128 }]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.search",
  "name": "Search",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Workspace search plugin (id ilium.core.search, sides: server and client) is the plugin form of Ilium's workspace search.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": [
      "job-runner"
    ]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": [
      "views"
    ]
  },
  "capabilities": {
    "server": [
      "inference.embed",
      "tree.read",
      "terminal.read-screen",
      "fs.read:project",
      "jobs.run"
    ],
    "client": [
      "ui.panels"
    ]
  },
  "activation": [
    "command:ilium.core.search/open",
    "view:search"
  ],
  "contributes": {
    "commands": [
      {
        "id": "open",
        "title": "Search the workspace",
        "category": "Search"
      }
    ],
    "keybindings": [
      {
        "command": "ilium.core.search/open",
        "chord": "/",
        "mode": "prefix"
      }
    ],
    "panels": [
      {
        "id": "search",
        "title": "Search",
        "modal": true
      }
    ],
    "jobs": [
      {
        "kind": "scan-files",
        "deadline_ms": 60000,
        "memory_mib": 128
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Samples

#### 1. The search panel

A command bound to a chord submits `open-view`. The panel shows a `text-input` and a `%list`. Choosing a hit jumps to it and submits `close-view`. See the [UI API](/pages/api-ui/), the [ui.panels grant](/pages/capability-ui-panels/), the [intents](/pages/api-intents/), [input widgets](/pages/widgets-input/) and [collection widgets](/pages/widgets-collections/). `jump_to` and `jumpTo` are the plugin's own helpers.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::OpenViewSpec;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue};
use ilium_plugin_sdk::ui::build::{ListItem, Tree};

fn panel() -> ViewTarget { ViewTarget::Panel("search".into()) }

impl Handler for SearchPanel {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for event in batch {
            match event {
                Event::CommandInvoked(c) if c.command == "ilium.core.search/open" => {
                    out.push(Submission {
                        intent: Intent::OpenView(OpenViewSpec { view: panel(), focus: true }),
                        correlation: 1,
                        dedup_key: None,
                    });
                }
                Event::UiAction(a) => match (a.key.as_str(), a.kind, a.value) {
                    ("query", ActionKind::Changed, ActionValue::Text(text)) => self.query = text,
                    ("hits", ActionKind::Activated, ActionValue::Index(i)) => {
                        self.jump_to(i as usize);
                        out.push(Submission {
                            intent: Intent::CloseView(panel()),
                            correlation: 2,
                            dedup_key: None,
                        });
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        out
    }
}

impl Views for SearchPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let query = tree.text_input("Search", self.query.clone()).key("query");
        let rows: Vec<ListItem> = self.hits.iter()
            .map(|hit| ListItem::new(vec![hit.title.clone()]).id(&hit.id).trailing(&hit.kind))
            .collect();
        let hits = tree.list(rows).key("hits");
        let body = tree
            .layout(Direction::Vertical, [Constraint::Length(3), Constraint::Fill(1)])
            .child(query).child(hits);
        tree.finish(body)
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { fill, layout, length, list, textInput, tree } from '@ilium/plugin-sdk/ui/build';

const PANEL = { tag: 'panel', val: 'search' } as const;
let query = '';
let hits: Array<{ id: string; title: string; kind: string }> = [];

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    const out: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'ilium.core.search/open') {
        out.push({ intent: { tag: 'open-view', val: { view: PANEL, focus: true } },
                   correlation: 1n, dedupKey: undefined });
      } else if (event.tag === 'ui-action') {
        const { key, kind, value } = event.val;
        if (key === 'query' && kind === 'changed' && value.tag === 'text') query = value.val;
        if (key === 'hits' && kind === 'activated' && value.tag === 'index') {
          jumpTo(value.val);
          out.push({ intent: { tag: 'close-view', val: PANEL }, correlation: 2n, dedupKey: undefined });
        }
      }
    }
    return out;
  },
  render() {
    const rows = hits.map((hit) => ({ id: hit.id, lines: [hit.title], trailing: hit.kind }));
    return tree(layout(
      { direction: 'vertical', constraints: [length(3), fill(1)] },
      [textInput('Search', query, { key: 'query' }), list(rows, { key: 'hits' })],
    ));
  },
});
```

#### 2. Search by meaning

The server embeds pane titles and screen snippets into a vector index, then answers a query by cosine similarity and returns the top 20 ids with scores. The host does the maths. See the [vector API](/pages/api-vector/) and the [inference.embed grant](/pages/capability-inference-embed/). An embed call takes at most 256 texts, so the samples below split the corpus into batches of 256 (`EMBED_BATCH`) and embed each batch in turn.

**Rust**

```rust
use ilium_plugin_sdk::host::vector;
use ilium_plugin_sdk::types::errors::PluginError;

const PURPOSE: &str = "workspace search";
/// `embed` accepts at most 256 texts per call.
const EMBED_BATCH: usize = 256;

pub struct MeaningIndex { index: u32 }

impl MeaningIndex {
    /// `items` holds (pane id, title and screen snippet).
    pub async fn build(items: &[(String, String)]) -> Result<MeaningIndex, PluginError> {
        let texts: Vec<String> = items.iter().map(|(_, text)| text.clone()).collect();
        let mut vectors: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        for batch in texts.chunks(EMBED_BATCH) {
            vectors.extend(vector::embed(batch, PURPOSE).await?);
        }
        let dimensions = vectors.first().map_or(0, |vector| vector.len());
        let index = vector::create_index("workspace", dimensions as u16)?;
        let entries: Vec<(String, Vec<f32>)> =
            items.iter().map(|(id, _)| id.clone()).zip(vectors).collect();
        vector::upsert(index, &entries)?;
        Ok(MeaningIndex { index })
    }

    pub async fn query(&self, text: &str) -> Result<Vec<(String, f32)>, PluginError> {
        let Some(first) = vector::embed(&[text.to_string()], PURPOSE).await?.into_iter().next()
        else { return Ok(Vec::new()) };
        vector::top_k(self.index, &first, 20)
    }
}
```

**TypeScript**

```ts
import { createIndex, embed, topK, upsert } from '@ilium/plugin-sdk/host/vector';

const PURPOSE = 'workspace search';
// `embed` accepts at most 256 texts per call.
const EMBED_BATCH = 256;

export async function buildIndex(items: Array<[string, string]>): Promise<number> {
  const texts = items.map(([, text]) => text);
  const vectors: number[][] = [];
  for (let start = 0; start < texts.length; start += EMBED_BATCH) {
    vectors.push(...(await embed(texts.slice(start, start + EMBED_BATCH), PURPOSE)));
  }
  const index = createIndex('workspace', vectors[0]?.length ?? 0);
  upsert(index, items.map(([id], position) => [id, vectors[position]] as [string, number[]]));
  return index;
}

export async function query(index: number, text: string): Promise<Array<[string, number]>> {
  const [vector] = await embed([text], PURPOSE);
  return vector === undefined ? [] : topK(index, vector, 20);
}
```

#### 3. Grouped results

Results sit in a `tree-list` with one parent row per kind (Agents, Terminals, Files). A `scrollbar` is bound to the list by its key. The plugin handles `ui-action` events for select and open. See [collection widgets](/pages/widgets-collections/), [layout widgets](/pages/widgets-layout-and-containers/) and the [event catalogue](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{ScrollbarOrientation, TreeItem};
use ilium_plugin_sdk::ui::build::Tree;

impl Views for SearchPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut items = Vec::new();
        for (group, hits) in &self.groups {
            items.push(TreeItem::new(group, group).depth(0).expandable(true));
            for hit in hits {
                items.push(TreeItem::new(&hit.id, &hit.title).depth(1));
            }
        }
        let mut tree = Tree::new();
        let results = tree.tree_list(items).key("results");
        let bar = tree.scrollbar(ScrollbarOrientation::VerticalRight, "results").key("results-bar");
        let row = tree
            .layout(Direction::Horizontal, [Constraint::Fill(1), Constraint::Length(1)])
            .child(results).child(bar);
        tree.finish(row)
    }
}
// In on_events: ("results", ActionKind::Activated, ActionValue::Text(id)) => self.open(&id),
//               ("results", ActionKind::Selected, ActionValue::Text(id)) => self.preview(&id),
```

**TypeScript**

```ts
import { fill, layout, length, scrollbar, tree, treeList } from '@ilium/plugin-sdk/ui/build';

const groups: Array<[string, Array<{ id: string; title: string }>]> = [];

export function render() {
  const items = groups.flatMap(([group, hits]) => [
    { id: group, label: group, depth: 0, expandable: true },
    ...hits.map((hit) => ({ id: hit.id, label: hit.title, depth: 1 })),
  ]);
  return tree(layout(
    { direction: 'horizontal', constraints: [fill(1), length(1)] },
    [
      treeList(items, { key: 'results' }),
      scrollbar('results', { orientation: 'vertical-right', key: 'results-bar' }),
    ],
  ));
}
// In onEvents: if (key === 'results' && kind === 'activated' && value.tag === 'text') open(value.val);
//              if (key === 'results' && kind === 'selected' && value.tag === 'text') preview(value.val);
```

#### 4. Scan files in a job

File names and contents are scanned off the event handler. The server lists the project with `fs.scan`, starts a `scan-files` job with the paths, and follows `job-progress`. See the [jobs API](/pages/api-jobs/), the [file API](/pages/api-fs/) and the [event catalogue](/pages/api-events/). `indexFiles` is the plugin's own function, and the Rust job runner mirrors the one in the Compaction plugin.

**Rust**

```rust
use ilium_plugin_sdk::host::{fs, jobs};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::requests::RequestResult;

fn project(path: &str) -> FsPath { FsPath { scope: "project".into(), path: path.into() } }

impl Handler for Scanner {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(c) if c.command == "ilium.core.search/rescan" => {
                    self.scan = fs::scan(&project(""), "**/*", 4096).ok();
                }
                Event::RequestCompleted(RequestResult { request: RequestRef::Call(id), .. })
                    if self.scan == Some(id) =>
                {
                    let paths: Vec<String> = fs::take_entries(id)
                        .unwrap_or_default().into_iter().map(|(path, _)| path).collect();
                    self.scan = None;
                    self.job = jobs::start_request("scan-files", paths.join("\n").as_bytes()).ok();
                }
                Event::JobProgress(p) if self.job == Some(p.request_id) => {
                    self.percent = p.percent; // batches stream back as the job runs
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { scan } from '@ilium/plugin-sdk/host/fs';
import { startRequest } from '@ilium/plugin-sdk/host/jobs';

const project = (path: string) => ({ scope: 'project', path });
let job: bigint | undefined;
let percent = 0;

export const { lifecycle, handler, jobRunner } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'ilium.core.search/rescan') {
        void scan(project(''), '**/*', 4096).then((entries) => {
          const paths = entries.map(([path]) => path).join('\n');
          job = startRequest('scan-files', new TextEncoder().encode(paths));
        });
      } else if (event.tag === 'job-progress' && event.val.requestId === job) {
        percent = event.val.percent; // batches stream back as the job runs
      }
    }
    return [];
  },
  run(_kind: string, input: Uint8Array): Uint8Array {
    return indexFiles(new TextDecoder().decode(input).split('\n')); // runs in the job
  },
});
```

#### 5. Refresh results on filter change

The filter field sends `filter-changed` with the text, debounced to 150 ms by the kernel. The plugin reruns the query only when the text differs from the last run, so a quick retype does not start a second scan.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue};

impl Handler for LiveFilter {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if let (ActionKind::FilterChanged, ActionValue::Text(text)) = (action.kind, action.value) {
                if text == self.last_query { continue; }
                self.last_query = text.clone();
                return vec![self.query_submission(text)];
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
export function onAction(event: Extract<Event, { tag: 'ui-action' }>): Submission[] {
  const { kind, value } = event.val;
  if (kind !== 'filter-changed' || value.tag !== 'text') return [];
  if (value.val === lastQuery) return [];
  lastQuery = value.val;
  return [querySubmission(value.val)];
}
```

### Extension points

Other plugins change what `ilium.core.search` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.search/<point>`). Contributors need `extensions.contribute:ilium.core.search/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `sources` | collect | Add a source of results, such as issues or notes. | entry: id, title, kind, service (answers a query with ranked hits) | standard |
| `rank` | hook (transform) | Change the order of results before they are shown. | query, hits (id, kind, score); reply: hits | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.search/query`: query: search all sources

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.search"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Adds issues as a result kind

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.search/sources"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.search/sources",
        "id": "issues",
        "priority": 50,
        "entry": {
          "title": "Issues",
          "kind": "issue",
          "service": "example.issues/search"
        }
      }
    ]
  }
}
```

### Extension points this plugin owns

The plugin owns the two points below (grant `extensions.provide:ilium.core.search/<point>`). The manifest above declares no `extensions.provide` grant and the page does not name a side for either point, so the side is unknown. Values marked (default) are the defaults of [Extension points](/pages/api-extension-points/), because this page does not state them.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.search/sources` | server | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |
| `ilium.core.search/rank` | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) |

Side basis: `ilium.core.search/sources` is server because the manifest declares no `extensions.provide` grant and names no side for the point, and the page describes a source entry's `service` as the handler that answers a query with ranked hits, while the server keeps the index and answers queries (Samples 2 and Services).

Side basis: `ilium.core.search/rank` is server because the manifest declares no grant and the page names no handler side, so the side where the calling logic runs decides it, and the server answers the query and holds the hits before the client panel shows them.

### ABI used

- [UI API](/pages/api-ui/) and [ui.panels](/pages/capability-ui-panels/)
- [Intents](/pages/api-intents/)
- [Vector API](/pages/api-vector/) and [inference.embed](/pages/capability-inference-embed/)
- [Jobs](/pages/api-jobs/) and [file API](/pages/api-fs/)
- [Events](/pages/api-events/)
- [Input widgets](/pages/widgets-input/), [collection widgets](/pages/widgets-collections/) and [layout widgets](/pages/widgets-layout-and-containers/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Compaction plugin](/pages/core-plugin-compaction/), which shows the job runner in full
