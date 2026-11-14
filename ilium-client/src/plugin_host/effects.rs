//! Client effects of plugin intents (gap G3).
//!
//! Some intents only make sense where a user sits: attention marks, toasts, opening a URL or a
//! file, and kernel commands such as `kernel.focus-node`. A client plugin submits them to its
//! own kernel facade; a server plugin's submission reaches every client as a
//! `ForwardedIntent`. Either way the intent is classified here into a [`ClientEffect`] the
//! event loop carries out (`effects_app`), a toast, or "not client-local" for the server.
//!
//! The module is pure: an intent in, a classification out.

use std::path::{Component, Path, PathBuf};

use ilium_plugin_api::generated::types::intents::{
    EditorOpenSpec, EditorPlacement, ExternalTarget, InvokeCommandSpec, MarkAttentionSpec,
    ToastSpec,
};
use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::generated::types::{ErrorCode, Intent, Outcome, PluginError};

/// The kernel command that focuses a node (`{"node": <id>}` or a bare id).
pub(crate) const FOCUS_NODE_COMMAND: &str = "kernel.focus-node";
/// The kernel command that freezes an agent pane (`{"node": <id>}` or a bare id). It keeps the
/// painted screen and queues the server's freeze, exactly like the pane's Freeze action.
pub(crate) const FREEZE_AGENT_COMMAND: &str = "kernel.freeze-agent";
/// Longest attention reason (reference-wit.md section 4.12).
const MAXIMUM_ATTENTION_REASON_CHARACTERS: usize = 80;
/// Longest toast title (reference-wit.md section 4.12).
const MAXIMUM_TOAST_TITLE_CHARACTERS: usize = 60;
/// Shortest and longest toast duration.
const TOAST_DURATION_MS: std::ops::RangeInclusive<u32> = 1_000..=30_000;
/// URL schemes a plugin may ask the desktop to open.
const OPENABLE_SCHEMES: &[&str] = &["https://", "http://", "mailto:"];

/// What the event loop does for a client-local intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClientEffect {
    /// Focus a pane, or select any other node, in the tree.
    FocusNode(ilium_core::NodeId),
    /// Put a pane into the attention queue with the plugin's reason.
    MarkAttention {
        node: ilium_core::NodeId,
        reason: String,
    },
    /// Clear the plugin's attention mark of a pane.
    AckAttention(ilium_core::NodeId),
    /// Open a URL or a file with the desktop's default application.
    OpenExternal(crate::open_target::OpenTarget),
    /// Open a file in a kernel editor pane. `line` is 1-based, as the kernel's own links are;
    /// the intent's 0-based line is converted here.
    EditorOpen {
        path: PathBuf,
        line: Option<u32>,
        placement: EditorPlacement,
    },
    /// Open a kernel Settings page by its id.
    OpenSettings(String),
    /// Remove the toast with this id if it is still shown.
    DismissToast(String),
    /// Set one presentation option of an editor pane (`kernel.set-editor-option`); `None`
    /// names the focused pane.
    SetEditorOption {
        pane: Option<ilium_core::NodeId>,
        option: EditorOption,
    },
    /// Freeze an agent pane (`kernel.freeze-agent`): stop its process and keep its screen.
    FreezeAgent(ilium_core::NodeId),
    /// Set the order and visibility of the tree row slots (`kernel.set-tree-row-layout`).
    SetTreeRowLayout(crate::tree_row_layout::TreeRowLayout),
    /// Set the panel padding (`kernel.set-padding`).
    SetPadding(crate::padding::PaddingSettings),
    /// Set the buttons of the tree toolbar and the inactivity threshold
    /// (`kernel.set-tree-toolbar`).
    SetTreeToolbar {
        entries: Vec<crate::tree_toolbar_layout::TreeToolbarEntry>,
        minutes: u16,
    },
}

