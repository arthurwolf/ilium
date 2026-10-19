# 18. Offer an extension point: a link preview plugin

## 18. Offer an extension point: a link preview plugin

This step makes your plugin the owner of an extension point. The plugin is `example.linkpreview`. When the user runs a command on a URL, the plugin builds a short preview. It does not know every site. It owns a **collect** point, `renderers`, where other packages list a URL pattern and the service that can render it. It also owns a **transform hook**, `before-preview`, where other packages can adjust the finished preview. A second package, `example.linkpreview-github`, fills the point for GitHub URLs. The step shows the dormant state of that package before the owner is installed.

You start a fresh plugin. You need tutorials 1 to 9, and you should have read [Services](/pages/services/) and [Architecture](/pages/architecture/). The full mechanism is in [Extension points](/pages/api-extension-points/). The points of the core plugins are in [Reference: core extension points](/pages/reference-core-extension-points/), and [Extending core plugins](/pages/extending-core-plugins/) compares the five ways to work with a core feature. This page does not repeat those tables.

### You will need

- Rust: stable 1.90 or later with the `wasm32-wasip2` target, and the crates `ilium-plugin-sdk`, `serde_json` and `regex-lite`. TypeScript: Bun 1.2 or later and `@ilium/plugin-sdk`. `RegExp` is built in.
- Grants for the owner: `extensions.provide:example.linkpreview/renderers`, `extensions.provide:example.linkpreview/before-preview`, `services.use:example.linkpreview-github/render` and `topics.subscribe:ilium.extensions/changed`. Nothing else.
- Grants for the contributor: `extensions.contribute:example.linkpreview/renderers` and `services.provide:example.linkpreview-github/render`.
- Both manifests set `"unstable": ["plugin-services"]` (dev mode, T2 and T3), because the renderer is a plugin-defined service.

### The surface this step uses

| Area | Name | Meaning |
|---|---|---|
| Manifest | `contributes.extension_points[]` | Declares `renderers` (collect, dynamic) and `before-preview` (hook, transform). The global ids are `example.linkpreview/renderers` and `example.linkpreview/before-preview`. |
| Manifest | `contributes.extensions[]` | The contributor writes `point`, `id`, `priority` and `entry`. |
| Grant | `extensions.provide:<point>` | Owner of one point. It may only own points that start with its own plugin id. |
| Grant | `extensions.contribute:<point>` | Contributor to one point. No wildcard. |
| Host | `extensions::list(point)` / `list(point)` | The active entries, in run order. |
| Host | `extensions::run_hook(point, payload).await` / `runHook(point, payload)` | Runs the chain and resolves with the final payload. |
| Host | `extensions::register` and `unregister` | Add or remove an entry of a `dynamic` point at run time. |
| Topic | `ilium.extensions/changed` | Tells the owner, and nobody else, that the list of a point changed. Payload `{ "point": "<id>" }`. |

### The flow

