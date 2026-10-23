//! reference-core-replacement-matrix.md, CLI names (DEV-848): with a replacement enabled, every CLI
//! name of the matrix row keeps the arguments, output and exit code it has with the built-in
//! plugin. Driven through the user-profile rig, one core plugin per run.

use crate::support::*;

const MATRIX_PAGE: &str = "reference-core-replacement-matrix";
const MATRIX_HEADING: &str = "### The matrix";

/// The core plugin id and the CLI names of each matrix row that names any CLI command.
fn matrix_cli_rows() -> Vec<(String, Vec<String>)> {
    let text = manual_page(MATRIX_PAGE);
    let mut in_matrix = false;
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.trim() == MATRIX_HEADING {
            in_matrix = true;
            continue;
        }
        if !in_matrix || !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() < 6 || cells[0] == "Plugin" || cells[0].starts_with("---") {
            continue;
        }
        let plugin = cells[0].split('`').nth(1).unwrap_or_default().to_owned();
        // The CLI cell is the fourth column; names come before any parenthesised alias list.
        let names: Vec<String> = cells[3]
            .split(" (")
            .next()
            .unwrap_or_default()
            .split(',')
            .map(|name| name.trim().trim_matches('`').to_owned())
            .filter(|name| !name.is_empty() && name != "none")
            .collect();
        if !names.is_empty() {
            rows.push((plugin, names));
        }
    }
    rows
}

/// The id of the replacement package used for one core plugin in these tests.
fn replacement_id(core: &str) -> String {
    format!(
        "com.example.cli-replacement-{}",
        core.trim_start_matches("ilium.core.")
    )
}

/// Installs a replacement of the core plugin with the user's consent and asserts it succeeds.
fn install_replacement(rig: &mut UserRig, core: &str) {
    rig.attach_client(vec![DialogAnswer::ReplaceInstallWithout]);
    let output = rig.install_file(
        &spec(&replacement_id(core), "1.0.0", &[]).replaces(&[core]),
        &["--allow-unsigned", "--yes"],
    );
    assert_eq!(
        result_exit_code(&output),
        0,
        "{core}: the replacement did not install: {:?}",
        error_codes(&output)
    );
}

/// REQ: DEV-848
#[test]
fn dev_848_every_cli_name_of_a_row_gives_the_same_output_and_exit_code_with_a_replacement() {
    for (core, names) in matrix_cli_rows() {
        let mut rig = UserRig::new();
        let baseline: Vec<(i32, String)> = names
            .iter()
            .map(|name| {
                let output = rig.cli(&[name.as_str(), "--help"]);
                (output.exit_code, output.stdout_raw.clone())
            })
            .collect();

        install_replacement(&mut rig, &core);

        for (name, (exit_code, stdout)) in names.iter().zip(baseline) {
            let output = rig.cli(&[name.as_str(), "--help"]);
            assert_eq!(
                output.exit_code, exit_code,
                "{core}: `{name}` exit code changed"
            );
            assert_eq!(output.stdout_raw, stdout, "{core}: `{name}` output changed");
        }
    }

    let mut rig = UserRig::new();
    let baseline = rig.cli(&["chat", "context", "--since-last-read"]);
    install_replacement(&mut rig, "ilium.core.chatroom");
    let output = rig.cli(&["chat", "context", "--since-last-read"]);
    assert_eq!(
        output.exit_code, baseline.exit_code,
        "`ilium chat context --since-last-read` changed its exit code"
    );
    assert_eq!(
        output.stdout_raw, baseline.stdout_raw,
        "`ilium chat context --since-last-read` changed its output"
    );
}
