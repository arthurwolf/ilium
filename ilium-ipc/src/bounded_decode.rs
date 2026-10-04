//! CPU-local, source-audited allocation admission for the closed IPC graph.
//!
//! This deliberately conservative policy counts inline handoffs as well as
//! cumulative collection buffers. It is not an allocator or RSS guarantee.
use crate::{ClientRequest, IpcError, ServerEvent};
use bincode::Options;
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde::Deserializer;
use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;
use std::mem::{align_of, size_of};

const ALLOCATION_LIMIT: usize = 64 * 1024 * 1024;
const DEPTH_LIMIT: usize = 128;
const STEP_LIMIT: usize = 256 * 1024 * 1024;

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::ClientRequest {}
    impl Sealed for super::ServerEvent {}
}
/// Only the audited request/event graphs are admitted by this decoder.
pub trait BoundedMessage: DeserializeOwned + sealed::Sealed {}
impl BoundedMessage for ClientRequest {}
impl BoundedMessage for ServerEvent {}

#[derive(Clone, Copy, Debug)]
struct Refusal {
    dimension: &'static str,
    requested: usize,
    limit: usize,
}
#[derive(Default)]
struct Budget {
    allocations: Cell<usize>,
    depth: Cell<usize>,
    steps: Cell<usize>,
    refusal: Cell<Option<Refusal>>,
}
struct Depth<'a>(&'a Budget);
impl Drop for Depth<'_> {
    fn drop(&mut self) {
        self.0.depth.set(self.0.depth.get() - 1);
    }
}
impl Budget {
    fn refuse<E: de::Error>(&self, dimension: &'static str, requested: usize, limit: usize) -> E {
        self.refusal.set(Some(Refusal {
            dimension,
            requested,
            limit,
        }));
        E::custom("bounded IPC decoding resource limit exceeded")
    }
    fn add<E: de::Error>(&self, bytes: usize) -> Result<(), E> {
        let total = self.allocations.get().saturating_add(bytes);
        if total > ALLOCATION_LIMIT {
            return Err(self.refuse("allocation", total, ALLOCATION_LIMIT));
        }
        self.allocations.set(total);
        Ok(())
    }
    fn step<E: de::Error>(&self) -> Result<(), E> {
        let next = self.steps.get().saturating_add(1);
        if next > STEP_LIMIT {
            return Err(self.refuse("steps", next, STEP_LIMIT));
        }
        self.steps.set(next);
        Ok(())
    }
    fn enter<E: de::Error>(&self) -> Result<Depth<'_>, E> {
        self.step()?;
        let next = self.depth.get() + 1;
        if next > DEPTH_LIMIT {
            return Err(self.refuse("depth", next, DEPTH_LIMIT));
        }
        self.depth.set(next);
        Ok(Depth(self))
    }
}
struct Seed<'a, S> {
    inner: S,
    budget: &'a Budget,
}
impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<'_, S> {
    type Value = S::Value;
    fn deserialize<D: Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<S::Value>())?;
        self.inner.deserialize(Decoder {
            inner: decoder,
            budget: self.budget,
        })
    }
}
struct MessageSeed<T>(PhantomData<T>);
impl<'de, T: serde::Deserialize<'de>> DeserializeSeed<'de> for MessageSeed<T> {
    type Value = T;
    fn deserialize<D: Deserializer<'de>>(self, decoder: D) -> Result<T, D::Error> {
        T::deserialize(decoder)
    }
}
/// Decode with the same fixed integer encoding and trailing-byte rejection as
/// ordinary framing, refusing resource admission before audited allocations.
pub fn decode_bounded<T: BoundedMessage>(payload: &[u8]) -> Result<T, IpcError> {
    decode_with_budget(payload, &Budget::default())
}
fn decode_with_budget<T: DeserializeOwned>(payload: &[u8], budget: &Budget) -> Result<T, IpcError> {
    let result = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .deserialize_seed(
            Seed {
                inner: MessageSeed::<T>(PhantomData),
                budget,
            },
            payload,
        );
    match (result, budget.refusal.get()) {
        (_, Some(refusal)) => Err(IpcError::DecodeResourceLimit {
            dimension: refusal.dimension,
            requested: refusal.requested,
            limit: refusal.limit,
        }),
        (value, None) => value.map_err(IpcError::from),
    }
}
struct Decoder<'a, D> {
    inner: D,
    budget: &'a Budget,
}
struct GuardedVisitor<'a, V> {
    inner: V,
    budget: &'a Budget,
    dynamic_sequence: bool,
}
impl<'de, D: Deserializer<'de>> Deserializer<'de> for Decoder<'_, D> {
    type Error = D::Error;
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_any(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_bool(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_i8(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_i16(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_i32(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_i64(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_i128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_i128(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_u8(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_u16(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_u32(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_u64(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_u128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_u128(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_f32(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_f64(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_char(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_str(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_str(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_bytes(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_bytes(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_option(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_unit(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_unit_struct(
            name,
            GuardedVisitor {
                inner: visitor,
                budget: self.budget,
                dynamic_sequence: false,
            },
        )
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        if name == crate::terminal_bytes::NAME {
            // This private closed-graph adapter copies a borrowed wire slice
            // into one exact-length Vec. Admission precedes that copy; no
            // generic collection growth or permissive allocation path is used.
            return self.inner.deserialize_bytes(TerminalBytesVisitor {
                inner: visitor,
                budget: self.budget,
            });
        }
        self.inner.deserialize_newtype_struct(
            name,
            GuardedVisitor {
                inner: visitor,
                budget: self.budget,
                dynamic_sequence: false,
            },
        )
    }
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_seq(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: true,
        })
    }
    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_tuple(
            len,
            GuardedVisitor {
                inner: visitor,
                budget: self.budget,
                dynamic_sequence: false,
            },
        )
    }
    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_tuple_struct(
            name,
            len,
            GuardedVisitor {
                inner: visitor,
                budget: self.budget,
                dynamic_sequence: false,
            },
        )
    }
    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_map(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_struct(
            name,
            fields,
            GuardedVisitor {
                inner: visitor,
                budget: self.budget,
                dynamic_sequence: false,
            },
        )
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_enum(
            name,
            variants,
            GuardedVisitor {
                inner: visitor,
                budget: self.budget,
                dynamic_sequence: false,
            },
        )
    }
    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_identifier(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let _depth = self.budget.enter()?;
        self.budget.add(size_of::<V::Value>())?;
        self.inner.deserialize_ignored_any(GuardedVisitor {
            inner: visitor,
            budget: self.budget,
            dynamic_sequence: false,
        })
    }
    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}
impl<'de, V: Visitor<'de>> Visitor<'de> for GuardedVisitor<'_, V> {
    type Value = V::Value;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        self.inner.expecting(formatter)
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_bool(value)
    }
    fn visit_i8<E: de::Error>(self, value: i8) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_i8(value)
    }
    fn visit_i16<E: de::Error>(self, value: i16) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_i16(value)
    }
    fn visit_i32<E: de::Error>(self, value: i32) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_i32(value)
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_i64(value)
    }
    fn visit_i128<E: de::Error>(self, value: i128) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_i128(value)
    }
    fn visit_u8<E: de::Error>(self, value: u8) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_u8(value)
    }
    fn visit_u16<E: de::Error>(self, value: u16) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_u16(value)
    }
    fn visit_u32<E: de::Error>(self, value: u32) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_u32(value)
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_u64(value)
    }
    fn visit_u128<E: de::Error>(self, value: u128) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_u128(value)
    }
    fn visit_f32<E: de::Error>(self, value: f32) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_f32(value)
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_f64(value)
    }
    fn visit_char<E: de::Error>(self, value: char) -> Result<Self::Value, E> {
        self.budget.step()?;
        self.inner.visit_char(value)
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.budget.add(value.len().saturating_mul(2))?;
        self.inner.visit_str(value)
    }
    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<Self::Value, E> {
        self.budget.add(value.len().saturating_mul(2))?;
        self.inner.visit_borrowed_str(value)
    }
    fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<Self::Value, E> {
        self.budget.add(value.len().saturating_mul(2))?;
        self.inner.visit_bytes(value)
    }
    fn visit_borrowed_bytes<E: de::Error>(self, value: &'de [u8]) -> Result<Self::Value, E> {
        self.budget.add(value.len().saturating_mul(2))?;
        self.inner.visit_borrowed_bytes(value)
    }
    fn visit_string<E: de::Error>(self, _value: String) -> Result<Self::Value, E> {
        Err(self.budget.refuse("nonborrowing string", usize::MAX, 0))
    }
    fn visit_byte_buf<E: de::Error>(self, _value: Vec<u8>) -> Result<Self::Value, E> {
        Err(self.budget.refuse("nonborrowing bytes", usize::MAX, 0))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_unit()
    }
    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_none()
    }
    fn visit_some<D: Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        self.inner.visit_some(Decoder {
            inner: decoder,
            budget: self.budget,
        })
    }
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        decoder: D,
    ) -> Result<Self::Value, D::Error> {
        self.inner.visit_newtype_struct(Decoder {
            inner: decoder,
            budget: self.budget,
        })
    }
    fn visit_seq<A: SeqAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_seq(Sequence {
            inner: access,
            budget: self.budget,
            dynamic: self.dynamic_sequence,
            count: 0,
            capacity: 0,
        })
    }
    fn visit_map<A: MapAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_map(Map {
            inner: access,
            budget: self.budget,
            count: 0,
            buckets: 0,
            key_size: 0,
            key_align: 1,
        })
    }
    fn visit_enum<A: EnumAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_enum(Enumeration {
            inner: access,
            budget: self.budget,
        })
    }
}
/// Only the source-audited terminal byte adapter reaches this visitor.
/// Its allocation is exactly the borrowed slice length. Reject every other
/// serde callback by default, including already-allocated buffers/sequences.
struct TerminalBytesVisitor<'a, V> {
    inner: V,
    budget: &'a Budget,
}
impl<'de, V: Visitor<'de>> Visitor<'de> for TerminalBytesVisitor<'_, V> {
    type Value = V::Value;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.expecting(formatter)
    }
    fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Self::Value, E> {
        self.budget.add(bytes.len())?;
        self.inner.visit_bytes(bytes)
    }
    fn visit_borrowed_bytes<E: de::Error>(self, bytes: &'de [u8]) -> Result<Self::Value, E> {
        self.budget.add(bytes.len())?;
        self.inner.visit_borrowed_bytes(bytes)
    }
}

