// Round-thirteen production requirements (PRO IDs), red until implemented.

/// REQ: PRO-001
#[test]
fn pro_001_gap_01() {
    // Observable: No text defines what the shell does when the client terminal is smaller than the smallest layout (the manual defines 40-column compact use but nothing below 40 columns or below a minimum row count). rg for 'too small', 'below N columns' and minimum size finds only unrelated hits.
    panic!("PRO-001 not implemented: The system must, when a client terminal is narrower than 20 columns or shorter than 6 rows, replace the whole screen with one line \'Terminal too small: need 20x6, have WxH\' in the plain role, must call views.render for no plugin view while that state lasts, must keep server work, panes and PTYs running, and must restore the previous layout, focus and scroll positions within 100 ms of the first resize that meets the minimum.");
}

/// REQ: PRO-002
#[test]
fn pro_002_gap_02() {
    // Observable: widgets-input.md says text fields use the real terminal cursor, but nothing requires the terminal cursor to follow keyboard focus on non-text widgets. Screen magnifiers and many screen readers track the hardware cursor, so focus on a list row, button or tab is invisible to them.
    panic!("PRO-002 not implemented: The system must park the terminal cursor on the first cell of the focused widget (or on the selected row for list-like widgets) after every render and focus change, must hide it only in views that declare a text-input focus elsewhere, and a harness test that moves focus through ten widgets must read back ten distinct cursor positions equal to each focused widget\'s origin cell.");
}

/// REQ: PRO-003
#[test]
fn pro_003_gap_03() {
    // Observable: The terminal bell (BEL, 0x07) is not mentioned anywhere in the core plugin pages; it is how many shells and agents ask for attention, and it interacts with sound muting, reduced motion, notifications and screen-reader mode.
    panic!("PRO-003 not implemented: The Terminal plugin must handle a BEL from a pane by marking that pane\'s tree row with the attention mark, playing the bell sound only when sound is not muted, showing a one-frame non-flashing visual cue instead when sound is muted or the pane is in screen-reader mode (announced as \'Bell in <pane title>\' within 1 second), collapsing bursts to at most 1 bell effect per pane per second, and never sending a desktop notification for BEL from a pane that has client focus.");
}

/// REQ: PRO-004
#[test]
fn pro_004_gap_04() {
    // Observable: Sessions defines snapshots, backups and corrupt-snapshot recovery but no rules for session names: allowed characters, length, collisions with an existing session, reserved names, or names that differ only in case or Unicode normalisation (file system and socket paths derive from them).
    panic!("PRO-004 not implemented: The Sessions plugin must accept session names of 1 to 64 Unicode characters after NFC normalisation, must refuse with refused(invalid-name) names containing path separators, control characters or only dots/spaces, must refuse with refused(exists) a name equal to an existing one under case-insensitive NFC comparison, and must derive the on-disk file name so that two accepted names never map to one file on case-insensitive file systems.");
}

/// REQ: PRO-005
#[test]
fn pro_005_gap_05() {
    // Observable: Only 'no provider selected' and quota failures are specified. Nothing says what happens when a model a user or plugin selected is withdrawn or renamed by the provider, or when a selected local model file is deleted.
    panic!("PRO-005 not implemented: When a request names a model that the provider\'s catalogue no longer lists, the Inference plugin must fail it without sending prompt text with not-found and detail \'model-withdrawn\', must mark the model [unavailable] in the ilium.models catalogue within one refresh interval, must show one dismissible notice per model naming every plugin and setting that selected it, and must not silently substitute another model.");
}

/// REQ: PRO-006
#[test]
fn pro_006_gap_06() {
    // Observable: The Terminal plugin page documents sequences and unsupported protocols but gives no testable limits or rules for scrollback size, selection and copy across wide/combining characters, or what happens to scrollback when memory is short.
    panic!("PRO-006 not implemented: The Terminal plugin must keep a configurable scrollback of 100 to 1,000,000 lines per pane (default 10,000), must evict oldest lines first without moving the viewport of a user who is scrolled up, must copy a selection that spans wide characters and combining sequences without splitting a grapheme cluster, and must report in the pane footer \'scrollback truncated\' once per eviction burst when the user is scrolled up.");
}