/// The kernel command that sets the tree row layout: `{"order": [...], "hidden": [...]}` with
/// the slot names `identity`, `objective`, `activity` and `title`. The kernel completes a
/// partial order and drops a hidden title, then saves the layout like the settings page did.
pub(crate) const SET_TREE_ROW_LAYOUT_COMMAND: &str = "kernel.set-tree-row-layout";

/// The layout of a `kernel.set-tree-row-layout` invocation, or why it is refused.
pub(crate) fn tree_row_layout_argument(
    arguments_json: &str,
) -> Result<crate::tree_row_layout::TreeRowLayout, String> {
    const USAGE: &str = "kernel.set-tree-row-layout takes {\"order\": [...], \"hidden\": [...]}";
    let value: serde_json::Value =
        serde_json::from_str(arguments_json).map_err(|error| format!("{USAGE}: {error}"))?;
    if !value.is_object() {
        return Err(USAGE.to_owned());
    }
    serde_json::from_value::<crate::tree_row_layout::TreeRowLayout>(value)
        .map(crate::tree_row_layout::TreeRowLayout::normalized)
        .map_err(|error| format!("{USAGE}: {error}"))
}

/// The kernel command that sets the tree toolbar: `{"entries": ["search", ...], "minutes": 15}`
/// with the persisted entry keys (`custom:<profile id>` for a custom agent) in toolbar order. The
/// Tree plugin draws the editor; the kernel only saves the result and redraws the tree.
pub(crate) const SET_TREE_TOOLBAR_COMMAND: &str = "kernel.set-tree-toolbar";

/// The buttons and the inactivity minutes (1 to 1440) of a `kernel.set-tree-toolbar` invocation,
/// or why it is refused. Repeated buttons are kept once.
pub(crate) fn tree_toolbar_argument(
    arguments_json: &str,
) -> Result<(Vec<crate::tree_toolbar_layout::TreeToolbarEntry>, u16), String> {
    const USAGE: &str =
        "kernel.set-tree-toolbar takes {\"entries\": [<button key>, ...], \"minutes\": <1 to 1440>}";
    let value: serde_json::Value =
        serde_json::from_str(arguments_json).map_err(|error| format!("{USAGE}: {error}"))?;
    let entries = value
        .get("entries")
        .cloned()
        .ok_or_else(|| USAGE.to_owned())?;
    let entries: Vec<crate::tree_toolbar_layout::TreeToolbarEntry> =
        serde_json::from_value(entries).map_err(|error| format!("{USAGE}: {error}"))?;
    let minutes = value
        .get("minutes")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| USAGE.to_owned())?;
    let layout = crate::tree_toolbar_layout::TreeToolbarLayout {
        entries,
        ..Default::default()
    }
    .normalized();
    Ok((layout.entries, minutes.clamp(1, 1440) as u16))
}

/// The kernel command that sets the panel padding: the whole padding table as JSON (`mode`,
/// `width_threshold`, `right_panels_shared` and one `{linked, all, sides}` object per panel kind).
/// The Layout plugin owns the stored values and the settings page; the kernel only applies them
/// to the geometry it draws with.
pub(crate) const SET_PADDING_COMMAND: &str = "kernel.set-padding";

/// The padding of a `kernel.set-padding` invocation, or why it is refused.
pub(crate) fn padding_argument(
    arguments_json: &str,
) -> Result<crate::padding::PaddingSettings, String> {
    crate::padding::PaddingSettings::from_json(arguments_json)
        .map_err(|reason| format!("kernel.set-padding: {reason}"))
}

/// The kernel command that sets an option of an editor pane:
/// `{"pane": <id>?, "option": "<name>", "value": <value>}`.
pub(crate) const SET_EDITOR_OPTION_COMMAND: &str = "kernel.set-editor-option";

