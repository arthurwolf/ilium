// Round-fifteen gap-closing requirements (PRM IDs), red until implemented.

/// REQ: PRM-001
#[test]
fn prm_001_gap_closing() {
    // Observable: Kill one fixture plugin, quarantine another through a fixture registry feed, restart the server, and assert in `ilium plugin list --json` the states `disabled-by-admin` and `quarantined` with their reasons and that neither plugin has an active side.
    panic!("PRM-001 not implemented: The system must list `disabled-by-admin` and `quarantined` as package states in `ilium plugin list --json`, with `disabled-reason` set to `admin-kill`, `registry.quarantine` or `suspected-escape`. It must store an admin kill in `<data>/plugins/kill.json` and a quarantine in `installed.json`, and both states must survive a server restart.");
}

/// REQ: PRM-002
#[test]
fn prm_002_gap_closing() {
    // Observable: Quarantine one plugin with each reason, run `enable <id>` and `enable <id> --accept-risk` on both, and assert that only the `suspected-escape` plugin becomes enabled and only with `--accept-risk`, and that the other exits 1 with `registry.quarantine`.
    panic!("PRM-002 not implemented: The system must lift a `suspected-escape` quarantine only through `ilium plugin enable <id> --accept-risk` or the kernel dialog of PRN-020, and must refuse `enable` for a `registry.quarantine` with the code `registry.quarantine` and exit code 1 even when `--accept-risk` is given.");
}

/// REQ: PRM-003
#[test]
fn prm_003_gap_closing() {
    // Observable: A documentation test collects every audit event name and every `--event` value quoted in the manual and asserts that each is a row of the SEC-222 catalogue and contains no dot; a host test triggers each catalogued containment event and asserts the `event` and `reason` fields.
    panic!("PRM-003 not implemented: The system must write only hyphenated `event` values in audit records, must carry dotted codes only in the `reason` field, and the event catalogue of SEC-222 must list every audit event the manual requires.");
}

/// REQ: PRM-004
#[test]
fn prm_004_gap_closing() {
    // Observable: Export one set of plugins both ways, install each on an offline fixture, and assert equal verification steps and equal `install` audit records; flip one byte in each bundle and assert `registry.hash-mismatch` with exit code 1 on both paths.
    panic!("PRM-004 not implemented: The system must treat the archive of `bundle create` and the directory of `export-bundle` as one bundle format: `bundle import <file>` followed by `install --from-bundle <dir> <id>`, and a direct `install --from-bundle <dir> <id>` on the exported directory, must verify the same signatures and hashes, fail with the same `registry.*` and `bundle.*` codes and exit codes, and write the same `install` audit record.");
}

/// REQ: PRM-005
#[test]
fn prm_005_gap_closing() {
    // Observable: Build each bundle against a fixture plugin that has a manifest and three grants, list the zip entries and assert they equal the documented list and that no entry holds the manifest bytes or a grant name outside the two named entries; run `diagnostics --out` and assert exit code 2 and no file.
    panic!("PRM-005 not implemented: The system must write, for each of `support-bundle <id>`, `support-bundle --all` and `doctor --bundle`, an archive whose entry names equal the documented list; none may contain a manifest file or a grant list outside `summary.txt` and `inventory.json`; and `diagnostics --out <file>` must fail with `usage.unknown-command` and exit code 2 and write no file.");
}

/// REQ: PRM-006
#[test]
fn prm_006_gap_closing() {
    // Observable: Write 205 fixture crash records, 3 of them dated 31 days back, start the server and assert that 200 remain and none is older than 30 days; a documentation check asserts every page naming a crash record retention states 30 days and 200 records.
    panic!("PRM-006 not implemented: The system must delete every crash record, whether of a plugin, a helper or the host, when it is more than 30 days old, and must keep at most 200 records by deleting the oldest first; no page may state another period or count for crash records.");
}

/// REQ: PRM-007
#[test]
fn prm_007_gap_closing() {
    // Observable: Write 25 MiB of log lines through a fixture plugin and assert exactly five files totalling at most 20 MiB; age one rotated file by 15 days, advance the fake clock 24 hours without a restart and assert it is gone; uninstall with `--purge` and assert no log file of the plugin exists after 1 minute.
    panic!("PRM-007 not implemented: The system must keep, for one plugin in one session, at most the active log file and four rotated files of 4 MiB each, must delete rotated files older than 14 days at server start and once every 24 hours, and must delete the log files of an uninstalled plugin 7 days after a plain uninstall and within 1 minute of `uninstall --purge`.");
}

/// REQ: PRM-008
#[test]
fn prm_008_gap_closing() {
    // Observable: Run both commands with no server, assert the ten keys and their order in the text output and equal values in the JSON output, then remove the notices file and assert that only the line `notices: missing` changes.
    panic!("PRM-008 not implemented: The system must print, for `ilium --version --verbose`, exactly ten lines in fixed order (`ilium <version>`, then `build`, `platform`, `host_api`, `contracts`, `wasmtime`, `channel`, `notices`, `revocation_feed`, `security_contact` as `key: value`), must print the same ten values under the same keys in `ilium version --json`, and both commands must exit with code 0 without a session.");
}

/// REQ: PRM-009
#[test]
fn prm_009_gap_closing() {
    // Observable: A lint test extracts every backticked `ilium plugin <word>` and every `usage.<code>` string from all manual pages, removes the exempt list, and fails naming each word with no inventory row and each code with no row in the error table.
    panic!("PRM-009 not implemented: The system must list, in the Command inventory of the CLI reference, every `ilium plugin <word>` subcommand that any manual page quotes as a command to run, and must list in its error table every `usage.<code>` error code that any page cites; words the inventory names as not existing, a deliberately misspelt word in a DEV-093 test, and non-error `usage.` strings such as `usage.spend` are exempt.");
}

/// REQ: PRM-010
#[test]
fn prm_010_gap_closing() {
    // Observable: Trigger each mechanism with a fixture (three start crashes, three side failures in 10 minutes, five side failures 11 minutes apart, three activation crashes under `dev`) and assert the documented name for each; repeat the two plugin rules with an essential core plugin and assert it is not stopped.
    panic!("PRM-010 not implemented: The system must report which rule stopped a side: a side in `stopped-by-failure` must carry `stop_rule` with the value `restart-rule` or `failure-24h` in `ilium plugin list --json` and in the `plugin-state-changed` log line; the host start guard must log under `crash-guard.*`; `dev` must emit `dev.crash-loop`; and no output may use the name crash-loop guard for a rule other than the host start guard.");
}

/// REQ: PRM-011
#[test]
fn prm_011_gap_closing() {
    // Observable: A documentation test parses the Deadline keys table and the limit tables of the errors-and-limits, architecture, performance and debugging pages and asserts equal defaults and ceilings per key and an effect cell that equals it or links to it; a host test breaches each key once and three times in a row with a fixture plugin and asserts the listed effect.
    panic!("PRM-011 not implemented: The system must apply to every deadline key the default, the two tier ceilings and the breach effect of the Deadline keys table, and every other page that shows a deadline key must show the same default and ceilings and must give that effect or link to the table.");
}

