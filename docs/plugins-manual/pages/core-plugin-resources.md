---
layout: page
title: Shared resources
permalink: /pages/core-plugin-resources/
---

## Shared resources

Shared resources coordinate exclusive access to hardware, such as a GPU or one USB development board, between agents in different projects on the same machine and operating-system user account. Each resource has one owner and a first-come, first-served queue. An agent reserves with `ilium resource reserve`, waits with `ilium resource wait` and releases with `ilium resource release`. The plugin id is `ilium.core.resources` and it runs on both sides: the server side owns the catalog, the reservations, the CLI and the instruction block, and the client side draws the tree rows, the control panel and the queue strip.

Queue rules are instructions to cooperating agents. Ilium does not stop an unrelated program from using the hardware, and the plugin never kills a process or resets a device.

### What it covers

- Shared resource queues: definitions, project links and reservations, described in the Shared resource queues guide (`src/docs/resource-queues.md`).
- The `ilium resource` command (alias `ilium ressource`) with `list`, `show`, `reserve`, `wait`, `status`, `estimate`, `release`, `cancel`, `recover`, `repair`, `rebind`, `gate`, `create`, `link`, `edit`, `unlink`, `archive` and `notify-urgent`.
- The project context menu entries "New resource…" and "Link existing resource…".
- The resource rows at the top of a project in the tree, the right-side detail view, the control panel and the segmented queue strip of a blocking wait.
- The resources instruction block in agent instruction files, and recovery of a reservation that shows `needs_attention`.

### What the user sees

<figure>
<pre class="term">╭─ 3090 GPU ─────────────────────────────────────╮
│ <span class="t-warn">TAKEN</span>  <span class="t-dim">held 12m of 20m · 2 waiting</span>             │
│ <span class="t-hi">Owner</span>   train-run      <span class="t-dim">ilium · pane 14</span>         │
│ <span class="t-hi">1</span>       eval-sweep     <span class="t-dim">manual · est. 10m</span>       │
│ <span class="t-hi">2</span>       render-check   <span class="t-dim">ilium · est. 5m</span>         │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative: the detail view of a taken resource with its owner and the ordered waiting list.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium resource reserve 3090-gpu 20m
{"type":"result","operation":"reserve","state":"queued",...}
<span class="t-prompt">$</span> ilium resource wait 7c1e5a52
{"type":"result","operation":"wait","state":"owned",...}
<span class="t-prompt">$</span> echo $?
<span class="t-ok">0</span>
</pre>
<figcaption>Rendered at 80 columns or fewer. Illustrative: the records are cut. A queued reserve grants nothing; the wait for the same reservation grants ownership.</figcaption>
</figure>

### Behaviour contract

The exact values a parity test uses are in this table. When it differs from `src/docs/resource-queues.md` or `ilium/src/resource.rs` at the baseline commit, the baseline wins and this page is a defect.

| Id | Rule | Exact value or text | Baseline source |
|---|---|---|---|
| resources-B01 | Queue policy | One owner per resource and a first-come, first-served queue | `src/docs/resource-queues.md` |
| resources-B02 | Output | One JSON object per line. A success has `"type":"result"`; a failure has `"type":"error"` with `operation`, `request_id`, `code` and `message` | `ilium/src/resource.rs` |
| resources-B03 | Exit status | 0 success; 2 usage or a refused request; 3 the reservation was cancelled or revoked (`cancelled`, `revoked`, `reservation_cancelled`, `reservation_revoked`); 4 the outcome is unknown (`outcome_unknown`, `observation_failed`, `store_error`); 5 not found (`not_found`, `reservation_not_found`, `receipt_expired`) | `ilium/src/resource.rs` |
| resources-B04 | Durations | A unit is required, such as `30s`, `10m` or `1h30m`; range one second to seven days | `src/docs/resource-queues.md#agent-command-workflow` |
| resources-B05 | Estimate | The total occupancy from acquisition, not additional time. Advisory: an overdue owner still owns the resource | `src/docs/resource-queues.md#estimates-and-recovery` |
| resources-B06 | Reserve forms | `reserve SLUG DURATION` and `reserve AGENT SLUG DURATION` | `ilium/src/resource.rs` |
| resources-B07 | Global flags | `--project <path>`, `--session <name>`, `--agent <pane or session/pane>`, `--request-id <key>` | `ilium/src/resource.rs` |
| resources-B08 | Gate states | `open`, `paused`, `unavailable` | `ilium/src/resource.rs` |
| resources-B09 | Resource kinds | `generic` (default), GPU and USB development board | `src/docs/resource-queues.md#create-or-link-a-resource` |
| resources-B10 | Active reservations | One agent invocation holds at most one active reservation across all resources | `src/docs/resource-queues.md#agent-command-workflow` |
| resources-B11 | Wait | No default timeout. Interrupting or disconnecting detaches the observer; the reservation remains | `src/docs/resource-queues.md#agent-command-workflow` |
| resources-B12 | Stale owner state | `needs_attention` | `src/docs/resource-queues.md#estimates-and-recovery` |
| resources-B13 | Project identity file | `.ilium/resource-project-id`, mirrored in project settings | `src/docs/resource-queues.md#moving-a-linked-project` |