/// A per-pane editor option and its wanted value. Each is a toggle of the editor toolbar, so
/// it changes presentation only, never the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorOption {
    /// `line-display`: `"clip"` or `"wrap"`.
    LineDisplay(crate::config::LineDisplay),
    /// `line-numbers`: a boolean.
    LineNumbers(bool),
    /// `minimap`: a boolean.
    Minimap(bool),
    /// `autosave`: a boolean.
    Autosave(bool),
}

/// The pane and option of a `kernel.set-editor-option` invocation, or why it is refused.
pub(crate) fn editor_option_argument(
    arguments_json: &str,
) -> Result<(Option<ilium_core::NodeId>, EditorOption), String> {
    const USAGE: &str =
        "kernel.set-editor-option takes {\"pane\": <id>, \"option\": <name>, \"value\": <value>}";
    let value: serde_json::Value =
        serde_json::from_str(arguments_json).map_err(|_| USAGE.to_owned())?;
    let fields = value.as_object().ok_or_else(|| USAGE.to_owned())?;
    let pane = match fields.get("pane") {
        None | Some(serde_json::Value::Null) => None,
        Some(pane) => Some(ilium_core::NodeId(
            pane.as_u64()
                .ok_or_else(|| "the pane is a node id".to_owned())?,
        )),
    };
    let option = fields
        .get("option")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| USAGE.to_owned())?;
    let wanted = fields.get("value").ok_or_else(|| USAGE.to_owned())?;
    let flag = || {
        wanted
            .as_bool()
            .ok_or_else(|| format!("{option} takes true or false"))
    };
    let option = match option {
        "line-display" => match wanted.as_str() {
            Some("clip") => EditorOption::LineDisplay(crate::config::LineDisplay::Clip),
            Some("wrap") => EditorOption::LineDisplay(crate::config::LineDisplay::Wrap),
            _ => return Err("line-display takes \"clip\" or \"wrap\"".to_owned()),
        },
        "line-numbers" => EditorOption::LineNumbers(flag()?),
        "minimap" => EditorOption::Minimap(flag()?),
        "autosave" => EditorOption::Autosave(flag()?),
        other => return Err(format!("{other} is not an editor option")),
    };
    Ok((pane, option))
}

/// How the client treats one intent.
#[derive(Debug)]
pub(crate) enum Classified {
    Effect(ClientEffect),
    /// A toast, as the JSON of its spec.
    Toast(String),
    /// The intent is invalid; the plugin sees this outcome.
    Refused(Outcome),
    /// Not a client-local intent: the server applies it.
    Server(Intent),
}

fn refused(code: ErrorCode, message: impl Into<String>) -> Classified {
    Classified::Refused(Outcome::Refused(PluginError::new(code, message.into())))
}

/// Resolves a plugin path for opening. A parent segment is refused. A relative path joins the
/// scope's base directory. An absolute path (a link a terminal pane printed, joined to the
/// pane's working directory) is accepted only when it lies inside the project or the home
/// directory, whichever scope the plugin named.
fn resolve_path(path: &FsPath, project_dir: &Path, home_dir: Option<&Path>) -> Option<PathBuf> {
    let requested = Path::new(&path.path);
    if requested
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    let base = match path.scope.as_str() {
        "project" => project_dir.to_path_buf(),
        "home" => home_dir?.to_path_buf(),
        _ => return None,
    };
    if requested.is_absolute() {
        let inside_project = requested.starts_with(project_dir);
        let inside_home = home_dir.is_some_and(|home| requested.starts_with(home));
        return (inside_project || inside_home).then(|| requested.to_path_buf());
    }
    if requested
        .components()
        .any(|component| matches!(component, Component::RootDir | Component::Prefix(_)))
    {
        return None;
    }
    Some(base.join(requested))
}

/// The node of a `kernel.focus-node` invocation: `{"node": 5}` or `5`.
pub(crate) fn focus_node_argument(arguments_json: &str) -> Option<ilium_core::NodeId> {
    let value: serde_json::Value = serde_json::from_str(arguments_json).ok()?;
    let id = match &value {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::Object(fields) => fields.get("node").and_then(serde_json::Value::as_u64),
        _ => None,
    }?;
    Some(ilium_core::NodeId(id))
}

