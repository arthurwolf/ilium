//! SCRATCH DIAGNOSTIC (never on the release branch): feed recorded Windows
//! ConPTY output to the vendored vt100 parser and print what it sees, with
//! each row's cell widths around non-ASCII glyphs.

#[test]
fn print_vt100_view_of_recorded_conpty_prefix() {
    let bytes = include_bytes!("scratch/conpty-prefix.bin");
    let mut parser = vt100::Parser::new(44, 120, 0);
    parser.process(bytes);
    let screen = parser.screen();
    println!("=== vt100 rows ===");
    for (index, row) in screen.rows(0, 120).enumerate() {
        println!("{index:02}|{row}");
    }
    println!("=== wrapped rows ===");
    for row in 0..44u16 {
        if screen.row_wrapped(row) {
            println!("row {row} is wrapped");
        }
    }
    println!("=== wide cells ===");
    for row in 0..44u16 {
        for column in 0..120u16 {
            if let Some(cell) = screen.cell(row, column) {
                if cell.is_wide() || cell.contents().chars().count() > 1 {
                    println!(
                        "row {row} col {column} wide={} {:?}",
                        cell.is_wide(),
                        cell.contents()
                    );
                }
            }
        }
    }
}
