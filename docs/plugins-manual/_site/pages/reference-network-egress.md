# Network egress reference

## Network egress reference

This page defines every way data leaves the machine because of a plugin, and the rules the host applies to each. **Egress** means data that goes out. It covers direct requests (`http`, streams, WebSocket) and host-initiated requests that the host makes on a plugin's behalf, such as inference, geocoding, notification channels and help sources.

A third-party plugin has no network of its own: its helper has an empty network namespace ([SEC-022]). All egress goes through the host, and the host applies this page. The grant syntax is in [Capability: http](/pages/capability-http/) and [API: http](/pages/api-http/). This page adds the policy those pages link to.

### You will need

- The `http` grant forms: `http:api.example.com`, `http:*.example.com`, `http:setting:<key>`.
- The [Threat model](/pages/reference-threat-model/), which names the network attacker (A12) and the web-page attacker (A10).
- The [Admin policy](/pages/reference-admin-policy/) for host lists.

## Scope

**[SEC-070]** The host MUST apply this page to every connection that originates from plugin activity: plugin `http` calls, HTTP streams, WebSocket, and each host-initiated feature in the inventory below. A feature that sends data off the machine and is missing from the inventory is a defect in the manual and a release blocker.

## Address classes

A name that a user granted may resolve to an address the user never meant to allow. This is the basis of DNS rebinding and of attacks on cloud metadata services.

**[SEC-072]** After DNS resolution the host MUST check every resolved address. These classes are refused with `not-permitted` and reason `private-address`, unless the grant is for that exact literal address or for `localhost` ([SEC-074]):

| Class | Range |
|---|---|
| Loopback | `127.0.0.0/8`, `::1` |
| Private | `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7` |
| Link-local | `169.254.0.0/16`, `fe80::/10` (includes the cloud metadata address `169.254.169.254`) |
| Carrier-grade NAT | `100.64.0.0/10` |
| Unspecified and broadcast | `0.0.0.0/8`, `::`, `255.255.255.255` |
| Multicast | `224.0.0.0/4`, `ff00::/8` |
| IPv4-mapped IPv6 of any class above | `::ffff:a.b.c.d` is checked as `a.b.c.d` |
| Documentation and benchmarking ranges | `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`, `198.18.0.0/15`, `2001:db8::/32` |

**[SEC-073]** The connection is pinned to the checked address. The host connects to the exact address it validated and never resolves the name a second time. If a name resolves to several addresses and any one is in a refused class, the request is refused entirely.

**[SEC-071]** IP literals. `http:203.0.113.5` and `http:[2001:db8::1]` are valid grant arguments for public addresses. A literal that falls in a refused class is valid only as an explicit grant of that literal, is high risk, and the review dialog shows it. Wildcards never match literals. `http:*` is refused with `manifest.grant-wildcard`.

**[SEC-074]** Loopback. `http:localhost` requires an explicit port (`http:localhost:8080`), is high risk, and can never reach the Ilium HTTP API port or the Ilium socket of any session: the host refuses those destinations with `not-permitted` reason `reserved`. The port check compares the destination with the listener ports of all Ilium sessions of the user, not only the current one.

`http:setting:<key>` takes its host from a setting. The same address checks apply to the value. A setting that holds a private address needs the user consent of SEC-074 (decision D23 in the [Design decisions](/pages/design-decisions/)), shown with the address class named.

## Redirects

**[SEC-075]** A request follows at most 5 redirects. Each hop is checked again against the plugin's grants and against the address classes. A redirect from `https` to `http` is refused with `not-permitted` reason `downgrade`.

**[SEC-076]** When a redirect changes the origin (scheme, host or port), the host removes the `Authorization`, `Proxy-Authorization` and `Cookie` request headers before it follows the hop.

## Transport security

**[SEC-077]** TLS. Only `https` is allowed for non-loopback hosts. TLS 1.2 is the minimum version; TLS 1.3 is preferred. Certificates are verified against the system trust roots plus, if the policy sets `[network] ca_bundle`, the bundle the administrator provides. A plugin cannot disable verification, pin a certificate or supply a trust root in version 0.1.0. A certificate failure returns `network` with reason `tls`.

