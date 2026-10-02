use super::error::{AssetError, Result};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

const MIB: u64 = 1024 * 1024;

/// Application limits, not claims about allocator overhead or a hard OS RSS
/// limit. All limits may be reduced for tests; `validate` disallows increasing
/// them beyond this reviewed foundation's ceilings.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub encoded_bytes: u64,
    pub metadata_bytes: u64,
    pub image_pixels: u64,
    pub png_chunks: usize,
    pub decoder_scratch_bytes: u64,
    pub textures: usize,
    pub requirements: usize,
    pub animation_frames: usize,
    pub frame_ticks: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            encoded_bytes: 64 * MIB,
            metadata_bytes: 512 * 1024,
            image_pixels: 32 * MIB,
            png_chunks: 8192,
            decoder_scratch_bytes: 64 * MIB,
            textures: 8192,
            requirements: 16384,
            animation_frames: 4096,
            frame_ticks: 1_000_000,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        let ceiling = Self::default();
        for (name, value, cap) in [
            ("encoded bytes", self.encoded_bytes, ceiling.encoded_bytes),
            (
                "metadata bytes",
                self.metadata_bytes,
                ceiling.metadata_bytes,
            ),
            ("decoded pixels", self.image_pixels, ceiling.image_pixels),
            (
                "PNG chunks",
                self.png_chunks as u64,
                ceiling.png_chunks as u64,
            ),
            (
                "decoder scratch",
                self.decoder_scratch_bytes,
                ceiling.decoder_scratch_bytes,
            ),
            ("textures", self.textures as u64, ceiling.textures as u64),
            (
                "requirements",
                self.requirements as u64,
                ceiling.requirements as u64,
            ),
            (
                "animation frames",
                self.animation_frames as u64,
                ceiling.animation_frames as u64,
            ),
            (
                "frame ticks",
                u64::from(self.frame_ticks),
                u64::from(ceiling.frame_ticks),
            ),
        ] {
            if value == 0 || value > cap {
                return Err(AssetError::Limit {
                    resource: name,
                    requested: value,
                    limit: cap,
                });
            }
        }
        Ok(())
    }

    /// Product, not a 4096-side test. 1021x16384 is a legitimate admitted image.
    pub fn rgba_bytes(&self, width: u32, height: u32) -> Result<usize> {
        self.validate()?;
        if width == 0 || height == 0 {
            return Err(AssetError::InvalidImage("zero image extent".into()));
        }
        let pixels = u64::from(width) * u64::from(height);
        if pixels > self.image_pixels {
            return Err(AssetError::Limit {
                resource: "decoded pixels",
                requested: pixels,
                limit: self.image_pixels,
            });
        }
        usize::try_from(pixels * 4).map_err(|_| AssetError::Allocation)
    }
}

