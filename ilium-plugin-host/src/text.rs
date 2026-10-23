//! Terminal cell width, wrapping and fitting of text (guide-internationalisation, USR-316).

/// The width of `text` in terminal cells.
pub fn terminal_width(_text: &str) -> usize {
    unimplemented!("host::text::terminal_width")
}

/// Wraps `text` into lines of at most `cells` cells.
pub fn wrap_to_width(_text: &str, _cells: usize) -> Vec<String> {
    unimplemented!("host::text::wrap_to_width")
}

/// Fits `text` into a box of `cells` cells.
pub fn fit_to_box(_text: &str, _cells: usize) -> String {
    unimplemented!("host::text::fit_to_box")
}
