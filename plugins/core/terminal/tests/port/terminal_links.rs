//! Twins of the ilium-client/src/terminal_links.rs tests: the plugin owns link discovery now
//! (`links::link_at`), so each kernel test is re-run against the moved functions with the exact
//! expected values of the original, plus the manifest-detector rows (`links::detect`).

use ilium_plugin_core_terminal::links::{detect, link_at, parse_path_line, TerminalLink};
use std::path::{Path, PathBuf};

mod kernel_twins {
    use super::*;

    /// REQ: PORT:ilium-client/src/terminal_links.rs::recognizes_url_and_file_line
    #[test]
    fn recognizes_url_and_file_line() {
        assert!(matches!(
            link_at("see https://example.test/a.", 10, Path::new("/tmp"), None),
            Some(TerminalLink::Url(_))
        ));
        assert!(matches!(
            link_at("src/main.rs:42:7", 4, Path::new("/repo"), None),
            Some(TerminalLink::File {
                line: Some(42),
                column: Some(7),
                ..
            })
        ));
    }

    /// REQ: PORT:ilium-client/src/terminal_links.rs::keeps_relative_references_relative_instead_of_becoming_absolute
    #[test]
    fn keeps_relative_references_relative_instead_of_becoming_absolute() {
        // A leading '.' must survive trimming: "../src/main.rs" collapsing
        // to "/src/main.rs" would silently turn a relative reference into
        // an absolute path to a completely different file.
        let link = link_at(
            "see ../src/main.rs for details",
            4,
            Path::new("/repo"),
            None,
        );
        assert_eq!(
            link,
            Some(TerminalLink::File {
                path: PathBuf::from("/repo/../src/main.rs"),
                line: None,
                column: None,
            })
        );
    }

    /// REQ: PORT:ilium-client/src/terminal_links.rs::expands_home_relative_references_against_the_injected_home_dir
    #[test]
    fn expands_home_relative_references_against_the_injected_home_dir() {
        let link = link_at(
            "~/.bashrc",
            0,
            Path::new("/repo"),
            Some(Path::new("/home/user")),
        );
        assert_eq!(
            link,
            Some(TerminalLink::File {
                path: PathBuf::from("/home/user/.bashrc"),
                line: None,
                column: None,
            })
        );
    }

    /// REQ: PORT:ilium-client/src/terminal_links.rs::keeps_a_non_numeric_middle_segment_attached_to_the_path
    #[test]
    fn keeps_a_non_numeric_middle_segment_attached_to_the_path() {
        // "a/b:c.txt:42": the middle segment isn't a line number, so the
        // whole "a/b:c.txt" must remain the path instead of losing "a/b".
        let link = link_at("a/b:c.txt:42", 0, Path::new("/repo"), None);
        assert_eq!(
            link,
            Some(TerminalLink::File {
                path: PathBuf::from("/repo/a/b:c.txt"),
                line: Some(42),
                column: None,
            })
        );
    }

    /// REQ: PORT:ilium-client/src/terminal_links.rs::strips_markdown_backticks_around_a_fenced_path
    #[test]
    fn strips_markdown_backticks_around_a_fenced_path() {
        // Markdown-fenced paths (`` `docs/guide.md` ``) are the dominant form
        // in editor buffers and agent chat output. Leaving the backticks
        // attached would make the resolved path never exist on disk.
        let link = link_at(
            "see `docs/guide.md` for details",
            5,
            Path::new("/repo"),
            None,
        );
        assert_eq!(
            link,
            Some(TerminalLink::File {
                path: PathBuf::from("/repo/docs/guide.md"),
                line: None,
                column: None,
            })
        );
    }

