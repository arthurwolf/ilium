### Changed
- Collapsed choice selectors and editable numeric inputs now follow one control convention. Choices show `← value + →`: the `←` and `→` buttons step back and forward, clicking the value steps forward and right-clicking it steps back, and `+` opens the full option list. Numbers show `-  value  +  *`: `-` and `+` follow each field's domain and step size, and `*` (or clicking the value) types a number. The onboarding keyboard editor's prefix rows use the same controls and the same letter list as Settings.
- The Padding tab's "Terminal columns" value uses the number control. The `«` and `»` buttons (steps of ten) are removed; `PageUp` and `PageDown` move the threshold by ten while its row is focused.

### Fixed
- Text-trigger delays reject invalid and out-of-range drafts instead of saving a substitute value. A completed earlier save cannot dismiss a later invalid draft.
- Location-coordinate arrows disable at the correct latitude and longitude limits, including OpenStreetMap's latitude range. Focused coordinates accept `-`, `+` and `*`; the location entry-mode selector accepts `+` for its complete catalog.
- Padding exact entry buffers the complete draft until confirmation, preserves the saved mode and threshold on cancellation, and retains invalid or failed saves for correction or retry through the shared numeric editor. Threshold arrows disable at domain limits, and the columns label fits compact layouts.
- Pane creation keeps repository admission off the nested request polling stack, avoiding debug server stack exhaustion without increasing stack or capacity limits. The admission task remains owned and aborts when its caller is cancelled.
- Global animation settings honor the same explicit configuration-directory override as other settings, keeping isolated runs inside their own configuration on every platform.
- Agent Cost medal scope uses the shared arrows and full choice catalog, including retained selection and retry when a configuration save fails.
- Appearance keyboard navigation scrolls the selected control into view at compact terminal heights, including sizing cards and mode-dependent rows.
- Terminal input retains simultaneous resize and wakeup notifications when returning a key, preventing rapid resize and navigation from leaving settings drawn at stale dimensions.
- Preserve invalid numeric paste for validation instead of stripping punctuation or executing delay shortcuts, and keep empty provider models unselected while enabling the first discovered choice.
- Numeric Settings and Cost editors load the saved field when reopened, preserve typed drafts during loading, and block confirmation when saved values cannot be read; a failed save no longer makes an unacknowledged runtime preview appear persisted.