/// REQ: PRM-101
#[test]
fn prm_101_gap_closing() {
    // Observable: Run the release gate on a manual tree where one api-* page holds 'proposed, owner to confirm': the gate exits non-zero and prints that page, line and requirement id; with the marker removed and a matching proposal-decisions.json entry it passes.
    panic!("PRM-101 not implemented: The system must fail the release of a host or SDK that declares an `ilium:plugin` contract version stable while any rule in `reference-wit`, a page named `api-*` or `reference-errors-and-limits` carries an unconfirmed-proposal marker (the word `proposed` followed in the same sentence by `owner to confirm`, in any letter case), must print the page, line and requirement id of each such rule, must require in the release record a `proposal-decisions.json` entry with the decision `confirmed`, `changed` or `deferred`, the owner and the date for every rule that carried a marker since the previous stable release, and must publish those pages with no marker.");
}

/// REQ: PRM-102
#[test]
fn prm_102_gap_closing() {
    // Observable: Read both WASI clocks 10,000 times from a T3 and a T1 fixture: every value is a multiple of 1,000,000 ns, `resolution` is 1,000,000 ns, monotonic values never decrease, and a manifest that asks for a finer clock is refused.
    panic!("PRM-102 not implemented: The system must return `clock.now-unix-millis`, `clock.monotonic-millis` and both `wasi:clocks` clocks with a resolution of 1 ms for every tier (WASI values are multiples of 1,000,000 ns and `resolution` reports 1,000,000 ns), must add a random offset of up to 1 ms per read for T2 and T3 plugins and none for T1, must never return a monotonic value lower than an earlier read of the same instance, and must offer no manifest key, grant or tier that gives a finer clock.");
}

/// REQ: PRM-103
#[test]
fn prm_103_gap_closing() {
    // Observable: Register 64 records from one plugin: the 65th returns `quota-exceeded`; a 256-byte branch returns `invalid-argument`; an 11th call inside one second returns `rate-limited`; a second plugin's `list` is empty and its `release` of the first plugin's record returns `not-found`.
    panic!("PRM-103 not implemented: The system must refuse `workspace.register` with `quota-exceeded` when the calling plugin holds 64 live records or the session holds 256, with `invalid-argument` when `branch` or `base` is longer than 255 bytes or a path is longer than 4,096 bytes, and with `rate-limited` after 10 calls in 1 s; `workspace.list` must return only the records that the calling plugin registered (plus those of a core plugin it replaces), and `workspace.release` of another plugin\'s record must return `not-found`.");
}

/// REQ: PRM-104
#[test]
fn prm_104_gap_closing() {
    // Observable: Load a policy file that sets every documented key: no `policy.invalid`; add one unknown key or one out-of-range value: the host reports `policy.invalid` naming that key.
    panic!("PRM-104 not implemented: The system must accept, without `policy.invalid`, a policy file that sets every key of the Schema block and key table of this page, each inside its stated type and range, must refuse a value outside the range or a key that is not in them with `policy.invalid` naming the key, and a release gate must fail when a page of the manual names a policy key that this page does not define.");
}

/// REQ: PRM-105
#[test]
fn prm_105_gap_closing() {
    // Observable: Run the schema test over the manual pages: it reports 177 full examples and 77 fragments and passes; insert an unknown key into one example and it fails naming that page and key.
    panic!("PRM-105 not implemented: The system must accept, in the published manifest schema and in the settings row checks, every manifest key and settings row key that a page of this manual specifies, and must state for each its type, bounds, default, tier restriction and error code on this page or on Settings schema reference. The schema test must validate every manifest example of the manual against the schema (a fragment that omits required keys is checked for unknown keys and for value types only), after replacing documentation placeholders (`<text>` values and `@@name@@` template tokens) with valid sample values, must assert that the schema refuses the reserved name on a page whose status line says `reserved`, must fail naming the page and the key on any key that the schema rejects, and must report the counts of full examples and fragments.");
}

/// REQ: PRM-106
#[test]
fn prm_106_gap_closing() {
    // Observable: Run the help-versus-inventory test: it passes; add a hidden flag to one command and it fails naming that command and flag.
    panic!("PRM-106 not implemented: The system must list every `ilium plugin` subcommand in the Command inventory of this page with its synopsis, its flags with type, default and meaning, whether it needs a session, its default output mode, its JSONL record types with their fields, and its exit codes. A test must compare the subcommands and flags of `ilium plugin --help` and of every `ilium plugin <command> --help` with this inventory and must fail naming each subcommand or flag that is in only one of them.");
}

/// REQ: PRM-107
#[test]
fn prm_107_gap_closing() {
    // Observable: Run each command with `--json` in the golden-output test: every printed record validates against its catalogue entry, and a record with an extra field makes the test fail naming the type and field.
    panic!("PRM-107 not implemented: The system must print, from every `ilium plugin` command, only JSONL record types that the record catalogue of this page lists, with only the listed fields and enumerated values. The golden-output test of each command must validate each printed record against its catalogue entry and must fail on an unlisted record type, field or enumerated value.");
}

/// REQ: PRM-108
#[test]
fn prm_108_gap_closing() {
    // Observable: Run the code-catalogue gate: it passes with 805 rows; add an uncatalogued dotted code to one page, or change one exit code in the binary, and the gate fails naming that code.
    panic!("PRM-108 not implemented: The system must list in this catalogue, or for the `lint.*` and `harness.*` families in the rule table that this catalogue names, every diagnostic code that a page of the manual names, each with its stage, its exit code and the page that states its cause and fix. A release gate must extract every dotted name in code type from the manual pages and every code string from the host binary, must resolve each name in the registries of the table above, and must fail naming each name that resolves in no registry, each code that the catalogue does not hold or whose stage or exit code differs from the binary, and must fail when the count stated in the catalogue text differs from the number of rows.");
}

/// REQ: PRM-109
#[test]
fn prm_109_gap_closing() {
    // Observable: Run the conformance suite against the bundled core plugins: one case per schema row passes; a replacement plugin whose service reply omits a required field makes its case fail naming the row and the field.
    panic!("PRM-109 not implemented: The system must give in the three tables above, and in the machine-readable schemas that this section names, the type, bounds and allowed values of every field of every core service method, hook point, collect point and topic, with the error codes, the grant and the size and rate limits of each service method. The conformance suite must hold one case for each row. The case must validate a real service reply, hook payload, collect entry and topic payload of the bundled core plugin, and of an installed plugin that lists the core plugin in `replaces`, against the schema, and must fail naming the row and the field on any difference.");
}