    /// REQ: PORT:ilium-client/src/terminal_links.rs::strips_a_matching_opening_bracket_from_a_wrapped_url
    #[test]
    fn strips_a_matching_opening_bracket_from_a_wrapped_url() {
        // "(https://example.test/docs)": the opening '(' must be stripped
        // from the token's start (matching the closing ')' already
        // stripped from its end), or the leftover '(' prefix would make
        // the URL fail its "https://" prefix check and go undetected.
        let link = link_at(
            "see (https://example.test/docs) for details",
            6,
            Path::new("/repo"),
            None,
        );
        assert_eq!(
            link,
            Some(TerminalLink::Url("https://example.test/docs".to_string()))
        );
    }

    /// REQ: PORT:ilium-client/src/terminal_links.rs::recognizes_a_bare_project_local_file_with_trailing_prose_punctuation
    #[test]
    fn recognizes_a_bare_project_local_file_with_trailing_prose_punctuation() {
        let link = link_at(
            "Read STYLE.md, before continuing",
            6,
            Path::new("/repo"),
            None,
        );
        assert_eq!(
            link,
            Some(TerminalLink::File {
                path: PathBuf::from("/repo/STYLE.md"),
                line: None,
                column: None,
            })
        );
    }

    /// REQ: PORT:ilium-client/src/terminal_links.rs::accounts_for_multi_byte_glyphs_when_mapping_a_cell_column
    #[test]
    fn accounts_for_multi_byte_glyphs_when_mapping_a_cell_column() {
        // "→ " is 4 bytes (a 3-byte arrow plus a space) but only 2 cells,
        // so cell column 2 (the 's' of the path) must not be treated as
        // byte offset 2, which would land inside the arrow's own encoding.
        let link = link_at("→ src/main.rs:42", 2, Path::new("/repo"), None);
        assert_eq!(
            link,
            Some(TerminalLink::File {
                path: PathBuf::from("/repo/src/main.rs"),
                line: Some(42),
                column: None,
            })
        );
    }
}

mod detectors {
    use super::*;

    #[test]
    fn path_line_detector_matches_the_manifest_regex_cases() {
        let found = detect("see ../src/main.rs:3 for details");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].detector_id, "path-line");
        assert_eq!(found[0].text, "../src/main.rs:3");
        assert_eq!(parse_path_line(&found[0].text), Some(("../src/main.rs", 3)));
    }

    #[test]
    fn a_path_line_inside_a_url_is_one_url_link() {
        let found = detect("open https://host.test/a/b.rs:12 now");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].detector_id, "url");
        assert_eq!(found[0].text, "https://host.test/a/b.rs:12");
    }

    #[test]
    fn parse_path_line_refuses_non_numeric_and_empty_parts() {
        assert_eq!(parse_path_line("a.rs:"), None);
        assert_eq!(parse_path_line(":4"), None);
        assert_eq!(parse_path_line("a.rs:x"), None);
        assert_eq!(parse_path_line("a.rs:99999999999"), None);
        assert_eq!(parse_path_line("a/b:c.rs:7"), Some(("a/b:c.rs", 7)));
    }

    #[test]
    fn link_display_joins_path_line_and_column() {
        let link = TerminalLink::File {
            path: PathBuf::from("/repo/a.rs"),
            line: Some(4),
            column: Some(2),
        };
        assert_eq!(link.display(), "/repo/a.rs:4:2");
        assert_eq!(link_at("", 0, Path::new("/"), None), None);
    }
}

mod file_targets {
    use ilium_plugin_core_terminal::links::{
        detector_at, file_target, file_target_of_text, FileTarget,
    };
    use regex::Regex;

    fn project(path: &str, line: Option<u32>) -> FileTarget {
        FileTarget {
            scope: "project",
            path: path.to_string(),
            line,
        }
    }

    /// The kernel-parity cases of the old `link_at`: the catch-all detector covers each token.
    #[test]
    fn the_path_detector_covers_bare_relative_home_and_file_url_tokens() {
        for (line, column, text) in [
            ("Read STYLE.md, before continuing", 6, "STYLE.md"),
            ("see ../src/main.rs for details", 4, "../src/main.rs"),
            ("~/.bashrc", 0, "~/.bashrc"),
            ("open file:///tmp/a.log now", 8, "file:///tmp/a.log"),
            ("\u{2192} src/main.rs:42", 4, "src/main.rs:42"),
            ("a/b:c.txt:42", 0, "a/b:c.txt:42"),
        ] {
            let found = detector_at(line, column).unwrap_or_else(|| panic!("{line:?} at {column}"));
            assert_eq!(found.text, text, "{line:?}");
        }
    }

