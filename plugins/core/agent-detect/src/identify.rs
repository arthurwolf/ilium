//! Process identity vendored from `ilium-detect`: which agent a process of the pane's tree is, from its name and
//! its argument vector (interpreter wrappers and literal shell command lines are unwrapped conservatively).

use std::path::Path;

pub fn interpreted_program_name(arguments: &[String]) -> Option<String> {
    let interpreter = argument_file_name(arguments, 0)?;
    let interpreter = interpreter.trim_end_matches(".exe");
    let is_shell = matches!(interpreter, "sh" | "bash" | "dash" | "zsh" | "ksh" | "fish");
    let mut options = arguments.iter().skip(1);
    let mut command_operand = false;
    let mut program = None;
    while let Some(argument) = options.next() {
        let text = argument.as_str();
        if text == "--" {
            // Option parsing ends here, even if the script is named -c.
            program = options.next();
            break;
        }
        if !text.starts_with('-') || text == "-" {
            program = Some(argument);
            break;
        }
        if is_shell {
            if interpreter == "fish" && text == "--command" {
                command_operand = true;
                program = options.next();
                break;
            }
            // Support the actual pane -c launch, login/interactive variants,
            // and their combined flags. Other options may consume operands
            // (e.g. bash --rcfile/-o): refuse inference rather than label the
            // operand as an agent. This is not a general shell option parser.
            if text.starts_with("--")
                || !text[1..]
                    .chars()
                    .all(|flag| matches!(flag, 'c' | 'i' | 'l'))
            {
                return None;
            }
            if text[1..].contains('c') {
                command_operand = true;
                program = options.next();
                break;
            }
            continue;
        }
        if interpreter == "env" && matches!(text, "-S" | "--split-string") {
            command_operand = true;
            program = options.next();
            break;
        }
        if matches!(
            text,
            "-c" | "-e" | "--eval" | "--print" | "-p" | "-m"
        ) {
            return None;
        }
    }
    let program = program?;
    if command_operand {
        literal_single_command_program(program.as_str())
    } else {
        std::path::Path::new(program.as_str())
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    }
}

/// Conservative identity parser, not a shell execution or expansion engine.
/// Validate the whole command even though only its first word is returned.
pub fn literal_single_command_program(command: &str) -> Option<String> {
    #[derive(Clone, Copy)]
    enum Quote {
        None,
        Single,
        Double,
    }
    let mut quote = Quote::None;
    let mut characters = command.chars();
    let mut word = String::new();
    let mut in_word = false;
    let mut program = None;
    while let Some(character) = characters.next() {
        if character == '\0' {
            return None;
        }
        match quote {
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::None;
                } else {
                    word.push(character);
                }
            }
            Quote::Double => match character {
                '"' => quote = Quote::None,
                '$' | '`' => return None,
                '\\' => {
                    let escaped = characters.next()?;
                    if matches!(escaped, '\0' | '\n' | '\r') {
                        return None;
                    }
                    if !matches!(escaped, '$' | '`' | '"' | '\\') {
                        word.push('\\');
                    }
                    word.push(escaped);
                }
                _ => word.push(character),
            },
            Quote::None => match character {
                '\'' => {
                    quote = Quote::Single;
                    in_word = true;
                }
                '"' => {
                    quote = Quote::Double;
                    in_word = true;
                }
                '\\' => {
                    let escaped = characters.next()?;
                    if matches!(escaped, '\0' | '\n' | '\r') {
                        return None;
                    }
                    word.push(escaped);
                    in_word = true;
                }
                ' ' | '\t' => {
                    if in_word && program.is_none() {
                        program = Some(std::mem::take(&mut word));
                    }
                    word.clear();
                    in_word = false;
                }
                ';' | '&' | '|' | '<' | '>' | '(' | ')' | '{' | '}' | '\n' | '\r' | '$' | '`'
                | '*' | '?' | '[' | ']' | '~' => return None,
                '#' if !in_word => return None,
                _ => {
                    word.push(character);
                    in_word = true;
                }
            },
        }
    }
    if !matches!(quote, Quote::None) {
        return None;
    }
    if in_word && program.is_none() {
        program = Some(word);
    }
    let program = program?;
    std::path::Path::new(&program)
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
}

/// File name of the argument at `index`, lowercased.
pub fn argument_file_name(arguments: &[String], index: usize) -> Option<String> {
    let argument = arguments.get(index)?;
    Path::new(argument)
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
}

/// Whether a program merely *runs* another program named after it.
///
/// Deliberately a closed list. Consulting the second argument for any process
/// would misread `vim codex.md` as the agent itself; consulting it only behind
/// a known interpreter keeps the rule to the case it exists for, and leaves
/// `sh -c "codex …"` alone because that command's second argument is `-c`.
pub fn is_interpreter_name(name: &str) -> bool {
    const INTERPRETERS: &[&str] = &[
        "sh", "bash", "dash", "zsh", "ksh", "fish", "env", "node", "nodejs", "deno", "bun",
        "python", "python3", "ruby", "perl",
    ];
    INTERPRETERS.contains(&name) || INTERPRETERS.contains(&name.trim_end_matches(".exe"))
}

/// Classifies a single (already-lowercased) process name against the shared
/// first-party provider registry, generic built-ins, then `extra_signatures`.
/// That order lets configuration add coverage without silently shadowing an

/// The lowercase names a process may be recognised by: its own name first, then the program an interpreter was handed
/// and the file name it was invoked as.
pub fn identifying_names(name: &str, arguments: &[String]) -> Vec<String> {
    let invoked = argument_file_name(arguments, 0);
    let script = invoked
        .as_deref()
        .is_some_and(is_interpreter_name)
        .then(|| interpreted_program_name(arguments))
        .flatten();
    let mut candidates = vec![name.to_lowercase()];
    for candidate in [script, invoked].into_iter().flatten() {
        if !candidate.is_empty() && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}