<figure>
<svg viewBox="0 0 480 330" role="img" aria-label="Sequence of a link preview with one contributed renderer">
  <title>Link preview sequence</title>
  <desc>The user runs the preview command. The owner lists the renderers in the registry, matches the URL, calls the renderer service of the contributor, runs the before-preview hook and shows the result.</desc>
  <defs>
    <marker id="t18-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="80" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="50" y="25" font-size="11" text-anchor="middle" fill="currentColor">User</text>
  <rect x="140" y="8" width="90" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="185" y="25" font-size="11" text-anchor="middle" fill="currentColor">Owner</text>
  <rect x="270" y="8" width="90" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="315" y="25" font-size="11" text-anchor="middle" fill="currentColor">Registry</text>
  <rect x="390" y="8" width="84" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="432" y="25" font-size="11" text-anchor="middle" fill="currentColor">GitHub pkg</text>
  <line x1="50" y1="34" x2="50" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="185" y1="34" x2="185" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="315" y1="34" x2="315" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="432" y1="34" x2="432" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="50" y1="58" x2="185" y2="58" stroke="currentColor" marker-end="url(#t18-arrow)"/>
  <text x="118" y="52" font-size="10" text-anchor="middle" fill="currentColor">preview command</text>
  <line x1="185" y1="88" x2="315" y2="88" stroke="currentColor" marker-end="url(#t18-arrow)"/>
  <text x="250" y="82" font-size="10" text-anchor="middle" fill="currentColor">list(renderers)</text>
  <line x1="315" y1="112" x2="185" y2="112" stroke="currentColor" marker-end="url(#t18-arrow)"/>
  <text x="250" y="106" font-size="10" text-anchor="middle" fill="currentColor">entries in run order</text>
  <line x1="185" y1="146" x2="432" y2="146" stroke="currentColor" marker-end="url(#t18-arrow)"/>
  <text x="308" y="140" font-size="10" text-anchor="middle" fill="currentColor">call render (service, 30 s cap)</text>
  <line x1="432" y1="172" x2="185" y2="172" stroke="currentColor" marker-end="url(#t18-arrow)"/>
  <text x="308" y="166" font-size="10" text-anchor="middle" fill="currentColor">title and summary</text>
  <line x1="185" y1="206" x2="315" y2="206" stroke="currentColor" marker-end="url(#t18-arrow)"/>
  <text x="250" y="200" font-size="10" text-anchor="middle" fill="currentColor">run-hook(before-preview)</text>
  <line x1="315" y1="230" x2="185" y2="230" stroke="currentColor" marker-end="url(#t18-arrow)"/>
  <text x="250" y="224" font-size="10" text-anchor="middle" fill="currentColor">final payload</text>
  <line x1="185" y1="268" x2="50" y2="268" stroke="currentColor" marker-end="url(#t18-arrow)"/>
  <text x="118" y="262" font-size="10" text-anchor="middle" fill="currentColor">toast with the preview</text>
  <line x1="315" y1="304" x2="185" y2="304" stroke="currentColor" stroke-dasharray="5 3" marker-end="url(#t18-arrow)"/>
  <text x="250" y="298" font-size="10" text-anchor="middle" fill="currentColor">changed (when a list changes)</text>
</svg>
<figcaption>One preview. The dashed arrow is the kernel topic <code>ilium.extensions/changed</code>, sent to the owner only. The owner then calls <code>list</code> again.</figcaption>
</figure>

### Step 1: the owner manifest

The owner declares both points. `renderers` is a collect point, so it has an `entry_schema`. It is `dynamic`, so a contributor may also add entries at run time. `before-preview` is a hook with the mode `transform`, so a handler may replace the payload.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.linkpreview",
  "name": "Link preview",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "icon": "link",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "unstable": ["plugin-services"],
  "capabilities": {
    "server": [
      "extensions.provide:example.linkpreview/renderers",
      "extensions.provide:example.linkpreview/before-preview",
      "services.use:example.linkpreview-github/render",
      "topics.subscribe:ilium.extensions/changed",
      "notify.toast"
    ]
  },
  "activation": ["command:example.linkpreview/preview"],
  "contributes": {
    "commands": [
      { "id": "preview", "title": "Link preview: show preview of a URL" }
    ],
    "topics": { "subscribes": ["ilium.extensions/changed"] },
    "extension_points": [
      {
        "id": "renderers",
        "kind": "collect",
        "title": "Link renderers",
        "version": "1.0.0",
        "dynamic": true,
        "max_entries": 32,
        "entry_schema": {
          "type": "object",
          "required": ["id", "regex", "service"],
          "properties": {
            "id": { "type": "string", "maxLength": 64 },
            "regex": { "type": "string", "maxLength": 256 },
            "service": { "type": "string", "maxLength": 128 }
          }
        }
      },
      {
        "id": "before-preview",
        "kind": "hook",
        "mode": "transform",
        "title": "Before a preview is shown",
        "version": "1.0.0",
        "risk": "standard",
        "deadline_ms": 20,
        "max_handlers": 4,
        "payload_schema": {
          "type": "object",
          "required": ["url", "title", "summary"],
          "properties": {
            "url": { "type": "string", "maxLength": 2048 },
            "title": { "type": "string", "maxLength": 200 },
            "summary": { "type": "string", "maxLength": 600 }
          }
        },
        "reply_schema": {
          "type": "object",
          "required": ["title", "summary"],
          "properties": {
            "title": { "type": "string", "maxLength": 200 },
            "summary": { "type": "string", "maxLength": 600 }
          }
        }
      }
    ]
  }
}
```

Points to notice:

- The global ids are the plugin id, a slash and the point id: `example.linkpreview/renderers`. Each `extensions.provide` grant names one of them. A grant for a point that starts with another plugin's id is refused at install with `manifest.grant-missing-extension`, and a point without a grant fails the same way.
- The owner does not list `hook-handler` in `exports`. It runs hooks and does not handle them.
- `version` is the version of the point's schema. Bump the minor number when you add an optional field. Bump the major number when you remove a field or make a field required. Contributors can ask for a minimum, as step 3 shows.
- `max_entries` caps the whole list at 32. A 33rd entry is refused with `cap-exceeded`. The default is 64 and the limit is 256.
- The owner grants itself `services.use` for exactly one renderer service. An entry that names another service is listed, but the call fails with `not-permitted`. Step 2 turns that into a plain preview.

### Step 2: the owner code

The owner has four jobs. It reads the list on start. It reads the list again when `ilium.extensions/changed` arrives for `renderers`. On the command it finds the first entry whose pattern matches, calls that entry's service, and runs the hook. If there is no match, no renderer or a failing renderer, it falls back to a plain preview from the URL. The preview never fails because a contributor is bad.

**Rust**

```rust
// src/lib.rs
use ilium_plugin_sdk::host::extensions;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::services;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;
use regex_lite::Regex;
use serde_json::{json, Value};

