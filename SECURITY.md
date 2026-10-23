# Security policy

## Reporting a vulnerability

Report a security problem in Ilium, its plugin host, the Wasmtime configuration or the plugin sandbox helper to:

**security@ilium.example** (placeholder until the project publishes a real address; a PGP key is published next to it when one exists)

Do not open a public issue for a suspected vulnerability.

## What happens after you report

- A report about the plugin host, the Wasmtime configuration or the sandbox helper is rated at least High. A sandbox escape is rated Critical.
- The project acknowledges a host or sandbox report within 2 working days, and every other report within 3 working days.
- The project gives a first assessment within 10 working days and keeps the reporter informed at least every 14 days.
- For a confirmed host or sandbox flaw the embargo is 90 days from the report, or until a fix is released, whichever is first. A fix is released within 30 days for a Critical flaw and within 60 days for a High flaw.
- Advisories name the affected plugin tiers and say whether the flaw can be reached without a grant.

The full process, including plugin reports and registry handling, is in [the security response page](docs/plugins-manual/pages/reference-security-response.md).
