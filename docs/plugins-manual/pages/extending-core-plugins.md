---
layout: page
title: Extending core plugins
permalink: /pages/extending-core-plugins/
---

## Extending core plugins

Many features of Ilium are core plugins: titles, cost, fleet, notifications, search and more. [Core plugins](/pages/core-plugins/) lists them. This page answers one question: how does another plugin control, change or build on one of those features? It also answers a second one: how does a plugin let others build on it?

The short answer is that there are five ways, from light to heavy, and every core plugin supports the first four in the same way. The fifth is available for all core plugins except two. All five use the public contract `ilium:plugin@0.1.0`. A third-party plugin gets nothing less than a core plugin gets.

### What you need

- **A grant for each way.** Observing needs a topic grant. Driving needs a service or command grant. Extending needs [extensions.contribute](/pages/capability-extensions-contribute/). Offering needs [extensions.provide](/pages/capability-extensions-provide/). Replacing needs no grant of its own to take the place of a core plugin, and the user confirms it. A replacement still declares the `services.provide` and `extensions.provide` grants for the services and extension points it re-offers, as the manifest example below shows.
- **The page of the core plugin.** Every core plugin page has an **Extension points** section that lists its points, services and topics. [Reference: core extension points](/pages/reference-core-extension-points/) lists all of them in one place.
- **An SDK.** Rust: stable 1.90 or later with the `wasm32-wasip2` target and the `ilium-plugin-sdk` crate. TypeScript: Bun 1.2 or later and `@ilium/plugin-sdk`.

### The dogfooding rule

Core plugins are ordinary plugins. They are bundled, they have an id that starts with `ilium.`, and they are first-party (tier T1). They use only the public contract. They have no private API and no hidden hooks. Everything a core plugin can do, a third-party plugin can do with the same grants.

Two things follow from this rule.

- **If a core plugin lets you change something, it says so.** The lever is a service, a topic, a command, a pane kind or an extension point. It is documented on its page.
- **If you need a lever that does not exist, ask for a new point, not a private path.** A new point is a normal change to the core plugin's manifest. It gets a version, a risk level and a review.

The kernel keeps only the primitives: the PTY, the tree storage, rendering, the plugin host, the grants and the four pane primitives (terminal, agent, editor, basic). Core plugins own the behaviour on top of them. See [architecture](/pages/architecture/) and [primitives](/pages/primitives/).

### Five ways to work with a core feature

