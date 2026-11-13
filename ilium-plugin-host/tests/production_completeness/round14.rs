// Round-fourteen production requirements (PRN IDs), red until implemented.

/// REQ: PRN-001
#[test]
fn prn_001_gap_01() {
    // Observable: reference-power-and-battery.md and the budget table bound idle wake-ups (B24, 100 per second), but I found no bound on steady-state idle CPU time or idle resident memory growth for a host with N enabled but unused plugins. A rg for 'idle cpu' and 'wakeups' in the budgets, performance and sizing pages found no matching requirement.
    panic!("PRN-001 not implemented: The system must, with 40 enabled plugins and no user input for 10 minutes, keep whole-host CPU use at or below 1 percent of one core on the reference machine and keep resident memory growth at or below 8 MiB over that period, and the soak harness must fail with budget.idle-exceeded otherwise.");
}

/// REQ: PRN-002
#[test]
fn prn_002_gap_02() {
    // Observable: Offline use is specified for update checks and verify (HST-693, HST-694), but I found no requirement for what the user sees when the cached revocation list is older than plugins.revocation_max_age_days while offline: whether install is refused, warned or allowed on already-installed plugins, and whether already-installed plugins keep running.
    panic!("PRN-002 not implemented: The system must, when the cached revocation list is older than plugins.revocation_max_age_days and no network is reachable, keep already-installed plugins running unchanged, refuse a new install or update with trust.revocation-stale unless the user confirms in a dialog naming the age in days, and record the confirmation in the audit log.");
}

/// REQ: PRN-003
#[test]
fn prn_003_gap_03() {
    // Observable: Soak testing covers load, but I found no requirement that a release-gating test exercises repeated install, update, rollback and uninstall cycles for leaked files, descriptors or registry growth. PRR-007 checks leftovers after one purge only.
    panic!("PRN-003 not implemented: The system must pass a release gate that runs 200 consecutive install, update, rollback and uninstall cycles of the same plugin id and ends with the data root, open file descriptor count and installed.json size within 1 percent of their starting values, failing with soak.lifecycle-leak and naming the first growing artefact.");
}

/// REQ: PRN-004
#[test]
fn prn_004_gap_04() {
    // Observable: Support bundles are specified for content, redaction and consent (OPS-005, PRR-004, PRP-018), but I found no upper size bound or deterministic truncation order when logs and traces are large, so a bundle may be too large to attach to a support request.
    panic!("PRN-004 not implemented: The system must cap a support bundle at 25 MiB by default, truncate by dropping the oldest log lines first while always keeping the header, the last crash record and the operation-id index, and state in a bundle manifest entry each file that was truncated and by how many bytes.");
}

/// REQ: PRN-005
#[test]
fn prn_005_gap_05() {
    // Observable: Registry tokens are issued only after a second factor and last 90 days (PKG-968, SEC-992). CI publishing therefore needs a long-lived stored secret that is rotated by hand. No page defines a keyless alternative, such as a short-lived credential bound to one CI run and one repository. An exact search for OIDC, workload identity, ambient credential and trusted publishing found nothing in the manual.
    panic!("PRN-005 not implemented: The system must let a publisher register a trusted CI identity (issuer, repository, workflow) for a namespace, and the registry must exchange a verifiable CI identity assertion for a publish credential that expires within 15 minutes, is valid for one publisher id, and is refused with registry.auth when the assertion\'s repository or workflow does not match the registration.");
}

/// REQ: PRN-006
#[test]
fn prn_006_gap_06() {
    // Observable: AUT-954 tells authors to send users to the Ilium project when the host caused a failure. Nothing gives an author a way to learn that a host release has a known defect that matches a failure they are debugging. Searches for known issues, errata and host bug found no such mechanism. The manual only has the generic 'internal' error code (a host bug).
    panic!("PRN-006 not implemented: The system must publish a signed, machine-readable feed of known host defects. Each entry must carry affected host versions, the error codes or log events that signal it, a workaround and the fixing version. `ilium plugin doctor` must print every entry that matches the installed host and the project\'s last failing run, and must label the entry as a host defect rather than a plugin defect.");
}

