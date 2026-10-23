//! Shared helpers of the host-production lane: the manual-page reader and the text assertion.

/// Reads one manual page from `docs/plugins-manual/pages` at the repository root.
pub fn manual_page(page_file: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs")
        .join("plugins-manual")
        .join("pages")
        .join(page_file);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{} must be readable: {error}", path.display());
    })
}

/// Asserts that the manual page states the given text (a rule, value, code or step).
pub fn assert_page_states(page: &str, statement: &str) {
    assert!(
        page.contains(statement),
        "the manual page must state: {statement}"
    );
}
