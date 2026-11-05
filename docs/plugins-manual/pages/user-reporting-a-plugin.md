---
layout: page
title: Reporting a malicious or abusive plugin
permalink: /pages/user-reporting-a-plugin/
---

## Reporting a malicious or abusive plugin

This page specifies how a user files a report about a plugin that is malicious or abusive, and what the report contains. It covers the choice of category, the exact payload, the consent prompt of `--submit`, the receipt and the decision lookup, the rate limits, and the redirection of vulnerability reports to the security contacts. It completes the operator intake defined by [PUB-046](/pages/publish-registry-operator-guide/#incidents) and the reporter privacy rule of [PUB-045](/pages/publish-registry-operator-guide/#incidents). Users who run into a plugin problem that is not abuse should read [Troubleshooting plugins](/pages/user-troubleshooting/) first.

## Choosing a report category

Every report carries one category. Use the category that matches what the plugin does. A report with the category `vulnerability` is not an abuse report. It describes a security flaw that a plugin author or the registry must fix, so the client does not file it as abuse. Choose `vulnerability` only when the problem is a flaw, such as a way to read data it should not read, and not when the plugin is simply harmful or unwanted.

Pass the category with `--category <name>`.

## What the report contains and what it never contains

A report carries exactly six fields. They are the plugin id, the plugin version, the package `sha256`, one rule id from the cached `policy.json`, a category, and a free text. The keys in the request body are `plugin_id`, `version`, `sha256`, `rule_id`, `category` and `text`, and no other key is sent.

The free text may be at most 4000 characters long. A longer text is refused by the client before anything is sent, with the error `report.text-too-long`. The report never contains the host name, the user name, file paths or the list of other installed plugins. The client does not add these fields, and the user does not need to remove them from the text.

## Submitting with --submit and the consent prompt

Run `ilium plugin report <id>` to see the contacts for the plugin. Without `--submit`, the command prints the contacts and sends nothing. No network request is made.

Run `ilium plugin report <id> --submit` to file the report. The command prints the exact JSON payload first and then asks for confirmation. Nothing is sent until you answer yes. If you answer `n`, the payload stays on your screen and still no request is made. Read the payload before you confirm, because it is exactly what the registry receives.

The rule is chosen with `--rule <rule-id>`. The rule id must be present in the cached `policy.json`. If it is not, the client refuses the report with `report.rule-unknown`, exits with code 2 and sends no request. For example, `ilium plugin report example.sync --submit --rule no-such-rule` ends with exit code 2 and no request.

## Receipt, decision and transparency log index

When the intake accepts a report, it answers with a report id. The client stores that id locally, so you can ask for the decision later without keeping a note of it.

Run `ilium plugin report --status <report-id>` to fetch the recorded decision. The command prints one JSON line with the fields `report_id` and `decision`. The value of `decision` is `pending` until the operator records a decision, and then it is the recorded value.

A decided report also appears in the transparency log. The log entry holds the report id and the decision. It never holds a token, an email address or any client identifier, so the public log cannot be used to link a report to the person who filed it.

## Rate limits and spam handling

The registry accepts at most 10 reports per client token per day. Each client token has its own count. The eleventh report in one day receives HTTP 429 with a `Retry-After` header. The client prints `report.rate-limited` together with the number of seconds to wait.

The limit exists to stop spam. Do not script repeated reports for the same plugin. One accurate report with a clear rule id is the most useful report the registry can receive.

## Reporting a security flaw instead of abuse

A flaw in a plugin is reported with the category `vulnerability`. Run `ilium plugin report example.sync --category vulnerability --submit` to file it. The report is redirected to the plugin's `security_contact` and to the registry security contact. The command prints both security contacts, and no abuse report request is sent.

The flaw is not filed as abuse, so the abuse intake never sees it. Use the security contacts that the command prints.

## Edge cases

A rule id that is missing from the cached `policy.json` is refused with `report.rule-unknown` and exit code 2. The refusal happens on the client, so the rule id must match an entry in the cache exactly.

A text longer than 4000 characters is refused with `report.text-too-long` before any request is sent. Shorten the text and submit again.

If the registry answers with HTTP 429, the client prints `report.rate-limited` with the wait time. Submit again only after that wait.

If `--status` prints `pending`, the operator has not recorded a decision yet. Run the same `--status` command again later to see whether a decision has been recorded.

## Requirements

- **[PDW-008]** `ilium plugin report <id> --submit` MUST print the exact JSON payload and require explicit confirmation before any request is sent; without `--submit` the command MUST print the contacts and send nothing.
- **[PDW-009]** The payload MUST contain only the plugin id, version, package `sha256`, one `policy.json` rule id, a category and a free text of at most 4000 characters, and MUST NOT contain the host name, user name, file paths or the list of other installed plugins.
- **[PDW-010]** The client MUST refuse a rule id not present in the cached `policy.json` with `report.rule-unknown` and exit code 2.
- **[PDW-011]** The intake MUST answer with a report id, the client MUST store it locally, and `ilium plugin report --status <report-id>` MUST fetch and print the recorded decision or `pending`.
- **[PDW-012]** The registry MUST accept at most 10 reports per client token per day and MUST answer excess reports with HTTP 429 and a `Retry-After` header, which the client MUST print as `report.rate-limited` with the wait time.
- **[PDW-013]** A report with category `vulnerability` MUST be redirected to the plugin's `security_contact` and the registry security contact, and MUST NOT be filed as an abuse report.
- **[PDW-014]** The reporter identity MUST NOT appear in any public document, including the transparency log entry for the decision ([PUB-045]).