fn kernel_command(spec: &InvokeCommandSpec) -> Classified {
    match spec.command.as_str() {
        FOCUS_NODE_COMMAND => match focus_node_argument(&spec.arguments_json) {
            Some(node) => Classified::Effect(ClientEffect::FocusNode(node)),
            None => refused(
                ErrorCode::InvalidArgument,
                "kernel.focus-node takes {\"node\": <id>}",
            ),
        },
        FREEZE_AGENT_COMMAND => match focus_node_argument(&spec.arguments_json) {
            Some(node) => Classified::Effect(ClientEffect::FreezeAgent(node)),
            None => refused(
                ErrorCode::InvalidArgument,
                "kernel.freeze-agent takes {\"node\": <id>}",
            ),
        },
        SET_EDITOR_OPTION_COMMAND => match editor_option_argument(&spec.arguments_json) {
            Ok((pane, option)) => {
                Classified::Effect(ClientEffect::SetEditorOption { pane, option })
            }
            Err(reason) => refused(ErrorCode::InvalidArgument, reason),
        },
        SET_TREE_ROW_LAYOUT_COMMAND => match tree_row_layout_argument(&spec.arguments_json) {
            Ok(layout) => Classified::Effect(ClientEffect::SetTreeRowLayout(layout)),
            Err(reason) => refused(ErrorCode::InvalidArgument, reason),
        },
        SET_TREE_TOOLBAR_COMMAND => match tree_toolbar_argument(&spec.arguments_json) {
            Ok((entries, minutes)) => {
                Classified::Effect(ClientEffect::SetTreeToolbar { entries, minutes })
            }
            Err(reason) => refused(ErrorCode::InvalidArgument, reason),
        },
        SET_PADDING_COMMAND => match padding_argument(&spec.arguments_json) {
            Ok(settings) => Classified::Effect(ClientEffect::SetPadding(settings)),
            Err(reason) => refused(ErrorCode::InvalidArgument, reason),
        },
        other => refused(
            ErrorCode::NotFound,
            format!("{other} is not a kernel command"),
        ),
    }
}

fn mark_attention(spec: MarkAttentionSpec) -> Classified {
    if spec.reason.chars().count() > MAXIMUM_ATTENTION_REASON_CHARACTERS {
        return refused(
            ErrorCode::InvalidArgument,
            "an attention reason is at most 80 characters",
        );
    }
    Classified::Effect(ClientEffect::MarkAttention {
        node: ilium_core::NodeId(spec.node.0),
        reason: spec.reason,
    })
}

fn toast(spec: &ToastSpec) -> Classified {
    if spec.title.chars().count() > MAXIMUM_TOAST_TITLE_CHARACTERS {
        return refused(
            ErrorCode::InvalidArgument,
            "a toast title is at most 60 characters",
        );
    }
    if spec
        .duration_ms
        .is_some_and(|duration| !TOAST_DURATION_MS.contains(&duration))
    {
        return refused(
            ErrorCode::InvalidArgument,
            "a toast lasts 1,000 to 30,000 ms",
        );
    }
    match serde_json::to_string(spec) {
        Ok(json) => Classified::Toast(json),
        Err(error) => Classified::Refused(Outcome::Failed(PluginError::new(
            ErrorCode::Internal,
            format!("the toast does not serialize: {error}"),
        ))),
    }
}