const RENDERERS: &str = "example.linkpreview/renderers";
const BEFORE_PREVIEW: &str = "example.linkpreview/before-preview";

#[derive(Clone)]
struct Renderer {
    extension: String,
    regex: Regex,
    service: String,
}

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct LinkPreview {
    renderers: Vec<Renderer>,
}

impl LinkPreview {
    fn reload(&mut self) {
        let entries = match extensions::list(RENDERERS) {
            Ok(entries) => entries,
            Err(error) => {
                log::log(Level::Warn, &format!("list failed: {error:?}"));
                return;
            }
        };
        self.renderers = entries
            .into_iter()
            .filter_map(|entry| {
                let value: Value = serde_json::from_str(&entry.entry_json).ok()?;
                // A bad pattern drops this entry only. The others stay usable.
                let regex = Regex::new(value["regex"].as_str()?).ok()?;
                Some(Renderer {
                    extension: entry.extension,
                    regex,
                    service: value["service"].as_str()?.to_string(),
                })
            })
            .collect();
        log::log(Level::Info, &format!("{} renderers loaded", self.renderers.len()));
    }
}

impl Lifecycle for LinkPreview {
    fn activate(&mut self) {
        self.reload();
    }
}

impl Handler for LinkPreview {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::TopicPublished(published) if published.topic == "ilium.extensions/changed" => {
                    let changed: Value = serde_json::from_str(&published.payload_json).unwrap_or(Value::Null);
                    if changed["point"] == RENDERERS {
                        self.reload();
                    }
                }
                Event::CommandInvoked(invoked) if invoked.command == "example.linkpreview/preview" => {
                    let arguments: Value = serde_json::from_str(&invoked.arguments_json).unwrap_or(Value::Null);
                    let url = arguments["url"].as_str().unwrap_or("").to_string();
                    task::spawn(build_preview(url, self.renderers.clone()));
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

async fn build_preview(url: String, renderers: Vec<Renderer>) {
    let mut preview = plain_preview(&url);
    if let Some(renderer) = renderers.iter().find(|renderer| renderer.regex.is_match(&url)) {
        match services::call(&renderer.service, "render", &json!({ "url": url }).to_string()).await {
            Ok(reply) => match parse_preview(&reply) {
                Some(rendered) => preview = rendered,
                None => log::log(Level::Warn, &format!("{} sent a bad reply", renderer.extension)),
            },
            Err(error) => log::log(Level::Warn, &format!("{} failed: {}", renderer.extension, error.code)),
        }
    }
    let payload = json!({ "url": url, "title": preview.0, "summary": preview.1 }).to_string();
    match extensions::run_hook(BEFORE_PREVIEW, &payload).await {
        Ok(Some(changed)) => {
            if let Some(adjusted) = parse_preview(&changed) {
                preview = adjusted;
            }
        }
        Ok(None) => {}
        Err(error) => log::log(Level::Warn, &format!("hook failed: {}", error.code)),
    }
    log::log(Level::Info, &format!("preview ready: {}", preview.0));
    // The toast submission is returned by the next batch in a full plugin.
}

fn parse_preview(json_text: &str) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(json_text).ok()?;
    Some((value["title"].as_str()?.to_string(), value["summary"].as_str()?.to_string()))
}

fn plain_preview(url: &str) -> (String, String) {
    let host = url.split('/').nth(2).unwrap_or(url);
    (host.to_string(), format!("No renderer for {host}."))
}
```

**TypeScript**

```ts
// src/index.ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { list, runHook } from '@ilium/plugin-sdk/host/extensions';
import { call } from '@ilium/plugin-sdk/host/services';
import { log } from '@ilium/plugin-sdk/host/log';

const RENDERERS = 'example.linkpreview/renderers';
const BEFORE_PREVIEW = 'example.linkpreview/before-preview';

interface Renderer { extension: string; regex: RegExp; service: string }
type Preview = { title: string; summary: string };

let renderers: Renderer[] = [];

function reload(): void {
  try {
    renderers = list(RENDERERS).flatMap((entry) => {
      try {
        const value = JSON.parse(entry.entryJson) as { regex: string; service: string };
        // A bad pattern drops this entry only. The others stay usable.
        return [{ extension: entry.extension, regex: new RegExp(value.regex), service: value.service }];
      } catch {
        return [];
      }
    });
    log('info', `${renderers.length} renderers loaded`);
  } catch (error) {
    log('warn', `list failed: ${String(error)}`);
  }
}

function plainPreview(url: string): Preview {
  const host = url.split('/')[2] ?? url;
  return { title: host, summary: `No renderer for ${host}.` };
}

function parsePreview(text: string): Preview | undefined {
  try {
    const value = JSON.parse(text) as Partial<Preview>;
    if (typeof value.title !== 'string' || typeof value.summary !== 'string') return undefined;
    return { title: value.title, summary: value.summary };
  } catch {
    return undefined;
  }
}

async function buildPreview(url: string, active: Renderer[]): Promise<void> {
  let preview = plainPreview(url);
  const renderer = active.find((candidate) => candidate.regex.test(url));
  if (renderer) {
    try {
      const reply = await call(renderer.service, 'render', JSON.stringify({ url }));
      const rendered = parsePreview(reply);
      if (rendered) preview = rendered;
      else log('warn', `${renderer.extension} sent a bad reply`);
    } catch (error) {
      log('warn', `${renderer.extension} failed: ${String((error as { code?: string }).code)}`);
    }
  }
  try {
    const changed = await runHook(BEFORE_PREVIEW, JSON.stringify({ url, ...preview }));
    const adjusted = changed === undefined ? undefined : parsePreview(changed);
    if (adjusted) preview = adjusted;
  } catch (error) {
    log('warn', `hook failed: ${String((error as { code?: string }).code)}`);
  }
  log('info', `preview ready: ${preview.title}`);
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    reload();
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'topic-published' && event.val.topic === 'ilium.extensions/changed') {
        const changed = JSON.parse(event.val.payloadJson) as { point?: string };
        if (changed.point === RENDERERS) reload();
      }
      if (event.tag === 'command-invoked' && event.val.command === 'example.linkpreview/preview') {
        const args = JSON.parse(event.val.argumentsJson) as { url?: string };
        void buildPreview(args.url ?? '', renderers);
      }
    }
    return [];
  },
});
```

Points to notice:

- `list` returns entries already sorted by the user's order, then priority, then plugin id. Take the first match.
- The owner keeps the compiled list and reloads on the topic. The topic carries only the point id, so the owner reloads the whole list.
- A contributor is untrusted input. The kernel checks `reply_schema` for the hook, but the owner parses the service reply itself.
- With no handler, `run_hook` returns nothing and the owner keeps its own preview. A full plugin returns an `Intent::Toast` from the next batch ([tutorial 4](/pages/tutorial-04-finished-notice/)).

### Step 3: the contributor package

The second package knows GitHub. It fills the point from its manifest, so it needs no code to register. Its service answers the `render` calls. It can also ask for a minimum version of the point: `example.linkpreview/renderers@1.0.0` means the same major version and a minor version of 0 or more.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.linkpreview-github",
  "name": "Link preview: GitHub",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "icon": "brand-github",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["service-provider"]
  },
  "unstable": ["plugin-services"],
  "capabilities": {
    "server": [
      "extensions.contribute:example.linkpreview/renderers",
      "services.provide:example.linkpreview-github/render"
    ]
  },
  "contributes": {
    "services": {
      "provides": [
        {
          "name": "example.linkpreview-github/render",
          "version": "1.0.0",
          "schema": "schemas/render.json"
        }
      ]
    },
    "extensions": [
      {
        "point": "example.linkpreview/renderers@1.0.0",
        "id": "github",
        "priority": 40,
        "entry": {
          "id": "github",
          "regex": "^https://github\\.com/[^/]+/[^/]+",
          "service": "example.linkpreview-github/render"
        }
      }
    ]
  }
}
```