/// REQ: PRO-007
#[test]
fn pro_007_gap_07() {
    // Observable: Panels declare modal or non-modal, but nothing defines how several plugins' modal requests stack when one is already open (for example a consent dialog, a plugin modal and a confirm), or how the shell prioritises its trusted dialogs over plugin modals.
    panic!("PRO-007 not implemented: The host must allow at most one plugin modal at a time, queue further plugin modal requests first-in first-out up to 4 per plugin and refuse the 5th with refused(rate-limited), must draw host-owned consent, confirm and security dialogs above any plugin modal without closing it, and must return focus to the plugin modal only after the host dialog closes.");
}

/// REQ: PRO-008
#[test]
fn pro_008_gap_08() {
    // Observable: Core plugins are discoverable in Settings, but there is no requirement for first-use discovery of a feature the user has never used or of an optional core plugin that is off by default (for example Voice or Kanban); PRR-025 only covers upgrades and PRQ-007 only a triggered disabled feature.
    panic!("PRO-008 not implemented: The system must list in Settings > Plugins, for every core plugin that is off, a one-line description, its grants and its disk or network cost (for example speech model size), must let the user enable it with one command that shows the grant review in the same dialog, and must show no unsolicited prompt to enable any plugin more than once per host install.");
}

/// REQ: PRO-009
#[test]
fn pro_009_gap_09() {
    // Observable: ilium plugin dev finds the author's existing session (DEV-004) and installs the in-development plugin into it. DEV-711 only separates the plugin's data folder. An unfinished plugin with --grant-all (DEV-184) can therefore read the real panes, terminal text and agent transcripts of the author's working session. No scratch or disposable session mode exists.
    panic!("PRO-009 not implemented: The system must offer ilium plugin dev --scratch-session, which starts a disposable session with its own state, runtime and socket folders and a fixture tree of 3 panes, installs the plugin only there, never contacts or reads any other session, and removes the session on exit; the first dev run in a project with no --session must print a notice that the plugin runs in the live session and name this flag.");
}

/// REQ: PRO-010
#[test]
fn pro_010_gap_10() {
    // Observable: PRQ-024 classifies public-surface changes, but the surface in versioning.md does not include the grant set. A new version that adds a grant forces re-consent for every installed user (DEV-120 shows it only in dev). Nothing tells the author at check or publish time, and nothing says a grant increase needs a minor bump.
    panic!("PRO-010 not implemented: The system must make ilium plugin check --baseline and publish report the grant delta against the baseline version as check.grant-added or check.grant-removed records. A version that adds any grant with a patch-level bump must fail with lint.semver-bump-too-small, and a high-risk grant addition must print a warning that every installed user will be asked again.");
}

/// REQ: PRO-011
#[test]
fn pro_011_gap_11() {
    // Observable: PRQ-024 does not say where the baseline comes from or what happens without one. A first release, an offline run, a yanked baseline version and a baseline from a different channel are all undefined, so CI results can differ between machines.
    panic!("PRO-011 not implemented: The system must resolve --baseline from an explicit package file, or else from the highest non-yanked stable version of the same id in the configured registry. With no baseline it must exit 0 with baseline.none-first-release, when the registry is unreachable it must fail with registry.unreachable unless a file was given, and the resolved baseline version and SHA-256 must appear in the summary record.");
}

/// REQ: PRO-012
#[test]
fn pro_012_gap_12() {
    // Observable: PRR-009 covers beta host contracts only. The manual says nothing about SDK pre-releases: npm dist-tags and crates.io pre-release ordering, whether a published plugin may pin a pre-release SDK, or how an author tries a beta SDK without risking a release.
    panic!("PRO-012 not implemented: The system must publish each SDK minor as a pre-release (-beta.N) at least 7 days before stable. ilium plugin new and upgrade-sdk must not select a pre-release unless --sdk-channel beta is given, and pack and publish must refuse a project whose resolved SDK version is a pre-release with pack.sdk-prerelease.");
}

