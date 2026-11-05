---
layout: page
title: Grant expiry in detail
permalink: /pages/reference-grant-expiry/
---

## Grant expiry in detail

This page specifies the full lifecycle of a grant expiry, which [SEC-717] introduced. It covers the timestamp forms the host accepts, how the host checks expiry when a plugin calls a capability, how clock changes and restarts are handled, what the expired state looks like and when it is cleaned up, and how a plugin gets a grant back after it expired. Plugin authors who depend on expiry behaviour and people who build tools around `ilium plugin grant` need this page. Audit records for expiry are described on the [Audit log](/pages/reference-audit-log/) page.

## Setting an expiry

A user sets an expiry with `ilium plugin grant <plugin-id> <grant> --until <timestamp>`. The host accepts only RFC 3339 timestamps in UTC, for example `2030-01-01T00:00:00Z`. A timestamp with a local offset is refused. The host also refuses a time in the past and a time more than 365 days ahead of the current time.

A refused timestamp exits with code 2 and prints the code `grant.expiry-invalid`. The grant store does not change when the host refuses a timestamp. For example, `ilium plugin grant example.sync fs.read --until 2020-01-01T00:00:00Z` fails with code 2 and leaves the stored grants as they were.

## Evaluating expiry at call time

The host does not rely on a timer to end a grant. At the start of every capability call, the host compares the expiry time of the grant with the current wall clock, corrected with a monotonic clock. If the expiry has passed, the call returns `not-permitted` with the reason `revoked`, and the plugin gets no data.

A call that started before the expiry and is still running when the expiry passes is treated the same way. Such a call ends with `not-permitted` and the reason `revoked`. The grant epoch increases by exactly one for this change. A long `fs.read` stream that is running when its grant expires ends with `not-permitted`, and `ilium plugin grants <plugin-id> --json` then shows the epoch one higher than before.

## Clock changes and restarts

A grant that has expired stays expired if the system clock is moved backwards. For example, if a grant expires and the clock is later set back by one day, the next call still returns `not-permitted` with the reason `revoked`. A user cannot revive an expired grant by changing the time on the machine.

The rules above apply after a restart as well. The host compares the stored expiry time with the clock when it starts, so a grant that expired while the host was not running is still refused. Nothing on this page adds a separate restart rule.

## Expired state and cleanup

An expired grant is shown as `expired`. It appears on the Plugins screen and in the output of `ilium plugin grants <plugin-id>`. With `--json`, the entry has `state` set to `"expired"`. The entry stays for 30 days after the expiry. After that, the host removes it from the grant store, and the grant no longer appears in any listing.

When a grant expires, the host re-activates the plugin with the reduced grant set. This is the same step the host takes when a user revokes a grant. The plugin's `activate` export runs once more, and the grant list it receives holds only the grants that remain. If a plugin has two grants and one expires, the plugin runs `activate` again and sees only the other grant. The lifecycle steps are described in [Lifecycle states](/pages/reference-plugin-lifecycle-states/).

## Re-granting after expiry

A plugin can only get a grant back through the user. Re-granting always opens the review dialog again, even if the same grant was approved before. Running `ilium plugin grant <plugin-id> <grant>` after an expiry opens the review dialog. The steps of the dialog are described in [Grant review and consent](/pages/reference-grant-review-and-consent/).

## Requirements

- **[PDY-001]** `ilium plugin grant --until <timestamp>` MUST accept only RFC 3339 UTC timestamps and MUST refuse a time in the past or more than 365 days ahead with exit code 2 and the code `grant.expiry-invalid`.
- **[PDY-002]** The host MUST compare the expiry against a monotonic-corrected wall clock at the start of every capability call, not only on a timer.
- **[PDY-003]** A call that started before the expiry and is still running when it passes MUST return `not-permitted` with reason `revoked`, and the grant epoch MUST bump once.
- **[PDY-004]** If the system clock moves backwards, a grant that has already expired MUST stay expired.
- **[PDY-005]** An expired grant MUST be shown as `expired` in the Plugins screen and in `ilium plugin grants` for 30 days and then removed from the store; re-granting MUST require the review dialog again.
- **[PDY-006]** On expiry the host MUST re-activate the plugin with the reduced grant set exactly as it does for a revoke.