### Settings it reads

The plugin owns no tab. It reads one key of the User Interface table through `settings.read-kernel:user-interface`.

| Key | Default | Range or values | Baseline source |
|---|---|---|---|
| `[ui] resource_queue_grouping` | true | true or false. On shows owners and waiting agents below the resource in the tree. It changes the display only | `src/docs/resource-queues.md#tree-detail-and-progress-displays` |

### Keys

The plugin binds no global chord. The control panel has one key per button, listed in its help (`?`). A greyed button explains why it cannot be used, on hover and on click.

### CLI

`ilium ressource` is an alias of `ilium resource`. Inside an Ilium pane the agent identity is inferred; an explicit agent reference must resolve to a live pane and invocation.

| Command | What it does |
|---|---|
| `resource list [--all]` | Lists the resources linked to the project, or every resource |
| `resource show <resource>` | Prints one definition with its description, rules and revision |
| `resource reserve [<agent>] <slug> <duration>` | Creates a reservation. The result is owned or queued |
| `resource wait <reservation>` | Blocks until the reservation owns the resource or reaches a terminal result |
| `resource status [<reservation>]` | Prints one reservation, or all reservations |
| `resource estimate <reservation> <duration>` | Replaces the estimate of total occupancy |
| `resource release <reservation>` | Ends an owned reservation |
| `resource cancel <reservation> [--reason <text> --expected-revision <n>]` | Ends a queued reservation |
| `resource recover <resource> --reason <text> [--expected-revision <n>]` | Clears a stale owner |
| `resource gate <resource> open\|paused\|unavailable [--reason <text>]` | Pauses the queue or marks the hardware unavailable |
| `resource repair <resource>` | Retries the instruction synchronisation of a linked resource |
| `resource rebind <project-id> <path> --expected-revision <n>` | Rebinds a moved project |
| `resource create --name <n> --slug <s> [--description --rules --kind --device-identity]` | Creates a definition and links it |
| `resource link <resource> --slug <s>`, `resource unlink <resource>` | Links or unlinks a project |
| `resource edit <resource> [...]`, `resource archive <resource>` | Edits or archives a definition |
| `resource notify-urgent` | Sends an urgent instruction notice to the project's agents |

### Failure behaviour

