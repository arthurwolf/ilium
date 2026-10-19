# API gaps found by the port twins (tests/port)

- ilium-plugin-testkit: `FakeHost::inject_failure(import: &str, error: PluginError)` is used with the import name `"settings.set"` for a failed settings write (F-090); the manual does not name the import. Used by budget, values, wizard and output twins.
- ilium-plugin-core-settings: view ids assumed: `ViewTarget::Panel("overview")`, `ViewTarget::OnboardingStep("first-run")`, `ViewTarget::Popup("agent-launcher" | "startup-progress" | "confirm" | "form-actions" | "text-prompt")` (F-089); the manual only lists the settings pages and the first-run step.
- ilium-plugin-core-settings: action row ids assumed for chrome that has no setting key: `"document"` (Scrolled, Number lines, positive = down), `"navigation"` (Scrolled on the overview panel), `"close"`, `"card"` (HoverEnter/HoverLeave), `"capture"`, `"form"` (F-089).
- ilium-plugin-sdk::testing `Screen::cell(row: u16, column: u16) -> ScreenCell` returns `Color` enum values (`Role`, `Rgb`, `Indexed`); the manual does not say which role is the destructive red, neutral dark grey or primary cyan of the original button tests, so the dialog twins compare tones for inequality only (F-089).
- manual: key names passed to `FakeHost::key` are assumed to be `"Esc"`, `"Enter"`, `"Tab"`, `"Left"`, `"Right"`, `"Up"`, `"Down"`, `"Home"`, `"End"`, `"PageUp"`, `"PageDown"`, `"Backspace"`, `"Plus"`, `"Minus"`, `"Space"` and `"Ctrl+<letter>"` (F-089).
- manual: setting keys assumed beyond the documented tables: `ui.agent_tree_model_icons`, `ui.terminal_text_selection_enabled`, `inference.openai_api_key`, `inference.restructure_prompt_token_limit`, `git.setup_command` (F-090).
- manual: the Panel `overview` is assumed to list tab names, and scrolling a tab by `Scrolled`+`Number` is assumed to move the document and clamp at both ends (F-089).
- manual: HoverEnter/HoverLeave and Selected actions on unknown rows are assumed to be ignored without a recorded failure (F-089).
