//! On-disk workspace snapshot: `.ilium/sessions.yml` under the session's
//! `--cwd`. Lets a killed/closed ilium be reopened in the same folder and
//! come back "close to" how it was left -- the same group/pane structure,
//! the same editor files reopened, and the same Claude/Codex sessions
//! resumed with their own CLI syntax (for example, `claude --resume <id>`
//! or `codex resume <id>`) rather than started fresh.
//!
//! This module only knows the serializable shape and how to read/write it
//! -- it has no idea what a `PaneRuntime` or `AgentProcessInfo` is. `App`
//! (in `app.rs`) is what walks its own live state to build a
//! `WorkspaceFile` for saving, and what turns a loaded one back into a
//! real tree with spawned panes. That keeps the dependency one-directional
//! (`app.rs` depends on this module, never the reverse) instead of this
//! schema module needing to know about application-level types.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Bumped whenever the shape of `WorkspaceFile` changes incompatibly. Not
/// currently enforced against on load (a mismatched version still loads
/// best-effort via serde's normal field-by-field deserialization) --
/// reserved for the day a breaking schema change needs an explicit
/// "can't read this, starting fresh" decision instead of a silent partial
/// parse.
const CURRENT_VERSION: u32 = 1;

/// Path, relative to the session's cwd, that a workspace snapshot is
/// saved to and loaded from.
const RELATIVE_PATH: &str = ".ilium/sessions.yml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceFile {
    pub version: u32,
    /// Top-level children of the (unsaved, implicit) session root --
    /// almost always all `Group`s, since panes can't live directly under
    /// the root (see `ilium_core::Tree::add_pane`'s own invariant), but
    /// a stray top-level `Terminal`/`Editor` here is tolerated on load by
    /// wrapping it into a default group rather than erroring.
    pub root: Vec<SavedNode>,
}

