---
layout: page
title: "Capability: extensions.provide"
permalink: /pages/capability-extensions-provide/
---

## extensions.provide:&lt;plugin-id&gt;/&lt;point&gt;

The `extensions.provide:<plugin-id>/<point>` capability lets a plugin own an extension point. An extension point is a named slot that other plugins fill with data (a collect point) or with code (a hook point). The owner reads the data with `list` and runs the code with `run-hook`. The owner decides when each happens. The pair of this grant is [extensions.contribute](/pages/capability-extensions-contribute/), which lets a plugin fill a slot. The full interface is in [extension points](/pages/api-extension-points/).

A plugin may own only points whose name starts with its own plugin id. The grant string must match a point that the plugin declares in `contributes.extension_points`.

### What you need

- **One grant for each point.** The grant goes in `capabilities.server` or `capabilities.client`, on the side that calls `list` or `run-hook`.
- **A declaration.** The point is listed in `contributes.extension_points` with its id, kind and schemas. A grant that names a point without a declaration is refused at install with `manifest.extension-point-invalid`. A declaration without a grant is refused with `manifest.grant-missing-extension`.
- **A language SDK.** Rust: crate `ilium-plugin-sdk`, module `ilium_plugin_sdk::host::extensions`. TypeScript: `@ilium/plugin-sdk/host/extensions`.
- **Nothing on the contributors.** The owner does not need to know who the contributors are. The kernel orders them.

### Grant syntax

The argument is the global point name: the plugin id, a slash, and the point id. The point id is 1 to 64 characters from `a-z`, `0-9` and `-`.

| Grant | Result at install | Reason |
|---|---|---|
| `extensions.provide:com.example.notes/before-export` | valid | one point of the plugin `com.example.notes` |
| `extensions.provide:com.example.notes` | refused, `manifest.grant-argument-missing` | the point id is required |
| `extensions.provide:com.example.notes/*` | refused, `manifest.grant-wildcard` | wildcards are refused |
| `extensions.provide:com.example.other/before-export` | refused, `manifest.extension-point-invalid` | the prefix is not this plugin's id |
| `extensions.provide:com.example.notes/missing` | refused, `manifest.extension-point-invalid` | no declared point has that id |
| `extensions.provide:com.example.notes/before-export@0.1.0` | refused, `manifest.grant-unknown` | a version is never part of a grant |

### What it allows

| Call | Meaning |
|---|---|
| `list(point)` | Read the active entries of a point. Dormant and disabled entries are left out. |
| `run-hook(point, payload)` | Run the handlers of a hook point and wait for the outcome. |
| `take-reply(request)` | Take the final payload or the answer of a finished chain. |
| the topic `ilium.extensions/changed` | Learn that contributions to the point changed. Delivered only to the owner. |

It does not allow anything on other plugins' points. It does not allow reading a contributor's code or state. A contributor sees only the payload of the owner's `payload_schema`.

### Risk

The grant is normal risk (not high risk). It is checked by default in the review dialog. Owning a point gives a plugin no power over the user's machine. It gives other plugins a way to run code at a place in the owner's flow, so the owner is responsible for what it puts in the payload. The owner sets `risk: "high"` on a point whose handlers can see or change sensitive data, such as text that goes to a terminal. A high-risk point makes the contributor's grant a high-risk (H) grant.

### Manifest

```json
{
  "id": "com.example.notes",
  "capabilities": {
    "server": ["extensions.provide:com.example.notes/before-export"]
  },
  "contributes": {
    "extension_points": [
      { "id": "before-export", "kind": "hook", "mode": "transform",
        "title": "Before export",
        "payload_schema": { "type": "object",
          "properties": { "text": { "type": "string", "maxLength": 65536 } } },
        "reply_schema": { "type": "object",
          "properties": { "text": { "type": "string", "maxLength": 65536 } } },
        "risk": "standard", "deadline_ms": 20, "max_handlers": 8, "version": "0.1.0" }
    ]
  }
}
```

### Example: run a hook

**Rust**

```rust
use ilium_plugin_sdk::host::extensions;
use ilium_plugin_sdk::host::log::{self, Level};

// manifest: "extensions.provide:com.example.notes/before-export"
async fn export_note(text: &str) {
    let payload = format!(r#"{{"text":{}}}"#, serde_json::to_string(text).unwrap_or_default());
    match extensions::run_hook("com.example.notes/before-export", &payload).await {
        Ok(Some(final_payload)) => log::log(Level::Info, &final_payload),
        Ok(None) => log::log(Level::Info, "no handler changed the text"),
        Err(error) => log::log(Level::Warn, &format!("{}: {}", error.code, error.message)),
    }
}
```

**TypeScript**

```ts
import { runHook } from '@ilium/plugin-sdk/host/extensions';
import { log } from '@ilium/plugin-sdk/host/log';

// manifest: "extensions.provide:com.example.notes/before-export"
export async function exportNote(text: string): Promise<void> {
  try {
    const finalPayload = await runHook('com.example.notes/before-export', JSON.stringify({ text }));
    log('info', finalPayload ?? 'no handler changed the text');
  } catch (error) {
    log('warn', `hook failed: ${String(error)}`);
  }
}
```

### Errors

| Code | When | Where it appears |
|---|---|---|
| `extension.not-granted` | The plugin lacks the grant for that point, or the user declined it. | synchronous |
| `extension.unknown-point` | The point is not declared by an installed and enabled plugin. | synchronous |
| `extension.schema-mismatch` | The payload fits the size cap but fails `payload_schema`. Base code `invalid-argument`. | synchronous |
| `extension.cap-exceeded` | The payload is over 64 KiB. Base code `invalid-argument`. | synchronous |
| `extension.reentrant` | A handler called `run-hook` on the point that is running it. | synchronous |
| `extension.vetoed` | A `veto` handler refused. Base code `conflict`. | completion, outcome `refused(conflict)` |

Every extension code travels as a base `plugin-error` code plus a message prefix; see [the code mapping](/pages/api-extension-points/#edge-cases-and-error-codes).

### Edge cases

- **No contributors.** `list` returns an empty list. `run-hook` resolves with outcome `ok` and no reply. The owner carries on with its original data.
- **Grant declined.** Every call returns `extension.not-granted`. Contributions to the point go dormant because the point has no active owner.
- **Owner disabled.** Its contributions go dormant at once. When the user enables the owner again, they become active and the owner receives `ilium.extensions/changed`.
- **Replaced owner.** A plugin that replaces a core plugin takes over its points. The replacement must declare them all. See [extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin).
- **Limits.** 16 points per plugin, up to 256 entries per collect point, up to 8 handlers per hook point, a chain of 100 ms.

### Review dialog

The install dialog lists one row for each owned point.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Review permissions: com.example.notes 0.1.0  │
│                                              │
│ [x] Offer the point Before export            │
│     extensions.provide:                      │
│     com.example.notes/before-export          │
│                                              │
│   [allow]  [decline]                         │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The review row for an owned point.</figcaption>
</figure>

### See also

- [extension points](/pages/api-extension-points/) for the interface and the hook rules.
- [extensions.contribute](/pages/capability-extensions-contribute/) for the grant of the other side.
- [extending core plugins](/pages/extending-core-plugins/) for the concept guide.
- [services](/pages/capability-services/) and [topics](/pages/capability-topics-subscribe/) for the other ways plugins work together.

Checked against `ilium:plugin@0.1.0`.

### Security

Owners declare risk and `on_failure` per [SEC-280] and [SEC-287]. See [Extension security](/pages/reference-extension-security/).