/// REQ: PRM-110
#[test]
fn prm_110_gap_closing() {
    // Observable: Load a policy with one `[[rollout]]` entry and stages `[1.00, 10.00, 100.00]`: it loads and a host outside stage 1 reports `held`; remove `rollout_state_dir` or set stages `[10.00, 5.00]`: the host reports `policy.invalid` naming the key and applies no rollout update.
    panic!("PRM-110 not implemented: The system must read the operator rollout keys `rollout_state_dir` and `[[rollout]]` (`id`, `plugin`, `version`, `stages`, `halt_failures`, `halt_hosts`, `halt_window_seconds`) from the admin policy with the types, ranges and defaults of the key table of this page. A policy file with a `[[rollout]]` entry and no `rollout_state_dir`, with a `stages` list that does not increase strictly or does not end at 100, or with a halt value outside its range must be refused with `policy.invalid` naming the key, and a host must then apply no update that a rollout entry of that file names.");
}

/// REQ: PRM-201
#[test]
fn prm_201_gap_closing() {
    // Observable: Install a fixture plugin with one empty `setup_required` secret row, assert that no export of it is called, that the row reads `Needs setup`, that the notice appears and `list --json` reports health `needs-setup`, then fill the row and assert the plugin is active and the row reads `On` within 1 second.
    panic!("PRM-201 not implemented: The system must, when an enabled plugin declares a settings row with `setup_required` and the value of that row is empty, keep the plugin not activated, show it in Settings > Plugins as `Needs setup` with no failure count and no crash mark, show one notice `Installed. <plugin> needs N settings before it works` with an `Open settings` action that puts the focus on the first empty marked row, report health `needs-setup` in `ilium plugin list --json`, and activate the plugin and show `On` within 1 second after the last empty marked row is filled.");
}

/// REQ: PRM-202
#[test]
fn prm_202_gap_closing() {
    // Observable: Open the detail view of a fixture package whose 70 KiB README holds a table, a link, a remote image and an escape sequence, with the network recorded, and assert the rendered cells show the link text with its URL, `[image: <alt>]`, no escape byte, the truncation line, one `alt (W x H px)` line per screenshot, and that no request was made.
    panic!("PRM-202 not implemented: The system must render a plugin README and CHANGELOG in the detail view from a fixed Markdown subset (headings, paragraphs, lists, code blocks, tables, links shown as text with the URL), remove control and escape characters, cap the shown text of each document at 64 KiB with the line `Truncated, open the repository`, show each screenshot as its alt text plus its pixel size, and never fetch a remote image.");
}

/// REQ: PRM-203
#[test]
fn prm_203_gap_closing() {
    // Observable: Run `ilium plugin preview --json` with the network blocked on a complete fixture package and assert exit 0, one `preview` record with every listed field and an unchanged installed set; then remove the README and assert exit 3 with `lint.preview-field-missing`.
    panic!("PRM-203 not implemented: The system must provide `ilium plugin preview <package-or-dir> [--locale <tag>] [--json]`, which prints, offline and without installing, what the store detail view and the install review dialog would show (name and description in the chosen locale, grants with risk levels, README text, screenshot alt text, support contact, licence expression), and exits 3 with `lint.preview-field-missing` or `lint.preview-truncated` when a shown field is missing or would be truncated.");
}

/// REQ: PRM-204
#[test]
fn prm_204_gap_closing() {
    // Observable: Start an install from the client with a throttled download, press Esc during `Downloading`, and assert that the hashes of installed.json, the store and the staging folder equal those taken before the start, that one `install` audit record has result `cancelled` and reason `install.cancelled`, and that the previous view is shown within 1 second.
    panic!("PRM-204 not implemented: The system must show, during an install or update started from the client, a progress line with the current step and a Cancel action reachable with `Esc` in every step before the commit step `Recording`. A cancel must leave `installed.json`, the plugin folders and the staging area byte-identical to their state before the start, must write one `install` or `update` audit record with result `cancelled`, reason `install.cancelled` and the step, and must return the screen to the previous view within 1 second.");
}

/// REQ: PRM-205
#[test]
fn prm_205_gap_closing() {
    // Observable: With `auto_update = install` and the key set to 48, offer a version published 1 hour ago and run two checks: assert no download, `held_until` in `list --json`, the `Held back until` line and exactly one `update-held` audit record; then mark that version as `fixed` in an advisory for the installed version and assert the next check installs it.
    panic!("PRM-205 not implemented: The system must support the policy key `packages.auto_update_min_age_hours` (0 to 720, default 0). While it is above 0, auto-update must not download or install a version whose registry publish time is younger than the value, must show `Held back until YYYY-MM-DD HH:mm:ss` in the update notice and the time in the field `held_until` of `ilium plugin list --json`, must still install at the next check a version named as `fixed` by an advisory for the installed version, and must write one `update-held` audit record per held version per machine.");
}

/// REQ: PRM-206
#[test]
fn prm_206_gap_closing() {
    // Observable: Install a fixture with `license = 'MIT'` and assert the dialog line `Licence: MIT` and `'license':'MIT'` in the JSON record; then, with `auto_update = install`, offer an update with a different expression and no new grants and assert it is not installed automatically and that the review dialog shows `Licence changed: MIT to <new>`.
    panic!("PRM-206 not implemented: The system must show the SPDX licence expression of the package in the install and update review dialogs, must show `No licence declared` when the manifest has none, must show `Licence changed: <old> to <new>` and require the review dialog (no automatic install) when the expression of an update differs from the installed version, and must carry the same value in the `license` field of `ilium plugin install --json`.");
}

/// REQ: PRM-207
#[test]
fn prm_207_gap_closing() {
    // Observable: With two fixture plugins enabled, add both ids to `plugins.deny_ids` with a `notice.contact`, restart the server, and assert that no export of either plugin was called, both rows read `Blocked by policy` with the detail line, exactly one notice names both plugins and the contact text, and the audit log holds one `plugin-state-changed` record with reason `policy` per plugin.
    panic!("PRM-207 not implemented: The system must, on the first start after a policy change that blocks previously enabled plugins, put each such plugin in `disabled-by-policy` before any of its code runs, show its row as `Blocked by policy` with the detail line `Disabled by your administrator` in that same start, show one notice `Your administrator turned off N plugins: <names, first 5>` with the `notice.contact` text ([USR-343]) and an `Open Plugins` action, and write one `plugin-state-changed` audit record with reason `policy` per plugin.");
}

/// REQ: PRM-208
#[test]
fn prm_208_gap_closing() {
    // Observable: Make a fixture plugin fail three times with cached registry data listing a newer version and the network recorded: assert the line and the Update action in the notice and the detail view and zero requests; repeat with the newer version pinned away by policy, and with no cached data, and assert the line is absent.
    panic!("PRM-208 not implemented: The system must, when a plugin\'s health is `failed` or `restarting` and the cached, unexpired registry metadata lists a newer non-yanked version that policy allows, add the line `Version X.Y.Z is available. Update?` with an Update action to the failure notice and to the plugin detail view. Without such a cached version, or when policy blocks the update, the line must be absent, and no network request may be made only to produce it.");
}