    #[test]
    fn a_url_loses_wrapping_and_sentence_punctuation() {
        for (line, text) in [
            (
                "see https://example.test/docs.",
                "https://example.test/docs",
            ),
            ("(https://example.test/docs)", "https://example.test/docs"),
            ("\"https://example.test\"", "https://example.test"),
            ("https://example.test/a?b=1;", "https://example.test/a?b=1"),
        ] {
            let offset = line.find("https").expect("has a url");
            let found = detector_at(line, offset).expect("url");
            assert_eq!(found.detector_id, "url");
            assert_eq!(found.text, text);
        }
    }

    #[test]
    fn the_second_column_of_a_path_line_is_still_one_link() {
        let found = detector_at("src/main.rs:42:7", 14).expect("path");
        assert_eq!(found.detector_id, "path");
        assert_eq!(found.text, "src/main.rs:42:7");
    }

    #[test]
    fn the_path_pattern_is_the_manifest_regex() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../manifest.json")).expect("manifest");
        let detectors = manifest["contributes"]["link_detectors"]
            .as_array()
            .expect("detectors");
        let pattern = |id: &str| {
            detectors
                .iter()
                .find(|detector| detector["id"] == id)
                .and_then(|detector| detector["regex"].as_str())
                .unwrap_or_default()
                .to_string()
        };
        assert_eq!(
            pattern("url"),
            ilium_plugin_core_terminal::links::URL_PATTERN
        );
        assert_eq!(
            pattern("path-line"),
            ilium_plugin_core_terminal::links::PATH_LINE_PATTERN
        );
        assert_eq!(
            pattern("path"),
            ilium_plugin_core_terminal::links::PATH_PATTERN
        );
        assert!(Regex::new(&pattern("path")).is_ok());
    }

    #[test]
    fn a_relative_path_joins_the_pane_working_directory() {
        assert_eq!(
            file_target_of_text("src/main.rs:42:7", Some("/repo/crate")),
            Some(project("/repo/crate/src/main.rs", Some(41)))
        );
        assert_eq!(
            file_target_of_text("../src/main.rs", Some("/repo/crate")),
            Some(project("/repo/src/main.rs", None))
        );
        assert_eq!(
            file_target_of_text("./STYLE.md", Some("/repo")),
            Some(project("/repo/STYLE.md", None))
        );
    }

    #[test]
    fn without_a_working_directory_the_path_stays_relative_to_the_project() {
        assert_eq!(
            file_target_of_text("src/main.rs:3", None),
            Some(project("src/main.rs", Some(2)))
        );
    }

    #[test]
    fn an_absolute_path_and_a_file_url_stay_absolute() {
        assert_eq!(
            file_target_of_text("/repo/a.rs:9", Some("/elsewhere")),
            Some(project("/repo/a.rs", Some(8)))
        );
        assert_eq!(
            file_target_of_text("file:///repo/a.log", Some("/elsewhere")),
            Some(project("/repo/a.log", None))
        );
    }

    #[test]
    fn a_home_reference_uses_the_home_scope() {
        assert_eq!(
            file_target_of_text("~/.bashrc", Some("/repo")),
            Some(FileTarget {
                scope: "home",
                path: ".bashrc".to_string(),
                line: None,
            })
        );
        assert_eq!(file_target("~", None, Some("/repo")), None);
        assert_eq!(file_target("~/", None, Some("/repo")), None);
    }

    #[test]
    fn an_empty_path_is_refused() {
        assert_eq!(file_target("", Some(3), None), None);
        assert_eq!(file_target_of_text(":3", None), None);
    }
}
