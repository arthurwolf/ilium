//! The `ilium.core.ambient/set-setting` service: validation of one write to the `animations` tab.
//!
//! Pure: no host call. The server part passes the kernel write in as a function
//! ([`handle`]'s `write` parameter), so every rule here is testable without a host. Payload
//! and reply follow `schemas/set-setting.json` (page core-plugin-ambient.md, "Server part").

use serde_json::{json, Value};

/// The one kernel tab this plugin may write (grant `settings.write-kernel:animations`).
pub const TAB: &str = "animations";

/// The scene ids of the manifest `scene` row.
pub const SCENES: [&str; 2] = ["stars", "clouds"];

/// Most entries of the manifest `saved_places` row (`max_entries`).
pub const MAX_SAVED_PLACES: usize = 8;

/// Longest key of a saved place (manifest `key_pattern` `^[a-z-]{1,24}$`).
const MAX_PLACE_KEY_CHARS: usize = 24;

/// Why a write was refused, in the WIT error-code spelling (`not-found`, `invalid-argument`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The WIT name of the error code.
    pub code: String,
    /// Plain text for the caller.
    pub message: String,
}

impl Refusal {
    fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }
}

/// One validated write: the kernel key and its value as JSON text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    /// The key inside the `animations` tab.
    pub key: String,
    /// The value as JSON text, as `kernel-settings.write` takes it.
    pub value_json: String,
}

/// A place is an object with a latitude, a longitude and an optional label.
fn is_place(value: &Value) -> bool {
    let Some(place) = value.as_object() else {
        return false;
    };
    let in_range = |name: &str, limit: f64| {
        place
            .get(name)
            .and_then(Value::as_f64)
            .is_some_and(|degrees| (-limit..=limit).contains(&degrees))
    };
    in_range("lat", 90.0)
        && in_range("lon", 180.0)
        && place.get("label").map_or(true, Value::is_string)
}

/// A saved-place key is 1 to 24 characters from `a-z` and `-`.
pub fn is_place_key(key: &str) -> bool {
    (1..=MAX_PLACE_KEY_CHARS).contains(&key.chars().count())
        && key
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '-')
}

fn is_saved_places(value: &Value) -> bool {
    value.as_object().is_some_and(|places| {
        places.len() <= MAX_SAVED_PLACES
            && places
                .iter()
                .all(|(key, place)| is_place_key(key) && is_place(place))
    })
}

/// Validates the value text of one key and returns it as JSON text.
fn value_json(key: &str, value: &str) -> Result<String, Refusal> {
    let invalid = || Refusal::new("invalid-argument", "the value is not allowed for this key");
    match key {
        "scene" if SCENES.contains(&value) => Ok(Value::String(value.to_owned()).to_string()),
        "semantic" if value == "true" || value == "false" => Ok(value.to_owned()),
        "observer" | "saved_places" => {
            let parsed: Value = serde_json::from_str(value).map_err(|_| invalid())?;
            let allowed = if key == "observer" {
                is_place(&parsed)
            } else {
                is_saved_places(&parsed)
            };
            if allowed {
                Ok(parsed.to_string())
            } else {
                Err(invalid())
            }
        }
        "scene" | "semantic" => Err(invalid()),
        _ => Err(Refusal::new("invalid-argument", "unknown key")),
    }
}

/// Parses and validates one `set` call.
pub fn parse(method: &str, payload: &str) -> Result<Write, Refusal> {
    if method != "set" {
        return Err(Refusal::new("not-found", "unknown method"));
    }
    let Ok(parsed) = serde_json::from_str::<Value>(payload) else {
        return Err(Refusal::new("invalid-argument", "the payload is not JSON"));
    };
    let text = |name: &str| parsed.get(name).and_then(Value::as_str);
    let (Some(tab), Some(key), Some(value)) = (text("tab"), text("key"), text("value")) else {
        return Err(Refusal::new(
            "invalid-argument",
            "tab, key and value are required strings",
        ));
    };
    if tab != TAB {
        return Err(Refusal::new(
            "not-permitted",
            "this plugin owns only the animations tab",
        ));
    }
    Ok(Write {
        key: key.to_owned(),
        value_json: value_json(key, value)?,
    })
}

/// The JSON text of a refusal reply.
pub fn error_reply(refusal: &Refusal) -> String {
    json!({ "error": { "code": refusal.code, "message": refusal.message } }).to_string()
}

/// The JSON text of the success reply.
pub fn ok_reply() -> String {
    json!({ "ok": true }).to_string()
}

