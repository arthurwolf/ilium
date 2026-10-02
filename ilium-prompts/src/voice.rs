//! Authored model-facing voice control text, compiled from editable templates.

pub const ONBOARDING_LIGHTBULB: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/onboarding-lightbulb.hbs"
));

pub const VOICE_MOD_SYSTEM_INSTRUCTIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/mod/system-instructions.hbs"
));
pub const VOICE_MOD_UNKNOWN_ILIUM_TOOL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/mod/unknown-ilium-tool.hbs"
));
pub const VOICE_MOD_INVALID_TOOL_ARGUMENTS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/mod/invalid-tool-arguments.hbs"
));
pub const VOICE_MOD_THE_ACTIVE_PANE_IS_CURRENTLY_A_DETECTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/mod/the-active-pane-is-currently-a-detected.hbs"
));
pub const VOICE_MOD_THE_ACTIVE_PANE_IS_NOT_CURRENTLY_A: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/mod/the-active-pane-is-not-currently-a.hbs"
));
pub const VOICE_MOD_ASK_ONLY_THE_EXACT_QUESTION_DO_NOT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/mod/ask-only-the-exact-question-do-not.hbs"
));
pub const VOICE_MOD_THAT_CONFIRMATION_TOKEN_IS_MISSING_EXPIRED_OR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/mod/that-confirmation-token-is-missing-expired-or.hbs"
));
pub const VOICE_TOOLS_INSPECT_THE_CURRENT_ILIUM_SESSION_TREE_FOCUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/inspect-the-current-ilium-session-tree-focus.hbs"
));
pub const VOICE_TOOLS_IMMEDIATELY_STOP_AND_DISABLE_THE_CURRENT_ILIUM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/immediately-stop-and-disable-the-current-ilium.hbs"
));
pub const VOICE_TOOLS_NAVIGATE_GLOBAL_UI_SURFACES_FOCUS_TREE_OR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/navigate-global-ui-surfaces-focus-tree-or.hbs"
));
pub const VOICE_TOOLS_CREATE_OPEN_ORGANIZE_RENAME_MOVE_REPARENT_SPLIT: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/voice/tools/create-open-organize-rename-move-reparent-split.hbs"
    ));
pub const VOICE_TOOLS_PRIMARY_VOICE_DICTATION_ACTION_SEND_THE_EXACT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/primary-voice-dictation-action-send-the-exact.hbs"
));
pub const VOICE_TOOLS_EXACT_TEXT_TO_TYPE_PRESERVING_SLASH_COMMANDS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/exact-text-to-type-preserving-slash-commands.hbs"
));
pub const VOICE_TOOLS_TYPE_EXACT_TEXT_INTO_A_TERMINAL_WITHOUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/type-exact-text-into-a-terminal-without.hbs"
));
pub const VOICE_TOOLS_EXACT_TEXT_TO_LEAVE_VISIBLE_AND_UNSUBMITTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/exact-text-to-leave-visible-and-unsubmitted.hbs"
));
pub const VOICE_TOOLS_CONTROL_NON_DICTATION_TERMINAL_BEHAVIOR_PRESS_A: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/voice/tools/control-non-dictation-terminal-behavior-press-a.hbs"
    ));
pub const VOICE_TOOLS_OPERATE_A_BUILT_IN_EDITOR_PANE_SAVE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/operate-a-built-in-editor-pane-save.hbs"
));
pub const VOICE_TOOLS_OPERATE_EVERY_KANBAN_SURFACE_SELECT_OPEN_CARDS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/operate-every-kanban-surface-select-open-cards.hbs"
));
pub const VOICE_TOOLS_GET_SET_OR_ADJUST_ANY_PERSISTED_SETTING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/get-set-or-adjust-any-persisted-setting.hbs"
));
pub const VOICE_TOOLS_SEARCH_ALL_RETAINED_TERMINAL_OUTPUT_OPEN_EDITOR: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/voice/tools/search-all-retained-terminal-output-open-editor.hbs"
    ));
pub const VOICE_TOOLS_DETACH_RESTART_THE_TUI_CLIENT_RESTART_THE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/detach-restart-the-tui-client-restart-the.hbs"
));
pub const VOICE_TOOLS_CONFIRM_OR_CANCEL_ONE_PENDING_ILIUM_ACTION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/confirm-or-cancel-one-pending-ilium-action.hbs"
));
pub const VOICE_TOOLS_SLASH_SEPARATED_NODE_NAME_PATH_FROM_THE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/tools/slash-separated-node-name-path-from-the.hbs"
));
pub const VOICE_POLICY_RUN_THE_SHELL_COMMAND_V0_IN_A: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/run-the-shell-command-v0-in-a.hbs"
));
pub const VOICE_POLICY_QUEUE_THIS_PROMPT_TO_BE_SUBMITTED_AUTOMATICALLY: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/voice/policy/queue-this-prompt-to-be-submitted-automatically.hbs"
    ));
pub const VOICE_POLICY_BOARD_HAS_NO_COLUMN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/board-has-no-column.hbs"
));
pub const VOICE_POLICY_BOARD_HAS_NO_CARD_V0_IN_COLUMN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/board-has-no-card-v0-in-column.hbs"
));
pub const VOICE_POLICY_PERMANENTLY_DELETE_THE_CARD_V0_FROM_COLUMN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/permanently-delete-the-card-v0-from-column.hbs"
));
pub const VOICE_POLICY_PERMANENTLY_DELETE_THE_COLUMN_V0_AND_ITS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/permanently-delete-the-column-v0-and-its.hbs"
));
pub const VOICE_POLICY_COMMAND_LINE_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/command-line-is-required.hbs"
));
pub const VOICE_POLICY_REPLACE_THE_EDITOR_S_ENTIRE_CURRENT_DOCUMENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/replace-the-editor-s-entire-current-document.hbs"
));
pub const VOICE_POLICY_PRESS_ENTER_AND_SUBMIT_WHAT_YOU_SEE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/press-enter-and-submit-what-you-see.hbs"
));
pub const VOICE_POLICY_SCHEDULE_THIS_TERMINAL_INPUT_FOR_AUTOMATIC_SUBMISSION: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/voice/policy/schedule-this-terminal-input-for-automatic-submission.hbs"
    ));
pub const VOICE_POLICY_QUEUE_THIS_PROMPT_TO_BE_SUBMITTED_AUTOMATICALLY_2: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/voice/policy/queue-this-prompt-to-be-submitted-automatically-2.hbs"
    ));
pub const VOICE_POLICY_RUNS_IS_REQUIRED_AND_MUST_BE_POSITIVE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/runs-is-required-and-must-be-positive.hbs"
));
pub const VOICE_POLICY_QUEUE_THIS_PROMPT_FOR_AUTOMATIC_SUBMISSION_AFTER: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/voice/policy/queue-this-prompt-for-automatic-submission-after.hbs"
    ));
pub const VOICE_POLICY_KILL_THE_ENTIRE_ILIUM_SESSION_AND_EVERY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/kill-the-entire-ilium-session-and-every.hbs"
));
pub const VOICE_POLICY_RESTART_THE_DETACHED_ILIUM_SERVER_AND_TEMPORARILY: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/voice/policy/restart-the-detached-ilium-server-and-temporarily.hbs"
    ));