`schemas/render.json` (params `{ "url": string }`, result `{ "title": string, "summary": string }`, each field with a `maxLength`):

```json
{ "methods": { "render": {
  "params": { "type": "object", "required": ["url"], "properties": { "url": { "type": "string", "maxLength": 2048 } } },
  "result": { "type": "object", "required": ["title", "summary"], "properties": {
    "title": { "type": "string", "maxLength": 200 }, "summary": { "type": "string", "maxLength": 600 } } }
} } }
```

The provider code reads the owner and the repository from the path. It does no network call, so it needs no `http` grant. A real renderer would add `http:api.github.com`, which is a separate grant that the user would see in the review dialog.

**Rust**

```rust
// src/lib.rs
use ilium_plugin_sdk::exports::{ServiceCall, ServiceProvider, ServiceReply};
use ilium_plugin_sdk::prelude::*;
use serde_json::{json, Value};

#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
struct GithubRenderer;

impl Lifecycle for GithubRenderer {}

impl Handler for GithubRenderer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl ServiceProvider for GithubRenderer {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        if call.method != "render" {
            return ServiceReply::Done(json!({ "title": "", "summary": "" }).to_string());
        }
        let params: Value = serde_json::from_str(&call.payload_json).unwrap_or(Value::Null);
        let url = params["url"].as_str().unwrap_or("");
        let mut parts = url.trim_start_matches("https://github.com/").split('/');
        let owner = parts.next().unwrap_or("");
        let repository = parts.next().unwrap_or("");
        ServiceReply::Done(
            json!({
                "title": format!("{owner}/{repository}"),
                "summary": format!("GitHub repository {repository} owned by {owner}."),
            })
            .to_string(),
        )
    }
}
```

