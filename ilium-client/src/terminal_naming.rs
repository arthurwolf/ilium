//! Infers a short-form (2-3 word) and long-form (up-to-7-word) pair of titles
//! for a plain terminal pane from its scrollback text plus its live pane
//! metadata, reusing the same `crate::naming` (Handlebars prompt + selected
//! provider + bounded-word-JSON-reply) pipeline `session_naming` uses for
//! agent panes. `ilium-client`'s tree panel shows the short title when the
//! panel is narrow and the long title when it's wide (see `crate::tree_ui`).
//!
//! Unlike `session_naming`, there is no transcript file to read here -- a
//! plain shell has no concept of a "session." The context is instead the
//! pane's entire scrollback (see `vt100::Screen::full_history_contents_capped`,
//! not just its current viewport -- a terminal's earliest commands are
//! usually the best evidence of what it's generally for) plus the
//! pane/project identity `crate::terminal_title_inference::terminal_title_input`
//! gathers from the tree. `screen_text` arrives here already clipped to a
//! bounded size (both ends kept, see that module) rather than being clipped
//! at this prompt-building boundary the way every other dynamic field is --
//! see `terminal_title_input`'s doc for why.

use std::path::PathBuf;

use ilium_core::NodeId;
use ilium_inference::TitleStyle;
use serde::Serialize;

use crate::naming::{self, BoundedField, DualTitle, PromptCompletionClient};

const TERMINAL_TITLE_SHORT_MIN_WORDS: usize = 2;
const TERMINAL_TITLE_SHORT_MAX_WORDS: usize = 3;
const TERMINAL_TITLE_LONG_MIN_WORDS: usize = 1;
const TERMINAL_TITLE_LONG_MAX_WORDS: usize = 7;

/// Immutable live context captured before the background worker begins --
/// the terminal-pane analogue of `session_naming::SessionTitleInput`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTitleInput {
    pub pane_id: NodeId,
    pub project_name: String,
    pub project_path: PathBuf,
    pub current_title: String,
    pub screen_text: String,
    pub parent_group: String,
    pub nearby_titles: Vec<String>,
}

const SUMMARY_INSTRUCTIONS: &str = ilium_prompts::naming::TERMINAL_SUMMARY;

// Dynamic values are JSON-string encoded before rendering, preserving shell
// characters without allowing screen text to close one of these prompt tags.
const TERMINAL_TITLE_TEMPLATE: &str = ilium_prompts::naming::TERMINAL_TITLE;

/// Clips every dynamic field and asks the selected provider for a short/long
/// title pair. This is the entry point
/// `naming_workers::spawn_terminal_title_worker` spawns a worker thread
/// around.
pub fn infer_terminal_title<G: PromptCompletionClient>(
    generator: &G,
    input: &TerminalTitleInput,
) -> anyhow::Result<DualTitle> {
    // Length-clipped (both ends kept) by `terminal_title_input` already --
    // see its doc for why that happens at capture time here rather than at
    // this prompt-building boundary like every other dynamic field below.
    // Still worth trimming here: an all-whitespace screen is as good as
    // empty regardless of which layer produced it.
    if input.screen_text.trim().is_empty() {
        anyhow::bail!(ilium_prompts::naming::NAMING_TERMINAL_NAMING_NO_SCREEN_CONTENT_AVAILABLE_TO_INFER_A_TERMINAL_TITLE_FROM);
    }

    let instructions = generator.prompt_instructions();
    let context = TerminalTitleContext {
        entry_naming: instructions.entry_naming.trim().to_owned(),
        naming_and_organization: instructions.naming_and_organization.trim().to_owned(),
        style_instructions: if generator.title_style() == TitleStyle::Labeling {
            crate::session_naming::LABEL_INSTRUCTIONS
        } else {
            SUMMARY_INSTRUCTIONS
        },
        output_example: if generator.title_style() == TitleStyle::Labeling {
            ilium_prompts::naming::TERMINAL_LABEL_EXAMPLE
        } else {
            ilium_prompts::naming::TERMINAL_SUMMARY_EXAMPLE
        },
        is_labeling: generator.title_style() == TitleStyle::Labeling,
        pane_id: naming::encode_untrusted_context(&input.pane_id.0.to_string()),
        current_title: naming::encode_untrusted_context(&naming::clip_llm_context_value(
            &input.current_title,
        )),
        project_name: naming::encode_untrusted_context(&naming::clip_llm_context_value(
            &input.project_name,
        )),
        project_path: naming::encode_untrusted_context(&naming::clip_llm_context_value(
            &input.project_path.display().to_string(),
        )),
        parent_group: naming::encode_untrusted_context(&naming::clip_llm_context_value(
            &input.parent_group,
        )),
        nearby_titles: input
            .nearby_titles
            .iter()
            .take(40)
            .map(|title| naming::encode_untrusted_context(&naming::clip_llm_context_value(title)))
            .collect(),
        screen_text: naming::encode_untrusted_context(&input.screen_text),
    };
    naming::render_complete_and_parse(
        generator,
        "terminal-title",
        TERMINAL_TITLE_TEMPLATE,
        &context,
        |response| parse_terminal_title_response_for_style(response, generator.title_style()),
    )
}

