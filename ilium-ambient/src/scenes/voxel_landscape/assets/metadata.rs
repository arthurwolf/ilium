//! Bounded JSON values with duplicate-key rejection. Original metadata bytes and
//! origins remain distinct from normalization; this is never a script evaluator.
use super::{
    budget::{ByteBudget, Cancel, Limits, Reservation},
    error::{AssetError, Result},
    identity::{BlobOrigin, Digest256, SourceBlob},
};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;
struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ValueVisitor;
        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("bounded unique-key JSON")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Bool(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Number(value.into())))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Number(value.into())))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Self::Value, E> {
                Number::from_f64(value)
                    .map(|v| UniqueValue(Value::Number(v)))
                    .ok_or_else(|| E::custom("nonfinite JSON number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                if value.len() > 65536 {
                    return Err(E::custom("JSON string exceeds 64 KiB"));
                }
                Ok(UniqueValue(Value::String(value.to_owned())))
            }
            fn visit_string<E: de::Error>(
                self,
                value: String,
            ) -> std::result::Result<Self::Value, E> {
                self.visit_str(&value)
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<UniqueValue>()? {
                    if values.len() >= 8192 {
                        return Err(de::Error::custom("JSON array exceeds 8192 entries"));
                    }
                    values.push(value.0);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut entries: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(key) = entries.next_key::<String>()? {
                    if values.len() >= 8192 || key.len() > 1024 {
                        return Err(de::Error::custom("JSON object/key limit"));
                    }
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate JSON object key"));
                    }
                    let value: UniqueValue = entries.next_value()?;
                    values.insert(key, value.0);
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(ValueVisitor)
    }
}
#[derive(Debug)]
pub struct Document {
    pub value: Value,
    pub origin: BlobOrigin,
    pub sha256: Digest256,
    _reservation: Reservation,
}
impl Document {
    // Keep the shared budget identity explicit.
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    } // Keep model/animation metadata on the same scene-wide account. // Implement the associated contract without hidden runtime I/O.
    pub fn parse(
        blob: &SourceBlob,
        limits: &Limits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        limits.validate()?;
        cancel.check()?;
        if !blob.uses_budget(budget) {
            return Err(invalid("metadata account differs from source account"));
        }
        super::source::check_limit(
            "metadata bytes",
            blob.bytes().len() as u64,
            limits.metadata_bytes,
        )?;
        // Includes map nodes, keys, temporary strings, and parser stack allowance.
        let reservation = budget.reserve(blob.bytes().len() as u64 * 128 + 65536, cancel)?;
        let UniqueValue(value) = serde_json::from_slice(blob.bytes())
            .map_err(|e| invalid(&super::error::summary(&e.to_string())))?;
        cancel.check()?;
        Ok(Self {
            value,
            origin: blob.origin().clone(),
            sha256: blob.digest(),
            _reservation: reservation,
        })
    }
}
pub(crate) fn invalid(message: &str) -> AssetError {
    AssetError::InvalidMetadata(message.into())
}
pub(crate) fn object(value: &Value) -> Result<&Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| invalid("expected JSON object"))
}
pub(crate) fn array(value: &Value) -> Result<&[Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid("expected JSON array"))
}
pub(crate) fn text(value: &Value) -> Result<&str> {
    value
        .as_str()
        .ok_or_else(|| invalid("expected JSON string"))
}
pub(crate) fn uint(value: &Value) -> Result<u32> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| invalid("expected unsigned 32-bit integer"))
}
pub(crate) fn boolean(value: &Value) -> Result<bool> {
    value
        .as_bool()
        .ok_or_else(|| invalid("expected JSON boolean"))
}
pub(crate) fn required<'a>(map: &'a Map<String, Value>, name: &str) -> Result<&'a Value> {
    map.get(name)
        .ok_or_else(|| invalid(&format!("missing metadata field: {name}")))
}
pub(crate) fn allowed(map: &Map<String, Value>, fields: &[&str]) -> Result<()> {
    if let Some(key) = map.keys().find(|key| !fields.contains(&key.as_str())) {
        return Err(AssetError::Unsupported(format!("metadata field {key}")));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::super::{review::fixture_origin, OriginKind};
    use super::*;
    use std::sync::atomic::AtomicBool;
    #[test]
    fn duplicate_nested_keys_trailing_data_and_excessive_arrays_are_rejected() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(64 << 20).unwrap();
        let limits = Limits::default();
        let big = format!("[{}]", vec!["0"; 8193].join(","));
        for input in [r#"{"a":{"x":1,"x":2}}"#, "{} {}", &big] {
            let blob = SourceBlob::new(
                input.as_bytes().to_vec(),
                fixture_origin(OriginKind::DiagnosticFixture),
                None,
                &limits,
                &budget,
                cancel,
            )
            .unwrap();
            assert!(Document::parse(&blob, &limits, &budget, cancel).is_err());
        }
        assert_eq!(budget.used(), 0);
    }
}