/// REQ: PRN-007
#[test]
fn prn_007_gap_07() {
    // Observable: The templates cover CI files, licence and lint settings, but no page says the generated project can be opened in a reproducible development environment. Searches for devcontainer, dev container and Docker found only sandbox and security pages, and the toolchain pages state no pinned container route. An author on a new machine therefore has to match the toolchain matrix by hand.
    panic!("PRN-007 not implemented: Each built-in template must ship a development-environment definition that pins the toolchain versions of reference-toolchain-support-matrix (Rust, Node, wasm tooling and the ilium CLI). A CI test must open it from a clean checkout and run `ilium plugin doctor`, `build`, `check` and `test` to success with no network access beyond the registry and package mirrors.");
}

/// REQ: PRN-008
#[test]
fn prn_008_gap_08() {
    // Observable: PRW-009 and PRS-001 exclude private panes and projects only for Titles and Notifications. Search embeds pane titles and screen snippets and sends them to the provider and stores them on disk. Recap and Compaction send transcript text to a provider. None of these three pages says that private panes, private projects or exclude-listed directories are left out.
    panic!("PRN-008 not implemented: The system must not embed, index, summarise or compact the text of a pane marked private, a pane in a private project, or a pane in a directory on the plugin\'s exclude list. Search, Recap and Compaction must send no byte of such a pane to any inference provider and must not store it in their data scope. Each must show the pane as \'excluded (private)\' in its result list or command, and one privacy-log record must name the exclusion without the text.");
}

/// REQ: PRN-009
#[test]
fn prn_009_gap_09() {
    // Observable: The Persistence paragraph says the manual states no rule that removes entries of a closed pane, and leaves cleanup to the plugin. There is no bound on index size, no rule for a changed embedding model, and no rule for deleting a session.
    panic!("PRN-009 not implemented: The system must remove a pane\'s search-index entries within 5 seconds of the pane closing and remove all entries of a session when it is deleted. It must cap the on-disk index at a configurable size (default 64 MiB) by evicting the oldest entries first. It must drop and rebuild the index after the selected embedding model changes, and show \'Index rebuilding\' while it does. A test closes 100 panes, deletes a session and switches model, then reads back the index entry count, size and model stamp.");
}

/// REQ: PRN-010
#[test]
fn prn_010_gap_10() {
    // Observable: No page defines the PTY size, or the render size of a pane, when several clients with different terminal sizes are attached to one session. PRO-001 covers only a single client that is too small. The words 'smallest client' and 'different sizes' do not appear anywhere in the manual.
    panic!("PRN-010 not implemented: The system must give each terminal pane one PTY size, the minimum columns and minimum rows over the clients currently viewing it, and must recompute it within 100 ms when a client attaches, detaches or resizes. Each larger client must draw the pane at that size and show the unused area as a visible margin. A client in the background or too small (PRO-001) must not count. The system must emit one pane-resized event per change, and a test with clients of 80x24 and 120x40 must read back identical PTY dimensions in both.");
}

/// REQ: PRN-011
#[test]
fn prn_011_gap_11() {
    // Observable: Nothing says what the Ambient and Scenes plugins do when no client is attached, when the client is hidden or when its pane is not visible. They run at the frame-rate cap with only a view-visibility pause on the settings preview. reference-power-and-battery is not tied to them.
    panic!("PRN-011 not implemented: The system must stop all ambient and scene rendering work, including frame generation and video decode, within 1 second when no client is attached or the client reports a hidden terminal. It must resume without a visible jump on the next attach. While running, it must cap the process CPU use at the configured budget (default 5 percent of one core at 24 fps on the 80x24 reference) and lower the frame rate automatically when the cap is exceeded. A test measures CPU use with zero clients and with a hidden client.");
}

