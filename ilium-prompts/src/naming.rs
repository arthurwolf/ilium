//! Compile-time embedded naming prompt catalog.

pub const PROJECT_NAME: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/project-name.hbs"
));
pub const LABEL_INSTRUCTIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/label-instructions.hbs"
));
pub const SESSION_SUMMARY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/session-summary.hbs"
));
pub const SESSION_TITLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/session-title.hbs"
));
pub const TERMINAL_SUMMARY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/terminal-summary.hbs"
));
pub const TERMINAL_TITLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/terminal-title.hbs"
));
pub const RESTRUCTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure.hbs"
));
pub const INFERENCE_TEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/inference-test.hbs"
));
pub const SESSION_LABEL_EXAMPLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/session-label-example.hbs"
));
pub const SESSION_SUMMARY_EXAMPLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/session-summary-example.hbs"
));
pub const UNAVAILABLE_PROCESS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/unavailable-process.hbs"
));
pub const ABSENT_CONTEXT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/absent-context.hbs"
));
pub const TERMINAL_LABEL_EXAMPLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/terminal-label-example.hbs"
));
pub const TERMINAL_SUMMARY_EXAMPLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/terminal-summary-example.hbs"
));
pub const UNAVAILABLE_TRANSCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/unavailable-transcript.hbs"
));
pub const RESTRUCTURE_SUMMARY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure-summary.hbs"
));
pub const RESTRUCTURE_LABEL_EXAMPLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure-label-example.hbs"
));
pub const RESTRUCTURE_SUMMARY_EXAMPLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure-summary-example.hbs"
));
pub const CONTENT_OMISSION_MARKER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/content-omission-marker.hbs"
));
pub const JSON_ONLY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/json-only.hbs"
));
pub const SMART_COPY_SYSTEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart-copy-system.hbs"
));

pub const NAMING_NAMING_V0_V1_CHARACTERS_OMITTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/naming/v0-v1-characters-omitted.hbs"
));

pub const NAMING_NAMING_V0: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/naming/v0.hbs"
));

pub const NAMING_NAMING_CONTEXT_LABEL_RESPONSE_MISSING_STRING_FIELD_FIELD: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/naming/context-label-response-missing-string-field-field.hbs"
    ));

pub const NAMING_NAMING_CONTEXT_LABEL_RESPONSE_FIELD_FIELD_MUST_BE_ONE_COMPACT_UTF_8_ICON_OR_EMOTICON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/naming/context-label-response-field-field-must-be-one-compact-utf-8-icon-or-emoticon.hbs"));

pub const NAMING_NAMING_CONTEXT_LABEL_RESPONSE_DID_NOT_CONTAIN_ONE_COMPLETE_JSON_OBJECT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/naming/context-label-response-did-not-contain-one-complete-json-object.hbs"));

pub const NAMING_NAMING_CONTEXT_LABEL_RESPONSE_CONTAINED_MULTIPLE_JSON_OBJECTS: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/naming/context-label-response-contained-multiple-json-objects.hbs"
    ));

pub const NAMING_NAMING_CONTEXT_LABEL_RESPONSE_FIELD_FIELD_MUST_CONTAIN_MIN_WORDS_TO_MAX_WORDS_SHORT_NON_EMPT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/naming/context-label-response-field-field-must-contain-min-words-to-max-words-short-non-empt.hbs"));

pub const NAMING_SESSION_NAMING_NO_PROJECT_VERIFIED_TRANSCRIPT_FOUND_FOR_SESSION: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/session_naming/no-project-verified-transcript-found-for-session.hbs"
    ));

pub const NAMING_SESSION_NAMING_NO_USER_TRANSCRIPT_ENTRIES_AVAILABLE_TO_INFER_A_SESSION_TITLE_FROM: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/session_naming/no-user-transcript-entries-available-to-infer-a-session-title-from.hbs"));

pub const NAMING_SESSION_NAMING_SESSION_TITLE_ONLY_NAMES_THE_AGENT_PROVIDER: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/session_naming/session-title-only-names-the-agent-provider.hbs"
    ));

pub const NAMING_SESSION_NAMING_CLAUDE_CODE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/session_naming/claude-code.hbs"
));

pub const NAMING_SESSION_NAMING_USER_SPECIFIED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/session_naming/user-specified.hbs"
));

pub const NAMING_SESSION_NAMING_WAITING_ON_BACKGROUND_TASKS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/session_naming/waiting-on-background-tasks.hbs"
));

