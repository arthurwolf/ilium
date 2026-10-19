# Requirements for capabilities

## Requirements for capabilities

Normative behaviour of grants: syntax, risk, install checks, dependencies, per-side grants and revocation.

The rows follow the format and the observation channels of the [requirements index](/pages/reference-requirements/). The id prefix is `CAP`.

| Id | Requirement | Source | Observable test |
|---|---|---|---|
| [CAP-001] | A grant string MUST be name or name:argument. The first colon MUST split name and argument. Later colons MUST belong to the argument. | [grant syntax](/pages/reference-capabilities/#grant-syntax) | (install-error) fs.read:picked:x -> argument is picked:x and the grant is judged on that. |
| [CAP-002] | An unknown grant name MUST fail the install with manifest.grant-unknown. | [errors and edge cases](/pages/reference-capabilities/#errors-and-edge-cases) | (install-error) list "files.read" -> manifest.grant-unknown. |
| [CAP-003] | A wildcard in a forbidden place MUST fail with manifest.grant-wildcard. commands.invoke:com.example.notes/* MUST be valid and commands.invoke:*/notes MUST be refused. | [grant syntax](/pages/reference-capabilities/#grant-syntax) | (install-error) list commands.invoke:*/notes -> manifest.grant-wildcard. |
| [CAP-004] | A grant that takes no argument MUST fail with manifest.grant-argument when given one. A grant that needs an argument MUST fail with manifest.grant-argument-missing when it has none. | [grant syntax](/pages/reference-capabilities/#grant-syntax) | (install-error) list process.run with no program -> manifest.grant-argument-missing. |
| [CAP-005] | A grant limited to bundled T1 plugins MUST fail with manifest.grant-tier in any other tier. | [errors and edge cases](/pages/reference-capabilities/#errors-and-edge-cases) | (install-error) a T2 package lists a T1-only grant -> manifest.grant-tier. |
| [CAP-006] | A grant MUST be refused with manifest.grant-side when listed on a side where it is not available. | [errors and edge cases](/pages/reference-capabilities/#errors-and-edge-cases) | (install-error) list a client-only grant under capabilities.server -> manifest.grant-side. |
| [CAP-007] | fs.pick MUST need fs.read:picked or fs.write:picked, and settings.write-all MUST need settings.read-all. A missing dependency MUST fail with manifest.grant-dependency. | [errors and edge cases](/pages/reference-capabilities/#errors-and-edge-cases) | (install-error) list fs.pick alone -> manifest.grant-dependency. |
| [CAP-008] | An fs scope that is unknown, or that does not accept the access, MUST fail with manifest.grant-scope: fs.write:package, fs.write:agent-sessions:<agent>, and an absolute path without a path template. | [Write grants](/pages/capability-fs-write/#grants-and-scopes) | (install-error) list fs.write:package -> manifest.grant-scope. |
| [CAP-009] | A process.run grant with arguments, a wildcard or a relative path with a separator MUST fail with manifest.grant-program. | [Run grants](/pages/capability-process-run/#grants) | (install-error) list "process.run:git status" -> manifest.grant-program. |
| [CAP-010] | Grants MUST have a risk tier of always, normal or high. The install dialog MUST list them by tier. A high grant MUST be listed and MUST be unchecked by default. | [grant syntax](/pages/reference-capabilities/#grant-syntax) | (screen-text) install a package with a high grant -> the dialog row is unchecked. |
| [CAP-011] | The always-granted capabilities (log, clock, timers, storage, settings, codec, fs.read:package, fs.read:data, fs.write:data, fs.read:cache, fs.write:cache, own-side messages, ui.invalidate for own views) MUST need no manifest entry and no user grant. | [always granted](/pages/reference-capabilities/#always-granted) | (sync-return) call log, storage and fs.write:data with an empty capabilities list -> every call succeeds. |
| [CAP-012] | Grants MUST be recorded per plugin and per side. A server grant MUST NOT authorize a client call. | [who grants what](/pages/reference-capabilities/#who-grants-what) | (sync-return) the client calls a server-only import -> not-permitted. |
| [CAP-013] | When the user declines a grant the plugin MUST still load. Calls that need it MUST return not-permitted. The granted list MUST omit it. | [errors and edge cases](/pages/reference-capabilities/#errors-and-edge-cases) | (sync-return) decline fs.read -> activate succeeds, fs.read returns not-permitted, granted omits fs.read. |
| [CAP-014] | A refused call MUST be logged as: refused <grant> because <reason>. | [errors and edge cases](/pages/reference-capabilities/#errors-and-edge-cases) | (log-line) call without the grant -> a log line starting "refused ". |
| [CAP-015] | Revoking a grant at run time MUST re-activate the plugin with the reduced set. In-flight calls that depended on it MUST end with revoked. Contributions that depended on it MUST be withdrawn. | [changing grants while the plugin runs](/pages/reference-capabilities/#changing-grants-while-the-plugin-runs) | (completion) revoke fs.read during a read -> failed or err revoked, then a new activate with granted reduced. |
| [CAP-016] | An upgrade that asks for new grants MUST show the user only the new grants. | [changing grants while the plugin runs](/pages/reference-capabilities/#changing-grants-while-the-plugin-runs) | (screen-text) upgrade adding one grant -> the dialog lists one grant. |
| [CAP-017] | The table of grant forms MUST list 87 forms. A grant used in a capability page MUST exist in the table. | [table](/pages/reference-capabilities/#table) | (install-error) run the check tool over the pages -> no grant string is missing from the table. |
| [CAP-018] | An import whose capability the manifest never requested MUST trap. An import whose grant the user declined or revoked MUST return not-permitted. | [traps](/pages/reference-errors-and-limits/#traps) | (trap) call an import never requested -> trap. (sync-return) call one the user declined -> err not-permitted. |

### See also

- [Requirements index](/pages/reference-requirements/)
- [Error codes and limits](/pages/reference-errors-and-limits/)