pub const VOICE_POLICY_CANCELLED_THE_PENDING_ACTION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/cancelled-the-pending-action.hbs"
));
pub const VOICE_POLICY_TARGET_IS_NOT_A_BOARD_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/target-is-not-a-board-pane.hbs"
));
pub const VOICE_POLICY_COLUMN_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/column-is-required.hbs"
));
pub const VOICE_POLICY_CARD_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/card-is-required.hbs"
));
pub const VOICE_POLICY_I_TYPED_IT_INTO_THE_TARGET_TERMINAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/i-typed-it-into-the-target-terminal.hbs"
));
pub const VOICE_POLICY_LEFT_THE_STAGED_TERMINAL_TEXT_VISIBLE_AND: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/policy/left-the-staged-terminal-text-visible-and.hbs"
));
pub const VOICE_EXECUTOR_SEARCH_RESULT_INDEX_V0_DOES_NOT_EXIST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/search-result-index-v0-does-not-exist.hbs"
));
pub const VOICE_EXECUTOR_FOUND_V0_WORKSPACE_RESULTS_FOR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/found-v0-workspace-results-for.hbs"
));
pub const VOICE_EXECUTOR_FOCUSED_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/focused-pane.hbs"
));
pub const VOICE_EXECUTOR_NODE_V0_IS_NOT_A_SPLIT_VIEW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/node-v0-is-not-a-split-view.hbs"
));
pub const VOICE_EXECUTOR_SHOWING_SPLIT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/showing-split.hbs"
));
pub const VOICE_EXECUTOR_OPENED_V0_SETTINGS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/opened-v0-settings.hbs"
));
pub const VOICE_EXECUTOR_INVALID_WORKSPACE_BRANCH: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/invalid-workspace-branch.hbs"
));
pub const VOICE_EXECUTOR_CREATING_A_V0_AGENT_IN_A_GIT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/creating-a-v0-agent-in-a-git.hbs"
));
pub const VOICE_EXECUTOR_CREATING_A_V0_AGENT_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/creating-a-v0-agent-pane.hbs"
));
pub const VOICE_EXECUTOR_A_SPLIT_VIEW_CAN_CONTAIN_AT_MOST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/a-split-view-can-contain-at-most.hbs"
));
pub const VOICE_EXECUTOR_NODE_V0_CANNOT_BE_EXPANDED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/node-v0-cannot-be-expanded.hbs"
));
pub const VOICE_EXECUTOR_NODE_V0_IS_NOT_A_LIVE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/node-v0-is-not-a-live.hbs"
));
pub const VOICE_EXECUTOR_NODE_V0_IS_NOT_A_TERMINAL_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/node-v0-is-not-a-terminal-pane.hbs"
));
pub const VOICE_EXECUTOR_NODE_V0_IS_NOT_AN_EDITOR_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/node-v0-is-not-an-editor-pane.hbs"
));
pub const VOICE_EXECUTOR_V0_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/v0-is-required.hbs"
));
pub const VOICE_EXECUTOR_V0_MUST_NOT_BE_EMPTY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/v0-must-not-be-empty.hbs"
));
pub const VOICE_EXECUTOR_UNKNOWN_SETTINGS_TAB: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/unknown-settings-tab.hbs"
));
pub const VOICE_EXECUTOR_READ_ILIUM_GET_STATE_AFTER_THE_SERVER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/read-ilium-get-state-after-the-server.hbs"
));
pub const VOICE_EXECUTOR_CURRENT_ILIUM_STATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/current-ilium-state.hbs"
));
pub const VOICE_EXECUTOR_QUEUED_TEXT_AND_ENTER_FOR_THE_TERMINAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/queued-text-and-enter-for-the-terminal.hbs"
));
pub const VOICE_EXECUTOR_STAGED_TEXT_IN_THE_TERMINAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/staged-text-in-the-terminal.hbs"
));
pub const VOICE_EXECUTOR_THERE_IS_NO_SELECTED_SEARCH_RESULT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/there-is-no-selected-search-result.hbs"
));
pub const VOICE_EXECUTOR_OPENED_THE_SELECTED_SEARCH_RESULT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/opened-the-selected-search-result.hbs"
));
pub const VOICE_EXECUTOR_CLOSED_WORKSPACE_SEARCH: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/closed-workspace-search.hbs"
));
pub const VOICE_EXECUTOR_FOCUSED_THE_LEFT_TREE_PANEL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/focused-the-left-tree-panel.hbs"
));
pub const VOICE_EXECUTOR_FOCUSED_THE_NEXT_VISIBLE_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/focused-the-next-visible-pane.hbs"
));
pub const VOICE_EXECUTOR_FOCUSED_THE_PREVIOUS_VISIBLE_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/focused-the-previous-visible-pane.hbs"
));
pub const VOICE_EXECUTOR_DIRECTION_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/direction-is-required.hbs"
));
pub const VOICE_EXECUTOR_MOVED_FOCUS_WITHIN_THE_VISIBLE_SPLIT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/moved-focus-within-the-visible-split.hbs"
));
pub const VOICE_EXECUTOR_OPENED_SETTINGS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/opened-settings.hbs"
));
pub const VOICE_EXECUTOR_TAB_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/tab-is-required.hbs"
));
pub const VOICE_EXECUTOR_OPENED_WORKSPACE_SEARCH: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/opened-workspace-search.hbs"
));
pub const VOICE_EXECUTOR_OPENED_HELP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/opened-help.hbs"
));
pub const VOICE_EXECUTOR_CLOSED_THE_CURRENT_OVERLAY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/closed-the-current-overlay.hbs"
));
pub const VOICE_EXECUTOR_CREATING_A_TERMINAL_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/creating-a-terminal-pane.hbs"
));
pub const VOICE_EXECUTOR_PROVIDER_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/provider-is-required.hbs"
));
pub const VOICE_EXECUTOR_WORKSPACE_BASE_MUST_NOT_BE_EMPTY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/workspace-base-must-not-be-empty.hbs"
));
pub const VOICE_EXECUTOR_CREATING_THE_COMMAND_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/creating-the-command-pane.hbs"
));
pub const VOICE_EXECUTOR_OPENING_THE_FILE_IN_AN_EDITOR_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/opening-the-file-in-an-editor-pane.hbs"
));
pub const VOICE_EXECUTOR_ADDING_THE_FOLDER_TO_THE_LEFT_PANEL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/adding-the-folder-to-the-left-panel.hbs"
));
pub const VOICE_EXECUTOR_ADDING_THE_PROJECT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/adding-the-project.hbs"
));
pub const VOICE_EXECUTOR_CHANGING_THE_PROJECT_S_FOLDER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/changing-the-project-s-folder.hbs"
));
pub const VOICE_EXECUTOR_CREATING_THE_GROUP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/creating-the-group.hbs"
));
pub const VOICE_EXECUTOR_BOARD_CREATION_FAILED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/board-creation-failed.hbs"
));
pub const VOICE_EXECUTOR_CREATING_THE_BOARD: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/creating-the-board.hbs"
));
pub const VOICE_EXECUTOR_CREATING_THE_SPLIT_VIEW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/creating-the-split-view.hbs"
));
pub const VOICE_EXECUTOR_RENAMING_THE_TREE_ITEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/renaming-the-tree-item.hbs"
));
pub const VOICE_EXECUTOR_MOVING_THE_TREE_ITEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/moving-the-tree-item.hbs"
));
pub const VOICE_EXECUTOR_REPARENTING_THE_TREE_ITEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/reparenting-the-tree-item.hbs"
));
pub const VOICE_EXECUTOR_CLOSING_THE_TREE_ITEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/closing-the-tree-item.hbs"
));
pub const VOICE_EXECUTOR_TOGGLED_THE_LEFT_PANEL_GROUP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/toggled-the-left-panel-group.hbs"
));
pub const VOICE_EXECUTOR_STARTED_AUTOMATIC_RETITLING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/started-automatic-retitling.hbs"
));
pub const VOICE_EXECUTOR_REQUESTED_PROJECT_RESTRUCTURING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/requested-project-restructuring.hbs"
));
pub const VOICE_EXECUTOR_REVERTING_THE_PROJECT_S_LATEST_RESTRUCTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/reverting-the-project-s-latest-restructure.hbs"
));
pub const VOICE_EXECUTOR_KEY_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/key-is-required.hbs"
));
pub const VOICE_EXECUTOR_SENT_THE_KEY_TO_THE_TERMINAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/sent-the-key-to-the-terminal.hbs"
));
pub const VOICE_EXECUTOR_TARGET_IS_NOT_A_TERMINAL_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/target-is-not-a-terminal-pane.hbs"
));
pub const VOICE_EXECUTOR_SCROLLED_THE_TERMINAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/scrolled-the-terminal.hbs"
));
pub const VOICE_EXECUTOR_RETURNED_TO_LIVE_TERMINAL_OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/returned-to-live-terminal-output.hbs"
));
pub const VOICE_EXECUTOR_SCHEDULED_TERMINAL_INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/scheduled-terminal-input.hbs"
));
pub const VOICE_EXECUTOR_QUEUED_THE_PROMPT_FOR_THE_AGENT_S: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/queued-the-prompt-for-the-agent-s.hbs"
));
pub const VOICE_EXECUTOR_CLEARING_THE_PANE_S_PROMPT_QUEUE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/clearing-the-pane-s-prompt-queue.hbs"
));
pub const VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/target-is-not-an-editor-pane.hbs"
));
pub const VOICE_EXECUTOR_THIS_EDITOR_HAS_NO_FILE_PATH_YET: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/this-editor-has-no-file-path-yet.hbs"
));
pub const VOICE_EXECUTOR_EDITOR_UPDATED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/editor-updated.hbs"
));
pub const VOICE_EXECUTOR_TEXT_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/text-is-required.hbs"
));
pub const VOICE_EXECUTOR_INSERTED_TEXT_INTO_THE_EDITOR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/inserted-text-into-the-editor.hbs"
));
pub const VOICE_EXECUTOR_REPLACED_THE_EDITOR_S_DOCUMENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/replaced-the-editor-s-document.hbs"
));
pub const VOICE_EXECUTOR_LINE_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/line-is-required.hbs"
));
pub const VOICE_EXECUTOR_MOVED_THE_EDITOR_CURSOR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/moved-the-editor-cursor.hbs"
));
pub const VOICE_EXECUTOR_TOGGLED_THE_RENDERED_SOURCE_VIEW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/toggled-the-rendered-source-view.hbs"
));
pub const VOICE_EXECUTOR_TOGGLED_LINE_NUMBERS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/toggled-line-numbers.hbs"
));
pub const VOICE_EXECUTOR_TOGGLED_THE_MINIMAP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/toggled-the-minimap.hbs"
));
pub const VOICE_EXECUTOR_TOGGLED_AUTOSAVE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/toggled-autosave.hbs"
));
pub const VOICE_EXECUTOR_DESTINATION_COLUMN_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/destination-column-is-required.hbs"
));
pub const VOICE_EXECUTOR_CHECKBOX_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/checkbox-is-required.hbs"
));
pub const VOICE_EXECUTOR_BOARD_UPDATED_AND_PERSISTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/board-updated-and-persisted.hbs"
));
pub const VOICE_EXECUTOR_SESSION_LIFECYCLE_REQUEST_QUEUED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/session-lifecycle-request-queued.hbs"
));
pub const VOICE_EXECUTOR_VOICE_MODE_STOPPED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/executor/voice-mode-stopped.hbs"
));
pub const VOICE_RESOLVER_NO_ILIUM_NODE_HAS_ID: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/resolver/no-ilium-node-has-id.hbs"
));
pub const VOICE_RESOLVER_NODE_V0_CANNOT_CONTAIN_ORDINARY_ILIUM_ITEMS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/resolver/node-v0-cannot-contain-ordinary-ilium-items.hbs"
));
pub const VOICE_RESOLVER_NO_ILIUM_NODE_IS_NAMED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/resolver/no-ilium-node-is-named.hbs"
));
pub const VOICE_RESOLVER_THE_NAME_V0_IS_AMBIGUOUS_USE_ONE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/resolver/the-name-v0-is-ambiguous-use-one.hbs"
));
pub const VOICE_RESOLVER_V0_IS_NOT_A_CONTAINER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/resolver/v0-is-not-a-container.hbs"
));
pub const VOICE_RESOLVER_NO_CHILD_NAMED_V0_EXISTS_UNDER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/resolver/no-child-named-v0-exists-under.hbs"
));
pub const VOICE_RESOLVER_PATH_COMPONENT_V0_IS_AMBIGUOUS_UNDER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/resolver/path-component-v0-is-ambiguous-under.hbs"
));
pub const VOICE_RESOLVER_NO_ACTIVE_OR_SELECTED_ILIUM_NODE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/resolver/no-active-or-selected-ilium-node.hbs"
));
pub const VOICE_RESOLVER_NO_DESTINATION_GROUP_IS_AVAILABLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/resolver/no-destination-group-is-available.hbs"
));
pub const VOICE_SETTINGS_UPDATED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/updated.hbs"
));
pub const VOICE_SETTINGS_ADJUSTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/adjusted.hbs"
));
pub const VOICE_SETTINGS_UNKNOWN_CONFIGURABLE_ICON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/unknown-configurable-icon.hbs"
));
pub const VOICE_SETTINGS_UNKNOWN_KEYBOARD_ACTION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/unknown-keyboard-action.hbs"
));
pub const VOICE_SETTINGS_UNKNOWN_TRIGGER_EVENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/unknown-trigger-event.hbs"
));
pub const VOICE_SETTINGS_UNKNOWN_TRIGGER_ACTION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/unknown-trigger-action.hbs"
));
pub const VOICE_SETTINGS_KILO_GATEWAY_MODEL_V0_IS_NOT_IN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/kilo-gateway-model-v0-is-not-in.hbs"
));
pub const VOICE_SETTINGS_UNKNOWN_OR_READ_ONLY_SETTING_PATH: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/unknown-or-read-only-setting-path.hbs"
));
pub const VOICE_SETTINGS_SETTING_V0_IS_NOT_ADJUSTABLE_USE_SET: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/setting-v0-is-not-adjustable-use-set.hbs"
));
pub const VOICE_SETTINGS_CURRENT_REDACTED_ILIUM_SETTINGS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/current-redacted-ilium-settings.hbs"
));
pub const VOICE_SETTINGS_PATH_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/path-is-required.hbs"
));
pub const VOICE_SETTINGS_VALUE_IS_REQUIRED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/value-is-required.hbs"
));
pub const VOICE_SETTINGS_STARTED_THE_INFERENCE_PROVIDER_TEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/started-the-inference-provider-test.hbs"
));
pub const VOICE_SETTINGS_STARTED_MODEL_DISCOVERY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/started-model-discovery.hbs"
));
pub const VOICE_SETTINGS_REQUESTED_A_SOUND_PREVIEW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/requested-a-sound-preview.hbs"
));
pub const VOICE_SETTINGS_UNFOCUSED_WIDTH_CANNOT_EXCEED_FOCUSED_WIDTH: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/unfocused-width-cannot-exceed-focused-width.hbs"
));
pub const VOICE_SETTINGS_FOCUSED_WIDTH_CANNOT_BE_SMALLER_THAN_UNFOCUSED: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/voice/settings/focused-width-cannot-be-smaller-than-unfocused.hbs"
    ));