/// REQ: PRO-013
#[test]
fn pro_013_gap_13() {
    // Observable: Translation files get only runtime checks: a placeholder mismatch falls back to English with i18n.placeholder-mismatch (USR-312, USR-034), and the lint catalogue has only lint.help-translation-orphan and lint.text-plural-missing-other. The author learns of a broken translation from a user's log, not from check. The manual has no extract-strings or catalogue-completeness workflow.
    panic!("PRO-013 not implemented: The system must make ilium plugin lint compare every i18n/<tag>.json with i18n/en.json and report lint.i18n-placeholder-mismatch (error), lint.i18n-key-missing (warn), lint.i18n-key-unused (warn) and lint.i18n-plural-category-invalid for the language (error), each naming file, key and tag. A fixture with one flaw of each kind must give exactly four findings, and a correct fixture none.");
}

/// REQ: PRO-014
#[test]
fn pro_014_gap_14() {
    // Observable: PRQ-029 gives consumers stub and fail service helpers. A provider of a versioned service (services.md, SVC rules) has no way to check its own implementation against the published service schema and semantics. Only the host enforces the schema at run time (SVC-003, SVC-009), so a provider can ship a method that fails its own result schema.
    panic!("PRO-014 not implemented: The system must provide host.verify_service_provider(service, version) in the harness (verifyServiceProvider in TypeScript). It must call every method of the provider schema with generated valid params, fail with harness.service-result-invalid naming the method when a result does not match the result schema, and fail with harness.service-method-missing for a declared method the plugin does not implement. The seed must be reproducible with ILIUM_PLUGIN_TEST_SEED.");
}

/// REQ: PRO-015
#[test]
fn pro_015_gap_15() {
    // Observable: A static-tree registry cannot count downloads (reference-release-rollout), and the manual has no author-facing signal on adoption, version spread, crash or disable rate. A publisher releasing or yanking a version has no evidence that it works in the field. The only channel is PRR-012, which is user-initiated.
    panic!("PRO-015 not implemented: The system must offer opt-in anonymous health aggregates per plugin id and version (active installs, crash restarts, user-disabled count). They must reach a publisher only when at least 20 distinct installs contribute to the bucket, must contain no machine id, path or plugin data, and must be readable with ilium plugin stats <id>. With telemetry consent off, the host must send nothing and stats must print stats.unavailable.");
}

/// REQ: PRO-016
#[test]
fn pro_016_gap_16() {
    // Observable: PRQ-009 handles data carry-over only for replacing a core plugin. A third-party plugin that is deprecated or reaches end of life with a successor (PKG-397, PKG-710, PUB-012) has no defined way for the successor to read the old plugin's kept data, so users start from nothing.
    panic!("PRO-016 not implemented: The system must let a successor plugin declare imports_from = [<old id>] in its manifest. After the user confirms, the host must give the successor a one-time read-only view of the old plugin\'s kept storage and settings values, report migrated and unmatched key counts, leave the old data untouched, and refuse with import.owner-mismatch unless the old plugin\'s deprecation names this successor id.");
}

/// REQ: PRO-017
#[test]
fn pro_017_gap_17() {
    // Observable: DEV-069 requires generated API documentation per SDK version, but nothing requires that every public SDK item is documented or has an example. A new exported function can ship with an empty doc page, and the manual's sample check (PRW-029) only covers the manual's own samples.
    panic!("PRO-017 not implemented: The system must fail the SDK release pipeline when any public item of ilium-plugin-sdk or @ilium/plugin-sdk has no documentation text, or when a public function that takes a grant lacks a doc line naming that grant, a doc line naming its error codes, and an example that compiles. The failure must name the item.");
}

/// REQ: PRO-018
#[test]
fn pro_018_gap_18() {
    // Observable: A storage database that fails its integrity check (HST-657) is moved aside and the plugin starts with empty storage. Nothing requires telling the user or the plugin, and lifecycle.md:526 says activate carries no flag about the past. The plugin cannot tell data loss from a fresh install, and the user is never offered the pre-migration copy or an archive.
    panic!("PRO-018 not implemented: The system must, when it moves a corrupt storage database aside, show one notice naming the plugin, bucket and moved-aside file, write a storage.reset record with the operation id, pass the plugin a one-time data-reset reason on its next activate, and list the newest pre-migration backup and newest archive that contain that bucket in `ilium plugin status`, restorable with one command that passes the bucket integrity check.");
}

