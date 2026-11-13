---
layout: page
title: toast
permalink: /pages/api-toast/
---

## toast

A toast is a short message that Ilium draws inside its own window, over the workspace, for a few seconds. It is the in-app counterpart of the desktop notice that the `notify` intent shows (see [Notifications and sounds](/pages/api-notifications/)). Use a toast when the user is looking at Ilium and the news is small: a copy finished, a scan found three items, a quota is nearly used. A toast carries a title, an optional body of rich text, a tone and at most two buttons. Each button runs one of the plugin's own commands.

Two intents control toasts: `toast` shows or replaces one, and `dismiss-toast` removes one. Both are submissions returned from the `handler` export. There is no read function: a plugin learns that the user pressed a button because its command is invoked, and it learns nothing else about the toast.

### What you need

- **Grant.** `notify.toast`. It is a normal-risk grant, available on the server and client sides. Request it per side in the manifest.
- **Commands.** Every toast button names a command that the plugin declares in `contributes.commands`. A button that names an unknown command refuses the whole intent with `invalid-argument`.
- **Language.** Rust with the crate `ilium-plugin-sdk`, or TypeScript with the package `@ilium/plugin-sdk`. Both build the same intent record.

```json
"capabilities": { "server": ["notify.toast"] },
"contributes": { "commands": [{ "id": "open-report", "title": "Open the scan report" }] }
```

See [Capabilities](/pages/capabilities/) for how grants are requested and shown to the user. The desktop counterpart is [notify.desktop](/pages/capability-notify-desktop/).

### Interface

```wit
// Excerpt of ilium:types/text in reference-wit
enum tone { info, success, warning, danger, muted }
```

The `toast-spec` record below belongs to `ilium:types/intents`.

```wit
// Excerpt of ilium:types/intents in reference-wit
record toast-action { label: string, command: string }       // label <= 24 characters

record toast-spec {
  id: option<string>,                 // a toast with the same id is replaced in place
  tone: tone,
  title: string,                      // at most 60 characters
  body: option<rich-text>,            // at most 200 characters of text
  actions: list<toast-action>,        // at most 2
  duration-ms: option<u32>,           // 1,000 to 30,000
}

// intent cases
toast(toast-spec),
dismiss-toast(string),                // the id
```

| Intent | Rust | TypeScript | Grant | Outcome |
|---|---|---|---|---|
| `toast` | `Intent::Toast(ToastSpec)` | `{ tag: 'toast', val: ToastSpec }` | `notify.toast` | `ok` once queued; `refused(rate-limited)`; `refused(invalid-argument)` |
| `dismiss-toast` | `Intent::DismissToast(String)` | `{ tag: 'dismiss-toast', val: string }` | `notify.toast` | `ok`, also when the id is not on screen |

When `duration-ms` is none, a toast of tone `danger` stays until the user dismisses it, and any other tone stays 5,000 ms. A value outside 1,000 to 30,000 ms is refused with `invalid-argument`. The countdown is drawn by the host; the plugin is not called while it runs.

### How a toast looks

The host draws a toast as a small rounded panel with the usual Ilium title notch, the body, a dim line for the buttons, and a gauge that shrinks until the toast closes. The gauge uses `━` for the time left and `─` for the time spent, as the preview dialogs of the client do. The tone sets the border colour and the title colour; the glyphs are the same for every tone.

<figure>
<pre class="term">╭  ≡ ● · Copied 3 regions ──────────╮
│ Smart Copy kept the last 3 blocks │
│ of the output of pane Shell.      │
│ Open report · Dismiss             │
│ ━━━━━━━━━━━━━━━━━━━───────────────│
╰───────────────────────────────────╯
</pre>
<figcaption>A toast of tone success with one button, about 60 percent through its 5,000 ms. Drawn from the preview dialog of the real client: rounded border, chrome title, dim hint line, gauge of heavy and light horizontal bars.</figcaption>
</figure>

At most `toasts_pending` (3, the same for every tier) toasts are alive at once; a fourth replaces the oldest. A toast never takes keyboard focus away from the pane the user is typing in. The buttons answer to a click.

<figure>
<svg viewBox="0 0 440 150" role="img" aria-label="Toast intent flow">
<title>Toast flow</title>
<desc>The plugin submits a toast intent. The host checks the grant, the rate and the id, queues the toast, draws it and runs the button command when the user presses it.</desc>
<defs><marker id="ta" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
<rect x="8" y="10" width="110" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="63" y="31" font-size="12" text-anchor="middle" fill="currentColor">handler: toast</text>
<line x1="118" y1="27" x2="150" y2="27" stroke="currentColor" marker-end="url(#ta)"/>
<rect x="150" y="10" width="130" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="215" y="31" font-size="12" text-anchor="middle" fill="currentColor">grant, rate, id check</text>
<line x1="280" y1="27" x2="312" y2="27" stroke="currentColor" marker-end="url(#ta)"/>
<rect x="312" y="10" width="120" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="372" y="31" font-size="12" text-anchor="middle" fill="currentColor">toast stack (3)</text>
<line x1="372" y1="44" x2="372" y2="80" stroke="currentColor" marker-end="url(#ta)"/>
<text x="380" y="66" font-size="11" fill="currentColor">button</text>
<rect x="312" y="80" width="120" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="372" y="101" font-size="12" text-anchor="middle" fill="currentColor">command-invoked</text>
<line x1="150" y1="97" x2="312" y2="97" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#ta)"/>
<text x="215" y="90" font-size="11" fill="currentColor" text-anchor="middle">refused: outcome only</text>
<text x="215" y="132" font-size="11" fill="currentColor" text-anchor="middle">origin = toast, command = the button's command</text>
</svg>
<figcaption>The plugin sees an outcome for the intent and, later, the invocation of the button's command. It never sees the countdown.</figcaption>
</figure>

