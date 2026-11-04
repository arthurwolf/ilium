---
layout: page
title: Mirror runbook
permalink: /pages/publish-mirror-runbook/
---

## Mirror runbook

A mirror is a copy of a registry's signed files that a team or an organisation runs for its own users. This page is the runbook for an operator who keeps a mirror current: how often to sync, how to check the result, how much disk to plan for, what the clients see when the mirror falls behind, and what to do when a sync fails. The rules for full and partial mirrors are in [Registry reference](/pages/reference-registry/#mirrors). The command syntax is in [Distribution commands](/pages/reference-plugin-distribution-cli/#registry-and-mirror).

### You will need

- Read access to the source registry over HTTPS, and the root key id of that registry (the value that `ilium plugin registry add --root-key` pins).
- A folder on a disk with enough free space. See [Disk planning](#disk-planning).
- A way to run the sync on a timer, for example a systemd timer or a cron entry. The sync is one command.
- For a partial mirror, the signing keys of the mirror and its own root file ([PKG-409](/pages/reference-registry/#mirrors)).

### One sync

The sync command copies the signed metadata and the blobs to a folder. It verifies every document and every blob before it writes it ([PKG-439](/pages/reference-plugin-distribution-cli/#registry-and-mirror)).

```
ilium plugin mirror sync /srv/ilium-mirror --registry https://registry.example.org --channel stable
```

Omit `--allow` for a full mirror. Use one `--allow <id>` for each plugin of a partial mirror. The command downloads only the files whose hash changed ([PKG-410](/pages/reference-registry/#mirrors)).

### Sync schedule

A client refuses a metadata document after its `expires` time ([PKG-416](/pages/reference-registry/#client-behaviour-on-each-failure)). A mirror that stops syncing therefore stops working for its users at a known time. The schedule must keep the mirror ahead of that time.

**[PUB-017]** A mirror MUST be synced at an interval of no more than half of the shortest `expires` lifetime of the metadata it serves. Proposed, owner to confirm: the timestamp document expires 24 hours after it is signed, so a mirror syncs at least every 12 hours.
**[PUB-018]** A mirror operator MUST alert when the `expires` time of `timestamp.json` on the mirror is less than 6 hours away. Proposed, owner to confirm: the alert threshold is 6 hours. Test: stop the sync, wait until the threshold, and check that the alert fires before the expiry.

Example systemd timer for a mirror that syncs every 6 hours:

```
[Unit]
Description=Sync the Ilium plugin mirror

[Service]
Type=oneshot
ExecStart=/usr/local/bin/ilium plugin mirror sync /srv/ilium-mirror --registry https://registry.example.org --channel stable

[Timer]
OnCalendar=*-*-* 00/6:15:00
Persistent=true

[Install]
WantedBy=timers.target
```

Run the unit as a user who owns `/srv/ilium-mirror` and has no other rights. The `Persistent=true` line makes a run that was missed while the machine was off start after the next boot. A sync is safe to run at any time, so a late or repeated run does no harm.

### Checking the result

Check the mirror the same way a client would. Add it as a registry in a test profile, pinning the root key from the source registry, and run a search and a verify:

```
ilium plugin registry add file:///srv/ilium-mirror --root-key <key_id>
ilium plugin search notes
ilium plugin info com.example.notes
```

`file://` is accepted for mirrors and bundles ([PKG-438](/pages/reference-plugin-distribution-cli/#registry-and-mirror)). The `info` line shows the same version and state as the source registry.

**[PUB-019]** A sync MUST leave the previous files in place when it fails, and MUST replace each file atomically ([PKG-439](/pages/reference-plugin-distribution-cli/#registry-and-mirror)). A reader who reads the mirror during a sync MUST see either the old file or the new file, never a partial file. Test: kill a sync during a blob download, then check that every file in the folder still verifies.
**[PUB-020]** A mirror MUST NOT change any signed byte. Its files MUST be byte-identical to the source for a full mirror ([PKG-408](/pages/reference-registry/#mirrors)). Test: compare the SHA-256 of each file on the mirror with the source; they match.
**[PUB-021]** A partial mirror MUST re-sign `index.json` and `snapshot.json` with its own keys and MUST publish its own `root.json` ([PKG-409](/pages/reference-registry/#mirrors)). Its operator MUST NOT reuse the source registry's root key.

### Propagating yanks and takedowns

A yank or a takedown reaches a mirror only at the next sync. Until then the mirror serves the old state. This is a delay the operator must plan for.

**[PUB-022]** A takedown ([PKG-403](/pages/reference-registry/#policy)) MUST be applied to a mirror within 6 hours of the source registry publishing it. Proposed, owner to confirm: a mirror operator who cannot meet this MUST remove the affected plugin from the mirror with `--allow` and sync again.
**[PUB-023]** A mirror operator MUST NOT remove a blob from a full mirror on its own decision. Removal follows a takedown or a signed change from the source registry, so that the mirror keeps the source's signed state.

### Disk planning

The size of a full mirror is the sum of the blob sizes and the metadata. A blob is at most 300 MiB ([PKG-400](/pages/reference-registry/#publishing)). An average package is much smaller. The planning rule below uses the real sizes of the source, and not the cap.

**[PUB-024]** A full mirror's folder MUST have free space of at least twice the size of the source's current blobs, so that a sync can write a new file before it removes the old one. Proposed, owner to confirm: the factor of 2 is a safety margin, not a measured need. Test: fill the disk to the threshold, run a sync, and check that the sync fails cleanly and leaves the previous files valid ([PUB-019](#propagating-yanks-and-takedowns)).
**[PUB-025]** A mirror operator MUST NOT store a mirror on a disk that is also the root of the operator's own build cache. Proposed, owner to confirm: keep the mirror on its own filesystem, so that a full build cache cannot stop the sync.

### Failure modes

| Symptom | Cause | Action |
|---|---|---|
| The sync exits 8 `registry.unreachable`. | The source is down or the network failed. | Nothing is damaged. Retry at the next timer run. Check the alert of [PUB-018](#sync-schedule). |
| The sync exits 1 `registry.hash-mismatch`. | A file on the source does not match its metadata. | Do not copy the file by hand. Report the file name and the time to the source registry operator. The mirror keeps the previous file. |
| The sync exits 1 `registry.signature-invalid`. | The source metadata fails the root key or the threshold. | Check that the root key id matches the source registry. If it does, treat the source as compromised and follow [Registry key compromise](/pages/reference-registry-key-compromise/). |
| Clients report `registry.metadata-expired`. | The mirror passed the `expires` time of a document. | Run a sync at once. Clients use the cache until its own expiry, then refuse. |
| Clients report `network` with reason `tls`. | The mirror's certificate is from an internal CA that clients do not trust. | Set `[network] ca_bundle` on the clients ([PKG-987](/pages/reference-registry/#mirrors)). Never disable verification. |

### Worked example: a takedown reaches the mirror

A registry takes down version 0.2.0 of `com.example.old-tools` at 09:40. The mirror's next sync runs at 12:15.

<pre class="term"><span class="t-prompt">$</span> ilium plugin mirror sync /srv/ilium-mirror --registry https://registry.example.org
<span class="t-ok">ok</span>   metadata verified: root v3, timestamp v4822, snapshot v4822
<span class="t-ok">ok</span>   revocations.json updated: 1 blocked id
<span class="t-ok">ok</span>   blob removed: com.example.old-tools 0.2.0
<span class="t-ok">ok</span>   sync complete, 4 files changed, 0 files failed
</pre>

The mirror is within the 6-hour window of [PUB-022](#propagating-yanks-and-takedowns). Clients that refresh metadata after 12:15 move the matching installed copies to `disabled-blocked` ([PKG-403](/pages/reference-registry/#policy)).

### Redistribution of paid and licensed plugins

Some plugins are sold, or check a licence key, and their seller grants use under end-user terms ([Paid and licensed plugins](/pages/reference-registry/#paid-and-licensed-plugins)). A copy of such a plugin on a mirror is a second copy that the seller did not hand out. Copying the blob is not a licence to serve it. The registry marks these versions with `licensing: true` in the index.

A sync therefore skips them by default. The operator decides per plugin whether to serve it, and only with the seller's permission on record.

**[PKG-995]** (proposed, owner to confirm) A mirror operator MUST NOT serve a version whose `licensing` field is `true` unless the operator holds the seller's written permission to redistribute it. The operator MUST keep that permission in a private record of the operator, and the registry and the mirror MUST NOT publish it.

**[PKG-996]** (proposed, owner to confirm) `ilium plugin mirror sync` MUST skip blobs whose `licensing` is `true` by default, and MUST copy them only when the flag `--include-licensed` is given. A sync that skips a blob MUST list the plugin id and version in its output, and the mirror MUST re-sign its `index.json` and `snapshot.json` as a partial mirror under [PKG-409].

### Edge cases

- **The source changes its root key.** The mirror keeps the old root until the operator re-pins the key on purpose. A new root key on the source is a key rotation, and the procedure is in [Release key compromise](/pages/reference-release-key-compromise/) for the project key. For a third-party registry, follow that registry's notice.
- **A mirror is used by a machine with no network.** The mirror is the only source. The air-gapped procedure is in [Registry reference](/pages/reference-registry/#air-gapped-installs). Use `ilium plugin export-bundle` when a single set of plugins is enough.
- **A sync runs while a publish is in progress on the source.** The sync takes the snapshot that the source serves at that moment. The next sync picks up the new version.

### See also

- [Registry operator guide](/pages/publish-registry-operator-guide/): the source side of the same metadata.
- [Yank, deprecate and takedown](/pages/publish-yank-deprecate-takedown/): what the mirror carries to users.
