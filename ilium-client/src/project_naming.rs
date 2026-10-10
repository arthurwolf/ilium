//! Builds a bounded project-context prompt and names a project.
//!
//! Direct `bootstrap_project_name` callers retain the persisting workflow.
//! Background workers use a proposal-only path so the event loop can reject
//! stale or opted-out results before writing project configuration.

use std::path::Path;

use serde::Serialize;

use crate::naming::{self, PromptCompletionClient};
use crate::project_config;

const ROOT_LISTING_MAX_LINES: usize = 100;
const DOCUMENT_MAX_LINES: usize = 2_000;
const PROJECT_NAME_MIN_WORDS: usize = 1;
const PROJECT_NAME_MAX_WORDS: usize = 2;

const PROJECT_NAME_TEMPLATE: &str = ilium_prompts::naming::PROJECT_NAME;

/// The persisted or newly inferred name returned by the boot workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectNameSource {
    Stored,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectNameBootstrap {
    pub project_name: String,
    pub icon: Option<String>,
    pub source: ProjectNameSource,
}

/// Reads only an already-persisted, valid project name. Startup uses this
/// before entering the TUI so it can decide whether a background inference
/// task is necessary without blocking the first rendered frame.
pub fn load_stored_project_name(cwd: &Path) -> anyhow::Result<Option<String>> {
    Ok(stored_project_name(&project_config::load(cwd)?))
}

/// Extracts and validates the project name already present in a loaded
/// config, without re-reading from disk. Shared by `load_stored_project_name`
/// (which has no config in hand yet and must load one) and
/// `bootstrap_project_name` (which already holds one loaded snapshot) so the
/// latter never issues a second `project_config::load` just to re-derive a
/// value it could read off the config it already has -- a second read could
/// race a concurrent writer and pair a stale name with an icon read from a
/// different snapshot of the file.
fn stored_project_name(config: &project_config::ProjectConfig) -> Option<String> {
    config.project_name.as_deref().and_then(|value| {
        naming::normalize_word_bounded(value, PROJECT_NAME_MIN_WORDS, PROJECT_NAME_MAX_WORDS)
    })
}

/// Reads the optional icon stored alongside a valid project name. Older
/// project configs deliberately remain title-only until fresh inference.
pub fn load_stored_project_icon(cwd: &Path) -> anyhow::Result<Option<String>> {
    Ok(project_config::load(cwd)?
        .project_icon
        .as_deref()
        .and_then(naming::normalize_icon))
}

/// Loads a stored name, or calls the selected provider exactly once to infer and save it.
/// Kept as the public, persisting workflow for existing direct callers.
pub fn bootstrap_project_name<G: PromptCompletionClient>(
    cwd: &Path,
    generator: &G,
) -> anyhow::Result<ProjectNameBootstrap> {
    let proposal = infer_project_name_without_persisting(cwd, generator)?;
    persist_inferred_project_name(cwd, proposal)
}

/// Computes one project-name proposal without writing project configuration.
/// Background workers use this so a late result can be rejected by the
/// client event loop before *any* inferred name reaches disk.
pub(crate) fn infer_project_name_without_persisting<G: PromptCompletionClient>(
    cwd: &Path,
    generator: &G,
) -> anyhow::Result<ProjectNameBootstrap> {
    let config = project_config::load(cwd)?;
    if let Some(project_name) = stored_project_name(&config) {
        return Ok(ProjectNameBootstrap {
            project_name,
            icon: config
                .project_icon
                .and_then(|icon| naming::normalize_icon(&icon)),
            source: ProjectNameSource::Stored,
        });
    }

    let mut context = ProjectContext::collect(cwd)?.encoded();
    let instructions = generator.prompt_instructions();
    context.project_naming = instructions.project_naming.trim().to_owned();
    context.naming_and_organization = instructions.naming_and_organization.trim().to_owned();
    let (project_name, icon) = naming::render_complete_and_parse(
        generator,
        "project-name",
        PROJECT_NAME_TEMPLATE,
        &context,
        parse_project_name_response,
    )?;

    Ok(ProjectNameBootstrap {
        project_name,
        icon: Some(icon),
        source: ProjectNameSource::Inferred,
    })
}