pub const VOICE_SETTINGS_MINIMUM_TERMINAL_WIDTH_IS_OUTSIDE_ILIUM_S: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/minimum-terminal-width-is-outside-ilium-s.hbs"
));
pub const VOICE_SETTINGS_COLOR_SCHEME_MUST_BE_DARK_OR_LIGHT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/color-scheme-must-be-dark-or-light.hbs"
));
pub const VOICE_SETTINGS_TASK_PROGRESS_STYLE_MUST_BE_BRAILLE_BLOCKS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/task-progress-style-must-be-braille-blocks.hbs"
));
pub const VOICE_SETTINGS_SCROLLBACK_BUDGET_IS_TOO_LARGE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/scrollback-budget-is-too-large.hbs"
));
pub const VOICE_SETTINGS_SCROLLBACK_BUDGET_MUST_BE_4_512_MIB: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/scrollback-budget-must-be-4-512-mib.hbs"
));
pub const VOICE_SETTINGS_AUTOSAVE_DELAY_IS_TOO_LARGE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/autosave-delay-is-too-large.hbs"
));
pub const VOICE_SETTINGS_AUTOSAVE_DELAY_MUST_BE_250_500_1000: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/autosave-delay-must-be-250-500-1000.hbs"
));
pub const VOICE_SETTINGS_SHORTCUT_BASE_MUST_BE_ONE_ASCII_LETTER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/shortcut-base-must-be-one-ascii-letter.hbs"
));
pub const VOICE_SETTINGS_KEYBOARD_PRESET_MUST_BE_SCREEN_OR_TMUX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/keyboard-preset-must-be-screen-or-tmux.hbs"
));
pub const VOICE_SETTINGS_KEYBOARD_BINDING_MUST_BE_ONE_PRINTABLE_KEY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/keyboard-binding-must-be-one-printable-key.hbs"
));
pub const VOICE_SETTINGS_KEYBOARD_BINDING_WAS_REJECTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/keyboard-binding-was-rejected.hbs"
));
pub const VOICE_SETTINGS_CARD_PREVIEW_LINE_COUNT_IS_TOO_LARGE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/card-preview-line-count-is-too-large.hbs"
));
pub const VOICE_SETTINGS_CARD_PREVIEW_LINES_MUST_BE_BETWEEN_1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/card-preview-lines-must-be-between-1.hbs"
));
pub const VOICE_SETTINGS_COLUMN_WIDTH_IS_TOO_LARGE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/column-width-is-too-large.hbs"
));
pub const VOICE_SETTINGS_MINIMUM_COLUMN_WIDTH_MUST_BE_BETWEEN_10: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/minimum-column-width-must-be-between-10.hbs"
));
pub const VOICE_SETTINGS_SOUND_SOURCE_MUST_BE_SYSTEM_BEEP_OR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/sound-source-must-be-system-beep-or.hbs"
));
pub const VOICE_SETTINGS_SOUND_FILE_IS_NOT_IN_ILIUM_S: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/sound-file-is-not-in-ilium-s.hbs"
));
pub const VOICE_SETTINGS_TRIGGER_ACTIONS_MUST_BE_AN_ARRAY_OF: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/trigger-actions-must-be-an-array-of.hbs"
));
pub const VOICE_SETTINGS_VOICE_OUTPUT_VOLUME_MUST_BE_BETWEEN_0: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/voice-output-volume-must-be-between-0.hbs"
));
pub const VOICE_SETTINGS_RESET_PLANNING_TIME_STYLE_MUST_BE_EXACT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/reset-planning-time-style-must-be-exact.hbs"
));
pub const VOICE_SETTINGS_GIT_DEFAULT_WHERE_MUST_BE_HERE_NEW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/git-default-where-must-be-here-new.hbs"
));
pub const VOICE_SETTINGS_GIT_DEFAULT_BASE_MUST_BE_CURRENT_OR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/git-default-base-must-be-current-or.hbs"
));
pub const VOICE_SETTINGS_GIT_BRANCH_LINE_MUST_BE_WORKTREE_ONLY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/git-branch-line-must-be-worktree-only.hbs"
));
pub const VOICE_SETTINGS_GIT_DEFAULT_CLOSE_POLICY_MUST_BE_KEEP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/git-default-close-policy-must-be-keep.hbs"
));
pub const VOICE_SETTINGS_SETTING_REGISTRY_COULD_NOT_REACH_THE_REQUESTED: &str =
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/voice/settings/setting-registry-could-not-reach-the-requested.hbs"
    ));