/// REQ: PRO-019
#[test]
fn pro_019_gap_19() {
    // Observable: Automatic rollback applies only when the first activate fails. A new version that activates once and then crash-loops to stopped-by-failure never rolls back by itself and just stays stopped. The runbook says only that the user must roll back manually, and no probation window or policy key exists.
    panic!("PRO-019 not implemented: The system must, when a plugin updated within the last 30 minutes reaches stopped-by-failure on its new version and the previous version is still in the store, show a notice with a one-command rollback and, when the admin policy key plugins.auto_rollback_on_probation is true, roll back automatically, mark the new version skip, log update.rolled-back with reason probation and write one audit record.");
}

/// REQ: PRO-020
#[test]
fn pro_020_gap_20() {
    // Observable: Restore has rules for space, format, expansion and server-running, but the manual does not define what a finished restore on another machine or account must report. Grants, consents and trust are skipped, rebuilt or tampered-renamed (HST-959), and secrets only with --include-secrets. The admin has no single machine-readable account of what still needs action.
    panic!("PRO-020 not implemented: The system must end `ilium plugin restore` (and --dry-run) with one `restore-summary` JSONL record listing per plugin id: data restored, grants needing re-review, secrets needing re-entry, trust state (kept or tampered-renamed), and plugins left disabled, and must exit 0 only when no plugin needs action, otherwise exit 5 with the same list in the human summary.");
}

/// REQ: PRO-021
#[test]
fn pro_021_gap_21() {
    // Observable: Redaction is specified per output (bundle USR-056, logs, backups, crash records), but no test seeds a known secret and checks every egress surface together. PRR-014, PRQ-014 and PRQ-017 cover host-bound use, short values and taint, not total leak coverage.
    panic!("PRO-021 not implemented: The system must pass a conformance case that sets a unique 32-character secret row value, exercises the plugin through log, crash, trap, settings export, config export, audit, support bundle (single and --all), trace file, telemetry summary and backup without --include-secrets, then scans every produced byte string, raw, base64 and percent-encoded, and fails the release naming each output that contains the value.");
}

/// REQ: PRO-022
#[test]
fn pro_022_gap_22() {
    // Observable: The manual has per-plugin status and health, a crash record and logs, but no stable machine-readable host state for external monitoring (safe mode, crash-mark count, disk step, revocation-feed age, pending update count, last successful backup age). OPS-220 says there is no service-manager readiness signal.
    panic!("PRO-022 not implemented: The system must provide `ilium plugin host-status --json` that prints one record with safe_mode, crash_marks_in_window, disk_step, revocation_age_seconds, plugins_by_health counts, update_pending count and last_backup_age_seconds when a backup directory is configured, with a documented exit code 0 healthy, 1 degraded, 2 critical, and stable field names under the CLI output stability policy.");
}

/// REQ: PRO-023
#[test]
fn pro_023_gap_23() {
    // Observable: Data root loss is specified at start (PRP-016) and for a read-only data directory, but not when the volume is unmounted, disconnected or turns read-only while the server runs and plugins are active.
    panic!("PRO-023 not implemented: The system must, when the data volume becomes unavailable or read-only while running, within 10 seconds show one banner, stop starting new third-party activations, keep running plugins alive with writes returning io unavailable (never counted as plugin failures), log host.data-root-lost once, and resume with host.data-root-restored and a single drained-write report when the same volume identity returns, or require restart if the identity differs.");
}

/// REQ: PRO-024
#[test]
fn pro_024_gap_24() {
    // Observable: HST-095 caps moved-aside .corrupt/.broken files, and LIF-958 keeps uninstalled data forever, but nothing bounds the total disk held by kept data, pre-migration backups/<id>/, superseded versions and archives together, or surfaces it. Only low-disk eviction (HST-661) acts, and only on caches.
    panic!("PRO-024 not implemented: The system must keep at most the 3 newest pre-migration backups per plugin and expire kept data of an uninstalled plugin only by explicit `ilium plugin gc --kept-data --older-than <days>`, report total bytes per class (active, backups, kept-uninstalled, moved-aside, caches) in `ilium plugin du --json`, and warn once when kept classes exceed 25 percent of the data volume.");
}