/// REQ: PRN-012
#[test]
fn prn_012_gap_12() {
    // Observable: COR-018 defines price matching by prefix and defaults for cache_write. It does not say what the Cost plugin shows for a model with no matching price entry, or for a price table older than the models in use. A missing price could be shown as 0.
    panic!("PRN-012 not implemented: The Cost plugin must show spend for a model with no matching price as \'unpriced\', never as zero, and must exclude it from totals and from the budget comparison. It must show one line \'N requests unpriced\' with an action that opens the prices setting. The built-in table must carry an issue date, and the plugin must show a stale-prices notice when that date is more than 90 days old. A test sends usage for an unknown model and checks the displayed totals, the unpriced count and that the budget alert does not treat it as free.");
}

/// REQ: PRN-013
#[test]
fn prn_013_gap_13() {
    // Observable: The palette page describes keyword matching only. PRQ-008 sets grouping, duplicate titles and scrolling, but the manual does not fix the result ordering for a query, how recently used commands are ranked, or whether a disabled core plugin's commands appear.
    panic!("PRN-013 not implemented: The system must order palette results by match quality, then by use in the last 30 days for that user, then by owner (core first) and title. It must list a command of a disabled plugin only in a separate \'Unavailable\' group with the enable action (PRQ-007). With an empty query it must show the 5 most recent commands first. The same query on the same data must give the same order in every client and language, and a test checks this for two languages.");
}

/// REQ: PRN-014
#[test]
fn prn_014_gap_14() {
    // Observable: Footer, tree and status contributions from many plugins have no stated priority or overflow rule. PRO-001 only covers a terminal below 20x6. Between that and a comfortable width, the manual does not say which status items drop first or how a user finds a hidden one.
    panic!("PRN-014 not implemented: The system must lay out status-area contributions by declared priority, then by core-first owner order. When the width is insufficient it must drop whole items from the lowest priority up, never cut one in the middle. It must show a \'+N\' overflow marker that opens a list of all items and their owners with keyboard access. Trusted host items (safe mode, consent, crash banners) must never be dropped. A test contributes 20 items at widths 40, 80 and 200 and checks the visible set, the marker count and that a plugin cannot raise its own priority above host items.");
}

/// REQ: PRN-015
#[test]
fn prn_015_gap_15() {
    // Observable: Compaction sends text to an agent or a provider and can change an agent's context. The page has the threshold rule COR-025 but no requirement for a failed or interrupted compaction, for a pane that is mid-turn, or for undoing an automatic compaction.
    panic!("PRN-015 not implemented: With Automatic on, the Compaction plugin must compact only a pane whose turn state is Idle, must skip it with one notice when the agent is working or awaiting approval, and must not compact the same pane twice within 10 minutes. It must write a pre-compaction snapshot reference to the notice log, and on provider failure it must change nothing in the pane and show the cause. A test drives a fixture agent through working, idle and failed-compaction states.");
}

/// REQ: PRN-016
#[test]
fn prn_016_gap_16() {
    // Observable: PRW-009 (Titles) and PRS-001 (Notifications) both rely on a pane or project 'marked private', but no page defines that mark: who sets it, how it is persisted, or its effect on plugin read grants. Terminal read, history, cell, journal and agent-transcript grants have no host-level exclusion for sensitive panes. The only protection is a no-echo check on the screen-changed event (SEC-371) and the advice in capability-terminal-read-screen.md that the plugin should explain the risk to the user.
    panic!("PRN-016 not implemented: The system must provide a user-settable \'private\' mark on a pane and on a project, stored in the session snapshot and project settings. While it is set, terminal.read-screen, terminal.read-cells, terminal.history, panes.read content fields, capability-journal-read and agent.transcript reads for that pane must return not-permitted with reason `private-pane` for every T2 and T3 plugin, and must write one audit record per refused plugin per pane. Test: mark a pane private, call each read from a plugin that holds the grant, and assert the refusal code. Then clear the mark and assert the reads succeed.");
}

