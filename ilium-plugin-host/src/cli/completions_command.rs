//! `ilium plugin completions <shell>` (reference-cli-completion.md DEV-855, DEV-869..871, DEV-873).
//! The scripts are generated from one command table, sorted by name, without a session or network.
use super::{offline, CliOutput};

const GLOBAL_FLAGS: [&str; 6] = ["--cwd", "--help", "--json", "--no-color", "--quiet", "--session"];

/// The command table: every kernel plugin command with the flags it accepts besides the global ones.
const COMMANDS: &[(&str, &[&str])] = &[
    ("audit", &[]),
    ("build", &[]),
    ("check", &[]),
    ("completions", &[]),
    ("dev", &[]),
    ("disable", &[]),
    ("doctor", &[]),
    ("enable", &[]),
    ("grants", &[]),
    ("inspect", &["--memory", "--verify", "--view"]),
    ("install", &[]),
    ("keygen", &[]),
    ("lint", &[]),
    ("list", &[]),
    ("log-level", &[]),
    ("logs", &["--follow", "--level", "--side", "--since", "--tail"]),
    ("new", &[]),
    ("pack", &[]),
    ("profile", &["--duration"]),
    ("publish", &[]),
    ("registry", &[]),
    ("sign", &[]),
    ("test", &[]),
    ("trace", &["--kinds", "--last", "--record", "--unredacted"]),
    ("trust", &[]),
    ("uninstall", &[]),
    ("verify", &[]),
];

fn flags_of(command: &str) -> Vec<&'static str> {
    let mut flags: Vec<&'static str> = GLOBAL_FLAGS.to_vec();
    if let Some((_, own)) = COMMANDS.iter().find(|(name, _)| *name == command) {
        flags.extend(own.iter());
    }
    flags.sort_unstable();
    flags
}

fn names() -> Vec<&'static str> {
    COMMANDS.iter().map(|(name, _)| *name).collect()
}

fn bash() -> String {
    let mut cases = String::new();
    for (name, _) in COMMANDS {
        cases.push_str(&format!(
            "        {name}) COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") ) ;;\n",
            flags_of(name).join(" ")
        ));
    }
    format!(
        "# bash completion for `ilium plugin`\n_ilium_plugin() {{\n    local cur=\"${{COMP_WORDS[COMP_CWORD]}}\"\n    local command=\"${{COMP_WORDS[2]}}\"\n    if [ \"$COMP_CWORD\" -le 2 ]; then\n        COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )\n        return\n    fi\n    case \"$command\" in\n{cases}    esac\n}}\ncomplete -F _ilium_plugin ilium\n",
        names().join(" ")
    )
}

fn zsh() -> String {
    let mut cases = String::new();
    for (name, _) in COMMANDS {
        cases.push_str(&format!(
            "        {name}) compadd -- {} ;;\n",
            flags_of(name).join(" ")
        ));
    }
    format!(
        "#compdef ilium\n# zsh completion for `ilium plugin`\n_ilium_plugin() {{\n    if (( CURRENT <= 3 )); then\n        compadd -- {}\n        return\n    fi\n    case \"$words[3]\" in\n{cases}    esac\n}}\ncompdef _ilium_plugin ilium\n",
        names().join(" ")
    )
}

fn fish() -> String {
    let mut text = String::from("# fish completion for `ilium plugin`\n");
    for (name, _) in COMMANDS {
        text.push_str(&format!(
            "complete -c ilium -n '__fish_seen_subcommand_from plugin; and not __fish_seen_subcommand_from {}' -a {name}\n",
            names().join(" ")
        ));
    }
    for (name, _) in COMMANDS {
        for flag in flags_of(name) {
            text.push_str(&format!(
                "complete -c ilium -n '__fish_seen_subcommand_from {name}' -l {}\n",
                flag.trim_start_matches("--")
            ));
        }
    }
    text
}

fn powershell() -> String {
    let mut text = String::from(
        "# PowerShell completion for `ilium plugin`\nRegister-ArgumentCompleter -Native -CommandName ilium -ScriptBlock {\n    param($wordToComplete, $commandAst, $cursorPosition)\n    $words = $commandAst.CommandElements | ForEach-Object { $_.ToString() }\n    $table = @{\n",
    );
    for (name, _) in COMMANDS {
        text.push_str(&format!("        '{name}' = '{}'\n", flags_of(name).join(" ")));
    }
    text.push_str(&format!(
        "    }}\n    $candidates = if ($words.Count -le 3) {{ '{}' -split ' ' }} else {{ $table[$words[2]] -split ' ' }}\n    $candidates | Where-Object {{ $_ -like \"$wordToComplete*\" }} | ForEach-Object {{\n        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)\n    }}\n}}\n",
        names().join(" ")
    ));
    text
}

/// Prints the completion script of one shell to standard output.
pub(super) fn run(args: &[String]) -> CliOutput {
    let words: Vec<&String> = args
        .iter()
        .filter(|arg| !matches!(arg.as_str(), "--json" | "--quiet" | "--no-color"))
        .collect();
    let Some(shell) = words.first() else {
        return offline::failure("usage.missing-argument", "A shell name is required", 2);
    };
    if words.len() > 1 {
        return offline::failure("usage.argument-invalid", "Unexpected positional argument", 2);
    }
    let script = match shell.as_str() {
        "bash" => bash(),
        "zsh" => zsh(),
        "fish" => fish(),
        "powershell" => powershell(),
        _ => {
            return offline::failure(
                "usage.invalid-value",
                "The shell must be bash, zsh, fish or powershell",
                2,
            )
        }
    };
    CliOutput {
        exit_code: 0,
        stdout_jsonl: Vec::new(),
        stdout_raw: script.clone(),
        stdout_text: script,
        stderr: String::new(),
    }
}