#[derive(Debug, Serialize)]
struct TerminalTitleContext {
    entry_naming: String,
    naming_and_organization: String,
    style_instructions: &'static str,
    output_example: &'static str,
    is_labeling: bool,
    pane_id: String,
    current_title: String,
    project_name: String,
    project_path: String,
    parent_group: String,
    nearby_titles: Vec<String>,
    screen_text: String,
}

#[cfg(test)]
fn parse_terminal_title_response(response: &str) -> anyhow::Result<DualTitle> {
    parse_terminal_title_response_for_style(response, TitleStyle::Summarization)
}

fn parse_terminal_title_response_for_style(
    response: &str,
    style: TitleStyle,
) -> anyhow::Result<DualTitle> {
    let title = naming::parse_dual_bounded_word_json(
        response,
        BoundedField {
            field: "terminal_title_short",
            min_words: if style == TitleStyle::Labeling {
                1
            } else {
                TERMINAL_TITLE_SHORT_MIN_WORDS
            },
            max_words: TERMINAL_TITLE_SHORT_MAX_WORDS,
        },
        BoundedField {
            field: "terminal_title_long",
            min_words: TERMINAL_TITLE_LONG_MIN_WORDS,
            max_words: TERMINAL_TITLE_LONG_MAX_WORDS,
        },
        "terminal-title",
    )?;

    // A missing/unparseable "command_hint" is never a reason to fail the
    // whole title inference -- worst case, the title just comes back
    // without a "[cmd]" prefix, same as before this field existed.
    let command_hint = naming::parse_structured_json_object(response, "terminal-title")
        .ok()
        .and_then(|parsed| naming::extract_optional_string_field(&parsed, "command_hint"));

    Ok(DualTitle {
        icon: title.icon,
        short: naming::format_with_command_hint(
            if style == TitleStyle::Labeling {
                title.short.to_uppercase()
            } else {
                title.short
            },
            command_hint.as_deref(),
        ),
        long: naming::format_with_command_hint(
            if style == TitleStyle::Labeling {
                title.long.to_uppercase()
            } else {
                title.long
            },
            command_hint.as_deref(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_inference::InferenceError;
    use std::cell::{Cell, RefCell};

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

    struct LabelGenerator(FakeGenerator);

    impl PromptCompletionClient for LabelGenerator {
        fn complete_prompt(&self, prompt: String) -> Result<String, InferenceError> {
            self.0.complete_prompt(prompt)
        }

        fn title_style(&self) -> TitleStyle {
            TitleStyle::Labeling
        }
    }

    fn input(screen_text: &str) -> TerminalTitleInput {
        TerminalTitleInput {
            pane_id: NodeId(7),
            project_name: "ilium".to_string(),
            project_path: PathBuf::from("/home/developer/projects/ilium"),
            current_title: "shell".to_string(),
            screen_text: screen_text.to_string(),
            parent_group: "work".to_string(),
            nearby_titles: vec!["SESSION BACKUPS".to_string()],
        }
    }

    #[test]
    fn terminal_labeling_uses_nearby_titles_and_one_word_label() {
        let generator = LabelGenerator(FakeGenerator::new(
            r#"{"icon":"🦀","command_hint":"","terminal_title_short":"Hierarchy","terminal_title_long":"Hierarchy"}"#,
        ));
        let result = infer_terminal_title(&generator, &input("$ cargo test")).unwrap();
        assert_eq!(result.short, "HIERARCHY");
        assert_eq!(result.long, "HIERARCHY");
        let prompt = generator.0.last_prompt.borrow();
        let prompt = prompt.as_deref().unwrap();
        assert!(prompt.contains("<nearby-title>\"SESSION BACKUPS\"</nearby-title>"));
        assert!(prompt.contains("untrusted context data"));
        assert!(!prompt.contains("Treat the current title as a strong prior"));
    }

    #[test]
    fn terminal_summarization_keeps_its_current_title_prior() {
        let generator = FakeGenerator::new(
            r#"{"icon":"🦀","command_hint":"cargo test","terminal_title_short":"Rust Tests","terminal_title_long":"Rust Project Tests"}"#,
        );
        infer_terminal_title(&generator, &input("$ cargo test")).unwrap();
        let prompt = generator.last_prompt.borrow();
        let prompt = prompt.as_deref().unwrap();
        assert!(prompt.contains("Treat the current title as a strong prior"));
        assert!(!prompt.contains("<nearby-title>"));
    }

    #[test]
    fn empty_screen_never_calls_the_gateway() {
        let generator = FakeGenerator::new(
            r#"{"icon":"🦀","terminal_title_short":"Rust Build","terminal_title_long":"Build Rust Project With Cargo"}"#,
        );
        let result = infer_terminal_title(&generator, &input("   \n  \n"));
        assert!(result.is_err());
        assert_eq!(generator.calls.get(), 0);
    }

    #[test]
    fn successful_response_returns_the_normalized_title_pair() {
        let generator = FakeGenerator::new(
            r#"{"icon":"🦀","terminal_title_short":"  Rust   Build  ","terminal_title_long":"Build Rust Project With Cargo"}"#,
        );
        let result =
            infer_terminal_title(&generator, &input("$ cargo build\n   Compiling ilium")).unwrap();
        assert_eq!(result.short, "Rust Build");
        assert_eq!(result.long, "Build Rust Project With Cargo");
        assert_eq!(generator.calls.get(), 1);
    }

    #[test]
    fn prompt_json_encodes_untrusted_screen_text_and_keeps_the_output_example() {
        let generator = FakeGenerator::new(
            r#"{"icon":"🦀","terminal_title_short":"Rust Build","terminal_title_long":"Build Rust Project With Cargo"}"#,
        );
        infer_terminal_title(&generator, &input("$ echo <hello> && echo done")).unwrap();

        let prompt = generator.last_prompt.borrow().clone().unwrap();
        assert!(prompt.contains("<terminal-screen>"));
        assert!(prompt.contains("$ echo \\u003chello\\u003e \\u0026\\u0026 echo done"));
        assert!(!prompt.contains("$ echo <hello> && echo done"));
        assert!(!prompt.contains("&lt;"));
        assert!(prompt.contains("command_hint"));
        assert!(prompt.contains("<pane-id>\"7\"</pane-id>"));
        assert!(prompt.contains("<project-name>\"ilium\"</project-name>"));
        assert!(prompt.contains("<project-path>\"/home/developer/projects/ilium\"</project-path>"));
        assert!(prompt.contains(
            "<output-example>{\"icon\":\"🦀\",\"command_hint\":\"cargo build\",\"terminal_title_short\":\"Rust Build\",\"terminal_title_long\":\"Build Rust Project With Cargo\"}</output-example>"
        ));
        assert!(prompt.contains(
            "The long title must use at most 7 words; this is a maximum, not a target or a minimum."
        ));
        assert!(prompt.contains(
            "A one- or two-word long title is correct when it names the work best; never add filler merely to make a long title longer."
        ));
    }

    #[test]
    fn a_reported_command_hint_is_prefixed_onto_both_titles() {
        let generator = FakeGenerator::new(
            r#"{"icon":"🦀","command_hint":"cargo build","terminal_title_short":"Rust Build","terminal_title_long":"Build Rust Project With Cargo"}"#,
        );
        let result =
            infer_terminal_title(&generator, &input("$ cargo build\n   Compiling ilium")).unwrap();
        assert_eq!(result.short, "[cargo build] Rust Build");
        assert_eq!(result.long, "[cargo build] Build Rust Project With Cargo");
    }

    #[test]
    fn an_empty_command_hint_leaves_titles_unprefixed() {
        let generator = FakeGenerator::new(
            r#"{"icon":"🐚","command_hint":"","terminal_title_short":"Idle Shell","terminal_title_long":"Empty Shell Prompt Waiting For Input"}"#,
        );
        let result = infer_terminal_title(&generator, &input("$ ")).unwrap();
        assert_eq!(result.short, "Idle Shell");
        assert_eq!(result.long, "Empty Shell Prompt Waiting For Input");
    }

    #[test]
    fn a_missing_command_hint_field_leaves_titles_unprefixed() {
        let generator = FakeGenerator::new(
            r#"{"icon":"🦀","terminal_title_short":"Rust Build","terminal_title_long":"Build Rust Project With Cargo"}"#,
        );
        let result = infer_terminal_title(&generator, &input("$ cargo build")).unwrap();
        assert_eq!(result.short, "Rust Build");
        assert_eq!(result.long, "Build Rust Project With Cargo");
    }

    #[test]
    fn rejects_non_json_and_out_of_range_word_counts() {
        assert!(parse_terminal_title_response("Build Rust Project").is_err());
        // "icon" is present in every case below so each assertion actually
        // fails for the word-count reason it's named for, rather than
        // short-circuiting on the unrelated missing-icon check that
        // `parse_dual_bounded_word_json` runs first.
        assert!(parse_terminal_title_response(
            r#"{"icon":"🦀","terminal_title_short":"Build","terminal_title_long":"Build Rust Project With Cargo"}"#
        )
        .is_err());
        assert!(parse_terminal_title_response(
            r#"{"icon":"🦀","terminal_title_short":"Rust Build","terminal_title_long":"Build The Whole Rust Project With Cargo Today"}"#
        )
        .is_err());
        assert!(parse_terminal_title_response(
            r#"{"icon":"🦀","terminal_title_short":"Rust Build","terminal_title_long":"Cargo"}"#
        )
        .is_ok());
    }

    #[test]
    fn rejects_a_response_missing_the_icon_field() {
        assert!(parse_terminal_title_response(
            r#"{"terminal_title_short":"Rust Build","terminal_title_long":"Build Rust Project With Cargo"}"#
        )
        .is_err());
    }

    #[test]
    fn an_all_whitespace_screen_is_rejected_even_though_it_isnt_literally_empty() {
        let generator = FakeGenerator::new(
            r#"{"icon":"🦀","terminal_title_short":"Rust Build","terminal_title_long":"Build Rust Project With Cargo"}"#,
        );
        let result = infer_terminal_title(&generator, &input("   \n  \t\n  "));
        assert!(result.is_err());
        assert_eq!(generator.calls.get(), 0);
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
            r#"{"icon":"🔐","terminal_title_short":"Auth Bug","terminal_title_long":"Fix Auth Bug In Login Flow"}"#,
        ));
        infer_terminal_title(&generator, &input("$ cargo test")).unwrap();
        let prompt = generator.0.last_prompt.borrow();
        let prompt = prompt.as_deref().unwrap();
        assert!(prompt.contains("Shared vocabulary"));
        assert!(!prompt.contains("Only organization"));
        assert!(prompt.contains("Entry {{> absent}} <x>&"));
        assert!(!prompt.contains("Project convention"));
    }
}
