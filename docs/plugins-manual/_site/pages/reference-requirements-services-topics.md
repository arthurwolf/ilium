# Requirements for services and topics

## Requirements for services and topics

Normative behaviour of services, topics and peer messages.

The rows follow the format and the observation channels of the [requirements index](/pages/reference-requirements/). The id prefix is `SVC`.

| Id | Requirement | Source | Observable test |
|---|---|---|---|
| [SVC-001] | A service call MUST need services.use:<name> and a provider MUST need services.provide:<name>. Without it MUST return not-permitted. | [edge cases and error codes](/pages/api-services/#edge-cases-and-error-codes) | (sync-return) call without services.use -> not-permitted. |
| [SVC-002] | A call with no installed and enabled provider MUST return not-found at once. | [errors and limits](/pages/services/#errors-and-limits) | (sync-return) call a service nobody provides -> not-found. |
| [SVC-003] | A call payload or a reply over 1 MiB, or one that fails its schema, MUST return invalid-argument. The provider MUST NOT be called for a bad payload. | [errors and limits](/pages/services/#errors-and-limits) | (sync-return) 1 MiB + 1 payload -> invalid-argument, provider log empty. |
| [SVC-004] | A method not in the provider schema MUST return unsupported. | [errors and limits](/pages/services/#errors-and-limits) | (sync-return) call method "x" -> unsupported. |
| [SVC-005] | A caller MUST be limited to 64 pending requests. The 65th call MUST return busy. | [edge cases and error codes](/pages/api-services/#edge-cases-and-error-codes) | (sync-return) 65 open calls -> busy. |
| [SVC-006] | A provider is compatible when the major version equals the required one and the minor is at least the required one. For major 0 the minor MUST be equal and the patch MUST be ignored. | [names versions and grants](/pages/services/#names-versions-and-grants) | (event-delivery) require 0.1.0, provide 1.0.0 -> consumer stays inactive. Provide 0.1.5 -> activates. |
| [SVC-007] | No reply within 30 seconds MUST complete the call as failed(timed-out). | [edge cases and error codes](/pages/api-services/#edge-cases-and-error-codes) | (completion) provider never replies -> failed(timed-out) at 30 s. |
| [SVC-008] | A provider that stops, restarts or is disabled before answering MUST make its pending callers complete as failed(cancelled). | [edge cases and error codes](/pages/api-services/#edge-cases-and-error-codes) | (completion) kill the provider mid-call -> failed(cancelled). |
| [SVC-009] | A reply that fails the result schema MUST complete the caller as failed(invalid-argument) and the provider MUST log an error. | [errors and limits](/pages/services/#errors-and-limits) | (completion) reply a number where a string is required -> failed(invalid-argument). |
| [SVC-010] | take-reply MUST return the JSON once. A second take MUST return none. | [edge cases and error codes](/pages/api-services/#edge-cases-and-error-codes) | (sync-return) take twice -> second is none. |
| [SVC-011] | A reply after 30 seconds MUST return not-found. | [edge cases and error codes](/pages/api-services/#edge-cases-and-error-codes) | (sync-return) reply at 31 s -> not-found. |
| [SVC-012] | Activation of a consumer MUST wait for a compatible provider. | [activation order](/pages/services/#activation-order) | (event-delivery) install the consumer first -> it activates after the provider. |
| [SVC-013] | Plugin-defined services are unstable in 0.1. A consumer MUST be updated together with its provider when the service API changes. | [names versions and grants](/pages/services/#names-versions-and-grants) | (event-delivery) bump a provider to a new service API version -> the old consumer goes inactive until updated. |
| [SVC-014] | A provider MUST have at most 64 calls in flight. The 65th MUST return busy at the call. | [errors and limits](/pages/services/#errors-and-limits) | (sync-return) 65 concurrent calls to one provider -> busy. |
| [SVC-020] | A plugin MUST publish only topics that start with its own plugin id and a slash. Another prefix MUST return not-permitted. | [edge cases and errors](/pages/api-messages-and-topics/#edge-cases-and-errors) | (sync-return) publish "other.x/y" -> not-permitted. |
| [SVC-021] | A topic not declared in contributes.topics.publishes MUST return invalid-argument. | [edge cases and errors](/pages/api-messages-and-topics/#edge-cases-and-errors) | (sync-return) publish an undeclared topic -> invalid-argument. |
| [SVC-022] | A topic name over 128 characters, or a payload that is not JSON or is over 64 KiB, MUST return invalid-argument. | [edge cases and errors](/pages/api-messages-and-topics/#edge-cases-and-errors) | (sync-return) 65 KiB payload -> invalid-argument. |
| [SVC-023] | More than 100 publishes per second per plugin MUST return rate-limited with retry-after-ms. | [edge cases and errors](/pages/api-messages-and-topics/#edge-cases-and-errors) | (sync-return) 101 publishes in a second -> rate-limited. |
| [SVC-024] | A subscription pattern MUST be an exact topic, a prefix with /*, or a bare *. A star in any other position MUST be invalid. | [topic names and patterns](/pages/api-messages-and-topics/#topic-names-and-patterns) | (install-error) subscribe "a*b" -> a manifest error naming the pattern. |
| [SVC-025] | A subscriber without the grant MUST receive no events and no error. | [edge cases and errors](/pages/api-messages-and-topics/#edge-cases-and-errors) | (event-delivery) subscribe without the grant -> nothing arrives. |
| [SVC-026] | A full queue for topic-published MUST drop the oldest event and count it. A full queue for message MUST NOT drop and MUST complete the intent as refused(busy) without restarting the plugin (see SVC-030). | [edge cases and errors](/pages/api-messages-and-topics/#edge-cases-and-errors) | (event-delivery) overflow each queue -> events-dropped for topics, refused(busy) for messages. |
| [SVC-027] | A message to a peer without messages.peer:<plugin-id> MUST complete as refused(not-permitted). To a plugin not installed or disabled MUST complete as refused(not-found). | [edge cases and errors](/pages/api-messages-and-topics/#edge-cases-and-errors) | (completion) message a disabled plugin -> refused(not-found). |
| [SVC-028] | A message channel MUST match [a-z0-9.-] and be at most 64 characters. A payload MUST be at most 1 MiB. Otherwise refused(invalid-argument). | [edge cases and errors](/pages/api-messages-and-topics/#edge-cases-and-errors) | (completion) channel "Bad Name" -> refused(invalid-argument). |
| [SVC-029] | A message to an installed plugin that is not active MUST activate it and deliver after activate. | [edge cases and errors](/pages/api-messages-and-topics/#edge-cases-and-errors) | (event-delivery) message a dormant plugin -> it activates, then gets the message. |
| [SVC-030] | When the receiver queue of a message is full and holds no drop-oldest event to evict, the message intent MUST complete refused(busy) and queue nothing, and the receiver MUST NOT be restarted. An accepted message MUST NOT be dropped. | [messages and topics](/pages/api-messages-and-topics/) | (completion) fill a receiver queue with never-dropped events, send a message -> refused(busy), receiver still active. |

### See also

- [Requirements index](/pages/reference-requirements/)
- [Error codes and limits](/pages/reference-errors-and-limits/)