| Way | What you do | Weight | Needs |
|---|---|---|---|
| **Observe** | Read events, topics, catalogues and decoration slots. | Lightest. Read only. | A topic grant, for example `topics.subscribe:ilium.core.notifications/raised`. |
| **Drive** | Call a service or a command of the core plugin, or send an intent that it also uses. | You act, the core plugin runs the feature. | `services.use:<id>/<name>` or `commands.invoke:<id>/<command>`. |
| **Extend** | Fill an extension point: a collect entry (data) or a hook (code in the core plugin's flow). | You change what the core plugin does. It stays in charge. | `extensions.contribute:<id>/<point>`. |
| **Offer** | Declare your own extension points so others can extend you. | You become the owner of a slot. | `extensions.provide:<your-id>/<point>`. |
| **Replace** | Install a plugin that takes over the whole surface of a core plugin. | Heaviest. The built-in plugin is turned off. | `replaces` in the manifest and the user's confirmation. |

Pick the lightest way that does the job. Each step up costs more review, more risk and more work to keep in step with the core plugin.

#### I want to ... use ...

| I want to ... | Use | Why |
|---|---|---|
| Know when a notice was raised | Observe | The topic `ilium.core.notifications/raised` pushes it to you. |
| Raise a notice myself | Drive | The service `ilium.core.notifications/notify` takes a title, a body and an urgency. |
| Stop some notices at night | Extend (hook) | `before-notify` is a `veto` hook. A handler that refuses stops the notice. |
| Add a chat webhook as a delivery channel | Extend (collect) | `channels` is a collect point. Your entry names a service of yours. |
| Rewrite text before it is copied | Extend (hook) | `ilium.core.smart-copy/after-copy` is a `transform` hook. |
| Add a new kind of thing to select in Smart Copy | Extend (collect) | `recognisers` is a collect point. |
| Let others add card actions to my board plugin | Offer | Declare `card-actions` as a collect point of your own. |
| Draw something next to a tree row | Observe and a kernel slot | Decoration slots are a kernel slot, not an extension point. See [tree.decorate](/pages/capability-tree-decorate/). |
| Change how the whole tree sorts and groups | Replace | No point covers the whole feature. |
| Show a fixed text in a menu | A provider | Use [menu provider](/pages/api-menu-provider/), a kernel slot. |

### One running scenario: quiet hours for notifications

The goal: the user wants no low-urgency notices between 22:00 and 07:00. The notifications feature is the core plugin `ilium.core.notifications`. Its surface, from [its page](/pages/core-plugin-notifications/):

- Service: `ilium.core.notifications/notify` (method `send`).
- Topic: `ilium.core.notifications/raised`, with `{ event, pane, urgency }`.
- Points: `before-notify` (hook, `veto`) and `channels` (collect).

Each way solves part of the goal, or a neighbouring goal. The sections below show the code.

#### Way 1: observe

Observe when you only need to know. This plugin counts how many notices were raised and writes the number to its log. It needs `topics.subscribe:ilium.core.notifications/raised` in its capabilities and lists the topic under `contributes.topics.subscribes`.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NoticeCounter {
    raised: u32,
}

impl Lifecycle for NoticeCounter {}

impl Handler for NoticeCounter {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::TopicPublished(published) = event {
                if published.topic == "ilium.core.notifications/raised" {
                    self.raised += 1;
                    log::log(Level::Info, &format!("notices raised: {}", self.raised));
                }
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

let raised = 0;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'topic-published' && event.val.topic === 'ilium.core.notifications/raised') {
        raised += 1;
        log('info', `notices raised: ${raised}`);
      }
    }
    return [];
  },
});
```

An observer cannot change anything. It also never slows the core plugin down. See [messages and topics](/pages/api-messages-and-topics/).

#### Way 2: drive

Drive when you want the core plugin to do its job for you. This plugin raises a notice when its own job ends. It needs `services.use:ilium.core.notifications/notify`.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::services;

// manifest: "services.use:ilium.core.notifications/notify"
async fn tell_user(title: &str) {
    let payload = format!(r#"{{"title":"{title}","body":"","urgency":"low"}}"#);
    if let Err(error) = services::call("ilium.core.notifications/notify", "send", &payload).await {
        log::log(Level::Warn, &format!("notify failed: {}", error.message));
    }
}
```

**TypeScript**

```ts
import { call } from '@ilium/plugin-sdk/host/services';
import { log } from '@ilium/plugin-sdk/host/log';

// manifest: "services.use:ilium.core.notifications/notify"
export async function tellUser(title: string): Promise<void> {
  try {
    await call(
      'ilium.core.notifications/notify',
      'send',
      JSON.stringify({ title, body: '', urgency: 'low' }),
    );
  } catch (error) {
    log('warn', `notify failed: ${String(error)}`);
  }
}
```

The call passes through the same `before-notify` chain as every other notice, so a quiet hours extension affects your notice too. Services follow the rules in [services](/pages/services/) and [services API](/pages/api-services/).

#### Way 3: extend

Extend when you want to change what the core plugin does. This is the way that solves the quiet hours goal. The extension is a `veto` handler on `before-notify`. It refuses a notice of urgency `low` at night.

Manifest of the extension package:

```json
{
  "id": "com.example.quiet-hours",
  "exports": ["hook-handler"],
  "capabilities": {
    "server": ["extensions.contribute:ilium.core.notifications/before-notify"]
  },
  "contributes": {
    "extensions": [
      { "point": "ilium.core.notifications/before-notify@0.1.0", "id": "quiet-hours",
        "priority": 40, "handler": "quiet_hours" }
    ]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::exports::{HookCall, HookHandler, HookReply};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server", exports = ["hook-handler"])]
#[derive(Default)]
struct QuietHours;

impl Lifecycle for QuietHours {}

impl Handler for QuietHours {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl HookHandler for QuietHours {
    fn handle_hook(&mut self, call: HookCall) -> HookReply {
        if call.extension != "quiet_hours" {
            return HookReply::Pass;
        }
        // Parse call.payload_json with a JSON library in a real plugin.
        let is_low = call.payload_json.contains(r#""urgency":"low""#);
        if is_low && is_quiet_time() {
            return HookReply::Refuse("quiet hours".to_string());
        }
        HookReply::Pass
    }
}

fn is_quiet_time() -> bool {
    false // stand-in for a clock read and the user's setting
}
```

