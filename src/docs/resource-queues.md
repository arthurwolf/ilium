# Shared resource queues

Resources coordinate exclusive access to shared hardware, such as a GPU or an individual USB development board, between agents in different projects on the same machine and operating-system user account. Each resource has one owner and a first-come, first-served queue. Queue rules are instructions to cooperating agents; Ilium does not stop unrelated programs from using the hardware.

## Create or link a resource

Open a project's context menu and choose **New resource…**. Enter its name, project-local slug, description and any cleanup or usage rules. Select Generic, GPU or USB development board and optionally enter a stable device identity. A board serial or GPU identifier is more useful than an interchangeable device path. Creation also links the resource to that project.

Choose **Link existing resource…** in another project to share the same queue. Select the existing definition and its local slug. Creating another resource with a similar name creates another queue; linking is how projects coordinate on the same hardware. A supplied device fingerprint must be unique, including when editing a definition.

Definitions retain their exclusive FIFO policy and creation/update times; project links retain their creation time. Definitions and reservations live in structured application data. Project settings reference the shared resource. Closing its creator project does not delete it. Global definition edits affect every linked project; local slugs belong to their projects. Busy links cannot be removed until their reservations are explicitly handled.

Ilium maintains a resources instruction block in supported agent instruction files. Existing authored text and other feature blocks are preserved. Instruction or settings write failures appear immediately as synchronization debt. Use the detail view’s repair action or `ilium resource repair SLUG` after correcting the file problem; repair does not recreate the resource or lose reservations.

## Agent command workflow

`ilium ressource` is an alias for `ilium resource`. Commands return structured JSON lines. Inside an Ilium pane the agent identity is inferred; an explicit agent reference must resolve to a live pane and invocation. A display title alone cannot authorize another agent's reservation.

```sh
ilium resource list
ilium resource show gpu
ilium resource reserve gpu 10m
# The requested explicit-agent form is also supported:
ilium resource reserve <agent-reference> gpu 10m
ilium resource wait <reservation-id>
ilium resource status <reservation-id>
ilium resource estimate <reservation-id> 20m
ilium resource release <reservation-id>
ilium resource cancel <reservation-id>
```

Examples use illustrative names and placeholders. Durations require units, such as `30s`, `10m` or `1h30m`, and range from one second to seven days. Estimate updates specify total occupancy from acquisition, not additional time.

Keep the reservation and request identifiers returned by reserve. A successful reserve may return **queued**: it does not grant hardware access. Only an owned result, or a successful wait for that same reservation, confirms current ownership. Retrying a mutation uses the same request identifier; do not invent another reservation after a lost acknowledgement. Old request identifiers outside the supported retry window are refused rather than treated as new work. The CLI automatically retries explicit temporary catalog-busy responses with the same request and correlation identifiers within the command deadline. It does not automatically retry an uncertain outcome or a revision conflict.

`wait` blocks until the reservation owns the resource or reaches a terminal result. It has no default timeout and automatically displays queue progress. Interrupting or disconnecting a wait detaches its observer; the reservation remains. Cancel a queued reservation explicitly when it is no longer needed. Once owned, stop and clean up your device work before releasing. One agent invocation may have only one active reservation across resources; release between phases requiring different devices.

## Tree, detail and progress displays

Linked resources appear at the top of their projects, after the chatroom. UTF-8 indicators show free, taken, attention and queue state. Select a resource to open its right-side detail view with description, rules, owner, ordered waiting list, estimates and management actions.

**Group agents under resources** defaults on and can be disabled in settings. This temporarily displays owners and waiting agents below the resource. Owners appear first; multiple waiters have global queue position numbers. Participants from other projects are marked references. Grouping changes display only: pane identities, real parents, split membership, terminal sizes and input routing remain intact. Disabling grouping retains resource rows and waiting badges.

The detail view also retains recent lifecycle events and offers estimate, queued cancellation and release actions for a verified local invocation. These actions keep the selected reservation and ownership generation even if the queue changes.

A blocking wait shows a segmented queue strip alongside ordinary progress monitors. Participant colors remain stable; names and positions also identify entries. Segment lengths describe estimated occupancy, not completed work or guaranteed access. Resource waiting uses separate admission from the eight ordinary task monitors. Display preferences never change reservations.

## Moving a linked project

The project carries a stable resource identity in `.ilium/resource-project-id`, mirrored in project settings. Keep that metadata when moving the directory. After opening the moved project in its own server, inspect the catalog and explicitly rebind its existing identity and canonical new path at the current catalog revision:

```sh
ilium resource rebind <project-id> <canonical-new-path> --expected-revision <revision>
```

Rebinding updates linked paths and repairs instructions while preserving reservation identities and ticket order. A moved project cannot mutate its claims until this explicit reconciliation succeeds. Copying a project’s resource identity into another independently used directory requires resolving that identity collision before both can coordinate safely.

## The resource control panel

Click a resource in the tree to open its control panel, or right-click it for a section of resource actions (open the panel, edit, pause or resume the queue, mark the hardware offline, remove from this project, delete). The panel shows a status badge, summary cards (status, who holds it, how many wait, how busy it was in the last 24 hours), the queue table, a graph of recent hold times, the details of every request, the projects using the resource and an activity log. Buttons are grouped as Resource, Queue, Recovery and Remove. Hover a button for a plain explanation; a greyed button explains why it cannot be used now, and clicking it shows the same reason. Each button also has a keyboard key, listed in the help (`?`).

In plain words: "Take back" removes a stuck owner and pauses the queue, "Confirm safe" says the hardware is safe so the next agent can start, "Pause queue" stops new grants without taking the hardware from its holder, and "Mark offline" means the hardware is unavailable. "Delete" removes the resource from this project and then deletes it; it is refused while agents still use or wait for it, or while another project shares it.

## Estimates and recovery

Estimates are advisory. An overdue owner still owns the resource, and following start estimates become unknown. A missing server or uncertain process observation cannot prove hardware is free. An owner's definitive exit or an unclean reboot requires attention because a child workload or device state may survive the agent.

Marking a resource unavailable preserves its owner and queue; release while unavailable does not grant a successor. When the owner is provably gone (the machine rebooted, or the owning agent process exited), the reservation shows `needs_attention`. An agent of a project linked to the resource clears it in one step with `ilium resource recover SLUG --reason "what you checked"`, run from its own pane; no typed confirmation or copy-paste is needed, and the reason is kept in the resource history. The agent must first make sure no workload still uses the device. A live owner is never cleared this way: recovery of a live owner first revokes the specified owner and pauses the resource. After checking that the physical resource is clear, a separate fresh interactive confirmation can reopen the queue. Recovery checks the current revision and cannot silently target a replacement owner. It never kills processes, resets boards or claims to physically fence another user of the hardware. Same-user processes share application authority, so interactive confirmation prevents accidents rather than malicious bypass.

Real GPU and USB behavior requires hardware qualification. Automated synthetic-resource tests establish queue behavior without taking over a real device. Remote machines, multiple operating-system users, capacity pools, priorities and atomic multi-resource bundles are outside this feature.