fn open_external(
    target: ExternalTarget,
    project_dir: &Path,
    home_dir: Option<&Path>,
) -> Classified {
    match target {
        ExternalTarget::Url(url) => {
            let lowered = url.to_ascii_lowercase();
            if OPENABLE_SCHEMES
                .iter()
                .any(|scheme| lowered.starts_with(scheme))
            {
                Classified::Effect(ClientEffect::OpenExternal(
                    crate::open_target::OpenTarget::Url(url),
                ))
            } else {
                refused(
                    ErrorCode::NotPermitted,
                    "only http, https and mailto URLs open externally",
                )
            }
        }
        ExternalTarget::File(path) => match resolve_path(&path, project_dir, home_dir) {
            Some(path) if path.is_dir() => Classified::Effect(ClientEffect::OpenExternal(
                crate::open_target::OpenTarget::Directory(path),
            )),
            Some(path) => Classified::Effect(ClientEffect::OpenExternal(
                crate::open_target::OpenTarget::File(path),
            )),
            None => refused(
                ErrorCode::InvalidArgument,
                "the file path must stay inside the project or home scope",
            ),
        },
        ExternalTarget::Settings(page) => Classified::Effect(ClientEffect::OpenSettings(page)),
    }
}

/// An `editor-open` of a plugin: the path must stay inside the project or home scope.
fn editor_open(spec: EditorOpenSpec, project_dir: &Path, home_dir: Option<&Path>) -> Classified {
    match resolve_path(&spec.path, project_dir, home_dir) {
        Some(path) => Classified::Effect(ClientEffect::EditorOpen {
            path,
            line: spec.line.map(|line| line.saturating_add(1)),
            placement: spec.placement,
        }),
        None => refused(
            ErrorCode::InvalidArgument,
            "the file path must stay inside the project or home scope",
        ),
    }
}

/// Classifies one intent for the client. `project_dir` and `home_dir` resolve file paths.
pub(crate) fn classify(intent: Intent, project_dir: &Path, home_dir: Option<&Path>) -> Classified {
    match intent {
        Intent::InvokeCommand(spec) if spec.command.starts_with("kernel.") => kernel_command(&spec),
        Intent::MarkAttention(spec) => mark_attention(spec),
        Intent::AckAttention(node) => {
            Classified::Effect(ClientEffect::AckAttention(ilium_core::NodeId(node.0)))
        }
        Intent::Toast(spec) => toast(&spec),
        Intent::DismissToast(id) => Classified::Effect(ClientEffect::DismissToast(id)),
        Intent::OpenExternal(spec) => open_external(spec.target, project_dir, home_dir),
        // The editor is drawn by the client, so a file opens where the user sits.
        Intent::EditorOpen(spec) => editor_open(spec, project_dir, home_dir),
        other => Classified::Server(other),
    }
}

/// The text a toast shows in the status line: its title, then its body text.
pub(crate) fn toast_text(toast_json: &str) -> Option<String> {
    let spec: ToastSpec = serde_json::from_str(toast_json).ok()?;
    let body: String = spec
        .body
        .iter()
        .flatten()
        .map(|span| span.text.as_str())
        .collect();
    Some(if body.is_empty() {
        spec.title
    } else {
        format!("{}: {body}", spec.title)
    })
}