/// REQ: PRM-301
#[test]
fn prm_301_gap_closing() {
    // Observable: Attach a client that never reads, write 1 GiB of output into one pane, and assert the server memory attributed to that client never exceeds 32 MiB and a second client shows the final screen.
    panic!("PRM-301 not implemented: The system must bound each attached client\'s outbound queue to one pending pane frame per pane (newer output replaces it with one full repaint of that pane) plus the plugin queue of HST-121, must keep server memory for one client\'s queues at or below 32 MiB, and must disconnect a client that would exceed it with ipc.client-too-slow.");
}

/// REQ: PRM-302
#[test]
fn prm_302_gap_closing() {
    // Observable: SIGSTOP an attached client while a pane prints, assert the disconnect 30 to 35 seconds later, continue the client and assert it re-attaches and shows the current screen.
    panic!("PRM-302 not implemented: The system must disconnect a client with ipc.client-too-slow when data has been queued for 30 seconds and its socket accepted no byte, must log the disconnect with cause \'stalled\' beside the plugin name of HST-523, and must send a full snapshot when that client attaches again.");
}

/// REQ: PRM-303
#[test]
fn prm_303_gap_closing() {
    // Observable: Attach two clients, SIGSTOP one, and assert the 99th percentile pane-output-to-frame time of the other grows by less than 50 ms and no plugin deadline breach is recorded.
    panic!("PRM-303 not implemented: A stalled or slow client must not delay a server-side plugin deadline, a PTY read, or frame delivery to any other attached client by more than 50 ms.");
}

/// REQ: PRM-304
#[test]
fn prm_304_gap_closing() {
    // Observable: On Windows hold an exclusive handle on the target for 1 second, call fs.rename and run an install, and assert both succeed with no error reported.
    panic!("PRM-304 not implemented: On Windows the system must retry a rename, replace or delete that fails with a sharing violation or access denied for up to 2 seconds (wait from 25 ms doubling to 400 ms) before returning an error, for fs.rename, fs.remove, replacing writes, write-atomic and the host\'s own install, update, uninstall, snapshot, installed.json, grants.json, consents.json, trust store and config.toml writes.");
}

/// REQ: PRM-305
#[test]
fn prm_305_gap_closing() {
    // Observable: On Windows hold an exclusive handle on the target for 5 seconds, call fs.rename, write grants.json and take a snapshot, and assert the io error with the file-in-use message and path, unchanged target bytes and no temporary files.
    panic!("PRM-305 not implemented: When the Windows retry time ends without success, the system must keep the previous target file intact, leave no partial file under the target name and no temporary file beside it, and report io with a message that starts with \'file-in-use:\' and names the path.");
}

/// REQ: PRM-306
#[test]
fn prm_306_gap_closing() {
    // Observable: Start server and client with XDG_RUNTIME_DIR unset and assert the client attaches, the fallback folder is 0700 and one runtime.dir-fallback line is logged; replace the fallback folder with a symlink and assert runtime.dir-unsafe.
    panic!("PRM-306 not implemented: When XDG_RUNTIME_DIR is unset, not owned by the user or not mode 0700, the system must use a 0700 per-user folder under the OS temp dir, log runtime.dir-fallback once with the reason and start; it must verify every runtime directory is a real directory owned by the user with mode 0700 and refuse to start with runtime.dir-unsafe when the fallback or the ILIUM_SOCKET_DIR folder fails that check.");
}

/// REQ: PRM-307
#[test]
fn prm_307_gap_closing() {
    // Observable: Start a session in a project with a 200-byte path and assert the server starts, a client attaches, the socket name is the digest and doctor prints the path, length and directory source; set ILIUM_SOCKET_DIR to a 100-byte path and assert runtime.socket-path-too-long.
    panic!("PRM-307 not implemented: When the session socket path would exceed the platform limit (104 bytes macOS, 108 Linux), the system must use a digest-only file name, never truncate a path, refuse to start with runtime.socket-path-too-long when the digest name does not fit either, and ilium plugin doctor must print the runtime directory, its source and the socket path with its length.");
}

/// REQ: PRM-308
#[test]
fn prm_308_gap_closing() {
    // Observable: Attach 16 clients and assert all draw, attach a 17th and assert the refusal, then detach one and assert the next attach succeeds.
    panic!("PRM-308 not implemented: The system must accept 16 simultaneously attached clients on one session and refuse the 17th attach with attach.client-limit, a message naming the limit and a non-zero exit code, without disturbing the attached clients.");
}

/// REQ: PRM-309
#[test]
fn prm_309_gap_closing() {
    // Observable: With default aggregate ceilings start 32 sessions in 32 project folders, attach one client to each, type into one pane of each and assert every session echoes the input and writes its own snapshot.
    panic!("PRM-309 not implemented: The system must support 32 concurrent sessions of one user on the reference machine, each with its own server, socket and snapshot.");
}

/// REQ: PRM-310
#[test]
fn prm_310_gap_closing() {
    // Observable: Measure frame time and server memory with 1 client and with 16 clients on a 100-pane session and compare both with the limits.
    panic!("PRM-310 not implemented: Each additional attached client must add at most 48 MiB of server memory, and with 16 clients on a 100-pane session the 99th percentile pane-output-to-frame time of the first client must stay under 50 ms and within 1.5 times its single-client value.");
}

/// REQ: PRM-311
#[test]
fn prm_311_gap_closing() {
    // Observable: Run the real client under a PTY, record the screen, send SIGTSTP and assert the reset sequences and cooked-mode attributes, resize the PTY three times, send SIGCONT and assert a full repaint at the last size within 200 ms matching the recorded content.
    panic!("PRM-311 not implemented: On SIGTSTP the client must restore the terminal (mouse reporting off, alternate screen left, raw mode left, cursor shown) before it stops, and on SIGCONT it must re-enter those modes, read the current size and repaint the whole screen within 200 ms, applying a burst of SIGWINCH as one resize.");
}

/// REQ: PRM-312
#[test]
fn prm_312_gap_closing() {
    // Observable: Stop the client for 40 seconds while a pane prints, continue it, and assert it shows the current screen, no deadline breach was logged and every plugin's failure count is unchanged.
    panic!("PRM-312 not implemented: The client must not report a client-side plugin deadline breach or add to a plugin\'s failure count for the interval in which it was stopped, and a client the server disconnected while stopped must attach again by itself on SIGCONT.");
}

/// REQ: PRM-313
#[test]
fn prm_313_gap_closing() {
    // Observable: In a container run the server as PID 1, orphan a pane child, assert no zombie after 1 second, send SIGTERM and assert a clean exit within 10 seconds with acknowledged storage writes on disk.
    panic!("PRM-313 not implemented: When the server runs as PID 1 it must reap orphaned child processes so none stays a zombie for more than 1 second, and must treat SIGTERM as the graceful stop of PRS-002 and exit within 10 seconds.");
}

/// REQ: PRM-314
#[test]
fn prm_314_gap_closing() {
    // Observable: Start the server in a container with --read-only, no TTY, an arbitrary uid and XDG_* pointing at a volume and assert it starts and a client attaches; unset the variables and assert data.root-unresolved.
    panic!("PRM-314 not implemented: When HOME is unset, unwritable or the uid has no passwd entry, the system must start with the roots named by the XDG_* variables, ILIUM_CONFIG_DIR and plugins.data_dir, use the read-only mode of HST-664 for a root that cannot be written, and refuse to start with data.root-unresolved naming the variables to set only when a root can be resolved from neither a home directory nor a variable.");
}

