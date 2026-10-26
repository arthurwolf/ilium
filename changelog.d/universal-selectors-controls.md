### Changed
- Collapsed choice selectors and editable numeric inputs now follow one control convention. Choices show `← value + →`: the `←` and `→` buttons step back and forward, clicking the value steps forward and right-clicking it steps back, and `+` opens the full option list. Numbers show `-  value  +  *`: `-` and `+` follow each field's domain and step size, and `*` (or clicking the value) types a number. The onboarding keyboard editor's prefix rows use the same controls and the same letter list as Settings.
- The Padding tab's "Terminal columns" value uses the number control. The `«` and `»` buttons (steps of ten) are removed; `PageUp` and `PageDown` move the threshold by ten while its row is focused.

### Fixed
- Text-trigger delays reject invalid and out-of-range drafts instead of saving a substitute value. A completed earlier save cannot dismiss a later invalid draft.
- Location-coordinate arrows disable at the correct latitude and longitude limits, including OpenStreetMap's latitude range. Focused coordinates accept `-`, `+` and `*`; the location entry-mode selector accepts `+` for its complete catalog.
- Padding exact entry buffers the complete draft until confirmation, preserves the saved mode and threshold on cancellation, and retains invalid or failed saves for correction or retry through the shared numeric editor. Threshold arrows disable at domain limits, and the columns label fits compact layouts.