pub const VOICE_SETTINGS_VALUE_MUST_BE_A_BOOLEAN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/value-must-be-a-boolean.hbs"
));
pub const VOICE_SETTINGS_VALUE_MUST_BE_A_NON_NEGATIVE_INTEGER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/value-must-be-a-non-negative-integer.hbs"
));
pub const VOICE_SETTINGS_VALUE_MUST_BE_A_STRING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/value-must-be-a-string.hbs"
));
pub const VOICE_SETTINGS_LEFT_PANEL_WIDTH_IS_OUTSIDE_ILIUM_S: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/left-panel-width-is-outside-ilium-s.hbs"
));
pub const VOICE_SETTINGS_LEFT_PANEL_SIZING_MODE_MUST_BE_FIXED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/left-panel-sizing-mode-must-be-fixed.hbs"
));
pub const VOICE_SETTINGS_INVALID_TREE_ORDER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/invalid-tree-order.hbs"
));
pub const VOICE_SETTINGS_INVALID_AGENT_IDENTIFIER_MODE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/invalid-agent-identifier-mode.hbs"
));
pub const VOICE_SETTINGS_MOTION_LEVEL_MUST_BE_FULL_REDUCED_OR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/motion-level-must-be-full-reduced-or.hbs"
));
pub const VOICE_SETTINGS_SIDEBAR_DENSITY_MUST_BE_COMPACT_STANDARD_OR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/sidebar-density-must-be-compact-standard-or.hbs"
));
pub const VOICE_SETTINGS_INVALID_NEW_PANE_DIRECTORY_POLICY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/invalid-new-pane-directory-policy.hbs"
));
pub const VOICE_SETTINGS_INVALID_SESSION_RECOVERY_POLICY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/invalid-session-recovery-policy.hbs"
));
pub const VOICE_SETTINGS_INVALID_INFERENCE_PROVIDER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/invalid-inference-provider.hbs"
));
pub const VOICE_SETTINGS_INVALID_TITLE_STYLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/invalid-title-style.hbs"
));
pub const VOICE_SETTINGS_INVALID_REALTIME_VOICE_MODEL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/invalid-realtime-voice-model.hbs"
));
pub const VOICE_SETTINGS_INVALID_REALTIME_VOICE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/invalid-realtime-voice.hbs"
));
pub const VOICE_SETTINGS_REASONING_EFFORT_MUST_BE_MINIMAL_LOW_OR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/reasoning-effort-must-be-minimal-low-or.hbs"
));
pub const VOICE_SETTINGS_VOICE_INPUT_MODE_MUST_BE_SEMANTIC_VAD: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/voice-input-mode-must-be-semantic-vad.hbs"
));
pub const VOICE_SETTINGS_VAD_EAGERNESS_MUST_BE_AUTO_LOW_MEDIUM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/voice/settings/vad-eagerness-must-be-auto-low-medium.hbs"
));