impl WorkspaceFile {
    pub fn new(root: Vec<SavedNode>) -> Self {
        Self {
            version: CURRENT_VERSION,
            root,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SavedNode {
    Group {
        name: String,
        children: Vec<SavedNode>,
    },
    Terminal {
        name: String,
        /// `None` for a plain shell; `Some` for a pane that was running a
        /// detected agent CLI.
        agent: Option<SavedAgent>,
        /// `None` means this pane's title has never been set by the user
        /// or by auto-inference (see `session_naming`) -- eligible for
        /// auto-inference to run on it. `#[serde(default)]` so a workspace
        /// saved before this field existed loads as `None`, i.e. still
        /// eligible, rather than failing to parse.
        #[serde(default)]
        title_source: Option<TitleSource>,
    },
    Editor {
        name: String,
        /// The file it was editing, if any had been picked yet.
        path: Option<PathBuf>,
    },
}

/// Who established a pane's current title -- distinguishes a title
/// `session_naming` inferred automatically (which a later inference pass
/// should never overwrite once set, but which the user is still free to
/// rename) from one the user typed themselves (which auto-inference must
/// never run for or overwrite; see `App::panes_needing_title_inference`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleSource {
    Auto,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedAgent {
    /// The literal command to invoke to start this agent again --
    /// `"claude"`, `"codex"`, or (for `AgentClass::Other`) whatever name
    /// `identify_agent` actually matched, which is already the right
    /// invocation. A plain string rather than a tagged enum: some
    /// enum-with-data shapes don't round-trip cleanly through YAML when
    /// nested inside a struct (observed directly -- `serde_norway` errors
    /// on it with "untagged and internally tagged enums do not support
    /// enum input"), and a human reading or hand-editing this file finds
    /// `command: claude` clearer than a tagged variant anyway.
    pub command: String,
    /// Known only once one of ilium's detection tiers resolved it; a
    /// pane saved before that happened restores as a fresh (non-resumed)
    /// agent invocation instead.
    pub session_id: Option<String>,
}

/// Writes `workspace` to `<cwd>/.ilium/sessions.yml`, creating the
/// `.ilium` directory if needed. Writes to a temp file in the same
/// directory and renames it into place, so a crash or kill mid-write (the
/// exact scenario this whole feature exists to survive) can never leave a
/// half-written, unparseable save file behind -- the rename is atomic, so
/// the file on disk is always either the previous complete save or the
/// new one, never a partial mix of both.
pub fn save(cwd: &Path, workspace: &WorkspaceFile) -> anyhow::Result<()> {
    let path = cwd.join(RELATIVE_PATH);
    let Some(parent) = path.parent() else {
        anyhow::bail!("save path {path:?} has no parent directory");
    };
    std::fs::create_dir_all(parent)?;

    let yaml = serde_norway::to_string(workspace)?;
    let temp_path = parent.join(format!(".sessions.yml.tmp-{}", std::process::id()));

    // Written as an inner closure so any failure after the temp file was
    // created -- a failed write, a failed fsync, or a failed rename -- can
    // still clean up the stray temp file below, rather than leaving a
    // partial `.sessions.yml.tmp-<pid>` behind forever on an error path.
    let result = (|| -> anyhow::Result<()> {
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(yaml.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp_path, &path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

/// Reads and parses `<cwd>/.ilium/sessions.yml`. `Ok(None)` means no save
/// file exists yet (the common case for a brand-new project) -- distinct
/// from `Err`, which means one exists but couldn't be read or parsed (a
/// corrupted or hand-edited-wrong file), so the caller can tell "nothing
/// to restore" apart from "something to warn about" while still starting
/// up either way.
pub fn load(cwd: &Path) -> anyhow::Result<Option<WorkspaceFile>> {
    let path = cwd.join(RELATIVE_PATH);
    // Read directly and distinguish "doesn't exist" from other I/O errors
    // via the error kind, rather than a separate `path.exists()` check
    // first: `exists()` swallows any error it hits (e.g. permission
    // denied on `.ilium`) and just reports `false`, which would silently
    // turn a real, reportable failure into "nothing to restore" --
    // exactly the ambiguity this function's own contract says `Err` must
    // preserve. Reading directly also removes the TOCTOU window between
    // the existence check and the read.
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let workspace: WorkspaceFile = serde_norway::from_str(&contents)?;
    Ok(Some(workspace))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn scratch_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ilium-workspace-file-tests")
            .join(format!("{:?}", thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn load_missing_file_returns_none() {
        let cwd = scratch_dir();
        assert!(load(&cwd).unwrap().is_none());
    }

    #[test]
    fn load_corrupt_file_returns_err_not_panic() {
        let cwd = scratch_dir();
        std::fs::create_dir_all(cwd.join(".ilium")).unwrap();
        std::fs::write(cwd.join(RELATIVE_PATH), "not: [valid, yaml for this schema").unwrap();
        assert!(load(&cwd).is_err());
    }

    #[test]
    fn save_then_load_round_trips_full_structure() {
        let cwd = scratch_dir();
        let workspace = WorkspaceFile::new(vec![SavedNode::Group {
            name: "default".to_string(),
            children: vec![
                SavedNode::Terminal {
                    name: "shell".to_string(),
                    agent: None,
                    title_source: None,
                },
                SavedNode::Terminal {
                    name: "claude shell".to_string(),
                    agent: Some(SavedAgent {
                        command: "claude".to_string(),
                        session_id: Some("22079c73-778b-40e6-8491-44848552d771".to_string()),
                    }),
                    title_source: Some(TitleSource::Auto),
                },
                SavedNode::Editor {
                    name: "main.rs".to_string(),
                    path: Some(PathBuf::from("/home/developer/dev/ai/ilium/ilium/src/main.rs")),
                },
                SavedNode::Group {
                    name: "nested".to_string(),
                    children: vec![SavedNode::Terminal {
                        name: "codex shell".to_string(),
                        agent: Some(SavedAgent {
                            command: "opencode".to_string(),
                            session_id: None,
                        }),
                        title_source: Some(TitleSource::User),
                    }],
                },
            ],
        }]);

        save(&cwd, &workspace).unwrap();
        let loaded = load(&cwd).unwrap().expect("file was just saved");

        assert_eq!(loaded.version, CURRENT_VERSION);
        assert_eq!(loaded.root.len(), 1);
        let SavedNode::Group { name, children } = &loaded.root[0] else {
            panic!("expected a group at the top level");
        };
        assert_eq!(name, "default");
        assert_eq!(children.len(), 4);
        let SavedNode::Terminal { title_source, .. } = &children[0] else {
            panic!("expected the plain shell terminal");
        };
        assert_eq!(*title_source, None);
        let SavedNode::Terminal { title_source, .. } = &children[1] else {
            panic!("expected the claude terminal");
        };
        assert_eq!(*title_source, Some(TitleSource::Auto));
        let SavedNode::Group { children, .. } = &children[3] else {
            panic!("expected the nested group");
        };
        let SavedNode::Terminal { title_source, .. } = &children[0] else {
            panic!("expected the codex terminal");
        };
        assert_eq!(*title_source, Some(TitleSource::User));
    }

    /// A workspace saved before `title_source` existed must still load --
    /// its terminals come back with `title_source: None`, i.e. still
    /// eligible for auto-inference, not a parse failure.
    #[test]
    fn loading_a_workspace_saved_before_title_source_existed_defaults_it_to_none() {
        let cwd = scratch_dir();
        std::fs::create_dir_all(cwd.join(".ilium")).unwrap();
        std::fs::write(
            cwd.join(RELATIVE_PATH),
            "version: 1\n\
             root:\n\
             - kind: group\n  \
               name: default\n  \
               children:\n  \
               - kind: terminal\n    \
                 name: shell\n    \
                 agent: null\n",
        )
        .unwrap();

        let loaded = load(&cwd).unwrap().expect("file should parse");
        let SavedNode::Group { children, .. } = &loaded.root[0] else {
            panic!("expected a group at the top level");
        };
        let SavedNode::Terminal { title_source, .. } = &children[0] else {
            panic!("expected a terminal");
        };
        assert_eq!(*title_source, None);
    }

    #[test]
    fn save_overwrites_a_previous_snapshot_atomically() {
        let cwd = scratch_dir();
        let first = WorkspaceFile::new(vec![SavedNode::Group {
            name: "first".to_string(),
            children: vec![],
        }]);
        let second = WorkspaceFile::new(vec![SavedNode::Group {
            name: "second".to_string(),
            children: vec![],
        }]);

        save(&cwd, &first).unwrap();
        save(&cwd, &second).unwrap();

        let loaded = load(&cwd).unwrap().unwrap();
        let SavedNode::Group { name, .. } = &loaded.root[0] else {
            panic!("expected a group");
        };
        assert_eq!(name, "second");
        // The temp file used for the atomic rename must never linger.
        let leftovers: Vec<_> = std::fs::read_dir(cwd.join(".ilium"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");
    }
}
