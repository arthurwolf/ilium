//! The dot raster and its dithering helpers live in `ilium-ambient` so the
//! legacy scenes and the hosted ambient scenes share one implementation (and
//! one fidelity test suite). This module only re-exports them.

pub(super) use ilium_ambient::raster::{hash, smoothstep, threshold, Raster};
