---
layout: page
title: Extension security
permalink: /pages/reference-extension-security/
---

## Extension security

Extension points let one plugin add data or code to another plugin's flow ([Extension points](/pages/api-extension-points/)). They are the largest new attack surface of the platform, because they connect plugins to each other and to the core plugins that carry Ilium's features. This page sets the security rules for hooks, collect points, replacement of core plugins and project values. It is the source of the risk marks of the core points listed in [Core extension points](/pages/reference-core-extension-points/).

Terms. A **point** is a named place where a plugin accepts contributions. The **owner** provides the point; a **contributor** adds to it. A **hook** is a point where contributors' code runs (modes `observe`, `transform`, `veto`, `first`). A **collect** point gathers entries (data). A **data class** is a label for the kind of information in a payload field.

### You will need

- [Extension points](/pages/api-extension-points/) and [Extending core plugins](/pages/extending-core-plugins/).
- The attacker classes A4, A5, A6 and A7 in the [Threat model](/pages/reference-threat-model/).

## Risk of a point

**[SEC-280]** A point is **high** risk when any of these is true:

1. its payload (or an entry's field) carries data covered by an H grant, or terminal, agent, clipboard, file-content or secret data;
2. a transform reply, a first reply or a collect entry determines text sent to a terminal or an agent, a file written, a program run, a network destination, or instructions given to an agent;
3. a veto of the point blocks input to a terminal or an agent, or a message to many agents.

A third-party owner cannot choose `standard` for a point that meets these criteria: the host raises the risk to `high` at install and refuses a manifest that declares `risk: "standard"` for such a point with `manifest.point-risk`.

**[SEC-281]** Data classes. Each field of `payload_schema` and `entry_schema` MAY carry an annotation `x-ilium-data` with one of: `terminal-text`, `agent-prompt`, `file-content`, `path`, `secret`, `user-text`. A point with any field of class `terminal-text`, `agent-prompt`, `file-content` or `secret` is `high`. The review dialog lists the classes in the contributor's **Adds to other plugins** row.

**[SEC-282]** A contributor to a point with a sensitive data class MUST also hold the grant that would let it read the same data directly: `terminal.read-screen` or `terminal.history` for `terminal-text`, `agent.read-prompts` for `agent-prompt`, a matching `fs.read` for `file-content`. A contributor that lacks the grant is refused at install with `manifest.grant-dependency`. Class `secret` has no read grant, so it needs no grant dependency: it raises the point to `high` and the review dialog lists it, nothing more. This closes data laundering through an owner (class A5).

**[SEC-283]** An entry that drives a privileged action of a T1 owner (a post-create step, an instruction block, a target file, a rule pack that sends text, a provider profile that redirects inference) MUST be validated by the owner with an explicit allowlist of what an entry can make it do. The review dialog shows a line `Can make <owner> <action>` per such entry type. The owner rejects any entry outside the allowlist with `extension.entry-rejected`.

## Fail-open and veto points

**[SEC-287]** Policy for failure. A handler that times out, traps or errors normally counts as a pass (fail open), so a slow handler cannot block the user. This is a deliberate trade between availability and safety. Hooks are **not a security boundary** on standard points; an administrator who needs enforcement uses the admin policy (`extensions.deny_points`), which the kernel enforces without handler code.

For `high` veto and transform points the owner declares `on_failure`: `"pass"` or `"refuse"`. It is allowed only when `risk` is `high` and `mode` is `veto` or `transform`; elsewhere the manifest fails with `manifest.extension-point-invalid`. The default for a high-risk point is `"refuse"`. With `refuse`, a handler failure refuses a plugin-initiated action with `extension.vetoed` (reason `guard-failed`). For an action the user started (the definition in [SEC-291]), the kernel shows a prompt `A guard did not answer. Continue anyway?` and proceeds only if the user agrees; the answer is audited.

**[SEC-288]** Order key. The run order is: user order first, then tier band (T1 before T2 and T3), then priority, then plugin id, then extension id. Declared priorities are kept and `ExtensionEntry.priority` returns the declared value. A T2 or T3 contributor therefore never runs before a T1 contributor of the same point unless the user moves it, and its priority orders it only among its own tier band.

**[SEC-284]** A transform handler's output is validated against `payload_schema` before the next handler runs, and every field not listed in `reply_schema.properties` must equal its input value. A failing output is treated as a pass and logged `extension.schema-mismatch`. For a point of mode `transform` whose result drives a privileged action, the owner MAY set `final_check: true`: after all transform handlers have run, the host calls once each contributor entry that declared `guard: true` on the final payload; any `refuse(reason)` refuses the action (`refused(conflict)`, `extension.vetoed`). This makes sure a guard sees the data the action will use.

**[SEC-286]** On `veto` points the handler sees the final payload (veto points have no transforms). On a point with `final_check` the guard sees the output of the transform chain, not the original.

**[SEC-285]** Withdrawn, see [HST-472].

**[SEC-289]** Nesting. A handler can start another hook chain with `run-hook`. The outermost chain is depth 1. A `run-hook` from a handler at depth 3 fails at once with `extension-error` `reentrant`, base code `conflict` and a message that starts with `extension.depth-exceeded`. It counts as a pass for the handler that caused it and not as a breach. A `run-hook` on any point already on the current chain stack, directly or indirectly, fails with `extension.reentrant`. The inner chain runs after the handler returns and its time does not count toward the outer deadline.

**[SEC-290]** Budgets. Per point the owner may call `run-hook` at most 200 times per second; further calls return `rate-limited`. Per handler, CPU time is limited to 50 ms per second; above it, calls to that handler are skipped for the rest of the second and count as a breach. A **breach** is defined in [Extension points](/pages/api-extension-points/#rules-for-payloads-deadlines-ordering-and-failure): a timeout, a trap, an error return, a reply over 64 KiB, a schema failure, a CPU budget overrun, or a skip because the contributor's instance was busy. Three consecutive breaches stop the extension until the contributor's instance restarts or the user presses Reset. The notice that announces the stop is drawn by the kernel and lists the plugin, the point and the cause. The audit log records `extension-disabled-after-breaches`.

**[SEC-291]** Vetoes of user actions. A chain is **user-started** when the payload field `source` is `user`, `paste`, `mouse`, `keys` or `manual-trigger`, or, for points without `source`, when the action came from a key, a click or a command the user invoked. A veto on a user-started action can be overridden by the user on these points: `ilium.core.tree/before-close`, `ilium.core.tree/before-move`, `ilium.core.worktrees/before-remove`, `ilium.core.layout/before-split`, `ilium.core.compaction/before-compact` and `ilium.core.terminal/before-send-input` (sources `user` and `paste` only). The kernel shows `<plugin> refused: <reason>. Continue anyway?` with default focus on **No**. An override is audited with the plugin and point. After an override the chain completes `ok` with no reply, and the owner's log gets `extension.veto-overridden` with the contributor and the point. The **User override** column of [Core extension points](/pages/reference-core-extension-points/) marks these points.

**[SEC-292]** Reasons and labels. Text that a contributor supplies (reasons, labels, titles, error text) is plain text of at most 120 characters, with control characters and bidirectional overrides removed ([SEC-380]). In the user interface it is always prefixed by the host with the contributor's display name, so a reason cannot imitate a kernel message. The message of the `extension.vetoed` error itself carries the contributor's plugin id.

## Risk marks of core points

The review of every core point against [SEC-280] gives 19 high-risk points. The [Core extension points](/pages/reference-core-extension-points/) table and each core page carry the same marks. Points not listed here are `standard`.

| Point | New risk | Reason | Notes |
|---|---|---|---|
| `ilium.core.terminal/before-send-input` | high | Input to terminals | `on_failure: refuse`; user prompt on guard failure |
| `ilium.core.terminal/open-link` | high | Decides how a link opens (a program or a URL target) | First-handler answer validated against the allowlist of open targets |
| `ilium.core.smart-copy/after-copy` | high | Sees copied secrets; rewrites clipboard text | Class `secret`, `terminal-text` |
| `ilium.core.editor/before-save` | high | Rewrites any saved file | Class `file-content` |
| `ilium.core.voice/transcript` | high | Changes which command runs | Class `user-text`; entries validated by the owner |
| `ilium.core.agent-detect/classify` | high | Can make a working agent look idle, so actions fire | Output limited to the activity enum |
| `ilium.core.fleet/before-broadcast` | high | Text to many agents | Class `agent-prompt` |
| `ilium.core.inference/before-request` | high | Sees and rewrites every prompt | Class `agent-prompt` |
| `ilium.core.inference/provider-profiles` | high | Redirects inference to another endpoint | Endpoint checked by [Network egress](/pages/reference-network-egress/); owner confirms with the user |
| `ilium.core.notifications/channels` | high | Sends notification bodies to webhooks | Webhook host checked as egress |
| `ilium.core.scheduler/before-send-scheduled` | high | Rewrites or refuses input that is sent later, without the user watching | Class `agent-prompt`; the veto only refuses, so it cannot add text |
| `ilium.core.text-triggers/before-fire` | high | Can stop a rule that is about to send text to a terminal | Veto only: it refuses or ends the rule and cannot change the text |
| `ilium.core.text-triggers/rule-packs` | high | Rules send text to terminals | Rules validated by the owner; see [Project values](#project-values) |
| `ilium.core.chatroom/before-post` | high | Rewrites messages that agents read | Class `agent-prompt` |
| `ilium.core.agent-setup/instruction-blocks` | high | Injects instructions into agent configuration files | Class `agent-prompt` |
| `ilium.core.agent-setup/targets` | high | Adds files that setup edits | Path must lie in `agent-config` scopes |
| `ilium.core.agent-setup/before-install` | high | Can refuse writing an instruction block to an agent file | Veto only: it refuses the write and cannot change the block or the path |
| `ilium.core.worktrees/post-create-steps` | high | Steps run programs | Each step's program needs a `process.run` grant of the contributor |
| `ilium.core.diagnostics/log-sinks` | high | Receives every log record | Records pass through redaction ([SEC-249]) |
| `ilium.core.tree/before-close`, `ilium.core.worktrees/before-remove` | standard | User-overridable ([SEC-291]) | |

**[SEC-293]** The core plugin pages for Smart copy, Editor, Voice, Agent detect, Chatroom, Agent setup, Worktrees, Diagnostics, Notifications, Text triggers, Inference, Terminal and Tree repeat the marks of their own points. A mismatch between a core page and this table is a defect.

## Replacement of core plugins

A plugin with `replaces` takes over a core plugin's surface ([Extending core plugins](/pages/extending-core-plugins/)). Twenty-eight core plugins can be replaced; `ilium.core.sessions` and `ilium.core.settings` cannot.

**[SEC-300]** A replacement has only the grants its own review dialog gave it. It never inherits the grants, consents, data or the replaced plugin's own contributions to other points. Contributions of other plugins to the replaced points are handled by [SEC-304].

**[SEC-301]** A T3 package cannot replace a core plugin (`manifest.replaces-tier`). A T2 replacement requires a confirmation that names the core plugin and lists every H grant the replaced surface needs. Policy `extensions.allow_replacement = false` refuses all replacements with `policy.replacement`.

**[SEC-302]** If the replaced surface needs grants that only T1 can hold (`process.run:sh`, `settings.read-all`, `plugins.manage`), a T2 package that claims the surface fails at install with `manifest.replaces-tier`, in addition to `manifest.replaces-surface-incomplete` for missing items.

**[SEC-303]** Namespace. A replacement serves the `ilium.core.<name>` surface ids, but its commands carry the replacement's display name and a `T2` badge in the palette and in help, and its keybindings rank as T2 (below T1 and user bindings). The ids `ilium.` stay reserved for bundled packages ([Packages](/pages/packages/)); a replacement keeps its own plugin id.

**[SEC-304]** Contributors. Plugins that contributed to points of the replaced core plugin are listed on the Plugins screen as **Now sending to <replacement>**. Their high-risk contributions are suspended until the user confirms them again for the new owner; standard contributions continue after a notice.

**[SEC-305]** Kernel-owned security functions never move to a replacement. They are: the grant check and the admission gate for host calls, the inference privacy log, consent prompts, the review dialog, breach notices, the recovery page, the audit log, safe mode, and the policy.

**[SEC-306]** Turning a replacement on or off writes `replacement-on` or `replacement-off` to the audit log. The replaced plugin's data folder stays on disk and is inaccessible to the replacement and to every other plugin.

## Project values

A **project value** is a setting stored in a repository's `.ilium/` folder. A cloned repository is attacker-controlled input (class A7). Plugin rows with `scope: "project"` and files such as `.ilium/team-rules.json` read by plugins would otherwise take effect when the user opens the folder.

**[SEC-310]** A project folder is **untrusted** until the user trusts it. When the user opens a project whose `.ilium/` holds plugin values, rules or sessions, the kernel asks once: `Trust this project? It contains plugin settings that could change how plugins behave.` The answer is stored per canonical path in the project trust store ([SEC-112]) and can be revoked in Settings > Plugins > Projects.

**[SEC-311]** In an untrusted project, project-scope plugin values are ignored: plugins read their defaults or user values. The `get` call reports `source: "default"` or `"user"`. A notice on the Plugins screen shows how many values were ignored.

**[SEC-312]** Even in a trusted project, rows whose type is `path`, `url`, `host` or command-like text, and any row marked `trust_sensitive: true`, take project values only with the extra step that the first use of each value is shown to the user (the same consent as `http:setting`, SEC-074).

**[SEC-313]** Secrets never come from project files ([SEC-242]).

**[SEC-314]** The policy `[projects] allow_project_values = false` ignores all project values regardless of trust.

**[SEC-315]** Plugins that read files under `.ilium/` themselves, such as the rule file of tutorial 20, MUST ask the host for the project trust state (`project.trusted()` in the SDK) and MUST ignore the file when it is false. They validate the file: at most 64 KiB, at most 200 rules, schema checked, each rule's fields length-capped, and no regex outside the limits of [SEC-046].

## UI integrity

A plugin that can draw text on screen can try to look like the kernel. The rules below keep kernel security UI recognisable.

**[SEC-377]** Kernel security UI (review dialog, trust dialog, consent prompts, breach notices, recovery page, safe-mode badge) is always drawn on top of plugin UI. No kernel dialog opens, and none is imitated, while the user types in a terminal pane: the kernel queues the dialog until the user leaves the pane or presses the dialog key. Keystrokes typed before a dialog appears never reach it ([SEC-103]).

**[SEC-378]** Plugin toasts, tree chrome, palette entries, help pages and intent chrome are drawn in the plugin style. They carry the plugin display name as a prefix that the plugin cannot remove. Only the kernel uses the rounded border of security dialogs.

**[SEC-379]** A plugin cannot read or draw over the cells of a kernel dialog. Screen contents of kernel dialogs are not delivered by `screen-changed` or any capture call.

**[SEC-380]** Every plugin-supplied text shown by the client (labels, titles, toast text, reasons, status segments, help text) is sanitised: C0 and C1 control characters, escape sequences, bidirectional override characters (U+202A to U+202E, U+2066 to U+2069), zero-width characters and box-drawing characters in sequences of three or more are removed or replaced with a space. Text is clipped to its field limit with an ellipsis.

**[SEC-381]** Display names are 3 to 40 characters, must not equal or start with `Ilium` or `Kernel` unless the plugin id starts with `ilium.`, and are checked at install (`manifest.display-name`).

## Worked example: a guard that times out

A T2 plugin contributes a veto handler to `ilium.core.terminal/before-send-input`, sleeping for its whole deadline on each call. With `on_failure: refuse` (the default for the point), every plugin-initiated send is refused with `extension.vetoed` (reason `guard-failed`) after 20 ms; the user's own paste shows `A guard did not answer. Continue anyway?` once, and the user can say yes. After three consecutive breaches the extension is stopped, the kernel draws the notice, and the audit log records it. With the previous fail-open rule, the handler would have been a free way to disable a guard. The test cases are in [Security testing](/pages/reference-security-testing/).

## Edge cases

| Situation | Result |
|---|---|
| Two handlers both rewrite and each exceed their deadline | Both are cut at their deadline; the chain keeps the last valid payload |
| A guard handler uses a nested hook that nests three deep | A `run-hook` from depth 3 returns `extension.depth-exceeded` and counts as a pass for that call |
| The user moves a T2 handler above a T1 handler | The user's order wins; priority is ignored |
| Project becomes trusted and then the repository is updated | Values read now come from the new files; the trust stays by path |
| Owner declares `risk: "standard"` for a point with a `terminal-text` field | Manifest refused with `manifest.point-risk` |

### See also

- [Extension points](/pages/api-extension-points/)
- [Extending core plugins](/pages/extending-core-plugins/)
- [Core extension points](/pages/reference-core-extension-points/)
- [Capability: extensions.contribute](/pages/capability-extensions-contribute/)
- [Threat model](/pages/reference-threat-model/)
- [Plugin settings extension](/pages/settings-extension/)

Contract: `ilium:plugin@0.1.0`.