**[SEC-078]** Proxy. The host reads `HTTPS_PROXY`, `HTTP_PROXY` and `NO_PROXY` once at server start. The policy key `[network] proxy` overrides them and MAY make a proxy mandatory. Plugins cannot set or read proxy settings. The address checks of [SEC-072] apply to the proxy-resolved destination when the proxy is a transparent one; with an explicit proxy, the check applies to the target name before the request is sent to the proxy.

## Request and response rules

**[SEC-079]** Forbidden request headers. A plugin cannot set: `Host`, `Content-Length`, `Connection`, `Transfer-Encoding`, `Upgrade`, `TE`, `Trailer`, any `Proxy-*` header, and `Cookie` (unless the plugin sets it itself; the host never adds cookies). A forbidden header returns `invalid-argument` for the request.

**[SEC-080]** The `User-Agent` is fixed by the host to `Ilium-plugin/<plugin-id>`. This is a privacy decision: it reveals that Ilium is in use and the plugin id, but not the Ilium version, the operating system or the user name. The plugin cannot change it.

**[SEC-081]** There is no cookie jar. The host stores no cookie between requests, and no jar is shared between plugins.

**[SEC-082]** Size caps. A request body is at most 8 MiB. A buffered response body is at most 32 MiB; a larger body ends with `quota-exceeded` and reason `response-size`. A stream delivers at most 256 MiB over its lifetime and 8 MiB per read; beyond that the stream ends with `quota-exceeded` and reason `stream-size`. The timeouts are those of the capability page.

**[SEC-083]** Egress budget. Each plugin has a budget of 16 MiB of request and response bytes per minute, summed over all its requests, counted on the wire after TLS framing is removed. Above the budget calls return `rate-limited` with `retry-after-ms`. The budget is per plugin side.

**[SEC-084]** Offline switch. Setting `network.offline` (or the policy key `[network] offline = true`) makes the host refuse all plugin egress with `not-permitted` and reason `offline`. Host-initiated features in the inventory below also stop, and report `offline` to their callers. Local destinations that the user granted explicitly (`http:localhost:<port>`) are also refused while offline, so the switch means no network traffic at all.

**[SEC-085]** The admin policy can add `allow_hosts` and `deny_hosts`. A deny rule always wins, and an allow list, if present, limits the grants to listed hosts. A request to a host that the policy refuses returns `not-permitted` with reason `policy`.

## Host-initiated egress inventory

Every feature in which the host sends data on a plugin's behalf.

| Feature | Destination | Data sent | Consent | Log |
|---|---|---|---|---|
| `inference` (default provider) | The provider configured in Settings > Inference; the default is the remote Kilo Gateway, no key | Prompt text and options the plugin passes | Review dialog names the provider and whether it is local or remote ([SEC-264]) | Privacy log |
| `inference.embed` | Same provider | Texts to embed | Same | Privacy log |
| `location.geocode` | Photon, with Nominatim as fallback | Place text | First-use consent (host geocoding, decision D14 in the [Design decisions](/pages/design-decisions/)) | Plugin log, audit |
| Notification channels (`ilium.core.notifications`) | A webhook URL the user configured | Title and body of the notification | The user configured the URL | Plugin log |
| Help sources (remote indexes) | The index host the user configured | The search query or the fetch path | The user configured the source | Plugin log |
| `open-external` | The user's desktop handler, not Ilium | A URL or file path | Origin consent (SEC-043) | Audit |
| `http`, streams, WebSocket | The granted hosts | Whatever the plugin sends | Review dialog | Plugin log, audit on first use of a host |
| `ilium.core.updates` (if present) | The project release host | Version query only, no identifiers | Opt-in setting, off by default | Plugin log |