/// Answers one service call: validates it, then asks `write` to store it in the kernel tab.
/// A failed kernel write is the reply, with the kernel's own error code.
pub fn handle(
    method: &str,
    payload: &str,
    write: impl FnOnce(&Write) -> Result<(), Refusal>,
) -> String {
    match parse(method, payload).and_then(|validated| write(&validated)) {
        Ok(()) => ok_reply(),
        Err(refusal) => error_reply(&refusal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(key: &str, value: &str) -> String {
        json!({ "tab": TAB, "key": key, "value": value }).to_string()
    }

    /// REQ: COR-030
    #[test]
    fn a_scene_id_is_written_as_a_json_string() {
        let write = parse("set", &call("scene", "clouds")).expect("valid");
        assert_eq!(write.key, "scene");
        assert_eq!(write.value_json, "\"clouds\"");
    }

    /// REQ: COR-030
    #[test]
    fn an_unknown_scene_is_refused() {
        let refusal = parse("set", &call("scene", "waves")).expect_err("refused");
        assert_eq!(refusal.code, "invalid-argument");
    }

    /// REQ: COR-030
    #[test]
    fn semantic_takes_only_true_or_false() {
        assert_eq!(
            parse("set", &call("semantic", "true"))
                .expect("valid")
                .value_json,
            "true"
        );
        assert!(parse("set", &call("semantic", "yes")).is_err());
    }

    /// REQ: COR-030
    #[test]
    fn observer_needs_coordinates_in_range() {
        let lisbon = r#"{"lat":38.7223,"lon":-9.1393,"label":"Lisbon"}"#;
        assert!(parse("set", &call("observer", lisbon)).is_ok());
        assert!(parse("set", &call("observer", r#"{"lat":91.0,"lon":0.0}"#)).is_err());
        assert!(parse("set", &call("observer", r#"{"lat":0.0,"lon":180.5}"#)).is_err());
        assert!(parse("set", &call("observer", r#"{"lat":0.0}"#)).is_err());
        assert!(parse(
            "set",
            &call("observer", r#"{"lat":0.0,"lon":0.0,"label":3}"#)
        )
        .is_err());
        assert!(parse("set", &call("observer", "not json")).is_err());
    }

    /// REQ: COR-030
    #[test]
    fn saved_places_follow_the_key_pattern_and_the_entry_limit() {
        let one = r#"{"home":{"lat":1.0,"lon":2.0}}"#;
        assert!(parse("set", &call("saved_places", one)).is_ok());
        let capital = r#"{"Home":{"lat":1.0,"lon":2.0}}"#;
        assert!(parse("set", &call("saved_places", capital)).is_err());
        let long_key = format!(r#"{{"{}":{{"lat":1.0,"lon":2.0}}}}"#, "a".repeat(25));
        assert!(parse("set", &call("saved_places", &long_key)).is_err());
        let mut places = serde_json::Map::new();
        for index in 0..=MAX_SAVED_PLACES {
            let key = char::from(b'a' + index as u8).to_string();
            places.insert(key, json!({ "lat": 0.0, "lon": 0.0 }));
        }
        assert_eq!(places.len(), MAX_SAVED_PLACES + 1);
        assert!(parse(
            "set",
            &call("saved_places", &Value::Object(places.clone()).to_string())
        )
        .is_err());
        places.remove("a");
        assert!(parse(
            "set",
            &call("saved_places", &Value::Object(places).to_string())
        )
        .is_ok());
    }

    /// REQ: COR-030
    #[test]
    fn another_tab_is_not_permitted() {
        let payload = json!({ "tab": "terminal", "key": "scene", "value": "stars" }).to_string();
        let refusal = parse("set", &payload).expect_err("refused");
        assert_eq!(refusal.code, "not-permitted");
    }

    /// REQ: COR-030
    #[test]
    fn a_malformed_payload_and_an_unknown_method_are_refused() {
        assert_eq!(parse("get", "{}").expect_err("method").code, "not-found");
        assert_eq!(
            parse("set", "[").expect_err("json").code,
            "invalid-argument"
        );
        assert_eq!(
            parse("set", r#"{"tab":"animations","key":"scene"}"#)
                .expect_err("missing value")
                .code,
            "invalid-argument"
        );
        assert_eq!(
            parse("set", &call("kind", "stars"))
                .expect_err("key")
                .message,
            "unknown key"
        );
    }

    /// REQ: COR-030
    #[test]
    fn a_valid_call_reaches_the_write_once_and_a_refused_call_never_does() {
        let mut written = Vec::new();
        let reply = handle("set", &call("scene", "stars"), |write| {
            written.push(write.clone());
            Ok(())
        });
        assert_eq!(reply, ok_reply());
        assert_eq!(written.len(), 1);
        let mut touched = false;
        let reply = handle("set", &call("scene", "nope"), |_| {
            touched = true;
            Ok(())
        });
        assert!(!touched);
        assert!(reply.contains("invalid-argument"));
    }

    /// REQ: COR-030
    #[test]
    fn a_failed_kernel_write_is_the_reply() {
        let reply = handle("set", &call("semantic", "false"), |_| {
            Err(Refusal::new("not-permitted", "the grant is missing"))
        });
        let parsed: Value = serde_json::from_str(&reply).expect("JSON");
        assert_eq!(parsed["error"]["code"], "not-permitted");
        assert_eq!(parsed["error"]["message"], "the grant is missing");
    }
}