/// REQ: PRN-017
#[test]
fn prn_017_gap_17() {
    // Observable: `ilium-plugins.lock` lives in the project and each entry carries a `source` field. Boundary B7 treats `.ilium/` files from a cloned repository as hostile, but the manual says nothing about validating a lock file's `source` values. PKG-428 only says `--locked` never contacts a registry for resolution. A hostile repository can therefore name an arbitrary URL, a plain-http URL, a loopback or private-range address, or a local path as a package source.
    panic!("PRN-017 not implemented: The system must validate every `source` in an `ilium-plugins.lock` against the configured registries, mirrors and the policy source allow-list before any network or file access. It must refuse an entry with scheme http, a loopback or private-range host, or a path outside the lock file\'s project, failing with `dependency.lock-source-refused` and exit 2 without making any request. A lock from a project that is not trusted must not be applied by `--locked` without the project-trust dialog of SEC-116. Test: a lock with `source` set to `http://127.0.0.1:8080/x`, then to `file:///etc`, then to an unlisted https host. Assert zero outbound connections and the error code each time.");
}

/// REQ: PRN-018
#[test]
fn prn_018_gap_18() {
    // Observable: Scoping an incident is entirely manual. The runbook tells the administrator to read audit records by hand to learn what a killed or blocklisted plugin could reach. The audit log records only the first egress host and process runs, not each read. No command combines the grants the plugin held, the time it was active, the folders and panes it could read, and its secrets, into one per-plugin 'could have reached' list. So users do not know which credentials to rotate after a malicious plugin ran.
    panic!("PRN-018 not implemented: The system must provide `ilium plugin exposure <id> [--since <time>] [--json]`. It prints, from the audit log and the grant history, the intervals in which the plugin was enabled, the grants held in each interval, the resolved file roots, the pane kinds and agent configuration folders in reach, whether it held `http` or `inference` egress, and the names of secrets it holds. It must label each item \'could have\' or \'did\', and never state \'none\' where the audit log lacks the data (it says \'unknown\'). Test: install a plugin with fs.read:project and http, kill it, and assert the output lists both grants with the correct interval and marks reads as \'could have\'.");
}

/// REQ: PRN-019
#[test]
fn prn_019_gap_19() {
    // Observable: EXT-072 keeps a stale list (more than 7 days without refresh) enforced and shows `Revocation data is stale`. It does not say what an install or update of a T2 package, or a new high-risk grant, does while the data is stale. An offline or air-gapped machine can therefore install a package signed by a key that was revoked days ago, with only a generic banner.
    panic!("PRN-019 not implemented: The system must, when revocation data is stale per EXT-072, show a line `Revocation data is N days old` in the install and update review dialog and in `ilium plugin install --json` as `revocation_age_days`. It must require `--accept-stale-revocation` for a non-interactive T2 install or update, and must refuse with exit 8 when the admin policy sets `require_fresh_revocation = true`. Test: age the list by 8 days and assert the notice, the flag requirement and the policy refusal.");
}

/// REQ: PRN-020
#[test]
fn prn_020_gap_20() {
    // Observable: PRR-018 (confinement self-test) and PRO-026 (denial counting) cover detecting a failing or probing sandbox. The manual does not define what the host does when a helper shows signs of an actual escape: a denied syscall class that only an exploited Wasmtime could reach, a helper memory fault outside guest linear memory, or a helper that opens a path not present in its mount namespace. Nothing says the host must quarantine the plugin across restarts, keep a forensic record, or tell the user to treat the machine as exposed. SEC-025 and SEC-171 only say how a patched release is delivered.
    panic!("PRN-020 not implemented: The system must classify a helper termination by a SIGSYS from the seccomp filter, a Landlock denial for a path outside the helper\'s mount, or a signal fault outside guest memory as `suspected-escape`. It must immediately kill the helper and every child, set the plugin to the `quarantined` containment state so it does not restart until the user re-enables it in a kernel dialog, write one `suspected-escape` audit record with the plugin id, the signal or denial class and the helper\'s own cgroup id, and show a kernel notice that says to treat the plugin\'s reachable data as exposed. Test: a fixture guest that triggers a forbidden syscall must end up quarantined, must not restart after a server restart, and must leave exactly one audit record.");
}