/// Commits an accepted inference after the event loop checks the current AI
/// decision and the worker's generation. A valid name written by another
/// client while inference was in flight wins under the project-config lock.
pub(crate) fn persist_inferred_project_name(
    cwd: &Path,
    proposal: ProjectNameBootstrap,
) -> anyhow::Result<ProjectNameBootstrap> {
    persist_inferred_project_name_with_fence(cwd, proposal, None)
}

pub(crate) fn persist_inferred_project_name_with_fence(
    cwd: &Path,
    proposal: ProjectNameBootstrap,
    decision: Option<&crate::naming_workers::AutomaticAiDecisionFence>,
) -> anyhow::Result<ProjectNameBootstrap> {
    if proposal.source == ProjectNameSource::Stored {
        return Ok(proposal);
    }
    let mut accepted = proposal.clone();
    let updated = project_config::update_if(cwd, |config| {
        if decision.is_some_and(|fence| !fence.is_current()) {
            return false;
        }
        if let Some(project_name) = stored_project_name(config) {
            accepted = ProjectNameBootstrap {
                project_name,
                icon: config
                    .project_icon
                    .as_deref()
                    .and_then(naming::normalize_icon),
                source: ProjectNameSource::Stored,
            };
        } else {
            config.project_name = Some(proposal.project_name.clone());
            config.project_icon = proposal.icon.clone();
        }
        true
    })?;
    if !updated {
        anyhow::bail!("Automatic project naming decision changed before persistence");
    }
    Ok(accepted)
}

#[derive(Debug, Serialize)]
struct ProjectContext {
    project_naming: String,
    naming_and_organization: String,
    project_path: String,
    root_listing: String,
    claude_md: String,
    readme_md: String,
}

impl ProjectContext {
    fn collect(cwd: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            project_naming: String::new(),
            naming_and_organization: String::new(),
            project_path: cwd.display().to_string(),
            root_listing: root_listing(cwd)?,
            claude_md: read_document_or_marker(&cwd.join("CLAUDE.md"))?,
            readme_md: read_document_or_marker(&cwd.join("README.md"))?,
        })
    }

    fn encoded(self) -> Self {
        Self {
            project_naming: self.project_naming,
            naming_and_organization: self.naming_and_organization,
            project_path: naming::encode_untrusted_context(&self.project_path),
            root_listing: naming::encode_untrusted_context(&self.root_listing),
            claude_md: naming::encode_untrusted_context(&self.claude_md),
            readme_md: naming::encode_untrusted_context(&self.readme_md),
        }
    }
}

fn root_listing(cwd: &Path) -> anyhow::Result<String> {
    let mut entries: Vec<String> = std::fs::read_dir(cwd)?
        .filter_map(Result::ok)
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            match entry.file_type() {
                Ok(file_type) if file_type.is_dir() => format!("{name}/"),
                Ok(file_type) if file_type.is_symlink() => format!("{name}@"),
                _ => name,
            }
        })
        .collect();
    // Sort before capping, not after: `read_dir` order is filesystem/hash
    // order, not alphabetical, so truncating first would keep an arbitrary
    // subset of entries -- easily dropping exactly the files (README.md,
    // Cargo.toml, CLAUDE.md) this listing exists to surface to the model.
    entries.sort_unstable_by_key(|entry| entry.to_lowercase());
    entries.truncate(ROOT_LISTING_MAX_LINES);
    Ok(entries.join("\n"))
}

/// Reads at most `DOCUMENT_MAX_LINES` lines of `path` without materializing
/// the whole file in memory first. CLAUDE.md/README.md are user-controlled
/// project content and may be arbitrarily large, so this streams line-by-line
/// and stops as soon as the cap is reached instead of buffering the entire
/// file (which `read_to_string` followed by truncation would do).
fn read_document_or_marker(path: &Path) -> anyhow::Result<String> {
    use std::io::BufRead;

    match std::fs::File::open(path) {
        Ok(file) => {
            let reader = std::io::BufReader::new(file);
            let mut lines = Vec::with_capacity(DOCUMENT_MAX_LINES.min(256));
            for line in reader.lines().take(DOCUMENT_MAX_LINES) {
                lines.push(line?);
            }
            Ok(lines.join("\n"))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ilium_prompts::naming::NAMING_PROJECT_NAMING_NOT_PRESENT.to_string())
        }
        Err(error) => Err(error.into()),
    }
}

