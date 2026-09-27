use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

#[test]
fn width_transition_preserves_text_after_vs16_icons() {
    let area = Rect::new(0, 0, 96, 8);
    let blank = Buffer::empty(area);
    let mut narrow = Buffer::empty(area);
    narrow.set_string(1, 1, "▼  🗂️      ledger-desk", Style::default());
    narrow.set_string(3, 4, "🖥️      Ledger", Style::default());
    narrow.set_string(
        25,
        1,
        "~/dev/ai/ilium/data/tmp/ledger-desk",
        Style::default(),
    );

    let mut wide = Buffer::empty(area);
    wide.set_string(1, 1, "▼  🗂️       ledger-desk", Style::default());
    wide.set_string(3, 4, "🖥️      Ledger", Style::default());
    wide.set_string(
        45,
        1,
        "~/dev/ai/ilium/data/tmp/ledger-desk",
        Style::default(),
    );

    let mut output = Vec::new();
    {
        let mut backend = CrosstermBackend::new(&mut output);
        backend.draw(blank.diff(&narrow).into_iter()).unwrap();
        backend.flush().unwrap();
    }
    let mut terminal = vt100::Parser::new(area.height, area.width, 0);
    terminal.process(&output);

    output.clear();
    {
        let mut backend = CrosstermBackend::new(&mut output);
        backend.draw(narrow.diff(&wide).into_iter()).unwrap();
        backend.flush().unwrap();
    }
    terminal.process(&output);
    let rendered = terminal.screen().contents();
    assert!(rendered.contains("ledger-desk"), "{rendered:?}");
    assert!(!rendered.contains("Ledgerr"), "{rendered:?}");
    assert!(
        rendered.contains("~/dev/ai/ilium/data/tmp/ledger-desk"),
        "{rendered:?}"
    );
}
