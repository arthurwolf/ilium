//! Serde newtypes that read a JSON value of the wrong shape as "absent"
//! instead of failing the whole record. The typed transcript structs built
//! from them behave exactly like the earlier `serde_json::Value` lookups
//! (`as_str`, `as_array`, `pointer`) while skipping, without allocating, the
//! fields they ignore (images, encrypted reasoning blobs).

use std::fmt;
use std::marker::PhantomData;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

/// A string field; any other JSON type, including `null`, reads as `None`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct LenientString(pub(crate) Option<String>);

impl LenientString {
    pub(crate) fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

/// An object field deserialized into `T`; any other JSON type reads as
/// `T::default()`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct LenientObject<T>(pub(crate) T);

/// An array field whose elements are `T`; any other JSON type reads as an
/// empty vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LenientSeq<T>(pub(crate) Vec<T>);

// Hand-written so an empty sequence needs no `T: Default` (the derive would
// require it, and the visitors answer scalars with this default).
impl<T> Default for LenientSeq<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

/// Consumes a sequence without keeping any of it.
fn drain_seq<'de, A: SeqAccess<'de>>(mut seq: A) -> Result<(), A::Error> {
    while seq.next_element::<IgnoredAny>()?.is_some() {}
    Ok(())
}

/// Consumes a map without keeping any of it.
fn drain_map<'de, A: MapAccess<'de>>(mut map: A) -> Result<(), A::Error> {
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
    Ok(())
}

/// The scalar visits every lenient visitor answers with its default value.
macro_rules! accept_scalars_as_default {
    () => {
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
    };
}

struct StringVisitor;

impl<'de> Visitor<'de> for StringVisitor {
    type Value = LenientString;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value (a string is kept)")
    }

    accept_scalars_as_default!();

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(LenientString(Some(value.to_owned())))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(LenientString(Some(value)))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        drain_seq(seq)?;
        Ok(LenientString::default())
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        drain_map(map)?;
        Ok(LenientString::default())
    }
}

impl<'de> Deserialize<'de> for LenientString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StringVisitor)
    }
}

struct ObjectVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de> + Default> Visitor<'de> for ObjectVisitor<T> {
    type Value = LenientObject<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value (an object is kept)")
    }

    accept_scalars_as_default!();

    fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
        Ok(LenientObject::default())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        drain_seq(seq)?;
        Ok(LenientObject::default())
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        T::deserialize(MapAccessDeserializer::new(map)).map(LenientObject)
    }
}

impl<'de, T: Deserialize<'de> + Default> Deserialize<'de> for LenientObject<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ObjectVisitor(PhantomData))
    }
}

struct SeqVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for SeqVisitor<T> {
    type Value = LenientSeq<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value (an array is kept)")
    }

    accept_scalars_as_default!();

    fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
        Ok(LenientSeq::default())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        Vec::<T>::deserialize(SeqAccessDeserializer::new(seq)).map(LenientSeq)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        drain_map(map)?;
        Ok(LenientSeq::default())
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for LenientSeq<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SeqVisitor(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    // The derive macro; the glob only brings the trait from `serde::de`.
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize, Default, Debug, PartialEq)]
    #[serde(default)]
    struct Block {
        #[serde(rename = "type")]
        kind: LenientString,
        text: LenientString,
    }

    #[derive(Deserialize, Default, Debug, PartialEq)]
    #[serde(default)]
    struct Record {
        name: LenientString,
        content: LenientSeq<LenientObject<Block>>,
        git: LenientObject<Block>,
    }

    fn parse(text: &str) -> Record {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn wrong_shapes_read_as_absent_instead_of_failing() {
        assert_eq!(parse(r#"{"name": 5}"#).name.as_deref(), None);
        assert_eq!(parse(r#"{"name": null}"#).name.as_deref(), None);
        assert_eq!(parse(r#"{"name": [1, {"a": 2}]}"#).name.as_deref(), None);
        assert_eq!(
            parse(r#"{"name": {"nested": [true]}}"#).name.as_deref(),
            None
        );
        assert_eq!(parse(r#"{"name": "x"}"#).name.as_deref(), Some("x"));
        assert_eq!(parse(r#"{"content": "not an array"}"#).content.0, vec![]);
        assert_eq!(parse(r#"{"git": [1]}"#).git.0, Block::default());
        assert_eq!(parse(r#"{"git": 1.5}"#).git.0, Block::default());
    }

    #[test]
    fn arrays_of_mixed_elements_keep_objects_and_default_the_rest() {
        let record = parse(
            r#"{"content": [{"type": "text", "text": "hi", "image": "ignored"}, 7, "s", null]}"#,
        );
        let blocks: Vec<&Block> = record.content.0.iter().map(|block| &block.0).collect();
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0].kind.as_deref(), Some("text"));
        assert_eq!(blocks[0].text.as_deref(), Some("hi"));
        assert_eq!(blocks[1], &Block::default());
        assert_eq!(blocks[3], &Block::default());
    }
}
