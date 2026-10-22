---
layout: page
title: Sandbox reference
permalink: /pages/reference-sandbox/
---

## Sandbox reference

This page is the normative definition of the sandbox that confines third-party plugins (tiers T2 and T3). The [Security model](/pages/security/) gives the overview. The [Threat model](/pages/reference-threat-model/) says why. This page says exactly what the host MUST build, so that an implementer can write it and a tester can check it.

Terms. A **helper** is a separate operating-system process that hosts the WebAssembly instance of one plugin side. A **namespace** is a Linux kernel feature that gives a process its own private view of one kind of resource (mounts, process ids, network, and so on). **seccomp** is a Linux filter that lets a process make only listed system calls. A **cgroup** is a Linux group of processes with resource limits. **no_new_privs** is a process flag that stops the process and its children from gaining privileges through set-id programs. **Dispatcher protocol** is the message channel between the Ilium server and a helper.

### You will need

- Linux with user namespaces, cgroup v2 and seccomp. The checks are in the [prerequisite section](#sandbox-unavailable).
- The grant names in the [Capability table](/pages/reference-capabilities/).
- For tests, the hostile fixtures in [Security testing](/pages/reference-security-testing/).

## Topology

**[SEC-020]** The host MUST start one helper process per T2 or T3 plugin and per side. Plugin A and plugin B never share a helper. The server side and the scene side of one plugin run in separate helpers. Client-side T2 plugins run in a helper started by each attached client, with the same confinement. The reason is decision D5: a shared helper would reduce isolation between plugins to Wasmtime alone, and Wasmtime has had sandbox-escape advisories. With one helper per plugin, an escape from Wasmtime reaches only that plugin's own mount namespace.

Consequences accepted by this decision:

- Memory use grows with the number of third-party plugins. Each helper adds a 32 MiB overhead to the sum of its plugins' `memory_mib`; that sum plus the overhead is what the cgroup and `session_memory_mib` count.
- Start-up takes one process start per plugin side. Lazy activation (D6) keeps this off the start path.
- Plugin to plugin calls (services, peer messages) go through the server, never directly between helpers.

<figure>
<svg viewBox="0 0 520 220" role="img" aria-label="One helper per plugin side">
  <title>Helper topology</title>
  <desc>The server holds the dispatcher. Each T2 or T3 plugin side has its own helper with its own mount namespace.</desc>
  <defs>
    <marker id="sb-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="80" width="130" height="60" rx="8" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="71" y="108" text-anchor="middle" fill="currentColor" font-size="11">Ilium server</text>
  <text x="71" y="124" text-anchor="middle" fill="currentColor" font-size="10">dispatcher</text>
  <rect x="200" y="6" width="150" height="56" rx="8" fill="none" stroke="currentColor" stroke-width="3"/>
  <text x="275" y="30" text-anchor="middle" fill="currentColor" font-size="11">Helper: notes, server</text>
  <text x="275" y="48" text-anchor="middle" fill="currentColor" font-size="10">mounts: project (ro), data</text>
  <rect x="200" y="82" width="150" height="56" rx="8" fill="none" stroke="currentColor" stroke-width="3"/>
  <text x="275" y="106" text-anchor="middle" fill="currentColor" font-size="11">Helper: notes, scene</text>
  <text x="275" y="124" text-anchor="middle" fill="currentColor" font-size="10">mounts: data only</text>
  <rect x="200" y="158" width="150" height="56" rx="8" fill="none" stroke="currentColor" stroke-width="3"/>
  <text x="275" y="182" text-anchor="middle" fill="currentColor" font-size="11">Helper: tidy, server</text>
  <text x="275" y="200" text-anchor="middle" fill="currentColor" font-size="10">mounts: worktrees, data</text>
  <path d="M136 100 L200 36" stroke="currentColor" fill="none" marker-end="url(#sb-arrow)"/>
  <path d="M136 110 L200 110" stroke="currentColor" fill="none" marker-end="url(#sb-arrow)"/>
  <path d="M136 120 L200 186" stroke="currentColor" fill="none" marker-end="url(#sb-arrow)"/>
  <rect x="400" y="82" width="112" height="56" rx="8" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="456" y="106" text-anchor="middle" fill="currentColor" font-size="10">process.run child</text>
  <text x="456" y="124" text-anchor="middle" fill="currentColor" font-size="10">same confinement</text>
  <path d="M350 110 L400 110" stroke="currentColor" fill="none" marker-end="url(#sb-arrow)"/>
</svg>
<figcaption>Each helper sees only the scopes its own plugin side was granted. Children inherit the confinement of the helper that started them.</figcaption>
</figure>

## Process confinement

Every row is a MUST. A helper that cannot be started with all of them is not started ([SEC-055]).

| Id | Requirement | How a tester checks it |
|---|---|---|
| [SEC-021] | The helper runs in new user, mount, PID, IPC, UTS and network namespaces. Inside the user namespace it has no capabilities outside it. | Hostile fixture reads `/proc/self/ns/*` and compares to the server's. All six differ. |
| [SEC-022] | The network namespace has no interface except a downed loopback. | Fixture calls `socket`+`connect` to loopback and to a public address. Both fail. |
| [SEC-023] | The mount namespace holds only: read-only `/usr`, `/bin`, `/lib`, `/lib64` and `/etc` (a minimal subset, no `/etc/passwd` home lists beyond uid maps); a fresh `/proc` for the PID namespace; a minimal `/dev` with `null`, `zero`, `urandom`, `full`; a tmpfs `/tmp` capped at 64 MiB; and the granted scopes, read-only for read grants. Every writable mount (`data`, `cache`, `picked`, any writable scope, and the `/tmp` tmpfs) is `noexec,nosuid,nodev`. | Fixture lists `/`; only these paths exist. Executing a binary from `/data` fails with `EACCES`. |
| [SEC-024] | The helper sets `no_new_privs`, installs a seccomp allowlist, and drops every capability before it reads the first plugin byte. | Fixture calls `prctl(PR_GET_NO_NEW_PRIVS)`. Result is 1. |
| [SEC-025] | The sandbox is defence in depth. A bug in Wasmtime that gives native code execution in the helper MUST NOT give access to any asset in the [threat model](/pages/reference-threat-model/) without a second bug in the kernel. | Escape probe suite, see [Security testing](/pages/reference-security-testing/). |
| [SEC-026] | The helper's `/proc` shows only its own PID namespace. It cannot read `/proc/<pid>/environ` of any process outside. `/sys` is not mounted. | Fixture reads `/proc/1/environ` (its own init) and the server's PID path. The second does not exist. |
| [SEC-027] | The helper's environment is rebuilt from an allowlist: `HOME` set to the data mount, `TMPDIR=/tmp`, `LANG=C.UTF-8`, `PATH` set to the fixed value `/usr/bin:/bin`, `ILIUM_PLUGIN_ID`. Nothing else is inherited. In particular `SSH_AUTH_SOCK`, `DBUS_SESSION_BUS_ADDRESS`, `WAYLAND_DISPLAY`, `DISPLAY`, `XDG_RUNTIME_DIR`, proxy variables and any `*_TOKEN`, `*_KEY` or `*_SECRET` variable are absent. | Fixture prints `environ`. Only the allowlist appears. |
| [SEC-028] | All file descriptors except the dispatcher channel and stdio on `/dev/null` are closed before the Wasmtime engine starts (close-range with no exceptions). The helper calls `setsid` and has no controlling terminal. Because the helper has no controlling terminal and cannot open the PTY master (it is not inherited), terminal injection through `TIOCSTI` and `TIOCLINUX` is impossible. The seccomp filter also denies those ioctl numbers. | Fixture enumerates `/proc/self/fd`; only fds 0 to 2 and the channel remain. `ioctl(0, TIOCSTI)` fails. |
| [SEC-029] | A cgroup v2 leaf is created per helper with: `memory.max` = the sum of the hosted instances' `memory_mib` plus 32 MiB overhead; `pids.max` = 64 plus 8 per allowed child; `cpu.weight` = 10 (low priority); `cpu.max` = the manifest `cpu_ms_per_s` quota expressed as a period. A memory kill is reported as a `trap.memory` and counts as a failure. | Fixture allocates past the limit; helper is killed; the log has the code. |

### The seccomp allowlist

**[SEC-067]** The filter is an allowlist of the system calls that Wasmtime, the allocator and the dispatcher channel need. Everything else returns `EPERM`; no call kills the helper. The implementation MUST publish the list as a generated file with the release so tests can compare. These calls are always denied, and the fixture suite calls each of them:

| Group | Calls |
|---|---|
| Debugging and memory of others | `ptrace`, `process_vm_readv`, `process_vm_writev`, `kcmp` |
| Keys and kernel objects | `keyctl`, `add_key`, `request_key`, `bpf`, `perf_event_open`, `userfaultfd`, `io_uring_setup`, `io_uring_enter`, `io_uring_register` |
| Namespaces and mounts | `mount`, `umount2`, `pivot_root`, `chroot`, `unshare`, `setns`, `open_by_handle_at` |
| Kernel modules and boot | `init_module`, `finit_module`, `delete_module`, `kexec_load`, `kexec_file_load`, `reboot` |
| Sockets (all families except the one dispatcher socketpair, which is inherited, not created) | `socket`, `socketpair`, `connect`, `bind`, `listen`, `accept`, `accept4` |
| Terminal injection | `ioctl` with request `TIOCSTI` or `TIOCLINUX` or `TIOCSCTTY` |
| Privilege | `setuid`, `setgid`, `setresuid`, `capset`, `personality` |

### Limits when cgroup delegation is missing

**[SEC-065]** If cgroup v2 is available but the user has no delegated controller (`memory`, `pids`, `cpu`), the host MUST enforce the same limits inside the helper with `setrlimit` (`RLIMIT_AS`, `RLIMIT_NPROC`, `RLIMIT_CPU`) and a watchdog in the server that samples the helper's RSS once a second. The audit log records `sandbox-degraded` with the controller list once per start. If the controllers needed for `memory.max` and `pids.max` are both missing and `setrlimit` cannot bound them, the helper is not started ([SEC-055]).

## Dispatcher protocol trust

**[SEC-050]** The server MUST treat every message from a helper as hostile. The identity of the plugin comes from the channel the message arrived on, never from a field in the message. A message that names another plugin id is dropped and counts as a failure.

**[SEC-051]** The server MUST enforce a maximum message size of 4 MiB (8 MiB for `terminal-view` frames) and a rate of 5,000 messages per second per helper. A larger or faster message ends the helper, and that ending counts as a failure toward the restart-and-disable rule, as in [SEC-052].

**[SEC-052]** A malformed message (bad framing, bad UTF-8, a schema violation, an unknown message kind, an out-of-range resource handle) kills the helper and counts as a failure toward the restart-and-disable rule. The server never tries to repair the message.

**[SEC-053]** Wasmtime configuration, for T1 and for the helper:

| Setting | Value |
|---|---|
| Linear-memory guard pages | On, 2 GiB guard region on 64-bit |
| Spectre mitigations | On (bounds-check hardening for table and memory access) |
| Epoch interruption | On, deadlines set per call; a missed deadline traps |
| `wasi:sockets`, `wasi:http` imports | Refused at install, and checked again at load; a component that imports them is never instantiated |
| `wasi:clocks`, `wasi:random` | Virtualised: clocks coarsened ([SEC-370]), random from the host CSPRNG |
| Wasmtime version | Pinned exactly in the release, recorded in the build metadata, see [Trust and supply chain](/pages/reference-trust-and-supply-chain/) |
| Memory and table maximums | From the manifest `limits`, clamped to the tier ceiling |

**[SEC-054]** Compiled module cache. For T2 and T3 the host MUST NOT load a precompiled artefact that was written by a process it cannot verify. If a cache exists it lives in the host's cache directory, owner-only, keyed by the SHA-256 of the component and the Wasmtime version, and its integrity (an HMAC under a per-install key held by the server) is checked before deserialisation. No plugin scope can reach it. When the check fails the module is recompiled. A deployment MAY disable the cache for T2 and T3 with the policy key `[packages] precompiled_cache = false`.

## Sandbox unavailable

**[SEC-055]** If the host cannot create the sandbox exactly as specified, T2 and T3 plugins MUST refuse to load with error code `sandbox.unavailable`. The message names the missing prerequisite (for example `unprivileged user namespaces are disabled: sysctl kernel.unprivileged_userns_clone`). There is no silent fallback to in-process or unconfined execution, on any platform and for any reason.

Typical prerequisites and the message the host prints:

| Prerequisite | Typical failure | Message names |
|---|---|---|
| Unprivileged user namespaces | Ubuntu 24.04 AppArmor restriction, container without `CAP_SYS_ADMIN`, hardened kernels | `user namespaces` and the sysctl or AppArmor profile |
| cgroup v2 | cgroup v1 only | `cgroup v2` |
| seccomp | Kernel built without it | `seccomp` |
| Mount of `/proc` in a PID namespace | Locked-down container | `procfs mount` |

T1 plugins and the kernel are unaffected. The Plugins screen shows an installed T2 or T3 plugin as `Cannot start: sandbox unavailable` with the message. The audit log records `sandbox-unavailable` once per start and prerequisite.

**[SEC-056]** Running as root. If the server's effective uid is 0, the host MUST refuse T2 and T3 plugins with `sandbox.unavailable` and the message `running as root is not supported for third-party plugins`. Root has capabilities that user-namespace rules do not remove for the server itself. A deployment that needs root MAY run Ilium as an unprivileged user instead.

**[SEC-057]** Platform matrix.

| Platform | T1 | T2/T3 | Reason |
|---|---|---|---|
| Linux, native | Yes | Yes, with the prerequisites above | Full sandbox |
| Linux, container | Yes | Only if the container grants user namespaces and cgroup delegation | Many container defaults refuse |
| WSL 2 | Yes | Yes if the kernel prerequisites hold, otherwise `sandbox.unavailable` | A real Linux kernel |
| WSL 1 | Yes | No | No namespaces |
| macOS | Yes | No (decision D5) | No equivalent sandbox |
| Windows | Yes | No (decision D5) | No equivalent sandbox |

## File scopes

File access is the most common route to harm, so its rules are normative here. The rules apply inside the helper and to the kernel's own file functions for T1.

The authoritative scope table. Other pages link here.

| Scope | Read | Write | Watch | Tiers | Notes |
|---|---|---|---|---|---|
| `package` | always | never | never | all | Unpacked `assets/`, read only |
| `data` | always | always | always | all | Plugin's private folder, mounted `noexec,nosuid,nodev` |
| `cache` | always | always | always | all | May be evicted |
| `project` | normal | H | normal | all | The session's project folder; `.ilium/` is excluded: it is missing from listings and every access returns `not-permitted` |
| `picked` | per pick | per pick | per pick | all | A path the user chose |
| `worktrees` | normal | H | normal | all | Setting `workspace.worktree_root` |
| `agent-sessions:<agent>` | H | refused | H | all | Transcript folders, read only |
| `agent-config:<agent>` | H | refused; changes go through `agent-config.propose:<agent>` (H) | refused | all | Configuration folders, credential files excluded ([SEC-063]); the host writes only the changes the user accepts in a proposal |
| `minecraft-saves` | normal | H | normal | all | The game's saves folder, for the Minecraft scenes |
| `path:{project}` / `{data}` / `{cache}` | normal | H | normal | all | Same as the named scope |
| `path:{config}` | H | H | H | all | The Ilium configuration folder; the deny set still applies |
| `path:{home}/...` | normal below a sensitive path-free subtree, H otherwise ([SEC-061], [SEC-062]) | H | same as read | all | Template with `{home}` |
| literal absolute path | H | H | H | all | Review dialog shows the path |

Writes to `agent-sessions` fail at install with `manifest.grant-scope`. Writes to `agent-config:<agent>` are also refused at install with `manifest.grant-scope`, and so is a watch on it. Agent scopes are read-only for plugins and `H` for read. A plugin that needs to change agent configuration holds the H grant `agent-config.propose:<agent>` instead: it submits whole new file contents, the kernel shows the full diff in a consent dialog, and the host applies only the files the user checks, all or none, after saving the previous contents in the plugin's `data` scope. Credential files ([SEC-063]) and the deny set ([SEC-060]) stay out of every proposal. See [agent-config.propose](/pages/capability-agent-config-propose/).

**[SEC-058]** Path resolution uses `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV` relative to a file descriptor opened on the scope root (or the closest equivalent where `openat2` is missing, which is an `openat` walk that checks every component with `O_NOFOLLOW`). The kernel never checks a path string and then opens it later. A symlink swap between check and open therefore cannot leave the scope.

**[SEC-059]** Hard links. When a file in a scope has a link count above 1, a write or a read-for-write open is refused when the device and inode differ from the scope's recorded inventory for writable scopes, and always refused when the other link is outside every writable mount (the host reads `st_nlink` and compares `st_dev` with the scope root). The error is `not-permitted`.

**[SEC-060]** The deny set. No scope, for any plugin including T1 plugins other than the owning core plugin, reaches: the Ilium configuration (`config.toml`), the grant store, the trust store, the consent store, the project trust store, the secrets file, the admin policy file, the audit log, the plugin install directory, every other plugin's `data` and `cache`, the runtime directory and the Ilium socket, and the HTTP API token file. A template that resolves into the deny set is refused at install with `manifest.grant-scope`. A path that passes the check at install but resolves into the set at run time (a symlink created later) is refused with `not-permitted`.

**[SEC-061]** The sensitive path set. A grant whose resolved root is, contains or lies below one of the following is high risk, and the review dialog shows the resolved path: `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.config/gcloud`, `~/.kube`, `~/.netrc`, `~/.git-credentials`, `~/.docker/config.json`, OS keyrings (`~/.local/share/keyrings`), browser profiles (`~/.mozilla`, `~/.config/google-chrome`, `~/.config/chromium`), password-manager stores, and agent credential files.

**[SEC-062]** A template whose resolved root is `$HOME`, `/`, or an ancestor of `$HOME` is high risk, because it reaches the sensitive set. `fs.read:path:{home}` is therefore H. `fs.read:path:{home}/Pictures/**` is normal.

**[SEC-063]** Agent credential files are excluded from `agent-config` scopes even with the grant. The named lists, relative to each agent's configuration folder:

| Agent | Excluded files |
|---|---|
| `claude` | `.credentials.json`, `credentials.json`, `.claude.json` keys block (the file is served with `oauthAccount` and key fields removed) |
| `codex` | `auth.json` |
| `gemini` | `oauth_creds.json`, `google_accounts.json` |
| `antigravity` | `auth.json`, `oauth_creds.json` |
| every agent | any file whose name matches `*credentials*`, `*token*`, `*secret*`, `*.pem`, `*.key` |

**[SEC-064]** Errors never include the resolved absolute path. A failing call returns the scope and the path relative to the scope root only.

Case-folding filesystems: the scope check compares resolved file identities (device and inode), not names, so `Data` and `data` cannot cross a boundary. Bind mounts: an `openat2` with `RESOLVE_NO_XDEV` stops at a mount point inside a scope root.

## Escape-hatch review

An escape hatch is a feature whose effect leaves the sandbox by design. Each one has a classification and a required mitigation.

| Feature | Class | Required mitigation | Id |
|---|---|---|---|
| `panes.create` with a `command` | Code execution | Needs the H grant `panes.create-command:<program>`; kernel confirmation on first use; see below | [SEC-040] |
| `panes.lifecycle` restart with a new command | Code execution | Same program grant; the confirmation names the program | [SEC-041] |
| `agent.signatures` resume argv | Code execution | Program must match `process_names` or a `panes.create-command` grant; one argv element for `{session}` | [SEC-042] |
| `terminal.write`, `terminal.raw-input` | Code execution (types into a shell) | H grants; `send-input` dedup and journal; lock rules | existing |
| `agent.control` | Code execution (drives an agent) | H grant | existing |
| `process.run:<program>` | Code execution | H grant per program; env denylist; fixed PATH; noexec data | [SEC-031] to [SEC-035] |
| `process.run:sh` | Code execution | T1 only | existing |
| `fs.write` to dotfiles or agent config | Code execution at next shell or agent start | Write outside `data` and `cache` is H; direct `agent-config` writes refused; agent configuration changes only through an H `agent-config.propose:<agent>` proposal that the user reviews as a diff; dotfile paths in `{home}` are in the sensitive set | [SEC-061], [CAP-887] |
| `clipboard.write` | UI integrity (paste hijack) | Size and control character rules | [SEC-044] |
| `open-external` file targets | Code execution through the desktop handler | Scope, mode bit and extension rules | [SEC-043] |
| `notify.desktop` | UI integrity (spoofing) | Title prefix by the host | [SEC-045] |
| `keys.intercept` | UI integrity (captures keystrokes) | Review wording names it; only active while the plugin's pane or mode has focus; audit | existing, [SEC-102] |
| `editor.provide` | UI integrity and data (sees file text) | H, review wording | [SEC-102] |
| `sessions.cross` | Acts in other sessions | T1 and T2 only; same-plugin commands only | existing |
| `workspace.manage` | Moves or deletes folders | H, review wording | existing |
| `cli.contribute` output read by agents | Prompt injection into agents | Output is plain text with a header `from plugin <id>`; agents are told by `ilium.core.agent-setup` instruction blocks | existing |
| http-api routes | Data egress to local clients | Routes need the token; see [Network egress](/pages/reference-network-egress/) | [SEC-097] |
| `collect` entries driving a T1 owner's action | Code execution via the owner | Owner validates with an allowlist; dialog shows what the entry can do | [SEC-283] |
| Replacement of a core plugin | Everything the core plugin could do | T3 cannot replace; no grant inheritance | [SEC-300] |

**[SEC-030]** Every row in this table MUST have the mitigation implemented. A feature added to the contract later MUST add a row before release.

### Process execution rules

**[SEC-031]** `process.run` and `process.spawn` need a grant for the exact program. A bare name such as `git` is resolved against the fixed system `PATH` `/usr/bin:/bin` (directories `/usr/bin` and `/bin`). It is never resolved against the plugin's own environment or against any writable scope.

**[SEC-032]** Children of T2 and T3 plugins run in the same confinement as the helper and have no network. A program that needs the network, such as `git fetch`, therefore fails by design. A plugin that needs a remote operation uses `http`.

**[SEC-033]** Environment keys are validated. These are refused with `invalid-argument`: `LD_*`, `DYLD_*`, `PATH`, `IFS`, `BASH_ENV`, `ENV`, `PYTHON*`, `NODE_OPTIONS`, `PERL5*`, `RUBY*`, `GIT_CONFIG*`, `GIT_SSH*`, `GIT_EXEC_PATH`, `GIT_ASKPASS`, plus any key that the admin policy adds under `[processes] deny_env`.

**[SEC-034]** A program may not resolve into `package`, `data`, `cache`, `picked` or any writable scope. Writable mounts of T2 and T3 are `noexec`, so a plugin cannot write a binary and run it. Absolute-path grants (`process.run:/usr/bin/git`) are compared to the resolved real path.

**[SEC-035]** Argument hazards. The review dialog shows for every `process.run:<program>` the line `This program can run other programs.` when the program is on the host's list of known launchers (`git`, `sh`, `bash`, `python*`, `node`, `rg` with `--pre`, `find`, `xargs`, `env`, `make`, `ssh`, `sudo`). The documentation of `process.run` warns of `git -c core.sshCommand=...`, `git` hooks in the repository, and `rg --pre`. The host does not filter arguments, because that cannot be done in general.

**[SEC-066]** A spawn that leaves the confinement (`panes.create` command, `panes.lifecycle` restart) is written to the audit log with the full argv, with secret values replaced by `<redacted>`.

### Pane commands

**[SEC-040]** `create-pane` with a `command` from a T2 or T3 plugin needs the H grant `panes.create-command:<program>`. `panes.create` itself stays normal risk and allows only a shell pane in a cwd below a scope the plugin may read. The pane process starts unconfined, with the user's privileges, so the kernel draws a confirmation the first time each program is started: `Notes Panel wants to run git in a new pane.` and the full argv. The answer is remembered per plugin and program as a consent. A command whose program matches no `panes.create-command` grant completes `refused(not-permitted)` with reason `not-granted` (`declined` when the grant was requested but unchecked). A command-less `create-pane` needs only `panes.create`. The install fails with `manifest.grant-dependency` when the manifest lists `panes.create-command:<program>` without `panes.create` in the same side. PATH is resolved as in [SEC-031]. T1 plugins follow the same rule: the grant is H and is asked at first activation.

**[SEC-041]** `panes.lifecycle` restart with a new command uses the same grant and the same confirmation.

**[SEC-042]** `agent.signatures` rules: the `resume` argv program must equal one of the signature's `process_names` or match a `panes.create-command:<program>` grant of the same plugin, otherwise the resume is not offered. `{session}` is substituted as exactly one argv element, never through a shell, and it is validated against `[A-Za-z0-9._:-]{1,128}`; a session id outside it makes the resume unavailable. The resume runs only on an explicit user action, and the confirmation shows the full command the first time. `actions.keys` are delivered only when the user or a plugin that holds `agent.control` triggers them. A plugin cannot mark a pane as an agent when the pane's process name does not match the signature's `process_names`. With these constraints `agent.signatures` stays normal risk.

### Other hatches

**[SEC-043]** `open-external` file targets: only paths in scopes the plugin can read; refused when the file has any executable mode bit; refused for the extensions `.desktop`, `.exe`, `.bat`, `.cmd`, `.com`, `.sh`, `.jar`, `.app`, `.appimage`, `.scr`, `.lnk`, `.command`, `.html`, `.htm`, `.svg`, `.js`, `.py`, `.pl`, `.rb`; and refused for MIME types the host treats as active content. Outcome code `refused(not-permitted)` with reason `file-type`. The origin prompt shows the punycode form for hosts that contain non-ASCII characters and highlights the registrable domain. A plugin may have at most one pending consent prompt and 6 per hour ([SEC-105]).

**[SEC-044]** `clipboard.write`: text larger than 64 KiB, or containing any control character other than tab, newline and carriage return, is refused with `invalid-argument`. Text with a newline is allowed. A write is accepted only from a user-initiated command or a keybinding while the user has focus in a terminal pane; a write caused by a timer or an event while the terminal has focus is refused with `not-permitted` reason `not-user-initiated`.

**[SEC-045]** `notify.desktop`: the host prefixes the title with the plugin display name and a colon. The plugin cannot supply the prefix.

**[SEC-046]** Regular expressions supplied by plugins (output filters, settings `pattern`, schema `pattern`, `when` expressions) MUST be compiled with a linear-time engine that has Rust `regex` semantics: no backreferences and no lookaround. A pattern is at most 1,024 bytes, and its compiled automaton is at most 256 KiB. A pattern that fails any limit is refused with `invalid-argument` at registration. Matching against a 1 MiB input finishes in time proportional to the input.

## Kernel prerequisites and diagnostics

Target behaviour (specified here before the code is written).

- [SEC-442] Ilium MUST document a minimum Linux kernel for T2 and T3 plugins and MUST check at server start, once per start, each prerequisite of [SEC-055] separately: unprivileged user namespaces, cgroup v2 delegation, seccomp filter support and `openat2` availability. The result of each check MUST be stored for the diagnostics below.
- [SEC-443] `ilium plugin doctor --sandbox` MUST print one JSONL `check` line per prerequisite with `name`, `ok`, the kernel value found and, when `ok` is false, the exact remediation (the sysctl, the kernel option or the cgroup delegation step). It MUST exit 0 when all pass and 1 otherwise, and it MUST NOT change any system setting.
- [SEC-444] A kernel without `openat2` MUST NOT make `sandbox.unavailable` by itself: the `openat` walk of [SEC-058] applies and `doctor` reports `openat2` as `ok: false, degraded: true`. A missing user namespace, cgroup v2 or seccomp prerequisite MUST give `sandbox.unavailable` with the message naming that one prerequisite.

## Edge cases

| Situation | Result |
|---|---|
| Two plugins ask for the same scope | Each helper gets its own bind mount. They never see each other's `tmp` |
| A scope root is deleted while a plugin runs | Reads return `not-found`; the mount is re-created at next activation |
| Plugin exceeds `pids.max` | `fork` fails inside the helper with `EAGAIN`; the plugin sees `quota-exceeded` for spawns |
| The server is killed | Helpers see the dispatcher channel close and exit within 2 s; children get SIGTERM then SIGKILL |
| A T2 plugin is installed on a machine with no user namespaces | Install succeeds (it is only files). Load fails with `sandbox.unavailable` |

### See also

- [Security model](/pages/security/)
- [Threat model](/pages/reference-threat-model/)
- [Network egress](/pages/reference-network-egress/)
- [Capability: process.run](/pages/capability-process-run/)
- [Security testing](/pages/reference-security-testing/)
- [Design decisions](/pages/design-decisions/)

Contract: `ilium:plugin@0.1.0`.
