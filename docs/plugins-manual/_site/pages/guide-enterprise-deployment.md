# Enterprise deployment

## Enterprise deployment

This guide is for people who run Ilium for others: a team lead, a system administrator, a security engineer. It shows how to decide which plugins are allowed, how to ship a plugin to everyone, how to run on shared machines and in CI, and how to check that the policy does what you think. The rules themselves are in [Admin policy](/pages/reference-admin-policy/). This page is the how-to.

### You will need

- Administrator rights on the machine, to write a root-owned file.
- Ilium installed for each user. Each user keeps their own settings, stores and sessions.
- Linux, if any third-party (T2 or T3) plugin should run. See [Sandbox](/pages/reference-sandbox/).
- A text editor and the CLI: `ilium plugin policy show`, `ilium plugin audit`, `ilium plugin inspect`.

## Choose a stance

Pick one of three stances before you write any policy.

| Stance | Who it fits | Policy |
|---|---|---|
| Built-in only | Regulated sites | `allowed_tiers = ["t1"]` |
| Signed from known publishers | Most companies | `allowed_tiers = ["t1","t2"]`, `allow_unsigned = false`, `publishers.allow = [...]` |
| Open with guard rails | Developer teams | Defaults, plus `grants.max_risk`, `deny_combinations` and a `deny` list |

## Worked examples

### Lock to one publisher

Create `/etc/ilium/policy.toml`, owned by root, mode 0644:

```toml
version = 1
[packages]
allowed_tiers  = ["t1", "t2"]
allow_unsigned = false
allow_dev_mode = false
[publishers]
allow = ["3f2a91c07b6d1e44a1b20c9e55d3f7a8b6c102e94d7798af13c56e0bd2f48a19"]
trusted_keys = ["3f2a91c07b6d1e44a1b20c9e55d3f7a8b6c102e94d7798af13c56e0bd2f48a19"]
```

The key in `trusted_keys` needs no user prompt. Everything else fails at install with `policy.publisher-denied`.

### Block all high-risk grants except a list

```toml
version = 1
[grants]
max_risk = "normal"
[grants.preapproved]
"example.git-tools" = ["process.run:git"]
```

`max_risk = "normal"` refuses high-risk grants. To give one plugin a high-risk grant such as `process.run:git`, leave `max_risk` at `high` and list the grant in `preapproved`. `max_risk` has no exception: it locks off every high-risk grant, and a `preapproved` entry for such a grant is locked off too ([SEC-198]). Use `preapproved` for normal-risk grants you reviewed.

### An offline site

```toml
version = 1
[network]
offline = true
[packages]
allowed_sources = ["/srv/ilium-plugins/"]
```

No plugin egress, and installs only from `/srv/ilium-plugins/`. The default inference provider, the remote Kilo Gateway, stops working; set `[inference] allowed_providers = ["local"]` and configure a local provider.

### Preinstall a team package for everyone

1. Put the signed package in a root-owned folder, for example `/srv/ilium-plugins/example.team-rules-1.0.0.iliumpkg`.
2. Add the publisher key to `trusted_keys`.
3. Add the id to `force_enabled` and list its grants under `grants.preapproved`.
4. Each user runs `ilium plugin install /srv/ilium-plugins/example.team-rules-1.0.0.iliumpkg` once, or the login script does. Because the grants are preapproved, no dialog opens.

Ilium does not install `force_enabled` plugins for the user. The policy states what must be on; the administrator makes sure it is installed.

## Multi-user machines

**[SEC-194]** Each user has their own stores: grants, consents, trust, project trust, secrets, audit log and plugin install directory, all below the user's own directories and with owner-only modes. No file is shared between users and a user's decisions never apply to another user.

**[SEC-195]** The system package directory, if you use one, is read-only to users. Installs from it copy into the user's install directory.

**[SEC-196]** Sockets and runtime directories are per user (`$XDG_RUNTIME_DIR/ilium/`, mode 0700). Sessions of one user are not visible to another, and `sessions.cross` lists only the same user's sessions.

**[SEC-197]** Do not run Ilium as root. The server refuses T2 and T3 plugins when its effective uid is 0 ([SEC-056]).

## CI and headless use

Where no client is attached, no one can approve a dialog ([SEC-116]). Use `preapproved` for the exact plugins your pipeline needs, and set `allow_unsigned = false`. `ilium plugin install --yes` skips the review dialog for normal grants only. A high-risk grant needs `--grant <grant>` and a new key needs `--trust <key_id>`. `install.review-required` appears only when no client is attached and `--yes` is not given. Pipelines can verify packages without installing:

```text
ilium plugin inspect ./dist/example.team-rules-1.0.0.iliumpkg
```

## Verify the policy

1. Run `ilium plugin policy show`. Check that every file you wrote has `"used": true` and the owner is root. A file with `"used": false` is ignored ([SEC-182]).
2. Try a forbidden install. It should fail with the `policy.*` code you expect.
3. Run `ilium plugin audit --event policy-loaded` and `--event policy-violation`.
4. Look at the Plugins screen. A forced or locked item shows a lock mark.

## Edge cases

| Situation | Result |
|---|---|
| A user edits `/etc/ilium/policy.toml` as root, adds a typo | `policy.invalid`; third parties stop loading; the notice shows the line |
| A user copies the system policy to their own folder | The user file can only make it stricter |
| An old plugin is installed before the policy exists | Disabled at next start if it violates the policy |
| A laptop leaves the office network | `offline = true` still applies; use the proxy key for site networks |

### See also

- [Admin policy](/pages/reference-admin-policy/)
- [Audit log](/pages/reference-audit-log/)
- [Network egress](/pages/reference-network-egress/)
- [Plugin CLI](/pages/reference-plugin-cli/)

Contract: `ilium:plugin@0.1.0`.