/// REQ: PRM-315
#[test]
fn prm_315_gap_closing() {
    // Observable: Run ilium plugin doctor in the read-only arbitrary-uid container and compare the five facts with the container's configuration.
    panic!("PRM-315 not implemented: ilium plugin doctor must print whether a container was detected, whether the server is PID 1, the uid and whether it has a passwd entry, whether standard input is a TTY, and each resolved root with its writable state.");
}

/// REQ: PRM-316
#[test]
fn prm_316_gap_closing() {
    // Observable: Attach a truecolor client, a 16-colour client and a NO_COLOR client to one session, show a view with an rgb span, and assert truecolor, nearest 16-colour and no colour sequences respectively with the same text in all three.
    panic!("PRM-316 not implemented: The system must detect colour depth, NO_COLOR and mouse support separately for each attached client from that client\'s own environment and terminal, and must reduce plugin colours per client so clients with different terminals on one session each get a correct paint.");
}

/// REQ: PRM-317
#[test]
fn prm_317_gap_closing() {
    // Observable: Start the client under a PTY that answers the query with white, with black and not at all and assert light, dark and dark; repeat with a configured theme and assert the configured theme is used each time.
    panic!("PRM-317 not implemented: When the config file sets no theme or sets auto, the client must choose its first theme from its own terminal\'s background: send the background colour query, wait at most 100 ms, choose light when relative luminance is above 0.5 and dark otherwise or when no answer arrives, and never override a theme set in the config file or picked by the user.");
}

/// REQ: PRM-401, F-117
#[test]
fn prm_401_gap_closing() {
    // Observable: Queue one fixture agent of each kind and three of one kind that differ in priority, age and pane id, and assert the order of the panel rows and of the overview cards.
    panic!("PRM-401 not implemented: The system must rank the attention queue by kind in the order Question, Stuck, Waiting on, Changed, and inside a kind by higher priority first, then the oldest item, then the lowest pane id.");
}

/// REQ: PRM-402
#[test]
fn prm_402_gap_closing() {
    // Observable: Drive four fixture agents into the four states with the mode `instant_only` and assert the four kinds; switch to `shared`, return a model list that omits one agent, and assert its item is still queued.
    panic!("PRM-402 not implemented: The system must create an instant item from detected agent state alone: waiting for approval as Question, a blocked or usage-limited goal as Stuck, waiting for subagents as Waiting on and an unread completion as Changed. A model reply must only add a reason, the exact question, a waiting-on target and a priority, and must never remove an instant item.");
}

/// REQ: PRM-403
#[test]
fn prm_403_gap_closing() {
    // Observable: Count the provider requests of a fixture provider in each of the four modes with five changed agents and a concurrency of 2, and assert 0, 0, 0 extra and 5 with at most 2 in flight; assert the mode `off` paints none of the five surfaces.
    panic!("PRM-403 not implemented: The system must make no model call in the modes `off` and `instant_only`, no call beyond the existing Sort call in the mode `shared`, and one call per changed agent in the mode `turbo` with at most `turbo_concurrency` calls in flight. In the mode `off` it must draw no button, panel, overview, arrival block or briefing box.");
}

/// REQ: PRM-404
#[test]
fn prm_404_gap_closing() {
    // Observable: Return a Sort reply with no list, with invalid JSON in the list and with a list cut after one item, and assert the tree plan is applied, one provider request was made each time and the instant items stay.
    panic!("PRM-404 not implemented: In the mode `shared` the system must ignore a missing, malformed or partial `attention` list in the Sort reply, and must never fail or retry the sort because of it.");
}

/// REQ: PRM-405
#[test]
fn prm_405_gap_closing() {
    // Observable: Answer 429 from a fixture provider with a limit of 16 and assert at most 8 calls in flight afterwards and a later retry; refresh twice with an unchanged screen and assert no second call for that agent.
    panic!("PRM-405 not implemented: In the mode `turbo` the system must halve the effective concurrency limit on HTTP 429 or 503, grow it back after successes and retry the call with backoff, and in every mode it must skip an agent whose screen evidence has not changed since its last analysis.");
}

/// REQ: PRM-406
#[test]
fn prm_406_gap_closing() {
    // Observable: Press the three chords in a client with two queued agents and assert the jump, the panel toggle and the open overview; remap `attention_next` to another chord and assert the new chord jumps and the old one does not.
    panic!("PRM-406 not implemented: The system must bind `Ctrl+B i` to the jump to the agent that needs the user most, `Ctrl+B I` to the toggle of the right panel and `Ctrl+B O` to the overview, and must let the user remap them under the names `attention_next`, `attention_panel` and `attention_overview`.");
}

/// REQ: PRM-407
#[test]
fn prm_407_gap_closing() {
    // Observable: Press twice while a fixture refresh is held open and assert two jumps, one refresh and one Sort; release the refresh with a new first item and assert the focused pane does not change.
    panic!("PRM-407 not implemented: A press of the Next: needs you button or its chord must jump at once from the queue held in memory, then start at most one background refresh, with a press during a refresh coalesced into it, and must also run the normal Sort when `press_sorts` is on. New results must update the list without moving the user.");
}

/// REQ: PRM-408
#[test]
fn prm_408_gap_closing() {
    // Observable: Queue three agents, press three times and assert three different panes, press a fourth time and assert the first pane; change one visited agent from Changed to Question and assert it is next; restart the client and assert the visit order is kept.
    panic!("PRM-408 not implemented: The system must remember visited agents, move a visited agent to the back of the panel and the overview, and return it early only when its kind becomes more urgent or after 5 minutes when its screen changed. When every agent has been visited, a press must go to the agent seen longest ago. The memory must be saved with the queue.");
}

/// REQ: PRM-409
#[test]
fn prm_409_gap_closing() {
    // Observable: Render the overview at 99, 100 and 160 columns and at 59 by 15 cells and compare with golden files; render an empty queue and assert All clear; press Enter on the second card and assert the focus, the looked-at mark and the closed dialog.
    panic!("PRM-409 not implemented: The overview must show one card per queued agent under one header per kind with a count, in one column below 100 terminal columns, two from 100 and three from 160, as a compact two-line list on a terminal smaller than 60 by 16 cells, and with the text All clear when the queue is empty. `Enter` or a click on a card must focus that agent, mark it as looked at and close the dialog, and `Esc` or `q` must close it without a jump.");
}

/// REQ: PRM-410
#[test]
fn prm_410_gap_closing() {
    // Observable: Jump to a fixture agent with a model reply of 400 characters per field and assert the rows, the cut and the `AI` label; repeat in the mode `instant_only` and assert the label `automatic`; shrink the pane to 23 columns and assert no box; turn `show_briefing` off and assert no box.
    panic!("PRM-410 not implemented: On arrival at an agent through the button, a chord, a panel row or the overview, the system must show the briefing box under the last-prompt banner while `show_briefing` is on and the mode is not `off`. The box must leave out a row with nothing to say, cut each model text at 300 characters, keep at least three terminal rows, show one compact line in a small pane and nothing in a pane narrower than 24 columns, and end when the item leaves the queue or the pane closes.");
}