struct Sequence<'a, A> {
    inner: A,
    budget: &'a Budget,
    dynamic: bool,
    count: usize,
    capacity: usize,
}
impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Sequence<'_, A> {
    type Error = A::Error;
    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        self.budget.step()?;
        if self.dynamic && size_of::<S::Value>() != 0 && self.count == self.capacity {
            let minimum = if size_of::<S::Value>() == 1 {
                8
            } else if size_of::<S::Value>() <= 1024 {
                4
            } else {
                1
            };
            let capacity = self.capacity.saturating_mul(2).max(minimum);
            self.budget
                .add(capacity.saturating_mul(size_of::<S::Value>()))?;
            self.capacity = capacity;
        }
        let value = self.inner.next_element_seed(Seed {
            inner: seed,
            budget: self.budget,
        })?;
        if value.is_some() {
            self.count = self
                .count
                .checked_add(1)
                .ok_or_else(|| self.budget.refuse("sequence count", usize::MAX, STEP_LIMIT))?;
        }
        Ok(value)
    }
    fn size_hint(&self) -> Option<usize> {
        Some(0)
    }
}
struct Map<'a, A> {
    inner: A,
    budget: &'a Budget,
    count: usize,
    buckets: usize,
    key_size: usize,
    key_align: usize,
}
fn map_layout(
    count: usize,
    key_size: usize,
    key_align: usize,
    value_size: usize,
    value_align: usize,
) -> Option<(usize, usize)> {
    let align = key_align.max(value_align);
    let tuple_size = key_size.checked_add(value_size)?.checked_add(align - 1)? & !(align - 1);
    let buckets = count.checked_mul(2)?.max(16).checked_next_power_of_two()?;
    let control_align = align.max(16);
    let control_offset = tuple_size
        .checked_mul(buckets)?
        .checked_add(control_align - 1)?
        & !(control_align - 1);
    Some((
        buckets,
        control_offset.checked_add(buckets)?.checked_add(16)?,
    ))
}
impl<'de, A: MapAccess<'de>> MapAccess<'de> for Map<'_, A> {
    type Error = A::Error;
    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        self.budget.step()?;
        self.key_size = size_of::<S::Value>();
        self.key_align = align_of::<S::Value>();
        self.inner.next_key_seed(Seed {
            inner: seed,
            budget: self.budget,
        })
    }
    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| self.budget.refuse("map count", usize::MAX, STEP_LIMIT))?;
        let (buckets, bytes) = map_layout(
            self.count,
            self.key_size,
            self.key_align,
            size_of::<S::Value>(),
            align_of::<S::Value>(),
        )
        .ok_or_else(|| {
            self.budget
                .refuse("map layout", usize::MAX, ALLOCATION_LIMIT)
        })?;
        if buckets > self.buckets {
            self.budget.add(bytes)?;
            self.buckets = buckets;
        }
        self.inner.next_value_seed(Seed {
            inner: seed,
            budget: self.budget,
        })
    }
    fn size_hint(&self) -> Option<usize> {
        Some(0)
    }
}
struct Enumeration<'a, A> {
    inner: A,
    budget: &'a Budget,
}
struct Variant<'a, A> {
    inner: A,
    budget: &'a Budget,
}
impl<'a, 'de, A: EnumAccess<'de>> EnumAccess<'de> for Enumeration<'a, A> {
    type Error = A::Error;
    type Variant = Variant<'a, A::Variant>;
    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self::Variant), A::Error> {
        let (value, variant) = self.inner.variant_seed(Seed {
            inner: seed,
            budget: self.budget,
        })?;
        Ok((
            value,
            Variant {
                inner: variant,
                budget: self.budget,
            },
        ))
    }
}
impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Variant<'_, A> {
    type Error = A::Error;
    fn unit_variant(self) -> Result<(), A::Error> {
        self.inner.unit_variant()
    }
    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, A::Error> {
        self.inner.newtype_variant_seed(Seed {
            inner: seed,
            budget: self.budget,
        })
    }
    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.inner.tuple_variant(
            len,
            GuardedVisitor {
                inner: visitor,
                budget: self.budget,
                dynamic_sequence: false,
            },
        )
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.inner.struct_variant(
            fields,
            GuardedVisitor {
                inner: visitor,
                budget: self.budget,
                dynamic_sequence: false,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn refusal_precedes_the_original_seed_allocation_callback() {
        struct ObservedSeed<'a>(&'a AtomicUsize);
        impl<'de> DeserializeSeed<'de> for ObservedSeed<'_> {
            type Value = [u8; 64];
            fn deserialize<D: Deserializer<'de>>(self, _: D) -> Result<Self::Value, D::Error> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok([0; 64])
            }
        }
        let called = AtomicUsize::new(0);
        let budget = Budget::default();
        budget.allocations.set(ALLOCATION_LIMIT - 32);
        let result = Seed {
            inner: ObservedSeed(&called),
            budget: &budget,
        }
        .deserialize(de::value::UnitDeserializer::<de::value::Error>::new());
        assert!(result.is_err());
        assert_eq!(called.load(Ordering::SeqCst), 0);
        assert_eq!(budget.depth.get(), 0);
    }

    #[test]
    fn sequence_growth_accounts_old_and_new_buffers_cumulatively() {
        let expected: Vec<u64> = (0..129).collect();
        let payload = bincode::serialize(&expected).unwrap();
        let budget = Budget::default();
        let decoded: Vec<u64> = decode_with_budget(&payload, &budget).unwrap();
        assert_eq!(decoded, expected);
        // RawVec capacities4,8,16,32,64,128,256, all admitted before push.
        assert!(budget.allocations.get() >= (4 + 8 + 16 + 32 + 64 + 128 + 256) * 8);
        assert_eq!(budget.depth.get(), 0);
    }

    #[test]
    fn map_layout_covers_installed_table_control_alignment_and_duplicate_pairs() {
        #[repr(align(64))]
        struct Aligned;
        let (_, layout) = map_layout(1, size_of::<Aligned>(), align_of::<Aligned>(), 8, 8).unwrap();
        assert!(layout >= 16 * 64 + 16 + 16);
        let pairs = vec![(1u64, "first".to_owned()), (1u64, "replacement".to_owned())];
        // Bincode map and sequence-of-pairs have identical length/pair wire layout.
        let payload = bincode::serialize(&pairs).unwrap();
        let budget = Budget::default();
        let decoded: HashMap<u64, String> = decode_with_budget(&payload, &budget).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[&1], "replacement");
        assert!(
            budget.allocations.get()
                >= map_layout(2, 8, 8, size_of::<String>(), align_of::<String>())
                    .unwrap()
                    .1
        );
    }

    #[test]
    fn strings_borrow_wire_before_owned_copy_and_trailing_bytes_still_fail() {
        let payload = bincode::serialize(&"retained original").unwrap();
        let budget = Budget::default();
        let decoded: String = decode_with_budget(&payload, &budget).unwrap();
        assert_eq!(decoded, "retained original");
        assert!(budget.allocations.get() >= 2 * decoded.len());
        let mut trailing = payload;
        trailing.push(0);
        assert!(matches!(
            decode_with_budget::<String>(&trailing, &Budget::default()),
            Err(IpcError::Bincode(_))
        ));
    }

    #[test]
    fn zero_sized_sequence_count_cannot_bypass_the_operation_ceiling() {
        let payload = u64::MAX.to_le_bytes();
        let budget = Budget::default();
        budget.steps.set(STEP_LIMIT - 16);
        assert!(matches!(
            decode_with_budget::<Vec<()>>(&payload, &budget),
            Err(IpcError::DecodeResourceLimit {
                dimension: "steps",
                ..
            })
        ));
        assert_eq!(budget.depth.get(), 0);
    }

    #[test]
    fn recursive_boxes_refuse_at_the_declared_depth_and_unwind_cleanly() {
        #[derive(serde::Serialize, serde::Deserialize)]
        enum Recursive {
            End,
            Next(Box<Recursive>),
        }
        let mut value = Recursive::End;
        for _ in 0..100 {
            value = Recursive::Next(Box::new(value));
        }
        let payload = bincode::serialize(&value).unwrap();
        let budget = Budget::default();
        assert!(matches!(
            decode_with_budget::<Recursive>(&payload, &budget),
            Err(IpcError::DecodeResourceLimit {
                dimension: "depth",
                ..
            })
        ));
        assert_eq!(budget.depth.get(), 0);
    }

    #[test]
    fn terminal_byte_refusal_precedes_the_original_copy_callback() {
        struct Observed<'a>(&'a AtomicUsize);
        impl<'de> Visitor<'de> for Observed<'_> {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("test bytes")
            }
            fn visit_borrowed_bytes<E: de::Error>(
                self,
                bytes: &'de [u8],
            ) -> Result<Self::Value, E> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(bytes.to_vec())
            }
        }
        let called = AtomicUsize::new(0);
        let budget = Budget::default();
        budget.allocations.set(ALLOCATION_LIMIT - 1);
        let visitor = TerminalBytesVisitor {
            inner: Observed(&called),
            budget: &budget,
        };
        let result: Result<Vec<u8>, de::value::Error> = visitor.visit_borrowed_bytes(&[1, 2]);
        assert!(result.is_err());
        assert_eq!(called.load(Ordering::SeqCst), 0);
        assert_eq!(budget.refusal.get().unwrap().dimension, "allocation");
        assert_eq!(budget.allocations.get(), ALLOCATION_LIMIT - 1);
    }

    #[test]
    fn conservative_raw_byte_policy_can_refuse_below_the_wire_limit() {
        let request = ClientRequest::KeyInput {
            pane_id: ilium_core::NodeId(2),
            bytes: vec![42; 16 * 1024 * 1024],
            submission: None,
        };
        let frame = crate::encode_frame(&request).unwrap();
        assert!(frame.retained_bytes() < crate::MAX_FRAME_LEN as usize);
        assert!(matches!(
            crate::decode_bounded_frame::<ClientRequest>(&frame),
            Err(IpcError::DecodeResourceLimit {
                dimension: "allocation",
                ..
            })
        ));
    }
}