- **The plugin is disabled or down.** `ilium resource` fails with the code of an unavailable plugin command, and the tree rows and the panel are not drawn. Definitions and reservations stay in storage: no owner changes and no successor is granted while the plugin is away.
- **A lost acknowledgement.** The caller retries the mutation with the same request identifier and gets the first outcome. A request identifier outside the retry window is refused with `receipt_expired` and exit status 5.
- **A temporary catalog-busy response.** The CLI retries by itself with the same request and correlation identifiers inside the command deadline. It does not retry an uncertain outcome or a revision conflict.
- **The owner is gone.** After a reboot or the exit of the owning agent process the reservation shows `needs_attention`. Nothing is granted until `resource recover` or the panel clears it.
- **An instruction or settings write fails.** The failure is shown as synchronisation debt on the resource. `resource repair` retries it and never recreates the resource or loses a reservation.

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "id": "ilium.core.resources",
  "capabilities": {
    "server": ["cli.contribute", "agent.read", "panes.read", "tree.decorate", "settings.read-kernel:user-interface"],
    "client": ["ui.panels", "ui.chrome", "settings.read-kernel:user-interface"]
  },
  "contributes": { "cli": [{ "name": "resource", "summary": "Shared resource queues and reservations", "output": "jsonl", "requires_server": true, "aliases": ["ressource"] }] }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-32-core-plugins). [COR-050] Every `cli` key in the fragment above appears with the same value in this manifest.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.resources",
  "name": "Shared resources",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Shared resources give hardware one owner and a first-come, first-served queue across projects.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0"
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0"
  },
  "capabilities": {
    "server": [
      "cli.contribute",
      "agent.read",
      "panes.read",
      "tree.decorate",
      "settings.read-kernel:user-interface",
      "extensions.provide:ilium.core.resources/resource-kinds",
      "extensions.provide:ilium.core.resources/before-grant"
    ],
    "client": [
      "ui.panels",
      "ui.chrome",
      "settings.read-kernel:user-interface"
    ]
  },
  "activation": [
    "startup"
  ],
  "contributes": {
    "cli": [{
      "name": "resource",
      "summary": "Shared resource queues and reservations",
      "output": "jsonl",
      "requires_server": true,
      "aliases": ["ressource"]
    }],
    "panels": [
      { "id": "resource", "title": "Resource", "modal": false }
    ],
    "extension_points": [
      {
        "id": "resource-kinds",
        "kind": "collect",
        "title": "Add a kind of resource, such as a test rig or a licence seat",
        "entry_schema": {
          "type": "object",
          "properties": {
            "id": { "type": "string" },
            "title": { "type": "string" },
            "identity_hint": { "type": "string" }
          },
          "required": ["id", "title", "identity_hint"]
        },
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "before-grant",
        "kind": "hook",
        "mode": "veto",
        "title": "Before a reservation is granted ownership",
        "payload_schema": {
          "type": "object",
          "properties": {
            "resource": { "type": "string" },
            "reservation": { "type": "string" },
            "pane": { "type": "string" },
            "estimate_seconds": { "type": "integer" }
          },
          "required": ["resource", "reservation", "pane", "estimate_seconds"]
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Samples

The samples of this page are not written yet. The plugin crate `plugins/core/resources` is a skeleton: it declares both sides and produces no contribution until the feature logic moves there.

### Extension points

Other plugins change what `ilium.core.resources` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.resources/<point>`). Contributors need `extensions.contribute:ilium.core.resources/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `resource-kinds` | collect | Add a kind of resource, such as a test rig or a licence seat. | entry: id, title, identity_hint | standard |
| `before-grant` | hook: veto | Refuse a grant that is about to give a reservation ownership. | payload: resource, reservation, pane, estimate_seconds | high |

A veto in `before-grant` leaves the reservation queued at its position; it never cancels it and never grants another reservation ahead of it.

**Services** (grant `services.use:<name>`):

- `ilium.core.resources/reservations`: list, status: read the resources of a project and the state of one reservation

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.resources/changed`: { resource, state, waiting }

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.resources"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

### Extension points this plugin owns

The values below are the ones the core extension points table uses for these two points. Both run on the server side, where the catalog and the queue live.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.resources/resource-kinds` | server | 0.1.0 | none (default) | 64 entries | no |
| `ilium.core.resources/before-grant` | server | 0.1.0 | 20 ms | 8 handlers | no |

### ABI used

- [CLI API](/pages/api-cli/) and [cli.contribute](/pages/capability-cli-contribute/)
- [Events](/pages/api-events/)
- [agent.read](/pages/capability-agent-read/) and [panes.read](/pages/capability-panes-read/)
- [tree.decorate](/pages/capability-tree-decorate/)
- [ui.panels](/pages/capability-ui-panels/) and [ui.chrome](/pages/capability-ui-chrome/)
- [Storage](/pages/api-storage/)
- [settings.read-kernel](/pages/capability-settings-read-kernel/)
- [Extension points](/pages/api-extension-points/)

### See also

- [Core replacement matrix](/pages/reference-core-replacement-matrix/) for the surface, stored data, CLI names and install codes that a replacement of this plugin must keep.
- [Core plugins](/pages/core-plugins/)
- [Progress](/pages/core-plugin-progress/), whose monitors a blocking wait is shown beside
- [Agent setup](/pages/core-plugin-agent-setup/), which installs the other instruction blocks

## Requirement statements

- **[PRM-451]** The system must give each resource exactly one owner and a first-come, first-served queue. `ilium resource reserve` must return a reservation that is either owned or queued, and a queued reservation must never grant access to the hardware.
- **[PRM-452]** Every `ilium resource` command must print one JSON object per line, with `type` set to `result` on success and to `error` with the fields `operation`, `request_id`, `code` and `message` on failure, and must exit 0 on success, 2 for a usage error or a refused request, 3 for a cancelled or revoked reservation, 4 for an unknown outcome and 5 when the reservation or receipt is not found. `ilium ressource` must behave as `ilium resource`.
- **[PRM-453]** The system must accept a duration only with units, such as `30s`, `10m` or `1h30m`, in the range of one second to seven days, and must treat an estimate update as the total occupancy from acquisition, not as additional time.
- **[PRM-454]** `ilium resource wait` must block with no default timeout until the reservation owns the resource or reaches a terminal result, and must show queue progress. Interrupting or disconnecting a wait must detach only its observer and keep the reservation at its queue position.
- **[PRM-455]** One agent invocation must hold at most one active reservation across all resources, and a second reserve while one is owned or queued must be refused with exit status 2.
- **[PRM-456]** A retried mutation that carries the same request identifier must return the outcome of the first attempt and must not create a second reservation. A request identifier outside the retry window must be refused, not treated as new work. The CLI must retry a temporary catalog-busy response by itself with the same request and correlation identifiers inside the command deadline, and must not retry an uncertain outcome or a revision conflict.
- **[PRM-457]** Inside an Ilium pane the system must infer the agent identity, and an explicit agent reference must resolve to a live pane and invocation. A display title alone must never authorise a reservation for another agent.
- **[PRM-458]** An estimate must be advisory: an overdue owner must keep the resource, the start estimates of the waiting reservations must become unknown, and no reservation must be released, cancelled or granted because an estimate passed.
- **[PRM-459]** When the owner is provably gone, after a reboot of the machine or the exit of the owning agent process, the reservation must show `needs_attention` and no successor must be granted. `ilium resource recover <slug> --reason <text>` from a pane of a linked project must clear it in one step with no typed confirmation and keep the reason in the resource history. Recovery must check the current catalog revision and must never kill a process or reset a device. For a live owner it must only revoke that owner and pause the resource, and must grant no successor until a separate interactive confirmation reopens the queue.
- **[PRM-460]** Marking a resource `unavailable` or `paused` with `ilium resource gate` must keep its owner and its queue, and a release while the resource is unavailable or paused must grant no successor until the gate is `open` again.
- **[PRM-461]** Creating a resource must also link it to the creating project, and linking an existing resource from another project must share the same queue. A device identity must be unique across definitions, on create and on edit. A link with an active reservation must not be removed, closing the creator project must not delete the definition, and an interrupted cleanup after the last unlink must be retried while the project is open and after a server restart.
- **[PRM-462]** The system must maintain a resources instruction block in the supported agent instruction files and must preserve authored text and the blocks of other features. A failed instruction or settings write must be shown at once as synchronisation debt, and `ilium resource repair <slug>` must retry it without recreating the resource or losing a reservation. A routine update must not start an agent turn, and `ilium resource notify-urgent` must send the notice to the idle agents of the project, queue it for the busy ones, skip the calling pane and report every recipient.
- **[PRM-463]** The system must show each linked resource at the top of its project in the tree, after the chatroom, with an indicator for free, taken, attention and queue state. With `[ui] resource_queue_grouping` on it must show owners first and then waiting agents with their global queue positions below the resource, and this grouping must change the display only: pane identities, real parents, split membership, terminal sizes and input routing must stay intact.
- **[PRM-464]** A project must carry a stable resource identity in `.ilium/resource-project-id`, and a moved project must not change its claims until `ilium resource rebind <project-id> <path> --expected-revision <revision>` succeeds. Rebinding must update the linked paths and repair the instructions and must keep the reservation identities and the ticket order.
- **[PRM-465]** While `ilium.core.resources` is disabled or down, the system must keep every definition, link and reservation in storage, must not change an owner and must not grant a successor, and `ilium resource` must fail without changing the catalog. On return the plugin must restore the queues with the same ticket order.
