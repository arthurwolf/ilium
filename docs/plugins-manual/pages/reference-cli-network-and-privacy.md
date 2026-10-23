---
layout: page
title: CLI network use and privacy
permalink: /pages/reference-cli-network-and-privacy/
---

## CLI network use and privacy

This page defines what the plugin command line may send over the network when it builds, publishes and updates, how to turn it off and what the offline mode guarantees. This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are in the range `DEV-894` to `DEV-895`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-894]** During `build`, `check`, `test` and `pack` the command line MUST make no network connection. During `publish`, `install` and `update` it MUST connect only to the registry named in the command or in the configuration, and MUST send only the package and the request fields that the registry protocol defines, with no analytics, usage data, project paths, user names or machine identifiers.

- **[DEV-895]** `--offline` and the environment variable `ILIUM_OFFLINE=1` MUST make every command that needs the network fail with exit code 1 and `net.offline` before any connection is attempted, and MUST make all other commands behave as without the flag. The command line MUST NOT include any telemetry that needs an opt-out; if it is ever added it MUST be off by default and enabled only by an explicit setting.

### Connections, timeouts and retries

`publish`, `install` and `update` use the registry client's transport for every registry and mirror request. The limits are those of [Registry reference](/pages/reference-registry/):

- Connect: 10 s.
- First byte of a metadata document: 30 s.
- One metadata document, total, including its retry waits: 120 s.
- One package download, total, including its retry waits: 600 s.

A connection error (a connect or first-byte timeout, or a connection reset) and a 5xx response are retried after waits of 1 s, 2 s and 4 s. A 4xx response is not retried, except a 429, which follows the `Retry-After` rule of the registry reference. A total limit that expires is not retried. The retry waits are counted in the total, so the wait for one metadata document never exceeds 120 s and the wait for one download never exceeds 600 s.

If a metadata request fails and a valid cached index exists, the command uses the cache and shows its age, as [PKG-415](/pages/reference-registry/) says, and no failure code is printed for that request.

### Proxies

The command line does not connect through a proxy unless one is configured. The first source that is set wins:

1. `[network] proxy` in the administrator policy ([Administrator policy](/pages/reference-admin-policy/)). A policy proxy is mandatory: the command never connects directly, and it ignores `NO_PROXY`.
2. `[plugins.registry] proxy` in the user configuration. The setting wins over the environment, as [PKG-374](/pages/reference-registry/) says.
3. `HTTPS_PROXY`, read once at the start of each command. Registry requests are HTTPS only ([PKG-371](/pages/reference-registry/)), so `HTTP_PROXY` does not apply to them.
4. No proxy: a direct connection.

`NO_PROXY` is read in steps 2 and 3. It lists exact host names and domain suffixes, and a request to a listed host connects directly. A proxy that is named by the policy or by a setting is never bypassed by a fallback to a direct connection. Proxy credentials in a proxy URL are never printed in any record or message.

### Certificate authorities

Registry and mirror requests verify TLS against two sets of roots, as [PKG-986](/pages/reference-registry/) says:

- The system trust roots.
- The bundle named by `[network] ca_bundle` in the administrator policy, if the policy sets one. The bundle adds roots; it does not replace the system roots.

The command line reads no other CA variable (such as `SSL_CERT_FILE`). No flag, environment variable or setting disables verification or adds a trust root, and a certificate failure never falls back to `http:`. TLS 1.2 is the minimum version ([SEC-077](/pages/reference-network-egress/)).

If the policy file exists but cannot be parsed, a network command refuses with `policy.invalid` before any connection. It then never falls back to the system roots or to a direct connection. A missing policy file is not an error.

### Order of checks

A network command checks, in this order: `--offline` or `ILIUM_OFFLINE=1` ([DEV-895](#rules)), then the policy file, then the proxy choice, then the connection and TLS. A check that fails stops the command before the next one runs.

### Connection failure codes

These codes are the failure codes of the rules above. Each prints a `diagnostic` record with a `reason` field (`connect-timeout`, `first-byte-timeout`, `total-timeout`, `connection`, `tls`, `proxy` or `proxy-auth`) and the request host and path. The exit codes follow [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes).

| Condition | Diagnostic code | Exit code | Retried |
|---|---|---|---|
| Connection error or timeout that persists after the retry waits, metadata request | `registry.unreachable` | 8 | Yes, 1 s, 2 s and 4 s |
| Connection error or timeout that persists after the retry waits, package download | `install.download-failed` | 8 | Yes, 1 s, 2 s and 4 s |
| 120 s total for a metadata document expires | `registry.unreachable` | 8 | No |
| 600 s total for a download expires | `install.download-failed` | 8 | No |
| Certificate or trust chain check fails (`network` with reason `tls` for plugins, [SEC-077](/pages/reference-network-egress/)) | `registry.tls` (proposed, owner to confirm) | 8 | No |
| Proxy cannot be reached, or answers `407` | `registry.proxy` (proposed, owner to confirm) | 8 | Connection errors yes; `407` no |
| Policy file cannot be parsed | `policy.invalid` | 7 | No |

`registry.tls` and `registry.proxy` are new codes, not yet in the [client error codes](/pages/reference-registry/). The exit codes 7 and 8 follow [PKG-424](/pages/reference-plugin-distribution-cli/#exit-codes).

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