pub const TEMPLATES: &[(&str, &str)] = &[
    ("voice/onboarding-lightbulb", ONBOARDING_LIGHTBULB),
    (
        "voice/mod/system-instructions",
        VOICE_MOD_SYSTEM_INSTRUCTIONS,
    ),
    ("voice/mod/unknown-ilium-tool", VOICE_MOD_UNKNOWN_ILIUM_TOOL),
    (
        "voice/mod/invalid-tool-arguments",
        VOICE_MOD_INVALID_TOOL_ARGUMENTS,
    ),
    (
        "voice/mod/the-active-pane-is-currently-a-detected",
        VOICE_MOD_THE_ACTIVE_PANE_IS_CURRENTLY_A_DETECTED,
    ),
    (
        "voice/mod/the-active-pane-is-not-currently-a",
        VOICE_MOD_THE_ACTIVE_PANE_IS_NOT_CURRENTLY_A,
    ),
    (
        "voice/mod/ask-only-the-exact-question-do-not",
        VOICE_MOD_ASK_ONLY_THE_EXACT_QUESTION_DO_NOT,
    ),
    (
        "voice/mod/that-confirmation-token-is-missing-expired-or",
        VOICE_MOD_THAT_CONFIRMATION_TOKEN_IS_MISSING_EXPIRED_OR,
    ),
    (
        "voice/tools/inspect-the-current-ilium-session-tree-focus",
        VOICE_TOOLS_INSPECT_THE_CURRENT_ILIUM_SESSION_TREE_FOCUS,
    ),
    (
        "voice/tools/immediately-stop-and-disable-the-current-ilium",
        VOICE_TOOLS_IMMEDIATELY_STOP_AND_DISABLE_THE_CURRENT_ILIUM,
    ),
    (
        "voice/tools/navigate-global-ui-surfaces-focus-tree-or",
        VOICE_TOOLS_NAVIGATE_GLOBAL_UI_SURFACES_FOCUS_TREE_OR,
    ),
    (
        "voice/tools/create-open-organize-rename-move-reparent-split",
        VOICE_TOOLS_CREATE_OPEN_ORGANIZE_RENAME_MOVE_REPARENT_SPLIT,
    ),
    (
        "voice/tools/primary-voice-dictation-action-send-the-exact",
        VOICE_TOOLS_PRIMARY_VOICE_DICTATION_ACTION_SEND_THE_EXACT,
    ),
    (
        "voice/tools/exact-text-to-type-preserving-slash-commands",
        VOICE_TOOLS_EXACT_TEXT_TO_TYPE_PRESERVING_SLASH_COMMANDS,
    ),
    (
        "voice/tools/type-exact-text-into-a-terminal-without",
        VOICE_TOOLS_TYPE_EXACT_TEXT_INTO_A_TERMINAL_WITHOUT,
    ),
    (
        "voice/tools/exact-text-to-leave-visible-and-unsubmitted",
        VOICE_TOOLS_EXACT_TEXT_TO_LEAVE_VISIBLE_AND_UNSUBMITTED,
    ),
    (
        "voice/tools/control-non-dictation-terminal-behavior-press-a",
        VOICE_TOOLS_CONTROL_NON_DICTATION_TERMINAL_BEHAVIOR_PRESS_A,
    ),
    (
        "voice/tools/operate-a-built-in-editor-pane-save",
        VOICE_TOOLS_OPERATE_A_BUILT_IN_EDITOR_PANE_SAVE,
    ),
    (
        "voice/tools/operate-every-kanban-surface-select-open-cards",
        VOICE_TOOLS_OPERATE_EVERY_KANBAN_SURFACE_SELECT_OPEN_CARDS,
    ),
    (
        "voice/tools/get-set-or-adjust-any-persisted-setting",
        VOICE_TOOLS_GET_SET_OR_ADJUST_ANY_PERSISTED_SETTING,
    ),
    (
        "voice/tools/search-all-retained-terminal-output-open-editor",
        VOICE_TOOLS_SEARCH_ALL_RETAINED_TERMINAL_OUTPUT_OPEN_EDITOR,
    ),
    (
        "voice/tools/detach-restart-the-tui-client-restart-the",
        VOICE_TOOLS_DETACH_RESTART_THE_TUI_CLIENT_RESTART_THE,
    ),
    (
        "voice/tools/confirm-or-cancel-one-pending-ilium-action",
        VOICE_TOOLS_CONFIRM_OR_CANCEL_ONE_PENDING_ILIUM_ACTION,
    ),
    (
        "voice/tools/slash-separated-node-name-path-from-the",
        VOICE_TOOLS_SLASH_SEPARATED_NODE_NAME_PATH_FROM_THE,
    ),
    (
        "voice/policy/run-the-shell-command-v0-in-a",
        VOICE_POLICY_RUN_THE_SHELL_COMMAND_V0_IN_A,
    ),
    (
        "voice/policy/queue-this-prompt-to-be-submitted-automatically",
        VOICE_POLICY_QUEUE_THIS_PROMPT_TO_BE_SUBMITTED_AUTOMATICALLY,
    ),
    (
        "voice/policy/board-has-no-column",
        VOICE_POLICY_BOARD_HAS_NO_COLUMN,
    ),
    (
        "voice/policy/board-has-no-card-v0-in-column",
        VOICE_POLICY_BOARD_HAS_NO_CARD_V0_IN_COLUMN,
    ),
    (
        "voice/policy/permanently-delete-the-card-v0-from-column",
        VOICE_POLICY_PERMANENTLY_DELETE_THE_CARD_V0_FROM_COLUMN,
    ),
    (
        "voice/policy/permanently-delete-the-column-v0-and-its",
        VOICE_POLICY_PERMANENTLY_DELETE_THE_COLUMN_V0_AND_ITS,
    ),
    (
        "voice/policy/command-line-is-required",
        VOICE_POLICY_COMMAND_LINE_IS_REQUIRED,
    ),
    (
        "voice/policy/replace-the-editor-s-entire-current-document",
        VOICE_POLICY_REPLACE_THE_EDITOR_S_ENTIRE_CURRENT_DOCUMENT,
    ),
    (
        "voice/policy/press-enter-and-submit-what-you-see",
        VOICE_POLICY_PRESS_ENTER_AND_SUBMIT_WHAT_YOU_SEE,
    ),
    (
        "voice/policy/schedule-this-terminal-input-for-automatic-submission",
        VOICE_POLICY_SCHEDULE_THIS_TERMINAL_INPUT_FOR_AUTOMATIC_SUBMISSION,
    ),
    (
        "voice/policy/queue-this-prompt-to-be-submitted-automatically-2",
        VOICE_POLICY_QUEUE_THIS_PROMPT_TO_BE_SUBMITTED_AUTOMATICALLY_2,
    ),
    (
        "voice/policy/runs-is-required-and-must-be-positive",
        VOICE_POLICY_RUNS_IS_REQUIRED_AND_MUST_BE_POSITIVE,
    ),
    (
        "voice/policy/queue-this-prompt-for-automatic-submission-after",
        VOICE_POLICY_QUEUE_THIS_PROMPT_FOR_AUTOMATIC_SUBMISSION_AFTER,
    ),
    (
        "voice/policy/kill-the-entire-ilium-session-and-every",
        VOICE_POLICY_KILL_THE_ENTIRE_ILIUM_SESSION_AND_EVERY,
    ),
    (
        "voice/policy/restart-the-detached-ilium-server-and-temporarily",
        VOICE_POLICY_RESTART_THE_DETACHED_ILIUM_SERVER_AND_TEMPORARILY,
    ),
    (
        "voice/policy/cancelled-the-pending-action",
        VOICE_POLICY_CANCELLED_THE_PENDING_ACTION,
    ),
    (
        "voice/policy/target-is-not-a-board-pane",
        VOICE_POLICY_TARGET_IS_NOT_A_BOARD_PANE,
    ),
    (
        "voice/policy/column-is-required",
        VOICE_POLICY_COLUMN_IS_REQUIRED,
    ),
    (
        "voice/policy/card-is-required",
        VOICE_POLICY_CARD_IS_REQUIRED,
    ),
    (
        "voice/policy/i-typed-it-into-the-target-terminal",
        VOICE_POLICY_I_TYPED_IT_INTO_THE_TARGET_TERMINAL,
    ),
    (
        "voice/policy/left-the-staged-terminal-text-visible-and",
        VOICE_POLICY_LEFT_THE_STAGED_TERMINAL_TEXT_VISIBLE_AND,
    ),
    (
        "voice/executor/search-result-index-v0-does-not-exist",
        VOICE_EXECUTOR_SEARCH_RESULT_INDEX_V0_DOES_NOT_EXIST,
    ),
    (
        "voice/executor/found-v0-workspace-results-for",
        VOICE_EXECUTOR_FOUND_V0_WORKSPACE_RESULTS_FOR,
    ),
    ("voice/executor/focused-pane", VOICE_EXECUTOR_FOCUSED_PANE),
    (
        "voice/executor/node-v0-is-not-a-split-view",
        VOICE_EXECUTOR_NODE_V0_IS_NOT_A_SPLIT_VIEW,
    ),
    ("voice/executor/showing-split", VOICE_EXECUTOR_SHOWING_SPLIT),
    (
        "voice/executor/opened-v0-settings",
        VOICE_EXECUTOR_OPENED_V0_SETTINGS,
    ),
    (
        "voice/executor/invalid-workspace-branch",
        VOICE_EXECUTOR_INVALID_WORKSPACE_BRANCH,
    ),
    (
        "voice/executor/creating-a-v0-agent-in-a-git",
        VOICE_EXECUTOR_CREATING_A_V0_AGENT_IN_A_GIT,
    ),
    (
        "voice/executor/creating-a-v0-agent-pane",
        VOICE_EXECUTOR_CREATING_A_V0_AGENT_PANE,
    ),
    (
        "voice/executor/a-split-view-can-contain-at-most",
        VOICE_EXECUTOR_A_SPLIT_VIEW_CAN_CONTAIN_AT_MOST,
    ),
    (
        "voice/executor/node-v0-cannot-be-expanded",
        VOICE_EXECUTOR_NODE_V0_CANNOT_BE_EXPANDED,
    ),
    (
        "voice/executor/node-v0-is-not-a-live",
        VOICE_EXECUTOR_NODE_V0_IS_NOT_A_LIVE,
    ),
    (
        "voice/executor/node-v0-is-not-a-terminal-pane",
        VOICE_EXECUTOR_NODE_V0_IS_NOT_A_TERMINAL_PANE,
    ),
    (
        "voice/executor/node-v0-is-not-an-editor-pane",
        VOICE_EXECUTOR_NODE_V0_IS_NOT_AN_EDITOR_PANE,
    ),
    (
        "voice/executor/v0-is-required",
        VOICE_EXECUTOR_V0_IS_REQUIRED,
    ),
    (
        "voice/executor/v0-must-not-be-empty",
        VOICE_EXECUTOR_V0_MUST_NOT_BE_EMPTY,
    ),
    (
        "voice/executor/unknown-settings-tab",
        VOICE_EXECUTOR_UNKNOWN_SETTINGS_TAB,
    ),
    (
        "voice/executor/read-ilium-get-state-after-the-server",
        VOICE_EXECUTOR_READ_ILIUM_GET_STATE_AFTER_THE_SERVER,
    ),
    (
        "voice/executor/current-ilium-state",
        VOICE_EXECUTOR_CURRENT_ILIUM_STATE,
    ),
    (
        "voice/executor/queued-text-and-enter-for-the-terminal",
        VOICE_EXECUTOR_QUEUED_TEXT_AND_ENTER_FOR_THE_TERMINAL,
    ),
    (
        "voice/executor/staged-text-in-the-terminal",
        VOICE_EXECUTOR_STAGED_TEXT_IN_THE_TERMINAL,
    ),
    (
        "voice/executor/there-is-no-selected-search-result",
        VOICE_EXECUTOR_THERE_IS_NO_SELECTED_SEARCH_RESULT,
    ),
    (
        "voice/executor/opened-the-selected-search-result",
        VOICE_EXECUTOR_OPENED_THE_SELECTED_SEARCH_RESULT,
    ),
    (
        "voice/executor/closed-workspace-search",
        VOICE_EXECUTOR_CLOSED_WORKSPACE_SEARCH,
    ),
    (
        "voice/executor/focused-the-left-tree-panel",
        VOICE_EXECUTOR_FOCUSED_THE_LEFT_TREE_PANEL,
    ),
    (
        "voice/executor/focused-the-next-visible-pane",
        VOICE_EXECUTOR_FOCUSED_THE_NEXT_VISIBLE_PANE,
    ),
    (
        "voice/executor/focused-the-previous-visible-pane",
        VOICE_EXECUTOR_FOCUSED_THE_PREVIOUS_VISIBLE_PANE,
    ),
    (
        "voice/executor/direction-is-required",
        VOICE_EXECUTOR_DIRECTION_IS_REQUIRED,
    ),
    (
        "voice/executor/moved-focus-within-the-visible-split",
        VOICE_EXECUTOR_MOVED_FOCUS_WITHIN_THE_VISIBLE_SPLIT,
    ),
    (
        "voice/executor/opened-settings",
        VOICE_EXECUTOR_OPENED_SETTINGS,
    ),
    (
        "voice/executor/tab-is-required",
        VOICE_EXECUTOR_TAB_IS_REQUIRED,
    ),
    (
        "voice/executor/opened-workspace-search",
        VOICE_EXECUTOR_OPENED_WORKSPACE_SEARCH,
    ),
    ("voice/executor/opened-help", VOICE_EXECUTOR_OPENED_HELP),
    (
        "voice/executor/closed-the-current-overlay",
        VOICE_EXECUTOR_CLOSED_THE_CURRENT_OVERLAY,
    ),
    (
        "voice/executor/creating-a-terminal-pane",
        VOICE_EXECUTOR_CREATING_A_TERMINAL_PANE,
    ),
    (
        "voice/executor/provider-is-required",
        VOICE_EXECUTOR_PROVIDER_IS_REQUIRED,
    ),
    (
        "voice/executor/workspace-base-must-not-be-empty",
        VOICE_EXECUTOR_WORKSPACE_BASE_MUST_NOT_BE_EMPTY,
    ),
    (
        "voice/executor/creating-the-command-pane",
        VOICE_EXECUTOR_CREATING_THE_COMMAND_PANE,
    ),
    (
        "voice/executor/opening-the-file-in-an-editor-pane",
        VOICE_EXECUTOR_OPENING_THE_FILE_IN_AN_EDITOR_PANE,
    ),
    (
        "voice/executor/adding-the-folder-to-the-left-panel",
        VOICE_EXECUTOR_ADDING_THE_FOLDER_TO_THE_LEFT_PANEL,
    ),
    (
        "voice/executor/adding-the-project",
        VOICE_EXECUTOR_ADDING_THE_PROJECT,
    ),
    (
        "voice/executor/changing-the-project-s-folder",
        VOICE_EXECUTOR_CHANGING_THE_PROJECT_S_FOLDER,
    ),
    (
        "voice/executor/creating-the-group",
        VOICE_EXECUTOR_CREATING_THE_GROUP,
    ),
    (
        "voice/executor/board-creation-failed",
        VOICE_EXECUTOR_BOARD_CREATION_FAILED,
    ),
    (
        "voice/executor/creating-the-board",
        VOICE_EXECUTOR_CREATING_THE_BOARD,
    ),
    (
        "voice/executor/creating-the-split-view",
        VOICE_EXECUTOR_CREATING_THE_SPLIT_VIEW,
    ),
    (
        "voice/executor/renaming-the-tree-item",
        VOICE_EXECUTOR_RENAMING_THE_TREE_ITEM,
    ),
    (
        "voice/executor/moving-the-tree-item",
        VOICE_EXECUTOR_MOVING_THE_TREE_ITEM,
    ),
    (
        "voice/executor/reparenting-the-tree-item",
        VOICE_EXECUTOR_REPARENTING_THE_TREE_ITEM,
    ),
    (
        "voice/executor/closing-the-tree-item",
        VOICE_EXECUTOR_CLOSING_THE_TREE_ITEM,
    ),
    (
        "voice/executor/toggled-the-left-panel-group",
        VOICE_EXECUTOR_TOGGLED_THE_LEFT_PANEL_GROUP,
    ),
    (
        "voice/executor/started-automatic-retitling",
        VOICE_EXECUTOR_STARTED_AUTOMATIC_RETITLING,
    ),
    (
        "voice/executor/requested-project-restructuring",
        VOICE_EXECUTOR_REQUESTED_PROJECT_RESTRUCTURING,
    ),
    (
        "voice/executor/reverting-the-project-s-latest-restructure",
        VOICE_EXECUTOR_REVERTING_THE_PROJECT_S_LATEST_RESTRUCTURE,
    ),
    (
        "voice/executor/key-is-required",
        VOICE_EXECUTOR_KEY_IS_REQUIRED,
    ),
    (
        "voice/executor/sent-the-key-to-the-terminal",
        VOICE_EXECUTOR_SENT_THE_KEY_TO_THE_TERMINAL,
    ),
    (
        "voice/executor/target-is-not-a-terminal-pane",
        VOICE_EXECUTOR_TARGET_IS_NOT_A_TERMINAL_PANE,
    ),
    (
        "voice/executor/scrolled-the-terminal",
        VOICE_EXECUTOR_SCROLLED_THE_TERMINAL,
    ),
    (
        "voice/executor/returned-to-live-terminal-output",
        VOICE_EXECUTOR_RETURNED_TO_LIVE_TERMINAL_OUTPUT,
    ),
    (
        "voice/executor/scheduled-terminal-input",
        VOICE_EXECUTOR_SCHEDULED_TERMINAL_INPUT,
    ),
    (
        "voice/executor/queued-the-prompt-for-the-agent-s",
        VOICE_EXECUTOR_QUEUED_THE_PROMPT_FOR_THE_AGENT_S,
    ),
    (
        "voice/executor/clearing-the-pane-s-prompt-queue",
        VOICE_EXECUTOR_CLEARING_THE_PANE_S_PROMPT_QUEUE,
    ),
    (
        "voice/executor/target-is-not-an-editor-pane",
        VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE,
    ),
    (
        "voice/executor/this-editor-has-no-file-path-yet",
        VOICE_EXECUTOR_THIS_EDITOR_HAS_NO_FILE_PATH_YET,
    ),
    (
        "voice/executor/editor-updated",
        VOICE_EXECUTOR_EDITOR_UPDATED,
    ),
    (
        "voice/executor/text-is-required",
        VOICE_EXECUTOR_TEXT_IS_REQUIRED,
    ),
    (
        "voice/executor/inserted-text-into-the-editor",
        VOICE_EXECUTOR_INSERTED_TEXT_INTO_THE_EDITOR,
    ),
    (
        "voice/executor/replaced-the-editor-s-document",
        VOICE_EXECUTOR_REPLACED_THE_EDITOR_S_DOCUMENT,
    ),
    (
        "voice/executor/line-is-required",
        VOICE_EXECUTOR_LINE_IS_REQUIRED,
    ),
    (
        "voice/executor/moved-the-editor-cursor",
        VOICE_EXECUTOR_MOVED_THE_EDITOR_CURSOR,
    ),
    (
        "voice/executor/toggled-the-rendered-source-view",
        VOICE_EXECUTOR_TOGGLED_THE_RENDERED_SOURCE_VIEW,
    ),
    (
        "voice/executor/toggled-line-numbers",
        VOICE_EXECUTOR_TOGGLED_LINE_NUMBERS,
    ),
    (
        "voice/executor/toggled-the-minimap",
        VOICE_EXECUTOR_TOGGLED_THE_MINIMAP,
    ),
    (
        "voice/executor/toggled-autosave",
        VOICE_EXECUTOR_TOGGLED_AUTOSAVE,
    ),
    (
        "voice/executor/destination-column-is-required",
        VOICE_EXECUTOR_DESTINATION_COLUMN_IS_REQUIRED,
    ),
    (
        "voice/executor/checkbox-is-required",
        VOICE_EXECUTOR_CHECKBOX_IS_REQUIRED,
    ),
    (
        "voice/executor/board-updated-and-persisted",
        VOICE_EXECUTOR_BOARD_UPDATED_AND_PERSISTED,
    ),
    (
        "voice/executor/session-lifecycle-request-queued",
        VOICE_EXECUTOR_SESSION_LIFECYCLE_REQUEST_QUEUED,
    ),
    (
        "voice/executor/voice-mode-stopped",
        VOICE_EXECUTOR_VOICE_MODE_STOPPED,
    ),
    (
        "voice/resolver/no-ilium-node-has-id",
        VOICE_RESOLVER_NO_ILIUM_NODE_HAS_ID,
    ),
    (
        "voice/resolver/node-v0-cannot-contain-ordinary-ilium-items",
        VOICE_RESOLVER_NODE_V0_CANNOT_CONTAIN_ORDINARY_ILIUM_ITEMS,
    ),
    (
        "voice/resolver/no-ilium-node-is-named",
        VOICE_RESOLVER_NO_ILIUM_NODE_IS_NAMED,
    ),
    (
        "voice/resolver/the-name-v0-is-ambiguous-use-one",
        VOICE_RESOLVER_THE_NAME_V0_IS_AMBIGUOUS_USE_ONE,
    ),
    (
        "voice/resolver/v0-is-not-a-container",
        VOICE_RESOLVER_V0_IS_NOT_A_CONTAINER,
    ),
    (
        "voice/resolver/no-child-named-v0-exists-under",
        VOICE_RESOLVER_NO_CHILD_NAMED_V0_EXISTS_UNDER,
    ),
    (
        "voice/resolver/path-component-v0-is-ambiguous-under",
        VOICE_RESOLVER_PATH_COMPONENT_V0_IS_AMBIGUOUS_UNDER,
    ),
    (
        "voice/resolver/no-active-or-selected-ilium-node",
        VOICE_RESOLVER_NO_ACTIVE_OR_SELECTED_ILIUM_NODE,
    ),
    (
        "voice/resolver/no-destination-group-is-available",
        VOICE_RESOLVER_NO_DESTINATION_GROUP_IS_AVAILABLE,
    ),
    ("voice/settings/updated", VOICE_SETTINGS_UPDATED),
    ("voice/settings/adjusted", VOICE_SETTINGS_ADJUSTED),
    (
        "voice/settings/unknown-configurable-icon",
        VOICE_SETTINGS_UNKNOWN_CONFIGURABLE_ICON,
    ),
    (
        "voice/settings/unknown-keyboard-action",
        VOICE_SETTINGS_UNKNOWN_KEYBOARD_ACTION,
    ),
    (
        "voice/settings/unknown-trigger-event",
        VOICE_SETTINGS_UNKNOWN_TRIGGER_EVENT,
    ),
    (
        "voice/settings/unknown-trigger-action",
        VOICE_SETTINGS_UNKNOWN_TRIGGER_ACTION,
    ),
    (
        "voice/settings/kilo-gateway-model-v0-is-not-in",
        VOICE_SETTINGS_KILO_GATEWAY_MODEL_V0_IS_NOT_IN,
    ),
    (
        "voice/settings/unknown-or-read-only-setting-path",
        VOICE_SETTINGS_UNKNOWN_OR_READ_ONLY_SETTING_PATH,
    ),
    (
        "voice/settings/setting-v0-is-not-adjustable-use-set",
        VOICE_SETTINGS_SETTING_V0_IS_NOT_ADJUSTABLE_USE_SET,
    ),
    (
        "voice/settings/current-redacted-ilium-settings",
        VOICE_SETTINGS_CURRENT_REDACTED_ILIUM_SETTINGS,
    ),
    (
        "voice/settings/path-is-required",
        VOICE_SETTINGS_PATH_IS_REQUIRED,
    ),
    (
        "voice/settings/value-is-required",
        VOICE_SETTINGS_VALUE_IS_REQUIRED,
    ),
    (
        "voice/settings/started-the-inference-provider-test",
        VOICE_SETTINGS_STARTED_THE_INFERENCE_PROVIDER_TEST,
    ),
    (
        "voice/settings/started-model-discovery",
        VOICE_SETTINGS_STARTED_MODEL_DISCOVERY,
    ),
    (
        "voice/settings/requested-a-sound-preview",
        VOICE_SETTINGS_REQUESTED_A_SOUND_PREVIEW,
    ),
    (
        "voice/settings/unfocused-width-cannot-exceed-focused-width",
        VOICE_SETTINGS_UNFOCUSED_WIDTH_CANNOT_EXCEED_FOCUSED_WIDTH,
    ),
    (
        "voice/settings/focused-width-cannot-be-smaller-than-unfocused",
        VOICE_SETTINGS_FOCUSED_WIDTH_CANNOT_BE_SMALLER_THAN_UNFOCUSED,
    ),
    (
        "voice/settings/minimum-terminal-width-is-outside-ilium-s",
        VOICE_SETTINGS_MINIMUM_TERMINAL_WIDTH_IS_OUTSIDE_ILIUM_S,
    ),
    (
        "voice/settings/color-scheme-must-be-dark-or-light",
        VOICE_SETTINGS_COLOR_SCHEME_MUST_BE_DARK_OR_LIGHT,
    ),
    (
        "voice/settings/task-progress-style-must-be-braille-blocks",
        VOICE_SETTINGS_TASK_PROGRESS_STYLE_MUST_BE_BRAILLE_BLOCKS,
    ),
    (
        "voice/settings/scrollback-budget-is-too-large",
        VOICE_SETTINGS_SCROLLBACK_BUDGET_IS_TOO_LARGE,
    ),
    (
        "voice/settings/scrollback-budget-must-be-4-512-mib",
        VOICE_SETTINGS_SCROLLBACK_BUDGET_MUST_BE_4_512_MIB,
    ),
    (
        "voice/settings/autosave-delay-is-too-large",
        VOICE_SETTINGS_AUTOSAVE_DELAY_IS_TOO_LARGE,
    ),
    (
        "voice/settings/autosave-delay-must-be-250-500-1000",
        VOICE_SETTINGS_AUTOSAVE_DELAY_MUST_BE_250_500_1000,
    ),
    (
        "voice/settings/shortcut-base-must-be-one-ascii-letter",
        VOICE_SETTINGS_SHORTCUT_BASE_MUST_BE_ONE_ASCII_LETTER,
    ),
    (
        "voice/settings/keyboard-preset-must-be-screen-or-tmux",
        VOICE_SETTINGS_KEYBOARD_PRESET_MUST_BE_SCREEN_OR_TMUX,
    ),
    (
        "voice/settings/keyboard-binding-must-be-one-printable-key",
        VOICE_SETTINGS_KEYBOARD_BINDING_MUST_BE_ONE_PRINTABLE_KEY,
    ),
    (
        "voice/settings/keyboard-binding-was-rejected",
        VOICE_SETTINGS_KEYBOARD_BINDING_WAS_REJECTED,
    ),
    (
        "voice/settings/card-preview-line-count-is-too-large",
        VOICE_SETTINGS_CARD_PREVIEW_LINE_COUNT_IS_TOO_LARGE,
    ),
    (
        "voice/settings/card-preview-lines-must-be-between-1",
        VOICE_SETTINGS_CARD_PREVIEW_LINES_MUST_BE_BETWEEN_1,
    ),
    (
        "voice/settings/column-width-is-too-large",
        VOICE_SETTINGS_COLUMN_WIDTH_IS_TOO_LARGE,
    ),
    (
        "voice/settings/minimum-column-width-must-be-between-10",
        VOICE_SETTINGS_MINIMUM_COLUMN_WIDTH_MUST_BE_BETWEEN_10,
    ),
    (
        "voice/settings/sound-source-must-be-system-beep-or",
        VOICE_SETTINGS_SOUND_SOURCE_MUST_BE_SYSTEM_BEEP_OR,
    ),
    (
        "voice/settings/sound-file-is-not-in-ilium-s",
        VOICE_SETTINGS_SOUND_FILE_IS_NOT_IN_ILIUM_S,
    ),
    (
        "voice/settings/trigger-actions-must-be-an-array-of",
        VOICE_SETTINGS_TRIGGER_ACTIONS_MUST_BE_AN_ARRAY_OF,
    ),
    (
        "voice/settings/voice-output-volume-must-be-between-0",
        VOICE_SETTINGS_VOICE_OUTPUT_VOLUME_MUST_BE_BETWEEN_0,
    ),
    (
        "voice/settings/reset-planning-time-style-must-be-exact",
        VOICE_SETTINGS_RESET_PLANNING_TIME_STYLE_MUST_BE_EXACT,
    ),
    (
        "voice/settings/git-default-where-must-be-here-new",
        VOICE_SETTINGS_GIT_DEFAULT_WHERE_MUST_BE_HERE_NEW,
    ),
    (
        "voice/settings/git-default-base-must-be-current-or",
        VOICE_SETTINGS_GIT_DEFAULT_BASE_MUST_BE_CURRENT_OR,
    ),
    (
        "voice/settings/git-branch-line-must-be-worktree-only",
        VOICE_SETTINGS_GIT_BRANCH_LINE_MUST_BE_WORKTREE_ONLY,
    ),
    (
        "voice/settings/git-default-close-policy-must-be-keep",
        VOICE_SETTINGS_GIT_DEFAULT_CLOSE_POLICY_MUST_BE_KEEP,
    ),
    (
        "voice/settings/setting-registry-could-not-reach-the-requested",
        VOICE_SETTINGS_SETTING_REGISTRY_COULD_NOT_REACH_THE_REQUESTED,
    ),
    (
        "voice/settings/value-must-be-a-boolean",
        VOICE_SETTINGS_VALUE_MUST_BE_A_BOOLEAN,
    ),
    (
        "voice/settings/value-must-be-a-non-negative-integer",
        VOICE_SETTINGS_VALUE_MUST_BE_A_NON_NEGATIVE_INTEGER,
    ),
    (
        "voice/settings/value-must-be-a-string",
        VOICE_SETTINGS_VALUE_MUST_BE_A_STRING,
    ),
    (
        "voice/settings/left-panel-width-is-outside-ilium-s",
        VOICE_SETTINGS_LEFT_PANEL_WIDTH_IS_OUTSIDE_ILIUM_S,
    ),
    (
        "voice/settings/left-panel-sizing-mode-must-be-fixed",
        VOICE_SETTINGS_LEFT_PANEL_SIZING_MODE_MUST_BE_FIXED,
    ),
    (
        "voice/settings/invalid-tree-order",
        VOICE_SETTINGS_INVALID_TREE_ORDER,
    ),
    (
        "voice/settings/invalid-agent-identifier-mode",
        VOICE_SETTINGS_INVALID_AGENT_IDENTIFIER_MODE,
    ),
    (
        "voice/settings/motion-level-must-be-full-reduced-or",
        VOICE_SETTINGS_MOTION_LEVEL_MUST_BE_FULL_REDUCED_OR,
    ),
    (
        "voice/settings/sidebar-density-must-be-compact-standard-or",
        VOICE_SETTINGS_SIDEBAR_DENSITY_MUST_BE_COMPACT_STANDARD_OR,
    ),
    (
        "voice/settings/invalid-new-pane-directory-policy",
        VOICE_SETTINGS_INVALID_NEW_PANE_DIRECTORY_POLICY,
    ),
    (
        "voice/settings/invalid-session-recovery-policy",
        VOICE_SETTINGS_INVALID_SESSION_RECOVERY_POLICY,
    ),
    (
        "voice/settings/invalid-inference-provider",
        VOICE_SETTINGS_INVALID_INFERENCE_PROVIDER,
    ),
    (
        "voice/settings/invalid-title-style",
        VOICE_SETTINGS_INVALID_TITLE_STYLE,
    ),
    (
        "voice/settings/invalid-realtime-voice-model",
        VOICE_SETTINGS_INVALID_REALTIME_VOICE_MODEL,
    ),
    (
        "voice/settings/invalid-realtime-voice",
        VOICE_SETTINGS_INVALID_REALTIME_VOICE,
    ),
    (
        "voice/settings/reasoning-effort-must-be-minimal-low-or",
        VOICE_SETTINGS_REASONING_EFFORT_MUST_BE_MINIMAL_LOW_OR,
    ),
    (
        "voice/settings/voice-input-mode-must-be-semantic-vad",
        VOICE_SETTINGS_VOICE_INPUT_MODE_MUST_BE_SEMANTIC_VAD,
    ),
    (
        "voice/settings/vad-eagerness-must-be-auto-low-medium",
        VOICE_SETTINGS_VAD_EAGERNESS_MUST_BE_AUTO_LOW_MEDIUM,
    ),
];