**TypeScript**

```ts
import { definePlugin, type HookCall, type HookReply } from '@ilium/plugin-sdk';

function isQuietTime(): boolean {
  const hour = new Date().getHours();
  return hour >= 22 || hour < 7;
}

export const { lifecycle, handler, hookHandler } = definePlugin({
  side: 'server',
  onEvents() {
    return [];
  },
  handleHook(call: HookCall): HookReply {
    if (call.extension !== 'quiet_hours') {
      return { tag: 'pass' };
    }
    const payload = JSON.parse(call.payloadJson) as { urgency?: string };
    if (payload.urgency === 'low' && isQuietTime()) {
      return { tag: 'refuse', val: 'quiet hours' };
    }
    return { tag: 'pass' };
  },
});
```

The handler sees the payload of `before-notify` (event, pane, title, urgency, channels) and nothing else. If it is slow or fails, the notice goes out. This is called fail-open and it is explained below.

A collect extension is simpler. It holds an entry instead of code. This manifest adds a delivery channel to the `channels` point. The entry names a service of the same plugin that delivers one notice.

```json
{
  "point": "ilium.core.notifications/channels",
  "id": "team-chat",
  "entry": { "id": "team-chat", "title": "Team chat", "service": "com.example.chat/deliver" }
}
```

#### Way 4: offer

Offer when your own plugin has a place where others could add something. This plugin shows a dashboard. It declares a collect point `tiles` so that other plugins can add tiles. It needs `extensions.provide:com.example.dash/tiles`. The full owner code is in [extension points](/pages/api-extension-points/).

```json
{
  "id": "com.example.dash",
  "capabilities": { "server": ["extensions.provide:com.example.dash/tiles"] },
  "contributes": {
    "extension_points": [
      { "id": "tiles", "kind": "collect", "title": "Dashboard tiles",
        "entry_schema": { "type": "object", "required": ["id", "title"],
          "properties": { "id": { "type": "string" }, "title": { "type": "string" } } },
        "max_entries": 32, "version": "0.1.0" }
    ]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::host::extensions;

// manifest: "extensions.provide:com.example.dash/tiles"
fn tile_titles() -> Vec<String> {
    extensions::list("com.example.dash/tiles")
        .map(|entries| entries.into_iter().map(|entry| entry.entry_json).collect())
        .unwrap_or_default()
}
```

**TypeScript**

```ts
import { list } from '@ilium/plugin-sdk/host/extensions';

// manifest: "extensions.provide:com.example.dash/tiles"
export function tileTitles(): string[] {
  return list('com.example.dash/tiles').map((entry) => entry.entryJson);
}
```

Offering is how a plugin of yours becomes a platform for other plugins. A core plugin is no different: its points are offered the same way.

<a id="replace"></a>

#### Way 5: replace

Replace when no lever is enough, for example when you want a different notification system. You install a plugin with `replaces`. It must provide the whole surface of the core plugin. Here the replacement is a plugin that offers notices in its own way and keeps the same service, topic and points.

```json
{
  "id": "com.example.notices",
  "version": "1.0.0",
  "replaces": ["ilium.core.notifications"],
  "capabilities": {
    "server": [
      "services.provide:ilium.core.notifications/notify",
      "extensions.provide:ilium.core.notifications/before-notify",
      "extensions.provide:ilium.core.notifications/channels"
    ]
  }
}
```

The rules of replacement are in the section "Replacing a core plugin" below.

### Reading the surface of any core plugin

A core plugin lists what it offers in three places.

1. **Its own page.** Every `core-plugin-*` page has an **Extension points** section. It has a table of points (kind, what it does, payload, risk), a list of services, a list of topics and a **Replaceable** line.
2. **The reference.** [Reference: core extension points](/pages/reference-core-extension-points/) lists the points of all core plugins in one table. Use it to search.
3. **The Plugins screen.** Settings > Plugins > the plugin shows its **Surface**: the services, points, topics, commands and pane kinds it offers. The manifest carries the same list in `surface`. See [reference: manifest](/pages/reference-manifest/).