pub const NAMING_SESSION_NAMING_A_BACKGROUND_TASK_IS_STILL_FINISHING_UP: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/session_naming/a-background-task-is-still-finishing-up.hbs"
    ));

pub const NAMING_SESSION_NAMING_WAITING_FOR_USER_APPROVAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/session_naming/waiting-for-user-approval.hbs"
));

pub const NAMING_TERMINAL_NAMING_NO_SCREEN_CONTENT_AVAILABLE_TO_INFER_A_TERMINAL_TITLE_FROM: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/terminal_naming/no-screen-content-available-to-infer-a-terminal-title-from.hbs"));

pub const NAMING_PROJECT_NAMING_NOT_PRESENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/project_naming/not-present.hbs"
));

pub const NAMING_RESTRUCTURE_PROJECT_ID_V0_TITLE_V1_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure/project-id-v0-title-v1-source.hbs"
));

pub const CLIPPED_LINES_V1_ID_V2_TITLE_V3_ICON_V4_SOURCE_V5_NAME_FIXED: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/clipped-lines-v1-id-v2-title-v3-icon-v4-source-v5-name-fixed.hbs"
    ));

pub const CLIPPED_LINES_AGENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/clipped-lines-agent.hbs"
));

pub const CLIPPED_LINES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/clipped-lines.hbs"
));

pub const NAMING_RESTRUCTURE_PROJECT_PROJECT_ID_NO_LONGER_EXISTS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure/project-project-id-no-longer-exists.hbs"
));

pub const NAMING_RESTRUCTURE_TREE_CHILD_CHILD_ID_NO_LONGER_EXISTS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure/tree-child-child-id-no-longer-exists.hbs"
));

pub const NAMING_RESTRUCTURE_SPLIT_VIEW_CHILD_ID_HAS_NO_ORIENTATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure/split-view-child-id-has-no-orientation.hbs"
));

pub const NAMING_RESTRUCTURE_SPLIT_VIEW_CHILD_ID_CONTAINS_A_NON_PANE_CHILD: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/restructure/split-view-child-id-contains-a-non-pane-child.hbs"
    ));

pub const NAMING_RESTRUCTURE_PLAIN_SHELL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure/plain-shell.hbs"
));

pub const NAMING_RESTRUCTURE_WAITING_FOR_YOUR_APPROVAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure/waiting-for-your-approval.hbs"
));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_PROMPT_EXCEEDED_THE_MAXIMUM_RESTRUCTURE_PROMPT_CHARACTERS_CHARACTER_SAFET: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-prompt-exceeded-the-maximum-restructure-prompt-characters-character-safet.hbs"));

pub const NAMING_RESTRUCTURE_NO_PRIOR_STRUCTURE_AVAILABLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure/no-prior-structure-available.hbs"
));

pub const NAMING_RESTRUCTURE_NO_PANES_OR_FOLDERS_TO_RESTRUCTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/restructure/no-panes-or-folders-to-restructure.hbs"
));

pub const NAMING_RESTRUCTURE_UNPARSEABLE_RESTRUCTURE_INFERENCE_RESPONSE: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/restructure/unparseable-restructure-inference-response.hbs"
    ));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_HAD_THE_WRONG_JSON_SHAPE_ERROR: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/restructure/restructure-response-had-the-wrong-json-shape-error.hbs"
    ));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_ID_ID_MORE_THAN_ONCE: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/restructure/restructure-response-referenced-id-id-more-than-once.hbs"
    ));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_THE_WRONG_LEAF_SET_MISSING_MISSING_UNEXPECTED_UNEXPEC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-referenced-the-wrong-leaf-set-missing-missing-unexpected-unexpec.hbs"));