/// Check both lifetime cancellation and request generation. The frame path does
/// not wait for cancellation; worker adapters poll this between bounded units.
#[derive(Clone, Copy)]
pub struct Cancel<'a> {
    stop: &'a AtomicBool,
    revision: Option<(&'a AtomicU64, u64)>,
}
impl<'a> Cancel<'a> {
    pub fn new(stop: &'a AtomicBool) -> Self {
        Self {
            stop,
            revision: None,
        }
    }
    pub fn for_revision(stop: &'a AtomicBool, revision: &'a AtomicU64, expected: u64) -> Self {
        Self {
            stop,
            revision: Some((revision, expected)),
        }
    }
    pub fn is_cancelled(self) -> bool {
        self.stop.load(Ordering::Acquire)
            || self
                .revision
                .is_some_and(|(current, expected)| current.load(Ordering::Acquire) != expected)
    }
    pub fn check(self) -> Result<()> {
        if self.is_cancelled() {
            Err(AssetError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
struct Counter {
    limit: u64,
    used: AtomicU64,
    peak: AtomicU64,
}

/// Clone this same budget into workers and retained snapshots. Creating a new
/// independent budget per request defeats the process-level limit; D2c owns one
/// shared budget for this scene family.
#[derive(Debug, Clone)]
pub struct ByteBudget(Arc<Counter>);
impl ByteBudget {
    pub fn new(limit: u64) -> Result<Self> {
        if limit == 0 || limit > 1024 * MIB {
            return Err(AssetError::Limit {
                resource: "working bytes",
                requested: limit,
                limit: 1024 * MIB,
            });
        }
        Ok(Self(Arc::new(Counter {
            limit,
            used: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        })))
    }
    pub fn used(&self) -> u64 {
        self.0.used.load(Ordering::Acquire)
    }
    pub fn peak(&self) -> u64 {
        self.0.peak.load(Ordering::Acquire)
    }
    pub fn limit(&self) -> u64 {
        self.0.limit
    }
    pub(crate) fn reserve(&self, bytes: u64, cancel: Cancel<'_>) -> Result<Reservation> {
        let mut old = self.used();
        loop {
            cancel.check()?;
            let Some(new) = old.checked_add(bytes) else {
                return Err(AssetError::Limit {
                    resource: "working bytes",
                    requested: u64::MAX,
                    limit: self.limit(),
                });
            };
            if new > self.limit() {
                return Err(AssetError::Limit {
                    resource: "working bytes",
                    requested: new,
                    limit: self.limit(),
                });
            }
            match self
                .0
                .used
                .compare_exchange_weak(old, new, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    self.0.peak.fetch_max(new, Ordering::AcqRel);
                    return Ok(Reservation {
                        counter: Arc::clone(&self.0),
                        bytes,
                    });
                }
                Err(actual) => old = actual,
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct Reservation {
    counter: Arc<Counter>,
    bytes: u64,
}
impl Reservation {
    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }
    pub(crate) fn belongs_to(&self, budget: &ByteBudget) -> bool {
        Arc::ptr_eq(&self.counter, &budget.0)
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.counter.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

pub(crate) fn zeroed_bytes(len: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| AssetError::Allocation)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tall_irregular_goodvibes_dimensions_use_product_budget() {
        let limits = Limits::default();
        assert_eq!(limits.rgba_bytes(512, 16384).unwrap(), 33_554_432);
        assert_eq!(limits.rgba_bytes(1021, 16384).unwrap(), 66_912_256);
        assert!(limits.rgba_bytes(16384, 16384).is_err());
        assert!(limits.rgba_bytes(u32::MAX, u32::MAX).is_err());
        assert!(limits.rgba_bytes(0, 1).is_err());
    }
    #[test]
    fn reservations_account_for_live_snapshots_and_release_on_error() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(16).unwrap();
        let a = budget.reserve(10, cancel).unwrap();
        let clone = budget.clone();
        assert!(clone.reserve(7, cancel).is_err());
        let b = clone.reserve(6, cancel).unwrap();
        assert_eq!(budget.used(), 16);
        drop(a);
        assert_eq!(clone.used(), 6);
        drop(b);
        assert_eq!(budget.used(), 0);
        assert_eq!(budget.peak(), 16);
    }
    #[test]
    fn supersession_and_stop_are_independent() {
        let stop = AtomicBool::new(false);
        let revision = AtomicU64::new(4);
        let cancel = Cancel::for_revision(&stop, &revision, 4);
        assert!(cancel.check().is_ok());
        revision.store(5, Ordering::Release);
        assert_eq!(cancel.check(), Err(AssetError::Cancelled));
        revision.store(4, Ordering::Release);
        stop.store(true, Ordering::Release);
        assert_eq!(cancel.check(), Err(AssetError::Cancelled));
    }
    #[test]
    fn invalid_limit_configuration_is_not_silently_unbounded() {
        let limits = Limits {
            image_pixels: u64::MAX,
            ..Limits::default()
        };
        assert!(limits.validate().is_err());
        assert!(ByteBudget::new(0).is_err());
    }
}
