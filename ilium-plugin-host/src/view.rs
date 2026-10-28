//! Static validation of plugin view documents (guide-accessibility, USR-306).

use ilium_plugin_api::error::Code;
use ilium_plugin_api::{ErrorCode, PluginError};

/// Widget kinds that can take focus and therefore need a text label (USR-306).
const FOCUSABLE_KINDS: [&str; 5] = ["button", "toggle", "input", "list-row", "list_row"];

fn view_error(code: &'static str, message: &str) -> PluginError {
    PluginError::with_prefix(
        ErrorCode::InvalidArgument,
        &Code::from_static(code),
        message,
    )
}

/// Validates a view JSON document; a focusable widget without a label is refused.
pub fn validate_view(view_json: &str) -> Result<(), PluginError> {
    let document: serde_json::Value = serde_json::from_str(view_json)
        .map_err(|error| view_error("ui.invalid-view", &format!("view is not JSON: {error}")))?;
    let Some(widgets) = document.get("widgets").and_then(|value| value.as_array()) else {
        return Ok(());
    };
    for widget in widgets {
        let kind = widget.get("kind").and_then(|value| value.as_str()).unwrap_or("");
        if !FOCUSABLE_KINDS.contains(&kind) {
            continue;
        }
        let has_label = widget
            .get("label")
            .and_then(|value| value.as_str())
            .is_some_and(|label| !label.trim().is_empty());
        if !has_label {
            let id = widget.get("id").and_then(|value| value.as_str()).unwrap_or("(no id)");
            return Err(view_error(
                "ui.label-missing",
                &format!("focusable {kind} widget {id:?} has no text label"),
            ));
        }
    }
    Ok(())
}
