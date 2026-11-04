---
layout: page
title: Legal requests
permalink: /pages/publish-legal-requests/
---

## Legal requests to the registry

A registry that stores uploads receives court orders, subpoenas, preservation requests and demands from government bodies, including bodies in other countries. This page says how the operator receives, checks, answers and records them. Copyright, trademark and patent claims are handled under [Rights-holder claims](/pages/publish-rights-claims/), and sanctions screening under [Governance](/pages/governance/). Neither is legal process under this page.

### You will need

- A legal contact address, written in the `legal_contact` field of `policy.json`. Legal process sent to any other address is not received under this page.
- The jurisdiction named in `policy.json`, which the operator uses to judge whether an order is valid ([PKG-086](/pages/reference-registry/#policy)).
- A private record for each request. The record is never published.

### What counts as legal process

Legal process is a written order or request from a court, a government body or a law enforcement agency. It asks the operator to disclose data, to preserve data or to remove content. A request from a private party with no order behind it is not legal process. The operator answers it through the abuse intake ([PUB-046](/pages/publish-registry-operator-guide/#incidents)) or refuses it.

### Receiving and verifying

**[PUB-073]** The registry operator MUST accept legal process only at the address that `policy.json` names in `legal_contact`, and MUST verify the authenticity of each order (the issuing body, the official channel and the case number) before it acts on the order.

### Disclosure

**[PUB-074]** The registry operator MUST disclose user or publisher data in response to legal process only when the process is valid under the law of the jurisdiction named in `policy.json`, MUST limit the disclosure to the scope of the process, and MUST record each request, its legal basis and its outcome in the operator's private record.

A preservation request is recorded in the same way. It keeps the named data until the operator decides on the request or the request expires. A preservation request alone never allows disclosure.

### Transparency report

**[PUB-075]** The registry operator MUST publish a transparency report at least once every 12 months that counts legal requests, takedowns by category and appeals, and MUST NOT name a requester or a publisher whose data was not disclosed.

### Edge cases

- **An order asks for data from a jurisdiction that the law of `policy.json` does not cover.** The operator refuses the order, records the refusal and the reason in its private record, and tells the issuing body the reason in writing.
- **An order arrives at the legal contact outside working hours.** The operator checks it when the next working day starts. The 12-month report still counts it.

### See also

- [Registry operator guide](/pages/publish-registry-operator-guide/): the operator's daily duties and the takedown procedure.
- [Governance](/pages/governance/): the moderation record and the sanctions rules.
- [Reference: registry policy](/pages/reference-registry/#policy): the content policy and the `policy.json` fields.