pub const NAMING_RESTRUCTURE_PROTECTED_SPLIT_VIEW_CONTEXT_DUPLICATED_ID: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/restructure/protected-split-view-context-duplicated-id.hbs"
    ));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CHANGED_THE_PROTECTED_SPLIT_VIEW_SET_MISSING_MISSING_UNEXPECTED: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-changed-the-protected-split-view-set-missing-missing-unexpected.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_CLAIMED_ID_WAS_A_PANE_BUT_THE_EXISTING_ITEM_IS_A_FOLDE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-node-path-claimed-id-was-a-pane-but-the-existing-item-is-a-folde.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_FOLDER_ID_INSIDE_A_SPLIT_VIEW: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-node-path-placed-folder-id-inside-a-split-view.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_CLAIMED_ID_WAS_A_FOLDER_BUT_THE_EXISTING_ITEM_IS_A_PAN: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-node-path-claimed-id-was-a-folder-but-the-existing-item-is-a-pan.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_A_GROUP_INSIDE_A_SPLIT_VIEW: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-node-path-placed-a-group-inside-a-split-view.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_AN_EXISTING_GROUP_INSIDE_A_SPLIT_VIEW: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-node-path-placed-an-existing-group-inside-a-split-view.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_NESTED_A_SPLIT_VIEW_INSIDE_ANOTHER_SPLIT_VIEW: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-node-path-nested-a-split-view-inside-another-split-view.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_INVENTED_UNKNOWN_SPLIT_VIEW_ID: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-node-path-invented-unknown-split-view-id.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_PROTECTED_SPLIT_VIEW_ID_MORE_THAN_ONCE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-referenced-protected-split-view-id-more-than-once.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_A_NON_PANE_INSIDE_PROTECTED_SPLIT_VIEW_ID: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-node-path-placed-a-non-pane-inside-protected-split-view-id.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CHANGED_PROTECTED_SPLIT_VIEW_ID_PANE_ORDER_OR_MEMBERSHIP_EXPECTE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-changed-protected-split-view-id-pane-order-or-membership-expecte.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_LABEL_TITLE_MUST_CONTAIN_1_TO_7_WORDS_AND_AT_MOST_64_CHARACTERS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-label-title-must-contain-1-to-7-words-and-at-most-64-characters.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_SHORT_LABEL_MUST_CONTAIN_1_TO_3_WORDS_AND_AT_MOST_64_CHARACTERS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-short-label-must-contain-1-to-3-words-and-at-most-64-characters.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_AN_EMPTY_TITLE: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/naming/restructure/restructure-response-contained-an-empty-title.hbs"
    ));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_A_CONTROL_CHARACTER_IN_A_TITLE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-contained-a-control-character-in-a-title.hbs"));

pub const NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_A_CONTROL_CHARACTER_IN_A_SHORT_TITLE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/restructure/restructure-response-contained-a-control-character-in-a-short-title.hbs"));

pub const NAMING_TRANSCRIPT_CONTEXT_V0_EARLIER_V1_ENTRIES_OMITTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/transcript_context/v0-earlier-v1-entries-omitted.hbs"
));

pub const NAMING_TRANSCRIPT_CONTEXT_AGENTS_MD_INSTRUCTIONS_FOR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/transcript_context/agents-md-instructions-for.hbs"
));

pub const NAMING_TRANSCRIPT_CONTEXT_CODEX_INTERNAL_CONTEXT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/transcript_context/codex-internal-context.hbs"
));

pub const NAMING_TRANSCRIPT_CONTEXT_ILIUM_PROGRESS_MONITOR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/transcript_context/ilium-progress-monitor.hbs"
));

pub const NAMING_SMART_COPY_FROZEN_TERMINAL_AND_PROGRAM_DETECTED_SELECTIONS_FOLLOW_AS_JSON_DATA_CELL_COLUMNS_IN_A: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/naming/smart_copy/frozen-terminal-and-program-detected-selections-follow-as-json-data-cell-columns-in-a.hbs"));

pub const NAMING_SMART_COPY_LINE_V0_IS_OUTSIDE_THE_SNAPSHOT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/line-v0-is-outside-the-snapshot.hbs"
));

pub const NAMING_SMART_COPY_LINE_V0_IS_BLANK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/line-v0-is-blank.hbs"
));

pub const NAMING_SMART_COPY_INVALID_WORD_RANGE_ON_LINE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/invalid-word-range-on-line.hbs"
));

pub const NAMING_SMART_COPY_UNKNOWN_STARTING_WORD_ON_LINE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/unknown-starting-word-on-line.hbs"
));

pub const NAMING_SMART_COPY_UNKNOWN_ENDING_WORD_ON_LINE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/unknown-ending-word-on-line.hbs"
));

pub const NAMING_SMART_COPY_V0_VISIBLE_EXCERPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/v0-visible-excerpt.hbs"
));

pub const NAMING_SMART_COPY_VISIBLE_FENCED_BLOCK_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/visible-fenced-block-l.hbs"
));

pub const NAMING_SMART_COPY_CODE_CONTENTS_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/code-contents-l.hbs"
));

pub const NAMING_SMART_COPY_VISIBLE_TABLE_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/visible-table-l.hbs"
));

pub const NAMING_SMART_COPY_TABLE_CELL_L_V0_C: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/table-cell-l-v0-c.hbs"
));

