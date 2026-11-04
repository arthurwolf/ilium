---
layout: page
title: Yank, deprecate and takedown
permalink: /pages/publish-yank-deprecate-takedown/
---

## Yank, deprecate and takedown

A published version is never deleted by its publisher ([PKG-399](/pages/reference-registry/#publishing)). A publisher who wants users to stop getting a version has four tools: `yank`, `deprecate`, `unlist` and the operator's `takedown`. Each one changes what the registry offers and what an installed copy does, and only some of them can be reversed. This page gives the decision table, the effect of each action on installed copies and lock files, and a worked example. The state names of the version come from [Publisher workflow and version states](/pages/publish-publisher-workflow/#version-states-on-the-registry).

### Choosing the action

| Situation | Action | Who runs it | Reversible |
|---|---|---|---|
| A version has a bug, but it is not a security problem. | `yank` with a reason. | Publisher | Yes, `unyank`. |
| A version has a security problem. Users must not install it, and the fix is published. | `yank` with a reason that starts with `security:`. | Publisher | Yes, but do not reverse it while the problem is open. |
| The whole plugin is abandoned or replaced by another plugin. | `deprecate` with a message and, when there is one, a successor id. | Publisher | Yes, by a new `deprecate` without the message. Proposed, owner to confirm. |
| The publisher wants the plugin to stop showing in search, but existing users must keep updating. | `unlist`. | Publisher | Yes. |
| The version is malware, a hidden data collection, an impersonation or a policy breach. | `takedown` by the operator. | Operator | Only by appeal, as a new version ([PKG-404](/pages/reference-registry/#policy)). |
| The publisher key or token leaked. | Key compromise procedure. | Publisher and operator | See [Registry key compromise](/pages/reference-registry-key-compromise/). |

### What each action does

| Effect | `yank` | `deprecate` | `unlist` | `takedown` |
|---|---|---|---|---|
| Changes `index.json` | Version is excluded from `latest_stable` and `latest_beta` when it was the newest. | Adds the `deprecated` object. | Removes the plugin. | Removes the plugin when the whole id is taken down. |
| Changes `plugins/<id>.json` | Sets the yank flag and reason on the version. | Not changed. | Not changed. | Adds the version or id to `blocked_ids` in `revocations.json`. |
| Blob | Kept. | Kept. | Kept. | Removed ([PKG-403](/pages/reference-registry/#policy)). |
| Installed copy that matches | Keeps running. Not chosen for updates. | Keeps running. The user sees the message and the successor. | Keeps running and updates. | Moves to `disabled-blocked` at the next metadata refresh. |
| New install by exact version | Refused with `registry.yanked` unless `--allow-yanked` ([PKG-420](/pages/reference-registry/#client-behaviour-on-each-failure)). | Allowed, with the message shown. | Allowed. | Refused. |
| New install by range | Skipped silently ([PKG-420](/pages/reference-registry/#client-behaviour-on-each-failure)). | Allowed. | Allowed. | Refused. |
| Lock file `ilium-plugins.lock` | The locked version still resolves for `--locked` installs. | Still resolves. | Still resolves. | Fails with `dependency.lock-mismatch` or a takedown message. |

**[PUB-008]** `yank` MUST require `--reason <text>` and MUST store the reason in the signed metadata, and the reason MUST appear to the user in `ilium plugin info`. Proposed, owner to confirm: the reason has at least 10 characters and at most 200 characters.
**[PUB-009]** A yank whose reason starts with `security:` MUST be applied by clients with `auto_update = install` at the next safe point, as [Registry reference](/pages/reference-registry/#notice-to-users-running-a-yanked-version) requires. Test: yank a version with `security:` in the reason, then check that a client with `auto_update = install` moves to the next version without a user action, and that a client with `auto_update = notify` only shows the notice.
**[PUB-010]** `yank` and `unyank` MUST NOT change the hash, the signature or the length of any blob. Test: compare the blob's `snapshot.json` entry before and after a yank and an unyank; the three values are equal.
**[PUB-011]** A `security:` yank MUST NOT be reversed by `unyank` while the reason is not withdrawn. Proposed, owner to confirm: `unyank` of a security yank requires the operator's confirmation, recorded in the transparency log ([PKG-802](/pages/reference-registry/#transparency-log-documents-and-inclusion-proofs)).
**[PUB-012]** `deprecate` MUST NOT change installed copies or block any install. It MUST show the message and the successor id when the successor is in the registry. Test: deprecate a plugin with a successor, install it, and check that the install succeeds and prints the message once.
**[PUB-013]** `unlist` MUST remove the plugin from `index.json` and from search, and MUST keep `plugins/<id>.json`, every blob and the lock resolution, as [PKG-398](/pages/reference-registry/#publishing) requires. Test: unlist a plugin, then run `ilium plugin update` on an installed copy and check that the update still succeeds.

- **[PUB-047]** The registry MUST refuse a yank reason, a deprecate message or a takedown reason that contains a control character other than line feed, with `registry.reason-invalid`; a client MUST render these texts as plain text, MUST NOT interpret them as links or terminal escapes, and MUST truncate them at 500 Unicode scalar values on screen (proposed, owner to confirm).

### Takedown

A takedown is the operator's removal. It is not a publisher action. The operator adds the version or the id to `blocked_ids` in `revocations.json`, removes the blob, and records the reason and the time ([PKG-403](/pages/reference-registry/#policy)). Clients apply it at the next metadata refresh.

**[PUB-014]** A takedown MUST record the reason, the time and the rule of [Content policy](/pages/reference-registry/#policy) that applies, in `revocations.json` and in the transparency log. The reason shown to users MUST be the text the operator publishes in `policy.json`, not a private note.
**[PUB-015]** A takedown MUST NOT delete the signed version entry. It removes the blob and marks the entry as blocked, so the hash remains known and a client can explain why an installed copy is disabled.
**[PUB-016]** After a takedown, every maintainer of the publisher id MUST receive a notice ([SEC-723](/pages/reference-trust-and-supply-chain/)), and the notice MUST include the appeal address from `policy.json`.

- **[PUB-048]** After a takedown, the registry MUST send a notice to every maintainer of every plugin whose latest release has a non-optional `requires.plugins` range that only the taken-down version satisfies, and MUST record the count of such dependents in the takedown's transparency log entry (proposed, owner to confirm).

### Worked example: a security yank

Version 0.3.0 of `com.example.notes` writes the auth token into a log file. Ana publishes 0.3.1 with the fix first, then yanks 0.3.0.

<pre class="term"><span class="t-prompt">$</span> ilium plugin publish dist/notes-0.3.1.iliumpkg --registry https://registry.example.org
<span class="t-ok">ok</span>   com.example.notes 0.3.1 live
<span class="t-prompt">$</span> ilium plugin yank com.example.notes@0.3.0 --reason "security: writes the auth token to the log file"
<span class="t-ok">ok</span>   com.example.notes 0.3.0 yanked, notice sent to 2 maintainers
</pre>

What follows for each kind of user:

- A user with `auto_update = install` gets 0.3.1 at the next safe point ([PUB-009](#what-each-action-does)). Pins are overridden for this case ([PKG-358](/pages/reference-registry/#notice-to-users-running-a-yanked-version)) unless an administrator pins the version in policy.
- A user with `auto_update = notify` sees a notice and runs `ilium plugin update com.example.notes` when ready.
- A user who runs `ilium plugin install com.example.notes@0.3.0` is refused with `registry.yanked`.
- A team with `ilium-plugins.lock` pinned to 0.3.0 sees the lock still resolve, and must run `ilium plugin update` to move forward.

Ana does not run `unyank` for 0.3.0. The reason stays until the issue is closed ([PUB-011](#what-each-action-does)).

### Edge cases

- **A yank of the only live version.** The plugin has no `latest_stable` after the yank. A new install by range fails with `registry.not-found`. An installed copy keeps working.
- **A yank during a takedown.** The takedown wins. The yank flag stays, and the version is shown as blocked.
- **A deprecate with a successor that is not in the registry.** The message is shown, and the successor is shown as text only. Proposed, owner to confirm: the registry refuses a successor id that does not exist with `registry.not-found`.
- **A user who has already run the yanked version.** Nothing is removed from the user's machine. Only the update path changes.

### See also

- [Publisher workflow and version states](/pages/publish-publisher-workflow/): the publish steps and error codes.
- [Mirror runbook](/pages/publish-mirror-runbook/): how mirrors carry a yank or a takedown to their users.
- [Registry operator guide](/pages/publish-registry-operator-guide/): who may run a takedown and how it is logged.
