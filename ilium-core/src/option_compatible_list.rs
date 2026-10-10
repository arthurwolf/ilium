//! Serde adapter for a list that replaced a former `Option<T>` field on the
//! bincode IPC wire.
//!
//! The IPC framing has no version handshake, so a newly installed `ilium`
//! command keeps talking to an already-running older server until the user
//! restarts Ilium. Changing a field from `Option<T>` to `Vec<T>` would shift
//! every following byte of the frame and make each attach fail to decode. This
//! adapter keeps the old bytes for zero or one item:
//!
//! - no item: tag `0` (bincode's `None`),
//! - one item: tag `1` followed by the item (bincode's `Some(item)`),
//! - several items: tag `2`, a `u64` count, then the items.
//!
//! An older reader therefore decodes every frame that carries at most one item
//! and refuses (instead of misreading) a frame with several. Human-readable
//! formats (the JSON session snapshots) use a plain list.
//!
//! Use with `#[serde(with = "ilium_core::option_compatible_list")]`.

use serde::de::{self, DeserializeOwned, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::marker::PhantomData;

const NO_ITEM: u8 = 0;
const ONE_ITEM: u8 = 1;
const SEVERAL_ITEMS: u8 = 2;

/// Upper bound on the item count a frame may claim, so a corrupt count
/// cannot drive an unbounded allocation. Every current user holds far fewer
/// (a pane has at most 8 progress monitors).
pub const MAX_DECODED_ITEMS: u64 = 1024;

pub fn serialize<T, S>(items: &[T], serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    if serializer.is_human_readable() {
        return items.serialize(serializer);
    }
    match items {
        [] => {
            let mut tuple = serializer.serialize_tuple(1)?;
            tuple.serialize_element(&NO_ITEM)?;
            tuple.end()
        }
        [item] => {
            let mut tuple = serializer.serialize_tuple(2)?;
            tuple.serialize_element(&ONE_ITEM)?;
            tuple.serialize_element(item)?;
            tuple.end()
        }
        items => {
            let mut tuple = serializer.serialize_tuple(items.len().saturating_add(2))?;
            tuple.serialize_element(&SEVERAL_ITEMS)?;
            tuple.serialize_element(&(items.len() as u64))?;
            for item in items {
                tuple.serialize_element(item)?;
            }
            tuple.end()
        }
    }
}

pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: DeserializeOwned,
    D: Deserializer<'de>,
{
    if deserializer.is_human_readable() {
        return Vec::<T>::deserialize(deserializer);
    }
    // bincode tuples carry no length, so the visitor reads exactly as many
    // elements as the tag announces; the declared length is only an upper
    // bound (tag + count + items).
    deserializer.deserialize_tuple(
        (MAX_DECODED_ITEMS as usize).saturating_add(2),
        ListVisitor(PhantomData),
    )
}

struct ListVisitor<T>(PhantomData<T>);

impl<'de, T: DeserializeOwned> Visitor<'de> for ListVisitor<T> {
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an optional-compatible list (tag 0, 1 or 2)")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Vec<T>, A::Error> {
        let tag: u8 = sequence
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        match tag {
            NO_ITEM => Ok(Vec::new()),
            ONE_ITEM => {
                let item = sequence
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                Ok(vec![item])
            }
            SEVERAL_ITEMS => {
                let count: u64 = sequence
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                if count > MAX_DECODED_ITEMS {
                    return Err(de::Error::invalid_value(
                        de::Unexpected::Unsigned(count),
                        &"at most MAX_DECODED_ITEMS items",
                    ));
                }
                // Bounded by MAX_DECODED_ITEMS above.
                let mut items = Vec::with_capacity(count as usize);
                for index in 0..count {
                    let item = sequence
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(index as usize + 2, &self))?;
                    items.push(item);
                }
                Ok(items)
            }
            other => Err(de::Error::invalid_value(
                de::Unexpected::Unsigned(u64::from(other)),
                &"list tag 0, 1 or 2",
            )),
        }
    }
}

/// The same layout for a `Result<Vec<T>, E>` whose `Ok` side replaced a
/// former `Result<Option<T>, E>`. The `Result` framing (variant index, then
/// payload) is unchanged; only the `Ok` payload uses the list layout above.
///
/// Use with `#[serde(with = "ilium_core::option_compatible_list::in_result")]`.
pub mod in_result {
    use serde::de::DeserializeOwned;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize)]
    #[serde(rename = "Result")]
    enum Borrowed<'a, T: Serialize, E: Serialize> {
        Ok(#[serde(with = "super")] &'a [T]),
        Err(&'a E),
    }

    #[derive(Deserialize)]
    #[serde(rename = "Result")]
    #[serde(bound(deserialize = "T: DeserializeOwned, E: Deserialize<'de>"))]
    enum Owned<T, E> {
        Ok(#[serde(with = "super")] Vec<T>),
        Err(E),
    }

    pub fn serialize<T, E, S>(result: &Result<Vec<T>, E>, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: Serialize,
        E: Serialize,
        S: Serializer,
    {
        match result {
            Ok(items) => Borrowed::<T, E>::Ok(items.as_slice()).serialize(serializer),
            Err(error) => Borrowed::<T, E>::Err(error).serialize(serializer),
        }
    }

    pub fn deserialize<'de, T, E, D>(deserializer: D) -> Result<Result<Vec<T>, E>, D::Error>
    where
        T: DeserializeOwned,
        E: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Ok(match Owned::deserialize(deserializer)? {
            Owned::Ok(items) => Ok(items),
            Owned::Err(error) => Err(error),
        })
    }
}
