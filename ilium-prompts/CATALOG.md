# Extracted prompt catalog

This mapping covers application source call sites captured before extraction. Line numbers refer to the frozen original snapshot; function/constant labels identify shared layouts.

| Original source | Template | Embedded constant |
|---|---|---|
| `ilium-client/src/project_naming.rs:20 PROJECT_NAME_TEMPLATE` | [naming/project-name](templates/naming/project-name.hbs) | `PROJECT_NAME` |
| `ilium-client/src/session_naming.rs:22 LABEL_INSTRUCTIONS` | [naming/label-instructions](templates/naming/label-instructions.hbs) | `LABEL_INSTRUCTIONS` |
| `ilium-client/src/session_naming.rs:33 SUMMARY_INSTRUCTIONS` | [naming/session-summary](templates/naming/session-summary.hbs) | `SESSION_SUMMARY` |
| `ilium-client/src/session_naming.rs:35 SESSION_TITLE_TEMPLATE` | [naming/session-title](templates/naming/session-title.hbs) | `SESSION_TITLE` |
| `ilium-client/src/terminal_naming.rs:45 SUMMARY_INSTRUCTIONS` | [naming/terminal-summary](templates/naming/terminal-summary.hbs) | `TERMINAL_SUMMARY` |
| `ilium-client/src/terminal_naming.rs:49 TERMINAL_TITLE_TEMPLATE` | [naming/terminal-title](templates/naming/terminal-title.hbs) | `TERMINAL_TITLE` |
| `ilium-client/src/restructure.rs:70 RESTRUCTURE_TEMPLATE` | [naming/restructure](templates/naming/restructure.hbs) | `RESTRUCTURE` |
| `ilium-client/src/inference_test.rs:9 TEST_PROMPT` | [naming/inference-test](templates/naming/inference-test.hbs) | `INFERENCE_TEST` |
| `ilium-client/src/agent_feature_setup.rs:22 CHATROOM_INSTRUCTION` | [agent/chatroom-instructions](templates/agent/chatroom-instructions.hbs) | `CHATROOM_INSTRUCTIONS` |
| `ilium-client/src/agent_feature_setup.rs:24 PROGRESS_INSTRUCTION` | [agent/progress-instructions](templates/agent/progress-instructions.hbs) | `PROGRESS_INSTRUCTIONS` |
| `ilium-client/src/chatroom.rs:19 COORDINATION_POSTING_GUIDANCE` | [agent/coordination-guidance](templates/agent/coordination-guidance.hbs) | `COORDINATION_GUIDANCE` |
| `ilium-client/src/app.rs:9785 ASK_FOR_UPDATE_PROMPT` | [agent/ask-for-update](templates/agent/ask-for-update.hbs) | `ASK_FOR_UPDATE` |
| `ilium-session-convert/src/claude_writer.rs:26 MISSING_RESULT_TEXT` | [conversion/missing-result](templates/conversion/missing-result.hbs) | `MISSING_RESULT` |
| `ilium-session-convert/src/claude_writer.rs:27 EMPTY_RESULT_TEXT` | [conversion/empty-result](templates/conversion/empty-result.hbs) | `EMPTY_RESULT` |
| `ilium-session-convert/src/claude_writer.rs:28 SYNTHETIC_FIRST_PROMPT` | [conversion/synthetic-first-prompt](templates/conversion/synthetic-first-prompt.hbs) | `SYNTHETIC_FIRST_PROMPT` |
| `ilium-client/src/session_naming.rs:279` | [naming/session-label-example](templates/naming/session-label-example.hbs) | `SESSION_LABEL_EXAMPLE` |
| `ilium-client/src/session_naming.rs:280` | [naming/session-summary-example](templates/naming/session-summary-example.hbs) | `SESSION_SUMMARY_EXAMPLE` |
| `ilium-client/src/session_naming.rs:300` | [naming/unavailable-process](templates/naming/unavailable-process.hbs) | `UNAVAILABLE_PROCESS` |
| `ilium-client/src/session_naming.rs:333` | [naming/absent-context](templates/naming/absent-context.hbs) | `ABSENT_CONTEXT` |
| `ilium-client/src/terminal_naming.rs:108` | [naming/terminal-label-example](templates/naming/terminal-label-example.hbs) | `TERMINAL_LABEL_EXAMPLE` |
| `ilium-client/src/terminal_naming.rs:110` | [naming/terminal-summary-example](templates/naming/terminal-summary-example.hbs) | `TERMINAL_SUMMARY_EXAMPLE` |
| `ilium-client/src/restructure.rs:490` | [naming/unavailable-transcript](templates/naming/unavailable-transcript.hbs) | `UNAVAILABLE_TRANSCRIPT` |
| `ilium-client/src/restructure.rs:589` | [naming/restructure-summary](templates/naming/restructure-summary.hbs) | `RESTRUCTURE_SUMMARY` |
| `ilium-client/src/restructure.rs:592` | [naming/restructure-label-example](templates/naming/restructure-label-example.hbs) | `RESTRUCTURE_LABEL_EXAMPLE` |
| `ilium-client/src/restructure.rs:593` | [naming/restructure-summary-example](templates/naming/restructure-summary-example.hbs) | `RESTRUCTURE_SUMMARY_EXAMPLE` |
| `ilium-client/src/restructure.rs:693` | [naming/content-omission-marker](templates/naming/content-omission-marker.hbs) | `CONTENT_OMISSION_MARKER` |
| `ilium-client/src/restructure.rs:2316` | [naming/unavailable-transcript](templates/naming/unavailable-transcript.hbs) | `UNAVAILABLE_TRANSCRIPT` |
| `ilium-inference/src/lib.rs:299; restructure.rs:176` | [naming/json-only](templates/naming/json-only.hbs) | `JSON_ONLY` |
| `ilium-client/src/smart_copy.rs:1403` | [naming/smart-copy-system](templates/naming/smart-copy-system.hbs) | `SMART_COPY_SYSTEM` |
| `ilium-client/src/naming.rs:129` | [naming/naming/v0-v1-characters-omitted](templates/naming/naming/v0-v1-characters-omitted.hbs) | `NAMING_NAMING_V0_V1_CHARACTERS_OMITTED` |
| `ilium-client/src/naming.rs:278` | [naming/naming/v0](templates/naming/naming/v0.hbs) | `NAMING_NAMING_V0` |
| `ilium-client/src/naming.rs:341` | [naming/naming/context-label-response-missing-string-field-field](templates/naming/naming/context-label-response-missing-string-field-field.hbs) | `NAMING_NAMING_CONTEXT_LABEL_RESPONSE_MISSING_STRING_FIELD_FIELD` |
| `ilium-client/src/naming.rs:230` | [naming/naming/context-label-response-field-field-must-be-one-compact-utf-8-icon-or-emoticon](templates/naming/naming/context-label-response-field-field-must-be-one-compact-utf-8-icon-or-emoticon.hbs) | `NAMING_NAMING_CONTEXT_LABEL_RESPONSE_FIELD_FIELD_MUST_BE_ONE_COMPACT_UTF_8_ICON_OR_EMOTICON` |
| `ilium-client/src/naming.rs:307` | [naming/naming/context-label-response-did-not-contain-one-complete-json-object](templates/naming/naming/context-label-response-did-not-contain-one-complete-json-object.hbs) | `NAMING_NAMING_CONTEXT_LABEL_RESPONSE_DID_NOT_CONTAIN_ONE_COMPLETE_JSON_OBJECT` |
| `ilium-client/src/naming.rs:310` | [naming/naming/context-label-response-contained-multiple-json-objects](templates/naming/naming/context-label-response-contained-multiple-json-objects.hbs) | `NAMING_NAMING_CONTEXT_LABEL_RESPONSE_CONTAINED_MULTIPLE_JSON_OBJECTS` |
| `ilium-client/src/naming.rs:341` | [naming/naming/context-label-response-missing-string-field-field](templates/naming/naming/context-label-response-missing-string-field-field.hbs) | `NAMING_NAMING_CONTEXT_LABEL_RESPONSE_MISSING_STRING_FIELD_FIELD` |
| `ilium-client/src/naming.rs:345` | [naming/naming/context-label-response-field-field-must-contain-min-words-to-max-words-short-non-empt](templates/naming/naming/context-label-response-field-field-must-contain-min-words-to-max-words-short-non-empt.hbs) | `NAMING_NAMING_CONTEXT_LABEL_RESPONSE_FIELD_FIELD_MUST_CONTAIN_MIN_WORDS_TO_MAX_WORDS_SHORT_NON_EMPT` |
| `ilium-client/src/session_naming.rs:151` | [naming/session_naming/no-project-verified-transcript-found-for-session](templates/naming/session_naming/no-project-verified-transcript-found-for-session.hbs) | `NAMING_SESSION_NAMING_NO_PROJECT_VERIFIED_TRANSCRIPT_FOUND_FOR_SESSION` |
| `ilium-client/src/session_naming.rs:164` | [naming/session_naming/no-user-transcript-entries-available-to-infer-a-session-title-from](templates/naming/session_naming/no-user-transcript-entries-available-to-infer-a-session-title-from.hbs) | `NAMING_SESSION_NAMING_NO_USER_TRANSCRIPT_ENTRIES_AVAILABLE_TO_INFER_A_SESSION_TITLE_FROM` |
| `ilium-client/src/session_naming.rs:210` | [naming/session_naming/session-title-only-names-the-agent-provider](templates/naming/session_naming/session-title-only-names-the-agent-provider.hbs) | `NAMING_SESSION_NAMING_SESSION_TITLE_ONLY_NAMES_THE_AGENT_PROVIDER` |
| `ilium-client/src/session_naming.rs:338` | [naming/session_naming/claude-code](templates/naming/session_naming/claude-code.hbs) | `NAMING_SESSION_NAMING_CLAUDE_CODE` |
| `ilium-client/src/session_naming.rs:626` | [naming/session_naming/user-specified](templates/naming/session_naming/user-specified.hbs) | `NAMING_SESSION_NAMING_USER_SPECIFIED` |
| `ilium-client/src/session_naming.rs:524` | [naming/session_naming/waiting-on-background-tasks](templates/naming/session_naming/waiting-on-background-tasks.hbs) | `NAMING_SESSION_NAMING_WAITING_ON_BACKGROUND_TASKS` |
| `ilium-client/src/session_naming.rs:525` | [naming/session_naming/a-background-task-is-still-finishing-up](templates/naming/session_naming/a-background-task-is-still-finishing-up.hbs) | `NAMING_SESSION_NAMING_A_BACKGROUND_TASK_IS_STILL_FINISHING_UP` |
| `ilium-client/src/session_naming.rs:355` | [naming/session_naming/waiting-for-user-approval](templates/naming/session_naming/waiting-for-user-approval.hbs) | `NAMING_SESSION_NAMING_WAITING_FOR_USER_APPROVAL` |
| `ilium-client/src/terminal_naming.rs:98` | [naming/terminal_naming/no-screen-content-available-to-infer-a-terminal-title-from](templates/naming/terminal_naming/no-screen-content-available-to-infer-a-terminal-title-from.hbs) | `NAMING_TERMINAL_NAMING_NO_SCREEN_CONTENT_AVAILABLE_TO_INFER_A_TERMINAL_TITLE_FROM` |
| `ilium-client/src/project_naming.rs:305` | [naming/project_naming/not-present](templates/naming/project_naming/not-present.hbs) | `NAMING_PROJECT_NAMING_NOT_PRESENT` |
| `ilium-client/src/restructure.rs:415` | [naming/restructure/project-id-v0-title-v1-source](templates/naming/restructure/project-id-v0-title-v1-source.hbs) | `NAMING_RESTRUCTURE_PROJECT_ID_V0_TITLE_V1_SOURCE` |
| `ilium-client/src/restructure.rs:451` | [naming/restructure/v0-v1-id-v2-title-v3-icon-v4-source-v5-name-fixed](templates/naming/restructure/v0-v1-id-v2-title-v3-icon-v4-source-v5-name-fixed.hbs) | `NAMING_RESTRUCTURE_V0_V1_ID_V2_TITLE_V3_ICON_V4_SOURCE_V5_NAME_FIXED` |
| `ilium-client/src/restructure.rs:511` | [naming/restructure/v0-agent](templates/naming/restructure/v0-agent.hbs) | `NAMING_RESTRUCTURE_V0_AGENT` |
| `ilium-client/src/restructure.rs:543` | [naming/clipped-lines](templates/naming/clipped-lines.hbs) | `CLIPPED_LINES` |
| `ilium-client/src/restructure.rs:413` | [naming/restructure/project-project-id-no-longer-exists](templates/naming/restructure/project-project-id-no-longer-exists.hbs) | `NAMING_RESTRUCTURE_PROJECT_PROJECT_ID_NO_LONGER_EXISTS` |
| `ilium-client/src/restructure.rs:433` | [naming/restructure/tree-child-child-id-no-longer-exists](templates/naming/restructure/tree-child-child-id-no-longer-exists.hbs) | `NAMING_RESTRUCTURE_TREE_CHILD_CHILD_ID_NO_LONGER_EXISTS` |
| `ilium-client/src/restructure.rs:382` | [naming/restructure/split-view-child-id-has-no-orientation](templates/naming/restructure/split-view-child-id-has-no-orientation.hbs) | `NAMING_RESTRUCTURE_SPLIT_VIEW_CHILD_ID_HAS_NO_ORIENTATION` |
| `ilium-client/src/restructure.rs:388` | [naming/restructure/split-view-child-id-contains-a-non-pane-child](templates/naming/restructure/split-view-child-id-contains-a-non-pane-child.hbs) | `NAMING_RESTRUCTURE_SPLIT_VIEW_CHILD_ID_CONTAINS_A_NON_PANE_CHILD` |
| `ilium-client/src/restructure.rs:413` | [naming/restructure/project-project-id-no-longer-exists](templates/naming/restructure/project-project-id-no-longer-exists.hbs) | `NAMING_RESTRUCTURE_PROJECT_PROJECT_ID_NO_LONGER_EXISTS` |
| `ilium-client/src/restructure.rs:433` | [naming/restructure/tree-child-child-id-no-longer-exists](templates/naming/restructure/tree-child-child-id-no-longer-exists.hbs) | `NAMING_RESTRUCTURE_TREE_CHILD_CHILD_ID_NO_LONGER_EXISTS` |
| `ilium-client/src/restructure.rs:1517` | [naming/restructure/plain-shell](templates/naming/restructure/plain-shell.hbs) | `NAMING_RESTRUCTURE_PLAIN_SHELL` |
| `ilium-client/src/restructure.rs:524` | [naming/session_naming/waiting-on-background-tasks](templates/naming/session_naming/waiting-on-background-tasks.hbs) | `NAMING_SESSION_NAMING_WAITING_ON_BACKGROUND_TASKS` |
| `ilium-client/src/restructure.rs:525` | [naming/session_naming/a-background-task-is-still-finishing-up](templates/naming/session_naming/a-background-task-is-still-finishing-up.hbs) | `NAMING_SESSION_NAMING_A_BACKGROUND_TASK_IS_STILL_FINISHING_UP` |
| `ilium-client/src/restructure.rs:526` | [naming/restructure/waiting-for-your-approval](templates/naming/restructure/waiting-for-your-approval.hbs) | `NAMING_RESTRUCTURE_WAITING_FOR_YOUR_APPROVAL` |
| `ilium-client/src/restructure.rs:746` | [naming/restructure/restructure-prompt-exceeded-the-maximum-restructure-prompt-characters-character-safet](templates/naming/restructure/restructure-prompt-exceeded-the-maximum-restructure-prompt-characters-character-safet.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_PROMPT_EXCEEDED_THE_MAXIMUM_RESTRUCTURE_PROMPT_CHARACTERS_CHARACTER_SAFET` |
| `ilium-client/src/restructure.rs:827` | [naming/restructure/no-prior-structure-available](templates/naming/restructure/no-prior-structure-available.hbs) | `NAMING_RESTRUCTURE_NO_PRIOR_STRUCTURE_AVAILABLE` |
| `ilium-client/src/restructure.rs:854` | [naming/restructure/no-panes-or-folders-to-restructure](templates/naming/restructure/no-panes-or-folders-to-restructure.hbs) | `NAMING_RESTRUCTURE_NO_PANES_OR_FOLDERS_TO_RESTRUCTURE` |
| `ilium-client/src/restructure.rs:946` | [naming/restructure/restructure-response-had-the-wrong-json-shape-error](templates/naming/restructure/restructure-response-had-the-wrong-json-shape-error.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_HAD_THE_WRONG_JSON_SHAPE_ERROR` |
| `ilium-client/src/restructure.rs:954` | [naming/restructure/restructure-response-referenced-id-id-more-than-once](templates/naming/restructure/restructure-response-referenced-id-id-more-than-once.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_ID_ID_MORE_THAN_ONCE` |
| `ilium-client/src/restructure.rs:964` | [naming/restructure/restructure-response-referenced-the-wrong-leaf-set-missing-missing-unexpected-unexpec](templates/naming/restructure/restructure-response-referenced-the-wrong-leaf-set-missing-missing-unexpected-unexpec.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_THE_WRONG_LEAF_SET_MISSING_MISSING_UNEXPECTED_UNEXPEC` |
| `ilium-client/src/restructure.rs:985` | [naming/restructure/protected-split-view-context-duplicated-id](templates/naming/restructure/protected-split-view-context-duplicated-id.hbs) | `NAMING_RESTRUCTURE_PROTECTED_SPLIT_VIEW_CONTEXT_DUPLICATED_ID` |
| `ilium-client/src/restructure.rs:1012` | [naming/restructure/restructure-response-changed-the-protected-split-view-set-missing-missing-unexpected](templates/naming/restructure/restructure-response-changed-the-protected-split-view-set-missing-missing-unexpected.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CHANGED_THE_PROTECTED_SPLIT_VIEW_SET_MISSING_MISSING_UNEXPECTED` |
| `ilium-client/src/restructure.rs:1517` | [naming/restructure/plain-shell](templates/naming/restructure/plain-shell.hbs) | `NAMING_RESTRUCTURE_PLAIN_SHELL` |
| `ilium-client/src/restructure.rs:1068` | [naming/restructure/restructure-response-node-path-claimed-id-was-a-pane-but-the-existing-item-is-a-folde](templates/naming/restructure/restructure-response-node-path-claimed-id-was-a-pane-but-the-existing-item-is-a-folde.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_CLAIMED_ID_WAS_A_PANE_BUT_THE_EXISTING_ITEM_IS_A_FOLDE` |
| `ilium-client/src/restructure.rs:1075` | [naming/restructure/restructure-response-node-path-placed-folder-id-inside-a-split-view](templates/naming/restructure/restructure-response-node-path-placed-folder-id-inside-a-split-view.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_FOLDER_ID_INSIDE_A_SPLIT_VIEW` |
| `ilium-client/src/restructure.rs:1080` | [naming/restructure/restructure-response-node-path-claimed-id-was-a-folder-but-the-existing-item-is-a-pan](templates/naming/restructure/restructure-response-node-path-claimed-id-was-a-folder-but-the-existing-item-is-a-pan.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_CLAIMED_ID_WAS_A_FOLDER_BUT_THE_EXISTING_ITEM_IS_A_PAN` |
| `ilium-client/src/restructure.rs:1087` | [naming/restructure/restructure-response-node-path-placed-a-group-inside-a-split-view](templates/naming/restructure/restructure-response-node-path-placed-a-group-inside-a-split-view.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_A_GROUP_INSIDE_A_SPLIT_VIEW` |
| `ilium-client/src/restructure.rs:1102` | [naming/restructure/restructure-response-node-path-placed-an-existing-group-inside-a-split-view](templates/naming/restructure/restructure-response-node-path-placed-an-existing-group-inside-a-split-view.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_AN_EXISTING_GROUP_INSIDE_A_SPLIT_VIEW` |
| `ilium-client/src/restructure.rs:1117` | [naming/restructure/restructure-response-node-path-nested-a-split-view-inside-another-split-view](templates/naming/restructure/restructure-response-node-path-nested-a-split-view-inside-another-split-view.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_NESTED_A_SPLIT_VIEW_INSIDE_ANOTHER_SPLIT_VIEW` |
| `ilium-client/src/restructure.rs:1122` | [naming/restructure/restructure-response-node-path-invented-unknown-split-view-id](templates/naming/restructure/restructure-response-node-path-invented-unknown-split-view-id.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_INVENTED_UNKNOWN_SPLIT_VIEW_ID` |
| `ilium-client/src/restructure.rs:1127` | [naming/restructure/restructure-response-referenced-protected-split-view-id-more-than-once](templates/naming/restructure/restructure-response-referenced-protected-split-view-id-more-than-once.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_PROTECTED_SPLIT_VIEW_ID_MORE_THAN_ONCE` |
| `ilium-client/src/restructure.rs:1138` | [naming/restructure/restructure-response-node-path-placed-a-non-pane-inside-protected-split-view-id](templates/naming/restructure/restructure-response-node-path-placed-a-non-pane-inside-protected-split-view-id.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_A_NON_PANE_INSIDE_PROTECTED_SPLIT_VIEW_ID` |
| `ilium-client/src/restructure.rs:1144` | [naming/restructure/restructure-response-changed-protected-split-view-id-pane-order-or-membership-expecte](templates/naming/restructure/restructure-response-changed-protected-split-view-id-pane-order-or-membership-expecte.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CHANGED_PROTECTED_SPLIT_VIEW_ID_PANE_ORDER_OR_MEMBERSHIP_EXPECTE` |
| `ilium-client/src/restructure.rs:1310` | [naming/restructure/restructure-label-title-must-contain-1-to-7-words-and-at-most-64-characters](templates/naming/restructure/restructure-label-title-must-contain-1-to-7-words-and-at-most-64-characters.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_LABEL_TITLE_MUST_CONTAIN_1_TO_7_WORDS_AND_AT_MOST_64_CHARACTERS` |
| `ilium-client/src/restructure.rs:1317` | [naming/restructure/restructure-short-label-must-contain-1-to-3-words-and-at-most-64-characters](templates/naming/restructure/restructure-short-label-must-contain-1-to-3-words-and-at-most-64-characters.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_SHORT_LABEL_MUST_CONTAIN_1_TO_3_WORDS_AND_AT_MOST_64_CHARACTERS` |
| `ilium-client/src/restructure.rs:1328` | [naming/restructure/restructure-response-contained-an-empty-title](templates/naming/restructure/restructure-response-contained-an-empty-title.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_AN_EMPTY_TITLE` |
| `ilium-client/src/restructure.rs:1331` | [naming/restructure/restructure-response-contained-a-control-character-in-a-title](templates/naming/restructure/restructure-response-contained-a-control-character-in-a-title.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_A_CONTROL_CHARACTER_IN_A_TITLE` |
| `ilium-client/src/restructure.rs:1348` | [naming/restructure/restructure-response-contained-a-control-character-in-a-short-title](templates/naming/restructure/restructure-response-contained-a-control-character-in-a-short-title.hbs) | `NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_A_CONTROL_CHARACTER_IN_A_SHORT_TITLE` |
| `ilium-client/src/transcript_context.rs:113` | [naming/transcript_context/v0-earlier-v1-entries-omitted](templates/naming/transcript_context/v0-earlier-v1-entries-omitted.hbs) | `NAMING_TRANSCRIPT_CONTEXT_V0_EARLIER_V1_ENTRIES_OMITTED` |
| `ilium-client/src/transcript_context.rs:375` | [naming/transcript_context/agents-md-instructions-for](templates/naming/transcript_context/agents-md-instructions-for.hbs) | `NAMING_TRANSCRIPT_CONTEXT_AGENTS_MD_INSTRUCTIONS_FOR` |
| `ilium-client/src/transcript_context.rs:377` | [naming/transcript_context/codex-internal-context](templates/naming/transcript_context/codex-internal-context.hbs) | `NAMING_TRANSCRIPT_CONTEXT_CODEX_INTERNAL_CONTEXT` |
| `ilium-client/src/transcript_context.rs:379` | [naming/transcript_context/ilium-progress-monitor](templates/naming/transcript_context/ilium-progress-monitor.hbs) | `NAMING_TRANSCRIPT_CONTEXT_ILIUM_PROGRESS_MONITOR` |
| `ilium-client/src/smart_copy.rs:1446` | [naming/smart_copy/frozen-terminal-and-program-detected-selections-follow-as-json-data-cell-columns-in-a](templates/naming/smart_copy/frozen-terminal-and-program-detected-selections-follow-as-json-data-cell-columns-in-a.hbs) | `NAMING_SMART_COPY_FROZEN_TERMINAL_AND_PROGRAM_DETECTED_SELECTIONS_FOLLOW_AS_JSON_DATA_CELL_COLUMNS_IN_A` |
| `ilium-client/src/smart_copy.rs:195` | [naming/smart_copy/line-v0-is-outside-the-snapshot](templates/naming/smart_copy/line-v0-is-outside-the-snapshot.hbs) | `NAMING_SMART_COPY_LINE_V0_IS_OUTSIDE_THE_SNAPSHOT` |
| `ilium-client/src/smart_copy.rs:197` | [naming/smart_copy/line-v0-is-blank](templates/naming/smart_copy/line-v0-is-blank.hbs) | `NAMING_SMART_COPY_LINE_V0_IS_BLANK` |
| `ilium-client/src/smart_copy.rs:220` | [naming/smart_copy/line-v0-is-outside-the-snapshot](templates/naming/smart_copy/line-v0-is-outside-the-snapshot.hbs) | `NAMING_SMART_COPY_LINE_V0_IS_OUTSIDE_THE_SNAPSHOT` |
| `ilium-client/src/smart_copy.rs:228` | [naming/smart_copy/invalid-word-range-on-line](templates/naming/smart_copy/invalid-word-range-on-line.hbs) | `NAMING_SMART_COPY_INVALID_WORD_RANGE_ON_LINE` |
| `ilium-client/src/smart_copy.rs:231` | [naming/smart_copy/unknown-starting-word-on-line](templates/naming/smart_copy/unknown-starting-word-on-line.hbs) | `NAMING_SMART_COPY_UNKNOWN_STARTING_WORD_ON_LINE` |
| `ilium-client/src/smart_copy.rs:234` | [naming/smart_copy/unknown-ending-word-on-line](templates/naming/smart_copy/unknown-ending-word-on-line.hbs) | `NAMING_SMART_COPY_UNKNOWN_ENDING_WORD_ON_LINE` |
| `ilium-client/src/smart_copy.rs:409` | [naming/smart_copy/v0-visible-excerpt](templates/naming/smart_copy/v0-visible-excerpt.hbs) | `NAMING_SMART_COPY_V0_VISIBLE_EXCERPT` |
| `ilium-client/src/smart_copy.rs:697` | [naming/smart_copy/visible-fenced-block-l](templates/naming/smart_copy/visible-fenced-block-l.hbs) | `NAMING_SMART_COPY_VISIBLE_FENCED_BLOCK_L` |
| `ilium-client/src/smart_copy.rs:708` | [naming/smart_copy/code-contents-l](templates/naming/smart_copy/code-contents-l.hbs) | `NAMING_SMART_COPY_CODE_CONTENTS_L` |
| `ilium-client/src/smart_copy.rs:747` | [naming/smart_copy/visible-table-l](templates/naming/smart_copy/visible-table-l.hbs) | `NAMING_SMART_COPY_VISIBLE_TABLE_L` |
| `ilium-client/src/smart_copy.rs:756` | [naming/smart_copy/table-cell-l-v0-c](templates/naming/smart_copy/table-cell-l-v0-c.hbs) | `NAMING_SMART_COPY_TABLE_CELL_L_V0_C` |
| `ilium-client/src/smart_copy.rs:799` | [naming/smart_copy/visible-terminal-frame-l](templates/naming/smart_copy/visible-terminal-frame-l.hbs) | `NAMING_SMART_COPY_VISIBLE_TERMINAL_FRAME_L` |
| `ilium-client/src/smart_copy.rs:809` | [naming/smart_copy/frame-contents-l](templates/naming/smart_copy/frame-contents-l.hbs) | `NAMING_SMART_COPY_FRAME_CONTENTS_L` |
| `ilium-client/src/smart_copy.rs:845` | [naming/smart_copy/visible-diff-l](templates/naming/smart_copy/visible-diff-l.hbs) | `NAMING_SMART_COPY_VISIBLE_DIFF_L` |
| `ilium-client/src/smart_copy.rs:879` | [naming/smart_copy/visible-diagnostic-l](templates/naming/smart_copy/visible-diagnostic-l.hbs) | `NAMING_SMART_COPY_VISIBLE_DIAGNOSTIC_L` |
| `ilium-client/src/smart_copy.rs:918` | [naming/smart_copy/visible-tree-l](templates/naming/smart_copy/visible-tree-l.hbs) | `NAMING_SMART_COPY_VISIBLE_TREE_L` |
| `ilium-client/src/smart_copy.rs:950` | [naming/smart_copy/visible-indented-code-l](templates/naming/smart_copy/visible-indented-code-l.hbs) | `NAMING_SMART_COPY_VISIBLE_INDENTED_CODE_L` |
| `ilium-client/src/smart_copy.rs:972` | [naming/smart_copy/heading-l](templates/naming/smart_copy/heading-l.hbs) | `NAMING_SMART_COPY_HEADING_L` |
| `ilium-client/src/smart_copy.rs:986` | [naming/smart_copy/heading-l](templates/naming/smart_copy/heading-l.hbs) | `NAMING_SMART_COPY_HEADING_L` |
| `ilium-client/src/smart_copy.rs:1010` | [naming/smart_copy/visible-section-l](templates/naming/smart_copy/visible-section-l.hbs) | `NAMING_SMART_COPY_VISIBLE_SECTION_L` |
| `ilium-client/src/smart_copy.rs:1029` | [naming/smart_copy/prompt-line-l](templates/naming/smart_copy/prompt-line-l.hbs) | `NAMING_SMART_COPY_PROMPT_LINE_L` |
| `ilium-client/src/smart_copy.rs:1064` | [naming/smart_copy/visible-v0-l](templates/naming/smart_copy/visible-v0-l.hbs) | `NAMING_SMART_COPY_VISIBLE_V0_L` |
| `ilium-client/src/smart_copy.rs:1088` | [naming/smart_copy/visible-paragraph-l](templates/naming/smart_copy/visible-paragraph-l.hbs) | `NAMING_SMART_COPY_VISIBLE_PARAGRAPH_L` |
| `ilium-client/src/smart_copy.rs:1112` | [naming/smart_copy/inline-code-l](templates/naming/smart_copy/inline-code-l.hbs) | `NAMING_SMART_COPY_INLINE_CODE_L` |
| `ilium-client/src/smart_copy.rs:1144` | [naming/smart_copy/url-l](templates/naming/smart_copy/url-l.hbs) | `NAMING_SMART_COPY_URL_L` |
| `ilium-client/src/smart_copy.rs:1326` | [naming/smart_copy/invalid-jsonl-candidate](templates/naming/smart_copy/invalid-jsonl-candidate.hbs) | `NAMING_SMART_COPY_INVALID_JSONL_CANDIDATE` |
| `ilium-client/src/smart_copy.rs:134` | [naming/smart_copy/candidate-label-is-invalid](templates/naming/smart_copy/candidate-label-is-invalid.hbs) | `NAMING_SMART_COPY_CANDIDATE_LABEL_IS_INVALID` |
| `ilium-client/src/smart_copy.rs:140` | [naming/smart_copy/candidate-kind-is-invalid](templates/naming/smart_copy/candidate-kind-is-invalid.hbs) | `NAMING_SMART_COPY_CANDIDATE_KIND_IS_INVALID` |
| `ilium-client/src/smart_copy.rs:143` | [naming/smart_copy/candidate-has-an-invalid-part-count](templates/naming/smart_copy/candidate-has-an-invalid-part-count.hbs) | `NAMING_SMART_COPY_CANDIDATE_HAS_AN_INVALID_PART_COUNT` |
| `ilium-client/src/smart_copy.rs:150` | [naming/smart_copy/line-list-is-empty](templates/naming/smart_copy/line-list-is-empty.hbs) | `NAMING_SMART_COPY_LINE_LIST_IS_EMPTY` |
| `ilium-client/src/smart_copy.rs:173` | [naming/smart_copy/candidate-resolves-only-to-whitespace](templates/naming/smart_copy/candidate-resolves-only-to-whitespace.hbs) | `NAMING_SMART_COPY_CANDIDATE_RESOLVES_ONLY_TO_WHITESPACE` |
| `ilium-client/src/smart_copy.rs:192` | [naming/smart_copy/line-ids-start-at-1](templates/naming/smart_copy/line-ids-start-at-1.hbs) | `NAMING_SMART_COPY_LINE_IDS_START_AT_1` |
| `ilium-client/src/smart_copy.rs:216` | [naming/smart_copy/line-ids-start-at-1](templates/naming/smart_copy/line-ids-start-at-1.hbs) | `NAMING_SMART_COPY_LINE_IDS_START_AT_1` |
| `ilium-client/src/smart_copy.rs:618` | [naming/smart_copy/ps](templates/naming/smart_copy/ps.hbs) | `NAMING_SMART_COPY_PS` |
| `ilium-client/src/smart_copy.rs:618` | [naming/smart_copy/c](templates/naming/smart_copy/c.hbs) | `NAMING_SMART_COPY_C` |
| `ilium-client/src/smart_copy.rs:627` | [naming/smart_copy/diff-git](templates/naming/smart_copy/diff-git.hbs) | `NAMING_SMART_COPY_DIFF_GIT` |
| `ilium-client/src/smart_copy.rs:629` | [naming/smart_copy/a](templates/naming/smart_copy/a.hbs) | `NAMING_SMART_COPY_A` |
| `ilium-client/src/smart_copy.rs:630` | [naming/smart_copy/begin-patch](templates/naming/smart_copy/begin-patch.hbs) | `NAMING_SMART_COPY_BEGIN_PATCH` |
| `ilium-client/src/smart_copy.rs:639` | [naming/smart_copy/traceback](templates/naming/smart_copy/traceback.hbs) | `NAMING_SMART_COPY_TRACEBACK` |
| `ilium-client/src/smart_copy.rs:641` | [naming/smart_copy/caused-by](templates/naming/smart_copy/caused-by.hbs) | `NAMING_SMART_COPY_CAUSED_BY` |
| `ilium-client/src/smart_copy.rs:642` | [naming/smart_copy/thread](templates/naming/smart_copy/thread.hbs) | `NAMING_SMART_COPY_THREAD` |
| `ilium-client/src/smart_copy.rs:830` | [naming/smart_copy/diff](templates/naming/smart_copy/diff.hbs) | `NAMING_SMART_COPY_DIFF` |
| `ilium-client/src/smart_copy.rs:831` | [naming/smart_copy/index](templates/naming/smart_copy/index.hbs) | `NAMING_SMART_COPY_INDEX` |
| `ilium-client/src/smart_copy.rs:832` | [naming/smart_copy/new-file](templates/naming/smart_copy/new-file.hbs) | `NAMING_SMART_COPY_NEW_FILE` |
| `ilium-client/src/smart_copy.rs:833` | [naming/smart_copy/deleted-file](templates/naming/smart_copy/deleted-file.hbs) | `NAMING_SMART_COPY_DELETED_FILE` |
| `ilium-client/src/smart_copy.rs:865` | [naming/smart_copy/at](templates/naming/smart_copy/at.hbs) | `NAMING_SMART_COPY_AT` |
| `ilium-client/src/smart_copy.rs:865` | [naming/smart_copy/caused-by](templates/naming/smart_copy/caused-by.hbs) | `NAMING_SMART_COPY_CAUSED_BY` |
| `ilium-client/src/smart_copy.rs:1322` | [naming/smart_copy/jsonl-record-exceeds-64-kib](templates/naming/smart_copy/jsonl-record-exceeds-64-kib.hbs) | `NAMING_SMART_COPY_JSONL_RECORD_EXCEEDS_64_KIB` |
| `ilium-client/src/control/mod.rs:278` | [voice/mod/system-instructions](templates/voice/mod/system-instructions.hbs) | `VOICE_MOD_SYSTEM_INSTRUCTIONS` |
| `ilium-client/src/control/mod.rs:366` | [voice/mod/unknown-ilium-tool](templates/voice/mod/unknown-ilium-tool.hbs) | `VOICE_MOD_UNKNOWN_ILIUM_TOOL` |
| `ilium-client/src/control/mod.rs:371` | [voice/mod/invalid-tool-arguments](templates/voice/mod/invalid-tool-arguments.hbs) | `VOICE_MOD_INVALID_TOOL_ARGUMENTS` |
| `ilium-client/src/control/mod.rs:71` | [voice/mod/the-active-pane-is-currently-a-detected](templates/voice/mod/the-active-pane-is-currently-a-detected.hbs) | `VOICE_MOD_THE_ACTIVE_PANE_IS_CURRENTLY_A_DETECTED` |
| `ilium-client/src/control/mod.rs:74` | [voice/mod/the-active-pane-is-not-currently-a](templates/voice/mod/the-active-pane-is-not-currently-a.hbs) | `VOICE_MOD_THE_ACTIVE_PANE_IS_NOT_CURRENTLY_A` |
| `ilium-client/src/control/mod.rs:209` | [voice/mod/ask-only-the-exact-question-do-not](templates/voice/mod/ask-only-the-exact-question-do-not.hbs) | `VOICE_MOD_ASK_ONLY_THE_EXACT_QUESTION_DO_NOT` |
| `ilium-client/src/control/mod.rs:232` | [voice/mod/that-confirmation-token-is-missing-expired-or](templates/voice/mod/that-confirmation-token-is-missing-expired-or.hbs) | `VOICE_MOD_THAT_CONFIRMATION_TOKEN_IS_MISSING_EXPIRED_OR` |
| `ilium-client/src/control/tools.rs:25` | [voice/tools/inspect-the-current-ilium-session-tree-focus](templates/voice/tools/inspect-the-current-ilium-session-tree-focus.hbs) | `VOICE_TOOLS_INSPECT_THE_CURRENT_ILIUM_SESSION_TREE_FOCUS` |
| `ilium-client/src/control/tools.rs:37` | [voice/tools/immediately-stop-and-disable-the-current-ilium](templates/voice/tools/immediately-stop-and-disable-the-current-ilium.hbs) | `VOICE_TOOLS_IMMEDIATELY_STOP_AND_DISABLE_THE_CURRENT_ILIUM` |
| `ilium-client/src/control/tools.rs:46` | [voice/tools/navigate-global-ui-surfaces-focus-tree-or](templates/voice/tools/navigate-global-ui-surfaces-focus-tree-or.hbs) | `VOICE_TOOLS_NAVIGATE_GLOBAL_UI_SURFACES_FOCUS_TREE_OR` |
| `ilium-client/src/control/tools.rs:61` | [voice/tools/create-open-organize-rename-move-reparent-split](templates/voice/tools/create-open-organize-rename-move-reparent-split.hbs) | `VOICE_TOOLS_CREATE_OPEN_ORGANIZE_RENAME_MOVE_REPARENT_SPLIT` |
| `ilium-client/src/control/tools.rs:93` | [voice/tools/primary-voice-dictation-action-send-the-exact](templates/voice/tools/primary-voice-dictation-action-send-the-exact.hbs) | `VOICE_TOOLS_PRIMARY_VOICE_DICTATION_ACTION_SEND_THE_EXACT` |
| `ilium-client/src/control/tools.rs:98` | [voice/tools/exact-text-to-type-preserving-slash-commands](templates/voice/tools/exact-text-to-type-preserving-slash-commands.hbs) | `VOICE_TOOLS_EXACT_TEXT_TO_TYPE_PRESERVING_SLASH_COMMANDS` |
| `ilium-client/src/control/tools.rs:106` | [voice/tools/type-exact-text-into-a-terminal-without](templates/voice/tools/type-exact-text-into-a-terminal-without.hbs) | `VOICE_TOOLS_TYPE_EXACT_TEXT_INTO_A_TERMINAL_WITHOUT` |
| `ilium-client/src/control/tools.rs:111` | [voice/tools/exact-text-to-leave-visible-and-unsubmitted](templates/voice/tools/exact-text-to-leave-visible-and-unsubmitted.hbs) | `VOICE_TOOLS_EXACT_TEXT_TO_LEAVE_VISIBLE_AND_UNSUBMITTED` |
| `ilium-client/src/control/tools.rs:119` | [voice/tools/control-non-dictation-terminal-behavior-press-a](templates/voice/tools/control-non-dictation-terminal-behavior-press-a.hbs) | `VOICE_TOOLS_CONTROL_NON_DICTATION_TERMINAL_BEHAVIOR_PRESS_A` |
| `ilium-client/src/control/tools.rs:138` | [voice/tools/operate-a-built-in-editor-pane-save](templates/voice/tools/operate-a-built-in-editor-pane-save.hbs) | `VOICE_TOOLS_OPERATE_A_BUILT_IN_EDITOR_PANE_SAVE` |
| `ilium-client/src/control/tools.rs:155` | [voice/tools/operate-every-kanban-surface-select-open-cards](templates/voice/tools/operate-every-kanban-surface-select-open-cards.hbs) | `VOICE_TOOLS_OPERATE_EVERY_KANBAN_SURFACE_SELECT_OPEN_CARDS` |
| `ilium-client/src/control/tools.rs:175` | [voice/tools/get-set-or-adjust-any-persisted-setting](templates/voice/tools/get-set-or-adjust-any-persisted-setting.hbs) | `VOICE_TOOLS_GET_SET_OR_ADJUST_ANY_PERSISTED_SETTING` |
| `ilium-client/src/control/tools.rs:190` | [voice/tools/search-all-retained-terminal-output-open-editor](templates/voice/tools/search-all-retained-terminal-output-open-editor.hbs) | `VOICE_TOOLS_SEARCH_ALL_RETAINED_TERMINAL_OUTPUT_OPEN_EDITOR` |
| `ilium-client/src/control/tools.rs:204` | [voice/tools/detach-restart-the-tui-client-restart-the](templates/voice/tools/detach-restart-the-tui-client-restart-the.hbs) | `VOICE_TOOLS_DETACH_RESTART_THE_TUI_CLIENT_RESTART_THE` |
| `ilium-client/src/control/tools.rs:216` | [voice/tools/confirm-or-cancel-one-pending-ilium-action](templates/voice/tools/confirm-or-cancel-one-pending-ilium-action.hbs) | `VOICE_TOOLS_CONFIRM_OR_CANCEL_ONE_PENDING_ILIUM_ACTION` |
| `ilium-client/src/control/tools.rs:244` | [voice/tools/slash-separated-node-name-path-from-the](templates/voice/tools/slash-separated-node-name-path-from-the.hbs) | `VOICE_TOOLS_SLASH_SEPARATED_NODE_NAME_PATH_FROM_THE` |
| `ilium-client/src/control/policy.rs:42` | [voice/policy/run-the-shell-command-v0-in-a](templates/voice/policy/run-the-shell-command-v0-in-a.hbs) | `VOICE_POLICY_RUN_THE_SHELL_COMMAND_V0_IN_A` |
| `ilium-client/src/control/policy.rs:83` | [voice/policy/queue-this-prompt-to-be-submitted-automatically](templates/voice/policy/queue-this-prompt-to-be-submitted-automatically.hbs) | `VOICE_POLICY_QUEUE_THIS_PROMPT_TO_BE_SUBMITTED_AUTOMATICALLY` |
| `ilium-client/src/control/policy.rs:171` | [voice/policy/board-has-no-column](templates/voice/policy/board-has-no-column.hbs) | `VOICE_POLICY_BOARD_HAS_NO_COLUMN` |
| `ilium-client/src/control/policy.rs:179` | [voice/policy/board-has-no-card-v0-in-column](templates/voice/policy/board-has-no-card-v0-in-column.hbs) | `VOICE_POLICY_BOARD_HAS_NO_CARD_V0_IN_COLUMN` |
| `ilium-client/src/control/policy.rs:181` | [voice/policy/permanently-delete-the-card-v0-from-column](templates/voice/policy/permanently-delete-the-card-v0-from-column.hbs) | `VOICE_POLICY_PERMANENTLY_DELETE_THE_CARD_V0_FROM_COLUMN` |
| `ilium-client/src/control/policy.rs:186` | [voice/policy/permanently-delete-the-column-v0-and-its](templates/voice/policy/permanently-delete-the-column-v0-and-its.hbs) | `VOICE_POLICY_PERMANENTLY_DELETE_THE_COLUMN_V0_AND_ITS` |
| `ilium-client/src/control/policy.rs:40` | [voice/policy/command-line-is-required](templates/voice/policy/command-line-is-required.hbs) | `VOICE_POLICY_COMMAND_LINE_IS_REQUIRED` |
| `ilium-client/src/control/policy.rs:51` | [voice/policy/replace-the-editor-s-entire-current-document](templates/voice/policy/replace-the-editor-s-entire-current-document.hbs) | `VOICE_POLICY_REPLACE_THE_EDITOR_S_ENTIRE_CURRENT_DOCUMENT` |
| `ilium-client/src/control/policy.rs:61` | [voice/policy/press-enter-and-submit-what-you-see](templates/voice/policy/press-enter-and-submit-what-you-see.hbs) | `VOICE_POLICY_PRESS_ENTER_AND_SUBMIT_WHAT_YOU_SEE` |
| `ilium-client/src/control/policy.rs:64` | [voice/policy/schedule-this-terminal-input-for-automatic-submission](templates/voice/policy/schedule-this-terminal-input-for-automatic-submission.hbs) | `VOICE_POLICY_SCHEDULE_THIS_TERMINAL_INPUT_FOR_AUTOMATIC_SUBMISSION` |
| `ilium-client/src/control/policy.rs:71` | [voice/policy/queue-this-prompt-to-be-submitted-automatically-2](templates/voice/policy/queue-this-prompt-to-be-submitted-automatically-2.hbs) | `VOICE_POLICY_QUEUE_THIS_PROMPT_TO_BE_SUBMITTED_AUTOMATICALLY_2` |
| `ilium-client/src/control/policy.rs:79` | [voice/policy/runs-is-required-and-must-be-positive](templates/voice/policy/runs-is-required-and-must-be-positive.hbs) | `VOICE_POLICY_RUNS_IS_REQUIRED_AND_MUST_BE_POSITIVE` |
| `ilium-client/src/control/policy.rs:87` | [voice/policy/queue-this-prompt-for-automatic-submission-after](templates/voice/policy/queue-this-prompt-for-automatic-submission-after.hbs) | `VOICE_POLICY_QUEUE_THIS_PROMPT_FOR_AUTOMATIC_SUBMISSION_AFTER` |
| `ilium-client/src/control/policy.rs:103` | [voice/policy/kill-the-entire-ilium-session-and-every](templates/voice/policy/kill-the-entire-ilium-session-and-every.hbs) | `VOICE_POLICY_KILL_THE_ENTIRE_ILIUM_SESSION_AND_EVERY` |
| `ilium-client/src/control/policy.rs:106` | [voice/policy/restart-the-detached-ilium-server-and-temporarily](templates/voice/policy/restart-the-detached-ilium-server-and-temporarily.hbs) | `VOICE_POLICY_RESTART_THE_DETACHED_ILIUM_SERVER_AND_TEMPORARILY` |
| `ilium-client/src/control/policy.rs:119` | [voice/policy/cancelled-the-pending-action](templates/voice/policy/cancelled-the-pending-action.hbs) | `VOICE_POLICY_CANCELLED_THE_PENDING_ACTION` |
| `ilium-client/src/control/policy.rs:147` | [voice/policy/cancelled-the-pending-action](templates/voice/policy/cancelled-the-pending-action.hbs) | `VOICE_POLICY_CANCELLED_THE_PENDING_ACTION` |
| `ilium-client/src/control/policy.rs:164` | [voice/policy/target-is-not-a-board-pane](templates/voice/policy/target-is-not-a-board-pane.hbs) | `VOICE_POLICY_TARGET_IS_NOT_A_BOARD_PANE` |
| `ilium-client/src/control/policy.rs:167` | [voice/policy/column-is-required](templates/voice/policy/column-is-required.hbs) | `VOICE_POLICY_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/policy.rs:175` | [voice/policy/card-is-required](templates/voice/policy/card-is-required.hbs) | `VOICE_POLICY_CARD_IS_REQUIRED` |
| `ilium-client/src/control/policy.rs:205` | [voice/policy/cancelled-the-pending-action](templates/voice/policy/cancelled-the-pending-action.hbs) | `VOICE_POLICY_CANCELLED_THE_PENDING_ACTION` |
| `ilium-client/src/control/policy.rs:221` | [voice/policy/i-typed-it-into-the-target-terminal](templates/voice/policy/i-typed-it-into-the-target-terminal.hbs) | `VOICE_POLICY_I_TYPED_IT_INTO_THE_TARGET_TERMINAL` |
| `ilium-client/src/control/policy.rs:236` | [voice/policy/left-the-staged-terminal-text-visible-and](templates/voice/policy/left-the-staged-terminal-text-visible-and.hbs) | `VOICE_POLICY_LEFT_THE_STAGED_TERMINAL_TEXT_VISIBLE_AND` |
| `ilium-client/src/control/executor.rs:134` | [voice/executor/search-result-index-v0-does-not-exist](templates/voice/executor/search-result-index-v0-does-not-exist.hbs) | `VOICE_EXECUTOR_SEARCH_RESULT_INDEX_V0_DOES_NOT_EXIST` |
| `ilium-client/src/control/executor.rs:145` | [voice/executor/search-result-index-v0-does-not-exist](templates/voice/executor/search-result-index-v0-does-not-exist.hbs) | `VOICE_EXECUTOR_SEARCH_RESULT_INDEX_V0_DOES_NOT_EXIST` |
| `ilium-client/src/control/executor.rs:183` | [voice/executor/found-v0-workspace-results-for](templates/voice/executor/found-v0-workspace-results-for.hbs) | `VOICE_EXECUTOR_FOUND_V0_WORKSPACE_RESULTS_FOR` |
| `ilium-client/src/control/executor.rs:200` | [voice/executor/focused-pane](templates/voice/executor/focused-pane.hbs) | `VOICE_EXECUTOR_FOCUSED_PANE` |
| `ilium-client/src/control/executor.rs:234` | [voice/executor/node-v0-is-not-a-split-view](templates/voice/executor/node-v0-is-not-a-split-view.hbs) | `VOICE_EXECUTOR_NODE_V0_IS_NOT_A_SPLIT_VIEW` |
| `ilium-client/src/control/executor.rs:238` | [voice/executor/showing-split](templates/voice/executor/showing-split.hbs) | `VOICE_EXECUTOR_SHOWING_SPLIT` |
| `ilium-client/src/control/executor.rs:257` | [voice/executor/opened-v0-settings](templates/voice/executor/opened-v0-settings.hbs) | `VOICE_EXECUTOR_OPENED_V0_SETTINGS` |
| `ilium-client/src/control/executor.rs:292` | [voice/executor/invalid-workspace-branch](templates/voice/executor/invalid-workspace-branch.hbs) | `VOICE_EXECUTOR_INVALID_WORKSPACE_BRANCH` |
| `ilium-client/src/control/executor.rs:310` | [voice/executor/creating-a-v0-agent-in-a-git](templates/voice/executor/creating-a-v0-agent-in-a-git.hbs) | `VOICE_EXECUTOR_CREATING_A_V0_AGENT_IN_A_GIT` |
| `ilium-client/src/control/executor.rs:324` | [voice/executor/creating-a-v0-agent-pane](templates/voice/executor/creating-a-v0-agent-pane.hbs) | `VOICE_EXECUTOR_CREATING_A_V0_AGENT_PANE` |
| `ilium-client/src/control/executor.rs:414` | [voice/executor/a-split-view-can-contain-at-most](templates/voice/executor/a-split-view-can-contain-at-most.hbs) | `VOICE_EXECUTOR_A_SPLIT_VIEW_CAN_CONTAIN_AT_MOST` |
| `ilium-client/src/control/executor.rs:459` | [voice/executor/node-v0-cannot-be-expanded](templates/voice/executor/node-v0-cannot-be-expanded.hbs) | `VOICE_EXECUTOR_NODE_V0_CANNOT_BE_EXPANDED` |
| `ilium-client/src/control/executor.rs:775` | [voice/executor/node-v0-is-not-a-live](templates/voice/executor/node-v0-is-not-a-live.hbs) | `VOICE_EXECUTOR_NODE_V0_IS_NOT_A_LIVE` |
| `ilium-client/src/control/executor.rs:781` | [voice/executor/node-v0-is-not-a-terminal-pane](templates/voice/executor/node-v0-is-not-a-terminal-pane.hbs) | `VOICE_EXECUTOR_NODE_V0_IS_NOT_A_TERMINAL_PANE` |
| `ilium-client/src/control/executor.rs:787` | [voice/executor/node-v0-is-not-an-editor-pane](templates/voice/executor/node-v0-is-not-an-editor-pane.hbs) | `VOICE_EXECUTOR_NODE_V0_IS_NOT_AN_EDITOR_PANE` |
| `ilium-client/src/control/executor.rs:813` | [voice/policy/board-has-no-column](templates/voice/policy/board-has-no-column.hbs) | `VOICE_POLICY_BOARD_HAS_NO_COLUMN` |
| `ilium-client/src/control/executor.rs:830` | [voice/policy/board-has-no-card-v0-in-column](templates/voice/policy/board-has-no-card-v0-in-column.hbs) | `VOICE_POLICY_BOARD_HAS_NO_CARD_V0_IN_COLUMN` |
| `ilium-client/src/control/executor.rs:836` | [voice/executor/v0-is-required](templates/voice/executor/v0-is-required.hbs) | `VOICE_EXECUTOR_V0_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:838` | [voice/executor/v0-must-not-be-empty](templates/voice/executor/v0-must-not-be-empty.hbs) | `VOICE_EXECUTOR_V0_MUST_NOT_BE_EMPTY` |
| `ilium-client/src/control/executor.rs:874` | [voice/executor/unknown-settings-tab](templates/voice/executor/unknown-settings-tab.hbs) | `VOICE_EXECUTOR_UNKNOWN_SETTINGS_TAB` |
| `ilium-client/src/control/executor.rs:48` | [voice/executor/read-ilium-get-state-after-the-server](templates/voice/executor/read-ilium-get-state-after-the-server.hbs) | `VOICE_EXECUTOR_READ_ILIUM_GET_STATE_AFTER_THE_SERVER` |
| `ilium-client/src/control/executor.rs:71` | [voice/executor/current-ilium-state](templates/voice/executor/current-ilium-state.hbs) | `VOICE_EXECUTOR_CURRENT_ILIUM_STATE` |
| `ilium-client/src/control/executor.rs:100` | [voice/executor/queued-text-and-enter-for-the-terminal](templates/voice/executor/queued-text-and-enter-for-the-terminal.hbs) | `VOICE_EXECUTOR_QUEUED_TEXT_AND_ENTER_FOR_THE_TERMINAL` |
| `ilium-client/src/control/executor.rs:114` | [voice/executor/staged-text-in-the-terminal](templates/voice/executor/staged-text-in-the-terminal.hbs) | `VOICE_EXECUTOR_STAGED_TEXT_IN_THE_TERMINAL` |
| `ilium-client/src/control/executor.rs:151` | [voice/executor/there-is-no-selected-search-result](templates/voice/executor/there-is-no-selected-search-result.hbs) | `VOICE_EXECUTOR_THERE_IS_NO_SELECTED_SEARCH_RESULT` |
| `ilium-client/src/control/executor.rs:155` | [voice/executor/opened-the-selected-search-result](templates/voice/executor/opened-the-selected-search-result.hbs) | `VOICE_EXECUTOR_OPENED_THE_SELECTED_SEARCH_RESULT` |
| `ilium-client/src/control/executor.rs:160` | [voice/executor/closed-workspace-search](templates/voice/executor/closed-workspace-search.hbs) | `VOICE_EXECUTOR_CLOSED_WORKSPACE_SEARCH` |
| `ilium-client/src/control/executor.rs:193` | [voice/executor/focused-the-left-tree-panel](templates/voice/executor/focused-the-left-tree-panel.hbs) | `VOICE_EXECUTOR_FOCUSED_THE_LEFT_TREE_PANEL` |
| `ilium-client/src/control/executor.rs:206` | [voice/executor/focused-the-next-visible-pane](templates/voice/executor/focused-the-next-visible-pane.hbs) | `VOICE_EXECUTOR_FOCUSED_THE_NEXT_VISIBLE_PANE` |
| `ilium-client/src/control/executor.rs:211` | [voice/executor/focused-the-previous-visible-pane](templates/voice/executor/focused-the-previous-visible-pane.hbs) | `VOICE_EXECUTOR_FOCUSED_THE_PREVIOUS_VISIBLE_PANE` |
| `ilium-client/src/control/executor.rs:215` | [voice/executor/direction-is-required](templates/voice/executor/direction-is-required.hbs) | `VOICE_EXECUTOR_DIRECTION_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:224` | [voice/executor/moved-focus-within-the-visible-split](templates/voice/executor/moved-focus-within-the-visible-split.hbs) | `VOICE_EXECUTOR_MOVED_FOCUS_WITHIN_THE_VISIBLE_SPLIT` |
| `ilium-client/src/control/executor.rs:244` | [voice/executor/opened-settings](templates/voice/executor/opened-settings.hbs) | `VOICE_EXECUTOR_OPENED_SETTINGS` |
| `ilium-client/src/control/executor.rs:247` | [voice/executor/tab-is-required](templates/voice/executor/tab-is-required.hbs) | `VOICE_EXECUTOR_TAB_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:263` | [voice/executor/opened-workspace-search](templates/voice/executor/opened-workspace-search.hbs) | `VOICE_EXECUTOR_OPENED_WORKSPACE_SEARCH` |
| `ilium-client/src/control/executor.rs:267` | [voice/executor/opened-help](templates/voice/executor/opened-help.hbs) | `VOICE_EXECUTOR_OPENED_HELP` |
| `ilium-client/src/control/executor.rs:271` | [voice/executor/closed-the-current-overlay](templates/voice/executor/closed-the-current-overlay.hbs) | `VOICE_EXECUTOR_CLOSED_THE_CURRENT_OVERLAY` |
| `ilium-client/src/control/executor.rs:281` | [voice/executor/creating-a-terminal-pane](templates/voice/executor/creating-a-terminal-pane.hbs) | `VOICE_EXECUTOR_CREATING_A_TERMINAL_PANE` |
| `ilium-client/src/control/executor.rs:285` | [voice/executor/provider-is-required](templates/voice/executor/provider-is-required.hbs) | `VOICE_EXECUTOR_PROVIDER_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:298` | [voice/executor/workspace-base-must-not-be-empty](templates/voice/executor/workspace-base-must-not-be-empty.hbs) | `VOICE_EXECUTOR_WORKSPACE_BASE_MUST_NOT_BE_EMPTY` |
| `ilium-client/src/control/executor.rs:336` | [voice/executor/creating-the-command-pane](templates/voice/executor/creating-the-command-pane.hbs) | `VOICE_EXECUTOR_CREATING_THE_COMMAND_PANE` |
| `ilium-client/src/control/executor.rs:343` | [voice/executor/opening-the-file-in-an-editor-pane](templates/voice/executor/opening-the-file-in-an-editor-pane.hbs) | `VOICE_EXECUTOR_OPENING_THE_FILE_IN_AN_EDITOR_PANE` |
| `ilium-client/src/control/executor.rs:351` | [voice/executor/adding-the-folder-to-the-left-panel](templates/voice/executor/adding-the-folder-to-the-left-panel.hbs) | `VOICE_EXECUTOR_ADDING_THE_FOLDER_TO_THE_LEFT_PANEL` |
| `ilium-client/src/control/executor.rs:357` | [voice/executor/adding-the-project](templates/voice/executor/adding-the-project.hbs) | `VOICE_EXECUTOR_ADDING_THE_PROJECT` |
| `ilium-client/src/control/executor.rs:363` | [voice/executor/changing-the-project-s-folder](templates/voice/executor/changing-the-project-s-folder.hbs) | `VOICE_EXECUTOR_CHANGING_THE_PROJECT_S_FOLDER` |
| `ilium-client/src/control/executor.rs:369` | [voice/executor/creating-the-group](templates/voice/executor/creating-the-group.hbs) | `VOICE_EXECUTOR_CREATING_THE_GROUP` |
| `ilium-client/src/control/executor.rs:396` | [voice/executor/board-creation-failed](templates/voice/executor/board-creation-failed.hbs) | `VOICE_EXECUTOR_BOARD_CREATION_FAILED` |
| `ilium-client/src/control/executor.rs:398` | [voice/executor/creating-the-board](templates/voice/executor/creating-the-board.hbs) | `VOICE_EXECUTOR_CREATING_THE_BOARD` |
| `ilium-client/src/control/executor.rs:423` | [voice/executor/creating-the-split-view](templates/voice/executor/creating-the-split-view.hbs) | `VOICE_EXECUTOR_CREATING_THE_SPLIT_VIEW` |
| `ilium-client/src/control/executor.rs:429` | [voice/executor/renaming-the-tree-item](templates/voice/executor/renaming-the-tree-item.hbs) | `VOICE_EXECUTOR_RENAMING_THE_TREE_ITEM` |
| `ilium-client/src/control/executor.rs:439` | [voice/executor/moving-the-tree-item](templates/voice/executor/moving-the-tree-item.hbs) | `VOICE_EXECUTOR_MOVING_THE_TREE_ITEM` |
| `ilium-client/src/control/executor.rs:445` | [voice/executor/reparenting-the-tree-item](templates/voice/executor/reparenting-the-tree-item.hbs) | `VOICE_EXECUTOR_REPARENTING_THE_TREE_ITEM` |
| `ilium-client/src/control/executor.rs:450` | [voice/executor/closing-the-tree-item](templates/voice/executor/closing-the-tree-item.hbs) | `VOICE_EXECUTOR_CLOSING_THE_TREE_ITEM` |
| `ilium-client/src/control/executor.rs:463` | [voice/executor/toggled-the-left-panel-group](templates/voice/executor/toggled-the-left-panel-group.hbs) | `VOICE_EXECUTOR_TOGGLED_THE_LEFT_PANEL_GROUP` |
| `ilium-client/src/control/executor.rs:471` | [voice/executor/started-automatic-retitling](templates/voice/executor/started-automatic-retitling.hbs) | `VOICE_EXECUTOR_STARTED_AUTOMATIC_RETITLING` |
| `ilium-client/src/control/executor.rs:478` | [voice/executor/requested-project-restructuring](templates/voice/executor/requested-project-restructuring.hbs) | `VOICE_EXECUTOR_REQUESTED_PROJECT_RESTRUCTURING` |
| `ilium-client/src/control/executor.rs:486` | [voice/executor/requested-project-restructuring](templates/voice/executor/requested-project-restructuring.hbs) | `VOICE_EXECUTOR_REQUESTED_PROJECT_RESTRUCTURING` |
| `ilium-client/src/control/executor.rs:493` | [voice/executor/reverting-the-project-s-latest-restructure](templates/voice/executor/reverting-the-project-s-latest-restructure.hbs) | `VOICE_EXECUTOR_REVERTING_THE_PROJECT_S_LATEST_RESTRUCTURE` |
| `ilium-client/src/control/executor.rs:504` | [voice/executor/key-is-required](templates/voice/executor/key-is-required.hbs) | `VOICE_EXECUTOR_KEY_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:511` | [voice/executor/sent-the-key-to-the-terminal](templates/voice/executor/sent-the-key-to-the-terminal.hbs) | `VOICE_EXECUTOR_SENT_THE_KEY_TO_THE_TERMINAL` |
| `ilium-client/src/control/executor.rs:516` | [voice/executor/target-is-not-a-terminal-pane](templates/voice/executor/target-is-not-a-terminal-pane.hbs) | `VOICE_EXECUTOR_TARGET_IS_NOT_A_TERMINAL_PANE` |
| `ilium-client/src/control/executor.rs:523` | [voice/executor/scrolled-the-terminal](templates/voice/executor/scrolled-the-terminal.hbs) | `VOICE_EXECUTOR_SCROLLED_THE_TERMINAL` |
| `ilium-client/src/control/executor.rs:530` | [voice/executor/returned-to-live-terminal-output](templates/voice/executor/returned-to-live-terminal-output.hbs) | `VOICE_EXECUTOR_RETURNED_TO_LIVE_TERMINAL_OUTPUT` |
| `ilium-client/src/control/executor.rs:541` | [voice/executor/scheduled-terminal-input](templates/voice/executor/scheduled-terminal-input.hbs) | `VOICE_EXECUTOR_SCHEDULED_TERMINAL_INPUT` |
| `ilium-client/src/control/executor.rs:551` | [voice/policy/runs-is-required-and-must-be-positive](templates/voice/policy/runs-is-required-and-must-be-positive.hbs) | `VOICE_POLICY_RUNS_IS_REQUIRED_AND_MUST_BE_POSITIVE` |
| `ilium-client/src/control/executor.rs:561` | [voice/executor/queued-the-prompt-for-the-agent-s](templates/voice/executor/queued-the-prompt-for-the-agent-s.hbs) | `VOICE_EXECUTOR_QUEUED_THE_PROMPT_FOR_THE_AGENT_S` |
| `ilium-client/src/control/executor.rs:566` | [voice/executor/clearing-the-pane-s-prompt-queue](templates/voice/executor/clearing-the-pane-s-prompt-queue.hbs) | `VOICE_EXECUTOR_CLEARING_THE_PANE_S_PROMPT_QUEUE` |
| `ilium-client/src/control/executor.rs:577` | [voice/executor/target-is-not-an-editor-pane](templates/voice/executor/target-is-not-an-editor-pane.hbs) | `VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE` |
| `ilium-client/src/control/executor.rs:595` | [voice/executor/target-is-not-an-editor-pane](templates/voice/executor/target-is-not-an-editor-pane.hbs) | `VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE` |
| `ilium-client/src/control/executor.rs:603` | [voice/executor/this-editor-has-no-file-path-yet](templates/voice/executor/this-editor-has-no-file-path-yet.hbs) | `VOICE_EXECUTOR_THIS_EDITOR_HAS_NO_FILE_PATH_YET` |
| `ilium-client/src/control/executor.rs:609` | [voice/executor/editor-updated](templates/voice/executor/editor-updated.hbs) | `VOICE_EXECUTOR_EDITOR_UPDATED` |
| `ilium-client/src/control/executor.rs:615` | [voice/executor/editor-updated](templates/voice/executor/editor-updated.hbs) | `VOICE_EXECUTOR_EDITOR_UPDATED` |
| `ilium-client/src/control/executor.rs:618` | [voice/executor/text-is-required](templates/voice/executor/text-is-required.hbs) | `VOICE_EXECUTOR_TEXT_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:620` | [voice/executor/target-is-not-an-editor-pane](templates/voice/executor/target-is-not-an-editor-pane.hbs) | `VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE` |
| `ilium-client/src/control/executor.rs:629` | [voice/executor/inserted-text-into-the-editor](templates/voice/executor/inserted-text-into-the-editor.hbs) | `VOICE_EXECUTOR_INSERTED_TEXT_INTO_THE_EDITOR` |
| `ilium-client/src/control/executor.rs:632` | [voice/executor/text-is-required](templates/voice/executor/text-is-required.hbs) | `VOICE_EXECUTOR_TEXT_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:634` | [voice/executor/target-is-not-an-editor-pane](templates/voice/executor/target-is-not-an-editor-pane.hbs) | `VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE` |
| `ilium-client/src/control/executor.rs:638` | [voice/executor/replaced-the-editor-s-document](templates/voice/executor/replaced-the-editor-s-document.hbs) | `VOICE_EXECUTOR_REPLACED_THE_EDITOR_S_DOCUMENT` |
| `ilium-client/src/control/executor.rs:641` | [voice/executor/line-is-required](templates/voice/executor/line-is-required.hbs) | `VOICE_EXECUTOR_LINE_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:644` | [voice/executor/target-is-not-an-editor-pane](templates/voice/executor/target-is-not-an-editor-pane.hbs) | `VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE` |
| `ilium-client/src/control/executor.rs:647` | [voice/executor/moved-the-editor-cursor](templates/voice/executor/moved-the-editor-cursor.hbs) | `VOICE_EXECUTOR_MOVED_THE_EDITOR_CURSOR` |
| `ilium-client/src/control/executor.rs:651` | [voice/executor/toggled-the-rendered-source-view](templates/voice/executor/toggled-the-rendered-source-view.hbs) | `VOICE_EXECUTOR_TOGGLED_THE_RENDERED_SOURCE_VIEW` |
| `ilium-client/src/control/executor.rs:655` | [voice/executor/toggled-line-numbers](templates/voice/executor/toggled-line-numbers.hbs) | `VOICE_EXECUTOR_TOGGLED_LINE_NUMBERS` |
| `ilium-client/src/control/executor.rs:659` | [voice/executor/toggled-the-minimap](templates/voice/executor/toggled-the-minimap.hbs) | `VOICE_EXECUTOR_TOGGLED_THE_MINIMAP` |
| `ilium-client/src/control/executor.rs:663` | [voice/executor/toggled-autosave](templates/voice/executor/toggled-autosave.hbs) | `VOICE_EXECUTOR_TOGGLED_AUTOSAVE` |
| `ilium-client/src/control/executor.rs:679` | [voice/policy/target-is-not-a-board-pane](templates/voice/policy/target-is-not-a-board-pane.hbs) | `VOICE_POLICY_TARGET_IS_NOT_A_BOARD_PANE` |
| `ilium-client/src/control/executor.rs:684` | [voice/policy/column-is-required](templates/voice/policy/column-is-required.hbs) | `VOICE_POLICY_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:688` | [voice/policy/column-is-required](templates/voice/policy/column-is-required.hbs) | `VOICE_POLICY_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:689` | [voice/policy/card-is-required](templates/voice/policy/card-is-required.hbs) | `VOICE_POLICY_CARD_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:695` | [voice/policy/column-is-required](templates/voice/policy/column-is-required.hbs) | `VOICE_POLICY_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:696` | [voice/policy/card-is-required](templates/voice/policy/card-is-required.hbs) | `VOICE_POLICY_CARD_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:709` | [voice/policy/column-is-required](templates/voice/policy/column-is-required.hbs) | `VOICE_POLICY_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:710` | [voice/policy/card-is-required](templates/voice/policy/card-is-required.hbs) | `VOICE_POLICY_CARD_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:715` | [voice/policy/column-is-required](templates/voice/policy/column-is-required.hbs) | `VOICE_POLICY_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:719` | [voice/policy/column-is-required](templates/voice/policy/column-is-required.hbs) | `VOICE_POLICY_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:720` | [voice/policy/card-is-required](templates/voice/policy/card-is-required.hbs) | `VOICE_POLICY_CARD_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:723` | [voice/executor/destination-column-is-required](templates/voice/executor/destination-column-is-required.hbs) | `VOICE_EXECUTOR_DESTINATION_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:727` | [voice/policy/column-is-required](templates/voice/policy/column-is-required.hbs) | `VOICE_POLICY_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:728` | [voice/policy/card-is-required](templates/voice/policy/card-is-required.hbs) | `VOICE_POLICY_CARD_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:729` | [voice/executor/checkbox-is-required](templates/voice/executor/checkbox-is-required.hbs) | `VOICE_EXECUTOR_CHECKBOX_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:734` | [voice/policy/column-is-required](templates/voice/policy/column-is-required.hbs) | `VOICE_POLICY_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:735` | [voice/policy/card-is-required](templates/voice/policy/card-is-required.hbs) | `VOICE_POLICY_CARD_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:740` | [voice/policy/column-is-required](templates/voice/policy/column-is-required.hbs) | `VOICE_POLICY_COLUMN_IS_REQUIRED` |
| `ilium-client/src/control/executor.rs:748` | [voice/executor/board-updated-and-persisted](templates/voice/executor/board-updated-and-persisted.hbs) | `VOICE_EXECUTOR_BOARD_UPDATED_AND_PERSISTED` |
| `ilium-client/src/control/executor.rs:758` | [voice/executor/session-lifecycle-request-queued](templates/voice/executor/session-lifecycle-request-queued.hbs) | `VOICE_EXECUTOR_SESSION_LIFECYCLE_REQUEST_QUEUED` |
| `ilium-client/src/control/executor.rs:768` | [voice/executor/voice-mode-stopped](templates/voice/executor/voice-mode-stopped.hbs) | `VOICE_EXECUTOR_VOICE_MODE_STOPPED` |
| `ilium-client/src/control/resolver.rs:16` | [voice/resolver/no-ilium-node-has-id](templates/voice/resolver/no-ilium-node-has-id.hbs) | `VOICE_RESOLVER_NO_ILIUM_NODE_HAS_ID` |
| `ilium-client/src/control/resolver.rs:72` | [voice/resolver/node-v0-cannot-contain-ordinary-ilium-items](templates/voice/resolver/node-v0-cannot-contain-ordinary-ilium-items.hbs) | `VOICE_RESOLVER_NODE_V0_CANNOT_CONTAIN_ORDINARY_ILIUM_ITEMS` |
| `ilium-client/src/control/resolver.rs:142` | [voice/resolver/no-ilium-node-is-named](templates/voice/resolver/no-ilium-node-is-named.hbs) | `VOICE_RESOLVER_NO_ILIUM_NODE_IS_NAMED` |
| `ilium-client/src/control/resolver.rs:144` | [voice/resolver/the-name-v0-is-ambiguous-use-one](templates/voice/resolver/the-name-v0-is-ambiguous-use-one.hbs) | `VOICE_RESOLVER_THE_NAME_V0_IS_AMBIGUOUS_USE_ONE` |
| `ilium-client/src/control/resolver.rs:165` | [voice/resolver/v0-is-not-a-container](templates/voice/resolver/v0-is-not-a-container.hbs) | `VOICE_RESOLVER_V0_IS_NOT_A_CONTAINER` |
| `ilium-client/src/control/resolver.rs:184` | [voice/resolver/no-child-named-v0-exists-under](templates/voice/resolver/no-child-named-v0-exists-under.hbs) | `VOICE_RESOLVER_NO_CHILD_NAMED_V0_EXISTS_UNDER` |
| `ilium-client/src/control/resolver.rs:190` | [voice/resolver/path-component-v0-is-ambiguous-under](templates/voice/resolver/path-component-v0-is-ambiguous-under.hbs) | `VOICE_RESOLVER_PATH_COMPONENT_V0_IS_AMBIGUOUS_UNDER` |
| `ilium-client/src/control/resolver.rs:45` | [voice/resolver/no-active-or-selected-ilium-node](templates/voice/resolver/no-active-or-selected-ilium-node.hbs) | `VOICE_RESOLVER_NO_ACTIVE_OR_SELECTED_ILIUM_NODE` |
| `ilium-client/src/control/resolver.rs:60` | [voice/resolver/no-destination-group-is-available](templates/voice/resolver/no-destination-group-is-available.hbs) | `VOICE_RESOLVER_NO_DESTINATION_GROUP_IS_AVAILABLE` |
| `ilium-client/src/control/settings.rs:40` | [voice/settings/updated](templates/voice/settings/updated.hbs) | `VOICE_SETTINGS_UPDATED` |
| `ilium-client/src/control/settings.rs:46` | [voice/settings/adjusted](templates/voice/settings/adjusted.hbs) | `VOICE_SETTINGS_ADJUSTED` |
| `ilium-client/src/control/settings.rs:183` | [voice/settings/unknown-configurable-icon](templates/voice/settings/unknown-configurable-icon.hbs) | `VOICE_SETTINGS_UNKNOWN_CONFIGURABLE_ICON` |
| `ilium-client/src/control/settings.rs:306` | [voice/settings/unknown-keyboard-action](templates/voice/settings/unknown-keyboard-action.hbs) | `VOICE_SETTINGS_UNKNOWN_KEYBOARD_ACTION` |
| `ilium-client/src/control/settings.rs:396` | [voice/settings/unknown-trigger-event](templates/voice/settings/unknown-trigger-event.hbs) | `VOICE_SETTINGS_UNKNOWN_TRIGGER_EVENT` |
| `ilium-client/src/control/settings.rs:404` | [voice/settings/unknown-trigger-action](templates/voice/settings/unknown-trigger-action.hbs) | `VOICE_SETTINGS_UNKNOWN_TRIGGER_ACTION` |
| `ilium-client/src/control/settings.rs:426` | [voice/settings/kilo-gateway-model-v0-is-not-in](templates/voice/settings/kilo-gateway-model-v0-is-not-in.hbs) | `VOICE_SETTINGS_KILO_GATEWAY_MODEL_V0_IS_NOT_IN` |
| `ilium-client/src/control/settings.rs:609` | [voice/settings/unknown-or-read-only-setting-path](templates/voice/settings/unknown-or-read-only-setting-path.hbs) | `VOICE_SETTINGS_UNKNOWN_OR_READ_ONLY_SETTING_PATH` |
| `ilium-client/src/control/settings.rs:613` | [voice/settings/unknown-or-read-only-setting-path](templates/voice/settings/unknown-or-read-only-setting-path.hbs) | `VOICE_SETTINGS_UNKNOWN_OR_READ_ONLY_SETTING_PATH` |
| `ilium-client/src/control/settings.rs:689` | [voice/settings/setting-v0-is-not-adjustable-use-set](templates/voice/settings/setting-v0-is-not-adjustable-use-set.hbs) | `VOICE_SETTINGS_SETTING_V0_IS_NOT_ADJUSTABLE_USE_SET` |
| `ilium-client/src/control/settings.rs:31` | [voice/settings/current-redacted-ilium-settings](templates/voice/settings/current-redacted-ilium-settings.hbs) | `VOICE_SETTINGS_CURRENT_REDACTED_ILIUM_SETTINGS` |
| `ilium-client/src/control/settings.rs:37` | [voice/settings/path-is-required](templates/voice/settings/path-is-required.hbs) | `VOICE_SETTINGS_PATH_IS_REQUIRED` |
| `ilium-client/src/control/settings.rs:38` | [voice/settings/value-is-required](templates/voice/settings/value-is-required.hbs) | `VOICE_SETTINGS_VALUE_IS_REQUIRED` |
| `ilium-client/src/control/settings.rs:43` | [voice/settings/path-is-required](templates/voice/settings/path-is-required.hbs) | `VOICE_SETTINGS_PATH_IS_REQUIRED` |
| `ilium-client/src/control/settings.rs:44` | [voice/executor/direction-is-required](templates/voice/executor/direction-is-required.hbs) | `VOICE_EXECUTOR_DIRECTION_IS_REQUIRED` |
| `ilium-client/src/control/settings.rs:51` | [voice/settings/started-the-inference-provider-test](templates/voice/settings/started-the-inference-provider-test.hbs) | `VOICE_SETTINGS_STARTED_THE_INFERENCE_PROVIDER_TEST` |
| `ilium-client/src/control/settings.rs:56` | [voice/settings/started-model-discovery](templates/voice/settings/started-model-discovery.hbs) | `VOICE_SETTINGS_STARTED_MODEL_DISCOVERY` |
| `ilium-client/src/control/settings.rs:60` | [voice/settings/requested-a-sound-preview](templates/voice/settings/requested-a-sound-preview.hbs) | `VOICE_SETTINGS_REQUESTED_A_SOUND_PREVIEW` |
| `ilium-client/src/control/settings.rs:77` | [voice/settings/unfocused-width-cannot-exceed-focused-width](templates/voice/settings/unfocused-width-cannot-exceed-focused-width.hbs) | `VOICE_SETTINGS_UNFOCUSED_WIDTH_CANNOT_EXCEED_FOCUSED_WIDTH` |
| `ilium-client/src/control/settings.rs:84` | [voice/settings/focused-width-cannot-be-smaller-than-unfocused](templates/voice/settings/focused-width-cannot-be-smaller-than-unfocused.hbs) | `VOICE_SETTINGS_FOCUSED_WIDTH_CANNOT_BE_SMALLER_THAN_UNFOCUSED` |
| `ilium-client/src/control/settings.rs:90` | [voice/settings/minimum-terminal-width-is-outside-ilium-s](templates/voice/settings/minimum-terminal-width-is-outside-ilium-s.hbs) | `VOICE_SETTINGS_MINIMUM_TERMINAL_WIDTH_IS_OUTSIDE_ILIUM_S` |
| `ilium-client/src/control/settings.rs:94` | [voice/settings/minimum-terminal-width-is-outside-ilium-s](templates/voice/settings/minimum-terminal-width-is-outside-ilium-s.hbs) | `VOICE_SETTINGS_MINIMUM_TERMINAL_WIDTH_IS_OUTSIDE_ILIUM_S` |
| `ilium-client/src/control/settings.rs:120` | [voice/settings/color-scheme-must-be-dark-or-light](templates/voice/settings/color-scheme-must-be-dark-or-light.hbs) | `VOICE_SETTINGS_COLOR_SCHEME_MUST_BE_DARK_OR_LIGHT` |
| `ilium-client/src/control/settings.rs:151` | [voice/settings/task-progress-style-must-be-braille-blocks](templates/voice/settings/task-progress-style-must-be-braille-blocks.hbs) | `VOICE_SETTINGS_TASK_PROGRESS_STYLE_MUST_BE_BRAILLE_BLOCKS` |
| `ilium-client/src/control/settings.rs:188` | [voice/settings/scrollback-budget-is-too-large](templates/voice/settings/scrollback-budget-is-too-large.hbs) | `VOICE_SETTINGS_SCROLLBACK_BUDGET_IS_TOO_LARGE` |
| `ilium-client/src/control/settings.rs:194` | [voice/settings/scrollback-budget-must-be-4-512-mib](templates/voice/settings/scrollback-budget-must-be-4-512-mib.hbs) | `VOICE_SETTINGS_SCROLLBACK_BUDGET_MUST_BE_4_512_MIB` |
| `ilium-client/src/control/settings.rs:258` | [voice/settings/autosave-delay-is-too-large](templates/voice/settings/autosave-delay-is-too-large.hbs) | `VOICE_SETTINGS_AUTOSAVE_DELAY_IS_TOO_LARGE` |
| `ilium-client/src/control/settings.rs:260` | [voice/settings/autosave-delay-must-be-250-500-1000](templates/voice/settings/autosave-delay-must-be-250-500-1000.hbs) | `VOICE_SETTINGS_AUTOSAVE_DELAY_MUST_BE_250_500_1000` |
| `ilium-client/src/control/settings.rs:289` | [voice/settings/shortcut-base-must-be-one-ascii-letter](templates/voice/settings/shortcut-base-must-be-one-ascii-letter.hbs) | `VOICE_SETTINGS_SHORTCUT_BASE_MUST_BE_ONE_ASCII_LETTER` |
| `ilium-client/src/control/settings.rs:296` | [voice/settings/keyboard-preset-must-be-screen-or-tmux](templates/voice/settings/keyboard-preset-must-be-screen-or-tmux.hbs) | `VOICE_SETTINGS_KEYBOARD_PRESET_MUST_BE_SCREEN_OR_TMUX` |
| `ilium-client/src/control/settings.rs:308` | [voice/settings/keyboard-binding-must-be-one-printable-key](templates/voice/settings/keyboard-binding-must-be-one-printable-key.hbs) | `VOICE_SETTINGS_KEYBOARD_BINDING_MUST_BE_ONE_PRINTABLE_KEY` |
| `ilium-client/src/control/settings.rs:319` | [voice/settings/keyboard-binding-was-rejected](templates/voice/settings/keyboard-binding-was-rejected.hbs) | `VOICE_SETTINGS_KEYBOARD_BINDING_WAS_REJECTED` |
| `ilium-client/src/control/settings.rs:324` | [voice/settings/card-preview-line-count-is-too-large](templates/voice/settings/card-preview-line-count-is-too-large.hbs) | `VOICE_SETTINGS_CARD_PREVIEW_LINE_COUNT_IS_TOO_LARGE` |
| `ilium-client/src/control/settings.rs:326` | [voice/settings/card-preview-lines-must-be-between-1](templates/voice/settings/card-preview-lines-must-be-between-1.hbs) | `VOICE_SETTINGS_CARD_PREVIEW_LINES_MUST_BE_BETWEEN_1` |
| `ilium-client/src/control/settings.rs:339` | [voice/settings/column-width-is-too-large](templates/voice/settings/column-width-is-too-large.hbs) | `VOICE_SETTINGS_COLUMN_WIDTH_IS_TOO_LARGE` |
| `ilium-client/src/control/settings.rs:341` | [voice/settings/minimum-column-width-must-be-between-10](templates/voice/settings/minimum-column-width-must-be-between-10.hbs) | `VOICE_SETTINGS_MINIMUM_COLUMN_WIDTH_MUST_BE_BETWEEN_10` |
| `ilium-client/src/control/settings.rs:356` | [voice/settings/sound-source-must-be-system-beep-or](templates/voice/settings/sound-source-must-be-system-beep-or.hbs) | `VOICE_SETTINGS_SOUND_SOURCE_MUST_BE_SYSTEM_BEEP_OR` |
| `ilium-client/src/control/settings.rs:369` | [voice/settings/sound-file-is-not-in-ilium-s](templates/voice/settings/sound-file-is-not-in-ilium-s.hbs) | `VOICE_SETTINGS_SOUND_FILE_IS_NOT_IN_ILIUM_S` |
| `ilium-client/src/control/settings.rs:399` | [voice/settings/trigger-actions-must-be-an-array-of](templates/voice/settings/trigger-actions-must-be-an-array-of.hbs) | `VOICE_SETTINGS_TRIGGER_ACTIONS_MUST_BE_AN_ARRAY_OF` |
| `ilium-client/src/control/settings.rs:520` | [voice/settings/voice-output-volume-must-be-between-0](templates/voice/settings/voice-output-volume-must-be-between-0.hbs) | `VOICE_SETTINGS_VOICE_OUTPUT_VOLUME_MUST_BE_BETWEEN_0` |
| `ilium-client/src/control/settings.rs:522` | [voice/settings/voice-output-volume-must-be-between-0](templates/voice/settings/voice-output-volume-must-be-between-0.hbs) | `VOICE_SETTINGS_VOICE_OUTPUT_VOLUME_MUST_BE_BETWEEN_0` |
| `ilium-client/src/control/settings.rs:552` | [voice/settings/reset-planning-time-style-must-be-exact](templates/voice/settings/reset-planning-time-style-must-be-exact.hbs) | `VOICE_SETTINGS_RESET_PLANNING_TIME_STYLE_MUST_BE_EXACT` |
| `ilium-client/src/control/settings.rs:572` | [voice/settings/git-default-where-must-be-here-new](templates/voice/settings/git-default-where-must-be-here-new.hbs) | `VOICE_SETTINGS_GIT_DEFAULT_WHERE_MUST_BE_HERE_NEW` |
| `ilium-client/src/control/settings.rs:586` | [voice/settings/git-default-base-must-be-current-or](templates/voice/settings/git-default-base-must-be-current-or.hbs) | `VOICE_SETTINGS_GIT_DEFAULT_BASE_MUST_BE_CURRENT_OR` |
| `ilium-client/src/control/settings.rs:594` | [voice/settings/git-branch-line-must-be-worktree-only](templates/voice/settings/git-branch-line-must-be-worktree-only.hbs) | `VOICE_SETTINGS_GIT_BRANCH_LINE_MUST_BE_WORKTREE_ONLY` |
| `ilium-client/src/control/settings.rs:604` | [voice/settings/git-default-close-policy-must-be-keep](templates/voice/settings/git-default-close-policy-must-be-keep.hbs) | `VOICE_SETTINGS_GIT_DEFAULT_CLOSE_POLICY_MUST_BE_KEEP` |
| `ilium-client/src/control/settings.rs:734` | [voice/settings/setting-registry-could-not-reach-the-requested](templates/voice/settings/setting-registry-could-not-reach-the-requested.hbs) | `VOICE_SETTINGS_SETTING_REGISTRY_COULD_NOT_REACH_THE_REQUESTED` |
| `ilium-client/src/control/settings.rs:740` | [voice/settings/value-must-be-a-boolean](templates/voice/settings/value-must-be-a-boolean.hbs) | `VOICE_SETTINGS_VALUE_MUST_BE_A_BOOLEAN` |
| `ilium-client/src/control/settings.rs:746` | [voice/settings/value-must-be-a-non-negative-integer](templates/voice/settings/value-must-be-a-non-negative-integer.hbs) | `VOICE_SETTINGS_VALUE_MUST_BE_A_NON_NEGATIVE_INTEGER` |
| `ilium-client/src/control/settings.rs:752` | [voice/settings/value-must-be-a-string](templates/voice/settings/value-must-be-a-string.hbs) | `VOICE_SETTINGS_VALUE_MUST_BE_A_STRING` |
| `ilium-client/src/control/settings.rs:769` | [voice/settings/left-panel-width-is-outside-ilium-s](templates/voice/settings/left-panel-width-is-outside-ilium-s.hbs) | `VOICE_SETTINGS_LEFT_PANEL_WIDTH_IS_OUTSIDE_ILIUM_S` |
| `ilium-client/src/control/settings.rs:771` | [voice/settings/left-panel-width-is-outside-ilium-s](templates/voice/settings/left-panel-width-is-outside-ilium-s.hbs) | `VOICE_SETTINGS_LEFT_PANEL_WIDTH_IS_OUTSIDE_ILIUM_S` |
| `ilium-client/src/control/settings.rs:784` | [voice/settings/left-panel-sizing-mode-must-be-fixed](templates/voice/settings/left-panel-sizing-mode-must-be-fixed.hbs) | `VOICE_SETTINGS_LEFT_PANEL_SIZING_MODE_MUST_BE_FIXED` |
| `ilium-client/src/control/settings.rs:797` | [voice/settings/invalid-tree-order](templates/voice/settings/invalid-tree-order.hbs) | `VOICE_SETTINGS_INVALID_TREE_ORDER` |
| `ilium-client/src/control/settings.rs:807` | [voice/settings/invalid-agent-identifier-mode](templates/voice/settings/invalid-agent-identifier-mode.hbs) | `VOICE_SETTINGS_INVALID_AGENT_IDENTIFIER_MODE` |
| `ilium-client/src/control/settings.rs:816` | [voice/settings/motion-level-must-be-full-reduced-or](templates/voice/settings/motion-level-must-be-full-reduced-or.hbs) | `VOICE_SETTINGS_MOTION_LEVEL_MUST_BE_FULL_REDUCED_OR` |
| `ilium-client/src/control/settings.rs:825` | [voice/settings/sidebar-density-must-be-compact-standard-or](templates/voice/settings/sidebar-density-must-be-compact-standard-or.hbs) | `VOICE_SETTINGS_SIDEBAR_DENSITY_MUST_BE_COMPACT_STANDARD_OR` |
| `ilium-client/src/control/settings.rs:834` | [voice/settings/invalid-new-pane-directory-policy](templates/voice/settings/invalid-new-pane-directory-policy.hbs) | `VOICE_SETTINGS_INVALID_NEW_PANE_DIRECTORY_POLICY` |
| `ilium-client/src/control/settings.rs:843` | [voice/settings/invalid-session-recovery-policy](templates/voice/settings/invalid-session-recovery-policy.hbs) | `VOICE_SETTINGS_INVALID_SESSION_RECOVERY_POLICY` |
| `ilium-client/src/control/settings.rs:854` | [voice/settings/invalid-inference-provider](templates/voice/settings/invalid-inference-provider.hbs) | `VOICE_SETTINGS_INVALID_INFERENCE_PROVIDER` |
| `ilium-client/src/control/settings.rs:862` | [voice/settings/invalid-title-style](templates/voice/settings/invalid-title-style.hbs) | `VOICE_SETTINGS_INVALID_TITLE_STYLE` |
| `ilium-client/src/control/settings.rs:871` | [voice/settings/invalid-realtime-voice-model](templates/voice/settings/invalid-realtime-voice-model.hbs) | `VOICE_SETTINGS_INVALID_REALTIME_VOICE_MODEL` |
| `ilium-client/src/control/settings.rs:879` | [voice/settings/invalid-realtime-voice](templates/voice/settings/invalid-realtime-voice.hbs) | `VOICE_SETTINGS_INVALID_REALTIME_VOICE` |
| `ilium-client/src/control/settings.rs:887` | [voice/settings/reasoning-effort-must-be-minimal-low-or](templates/voice/settings/reasoning-effort-must-be-minimal-low-or.hbs) | `VOICE_SETTINGS_REASONING_EFFORT_MUST_BE_MINIMAL_LOW_OR` |
| `ilium-client/src/control/settings.rs:894` | [voice/settings/voice-input-mode-must-be-semantic-vad](templates/voice/settings/voice-input-mode-must-be-semantic-vad.hbs) | `VOICE_SETTINGS_VOICE_INPUT_MODE_MUST_BE_SEMANTIC_VAD` |
| `ilium-client/src/control/settings.rs:903` | [voice/settings/vad-eagerness-must-be-auto-low-medium](templates/voice/settings/vad-eagerness-must-be-auto-low-medium.hbs) | `VOICE_SETTINGS_VAD_EAGERNESS_MUST_BE_AUTO_LOW_MEDIUM` |
| `ilium-client/src/agent_feature_setup.rs:184` | [agent/managed-block](templates/agent/managed-block.hbs) | `MANAGED_BLOCK` |
| `ilium-client/src/agent_from_line.rs:162` | [agent/goal-from-line](templates/agent/goal-from-line.hbs) | `GOAL_FROM_LINE` |
| `ilium-client/src/chatroom.rs:119` | [agent/chatroom-record-row](templates/agent/chatroom-record-row.hbs) | `CHATROOM_RECORD_ROW` |
| `ilium-client/src/chatroom.rs:148` | [agent/chatroom-empty-context](templates/agent/chatroom-empty-context.hbs) | `CHATROOM_EMPTY_CONTEXT` |
| `ilium-client/src/chatroom.rs:152` | [agent/chatroom-context](templates/agent/chatroom-context.hbs) | `CHATROOM_CONTEXT` |
| `ilium-client/src/chatroom.rs:156` | [agent/chatroom-context-row](templates/agent/chatroom-context-row.hbs) | `CHATROOM_CONTEXT_ROW` |
| `ilium-client/src/chatroom.rs:187` | [agent/chatroom-title](templates/agent/chatroom-title.hbs) | `CHATROOM_TITLE` |
| `ilium-client/src/chatroom.rs:190` | [agent/chatroom-header-guidance](templates/agent/chatroom-header-guidance.hbs) | `CHATROOM_HEADER_GUIDANCE` |
| `ilium-client/src/chatroom.rs:192` | [agent/chatroom-messages-heading](templates/agent/chatroom-messages-heading.hbs) | `CHATROOM_MESSAGES_HEADING` |
| `ilium-server/src/agent_delivery.rs:137` | [agent/progress-done](templates/agent/progress-done.hbs) | `PROGRESS_DONE` |
| `ilium-server/src/agent_delivery.rs:143` | [agent/progress-error](templates/agent/progress-error.hbs) | `PROGRESS_ERROR` |
| `ilium-server/src/agent_delivery.rs:148` | [agent/progress-unspecified-error](templates/agent/progress-unspecified-error.hbs) | `PROGRESS_UNSPECIFIED_ERROR` |
| `ilium-server/src/agent_delivery.rs:151` | [agent/progress-unknown](templates/agent/progress-unknown.hbs) | `PROGRESS_UNKNOWN` |
| `ilium-server/src/agent_delivery.rs:162` | [agent/progress-monitor-failure](templates/agent/progress-monitor-failure.hbs) | `PROGRESS_MONITOR_FAILURE` |
| `ilium-server/src/agent_delivery.rs:399` | [agent/progress-empty-status](templates/agent/progress-empty-status.hbs) | `PROGRESS_EMPTY_STATUS` |
| `ilium-session-convert/src/claude_writer.rs:317` | [conversion/truncated-result](templates/conversion/truncated-result.hbs) | `TRUNCATED_RESULT` |
| `ilium-session-convert/src/claude_writer.rs:373` | [conversion/bash-description-workdir](templates/conversion/bash-description-workdir.hbs) | `BASH_DESCRIPTION_WORKDIR` |
| `ilium-session-convert/src/claude_writer.rs:374` | [conversion/bash-description](templates/conversion/bash-description.hbs) | `BASH_DESCRIPTION` |
| `ilium-client/src/app.rs close_confirmation_message` | [naming/close-container](templates/naming/close-container.hbs) | `render by name` |
| `ilium-client/src/app.rs close_confirmation_message` | [naming/close-dirty-editor](templates/naming/close-dirty-editor.hbs) | `render by name` |
| `ilium-server/src/ipc/handlers.rs` | [naming/progress-restoration-failure](templates/naming/progress-restoration-failure.hbs) | `render by name` |
| `ilium-client/src/restructure.rs format_transcript_entries` | [naming/transcript-row](templates/naming/transcript-row.hbs) | `render by name` |
| `ilium-client/src/restructure.rs clip_restructure_evidence` | [naming/clipped-context](templates/naming/clipped-context.hbs) | `render by name` |
| `ilium-client/src/naming.rs encode_untrusted_context` | [naming/unavailable-context](templates/naming/unavailable-context.hbs) | `render by name` |
| `ilium-server/src/ipc/handlers.rs monitor restoration fallback` | [agent/progress-observation-stopped](templates/agent/progress-observation-stopped.hbs) | `PROGRESS_OBSERVATION_STOPPED` |
| `ilium-remote-compaction/src/technique.rs system prompt guard shared by every technique` | [compaction/guard](templates/compaction/guard.hbs) | `GUARD` |
| `ilium-remote-compaction/src/technique.rs Claude Code technique system prompt` | [compaction/claude-code-system](templates/compaction/claude-code-system.hbs) | `CLAUDE_CODE_SYSTEM` |
| `ilium-remote-compaction/src/technique.rs Codex technique system prompt` | [compaction/codex-system](templates/compaction/codex-system.hbs) | `CODEX_SYSTEM` |
| `ilium-remote-compaction/src/write_codex.rs Codex summary prefix` | [compaction/codex-summary-prefix](templates/compaction/codex-summary-prefix.hbs) | `CODEX_SUMMARY_PREFIX` |
| `ilium-remote-compaction/src/technique.rs opencode technique system prompt` | [compaction/opencode-system](templates/compaction/opencode-system.hbs) | `OPENCODE_SYSTEM` |
| `ilium-remote-compaction/src/technique.rs Gemini CLI technique system prompt` | [compaction/gemini-cli-system](templates/compaction/gemini-cli-system.hbs) | `GEMINI_CLI_SYSTEM` |
| `ilium-remote-compaction/src/technique.rs Best of all worlds technique system prompt` | [compaction/best-of-all-worlds-system](templates/compaction/best-of-all-worlds-system.hbs) | `BEST_OF_ALL_WORLDS_SYSTEM` |
| `ilium-remote-compaction/src/technique.rs Custom technique system prompt` | [compaction/custom-system](templates/compaction/custom-system.hbs) | `CUSTOM_SYSTEM` |
| `ilium-remote-compaction/src/technique.rs chunk user message` | [compaction/user](templates/compaction/user.hbs) | `USER` |
| `ilium-remote-compaction/src/technique.rs merge user message` | [compaction/merge-user](templates/compaction/merge-user.hbs) | `MERGE_USER` |
| `ilium-remote-compaction/src/summarize.rs retry note` | [compaction/retry-note](templates/compaction/retry-note.hbs) | `RETRY_NOTE` |
| `ilium-remote-compaction/src/write_claude.rs summary record wrapper` | [compaction/claude-summary-wrapper](templates/compaction/claude-summary-wrapper.hbs) | `CLAUDE_SUMMARY_WRAPPER` |
| `ilium-remote-compaction/src/input.rs deterministic ledger layout` | [compaction/ledger](templates/compaction/ledger.hbs) | `LEDGER` |
| `ilium-remote-compaction/src/summarize.rs deterministic fallback summary` | [compaction/fallback-summary](templates/compaction/fallback-summary.hbs) | `FALLBACK_SUMMARY` |
| `ilium-remote-compaction/src/write_codex.rs retained-tail activity note` | [compaction/tail-activity](templates/compaction/tail-activity.hbs) | `TAIL_ACTIVITY` |
| `ilium-remote-compaction/src/input.rs omitted tool-result marker` | [compaction/tool-result-omitted](templates/compaction/tool-result-omitted.hbs) | `TOOL_RESULT_OMITTED` |
| `ilium-remote-compaction/src/input.rs truncated turn marker` | [compaction/turn-truncated](templates/compaction/turn-truncated.hbs) | `TURN_TRUNCATED` |
| `ilium-remote-compaction/src/neutral.rs image placeholder` | [compaction/image-placeholder](templates/compaction/image-placeholder.hbs) | `IMAGE_PLACEHOLDER` |
| `ilium-remote-compaction/src/neutral.rs encrypted earlier compaction note` | [compaction/opaque-compaction-note](templates/compaction/opaque-compaction-note.hbs) | `OPAQUE_COMPACTION_NOTE` |
| `ilium-remote-compaction/src/summarize.rs ledger appendix` | [compaction/ledger-appendix](templates/compaction/ledger-appendix.hbs) | `LEDGER_APPENDIX` |