/// REQ: PRM-411
#[test]
fn prm_411_gap_closing() {
    // Observable: Snooze a Changed agent, update its screen and assert it stays hidden and the count badge drops by one; change it to Question and assert it returns; press S and assert a card marked snoozed; restart and assert the snooze is kept.
    panic!("PRM-411 not implemented: A snoozed agent must be left out of the overview, the panel and the Next: needs you button, must stay snoozed when its text or screen evidence changes inside the same kind, and must return when its kind changes or it stops needing attention. `S` in the overview must show and hide snoozed cards, and snoozes must be saved with the queue.");
}

/// REQ: PRM-412
#[test]
fn prm_412_gap_closing() {
    // Observable: Write a queue with one live agent, one missing agent and one agent with other screen evidence, start the client and assert only the live item remains; set the two keys to 0 and to 100000 in the config file and assert the clamped values 1, 64, 60 and 86400.
    panic!("PRM-412 not implemented: The system must store the queue as one global list tagged by project, write it when it changes, and on start drop every item whose agent no longer exists or whose screen evidence differs. It must clamp `turbo_concurrency` to 1 to 64 and `expiry_seconds` to 60 to 86400, and drop an item that was not refreshed within `expiry_seconds`.");
}

/// REQ: PRM-413
#[test]
fn prm_413_gap_closing() {
    // Observable: Disable the plugin with two queued agents and assert no button, panel or box, no reaction to the three chords, a byte-identical queue file and a working attention mark; enable it and assert the two items return.
    panic!("PRM-413 not implemented: While `ilium.core.attention` is disabled or down, the system must draw none of its surfaces, the three chords must do nothing, the stored queue must be kept unmodified, and the attention marks of the notifications plugin must keep working.");
}

/// REQ: PRM-451, F-118
#[test]
fn prm_451_gap_closing() {
    // Observable: Reserve one synthetic resource from three fixture panes and assert one owned and two queued results in request order; release the owner and assert the first queued reservation becomes the owner and the second stays queued.
    panic!("PRM-451 not implemented: The system must give each resource exactly one owner and a first-come, first-served queue. `ilium resource reserve` must return a reservation that is either owned or queued, and a queued reservation must never grant access to the hardware.");
}

/// REQ: PRM-452
#[test]
fn prm_452_gap_closing() {
    // Observable: Run a successful reserve, a reserve with a bad duration, a wait on a cancelled reservation, a status against a fixture store that fails and a status of an unknown id, and assert the exit statuses 0, 2, 3, 4 and 5 and the record fields; run `ilium ressource list` and assert the same output as `ilium resource list`.
    panic!("PRM-452 not implemented: Every `ilium resource` command must print one JSON object per line, with `type` set to `result` on success and to `error` with the fields `operation`, `request_id`, `code` and `message` on failure, and must exit 0 on success, 2 for a usage error or a refused request, 3 for a cancelled or revoked reservation, 4 for an unknown outcome and 5 when the reservation or receipt is not found. `ilium ressource` must behave as `ilium resource`.");
}

/// REQ: PRM-453
#[test]
fn prm_453_gap_closing() {
    // Observable: Reserve with `10`, `0s`, `8d` and `10m` and assert three refusals with exit status 2 and one success; update the estimate to `20m` after 5 minutes of fixture time and assert an expected end 20 minutes after acquisition.
    panic!("PRM-453 not implemented: The system must accept a duration only with units, such as `30s`, `10m` or `1h30m`, in the range of one second to seven days, and must treat an estimate update as the total occupancy from acquisition, not as additional time.");
}

/// REQ: PRM-454
#[test]
fn prm_454_gap_closing() {
    // Observable: Start a wait on a queued reservation, interrupt it and assert the reservation is still queued at the same position; start it again, release the owner and assert the wait ends with an owned result and exit status 0.
    panic!("PRM-454 not implemented: `ilium resource wait` must block with no default timeout until the reservation owns the resource or reaches a terminal result, and must show queue progress. Interrupting or disconnecting a wait must detach only its observer and keep the reservation at its queue position.");
}

/// REQ: PRM-455
#[test]
fn prm_455_gap_closing() {
    // Observable: Reserve a first synthetic resource from a fixture pane, reserve a second one from the same pane and assert a refusal with exit status 2 and an unchanged queue of the second resource; release the first and assert the second reserve succeeds.
    panic!("PRM-455 not implemented: One agent invocation must hold at most one active reservation across all resources, and a second reserve while one is owned or queued must be refused with exit status 2.");
}

/// REQ: PRM-456
#[test]
fn prm_456_gap_closing() {
    // Observable: Send the same reserve twice with one `--request-id` and assert one reservation and equal results; replay an expired identifier and assert a refusal with exit status 5; make the fixture catalog answer busy once and assert one success with one request identifier; make it answer a revision conflict and assert no retry.
    panic!("PRM-456 not implemented: A retried mutation that carries the same request identifier must return the outcome of the first attempt and must not create a second reservation. A request identifier outside the retry window must be refused, not treated as new work. The CLI must retry a temporary catalog-busy response by itself with the same request and correlation identifiers inside the command deadline, and must not retry an uncertain outcome or a revision conflict.");
}

/// REQ: PRM-457
#[test]
fn prm_457_gap_closing() {
    // Observable: Reserve from a fixture pane without `--agent` and assert the owner is that pane; reserve with the reference of a closed pane and with a display title and assert two refusals with exit status 2 and no reservation.
    panic!("PRM-457 not implemented: Inside an Ilium pane the system must infer the agent identity, and an explicit agent reference must resolve to a live pane and invocation. A display title alone must never authorise a reservation for another agent.");
}

/// REQ: PRM-458
#[test]
fn prm_458_gap_closing() {
    // Observable: Let the fixture clock pass the estimate of an owned reservation and assert the owner is unchanged, the waiting entries show an unknown start and no lifecycle event of release, cancel or grant was written.
    panic!("PRM-458 not implemented: An estimate must be advisory: an overdue owner must keep the resource, the start estimates of the waiting reservations must become unknown, and no reservation must be released, cancelled or granted because an estimate passed.");
}

/// REQ: PRM-459
#[test]
fn prm_459_gap_closing() {
    // Observable: Exit the fixture owner process and assert `needs_attention` and a queued successor; run `recover` with a reason from a linked pane and assert the grant to the successor and the reason in the history; run `recover` against a live owner and assert the owner is revoked, the queue is paused and no successor is granted; assert no signal was sent to any fixture process.
    panic!("PRM-459 not implemented: When the owner is provably gone, after a reboot of the machine or the exit of the owning agent process, the reservation must show `needs_attention` and no successor must be granted. `ilium resource recover <slug> --reason <text>` from a pane of a linked project must clear it in one step with no typed confirmation and keep the reason in the resource history. Recovery must check the current catalog revision and must never kill a process or reset a device. For a live owner it must only revoke that owner and pause the resource, and must grant no successor until a separate interactive confirmation reopens the queue.");
}

