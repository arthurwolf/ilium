---
layout: page
title: Plugin to plugin calls
permalink: /pages/api-plugin-to-plugin/
---

## Plugin to plugin calls

When one plugin calls another, the call acts with two sets of grants: the caller's grants decide whether the caller may ask for the action, and the callee's grants decide what the callee does on the caller's behalf. The host checks both, so a callee cannot be used to reach a resource that the caller could not reach itself.

### Grant intersection rule

**[SEC-716]** When plugin A calls plugin B, the host MUST authorise the action against the grants of A for the resource and against the grants of B for what B does on its behalf, and MUST NOT let B use its grants for a request whose origin plugin lacks the same grant; a violation returns `not-permitted` with detail `confused-deputy`.

Test: plugin A, which lacks a grant that plugin B holds, calls B to use that resource. Assert `not-permitted` with detail `confused-deputy`, and assert that B's grant was not used.