/// The id of a toast, when it has one.
pub(crate) fn toast_id(toast_json: &str) -> Option<String> {
    serde_json::from_str::<ToastSpec>(toast_json).ok()?.id
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_api::generated::types::intents::OpenExternalSpec;
    use ilium_plugin_api::generated::types::{NodeId, Tone};

    fn classify_here(intent: Intent) -> Classified {
        classify(
            intent,
            Path::new("/work/project"),
            Some(Path::new("/home/someone")),
        )
    }

    #[test]
    fn focus_node_reads_an_object_or_a_bare_id() {
        assert_eq!(
            focus_node_argument("{\"node\": 12}"),
            Some(ilium_core::NodeId(12))
        );
        assert_eq!(focus_node_argument("7"), Some(ilium_core::NodeId(7)));
        assert_eq!(focus_node_argument("{}"), None);
        assert_eq!(focus_node_argument("not json"), None);
        let intent = Intent::InvokeCommand(InvokeCommandSpec {
            command: FOCUS_NODE_COMMAND.to_owned(),
            arguments_json: "{\"node\": 3}".to_owned(),
        });
        assert!(matches!(
            classify_here(intent),
            Classified::Effect(ClientEffect::FocusNode(ilium_core::NodeId(3)))
        ));
    }

    #[test]
    fn freeze_agent_names_a_node() {
        let freeze = |arguments_json: &str| {
            classify_here(Intent::InvokeCommand(InvokeCommandSpec {
                command: FREEZE_AGENT_COMMAND.to_owned(),
                arguments_json: arguments_json.to_owned(),
            }))
        };
        assert!(matches!(
            freeze("{\"node\": 4}"),
            Classified::Effect(ClientEffect::FreezeAgent(ilium_core::NodeId(4)))
        ));
        assert!(matches!(freeze("{}"), Classified::Refused(_)));
    }

    #[test]
    fn set_tree_row_layout_takes_a_normalized_slot_order() {
        use crate::tree_row_layout::{TreeRowElement, TreeRowLayout};
        let set = |arguments_json: &str| {
            classify_here(Intent::InvokeCommand(InvokeCommandSpec {
                command: SET_TREE_ROW_LAYOUT_COMMAND.to_owned(),
                arguments_json: arguments_json.to_owned(),
            }))
        };
        let Classified::Effect(ClientEffect::SetTreeRowLayout(layout)) =
            set(r#"{"order": ["title", "identity"], "hidden": ["title", "activity"]}"#)
        else {
            panic!("a valid layout is an effect");
        };
        assert_eq!(
            layout,
            TreeRowLayout {
                order: vec![
                    TreeRowElement::Title,
                    TreeRowElement::Identity,
                    TreeRowElement::Objective,
                    TreeRowElement::Activity,
                ],
                hidden: vec![TreeRowElement::Activity],
            }
        );
        assert!(matches!(
            set(r#"{"order": ["nope"]}"#),
            Classified::Refused(_)
        ));
        assert!(matches!(set("[]"), Classified::Refused(_)));
    }

    #[test]
    fn set_padding_takes_a_whole_checked_padding_table() {
        use crate::padding::{PaddingMode, PaddingSettings};
        let set = |arguments_json: &str| {
            classify_here(Intent::InvokeCommand(InvokeCommandSpec {
                command: SET_PADDING_COMMAND.to_owned(),
                arguments_json: arguments_json.to_owned(),
            }))
        };
        let Classified::Effect(ClientEffect::SetPadding(settings)) =
            set(r#"{"mode": "width_dependent", "width_threshold": 150}"#)
        else {
            panic!("a valid padding table is an effect");
        };
        assert_eq!(settings.mode, PaddingMode::WidthDependent);
        assert_eq!(settings.width_threshold, 150);
        assert_eq!(settings.tree, PaddingSettings::default().tree);
        assert!(matches!(
            set(r#"{"width_threshold": 5}"#),
            Classified::Refused(_)
        ));
        assert!(matches!(set(r#"{"mode": 3}"#), Classified::Refused(_)));
        assert!(matches!(set("[]"), Classified::Refused(_)));
    }

    #[test]
    fn set_tree_toolbar_takes_button_keys_and_checked_minutes() {
        use crate::tree_toolbar_layout::TreeToolbarEntry;
        let set = |arguments_json: &str| {
            classify_here(Intent::InvokeCommand(InvokeCommandSpec {
                command: SET_TREE_TOOLBAR_COMMAND.to_owned(),
                arguments_json: arguments_json.to_owned(),
            }))
        };
        let Classified::Effect(ClientEffect::SetTreeToolbar { entries, minutes }) = set(
            r#"{"entries": ["settings", "custom:mine", "settings", "search"], "minutes": 5000}"#,
        ) else {
            panic!("a valid toolbar is an effect");
        };
        assert_eq!(
            entries,
            vec![
                TreeToolbarEntry::Settings,
                TreeToolbarEntry::CustomAgent("mine".into()),
                TreeToolbarEntry::Search,
            ]
        );
        assert_eq!(minutes, 1440);
        assert!(matches!(
            set(r#"{"entries": [], "minutes": 0}"#),
            Classified::Effect(ClientEffect::SetTreeToolbar { minutes: 1, .. })
        ));
        for bad in [
            r#"{"entries": ["nope"], "minutes": 5}"#,
            r#"{"entries": ["search"]}"#,
            r#"{"minutes": 5}"#,
            "[]",
            "",
        ] {
            assert!(matches!(set(bad), Classified::Refused(_)), "{bad}");
        }
    }

    fn set_editor_option(arguments_json: &str) -> Classified {
        classify_here(Intent::InvokeCommand(InvokeCommandSpec {
            command: SET_EDITOR_OPTION_COMMAND.to_owned(),
            arguments_json: arguments_json.to_owned(),
        }))
    }

    #[test]
    fn set_editor_option_names_a_pane_option_and_value() {
        assert!(matches!(
            set_editor_option(r#"{"pane": 9, "option": "line-display", "value": "wrap"}"#),
            Classified::Effect(ClientEffect::SetEditorOption {
                pane: Some(ilium_core::NodeId(9)),
                option: EditorOption::LineDisplay(crate::config::LineDisplay::Wrap),
            })
        ));
        assert!(matches!(
            set_editor_option(r#"{"option": "minimap", "value": false}"#),
            Classified::Effect(ClientEffect::SetEditorOption {
                pane: None,
                option: EditorOption::Minimap(false),
            })
        ));
        assert_eq!(
            editor_option_argument(r#"{"option": "line-numbers", "value": true}"#),
            Ok((None, EditorOption::LineNumbers(true)))
        );
        assert_eq!(
            editor_option_argument(r#"{"option": "autosave", "value": false}"#),
            Ok((None, EditorOption::Autosave(false)))
        );
        for refused in [
            r#"{"option": "line-display", "value": "fold"}"#,
            r#"{"option": "minimap", "value": "yes"}"#,
            r#"{"option": "font-size", "value": 3}"#,
            r#"{"pane": "nine", "option": "minimap", "value": true}"#,
            r#"{"option": "minimap"}"#,
            "not json",
        ] {
            assert!(
                matches!(set_editor_option(refused), Classified::Refused(_)),
                "{refused} must be refused"
            );
        }
    }

    #[test]
    fn an_unknown_kernel_command_is_refused_and_a_plugin_command_goes_on() {
        let kernel = Intent::InvokeCommand(InvokeCommandSpec {
            command: "kernel.reboot".to_owned(),
            arguments_json: String::new(),
        });
        assert!(matches!(classify_here(kernel), Classified::Refused(_)));
        let plugin = Intent::InvokeCommand(InvokeCommandSpec {
            command: "acme/run".to_owned(),
            arguments_json: String::new(),
        });
        assert!(matches!(classify_here(plugin), Classified::Server(_)));
    }

    #[test]
    fn attention_marks_are_bounded() {
        let ok = Intent::MarkAttention(MarkAttentionSpec {
            node: NodeId(4),
            reason: "tests failed".to_owned(),
        });
        assert!(matches!(
            classify_here(ok),
            Classified::Effect(ClientEffect::MarkAttention {
                node: ilium_core::NodeId(4),
                ..
            })
        ));
        let long = Intent::MarkAttention(MarkAttentionSpec {
            node: NodeId(4),
            reason: "x".repeat(81),
        });
        assert!(matches!(classify_here(long), Classified::Refused(_)));
        assert!(matches!(
            classify_here(Intent::AckAttention(NodeId(4))),
            Classified::Effect(ClientEffect::AckAttention(ilium_core::NodeId(4)))
        ));
    }

    #[test]
    fn only_safe_urls_and_scoped_files_open() {
        let url = |text: &str| {
            Intent::OpenExternal(OpenExternalSpec {
                target: ExternalTarget::Url(text.to_owned()),
            })
        };
        assert!(matches!(
            classify_here(url("https://example.org")),
            Classified::Effect(ClientEffect::OpenExternal(_))
        ));
        assert!(matches!(
            classify_here(url("file:///etc/passwd")),
            Classified::Refused(_)
        ));
        let file = |path: &str| {
            Intent::OpenExternal(OpenExternalSpec {
                target: ExternalTarget::File(FsPath {
                    scope: "project".to_owned(),
                    path: path.to_owned(),
                }),
            })
        };
        assert!(matches!(
            classify_here(file("README.md")),
            Classified::Effect(ClientEffect::OpenExternal(crate::open_target::OpenTarget::File(path)))
                if path == Path::new("/work/project/README.md")
        ));
        assert!(matches!(
            classify_here(file("../secret")),
            Classified::Refused(_)
        ));
    }

    #[test]
    fn editor_open_resolves_the_scope_and_makes_the_line_one_based() {
        let open = |scope: &str, path: &str, line: Option<u32>| {
            classify_here(Intent::EditorOpen(EditorOpenSpec {
                path: FsPath {
                    scope: scope.to_owned(),
                    path: path.to_owned(),
                },
                line,
                placement: EditorPlacement::Split,
            }))
        };
        assert!(matches!(
            open("project", "src/main.rs", Some(41)),
            Classified::Effect(ClientEffect::EditorOpen { path, line: Some(42), placement: EditorPlacement::Split })
                if path == Path::new("/work/project/src/main.rs")
        ));
        assert!(matches!(
            open("home", "notes.md", None),
            Classified::Effect(ClientEffect::EditorOpen { path, line: None, .. })
                if path == Path::new("/home/someone/notes.md")
        ));
        assert!(matches!(
            open("project", "../outside.rs", Some(0)),
            Classified::Refused(_)
        ));
        assert!(matches!(
            open("project", "//example.org", Some(0)),
            Classified::Refused(_)
        ));
        // An absolute path is accepted only inside the project or the home directory.
        assert!(matches!(
            open("project", "/work/project/crate/src/lib.rs", Some(0)),
            Classified::Effect(ClientEffect::EditorOpen { path, .. })
                if path == Path::new("/work/project/crate/src/lib.rs")
        ));
        assert!(matches!(
            open("project", "/home/someone/.bashrc", None),
            Classified::Effect(ClientEffect::EditorOpen { path, .. })
                if path == Path::new("/home/someone/.bashrc")
        ));
        assert!(matches!(
            open("project", "/etc/passwd", None),
            Classified::Refused(_)
        ));
        assert!(matches!(
            open("project", "/work/project/../secret", None),
            Classified::Refused(_)
        ));
        assert!(matches!(
            open("cache", "x.rs", Some(0)),
            Classified::Refused(_)
        ));
    }

    #[test]
    fn a_toast_keeps_its_id_and_text() {
        let spec = ToastSpec {
            id: Some("build".to_owned()),
            tone: Tone::Info,
            title: "Build failed".to_owned(),
            body: None,
            actions: Vec::new(),
            duration_ms: Some(5_000),
        };
        let Classified::Toast(json) = classify_here(Intent::Toast(spec.clone())) else {
            panic!("a valid toast is shown");
        };
        assert_eq!(toast_id(&json).as_deref(), Some("build"));
        assert_eq!(toast_text(&json).as_deref(), Some("Build failed"));
        let short = ToastSpec {
            duration_ms: Some(10),
            ..spec
        };
        assert!(matches!(
            classify_here(Intent::Toast(short)),
            Classified::Refused(_)
        ));
    }
}