Names follow one pattern:

| Item | Form | Example |
|---|---|---|
| Point | `<plugin-id>/<point>` | `ilium.core.fleet/before-broadcast` |
| Service | `<plugin-id>/<name>` | `ilium.core.fleet/broadcast` |
| Topic | `<plugin-id>/<name>` | `ilium.core.tree/changed` |
| Command | `<plugin-id>/<command>` | `ilium.core.kanban/open-board` |

A third-party plugin that wants a core topic lists it in `contributes.topics.subscribes` and holds the matching topic grant. A core service uses the normal `services.provide` and `services.use` grants. Services named `ilium.core.<plugin>/<name>` are stable and need no `unstable` opt-in, to call them or to provide them as a replacement ([Versioning](/pages/versioning/)). See [capability: services](/pages/capability-services/).

### Timing of hooks on core points

A hook on a core plugin's point runs on the side that owns the point (see the Side column of [Core extension points](/pages/reference-core-extension-points/)), one handler after another, under the deadlines in [Extension points](/pages/api-extension-points/) (20 ms per handler by default, set per handler by its manifest `deadline_ms` between 5 and 50 ms, 100 ms for the chain, measured from the first handler). For `veto`, `transform` and `first` points the core plugin holds that one action until the chain completes (at most the 100 ms cap plus one deadline) and keeps serving other events meanwhile. For `observe` points the action does not wait, and observe handlers may run after it. [EXT-029] See [Host scheduling](/pages/reference-host-scheduling/).

### Ordering, priorities and the user

- **Priority.** A contribution has a priority from 0 to 100, 50 by default. Lower runs first, but only within a tier band: a T1 contribution runs before a T2 or T3 contribution of any priority, then priority orders contributions inside the band, then plugin id order, then extension id order [SEC-288]. The declared priority is kept.
- **User order.** On the owner's Extensions tab the user can move any contributor up or down and turn it off. The user's order is a stored list per point. It overrides priority and tier, a contributor installed later runs after the stored items, and an update keeps the stored position. The host stores it, not the plugin.
- **Chains.** A `transform` chain passes each handler the output of the previous one. A `veto` chain stops at the first refusal. A `first` chain stops at the first answer. An `observe` chain calls everyone.
- **Fail open.** A handler that times out, traps or errors counts as `pass`. A timeout never vetoes. A breach is a timeout, a trap, an error return, an oversize or schema-failing reply, a CPU overrun or a skip because the instance was busy. Three consecutive breaches stop that extension until the contributor's instance restarts or the user presses Reset. The user sees `stopped after 3 failures` and a Reset button. The whole chain has a cap of 100 ms, and handlers that have not started by then are skipped.
- **Dormant contributions.** A contribution is dormant when its owner is not installed, is disabled, or does not satisfy its `@version`. It stays in the package and does nothing. It becomes active when the owner appears, and the Plugins screen shows `waiting for <owner-id>` until then. Install never fails because an owner is missing. See [lifecycle](/pages/lifecycle/).
- **Point versions.** Every point has a semver `version` for its schema. A contributor can write `<point>@<minimum-version>`. The version is a caret range (PKG-317, PKG-318, PKG-328 in [Dependency resolution](/pages/reference-dependency-resolution/)): `@1.2.0` accepts `>=1.2.0 <2.0.0` and `@0.1.0` accepts `>=0.1.0 <0.2.0`. A contribution without `@version` matches any owner version. A contribution that does not match stays dormant. See [versioning](/pages/versioning/).

A short rule for choosing a mode when you offer a point: use `observe` when you only tell others, `transform` when others may change data, `veto` when others may stop an action, and `first` when exactly one answer is needed.

### Security and consent

