//! Guards the context-free description contract of the progress-monitor
//! instruction block that Ilium writes into agent instruction files.

fn instructions() -> &'static str {
    ilium_prompts::agent::PROGRESS_INSTRUCTIONS
}

#[test]
fn probe_contract_names_compact_and_long_descriptions() {
    let text = instructions();
    for required in ["`message`", "`details`", "one-line", "multi-line"] {
        assert!(text.contains(required), "missing {required}");
    }
}

#[test]
fn descriptions_must_stand_alone_for_a_reader_without_session_context() {
    let text = instructions();
    for required in [
        "has NOT read your session",
        "MUST stand alone",
        "WHAT is being done",
        "WHICH thing",
        "Never rely on session context",
        "what `percent` measures",
    ] {
        assert!(text.contains(required), "missing {required}");
    }
}

#[test]
fn long_description_has_labelled_lines_for_what_why_now_next_watch() {
    let text = instructions();
    for label in ["`What:`", "`Why:`", "`Now:`", "`Next:`", "`Watch:`"] {
        assert!(text.contains(label), "missing {label}");
    }
}

#[test]
fn lifecycle_rules_survive_the_rewrite() {
    let text = instructions();
    for required in [
        "ilium progress check --command",
        "ilium progress set --command",
        "MUST NOT poll in any form",
        "ilium progress clear --monitor-id",
    ] {
        assert!(text.contains(required), "missing {required}");
    }
}