### Rules

- **Replace by id.** A toast whose `id` equals one already on screen replaces it in place and restarts its countdown. Use a stable id for progress-like news ("scan", "copied") so the stack does not fill with copies. A toast without an id is always new.
- **Rate.** 3 toasts per 10 seconds per plugin. The fourth within the window completes as `refused(rate-limited)`. Replacing by id counts as a toast.
- **Length.** The title is cut by the plugin: over 60 characters is `invalid-argument`, not a silent cut. The body is rich text with at most 200 characters in total; over that is `invalid-argument`. Button labels are at most 24 characters.
- **Buttons.** At most two. A button runs the named command with `origin: toast` and `context` set to the toast `id`. The toast closes when a button is pressed.
- **Accessibility.** With reduced motion the gauge is drawn as a fixed text `5 s left` that updates once per second, not as a shrinking bar. A `danger` toast stays until dismissed while a client is attached.
- **Not a log.** A toast is gone when it closes. Put the permanent record in the plugin [log](/pages/api-log/), or in a pane.

### Examples

#### Example 1: a success toast with one button

Grants: `notify.toast`. Commands: `open-report`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{Intent, ToastAction, ToastSpec};
use ilium_plugin_sdk::types::text::{RichSpan, Tone};

fn copied_toast(regions: usize) -> Submission {
    Submission::new(Intent::Toast(ToastSpec {
        id: Some("copied".into()),
        tone: Tone::Success,
        title: format!("Copied {regions} regions"),
        body: Some(vec![vec![RichSpan::plain("Smart Copy kept the last blocks.")]]),
        actions: vec![ToastAction { label: "Open report".into(), command: "open-report".into() }],
        duration_ms: None,
    }))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk';
import { richSpan } from '@ilium/plugin-sdk/ui/build';

export function copiedToast(regions: number): Submission {
  return {
    intent: {
      tag: 'toast',
      val: {
        id: 'copied',
        tone: 'success',
        title: `Copied ${regions} regions`,
        body: [[richSpan('Smart Copy kept the last blocks.')]],
        actions: [{ label: 'Open report', command: 'open-report' }],
        durationMs: undefined,
      },
    },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

#### Example 2: a danger toast, then dismiss it when the problem is gone

A `danger` toast has no timeout. The plugin removes it by id when the cause ends.

**Rust**

```rust
fn quota_alert(used_percent: u8) -> Vec<Submission> {
    if used_percent >= 95 {
        vec![Submission::new(Intent::Toast(ToastSpec {
            id: Some("quota".into()),
            tone: Tone::Danger,
            title: "Quota almost used".into(),
            body: None,
            actions: vec![],
            duration_ms: None,
        }))]
    } else {
        vec![Submission::new(Intent::DismissToast("quota".into()))]
    }
}
```

**TypeScript**

```ts
export function quotaAlert(usedPercent: number): Submission[] {
  if (usedPercent >= 95) {
    return [{
      intent: { tag: 'toast', val: { id: 'quota', tone: 'danger', title: 'Quota almost used',
        body: undefined, actions: [], durationMs: undefined } },
      correlation: 1n, dedupKey: undefined,
    }];
  }
  return [{ intent: { tag: 'dismiss-toast', val: 'quota' }, correlation: 2n, dedupKey: undefined }];
}
```

### Errors and limits

| Case | Outcome |
|---|---|
| Grant `notify.toast` missing | `refused(not-permitted)` |
| 4th toast of the plugin within 10 s | `refused(rate-limited)` |
| Title over 60 characters, body over 200, more than 2 buttons, duration outside 1,000 to 30,000 ms | `refused(invalid-argument)` with the field in the message |
| Button names an unknown command | `refused(invalid-argument)` |
| `dismiss-toast` of an id not on screen | `ok` |

Limits: `toasts_pending` 3 (fixed), toast actions 2, body 200 characters, title 60 characters. See [Errors and limits](/pages/reference-errors-and-limits/).

### See also

- [Notifications and sounds](/pages/api-notifications/) for desktop notices and the `coalesce-key` field.
- [Commands](/pages/api-commands/) for the commands that toast buttons run.
- [Requests and outcomes](/pages/requests-and-outcomes/) for the outcome cases.
- [Command palette](/pages/api-command-palette/) for the other in-app surface that runs commands.

### Security

Toast text is sanitised and prefixed with the plugin name ([SEC-378], [SEC-380]).
