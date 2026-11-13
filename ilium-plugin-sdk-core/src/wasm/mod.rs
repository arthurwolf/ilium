//! Component mode (wasm32 only). A plugin written against the mirror types and traits of
//! `crate::generated` becomes a WebAssembly component through this module:
//!
//! * `bindings` holds the wit-bindgen bindings of every import interface;
//! * `IntoWit` / `IntoMirror` convert between the mirror types and the wit-bindgen types;
//! * `imports` wraps every host function behind `host()` with mirror types;
//! * the `__ilium_wasm_*` macros (crate root) are expanded by `#[ilium_plugin]`.
//!
//! Everything but the two traits and their std impls is generated (`glue.rs`).

mod glue;

pub use glue::{bindings, imports};
pub use wit_bindgen;

use crate::plugin::HostHandle;

/// Mirror value -> wit-bindgen value.
pub trait IntoWit<W> {
    /// Converts, consuming the mirror value.
    fn into_wit(self) -> W;
}

/// wit-bindgen value -> mirror value.
pub trait IntoMirror<M> {
    /// Converts, consuming the wit-bindgen value.
    fn into_mirror(self) -> M;
}

/// The handle behind the host interfaces a component holds no grant for (`off` arms).
pub fn detached() -> HostHandle {
    HostHandle::detached()
}

macro_rules! identity {
    ($($name:ty),* $(,)?) => {
        $(
            impl IntoWit<$name> for $name {
                fn into_wit(self) -> $name {
                    self
                }
            }
            impl IntoMirror<$name> for $name {
                fn into_mirror(self) -> $name {
                    self
                }
            }
        )*
    };
}

identity!(
    u8,
    u16,
    u32,
    u64,
    i8,
    i16,
    i32,
    i64,
    f32,
    f64,
    bool,
    char,
    String,
    ()
);

impl<T: IntoWit<W>, W> IntoWit<Vec<W>> for Vec<T> {
    fn into_wit(self) -> Vec<W> {
        self.into_iter().map(IntoWit::into_wit).collect()
    }
}

impl<T: IntoMirror<M>, M> IntoMirror<Vec<M>> for Vec<T> {
    fn into_mirror(self) -> Vec<M> {
        self.into_iter().map(IntoMirror::into_mirror).collect()
    }
}

impl<T: IntoWit<W>, W> IntoWit<Option<W>> for Option<T> {
    fn into_wit(self) -> Option<W> {
        self.map(IntoWit::into_wit)
    }
}

impl<T: IntoMirror<M>, M> IntoMirror<Option<M>> for Option<T> {
    fn into_mirror(self) -> Option<M> {
        self.map(IntoMirror::into_mirror)
    }
}

impl<T: IntoWit<W>, W, E: IntoWit<F>, F> IntoWit<Result<W, F>> for Result<T, E> {
    fn into_wit(self) -> Result<W, F> {
        match self {
            Ok(value) => Ok(value.into_wit()),
            Err(error) => Err(error.into_wit()),
        }
    }
}

impl<T: IntoMirror<M>, M, E: IntoMirror<N>, N> IntoMirror<Result<M, N>> for Result<T, E> {
    fn into_mirror(self) -> Result<M, N> {
        match self {
            Ok(value) => Ok(value.into_mirror()),
            Err(error) => Err(error.into_mirror()),
        }
    }
}

impl<A: IntoWit<X>, X, B: IntoWit<Y>, Y> IntoWit<(X, Y)> for (A, B) {
    fn into_wit(self) -> (X, Y) {
        (self.0.into_wit(), self.1.into_wit())
    }
}

impl<A: IntoMirror<X>, X, B: IntoMirror<Y>, Y> IntoMirror<(X, Y)> for (A, B) {
    fn into_mirror(self) -> (X, Y) {
        (self.0.into_mirror(), self.1.into_mirror())
    }
}

impl<A: IntoWit<X>, X, B: IntoWit<Y>, Y, C: IntoWit<Z>, Z> IntoWit<(X, Y, Z)> for (A, B, C) {
    fn into_wit(self) -> (X, Y, Z) {
        (self.0.into_wit(), self.1.into_wit(), self.2.into_wit())
    }
}

impl<A: IntoMirror<X>, X, B: IntoMirror<Y>, Y, C: IntoMirror<Z>, Z> IntoMirror<(X, Y, Z)>
    for (A, B, C)
{
    fn into_mirror(self) -> (X, Y, Z) {
        (
            self.0.into_mirror(),
            self.1.into_mirror(),
            self.2.into_mirror(),
        )
    }
}
