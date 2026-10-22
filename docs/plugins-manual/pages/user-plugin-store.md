---
layout: page
title: Plugin store
permalink: /pages/user-plugin-store/
---

## Plugin store

The plugin store is where you find plugins in a registry, read about them and install them. A registry is a list of plugins that someone runs and signs. You use the store from Settings, Plugins, Browse, or from the command line. This page shows both.

### You will need

- At least one registry set up. Ilium ships with none. Add one with `ilium plugin registry add <url> --root-key <key_id>`, or ask your administrator ([Plugin administration](/pages/guide-plugin-administration/)).
- To install, read [Install and manage plugins](/pages/user-install-and-manage/).

Requirement ids such as `PKG-530` are in [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

## Search

Type words in the Browse tab, or run `ilium plugin search git --category tools`. Results show the name, publisher, latest version, categories and tier badge.

| Id | Rule |
|---|---|
| [PKG-530] | The store MUST show one row per plugin id with name, publisher, latest version, tier badge and a short description. |
| [PKG-531] | The category filter MUST offer only the closed category list of [Registry reference](/pages/reference-registry/#search-and-categories). |
| [PKG-532] | With no registry configured the store MUST show the empty state "No registry is set up" with the command to add one. It MUST NOT contact the network. |
| [PKG-533] | Yanked versions MUST NOT appear in results. A deprecated plugin MUST show a badge and its successor. |
| [PKG-534] | User ratings are not shown (not in 0.1). |

<figure>
<pre class="term">╭  ≡ ● · Browse ───────────────────────────────╮
│ Search: git                                  │
│ Category: tools                              │
│                                              │
│ ▸ Git tools     Example Labs  1.4.2 T2       │
│   Branch and diff helpers.                   │
│ ▸ Git blame lens Acme        0.3.0 T3        │
│   Deprecated. Use Git tools.                 │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative, not generated. Browse tab with two results.</figcaption>
</figure>

## Read before you install

Open a row to see the description, the requested grants, the dependencies, the size, the publisher key fingerprint and whether the key is already trusted.

| Id | Rule |
|---|---|
| [PKG-535] | The detail view MUST list every requested grant with its risk level, as the review dialog does. |
| [PKG-536] | The detail view MUST show the publisher id and key status: `trusted`, `unknown` or `revoked`. |
| [PKG-537] | A plugin with a revoked key or a blocked id MUST NOT have an Install button. It shows the reason. |

## Install, update and remove

Press Install, or run `ilium plugin install com.example.git-tools`. The review dialog opens. The store never installs without it. Update with the Update button or `ilium plugin update <id>`. Details: [Install and update](/pages/reference-install-and-update/).

| Id | Rule |
|---|---|
| [PKG-538] | Install from the store MUST use the same pipeline and dialog as a file install. |
| [PKG-539] | The store MUST show an update badge when the registry has a newer allowed version. It MUST NOT install it unless the user chooses, or has set `auto_update = install`. |

## When the registry is unreachable

The store shows cached results with their age ("Updated 3 days ago"). Install of a cached package works only if the package file is in the cache.

| Id | Rule |
|---|---|
| [PKG-540] | When offline the store MUST show the cache age and mark the view as stale. |
| [PKG-541] | The store MUST refuse metadata older than the expiry rule and show `registry.metadata-expired`. |

## Edge cases

| Case | Result |
|---|---|
| Two registries list the same id | The registry listed first in the configuration wins. The detail view names the source. |
| Search has no match | The view shows "No plugins found" and keeps the filters. |
| Policy blocks a plugin | The row shows "Blocked by policy". |

## See also

- [Install and manage plugins](/pages/user-install-and-manage/)
- [Registry reference](/pages/reference-registry/)
- [Plugin administration](/pages/guide-plugin-administration/)

## Production additions: T7-users-privacy-a11y

- [USR-347] Every error code the user can see in a notice or CLI message MUST have a catalogue entry with a one-sentence cause and a one-sentence next step, and `ilium plugin explain <code>` MUST print them and exit 0, or exit 2 for an unknown code. Test: iterate the error code table and assert that every user-visible code has both sentences; run `ilium plugin explain` on one known code and one unknown code and assert exit 0 and exit 2.
- [USR-349] A plugin's Details page MUST show its `contact` and `security_contact` from the manifest as text only (never opened without the link-consent prompt of USR-083), and `ilium plugin report <id>` MUST print them with the version and key fingerprint and send nothing. Test: open the Details page of a plugin that has both contacts and assert that the text appears with no link opened; run `ilium plugin report <id>` with the network blocked and assert that it prints the contacts, version and fingerprint and makes no request.
