//! Converts SDK scopes to the broker's closed types. This creates requests,
//! never grants; actual platform policy and selected resources remain native.
use crate::{
    error::{AnimationError, Result},
    manifest::Capability as WireCapability,
    permissions::{AudioProduct, Capability, HttpMethod, Right, Scope, Selection},
};
use serde_json::Value;
use std::collections::BTreeSet;

pub fn right(input: &WireCapability) -> Result<Right> {
    let id: Capability = serde_json::from_value(Value::String(input.id.clone()))?;
    // The tagged shape is authoritative for new SDK packages; the compact
    // v1 spellings below retain precise, deliberately limited meanings.
    if input.scope.get("kind").is_some() {
        let scope = serde_json::from_value(input.scope.clone())?;
        return normalize(Right { id, scope });
    }
    let scope = match id {
        Capability::NetworkHttp | Capability::NetworkLocal => {
            keys(&input.scope, &["origins", "methods"])?;
            let origins = serde_json::from_value(
                input
                    .scope
                    .get("origins")
                    .cloned()
                    .ok_or_else(|| invalid("origins"))?,
            )?;
            let methods = input.scope.get("methods").map_or_else(
                || Ok(BTreeSet::from([HttpMethod::Get])),
                |methods| serde_json::from_value(methods.clone()),
            )?;
            Scope::Network { origins, methods }
        }
        Capability::InputPointer | Capability::ScreenOcclusion => {
            literal(&input.scope, "animation_viewport")?;
            Scope::AnimationViewport
        }
        Capability::LocationObserver => {
            literal(&input.scope, "session")?;
            Scope::Observer
        }
        Capability::StatePersist => {
            literal(&input.scope, "session")?;
            Scope::Namespace {
                name: "session".into(),
            }
        }
        Capability::DiskRead | Capability::DiskWrite => {
            keys(&input.scope, &["selection", "access", "slot"])?;
            literal(
                input
                    .scope
                    .get("access")
                    .ok_or_else(|| invalid("disk access"))?,
                if id == Capability::DiskRead {
                    "read"
                } else {
                    "write"
                },
            )?;
            let selection: Selection = serde_json::from_value(
                input
                    .scope
                    .get("selection")
                    .cloned()
                    .ok_or_else(|| invalid("selection"))?,
            )?;
            let slot = input.scope.get("slot").map_or(Ok("selected"), |slot| {
                slot.as_str().ok_or_else(|| invalid("disk slot"))
            })?;
            Scope::Disk {
                slot: slot.into(),
                selection,
            }
        }
        Capability::AudioLoopback | Capability::AudioMicrophone => {
            keys(&input.scope, &["source", "products"])?;
            let device = input
                .scope
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("audio source"))?;
            let products = input.scope.get("products").map_or_else(
                || {
                    Ok(BTreeSet::from([
                        AudioProduct::Level,
                        AudioProduct::Waveform,
                        AudioProduct::Envelope,
                        AudioProduct::Bands,
                        AudioProduct::History,
                    ]))
                },
                |products| serde_json::from_value(products.clone()),
            )?;
            Scope::Audio {
                device: device.into(),
                products,
            }
        }
        Capability::DeviceGpu => {
            // A session string cannot authorize arbitrary future kernels.
            // Authors must name kernels; native policy independently limits
            // this same set to kernels actually available on the platform.
            keys(&input.scope, &["kernels"])?;
            Scope::Gpu {
                kernels: serde_json::from_value(
                    input
                        .scope
                        .get("kernels")
                        .cloned()
                        .ok_or_else(|| invalid("GPU kernels"))?,
                )?,
            }
        }
    };
    normalize(Right { id, scope })
}

fn normalize(right: Right) -> Result<Right> {
    right
        .normalized()
        .map_err(|error| AnimationError::InvalidPackage(error.to_string()))
}
fn literal(value: &Value, expected: &str) -> Result<()> {
    if value.as_str() == Some(expected) {
        Ok(())
    } else {
        Err(invalid("scope literal"))
    }
}
fn keys(value: &Value, permitted: &[&str]) -> Result<()> {
    let object = value.as_object().ok_or_else(|| invalid("scope object"))?;
    if object.keys().all(|key| permitted.contains(&key.as_str())) {
        Ok(())
    } else {
        Err(invalid("unknown scope field"))
    }
}
fn invalid(message: &str) -> AnimationError {
    AnimationError::InvalidPackage(format!("permission scope: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn convert(id: &str, scope: Value) -> Result<Right> {
        right(&WireCapability {
            id: id.into(),
            scope,
        })
    }
    #[test]
    fn compact_network_scope_defaults_to_get_and_rejects_hidden_authority() {
        let right = convert(
            "network.http",
            json!({"origins":["https://EXAMPLE.org:443/"]}),
        )
        .unwrap();
        assert_eq!(
            right.scope,
            Scope::Network {
                origins: BTreeSet::from(["https://example.org".into()]),
                methods: BTreeSet::from([HttpMethod::Get]),
            }
        );
        assert!(convert(
            "network.http",
            json!({"origins":["https://example.org"],"credentials":"browser"})
        )
        .is_err());
        assert!(convert(
            "network.http",
            json!({"origins":["https://example.org/path"]})
        )
        .is_err());
    }
    #[test]
    fn closed_scopes_and_named_kernels_are_required_for_specific_authority() {
        assert!(convert("device.gpu", json!("session")).is_err());
        assert!(convert("device.gpu", json!({"kernels":["mesh"]})).is_ok());
        assert!(convert("input.pointer", json!({"kind":"animation_viewport"})).is_ok());
        assert!(convert("disk.read", json!({"selection":"file","access":"write"})).is_err());
        assert!(convert("audio.microphone", json!({"kind":"observer"})).is_err());
    }
}
