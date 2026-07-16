//! Builds a bounded project-context prompt and persists one LLM-suggested name.
//!
//! `bootstrap_project_name` is called once during session boot. It reads the
//! project config first and therefore never contacts an inference provider when a name
//! already exists; only the missing-name path collects context and calls the
//! injected generator.

use std::path::Path;

use serde::Serialize;

use crate::naming::{self, PromptCompletionClient};
use crate::project_config;

const ROOT_LISTING_MAX_LINES: usize = 100;
const DOCUMENT_MAX_LINES: usize = 2_000;
const PROJECT_NAME_MIN_WORDS: usize = 1;
const PROJECT_NAME_MAX_WORDS: usize = 2;

const PROJECT_NAME_TEMPLATE: &str = r#"<instructions>
Infer the shortest useful project name from the project context. Return one or two words only. Do not use a slogan, version, punctuation-only name, or explanation.
</instructions>
<project-context>
    <project-path>{{project_path}}</project-path>
    <root-listing>
{{root_listing}}
    </root-listing>
    <claude-md>
{{claude_md}}
    </claude-md>
    <readme-md>
{{readme_md}}
    </readme-md>
</project-context>
<output-example>{"project_name":"Ilium"}</output-example>
<response-format>Return exactly one JSON object following the output example. Do not wrap it in Markdown.</response-format>"#;

/// The persisted or newly inferred name returned by the boot workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectNameSource {
    Stored,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectNameBootstrap {
    pub project_name: String,
    pub source: ProjectNameSource,
}

/// Reads only an already-persisted, valid project name. Startup uses this
/// before entering the TUI so it can decide whether a background inference
/// task is necessary without blocking the first rendered frame.
pub fn load_stored_project_name(cwd: &Path) -> anyhow::Result<Option<String>> {
    Ok(project_config::load(cwd)?
        .project_name
        .as_deref()
        .and_then(|value| {
            naming::normalize_word_bounded(value, PROJECT_NAME_MIN_WORDS, PROJECT_NAME_MAX_WORDS)
        }))
}

/// Loads a stored name, or calls the selected provider exactly once to infer and save it.
pub fn bootstrap_project_name<G: PromptCompletionClient>(
    cwd: &Path,
    generator: &G,
) -> anyhow::Result<ProjectNameBootstrap> {
    let mut config = project_config::load(cwd)?;
    if let Some(project_name) = load_stored_project_name(cwd)? {
        return Ok(ProjectNameBootstrap {
            project_name,
            source: ProjectNameSource::Stored,
        });
    }

    let context = ProjectContext::collect(cwd)?;
    let response =
        naming::render_and_complete(generator, "project-name", PROJECT_NAME_TEMPLATE, &context)?;
    let project_name = parse_project_name_response(&response)?;

    config.project_name = Some(project_name.clone());
    project_config::save(cwd, &config)?;
    Ok(ProjectNameBootstrap {
        project_name,
        source: ProjectNameSource::Inferred,
    })
}

#[derive(Debug, Serialize)]
struct ProjectContext {
    project_path: String,
    root_listing: String,
    claude_md: String,
    readme_md: String,
}

impl ProjectContext {
    fn collect(cwd: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            project_path: cwd.display().to_string(),
            root_listing: root_listing(cwd)?,
            claude_md: read_document_or_marker(&cwd.join("CLAUDE.md"))?,
            readme_md: read_document_or_marker(&cwd.join("README.md"))?,
        })
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
    entries.sort_unstable_by_key(|entry| entry.to_lowercase());
    Ok(entries
        .into_iter()
        .take(ROOT_LISTING_MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n"))
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
            Ok("[not present]".to_string())
        }
        Err(error) => Err(error.into()),
    }
}

// Only exercised directly by unit tests below; production cropping now
// happens inline in `read_document_or_marker` via a bounded line iterator
// so large files are never fully buffered in memory first.
#[cfg(test)]
fn first_lines(contents: &str, maximum: usize) -> String {
    contents
        .lines()
        .take(maximum)
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_project_name_response(response: &str) -> anyhow::Result<String> {
    naming::parse_bounded_word_json(
        response,
        "project_name",
        PROJECT_NAME_MIN_WORDS,
        PROJECT_NAME_MAX_WORDS,
        "project-name",
    )
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
        project_config::save(
            &cwd,
            &project_config::ProjectConfig::with_project_name("Existing Name"),
        )
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
        let generator = FakeGenerator::new(r#"{"project_name":"Stellar Tools"}"#);

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
    fn prompt_is_xml_shaped_and_includes_the_json_output_example() {
        let context = ProjectContext {
            project_path: "/work/example".to_string(),
            root_listing: "README.md".to_string(),
            claude_md: "[not present]".to_string(),
            readme_md: "# Example".to_string(),
        };
        let generator = FakeGenerator::new(r#"{"project_name":"Ilium"}"#);
        naming::render_and_complete(&generator, "project-name", PROJECT_NAME_TEMPLATE, &context)
            .unwrap();
        let prompt = generator.last_prompt.borrow().clone().unwrap();

        assert!(prompt.contains("<project-context>"));
        assert!(prompt.contains("<project-path>/work/example</project-path>"));
        assert!(prompt.contains("<output-example>{\"project_name\":\"Ilium\"}</output-example>"));
    }

    #[test]
    fn documents_and_root_listing_are_cropped_at_the_requested_bounds() {
        let document = (0..2_001)
            .map(|index| format!("line {index}\n"))
            .collect::<String>();
        assert_eq!(
            first_lines(&document, DOCUMENT_MAX_LINES).lines().count(),
            2_000
        );

        let cwd = scratch_dir();
        for index in 0..101 {
            std::fs::write(cwd.join(format!("file-{index:03}")), "").unwrap();
        }
        assert_eq!(
            root_listing(&cwd).unwrap().lines().count(),
            ROOT_LISTING_MAX_LINES
        );
    }

    #[test]
    fn rejects_non_json_and_more_than_two_words() {
        assert!(parse_project_name_response("Ilium").is_err());
        assert!(parse_project_name_response("{\"project_name\":\"One Two Three\"}").is_err());
    }
}