**TypeScript**

```ts
// src/index.ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { ServiceCall, ServiceReply } from '@ilium/plugin-sdk';

export const { lifecycle, handler, serviceProvider } = definePlugin({
  side: 'server',
  onEvents() {
    return [];
  },
  handle(call: ServiceCall): ServiceReply {
    if (call.method !== 'render') {
      return { tag: 'done', val: JSON.stringify({ title: '', summary: '' }) };
    }
    const params = JSON.parse(call.payloadJson) as { url?: string };
    const [owner = '', repository = ''] = (params.url ?? '')
      .replace('https://github.com/', '')
      .split('/');
    return {
      tag: 'done',
      val: JSON.stringify({
        title: `${owner}/${repository}`,
        summary: `GitHub repository ${repository} owned by ${owner}.`,
      }),
    };
  },
});
```

Points to notice:

- The manifest is the whole contribution. The entry is checked against the owner's `entry_schema` when the package is installed. A missing `service` field fails with `manifest.extension-entry-invalid`, and a missing grant with `manifest.grant-missing-extension`.
- `priority` 40 puts GitHub before the default 50. Lower runs first, and the user can still move it on the owner's Extensions tab.
- A missing `service` field fails the install with `manifest.extension-entry-invalid`.

### Step 4: dormant until the owner is installed