fn parse_project_name_response(response: &str) -> anyhow::Result<(String, String)> {
    let parsed = naming::parse_structured_json_object(response, "project-name")?;
    let name = naming::parse_bounded_word_json(
        response,
        "project_name",
        PROJECT_NAME_MIN_WORDS,
        PROJECT_NAME_MAX_WORDS,
        "project-name",
    )?;
    Ok((
        name,
        naming::extract_icon_field(&parsed, "icon", "project-name")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_inference::InferenceError;
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;

    struct FakeGenerator {
        calls: Cell<u8>,
        last_prompt: RefCell<Option<String>>,
        response: String,
    }

    impl FakeGenerator {
        fn new(response: impl Into<String>) -> Self {
            Self {
                calls: Cell::new(0),
                last_prompt: RefCell::new(None),
                response: response.into(),
            }
        }
    }

    impl PromptCompletionClient for FakeGenerator {
        fn complete_prompt(&self, prompt: String) -> Result<String, InferenceError> {
            self.calls.set(self.calls.get() + 1);
            *self.last_prompt.borrow_mut() = Some(prompt);
            Ok(self.response.clone())
        }
    }

    fn scratch_dir() -> PathBuf {
        let path = std::env::temp_dir()
            .join("ilium-project-naming-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn stored_name_skips_the_gateway_entirely() {
        let cwd = scratch_dir();
        project_config::update(&cwd, |config| {
            config.project_name = Some("Existing Name".to_string());
        })
        .unwrap();
        let generator = FakeGenerator::new(r#"{"project_name":"Wrong"}"#);

        let result = bootstrap_project_name(&cwd, &generator).unwrap();

        assert_eq!(result.project_name, "Existing Name");
        assert_eq!(result.source, ProjectNameSource::Stored);
        assert_eq!(generator.calls.get(), 0);
    }

    #[test]
    fn missing_name_collects_context_then_persists_one_inference() {
        let cwd = scratch_dir();
        std::fs::write(cwd.join("README.md"), "# Stellar tools\n").unwrap();
        let generator = FakeGenerator::new(r#"{"project_name":"Stellar Tools","icon":"✨"}"#);

        let result = bootstrap_project_name(&cwd, &generator).unwrap();

        assert_eq!(result.project_name, "Stellar Tools");
        assert_eq!(result.source, ProjectNameSource::Inferred);
        assert_eq!(generator.calls.get(), 1);
        assert_eq!(
            project_config::load(&cwd).unwrap().project_name.as_deref(),
            Some("Stellar Tools")
        );
    }

    #[test]
    fn inference_only_has_no_project_config_side_effect_until_commit() {
        let cwd = scratch_dir();
        let generator = FakeGenerator::new(r#"{"project_name":"Stellar Tools","icon":"✨"}"#);

        let proposal = infer_project_name_without_persisting(&cwd, &generator).unwrap();

        assert_eq!(proposal.source, ProjectNameSource::Inferred);
        assert_eq!(generator.calls.get(), 1);
        assert!(!cwd.join(".ilium/config.yaml").exists());
        assert_eq!(project_config::load(&cwd).unwrap().project_name, None);

        let committed = persist_inferred_project_name(&cwd, proposal).unwrap();
        assert_eq!(committed.source, ProjectNameSource::Inferred);
        assert_eq!(
            project_config::load(&cwd).unwrap().project_name.as_deref(),
            Some("Stellar Tools")
        );
    }

    #[test]
    fn commit_preserves_a_valid_name_written_during_inference() {
        let cwd = scratch_dir();
        let generator = FakeGenerator::new(r#"{"project_name":"Stellar Tools","icon":"✨"}"#);
        let proposal = infer_project_name_without_persisting(&cwd, &generator).unwrap();
        project_config::update(&cwd, |config| {
            config.project_name = Some("User Choice".to_string());
            config.project_icon = Some("🧭".to_string());
        })
        .unwrap();

        let accepted = persist_inferred_project_name(&cwd, proposal).unwrap();

        assert_eq!(accepted.source, ProjectNameSource::Stored);
        assert_eq!(accepted.project_name, "User Choice");
        assert_eq!(accepted.icon.as_deref(), Some("🧭"));
        let saved = project_config::load(&cwd).unwrap();
        assert_eq!(saved.project_name.as_deref(), Some("User Choice"));
        assert_eq!(saved.project_icon.as_deref(), Some("🧭"));
    }

    #[test]
    fn inference_update_preserves_project_scoped_ui_preferences() {
        let cwd = scratch_dir();
        std::fs::create_dir_all(cwd.join(".ilium")).unwrap();
        std::fs::write(
            cwd.join(".ilium/config.yaml"),
            "show project separators: true\ncustom: preserve\n",
        )
        .unwrap();
        let generator = FakeGenerator::new(r#"{"project_name":"Stellar Tools","icon":"✨"}"#);

        let result = bootstrap_project_name(&cwd, &generator).unwrap();

        let config = project_config::load(&cwd).unwrap();
        assert_eq!(result.project_name, "Stellar Tools");
        assert!(config.show_project_separators);
        assert!(std::fs::read_to_string(cwd.join(".ilium/config.yaml"))
            .unwrap()
            .contains("custom: preserve"));
    }

    #[test]
    fn prompt_is_xml_shaped_and_includes_the_json_output_example() {
        let context = ProjectContext {
            project_naming: String::new(),
            naming_and_organization: String::new(),
            project_path: "/work/example".to_string(),
            root_listing: "README.md".to_string(),
            claude_md: "[not present]".to_string(),
            readme_md: "# Example".to_string(),
        };
        let generator = FakeGenerator::new(r#"{"project_name":"Ilium","icon":"🧭"}"#);
        naming::render_and_complete(
            &generator,
            "project-name",
            PROJECT_NAME_TEMPLATE,
            &context.encoded(),
        )
        .unwrap();
        let prompt = generator.last_prompt.borrow().clone().unwrap();

        assert!(prompt.contains("<project-context>"));
        assert!(prompt.contains("<project-path>\"/work/example\"</project-path>"));
        assert!(prompt.contains(
            "<output-example>{\"project_name\":\"Ilium\",\"icon\":\"🧭\"}</output-example>"
        ));
    }

    #[test]
    fn documents_and_root_listing_are_cropped_at_the_requested_bounds() {
        let cwd = scratch_dir();
        let document = (0..2_001)
            .map(|index| format!("line {index}\n"))
            .collect::<String>();
        std::fs::write(cwd.join("CLAUDE.md"), document).unwrap();
        assert_eq!(
            read_document_or_marker(&cwd.join("CLAUDE.md"))
                .unwrap()
                .lines()
                .count(),
            DOCUMENT_MAX_LINES
        );

        for index in 0..101 {
            std::fs::write(cwd.join(format!("file-{index:03}")), "").unwrap();
        }
        let listing = root_listing(&cwd).unwrap();
        assert_eq!(listing.lines().count(), ROOT_LISTING_MAX_LINES);
        // Sorting happens before the cap, not after: the alphabetically
        // first 100 entries are kept, not an arbitrary filesystem-order
        // subset -- so `file-000` survives and `file-100` is the one
        // dropped, alongside `CLAUDE.md` written above.
        assert!(listing.contains("file-000"));
        assert!(!listing.contains("file-100"));
    }

    #[test]
    fn rejects_non_json_and_more_than_two_words() {
        assert!(parse_project_name_response("Ilium").is_err());
        assert!(parse_project_name_response("{\"project_name\":\"One Two Three\"}").is_err());
    }

    struct InstructionGenerator(FakeGenerator);
    impl PromptCompletionClient for InstructionGenerator {
        fn complete_prompt(&self, prompt: String) -> Result<String, InferenceError> {
            self.0.complete_prompt(prompt)
        }
        fn prompt_instructions(&self) -> ilium_inference::PromptInstructions {
            ilium_inference::PromptInstructions {
                entry_naming: "Entry {{> absent}} <x>&".into(),
                naming_and_organization: "Shared vocabulary".into(),
                project_naming: "Project convention".into(),
                organization: "Only organization".into(),
                ..Default::default()
            }
        }
    }
    #[test]
    fn custom_instructions_reach_real_inference_request() {
        let generator = InstructionGenerator(FakeGenerator::new(
            r#"{"project_name":"Ilium","icon":"🧭"}"#,
        ));
        bootstrap_project_name(&scratch_dir(), &generator).unwrap();
        let prompt = generator.0.last_prompt.borrow();
        let prompt = prompt.as_deref().unwrap();
        assert!(prompt.contains("Shared vocabulary"));
        assert!(!prompt.contains("Only organization"));
        assert!(prompt.contains("Project convention"));
        assert!(!prompt.contains("Entry {{> absent}} <x>&"));
    }
}