pub const NAMING_SMART_COPY_VISIBLE_TERMINAL_FRAME_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/visible-terminal-frame-l.hbs"
));

pub const NAMING_SMART_COPY_FRAME_CONTENTS_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/frame-contents-l.hbs"
));

pub const NAMING_SMART_COPY_VISIBLE_DIFF_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/visible-diff-l.hbs"
));

pub const NAMING_SMART_COPY_VISIBLE_DIAGNOSTIC_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/visible-diagnostic-l.hbs"
));

pub const NAMING_SMART_COPY_VISIBLE_TREE_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/visible-tree-l.hbs"
));

pub const NAMING_SMART_COPY_VISIBLE_INDENTED_CODE_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/visible-indented-code-l.hbs"
));

pub const NAMING_SMART_COPY_HEADING_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/heading-l.hbs"
));

pub const NAMING_SMART_COPY_VISIBLE_SECTION_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/visible-section-l.hbs"
));

pub const NAMING_SMART_COPY_PROMPT_LINE_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/prompt-line-l.hbs"
));

pub const NAMING_SMART_COPY_VISIBLE_V0_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/visible-v0-l.hbs"
));

pub const NAMING_SMART_COPY_VISIBLE_PARAGRAPH_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/visible-paragraph-l.hbs"
));

pub const NAMING_SMART_COPY_INLINE_CODE_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/inline-code-l.hbs"
));

pub const NAMING_SMART_COPY_URL_L: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/url-l.hbs"
));

pub const NAMING_SMART_COPY_INVALID_JSONL_CANDIDATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/invalid-jsonl-candidate.hbs"
));

pub const NAMING_SMART_COPY_CANDIDATE_LABEL_IS_INVALID: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/candidate-label-is-invalid.hbs"
));

pub const NAMING_SMART_COPY_CANDIDATE_KIND_IS_INVALID: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/candidate-kind-is-invalid.hbs"
));

pub const NAMING_SMART_COPY_CANDIDATE_HAS_AN_INVALID_PART_COUNT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/candidate-has-an-invalid-part-count.hbs"
));

pub const NAMING_SMART_COPY_LINE_LIST_IS_EMPTY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/line-list-is-empty.hbs"
));

pub const NAMING_SMART_COPY_CANDIDATE_RESOLVES_ONLY_TO_WHITESPACE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/candidate-resolves-only-to-whitespace.hbs"
));

pub const NAMING_SMART_COPY_LINE_IDS_START_AT_1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/line-ids-start-at-1.hbs"
));

pub const NAMING_SMART_COPY_PS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/ps.hbs"
));

pub const NAMING_SMART_COPY_C: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/c.hbs"
));

pub const NAMING_SMART_COPY_DIFF_GIT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/diff-git.hbs"
));

pub const NAMING_SMART_COPY_A: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/a.hbs"
));

pub const NAMING_SMART_COPY_BEGIN_PATCH: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/begin-patch.hbs"
));

pub const NAMING_SMART_COPY_TRACEBACK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/traceback.hbs"
));

pub const NAMING_SMART_COPY_CAUSED_BY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/caused-by.hbs"
));

pub const NAMING_SMART_COPY_THREAD: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/thread.hbs"
));

pub const NAMING_SMART_COPY_DIFF: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/diff.hbs"
));

pub const NAMING_SMART_COPY_INDEX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/index.hbs"
));

pub const NAMING_SMART_COPY_NEW_FILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/new-file.hbs"
));

pub const NAMING_SMART_COPY_DELETED_FILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/deleted-file.hbs"
));

pub const NAMING_SMART_COPY_AT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/at.hbs"
));

pub const NAMING_SMART_COPY_JSONL_RECORD_EXCEEDS_64_KIB: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/smart_copy/jsonl-record-exceeds-64-kib.hbs"
));

pub const CLOSE_CONTAINER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/close-container.hbs"
));

pub const CLOSE_DIRTY_EDITOR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/close-dirty-editor.hbs"
));

pub const PROGRESS_RESTORATION_FAILURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/progress-restoration-failure.hbs"
));

pub const TRANSCRIPT_ROW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/transcript-row.hbs"
));

pub const CLIPPED_CONTEXT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/clipped-context.hbs"
));

pub const UNAVAILABLE_CONTEXT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/naming/unavailable-context.hbs"
));

