# Requirements for requests and outcomes

## Requirements for requests and outcomes

Normative behaviour of submissions, request-ids, outcomes, dedup keys and the input journal.

The rows follow the format and the observation channels of the [requirements index](/pages/reference-requirements/). The id prefix is `RQO`.

| Id | Requirement | Source | Observable test |
|---|---|---|---|
| [RQO-001] | Every submission MUST complete exactly once. | [the outcomes](/pages/requests-and-outcomes/#the-outcomes) | (completion) submit ten intents -> ten request-results, one each. |
| [RQO-002] | A call that returns a request-id MUST complete once with a request-result and the matching request-id. | [two kinds of identifier](/pages/requests-and-outcomes/#two-kinds-of-identifier) | (completion) start a slow call -> one request-result with that id. |
| [RQO-003] | Submissions of one batch MUST be applied in order. A later submission for a node an earlier one removed MUST complete as failed(not-found). | [the outcomes](/pages/requests-and-outcomes/#the-outcomes) | (completion) remove a node then rename it in one batch -> second is failed(not-found). |
| [RQO-004] | duplicate(original) MUST be returned while now_wall_ms - accepted_wall_ms < 600,000 for the dedup key, naming the ref of the first submission. Nothing MUST be applied. | [duplicate submissions and dedup keys](/pages/requests-and-outcomes/#duplicate-submissions-and-dedup-keys) | (completion) submit the same key twice -> the second is duplicate and no second effect; at 600,000 ms the key is free. |
| [RQO-005] | ok-epoch(u64) MUST be the outcome of a send-input applied to an agent pane and carry the new prompt epoch. | [the outcomes](/pages/requests-and-outcomes/#the-outcomes) | (completion) send-input to an idle agent pane -> ok-epoch with a number greater than the previous one. |
| [RQO-006] | cancelled MUST be used when the request stopped because the plugin stopped, the user cancelled or the host shut down. | [the outcomes](/pages/requests-and-outcomes/#the-outcomes) | (completion) cancel a slow call -> cancelled. |
| [RQO-007] | A sibling of a refused submission MUST complete as refused(cancelled) and not as cancelled. | [the outcomes](/pages/requests-and-outcomes/#the-outcomes) | (completion) refuse a batch of three -> all three are refused, siblings with code cancelled. |
| [RQO-008] | A completed result MUST be held for 30 seconds or until the first take. After that take MUST return none and the SDK wrapper MUST return not-found. | [limits](/pages/requests-and-outcomes/#limits) | (sync-return) take after 31 s -> not-found. A second take -> not-found. |
| [RQO-009] | When a malformed submission and a missing grant exist in one batch, the host MUST run the shape check for the whole list before the grant check and report the lowest-index malformed submission. | [the outcomes](/pages/requests-and-outcomes/#the-outcomes) | (completion) batch with both defects -> refused with invalid-argument and the index of the malformed one. |
| [RQO-010] | More than 256 submissions from one on-events call MUST trap. It MUST count as one failure, apply no submission and complete none. | [the outcomes](/pages/requests-and-outcomes/#the-outcomes) | (trap) return 257 submissions -> trap, none applied. |
| [RQO-011] | A refused batch MUST apply nothing, MUST reserve no dedup key and MUST log 'batch refused: submission <index>' with a 0-based index. | [the outcomes](/pages/requests-and-outcomes/#the-outcomes) | (log-line) refuse a batch -> the log names the index. The same keys are then usable again. |
| [RQO-020] | A dedup key MUST be 1 to 128 characters from A-Z a-z 0-9 . _ : / -. | [duplicate submissions and dedup keys](/pages/requests-and-outcomes/#duplicate-submissions-and-dedup-keys) | (completion) submit a 129 character key -> refused(invalid-argument). |
| [RQO-021] | A dedup key MUST be scoped to the pair (plugin id, client id) for forwarded submissions (HST-113) and reserved for 10 minutes from acceptance. | [duplicate submissions and dedup keys](/pages/requests-and-outcomes/#duplicate-submissions-and-dedup-keys) | (completion) two plugins use the same key -> no duplicate between them. |
| [RQO-022] | A retry of an accepted submission that failed MUST use a new dedup key. The old key stays reserved for 600,000 ms. | [the outcomes](/pages/requests-and-outcomes/#the-outcomes) | (completion) failed(io) then retry with the same key -> duplicate. |
| [RQO-030] | The kernel MUST journal the keys of send-input and agent-action before it writes to the terminal, so a duplicate is caught after a server restart. A key never stays unusable for more than 10 minutes and input is never replayed. | [uncertain input after a restart](/pages/requests-and-outcomes/#uncertain-input-after-a-restart) | (completion) restart the server and resubmit within 10 minutes -> duplicate. After 10 minutes -> accepted. |
| [RQO-031] | After a restart that interrupted a send-input the plugin MUST receive input-uncertain once and MUST NOT receive a replay. | [uncertain input after a restart](/pages/requests-and-outcomes/#uncertain-input-after-a-restart) | (event-delivery) kill the server mid-input and activate -> one input-uncertain. |
| [RQO-040] | A hook run MUST complete with the outcome of the chain: a veto becomes refused(conflict) with extension.vetoed in the message. | [hook runs](/pages/requests-and-outcomes/#hook-runs) | (completion) a veto handler refuses a submission -> refused(conflict) and the message starts "extension.vetoed: ". |

### See also

- [Requirements index](/pages/reference-requirements/)
- [Error codes and limits](/pages/reference-errors-and-limits/)
