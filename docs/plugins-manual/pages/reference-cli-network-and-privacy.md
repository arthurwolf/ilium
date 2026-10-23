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

### Authenticating proxies on managed networks

This section is target behaviour. It adds no requirement id: the proxy choice is [PKG-374](/pages/reference-registry/), the HTTPS-only rule is [PKG-371](/pages/reference-registry/), and the header rule is [SEC-076](/pages/reference-network-egress/). The names marked "proposed" below are not yet in a requirements file.

**Supported schemes.** The registry and update client supports one proxy authentication scheme: HTTP Basic, sent as a `Proxy-Authorization: Basic` header on the `CONNECT` request that opens the tunnel to the registry. It is sent only after the proxy has answered `407` with a `Proxy-Authenticate: Basic` challenge, or when the proxy URL carries credentials and the first `CONNECT` is sent with them. The proxy URL has the form `http://user:password@proxy.example.com:3128` or `https://user:password@proxy.example.com:3128`; user and password are percent-decoded. These are explicitly unsupported, and the client never attempts them:

- NTLM and Negotiate (Kerberos): a `407` that offers only these schemes is a failure with the detail `unsupported-scheme`. The client does not use the logged-in user's ticket.
- Digest: not supported, same detail.
- A PAC file (proxy auto-configuration) or WPAD discovery: the client never fetches or runs one. A setting or a variable that holds a PAC URL is refused with `usage.invalid-value`. An administrator on a PAC network sets `[network] proxy` to the fixed proxy that the PAC file would choose.

A site that can only offer NTLM or Kerberos runs a local forwarding proxy that adds Basic or no authentication, and names that proxy in `[network] proxy`.

**Where a credential may be stored.**

| Place | Allowed | Rule |
|---|---|---|
| `[network] proxy` in the administrator policy | Address only | A policy proxy URL that contains `user:password@` makes the policy invalid: `policy.invalid`, exit code 7. The policy file is readable by every user and is never a secret store. |
| `[plugins.registry] proxy` in the user configuration | Address only | A URL with credentials in this file is refused with `usage.invalid-value`, and the message names the setting, never the password. |
| `HTTPS_PROXY` | Yes | The URL may carry credentials. The variable is read once at the start of each command (see [Proxies](#proxies)). |
| `ILIUM_PROXY_USER` and `ILIUM_PROXY_PASSWORD` (proposed, owner to confirm) | Yes | Supply credentials for a proxy URL without them, whichever source chose the proxy, including the policy. They are read once at the start of each command. If both the URL and the variables carry credentials, the URL wins. |
| The user's secret store (proposed, owner to confirm) | Yes | The operating-system secret store entry named `ilium-proxy`, read when neither of the sources above supplies credentials. The command never writes to it. |

An administrator who needs a mandatory authenticated proxy therefore writes the address in the policy and leaves the credentials to each user's environment or secret store.

**What is sent.** The credential goes only to the proxy named by the chosen source, only over the `CONNECT` request, and never to the registry. The `Proxy-Authorization` header is removed before any redirect that changes origin ([SEC-076](/pages/reference-network-egress/)). A credential is never printed in a record, a message, a `diagnostic` or a log, and the proxy address is printed without its `user:password@` part.

**When the proxy answers 407.**

| Situation | Result |
|---|---|
| `407` with no credential available | `registry.proxy`, reason `proxy-auth`, detail `credentials-missing`, exit code 8. The message names the proxy host and the places in the table above. |
| `407` after a credential was sent | `registry.proxy`, reason `proxy-auth`, detail `credentials-rejected`, exit code 8. |
| `407` that offers only an unsupported scheme | `registry.proxy`, reason `proxy-auth`, detail `unsupported-scheme`, exit code 8. |

A `407` is never retried, because a repeat with the same credential gives the same answer. The command makes no second attempt with another scheme, and it prints no part of the credential. `registry.proxy` and its exit code are those of the [failure code table](#connection-failure-codes).

**The policy proxy stays mandatory.** When the proxy comes from `[network] proxy`, an authentication failure does not change the rules of [Proxies](#proxies): the command never connects directly, never falls back to a lower source, never ignores the proxy because of `NO_PROXY`, and never falls back to `http:`. The command fails with `registry.proxy` and no connection to the registry is attempted. For plugins, the host reports the same condition as `network` with no direct fallback ([Network egress](/pages/reference-network-egress/)). With a proxy from a user setting or from `HTTPS_PROXY`, a failed authentication fails the same way and does not fall back to a direct connection either, because [Proxies](#proxies) forbids bypassing a named proxy.

**Offline mode and cached data.** `--offline` stops the command before the proxy check ([Order of checks](#order-of-checks)), so no credential is read. A valid cached index is used after a failed metadata request as the registry reference says, and then no `registry.proxy` code is printed for that request.

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