/// REQ: PRM-460
#[test]
fn prm_460_gap_closing() {
    // Observable: Gate a synthetic resource with one owner and one waiter to `unavailable`, release the owner and assert the waiter stays queued; gate it to `open` and assert the waiter becomes the owner; repeat with `paused`.
    panic!("PRM-460 not implemented: Marking a resource `unavailable` or `paused` with `ilium resource gate` must keep its owner and its queue, and a release while the resource is unavailable or paused must grant no successor until the gate is `open` again.");
}

/// REQ: PRM-461
#[test]
fn prm_461_gap_closing() {
    // Observable: Create a resource in one fixture project, link it in a second and reserve from both, and assert one queue; create a second resource with the same device identity and assert a refusal; unlink a project that holds a reservation and assert a refusal; close the first project and assert the second still lists the resource.
    panic!("PRM-461 not implemented: Creating a resource must also link it to the creating project, and linking an existing resource from another project must share the same queue. A device identity must be unique across definitions, on create and on edit. A link with an active reservation must not be removed, closing the creator project must not delete the definition, and an interrupted cleanup after the last unlink must be retried while the project is open and after a server restart.");
}

/// REQ: PRM-462
#[test]
fn prm_462_gap_closing() {
    // Observable: Link a resource in a fixture project with an authored instruction file and another feature block and assert only the resources block changed; make the file read-only, edit the rules and assert the debt mark; restore the file, run `repair` and assert the block and an unchanged reservation list; run `notify-urgent` with one idle and one busy fixture agent and assert the recipient records.
    panic!("PRM-462 not implemented: The system must maintain a resources instruction block in the supported agent instruction files and must preserve authored text and the blocks of other features. A failed instruction or settings write must be shown at once as synchronisation debt, and `ilium resource repair <slug>` must retry it without recreating the resource or losing a reservation. A routine update must not start an agent turn, and `ilium resource notify-urgent` must send the notice to the idle agents of the project, queue it for the busy ones, skip the calling pane and report every recipient.");
}

/// REQ: PRM-463
#[test]
fn prm_463_gap_closing() {
    // Observable: Render the tree of a fixture project with one resource, one owner and two waiters and compare with a golden file; record the pane ids, parents, split membership and sizes, turn the key off and on, and assert they are equal each time and that typed input reaches the same pane.
    panic!("PRM-463 not implemented: The system must show each linked resource at the top of its project in the tree, after the chatroom, with an indicator for free, taken, attention and queue state. With `[ui] resource_queue_grouping` on it must show owners first and then waiting agents with their global queue positions below the resource, and this grouping must change the display only: pane identities, real parents, split membership, terminal sizes and input routing must stay intact.");
}

/// REQ: PRM-464
#[test]
fn prm_464_gap_closing() {
    // Observable: Move a fixture project that owns a reservation, run `reserve` and assert a refusal; run `rebind` with a stale revision and assert a refusal, then with the current revision and assert success, the same reservation identity and the same queue order.
    panic!("PRM-464 not implemented: A project must carry a stable resource identity in `.ilium/resource-project-id`, and a moved project must not change its claims until `ilium resource rebind <project-id> <path> --expected-revision <revision>` succeeds. Rebinding must update the linked paths and repair the instructions and must keep the reservation identities and the ticket order.");
}

/// REQ: PRM-465
#[test]
fn prm_465_gap_closing() {
    // Observable: Disable the plugin with one owner and two waiters, run `ilium resource release` and assert a failure and an unchanged store; enable the plugin and assert the same owner and the same queue order.
    panic!("PRM-465 not implemented: While `ilium.core.resources` is disabled or down, the system must keep every definition, link and reservation in storage, must not change an owner and must not grant a successor, and `ilium resource` must fail without changing the catalog. On return the plugin must restore the queues with the same ticket order.");
}

/// REQ: PRM-501, F-119
#[test]
fn prm_501_gap_closing() {
    // Observable: Load three fixture rules where the second and third both match a pane and assert the icon of the second; evaluate `A OR B AND C` on fixture facts and assert the grouping; negate a condition on an unobserved fact and assert the rule does not match and the Otherwise icon is shown.
    panic!("PRM-501 not implemented: In the display mode `custom` the system must evaluate the custom status rules from top to bottom and let the first enabled rule whose expression matches choose the one status icon of a pane, with AND binding tighter than OR, NOT applied to a single condition, and a condition on a fact that cannot be observed counted as unknown, also when negated. When no rule matches, the Otherwise row must decide, and an empty Otherwise row must leave the status slot blank.");
}

/// REQ: PRM-502
#[test]
fn prm_502_gap_closing() {
    // Observable: Add a rule in the editor of one fixture project and assert it applies in a second project and after a restart; try to add a 65th rule, a 9th group, a 9th condition and a name of 49 characters and assert four refusals; render a tree with an agent, an editor and a board and assert one slot of three cells on each row.
    panic!("PRM-502 not implemented: The custom status rules must be global to every project and session, must be saved as the user edits them in the rules editor, and must be limited to 64 rules, 8 AND groups per rule, 8 conditions per group, 64 conditions per rule and rule names of 48 characters. In the mode `custom` every pane row, including editors and boards, must show exactly one status slot of three cells.");
}

/// REQ: PRM-503, F-120
#[test]
fn prm_503_gap_closing() {
    // Observable: Close fixture panes with each of the three values on a clean merged worktree and assert the worktree and the branch that remain; run `remove` on a worktree with uncommitted work and on one that Ilium did not create and assert both stay on disk.
    panic!("PRM-503 not implemented: `ilium close-pane` must close the pane it runs in, or the pane named by `--pane`, in the session of the calling pane, and must treat the worktree of that pane as `--worktree` says: `keep` (the default) leaves it on disk, `remove` deletes it only when it is clean, merged and unused, and `remove-and-branch` also deletes its creation branch when that is safe. Only a worktree that Ilium created may be removed, and the removal must be done by the worktrees plugin.");
}

/// REQ: PRM-504
#[test]
fn prm_504_gap_closing() {
    // Observable: Run `close-pane --worktree remove` on a dirty fixture worktree and assert exit status 1, an unchanged checkout and a reason on the output; run it with an unknown pane id and assert exit status 2; hold the fixture server answer past a timeout of 1 second and assert exit status 1.
    panic!("PRM-504 not implemented: `ilium close-pane` must never discard uncommitted work. It must exit 0 when the pane was closed and the requested removal was done, 1 when the removal was blocked or its outcome is uncertain, and 2 for a usage error, and it must wait at most the number of seconds of `--timeout-seconds` (default 120) for the answer of the server.");
}

