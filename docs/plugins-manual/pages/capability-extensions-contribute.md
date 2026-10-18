---
layout: page
title: "Capability: extensions.contribute"
permalink: /pages/capability-extensions-contribute/
---

## extensions.contribute:&lt;plugin-id&gt;/&lt;point&gt;

The `extensions.contribute:<plugin-id>/<point>` capability lets a plugin fill an extension point that another plugin owns. A collect point takes an entry: a piece of data the owner reads. A hook point takes a handler: code that the owner runs at a fixed place in its flow. Core plugins own many such points, so this grant is how a third-party plugin extends a core feature. The pair of this grant is [extensions.provide](/pages/capability-extensions-provide/). The full interface is in [extension points](/pages/api-extension-points/).

There is no grant that covers every point. Each point needs its own grant.

### What you need

- **One grant for each point you fill.** The grant goes in `capabilities.server` or `capabilities.client`, on the side that owns the call (see below).
- **A declaration.** Each entry or handler is listed in `contributes.extensions`. A grant without a declaration, or a declaration without a grant, is refused at install with `manifest.grant-missing-extension`.
- **The `hook-handler` export** on any side that has hook extensions. Without it install fails with `manifest.exports-mismatch`.
- **A language SDK.** Rust: crate `ilium-plugin-sdk`, trait `ilium_plugin_sdk::exports::HookHandler`. TypeScript: `@ilium/plugin-sdk`, member `handleHook` of `definePlugin`.

The owner does not have to be installed. A contribution to a point whose owner is missing or disabled is dormant. It stays in the package and counts for nothing until the owner appears.

### Grant syntax

| Grant | Result at install | Reason |
|---|---|---|
| `extensions.contribute:ilium.core.smart-copy/after-copy` | valid | one point of a core plugin |
| `extensions.contribute:ilium.core.smart-copy` | refused, `manifest.grant-argument-missing` | the point id is required |
| `extensions.contribute:ilium.core.smart-copy/*` | refused, `manifest.grant-wildcard` | wildcards are refused |
| `extensions.contribute:*` | refused, `manifest.grant-wildcard` | wildcards are refused in any position |
| `extensions.contribute-any` | refused, `manifest.grant-unknown` | this grant does not exist; list each point |
| `extensions.contribute:ilium.core.smart-copy/after-copy@0.1.0` | refused, `manifest.grant-unknown` | a version is never part of a grant |

A version can appear in the `point` field of a `contributes.extensions` entry, for example `ilium.core.smart-copy/after-copy@0.1.0`. A bare version is a caret range (`@0.1.0` accepts `>=0.1.0 <0.2.0`); see [dependency resolution](/pages/reference-dependency-resolution/) (PKG-317, PKG-318, PKG-328). See [versioning](/pages/versioning/).

### Side

A handler runs on the side that owns the call. A server-owned point calls the server half of the contributor. A client-owned point calls the client half. A contributor declares its handler and its grant on that side. A handler declared on the wrong side is refused at install with `manifest.extension-side`.

### What it allows

| Call | Meaning |
|---|---|
| An entry in `contributes.extensions` | Adds data to a collect point. Validated against `entry_schema` at install, on update and on `register`. |
| A handler in `contributes.extensions` | Adds code to a hook point. The `hook-handler` export receives the calls. |
| `register(point, extension, priority, entry)` | Adds or replaces an entry at run time. Only for collect points with `dynamic: true`. |
| `unregister(point, extension)` | Removes an entry this plugin registered. |

A handler gets only the payload of the owner's `payload_schema`. It may call other host functions its own grants allow, synchronously, within its deadline.

### Risk

The grant is normal risk (not high risk) for a point with `risk: "standard"`. It is high risk (H) for a point with `risk: "high"`. The owner sets the risk, because the owner knows what its payload holds. A high-risk grant starts unchecked in the review dialog, and the line carries a warning. Examples are the `before-send-input` point of the terminal, `before-fire` of text triggers, and `before-broadcast` of fleet.

A handler on a `veto` point can stop an action. A handler on a `transform` point can change data in flight. Both are limited. A handler that is slow, traps or errors counts as `pass`, so a broken contributor never blocks the owner. Three consecutive breaches (a timeout, trap, error or disallowed reply, with no successful handler call between them) stop that extension until the plugin restarts.

### Manifest

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

### Example: a handler

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
        // Parse call.payload_json with a JSON library and read the hour in a real plugin.
        if call.payload_json.contains(r#""urgency":"low""#) && is_night() {
            return HookReply::Refuse("quiet hours".to_string());
        }
        HookReply::Pass
    }
}

fn is_night() -> bool {
    false // stand-in for a clock read
}
```

**TypeScript**

```ts
import { definePlugin, type HookCall, type HookReply } from '@ilium/plugin-sdk';

function isNight(): boolean {
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
    if (payload.urgency === 'low' && isNight()) {
      return { tag: 'refuse', val: 'quiet hours' };
    }
    return { tag: 'pass' };
  },
});
```

### Errors

| Code | When | Where it appears |
|---|---|---|
| `manifest.grant-missing-extension` | An extension has no matching grant. | install |
| `manifest.extension-entry-invalid` | An entry fails the owner's `entry_schema`. | install, update |
| `manifest.extension-side` | The handler is on the wrong side. | install |
| `manifest.extension-limit` | More than 64 extensions in one plugin. | install |
| `extension.not-granted` | `register` without the grant, or the user declined it. | synchronous |
| `extension.not-dynamic` | `register` on a point that is not dynamic. | synchronous |
| `extension.cap-exceeded` | The point already holds `max_entries` entries or `max_handlers` handlers. | synchronous, or the extension stays dormant |
| `extension.handler-timeout` | The handler passed its deadline. Counts as `pass`. | log |
| `extension.reply-not-allowed` | The reply does not fit the mode. Counts as `pass`. | log |
| `extension.disabled` | Stopped after three consecutive breaches, or turned off by the user. | log, Plugins screen |

### Edge cases

- **Owner missing.** The contribution is dormant. The Plugins screen shows `waiting for <owner>`. Install succeeds.
- **Grant declined.** The extension is not added. Other extensions of the same package still work.
- **Point full.** If the owner's point already holds its maximum, the new extension stays dormant and the log gets `extension.cap-exceeded`.
- **Replaced owner.** If the user replaces the owner by another plugin that declares the same point, contributions move to the replacement. A replacement that lacks the point leaves them dormant.
- **User order.** The user can move or turn off your extension on the owner's Extensions tab. Your priority is a default.

### Review dialog

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Review: com.example.quiet-hours 0.1.0        │
│                                              │
│ Adds to other plugins                        │
│ [x] Notifications > Before notify            │
│     can stop a notice from being shown       │
│                                              │
│   [allow]  [decline]                         │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A standard-risk contribution is checked by default. A high-risk one starts unchecked and carries a warning.</figcaption>
</figure>

### See also

- [extension points](/pages/api-extension-points/) for the interface and the hook rules.
- [extensions.provide](/pages/capability-extensions-provide/) for the grant of the owner.
- [extending core plugins](/pages/extending-core-plugins/) for the concept guide.
- [reference: core extension points](/pages/reference-core-extension-points/) for the points of every core plugin.
- [security](/pages/security/) for grant risk and consent.

Checked against `ilium:plugin@0.1.0`.

### Security

Contributors to sensitive points need the matching read grant ([SEC-282]). See [Extension security](/pages/reference-extension-security/).
