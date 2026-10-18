---
layout: page
title: The Extensions tab
permalink: /pages/user-extensions-tab/
---

## The Extensions tab

Plugins can add to what other plugins do. A plugin may add a step after Smart Copy copies text, a rule that checks a trigger before it sends, or a new entry in a list that another plugin keeps. The places where this is allowed are called extension points. The Extensions tab shows these additions for one plugin, lets you turn each one off, and lets you decide the order in which they run. This page explains the tab for users.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/). The technical reference is [Extension points](/pages/api-extension-points/).

<div class="note">
<p>Decision status. The platform is not implemented. The wording in the renders is a working text.</p>
</div>

### Contents

- Owners and contributors
- Open the tab
- The owner view
- The contributor view
- Change the order
- Turn an extension off
- The two special states
- What the host does to protect you
- Replacements
- Errors and edge cases
- Requirements

### Owners and contributors

- The **owner** of an extension point is the plugin that offers it. Smart Copy owns "After copy". It runs the extensions at the right moment.
- A **contributor** is a plugin that adds an extension to a point it does not own.

One plugin can be both. Core plugins own many points: every core plugin page has an Extension points section, and [Core extension points](/pages/reference-core-extension-points/) lists all of them.

An extension is only an extension. It is not a permission. A contributor needs a permission to fill each point, and you see it in the install dialog in the group **Adds to other plugins**. Points marked high risk, such as a rule that can stop a trigger from sending text, start unchecked.

### Open the tab

1. Open Settings with `Ctrl+B` then `:`.
2. Choose **Plugins** and select a plugin.
3. Choose the **Extensions** tab.

The tab shows two lists: the points that the plugin owns, with everyone who contributes to them, and the extensions that the plugin contributes to other plugins. A plugin that neither owns a point nor contributes to one has no Extensions tab.

### The owner view

Select an owner such as Smart Copy. Each of its points lists the contributors in the order they run.

<figure>
<pre class="term"><span class="t-hi">╭─ Smart Copy &gt; Extensions ────────────────────╮</span>
│ After copy (transform)                       │
│  1 [x] com.example.tickets  strip_tokens     │
│  2 [x] com.example.redact   mask_emails      │
│  3 [ ] com.example.legacy   old_filter       │
│                                              │
│ Recognisers (collect)                        │
│    [x] com.example.tickets  jira-key         │
│                                              │
│ Up/Down move   Space on or off   r reset     │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The owner view.</figcaption>
</figure>

The number is the place in the chain. `[x]` means on. `[ ]` means you turned it off. The word in brackets after the point name tells how the owner uses the extensions:

| Word | What the owner does |
|---|---|
| observe | Tells every contributor what happened. Their answers are ignored |
| transform | Passes the data from one contributor to the next, in order |
| veto | Asks each contributor in order whether to allow it, and stops at the first no |
| first | Asks each in order, and takes the first answer |
| collect | Gathers entries from all contributors into one list |

### The contributor view

Select a contributor such as `com.example.tickets`. The tab lists, for each point it fills, the owner and the point, with a switch.

<figure>
<pre class="term"><span class="t-hi">╭─ com.example.tickets &gt; Extensions ───────────╮</span>
│ Smart Copy &gt; After copy                      │
│   [x] strip_tokens                           │
│   stopped after 3 failures          [Reset]  │
│                                              │
│ Notes Plus &gt; Export hook                     │
│   [x] stamp_header                           │
│   waiting for com.example.notes-plus         │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The contributor view.</figcaption>
</figure>

### Change the order

By default extensions run in the order of their priority, a number the contributor sets. Lower runs first, but a contributor from a built-in (T1) plugin always runs before one from a T2 or T3 plugin, whatever the priorities. A tie goes in plugin id order. You can override this. Select a row in the owner view and press `Up` or `Down`. The order you set is saved by Ilium, not by the plugin, and it wins over the priority. A new contributor goes to the end of your order.

- [USR-050] The Extensions tab MUST list contributors of each point in run order, MUST let the user move a contributor up or down, and the user's order MUST win over priority. Test: set priorities 10 and 20, move the second above the first, call the point, and assert the new order.
- [USR-051] The host, not the plugin, MUST store the user's order and the toggles, and MUST keep them across restarts. Test: reorder, restart, list.

### Turn an extension off

Press `Space` on a row. The extension stays installed but the owner does not call it. Your choice is saved and survives a restart, an update of the contributor and a turn-off and on of the contributor. It ends only when you turn the row on again, or when you uninstall the contributor. If the contributor is uninstalled, its entries are removed from your order.

Turning an extension off does not change the contributor's permissions.

### The two special states

**Waiting for <owner>.** The contributor adds to a point whose owner is not installed or is turned off. The extension does nothing and your switch keeps its setting. When the owner returns, the extension starts working again without any action from you. Installing a plugin never fails because its owner is missing.

**Stopped after 3 failures.** An extension must answer quickly: each one gets between 5 ms and 50 ms, 20 ms by default, and a whole chain gets 100 ms. A slow or failing extension is skipped for that call, as if it had said "no change". A veto extension that is slow never blocks. If an extension is slow or fails three times in a row, Ilium stops it, shows a notice, and the row reads "stopped after 3 failures" with a **Reset** button. The stop is not saved on disk. It ends when the contributor restarts or when you press Reset.

- [USR-052] A hook that times out, traps or errors MUST count as "no change", and a timeout MUST NOT block or veto. Test: let a veto handler sleep past its deadline and assert the action goes ahead.
- [USR-053] Three breaches in a row MUST stop the extension, MUST show a notice and a Reset button, and the stop MUST NOT be written to disk. Test: cause three breaches, restart the contributor, and assert the extension is active again.
- [USR-054] An extension whose owner is missing or off MUST show "waiting for <owner>" and MUST become active when the owner returns. Test: remove the owner, check the label, reinstall.

### What the host does to protect you

- A payload that goes to an extension is limited to 64 KiB. An extension sees only the data of the point and nothing else of the owner.
- An extension cannot call the point that is running it.
- An extension runs inside the contributor, with only the contributor's own permissions.
- The host logs every breach in the contributor's log. See [Troubleshooting plugins](/pages/user-troubleshooting/).

### Replacements

Instead of adding to a core plugin, a plugin may replace it. This is separate from extensions and asks you to confirm. See [Core plugins for users](/pages/user-core-plugins/).

### Errors and edge cases

| Situation | What happens |
|---|---|
| Two contributors have the same tier and the same priority | Plugin id order decides, until you set an order |
| You turn off the owner | The extensions go dormant, "waiting for <owner>" |
| You turn on a contributor you had switched off | The row stays as you set it, and you switch it on |
| The contributor is stopped and you restart Ilium | The stop is gone. After three more breaches it stops again |
| A package adds to a point that does not exist | Install does not fail. The extension waits for the owner |
| Your administrator blocks a contributor | The row shows "Blocked by policy" and does nothing |

### See also

- [Plugins for users](/pages/user-plugins-overview/)
- [Core plugins for users](/pages/user-core-plugins/)
- [Extension points](/pages/api-extension-points/)
- [Core extension points](/pages/reference-core-extension-points/)
- [Troubleshooting plugins](/pages/user-troubleshooting/)

API version checked: `ilium:plugin@0.1.0`.