- **Grants are per point.** `extensions.contribute:<plugin-id>/<point>` names one point. Wildcards are refused. There is no grant that covers every point. See [capability: extensions.contribute](/pages/capability-extensions-contribute/).
- **High-risk points.** An owner marks a point `risk: "high"` when its handlers can see or change sensitive data or stop sensitive actions. Examples are text sent to a terminal, a broadcast to many agents, a text trigger about to fire and a scheduled input about to be sent. Contributing to such a point is a high-risk (H) grant. The review dialog shows a warning, and the row starts unchecked.
- **What a hook can see.** Only the payload of the owner's `payload_schema`. Payload fields are chosen by the owner, and they are listed in the table of the core plugin. A handler cannot read the owner's state or the other handlers.
- **What a hook can do.** Only what the mode allows. Replies the mode does not allow count as `pass`.
- **The user stays in charge.** The review dialog groups the package's extensions under **Adds to other plugins**. The user can turn any extension off later.
- **Replacement is a consent.** The user must confirm that the built-in plugin is turned off. See below.

See [security](/pages/security/) for the grant risk table and [packages](/pages/packages/) for the review dialog.

### Replacing a core plugin

A package with `"replaces": ["ilium.core.titles"]` can take the place of a core plugin. The list has at most 4 ids.