pub const TEMPLATES: &[(&str, &str)] = &[
    ("naming/unavailable-context", UNAVAILABLE_CONTEXT),
    ("naming/clipped-context", CLIPPED_CONTEXT),
    ("naming/transcript-row", TRANSCRIPT_ROW),
    ("naming/progress-restoration-failure", PROGRESS_RESTORATION_FAILURE),
    ("naming/close-dirty-editor", CLOSE_DIRTY_EDITOR),
    ("naming/close-container", CLOSE_CONTAINER),
    ("naming/smart_copy/jsonl-record-exceeds-64-kib", NAMING_SMART_COPY_JSONL_RECORD_EXCEEDS_64_KIB),
    ("naming/smart_copy/at", NAMING_SMART_COPY_AT),
    ("naming/smart_copy/deleted-file", NAMING_SMART_COPY_DELETED_FILE),
    ("naming/smart_copy/new-file", NAMING_SMART_COPY_NEW_FILE),
    ("naming/smart_copy/index", NAMING_SMART_COPY_INDEX),
    ("naming/smart_copy/diff", NAMING_SMART_COPY_DIFF),
    ("naming/smart_copy/thread", NAMING_SMART_COPY_THREAD),
    ("naming/smart_copy/caused-by", NAMING_SMART_COPY_CAUSED_BY),
    ("naming/smart_copy/traceback", NAMING_SMART_COPY_TRACEBACK),
    ("naming/smart_copy/begin-patch", NAMING_SMART_COPY_BEGIN_PATCH),
    ("naming/smart_copy/a", NAMING_SMART_COPY_A),
    ("naming/smart_copy/diff-git", NAMING_SMART_COPY_DIFF_GIT),
    ("naming/smart_copy/c", NAMING_SMART_COPY_C),
    ("naming/smart_copy/ps", NAMING_SMART_COPY_PS),
    ("naming/smart_copy/line-ids-start-at-1", NAMING_SMART_COPY_LINE_IDS_START_AT_1),
    ("naming/smart_copy/candidate-resolves-only-to-whitespace", NAMING_SMART_COPY_CANDIDATE_RESOLVES_ONLY_TO_WHITESPACE),
    ("naming/smart_copy/line-list-is-empty", NAMING_SMART_COPY_LINE_LIST_IS_EMPTY),
    ("naming/smart_copy/candidate-has-an-invalid-part-count", NAMING_SMART_COPY_CANDIDATE_HAS_AN_INVALID_PART_COUNT),
    ("naming/smart_copy/candidate-kind-is-invalid", NAMING_SMART_COPY_CANDIDATE_KIND_IS_INVALID),
    ("naming/smart_copy/candidate-label-is-invalid", NAMING_SMART_COPY_CANDIDATE_LABEL_IS_INVALID),
    ("naming/smart_copy/invalid-jsonl-candidate", NAMING_SMART_COPY_INVALID_JSONL_CANDIDATE),
    ("naming/smart_copy/url-l", NAMING_SMART_COPY_URL_L),
    ("naming/smart_copy/inline-code-l", NAMING_SMART_COPY_INLINE_CODE_L),
    ("naming/smart_copy/visible-paragraph-l", NAMING_SMART_COPY_VISIBLE_PARAGRAPH_L),
    ("naming/smart_copy/visible-v0-l", NAMING_SMART_COPY_VISIBLE_V0_L),
    ("naming/smart_copy/prompt-line-l", NAMING_SMART_COPY_PROMPT_LINE_L),
    ("naming/smart_copy/visible-section-l", NAMING_SMART_COPY_VISIBLE_SECTION_L),
    ("naming/smart_copy/heading-l", NAMING_SMART_COPY_HEADING_L),
    ("naming/smart_copy/visible-indented-code-l", NAMING_SMART_COPY_VISIBLE_INDENTED_CODE_L),
    ("naming/smart_copy/visible-tree-l", NAMING_SMART_COPY_VISIBLE_TREE_L),
    ("naming/smart_copy/visible-diagnostic-l", NAMING_SMART_COPY_VISIBLE_DIAGNOSTIC_L),
    ("naming/smart_copy/visible-diff-l", NAMING_SMART_COPY_VISIBLE_DIFF_L),
    ("naming/smart_copy/frame-contents-l", NAMING_SMART_COPY_FRAME_CONTENTS_L),
    ("naming/smart_copy/visible-terminal-frame-l", NAMING_SMART_COPY_VISIBLE_TERMINAL_FRAME_L),
    ("naming/smart_copy/table-cell-l-v0-c", NAMING_SMART_COPY_TABLE_CELL_L_V0_C),
    ("naming/smart_copy/visible-table-l", NAMING_SMART_COPY_VISIBLE_TABLE_L),
    ("naming/smart_copy/code-contents-l", NAMING_SMART_COPY_CODE_CONTENTS_L),
    ("naming/smart_copy/visible-fenced-block-l", NAMING_SMART_COPY_VISIBLE_FENCED_BLOCK_L),
    ("naming/smart_copy/v0-visible-excerpt", NAMING_SMART_COPY_V0_VISIBLE_EXCERPT),
    ("naming/smart_copy/unknown-ending-word-on-line", NAMING_SMART_COPY_UNKNOWN_ENDING_WORD_ON_LINE),
    ("naming/smart_copy/unknown-starting-word-on-line", NAMING_SMART_COPY_UNKNOWN_STARTING_WORD_ON_LINE),
    ("naming/smart_copy/invalid-word-range-on-line", NAMING_SMART_COPY_INVALID_WORD_RANGE_ON_LINE),
    ("naming/smart_copy/line-v0-is-blank", NAMING_SMART_COPY_LINE_V0_IS_BLANK),
    ("naming/smart_copy/line-v0-is-outside-the-snapshot", NAMING_SMART_COPY_LINE_V0_IS_OUTSIDE_THE_SNAPSHOT),
    ("naming/smart_copy/frozen-terminal-and-program-detected-selections-follow-as-json-data-cell-columns-in-a", NAMING_SMART_COPY_FROZEN_TERMINAL_AND_PROGRAM_DETECTED_SELECTIONS_FOLLOW_AS_JSON_DATA_CELL_COLUMNS_IN_A),
    ("naming/transcript_context/ilium-progress-monitor", NAMING_TRANSCRIPT_CONTEXT_ILIUM_PROGRESS_MONITOR),
    ("naming/transcript_context/codex-internal-context", NAMING_TRANSCRIPT_CONTEXT_CODEX_INTERNAL_CONTEXT),
    ("naming/transcript_context/agents-md-instructions-for", NAMING_TRANSCRIPT_CONTEXT_AGENTS_MD_INSTRUCTIONS_FOR),
    ("naming/transcript_context/v0-earlier-v1-entries-omitted", NAMING_TRANSCRIPT_CONTEXT_V0_EARLIER_V1_ENTRIES_OMITTED),
    ("naming/restructure/restructure-response-contained-a-control-character-in-a-short-title", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_A_CONTROL_CHARACTER_IN_A_SHORT_TITLE),
    ("naming/restructure/restructure-response-contained-a-control-character-in-a-title", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_A_CONTROL_CHARACTER_IN_A_TITLE),
    ("naming/restructure/restructure-response-contained-an-empty-title", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_AN_EMPTY_TITLE),
    ("naming/restructure/restructure-short-label-must-contain-1-to-3-words-and-at-most-64-characters", NAMING_RESTRUCTURE_RESTRUCTURE_SHORT_LABEL_MUST_CONTAIN_1_TO_3_WORDS_AND_AT_MOST_64_CHARACTERS),
    ("naming/restructure/restructure-label-title-must-contain-1-to-7-words-and-at-most-64-characters", NAMING_RESTRUCTURE_RESTRUCTURE_LABEL_TITLE_MUST_CONTAIN_1_TO_7_WORDS_AND_AT_MOST_64_CHARACTERS),
    ("naming/restructure/restructure-response-changed-protected-split-view-id-pane-order-or-membership-expecte", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CHANGED_PROTECTED_SPLIT_VIEW_ID_PANE_ORDER_OR_MEMBERSHIP_EXPECTE),
    ("naming/restructure/restructure-response-node-path-placed-a-non-pane-inside-protected-split-view-id", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_A_NON_PANE_INSIDE_PROTECTED_SPLIT_VIEW_ID),
    ("naming/restructure/restructure-response-referenced-protected-split-view-id-more-than-once", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_PROTECTED_SPLIT_VIEW_ID_MORE_THAN_ONCE),
    ("naming/restructure/restructure-response-node-path-invented-unknown-split-view-id", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_INVENTED_UNKNOWN_SPLIT_VIEW_ID),
    ("naming/restructure/restructure-response-node-path-nested-a-split-view-inside-another-split-view", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_NESTED_A_SPLIT_VIEW_INSIDE_ANOTHER_SPLIT_VIEW),
    ("naming/restructure/restructure-response-node-path-placed-an-existing-group-inside-a-split-view", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_AN_EXISTING_GROUP_INSIDE_A_SPLIT_VIEW),
    ("naming/restructure/restructure-response-node-path-placed-a-group-inside-a-split-view", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_A_GROUP_INSIDE_A_SPLIT_VIEW),
    ("naming/restructure/restructure-response-node-path-claimed-id-was-a-folder-but-the-existing-item-is-a-pan", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_CLAIMED_ID_WAS_A_FOLDER_BUT_THE_EXISTING_ITEM_IS_A_PAN),
    ("naming/restructure/restructure-response-node-path-placed-folder-id-inside-a-split-view", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_FOLDER_ID_INSIDE_A_SPLIT_VIEW),
    ("naming/restructure/restructure-response-node-path-claimed-id-was-a-pane-but-the-existing-item-is-a-folde", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_CLAIMED_ID_WAS_A_PANE_BUT_THE_EXISTING_ITEM_IS_A_FOLDE),
    ("naming/restructure/restructure-response-changed-the-protected-split-view-set-missing-missing-unexpected", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CHANGED_THE_PROTECTED_SPLIT_VIEW_SET_MISSING_MISSING_UNEXPECTED),
    ("naming/restructure/protected-split-view-context-duplicated-id", NAMING_RESTRUCTURE_PROTECTED_SPLIT_VIEW_CONTEXT_DUPLICATED_ID),
    ("naming/restructure/restructure-response-referenced-the-wrong-leaf-set-missing-missing-unexpected-unexpec", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_THE_WRONG_LEAF_SET_MISSING_MISSING_UNEXPECTED_UNEXPEC),
    ("naming/restructure/restructure-response-referenced-id-id-more-than-once", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_ID_ID_MORE_THAN_ONCE),
    ("naming/restructure/restructure-response-had-the-wrong-json-shape-error", NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_HAD_THE_WRONG_JSON_SHAPE_ERROR),
    ("naming/restructure/unparseable-restructure-inference-response", NAMING_RESTRUCTURE_UNPARSEABLE_RESTRUCTURE_INFERENCE_RESPONSE),
    ("naming/restructure/no-panes-or-folders-to-restructure", NAMING_RESTRUCTURE_NO_PANES_OR_FOLDERS_TO_RESTRUCTURE),
    ("naming/restructure/no-prior-structure-available", NAMING_RESTRUCTURE_NO_PRIOR_STRUCTURE_AVAILABLE),
    ("naming/restructure/restructure-prompt-exceeded-the-maximum-restructure-prompt-characters-character-safet", NAMING_RESTRUCTURE_RESTRUCTURE_PROMPT_EXCEEDED_THE_MAXIMUM_RESTRUCTURE_PROMPT_CHARACTERS_CHARACTER_SAFET),
    ("naming/restructure/waiting-for-your-approval", NAMING_RESTRUCTURE_WAITING_FOR_YOUR_APPROVAL),
    ("naming/restructure/plain-shell", NAMING_RESTRUCTURE_PLAIN_SHELL),
    ("naming/restructure/split-view-child-id-contains-a-non-pane-child", NAMING_RESTRUCTURE_SPLIT_VIEW_CHILD_ID_CONTAINS_A_NON_PANE_CHILD),
    ("naming/restructure/split-view-child-id-has-no-orientation", NAMING_RESTRUCTURE_SPLIT_VIEW_CHILD_ID_HAS_NO_ORIENTATION),
    ("naming/restructure/tree-child-child-id-no-longer-exists", NAMING_RESTRUCTURE_TREE_CHILD_CHILD_ID_NO_LONGER_EXISTS),
    ("naming/restructure/project-project-id-no-longer-exists", NAMING_RESTRUCTURE_PROJECT_PROJECT_ID_NO_LONGER_EXISTS),
    ("naming/clipped-lines", CLIPPED_LINES),
    ("naming/clipped-lines-agent", CLIPPED_LINES_AGENT),
    ("naming/clipped-lines-v1-id-v2-title-v3-icon-v4-source-v5-name-fixed", CLIPPED_LINES_V1_ID_V2_TITLE_V3_ICON_V4_SOURCE_V5_NAME_FIXED),
    ("naming/restructure/project-id-v0-title-v1-source", NAMING_RESTRUCTURE_PROJECT_ID_V0_TITLE_V1_SOURCE),
    ("naming/project_naming/not-present", NAMING_PROJECT_NAMING_NOT_PRESENT),
    ("naming/terminal_naming/no-screen-content-available-to-infer-a-terminal-title-from", NAMING_TERMINAL_NAMING_NO_SCREEN_CONTENT_AVAILABLE_TO_INFER_A_TERMINAL_TITLE_FROM),
    ("naming/session_naming/waiting-for-user-approval", NAMING_SESSION_NAMING_WAITING_FOR_USER_APPROVAL),
    ("naming/session_naming/a-background-task-is-still-finishing-up", NAMING_SESSION_NAMING_A_BACKGROUND_TASK_IS_STILL_FINISHING_UP),
    ("naming/session_naming/waiting-on-background-tasks", NAMING_SESSION_NAMING_WAITING_ON_BACKGROUND_TASKS),
    ("naming/session_naming/user-specified", NAMING_SESSION_NAMING_USER_SPECIFIED),
    ("naming/session_naming/claude-code", NAMING_SESSION_NAMING_CLAUDE_CODE),
    ("naming/session_naming/session-title-only-names-the-agent-provider", NAMING_SESSION_NAMING_SESSION_TITLE_ONLY_NAMES_THE_AGENT_PROVIDER),
    ("naming/session_naming/no-user-transcript-entries-available-to-infer-a-session-title-from", NAMING_SESSION_NAMING_NO_USER_TRANSCRIPT_ENTRIES_AVAILABLE_TO_INFER_A_SESSION_TITLE_FROM),
    ("naming/session_naming/no-project-verified-transcript-found-for-session", NAMING_SESSION_NAMING_NO_PROJECT_VERIFIED_TRANSCRIPT_FOUND_FOR_SESSION),
    ("naming/naming/context-label-response-field-field-must-contain-min-words-to-max-words-short-non-empt", NAMING_NAMING_CONTEXT_LABEL_RESPONSE_FIELD_FIELD_MUST_CONTAIN_MIN_WORDS_TO_MAX_WORDS_SHORT_NON_EMPT),
    ("naming/naming/context-label-response-contained-multiple-json-objects", NAMING_NAMING_CONTEXT_LABEL_RESPONSE_CONTAINED_MULTIPLE_JSON_OBJECTS),
    ("naming/naming/context-label-response-did-not-contain-one-complete-json-object", NAMING_NAMING_CONTEXT_LABEL_RESPONSE_DID_NOT_CONTAIN_ONE_COMPLETE_JSON_OBJECT),
    ("naming/naming/context-label-response-field-field-must-be-one-compact-utf-8-icon-or-emoticon", NAMING_NAMING_CONTEXT_LABEL_RESPONSE_FIELD_FIELD_MUST_BE_ONE_COMPACT_UTF_8_ICON_OR_EMOTICON),
    ("naming/naming/context-label-response-missing-string-field-field", NAMING_NAMING_CONTEXT_LABEL_RESPONSE_MISSING_STRING_FIELD_FIELD),
    ("naming/naming/v0", NAMING_NAMING_V0),
    ("naming/naming/v0-v1-characters-omitted", NAMING_NAMING_V0_V1_CHARACTERS_OMITTED),
    ("naming/project-name", PROJECT_NAME),
    ("naming/label-instructions", LABEL_INSTRUCTIONS),
    ("naming/session-summary", SESSION_SUMMARY),
    ("naming/session-title", SESSION_TITLE),
    ("naming/terminal-summary", TERMINAL_SUMMARY),
    ("naming/terminal-title", TERMINAL_TITLE),
    ("naming/restructure", RESTRUCTURE),
    ("naming/inference-test", INFERENCE_TEST),
    ("naming/session-label-example", SESSION_LABEL_EXAMPLE),
    ("naming/session-summary-example", SESSION_SUMMARY_EXAMPLE),
    ("naming/unavailable-process", UNAVAILABLE_PROCESS),
    ("naming/absent-context", ABSENT_CONTEXT),
    ("naming/terminal-label-example", TERMINAL_LABEL_EXAMPLE),
    ("naming/terminal-summary-example", TERMINAL_SUMMARY_EXAMPLE),
    ("naming/unavailable-transcript", UNAVAILABLE_TRANSCRIPT),
    ("naming/restructure-summary", RESTRUCTURE_SUMMARY),
    ("naming/restructure-label-example", RESTRUCTURE_LABEL_EXAMPLE),
    ("naming/restructure-summary-example", RESTRUCTURE_SUMMARY_EXAMPLE),
    ("naming/content-omission-marker", CONTENT_OMISSION_MARKER),
    ("naming/json-only", JSON_ONLY),
    ("naming/smart-copy-system", SMART_COPY_SYSTEM),
];