Install the contributor first, with the owner missing. The install does not fail. The entry stays in the package and counts for nothing. The Plugins screen shows `waiting for example.linkpreview`. Install the owner second, and the entry becomes active. The kernel sends `ilium.extensions/changed` with `{ "point": "example.linkpreview/renderers" }` to the owner. The owner reloads and shows `1 renderers loaded` in its log.

The same happens when the owner is disabled: the entries go dormant at once and a preview falls back to plain text. An entry that asks for `renderers@1.2.0` from an owner at `1.0.0` is dormant too, and the Plugins screen names the version the owner must reach.

### Step 5: dynamic entries with register and unregister

Because `renderers` is `dynamic`, a contributor may add and remove entries while it runs. This is how a plugin offers a renderer for something it only learns at run time, such as the host name of a company wiki read from its settings. The contributor needs the same `extensions.contribute` grant. The entry is validated again against the schema.

**Rust**

```rust
use ilium_plugin_sdk::host::extensions;

fn add_wiki(host: &str) -> Result<(), String> {
    let regex = host.replace('.', "\\.");
    let entry = format!(
        r#"{{"id":"wiki","regex":"^https://{regex}/","service":"example.linkpreview-github/render"}}"#
    );
    extensions::register("example.linkpreview/renderers", "wiki", 60, &entry)
        .map_err(|error| format!("{error:?}"))
}

// Remove it later: extensions::unregister("example.linkpreview/renderers", "wiki")
```

**TypeScript**

```ts
import { register } from '@ilium/plugin-sdk/host/extensions';

export function addWiki(host: string): void {
  const regex = host.replace(/\./g, '\\.');
  const entry = JSON.stringify({
    id: 'wiki',
    regex: `^https://${regex}/`,
    service: 'example.linkpreview-github/render',
  });
  register('example.linkpreview/renderers', 'wiki', 60, entry);
}

// Remove it later: unregister('example.linkpreview/renderers', 'wiki')
```

Registering an id the plugin already used replaces that entry. A plugin can only remove entries it registered, and a call on a point that is not `dynamic` fails with `not-dynamic`. A registered entry does not survive a restart of the contributor, so register again in `activate`. A dynamic entry has the same cap, the same priority rules and the same toggles as a manifest entry.

### Step 6: change the schema without breaking contributors

Point versions let you grow the schema. Suppose you want to add an optional `icon` field to renderer entries.

1. Add `icon` to `properties`, not to `required`, and set `"version": "1.1.0"`. Contributors that wrote `renderers@1.0.0` stay active and the owner uses a default icon.
2. Contributors that need the field write `renderers@1.1.0`. On an owner still at `1.0.0` they are dormant, which is correct.
3. To remove a field or require one, bump to `2.0.0`. Every contributor that asked for `@1.0.0` goes dormant.

A contributor that does not write a version always matches. Tell contributors to write one.

### Security and consent

| Grant | Who | Risk | Why |
|---|---|---|---|
| `extensions.provide:example.linkpreview/renderers` | Owner | normal | The plugin owns the point. |
| `extensions.provide:example.linkpreview/before-preview` | Owner | normal | The plugin owns the hook. The point has `risk: "standard"`. |
| `extensions.contribute:example.linkpreview/renderers` | GitHub package | normal | The package adds one entry. |
| `services.use:example.linkpreview-github/render` | Owner | normal | The owner calls one named service. |
| `services.provide:example.linkpreview-github/render` | GitHub package | normal | The package answers that service. |
| `topics.subscribe:ilium.extensions/changed` | Owner | normal | The owner hears about list changes. |

- A contributor is untrusted input. It can name any service in its entry. The owner can call only services it holds a `services.use` grant for, so an entry with a different service fails with `not-permitted` and the owner falls back.
- A contributor cannot see the owner's state. A handler sees the payload and nothing else.
- Mark a hook `high` only when a handler could do harm. A preview carries no secret, so `before-preview` stays standard.

### Test it

Test the manifests first. Remove `"service"` from the GitHub entry and run `ilium plugin build`. It reports `manifest.extension-entry-invalid`. Restore it, then remove the `extensions.contribute` grant to see `manifest.grant-missing-extension`.

Then run the unit tests. The [Testing](/pages/testing/) page documents the fake host. It delivers events and records the calls. Use `FakeSession` to run the owner and a stub contributor together, so the test covers the real `list` and `run-hook` path. Use a fixture for host data. Test the pure parts directly, as plain functions. See [Test harness reference](/pages/reference-test-harness/).

**Rust**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_preview_names_the_host() {
        let preview = plain_preview("https://example.org/a/b");
        assert_eq!(preview.0, "example.org");
    }

    #[test]
    fn bad_service_reply_is_ignored() {
        assert!(parse_preview("not json").is_none());
        assert!(parse_preview(r#"{"title":"a","summary":"b"}"#).is_some());
    }
}
```