- **Which plugins.** A core plugin that can be replaced carries `"replaceable": true` and a `surface` list in its manifest (fields in [Manifest reference](/pages/reference-manifest/#top-level-keys)). `ilium.core.sessions` and `ilium.core.settings` are not replaceable, because they protect saved sessions and they host the Plugins screen itself.
- **The whole surface.** The replacement must provide every `surface` item of the replaced plugin: its services, points, commands, pane kinds, topics and cli commands. Values in `surface` are `kind:name` pairs. A `points` item matches only with the same kind, the same mode and the same major version; a `services` item needs the same method names and the same major version. Otherwise `manifest.replaces-surface-incomplete` names the item and the reason. The error lists the missing items. The surface is checked again at each enable and after each Ilium update. An enabled replacement that no longer covers the surface is turned off, the built-in plugin comes back, and a notice names the missing items. [EXT-051]
- **One at a time.** Several installed packages may replace one core plugin, and at most one is enabled. Enabling another one asks the user and turns the previous replacement off. Only one of the pair (built-in and replacement) is enabled. When the user enables the replacement, the dialog says `Replaces Titles and organisation. The built-in plugin is turned off while this one is on.` The user confirms. Turning the replacement off or uninstalling it turns the built-in plugin back on.
- **Uninstall of the replacement.** Removing the replacement, or turning it off, turns the built-in plugin back on. See the [uninstall cascade](/pages/lifecycle/#uninstall-cascade). The built-in plugin's data was never read or changed by the replacement.
- **Contributions carry over.** Standard contributions of other plugins to the replaced points move to the replacement after a notice. High-risk ones are suspended until the user confirms them for the new owner ([SEC-304]).
- **What is lost.** The settings tabs, commands and pane kinds of the replaced plugin disappear while it is off. Its stored data stays on disk and is not read by the replacement. When the built-in plugin comes back, it finds its data as it left it.
- **Install errors.** `manifest.replaces-unknown` (no such plugin), `manifest.replaces-not-replaceable`, `manifest.replaces-surface-incomplete`, `manifest.replaces-cycle` (a package lists its own id, or an id twice). A list of more than 4 ids fails with `manifest.schema`.

A replacement carries a real cost. It must keep up with every new point of the core plugin. Prefer extending when you can.

### What the user keeps when a core plugin is off

Extending or replacing a core plugin never removes what the user already has. When the plugin is disabled, the kernel fallback applies and the stored data stays. The table for all 30 core plugins is in [kernel and boot](/pages/reference-kernel-and-boot/). A replacement MUST provide the `surface` of the plugin it replaces, and it MUST keep the CLI commands of the replaced plugin (see [CLI compatibility](/pages/reference-cli-compatibility/)). [COR-022] [COR-040]

### Testing and debugging

- **Unit tests.** The fake host in [testing](/pages/testing/) runs one plugin instance with the grants you give it, and `FakeSession` runs several plugins and core extension point stubs together. Deliver a batch of events and check the submissions. A hook handler is a plain function over a payload, so you can also call it directly in a test.
- **Requests.** `run-hook` is a request. A test completes it by hand with `host.complete(request_id, outcome)` and checks the log. The method is described in [testing](/pages/testing/) under requests, jobs and services.
- **Fixtures.** `ilium plugin test` runs every fixture, compares its expectation file, and exits 3 when one differs. See [Test harness reference](/pages/reference-test-harness/).
- **Logs.** `ilium plugin logs <id> --tail 20` shows the log of one plugin. The host writes the extension codes there: `extension.handler-timeout`, `extension.reply-not-allowed`, `extension.schema-mismatch`, `extension.disabled`. [Debugging](/pages/debugging/) explains the log format and traps.
- **Development reload.** `ilium plugin dev` reloads a plugin when you rebuild it. See [debugging](/pages/debugging/).

### Frequently asked questions

**Is there one place to see everything a core plugin exposes?**
Yes. The **Extension points** section of its page, and the **Surface** on the Plugins screen. [Reference: core extension points](/pages/reference-core-extension-points/) has all of them in one list.

**Can I add a hook to a core plugin that has no point for it?**
No. A hook exists only where the owner declared a point. If you need one, ask for a new point on that core plugin. It is a normal manifest change with a version.

**Can a hook break the core plugin?**
A hook can refuse or change data only where the mode allows it. A hook that is late, traps or errors is ignored, and three consecutive breaches stop it. A hook cannot crash the owner.

**Does a hook slow the feature down?**
A little. T1 handlers share the process. T2 and T3 handlers cost one helper round trip. A handler gets 20 ms by default (its manifest `deadline_ms` may set 5 to 50 ms) and a chain gets 100 ms. Owners should not put hooks on a path hotter than about 100 calls per second.

**What if the owner is not installed?**
Your contribution is dormant. It does nothing, and the Plugins screen shows `waiting for <owner-id>`. It becomes active when the owner appears.

**What is the difference between a service and an extension point?**
A service has one provider and many callers, and a caller pulls an answer. An extension point has one owner and many contributors, and the owner decides when to read data or run handlers. See [services](/pages/services/).

**What is the difference between an extension point and a kernel slot?**
A kernel slot, such as a decoration slot, a menu provider or a status segment, is defined by the kernel. An extension point is defined by any plugin. Extension points do not replace kernel slots.

**Can two plugins fill the same point?**
Yes. Up to `max_handlers` handlers (8) or `max_entries` entries (64 on core points) per point. They run in the order described above, and the user can reorder them. Contributors beyond the cap are dormant with the reason `point full (<n>)`.

**Can I replace a core plugin and keep its data?**
No. The replacement does not read the replaced plugin's stored data. The data stays on disk for the built-in plugin to use when it returns.

**Can I replace the sessions or settings plugin?**
No. `ilium.core.sessions` and `ilium.core.settings` have `replaceable: false`.

**Does a wildcard grant exist?**
No. `manifest.grant-wildcard` refuses it. List each point.

When a replacement plugin is disabled by failure, the built-in core plugin it replaced turns on again and a notice says so. The replacement returns only when you enable it. [LIF-273]

- [LIF-273] A replacement disabled by failure MUST re-enable the replaced core plugin with a visible notice. Test: the replacement is disabled by the restart and disable rule (three crashes).

### See also

- [Extension points](/pages/api-extension-points/) for the full interface, the hook modes and the error codes.
- [Reference: core extension points](/pages/reference-core-extension-points/) for the points of every core plugin.
- [extensions.provide](/pages/capability-extensions-provide/) and [extensions.contribute](/pages/capability-extensions-contribute/) for the grants.
- [Core plugins](/pages/core-plugins/) for the list of core plugins.
- [Services](/pages/services/), [services API](/pages/api-services/), [messages and topics](/pages/api-messages-and-topics/) and [menu provider](/pages/api-menu-provider/) for the other mechanisms.
- [Capabilities](/pages/capabilities/), [security](/pages/security/), [packages](/pages/packages/) and [settings extension](/pages/settings-extension/) for grants, consent and settings.
- [Testing](/pages/testing/) and [debugging](/pages/debugging/) for tools.

Checked against `ilium:plugin@0.1.0`.

### Security

Risk marks follow [SEC-280]; replacement rules are [SEC-300] to [SEC-306] in [Extension security](/pages/reference-extension-security/).