/// REQ: PRM-505, F-121
#[test]
fn prm_505_gap_closing() {
    // Observable: Run `ilium ctl tree` in each of the four formats against a fixture session and parse each output; run a rename, a rename of an unknown node, a command with an unknown flag and a command with no session, and assert the exit statuses 0, 3, 2 and 4 and the record types.
    panic!("PRM-505 not implemented: `ilium ctl` must give agents and scripts remote control of a running session: read commands must print one document in the format of `--format` (`json`, `jsonl`, `yaml` or `markdown`), and mutations must print JSONL records whose `type` is `result` or `error`. It must exit 0 on success, 2 for bad arguments or a bad selector, 3 when the server rejected or did not confirm the operation and 4 when no session can be reached.");
}

/// REQ: PRM-506
#[test]
fn prm_506_gap_closing() {
    // Observable: For every body printed by `ilium ctl ops`, send it through `ilium ctl op` and through the HTTP API of two equal fixture sessions and assert equal results and equal trees.
    panic!("PRM-506 not implemented: Every operation of `ilium ctl op` and the same operation sent to the local HTTP API must be translated to the same request and must give the same result, and `ilium ctl ops` must print one example body for every operation that both accept.");
}

/// REQ: PRM-507, F-122
#[test]
fn prm_507_gap_closing() {
    // Observable: Type a query into the picker of a fixture client with the network blocked and assert ranked results and no network request; hold the fixture model load open and assert the picker still handles key presses within one frame.
    panic!("PRM-507 not implemented: The icon picker must rank the whole icon catalogue for every non-empty query by meaning, with an embedding model that runs on the CPU of the client, and must use no remote inference service, no GPU and no database for it. Loading the model and ranking must run off the UI thread, so the picker stays responsive while a search runs.");
}

/// REQ: PRM-508
#[test]
fn prm_508_gap_closing() {
    // Observable: Open the picker from an icon row, choose an icon and assert the row and the config file; open it again, press Esc and assert no change; remove the fixture model file and assert the unranked list and the notice.
    panic!("PRM-508 not implemented: The icon picker must apply the chosen icon to the setting row that opened it and save it at once, must keep the previous icon when the user cancels, and must show the catalogue unranked with a notice when the search model cannot be loaded.");
}

/// REQ: PRM-509, F-123
#[test]
fn prm_509_gap_closing() {
    // Observable: Open the picker from the shared observer row and from a map scene row, set coordinates in one and search a fixture address in the other, confirm both and assert the two stored values; assert latitude is refused outside -90 to 90 and longitude outside -180 to 180.
    panic!("PRM-509 not implemented: The location picker must offer an address search and bounded latitude and longitude controls that update the same candidate as the world map, must confirm the candidate with `Enter` or the Use location button, and must store it either as the shared observer or as the independent location of one map scene, as the row that opened it says.");
}

/// REQ: PRM-510
#[test]
fn prm_510_gap_closing() {
    // Observable: Search with the fixture geocoder answering an error and an empty list and assert an unchanged candidate and a notice; inspect the request and assert it holds only the address; block the network and assert the arrow keys and a mouse click still move the crosshair.
    panic!("PRM-510 not implemented: An address lookup of the location picker must run off the UI thread, must send only the typed address to the geocoding service, must be refused without the grant `location.geocode`, and a failed or empty lookup must leave the candidate unchanged and show a notice. Coordinate entry and the map crosshair must work with no network.");
}

/// REQ: PRM-511, F-124
#[test]
fn prm_511_gap_closing() {
    // Observable: Start a fixture client against a fixture collection with two enabled rows and one disabled row and assert two proxies in use; assert the collection received no write and that the config file holds the connection and the mapping but no row.
    panic!("PRM-511 not implemented: The system must read the enabled rows of the paid-proxy collection once at client start, from the database connection and field mapping stored in the settings, and must hand them to the inference workers of the gateway provider. It must never write to that database and must never write the proxy rows to `config.toml`.");
}

/// REQ: PRM-512
#[test]
fn prm_512_gap_closing() {
    // Observable: Start with an unreachable fixture database and assert the notice and one log line; send a request through a provider with a forced proxy and assert the error `network` and no direct connection in the fixture network log.
    panic!("PRM-512 not implemented: When the proxy collection cannot be read or holds no enabled row, the system must report it in the Inference tab and in the log, and a provider for which a proxy is forced must fail its requests with `network` instead of connecting directly.");
}

/// REQ: PRM-513, F-125
#[test]
fn prm_513_gap_closing() {
    // Observable: Start a fixture agent in a worktree and assert the token exists on disk before the first byte of the PTY; request removal and assert a refusal; stop the agent, leave a fixture child in the directory and assert a refusal; end the child and assert the removal succeeds.
    panic!("PRM-513 not implemented: The system must write a durable refusal token for a worktree before any process of an agent starts in it, and must refuse to delete the worktree while a token exists. It must erase a token only after the process lineage is proven stopped and a scan finds no process using the directory.");
}

/// REQ: PRM-514
#[test]
fn prm_514_gap_closing() {
    // Observable: Create a worktree through the plugin and one by hand, close every pane, list the retained worktrees and assert only the first is offered for removal; edit the evidence of the first and assert its removal is refused.
    panic!("PRM-514 not implemented: The system must record ownership evidence for every worktree it creates in the per-worktree metadata of Git, not in the checkout, and must list and remove a retained worktree only when that evidence matches. The inventory and the removal of retained worktrees must work when no pane uses the worktree any more.");
}

/// REQ: PRM-515
#[test]
fn prm_515_gap_closing() {
    // Observable: Create a worktree with a setup command that prints a marker text and exits 1, and assert the checkout and the copied files exist, a failure is reported and the marker text is in no log, record or error.
    panic!("PRM-515 not implemented: The post-create setup command of a worktree must be one shell command that the user supplied, must run in the new checkout, and its output must not be kept, logged or put into an error message. When the command cannot start or fails, the system must keep the checkout and the copied files and report the failure.");
}

/// REQ: PRM-516, F-126
#[test]
fn prm_516_gap_closing() {
    // Observable: Feed a fixture transcript with a wait to the observer and assert the evidence; restart the fixture agent under another process identity and assert the evidence is gone; search the stored state and assert it holds no prompt and no command text.
    panic!("PRM-516 not implemented: The system must derive the wait evidence of a provider cache only from the verified transcript of the live agent process, without keeping prompts or command bodies, and must discard it when the identity of that process changes.");
}

/// REQ: PRM-517
#[test]
fn prm_517_gap_closing() {
    // Observable: Capture a fixture request that created a cache, trigger a refresh and assert an equal account, endpoint and prefix and a zero output limit; trigger one for a request without a cache and assert nothing is sent; change the account and assert the capture is dropped; search the logs and the data folder for the fixture credential and assert no match.
    panic!("PRM-517 not implemented: A cache refresh request must produce no output tokens and must reuse the account, the endpoint, the headers and the exact cached prefix of the original request. The system must send it only when the original request is known to have created a cache, must never store, log or show the captured request, and must drop the capture when the invocation, the account, the endpoint, the cache prefix or the transport changes.");
}