**[SEC-086]** The host MUST apply [SEC-072] to [SEC-084] to each host-initiated destination that a user or plugin can configure (webhook URLs, help sources, provider endpoints). Webhook and provider endpoints in the private classes need an explicit setting confirmation like `http:setting:<key>`.

**[SEC-087]** The audit log records the first request to each new host by a plugin (`egress-host-first-use`) with the plugin id and host name only, not paths or bodies.

## HTTP API listener

The HTTP API lets programs on the machine drive Ilium. Plugins can add routes with `http-api.route`; see [Capability: http-api.route](/pages/capability-http-api-route/).

**[SEC-097]** The listener MUST bind only to loopback, MUST require the bearer token on every request, and MUST check the `Host` header against the listener's own address and the `Origin` header, if present, against the same-origin rule. A request with an unexpected `Host` (DNS rebinding) or a foreign `Origin` is refused with 403 before routing. Token comparison is constant-time ([SEC-376]).

**[SEC-098]** Plugins can never obtain the token. The token file `$XDG_RUNTIME_DIR/ilium/http-api.token` is in the file deny set ([SEC-060]), and the token is never passed in events, settings, environment variables or process arguments.

**[SEC-099]** The token is created with mode 0600 when the server starts and replaced at each start. Any process of the same user can read it, so the HTTP API is shell-equivalent for same-user processes. This is an accepted risk (non-goal [SEC-008]).

## Examples

A plugin granted `http:api.example.com` calls `GET https://api.example.com/v1`. The name resolves to `10.1.2.3`. The host refuses with `not-permitted`, reason `private-address`. The same plugin granted `http:10.1.2.3` as a literal gets through, and the review dialog showed `Connect to 10.1.2.3 (a private address)` as high risk.

A plugin sends `Authorization: Bearer abc` to `https://a.example.com`, which redirects to `https://b.example.org`. The host drops the header before the second hop. If the redirect target is `http://b.example.org`, the host refuses it with reason `downgrade`.

**Rust**

```rust
use ilium_plugin_sdk::host::http::{self, HttpRequest};
use ilium_plugin_sdk::prelude::*;

/// Fetches a status page. Returns None when the request fails, for example when egress is
/// refused with not-permitted (reason private-address, offline or policy).
async fn fetch_status() -> Option<Vec<u8>> {
    let request = HttpRequest {
        method: "GET".to_string(),
        url: "https://status.example.com/api/v1/summary".to_string(),
        headers: Vec::new(),
        body: None,
        timeout_millis: 5_000,
    };
    match http::fetch(request).await {
        Ok(response) => Some(response.body),
        Err(_refused) => None,
    }
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { fetch as httpFetch } from '@ilium/plugin-sdk/host/http';

async function fetchStatus(): Promise<Uint8Array | undefined> {
  try {
    const response = await httpFetch({
      method: 'GET',
      url: 'https://status.example.com/api/v1/summary',
      headers: [],
      body: undefined,
      timeoutMillis: 5000,
    });
    return response.body;
  } catch (error) {
    if (error instanceof PluginError && error.code === 'not-permitted') {
      return undefined; // reason is 'private-address', 'offline' or 'policy'
    }
    throw error;
  }
}
```

## Edge cases

| Situation | Result |
|---|---|
| The name resolves to both a public and a private address | Refused, [SEC-072] |
| DNS changes between two requests | Each request resolves and checks again; the connection pins to its own check |
| A granted host redirects to an ungranted host | `not-permitted`, reason `not-granted`, at the hop |
| IPv6 zone identifiers (`fe80::1%eth0`) | Refused as link-local |
| Offline switch turned on during a stream | The stream ends with `not-permitted` reason `offline` |
| Proxy forced by policy is unreachable | `network`; no direct fallback |

### See also

- [Capability: http](/pages/capability-http/)
- [API: http](/pages/api-http/)
- [Admin policy](/pages/reference-admin-policy/)
- [Privacy and security for users](/pages/user-plugin-security-and-privacy/)
- [Threat model](/pages/reference-threat-model/)

Contract: `ilium:plugin@0.1.0`.
