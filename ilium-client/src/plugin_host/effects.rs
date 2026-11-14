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
    ExternalTarget, InvokeCommandSpec, MarkAttentionSpec, ToastSpec,
};
use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::generated::types::{ErrorCode, Intent, Outcome, PluginError};

/// The kernel command that focuses a node (`{"node": <id>}` or a bare id).
pub(crate) const FOCUS_NODE_COMMAND: &str = "kernel.focus-node";
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
    /// Open a kernel Settings page by its id.
    OpenSettings(String),
    /// Remove the toast with this id if it is still shown.
    DismissToast(String),
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

/// Resolves a plugin path for opening. Parent segments and absolute paths are refused.
fn resolve_path(path: &FsPath, project_dir: &Path, home_dir: Option<&Path>) -> Option<PathBuf> {
    let relative = Path::new(&path.path);
    let escapes = relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if escapes {
        return None;
    }
    let base = match path.scope.as_str() {
        "project" => project_dir.to_path_buf(),
        "home" => home_dir?.to_path_buf(),
        _ => return None,
    };
    Some(base.join(relative))
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
