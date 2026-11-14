//! From the user's change of a plugin setting row to a call of the `set-setting` service.
//!
//! The rows `scene`, `semantic`, `observer` and `saved_places` of the manifest are the plugin's
//! own settings, kept in the plugin's settings store. The Animations kernel tab is what the
//! client actually shows, and the one write to it is the service `ilium.core.ambient/set-setting`
//! (see [`crate::set_setting`]). When the user changes one of these rows, the client part turns
//! the new value into a service call here; the server part validates and stores it, and the
//! client applies it to the live animation. Pure: no host call.

use serde_json::{json, Value};

use crate::set_setting::TAB;

/// The service that stores one value in the `animations` kernel tab.
pub const SERVICE: &str = "ilium.core.ambient/set-setting";

/// The one method of [`SERVICE`].
pub const METHOD: &str = "set";

/// The setting rows that reach the kernel tab, in the order of the manifest.
pub const KEYS: [&str; 4] = ["scene", "semantic", "observer", "saved_places"];

/// One call of the service: the row it carries and the payload to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The settings row.
    pub key: String,
    /// The JSON text of the service payload (`schemas/set-setting.json`, method `set`).
    pub payload_json: String,
}

/// The value of a row as the service takes it: the scene id and the booleans as plain text,
/// the location and the places as JSON text. A value of the wrong JSON type has no form.
fn value_text(key: &str, value: &Value) -> Option<String> {
    match (key, value) {
        ("scene", Value::String(scene)) => Some(scene.clone()),
        ("semantic", Value::Bool(flag)) => Some(flag.to_string()),
        ("observer" | "saved_places", Value::Object(_)) => Some(value.to_string()),
        _ => None,
    }
}

/// The calls for the changed rows, one per row of [`KEYS`] that `changed` names, in the order of
/// `changed`. `values` is the plugin's settings object (`settings.get-json`). A row that is not
/// ours, repeated, absent from `values` or of the wrong type makes no call.
pub fn calls(changed: &[String], values: &Value) -> Vec<Call> {
    let mut calls: Vec<Call> = Vec::new();
    for key in changed {
        if !KEYS.contains(&key.as_str()) || calls.iter().any(|call| &call.key == key) {
            continue;
        }
        let Some(text) = values.get(key).and_then(|value| value_text(key, value)) else {
            continue;
        };
        calls.push(Call {
            key: key.clone(),
            payload_json: json!({ "tab": TAB, "key": key, "value": text }).to_string(),
        });
    }
    calls
}

/// The refusal text of a service reply `{"error": {"code": .., "message": ..}}`, if it is one.
pub fn refusal_of(reply: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(reply).ok()?;
    let error = parsed.get("error")?;
    let code = error.get("code").and_then(Value::as_str).unwrap_or("error");
    let message = error.get("message").and_then(Value::as_str).unwrap_or("");
    Some(format!("{code}: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|key| (*key).to_owned()).collect()
    }

    fn payload(call: &Call) -> Value {
        serde_json::from_str(&call.payload_json).expect("the payload is JSON")
    }

    /// REQ: COR-030
    #[test]
    fn a_changed_scene_row_calls_the_service_with_the_scene_id() {
        let values = json!({ "scene": "clouds", "semantic": false });
        let calls = calls(&changed(&["scene"]), &values);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].key, "scene");
        assert_eq!(
            payload(&calls[0]),
            json!({ "tab": "animations", "key": "scene", "value": "clouds" })
        );
    }

    /// REQ: COR-030
    #[test]
    fn a_toggle_is_sent_as_true_or_false_text() {
        let calls = calls(&changed(&["semantic"]), &json!({ "semantic": true }));
        assert_eq!(payload(&calls[0])["value"], "true");
        let calls = super::calls(&changed(&["semantic"]), &json!({ "semantic": false }));
        assert_eq!(payload(&calls[0])["value"], "false");
    }

    /// REQ: COR-030
    #[test]
    fn a_location_and_the_saved_places_are_sent_as_json_text() {
        let values = json!({
            "observer": { "lat": 38.7223, "lon": -9.1393, "label": "Lisbon" },
            "saved_places": { "home": { "lat": 1.0, "lon": 2.0 } },
        });
        let calls = calls(&changed(&["observer", "saved_places"]), &values);
        assert_eq!(calls.len(), 2);
        let observer: Value =
            serde_json::from_str(payload(&calls[0])["value"].as_str().expect("text"))
                .expect("JSON");
        assert_eq!(observer["label"], "Lisbon");
        let places: Value =
            serde_json::from_str(payload(&calls[1])["value"].as_str().expect("text"))
                .expect("JSON");
        assert_eq!(places["home"]["lat"].as_f64(), Some(1.0));
    }

    /// Every payload this module builds passes the service's own validation.
    ///
    /// REQ: COR-030
    #[test]
    fn every_payload_is_accepted_by_the_service() {
        let values = json!({
            "scene": "stars",
            "semantic": true,
            "observer": { "lat": 38.7223, "lon": -9.1393, "label": "Lisbon" },
            "saved_places": { "home": { "lat": 1.0, "lon": 2.0 } },
        });
        let calls = calls(&changed(&KEYS), &values);
        assert_eq!(calls.len(), KEYS.len());
        for call in &calls {
            assert!(
                crate::set_setting::parse(METHOD, &call.payload_json).is_ok(),
                "{}",
                call.key
            );
        }
    }

    /// REQ: COR-030
    #[test]
    fn rows_that_are_not_ours_or_repeated_or_missing_make_no_call() {
        let values = json!({ "scene": "stars", "other": "x" });
        assert!(calls(&changed(&["other"]), &values).is_empty());
        assert!(calls(&changed(&["semantic"]), &values).is_empty());
        assert_eq!(calls(&changed(&["scene", "scene"]), &values).len(), 1);
        assert!(calls(&[], &values).is_empty());
    }

    /// REQ: COR-030
    #[test]
    fn a_value_of_the_wrong_type_makes_no_call() {
        let values =
            json!({ "scene": 3, "semantic": "yes", "observer": [1], "saved_places": null });
        assert!(calls(&changed(&KEYS), &values).is_empty());
        assert!(calls(&changed(&["scene"]), &Value::Null).is_empty());
    }

    /// REQ: COR-030
    #[test]
    fn a_refusal_reply_is_named_and_a_success_reply_is_not() {
        let refused = crate::set_setting::error_reply(&crate::set_setting::Refusal {
            code: "not-permitted".to_owned(),
            message: "the grant is missing".to_owned(),
        });
        assert_eq!(
            refusal_of(&refused).as_deref(),
            Some("not-permitted: the grant is missing")
        );
        assert_eq!(refusal_of(&crate::set_setting::ok_reply()), None);
        assert_eq!(refusal_of("not json"), None);
    }
}