/// REQ: PRO-025
#[test]
fn pro_025_gap_25() {
    // Observable: Budgets and health are specified per plugin, but after an install or update the operator has no required before/after comparison to see that the change itself regressed startup time, memory or failure counts. Telemetry has startup_phase_us per activation but nothing ties it to the version delta.
    panic!("PRO-025 not implemented: The system must record, per plugin version, activation time, peak memory, failure count and quota-hit count for the first 24 hours of activity, and show the delta against the previous version in `ilium plugin history` and `plugin status --json` (fields prev_version and delta), flagging a regression when activation time or peak memory is more than 50 percent above the previous version.");
}

/// REQ: PRO-026
#[test]
fn pro_026_gap_26() {
    // Observable: SEC-067 says every non-allowlisted system call returns EPERM and never kills the helper. No page says that such denied calls are counted or recorded. A plugin that repeatedly probes for a sandbox escape (forbidden syscalls, mount, ptrace, namespace calls, denied fs paths) leaves no trace in the audit log, the plugin doctor output or the host log.
    panic!("PRO-026 not implemented: The system must count denied seccomp and Landlock events per helper, write one rate-limited `sandbox-probe` audit record per helper run when the count passes a threshold of 20 denials, naming the plugin id, the syscall or path class and the count (never plugin data), and show `Sandbox probing detected` in `plugin doctor`; a hostile fixture issuing 1000 forbidden calls yields exactly one record per run and the plugin keeps running.");
}

/// REQ: PRO-027
#[test]
fn pro_027_gap_27() {
    // Observable: SEC-015 refuses a feed, release blocklist or policy blocklist whose `issued` is earlier than the last good copy. It does not say what happens to a feed whose `issued` is far in the future, whether from a mistaken or compromised signer or a wrong clock. Accepting one raises the stored high-water mark, and every later genuine feed is then refused as older (a freeze attack).
    panic!("PRO-027 not implemented: The system must refuse a revocation feed, release blocklist or policy blocklist whose `issued` is more than 24 hours after the host clock, with audit `signature-failure` naming both times and without raising the stored last-issued mark; a fixture feed issued 30 days in the future is refused, and a following feed with a normal `issued` time is accepted.");
}

/// REQ: PRO-028
#[test]
fn pro_028_gap_28() {
    // Observable: The 7-day staleness clock (EXT-072) and `max_revocation_age` (PRQ-011) do not say which clock they use. Grant expiry is protected against a backward clock (PDY-004), but revocation freshness is not. Setting the wall clock back keeps stale revocation data looking fresh and defeats the T2/T3 activation block.
    panic!("PRO-028 not implemented: The system must measure the revocation staleness age and `max_revocation_age` with the monotonic clock since the last verified refresh, persisted with the boot id. A backward wall-clock change of at least 1 hour must not make the data look fresher, and after a reboot the age must be at least the larger of the persisted elapsed value and the wall-clock difference. A test setting the clock back 30 days on a stale feed must still give `trust.revocation-stale`.");
}

/// REQ: PRO-029
#[test]
fn pro_029_gap_29() {
    // Observable: SEC-224 deletes rotated audit files after `audit.keep_days`. The audit hold (PRZ-020) is only set by an administrator command. When a plugin is blocklisted, its key revoked, or its sandbox self-test fails, nothing keeps the evidence about that plugin from being pruned, which matters on a single-user machine with no administrator.
    panic!("PRO-029 not implemented: The system must, when a plugin is disabled by blocklist, key revocation or `sandbox.unavailable self-test-failed`, exempt every audit record naming that plugin id from keep_days pruning for 365 further days (or until the user clears it), and write one `audit-auto-hold` record naming the plugin and trigger; a fixture with 400-day-old records for a blocked plugin keeps those records after pruning while unrelated records are pruned.");
}

/// REQ: PRO-030
#[test]
fn pro_030_gap_30() {
    // Observable: SEC-200 states that the server never sends a secret value to the client except to the owning settings editor on reveal. It does not say what happens when two clients are attached to one session (PRQ-001 makes preferences per client). The reveal and the secret entry could be mirrored to a second client, an observer or recording client, or the screen-reader mirror.
    panic!("PRO-030 not implemented: The system must send a revealed or typed secret value only to the one client that performed the reveal or entry. Other attached clients must show the row as masked with `Being edited in another client` and no value bytes in their frames, and the value must be masked in screen-reader output, session recordings and `plugin doctor` bundles. A two-client test must find the secret string in zero frames bytes of the second client.");
}