**TypeScript**

```ts
import { expect, test } from 'bun:test';
import { parsePreview, plainPreview } from './index';

test('plain preview names the host', () => {
  expect(plainPreview('https://example.org/a/b').title).toBe('example.org');
});

test('bad service reply is ignored', () => {
  expect(parsePreview('not json')).toBeUndefined();
  expect(parsePreview('{"title":"a","summary":"b"}')).toBeDefined();
});
```

Export `parsePreview` and `plainPreview` for the test. Then test the whole flow in the app, using the steps from [Debugging](/pages/debugging/):

1. Pack and install the contributor first. Open Settings > Plugins and check that it says `waiting for example.linkpreview`.
2. Install the owner. Run `ilium plugin logs example.linkpreview --tail 5`. It must show `1 renderers loaded`.
3. Run the command with a GitHub URL, then with another URL. The log must show `preview ready:` with the repository, then with the host name.
4. Disable the GitHub package. The owner log must show `0 renderers loaded`.

### What you see

The review dialog of the contributor groups its contribution under **Adds to other plugins**. The owner is named, even though it is not installed yet.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Review: example.linkpreview-github 0.1.0     │
│                                              │
│ Adds to other plugins                        │
│   Link preview > Link renderers              │
│     adds a link pattern and its renderer     │
│                                              │
│ Grants                                       │
│   [x] extensions.contribute: link renderers  │
│   [x] services.provide: render               │
│                                              │
│   [allow]  [decline]                         │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The dialog for the contributor. Both grants are normal risk, so both are checked.</figcaption>
</figure>

On the Plugins screen the entry shows `waiting for example.linkpreview` until the owner is installed.

### What can go wrong

| Code or symptom | Cause | Fix |
|---|---|---|
| `manifest.extension-point-invalid`, `manifest.extension-point-duplicate` | A bad id, kind, mode or schema keyword, or two points with one id. | Fix the point entry. |
| `manifest.grant-missing-extension`, `manifest.grant-wildcard` | A point or extension has no matching grant, or the grant ends in `*`. | Add the exact grant. |
| `manifest.extension-entry-invalid` | The entry fails the owner's `entry_schema`. | Match the schema. |
| `extension.schema-mismatch` on `run-hook` | The payload fails `payload_schema`. | Trim title and summary. |
| `extension.cap-exceeded`, `extension.not-dynamic` | Over `max_entries`, or `register` on a static point. | Raise the cap, or set `dynamic`. |
| The renderer never runs | The entry is dormant, disabled, or sorted below another match. | Check the Extensions tab. |
| `not-permitted` from the service | The owner has no `services.use` for the entry's service. | Add the grant, or ignore that renderer. |

### See also

- [Extension points](/pages/api-extension-points/) for the full mechanism, the functions and the error codes.
- [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.
- [Reference: core extension points](/pages/reference-core-extension-points/) for the points the core plugins own.
- [Services](/pages/services/) and [services API](/pages/api-services/) for the renderer service.
- [Tutorial 19: stop core actions with hooks](/pages/tutorial-19-stop-core-actions-with-hooks/) for the handler side of a hook.
- [Tutorial 22: a versioned service with fallbacks](/pages/tutorial-22-versioned-service-with-fallbacks/) for service versions.
- [Testing](/pages/testing/), [Debugging](/pages/debugging/) and [Manifest reference](/pages/reference-manifest/).

API checked against `ilium:plugin@0.1.0`.
